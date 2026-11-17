use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    #[default]
    Builtin,
    Wasm,
}

/// How a plugin got here: with Hestia, from a directory in developer mode, or
/// from a repository whose signed index vouched for it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    #[default]
    Builtin,
    Dev,
    Repository,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    #[default]
    Enabled,
    Disabled,
    Failed,
    /// A plugin it declares in `[dependencies]` is not installed at a version it accepts.
    MissingDependency,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub kind: PluginKind,
    pub source: PluginSource,
    pub version: String,
    pub state: PluginState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provides: Option<String>,
    /// The ids of the plugins it needs. For a WASM plugin, the ones it declares
    /// in `[dependencies]` and the built-ins that provide the packages it
    /// imports.
    pub requires: Vec<String>,
    pub commands: Vec<super::CommandSpec>,
    /// The optional permissions the user granted.
    pub optional: Vec<String>,
    /// The entries it may reach; absent unless it holds a permission that takes
    /// them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<super::EntryScope>,
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
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginTarget;
    type Result = PluginInfo;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginDisableParams {
    pub id: String,
    /// Also disable every enabled plugin that needs this one. Without it, the
    /// call is refused with `plugin_required` naming them.
    pub cascade: bool,
}

/// Refused while an enabled plugin requires this one, naming all of them,
/// unless `cascade` disables them first; refused too while the plugin has work
/// running. Only hides and unlinks, so its data on disk is untouched.
pub struct PluginDisable;
impl Contract for PluginDisable {
    const CHANNEL: &'static str = "plugin.disable";
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginDisableParams;
    type Result = PluginInfo;
}

/// Deletes an installed plugin and its compiled cache entry. A built-in cannot
/// be removed, nor a plugin another installed plugin depends on.
pub struct PluginRemove;
impl Contract for PluginRemove {
    const CHANNEL: &'static str = "plugin.remove";
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginTarget;
    type Result = Empty;
}

/// A secret the user entered for a plugin. Deliberately not `Debug`, so the
/// value cannot reach a log line through a formatted payload.
#[derive(Serialize, Deserialize, Default, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginSecretParams {
    pub id: String,
    pub key: String,
    pub value: String,
}

/// Stores a secret for a WASM plugin that asks for the `secrets` permission.
pub struct PluginSecretSet;
impl Contract for PluginSecretSet {
    const CHANNEL: &'static str = "plugin.secret.set";
    type Params = PluginSecretParams;
    type Result = Empty;
}
