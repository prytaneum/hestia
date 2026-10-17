use crate::session::{job_id, Session};
use ipc::errors::IpcError;
use proto::backup::{
    BackupInfo, ServerBackupCreate, ServerBackupCreateParams, ServerBackupList, ServerBackupRef,
    ServerBackupRemove, ServerBackupRestore, ServerBackupRestoreParams,
};
use proto::minecraft::{
    ConfigEntry, Flavor, GameVersion, LoadersParams, ProvisionProgress, VersionsParams,
};
use proto::process::ProcessLogLine;
use proto::server::{
    ServerCommand, ServerCommandComplete, ServerCommandCompleteParams, ServerCommandCompleteResult,
    ServerCommandParams, ServerConfigGet, ServerConfigGetParams, ServerConfigList, ServerConfigSet,
    ServerConfigSetParams, ServerCreate, ServerCreateParams, ServerDetail, ServerDetails,
    ServerDone, ServerFlavors, ServerInfo, ServerList, ServerLoaders, ServerLogs, ServerLogsParams,
    ServerPing, ServerPingResult, ServerProperties, ServerProperty, ServerRef, ServerRemove,
    ServerRename, ServerRenameParams, ServerRestart, ServerStart, ServerStartResult, ServerStatus,
    ServerStop, ServerUpdate, ServerUpdateParams, ServerVersions,
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
        self.session.call::<ServerVersions>(&params).await
    }

    pub async fn loaders(&self, flavor: &str, version: &str) -> Result<Vec<String>, IpcError> {
        let params = LoadersParams {
            flavor: flavor.to_string(),
            version: version.to_string(),
        };
        self.session.call::<ServerLoaders>(&params).await
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
    ) -> Result<ServerDone, IpcError> {
        let id = job_id("server-create");
        params.id = id.clone();
        self.session
            .run_job::<ServerCreate>(&id, &params, on_progress)
            .await
    }

    /// Move a stopped server to another version, blocking until the daemon
    /// reports done or error and forwarding each progress event to
    /// `on_progress`. `params.allow_downgrade` asserts the user confirmed a
    /// downgrade; the job id is filled in here.
    pub async fn update(
        &self,
        mut params: ServerUpdateParams,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ServerDone, IpcError> {
        let id = job_id("server-update");
        params.id = id.clone();
        self.session
            .run_job::<ServerUpdate>(&id, &params, on_progress)
            .await
    }

    pub async fn list(&self) -> Result<Vec<ServerInfo>, IpcError> {
        self.session.call::<ServerList>(&proto::Empty {}).await
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
            .call::<ServerRestart>(&server_ref(server))
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
        self.session.call::<ServerLogs>(&params).await
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
        Ok(self
            .session
            .run_job::<ServerBackupCreate>(&id, &params, on_progress)
            .await?
            .backup)
    }

    pub async fn backup_list(&self, server: &str) -> Result<Vec<BackupInfo>, IpcError> {
        self.session
            .call::<ServerBackupList>(&server_ref(server))
            .await
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
        Ok(self
            .session
            .run_job::<ServerBackupRestore>(&id, &params, on_progress)
            .await?
            .backup)
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
        self.session
            .call::<ServerProperties>(&server_ref(server))
            .await
    }
}

fn server_ref(server: &str) -> ServerRef {
    ServerRef {
        server: server.to_string(),
    }
}
