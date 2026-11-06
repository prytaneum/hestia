//! The plugin repositories the user added, and the last good copy of each
//! index. The pinned key is what a repository is trusted by; the metadata is
//! only a snapshot for showing it before its index is read.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use proto::repository::RepositoryIndex;
use serde::{Deserialize, Serialize};

use crate::net::store;
use crate::schema::{self, Document};

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryEntry {
    /// The index URL, which identifies the repository.
    pub url: String,
    /// The minisign public key pinned when the repository was added.
    pub key: String,
    pub name: String,
    pub label: String,
    pub website: String,
    pub added_unix: i64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Saved {
    repositories: Vec<RepositoryEntry>,
}

impl Document for Saved {
    const NAME: &'static str = "repositories.json";
}

pub struct Repositories {
    path: PathBuf,
    writes: Mutex<()>,
}

impl Repositories {
    pub fn new(path: PathBuf) -> Self {
        Repositories {
            path,
            writes: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Vec<RepositoryEntry> {
        schema::load::<Saved>(&self.path)
            .map(|saved| saved.repositories)
            .unwrap_or_default()
    }

    pub fn get(&self, url: &str) -> Option<RepositoryEntry> {
        self.list().into_iter().find(|entry| entry.url == url)
    }

    /// Adds the repository, or replaces the one with the same URL.
    pub fn save(&self, entry: RepositoryEntry) -> Result<()> {
        self.update(|repositories| {
            match repositories.iter_mut().find(|saved| saved.url == entry.url) {
                Some(saved) => *saved = entry,
                None => repositories.push(entry),
            }
            true
        })?;
        Ok(())
    }

    /// False when no such repository is configured. Its cached index goes
    /// with it only in the sense that nothing reads it any more.
    pub fn remove(&self, url: &str) -> Result<bool> {
        self.update(|repositories| {
            let before = repositories.len();
            repositories.retain(|saved| saved.url != url);
            repositories.len() != before
        })
    }

    fn update(&self, change: impl FnOnce(&mut Vec<RepositoryEntry>) -> bool) -> Result<bool> {
        let _writes = self.writes.lock().unwrap();
        let mut saved = schema::load::<Saved>(&self.path).unwrap_or_default();
        let changed = change(&mut saved.repositories);
        if changed {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            schema::save(&self.path, &saved)?;
        }
        Ok(changed)
    }
}

/// The last index of `url` that passed verification.
pub fn cached_index(url: &str) -> Option<RepositoryIndex> {
    let body = store::load(url)?;
    serde_json::from_str(&body)
        .inspect_err(|error| tracing::warn!(url, %error, "the cached plugin index is unreadable"))
        .ok()
}

/// Only for bytes that passed verification, which are JSON.
pub fn cache_index(url: &str, bytes: &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(body) => store::save(url, body),
        Err(error) => tracing::warn!(url, %error, "plugin index not cached"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str) -> RepositoryEntry {
        RepositoryEntry {
            url: url.to_string(),
            key: "key".to_string(),
            ..RepositoryEntry::default()
        }
    }

    #[test]
    fn repositories_persist_replace_by_url_and_remove() {
        let home = tempfile::tempdir().unwrap();
        let repositories = Repositories::new(home.path().join("repositories.json"));

        repositories
            .save(entry("https://a.example/i.json"))
            .unwrap();
        repositories
            .save(entry("https://b.example/i.json"))
            .unwrap();
        let mut renamed = entry("https://a.example/i.json");
        renamed.name = "A".to_string();
        repositories.save(renamed).unwrap();

        let saved = repositories.list();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].name, "A");
        assert!(repositories.remove("https://a.example/i.json").unwrap());
        assert!(!repositories.remove("https://a.example/i.json").unwrap());
        assert_eq!(repositories.list().len(), 1);
        assert!(repositories.get("https://b.example/i.json").is_some());
    }
}
