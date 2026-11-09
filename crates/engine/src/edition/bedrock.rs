//! Bedrock servers: a native executable, a console over its stdin, a status
//! read from that console, saves held with the `save hold` / `save query`
//! protocol. The game speaks NetherNet only — BDS 1.26 refuses RakNet clients —
//! so `transport` is pinned at every start and the ports follow from it.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use proto::content::ContentKind;
use proto::error::{ErrorInfo, Unsupported};
use proto::instance::WorldInfo;
use proto::keys::KeyRule;
use proto::players::{PlayerEntry, PlayerList, ServerPlayersResult};
use proto::process::{LogSource, StopMethod};
use proto::server::{PortProtocol, ServerPingResult, ServerPort};
use serde::Deserialize;

use super::{table, ProcessIo, INT_MAX};
use crate::cancel::Job;
use crate::engine::Engine;
use crate::hold::{self, HoldError, Truncation};
use crate::minecraft::bedrock::world;
use crate::properties;
use crate::servers::{allocate_port, unavailable, ServerRecord, Transport};

pub(super) const GAME_PORT_BASE: u16 = 19132;
const SECOND_PORT_BASE: u16 = 19133;
pub(super) const RESUME: &str = "save resume";
const HOLD_POLL: Duration = Duration::from_millis(500);
const HOLD_LIMIT: Duration = Duration::from_secs(30);

const ALLOWLIST_FILE: &str = "allowlist.json";
const PERMISSIONS_FILE: &str = "permissions.json";

/// Keys hestia owns: the ports, and the transport BDS 1.26 only serves one way.
pub(super) const NAME_KEY: &str = "server-name";

pub(super) const MANAGED: &[&str] = &[
    "server-port",
    "server-portv6",
    "server-udp-ports",
    "transport",
];

const BOOLEANS: &[&str] = &[
    "allow-cheats",
    "allow-inbound-script-debugging",
    "allow-list",
    "allow-outbound-script-debugging",
    "block-network-ids-are-hashes",
    "client-side-chunk-generation-enabled",
    "content-log-console-output-enabled",
    "content-log-file-enabled",
    "disable-custom-skins",
    "disable-persona",
    "disable-player-interaction",
    "enable-lan-visibility",
    "force-gamemode",
    "online-mode",
    "server-authoritative-dismount-strict",
    "server-authoritative-entity-interactions-strict",
    "server-authoritative-movement-strict",
    "texturepack-required",
];

const INTEGERS: &[(&str, i64, i64)] = &[
    ("compression-threshold", 0, 65535),
    ("max-players", 1, INT_MAX),
    ("max-threads", 0, INT_MAX),
    ("player-idle-timeout", 0, INT_MAX),
    ("server-port", 1, 65535),
    ("server-portv6", 1, 65535),
    ("tick-distance", 4, 12),
    ("view-distance", 5, INT_MAX),
];

const ENFORCEMENT: &[&str] = &["disabled", "observe", "enforce"];

const CHOICES: &[(&str, &[&str])] = &[
    ("chat-restriction", &["None", "Dropped", "Disabled"]),
    ("compression-algorithm", &["zlib", "snappy"]),
    (
        "content-log-level",
        &["error", "warning", "info", "verbose"],
    ),
    (
        "default-player-permission-level",
        &["visitor", "member", "operator"],
    ),
    ("difficulty", &["peaceful", "easy", "normal", "hard"]),
    ("gamemode", &["survival", "creative", "adventure"]),
    (
        "script-debugger-auto-attach",
        &["disabled", "connect", "listen"],
    ),
    (
        "server-authoritative-entity-attack-range-validation",
        ENFORCEMENT,
    ),
    (
        "server-authoritative-entity-attack-request-validation",
        ENFORCEMENT,
    ),
];

static TABLE: LazyLock<Vec<KeyRule>> = LazyLock::new(|| table(BOOLEANS, INTEGERS, CHOICES));

pub(super) fn property_rules() -> &'static [KeyRule] {
    &TABLE
}

pub(super) fn packs() -> Vec<ContentKind> {
    vec![ContentKind::BehaviorPack, ContentKind::ResourcePack]
}

/// NetherNet listens on TCP `server-port` (dual-stack) and takes its client UDP
/// port from `server-udp-ports`; `server-portv6` is ignored there but kept equal.
pub(super) fn ports(record: &ServerRecord) -> Vec<ServerPort> {
    let game = ServerPort {
        port: record.game_port,
        protocol: PortProtocol::Tcp,
    };
    let second = record.second_port.map(|port| ServerPort {
        port,
        protocol: PortProtocol::Udp,
    });
    std::iter::once(game).chain(second).collect()
}

