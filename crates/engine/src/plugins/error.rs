//! The one typed error between host and plugin. [`GuestError`] mirrors the WIT
//! `plugin-error` in Rust, so nothing outside the sandbox sees the generated
//! type. [`ErrorInfo`] maps down into it for a plugin's host calls, and it maps
//! up into [`ErrorInfo`] when a plugin answers the host
//! ([0115](../../../../docs/decisions/0115-a-plugin-error-is-its-own-typed-enum.md)).

use proto::error::{ErrorInfo, Service};

use super::download::MAX_DOWNLOAD_BYTES;
use super::imports::bindings::hestia::plugin::types::{PluginError, Requirement, UpstreamFailure};
use super::ipc::IpcError;
use super::lock::LockError;
use super::process::ProgramError;
use super::vouch::Refused;
use crate::cancel::is_cancelled;
use crate::download::Fault;

/// Why a call between the host and a plugin failed, with the case kept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuestError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Denied(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{}", .0.as_deref().unwrap_or("cannot reach the network"))]
    Offline(Option<String>),
    #[error("{}", upstream_text(*status, *retry_after, message))]
    Upstream {
        status: Option<u16>,
        retry_after: Option<u32>,
        message: String,
    },
    #[error("{}", rate_limited_text(*.0))]
    RateLimited(Option<u32>),
    #[error("sign-in is required")]
    SignInRequired,
    #[error("the setting `{0}` is not set")]
    NotConfigured(String),
    #[error("{}", requirement_text(name, url.as_deref()))]
    MissingRequirement { name: String, url: Option<String> },
    #[error("{0}")]
    Busy(String),
    #[error("{0}")]
    Failed(String),
}

fn upstream_text(status: Option<u16>, retry_after: Option<u32>, message: &str) -> String {
    let mut text = match (message.is_empty(), status) {
        (false, _) => message.to_string(),
        (true, Some(status)) => format!("the service answered {status}"),
        (true, None) => "the service failed".to_string(),
    };
    if let Some(seconds) = retry_after {
        text.push_str(&format!(" (retry in {seconds}s)"));
    }
    text
}

fn rate_limited_text(retry_after: Option<u32>) -> String {
    match retry_after {
        Some(seconds) => format!("the service is rate limiting requests; retry in {seconds}s"),
        None => "the service is rate limiting requests".to_string(),
    }
}

fn requirement_text(name: &str, url: Option<&str>) -> String {
    match url {
        Some(url) => format!("{name} is not installed; get it from {url}"),
        None => format!("{name} is not installed"),
    }
}

impl GuestError {
    /// The case's name, for a log line that must not carry its message.
    pub fn kind(&self) -> &'static str {
        match self {
            GuestError::NotFound(_) => "not-found",
            GuestError::Invalid(_) => "invalid",
            GuestError::Denied(_) => "denied",
            GuestError::Unsupported(_) => "unsupported",
            GuestError::Offline(_) => "offline",
            GuestError::Upstream { .. } => "upstream",
            GuestError::RateLimited(_) => "rate-limited",
            GuestError::SignInRequired => "sign-in-required",
            GuestError::NotConfigured(_) => "not-configured",
            GuestError::MissingRequirement { .. } => "missing-requirement",
            GuestError::Busy(_) => "busy",
            GuestError::Failed(_) => "failed",
        }
    }

    /// What a plugin is told of a failed engine download: a refusal of the
    /// grant is `denied`, a missing file `not-found`, any other status
    /// `upstream` with the status, no route `offline`, and a body over the
    /// cap `invalid`. Anything else is `failed` without the engine's own
    /// message, which may name host paths.
    pub(super) fn fetch(error: &anyhow::Error) -> Self {
        if let Some(refused) = error.downcast_ref::<Refused>() {
            return GuestError::Denied(refused.to_string());
        }
        match error.downcast_ref::<Fault>() {
            Some(Fault::Status { status: 404, .. }) => {
                return GuestError::NotFound("the server has no such file".into())
            }
            Some(Fault::Status { status, .. }) => {
                return GuestError::Upstream {
                    status: Some(*status),
                    retry_after: None,
                    message: format!("the server answered {status}"),
                }
            }
            Some(Fault::TooLarge { .. }) => {
                return GuestError::Invalid(format!(
                    "the file is larger than the {MAX_DOWNLOAD_BYTES} bytes a download may be"
                ))
            }
            None => {}
        }
        if is_cancelled(error) {
            return GuestError::Failed("the call was cancelled".into());
        }
        match error.downcast_ref::<ErrorInfo>() {
            Some(info @ (ErrorInfo::Offline { .. } | ErrorInfo::OfflineMode)) => {
                GuestError::from(info)
            }
            Some(ErrorInfo::DownloadFailed { detail }) => GuestError::Failed(detail.clone()),
            _ => GuestError::Failed("the download could not be completed".into()),
        }
    }

    /// What the host tells its own caller about a plugin's failure. `service`
    /// is the provider the plugin answered for, when it answered for one: only
    /// then can an upstream failure be attributed to something the user knows.
    pub fn into_info(self, plugin: &str, service: Option<Service>) -> ErrorInfo {
        let detail = self.to_string();
        match (self, service) {
            (GuestError::Offline(_), service) => ErrorInfo::Offline { service },
            (GuestError::Upstream { .. } | GuestError::RateLimited(_), Some(service)) => {
                ErrorInfo::Upstream { service, detail }
            }
            (GuestError::SignInRequired, _) => ErrorInfo::SignInRequired,
            (GuestError::NotConfigured(key), _) => ErrorInfo::PluginSettingInvalid {
                plugin: plugin.to_string(),
                key,
                detail: "it has no value".to_string(),
            },
            _ => ErrorInfo::PluginCallFailed {
                id: plugin.to_string(),
                detail,
            },
        }
    }
}

