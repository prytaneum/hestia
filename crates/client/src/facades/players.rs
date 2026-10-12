//! `server.players.*` — who is on a server and who may be.

use proto::players::{
    PlayerList, ServerPlayerKick, ServerPlayerKickParams, ServerPlayerSet, ServerPlayerSetParams,
    ServerPlayers, ServerPlayersResult,
};
use proto::server::ServerRef;

use crate::{IpcError, Session};

pub struct Players<'a> {
    pub(crate) session: &'a Session,
}

impl Players<'_> {
    /// Who is online and the three lists; the lists answer for a stopped
    /// server too.
    pub async fn list(&self, server: &str) -> Result<ServerPlayersResult, IpcError> {
        self.session
            .call::<ServerPlayers>(&ServerRef {
                server: server.to_string(),
            })
            .await
    }

    /// Put `player` on `list` or take them off it. Needs the server running.
    pub async fn set(
        &self,
        server: &str,
        player: &str,
        list: PlayerList,
        listed: bool,
    ) -> Result<(), IpcError> {
        let params = ServerPlayerSetParams {
            server: server.to_string(),
            player: player.to_string(),
            list,
            listed,
        };
        self.session.call::<ServerPlayerSet>(&params).await?;
        Ok(())
    }

    pub async fn kick(&self, server: &str, player: &str) -> Result<(), IpcError> {
        let params = ServerPlayerKickParams {
            server: server.to_string(),
            player: player.to_string(),
        };
        self.session.call::<ServerPlayerKick>(&params).await?;
        Ok(())
    }
}
