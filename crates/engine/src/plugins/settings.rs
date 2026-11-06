use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use proto::plugin::{Setting, SettingKind, SettingValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store::Plugins;
use crate::schema::{self, Document};

const FILE: &str = "settings.json";

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Stored {
    values: BTreeMap<String, Value>,
}

impl Document for Stored {
    const NAME: &'static str = FILE;
}

/// Why a setting could not be changed.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("the plugin declares no setting `{0}`")]
    Unknown(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

/// One plugin's declared settings and the values the user stored for them in
/// `plugins/<id>/settings.json`. The file is owner-only, written like
/// `accounts.json`, because it may hold secrets; a secret's value leaves the
/// daemon only for its own plugin, through [`get`](PluginSettings::get).
pub struct PluginSettings {
    path: PathBuf,
    declared: Vec<Setting>,
}

impl PluginSettings {
    pub fn new(path: PathBuf, declared: Vec<Setting>) -> Self {
        PluginSettings { path, declared }
    }

    fn stored(&self) -> BTreeMap<String, Value> {
        schema::load::<Stored>(&self.path)
            .unwrap_or_default()
            .values
    }

    /// Every declared setting; a secret reports only whether it is set.
    pub fn list(&self) -> Vec<SettingValue> {
        let stored = self.stored();
        self.declared
            .iter()
            .map(|setting| {
                let own = stored.get(&setting.key);
                SettingValue {
                    key: setting.key.clone(),
                    set: own.is_some(),
                    value: match setting.kind {
                        SettingKind::Secret => None,
                        _ => own.or(setting.default.as_ref()).cloned(),
                    },
                }
            })
            .collect()
    }

    /// The value in clear, secrets included: for the plugin that owns it.
    pub fn get(&self, key: &str) -> Option<Value> {
        let setting = self.declared.iter().find(|setting| setting.key == key)?;
        self.stored()
            .remove(key)
            .or_else(|| setting.default.clone())
    }

    /// Stores `value` for `key` after checking it against the declared kind;
    /// `null` clears the key.
    pub fn set(&self, key: &str, value: Value) -> Result<(), SettingsError> {
        let setting = self
            .declared
            .iter()
            .find(|setting| setting.key == key)
            .ok_or_else(|| SettingsError::Unknown(key.to_string()))?;
        let mut stored = Stored {
            values: self.stored(),
        };
        if value.is_null() {
            stored.values.remove(key);
        } else {
            setting.check(&value).map_err(SettingsError::Invalid)?;
            stored.values.insert(key.to_string(), value);
        }
        schema::save_private(&self.path, &stored)?;
        tracing::info!(setting = %key, "plugin setting changed");
        Ok(())
    }
}

impl Plugins {
    /// Blocking: reads the installed manifest for what the plugin declares.
    pub fn settings(&self, id: &str) -> Result<PluginSettings> {
        let declared = self.read_manifest(id)?.settings;
        Ok(PluginSettings::new(
            self.plugin_dir(id)?.join(FILE),
            declared,
        ))
    }

    /// Blocking: [`PluginSettings::set`] serialised with the other writes.
    pub fn set_setting(&self, id: &str, key: &str, value: Value) -> Result<(), SettingsError> {
        let settings = self.settings(id)?;
        let _writes = self.writes.lock().unwrap();
        settings.set(key, value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn declared() -> Vec<Setting> {
        let setting = |key: &str, kind, default: Option<Value>, options: &[&str]| Setting {
            key: key.into(),
            kind,
            default,
            options: options.iter().map(|option| option.to_string()).collect(),
            ..Setting::default()
        };
        vec![
            setting("api-key", SettingKind::Secret, None, &[]),
            setting("limit", SettingKind::Number, Some(json!(10)), &[]),
            setting("loud", SettingKind::Bool, None, &[]),
            setting("region", SettingKind::Select, None, &["eu", "us"]),
        ]
    }

    fn settings(dir: &tempfile::TempDir) -> PluginSettings {
        PluginSettings::new(dir.path().join(FILE), declared())
    }

    #[test]
    fn a_secret_is_masked_in_the_list_but_clear_for_its_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let settings = settings(&dir);
        settings.set("api-key", json!("hunter2")).unwrap();

        let listed = settings.list();
        assert!(listed[0].set && listed[0].value.is_none());
        assert!(!serde_json::to_string(&listed).unwrap().contains("hunter2"));
        assert_eq!(settings.get("api-key"), Some(json!("hunter2")));
    }

    #[test]
    fn defaults_apply_until_set_and_null_restores_them() {
        let dir = tempfile::tempdir().unwrap();
        let settings = settings(&dir);
        assert_eq!(settings.get("limit"), Some(json!(10)));
        assert!(!settings.list()[1].set);

        settings.set("limit", json!(3)).unwrap();
        assert_eq!(settings.get("limit"), Some(json!(3)));
        settings.set("limit", Value::Null).unwrap();
        assert_eq!(settings.get("limit"), Some(json!(10)));
    }

    #[test]
    fn a_value_of_the_wrong_kind_or_an_unknown_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let settings = settings(&dir);
        for (key, value) in [
            ("limit", json!("many")),
            ("loud", json!("yes")),
            ("region", json!("moon")),
            ("api-key", json!(7)),
        ] {
            assert!(
                matches!(settings.set(key, value), Err(SettingsError::Invalid(_))),
                "{key}"
            );
        }
        assert!(matches!(
            settings.set("nope", json!(1)),
            Err(SettingsError::Unknown(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let settings = settings(&dir);
        settings.set("api-key", json!("hunter2")).unwrap();
        let mode = std::fs::metadata(dir.path().join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }
}
