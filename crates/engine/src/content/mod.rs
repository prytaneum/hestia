//! Third-party content provider aggregate: the source registry (every source is
//! a plugin's, behind one trait) and the search/project/versions/modpack entry
//! points over it. Stateless — every result is fetched upstream — so it needs no
//! data directory, exactly like the `minecraft` aggregate.

pub(crate) mod bedrock;
pub(crate) mod exclude;
pub(crate) mod icon;
pub(crate) mod inspect;
pub(crate) mod install;
pub(crate) mod modpack;
pub(crate) mod pack;
pub(crate) mod pool;
pub(crate) mod provider;
pub(crate) mod record;
pub(crate) mod updates;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use kernel::vetting::Policy;
use proto::content::{
    ContentFile, ContentInspectResult, ContentKind, ContentMatch, ContentProject, ContentSource,
    ContentVersion, InstalledContent, ModpackFile, ResolvedModpack, ResolvedUrl, SearchQuery,
    SearchResult, VersionQuery, LOCAL_SOURCE,
};

use crate::roster::{RegisterError, Roster, Standing};
use proto::error::ErrorInfo;
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
            record::Project::of(kind, &self.project),
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
    providers: Roster<dyn ContentProvider>,
}

impl Default for Content {
    fn default() -> Self {
        Self::with_providers(Vec::new())
    }
}

impl Content {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build over a given registry rather than the shipped one. The seam a test
    /// crosses to install content without reaching a platform.
    pub fn with_providers(providers: Vec<Box<dyn ContentProvider>>) -> Self {
        Content {
            providers: Roster::builtin(providers.into_iter().map(|p| {
                let provider: Arc<dyn ContentProvider> = p.into();
                (provider.id().to_string(), provider)
            })),
        }
    }

    /// Add `owner`'s source, whose id must read `<owner>.<id>`.
    pub fn register(
        &self,
        owner: &str,
        standing: Standing,
        provider: Arc<dyn ContentProvider>,
    ) -> Result<(), RegisterError> {
        self.providers
            .register(owner, standing, provider.id(), provider.clone())
    }

    /// The full id of the source `name` means, for what is stored.
    pub fn canonical(&self, name: &str) -> Result<String> {
        self.resolve(name).map(|(id, _)| id)
    }

    /// [`Content::canonical`] for a name to compare: one that means no source
    /// (empty, local, or unregistered) stays as given.
    pub(crate) fn named(&self, name: &str) -> String {
        self.canonical(name).unwrap_or_else(|_| name.to_string())
    }

    /// Remove every source `owner` registered. Content already installed from
    /// one keeps its record; [`Content::missing_sources`] names the source gone.
    pub fn unregister(&self, owner: &str) -> usize {
        self.providers.unregister(owner)
    }

