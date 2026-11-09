//! Who is on a server and who may be. The lists are read from the files the
//! server keeps beside its world, so they answer for a stopped server too;
//! every change goes through the running server's console, which resolves the
//! name to an account and rewrites its own file.

use std::path::Path;

use anyhow::{bail, Result};
use proto::error::{ErrorInfo, Field, Reason, Unsupported};
use proto::minecraft::Edition;
use proto::players::{PlayerEntry, PlayerList, ServerPlayersResult};
use serde::Deserialize;

use crate::engine::Engine;

const WHITELIST_FILE: &str = "whitelist.json";
const OPS_FILE: &str = "ops.json";
const BANNED_FILE: &str = "banned-players.json";
const ALLOWLIST_FILE: &str = "allowlist.json";
const PERMISSIONS_FILE: &str = "permissions.json";

// Past the vanilla 16 so a proxy's prefixed names (Floodgate's `.Steve`) fit.
const ONLINE_MARKER: &str = "players online:";

const MAX_NAME_LEN: usize = 32;

/// The lists a server of this edition keeps; Bedrock has no ban list.
pub fn player_lists(edition: Edition) -> Vec<PlayerList> {
    match edition {
        Edition::Java => vec![PlayerList::Whitelist, PlayerList::Ops, PlayerList::Banned],
        Edition::Bedrock => vec![PlayerList::Whitelist, PlayerList::Ops],
    }
}

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
        Ok(match record.profile.runtime.edition() {
            Edition::Java => ServerPlayersResult {
                online,
                whitelist: read_list(&data.join(WHITELIST_FILE)),
                ops: read_list(&data.join(OPS_FILE)),
                banned: read_list(&data.join(BANNED_FILE)),
            },
            Edition::Bedrock => {
                let whitelist = read_allowlist(&data.join(ALLOWLIST_FILE));
                let ops = read_operators(&data.join(PERMISSIONS_FILE), &whitelist);
                ServerPlayersResult {
                    online,
                    whitelist,
                    ops,
                    banned: Vec::new(),
                }
            }
        })
    }

    pub(crate) async fn online_players(&self, reference: &str) -> Result<Vec<PlayerEntry>> {
        let record = self.servers.require(reference)?;
        let command = match record.profile.runtime.edition() {
            Edition::Java => "list uuids",
            Edition::Bedrock => "list",
        };
        let reply = self.server_command(reference, command).await?;
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
        let edition = self.servers.require(reference)?.profile.runtime.edition();
        let (player, command) = set_command(edition, player, list, listed)?;
        if edition == Edition::Bedrock && list == PlayerList::Ops {
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
        let edition = self.servers.require(reference)?.profile.runtime.edition();
        let player = checked_name(edition, player)?;
        tracing::info!(server = reference, player, "player kicked");
        self.server_command(reference, &format!("kick {}", argument(edition, player)))
            .await
    }
}

/// The `N/M` a Bedrock `list` reply opens with (`There are 0/10 players online:`).
pub(crate) fn parse_player_count(reply: &str) -> Option<(u32, u32)> {
    let (before, _) = reply.split_once(ONLINE_MARKER)?;
    let (online, max) = before.split_whitespace().last()?.split_once('/')?;
    Some((online.parse().ok()?, max.parse().ok()?))
}

/// The console command for a list change, with the name it was built for.
fn set_command(
    edition: Edition,
    player: &str,
    list: PlayerList,
    listed: bool,
) -> Result<(&str, String)> {
    let player = checked_name(edition, player)?;
    let verb = match (edition, list, listed) {
        (Edition::Java, PlayerList::Whitelist, true) => "whitelist add",
        (Edition::Java, PlayerList::Whitelist, false) => "whitelist remove",
        (Edition::Bedrock, PlayerList::Whitelist, true) => "allowlist add",
        (Edition::Bedrock, PlayerList::Whitelist, false) => "allowlist remove",
        (_, PlayerList::Ops, true) => "op",
        (_, PlayerList::Ops, false) => "deop",
        (Edition::Java, PlayerList::Banned, true) => "ban",
        (Edition::Java, PlayerList::Banned, false) => "pardon",
        (Edition::Bedrock, PlayerList::Banned, _) => bail!(ErrorInfo::UnsupportedOperation {
            reason: Unsupported::BedrockBans,
        }),
    };
    Ok((player, format!("{verb} {}", argument(edition, player))))
}

