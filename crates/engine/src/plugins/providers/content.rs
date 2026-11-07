//! Content sources contributed by plugins, wrapped in a
//! [`PluginContentProvider`] that the content aggregate treats like any
//! built-in source. The guest only describes what exists; the engine downloads
//! the files it lists, and only from hosts the plugin was granted.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use proto::content::{
    ContentKind, ContentProject, ContentVersion, SearchQuery, SearchResult, SourceFileRef,
    SourceInfo, VersionQuery,
};
use proto::plugin::Manifest;
use proto::provider::content::{self as op, IdQuery, ProjectQuery, Sha1Query, Source, UrlQuery};
use proto::Empty;

use super::{connect_each, registered, Guest};
use crate::content::provider::{ContentProvider, LocalFile, UrlRef};
use crate::engine::Engine;
use crate::plugins::http::download_allowed;
use crate::plugins::runtime::LoadedPlugin;

/// A source a plugin contributes, registered as `<plugin>.<id>`.
pub struct PluginContentProvider {
    guest: Guest,
    info: SourceInfo,
}

impl PluginContentProvider {
    /// Asks the guest to describe its source.
    async fn connect(guest: Guest) -> Result<Self> {
        let info = guest.ask::<op::Info>(&Empty {}).await?;
        Ok(PluginContentProvider { guest, info })
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
            .retain(|file| download_allowed(self.guest.plugin().grant(), &file.artifact.url));
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
        !self.guest.plugin().is_disabled()
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
        if !download_allowed(self.guest.plugin().grant(), url) {
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

    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        let query = Sha1Query {
            sha1s: files.iter().map(|file| file.sha1.clone()).collect(),
        };
        let found = self
            .guest
            .try_ask::<op::Identify>(&query)
            .await?
            .unwrap_or_default();
        Ok(found
            .into_iter()
            .filter_map(|(sha1, version)| Some((sha1, self.version_of(version)?)))
            .collect())
    }
}

pub(super) async fn connect(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
) -> Vec<PluginContentProvider> {
    connect_each::<Source, _, _, _>(plugin, manifest, PluginContentProvider::connect).await
}

pub(super) fn register(
    engine: &Engine,
    owner: &str,
    official: bool,
    sources: Vec<PluginContentProvider>,
) {
    for source in sources {
        let id = source.guest.id().to_string();
        let outcome = engine.content().register(owner, official, Arc::new(source));
        registered(owner, "content", &id, outcome);
    }
}
