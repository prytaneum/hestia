use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::choice::Choice;
use super::registry::PluginInfo;
use crate::change::Scope;
use crate::contract::Contract;
use crate::warning::WarningInfo;

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
    /// Chosen by the user at approval, never written in the manifest.
    Entries,
}

impl std::fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ScopeKind::Hosts => "hosts",
            ScopeKind::Paths => "paths",
            ScopeKind::Endpoints => "endpoints",
            ScopeKind::Programs => "programs",
            ScopeKind::Entries => "entries",
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
    /// The manifest marks it optional: it arrives declined, and the user may
    /// grant or revoke it later.
    pub optional: bool,
    /// The user chooses which entries it reaches, at approval.
    pub entries: bool,
}

/// A schedule the plugin wakes itself with: it runs without anyone asking, so
/// the user approves it as they do a permission.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct GrantedSchedule {
    pub id: String,
    /// How often it ticks, as the manifest wrote it: `30m`, `6h` or `1d`.
    pub every: String,
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
    pub schedules: Vec<GrantedSchedule>,
}

/// Who vouches for a package from a repository: the repository that listed it
/// and the author whose key signed it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct SignerInfo {
    pub repository: String,
    /// SHA-256 of the author's public key, in hex.
    pub fingerprint: String,
    /// Whether the author's key is one of Hestia's own.
    pub official: bool,
}

/// A package read before anything of it runs.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Inspection {
    pub id: String,
    pub name: String,
    pub version: String,
    pub grant: Grant,
    /// What the package does that will stop working, such as importing a
    /// deprecated interface.
    pub warnings: Vec<WarningInfo>,
    /// Absent for a package read from a directory, which nothing signs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<SignerInfo>,
    /// Covers the package, the grant and the signer; `plugin.install` carries it back.
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
    pub choice: Choice,
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
