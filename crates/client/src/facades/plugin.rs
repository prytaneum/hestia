use ipc::errors::IpcError;
use proto::plugin::{
    PluginAssetParams, PluginAssetResult, PluginInfo, PluginInspection, PluginInstallDone,
    PluginInstallParams, PluginInstallProgress, PluginPackageRef, PluginRef, PluginRunDone,
    PluginRunParams, PluginRunProgress,
};

use crate::session::{job_id, Session};

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
