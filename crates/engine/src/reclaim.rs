//! Reclamation of abandoned temp artifacts.
//!
//! Several subsystems write through a temp artifact so their result lands whole
//! or not at all: a `.part` file (the backup archive, the downloader) or a
//! `.staging` directory (the Java installer). The convention assumes the process
//! that created one either finishes or cleans up — which is exactly the
//! assumption a crash breaks, and nothing else ever reaped them.
//!
//! **The invariant:** a temp artifact is only valid while the job that created
//! it holds its in-flight claim (the daemon's `Claims`). No claim survives a
//! restart, so at startup every artifact still on disk is abandoned by
//! definition — its job is gone and will never finish. Each owning subsystem
//! therefore reclaims its own on init, the way the process supervisor sweeps
//! stray process directories.

use std::path::Path;

/// What one reclamation pass freed. Reported rather than silently discarded, so
/// a leak is visible in the log instead of only in disk usage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Reclaimed {
    pub entries: usize,
    pub bytes: u64,
}

impl Reclaimed {
    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    fn add(&mut self, bytes: u64) {
        self.entries += 1;
        self.bytes += bytes;
    }
}

impl std::ops::AddAssign for Reclaimed {
    fn add_assign(&mut self, other: Self) {
        self.entries += other.entries;
        self.bytes += other.bytes;
    }
}

/// Delete every entry directly under `dir` whose name ends in `suffix`. Not
/// recursive: each caller knows the one directory its artifacts land in, and a
/// deep walk of a data home (an asset store is six figures of files) is not
/// something to pay for at every start.
pub fn suffixed(dir: &Path, suffix: &str) -> Reclaimed {
    let mut freed = Reclaimed::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return freed;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with(suffix) {
            continue;
        }
        let path = entry.path();
        let removed = match entry.file_type() {
            Ok(kind) if kind.is_dir() => {
                let bytes = crate::usage::dir_size(&path);
                std::fs::remove_dir_all(&path).map(|()| bytes)
            }
            _ => {
                let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                std::fs::remove_file(&path).map(|()| bytes)
            }
        };
        match removed {
            Ok(bytes) => {
                tracing::debug!(path = %path.display(), bytes, "reclaimed an abandoned temp artifact");
                freed.add(bytes);
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot reclaim a temp artifact")
            }
        }
    }
    freed
}

/// Empty a scratch directory of everything, keeping the directory itself.
pub fn contents(dir: &Path) -> Reclaimed {
    let mut freed = Reclaimed::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return freed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = entry.file_type().map(|k| k.is_dir()).unwrap_or(false);
        let bytes = if is_dir {
            crate::usage::dir_size(&path)
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };
        let removed = if is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match removed {
            Ok(()) => {
                tracing::debug!(path = %path.display(), bytes, "reclaimed an abandoned temp artifact");
                freed.add(bytes);
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot reclaim a temp artifact")
            }
        }
    }
    freed
}
