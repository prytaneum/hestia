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

/// What a plugin may do with one setting. The order is the order of trust:
/// a parent object is only as open as its least open key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PluginAccess {
    Closed,
    Read,
    ReadWrite,
}

use PluginAccess::{Closed, Read, ReadWrite};

/// The one table of what plugins may do with each setting. Every key of the
/// settings schema and each reserved key is listed, in the kebab-case config
/// vocabulary; a key missing here is closed to plugins, and the engine's drift
/// test fails until it is listed. A key is `ReadWrite` only when changing it
/// cannot redirect where data lives, what runs, what is downloaded or what
/// reaches the network.
pub const PLUGIN_ACCESS: &[(&str, PluginAccess)] = &[
    (HOME_KEY, Closed),
    (AUTOSTART_KEY, Closed),
    (DEVELOPER_KEY, Closed),
    ("defaults.memory", Read),
    ("defaults.jvm-args", Closed),
    ("instance.multi-session", Read),
    ("modpack.default-excludes", Read),
    ("modpack.exclude-files", Read),
    ("modpack.force-include-files", Read),
    ("modpack.overrides-exclusions", Read),
    ("discord.enabled", Read),
    ("update.channel", Closed),
    ("network.offline", Closed),
    ("java.provider", Closed),
    ("plugins.auto-update", Closed),
];

fn segments(key: &str) -> Vec<String> {
    key.split('.').map(naming::config_key_to_field).collect()
}

fn under(key: &[String]) -> impl Iterator<Item = PluginAccess> + '_ {
    PLUGIN_ACCESS
        .iter()
        .filter(|(listed, _)| segments(listed).starts_with(key))
        .map(|(_, access)| *access)
}

/// Whether a plugin may set `key`. A key is matched in its canonical form, so
/// `defaults.jvmArgs` is the same key as `defaults.jvm-args`, and setting a
/// parent object counts as setting every key under it. An unlisted key is not
/// writable.
pub fn plugin_may_write(key: &str) -> bool {
    let key = segments(key);
    let mut keys = under(&key).peekable();
    keys.peek().is_some() && keys.all(|access| access == ReadWrite)
}

/// Whether a plugin may see at least one setting at `key`; a parent object is
/// readable when any key under it is, and [`plugin_view`] drops the rest.
pub fn plugin_may_read(key: &str) -> bool {
    under(&segments(key)).any(|access| access >= Read)
}

/// `value`, found at `key`, reduced to the settings a plugin may read. Objects
/// left empty are dropped, and `None` means nothing in it is readable. Keys in
/// the value may be kebab-case or camelCase.
pub fn plugin_view(key: &str, value: Value) -> Option<Value> {
    let path = if key.is_empty() {
        Vec::new()
    } else {
        segments(key)
    };
    filter(path, value)
}

fn filter(path: Vec<String>, value: Value) -> Option<Value> {
    match value {
        Value::Object(map) => {
            let kept: serde_json::Map<String, Value> = map
                .into_iter()
                .filter_map(|(name, child)| {
                    let mut next = path.clone();
                    next.push(naming::config_key_to_field(&name));
                    filter(next, child).map(|child| (name, child))
                })
                .collect();
            (!kept.is_empty()).then_some(Value::Object(kept))
        }
        leaf => PLUGIN_ACCESS
            .iter()
            .any(|(listed, access)| *access >= Read && segments(listed) == path)
            .then_some(leaf),
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
