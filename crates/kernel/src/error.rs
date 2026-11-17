use proto::error::{ErrorInfo, IoOp};

use crate::grant::GrantError;
use crate::manifest::ManifestError;
use crate::runtime::{SandboxError, ShapeError};
use crate::trust::TrustError;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("no plugin '{0}'")]
    NotFound(String),
    #[error("plugin '{plugin}' is required by {}", dependents.join(", "))]
    Required {
        plugin: String,
        dependents: Vec<String>,
    },
    #[error("its version {version} is outside what {} accept", dependents.join(", "))]
    Rejected {
        version: String,
        dependents: Vec<String>,
    },
    #[error("plugin '{plugin}' needs '{dependency}' {range}, which is not installed at a version it accepts")]
    MissingDependency {
        plugin: String,
        dependency: String,
        range: String,
    },
    #[error("plugin '{0}' has work running")]
    Busy(String),
    #[error("plugin '{plugin}' failed to enable: {source}")]
    Failed {
        plugin: String,
        source: anyhow::Error,
    },
    #[error("cannot install the plugin: {0}")]
    Package(#[from] PackageError),
    #[error("plugin '{0}' is already installed")]
    Exists(String),
    #[error("plugin '{0}' changed since it was approved")]
    Changed(String),
    #[error("a plugin installed from a directory needs developer mode: `hestia config set plugins.developer true`")]
    DeveloperMode,
    #[error(transparent)]
    Trust(#[from] TrustError),
    #[error("no repository '{0}'")]
    UnknownRepository(String),
    #[error("'{0}' is not a valid id: use lowercase letters, digits and '-'")]
    InvalidId(String),
    #[error("repository '{0}' is already added")]
    RepositoryExists(String),
    #[error("repository '{repository}' lists no release of '{id}'")]
    NoRelease { repository: String, id: String },
    #[error("plugin '{0}' was not installed from a repository")]
    NotFromRepository(String),
    #[error("plugin '{0}' is up to date")]
    UpToDate(String),
    #[error("plugin '{0}' is built in and cannot be removed")]
    Builtin(String),
    #[error("cannot store the secret: {0}")]
    Secret(String),
    #[error("WASM plugins are unavailable: the plugin sandbox did not start")]
    NoSandbox,
    #[error("could not {operation} plugin files: {detail}")]
    Io { operation: IoOp, detail: String },
    #[error("plugin '{0}' is not enabled")]
    NotEnabled(String),
    #[error("plugin '{plugin}' has no command '{command}'")]
    NoCommand { plugin: String, command: String },
    #[error("{0}")]
    Arguments(String),
    #[error("plugin '{plugin}': {source}")]
    Call {
        plugin: String,
        source: SandboxError,
    },
    #[error("invalid built-in set: {0}")]
    Invalid(String),
    #[error("cannot save plugin state: {0}")]
    Save(common::schema::SchemaError),
}

/// Why a package cannot be installed or loaded.
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("{0} is not a regular file")]
    NotAFile(String),
    #[error("{file} is larger than {max} bytes")]
    TooLarge { file: String, max: u64 },
    #[error("cannot read {file}: {detail}")]
    Unreadable { file: String, detail: String },
    #[error("plugin.toml is not UTF-8")]
    NotUtf8,
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Shape(#[from] ShapeError),
    #[error(transparent)]
    Grant(#[from] GrantError),
    #[error("its manifest names '{0}'")]
    Renamed(String),
    #[error("its dependencies loop back through {0}")]
    DependencyCycle(String),
    #[error("its dependencies are nested more than {0} deep")]
    DependencyDepth(usize),
    #[error("its dependencies bring more than {0} plugins")]
    TooManyDependencies(usize),
    #[error("it needs {0}, which no enabled plugin provides")]
    Unprovided(String),
    #[error(transparent)]
    Load(#[from] SandboxError),
}

impl PluginError {
    pub(crate) fn io(operation: IoOp, error: impl std::fmt::Display) -> Self {
        PluginError::Io {
            operation,
            detail: error.to_string(),
        }
    }
}

impl From<PluginError> for ErrorInfo {
    fn from(error: PluginError) -> Self {
        match error {
            PluginError::NotFound(plugin) => ErrorInfo::PluginNotFound { plugin },
            PluginError::Required { plugin, dependents } => {
                ErrorInfo::PluginRequired { plugin, dependents }
            }
            PluginError::Busy(plugin) => ErrorInfo::PluginBusy { plugin },
            PluginError::Failed { plugin, source } => ErrorInfo::PluginFailed {
                plugin,
                detail: format!("{source:#}"),
            },
            PluginError::Package(reason) => ErrorInfo::PluginInvalid {
                detail: reason.to_string(),
            },
            PluginError::MissingDependency {
                plugin,
                dependency,
                range,
            } => ErrorInfo::PluginMissingDependency {
                plugin,
                dependency,
                range,
            },
            rejected @ PluginError::Rejected { .. } => ErrorInfo::PluginInvalid {
                detail: rejected.to_string(),
            },
            PluginError::NotEnabled(plugin) => ErrorInfo::PluginDisabled { plugin },
            PluginError::NoCommand { plugin, command } => ErrorInfo::PluginInvalid {
                detail: format!("plugin '{plugin}' has no command '{command}'"),
            },
            PluginError::Arguments(detail) => ErrorInfo::PluginInvalid { detail },
            PluginError::Call { plugin, source } => ErrorInfo::PluginFailed {
                plugin,
                detail: source.to_string(),
            },
            PluginError::Exists(plugin) => ErrorInfo::PluginExists { plugin },
            PluginError::Changed(plugin) => ErrorInfo::PluginChanged { plugin },
            PluginError::Builtin(plugin) => ErrorInfo::PluginBuiltin { plugin },
            PluginError::Io { operation, detail } => ErrorInfo::Io { operation, detail },
            PluginError::Trust(TrustError::Fetch(detail)) => ErrorInfo::DownloadFailed { detail },
            PluginError::Trust(reason) => ErrorInfo::PluginUntrusted {
                detail: reason.to_string(),
            },
            PluginError::UnknownRepository(repository) => {
                ErrorInfo::PluginRepositoryNotFound { repository }
            }
            PluginError::RepositoryExists(repository) => {
                ErrorInfo::PluginRepositoryExists { repository }
            }
            PluginError::NoRelease { repository, id } => ErrorInfo::PluginReleaseNotFound {
                repository,
                plugin: id,
            },
            PluginError::NotFromRepository(plugin) => ErrorInfo::PluginNotFromRepository { plugin },
            PluginError::UpToDate(plugin) => ErrorInfo::PluginUpToDate { plugin },
            PluginError::InvalidId(id) => ErrorInfo::PluginIdInvalid { id },
            PluginError::DeveloperMode => ErrorInfo::PluginDeveloperMode,
            PluginError::Secret(detail) => ErrorInfo::MalformedRequest { detail },
            other => ErrorInfo::Internal {
                detail: other.to_string(),
            },
        }
    }
}
