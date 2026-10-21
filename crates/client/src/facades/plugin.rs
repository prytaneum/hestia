use ipc::errors::IpcError;
use proto::plugin::{
    PluginInfo, PluginInspection, PluginInstallDone, PluginInstallParams, PluginInstallProgress,
    PluginPackageRef, PluginRef,
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
        let id = job_id("plugin-install");
        let params = PluginInstallParams {
            id: id.clone(),
            path: path.to_string(),
            approve: approve.to_string(),
        };
        self.session
            .run_job::<proto::plugin::PluginInstall>(&id, &params, on_progress)
            .await
    }
}

fn reference(id: &str) -> PluginRef {
    PluginRef { id: id.to_string() }
}
