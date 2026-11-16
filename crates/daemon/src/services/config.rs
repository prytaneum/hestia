//! The typed settings store, plus the reserved `home` and `autostart` keys the
//! daemon routes to the path pointer and the login registration.

use std::path::Path;

use engine::ConfigError;
use proto::change::{ChangedEvent, Scope};
use proto::config::{
    ConfigGet, ConfigGetResult, ConfigList, ConfigListResult, ConfigSet, AUTOSTART_KEY, HOME_KEY,
};
use proto::error::{ErrorInfo, ValueKind};
use proto::Empty;
use serde_json::{json, Value};

use crate::autostart;
use crate::runtime::Channels;

const DEVELOPER_KEY: &str = "plugins.developer";

fn config_err(e: ConfigError) -> ErrorInfo {
    match e {
        ConfigError::UnknownKey(key) => ErrorInfo::ConfigKeyUnknown { key },
        ConfigError::TypeMismatch { key, expected } => {
            ErrorInfo::ConfigTypeMismatch { key, expected }
        }
        ConfigError::InvalidValue { key, source } => {
            tracing::warn!(key, error = %source, "config value rejected");
            ErrorInfo::ConfigRejected { key }
        }
        ConfigError::Rejected(info) => info,
        ConfigError::Io(e) => ErrorInfo::Internal {
            detail: e.to_string(),
        },
        ConfigError::Save(e) => ErrorInfo::Internal {
            detail: format!("{e:#}"),
        },
    }
}

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ConfigGet, _, _>(|p, ctx| async move {
        if p.key == HOME_KEY {
            return Ok(ConfigGetResult {
                value: json!(ctx.runtime.engine().data_home().display().to_string()),
            });
        }
        if p.key == AUTOSTART_KEY {
            return Ok(ConfigGetResult {
                value: json!(autostart::is_enabled()),
            });
        }
        let value = ctx
            .runtime
            .engine()
            .config()
            .get(&p.key)
            .map_err(config_err)?;
        Ok(ConfigGetResult { value })
    });

    on.handle::<ConfigSet, _, _>(|p, ctx| async move {
        if p.key == HOME_KEY {
            let Value::String(dir) = p.value else {
                return Err(ErrorInfo::ConfigTypeMismatch {
                    key: HOME_KEY.into(),
                    expected: ValueKind::String,
                });
            };
            if ctx.runtime.processes().any_running() || ctx.runtime.cancellations().any_running() {
                return Err(ErrorInfo::WorkRunning);
            }
            let target = Path::new(&dir);
            if !dir.is_empty() {
                if !target.is_absolute() {
                    return Err(ErrorInfo::ConfigRejected {
                        key: HOME_KEY.into(),
                    });
                }
                std::fs::create_dir_all(target).map_err(|e| ErrorInfo::Internal {
                    detail: e.to_string(),
                })?;
            }
            common::paths::set_persisted_home(target).map_err(|e| ErrorInfo::Internal {
                detail: e.to_string(),
            })?;
            tracing::info!(home = %dir, "data home changed, stopping the daemon");
            ctx.runtime.stop_soon(false);
            return Ok(Empty {});
        }
        if p.key == AUTOSTART_KEY {
            let Value::Bool(enabled) = p.value else {
                return Err(ErrorInfo::ConfigTypeMismatch {
                    key: AUTOSTART_KEY.into(),
                    expected: ValueKind::Boolean,
                });
            };
            autostart::set(enabled).map_err(crate::runtime::engine_error)?;
            return Ok(Empty {});
        }
        let key = p.key;
        ctx.runtime
            .engine()
            .set_config(&key, p.value)
            .map_err(config_err)?;
        tracing::info!(key = %key, "config updated");
        if key == DEVELOPER_KEY {
            let developer = ctx.runtime.engine().config().settings().plugins.developer;
            let changed = ctx.runtime.plugins().set_developer(developer).await;
            if !changed.is_empty() {
                ctx.runtime.hub().publish_topic(&ChangedEvent {
                    scope: Scope::Plugins,
                    ids: changed,
                });
            }
        }
        Ok(Empty {})
    });

    on.handle::<ConfigList, _, _>(|_: Empty, ctx| async move {
        let mut entries = ctx.runtime.engine().config().all();
        if let Value::Object(map) = &mut entries {
            map.insert(
                HOME_KEY.into(),
                json!(ctx.runtime.engine().data_home().display().to_string()),
            );
            map.insert(AUTOSTART_KEY.into(), json!(autostart::is_enabled()));
        }
        Ok(ConfigListResult { entries })
    });
}
