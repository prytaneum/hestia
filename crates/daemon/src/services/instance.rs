//! Minecraft instances (clients): the provider catalogue, the record lifecycle,
//! launch over the supervisor, and the per-instance JVM settings. Backups live
//! in `backup`, content installs in `content`.

use engine::Side;
use proto::error::{EntryKind, ErrorInfo, Field};
use proto::instance::{
    AddressPing, InstanceConfigGet, InstanceConfigGetResult, InstanceConfigList,
    InstanceConfigListResult, InstanceConfigSet, InstanceCreate, InstanceCreateResult,
    InstanceFlavors, InstanceInfoQuery, InstanceLaunch, InstanceLaunchResult, InstanceList,
    InstanceListResult, InstanceLoaders, InstanceLogs, InstanceRecent, InstanceRecentResult,
    InstanceRemove, InstanceRename, InstanceServerEdit, InstanceServerRemove, InstanceServers,
    InstanceServersArrange, InstanceServersResult, InstanceServersWriteResult, InstanceStop,
    InstanceUpdate, InstanceUpdateResult, InstanceVersions, InstanceWorlds, InstanceWorldsResult,
    ServerEntry,
};
use proto::minecraft::{ConfigEntry, FlavorsResult, LoadersResult, VersionsResult};
use proto::process::ProcessLogsResult;
use proto::Empty;

use super::guards::{instance_for, Intent};
use crate::runtime::{Channels, LaunchOrder};

