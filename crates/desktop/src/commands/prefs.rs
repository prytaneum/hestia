//! Desktop-local preferences, written straight into the hestia data home.
//!
//! Front-end UI state (a dismissed first-run overlay, remembered view) is not
//! the daemon's concern, so it never crosses the socket: these commands resolve
//! the same data home the engine uses (`common::paths`, honouring `--home` /
//! `$HESTIA_HOME` / the persisted pointer) and read/write `prefs.json` there
//! directly. Schema-less — the front-end owns its own keys.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::bridge::CallError;

type Map = BTreeMap<String, Value>;

// Serialize the read-modify-write so two concurrent commands can't lose an edit.
static LOCK: Mutex<()> = Mutex::new(());

fn path() -> PathBuf {
    common::paths::data_home(None).join("prefs.json")
}

fn load(path: &Path) -> Map {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %e, "prefs unreadable; using defaults");
            }
            return Map::new();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        let corrupt = path.with_extension("json.corrupt");
        tracing::warn!(error = %e, kept = %corrupt.display(), "prefs unparsable; quarantining");
        if let Err(e) = std::fs::rename(path, &corrupt) {
            tracing::warn!(error = %e, "could not quarantine prefs");
        }
        Map::new()
    })
}

fn save(path: &Path, values: &Map) -> Result<(), CallError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(values)?;
    common::atomic::write(path, format!("{text}\n").as_bytes())?;
    Ok(())
}

#[tauri::command]
pub fn prefs_list() -> Result<Value, CallError> {
    let _guard = LOCK.lock().unwrap();
    Ok(serde_json::to_value(load(&path())).unwrap_or_else(|_| Value::Object(Default::default())))
}

#[tauri::command]
pub fn prefs_set(key: String, value: Value) -> Result<(), CallError> {
    tracing::debug!(key, "pref set");
    let _guard = LOCK.lock().unwrap();
    let path = path();
    let mut values = load(&path);
    values.insert(key, value);
    save(&path, &values)
}

#[tauri::command]
pub fn prefs_remove(key: String) -> Result<(), CallError> {
    tracing::debug!(key, "pref removed");
    let _guard = LOCK.lock().unwrap();
    let path = path();
    let mut values = load(&path);
    values.remove(&key);
    save(&path, &values)
}
