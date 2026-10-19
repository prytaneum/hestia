//! `job.*` — the jobs the daemon is running.

use proto::job::{JobCancel, JobCancelParams, JobInfo, JobList};

use crate::{IpcError, Session};

pub struct Jobs<'a> {
    pub(crate) session: &'a Session,
}

impl Jobs<'_> {
    pub async fn list(&self) -> Result<Vec<JobInfo>, IpcError> {
        self.session.call::<JobList>(&proto::Empty {}).await
    }

    /// Ask the daemon to cancel a running job by the id its events carry.
    /// `false` means it was already over — a normal race, not an error.
    ///
    /// Cancelling is always explicit: a disconnecting client never cancels
    /// anything, because a job outlives the client that started it.
    pub async fn cancel(&self, id: &str) -> Result<bool, IpcError> {
        let params = JobCancelParams { id: id.to_string() };
        Ok(self.session.call::<JobCancel>(&params).await?.cancelled)
    }
}
