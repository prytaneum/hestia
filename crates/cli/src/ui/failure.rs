//! What a failed command tells the person: the error itself and, when there is
//! one, the next step. The daemon's `ErrorInfo` derives its own hint from its
//! variant; the CLI adds the one failure the daemon cannot report, its own
//! being down.

use std::fmt;

use client::proto::error::ErrorInfo;
use client::IpcError;
use serde_json::Value;

/// A command needed the daemon and it was not there. Carried as the context of
/// the connection failure so the hint is derived from the type, not the text.
#[derive(Debug)]
pub struct DaemonNotRunning;

impl fmt::Display for DaemonNotRunning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the daemon is not running")
    }
}

/// The `code` and raw `ErrorInfo` of a daemon failure anywhere in the chain.
pub fn daemon_error(error: &anyhow::Error) -> Option<(&str, &Value)> {
    error.chain().find_map(|cause| match cause.downcast_ref() {
        Some(IpcError::Daemon { code, info, .. }) => Some((code.as_str(), info)),
        _ => None,
    })
}

pub fn hint(error: &anyhow::Error) -> Option<String> {
    if let Some((_, info)) = daemon_error(error) {
        return serde_json::from_value::<ErrorInfo>(info.clone())
            .ok()?
            .hint();
    }
    error
        .downcast_ref::<DaemonNotRunning>()
        .map(|_| "start it with `hestia daemon start`".to_string())
}

pub fn print(error: &anyhow::Error) {
    eprintln!("{error:#}");
    if let Some(hint) = hint(error) {
        eprintln!("  {hint}");
    }
}
