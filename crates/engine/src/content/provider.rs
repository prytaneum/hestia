//! The content-provider seam. A source platform implements the trait — listing,
//! searching, and resolving projects and their versions — and the `Content`
//! aggregate holds a boxed registry of each. Adding a platform is a new impl plus
//! one line in `Content::new`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use async_trait::async_trait;
use proto::content::{
    ContentKind, ContentProject, ContentVersion, ResolvedModpack, SearchQuery, VersionQuery,
};

use serde_json::Value;

use super::install;
use crate::config::ContentSettings;

/// A project reference recognised in a platform's own site URL, optionally
/// pinned to one version. `kind` is what the URL's own path says the project is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlRef {
    pub project: String,
    pub version: Option<String>,
    pub kind: Option<ContentKind>,
}

/// What a platform's download URL says about the file behind it. A pack index
/// names its files by URL and hash alone, so this is what makes each one an
/// ordinary tracked pool item rather than an anonymous jar — and it costs
/// nothing, since the ids are already in the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub project_id: String,
    pub version_id: String,
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

#[async_trait]
pub trait ContentProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// What this platform catalogues; a front-end offers it for those alone.
    fn kinds(&self) -> Vec<ContentKind>;
    /// Whether it can serve as configured — an unset API key, today.
    fn available(&self) -> bool {
        true
    }
    /// Called at startup and after every `config set`, so a key takes effect on
    /// the running daemon.
    fn configure(&self, _settings: &ContentSettings) {}
    /// Recognise a project/version page URL on this platform's site.
    fn parse_url(&self, url: &str) -> Option<UrlRef>;
    /// Recognise one of this platform's own file download URLs, when it carries
    /// the ids. `None` for a URL the platform does not serve.
    fn parse_file_url(&self, _url: &str) -> Option<FileRef> {
        None
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
    /// The version each file is a release of, keyed by the file's sha1.
    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        let _ = files;
        Ok(HashMap::new())
    }

    /// Leaves the pack's own project and version ids empty: a manifest carries neither.
    async fn resolve_manifest(&self, _manifest: &Value) -> Result<ResolvedModpack> {
        bail!("{} has no pack manifest of its own", self.name())
    }
    /// A modpack version's manifest, without its archive.
    async fn resolve_modpack(&self, version_id: &str) -> Result<ResolvedModpack>;
    /// The modpack version's whole archive. Installing needs the bytes as well
    /// as the manifest — a pack's `overrides/` live only inside the zip — so the
    /// install path fetches once through this rather than resolving and then
    /// downloading the same file again.
    async fn fetch_modpack(&self, version_id: &str) -> Result<(ResolvedModpack, Vec<u8>)>;
}