impl From<PluginError> for GuestError {
    fn from(error: PluginError) -> Self {
        match error {
            PluginError::NotFound(message) => GuestError::NotFound(message),
            PluginError::Invalid(message) => GuestError::Invalid(message),
            PluginError::Denied(message) => GuestError::Denied(message),
            PluginError::Unsupported(message) => GuestError::Unsupported(message),
            PluginError::Offline(detail) => GuestError::Offline(detail),
            PluginError::Upstream(failure) => GuestError::Upstream {
                status: failure.status,
                retry_after: failure.retry_after,
                message: failure.message,
            },
            PluginError::RateLimited(retry_after) => GuestError::RateLimited(retry_after),
            PluginError::SignInRequired => GuestError::SignInRequired,
            PluginError::NotConfigured(key) => GuestError::NotConfigured(key),
            PluginError::MissingRequirement(requirement) => GuestError::MissingRequirement {
                name: requirement.name,
                url: requirement.url,
            },
            PluginError::Busy(message) => GuestError::Busy(message),
            PluginError::Failed(message) => GuestError::Failed(message),
        }
    }
}

impl From<GuestError> for PluginError {
    fn from(error: GuestError) -> Self {
        match error {
            GuestError::NotFound(message) => PluginError::NotFound(message),
            GuestError::Invalid(message) => PluginError::Invalid(message),
            GuestError::Denied(message) => PluginError::Denied(message),
            GuestError::Unsupported(message) => PluginError::Unsupported(message),
            GuestError::Offline(detail) => PluginError::Offline(detail),
            GuestError::Upstream {
                status,
                retry_after,
                message,
            } => PluginError::Upstream(UpstreamFailure {
                status,
                retry_after,
                message,
            }),
            GuestError::RateLimited(retry_after) => PluginError::RateLimited(retry_after),
            GuestError::SignInRequired => PluginError::SignInRequired,
            GuestError::NotConfigured(key) => PluginError::NotConfigured(key),
            GuestError::MissingRequirement { name, url } => {
                PluginError::MissingRequirement(Requirement { name, url })
            }
            GuestError::Busy(message) => PluginError::Busy(message),
            GuestError::Failed(message) => PluginError::Failed(message),
        }
    }
}

impl From<ProgramError> for GuestError {
    fn from(error: ProgramError) -> Self {
        match error {
            ProgramError::Denied(message) => GuestError::Denied(message),
            ProgramError::NotFound(message) => GuestError::NotFound(message),
            ProgramError::Invalid(message) => GuestError::Invalid(message),
            ProgramError::Missing { name, url } => GuestError::MissingRequirement { name, url },
            ProgramError::Unapproved(_) => GuestError::Denied(error.to_string()),
            ProgramError::Failed(message) => GuestError::Failed(message),
        }
    }
}

impl From<IpcError> for GuestError {
    fn from(error: IpcError) -> Self {
        match error {
            IpcError::Denied(message) => GuestError::Denied(message),
            IpcError::NotFound(message) => GuestError::NotFound(message),
            IpcError::Invalid(message) => GuestError::Invalid(message),
            IpcError::Failed(message) => GuestError::Failed(message),
        }
    }
}

impl From<LockError> for GuestError {
    fn from(error: LockError) -> Self {
        match error {
            LockError::Invalid(message) => GuestError::Invalid(message),
            LockError::Busy(message) => GuestError::Busy(message),
        }
    }
}