    /// The sources `items` were installed from that no registered provider
    /// answers to any more, sorted. A local-file import has no source to lose.
    pub fn missing_sources(&self, items: &[InstalledContent]) -> Vec<String> {
        items
            .iter()
            .map(|item| item.source.as_str())
            .filter(|source| *source != LOCAL_SOURCE)
            .filter(|source| self.resolve(source).is_err())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// A failed source is logged, not returned: a lookup must not stop an import.
    pub async fn identify(&self, files: &[LocalFile]) -> HashMap<String, Vec<Identified>> {
        let mut found: HashMap<String, Vec<Identified>> = HashMap::new();
        if files.is_empty() {
            return found;
        }
        for provider in self.providers.snapshot().iter().filter(|p| p.available()) {
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

    /// The sources that can serve a request, and the ones that wait on a
    /// setting, which say which — a platform whose API key is unset is listed,
    /// not offered.
    pub fn sources(&self) -> Vec<ContentSource> {
        self.providers
            .snapshot()
            .iter()
            .filter_map(|p| {
                let requires_setting = p.missing_setting();
                (p.available() || requires_setting.is_some()).then(|| ContentSource {
                    id: p.id().to_string(),
                    name: p.name().to_string(),
                    kinds: p.kinds(),
                    editions: p.editions(),
                    requires_setting,
                })
            })
            .collect()
    }

    pub fn ensure_serves(&self, source: &str, edition: proto::minecraft::Edition) -> Result<()> {
        let provider = self.provider(source)?;
        if provider.editions().contains(&edition) {
            return Ok(());
        }
        anyhow::bail!(proto::error::ErrorInfo::ContentSourceEdition {
            source: provider.name().to_string(),
        })
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
        let (source, reference) = self.parse_url(url).await.ok_or_else(|| {
            anyhow::Error::from(ErrorInfo::UnsupportedContentUrl {
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
            loaders = ?query.loaders,
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
    /// One source's bulk update lookup; `None` when it has none.
    pub(crate) async fn latest(
        &self,
        source: &str,
        query: &provider::LatestQuery,
    ) -> Result<Option<HashMap<String, String>>> {
        let provider = self.provider(source)?;
        tracing::debug!(
            source = provider.id(),
            files = query.files.len(),
            game_version = %query.game_version,
            loaders = ?query.loaders,
            "content update lookup"
        );
        provider.latest(query).await
    }

    pub async fn versions_by_id(
        &self,
        source: &str,
        ids: &[String],
    ) -> Result<Vec<ContentVersion>> {
        self.provider(source)?.versions_by_id(ids).await
    }

    /// Recognise a project/version page URL on any registered platform's site,
    /// returning the owning source id and the reference it names.
    pub(crate) async fn parse_url(&self, url: &str) -> Option<(String, UrlRef)> {
        for provider in self.providers.snapshot() {
            if let Some(reference) = provider.parse_url(url).await {
                return Some((provider.id().to_string(), reference));
            }
        }
        None
    }

    pub(crate) async fn recognise_files(&self, files: &mut [ModpackFile]) {
        let providers = self.providers.snapshot();
        for file in files {
            for provider in &providers {
                if let Some(found) = provider.recognise_file_url(&file.artifact.url).await {
                    file.source = provider.id().to_string();
                    file.project_id = found.project;
                    file.version_id = found.version;
                    break;
                }
            }
        }
    }

    /// The policy the files of source `id` are downloaded under. An empty or
    /// local source names files only through an archive, which is untrusted;
    /// any other unknown source is refused.
    pub(crate) fn download_policy(&self, id: &str) -> Result<Arc<Policy>> {
        if id.is_empty() || id == LOCAL_SOURCE {
            return Ok(crate::net::hosts::untrusted());
        }
        Ok(self.provider(id)?.download_policy())
    }

    pub(crate) fn datapacks_as_mods(&self, id: &str) -> bool {
        self.provider(id).is_ok_and(|p| p.datapacks_as_mods())
    }

    fn resolve(&self, name: &str) -> Result<(String, Arc<dyn ContentProvider>)> {
        self.providers
            .resolve(name, |_| true)
            .map_err(|unresolved| unresolved.into_error(name))
    }

    /// The provider for `id`; an empty id selects the default — the first
    /// source that can serve. A source that is registered but unconfigured is
    /// refused by name rather than reported unknown, since the difference is
    /// what the user has to act on.
    fn provider(&self, id: &str) -> Result<Arc<dyn ContentProvider>> {
        let providers = self.providers.snapshot();
        if id.is_empty() {
            if let Some(ready) = providers.iter().find(|p| p.available()) {
                return Ok(ready.clone());
            }
            return Err(match providers.first() {
                Some(waiting) => ErrorInfo::ContentSourceUnavailable {
                    source: waiting.id().to_string(),
                    setting: waiting.missing_setting(),
                },
                None => ErrorInfo::NoContentSource,
            }
            .into());
        }
        let (_, provider) = self.resolve(id)?;
        if !provider.available() {
            return Err(ErrorInfo::ContentSourceUnavailable {
                source: provider.id().to_string(),
                setting: provider.missing_setting(),
            }
            .into());
        }
        Ok(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::content::SourceFileRef;
    use proto::error::ErrorInfo;
    use proto::minecraft::Edition;

    struct Recogniser {
        id: &'static str,
        prefix: &'static str,
    }

    #[async_trait::async_trait]
    impl ContentProvider for Recogniser {
        fn id(&self) -> &str {
            self.id
        }
        fn name(&self) -> &str {
            self.id
        }
        fn kinds(&self) -> Vec<ContentKind> {
            Vec::new()
        }
        fn download_policy(&self) -> Arc<Policy> {
            crate::net::hosts::untrusted()
        }
        fn datapacks_as_mods(&self) -> bool {
            self.id == "mods"
        }
        async fn recognise_file_url(&self, url: &str) -> Option<SourceFileRef> {
            url.strip_prefix(self.prefix).map(|rest| SourceFileRef {
                project: rest.to_string(),
                version: "v1".to_string(),
            })
        }
        async fn parse_url(&self, _url: &str) -> Option<provider::UrlRef> {
            None
        }
        async fn search(&self, _query: &SearchQuery) -> Result<SearchResult> {
            unreachable!()
        }
        async fn project(&self, _: &str, _: Option<ContentKind>) -> Result<ContentProject> {
            unreachable!()
        }
        async fn versions(&self, _query: &VersionQuery) -> Result<Vec<ContentVersion>> {
            unreachable!()
        }
    }

    fn file(url: &str) -> ModpackFile {
        let mut file = ModpackFile::default();
        file.artifact.url = url.to_string();
        file
    }

    fn recognisers() -> Content {
        Content::with_providers(vec![
            Box::new(Recogniser {
                id: "mods",
                prefix: "https://mods.invalid/",
            }),
            Box::new(Recogniser {
                id: "other",
                prefix: "https://other.invalid/",
            }),
        ])
    }

    #[tokio::test]
    async fn a_file_is_tracked_to_the_source_that_recognises_its_url() {
        let mut files = vec![
            file("https://other.invalid/sodium"),
            file("https://elsewhere.invalid/x"),
        ];
        recognisers().recognise_files(&mut files).await;

        assert_eq!(files[0].source, "other");
        assert_eq!(files[0].project_id, "sodium");
        assert_eq!(files[0].version_id, "v1");
        assert!(files[1].source.is_empty() && files[1].project_id.is_empty());
    }

    #[test]
    fn a_java_source_refuses_a_bedrock_entry() {
        let content = recognisers();
        assert!(content.ensure_serves("mods", Edition::Java).is_ok());
        let refused = content.ensure_serves("mods", Edition::Bedrock).unwrap_err();
        assert!(matches!(
            refused.downcast_ref::<ErrorInfo>(),
            Some(ErrorInfo::ContentSourceEdition { .. })
        ));
    }
}
