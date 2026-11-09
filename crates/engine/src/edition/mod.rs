//! Everything that differs between a Java and a Bedrock server, in one place.
//! Each edition is a module of the same small set of functions; this file picks
//! the module by the record's [`Edition`] and holds what the two share. A flow
//! never matches on an edition — it asks here — so an edition-level feature is
//! one change in one module, and a provider only describes its distribution.

mod bedrock;
mod java;

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};
use proto::content::{ContentKind, InstalledContent, UntrackedFile};
use proto::error::{ErrorInfo, Field, Reason, Unsupported};
use proto::instance::WorldInfo;
use proto::keys::{KeyKind, KeyRule};
use proto::minecraft::Edition;
use proto::players::{PlayerEntry, PlayerList, ServerPlayersResult};
use proto::process::{LogSource, StopMethod};
use proto::server::{ServerPingResult, ServerPort};

use crate::cancel::Job;
use crate::console::Transport as ConsoleTransport;
use crate::engine::Engine;
use crate::hold::Truncation;
use crate::jvm::JavaSettings;
use crate::keys::{self, invalid, rule};
use crate::minecraft::{Loads, Side};
use crate::process::ProcessSupervisor;
use crate::servers::{ServerRecord, Transport};

const INT_MAX: i64 = i32::MAX as i64;
const MAX_NAME_LEN: usize = 32;

/// The editions a side can run: a server either, an instance only Java.
pub fn offered(side: Side) -> &'static [Edition] {
    match side {
        Side::Server => &[Edition::Java, Edition::Bedrock],
        Side::Client => &[Edition::Java],
    }
}

/// Everything an entry of this edition, flavor and side accepts: whatever the
/// flavor's own loader takes, plus what the edition and side read for themselves.
pub fn accepted_kinds(side: Side, edition: Edition, loads: Loads) -> Vec<ContentKind> {
    let mut kinds: Vec<ContentKind> = loads.into_iter().collect();
    kinds.extend(match edition {
        Edition::Java => java::packs(side),
        Edition::Bedrock => bedrock::packs(),
    });
    kinds
}

/// Every rule a server's settings are checked against.
pub fn config_rules(edition: Edition) -> Vec<KeyRule> {
    let mut rules = match edition {
        Edition::Java => JavaSettings::rules(),
        Edition::Bedrock => Vec::new(),
    };
    rules.extend(property_rules(edition).iter().cloned());
    rules
}

fn property_rules(edition: Edition) -> &'static [KeyRule] {
    match edition {
        Edition::Java => java::property_rules(),
        Edition::Bedrock => bedrock::property_rules(),
    }
}

fn table(
    booleans: &[&str],
    integers: &[(&str, i64, i64)],
    choices: &[(&str, &[&str])],
) -> Vec<KeyRule> {
    let booleans = booleans.iter().map(|key| rule(key, KeyKind::Boolean));
    let integers = integers
        .iter()
        .map(|&(key, min, max)| rule(key, KeyKind::Integer { min, max }));
    let choices = choices.iter().map(|&(key, values)| {
        let values = values.iter().map(|v| v.to_string()).collect();
        rule(key, KeyKind::Choice { values })
    });
    booleans.chain(integers).chain(choices).collect()
}

fn validate(edition: Edition, key: &str, value: &str) -> Result<(), Reason> {
    match property_rules(edition).iter().find(|rule| rule.key == key) {
        Some(rule) => keys::validate(&rule.kind, value),
        None => {
            tracing::debug!(
                key,
                "no value rule for this property; accepting it as a string"
            );
            Ok(())
        }
    }
}

/// Hold a pending `server.properties` edit to its rule and to whatever
/// invariant the edition keeps between keys.
pub(crate) fn check_setting(edition: Edition, file: &Path, key: &str, value: &str) -> Result<()> {
    validate(edition, key, value).map_err(invalid(key))?;
    match edition {
        Edition::Java => java::check_invariants(file, key, value),
        Edition::Bedrock => Ok(()),
    }
}

/// The `server.properties` keys hestia owns; a `config set` to one is rejected.
/// The property a server's listed name lives in.
pub fn name_key(edition: Edition) -> &'static str {
    match edition {
        Edition::Java => java::NAME_KEY,
        Edition::Bedrock => bedrock::NAME_KEY,
    }
}

pub(crate) fn managed_keys(edition: Edition) -> &'static [&'static str] {
    match edition {
        Edition::Java => java::MANAGED,
        Edition::Bedrock => bedrock::MANAGED,
    }
}

/// Where the game port is searched from and what must be free on it.
pub(crate) fn game_port_base(edition: Edition) -> (u16, Transport) {
    match edition {
        Edition::Java => (java::GAME_PORT_BASE, Transport::Tcp),
        Edition::Bedrock => (bedrock::GAME_PORT_BASE, Transport::TcpAndUdp),
    }
}

