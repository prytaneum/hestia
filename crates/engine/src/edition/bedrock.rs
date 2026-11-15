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
use proto::error::ErrorInfo;
use proto::instance::WorldInfo;
use proto::keys::KeyRule;
use proto::process::{LogSource, StopMethod};
use proto::server::{PortProtocol, ServerPingResult, ServerPort};

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
        .server_command(&record.id, super::list_command(record.edition))
        .await
        .context("cannot reach the server's console")?;
    let (players_online, players_max) = parse_player_count(&reply)
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

/// The `N/M` a `list` reply opens with (`There are 0/10 players online:`).
fn parse_player_count(reply: &str) -> Option<(u32, u32)> {
    let (before, _) = reply.split_once(super::ONLINE_MARKER)?;
    let (online, max) = before.split_whitespace().last()?.split_once('/')?;
    Some((online.parse().ok()?, max.parse().ok()?))
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
}
