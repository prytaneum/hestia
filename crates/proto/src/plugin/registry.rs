use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    #[default]
    Builtin,
    Wasm,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    #[default]
    Enabled,
    Disabled,
    Failed,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub kind: PluginKind,
    pub version: String,
    pub state: PluginState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provides: Option<String>,
    pub requires: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginTarget {
    pub id: String,
}

pub struct PluginList;
impl Contract for PluginList {
    const CHANNEL: &'static str = "plugin.list";
    type Params = Empty;
    type Result = Vec<PluginInfo>;
}

/// Enables the plugin after everything it requires.
pub struct PluginEnable;
impl Contract for PluginEnable {
    const CHANNEL: &'static str = "plugin.enable";
    type Params = PluginTarget;
    type Result = PluginInfo;
}

/// Refused while an enabled plugin requires this one or the plugin has work
/// running; only hides and unlinks, so its data on disk is untouched.
pub struct PluginDisable;
impl Contract for PluginDisable {
    const CHANNEL: &'static str = "plugin.disable";
    type Params = PluginTarget;
    type Result = PluginInfo;
}
