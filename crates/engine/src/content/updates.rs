//! What earlier update checks found. A check inside the TTL is answered from
//! here without asking upstream, and the list of pending updates never waits
//! on the network: an answer past its TTL is still served until a check
//! replaces it. Modrinth's own launcher keeps the same 30-minute window.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use proto::content::{ContentVersion, InstalledContent};

pub(crate) const TTL: Duration = Duration::from_secs(30 * 60);

/// One installed release asked about under one game version and loader. The
/// installed version is part of the key, so updating an item retires its old
/// answer rather than having to invalidate it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct UpdateKey {
    source: String,
    project_id: String,
    version_id: String,
    game_version: String,
    loader: Option<String>,
}

impl UpdateKey {
    pub(crate) fn of(item: &InstalledContent, game_version: &str, loader: Option<&str>) -> Self {
        UpdateKey {
            source: item.source.clone(),
            project_id: item.project_id.clone(),
            version_id: item.version_id.clone(),
            game_version: game_version.to_string(),
            loader: loader.map(str::to_string),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Latest {
    Current,
    Newer(Box<ContentVersion>),
}

impl Latest {
    fn newer_id(&self) -> Option<&str> {
        match self {
            Latest::Current => None,
            Latest::Newer(version) => Some(&version.id),
        }
    }
}

struct Answer {
    latest: Latest,
    checked: Instant,
}

#[derive(Default)]
pub(crate) struct UpdateCache {
    answers: Mutex<HashMap<UpdateKey, Answer>>,
}

impl UpdateCache {
    /// The answer when it is younger than the TTL.
    pub(crate) fn fresh(&self, key: &UpdateKey, now: Instant) -> Option<Latest> {
        let answers = self.answers.lock().unwrap();
        answers
            .get(key)
            .filter(|answer| now.duration_since(answer.checked) < TTL)
            .map(|answer| answer.latest.clone())
    }

    /// The answer whatever its age.
    pub(crate) fn known(&self, key: &UpdateKey) -> Option<Latest> {
        let answers = self.answers.lock().unwrap();
        answers.get(key).map(|answer| answer.latest.clone())
    }

    /// Store an answer. True when it reports an update no earlier answer for
    /// the key had — the moment worth telling a user about.
    pub(crate) fn record(&self, key: UpdateKey, latest: Latest, now: Instant) -> bool {
        let mut answers = self.answers.lock().unwrap();
        let fresh = latest.newer_id().is_some()
            && answers.get(&key).and_then(|a| a.latest.newer_id()) != latest.newer_id();
        answers.insert(
            key,
            Answer {
                latest,
                checked: now,
            },
        );
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> UpdateKey {
        let item = InstalledContent {
            source: "modrinth".to_string(),
            project_id: "AANobbMI".to_string(),
            version_id: "v1".to_string(),
            ..Default::default()
        };
        UpdateKey::of(&item, "1.21.1", Some("fabric"))
    }

    fn id(latest: Option<Latest>) -> Option<String> {
        latest.and_then(|l| l.newer_id().map(str::to_string))
    }

    fn newer(id: &str) -> Latest {
        Latest::Newer(Box::new(ContentVersion {
            id: id.to_string(),
            ..Default::default()
        }))
    }

    #[test]
    fn an_answer_is_fresh_inside_the_ttl_and_known_after_it() {
        let cache = UpdateCache::default();
        let then = Instant::now();
        cache.record(key(), newer("v2"), then);

        assert_eq!(id(cache.fresh(&key(), then + TTL / 2)), Some("v2".into()));
        assert!(cache.fresh(&key(), then + TTL).is_none());
        assert_eq!(id(cache.known(&key())), Some("v2".into()));
    }

    #[test]
    fn only_a_newly_reported_update_is_fresh() {
        let cache = UpdateCache::default();
        let now = Instant::now();

        assert!(!cache.record(key(), Latest::Current, now));
        assert!(cache.record(key(), newer("v2"), now));
        assert!(!cache.record(key(), newer("v2"), now));
        assert!(cache.record(key(), newer("v3"), now));
    }

    #[test]
    fn the_installed_version_is_part_of_the_key() {
        let cache = UpdateCache::default();
        cache.record(key(), newer("v2"), Instant::now());
        let updated = InstalledContent {
            source: "modrinth".to_string(),
            project_id: "AANobbMI".to_string(),
            version_id: "v2".to_string(),
            ..Default::default()
        };

        assert!(cache
            .known(&UpdateKey::of(&updated, "1.21.1", Some("fabric")))
            .is_none());
    }
}
