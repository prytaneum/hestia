use proto::error::ErrorInfo;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("no plugin '{0}'")]
    NotFound(String),
    #[error("plugin '{plugin}' is required by {}", dependents.join(", "))]
    Required {
        plugin: String,
        dependents: Vec<String>,
    },
    #[error("plugin '{0}' has work running")]
    Busy(String),
    #[error("plugin '{plugin}' failed to enable: {source}")]
    Failed {
        plugin: String,
        source: anyhow::Error,
    },
    #[error("invalid built-in set: {0}")]
    Invalid(String),
    #[error("cannot save plugin state: {0}")]
    Save(common::schema::SchemaError),
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
            other => ErrorInfo::Internal {
                detail: other.to_string(),
            },
        }
    }
}
