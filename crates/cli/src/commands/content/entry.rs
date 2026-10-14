//! A server or instance addressed for content operations. The two facades
//! expose byte-for-byte identical content methods, so this dispatches on the
//! kind once and lets callers stop re-matching server-vs-instance at every
//! call.

use client::proto::content::{
    ContentAddSpec, ContentFailure, ContentKind, ContentUpdate, InstalledContent, UntrackedFile,
};
use client::proto::minecraft::ProvisionProgress;
use client::{Client, IpcError};

use super::EntryKind;

pub(super) struct ContentEntry<'a> {
    client: &'a Client,
    kind: EntryKind,
    id: String,
}

impl<'a> ContentEntry<'a> {
    pub(super) fn new(client: &'a Client, kind: EntryKind, id: impl Into<String>) -> Self {
        ContentEntry {
            client,
            kind,
            id: id.into(),
        }
    }

    pub(super) async fn list(
        &self,
        kind: ContentKind,
    ) -> Result<(Vec<InstalledContent>, Vec<UntrackedFile>), IpcError> {
        match self.kind {
            EntryKind::Server => self.client.server().content_list(&self.id, kind).await,
            EntryKind::Instance => self.client.instance().content_list(&self.id, kind).await,
        }
    }

    pub(super) async fn add(
        &self,
        spec: ContentAddSpec,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>), IpcError> {
        match self.kind {
            EntryKind::Server => {
                self.client
                    .server()
                    .content_add(&self.id, spec, on_progress)
                    .await
            }
            EntryKind::Instance => {
                self.client
                    .instance()
                    .content_add(&self.id, spec, on_progress)
                    .await
            }
        }
    }

    pub(super) async fn remove(
        &self,
        kind: ContentKind,
        items: &[String],
        worlds: &[String],
    ) -> Result<(), IpcError> {
        match self.kind {
            EntryKind::Server => {
                self.client
                    .server()
                    .content_remove(&self.id, kind, items, worlds)
                    .await
            }
            EntryKind::Instance => {
                self.client
                    .instance()
                    .content_remove(&self.id, kind, items, worlds)
                    .await
            }
        }
    }

    pub(super) async fn update(
        &self,
        kind: ContentKind,
        items: &[String],
        path: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>), IpcError> {
        match self.kind {
            EntryKind::Server => {
                self.client
                    .server()
                    .content_update(&self.id, kind, items, path, on_progress)
                    .await
            }
            EntryKind::Instance => {
                self.client
                    .instance()
                    .content_update(&self.id, kind, items, path, on_progress)
                    .await
            }
        }
    }

    pub(super) async fn enable(
        &self,
        kind: ContentKind,
        item: &str,
        enabled: bool,
        worlds: &[String],
    ) -> Result<(), IpcError> {
        match self.kind {
            EntryKind::Server => {
                self.client
                    .server()
                    .content_enable(&self.id, kind, item, enabled, worlds)
                    .await
            }
            EntryKind::Instance => {
                self.client
                    .instance()
                    .content_enable(&self.id, kind, item, enabled, worlds)
                    .await
            }
        }
    }

    pub(super) async fn check_updates(
        &self,
        kind: ContentKind,
    ) -> Result<Vec<ContentUpdate>, IpcError> {
        match self.kind {
            EntryKind::Server => {
                self.client
                    .server()
                    .content_check_updates(&self.id, kind)
                    .await
            }
            EntryKind::Instance => {
                self.client
                    .instance()
                    .content_check_updates(&self.id, kind)
                    .await
            }
        }
    }

    pub(super) async fn set_version(
        &self,
        kind: ContentKind,
        item: &str,
        version: &str,
        source: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<Vec<InstalledContent>, IpcError> {
        match self.kind {
            EntryKind::Server => {
                self.client
                    .server()
                    .content_set_version(&self.id, kind, item, version, source, on_progress)
                    .await
            }
            EntryKind::Instance => {
                self.client
                    .instance()
                    .content_set_version(&self.id, kind, item, version, source, on_progress)
                    .await
            }
        }
    }
}
