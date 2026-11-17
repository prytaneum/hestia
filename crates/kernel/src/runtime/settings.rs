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

    /// A file this build cannot read as a document is set aside by `schema`; one
    /// it cannot read at all fails, so the next write never lands on top of it.
    fn load(self, path: &Path) -> Result<BTreeMap<String, Value>, PluginError> {
        match self {
            Kind::Settings => schema::try_load::<Settings>(path).map(|d| d.map(|d| d.values)),
            Kind::Secrets => schema::try_load::<Secrets>(path).map(|d| d.map(|d| d.values)),
        }
        .map(Option::unwrap_or_default)
        .map_err(|error| {
            tracing::error!(%error, "plugin state not read");
            PluginError::Failed("cannot read its stored state".into())
        })
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
    fn values(&self, dir: &Path, kind: Kind) -> Result<Values, PluginError> {
        let path = kind.path(dir);
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(values) = open.get(&path) {
            return Ok(values.clone());
        }
        let values = Arc::new(Mutex::new(kind.load(&path)?));
        open.insert(path, values.clone());
        Ok(values)
    }

    pub(super) fn forget(&self, dir: &Path) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.retain(|path, _| path.parent() != Some(dir));
    }

    fn get(&self, dir: &Path, kind: Kind, key: &str) -> Result<Option<Value>, PluginError> {
        let values = self.values(dir, kind)?;
        let values = values.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(values.get(key).cloned())
    }

    fn set(&self, dir: &Path, kind: Kind, key: &str, value: Value) -> Result<(), PluginError> {
        if key.is_empty() || key.len() > KEY_BYTES {
            return Err(PluginError::Invalid("key".into()));
        }
        let values = self.values(dir, kind)?;
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

/// Runs blocking file work off the async executor.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, PluginError> + Send + 'static,
) -> Result<T, PluginError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| PluginError::Failed(error.to_string()))?
}

/// Stores a setting, whether the plugin or the user changed it.
pub(super) async fn save_setting(
    shared: &Shared,
    plugin: &str,
    key: &str,
    value: &str,
) -> Result<(), PluginError> {
    let dir = state_dir(shared, plugin)?;
    let (files, key, value) = (shared.files.clone(), key.to_owned(), json(value)?);
    blocking(move || files.set(&dir, Kind::Settings, &key, value)).await
}

/// Stores a secret the user entered; the value never reaches a log.
pub(super) async fn save_secret(
    shared: &Shared,
    plugin: &str,
    key: &str,
    value: &str,
) -> Result<(), PluginError> {
    if value.len() > VALUE_BYTES {
        return Err(PluginError::Invalid("value too large".into()));
    }
    let dir = state_dir(shared, plugin)?;
    let (files, key, value) = (shared.files.clone(), key.to_owned(), value.to_owned());
    blocking(move || files.set(&dir, Kind::Secrets, &key, Value::String(value))).await
}

async fn load(
    shared: &Shared,
    plugin: &str,
    kind: Kind,
    key: String,
) -> Result<Option<Value>, PluginError> {
    let dir = state_dir(shared, plugin)?;
    let files = shared.files.clone();
    blocking(move || files.get(&dir, kind, &key)).await
}

impl settings::Host for State {
    async fn get(&mut self, key: String) -> Result<Option<String>, PluginError> {
        let found = load(&self.shared, &self.plugin, Kind::Settings, key).await?;
        Ok(found.map(|value| value.to_string()))
    }

    async fn set(&mut self, key: String, value: String) -> Result<(), PluginError> {
        save_setting(&self.shared, &self.plugin, &key, &value).await
    }
}

/// Linked for every plugin; one the user did not grant `secrets` reaches the
/// grant's denied stub instead.
impl secrets::Host for State {
    async fn get(&mut self, key: String) -> Result<Option<String>, PluginError> {
        let found = load(&self.shared, &self.plugin, Kind::Secrets, key).await?;
        Ok(found.and_then(|value| value.as_str().map(str::to_owned)))
    }

    async fn set(&mut self, key: String, value: String) -> Result<(), PluginError> {
        save_secret(&self.shared, &self.plugin, &key, &value).await
    }
}
