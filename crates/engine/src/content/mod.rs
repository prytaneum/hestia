//! Third-party content provider aggregate: the source registry (Modrinth and
//! CurseForge behind one trait) and the search/project/versions/modpack entry
//! points over it. Stateless — every result is fetched upstream — so it needs no
//! data directory, exactly like the `minecraft` aggregate. The only state is the
//! per-source configuration `configure()` hands down, since CurseForge serves
//! nothing without an API key.

pub(crate) mod curseforge;
pub(crate) mod exclude;
pub(crate) mod icon;
pub(crate) mod inspect;
pub(crate) mod install;
mod json;
pub(crate) mod modpack;
mod modrinth;
pub(crate) mod pack;
pub(crate) mod pool;
pub(crate) mod provider;
pub(crate) mod record;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::{Context, Result};
use proto::content::{
    ContentFile, ContentInspectResult, ContentKind, ContentMatch, ContentProject, ContentSource,
    ContentVersion, ResolvedModpack, ResolvedUrl, SearchQuery, SearchResult, VersionQuery,
};

use crate::config::ContentSettings;
use provider::ContentProvider;
pub(crate) use provider::{LocalFile, UrlRef};

/// Reading a content document is what quarantines a malformed or too-new one, so
/// this is a read of each — called after an import lands a tree written by
/// another build, rather than waiting for the first thing that happens to ask.
pub(crate) fn validate(entry_dir: &Path) {
    install::load(entry_dir);
    modpack::load(entry_dir);
}

#[derive(Debug, Clone)]
pub struct Identified {
    pub project: ContentProject,
    pub version: ContentVersion,
    pub file: ContentFile,
}

impl Identified {
    pub(crate) fn recorded(
        &self,
        kind: ContentKind,
        filename: String,
        sha1: String,
    ) -> (record::Project, record::Release) {
        (
            record::Project {
                kind,
                project_id: self.project.id.clone(),
                slug: self.project.slug.clone(),
                title: self.project.title.clone(),
                icon_url: self.project.icon_url.clone(),
            },
            record::Release {
                source: self.version.source.clone(),
                version_id: self.version.id.clone(),
                version_number: self.version.version_number.clone(),
                filename,
                sha1,
                url: self.file.artifact.url.clone(),
            },
        )
    }
}

impl From<Identified> for ContentMatch {
    fn from(identified: Identified) -> Self {
        ContentMatch {
            project: identified.project,
            version_number: identified.version.version_number,
        }
    }
}

fn published_file(version: &ContentVersion, sha1: &str) -> Option<ContentFile> {
    version
        .files
        .iter()
        .find(|f| {
            f.artifact
                .checksum
                .as_ref()
                .is_some_and(|c| c.hex.eq_ignore_ascii_case(sha1))
        })
        .or_else(|| install::primary_file(version).ok())
        .cloned()
}

