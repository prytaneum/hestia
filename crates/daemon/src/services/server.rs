//! Minecraft servers: the lifecycle over the supervisor and the rcon console.
//! What an instance shares lives in `entry`, backups in `backup`, content
//! installs in `content`.

use proto::error::{EntryKind, ErrorInfo, Field};
use proto::job::JobStarted;
use proto::server::{
    ServerCommand, ServerCommandComplete, ServerCommandResult, ServerCreate, ServerDetail,
    ServerList, ServerPing, ServerProperties, ServerRestart, ServerStart, ServerStartResult,
    ServerStatus, ServerUpdate,
};
use proto::Empty;

use super::guards::{is_running, server_for, server_held, Hold, Intent};
use crate::runtime::{server_process_id, Channels, JobKind};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerCreate, _, _>(|p, ctx| async move {
        if p.flavor.is_empty() || p.version.is_empty() {
            return Err(ErrorInfo::FieldsRequired {
                fields: vec![Field::Flavor, Field::Version],
            });
        }
        if !p.eula {
            return Err(ErrorInfo::EulaRequired);
        }
        match ctx.runtime.jobs().start_server_create(p) {
            Some(id) => Ok(JobStarted { id }),
            None => Err(ErrorInfo::CreateInProgress),
        }
    });

    on.handle::<ServerUpdate, _, _>(|p, ctx| async move {
        if p.version.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: Field::Version,
            });
        }
        let hold = match p.restart {
            true => Hold::live(JobKind::Update),
            false => Hold::stopped(JobKind::Update),
        };
        let (record, claim) = server_held(&ctx, &p.server, hold)?;
        let id = ctx.runtime.jobs().start_server_update(record, p, claim);
        Ok(JobStarted { id })
    });

    on.handle::<ServerList, _, _>(|_: Empty, ctx| async move {
        let servers = ctx
            .runtime
            .engine()
            .servers()
            .list()
            .into_iter()
            .map(|r| ctx.runtime.server_view(r))
            .collect();
        Ok(servers)
    });

    on.handle::<ServerStatus, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        Ok(ctx.runtime.server_view(record))
    });

    on.handle::<ServerDetail, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        ctx.runtime
            .engine()
            .server_detail(&record.id)
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ServerPing, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        ctx.runtime
            .engine()
            .server_ping(&record.id)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ServerStart, _, _>(|p, ctx| async move {
        let (record, _claim) = server_held(&ctx, &p.server, Hold::stopped(JobKind::Launch))?;
        tracing::info!(server = %record.id, name = %record.name, "starting server");
        let info =
            crate::runtime::start_server(ctx.runtime.engine(), ctx.runtime.processes(), &record.id)
                .await?;
        Ok(ServerStartResult {
            process_id: info.id,
            pid: info.pid,
        })
    });

    on.handle::<ServerRestart, _, _>(|p, ctx| async move {
        let (record, _claim) = server_held(&ctx, &p.server, Hold::live(JobKind::Launch))?;
        if !is_running(&ctx, &server_process_id(&record.id)) {
            return Err(ErrorInfo::NotRunning {
                entry: EntryKind::Server,
                name: record.name.clone(),
            });
        }
        tracing::info!(server = %record.id, name = %record.name, "restarting server");
        let info = ctx.runtime.restart_server(&record).await?;
        Ok(ServerStartResult {
            process_id: info.id,
            pid: info.pid,
        })
    });

    on.handle::<ServerCommand, _, _>(|p, ctx| async move {
        if p.command.trim().is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: Field::Command,
            });
        }
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        if !is_running(&ctx, &server_process_id(&record.id)) {
            return Err(ErrorInfo::NotRunning {
                entry: EntryKind::Server,
                name: record.name.clone(),
            });
        }
        let response = ctx
            .runtime
            .engine()
            .server_command(&record.id, &p.command)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ServerCommandResult { response })
    });

    on.handle::<ServerCommandComplete, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        if !is_running(&ctx, &server_process_id(&record.id)) {
            return Err(ErrorInfo::NotRunning {
                entry: EntryKind::Server,
                name: record.name.clone(),
            });
        }
        ctx.runtime
            .engine()
            .server_complete(&record.id, &p.line)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ServerProperties, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let properties = ctx
            .runtime
            .engine()
            .servers()
            .properties(&record.id)
            .map_err(crate::runtime::engine_error)?;
        Ok(properties)
    });
}