/// The shared shape of a multiplayer-list write: the list as it now stands,
/// carrying whatever the write could not guarantee.
fn server_list_result(written: engine::ServerListWrite) -> InstanceServersWriteResult {
    InstanceServersWriteResult {
        servers: written.servers,
        warnings: written.warnings,
    }
}

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<InstanceFlavors, _, _>(|_: Empty, ctx| async move {
        Ok(FlavorsResult {
            flavors: ctx.runtime.engine().flavors(Side::Client).await,
        })
    });

    on.handle::<InstanceVersions, _, _>(|p, ctx| async move {
        let versions = ctx
            .runtime
            .engine()
            .minecraft()
            .versions(Side::Client, &p.flavor)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(VersionsResult { versions })
    });

    on.handle::<InstanceLoaders, _, _>(|p, ctx| async move {
        let loaders = ctx
            .runtime
            .engine()
            .minecraft()
            .loader_versions(Side::Client, &p.flavor, &p.version)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(LoadersResult { loaders })
    });

    on.handle::<InstanceCreate, _, _>(|p, ctx| async move {
        if p.flavor.is_empty() || p.version.is_empty() {
            return Err(ErrorInfo::FieldsRequired {
                fields: vec![Field::Flavor, Field::Version],
            });
        }
        let record = ctx
            .runtime
            .engine()
            .create_instance(&p.name, &p.flavor, &p.version, p.loader_version, &p.config)
            .await
            .map_err(crate::runtime::engine_error)?;
        tracing::info!(
            instance = %record.id,
            name = %record.name,
            flavor = %record.profile.flavor,
            version = %record.profile.game_version,
            "instance created"
        );
        Ok(InstanceCreateResult {
            instance: ctx.runtime.instance_view(record),
        })
    });

    on.handle::<InstanceUpdate, _, _>(|p, ctx| async move {
        if p.version.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: Field::Version,
            });
        }
        let record = instance_for(&ctx, &p.instance, Intent::Lifecycle)?;
        let record = ctx
            .runtime
            .engine()
            .update_instance(&record.id, &p.version, p.loader_version, p.allow_downgrade)
            .await
            .map_err(crate::runtime::engine_error)?;
        tracing::info!(
            instance = %record.id,
            version = %record.profile.game_version,
            "instance updated"
        );
        Ok(InstanceUpdateResult {
            instance: ctx.runtime.instance_view(record),
        })
    });

    on.handle::<InstanceList, _, _>(|_: Empty, ctx| async move {
        let instances = ctx
            .runtime
            .engine()
            .instances()
            .list()
            .into_iter()
            .map(|r| ctx.runtime.instance_view(r))
            .collect();
        Ok(InstanceListResult { instances })
    });

    on.handle::<InstanceInfoQuery, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        ctx.runtime
            .engine()
            .instance_detail(&record.id)
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<InstanceWorlds, _, _>(|p, ctx| async move {
        let worlds = ctx
            .runtime
            .engine()
            .instance_worlds(&p.instance)
            .map_err(|_| ErrorInfo::EntryNotFound {
                entry: EntryKind::Instance,
                reference: p.instance.clone(),
            })?;
        Ok(InstanceWorldsResult { worlds })
    });

    on.handle::<InstanceRecent, _, _>(|_: Empty, ctx| async move {
        Ok(InstanceRecentResult {
            plays: ctx.runtime.engine().recent_plays(),
        })
    });

    on.handle::<InstanceServers, _, _>(|p, ctx| async move {
        let servers = ctx
            .runtime
            .engine()
            .instance_servers(&p.instance)
            .map_err(crate::runtime::engine_error)?;
        Ok(InstanceServersResult { servers })
    });

    on.handle::<InstanceServerEdit, _, _>(|p, ctx| async move {
        let written = ctx
            .runtime
            .engine()
            .edit_instance_server(
                &p.instance,
                &p.server,
                ServerEntry {
                    name: p.name,
                    address: p.address,
                    accept_textures: p.accept_textures,
                    ..ServerEntry::default()
                },
            )
            .map_err(crate::runtime::engine_error)?;
        Ok(server_list_result(written))
    });

    on.handle::<InstanceServerRemove, _, _>(|p, ctx| async move {
        let written = ctx
            .runtime
            .engine()
            .remove_instance_server(&p.instance, &p.server)
            .map_err(crate::runtime::engine_error)?;
        Ok(server_list_result(written))
    });

    on.handle::<InstanceServersArrange, _, _>(|p, ctx| async move {
        let written = ctx
            .runtime
            .engine()
            .arrange_instance_servers(&p.instance, &p.order)
            .map_err(crate::runtime::engine_error)?;
        Ok(server_list_result(written))
    });

    on.handle::<AddressPing, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .ping_address(&p.address)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<InstanceRemove, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Lifecycle)?;
        ctx.runtime
            .engine()
            .remove_instance(&record.id)
            .map_err(crate::runtime::engine_error)?;
        ctx.runtime.discard_instance_sessions(&record.id);
        tracing::info!(instance = %record.id, name = %record.name, "instance removed");
        Ok(Empty {})
    });

    on.handle::<InstanceRename, _, _>(|p, ctx| async move {
        if p.name.trim().is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Name });
        }
        let record = instance_for(&ctx, &p.instance, Intent::Lifecycle)?;
        let renamed = ctx
            .runtime
            .engine()
            .instances()
            .rename(&record.id, &p.name)
            .map_err(crate::runtime::engine_error)?;
        tracing::info!(id = %renamed.id, name = %renamed.name, "instance renamed");
        Ok(ctx.runtime.instance_view(renamed))
    });

    on.handle::<InstanceLaunch, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Start)?;
        // The account's tokens can no longer be refreshed: block up front so a
        // dead sign-in prompts re-login instead of failing mid-launch. An
        // offline launch never presents a token, so the gate does not apply.
        if !p.offline && ctx.runtime.engine().accounts().needs_reauth(&p.account) {
            return Err(ErrorInfo::SessionExpired {
                reference: p.account.clone(),
            });
        }
        // Concurrent sessions are gated twice: `instance.multi-session` has to
        // allow them at all, and the launch opts in with `new_session`.
        let running = ctx.runtime.instance_running(&record.id);
        if running {
            if !p.new_session {
                return Err(ErrorInfo::EntryRunning {
                    entry: EntryKind::Instance,
                    name: record.name.clone(),
                });
            }
            if !ctx
                .runtime
                .engine()
                .config()
                .settings()
                .instance
                .multi_session
            {
                return Err(ErrorInfo::MultiSessionDisabled {
                    name: record.name.clone(),
                });
            }
        }
        match ctx.runtime.instance_launches().start(LaunchOrder {
            instance_id: record.id,
            account: p.account,
            reconcile: !running,
            quick_play: p.quick_play,
            offline: p.offline,
            id: p.id,
        }) {
            Some(id) => Ok(InstanceLaunchResult { id }),
            None => Err(ErrorInfo::Internal {
                detail: "that instance could not be launched".into(),
            }),
        }
    });

    on.handle::<InstanceStop, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        let sessions = ctx.runtime.instance_sessions(&record.id);
        match p.session {
            // Stop one named session, refusing an id that is not this instance's.
            Some(session) => {
                if !sessions.iter().any(|s| s.id == session) {
                    return Err(ErrorInfo::ProcessNotFound {
                        id: session.clone(),
                    });
                }
                ctx.runtime.processes().stop(&session);
            }
            None => {
                let stopped = ctx.runtime.stop_instance_sessions(&record.id);
                if stopped == 0 {
                    return Err(ErrorInfo::NotRunning {
                        entry: EntryKind::Instance,
                        name: record.name.clone(),
                    });
                }
            }
        }
        Ok(Empty {})
    });

    on.handle::<InstanceLogs, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        // A specific session, else the newest running one, else the newest.
        let sessions = ctx.runtime.instance_sessions(&record.id);
        let target = match &p.session {
            Some(session) => sessions
                .iter()
                .find(|s| &s.id == session)
                .map(|s| s.id.clone()),
            None => sessions
                .iter()
                .find(|s| s.state == proto::process::ProcessState::Running)
                .or_else(|| sessions.first())
                .map(|s| s.id.clone()),
        };
        let lines = target
            .and_then(|id| ctx.runtime.processes().logs(&id, p.tail))
            .unwrap_or_default();
        Ok(ProcessLogsResult { lines })
    });

    on.handle::<InstanceConfigGet, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        match ctx
            .runtime
            .engine()
            .instances()
            .config_get(&record.id, &p.key)
        {
            Ok(Some(value)) => Ok(InstanceConfigGetResult { value }),
            Ok(None) => Err(ErrorInfo::ConfigKeyUnset { key: p.key.clone() }),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });

    on.handle::<InstanceConfigSet, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        ctx.runtime
            .engine()
            .instances()
            .config_set(&record.id, &p.key, &p.value)
            .map_err(crate::runtime::engine_error)?;
        tracing::info!(instance = %record.id, key = %p.key, "instance config updated");
        Ok(Empty {})
    });

    on.handle::<InstanceConfigList, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        let entries = ctx
            .runtime
            .engine()
            .instances()
            .config_list(&record.id)
            .map_err(crate::runtime::engine_error)?
            .into_iter()
            .map(|(key, value)| ConfigEntry { key, value })
            .collect();
        Ok(InstanceConfigListResult { entries })
    });
}
