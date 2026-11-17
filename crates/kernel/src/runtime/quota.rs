use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use cap_std::ambient_authority;
use cap_std::fs::Dir;

pub(super) const DATA_QUOTA: u64 = 8 * 1024 * 1024 * 1024;
const DEPTH_MAX: usize = 64;

/// What a plugin's `/data` may hold, and which plugins held more at the end of
/// their last call. Writes through the WASI preopen cannot be stopped as they
/// happen, so a plugin that ends a call over the limit is only barred from
/// unattended calls until a later call ends under it.
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

    pub fn is_over(&self, plugin: &str) -> bool {
        self.over
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(plugin)
    }

    /// Measures `data` off the executor and records whether it is over the
    /// limit. A directory that cannot be measured leaves the record as it was.
    pub async fn measure(&self, plugin: &str, data: PathBuf) {
        let used = tokio::task::spawn_blocking(move || {
            let dir = Dir::open_ambient_dir(data, ambient_authority())?;
            stored(&dir, 0)
        })
        .await;
        let used = match used {
            Ok(Ok(used)) => used,
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => return,
            Ok(Err(error)) => {
                tracing::warn!(plugin, %error, "cannot measure the plugin's data");
                return;
            }
            Err(error) => {
                tracing::warn!(plugin, %error, "measuring the plugin's data failed");
                return;
            }
        };
        let mut over = self.over.lock().unwrap_or_else(PoisonError::into_inner);
        match used > self.limit {
            true if over.insert(plugin.to_owned()) => {
                tracing::warn!(
                    plugin,
                    used,
                    limit = self.limit,
                    "plugin data is over its quota; unattended calls are refused until it is under"
                );
            }
            false if over.remove(plugin) => {
                tracing::info!(plugin, used, "plugin data is back under its quota");
            }
            _ => {}
        }
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
