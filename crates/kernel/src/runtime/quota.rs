use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use cap_std::ambient_authority;
use cap_std::fs::Dir;

pub(super) const DATA_QUOTA: u64 = 8 * 1024 * 1024 * 1024;
const DEPTH_MAX: usize = 64;

/// What a plugin's `/data` may hold, and which plugins held more when last
/// measured. Writes through the WASI preopen cannot be stopped as they happen,
/// so the limit is enforced on the unattended path: `/data` is measured before
/// each schedule tick, and an over-quota plugin gets none.
pub(super) struct Quota {
    limit: u64,
    over: Mutex<HashSet<String>>,
}

impl Default for Quota {
    fn default() -> Self {
        Quota::new(DATA_QUOTA)
    }
}

impl Quota {
    pub fn new(limit: u64) -> Self {
        Quota {
            limit,
            over: Mutex::default(),
        }
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Measures `data` off the executor and answers whether it is within the
    /// limit. A directory that cannot be measured keeps the last answer.
    pub async fn admits(&self, plugin: &str, data: PathBuf) -> bool {
        let used = tokio::task::spawn_blocking(move || {
            let dir = Dir::open_ambient_dir(data, ambient_authority())?;
            stored(&dir, 0)
        })
        .await;
        let mut over = self.over.lock().unwrap_or_else(PoisonError::into_inner);
        let used = match used {
            Ok(Ok(used)) => used,
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Ok(Err(error)) => {
                tracing::warn!(plugin, %error, "cannot measure the plugin's data");
                return !over.contains(plugin);
            }
            Err(error) => {
                tracing::warn!(plugin, %error, "measuring the plugin's data failed");
                return !over.contains(plugin);
            }
        };
        match used > self.limit {
            true if over.insert(plugin.to_owned()) => {
                tracing::warn!(
                    plugin,
                    used,
                    limit = self.limit,
                    "plugin data is over its quota; schedule ticks are refused until it is under"
                );
            }
            false if over.remove(plugin) => {
                tracing::info!(plugin, used, "plugin data is back under its quota");
            }
            _ => {}
        }
        used <= self.limit
    }
}

/// The bytes of the regular files under `dir`; links are not followed.
pub(super) fn stored(dir: &Dir, depth: usize) -> std::io::Result<u64> {
    if depth > DEPTH_MAX {
        return Err(std::io::Error::other(
            "the data directory is nested too deeply",
        ));
    }
    let mut total = 0u64;
    for entry in dir.entries()? {
        let entry = entry?;
        let counted = match entry.file_type()? {
            kind if kind.is_file() => entry.metadata().map(|found| found.len()),
            kind if kind.is_dir() => entry
                .open_dir()
                .and_then(|nested| stored(&nested, depth + 1)),
            _ => Ok(0),
        };
        match counted {
            Ok(bytes) => total = total.saturating_add(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(total)
}
