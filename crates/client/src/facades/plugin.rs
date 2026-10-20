use ipc::errors::IpcError;
use proto::plugin::{PluginInfo, PluginRef};

use crate::session::Session;

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
}

fn reference(id: &str) -> PluginRef {
    PluginRef { id: id.to_string() }
}
