use ipc::errors::IpcError;

use crate::session::Session;

pub struct Plugin<'a> {
    pub(crate) session: &'a Session,
}

impl Plugin<'_> {
    pub async fn list(&self) -> Result<Vec<proto::plugin::PluginInfo>, IpcError> {
        self.session
            .call::<proto::plugin::PluginList>(&proto::Empty {})
            .await
    }

    pub async fn enable(&self, id: &str) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginEnable>(&proto::plugin::PluginTarget { id: id.into() })
            .await
    }

    pub async fn disable(&self, id: &str) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginDisable>(&proto::plugin::PluginTarget { id: id.into() })
            .await
    }

    pub async fn inspect(&self, path: &str) -> Result<proto::plugin::Inspection, IpcError> {
        self.session
            .call::<proto::plugin::PluginInspect>(&proto::plugin::PluginPackage {
                path: path.into(),
            })
            .await
    }

    pub async fn install(
        &self,
        path: &str,
        approval_digest: &str,
        choice: proto::plugin::Choice,
    ) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginInstall>(&proto::plugin::PluginInstallParams {
                path: path.into(),
                approval_digest: approval_digest.into(),
                choice,
            })
            .await
    }

    pub async fn set_permission(
        &self,
        id: &str,
        permission: &str,
        granted: bool,
    ) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginPermission>(&proto::plugin::PluginPermissionParams {
                id: id.into(),
                permission: permission.into(),
                granted,
            })
            .await
    }

    pub async fn remove(&self, id: &str) -> Result<(), IpcError> {
        self.session
            .call::<proto::plugin::PluginRemove>(&proto::plugin::PluginTarget { id: id.into() })
            .await
            .map(|_| ())
    }

    pub async fn set_secret(&self, id: &str, key: &str, value: &str) -> Result<(), IpcError> {
        self.session
            .call::<proto::plugin::PluginSecretSet>(&proto::plugin::PluginSecretParams {
                id: id.into(),
                key: key.into(),
                value: value.into(),
            })
            .await
            .map(|_| ())
    }

    pub async fn repositories(&self) -> Result<Vec<proto::plugin::RepositoryInfo>, IpcError> {
        self.session
            .call::<proto::plugin::PluginRepoList>(&proto::Empty {})
            .await
    }

    pub async fn add_repository(
        &self,
        id: &str,
        url: &str,
        key: &str,
    ) -> Result<proto::plugin::RepositoryInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginRepoAdd>(&proto::plugin::PluginRepoAddParams {
                id: id.into(),
                url: url.into(),
                key: key.into(),
            })
            .await
    }

    pub async fn remove_repository(&self, id: &str) -> Result<(), IpcError> {
        self.session
            .call::<proto::plugin::PluginRepoRemove>(&proto::plugin::PluginTarget { id: id.into() })
            .await
            .map(|_| ())
    }

    /// Runs one of a plugin's commands as a job under the caller's `id`,
    /// drawing what it shows through `on_plugin`; blocks until it settles.
    pub async fn run(
        &self,
        id: &str,
        plugin: &str,
        command: &str,
        args: &str,
        on_progress: impl Fn(&proto::plugin::PluginRunProgress) + Send + Sync + 'static,
        on_plugin: impl Fn(&ipc::protocol::Event) + Send + Sync + 'static,
    ) -> Result<(), IpcError> {
        let params = proto::plugin::PluginRunParams {
            id: id.into(),
            plugin: plugin.into(),
            command: command.into(),
            args: args.into(),
        };
        self.session
            .run_job_drawing::<proto::plugin::PluginRun>(id, &params, on_progress, on_plugin)
            .await
            .map(|_| ())
    }

    pub async fn inspect_release(
        &self,
        release: &proto::plugin::PluginReleaseParams,
    ) -> Result<proto::plugin::Inspection, IpcError> {
        self.session
            .call::<proto::plugin::PluginInspectRelease>(release)
            .await
    }

    pub async fn install_release(
        &self,
        release: proto::plugin::PluginReleaseParams,
        approval_digest: &str,
        choice: proto::plugin::Choice,
    ) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginInstallRelease>(
                &proto::plugin::PluginInstallReleaseParams {
                    release,
                    approval_digest: approval_digest.into(),
                    choice,
                },
            )
            .await
    }

    pub async fn updates(&self) -> Result<proto::plugin::PluginUpdatesResult, IpcError> {
        self.session
            .call::<proto::plugin::PluginUpdates>(&proto::Empty {})
            .await
    }

    pub async fn inspect_update(
        &self,
        id: &str,
    ) -> Result<proto::plugin::UpdateInspection, IpcError> {
        self.session
            .call::<proto::plugin::PluginUpdateInspect>(&proto::plugin::PluginTarget {
                id: id.into(),
            })
            .await
    }

    pub async fn update(
        &self,
        id: &str,
        approval_digest: &str,
        entries: Option<proto::plugin::EntryScope>,
    ) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginUpdateApply>(&proto::plugin::PluginUpdateApplyParams {
                id: id.into(),
                approval_digest: approval_digest.into(),
                entries,
            })
            .await
    }

    /// Replaces the entries `id` may reach; they are named by id or name.
    pub async fn set_entries(
        &self,
        id: &str,
        entries: proto::plugin::EntryScope,
    ) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginEntries>(&proto::plugin::PluginEntriesParams {
                id: id.into(),
                entries,
            })
            .await
    }

    /// Answers a question a plugin asked; the first answer wins.
    pub async fn answer(
        &self,
        request: &str,
        plugin: &str,
        accepted: bool,
    ) -> Result<(), IpcError> {
        self.session
            .call::<proto::plugin::PluginAnswer>(&proto::plugin::PluginAnswerParams {
                request: request.into(),
                plugin: plugin.into(),
                accepted,
            })
            .await
            .map(|_| ())
    }
}
