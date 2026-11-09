//! The index a plugin repository publishes: what is in it, which builds are
//! installable and which versions are revoked. See `docs/plugins/repository.md`.

use serde::{Deserialize, Serialize};

use crate::plugin::Permission;

pub const INDEX_SCHEMA_VERSION: u32 = 1;
pub const INDEX_FILE: &str = "plugins.json";

/// How long an index stays fresh after it is signed.
pub const INDEX_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryIndex {
    pub schema_version: u32,
    /// The signing time in unix seconds: monotonic without stored state, so a
    /// client can refuse a rollback.
    pub version: u64,
    /// Unix seconds after which the index is stale: `version` plus
    /// [`INDEX_LIFETIME_SECONDS`]. The signed trusted comment repeats it.
    pub expires: i64,
    /// Where the source lives; with `commit`, it traces a listing to its source.
    pub repository: String,
    pub commit: String,
    pub name: String,
    /// The store badge, up to 4 characters.
    pub label: String,
    pub website: String,
    /// The minisign public key the repository signs its index and packages with.
    pub key: String,
    /// An optional spare key, pinned with `key`; an index signed by it verifies,
    /// which is how a repository rotates without users re-adding it.
    pub next: String,
    pub plugins: Vec<IndexedPlugin>,
    pub revoked: Vec<Revocation>,
}

/// One plugin as the default branch declares it. Without a build of
/// `version` it is listed but not installable.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct IndexedPlugin {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Repository-relative path of the icon.
    pub icon: Option<String>,
    /// Repository-relative path of the plugin's folder.
    pub path: String,
    pub version: String,
    pub permissions: Vec<Permission>,
    pub commit: String,
    pub builds: Vec<Build>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Build {
    pub version: String,
    /// The caret requirement `pack` wrote; a theme-only package has none.
    pub api: Option<String>,
    /// The package's path relative to the index URL, so a mirror or another
    /// host serves the same tree.
    pub file: String,
    /// Hex sha256 of the package.
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Revocation {
    pub id: String,
    pub versions: Vec<String>,
    pub reason: String,
}

/// The trusted comment a build's signature must carry. Signer and verifier
/// both compare it as a whole string, so a signature binds to one plugin,
/// version and file name.
pub fn build_comment(id: &str, version: &str, file: &str) -> String {
    format!("hestia-build id={id} version={version} file={file}")
}

/// The trusted comment an index's signature must carry; the version makes an
/// older signed index detectable as a rollback and `expires` makes a frozen one
/// detectable as stale.
pub fn index_comment(version: u64, expires: i64) -> String {
    format!("hestia-index version={version} expires={expires}")
}
