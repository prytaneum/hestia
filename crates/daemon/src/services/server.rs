//! Minecraft servers: the provider catalogue, the lifecycle over the supervisor,
//! the rcon console, and the per-server settings. Backups live in `backup`,
//! content installs in `content`.

use proto::error::{EntryKind, ErrorInfo, Field};
use proto::minecraft::{ConfigEntry, FlavorsResult, LoadersResult, VersionsResult};
use proto::process::ProcessLogsResult;
use proto::server::{
    ServerCommand, ServerCommandComplete, ServerCommandResult, ServerConfigGet,
    ServerConfigGetResult, ServerConfigList, ServerConfigListResult, ServerConfigSet, ServerCreate,
    ServerCreateResult, ServerDetail, ServerFlavors, ServerList, ServerListResult, ServerLoaders,
    ServerLogs, ServerPing, ServerProperties, ServerPropertiesResult, ServerRemove, ServerRename,
    ServerResolve, ServerRestart, ServerStart, ServerStartResult, ServerStatus, ServerStop,
    ServerUpdate, ServerUpdateResult, ServerVersions,
};
use proto::Empty;

use super::guards::{is_running, server_for, Intent};
use crate::runtime::{server_process_id, Channels};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerFlavors, _, _>(|_: Empty, ctx| async move {
        Ok(FlavorsResult {
            flavors: ctx.runtime.engine().server_flavors().await,
        })
    });

    on.handle::<ServerVersions, _, _>(|p, ctx| async move {
        let versions = ctx
            .runtime
            .engine()
            .minecraft()
            .server_versions(&p.flavor)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(VersionsResult { versions })
    });

    on.handle::<ServerResolve, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .minecraft()
            .resolve_server(&p.flavor, &p.version, p.loader_version)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ServerLoaders, _, _>(|p, ctx| async move {
        let loaders = ctx
            .runtime
            .engine()
            .minecraft()
            .server_loader_versions(&p.flavor, &p.version)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(LoadersResult { loaders })
    });

    on.handle::<ServerCreate, _, _>(|p, ctx| async move {
        if p.flavor.is_empty() || p.version.is_empty() {
            return Err(ErrorInfo::FieldsRequired {
                fields: vec![Field::Flavor, Field::Version],
            });
        }
        if !p.eula {
            return Err(ErrorInfo::EulaRequired);
        }
        match ctx.runtime.server_creates().start(p) {
            Some(id) => Ok(ServerCreateResult { id }),
            None => Err(ErrorInfo::CreateInProgress),
        }
    });

    on.handle::<ServerUpdate, _, _>(|p, ctx| async move {
        if p.version.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: Field::Version,
            });
        }
        let record = server_for(&ctx, &p.server, Intent::Lifecycle)?;
        match ctx.runtime.server_updates().start(record.id, p) {
            Some(id) => Ok(ServerUpdateResult { id }),
            None => Err(ErrorInfo::UpdateInProgress {
                name: record.name.clone(),
            }),
        }
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
        Ok(ServerListResult { servers })
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

    on.handle::<ServerRemove, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Lifecycle)?;
        ctx.runtime
            .engine()
            .servers()
            .remove(&record.id)
            .map_err(crate::runtime::engine_error)?;
        ctx.runtime
            .processes()
            .discard(&server_process_id(&record.id));
        tracing::info!(server = %record.id, name = %record.name, "server removed");
        Ok(Empty {})
    });

    on.handle::<ServerRename, _, _>(|p, ctx| async move {
        if p.name.trim().is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Name });
        }
        let record = server_for(&ctx, &p.server, Intent::Lifecycle)?;
        let renamed = ctx
            .runtime
            .engine()
            .servers()
            .rename(&record.id, &p.name)
            .map_err(crate::runtime::engine_error)?;
        tracing::info!(id = %renamed.id, name = %renamed.name, "server renamed");
        Ok(ctx.runtime.server_view(renamed))
    });

    on.handle::<ServerStart, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Start)?;
        tracing::info!(server = %record.id, name = %record.name, "starting server");
        let info = ctx.runtime.start_server(&record.id).await?;
        Ok(ServerStartResult {
            process_id: info.id,
            pid: info.pid,
        })
    });

    on.handle::<ServerStop, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let process_id = server_process_id(&record.id);
        if !is_running(&ctx, &process_id) {
            return Err(ErrorInfo::NotRunning {
                entry: EntryKind::Server,
                name: record.name.clone(),
            });
        }
        ctx.runtime.processes().stop(&process_id);
        Ok(Empty {})
    });

    on.handle::<ServerRestart, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Restart)?;
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

    on.handle::<ServerLogs, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let lines = ctx
            .runtime
            .processes()
            .logs(&server_process_id(&record.id), p.tail)
            .unwrap_or_default();
        Ok(ProcessLogsResult { lines })
    });

    on.handle::<ServerConfigGet, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        match ctx
            .runtime
            .engine()
            .servers()
            .config_get(&record.id, &p.key)
        {
            Ok(Some(value)) => Ok(ServerConfigGetResult { value }),
            Ok(None) => Err(ErrorInfo::ConfigKeyUnset { key: p.key.clone() }),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });

    on.handle::<ServerConfigSet, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        ctx.runtime
            .engine()
            .servers()
            .config_set(&record.id, &p.key, &p.value)
            .map_err(crate::runtime::engine_error)?;
        tracing::info!(server = %record.id, key = %p.key, "server config updated");
        Ok(Empty {})
    });

    on.handle::<ServerConfigList, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let entries = ctx
            .runtime
            .engine()
            .servers()
            .config_list(&record.id)
            .map_err(crate::runtime::engine_error)?
            .into_iter()
            .map(|(key, value)| ConfigEntry { key, value })
            .collect();
        Ok(ServerConfigListResult { entries })
    });

    on.handle::<ServerProperties, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let properties = ctx
            .runtime
            .engine()
            .servers()
            .properties(&record.id)
            .map_err(crate::runtime::engine_error)?;
        Ok(ServerPropertiesResult { properties })
    });
}
