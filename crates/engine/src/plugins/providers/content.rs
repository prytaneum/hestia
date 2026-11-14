//! Content sources contributed by plugins, wrapped in a
//! [`PluginContentProvider`] that the content aggregate treats like any
//! built-in source. The guest only describes what exists; the engine downloads
//! the files it lists, and only from hosts the plugin was granted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use proto::content::{
    ContentKind, ContentProject, ContentVersion, ModpackFile, RequiredSetting, ResolvedModpack,
    SearchQuery, SearchResult, SourceFileRef, SourceInfo, VersionQuery,
};
use proto::error::ArchiveFault;
use proto::minecraft::Artifact;
use proto::plugin::Manifest;
use proto::provider::content::{
    self as op, IdQuery, IdentifyFile, IdentifyQuery, PackManifestQuery, ProjectQuery, Source,
    UrlQuery,
};
use proto::Empty;
use serde_json::Value;

use super::{connect_each, registered, Guest};
use crate::content::pack;
use crate::content::provider::{ContentProvider, LocalFile, UrlRef};
use crate::engine::Engine;
use crate::plugins::context::{CallContext, Mount};
use crate::plugins::runtime::LoadedPlugin;
use crate::plugins::vouch::Policy;

/// Where the files being identified appear to the guest, for that call only.
const IDENTIFY_MOUNT: &str = "/files";

/// Hashing every mod of a pack is guest compute, not a lookup.
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(120);

/// A source a plugin contributes, registered as `<plugin>.<id>`, or bare when official.
pub struct PluginContentProvider {
    guest: Guest,
    info: SourceInfo,
    policy: Arc<Policy>,
}

impl PluginContentProvider {
    /// Asks the guest to describe its source. A source that waits on a setting
    /// the plugin does not declare could never serve, so it is refused.
    async fn connect(guest: Guest) -> Result<Self> {
        let info = guest.ask::<op::Info>(&Empty {}).await?;
        if let Some(key) = &info.requires_setting {
            if !guest.plugin().settings().declares(key) {
                return Err(guest.failed(format!("it requires the undeclared setting `{key}`")));
            }
        }
        let policy = Arc::new(Policy::new(guest.plugin().grant()));
        Ok(PluginContentProvider {
            guest,
            info,
            policy,
        })
    }

    fn project_of(&self, mut project: ContentProject) -> ContentProject {
        project.source = self.guest.id().to_string();
        project
    }

    /// Stamps the version as this source's and keeps only the files the engine
    /// may download for the plugin; a version left with none is dropped.
    fn version_of(&self, mut version: ContentVersion) -> Option<ContentVersion> {
        version.source = self.guest.id().to_string();
        let listed = version.files.len();
        version
            .files
            .retain(|file| self.downloadable(&file.artifact));
        if version.files.len() < listed {
            tracing::warn!(
                source = %self.guest.id(),
                version = %version.id,
                dropped = listed - version.files.len(),
                "files outside the plugin's network hosts were dropped"
            );
        }
        (listed == 0 || !version.files.is_empty()).then_some(version)
    }

    fn downloadable(&self, artifact: &Artifact) -> bool {
        self.policy.vet_artifact(artifact).is_ok()
    }

    /// A pack file's path stays inside the entry; its URL is empty when its
    /// author withholds it, which the download reports by name, and otherwise
    /// held to the same rule as any file the source names.
    fn vet_pack_file(&self, file: &mut ModpackFile) -> Result<()> {
        if !pack::is_safe_path(&file.path) {
            tracing::warn!(source = %self.guest.id(), path = %file.path, "a pack file escapes its directory");
            return Err(pack::invalid(ArchiveFault::UnsafePath));
        }
        if !file.artifact.url.is_empty() {
            self.policy.vet_artifact(&file.artifact).map_err(|refused| {
                tracing::warn!(source = %self.guest.id(), path = %file.path, %refused, "a pack file was refused");
                self.guest
                    .failed(format!("the pack file `{}`: {refused}", file.path))
            })?;
        }
        file.source = self.guest.id().to_string();
        if let Some(project) = file.project.take() {
            file.project = Some(self.project_of(project));
        }
        Ok(())
    }

    fn versions_of(&self, versions: Vec<ContentVersion>) -> Vec<ContentVersion> {
        versions
            .into_iter()
            .filter_map(|version| self.version_of(version))
            .collect()
    }
}

#[async_trait]
impl ContentProvider for PluginContentProvider {
    fn id(&self) -> &str {
        self.guest.id()
    }

    fn name(&self) -> &str {
        &self.info.name
    }

    fn kinds(&self) -> Vec<ContentKind> {
        self.info.kinds.clone()
    }

    fn available(&self) -> bool {
        !self.guest.plugin().is_disabled() && self.missing_setting().is_none()
    }

    fn missing_setting(&self) -> Option<RequiredSetting> {
        let key = self.info.requires_setting.as_ref()?;
        let plugin = self.guest.plugin();
        (!plugin.is_disabled() && !plugin.settings().is_filled(key)).then(|| RequiredSetting {
            plugin: Some(plugin.id().to_string()),
            key: key.clone(),
        })
    }

    async fn parse_url(&self, url: &str) -> Option<UrlRef> {
        let found = self
            .guest
            .try_ask::<op::ParseUrl>(&UrlQuery { url: url.into() })
            .await
            .inspect_err(|error| {
                tracing::warn!(source = %self.guest.id(), error = %format!("{error:#}"), "content source could not parse a URL");
            })
            .ok()???;
        Some(UrlRef {
            project: found.project,
            version: found.version,
            kind: found.kind,
        })
    }

