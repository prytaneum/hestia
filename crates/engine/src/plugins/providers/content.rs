//! Content sources contributed by plugins. A plugin's `content-source` export
//! is wrapped in a [`PluginContentProvider`], which the content aggregate
//! treats like any built-in source. The guest only describes what exists; the
//! engine downloads the files it lists, and only from hosts the plugin was
//! granted.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use proto::content::{
    ContentKind, ContentProject, ContentVersion, SearchQuery, SearchResult, SourceFileRef,
    SourceInfo, SourceUrl, VersionQuery,
};
use proto::plugin::{Manifest, CONTENT_PROVIDER};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::call_failed;
use crate::content::provider::{ContentProvider, LocalFile, UrlRef};
use crate::content::Content;
use crate::plugins::chain::CallChain;
use crate::plugins::http::download_allowed;
use crate::plugins::runtime::{LoadedPlugin, PROVIDER_TIMEOUT};

/// One function of the `content-source` export.
#[derive(Debug, Clone, Copy)]
pub(in crate::plugins) enum SourceOp {
    Info,
    ParseUrl,
    RecogniseFileUrl,
    Search,
    Project,
    Projects,
    Versions,
    VersionsById,
    Identify,
}

impl SourceOp {
    fn name(self) -> &'static str {
        match self {
            SourceOp::Info => "info",
            SourceOp::ParseUrl => "parse-url",
            SourceOp::RecogniseFileUrl => "recognise-file-url",
            SourceOp::Search => "search",
            SourceOp::Project => "project",
            SourceOp::Projects => "projects",
            SourceOp::Versions => "versions",
            SourceOp::VersionsById => "versions-by-id",
            SourceOp::Identify => "identify",
        }
    }
}

/// A source a plugin contributes, registered as `<plugin>.<id>`.
pub struct PluginContentProvider {
    id: String,
    local: String,
    plugin: Arc<LoadedPlugin>,
    info: SourceInfo,
}

impl PluginContentProvider {
    /// Asks the guest to describe the source `local` of its manifest.
    pub async fn connect(plugin: Arc<LoadedPlugin>, local: &str) -> Result<Self> {
        let mut provider = PluginContentProvider {
            id: format!("{}.{local}", plugin.id()),
            local: local.to_string(),
            plugin,
            info: SourceInfo::default(),
        };
        provider.info = provider.ask(SourceOp::Info, Value::Null).await?;
        Ok(provider)
    }

    async fn ask<T: DeserializeOwned>(&self, op: SourceOp, args: Value) -> Result<T> {
        let reply = self
            .plugin
            .content_source(
                &CallChain::default(),
                op,
                &self.local,
                &args.to_string(),
                PROVIDER_TIMEOUT,
            )
            .await
            .map_err(|error| self.failed(format!("{}: {error}", op.name())))?;
        serde_json::from_str(&reply).map_err(|error| {
            tracing::warn!(plugin = self.plugin.id(), source = %self.local, op = op.name(), %error, "malformed content source reply");
            self.failed(format!("{} returned an unreadable reply", op.name()))
        })
    }

    fn failed(&self, detail: String) -> anyhow::Error {
        call_failed(self.plugin.id(), detail)
    }

    fn project_of(&self, mut project: ContentProject) -> ContentProject {
        project.source = self.id.clone();
        project
    }

    /// Stamps the version as this source's and keeps only the files the engine
    /// may download for the plugin; a version left with none is dropped.
    fn version_of(&self, mut version: ContentVersion) -> Option<ContentVersion> {
        version.source = self.id.clone();
        let listed = version.files.len();
        version
            .files
            .retain(|file| download_allowed(self.plugin.grant(), &file.artifact.url));
        if version.files.len() < listed {
            tracing::warn!(
                plugin = self.plugin.id(),
                source = %self.local,
                version = %version.id,
                dropped = listed - version.files.len(),
                "files outside the plugin's network hosts were dropped"
            );
        }
        (listed == 0 || !version.files.is_empty()).then_some(version)
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
        &self.id
    }

    fn name(&self) -> &str {
        &self.info.name
    }

