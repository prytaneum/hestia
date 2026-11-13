//! The plugin's own state: typed JSON files in its `data/` directory, the only
//! place a plugin's state outlives a call. A file is named by a short key,
//! `[A-Za-z0-9_-]`, and stored as `<key>.json`.
//!
//! Calls may run concurrently, and each [`save`] replaces the whole file, so
//! when two calls save the same key the last writer wins. To change a value
//! another call may change too, use [`update`]: it holds a lock over the key
//! from the read to the write.
//!
//! ```no_run
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, Default)]
//! struct Counter {
//!     runs: u32,
//! }
//!
//! let mut counter: Counter = hestia::data::load("counter")?.unwrap_or_default();
//! counter.runs += 1;
//! hestia::data::save("counter", &counter)?;
//! # Ok::<(), hestia::Error>(())
//! ```

use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use proto::plugin::DATA_MOUNT;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::bindings::hestia::plugin::data as host;
use crate::Error;

const KEY_LENGTH: std::ops::RangeInclusive<usize> = 1..=64;

/// Reads the value saved under `key`, or `None` when nothing was saved.
///
/// ```no_run
/// let greeting: Option<String> = hestia::data::load("greeting")?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn load<T: DeserializeOwned>(key: &str) -> Result<Option<T>, Error> {
    match fs::read(path(key)?) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Saves `value` under `key`, replacing what was there. The file is written
/// aside and renamed into place, so a reader never sees half of it.
///
/// ```no_run
/// hestia::data::save("greeting", &"hello")?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn save<T: Serialize>(key: &str, value: &T) -> Result<(), Error> {
    let target = path(key)?;
    let bytes = serde_json::to_vec(value)?;
    let (staged, mut file) = stage(&target)?;
    let written = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file);
    match written.and_then(|()| fs::rename(&staged, &target)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&staged);
            Err(error.into())
        }
    }
}

/// Reads the value under `key` (or its default), lets `change` edit it, and
/// saves it, atomically with respect to every other [`update`] of the same key
/// in this plugin's calls: a concurrent one waits its turn, so no update is lost.
///
/// The host holds the lock, and releases it when the call ends however it ends.
/// A call waits a few seconds for another to finish, then fails with
/// [`Error::Busy`]. Do not call [`update`] on the same key from inside `change`
/// ([`Error::Invalid`]), and keep `change` quick: it holds everyone else up. A
/// plain [`save`] does not take the lock, so mix it in only when losing a race
/// is acceptable.
///
/// ```no_run
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize, Default)]
/// struct Counter {
///     runs: u32,
/// }
///
/// let runs = hestia::data::update("counter", |counter: &mut Counter| {
///     counter.runs += 1;
///     counter.runs
/// })?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn update<T, R>(key: &str, change: impl FnOnce(&mut T) -> R) -> Result<R, Error>
where
    T: Serialize + DeserializeOwned + Default,
{
    path(key)?;
    host::lock(key)?;
    let _held = Held(key);
    let mut value = load::<T>(key)?.unwrap_or_default();
    let result = change(&mut value);
    save(key, &value)?;
    Ok(result)
}

struct Held<'a>(&'a str);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let _ = host::unlock(self.0);
    }
}

fn path(key: &str) -> Result<PathBuf, Error> {
    let valid = KEY_LENGTH.contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !valid {
        return Err(Error::Invalid(format!(
            "`{key}` is not a data key: use 1 to 64 of A-Z, a-z, 0-9, - and _"
        )));
    }
    Ok(Path::new(DATA_MOUNT).join(format!("{key}.json")))
}

/// A fresh temp file beside `target`. Concurrent saves of one key each take
/// their own, so their bytes never interleave.
fn stage(target: &Path) -> Result<(PathBuf, File), Error> {
    for attempt in 0u32.. {
        let staged = target.with_extension(format!("json.{attempt}.part"));
        match File::create_new(&staged) {
            Ok(file) => return Ok((staged, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("a directory cannot hold every staging name")
}
