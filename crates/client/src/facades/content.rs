use ipc::errors::IpcError;
use proto::content::{
    ContentAddSpec, ContentDone, ContentInspect, ContentInspectParams, ContentInspectResult,
    ContentKind, ContentProject, ContentProjectGet, ContentResolveUrl, ContentSearch,
    ContentSource, ContentSources, ContentUpdate, ContentVersion, ContentVersions, EntryContentAdd,
    EntryContentAddParams, EntryContentCheckUpdates, EntryContentCheckUpdatesParams,
    EntryContentEnable, EntryContentEnableParams, EntryContentList, EntryContentListParams,
    EntryContentRemove, EntryContentRemoveParams, EntryContentSetVersion,
    EntryContentSetVersionParams, EntryContentUpdate, EntryContentUpdateParams, InstalledContent,
    ProjectParams, ResolveUrlParams, ResolvedUrl, SearchQuery, SearchResult, UntrackedFile,
    VersionQuery,
};
use proto::error::EntryTarget;
use proto::minecraft::ProvisionProgress;

use crate::session::{job_id, Session};

pub struct Content<'a> {
    pub(crate) session: &'a Session,
}

impl Content<'_> {
    /// The available content sources (modrinth, …).
    pub async fn sources(&self) -> Result<Vec<ContentSource>, IpcError> {
        self.session.call::<ContentSources>(&proto::Empty {}).await
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
        self.session.call::<ContentVersions>(query).await
    }

    /// Classify a daemon-local file for import (detected kind + validity).
    pub async fn inspect(&self, path: &str) -> Result<ContentInspectResult, IpcError> {
        let params = ContentInspectParams {
            path: path.to_string(),
        };
        self.session.call::<ContentInspect>(&params).await
    }

    /// Install a batch of content into an entry, blocking until the daemon
    /// reports done or error and forwarding each progress event to
    /// `on_progress`. Returns everything installed (items plus required
    /// dependencies) and, per item that could not be installed, a failure.
    pub async fn add(
        &self,
        target: &EntryTarget,
        spec: ContentAddSpec,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ContentDone, IpcError> {
        let id = job_id("content-add");
        let params = EntryContentAddParams {
            target: target.clone(),
            spec,
            id: id.clone(),
        };
        self.session
            .run_job::<EntryContentAdd>(&id, &params, on_progress)
            .await
    }

    pub async fn list(
        &self,
        target: &EntryTarget,
        kind: ContentKind,
    ) -> Result<(Vec<InstalledContent>, Vec<UntrackedFile>), IpcError> {
        let params = EntryContentListParams {
            target: target.clone(),
            kind,
        };
        let result = self.session.call::<EntryContentList>(&params).await?;
        Ok((result.items, result.untracked))
    }

    /// Uninstall the named items and every mirror of them, in one call. A
    /// non-empty `worlds` instead narrows a datapack to the save worlds it
    /// keeps loading in.
    pub async fn remove(
        &self,
        target: &EntryTarget,
        kind: ContentKind,
        items: &[String],
        worlds: &[String],
    ) -> Result<(), IpcError> {
        let params = EntryContentRemoveParams {
            target: target.clone(),
            kind,
            items: items.to_vec(),
            worlds: worlds.to_vec(),
        };
        self.session.call::<EntryContentRemove>(&params).await?;
        Ok(())
    }

    /// Update platform-sourced content to its newest compatible version — the
    /// named items in one job, or every item of the kind when `items` is empty,
    /// with a failure per item that could not move. A non-empty `path` instead
    /// updates the one named local item from that file.
    pub async fn update(
        &self,
        target: &EntryTarget,
        kind: ContentKind,
        items: &[String],
        path: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ContentDone, IpcError> {
        let id = job_id("content-update");
        let params = EntryContentUpdateParams {
            target: target.clone(),
            kind,
            items: items.to_vec(),
            path: path.to_string(),
            id: id.clone(),
        };
        self.session
            .run_job::<EntryContentUpdate>(&id, &params, on_progress)
            .await
    }

    /// Enable or disable one installed item; a non-empty `worlds` scopes a
    /// datapack toggle to those save worlds.
    pub async fn enable(
        &self,
        target: &EntryTarget,
        kind: ContentKind,
        item: &str,
        enabled: bool,
        worlds: &[String],
    ) -> Result<(), IpcError> {
        let params = EntryContentEnableParams {
            target: target.clone(),
            kind,
            item: item.to_string(),
            enabled,
            worlds: worlds.to_vec(),
        };
        self.session.call::<EntryContentEnable>(&params).await?;
        Ok(())
    }

    /// Which platform-sourced items of the kind have a newer compatible version.
    pub async fn check_updates(
        &self,
        target: &EntryTarget,
        kind: ContentKind,
    ) -> Result<Vec<ContentUpdate>, IpcError> {
        let params = EntryContentCheckUpdatesParams {
            target: target.clone(),
            kind,
        };
        self.session.call::<EntryContentCheckUpdates>(&params).await
    }

    /// Re-pin one item to a specific published `version` (id or number).
    pub async fn set_version(
        &self,
        target: &EntryTarget,
        kind: ContentKind,
        item: &str,
        version: &str,
        source: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<Vec<InstalledContent>, IpcError> {
        let id = job_id("content-set-version");
        let params = EntryContentSetVersionParams {
            target: target.clone(),
            kind,
            item: item.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            id: id.clone(),
        };
        Ok(self
            .session
            .run_job::<EntryContentSetVersion>(&id, &params, on_progress)
            .await?
            .items)
    }
}
