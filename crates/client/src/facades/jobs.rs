//! Job drivers shared between facades: the four backup job types publish the
//! same `backup.*` topics (and the content jobs the same `content.*` topics),
//! disambiguated by job id.

use ipc::errors::IpcError;
use ipc::protocol::Event;
use proto::backup::BackupInfo;
use proto::content::{ContentFailure, InstalledContent};
use proto::minecraft::ProvisionProgress;
use serde_json::Value;

use crate::session::Session;

/// Drive one backup/restore job: forward its progress events and decode the
/// done event's backup.
pub(super) async fn run_backup_job<F, Fut>(
    session: &Session,
    id: &str,
    on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    start: F,
) -> Result<BackupInfo, IpcError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), IpcError>>,
{
    let payload = session
        .run_job(
            id,
            "backup.done",
            "backup.error",
            forward(on_progress),
            start,
        )
        .await?;
    serde_json::from_value(payload.get("backup").cloned().unwrap_or(Value::Null))
        .map_err(|e| IpcError::Malformed(e.to_string()))
}

/// Drive one content install/update job: forward its progress events and decode
/// the done event's installed items and per-item failures.
pub(super) async fn run_content_job<F, Fut>(
    session: &Session,
    id: &str,
    on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    start: F,
) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>), IpcError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), IpcError>>,
{
    let payload = session
        .run_job(
            id,
            "content.done",
            "content.error",
            forward(on_progress),
            start,
        )
        .await?;
    let items = serde_json::from_value(payload.get("items").cloned().unwrap_or(Value::Null))
        .map_err(|e| IpcError::Malformed(e.to_string()))?;
    let failures = serde_json::from_value(payload.get("failures").cloned().unwrap_or(Value::Null))
        .unwrap_or_default();
    Ok((items, failures))
}

/// Adapt a `ProvisionProgress` callback into the session's raw event callback,
/// ignoring events that do not decode as progress.
pub(super) fn forward(
    on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
) -> impl Fn(&Event) + Send + Sync + 'static {
    move |event: &Event| {
        if let Ok(progress) = serde_json::from_value::<ProvisionProgress>(event.payload.clone()) {
            on_progress(&progress);
        }
    }
}
