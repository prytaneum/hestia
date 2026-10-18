//! The verbs a server and an instance share, one handler each over the side the
//! payload names.

use engine::Side;
use proto::entry::{
    EntryConfigGet, EntryConfigList, EntryConfigListResult, EntryConfigSet, EntryConfigValue,
    EntryFlavors, EntryLoaders, EntryLogs, EntryRemove, EntryRename, EntryRenamed, EntryStop,
    EntryVersions,
};
use proto::error::{EntryKind, ErrorInfo, Field};
use proto::minecraft::{ConfigEntry, FlavorsResult};
use proto::process::ProcessState;
use proto::Empty;

use super::guards::{entry_for, entry_held, side_for, Hold, Intent};
use crate::runtime::{engine_error, server_process_id, Channels, JobKind};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<EntryFlavors, _, _>(|p, ctx| async move {
        let side = side_for(&ctx, p.kind)?;
        Ok(FlavorsResult {
            flavors: ctx.runtime.engine().flavors(side).await,
            rules: config_rules(side),
        })
    });

    on.handle::<EntryVersions, _, _>(|p, ctx| async move {
        let side = side_for(&ctx, p.kind)?;
        ctx.runtime
            .engine()
            .minecraft()
            .versions(side, &p.flavor)
            .await
            .map_err(engine_error)
    });

    on.handle::<EntryLoaders, _, _>(|p, ctx| async move {
        let side = side_for(&ctx, p.kind)?;
        ctx.runtime
            .engine()
            .minecraft()
            .loader_versions(side, &p.flavor, &p.version)
            .await
            .map_err(engine_error)
    });

    on.handle::<EntryRemove, _, _>(|target, ctx| async move {
        let (entry, _claim) = entry_held(&ctx, &target, Hold::stopped(JobKind::Content))?;
        let engine = ctx.runtime.engine();
        match entry.kind {
            EntryKind::Server => {
                engine.servers().remove(&entry.id).map_err(engine_error)?;
                ctx.runtime
                    .processes()
                    .discard(&server_process_id(&entry.id));
            }
            EntryKind::Instance => {
                engine.remove_instance(&entry.id).map_err(engine_error)?;
                ctx.runtime.discard_instance_sessions(&entry.id);
            }
        }
        tracing::info!(kind = ?entry.kind, id = %entry.id, name = %entry.name, "entry removed");
        Ok(Empty {})
    });

    on.handle::<EntryRename, _, _>(|p, ctx| async move {
        if p.name.trim().is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Name });
        }
        let (entry, _claim) = entry_held(&ctx, &p.target, Hold::stopped(JobKind::Content))?;
        let engine = ctx.runtime.engine();
        let (id, name) = match entry.kind {
            EntryKind::Server => {
                let renamed = engine
                    .servers()
                    .rename(&entry.id, &p.name)
                    .map_err(engine_error)?;
                (renamed.id, renamed.name)
            }
            EntryKind::Instance => {
                let renamed = engine
                    .instances()
                    .rename(&entry.id, &p.name)
                    .map_err(engine_error)?;
                (renamed.id, renamed.name)
            }
        };
        tracing::info!(kind = ?entry.kind, %id, %name, "entry renamed");
        Ok(EntryRenamed { id, name })
    });

    on.handle::<EntryStop, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let not_running = || ErrorInfo::NotRunning {
            entry: entry.kind,
            name: entry.name.clone(),
        };
        match (entry.kind, p.session) {
            (EntryKind::Server, Some(session)) => {
                return Err(ErrorInfo::ProcessNotFound { id: session });
            }
            (EntryKind::Server, None) => {
                let process_id = server_process_id(&entry.id);
                if !ctx.runtime.process_running(&process_id) {
                    return Err(not_running());
                }
                ctx.runtime.processes().stop(&process_id);
            }
            (EntryKind::Instance, Some(session)) => {
                let sessions = ctx.runtime.instance_sessions(&entry.id);
                if !sessions.iter().any(|s| s.id == session) {
                    return Err(ErrorInfo::ProcessNotFound { id: session });
                }
                ctx.runtime.processes().stop(&session);
            }
            (EntryKind::Instance, None) => {
                if ctx.runtime.stop_instance_sessions(&entry.id) == 0 {
                    return Err(not_running());
                }
            }
        }
        Ok(Empty {})
    });

    on.handle::<EntryLogs, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let process = match (entry.kind, &p.session) {
            (EntryKind::Server, Some(session)) => {
                return Err(ErrorInfo::ProcessNotFound {
                    id: session.clone(),
                });
            }
            (EntryKind::Server, None) => Some(server_process_id(&entry.id)),
            // A specific session, else the newest running one, else the newest.
            (EntryKind::Instance, session) => {
                let sessions = ctx.runtime.instance_sessions(&entry.id);
                match session {
                    Some(session) => sessions.iter().find(|s| &s.id == session),
                    None => sessions
                        .iter()
                        .find(|s| s.state == ProcessState::Running)
                        .or_else(|| sessions.first()),
                }
                .map(|s| s.id.clone())
            }
        };
        Ok(process
            .and_then(|id| ctx.runtime.processes().logs(&id, p.tail))
            .unwrap_or_default())
    });

    on.handle::<EntryConfigGet, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let engine = ctx.runtime.engine();
        let value = match entry.kind {
            EntryKind::Server => engine.servers().config_get(&entry.id, &p.key),
            EntryKind::Instance => engine.instances().config_get(&entry.id, &p.key),
        };
        match value {
            Ok(Some(value)) => Ok(EntryConfigValue { value }),
            Ok(None) => Err(ErrorInfo::ConfigKeyUnset { key: p.key.clone() }),
            Err(e) => Err(engine_error(e)),
        }
    });

    on.handle::<EntryConfigSet, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let engine = ctx.runtime.engine();
        match entry.kind {
            EntryKind::Server => engine.servers().config_set(&entry.id, &p.key, &p.value),
            EntryKind::Instance => engine.instances().config_set(&entry.id, &p.key, &p.value),
        }
        .map_err(engine_error)?;
        tracing::info!(kind = ?entry.kind, id = %entry.id, key = %p.key, "entry config updated");
        Ok(Empty {})
    });

    on.handle::<EntryConfigList, _, _>(|target, ctx| async move {
        let entry = entry_for(&ctx, &target, Intent::Read)?;
        let engine = ctx.runtime.engine();
        let (values, rules) = match entry.kind {
            EntryKind::Server => (
                engine.servers().config_list(&entry.id),
                config_rules(Side::Server),
            ),
            EntryKind::Instance => (
                engine.instances().config_list(&entry.id),
                config_rules(Side::Client),
            ),
        };
        let entries = values
            .map_err(engine_error)?
            .into_iter()
            .map(|(key, value)| ConfigEntry { key, value })
            .collect();
        Ok(EntryConfigListResult { entries, rules })
    });
}

fn config_rules(side: Side) -> Vec<proto::keys::KeyRule> {
    match side {
        Side::Server => engine::Servers::config_rules(),
        Side::Client => engine::Instances::config_rules(),
    }
}
