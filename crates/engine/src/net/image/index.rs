//! Which cached blob answers a URL. The blobs are content-addressed, so the
//! URL -> blob link is the only thing the cache cannot derive; it is a small
//! JSON file beside them, read per lookup so a `cache clear` that deletes it
//! cannot leave a stale in-memory copy behind.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Oldest entries are dropped past this, so the file stays small and the blobs
/// it keeps alive stay bounded.
const MAX_ENTRIES: usize = 256;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub sha256: String,
    pub mime: String,
    pub fetched_unix: i64,
}

pub struct Index {
    path: PathBuf,
    writing: Mutex<()>,
}

impl Index {
    pub fn new(path: PathBuf) -> Self {
        Index {
            path,
            writing: Mutex::new(()),
        }
    }

    pub fn lookup(&self, url: &str) -> Option<Entry> {
        self.read().remove(url)
    }

    /// Record `url`, returning the digests of blobs no entry refers to any more.
    pub fn insert(&self, url: &str, entry: Entry) -> Vec<String> {
        let _guard = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut entries = self.read();
        let mut released = Vec::new();
        released.extend(entries.insert(url.to_string(), entry).map(|old| old.sha256));
        while entries.len() > MAX_ENTRIES {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.fetched_unix)
                .map(|(url, _)| url.clone())
            else {
                break;
            };
            released.extend(entries.remove(&oldest).map(|old| old.sha256));
        }
        released.retain(|sha| !entries.values().any(|entry| &entry.sha256 == sha));
        released.sort();
        released.dedup();
        self.write(&entries);
        released
    }

    fn read(&self) -> BTreeMap<String, Entry> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, entries: &BTreeMap<String, Entry>) {
        let Ok(bytes) = serde_json::to_vec(entries) else {
            return;
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = common::atomic::write(&self.path, &bytes) {
            tracing::warn!(%error, "could not persist the image index");
        }
    }
}