    fn kinds(&self) -> Vec<ContentKind> {
        self.info.kinds.clone()
    }

    fn available(&self) -> bool {
        !self.plugin.is_disabled()
    }

    async fn parse_url(&self, url: &str) -> Option<UrlRef> {
        let found: Option<SourceUrl> = self
            .ask(SourceOp::ParseUrl, json!({ "url": url }))
            .await
            .inspect_err(|error| {
                tracing::warn!(source = %self.id, error = %format!("{error:#}"), "content source could not parse a URL");
            })
            .ok()?;
        found.map(|found| UrlRef {
            project: found.project,
            version: found.version,
            kind: found.kind,
        })
    }

    async fn recognise_file_url(&self, url: &str) -> Option<SourceFileRef> {
        if !download_allowed(self.plugin.grant(), url) {
            return None;
        }
        self.ask(SourceOp::RecogniseFileUrl, json!({ "url": url }))
            .await
            .inspect_err(|error| {
                tracing::warn!(source = %self.id, error = %format!("{error:#}"), "content source could not recognise a file URL");
            })
            .ok()?
    }

    fn datapacks_as_mods(&self) -> bool {
        self.info.datapacks_as_mods
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let mut result: SearchResult = self.ask(SourceOp::Search, json!(query)).await?;
        result.hits = result
            .hits
            .into_iter()
            .map(|hit| self.project_of(hit))
            .collect();
        Ok(result)
    }

    async fn project(&self, project: &str, kind: Option<ContentKind>) -> Result<ContentProject> {
        let found = self
            .ask(
                SourceOp::Project,
                json!({ "project": project, "kind": kind }),
            )
            .await?;
        Ok(self.project_of(found))
    }

    async fn projects(&self, ids: &[String]) -> Result<Vec<ContentProject>> {
        let found: Vec<ContentProject> =
            self.ask(SourceOp::Projects, json!({ "ids": ids })).await?;
        Ok(found
            .into_iter()
            .map(|project| self.project_of(project))
            .collect())
    }

    async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>> {
        let found = self.ask(SourceOp::Versions, json!(query)).await?;
        Ok(self.versions_of(found))
    }

    async fn versions_by_id(&self, ids: &[String]) -> Result<Vec<ContentVersion>> {
        let found = self
            .ask(SourceOp::VersionsById, json!({ "ids": ids }))
            .await?;
        Ok(self.versions_of(found))
    }

    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        let sha1s: Vec<&str> = files.iter().map(|file| file.sha1.as_str()).collect();
        let found: HashMap<String, ContentVersion> = self
            .ask(SourceOp::Identify, json!({ "sha1s": sha1s }))
            .await?;
        Ok(found
            .into_iter()
            .filter_map(|(sha1, version)| Some((sha1, self.version_of(version)?)))
            .collect())
    }
}

/// Connects every content source the manifest contributes. One that fails to
/// describe itself is logged and left out; the plugin stays enabled.
pub async fn connect_sources(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
) -> Vec<PluginContentProvider> {
    let mut sources = Vec::new();
    for contribution in &manifest.contributes.providers {
        if contribution.kind != CONTENT_PROVIDER {
            continue;
        }
        match PluginContentProvider::connect(plugin.clone(), &contribution.id).await {
            Ok(source) => sources.push(source),
            Err(error) => tracing::warn!(
                plugin = plugin.id(),
                source = %contribution.id,
                error = %format!("{error:#}"),
                "content source not registered"
            ),
        }
    }
    sources
}

/// Adds `sources` to `content` under `owner`. `official` is whether the
/// plugin's key is the official one; see [`Roster::register`](crate::roster).
pub fn register_sources(
    content: &Content,
    owner: &str,
    official: bool,
    sources: Vec<PluginContentProvider>,
) {
    for source in sources {
        let id = source.id.clone();
        match content.register(owner, official, Arc::new(source)) {
            Ok(()) => tracing::info!(plugin = owner, source = %id, "content source registered"),
            Err(error) => {
                tracing::warn!(plugin = owner, source = %id, %error, "content source refused")
            }
        }
    }
}
