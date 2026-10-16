//! Modpacks: install one into a new or existing entry, move it to another
//! published version, read which pack an entry runs, and take it back out.
//!
//! The install and update verbs are jobs — a pack is a hundred downloads and a
//! game directory rewrite — so they publish the `modpack.*` topics and settle on
//! the done event, which carries the entry the pack landed in. That id is the
//! only way a caller learns which entry a *creating* install just made.

use std::time::Duration;

use ipc::errors::IpcError;
use proto::error::EntryTarget;
use proto::minecraft::ProvisionProgress;
use proto::modpack::{
    EntryModpackCheckParams, EntryModpackCheckUpdate, EntryModpackFileParams,
    EntryModpackPreviewUpdate, EntryModpackRemove, EntryModpackRemoveParams, EntryModpackStatus,
    EntryModpackStatusParams, EntryModpackUpdate, EntryModpackUpdateParams, InstalledModpack,
    InstanceModpackInstall, InstanceModpackInstallParams, ModpackDoneEvent, ModpackFilePreview,
    ModpackMove, ModpackRef, ModpackRemoveResult, ModpackTarget, ModpackUpdate,
    ServerModpackInstall, ServerModpackInstallParams,
};

use crate::facades::jobs::forward;
use crate::session::{job_id, Session};

/// A catalogue round trip, so it outlives the default call timeout.
const CHECK_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Modpack<'a> {
    pub(crate) session: &'a Session,
}

impl Modpack<'_> {
    /// Install a pack into a new or existing instance.
    pub async fn install_instance(
        &self,
        pack: ModpackRef,
        target: ModpackTarget,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ModpackDoneEvent, IpcError> {
        let id = job_id("instance-modpack-install");
        let session = self.session;
        let params = InstanceModpackInstallParams {
            pack,
            target,
            id: id.clone(),
        };
        self.drive(&id, on_progress, move || async move {
            session
                .call::<InstanceModpackInstall>(&params)
                .await
                .map(|_| ())
        })
        .await
    }

    /// Install a pack into a new or existing server. `eula` is required when the
    /// target creates one.
    pub async fn install_server(
        &self,
        pack: ModpackRef,
        target: ModpackTarget,
        eula: bool,
        port: Option<u16>,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ModpackDoneEvent, IpcError> {
        let id = job_id("server-modpack-install");
        let session = self.session;
        let params = ServerModpackInstallParams {
            pack,
            target,
            eula,
            port,
            id: id.clone(),
        };
        self.drive(&id, on_progress, move || async move {
            session
                .call::<ServerModpackInstall>(&params)
                .await
                .map(|_| ())
        })
        .await
    }

    /// Move an entry's pack to another published version (empty takes the
    /// newest) or to a pack file. A pack update carries the game version with
    /// it, so an older one needs `allow_downgrade`.
    pub async fn update(
        &self,
        target: &EntryTarget,
        to: ModpackMove,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ModpackDoneEvent, IpcError> {
        let id = job_id("modpack-update");
        let session = self.session;
        let params = EntryModpackUpdateParams {
            target: target.clone(),
            to,
            id: id.clone(),
        };
        self.drive(&id, on_progress, move || async move {
            session
                .call::<EntryModpackUpdate>(&params)
                .await
                .map(|_| ())
        })
        .await
    }

    /// What updating an entry's pack from the file at `path` would do.
    pub async fn preview_update(
        &self,
        target: &EntryTarget,
        path: &str,
    ) -> Result<ModpackFilePreview, IpcError> {
        let params = EntryModpackFileParams {
            target: target.clone(),
            path: path.to_string(),
        };
        self.session
            .call_with_timeout::<EntryModpackPreviewUpdate>(&params, CHECK_TIMEOUT)
            .await
    }

    /// The pack an entry runs, or `None` when it was not built from one.
    pub async fn status(&self, target: &EntryTarget) -> Result<Option<InstalledModpack>, IpcError> {
        let params = EntryModpackStatusParams {
            target: target.clone(),
        };
        Ok(self.session.call::<EntryModpackStatus>(&params).await?.pack)
    }

    /// Whether the entry's pack has a newer published version. `None` when
    /// there is nothing to check — no pack, or one imported from a file.
    pub async fn check_update(
        &self,
        target: &EntryTarget,
        game_version: &str,
    ) -> Result<Option<ModpackUpdate>, IpcError> {
        let params = EntryModpackCheckParams {
            target: target.clone(),
            game_version: game_version.to_string(),
        };
        Ok(self
            .session
            .call_with_timeout::<EntryModpackCheckUpdate>(&params, CHECK_TIMEOUT)
            .await?
            .update)
    }

    pub async fn remove(
        &self,
        target: &EntryTarget,
        keep_content: bool,
    ) -> Result<ModpackRemoveResult, IpcError> {
        let params = EntryModpackRemoveParams {
            target: target.clone(),
            keep_content,
        };
        self.session.call::<EntryModpackRemove>(&params).await
    }

    async fn drive<F, Fut>(
        &self,
        id: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
        start: F,
    ) -> Result<ModpackDoneEvent, IpcError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), IpcError>>,
    {
        let payload = self
            .session
            .run_job(
                id,
                "modpack.done",
                "modpack.error",
                forward(on_progress),
                start,
            )
            .await?;
        serde_json::from_value(payload).map_err(|e| IpcError::Malformed(e.to_string()))
    }
}
