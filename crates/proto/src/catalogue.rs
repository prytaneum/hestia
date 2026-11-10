//! Plugin repositories as users see them: the stores they add, and the one
//! catalogue of what those stores offer. The index a repository publishes is
//! [`crate::repository`]; trust follows its key
//! ([0097](../../docs/decisions/0097-trust-follows-the-repository-key.md)).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};
use crate::plugin::Permission;
use crate::warning::WarningInfo;

/// A configured repository, as last fetched.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryInfo {
    /// The index URL, which identifies the repository.
    pub url: String,
    pub name: String,
    /// The store badge, up to 4 characters.
    pub label: String,
    pub website: String,
    /// The pinned minisign key's id. The key, never the name, is what is trusted.
    pub key_id: String,
    /// The repository Hestia ships with; it cannot be removed.
    pub official: bool,
    pub plugins: u32,
    /// The index version last accepted.
    pub version: u64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryAddParams {
    /// An index URL, `github.com/owner/repo` or `owner/repo`.
    pub url: String,
    /// Save the repository. Without it the call only reports what would be
    /// trusted, so a front-end can show the key id first.
    pub confirm: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryAdded {
    pub repository: RepositoryInfo,
    /// It was configured before this call.
    pub configured: bool,
    /// This call wrote it.
    pub saved: bool,
}

/// Fetches and verifies the index; saves it only with `confirm`.
pub struct PluginRepoAdd;
impl Contract for PluginRepoAdd {
    const CHANNEL: &'static str = "plugin.repo.add";
    const TIMEOUT: Duration = Duration::from_secs(60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = RepositoryAddParams;
    type Result = RepositoryAdded;
}

/// Reads the last good copies; no network.
pub struct PluginRepoList;
impl Contract for PluginRepoList {
    const CHANNEL: &'static str = "plugin.repo.list";
    type Params = Empty;
    type Result = Vec<RepositoryInfo>;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryRef {
    pub url: String,
}

/// Plugins installed from the repository keep running, as obsolete.
pub struct PluginRepoRemove;
impl Contract for PluginRepoRemove {
    const CHANNEL: &'static str = "plugin.repo.remove";
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = RepositoryRef;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryRefreshParams {
    /// One repository; every configured one when absent.
    pub url: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryRefreshed {
    pub repositories: Vec<RepositoryInfo>,
    /// A repository that could not be refreshed keeps its cached index.
    pub warnings: Vec<WarningInfo>,
}

/// An unreachable repository is a warning and keeps its cached index. For one
/// named repository, a refused index (another key, a bad signature, an older
/// version) is an error; refreshing all reports it as a warning instead.
pub struct PluginRepoRefresh;
impl Contract for PluginRepoRefresh {
    const CHANNEL: &'static str = "plugin.repo.refresh";
    const TIMEOUT: Duration = Duration::from_secs(2 * 60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = RepositoryRefreshParams;
    type Result = RepositoryRefreshed;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// A build of the latest version matches this Hestia's API.
    #[default]
    Prebuilt,
    /// The latest version has no build yet.
    Unreleased,
    /// The latest version's builds need another API than this Hestia's.
    Incompatible,
}

/// A repository that lists a plugin.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Lister {
    pub url: String,
    pub label: String,
    pub name: String,
}

/// One plugin as the configured repositories offer it. Repositories pinning the
/// same key serve the same builds, so they are one entry naming every lister;
/// the same id under another key is another entry.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CataloguePlugin {
    pub id: String,
    pub name: String,
    pub description: String,
    /// An https image URL the image proxy can load.
    pub icon: Option<String>,
    /// The latest listed version.
    pub version: String,
    pub permissions: Vec<Permission>,
    /// The signing key's id.
    pub key_id: String,
    pub listed_by: Vec<Lister>,
    pub availability: Availability,
    /// The newest build this Hestia can install, if any. Older than `version`
    /// when the latest is unreleased or incompatible.
    pub build: Option<String>,
    /// For an incompatible plugin, the API requirement of its latest build.
    pub requires: Option<String>,
    /// The installed version, when it was installed from this key.
    pub installed: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogueParams {
    /// Only this repository's plugins.
    pub repo: Option<String>,
    /// Matches id, name and description, ignoring case.
    pub query: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Catalogue {
    pub plugins: Vec<CataloguePlugin>,
    pub repositories: Vec<RepositoryInfo>,
    pub warnings: Vec<WarningInfo>,
}

/// Refreshes every repository the filter names, then falls back to the cached
/// copy of any that cannot be reached, so browsing works offline.
pub struct PluginCatalogue;
impl Contract for PluginCatalogue {
    const CHANNEL: &'static str = "plugin.catalogue";
    const TIMEOUT: Duration = Duration::from_secs(2 * 60);
    type Params = CatalogueParams;
    type Result = Catalogue;
}
