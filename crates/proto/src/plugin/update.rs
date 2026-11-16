use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::grant::Inspection;
use super::registry::{PluginInfo, PluginTarget};
use crate::change::Scope;
use crate::contract::{Contract, Empty};

/// A newer version a repository's signed index lists for an installed plugin.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginUpdate {
    pub id: String,
    pub repository: String,
    pub installed: String,
    pub available: String,
}

/// A repository whose index could not be trusted or reached.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryFailure {
    pub repository: String,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginUpdatesResult {
    pub updates: Vec<PluginUpdate>,
    pub failed: Vec<RepositoryFailure>,
}

/// Fetches every repository's index and lists the newer versions of what it
/// installed. A revoked installed version is disabled on the way.
pub struct PluginUpdates;
impl Contract for PluginUpdates {
    const CHANNEL: &'static str = "plugin.updates";
    const TIMEOUT: Duration = Duration::from_secs(5 * 60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = Empty;
    type Result = PluginUpdatesResult;
}

/// What an update would install. `widened` says it asks for more than the user
/// approved: a new permission, a scope entry or a required package. Such an
/// update is never applied without the digest of what was shown.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateInspection {
    pub installed: String,
    pub inspection: Inspection,
    pub widened: bool,
}

pub struct PluginUpdateInspect;
impl Contract for PluginUpdateInspect {
    const CHANNEL: &'static str = "plugin.update.inspect";
    const TIMEOUT: Duration = Duration::from_secs(5 * 60);
    type Params = PluginTarget;
    type Result = UpdateInspection;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginUpdateApplyParams {
    pub id: String,
    pub approval_digest: String,
}

/// Replaces the installed version with the update if it still derives the
/// approved digest. A version that fails to load is rolled back.
pub struct PluginUpdateApply;
impl Contract for PluginUpdateApply {
    const CHANNEL: &'static str = "plugin.update";
    const TIMEOUT: Duration = Duration::from_secs(5 * 60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginUpdateApplyParams;
    type Result = PluginInfo;
}