impl From<&ErrorInfo> for GuestError {
    fn from(info: &ErrorInfo) -> Self {
        use ErrorInfo::*;
        let message = info.to_string();
        match info {
            EntryNotFound { .. }
            | ProcessNotFound { .. }
            | BackupNotFound { .. }
            | ContentNotFound { .. }
            | SkinNotFound { .. }
            | WorldNotFound { .. }
            | ServerListEntryNotFound { .. }
            | AccountNotFound { .. }
            | VersionNotFound { .. }
            | TaskNotFound { .. }
            | PluginNotFound { .. }
            | RepositoryNotFound { .. }
            | FileNotFound { .. }
            | ConfigKeyUnknown { .. }
            | UnknownChannel { .. } => GuestError::NotFound(message),

            FieldRequired { .. }
            | FieldsRequired { .. }
            | InvalidValue { .. }
            | MutuallyExclusive { .. }
            | NothingToDo { .. }
            | EulaRequired
            | TextureNotPng
            | TextureSizeUnsupported { .. }
            | IconNotAnImage
            | IconTooLarge { .. }
            | FileRejected { .. }
            | ConfigTypeMismatch { .. }
            | ConfigRejected { .. }
            | ConfigInvalid { .. }
            | HardcoreLocked { .. }
            | HardcoreRequires { .. }
            | AlreadyExists { .. }
            | PortUnavailable { .. }
            | NotRunning { .. }
            | JavaProviderUnknown { .. }
            | NotAModpack { .. }
            | ModpackInvalid { .. }
            | ModpackEntryMismatch { .. }
            | ModpackDifferentPack { .. }
            | ModpackNotInstalled { .. }
            | ArchiveUnrecognised { .. }
            | ArchiveInvalid { .. }
            | ContentKindMismatch { .. }
            | SyncSourceRequired { .. }
            | PluginInvalid { .. }
            | PluginCommandInvalid { .. }
            | PluginPageInvalid { .. }
            | PluginSettingInvalid { .. }
            | RepositoryInvalid { .. }
            | MalformedRequest { .. } => GuestError::Invalid(message),

            DeveloperModeOff | ElevationRequired { .. } | ConfigKeyDenied { .. } => {
                GuestError::Denied(message)
            }

            UnsupportedOperation { .. }
            | ContentKindRejected { .. }
            | QuickPlayUnsupported { .. }
            | ModpackLoaderUnsupported { .. }
            | UnsupportedContentUrl { .. }
            | ArchiveUnsupported { .. }
            | ArchiveTooNew { .. }
            | MultiSessionDisabled { .. }
            | SyncUnitDisabled { .. }
            | IncompatibleVersion { .. } => GuestError::Unsupported(message),

            Offline { .. } | OfflineMode => GuestError::Offline(Some(message)),

            Upstream { .. } => GuestError::Upstream {
                status: None,
                retry_after: None,
                message,
            },

            SignInRequired | SessionExpired { .. } | LoginDeclined | LoginTimedOut => {
                GuestError::SignInRequired
            }

            ConfigKeyUnset { key } => GuestError::NotConfigured(key.clone()),

            MissingRequirement { name, url, .. } => GuestError::MissingRequirement {
                name: name.clone(),
                url: Some(url.clone()),
            },

            EntryRunning { .. }
            | Provisioning { .. }
            | UpdateInProgress { .. }
            | ContentInProgress { .. }
            | BackupInProgress { .. }
            | ExportInProgress { .. }
            | CreateInProgress
            | WorkRunning
            | JavaInstallInProgress { .. }
            | PluginInstallInProgress { .. } => GuestError::Busy(message),

            PluginApprovalStale
            | PluginUpdateNeedsApproval { .. }
            | UpdatePluginsIncompatible { .. }
            | UpdatePluginsNeedApproval { .. }
            | RepositoryKeyChanged { .. }
            | RepositoryRefused { .. }
            | RepositoryExpired { .. }
            | RepositoryOfficial { .. }
            | PluginBuildUnavailable { .. }
            | PluginUnavailable { .. }
            | PluginCallFailed { .. }
            | PluginUiInvalid { .. }
            | PluginVetoed { .. }
            | StopTimedOut { .. }
            | ContentSourceUnavailable { .. }
            | ContentDownloadBlocked { .. }
            | Io { .. }
            | DownloadFailed { .. }
            | Internal { .. } => GuestError::Failed(message),
        }
    }
}

#[cfg(test)]
mod tests;
