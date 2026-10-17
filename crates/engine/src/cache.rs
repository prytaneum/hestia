//! Content-addressed store of verified downloads, keyed by checksum
//! (`<dir>/<algorithm>/<hex[0:2]>/<hex>`). Blobs are immutable; consumers
//! re-verify on the way out, so a damaged blob is evicted, never served.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use proto::download::{Checksum, HashAlgorithm};

pub struct CacheEntry {
    pub checksum: Checksum,
    pub size: u64,
}

#[derive(Default, Clone, Copy)]
pub struct CacheUsage {
    pub entries: u64,
    pub bytes: u64,
}

pub struct Cache {
    dir: PathBuf,
}

const ALGORITHMS: [HashAlgorithm; 2] = [HashAlgorithm::Sha1, HashAlgorithm::Sha256];

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Cache { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn lookup(&self, checksum: &Checksum) -> Option<PathBuf> {
        if !checksum.is_valid() {
            return None;
        }
        let blob = blob_path(self.dir(), checksum);
        blob.is_file().then_some(blob)
    }

    /// Best effort: a failure to cache never fails the download that fed it.
    pub fn store(&self, file: &Path, checksum: &Checksum) {
        if !checksum.is_valid() {
            return;
        }
        let blob = blob_path(self.dir(), checksum);
        if blob.exists() {
            return;
        }
        let Some(parent) = blob.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        let tmp = blob.with_extension(format!("part{n}"));
        if let Err(e) = std::fs::copy(file, &tmp) {
            tracing::warn!(hex = %checksum.hex, "failed to stage cache blob: {e}");
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        if std::fs::rename(&tmp, &blob).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        tracing::debug!(algorithm = checksum.algorithm.as_str(), hex = %checksum.hex, "cached blob");
    }

    pub fn evict(&self, checksum: &Checksum) {
        if !checksum.is_valid() {
            return;
        }
        let _ = std::fs::remove_file(blob_path(self.dir(), checksum));
    }

    pub fn entries(&self) -> Vec<CacheEntry> {
        let base = self.dir();
        let mut out = Vec::new();
        for algorithm in ALGORITHMS {
            let root = base.join(algorithm.as_str());
            collect_entries(&root, algorithm, &mut out);
        }
        out
    }

    pub fn usage(&self) -> CacheUsage {
        let mut usage = CacheUsage::default();
        for entry in self.entries() {
            usage.entries += 1;
            usage.bytes += entry.size;
        }
        usage
    }

    pub fn clear(&self) -> CacheUsage {
        let freed = self.usage();
        let base = self.dir();
        for algorithm in ALGORITHMS {
            let _ = std::fs::remove_dir_all(base.join(algorithm.as_str()));
        }
        tracing::info!(
            entries = freed.entries,
            bytes = freed.bytes,
            "cache cleared"
        );
        freed
    }
}

fn blob_path(dir: &Path, checksum: &Checksum) -> PathBuf {
    let hex = checksum.hex.to_lowercase();
    dir.join(checksum.algorithm.as_str())
        .join(&hex[0..2])
        .join(&hex)
}

fn collect_entries(root: &Path, algorithm: HashAlgorithm, out: &mut Vec<CacheEntry>) {
    let Ok(shards) = std::fs::read_dir(root) else {
        return;
    };
    for shard in shards.flatten() {
        let Ok(blobs) = std::fs::read_dir(shard.path()) else {
            continue;
        };
        for blob in blobs.flatten() {
            let Ok(meta) = blob.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let hex = blob.file_name().to_string_lossy().into_owned();
            let checksum = Checksum { algorithm, hex };
            if checksum.is_valid() {
                out.push(CacheEntry {
                    checksum,
                    size: meta.len(),
                });
            }
        }
    }
}
