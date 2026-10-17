use std::path::{Path, PathBuf};

use ipc::errors::IpcError;

use crate::session::{job_id, Session};

pub struct Update<'a> {
    pub(crate) session: &'a Session,
}

impl Update<'_> {
    pub async fn check(&self) -> Result<proto::update::UpdateCheckResult, IpcError> {
        self.session
            .call::<proto::update::UpdateCheck>(&proto::Empty {})
            .await
    }

    /// Resolves to the staged path and the version it carries.
    pub async fn download(
        &self,
        on_progress: impl Fn(&proto::download::DownloadProgress) + Send + Sync + 'static,
    ) -> Result<(PathBuf, String), IpcError> {
        use proto::update::{UpdateDownload, UpdateDownloadParams};

        let id = job_id("update");
        let params = UpdateDownloadParams { id: id.clone() };
        let done = self
            .session
            .run_job::<UpdateDownload>(&id, &params, on_progress)
            .await?;
        Ok((done.path, done.version))
    }

    pub async fn apply(&self, path: &Path) -> Result<proto::update::UpdateApplyResult, IpcError> {
        self.session
            .call::<proto::update::UpdateApply>(&proto::update::UpdateApplyParams {
                path: path.to_path_buf(),
            })
            .await
    }
}
