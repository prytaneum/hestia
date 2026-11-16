use ipc::errors::IpcError;

use crate::session::Session;

pub struct Daemon<'a> {
    pub(crate) session: &'a Session,
}

impl Daemon<'_> {
    pub async fn status(&self) -> Result<proto::daemon::DaemonStatusResult, IpcError> {
        self.session
            .call::<proto::daemon::DaemonStatus>(&proto::Empty {})
            .await
    }

    /// Without `stop_processes` the supervised workloads keep running and the
    /// next daemon re-adopts them.
    pub async fn stop(
        &self,
        stop_processes: bool,
    ) -> Result<proto::daemon::DaemonStopResult, IpcError> {
        self.session
            .call::<proto::daemon::DaemonStop>(&proto::daemon::DaemonStopParams { stop_processes })
            .await
    }
}
