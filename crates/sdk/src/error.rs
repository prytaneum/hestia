use crate::bindings::hestia::plugin::types::PluginError;

/// Why a plugin's call failed, or why the plugin itself fails a call.
///
/// ```
/// let error = hestia::Error::Failed("the tunnel is not configured".into());
/// assert_eq!(error.to_string(), "the tunnel is not configured");
/// ```
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The plugin lacks the permission the call needs.
    #[error("{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Failed(String),
    /// A payload did not encode, or a reply did not decode as its contract says.
    #[error("payload: {0}")]
    Json(#[from] serde_json::Error),
    /// A [`data`](crate::data) file could not be read or written.
    #[error("data: {0}")]
    Io(#[from] std::io::Error),
}

impl From<PluginError> for Error {
    fn from(error: PluginError) -> Self {
        match error {
            PluginError::PermissionDenied(message) => Error::PermissionDenied(message),
            PluginError::NotFound(message) => Error::NotFound(message),
            PluginError::Invalid(message) => Error::Invalid(message),
            PluginError::Failed(message) => Error::Failed(message),
        }
    }
}

impl From<Error> for PluginError {
    fn from(error: Error) -> Self {
        match error {
            Error::PermissionDenied(message) => PluginError::PermissionDenied(message),
            Error::NotFound(message) => PluginError::NotFound(message),
            Error::Invalid(message) => PluginError::Invalid(message),
            Error::Failed(message) => PluginError::Failed(message),
            other => PluginError::Failed(other.to_string()),
        }
    }
}
