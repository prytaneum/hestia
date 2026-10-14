use std::time::Duration;

use crate::facades::jobs::{forward, run_backup_job, run_content_job};
use crate::session::{job_id, Session};
use ipc::errors::IpcError;
use proto::backup::{
    BackupInfo, ServerBackupCreate, ServerBackupCreateParams, ServerBackupList, ServerBackupRef,
    ServerBackupRemove, ServerBackupRestore, ServerBackupRestoreParams,
};
use proto::content::{
    ContentAddSpec, ContentFailure, ContentKind, ContentMatch, ContentUpdate, InstalledContent,
    ServerContentAdd, ServerContentAddParams, ServerContentCheckUpdates,
    ServerContentCheckUpdatesParams, ServerContentEnable, ServerContentEnableParams,
    ServerContentList, ServerContentListParams, ServerContentMatches, ServerContentMatchesParams,
    ServerContentRemove, ServerContentRemoveParams, ServerContentSetVersion,
    ServerContentSetVersionParams, ServerContentUpdate, ServerContentUpdateParams, UntrackedFile,
};
use proto::minecraft::{
    ConfigEntry, Flavor, GameVersion, LoadersParams, ProvisionProgress, ResolveParams,
    ServerProfile, VersionsParams,
};
use proto::process::ProcessLogLine;
use proto::server::{
    ServerCommand, ServerCommandComplete, ServerCommandCompleteParams, ServerCommandCompleteResult,
    ServerCommandParams, ServerConfigGet, ServerConfigGetParams, ServerConfigList, ServerConfigSet,
    ServerConfigSetParams, ServerCreate, ServerCreateDoneEvent, ServerCreateParams, ServerDetail,
    ServerDetails, ServerFlavors, ServerInfo, ServerList, ServerLoaders, ServerLogs,
    ServerLogsParams, ServerPing, ServerPingResult, ServerProperties, ServerProperty, ServerRef,
    ServerRemove, ServerRename, ServerRenameParams, ServerResolve, ServerRestart, ServerStart,
    ServerStartResult, ServerStatus, ServerStop, ServerUpdate, ServerUpdateDoneEvent,
    ServerUpdateParams, ServerVersions,
};

pub struct Server<'a> {
    pub(crate) session: &'a Session,
}

