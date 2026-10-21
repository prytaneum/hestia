//! Persistent Minecraft instance (client) store: each instance lives at
//! `<dir>/<slug>/` (the name slugged; the id is opaque) — the `instance.json`
//! record beside `data/`, the game
//! directory the client writes into (saves, options). The root is reserved
//! for managed content directories (`mods/`, `resourcepacks/`, `configs/`);
//! every directory appears on demand. Files shared across
//! instances (client jars, libraries, assets) live in the engine-wide stores
//! and are materialised at launch.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::error::EntryKind;
use proto::keys::KeyRule;
use proto::sync::SyncOverrides;
use serde::{Deserialize, Serialize};

use crate::jvm::{JavaSettings, JVM_ARGS_KEY, MEMORY_KEY};
use crate::minecraft::profile::InstanceProfile;
use crate::registry::{Entry, Store};
use crate::schema::Document;

pub(crate) const DATA: &str = "data";

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRecord {
    pub id: String,
    pub name: String,
    pub created_unix: i64,
    /// Unix time of the most recent launch; `None` until first played.
    #[serde(default)]
    pub last_played_unix: Option<i64>,
    /// Cumulative seconds played, accumulated as each session exits.
    #[serde(default)]
    pub playtime_seconds: i64,
    /// Per-entry JVM tuning (memory, extra flags) injected at each launch.
    #[serde(default)]
    pub jvm: JavaSettings,
    /// What this instance does differently from the sync catalogue.
    #[serde(default, skip_serializing_if = "SyncOverrides::is_empty")]
    pub sharing: SyncOverrides,
    pub profile: InstanceProfile,
}

impl Document for InstanceRecord {
    const NAME: &'static str = "instance.json";
}

impl Entry for InstanceRecord {
    const KIND: EntryKind = EntryKind::Instance;

    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

pub struct Instances {
    store: Store<InstanceRecord>,
}

impl Instances {
    pub fn new(dir: PathBuf) -> Self {
        Instances {
            store: Store::new(dir),
        }
    }

    /// The instance's directory, named for its current display name, so a
    /// rename moves it; the id stays the entry's stable internal key.
    pub fn instance_dir(&self, record: &InstanceRecord) -> PathBuf {
        self.store.entry_dir(record)
    }

    /// The instance's game directory — everything the client itself reads and
    /// writes (saves, options, logs).
    pub fn data_dir(&self, record: &InstanceRecord) -> PathBuf {
        self.instance_dir(record).join(DATA)
    }

    pub fn list(&self) -> Vec<InstanceRecord> {
        self.store.list()
    }

    /// Find one instance by id or name (any spelling that slugs the same).
    pub fn get(&self, reference: &str) -> Option<InstanceRecord> {
        self.store.get(reference)
    }

    pub fn require(&self, reference: &str) -> Result<InstanceRecord> {
        self.store.require(reference)
    }

    pub fn create(&self, name: &str, profile: InstanceProfile) -> Result<InstanceRecord> {
        self.adopt(
            name,
            InstanceRecord {
                id: String::new(),
                name: name.to_string(),
                created_unix: common::now_unix(),
                last_played_unix: None,
                playtime_seconds: 0,
                jvm: JavaSettings::default(),
                sharing: SyncOverrides::default(),
                profile,
            },
        )
    }

    /// Register a record that came from somewhere other than a fresh create —
    /// an imported archive, which carries the instance's settings and how long
    /// it has been played. The **id is always freshly allocated**: an id is this
    /// launcher's internal key, and honouring one from a file would let an
    /// archive collide with an existing entry (or with itself, imported twice).
    pub fn adopt(&self, name: &str, record: InstanceRecord) -> Result<InstanceRecord> {
        let record = self.store.register(name, |id| {
            Ok(InstanceRecord {
                id,
                name: name.to_string(),
                ..record
            })
        })?;
        tracing::info!(id = %record.id, name, "instance registered");
        Ok(record)
    }

    /// Swap the record onto a freshly resolved profile; the new version's files
    /// (version-keyed under the shared roots) materialise at the next launch.
    /// Name, JVM settings, and the game directory are untouched.
    pub fn update(&self, id: &str, profile: InstanceProfile) -> Result<InstanceRecord> {
        let record = self.store.mutate(id, |record| {
            record.profile = profile;
            Ok(record.clone())
        })?;
        tracing::info!(
            id = %record.id,
            version = %record.profile.game_version,
            loader = ?record.profile.loader_version,
            "instance updated"
        );
        Ok(record)
    }

    /// Stamp the most-recent-launch time. Called when a session spawns, so the
    /// next launch no longer counts as a first play.
    pub fn mark_launched(&self, id: &str) -> Result<()> {
        self.store.mutate(id, |record| {
            record.last_played_unix = Some(common::now_unix());
            Ok(())
        })
    }

    /// Add an exited session's duration to the cumulative playtime. A
    /// non-positive duration is a no-op.
    pub fn add_playtime(&self, id: &str, seconds: i64) -> Result<()> {
        if seconds <= 0 {
            return Ok(());
        }
        self.store.mutate(id, |record| {
            record.playtime_seconds += seconds;
            Ok(())
        })
    }

    /// Read one JVM setting (`memory` / `jvm-args`); `Ok(None)` means unset. An
    /// unknown key is an error naming the valid keys.
    pub fn config_get(&self, id: &str, key: &str) -> Result<Option<String>> {
        self.store.require(id)?.jvm.get(key).with_context(|| {
            format!("unknown key '{key}' (valid keys: {MEMORY_KEY}, {JVM_ARGS_KEY})")
        })
    }

    /// Every rule an instance's settings are checked against.
    pub fn config_rules() -> Vec<KeyRule> {
        JavaSettings::rules()
    }

    /// Write one JVM setting; an empty value clears it. Settings take effect on
    /// the next launch.
    pub fn config_set(&self, id: &str, key: &str, value: &str) -> Result<()> {
        self.store.mutate(id, |record| {
            if !record.jvm.set(key, value)? {
                bail!(proto::error::ErrorInfo::ConfigKeyUnknown {
                    key: key.to_string()
                });
            }
            Ok(())
        })
    }

    /// Both JVM settings with their current values (empty when unset).
    pub fn config_list(&self, id: &str) -> Result<Vec<(String, String)>> {
        Ok(self.store.require(id)?.jvm.entries())
    }

    pub fn set_overrides(&self, id: &str, overrides: SyncOverrides) -> Result<InstanceRecord> {
        self.store.mutate(id, |record| {
            record.sharing = overrides;
            Ok(record.clone())
        })
    }

    /// Rename an instance: rewrite the display name and move its directory to
    /// the new slug. The id is stable, so JVM settings and game data are
    /// untouched. The caller guarantees the instance is stopped and not busy.
    pub fn rename(&self, reference: &str, new_name: &str) -> Result<InstanceRecord> {
        self.store.rename(reference, new_name)
    }

    /// Delete an instance's directory (record, saves and all). Returns false
    /// when no instance matches.
    pub fn remove(&self, reference: &str) -> Result<bool> {
        self.store.remove(reference)
    }
}

/// The instance's save worlds as data-relative `saves/<name>` paths — where a
/// datapack is mirrored to.
pub(crate) fn save_worlds(data_dir: &Path) -> Vec<String> {
    let mut worlds: Vec<String> = std::fs::read_dir(data_dir.join("saves"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| format!("saves/{}", entry.file_name().to_string_lossy()))
        .collect();
    worlds.sort();
    worlds
}
