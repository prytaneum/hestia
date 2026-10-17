use std::collections::BTreeSet;

use ipc::errors::IpcError;

use crate::session::Session;

pub struct Sync<'a> {
    pub(crate) session: &'a Session,
}

impl Sync<'_> {
    pub async fn get(&self) -> Result<proto::sync::SyncConfig, IpcError> {
        self.session
            .call::<proto::sync::SyncGet>(&proto::Empty {})
            .await
    }

    pub async fn sources(
        &self,
        unit: proto::sync::SyncUnit,
    ) -> Result<Vec<proto::sync::SyncSource>, IpcError> {
        let params = proto::sync::SyncSourcesParams { unit };
        self.session.call::<proto::sync::SyncSources>(&params).await
    }

    /// `source` may be empty only when at most one instance holds the file.
    pub async fn enable(
        &self,
        unit: proto::sync::SyncUnit,
        source: &str,
    ) -> Result<proto::sync::SyncConfig, IpcError> {
        let params = proto::sync::SyncEnableParams {
            unit,
            source: source.to_string(),
        };
        self.session.call::<proto::sync::SyncEnable>(&params).await
    }

    pub async fn disable(
        &self,
        unit: proto::sync::SyncUnit,
    ) -> Result<proto::sync::SyncConfig, IpcError> {
        let params = proto::sync::SyncDisableParams { unit };
        self.session.call::<proto::sync::SyncDisable>(&params).await
    }

    pub async fn set_unsynced(
        &self,
        unsynced: BTreeSet<String>,
    ) -> Result<proto::sync::SyncConfig, IpcError> {
        let params = proto::sync::SyncKeysParams { unsynced };
        self.session.call::<proto::sync::SyncKeys>(&params).await
    }

    pub async fn options(&self) -> Result<Vec<proto::sync::SyncOption>, IpcError> {
        Ok(self
            .session
            .call::<proto::sync::SyncOptionsGet>(&proto::Empty {})
            .await?
            .options)
    }

    pub async fn set_option(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Vec<proto::sync::SyncOption>, IpcError> {
        let params = proto::sync::SyncOptionSetParams {
            key: key.to_string(),
            value: value.to_string(),
        };
        Ok(self
            .session
            .call::<proto::sync::SyncOptionSet>(&params)
            .await?
            .options)
    }

    pub async fn packs(&self) -> Result<Vec<proto::sync::SharedPack>, IpcError> {
        Ok(self
            .session
            .call::<proto::sync::SyncPacks>(&proto::Empty {})
            .await?
            .packs)
    }

    pub async fn set_pack(
        &self,
        pack: &str,
        enabled: bool,
    ) -> Result<Vec<proto::sync::SharedPack>, IpcError> {
        Ok(self
            .session
            .call::<proto::sync::SyncPackSet>(&proto::sync::SyncPackSetParams {
                pack: pack.to_string(),
                enabled,
            })
            .await?
            .packs)
    }

    pub async fn remove_pack(&self, pack: &str) -> Result<Vec<proto::sync::SharedPack>, IpcError> {
        Ok(self
            .session
            .call::<proto::sync::SyncPackRemove>(&proto::sync::SyncPackRemoveParams {
                pack: pack.to_string(),
            })
            .await?
            .packs)
    }

    pub async fn status(&self) -> Result<Vec<proto::sync::InstanceSyncStatus>, IpcError> {
        self.session
            .call::<proto::sync::SyncStatus>(&proto::Empty {})
            .await
    }

    /// `shared` unset returns the instance to following the catalogue.
    pub async fn set_instance_unit(
        &self,
        instance: &str,
        unit: proto::sync::SyncUnit,
        shared: Option<bool>,
    ) -> Result<proto::sync::InstanceSyncStatus, IpcError> {
        let params = proto::sync::InstanceSyncUnitParams {
            instance: instance.to_string(),
            unit,
            shared,
        };
        self.session
            .call::<proto::sync::InstanceSyncUnit>(&params)
            .await
    }

    pub async fn set_instance_unsynced(
        &self,
        instance: &str,
        unsynced: BTreeSet<String>,
    ) -> Result<proto::sync::InstanceSyncStatus, IpcError> {
        let params = proto::sync::InstanceSyncKeysParams {
            instance: instance.to_string(),
            unsynced,
        };
        self.session
            .call::<proto::sync::InstanceSyncKeys>(&params)
            .await
    }
}