impl Server<'_> {
    pub async fn flavors(&self) -> Result<Vec<Flavor>, IpcError> {
        Ok(self
            .session
            .call::<ServerFlavors>(&proto::Empty {})
            .await?
            .flavors)
    }

    pub async fn versions(&self, flavor: &str) -> Result<Vec<GameVersion>, IpcError> {
        let params = VersionsParams {
            flavor: flavor.to_string(),
        };
        Ok(self.session.call::<ServerVersions>(&params).await?.versions)
    }

    pub async fn resolve(
        &self,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
    ) -> Result<ServerProfile, IpcError> {
        let params = ResolveParams {
            flavor: flavor.to_string(),
            version: version.to_string(),
            loader_version,
        };
        self.session.call::<ServerResolve>(&params).await
    }

    pub async fn loaders(&self, flavor: &str, version: &str) -> Result<Vec<String>, IpcError> {
        let params = LoadersParams {
            flavor: flavor.to_string(),
            version: version.to_string(),
        };
        Ok(self.session.call::<ServerLoaders>(&params).await?.loaders)
    }

    /// Create a fully provisioned server, blocking until the daemon reports
    /// done or error and forwarding each progress event to `on_progress`.
    /// `params.eula` asserts the user accepted the Minecraft EULA; the job id
    /// is filled in here. The result carries any degraded outcome of the
    /// pipeline — a create can succeed with a best-effort step having failed.
    pub async fn create(
        &self,
        mut params: ServerCreateParams,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ServerCreateDoneEvent, IpcError> {
        let id = job_id("server-create");
        params.id = id.clone();

        let session = self.session;
        let payload = self
            .session
            .run_job(
                &id,
                "server.create.done",
                "server.create.error",
                forward(on_progress),
                move || async move { session.call::<ServerCreate>(&params).await.map(|_| ()) },
            )
            .await?;

        serde_json::from_value(payload).map_err(|e| IpcError::Malformed(e.to_string()))
    }

    /// Move a stopped server to another version, blocking until the daemon
    /// reports done or error and forwarding each progress event to
    /// `on_progress`. `params.allow_downgrade` asserts the user confirmed a
    /// downgrade; the job id is filled in here.
    pub async fn update(
        &self,
        mut params: ServerUpdateParams,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ServerUpdateDoneEvent, IpcError> {
        let id = job_id("server-update");
        params.id = id.clone();

        let session = self.session;
        let payload = self
            .session
            .run_job(
                &id,
                "server.update.done",
                "server.update.error",
                forward(on_progress),
                move || async move { session.call::<ServerUpdate>(&params).await.map(|_| ()) },
            )
            .await?;

        serde_json::from_value(payload).map_err(|e| IpcError::Malformed(e.to_string()))
    }

    pub async fn list(&self) -> Result<Vec<ServerInfo>, IpcError> {
        Ok(self
            .session
            .call::<ServerList>(&proto::Empty {})
            .await?
            .servers)
    }

    pub async fn status(&self, server: &str) -> Result<ServerInfo, IpcError> {
        self.session.call::<ServerStatus>(&server_ref(server)).await
    }

    /// The server's static, informational view (locations + disk footprint).
    pub async fn info(&self, server: &str) -> Result<ServerDetails, IpcError> {
        self.session.call::<ServerDetail>(&server_ref(server)).await
    }

    /// A Server List Ping status snapshot; only a running server answers.
    pub async fn ping(&self, server: &str) -> Result<ServerPingResult, IpcError> {
        self.session.call::<ServerPing>(&server_ref(server)).await
    }

    pub async fn remove(&self, server: &str) -> Result<(), IpcError> {
        self.session
            .call::<ServerRemove>(&server_ref(server))
            .await?;
        Ok(())
    }

    /// Rename a stopped server; the id (directory slug) is re-derived from the
    /// new name. Returns the updated record, whose `id` and `name` reflect the
    /// rename.
    pub async fn rename(&self, server: &str, name: &str) -> Result<ServerInfo, IpcError> {
        let params = ServerRenameParams {
            server: server.to_string(),
            name: name.to_string(),
        };
        self.session.call::<ServerRename>(&params).await
    }

    pub async fn start(&self, server: &str) -> Result<ServerStartResult, IpcError> {
        self.session.call::<ServerStart>(&server_ref(server)).await
    }

    pub async fn stop(&self, server: &str) -> Result<(), IpcError> {
        self.session.call::<ServerStop>(&server_ref(server)).await?;
        Ok(())
    }

    /// Stop and start a running server. The daemon answers once the new
    /// process is up, so the call outlasts its wait for the old one to exit.
    pub async fn restart(&self, server: &str) -> Result<ServerStartResult, IpcError> {
        self.session
            .call_with_timeout::<ServerRestart>(&server_ref(server), Duration::from_secs(150))
            .await
    }

    pub async fn logs(
        &self,
        server: &str,
        tail: Option<usize>,
    ) -> Result<Vec<ProcessLogLine>, IpcError> {
        let params = ServerLogsParams {
            server: server.to_string(),
            tail,
        };
        Ok(self.session.call::<ServerLogs>(&params).await?.lines)
    }

    /// Send one console command to a running server; returns its reply.
    pub async fn command(&self, server: &str, command: &str) -> Result<String, IpcError> {
        let params = ServerCommandParams {
            server: server.to_string(),
            command: command.to_string(),
        };
        Ok(self.session.call::<ServerCommand>(&params).await?.response)
    }

    /// What a half-typed console line could continue with on a running server.
    pub async fn complete(
        &self,
        server: &str,
        line: &str,
    ) -> Result<ServerCommandCompleteResult, IpcError> {
        let params = ServerCommandCompleteParams {
            server: server.to_string(),
            line: line.to_string(),
        };
        self.session.call::<ServerCommandComplete>(&params).await
    }

    /// Archive the server's data directory, blocking until the daemon reports
    /// done or error and forwarding each progress event to `on_progress`. A
    /// running server keeps running — its world saving pauses around the
    /// archive.
    pub async fn backup_create(
        &self,
        server: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<BackupInfo, IpcError> {
        let id = job_id("server-backup");
        let params = ServerBackupCreateParams {
            server: server.to_string(),
            id: id.clone(),
        };
        let session = self.session;
        run_backup_job(session, &id, on_progress, move || async move {
            session
                .call::<ServerBackupCreate>(&params)
                .await
                .map(|_| ())
        })
        .await
    }

    pub async fn backup_list(&self, server: &str) -> Result<Vec<BackupInfo>, IpcError> {
        Ok(self
            .session
            .call::<ServerBackupList>(&server_ref(server))
            .await?
            .backups)
    }

    /// Replace a stopped server's data directory with a backup's content,
    /// blocking until the daemon reports done or error and forwarding each
    /// progress event to `on_progress`.
    pub async fn backup_restore(
        &self,
        server: &str,
        backup: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<BackupInfo, IpcError> {
        let id = job_id("server-restore");
        let params = ServerBackupRestoreParams {
            server: server.to_string(),
            backup: backup.to_string(),
            id: id.clone(),
        };
        let session = self.session;
        run_backup_job(session, &id, on_progress, move || async move {
            session
                .call::<ServerBackupRestore>(&params)
                .await
                .map(|_| ())
        })
        .await
    }

    pub async fn backup_remove(&self, server: &str, backup: &str) -> Result<(), IpcError> {
        let params = ServerBackupRef {
            server: server.to_string(),
            backup: backup.to_string(),
        };
        self.session.call::<ServerBackupRemove>(&params).await?;
        Ok(())
    }

    /// Read one setting; `None` when it is not set (a `not_found` from the
    /// daemon).
    pub async fn config_get(&self, server: &str, key: &str) -> Result<Option<String>, IpcError> {
        let params = ServerConfigGetParams {
            server: server.to_string(),
            key: key.to_string(),
        };
        Ok(self
            .session
            .try_call::<ServerConfigGet>(&params)
            .await?
            .map(|r| r.value))
    }

    pub async fn config_set(&self, server: &str, key: &str, value: &str) -> Result<(), IpcError> {
        let params = ServerConfigSetParams {
            server: server.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        };
        self.session.call::<ServerConfigSet>(&params).await?;
        Ok(())
    }

    pub async fn config_list(&self, server: &str) -> Result<Vec<ConfigEntry>, IpcError> {
        Ok(self
            .session
            .call::<ServerConfigList>(&server_ref(server))
            .await?
            .entries)
    }

    /// Every `server.properties` key the server's version defines, with the
    /// value in effect.
    pub async fn properties(&self, server: &str) -> Result<Vec<ServerProperty>, IpcError> {
        Ok(self
            .session
            .call::<ServerProperties>(&server_ref(server))
            .await?
            .properties)
    }

    /// Install a batch of content into a server, blocking until the daemon
    /// reports done or error and forwarding each progress event to
    /// `on_progress`. Returns everything installed (items plus required
    /// dependencies) and, per item that could not be installed, a failure.
    pub async fn content_add(
        &self,
        server: &str,
        spec: ContentAddSpec,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>), IpcError> {
        let id = job_id("server-content-add");
        let params = ServerContentAddParams {
            server: server.to_string(),
            spec,
            id: id.clone(),
        };
        let session = self.session;
        run_content_job(session, &id, on_progress, move || async move {
            session.call::<ServerContentAdd>(&params).await.map(|_| ())
        })
        .await
    }

    pub async fn content_list(
        &self,
        server: &str,
        kind: ContentKind,
    ) -> Result<(Vec<InstalledContent>, Vec<UntrackedFile>), IpcError> {
        let params = ServerContentListParams {
            server: server.to_string(),
            kind,
        };
        let result = self.session.call::<ServerContentList>(&params).await?;
        Ok((result.items, result.untracked))
    }

    /// Uninstall the named items and every mirror of them, in one call. A
    /// non-empty `worlds` instead narrows a datapack to the save worlds it
    /// keeps loading in.
    pub async fn content_remove(
        &self,
        server: &str,
        kind: ContentKind,
        items: &[String],
        worlds: &[String],
    ) -> Result<(), IpcError> {
        let params = ServerContentRemoveParams {
            server: server.to_string(),
            kind,
            items: items.to_vec(),
            worlds: worlds.to_vec(),
        };
        self.session.call::<ServerContentRemove>(&params).await?;
        Ok(())
    }

    /// Update platform-sourced content to its newest compatible version — the
    /// named items in one job, or every item of the kind when `items` is empty.
    /// A non-empty `path` instead updates the one named local item from that file.
    pub async fn content_update(
        &self,
        server: &str,
        kind: ContentKind,
        items: &[String],
        path: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<Vec<InstalledContent>, IpcError> {
        let id = job_id("server-content-update");
        let params = ServerContentUpdateParams {
            server: server.to_string(),
            kind,
            items: items.to_vec(),
            path: path.to_string(),
            id: id.clone(),
        };
        let session = self.session;
        run_content_job(session, &id, on_progress, move || async move {
            session
                .call::<ServerContentUpdate>(&params)
                .await
                .map(|_| ())
        })
        .await
        .map(|(items, _)| items)
    }

    /// Enable or disable one installed item; a non-empty `worlds` scopes a
    /// datapack toggle to those save worlds.
    pub async fn content_enable(
        &self,
        server: &str,
        kind: ContentKind,
        item: &str,
        enabled: bool,
        worlds: &[String],
    ) -> Result<(), IpcError> {
        let params = ServerContentEnableParams {
            server: server.to_string(),
            kind,
            item: item.to_string(),
            enabled,
            worlds: worlds.to_vec(),
        };
        self.session.call::<ServerContentEnable>(&params).await?;
        Ok(())
    }

    /// Which platform-sourced items of the kind have a newer compatible version.
    pub async fn content_check_updates(
        &self,
        server: &str,
        kind: ContentKind,
    ) -> Result<Vec<ContentUpdate>, IpcError> {
        let params = ServerContentCheckUpdatesParams {
            server: server.to_string(),
            kind,
        };
        let result = self
            .session
            .call_with_timeout::<ServerContentCheckUpdates>(&params, Duration::from_secs(120))
            .await?;
        Ok(result.updates)
    }

    pub async fn content_matches(
        &self,
        server: &str,
        kind: ContentKind,
        item: &str,
    ) -> Result<Vec<ContentMatch>, IpcError> {
        let params = ServerContentMatchesParams {
            server: server.to_string(),
            kind,
            item: item.to_string(),
        };
        let result = self
            .session
            .call_with_timeout::<ServerContentMatches>(&params, Duration::from_secs(120))
            .await?;
        Ok(result.matches)
    }

    /// Re-pin one item to a specific published `version` (id or number).
    pub async fn content_set_version(
        &self,
        server: &str,
        kind: ContentKind,
        item: &str,
        version: &str,
        source: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<Vec<InstalledContent>, IpcError> {
        let id = job_id("server-content-set-version");
        let params = ServerContentSetVersionParams {
            server: server.to_string(),
            kind,
            item: item.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            id: id.clone(),
        };
        let session = self.session;
        run_content_job(session, &id, on_progress, move || async move {
            session
                .call::<ServerContentSetVersion>(&params)
                .await
                .map(|_| ())
        })
        .await
        .map(|(items, _)| items)
    }
}

fn server_ref(server: &str) -> ServerRef {
    ServerRef {
        server: server.to_string(),
    }
}
