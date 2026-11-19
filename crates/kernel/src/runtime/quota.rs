use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use cap_std::ambient_authority;
use cap_std::fs::{Dir, DirEntry};

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
            kind if kind.is_file() => file_bytes(&entry),
            kind if kind.is_dir() => entry.open_dir().and_then(|nested| {
                let named = dir_streams(&nested)?;
                stored(&nested, depth + 1).map(|inside| inside.saturating_add(named))
            }),
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

#[cfg(not(windows))]
fn file_bytes(entry: &DirEntry) -> std::io::Result<u64> {
    entry.metadata().map(|found| found.len())
}

#[cfg(not(windows))]
fn dir_streams(_: &Dir) -> std::io::Result<u64> {
    Ok(0)
}

#[cfg(windows)]
fn file_bytes(entry: &DirEntry) -> std::io::Result<u64> {
    match entry.open() {
        Ok(file) => streams(&file),
        Err(_) => entry.metadata().map(|found| found.len()),
    }
}

#[cfg(windows)]
fn dir_streams(dir: &Dir) -> std::io::Result<u64> {
    streams(dir)
}

/// The bytes in every stream of what `handle` opens. Windows lets a file or a
/// directory carry alternate data streams (`name:stream`) that its length leaves
/// out, and a plugin can create them through `/data`. More streams than one
/// query lists count as over any quota.
#[cfg(windows)]
fn streams(handle: &impl std::os::windows::io::AsRawHandle) -> std::io::Result<u64> {
    use windows_sys::Win32::Foundation::{ERROR_HANDLE_EOF, ERROR_MORE_DATA};
    use windows_sys::Win32::Storage::FileSystem::{
        FileStreamInfo, GetFileInformationByHandleEx, FILE_STREAM_INFO,
    };
    const QUERY_BYTES_MAX: usize = 1 << 20;
    let mut buffer = vec![0u64; 512];
    loop {
        // SAFETY: the handle is open for the call; the buffer is 8-aligned and
        // its length in bytes is the size passed.
        let queried = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileStreamInfo,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
            )
        };
        if queried != 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_HANDLE_EOF) => return Ok(0),
            Some(ERROR_MORE_DATA) if buffer.len() * 8 < QUERY_BYTES_MAX => {
                buffer.resize(buffer.len() * 2, 0)
            }
            Some(ERROR_MORE_DATA) => return Ok(u64::MAX),
            _ => return Err(error),
        }
    }
    let bytes = buffer.len() * 8;
    let (mut offset, mut total) = (0usize, 0u64);
    while offset + std::mem::size_of::<FILE_STREAM_INFO>() <= bytes {
        // SAFETY: the query wrote a chain of 8-aligned FILE_STREAM_INFO records
        // from the start of the buffer, and the bound above keeps the read in it.
        let info = unsafe {
            &*buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<FILE_STREAM_INFO>()
        };
        total = total.saturating_add(u64::try_from(info.StreamSize).unwrap_or(0));
        if info.NextEntryOffset == 0 {
            break;
        }
        offset += info.NextEntryOffset as usize;
    }
    Ok(total)
}
