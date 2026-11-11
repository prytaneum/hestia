//! The plugin repositories the user added, and the last good copy of each
//! index. The pinned key is what a repository is trusted by; the metadata is
//! only a snapshot for showing it before its index is read.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use proto::error::ErrorInfo;
use proto::repository::RepositoryIndex;
use serde::{Deserialize, Serialize};

use super::repos::RepoError;
use crate::net::store;
use crate::schema::{self, Document};

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryEntry {
    /// The index URL, which identifies the repository.
    pub url: String,
    /// The minisign public key pinned when the repository was added.
    pub key: String,
    /// Its spare key, pinned with it; empty when the repository has none.
    pub next: String,
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
    /// Seeds the official repository when the file lacks it, and re-pins it to
    /// the keys this build trusts when the file names others.
    pub fn new(path: PathBuf) -> Self {
        let repositories = Repositories {
            path,
            writes: Mutex::new(()),
        };
        let seeded = repositories
            .update(|saved| seed_official_with(saved, &common::app::official_plugin_pubkeys()));
        if let Err(error) = seeded {
            tracing::warn!(%error, "the official plugin repository was not seeded");
        }
        repositories
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
        if is_official_repository(url) {
            return Err(ErrorInfo::from(RepoError::Official {
                repository: url.to_string(),
            })
            .into());
        }
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

/// The index URL of the official repository.
pub(super) fn official_url() -> String {
    super::index::expand(common::app::OFFICIAL_PLUGIN_REPOSITORY)
        .expect("the official repository is a valid owner/repo")
}

/// Whether `url` is the official repository, which is never removed.
pub fn is_official_repository(url: &str) -> bool {
    url == official_url()
}

/// Pins the official keys: seeding is the only path that may, since a
/// user-added repository declaring one is refused. The binary is the trust
/// anchor, so keys on disk that differ (a rotation, a hand edit) are replaced.
pub(crate) fn seed_official_with(repositories: &mut Vec<RepositoryEntry>, keys: &[&str]) -> bool {
    let url = official_url();
    let key = keys.first().copied().unwrap_or_default().to_string();
    let next = keys.get(1).copied().unwrap_or_default().to_string();
    if let Some(saved) = repositories.iter_mut().find(|saved| saved.url == url) {
        if saved.key == key && saved.next == next {
            return false;
        }
        tracing::info!("re-pinning the official plugin repository to this build's keys");
        saved.key = key;
        saved.next = next;
        return true;
    }
    repositories.insert(
        0,
        RepositoryEntry {
            url,
            key,
            next,
            name: "Official".to_string(),
            label: "HST".to_string(),
            added_unix: common::now_unix(),
            ..RepositoryEntry::default()
        },
    );
    true
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
    fn the_official_repository_is_seeded_once_and_again_when_missing() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("repositories.json");

        let first = Repositories::new(path.clone()).list();
        assert_eq!(first.len(), 1);
        assert!(is_official_repository(&first[0].url));
        let keys = common::app::official_plugin_pubkeys();
        assert_eq!(first[0].key, keys[0]);
        assert_eq!(
            Some(first[0].next.as_str()),
            keys.get(1).copied().or(Some(""))
        );

        let reopened = Repositories::new(path.clone());
        assert_eq!(reopened.list(), first);

        reopened.save(entry("https://a.example/i.json")).unwrap();
        reopened
            .update(|saved| {
                saved.clear();
                true
            })
            .unwrap();
        let reseeded = Repositories::new(path).list();
        assert_eq!(reseeded.len(), 1);
        assert!(is_official_repository(&reseeded[0].url));
    }

    #[test]
    fn the_official_repository_is_repinned_to_this_builds_keys() {
        let mut entries = Vec::new();
        seed_official_with(&mut entries, &["OLD", "OLDNEXT"]);

        assert!(seed_official_with(&mut entries, &["NEW", "NEWNEXT"]));

        assert_eq!(entries.len(), 1);
        assert_eq!(
            (entries[0].key.as_str(), entries[0].next.as_str()),
            ("NEW", "NEWNEXT")
        );
        assert!(!seed_official_with(&mut entries, &["NEW", "NEWNEXT"]));
    }

    #[test]
    fn the_official_repository_is_never_removed() {
        let home = tempfile::tempdir().unwrap();
        let repositories = Repositories::new(home.path().join("repositories.json"));

        assert!(repositories.remove(&official_url()).is_err());
        assert_eq!(repositories.list().len(), 1);
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
        assert_eq!(saved.len(), 3);
        assert_eq!(saved[1].name, "A");
        assert!(repositories.remove("https://a.example/i.json").unwrap());
        assert!(!repositories.remove("https://a.example/i.json").unwrap());
        assert_eq!(repositories.list().len(), 2);
        assert!(repositories.get("https://b.example/i.json").is_some());
    }
}
