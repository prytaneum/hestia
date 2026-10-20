//! Minecraft instances (clients): the record lifecycle and launch over the
//! supervisor. What a server shares lives in `entry`, content installs in
//! `content`.

use proto::error::{EntryKind, ErrorInfo, Field};
use proto::instance::{
    AddressPing, InstanceCreate, InstanceInfoQuery, InstanceLaunch, InstanceList, InstanceRecent,
    InstanceServerEdit, InstanceServerRemove, InstanceServers, InstanceServersArrange,
    InstanceServersWriteResult, InstanceUpdate, InstanceWorlds, Replace, ServerEntry,
};
use proto::job::JobStarted;
use proto::process::ProcessState;
use proto::Empty;

use super::guards::{instance_for, instance_held, Hold, Intent};
use crate::runtime::{Channels, JobKind, LaunchOrder};

/// The shared shape of a multiplayer-list write: the list as it now stands,
/// carrying whatever the write could not guarantee.
fn server_list_result(written: engine::ServerListWrite) -> InstanceServersWriteResult {
    InstanceServersWriteResult {
        servers: written.servers,
        warnings: written.warnings,
    }
}

pub(super) fn register(on: &mut Channels<'_>) {
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
        Ok(ctx.runtime.instance_view(record))
    });

    on.handle::<InstanceUpdate, _, _>(|p, ctx| async move {
        if p.version.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: Field::Version,
            });
        }
        let (record, claim) = instance_held(&ctx, &p.instance, Hold::stopped(JobKind::Update))?;
        let id = ctx
            .runtime
            .jobs()
            .start_instance_update(record.id, p, claim, ctx.chain.clone());
        Ok(JobStarted { id })
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
        Ok(instances)
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
        Ok(worlds)
    });

    on.handle::<InstanceRecent, _, _>(|_: Empty, ctx| async move {
        Ok(ctx.runtime.engine().recent_plays())
    });

    on.handle::<InstanceServers, _, _>(|p, ctx| async move {
        let servers = ctx
            .runtime
            .engine()
            .instance_servers(&p.instance)
            .map_err(crate::runtime::engine_error)?;
        Ok(servers)
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

    on.handle::<InstanceLaunch, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Idle)?;
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
        let sessions: Vec<_> = ctx
            .runtime
            .instance_sessions(&record.id)
            .into_iter()
            .filter(|s| s.state == ProcessState::Running)
            .collect();
        let replace: Vec<String> = match &p.replace {
            None => Vec::new(),
            Some(Replace::All) => sessions.iter().map(|s| s.id.clone()).collect(),
            Some(Replace::One(session)) => {
                if !sessions.iter().any(|s| &s.id == session) {
                    return Err(ErrorInfo::ProcessNotFound {
                        id: session.clone(),
                    });
                }
                vec![session.clone()]
            }
        };
        let running = sessions.iter().any(|s| !replace.contains(&s.id));
        if running {
            if !p.new_session && p.replace.is_none() {
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
        let id = ctx.runtime.jobs().start_instance_launch(
            LaunchOrder {
                instance_id: record.id,
                instance_name: record.name,
                account: p.account,
                reconcile: !running,
                quick_play: p.quick_play,
                offline: p.offline,
                replace,
                id: p.id,
            },
            ctx.chain.clone(),
        );
        Ok(JobStarted { id })
    });
}
