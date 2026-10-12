//! A server's players. Reading answers for a stopped server; every change is a
//! console command, so it needs the server up.

use proto::error::{EntryKind, ErrorInfo};
use proto::players::{ServerPlayerKick, ServerPlayerSet, ServerPlayers};
use proto::Empty;

use super::guards::{is_running, server_for, Intent};
use crate::runtime::{server_process_id, Channels, HandlerContext};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerPlayers, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let running = is_running(&ctx, &server_process_id(&record.id));
        ctx.runtime
            .engine()
            .server_players(&record.id, running)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ServerPlayerSet, _, _>(|p, ctx| async move {
        let id = running_server(&ctx, &p.server)?;
        ctx.runtime
            .engine()
            .server_player_set(&id, &p.player, p.list, p.listed)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(Empty {})
    });

    on.handle::<ServerPlayerKick, _, _>(|p, ctx| async move {
        let id = running_server(&ctx, &p.server)?;
        ctx.runtime
            .engine()
            .server_player_kick(&id, &p.player)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(Empty {})
    });
}

fn running_server(ctx: &HandlerContext, reference: &str) -> Result<String, ErrorInfo> {
    let record = server_for(ctx, reference, Intent::Read)?;
    if !is_running(ctx, &server_process_id(&record.id)) {
        return Err(ErrorInfo::NotRunning {
            entry: EntryKind::Server,
            name: record.name,
        });
    }
    Ok(record.id)
}
