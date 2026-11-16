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
}
