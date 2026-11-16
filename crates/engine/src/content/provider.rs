//! The content-provider seam. A source platform implements the trait — listing,
//! searching, and resolving projects and their versions — and the `Content`
//! aggregate holds a registry of each. A platform is a plugin's content source.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use kernel::vetting::Policy;
use proto::content::{
    ContentKind, ContentProject, ContentVersion, RequiredSetting, ResolvedModpack, SearchQuery,
    SourceFileRef, VersionQuery,
};

use serde_json::Value;

use super::install;

/// A project reference recognised in a platform's own site URL, optionally
/// pinned to one version. `kind` is what the URL's own path says the project is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlRef {
    pub project: String,
    pub version: Option<String>,
    pub kind: Option<ContentKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    pub path: PathBuf,
    pub sha1: String,
}

impl LocalFile {
    pub fn read(path: &Path) -> Result<Self> {
        Ok(LocalFile {
            path: path.to_path_buf(),
            sha1: install::sha1_file(path)?,
        })
    }
}

/// One installed file a bulk update lookup asks about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstalledFile {
    pub project_id: String,
    pub sha1: String,
}

/// What one bulk update lookup covers: files sharing a game version and the
/// loaders their kind installs for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LatestQuery {
    pub game_version: String,
    pub loaders: Vec<String>,
    pub files: Vec<InstalledFile>,
}

#[async_trait]
pub trait ContentProvider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    /// What this platform catalogues; a front-end offers it for those alone.
    fn kinds(&self) -> Vec<ContentKind>;
    /// The game editions its catalogue is for.
    fn editions(&self) -> Vec<proto::minecraft::Edition> {
        vec![proto::minecraft::Edition::Java]
    }
    /// Whether it can serve as configured — a required plugin setting is set.
    fn available(&self) -> bool {
        true
    }
    /// The setting whose being unset is why [`ContentProvider::available`] is
    /// false, for a front-end to say and link; `None` for any other reason.
    fn missing_setting(&self) -> Option<RequiredSetting> {
        None
    }
    /// Recognise a project/version page URL on this platform's site.
    async fn parse_url(&self, url: &str) -> Option<UrlRef>;
    /// Recognise the project and version behind the URL of a file this
    /// platform serves, so a pack's file is tracked to its source.
    async fn recognise_file_url(&self, _url: &str) -> Option<SourceFileRef> {
        None
    }
    /// The policy the files this source names are downloaded under: the hosts
    /// the source states.
    fn download_policy(&self) -> Arc<Policy>;
    /// Whether a datapack is catalogued as a `Mod` project here.
    fn datapacks_as_mods(&self) -> bool {
        false
    }
    async fn search(&self, query: &SearchQuery) -> Result<proto::content::SearchResult>;
    /// A project's detail, stamped with the caller's `kind` when it publishes it.
    async fn project(&self, project: &str, kind: Option<ContentKind>) -> Result<ContentProject>;
    /// Several projects at once. A pack index resolves to a hundred-odd project
    /// ids whose titles and icons are all wanted at the same moment, and a
    /// platform that rate-limits will not survive that many single lookups —
    /// so a source that has a bulk endpoint overrides this.
    async fn projects(&self, ids: &[String]) -> Result<Vec<ContentProject>> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(self.project(id, None).await?);
        }
        Ok(out)
    }
    async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>>;
    /// Several versions by id. The bulk twin of [`ContentProvider::projects`],
    /// and wanted for the same reason: a pack index identifies its files by
    /// version id, and without their *numbers* the pool lists a hundred mods
    /// with no version against their name.
    async fn versions_by_id(&self, ids: &[String]) -> Result<Vec<ContentVersion>> {
        let _ = ids;
        Ok(Vec::new())
    }
    /// The newest compatible version id of each file's project — the newest
    /// release, else the newest of any channel — keyed by project id, in as
    /// few requests as the platform allows. A project missing from the map has
    /// no compatible version at all. `None` means the source has no bulk
    /// lookup, and its items resolve one at a time through `versions`.
    async fn latest(&self, query: &LatestQuery) -> Result<Option<HashMap<String, String>>> {
        let _ = query;
        Ok(None)
    }
    /// The version each file is a release of, keyed by the file's sha1.
    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        let _ = files;
        Ok(HashMap::new())
    }

    /// Leaves the pack's own project and version ids empty: a manifest carries neither.
    async fn resolve_manifest(&self, _manifest: &Value) -> Result<ResolvedModpack> {
        bail!("{} has no pack manifest of its own", self.name())
    }
}
