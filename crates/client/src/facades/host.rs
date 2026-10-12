use ipc::errors::IpcError;

use crate::session::Session;

pub struct Host<'a> {
    pub(crate) session: &'a Session,
}

impl Host<'_> {
    pub async fn info(&self) -> Result<proto::host::HostInfoResult, IpcError> {
        self.session
            .call::<proto::host::HostInfo>(&proto::Empty {})
            .await
    }
}
