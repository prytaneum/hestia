use crate::bindings::hestia::plugin::types::{PluginError, Requirement, UpstreamFailure};

/// Why a call failed, whether the host's call to the plugin or the plugin's
/// call to the host. The host keeps the case, so return the one that says what
/// happened: the user is told what to do about it, and the host can attribute an
/// upstream failure to the provider that suffered it.
///
/// ```
/// let error = hestia::Error::NotConfigured("api-key".into());
/// assert_eq!(error.to_string(), "the setting `api-key` is not set");
/// ```
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nothing matches what was asked for.
    #[error("{0}")]
    NotFound(String),
    /// The request is malformed or not valid right now.
    #[error("{0}")]
    Invalid(String),
    /// The plugin lacks the permission or grant the call needs, or an http
    /// request was refused because the manifest did not grant its host.
    #[error("{0}")]
    Denied(String),
    /// The plugin does not implement what it was asked. The host falls back
    /// where the operation is optional, and fails the call where it is not.
    #[error("{0}")]
    Unsupported(String),
    /// A host could not be reached: no connection, no such name, or no answer in
    /// time. The detail names what was being reached.
    #[error("{}", .0.as_deref().unwrap_or("cannot reach the network"))]
    Offline(Option<String>),
    /// An upstream service answered with a failure.
    #[error("{message}")]
    Upstream {
        /// The HTTP status, when the failure had one.
        status: Option<u16>,
        /// Seconds the service asked to wait before trying again.
        retry_after: Option<u32>,
        message: String,
    },
    /// An upstream service asked to slow down; seconds to wait, when known.
    #[error("rate limited")]
    RateLimited { retry_after: Option<u32> },
    /// An account has to be signed in before the call can succeed.
    #[error("sign-in is required")]
    SignInRequired,
    /// A setting the call needs has no value; the string is its key.
    #[error("the setting `{0}` is not set")]
    NotConfigured(String),
    /// Something the call needs is not installed on this computer.
    #[error("{name} is not installed")]
    MissingRequirement {
        name: String,
        /// Where to get it, when known.
        url: Option<String>,
    },
    /// Another operation holds what the call needs; try again later.
    #[error("{0}")]
    Busy(String),
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
            PluginError::NotFound(message) => Error::NotFound(message),
            PluginError::Invalid(message) => Error::Invalid(message),
            PluginError::Denied(message) => Error::Denied(message),
            PluginError::Unsupported(message) => Error::Unsupported(message),
            PluginError::Offline(detail) => Error::Offline(detail),
            PluginError::Upstream(failure) => Error::Upstream {
                status: failure.status,
                retry_after: failure.retry_after,
                message: failure.message,
            },
            PluginError::RateLimited(retry_after) => Error::RateLimited { retry_after },
            PluginError::SignInRequired => Error::SignInRequired,
            PluginError::NotConfigured(key) => Error::NotConfigured(key),
            PluginError::MissingRequirement(requirement) => Error::MissingRequirement {
                name: requirement.name,
                url: requirement.url,
            },
            PluginError::Busy(message) => Error::Busy(message),
            PluginError::Failed(message) => Error::Failed(message),
        }
    }
}

impl From<Error> for PluginError {
    fn from(error: Error) -> Self {
        match error {
            Error::NotFound(message) => PluginError::NotFound(message),
            Error::Invalid(message) => PluginError::Invalid(message),
            Error::Denied(message) => PluginError::Denied(message),
            Error::Unsupported(message) => PluginError::Unsupported(message),
            Error::Offline(detail) => PluginError::Offline(detail),
            Error::Upstream {
                status,
                retry_after,
                message,
            } => PluginError::Upstream(UpstreamFailure {
                status,
                retry_after,
                message,
            }),
            Error::RateLimited { retry_after } => PluginError::RateLimited(retry_after),
            Error::SignInRequired => PluginError::SignInRequired,
            Error::NotConfigured(key) => PluginError::NotConfigured(key),
            Error::MissingRequirement { name, url } => {
                PluginError::MissingRequirement(Requirement { name, url })
            }
            Error::Busy(message) => PluginError::Busy(message),
            Error::Failed(message) => PluginError::Failed(message),
            other @ (Error::Json(_) | Error::Io(_)) => PluginError::Failed(other.to_string()),
        }
    }
}