fn argument(edition: Edition, player: &str) -> String {
    match edition {
        Edition::Java => player.to_string(),
        Edition::Bedrock => format!("\"{player}\""),
    }
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

/// A name is spliced into a console command, so anything that could end the
/// argument or start another command is refused before it gets there. Bedrock
/// names are quoted, so they may hold spaces but not what could close the quote.
fn checked_name(edition: Edition, player: &str) -> Result<&str> {
    let player = player.trim();
    let allowed = |c: char| match edition {
        Edition::Java => c.is_ascii_alphanumeric() || c == '_' || c == '.',
        Edition::Bedrock => !c.is_control() && c != '"' && c != '\\',
    };
    if player.is_empty() || player.chars().count() > MAX_NAME_LEN || !player.chars().all(allowed) {
        bail!(ErrorInfo::InvalidValue {
            field: Field::Player,
            reason: Reason::PlayerName,
        });
    }
    Ok(player)
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

#[derive(Deserialize)]
struct AllowlistEntry {
    name: String,
    #[serde(default)]
    xuid: String,
}

#[derive(Deserialize)]
struct Permission {
    permission: String,
    xuid: String,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Vec<T> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_else(|error| {
        tracing::warn!(path = %path.display(), %error, "cannot read a player list");
        Vec::new()
    })
}

/// Bedrock's `allowlist.json`; the XUID rides in the entry's `uuid`.
fn read_allowlist(path: &Path) -> Vec<PlayerEntry> {
    let mut entries: Vec<PlayerEntry> = read_json::<AllowlistEntry>(path)
        .into_iter()
        .map(|entry| PlayerEntry {
            name: entry.name,
            uuid: entry.xuid,
            reason: None,
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

/// Bedrock's operators from `permissions.json`, named from the allowlist when
/// the same XUID is on it and by the XUID otherwise.
fn read_operators(path: &Path, allowlist: &[PlayerEntry]) -> Vec<PlayerEntry> {
    let mut entries: Vec<PlayerEntry> = read_json::<Permission>(path)
        .into_iter()
        .filter(|entry| entry.permission == "operator")
        .map(|entry| PlayerEntry {
            name: allowlist
                .iter()
                .find(|listed| !listed.uuid.is_empty() && listed.uuid == entry.xuid)
                .map_or_else(|| entry.xuid.clone(), |listed| listed.name.clone()),
            uuid: entry.xuid,
            reason: None,
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
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
    fn a_name_that_could_break_out_of_the_command_is_refused() {
        for name in ["", "a b", "Steve\nstop", "@a", "x;y", &"x".repeat(33)] {
            assert!(checked_name(Edition::Java, name).is_err(), "{name:?}");
        }
        assert_eq!(checked_name(Edition::Java, " Steve_1 ").unwrap(), "Steve_1");
        assert_eq!(checked_name(Edition::Java, ".Bedrock").unwrap(), ".Bedrock");
    }

    #[test]
    fn a_bedrock_name_is_quoted_and_cannot_close_the_quote() {
        for name in [
            "",
            "a\"b",
            "a\\b",
            "Steve\nstop",
            "a\rb",
            "a\0b",
            &"x".repeat(33),
        ] {
            assert!(checked_name(Edition::Bedrock, name).is_err(), "{name:?}");
        }
        let set = |list, listed| set_command(Edition::Bedrock, " Cool Steve ", list, listed);
        assert_eq!(
            set(PlayerList::Whitelist, true).unwrap().1,
            r#"allowlist add "Cool Steve""#
        );
        assert_eq!(
            set(PlayerList::Whitelist, false).unwrap().1,
            r#"allowlist remove "Cool Steve""#
        );
        assert_eq!(set(PlayerList::Ops, true).unwrap().1, r#"op "Cool Steve""#);
        assert_eq!(
            set(PlayerList::Ops, false).unwrap().1,
            r#"deop "Cool Steve""#
        );
        assert_eq!(argument(Edition::Bedrock, "A B"), r#""A B""#);
    }

    #[test]
    fn bedrock_has_no_ban_list() {
        for listed in [true, false] {
            let error = set_command(Edition::Bedrock, "Steve", PlayerList::Banned, listed)
                .unwrap_err()
                .downcast::<ErrorInfo>()
                .unwrap();
            assert_eq!(
                error,
                ErrorInfo::UnsupportedOperation {
                    reason: Unsupported::BedrockBans
                }
            );
        }
        assert_eq!(
            set_command(Edition::Java, "Steve", PlayerList::Banned, true)
                .unwrap()
                .1,
            "ban Steve"
        );
        assert_eq!(player_lists(Edition::Bedrock).len(), 2);
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
    fn the_bedrock_files_are_read_and_a_bad_one_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let allow = dir.path().join(ALLOWLIST_FILE);
        let perms = dir.path().join(PERMISSIONS_FILE);
        assert!(read_allowlist(&allow).is_empty());
        assert!(read_operators(&perms, &[]).is_empty());

        std::fs::write(
            &allow,
            r#"[{"name":"Zed","xuid":"2","ignoresPlayerLimit":false},{"name":"Cool Steve","xuid":"1","ignoresPlayerLimit":false},{"name":"NoXuid"}]"#,
        )
        .unwrap();
        std::fs::write(
            &perms,
            r#"[{"permission":"operator","xuid":"1"},{"permission":"member","xuid":"2"},{"permission":"operator","xuid":"9"}]"#,
        )
        .unwrap();
        let whitelist = read_allowlist(&allow);
        assert_eq!(whitelist[0].name, "Cool Steve");
        assert_eq!(whitelist[0].uuid, "1");
        let ops = read_operators(&perms, &whitelist);
        let named: Vec<_> = ops.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(named, ["9", "Cool Steve"]);

        std::fs::write(&allow, "not json").unwrap();
        std::fs::write(&perms, "{").unwrap();
        assert!(read_allowlist(&allow).is_empty());
        assert!(read_operators(&perms, &[]).is_empty());
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
