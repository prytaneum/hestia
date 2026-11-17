use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use common::schema::{self, Document};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::bindings::hestia::kernel::{secrets, settings, types::PluginError};
use super::host::State;
use super::shared::Shared;

const KEY_BYTES: usize = 128;
const VALUE_BYTES: usize = 64 << 10;
const KEYS: usize = 256;
const FILE_BYTES: usize = 1 << 20;

/// A plugin's settings, as JSON values the manifest describes.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Settings {
    values: BTreeMap<String, Value>,
}

impl Document for Settings {
    const NAME: &'static str = "settings.json";
}

/// A plugin's secrets, written owner-only.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Secrets {
    values: BTreeMap<String, Value>,
}

impl Document for Secrets {
    const NAME: &'static str = "secrets.json";
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Settings,
    Secrets,
}

impl Kind {
    fn path(self, dir: &Path) -> PathBuf {
        dir.join(match self {
            Kind::Settings => Settings::NAME,
            Kind::Secrets => Secrets::NAME,
        })
    }

    /// A file this build cannot read is set aside by `schema`, never written over.
    fn load(self, path: &Path) -> BTreeMap<String, Value> {
        match self {
            Kind::Settings => schema::load::<Settings>(path).map(|d| d.values),
            Kind::Secrets => schema::load::<Secrets>(path).map(|d| d.values),
        }
        .unwrap_or_default()
    }

    fn save(
        self,
        path: &Path,
        values: &BTreeMap<String, Value>,
    ) -> Result<(), schema::SchemaError> {
        let values = values.clone();
        match self {
            Kind::Settings => schema::save(path, &Settings { values }),
            Kind::Secrets => schema::save_private(path, &Secrets { values }),
        }
    }
}

type Values = Arc<Mutex<BTreeMap<String, Value>>>;

/// Every plugin's settings and secrets, read once and kept in memory; each
/// file has its own lock, so one plugin never waits on another's.
#[derive(Default)]
pub(super) struct Files {
    open: Mutex<HashMap<PathBuf, Values>>,
}

impl Files {
    fn values(&self, dir: &Path, kind: Kind) -> Values {
        let path = kind.path(dir);
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(kind.load(&path))))
            .clone()
    }

    pub(super) fn forget(&self, dir: &Path) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.retain(|path, _| path.parent() != Some(dir));
    }

    fn get(&self, dir: &Path, kind: Kind, key: &str) -> Option<Value> {
        let values = self.values(dir, kind);
        let values = values.lock().unwrap_or_else(PoisonError::into_inner);
        values.get(key).cloned()
    }

    fn set(&self, dir: &Path, kind: Kind, key: &str, value: Value) -> Result<(), PluginError> {
        if key.is_empty() || key.len() > KEY_BYTES {
            return Err(PluginError::Invalid("key".into()));
        }
        let values = self.values(dir, kind);
        let mut values = values.lock().unwrap_or_else(PoisonError::into_inner);
        if !values.contains_key(key) && values.len() >= KEYS {
            return Err(PluginError::Invalid(format!("more than {KEYS} keys")));
        }
        let mut next = values.clone();
        next.insert(key.to_owned(), value);
        let size = serde_json::to_vec(&next).map_or(usize::MAX, |bytes| bytes.len());
        if size > FILE_BYTES {
            return Err(PluginError::Invalid(format!(
                "more than {FILE_BYTES} bytes stored"
            )));
        }
        let saved = std::fs::create_dir_all(dir)
            .map_err(|error| error.to_string())
            .and_then(|()| {
                kind.save(&kind.path(dir), &next)
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = saved {
            tracing::warn!(%error, "plugin state not written");
            return Err(PluginError::Failed("cannot save".into()));
        }
        *values = next;
        Ok(())
    }
}

fn json(value: &str) -> Result<Value, PluginError> {
    if value.len() > VALUE_BYTES {
        return Err(PluginError::Invalid("value too large".into()));
    }
    serde_json::from_str(value).map_err(|_| PluginError::Invalid("value is not JSON".into()))
}

fn state_dir(shared: &Shared, plugin: &str) -> Result<PathBuf, PluginError> {
    shared
        .services
        .state_dir(plugin)
        .ok_or(PluginError::Unsupported)
}

/// Stores a setting, whether the plugin or the user changed it.
pub(super) fn save_setting(
    shared: &Shared,
    plugin: &str,
    key: &str,
    value: &str,
) -> Result<(), PluginError> {
    let dir = state_dir(shared, plugin)?;
    shared.files.set(&dir, Kind::Settings, key, json(value)?)
}

/// Stores a secret the user entered; the value never reaches a log.
pub(super) fn save_secret(
    shared: &Shared,
    plugin: &str,
    key: &str,
    value: &str,
) -> Result<(), PluginError> {
    if value.len() > VALUE_BYTES {
        return Err(PluginError::Invalid("value too large".into()));
    }
    let dir = state_dir(shared, plugin)?;
    shared
        .files
        .set(&dir, Kind::Secrets, key, Value::String(value.to_owned()))
}

impl settings::Host for State {
    fn get(&mut self, key: String) -> Result<Option<String>, PluginError> {
        let dir = state_dir(&self.shared, &self.plugin)?;
        let found = self.shared.files.get(&dir, Kind::Settings, &key);
        Ok(found.map(|value| value.to_string()))
    }

    fn set(&mut self, key: String, value: String) -> Result<(), PluginError> {
        save_setting(&self.shared, &self.plugin, &key, &value)
    }
}

/// Linked for every plugin; one the user did not grant `secrets` reaches the
/// grant's denied stub instead.
impl secrets::Host for State {
    fn get(&mut self, key: String) -> Result<Option<String>, PluginError> {
        let dir = state_dir(&self.shared, &self.plugin)?;
        let found = self.shared.files.get(&dir, Kind::Secrets, &key);
        Ok(found.and_then(|value| value.as_str().map(str::to_owned)))
    }

    fn set(&mut self, key: String, value: String) -> Result<(), PluginError> {
        save_secret(&self.shared, &self.plugin, &key, &value)
    }
}
