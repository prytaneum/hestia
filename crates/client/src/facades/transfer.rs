//! Instance import and export. Its own facade rather than more methods on
//! `Instance` for the same reason `Modpack` is: the channels are `instance.*`,
//! but the concern is not the instance store.

use ipc::errors::IpcError;
use proto::content::ContentFailure;
use proto::instance::InstanceInfo;
use proto::minecraft::ProvisionProgress;
use proto::transfer::{
    ArchiveInfo, ArchiveRef, ExportFormat, ImportFormat, InstanceExport, InstanceExportParams,
    InstanceImport, InstanceImportInspect, InstanceImportParams,
};
use proto::warning::WarningInfo;
use serde_json::Value;

use crate::facades::jobs::forward;
use crate::session::{job_id, Session};

/// What a finished export came to.
pub struct Exported {
    pub path: String,
    pub size_bytes: u64,
    pub files: u64,
    pub warnings: Vec<WarningInfo>,
}

/// What a finished import came to.
pub struct Imported {
    pub format: ImportFormat,
    pub instance: InstanceInfo,
    pub failures: Vec<ContentFailure>,
    pub warnings: Vec<WarningInfo>,
}

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
    ) -> Result<Exported, IpcError> {
        let id = job_id("instance-export");
        let session = self.session;
        let params = InstanceExportParams {
            instance: instance.to_string(),
            format,
            destination: destination.to_string(),
            exclude,
            id: id.clone(),
        };
        let payload = self
            .session
            .run_job(
                &id,
                "instance.export.done",
                "instance.export.error",
                forward(on_progress),
                move || async move { session.call::<InstanceExport>(&params).await.map(|_| ()) },
            )
            .await?;
        Ok(Exported {
            path: string_field(&payload, "path"),
            size_bytes: u64_field(&payload, "sizeBytes"),
            files: u64_field(&payload, "files"),
            warnings: decode(&payload, "warnings"),
        })
    }

    /// Import an archive as a new instance, blocking until it lands. `name`
    /// overrides the one the archive carries; the path must be absolute.
    pub async fn import(
        &self,
        path: &str,
        name: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<Imported, IpcError> {
        let id = job_id("instance-import");
        let session = self.session;
        let params = InstanceImportParams {
            path: path.to_string(),
            name: name.to_string(),
            id: id.clone(),
        };
        let payload = self
            .session
            .run_job(
                &id,
                "instance.import.done",
                "instance.import.error",
                forward(on_progress),
                move || async move { session.call::<InstanceImport>(&params).await.map(|_| ()) },
            )
            .await?;
        let instance =
            serde_json::from_value(payload.get("instance").cloned().unwrap_or(Value::Null))
                .map_err(|e| IpcError::Malformed(e.to_string()))?;
        Ok(Imported {
            format: decode(&payload, "format"),
            instance,
            failures: decode(&payload, "failures"),
            warnings: decode(&payload, "warnings"),
        })
    }
}

/// A best-effort field of a done event. The instance itself is decoded
/// strictly (an import that cannot say what it created is a malformed answer);
/// everything else here is descriptive and a missing one is not worth failing
/// a job that already succeeded over.
fn decode<T: serde::de::DeserializeOwned + Default>(payload: &Value, key: &str) -> T {
    payload
        .get(key)
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn string_field(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn u64_field(payload: &Value, key: &str) -> u64 {
    payload.get(key).and_then(Value::as_u64).unwrap_or_default()
}
