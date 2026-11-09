//! Java servers: a jar on a Java runtime, a console over RCON, a status from the
//! Server List Ping, saves paused with `save-off`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{bail, Context, Result};
use proto::content::ContentKind;
use proto::error::ErrorInfo;
use proto::instance::WorldInfo;
use proto::keys::KeyRule;
use proto::players::{PlayerEntry, PlayerList, ServerPlayersResult};
use proto::process::{LogSource, StopMethod};
use proto::server::{PortProtocol, ServerPingResult, ServerPort};

use super::{table, ProcessIo, INT_MAX};
use crate::console::Transport as ConsoleTransport;
use crate::engine::Engine;
use crate::hold::Truncation;
use crate::minecraft::{ping, world, Side};
use crate::properties;
use crate::servers::{allocate_port, unavailable, RconConfig, ServerRecord, Transport};

pub(super) const GAME_PORT_BASE: u16 = 25565;
const RCON_PORT_BASE: u16 = 25575;
pub(super) const RESUME: &str = "save-on";

const WHITELIST_FILE: &str = "whitelist.json";
const OPS_FILE: &str = "ops.json";
const BANNED_FILE: &str = "banned-players.json";

/// Keys hestia owns: the game port is fixed at create, rcon is configured at start.
pub(super) const MANAGED: &[&str] = &["server-port", "enable-rcon", "rcon.port", "rcon.password"];

const BOOLEANS: &[&str] = &[
    "accepts-transfers",
    "allow-flight",
    "allow-nether",
    "broadcast-console-to-ops",
    "broadcast-rcon-to-ops",
    "enable-command-block",
    "enable-jmx-monitoring",
    "enable-query",
    "enable-status",
    "enforce-secure-profile",
    "enforce-whitelist",
    "force-gamemode",
    "generate-structures",
    "hardcore",
    "hide-online-players",
    "log-ips",
    "online-mode",
    "prevent-proxy-connections",
    "pvp",
    "require-resource-pack",
    "spawn-animals",
    "spawn-monsters",
    "spawn-npcs",
    "sync-chunk-writes",
    "use-native-transport",
    "white-list",
];

const INTEGERS: &[(&str, i64, i64)] = &[
    ("entity-broadcast-range-percentage", 10, 1000),
    ("function-permission-level", 1, 4),
    ("max-players", 1, INT_MAX),
    ("max-world-size", 1, 29_999_984),
    ("network-compression-threshold", -1, INT_MAX),
    ("op-permission-level", 0, 4),
    ("pause-when-empty-seconds", -1, INT_MAX),
    ("player-idle-timeout", 0, INT_MAX),
    ("query.port", 1, 65535),
    ("rate-limit", 0, INT_MAX),
    ("server-port", 1, 65535),
    ("simulation-distance", 3, 32),
    ("spawn-protection", 0, INT_MAX),
    ("view-distance", 3, 32),
];

// Before 1.13 the game wrote difficulty and gamemode as numeric ids.
const CHOICES: &[(&str, &[&str])] = &[
    (
        "difficulty",
        &["peaceful", "easy", "normal", "hard", "0", "1", "2", "3"],
    ),
    (
        "gamemode",
        &[
            "survival",
            "creative",
            "adventure",
            "spectator",
            "0",
            "1",
            "2",
            "3",
        ],
    ),
    ("region-file-compression", &["deflate", "lz4", "none"]),
];

static TABLE: LazyLock<Vec<KeyRule>> = LazyLock::new(|| table(BOOLEANS, INTEGERS, CHOICES));

pub(super) fn property_rules() -> &'static [KeyRule] {
    &TABLE
}

pub(super) fn check_invariants(file: &Path, key: &str, value: &str) -> Result<()> {
    if properties::HARDCORE_KEYS.contains(&key) {
        properties::check_hardcore_invariant(file, key, value)?;
    }
    Ok(())
}

pub(super) fn packs(side: Side) -> Vec<ContentKind> {
    let mut kinds = Vec::new();
    if side == Side::Client {
        kinds.push(ContentKind::ResourcePack);
        kinds.push(ContentKind::Shader);
    }
    kinds.push(ContentKind::DataPack);
    kinds
}