pub(super) fn configure_start(
    record: &mut ServerRecord,
    file: &Path,
    claimed: HashSet<u16>,
) -> Result<()> {
    let game = record.game_port;
    if unavailable(&claimed, game, Transport::TcpAndUdp) {
        bail!(ErrorInfo::PortUnavailable { port: game });
    }
    let mut taken = claimed;
    taken.insert(game);
    let second = match record.second_port {
        Some(port) if !unavailable(&taken, port, Transport::Udp) => port,
        _ => allocate_port(SECOND_PORT_BASE, &taken, Transport::Udp)?,
    };
    properties::merge_properties(file, &start_entries(game, second))?;
    record.second_port = Some(second);
    Ok(())
}

fn start_entries(game: u16, second: u16) -> Vec<(&'static str, String)> {
    vec![
        ("server-port", game.to_string()),
        ("server-portv6", second.to_string()),
        ("server-udp-ports", second.to_string()),
        ("transport", "nethernet".to_string()),
    ]
}

pub(super) fn process_io() -> ProcessIo {
    ProcessIo {
        log: LogSource::Capture,
        stdin: true,
        stop: StopMethod::Line("stop".into()),
    }
}

/// BDS answers no ping, so the player count comes from the console, the name
/// from `server.properties` and the version from the record.
pub(super) async fn status(engine: &Engine, record: &ServerRecord) -> Result<ServerPingResult> {
    let reply = engine
        .server_command(&record.id, "list")
        .await
        .context("cannot reach the server's console")?;
    let (players_online, players_max) = crate::engine::parse_player_count(&reply)
        .with_context(|| format!("unexpected list reply from {}", record.id))?;
    let file = engine
        .servers()
        .data_dir(record)
        .join(properties::PROPERTIES);
    Ok(ServerPingResult {
        players_online,
        players_max,
        motd: properties::read_property(&file, "server-name").unwrap_or_default(),
        version: record.profile.game_version.clone(),
        ..Default::default()
    })
}

/// `save hold`, then `save query` until the server lists the files to copy and
/// the length of each.
pub(super) async fn pause_saves(engine: &Engine, server: &str) -> Result<Truncation> {
    tracing::info!(server, "holding world saves");
    engine.server_command(server, "save hold").await?;
    let started = Instant::now();
    let mut polls = 0u32;
    loop {
        polls += 1;
        let reply = engine.server_command(server, "save query").await?;
        if let Some(files) = hold::parse_query(&reply)? {
            tracing::debug!(
                server,
                polls,
                truncated = files.len(),
                "world files are ready"
            );
            return Ok(files);
        }
        if started.elapsed() >= HOLD_LIMIT {
            tracing::debug!(server, polls, "world files never became ready");
            return Err(HoldError::NotReady(HOLD_LIMIT.as_secs()).into());
        }
        tokio::time::sleep(HOLD_POLL).await;
    }
}

pub(super) fn worlds(data_dir: &Path, active: &str) -> Vec<WorldInfo> {
    world::list(&data_dir.join("worlds"), active)
}

pub(super) fn import_world(data_dir: &Path, archive: &Path, job: &Job<'_>) -> Result<WorldInfo> {
    world::import(data_dir, archive, job)
}

pub(super) fn read_players(data_dir: &Path, online: Vec<PlayerEntry>) -> ServerPlayersResult {
    let whitelist = read_allowlist(&data_dir.join(ALLOWLIST_FILE));
    let ops = read_operators(&data_dir.join(PERMISSIONS_FILE), &whitelist);
    ServerPlayersResult {
        online,
        whitelist,
        ops,
        banned: Vec::new(),
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

pub(super) fn list_verb(list: PlayerList, listed: bool) -> Result<&'static str> {
    Ok(match (list, listed) {
        (PlayerList::Whitelist, true) => "allowlist add",
        (PlayerList::Whitelist, false) => "allowlist remove",
        (PlayerList::Ops, true) => "op",
        (PlayerList::Ops, false) => "deop",
        (PlayerList::Banned, _) => bail!(ErrorInfo::UnsupportedOperation {
            reason: Unsupported::BedrockBans,
        }),
    })
}

pub(super) fn name_char(c: char) -> bool {
    !c.is_control() && c != '"' && c != '\\'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_start_pins_nethernet_and_mirrors_the_second_port() {
        assert_eq!(
            start_entries(19132, 19133),
            vec![
                ("server-port", "19132".to_string()),
                ("server-portv6", "19133".to_string()),
                ("server-udp-ports", "19133".to_string()),
                ("transport", "nethernet".to_string()),
            ]
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
}
