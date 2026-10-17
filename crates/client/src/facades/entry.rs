use ipc::errors::IpcError;
use proto::entry::{
    EntryConfigGet, EntryConfigGetParams, EntryConfigList, EntryConfigSet, EntryConfigSetParams,
    EntryFlavors, EntryFlavorsParams, EntryLoaders, EntryLoadersParams, EntryLogs, EntryLogsParams,
    EntryRemove, EntryRename, EntryRenameParams, EntryRenamed, EntryStop, EntryStopParams,
    EntryVersions, EntryVersionsParams,
};
use proto::error::{EntryKind, EntryTarget};
use proto::minecraft::{ConfigEntry, Flavor, GameVersion};
use proto::process::ProcessLogLine;

use crate::session::Session;

pub struct Entry<'a> {
    pub(crate) session: &'a Session,
}

impl Entry<'_> {
    pub async fn flavors(&self, kind: EntryKind) -> Result<Vec<Flavor>, IpcError> {
        let params = EntryFlavorsParams { kind };
        Ok(self.session.call::<EntryFlavors>(&params).await?.flavors)
    }

    pub async fn versions(
        &self,
        kind: EntryKind,
        flavor: &str,
    ) -> Result<Vec<GameVersion>, IpcError> {
        let params = EntryVersionsParams {
            kind,
            flavor: flavor.to_string(),
        };
        self.session.call::<EntryVersions>(&params).await
    }

    pub async fn loaders(
        &self,
        kind: EntryKind,
        flavor: &str,
        version: &str,
    ) -> Result<Vec<String>, IpcError> {
        let params = EntryLoadersParams {
            kind,
            flavor: flavor.to_string(),
            version: version.to_string(),
        };
        self.session.call::<EntryLoaders>(&params).await
    }

    pub async fn remove(&self, target: &EntryTarget) -> Result<(), IpcError> {
        self.session.call::<EntryRemove>(target).await?;
        Ok(())
    }

    /// Rename a stopped entry; the id (directory slug) is re-derived from the
    /// new name.
    pub async fn rename(&self, target: &EntryTarget, name: &str) -> Result<EntryRenamed, IpcError> {
        let params = EntryRenameParams {
            target: target.clone(),
            name: name.to_string(),
        };
        self.session.call::<EntryRename>(&params).await
    }

    pub async fn stop(
        &self,
        target: &EntryTarget,
        session: Option<String>,
    ) -> Result<(), IpcError> {
        let params = EntryStopParams {
            target: target.clone(),
            session,
        };
        self.session.call::<EntryStop>(&params).await?;
        Ok(())
    }

    pub async fn logs(
        &self,
        target: &EntryTarget,
        session: Option<String>,
        tail: Option<usize>,
    ) -> Result<Vec<ProcessLogLine>, IpcError> {
        let params = EntryLogsParams {
            target: target.clone(),
            session,
            tail,
        };
        self.session.call::<EntryLogs>(&params).await
    }

    /// Read one setting; `None` when it is not set (a `not_found` from the
    /// daemon).
    pub async fn config_get(
        &self,
        target: &EntryTarget,
        key: &str,
    ) -> Result<Option<String>, IpcError> {
        let params = EntryConfigGetParams {
            target: target.clone(),
            key: key.to_string(),
        };
        Ok(self
            .session
            .try_call::<EntryConfigGet>(&params)
            .await?
            .map(|r| r.value))
    }

    pub async fn config_set(
        &self,
        target: &EntryTarget,
        key: &str,
        value: &str,
    ) -> Result<(), IpcError> {
        let params = EntryConfigSetParams {
            target: target.clone(),
            key: key.to_string(),
            value: value.to_string(),
        };
        self.session.call::<EntryConfigSet>(&params).await?;
        Ok(())
    }

    pub async fn config_list(&self, target: &EntryTarget) -> Result<Vec<ConfigEntry>, IpcError> {
        Ok(self.session.call::<EntryConfigList>(target).await?.entries)
    }
}
