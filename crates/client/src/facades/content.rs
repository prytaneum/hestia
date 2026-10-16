use ipc::errors::IpcError;
use proto::content::{
    ContentInspect, ContentInspectParams, ContentInspectResult, ContentKind, ContentProject,
    ContentProjectGet, ContentResolveUrl, ContentSearch, ContentSource, ContentSources,
    ContentVersion, ContentVersions, ProjectParams, ResolveUrlParams, ResolvedUrl, SearchQuery,
    SearchResult, VersionQuery,
};

use crate::session::Session;

pub struct Content<'a> {
    pub(crate) session: &'a Session,
}

impl Content<'_> {
    /// The available content sources (modrinth, …).
    pub async fn sources(&self) -> Result<Vec<ContentSource>, IpcError> {
        Ok(self
            .session
            .call::<ContentSources>(&proto::Empty {})
            .await?
            .sources)
    }

    /// A paginated search over a source (empty `query.source` picks the default).
    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResult, IpcError> {
        self.session.call::<ContentSearch>(query).await
    }

    /// A project's detail. `kind` is the browse context, stamped onto the
    /// answer when the project publishes it.
    pub async fn project(
        &self,
        source: &str,
        project: &str,
        kind: Option<ContentKind>,
    ) -> Result<ContentProject, IpcError> {
        let params = ProjectParams {
            source: source.to_string(),
            project: project.to_string(),
            kind,
        };
        self.session.call::<ContentProjectGet>(&params).await
    }

    /// Resolve a source page URL to the project (and pinned version) it names.
    pub async fn resolve_url(&self, url: &str) -> Result<ResolvedUrl, IpcError> {
        let params = ResolveUrlParams {
            url: url.to_string(),
        };
        self.session.call::<ContentResolveUrl>(&params).await
    }

    pub async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>, IpcError> {
        Ok(self.session.call::<ContentVersions>(query).await?.versions)
    }

    /// Classify a daemon-local file for import (detected kind + validity).
    pub async fn inspect(&self, path: &str) -> Result<ContentInspectResult, IpcError> {
        let params = ContentInspectParams {
            path: path.to_string(),
        };
        self.session.call::<ContentInspect>(&params).await
    }
}