    async fn recognise_file_url(&self, url: &str) -> Option<SourceFileRef> {
        if !self.policy.permits(url, false) {
            return None;
        }
        self.guest
            .try_ask::<op::RecogniseFileUrl>(&UrlQuery { url: url.into() })
            .await
            .inspect_err(|error| {
                tracing::warn!(source = %self.guest.id(), error = %format!("{error:#}"), "content source could not recognise a file URL");
            })
            .ok()?
            .flatten()
    }

    fn download_policy(&self) -> Option<Arc<Policy>> {
        Some(self.policy.clone())
    }

    fn datapacks_as_mods(&self) -> bool {
        self.info.datapacks_as_mods
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let mut result = self.guest.ask::<op::Search>(query).await?;
        result.hits = result
            .hits
            .into_iter()
            .map(|hit| self.project_of(hit))
            .collect();
        Ok(result)
    }

    async fn project(&self, project: &str, kind: Option<ContentKind>) -> Result<ContentProject> {
        let query = ProjectQuery {
            project: project.into(),
            kind,
        };
        let found = self.guest.ask::<op::Project>(&query).await?;
        Ok(self.project_of(found))
    }

    /// A guest without `projects` is asked for each project in turn.
    async fn projects(&self, ids: &[String]) -> Result<Vec<ContentProject>> {
        let query = IdQuery { ids: ids.to_vec() };
        let found = match self.guest.try_ask::<op::Projects>(&query).await? {
            Some(found) => found,
            None => {
                let mut found = Vec::with_capacity(ids.len());
                for id in ids {
                    found.push(self.project(id, None).await?);
                }
                return Ok(found);
            }
        };
        Ok(found
            .into_iter()
            .map(|project| self.project_of(project))
            .collect())
    }

    async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>> {
        let found = self.guest.ask::<op::Versions>(query).await?;
        Ok(self.versions_of(found))
    }

    async fn versions_by_id(&self, ids: &[String]) -> Result<Vec<ContentVersion>> {
        let query = IdQuery { ids: ids.to_vec() };
        let found = self
            .guest
            .try_ask::<op::VersionsById>(&query)
            .await?
            .unwrap_or_default();
        Ok(self.versions_of(found))
    }

    /// Only the files being identified are lent to the guest: staged as links
    /// (copies across filesystems) in a directory of their own, mounted
    /// read-only for this call, and removed after it.
    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        if files.is_empty() {
            return Ok(HashMap::new());
        }
        let scratch = self.guest.plugin().scratch_dir().to_path_buf();
        let paths: Vec<_> = files.iter().map(|file| file.path.clone()).collect();
        let staged = tokio::task::spawn_blocking(move || stage(&scratch, &paths))
            .await
            .context("staging the files panicked")??;
        let query = IdentifyQuery {
            files: files
                .iter()
                .enumerate()
                .map(|(index, file)| IdentifyFile {
                    sha1: file.sha1.clone(),
                    path: format!("{IDENTIFY_MOUNT}/{index}"),
                })
                .collect(),
        };
        let context = CallContext {
            mounts: vec![Mount {
                host: staged.path().to_path_buf(),
                guest: IDENTIFY_MOUNT,
                writable: false,
            }],
            ..CallContext::default()
        };
        let found = self
            .guest
            .try_ask_within::<op::Identify>(&query, IDENTIFY_TIMEOUT, context)
            .await?
            .unwrap_or_default();
        Ok(found
            .into_iter()
            .filter_map(|(sha1, version)| Some((sha1, self.version_of(version)?)))
            .collect())
    }

    async fn resolve_manifest(&self, manifest: &Value) -> Result<ResolvedModpack> {
        let query = PackManifestQuery {
            manifest: manifest.clone(),
        };
        let mut pack = self
            .guest
            .try_ask::<op::ResolveManifest>(&query)
            .await?
            .with_context(|| format!("{} has no pack manifest of its own", self.info.name))?;
        pack.source = self.guest.id().to_string();
        for file in &mut pack.files {
            self.vet_pack_file(file)?;
        }
        Ok(pack)
    }
}

/// A staging directory that is removed when it goes out of scope.
struct Staged(PathBuf);

impl Staged {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            tracing::warn!(dir = %self.0.display(), %error, "cannot remove an identify staging directory");
        }
    }
}

/// Links `paths` into a fresh directory under `scratch`, as `0`, `1`, … in the
/// order given; a file that cannot be linked is copied.
fn stage(scratch: &Path, paths: &[PathBuf]) -> Result<Staged> {
    static STAGINGS: AtomicU64 = AtomicU64::new(0);
    let dir = scratch.join(format!(
        ".identify-{}-{}",
        std::process::id(),
        STAGINGS.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).context("cannot create a staging directory")?;
    let staged = Staged(dir);
    for (index, path) in paths.iter().enumerate() {
        let target = staged.path().join(index.to_string());
        if std::fs::hard_link(path, &target).is_err() {
            std::fs::copy(path, &target)
                .with_context(|| format!("cannot stage {}", path.display()))?;
        }
    }
    Ok(staged)
}

pub(super) async fn connect(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
    official: bool,
) -> Vec<PluginContentProvider> {
    connect_each::<Source, _, _, _>(plugin, manifest, official, PluginContentProvider::connect)
        .await
}

pub(super) fn register(
    engine: &Engine,
    owner: &str,
    official: bool,
    sources: Vec<PluginContentProvider>,
) {
    for source in sources {
        let id = source.guest.id().to_string();
        let live = source.guest.live();
        let outcome = engine
            .content()
            .register(owner, official, live, Arc::new(source));
        registered(owner, "content", &id, outcome);
    }
}
