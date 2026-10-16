//! The error codes a client branches on. The daemon's codes are produced by
//! `ErrorInfo::code()` in `proto`, which cannot depend on this crate.

pub const NOT_FOUND: &str = "not_found";
pub const UNAUTHORIZED: &str = "unauthorized";

use thiserror::Error;

/// A transport- or protocol-level failure surfaced to callers of the client SDK.
#[derive(Debug, Error)]
pub enum IpcError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame too large: {0} bytes")]
    FrameTooLarge(u32),
    #[error("malformed frame: {0}")]
    Malformed(String),
    #[error("daemon connection lost")]
    ConnectionLost,
    #[error(
        "incompatible protocol version: the daemon speaks version {got}, this build speaks {want}"
    )]
    IncompatibleVersion { got: i64, want: i64 },
    #[error("timed out waiting for daemon response on '{0}'")]
    Timeout(String),
    /// The job was cancelled at someone's request. Not a failure — nothing went
    /// wrong — so a front-end reports it as such rather than as an error.
    #[error("cancelled")]
    Cancelled,
    #[error("{message}")]
    Daemon {
        code: String,
        message: String,
        /// The raw serialized `proto::error::ErrorInfo` for structured consumers
        /// (the desktop forwards it to the webview to localize); `code` and
        /// `message` are the client's local projection of it.
        info: serde_json::Value,
    },
}