async fn identify_with(
    provider: &dyn ContentProvider,
    files: &[LocalFile],
) -> Result<HashMap<String, Identified>> {
    let versions = provider.identify(files).await?;
    if versions.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<String> = versions
        .values()
        .map(|v| v.project_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let projects: HashMap<String, ContentProject> = provider
        .projects(&ids)
        .await?
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    Ok(versions
        .into_iter()
        .filter_map(|(sha1, version)| {
            let project = projects.get(&version.project_id)?.clone();
            let file = published_file(&version, &sha1)?;
            Some((
                sha1,
                Identified {
                    project,
                    version,
                    file,
                },
            ))
        })
        .collect())
}

pub struct Content {
    providers: Vec<Box<dyn ContentProvider>>,
}

impl Default for Content {
    fn default() -> Self {
        Content {
            providers: vec![
                Box::new(modrinth::Modrinth),
                Box::new(curseforge::CurseForge::default()),
            ],
        }
    }
}

impl Content {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build over a given registry rather than the shipped one. The seam a test
    /// crosses to install content without reaching a platform.
    pub fn with_providers(providers: Vec<Box<dyn ContentProvider>>) -> Self {
        Content { providers }
    }

    /// Hand each source the settings it needs. Called at startup and after
    /// every `config set`, so a key takes effect on the running daemon.
    pub fn configure(&self, settings: &ContentSettings) {
        for provider in &self.providers {
            provider.configure(settings);
        }
    }

    /// A failed source is logged, not returned: a lookup must not stop an import.
    pub async fn identify(&self, files: &[LocalFile]) -> HashMap<String, Vec<Identified>> {
        let mut found: HashMap<String, Vec<Identified>> = HashMap::new();
        if files.is_empty() {
            return found;
        }
        for provider in self.providers.iter().filter(|p| p.available()) {
            match identify_with(provider.as_ref(), files).await {
                Ok(matched) => {
                    for (sha1, identified) in matched {
                        found.entry(sha1).or_default().push(identified);
                    }
                }
                Err(error) => tracing::warn!(
                    source = provider.id(),
                    files = files.len(),
                    error = %format!("{error:#}"),
                    "cannot identify local files on this source"
                ),
            }
        }
        tracing::info!(
            files = files.len(),
            identified = found.len(),
            "local file lookup"
        );
        found
    }

    /// Classify a local file for import (the daemon is the only side that can
    /// read a client-picked path).
    pub async fn inspect(&self, path: &str) -> ContentInspectResult {
        let mut result = self.classify(path);
        if !result.valid {
            return result;
        }
        let Ok(file) = LocalFile::read(Path::new(path)) else {
            return result;
        };
        result.matches = self
            .identify(std::slice::from_ref(&file))
            .await
            .remove(&file.sha1)
            .unwrap_or_default()
            .into_iter()
            .map(ContentMatch::from)
            .collect();
        result
    }

    fn classify(&self, path: &str) -> ContentInspectResult {
        let p = Path::new(path);
        let filename = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let refused = |filename, reason| ContentInspectResult {
            filename,
            reason,
            ..ContentInspectResult::default()
        };
        if !p.is_file() {
            return refused(filename, format!("no file at {}", p.display()));
        }
        match inspect::classify(p) {
            Ok(kind) => ContentInspectResult {
                valid: true,
                kind,
                filename,
                ..ContentInspectResult::default()
            },
            Err(e) => refused(filename, format!("{e:#}")),
        }
    }

    /// The sources that can actually serve a request — a platform whose API key
    /// is unset is registered but not offered.
    pub fn sources(&self) -> Vec<ContentSource> {
        self.providers
            .iter()
            .filter(|p| p.available())
            .map(|p| ContentSource {
                id: p.id().to_string(),
                name: p.name().to_string(),
                kinds: p.kinds(),
            })
            .collect()
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let provider = self.provider(&query.source)?;
        tracing::info!(
            source = provider.id(),
            kind = ?query.kind,
            query = %query.query,
            offset = query.offset,
            limit = query.limit,
            "content search"
        );
        provider.search(query).await
    }

    pub async fn project(
        &self,
        source: &str,
        project: &str,
        kind: Option<ContentKind>,
    ) -> Result<ContentProject> {
        let provider = self.provider(source)?;
        tracing::info!(
            source = provider.id(),
            project,
            ?kind,
            "content project lookup"
        );
        provider.project(project, kind).await
    }

    /// Resolve a source page URL to the project it names (and the version it
    /// pins, for a version page).
    pub async fn resolve_url(&self, url: &str) -> Result<ResolvedUrl> {
        let (source, reference) = self.parse_url(url).ok_or_else(|| {
            anyhow::Error::from(proto::error::ErrorInfo::UnsupportedContentUrl {
                url: url.to_string(),
            })
        })?;
        let project = self
            .project(&source, &reference.project, reference.kind)
            .await?;
        Ok(ResolvedUrl {
            project,
            version_id: reference.version.unwrap_or_default(),
        })
    }

    pub async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>> {
        let provider = self.provider(&query.source)?;
        tracing::info!(
            source = provider.id(),
            project = %query.project,
            loader = ?query.loader,
            game_version = ?query.game_version,
            "content versions lookup"
        );
        provider.versions(query).await
    }

    pub async fn resolve_manifest(
        &self,
        source: &str,
        manifest: &serde_json::Value,
    ) -> Result<ResolvedModpack> {
        self.provider(source)?.resolve_manifest(manifest).await
    }

    /// Several projects at once — one request where the provider has a bulk
    /// endpoint, which is what keeps a pack's hundred-odd lookups affordable.
    pub async fn projects(&self, source: &str, ids: &[String]) -> Result<Vec<ContentProject>> {
        self.provider(source)?.projects(ids).await
    }

    /// Several versions by id — the bulk twin of [`Content::projects`].
    pub async fn versions_by_id(
        &self,
        source: &str,
        ids: &[String],
    ) -> Result<Vec<ContentVersion>> {
        self.provider(source)?.versions_by_id(ids).await
    }

    /// Recognise a project/version page URL on any registered platform's site,
    /// returning the owning source id and the reference it names.
    pub(crate) fn parse_url(&self, url: &str) -> Option<(String, UrlRef)> {
        self.providers
            .iter()
            .find_map(|p| p.parse_url(url).map(|r| (p.id().to_string(), r)))
    }

    /// The provider for `id`; an empty id selects the default — the first
    /// source that can serve. A source that is registered but unconfigured is
    /// refused by name rather than reported unknown, since the difference is
    /// what the user has to act on.
    fn provider(&self, id: &str) -> Result<&dyn ContentProvider> {
        if id.is_empty() {
            return self
                .providers
                .iter()
                .map(AsRef::as_ref)
                .find(|p| p.available())
                .context("no content source is configured");
        }
        let provider = self
            .providers
            .iter()
            .map(AsRef::as_ref)
            .find(|p| p.id() == id)
            .with_context(|| format!("unknown content source: {id}"))?;
        if !provider.available() {
            return Err(proto::error::ErrorInfo::ContentSourceUnavailable {
                source: provider.id().to_string(),
            }
            .into());
        }
        Ok(provider)
    }
}