/// Every port the server binds, the game port first — what a user forwards.
pub(crate) fn ports(record: &ServerRecord) -> Vec<ServerPort> {
    match record.edition {
        Edition::Java => java::ports(record),
        Edition::Bedrock => bedrock::ports(record),
    }
}

/// Write what the edition needs on disk at provisioning.
pub(crate) fn provision(edition: Edition, data_dir: &Path) -> Result<()> {
    match edition {
        Edition::Java => java::provision(data_dir),
        Edition::Bedrock => Ok(()),
    }
}

/// Reconcile `server.properties` and the record with the claimed ports before a
/// spawn. The game port never moves — an outside squatter is an error.
pub(crate) fn configure_start(
    record: &mut ServerRecord,
    file: &Path,
    claimed: HashSet<u16>,
) -> Result<()> {
    match record.edition {
        Edition::Java => java::configure_start(record, file, claimed),
        Edition::Bedrock => bedrock::configure_start(record, file, claimed),
    }
}

/// How the supervisor runs a server process of this edition.
pub struct ProcessIo {
    pub log: LogSource,
    pub stdin: bool,
    pub stop: StopMethod,
}

pub fn process_io(edition: Edition, cwd: &Path) -> ProcessIo {
    match edition {
        Edition::Java => java::process_io(cwd),
        Edition::Bedrock => bedrock::process_io(),
    }
}

/// Whether the server's console can be reached right now.
pub fn has_console(record: &ServerRecord) -> bool {
    match record.edition {
        Edition::Java => record.rcon.is_some(),
        Edition::Bedrock => true,
    }
}

pub(crate) fn console_transport(
    record: &ServerRecord,
    processes: &Arc<ProcessSupervisor>,
) -> Result<ConsoleTransport> {
    match record.edition {
        Edition::Java => java::console_transport(record),
        Edition::Bedrock => Ok(ConsoleTransport::Pipe(Arc::clone(processes))),
    }
}

pub(crate) async fn status(engine: &Engine, record: &ServerRecord) -> Result<ServerPingResult> {
    match record.edition {
        Edition::Java => java::status(record).await,
        Edition::Bedrock => bedrock::status(engine, record).await,
    }
}

/// Pause world writes before archiving a live server; the lengths to copy each
/// file to (empty when the edition needs no truncation).
pub(crate) async fn pause_saves(
    engine: &Engine,
    edition: Edition,
    server: &str,
) -> Result<Truncation> {
    match edition {
        Edition::Java => java::pause_saves(engine, server).await,
        Edition::Bedrock => bedrock::pause_saves(engine, server).await,
    }
}

pub(crate) fn resume_command(edition: Edition) -> &'static str {
    match edition {
        Edition::Java => java::RESUME,
        Edition::Bedrock => bedrock::RESUME,
    }
}

/// The worlds a server has: Java the one `level-name` names, Bedrock every
/// folder under `worlds/`, the active one first.
pub(crate) fn worlds(edition: Edition, data_dir: &Path, active: &str) -> Vec<WorldInfo> {
    match edition {
        Edition::Java => java::worlds(data_dir, active),
        Edition::Bedrock => bedrock::worlds(data_dir, active),
    }
}

pub(crate) fn import_world(
    edition: Edition,
    data_dir: &Path,
    archive: &Path,
    job: &Job<'_>,
) -> Result<WorldInfo> {
    match edition {
        Edition::Java => bail!(ErrorInfo::UnsupportedOperation {
            reason: Unsupported::WorldImportBedrockOnly,
        }),
        Edition::Bedrock => bedrock::import_world(data_dir, archive, job),
    }
}

/// Bring the entry's managed content onto the server's disk before a start.
pub(crate) fn sync_content(
    edition: Edition,
    entry_dir: &Path,
    data_dir: &Path,
    level: &str,
) -> Result<()> {
    crate::content::install::sync(entry_dir, data_dir, &[level.to_string()])?;
    match edition {
        Edition::Java => Ok(()),
        Edition::Bedrock => crate::content::bedrock::sync(entry_dir, data_dir, level),
    }
}

pub(crate) fn untracked(
    edition: Edition,
    data_dir: &Path,
    kind: ContentKind,
    items: &[InstalledContent],
    worlds: &[String],
) -> Vec<UntrackedFile> {
    match edition {
        Edition::Java => crate::content::install::untracked(data_dir, kind, items, worlds),
        Edition::Bedrock => crate::content::bedrock::untracked(data_dir, kind),
    }
}

/// The lists a server of this edition keeps; Bedrock has no ban list.
pub fn player_lists(edition: Edition) -> Vec<PlayerList> {
    match edition {
        Edition::Java => vec![PlayerList::Whitelist, PlayerList::Ops, PlayerList::Banned],
        Edition::Bedrock => vec![PlayerList::Whitelist, PlayerList::Ops],
    }
}