pub(super) fn ports(record: &ServerRecord) -> Vec<ServerPort> {
    vec![ServerPort {
        port: record.game_port,
        protocol: PortProtocol::Tcp,
    }]
}

pub(super) fn provision(data_dir: &Path) -> Result<()> {
    std::fs::write(data_dir.join("eula.txt"), "eula=true\n").context("cannot write eula.txt")
}

/// The rcon port is internal and reallocates freely; a first start gains its
/// rcon here.
pub(super) fn configure_start(
    record: &mut ServerRecord,
    file: &Path,
    claimed: HashSet<u16>,
) -> Result<()> {
    let game_port = record.game_port;
    if unavailable(&claimed, game_port, Transport::Tcp) {
        bail!(ErrorInfo::PortUnavailable { port: game_port });
    }
    let rcon = match record.rcon.take() {
        Some(cfg) if !unavailable(&claimed, cfg.port, Transport::Tcp) => cfg,
        prior => RconConfig {
            port: allocate_port(RCON_PORT_BASE, &claimed, Transport::Tcp)?,
            password: prior.map(|c| c.password).unwrap_or_else(generate_password),
        },
    };
    properties::merge_properties(
        file,
        &[
            ("server-port", game_port.to_string()),
            ("enable-rcon", "true".to_string()),
            ("rcon.port", rcon.port.to_string()),
            ("rcon.password", rcon.password.clone()),
        ],
    )?;
    record.rcon = Some(rcon);
    Ok(())
}

// Vanilla has no rcon bind-address setting — the listener is reachable from
// the network, so the password is the only barrier. Never log it.
fn generate_password() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("system RNG must be available for the rcon password");
    bytes
        .iter()
        .map(|b| CHARSET[(*b as usize) % CHARSET.len()] as char)
        .collect()
}

pub(super) fn process_io(cwd: &Path) -> ProcessIo {
    ProcessIo {
        log: LogSource::File(cwd.join("logs").join("latest.log")),
        stdin: false,
        stop: StopMethod::Signal,
    }
}

pub(super) fn console_transport(record: &ServerRecord) -> Result<ConsoleTransport> {
    let rcon = record
        .rcon
        .clone()
        .context("this server has no console yet (restart it to enable one)")?;
    Ok(ConsoleTransport::Rcon(rcon))
}

pub(super) async fn status(record: &ServerRecord) -> Result<ServerPingResult> {
    ping::ping(record.game_port).await
}

/// `save-off` stops autosaves and `save-all flush` forces everything pending
/// onto disk, so no truncation is needed.
pub(super) async fn pause_saves(engine: &Engine, server: &str) -> Result<Truncation> {
    engine.server_command(server, "save-off").await?;
    engine.server_command(server, "save-all flush").await?;
    Ok(Truncation::new())
}

pub(super) fn worlds(data_dir: &Path, active: &str) -> Vec<WorldInfo> {
    data_dir
        .join(active)
        .is_dir()
        .then(|| world::describe(data_dir, active))
        .into_iter()
        .collect()
}

pub(super) fn read_players(data_dir: &Path, online: Vec<PlayerEntry>) -> ServerPlayersResult {
    ServerPlayersResult {
        online,
        whitelist: read_list(&data_dir.join(WHITELIST_FILE)),
        ops: read_list(&data_dir.join(OPS_FILE)),
        banned: read_list(&data_dir.join(BANNED_FILE)),
    }
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

pub(super) fn list_verb(list: PlayerList, listed: bool) -> Result<&'static str> {
    Ok(match (list, listed) {
        (PlayerList::Whitelist, true) => "whitelist add",
        (PlayerList::Whitelist, false) => "whitelist remove",
        (PlayerList::Ops, true) => "op",
        (PlayerList::Ops, false) => "deop",
        (PlayerList::Banned, true) => "ban",
        (PlayerList::Banned, false) => "pardon",
    })
}

pub(super) fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

#[cfg(test)]
mod tests {
    use super::*;

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
