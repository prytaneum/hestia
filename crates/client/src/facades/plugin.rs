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
    ) -> Result<proto::plugin::PluginInfo, IpcError> {
        self.session
            .call::<proto::plugin::PluginInstall>(&proto::plugin::PluginInstallParams {
                path: path.into(),
                approval_digest: approval_digest.into(),
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
}
