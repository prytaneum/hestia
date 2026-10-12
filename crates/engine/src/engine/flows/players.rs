//! Who is on a server and who may be. The lists are read from the files the
//! server keeps beside its world, so they answer for a stopped server too;
//! every change goes through the running server's console, which resolves the
//! name to an account and rewrites its own file.

use std::path::Path;

use anyhow::{bail, Context, Result};
use proto::error::{ErrorInfo, Field, Reason};
use proto::players::{PlayerEntry, PlayerList, ServerPlayersResult};

use crate::engine::Engine;

const WHITELIST_FILE: &str = "whitelist.json";
const OPS_FILE: &str = "ops.json";
const BANNED_FILE: &str = "banned-players.json";

// Past the vanilla 16 so a proxy's prefixed names (Floodgate's `.Steve`) fit.
const MAX_NAME_LEN: usize = 32;

impl Engine {
    /// The three lists from disk, plus the names online when `running`.
    pub async fn server_players(
        &self,
        reference: &str,
        running: bool,
    ) -> Result<ServerPlayersResult> {
        let record = self
            .servers
            .get(reference)
            .with_context(|| format!("unknown server: {reference}"))?;
        let data = self.servers.data_dir(&record);
        let online = match running {
            true => parse_online(&self.server_command(&record.id, "list").await?),
            false => Vec::new(),
        };
        Ok(ServerPlayersResult {
            online,
            whitelist: read_list(&data.join(WHITELIST_FILE)),
            ops: read_list(&data.join(OPS_FILE)),
            banned: read_list(&data.join(BANNED_FILE)),
        })
    }

    /// Put `player` on `list` or take them off it, on a running server.
    pub async fn server_player_set(
        &self,
        reference: &str,
        player: &str,
        list: PlayerList,
        listed: bool,
    ) -> Result<String> {
        let player = checked_name(player)?;
        let command = match (list, listed) {
            (PlayerList::Whitelist, true) => format!("whitelist add {player}"),
            (PlayerList::Whitelist, false) => format!("whitelist remove {player}"),
            (PlayerList::Ops, true) => format!("op {player}"),
            (PlayerList::Ops, false) => format!("deop {player}"),
            (PlayerList::Banned, true) => format!("ban {player}"),
            (PlayerList::Banned, false) => format!("pardon {player}"),
        };
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
        let player = checked_name(player)?;
        tracing::info!(server = reference, player, "player kicked");
        self.server_command(reference, &format!("kick {player}"))
            .await
    }
}

/// A name is spliced into a console command, so anything that could end the
/// argument or start another command is refused before it gets there.
fn checked_name(player: &str) -> Result<&str> {
    let player = player.trim();
    let valid = !player.is_empty()
        && player.len() <= MAX_NAME_LEN
        && player
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if !valid {
        bail!(ErrorInfo::InvalidValue {
            field: Field::Player,
            reason: Reason::PlayerName,
        });
    }
    Ok(player)
}

/// The names out of a `list` reply. Every version puts them after the first
/// colon, comma-separated — on the same line or the next.
fn parse_online(reply: &str) -> Vec<String> {
    let Some((_, names)) = reply.split_once(':') else {
        return Vec::new();
    };
    names
        .split([',', '\n'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// One of the server's player lists. A missing or unreadable file is an empty
/// list: a server that has never run has none of them yet.
fn read_list(path: &Path) -> Vec<PlayerEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<PlayerEntry>>(&text) {
        Ok(mut entries) => {
            entries.sort_by_key(|entry| entry.name.to_lowercase());
            entries
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cannot read a player list");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_could_break_out_of_the_command_is_refused() {
        for name in ["", "a b", "Steve\nstop", "@a", "x;y", &"x".repeat(33)] {
            assert!(checked_name(name).is_err(), "{name:?}");
        }
        assert_eq!(checked_name(" Steve_1 ").unwrap(), "Steve_1");
        assert_eq!(checked_name(".Bedrock").unwrap(), ".Bedrock");
    }

    #[test]
    fn the_online_names_are_read_from_either_reply_shape() {
        assert_eq!(
            parse_online("There are 2 of a max of 20 players online: Alex, Steve"),
            ["Alex", "Steve"]
        );
        assert_eq!(
            parse_online("There are 2/20 players online:\nAlex, Steve"),
            ["Alex", "Steve"]
        );
        assert!(parse_online("There are 0 of a max of 20 players online: ").is_empty());
    }

    #[test]
    fn a_list_keeps_the_reason_and_survives_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(BANNED_FILE);
        assert!(read_list(&path).is_empty());

        std::fs::write(
            &path,
            r#"[{"uuid":"u2","name":"zed","created":"x","source":"Server","expires":"forever","reason":"griefing"},
                {"uuid":"u1","name":"Alex","level":4}]"#,
        )
        .unwrap();
        let list = read_list(&path);
        assert_eq!(list[0].name, "Alex");
        assert_eq!(list[1].reason.as_deref(), Some("griefing"));
    }
}
