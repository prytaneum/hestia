//! `job.list` — the jobs the daemon is running.

use proto::job::{JobInfo, JobList};

use crate::{IpcError, Session};

pub struct Jobs<'a> {
    pub(crate) session: &'a Session,
}

impl Jobs<'_> {
    pub async fn list(&self) -> Result<Vec<JobInfo>, IpcError> {
        self.session.call::<JobList>(&proto::Empty {}).await
    }
}
