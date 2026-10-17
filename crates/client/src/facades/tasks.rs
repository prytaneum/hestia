//! `server.task.*` — what a server runs on a schedule.

use proto::server::ServerRef;
use proto::tasks::{
    ServerTask, ServerTaskList, ServerTaskRemove, ServerTaskRemoveParams, ServerTaskSave,
    ServerTaskSaveParams,
};

use crate::{IpcError, Session};

pub struct Tasks<'a> {
    pub(crate) session: &'a Session,
}

impl Tasks<'_> {
    pub async fn list(&self, server: &str) -> Result<Vec<ServerTask>, IpcError> {
        self.session
            .call::<ServerTaskList>(&ServerRef {
                server: server.to_string(),
            })
            .await
    }

    /// Create the task when its id is empty, replace the one it names
    /// otherwise.
    pub async fn save(&self, server: &str, task: ServerTask) -> Result<ServerTask, IpcError> {
        let params = ServerTaskSaveParams {
            server: server.to_string(),
            task,
        };
        self.session.call::<ServerTaskSave>(&params).await
    }

    pub async fn remove(&self, server: &str, id: &str) -> Result<(), IpcError> {
        let params = ServerTaskRemoveParams {
            server: server.to_string(),
            id: id.to_string(),
        };
        self.session.call::<ServerTaskRemove>(&params).await?;
        Ok(())
    }
}
