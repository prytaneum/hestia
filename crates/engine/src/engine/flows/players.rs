//! Who is on a server and who may be. The lists are read from the files the
//! server keeps beside its world, so they answer for a stopped server too;
//! every change goes through the running server's console, which resolves the
//! name to an account and rewrites its own file.

use anyhow::{bail, Result};
use proto::error::ErrorInfo;
use proto::players::{PlayerEntry, PlayerList, ServerPlayersResult};

use crate::edition;
use crate::engine::Engine;

// Past the vanilla 16 so a proxy's prefixed names (Floodgate's `.Steve`) fit.
const ONLINE_MARKER: &str = "players online:";

impl Engine {
    /// The lists from disk, plus who is online when `running`.
    pub async fn server_players(
        &self,
        reference: &str,
        running: bool,
    ) -> Result<ServerPlayersResult> {
        let record = self.servers.require(reference)?;
        let data = self.servers.data_dir(&record);
        let online = match running {
            true => self.online_players(&record.id).await?,
            false => Vec::new(),
        };
        Ok(edition::read_players(record.edition, &data, online))
    }

    pub(crate) async fn online_players(&self, reference: &str) -> Result<Vec<PlayerEntry>> {
        let record = self.servers.require(reference)?;
        let reply = self
            .server_command(reference, edition::list_command(record.edition))
            .await?;
        Ok(parse_online(&reply))
    }

    /// Put `player` on `list` or take them off it, on a running server.
    pub async fn server_player_set(
        &self,
        reference: &str,
        player: &str,
        list: PlayerList,
        listed: bool,
    ) -> Result<String> {
        let edition = self.servers.require(reference)?.edition;
        let (player, command) = edition::set_command(edition, player, list, listed)?;
        if edition::op_needs_online(edition) && list == PlayerList::Ops {
            require_online(&self.online_players(reference).await?, player)?;
        }
        tracing::info!(
            server = reference,
            player,
            ?list,
            listed,
            "player list changed"
        );
        self.server_command(reference, &command).await
    }

    pub async fn server_player_kick(&self, reference: &str, player: &str) -> Result<String> {
        let edition = self.servers.require(reference)?.edition;
        let command = edition::kick_command(edition, player)?;
        tracing::info!(server = reference, player = player.trim(), "player kicked");
        self.server_command(reference, &command).await
    }
}

/// The `N/M` a Bedrock `list` reply opens with (`There are 0/10 players online:`).
pub(crate) fn parse_player_count(reply: &str) -> Option<(u32, u32)> {
    let (before, _) = reply.split_once(ONLINE_MARKER)?;
    let (online, max) = before.split_whitespace().last()?.split_once('/')?;
    Some((online.parse().ok()?, max.parse().ok()?))
}

/// BDS resolves an operator by connection, not by account, so the player must
/// be on the server.
fn require_online(online: &[PlayerEntry], player: &str) -> Result<()> {
    match online
        .iter()
        .any(|entry| entry.name.eq_ignore_ascii_case(player))
    {
        true => Ok(()),
        false => bail!(ErrorInfo::PlayerNotOnline {
            name: player.to_string()
        }),
    }
}

/// The players out of a `list` or `list uuids` reply. Every version puts them after
/// `players online:`, comma-separated — on the same line or the next — each as
/// `Name (uuid)`. A server too old to know the argument answers with bare
/// names, which leaves the uuid empty. A Bedrock reply is read from the log,
/// so lines around those two are someone else's output.
fn parse_online(reply: &str) -> Vec<PlayerEntry> {
    let Some((_, after)) = reply.split_once(ONLINE_MARKER) else {
        return Vec::new();
    };
    let mut lines = after.splitn(3, '\n');
    [lines.next(), lines.next()]
        .into_iter()
        .flatten()
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|player| !player.is_empty())
        .map(|player| {
            let (name, uuid) = player
                .strip_suffix(')')
                .and_then(|rest| rest.rsplit_once(" ("))
                .unwrap_or((player, ""));
            PlayerEntry {
                name: name.to_string(),
                uuid: uuid.to_string(),
                reason: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_count_is_read_from_a_list_reply() {
        assert_eq!(
            parse_player_count("[INFO] There are 2/10 players online:\nA, B"),
            Some((2, 10))
        );
        assert_eq!(parse_player_count("Unknown command"), None);
    }

    #[test]
    fn an_operator_must_be_online_on_bedrock() {
        let online = parse_online("There are 1/10 players online:\nCool Steve");
        assert!(require_online(&online, "cool steve").is_ok());
        let error = require_online(&online, "Alex")
            .unwrap_err()
            .downcast::<ErrorInfo>()
            .unwrap();
        assert_eq!(
            error,
            ErrorInfo::PlayerNotOnline {
                name: "Alex".into()
            }
        );
    }

    #[test]
    fn a_bedrock_list_reply_is_read_with_or_without_players() {
        let names = |reply: &str| -> Vec<String> {
            parse_online(reply).into_iter().map(|p| p.name).collect()
        };
        assert!(names("There are 0/10 players online:").is_empty());
        assert!(names("There are 0/10 players online:\n").is_empty());
        assert_eq!(
            names("There are 3/10 players online:\nAlex, Cool Steve ,Zed"),
            ["Alex", "Cool Steve", "Zed"]
        );
        assert_eq!(
            names("Player connected: Bob, xuid: 1\nThere are 1/10 players online:\nBob\nPlayer connected: Eve, xuid: 2"),
            ["Bob"],
            "log lines around the reply are not players"
        );
    }

    #[test]
    fn the_online_players_are_read_from_either_reply_shape() {
        let pairs = |reply: &str| -> Vec<(String, String)> {
            parse_online(reply)
                .into_iter()
                .map(|player| (player.name, player.uuid))
                .collect()
        };
        let pair = |name: &str, uuid: &str| (name.to_string(), uuid.to_string());

        assert_eq!(
            pairs("There are 2 of a max of 20 players online: Alex (u1), Steve (u2)"),
            [pair("Alex", "u1"), pair("Steve", "u2")]
        );
        assert_eq!(
            pairs("There are 2/20 players online:\nAlex, Steve"),
            [pair("Alex", ""), pair("Steve", "")]
        );
        assert!(parse_online("There are 0 of a max of 20 players online: ").is_empty());
    }
}
