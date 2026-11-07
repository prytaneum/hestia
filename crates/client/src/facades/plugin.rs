use ipc::errors::IpcError;
use proto::catalogue::{
    Catalogue, CatalogueParams, RepositoryAddParams, RepositoryAdded, RepositoryInfo,
    RepositoryRef, RepositoryRefreshParams, RepositoryRefreshed,
};
use proto::plugin::{
    PluginAssetParams, PluginAssetResult, PluginInfo, PluginInspection, PluginInstallDone,
    PluginInstallParams, PluginInstallProgress, PluginLogEvent, PluginPackageRef, PluginPage,
    PluginRef, PluginRunDone, PluginRunParams, PluginRunProgress, PluginSettingsSetParams,
    PluginStateEvent, PluginUiActionParams, PluginUiRenderParams, SettingValue,
};
use proto::ui::{Effect, UiNode};
use proto::Topic;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::session::{job_id, Session};

/// What happens to a followed dev plugin.
#[derive(Debug, Clone)]
pub enum DevEvent {
    Log(PluginLogEvent),
    State(PluginStateEvent),
}

pub struct Plugin<'a> {
    pub(crate) session: &'a Session,
}

impl Plugin<'_> {
    pub async fn list(&self) -> Result<Vec<PluginInfo>, IpcError> {
        self.session
            .call::<proto::plugin::PluginList>(&proto::Empty {})
            .await
    }

    pub async fn info(&self, id: &str) -> Result<PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginGet>(&reference(id))
            .await
    }

    pub async fn settings(&self, id: &str) -> Result<Vec<SettingValue>, IpcError> {
        self.session
            .call::<proto::plugin::PluginSettingsGet>(&reference(id))
            .await
    }

    /// `null` clears the setting back to its default.
    pub async fn set_setting(
        &self,
        id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), IpcError> {
        self.session
            .call::<proto::plugin::PluginSettingsSet>(&PluginSettingsSetParams {
                id: id.to_string(),
                key: key.to_string(),
                value,
            })
            .await?;
        Ok(())
    }

    pub async fn asset(&self, plugin: &str, path: &str) -> Result<PluginAssetResult, IpcError> {
        self.session
            .call::<proto::plugin::PluginAsset>(&PluginAssetParams {
                plugin: plugin.to_string(),
                path: path.to_string(),
            })
            .await
    }

    pub async fn enable(&self, id: &str) -> Result<PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginEnable>(&reference(id))
            .await
    }

    pub async fn disable(&self, id: &str) -> Result<PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginDisable>(&reference(id))
            .await
    }

    pub async fn remove(&self, id: &str) -> Result<(), IpcError> {
        self.session
            .call::<proto::plugin::PluginRemove>(&reference(id))
            .await?;
        Ok(())
    }

    /// `path` must be absolute: the daemon resolves it.
    pub async fn inspect(&self, path: &str) -> Result<PluginInspection, IpcError> {
        let params = PluginPackageRef {
            path: path.to_string(),
            ..PluginPackageRef::default()
        };
        self.session
            .call::<proto::plugin::PluginInspect>(&params)
            .await
    }

    /// The daemon downloads an https `url`; install the result with
    /// [`Self::install_staged`] using [`PluginInspection::staged`].
    pub async fn inspect_url(&self, url: &str) -> Result<PluginInspection, IpcError> {
        let params = PluginPackageRef {
            url: url.to_string(),
            ..PluginPackageRef::default()
        };
        self.session
            .call::<proto::plugin::PluginInspect>(&params)
            .await
    }

    /// The daemon downloads the newest compatible build of `plugin` from the
    /// repository at `repo` and verifies it against the repository's pinned
    /// key; install the result with [`Self::install_staged`].
    pub async fn inspect_repo(
        &self,
        repo: &str,
        plugin: &str,
    ) -> Result<PluginInspection, IpcError> {
        let params = PluginPackageRef {
            repo: repo.to_string(),
            plugin: plugin.to_string(),
            ..PluginPackageRef::default()
        };
        self.session
            .call::<proto::plugin::PluginInspect>(&params)
            .await
    }

    /// Fetches the index, verifies it and shows what would be trusted. With
    /// `confirm` it also saves the repository.
    pub async fn repo_add(&self, url: &str, confirm: bool) -> Result<RepositoryAdded, IpcError> {
        let params = RepositoryAddParams {
            url: url.to_string(),
            confirm,
        };
        self.session
            .call::<proto::catalogue::PluginRepoAdd>(&params)
            .await
    }

    pub async fn repo_list(&self) -> Result<Vec<RepositoryInfo>, IpcError> {
        self.session
            .call::<proto::catalogue::PluginRepoList>(&proto::Empty {})
            .await
    }

    pub async fn repo_remove(&self, url: &str) -> Result<(), IpcError> {
        let params = RepositoryRef {
            url: url.to_string(),
        };
        self.session
            .call::<proto::catalogue::PluginRepoRemove>(&params)
            .await?;
        Ok(())
    }

    /// One repository, or every configured one for `None`.
    pub async fn repo_refresh(&self, url: Option<&str>) -> Result<RepositoryRefreshed, IpcError> {
        let params = RepositoryRefreshParams {
            url: url.map(str::to_string),
        };
        self.session
            .call::<proto::catalogue::PluginRepoRefresh>(&params)
            .await
    }

    pub async fn catalogue(&self, params: CatalogueParams) -> Result<Catalogue, IpcError> {
        self.session
            .call::<proto::catalogue::PluginCatalogue>(&params)
            .await
    }

    /// `approve` is the grant digest from [`Self::inspect`] the user agreed to.
    pub async fn install(
        &self,
        path: &str,
        approve: &str,
        on_progress: impl Fn(&PluginInstallProgress) + Send + Sync + 'static,
    ) -> Result<PluginInstallDone, IpcError> {
        let params = PluginInstallParams {
            path: path.to_string(),
            approve: approve.to_string(),
            ..PluginInstallParams::default()
        };
        self.run_install(params, on_progress).await
    }

    /// Runs the folder at `path` in place and reloads it when it changes;
    /// `approve` is the grant digest from [`Self::inspect`].
    pub async fn install_dev(
        &self,
        path: &str,
        approve: &str,
    ) -> Result<PluginInstallDone, IpcError> {
        let params = PluginInstallParams {
            path: path.to_string(),
            approve: approve.to_string(),
            dev: true,
            ..PluginInstallParams::default()
        };
        self.run_install(params, |_| {}).await
    }

    /// Streams the log lines and state changes of plugin `id` until the
    /// receiver drops or the connection closes.
    pub async fn follow(&self, id: &str) -> Result<UnboundedReceiver<DevEvent>, IpcError> {
        let mut listener = self.session.listen_subscribed(Some(id)).await?;
        let (tx, rx) = unbounded_channel();
        let id = id.to_string();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = tx.closed() => break,
                    event = listener.recv() => {
                        let Some(event) = event else { break };
                        let forwarded = match event.topic.as_str() {
                            PluginLogEvent::TOPIC => serde_json::from_value(event.payload)
                                .ok()
                                .filter(|e: &PluginLogEvent| e.id == id)
                                .map(DevEvent::Log),
                            PluginStateEvent::TOPIC => serde_json::from_value(event.payload)
                                .ok()
                                .filter(|e: &PluginStateEvent| e.id == id)
                                .map(DevEvent::State),
                            _ => None,
                        };
                        if let Some(forwarded) = forwarded {
                            let _ = tx.send(forwarded);
                        }
                    }
                }
            }
        });
        Ok(rx)
    }

    /// Installs the file the daemon downloaded for an [`Self::inspect_url`].
    pub async fn install_staged(
        &self,
        staged: &str,
        approve: &str,
        on_progress: impl Fn(&PluginInstallProgress) + Send + Sync + 'static,
    ) -> Result<PluginInstallDone, IpcError> {
        let params = PluginInstallParams {
            staged: staged.to_string(),
            approve: approve.to_string(),
            ..PluginInstallParams::default()
        };
        self.run_install(params, on_progress).await
    }

    /// Runs a command the plugin contributes and waits for its output. The
    /// caller fills `plugin`, `command`, `entry` (for a scoped command) and `args`.
    pub async fn run(
        &self,
        mut params: PluginRunParams,
        on_progress: impl Fn(&PluginRunProgress) + Send + Sync + 'static,
    ) -> Result<PluginRunDone, IpcError> {
        params.id = job_id("plugin-run");
        self.session
            .run_job::<proto::plugin::PluginRun>(&params.id.clone(), &params, on_progress)
            .await
    }

    pub async fn pages(&self) -> Result<Vec<PluginPage>, IpcError> {
        self.session
            .call::<proto::plugin::PluginPages>(&proto::Empty {})
            .await
    }

    pub async fn render(&self, params: &PluginUiRenderParams) -> Result<UiNode, IpcError> {
        self.session
            .call::<proto::plugin::PluginUiRender>(params)
            .await
    }

    pub async fn action(&self, params: &PluginUiActionParams) -> Result<Vec<Effect>, IpcError> {
        self.session
            .call::<proto::plugin::PluginUiAction>(params)
            .await
    }

    async fn run_install(
        &self,
        mut params: PluginInstallParams,
        on_progress: impl Fn(&PluginInstallProgress) + Send + Sync + 'static,
    ) -> Result<PluginInstallDone, IpcError> {
        params.id = job_id("plugin-install");
        self.session
            .run_job::<proto::plugin::PluginInstall>(&params.id.clone(), &params, on_progress)
            .await
    }
}

fn reference(id: &str) -> PluginRef {
    PluginRef { id: id.to_string() }
}
