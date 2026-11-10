use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::change::Scope;
use crate::contract::{Contract, Empty};
use crate::naming;
use crate::plugin::Permission;

/// Reserved keys the daemon routes to their own subsystem instead of the settings
/// store: the data-directory pointer and the login registration.
pub const HOME_KEY: &str = "home";
pub const AUTOSTART_KEY: &str = "autostart";
/// The setting that unlocks the plugin authoring tools.
pub const DEVELOPER_KEY: &str = "developer";
/// The setting that holds the CurseForge API key.
pub const CURSEFORGE_KEY: &str = "content.curseforge-key";

/// Keys only the user may set: they decide where data lives, what runs at login,
/// which tools unlock, which runtime is downloaded and what every launch passes
/// to the JVM.
const USER_ONLY_KEYS: [&str; 5] = [
    HOME_KEY,
    AUTOSTART_KEY,
    DEVELOPER_KEY,
    "java.provider",
    "defaults.jvm-args",
];

/// Keys a plugin may not read: secrets.
const PLUGIN_HIDDEN_KEYS: [&str; 1] = [CURSEFORGE_KEY];

fn segments(key: &str) -> Vec<String> {
    key.split('.').map(naming::config_key_to_field).collect()
}

/// Whether a plugin may set `key`. A key is matched in its canonical form, so
/// `defaults.jvmArgs` is the same key as `defaults.jvm-args`, and setting a
/// parent object counts as setting every key under it.
pub fn plugin_may_write(key: &str) -> bool {
    let key = segments(key);
    !USER_ONLY_KEYS
        .iter()
        .any(|protected| segments(protected).starts_with(&key))
}

/// Whether a plugin may read `key` itself. A parent of a hidden key is readable;
/// [`hide_from_plugin`] removes the hidden key from what it returns.
pub fn plugin_may_read(key: &str) -> bool {
    let key = segments(key);
    !PLUGIN_HIDDEN_KEYS
        .iter()
        .any(|hidden| key.starts_with(&segments(hidden)))
}

/// Remove every key a plugin may not read from `value`, the settings found at
/// `prefix` (empty for the whole tree).
pub fn hide_from_plugin(prefix: &str, value: &mut Value) {
    let prefix = if prefix.is_empty() {
        Vec::new()
    } else {
        segments(prefix)
    };
    for hidden in PLUGIN_HIDDEN_KEYS {
        let hidden = segments(hidden);
        if let Some(rest) = hidden.strip_prefix(prefix.as_slice()) {
            remove_path(value, rest);
        }
    }
}

fn remove_path(value: &mut Value, path: &[String]) {
    let (Some((first, rest)), Value::Object(map)) = (path.split_first(), &mut *value) else {
        return;
    };
    let Some(name) = map
        .keys()
        .find(|name| naming::config_key_to_field(name) == *first)
        .cloned()
    else {
        return;
    };
    if rest.is_empty() {
        map.remove(&name);
    } else if let Some(child) = map.get_mut(&name) {
        remove_path(child, rest);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ConfigGetParams {
    pub key: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigGetResult {
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub value: Value,
}

pub struct ConfigGet;
impl Contract for ConfigGet {
    const CHANNEL: &'static str = "config.get";
    const PLUGIN: Option<Permission> = Some(Permission::ConfigRead);
    type Params = ConfigGetParams;
    type Result = ConfigGetResult;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetParams {
    pub key: String,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub value: Value,
}

pub struct ConfigSet;
impl Contract for ConfigSet {
    const CHANNEL: &'static str = "config.set";
    const PLUGIN: Option<Permission> = Some(Permission::ConfigWrite);
    const CHANGES: Option<Scope> = Some(Scope::Config);
    type Params = ConfigSetParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigListResult {
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
    pub entries: Value,
}

pub struct ConfigList;
impl Contract for ConfigList {
    const CHANNEL: &'static str = "config.list";
    const PLUGIN: Option<Permission> = Some(Permission::ConfigRead);
    type Params = Empty;
    type Result = ConfigListResult;
}
