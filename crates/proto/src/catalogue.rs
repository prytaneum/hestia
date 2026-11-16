//! Plugin repositories as users see them: the stores they add, and the one
//! catalogue of what those stores offer. The index a repository publishes is
//! [`crate::repository`]; trust follows its key.

use serde::{Deserialize, Serialize};

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
    pub permissions: Vec<String>,
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