/// The lists from the files the server keeps beside its world.
pub(crate) fn read_players(
    edition: Edition,
    data_dir: &Path,
    online: Vec<PlayerEntry>,
) -> ServerPlayersResult {
    match edition {
        Edition::Java => java::read_players(data_dir, online),
        Edition::Bedrock => bedrock::read_players(data_dir, online),
    }
}

pub(crate) fn list_command(edition: Edition) -> &'static str {
    match edition {
        Edition::Java => "list uuids",
        Edition::Bedrock => "list",
    }
}

/// BDS resolves an operator by connection, not by account, so the player must
/// be on the server.
pub(crate) fn op_needs_online(edition: Edition) -> bool {
    edition == Edition::Bedrock
}

/// The console command for a list change, with the name it was built for.
pub(crate) fn set_command(
    edition: Edition,
    player: &str,
    list: PlayerList,
    listed: bool,
) -> Result<(&str, String)> {
    let player = checked_name(edition, player)?;
    let verb = match edition {
        Edition::Java => java::list_verb(list, listed)?,
        Edition::Bedrock => bedrock::list_verb(list, listed)?,
    };
    Ok((player, format!("{verb} {}", argument(edition, player))))
}

pub(crate) fn kick_command(edition: Edition, player: &str) -> Result<String> {
    let player = checked_name(edition, player)?;
    Ok(format!("kick {}", argument(edition, player)))
}

fn argument(edition: Edition, player: &str) -> String {
    match edition {
        Edition::Java => player.to_string(),
        Edition::Bedrock => format!("\"{player}\""),
    }
}

/// A name is spliced into a console command, so anything that could end the
/// argument or start another command is refused before it gets there. Bedrock
/// names are quoted, so they may hold spaces but not what could close the quote.
fn checked_name(edition: Edition, player: &str) -> Result<&str> {
    let player = player.trim();
    let allowed = |c: char| match edition {
        Edition::Java => java::name_char(c),
        Edition::Bedrock => bedrock::name_char(c),
    };
    if player.is_empty() || player.chars().count() > MAX_NAME_LEN || !player.chars().all(allowed) {
        bail!(ErrorInfo::InvalidValue {
            field: Field::Player,
            reason: Reason::PlayerName,
        });
    }
    Ok(player)
}

#[cfg(test)]
mod tests {

    #[test]
    fn each_edition_names_the_key_a_server_is_listed_under() {
        assert_eq!(name_key(Edition::Java), "motd");
        assert_eq!(name_key(Edition::Bedrock), "server-name");
    }

    use super::*;

    #[test]
    fn a_known_property_is_held_to_its_rule_and_an_unknown_one_is_not() {
        assert_eq!(validate(Edition::Java, "view-distance", "12"), Ok(()));
        assert_eq!(
            validate(Edition::Java, "view-distance", "900"),
            Err(Reason::OutOfRange { min: 3, max: 32 })
        );
        assert_eq!(
            validate(Edition::Java, "max-players", "abc"),
            Err(Reason::NotWholeNumber)
        );
        assert_eq!(
            validate(Edition::Java, "pvp", "maybe"),
            Err(Reason::NotBoolean)
        );
        assert_eq!(
            validate(Edition::Java, "a-future-property", "anything"),
            Ok(())
        );
    }

    #[test]
    fn bedrock_rules_hold_its_own_keys_to_its_own_values() {
        let check = |key, value| validate(Edition::Bedrock, key, value);
        assert_eq!(check("gamemode", "creative"), Ok(()));
        assert!(matches!(
            check("gamemode", "spectator"),
            Err(Reason::NotOneOf { .. })
        ));
        assert_eq!(
            check("tick-distance", "3"),
            Err(Reason::OutOfRange { min: 4, max: 12 })
        );
        assert_eq!(
            check("view-distance", "4"),
            Err(Reason::OutOfRange {
                min: 5,
                max: INT_MAX
            })
        );
        assert_eq!(check("allow-cheats", "maybe"), Err(Reason::NotBoolean));
        assert_eq!(check("server-name", "any; thing"), Ok(()));
        assert_eq!(
            validate(Edition::Java, "tick-distance", "3"),
            Ok(()),
            "a Java server has no such rule"
        );
    }

    #[test]
    fn transport_is_managed_on_bedrock_and_not_a_rule() {
        assert!(managed_keys(Edition::Bedrock).contains(&"transport"));
        assert!(property_rules(Edition::Bedrock)
            .iter()
            .all(|rule| rule.key != "transport"));
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
    fn a_world_import_is_bedrock_only() {
        let cancel = crate::cancel::Cancel::new();
        let report = |_: &proto::minecraft::ProvisionProgress| {};
        let error = import_world(
            Edition::Java,
            Path::new("."),
            Path::new("a.mcworld"),
            &Job::new(&report, &cancel),
        )
        .unwrap_err()
        .downcast::<ErrorInfo>()
        .unwrap();
        assert_eq!(
            error,
            ErrorInfo::UnsupportedOperation {
                reason: Unsupported::WorldImportBedrockOnly
            }
        );
    }
}
