use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::grant::Inspection;
use super::registry::{PluginInfo, PluginTarget};
use crate::change::Scope;
use crate::contract::{Contract, Empty};

/// A repository the user added and the key pinned for it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub id: String,
    pub url: String,
    /// A short name for the pinned key.
    pub signer: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginRepoAddParams {
    pub id: String,
    /// Where the signed `index.json` is; its signature is the same URL with
    /// `.minisig` added.
    pub url: String,
    /// The minisign public key every index of this repository must verify under.
    pub key: String,
}

pub struct PluginRepoList;
impl Contract for PluginRepoList {
    const CHANNEL: &'static str = "plugin.repo.list";
    type Params = Empty;
    type Result = Vec<RepositoryInfo>;
}

/// Pins the key and fetches the index once, so a wrong key or URL fails here.
pub struct PluginRepoAdd;
impl Contract for PluginRepoAdd {
    const CHANNEL: &'static str = "plugin.repo.add";
    const TIMEOUT: Duration = Duration::from_secs(60);
    type Params = PluginRepoAddParams;
    type Result = RepositoryInfo;
}

/// Forgets the repository. What it installed stays, but no longer updates.
pub struct PluginRepoRemove;
impl Contract for PluginRepoRemove {
    const CHANNEL: &'static str = "plugin.repo.remove";
    type Params = PluginTarget;
    type Result = Empty;
}

/// A plugin of a repository, newest unrevoked version unless one is named.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginReleaseParams {
    pub repository: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInstallReleaseParams {
    pub release: PluginReleaseParams,
    pub approval_digest: String,
}

/// Downloads the release, checks it against the signed index and derives its
/// grant; nothing is installed or run.
pub struct PluginInspectRelease;
impl Contract for PluginInspectRelease {
    const CHANNEL: &'static str = "plugin.release.inspect";
    const TIMEOUT: Duration = Duration::from_secs(5 * 60);
    type Params = PluginReleaseParams;
    type Result = Inspection;
}

/// Installs the release if it still derives the approved digest, then starts it.
pub struct PluginInstallRelease;
impl Contract for PluginInstallRelease {
    const CHANNEL: &'static str = "plugin.release.install";
    const TIMEOUT: Duration = Duration::from_secs(5 * 60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginInstallReleaseParams;
    type Result = PluginInfo;
}
