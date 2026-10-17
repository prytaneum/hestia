//! Instance import and export. Its own facade rather than more methods on
//! `Instance` for the same reason `Modpack` is: the channels are `instance.*`,
//! but the concern is not the instance store.

use ipc::errors::IpcError;
use proto::minecraft::ProvisionProgress;
use proto::transfer::{
    ArchiveInfo, ArchiveRef, ExportDone, ExportFormat, ImportDone, InstanceExport,
    InstanceExportParams, InstanceImport, InstanceImportInspect, InstanceImportParams,
};

use crate::session::{job_id, Session};

pub struct Transfer<'a> {
    pub(crate) session: &'a Session,
}

impl Transfer<'_> {
    /// What an archive is, without importing it.
    pub async fn inspect(&self, path: &str) -> Result<ArchiveInfo, IpcError> {
        let params = ArchiveRef {
            path: path.to_string(),
        };
        self.session.call::<InstanceImportInspect>(&params).await
    }

    /// Write an instance out as an archive, blocking until it is written.
    /// `destination` may be a file, a directory, or empty for the daemon's own
    /// `exports/` — and must be **absolute**, since the daemon resolves it.
    pub async fn export(
        &self,
        instance: &str,
        format: ExportFormat,
        destination: &str,
        exclude: Vec<String>,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ExportDone, IpcError> {
        let id = job_id("instance-export");
        let params = InstanceExportParams {
            instance: instance.to_string(),
            format,
            destination: destination.to_string(),
            exclude,
            id: id.clone(),
        };
        self.session
            .run_job::<InstanceExport>(&id, &params, on_progress)
            .await
    }

    /// Import an archive as a new instance, blocking until it lands. `name`
    /// overrides the one the archive carries; the path must be absolute.
    pub async fn import(
        &self,
        path: &str,
        name: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<ImportDone, IpcError> {
        let id = job_id("instance-import");
        let params = InstanceImportParams {
            path: path.to_string(),
            name: name.to_string(),
            id: id.clone(),
        };
        self.session
            .run_job::<InstanceImport>(&id, &params, on_progress)
            .await
    }
}
