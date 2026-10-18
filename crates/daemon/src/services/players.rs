//! A server's players. Reading answers for a stopped server; every change is a
//! console command, so it needs the server up.

use proto::error::ErrorInfo;
use proto::players::{ServerPlayerKick, ServerPlayerSet, ServerPlayers};
use proto::Empty;

use super::guards::{server_for, server_running, Intent};
use crate::runtime::{server_process_id, Channels, HandlerContext};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerPlayers, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let running = ctx.runtime.process_running(&server_process_id(&record.id));
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
    Ok(server_running(ctx, reference)?.id)
}
