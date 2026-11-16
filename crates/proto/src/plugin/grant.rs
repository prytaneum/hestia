use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::registry::PluginInfo;
use crate::change::Scope;
use crate::contract::Contract;

/// What a permission reaches. `Unrestricted` runs native code or deletes data
/// and is stated as such in the prompt.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    #[default]
    Sandboxed,
    Unrestricted,
}

/// What the manifest narrows a permission to.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    #[default]
    Hosts,
    Paths,
    Endpoints,
    Programs,
}

impl std::fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ScopeKind::Hosts => "hosts",
            ScopeKind::Paths => "paths",
            ScopeKind::Endpoints => "endpoints",
            ScopeKind::Programs => "programs",
        })
    }
}

/// A program the user approved: the file at `path`, as its digest was at
/// approval, and the exact arguments it may run with.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Program {
    pub path: String,
    pub args: Vec<String>,
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PermissionScope {
    pub kind: ScopeKind,
    pub values: Vec<String>,
    /// For `hosts`: those that may serve a body nothing verifies.
    pub unhashed: Vec<String>,
    /// For `programs`: each invocation approved for this platform.
    pub programs: Vec<Program>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct GrantedPermission {
    pub permission: String,
    pub tier: Tier,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<PermissionScope>,
}

/// What the user approves, derived from the component and the manifest.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Grant {
    pub permissions: Vec<GrantedPermission>,
    /// The interfaces the component exports.
    pub contributes: Vec<String>,
    /// The packages other than the kernel's that the component imports.
    pub requires: Vec<String>,
}

/// A package read before anything of it runs.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Inspection {
    pub id: String,
    pub name: String,
    pub version: String,
    pub grant: Grant,
    /// Covers the package and the grant; `plugin.install` carries it back.
    pub approval_digest: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginPackage {
    /// A directory holding `plugin.toml` and `plugin.wasm`.
    pub path: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInstallParams {
    pub path: String,
    pub approval_digest: String,
}

/// Reads the package and derives its grant; nothing is installed or run.
pub struct PluginInspect;
impl Contract for PluginInspect {
    const CHANNEL: &'static str = "plugin.inspect";
    const TIMEOUT: Duration = Duration::from_secs(60);
    type Params = PluginPackage;
    type Result = Inspection;
}

/// Installs the package if it still derives the approved digest, then starts it.
pub struct PluginInstall;
impl Contract for PluginInstall {
    const CHANNEL: &'static str = "plugin.install";
    const TIMEOUT: Duration = Duration::from_secs(60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginInstallParams;
    type Result = PluginInfo;
}
