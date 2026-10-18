use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{bail, Context, Result};
use proto::error::Reason;
use proto::keys::{KeyKind, KeyRule};
use proto::server::ServerProperty;

use crate::keys::{self, rule};

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

const INT_MAX: i64 = i32::MAX as i64;

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

static TABLE: LazyLock<Vec<KeyRule>> = LazyLock::new(|| {
    let booleans = BOOLEANS.iter().map(|key| rule(key, KeyKind::Boolean));
    let integers = INTEGERS
        .iter()
        .map(|&(key, min, max)| rule(key, KeyKind::Integer { min, max }));
    let choices = CHOICES.iter().map(|&(key, values)| {
        let values = values.iter().map(|v| v.to_string()).collect();
        rule(key, KeyKind::Choice { values })
    });
    booleans.chain(integers).chain(choices).collect()
});

pub(crate) fn rules() -> &'static [KeyRule] {
    &TABLE
}

pub(crate) fn validate(key: &str, value: &str) -> Result<(), Reason> {
    match TABLE.iter().find(|rule| rule.key == key) {
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

/// `server.properties` keys hestia owns; a `config set` to any of them is
/// rejected (the game port is fixed at create, rcon is configured at start).
pub(crate) const MANAGED: &[&str] = &["server-port", "enable-rcon", "rcon.port", "rcon.password"];

/// Never sent to a front-end: nothing there needs the console's credential.
pub(crate) const SECRET: &[&str] = &["rcon.password"];

pub(crate) const PROPERTIES: &str = "server.properties";

/// Parse `server.properties` into key/value pairs, skipping blank and comment
/// lines. Values are kept verbatim; the split is on the first `=`.
pub(crate) fn read_properties(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

/// The union of the schema's keys and the live file's, ordered by key, each
/// with the live value (the schema's default when the file has none).
pub(crate) fn describe_properties(
    live: &[(String, String)],
    schema: &[(String, String)],
) -> Vec<ServerProperty> {
    let value_of = |pairs: &[(String, String)], key: &str| {
        pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    let hardcore = value_of(live, "hardcore").is_some_and(|v| prop_is_true(&v));
    let keys: BTreeSet<&String> = schema.iter().chain(live).map(|(key, _)| key).collect();

    keys.into_iter()
        .map(|key| {
            let default = value_of(schema, key);
            let secret = SECRET.contains(&key.as_str());
            ServerProperty {
                key: key.clone(),
                value: match secret {
                    true => String::new(),
                    false => value_of(live, key)
                        .or_else(|| default.clone())
                        .unwrap_or_default(),
                },
                default: default.filter(|_| !secret),
                managed: MANAGED.contains(&key.as_str()),
                secret,
                locked: hardcore && HARDCORE_PINNED.contains(&key.as_str()),
            }
        })
        .collect()
}

pub(crate) fn read_property(path: &Path, key: &str) -> Option<String> {
    read_properties(path)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// The `server.properties` keys hardcore mode interlocks: enabling it forces
/// `difficulty=hard` and `gamemode=survival`.
pub(crate) const HARDCORE_KEYS: &[&str] = &["hardcore", "difficulty", "gamemode"];

/// The two of those hardcore fixes while it is on.
const HARDCORE_PINNED: &[&str] = &["difficulty", "gamemode"];

fn prop_is_true(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("true")
}

// Modern versions serialize these as names; pre-1.13 used the numeric ids
// (difficulty 3 = hard, gamemode 0 = survival), so accept both.
fn prop_is_hard(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("hard") || value == "3"
}

fn prop_is_survival(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("survival") || value == "0"
}

/// Enforce Minecraft's rule that hardcore forces `difficulty=hard` and
/// `gamemode=survival`: reject a `config set` to any of the three keys that
/// would leave them contradicting, so the file can never drift into a state
/// the game silently overrides at runtime. `key`/`value` is the pending edit.
pub(crate) fn check_hardcore_invariant(properties: &Path, key: &str, value: &str) -> Result<()> {
    let effective = |name: &str| -> Option<String> {
        if name == key {
            Some(value.to_string())
        } else {
            read_property(properties, name)
        }
    };
    if !effective("hardcore").as_deref().is_some_and(prop_is_true) {
        return Ok(());
    }
    let bad_difficulty = effective("difficulty").is_some_and(|v| !prop_is_hard(&v));
    let bad_gamemode = effective("gamemode").is_some_and(|v| !prop_is_survival(&v));
    if !bad_difficulty && !bad_gamemode {
        return Ok(());
    }
    let locked = |value: &str| proto::error::ErrorInfo::HardcoreLocked {
        key: key.to_string(),
        value: value.to_string(),
    };
    bail!(match key {
        "difficulty" => locked("hard"),
        "gamemode" => locked("survival"),
        _ => proto::error::ErrorInfo::HardcoreRequires {
            settings: [
                bad_difficulty.then_some("difficulty=hard"),
                bad_gamemode.then_some("gamemode=survival"),
            ]
            .into_iter()
            .flatten()
            .map(str::to_string)
            .collect(),
        },
    });
}

/// The server's world directory name (`level-name`, default `world`), read from
/// `server.properties` in `data_dir`. This is where the server keeps its world,
/// and so where datapacks install.
pub(crate) fn level_name(data_dir: &Path) -> String {
    read_property(&data_dir.join(PROPERTIES), "level-name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "world".to_string())
}

/// Add the keys the file does not yet carry, leaving every present key at its
/// current value. This is how a version update introduces the keys its schema
/// added without touching what the user set — and how a fresh server's file
/// starts out as the full schema. Keys no current version knows stay in the
/// file: it holds values, and deleting lines the user or a mod may own is worse
/// than the drift.
pub(crate) fn seed_properties(path: &Path, schema: &[(String, String)]) -> Result<()> {
    let present: HashSet<String> = read_properties(path).into_iter().map(|(k, _)| k).collect();
    let missing: Vec<(&str, String)> = schema
        .iter()
        .filter(|(key, _)| !present.contains(key))
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    merge_properties(path, &missing)
}

/// Rewrite `entries` into the properties file, preserving every other line
/// (user edits included) and appending keys not yet present. The data
/// directory appears on demand with the file.
pub(crate) fn merge_properties(path: &Path, entries: &[(&str, String)]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    for (key, value) in entries {
        let prefix = format!("{key}=");
        let entry = format!("{key}={value}");
        match lines
            .iter_mut()
            .find(|l| l.trim_start().starts_with(&prefix))
        {
            Some(line) => *line = entry,
            None => lines.push(entry),
        }
    }
    common::atomic::write(path, (lines.join("\n") + "\n").as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_property_is_held_to_its_rule_and_an_unknown_one_is_not() {
        assert_eq!(validate("view-distance", "12"), Ok(()));
        assert_eq!(
            validate("view-distance", "900"),
            Err(Reason::OutOfRange { min: 3, max: 32 })
        );
        assert_eq!(validate("max-players", "abc"), Err(Reason::NotWholeNumber));
        assert_eq!(validate("pvp", "maybe"), Err(Reason::NotBoolean));
        assert_eq!(validate("a-future-property", "anything"), Ok(()));
    }

    fn schema(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn properties_are_whatever_the_schema_and_the_file_carry() {
        let described = describe_properties(
            &schema(&[("motd", "mine"), ("retired", "leftover")]),
            &schema(&[("motd", "A Minecraft Server"), ("pvp", "true")]),
        );
        let row = |key: &str| described.iter().find(|p| p.key == key).unwrap();

        assert_eq!(
            described.iter().map(|p| &p.key).collect::<Vec<_>>(),
            ["motd", "pvp", "retired"]
        );
        assert_eq!(row("motd").value, "mine");
        assert_eq!(row("motd").default.as_deref(), Some("A Minecraft Server"));
        assert_eq!(row("pvp").value, "true");
        assert_eq!(row("retired").default, None);
    }

    #[test]
    fn a_managed_key_is_flagged_and_the_credential_is_withheld() {
        let pairs = schema(&[("server-port", "25565"), ("rcon.password", "hunter2")]);
        let described = describe_properties(&pairs, &pairs);
        let row = |key: &str| described.iter().find(|p| p.key == key).unwrap();

        assert!(row("server-port").managed);
        assert_eq!(row("server-port").value, "25565");
        assert!(row("rcon.password").secret);
        assert_eq!(row("rcon.password").value, "");
        assert_eq!(row("rcon.password").default, None);
    }

    #[test]
    fn hardcore_locks_what_it_pins_only_while_it_is_on() {
        let on = schema(&[("hardcore", "true"), ("difficulty", "hard"), ("motd", "")]);
        let described = describe_properties(&on, &[]);
        let locked =
            |list: &[ServerProperty], key: &str| list.iter().find(|p| p.key == key).unwrap().locked;
        assert!(locked(&described, "difficulty"));
        assert!(!locked(&described, "motd"));

        let off = schema(&[("hardcore", "false"), ("difficulty", "easy")]);
        assert!(!locked(&describe_properties(&off, &[]), "difficulty"));
    }

    #[test]
    fn seeding_adds_new_keys_and_keeps_every_existing_value() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(PROPERTIES);

        // A fresh server starts out as the whole schema.
        seed_properties(&path, &schema(&[("motd", "A Minecraft Server")])).unwrap();
        assert_eq!(
            read_property(&path, "motd").as_deref(),
            Some("A Minecraft Server")
        );

        // The user changes a value and the file keeps a key from an older
        // version; an update's schema adds one key.
        merge_properties(&path, &[("motd", "mine".to_string())]).unwrap();
        merge_properties(&path, &[("retired", "leftover".to_string())]).unwrap();
        seed_properties(
            &path,
            &schema(&[("motd", "A Minecraft Server"), ("new-key", "default")]),
        )
        .unwrap();

        assert_eq!(read_property(&path, "motd").as_deref(), Some("mine"));
        assert_eq!(read_property(&path, "new-key").as_deref(), Some("default"));
        assert_eq!(
            read_property(&path, "retired").as_deref(),
            Some("leftover"),
            "a key the new version does not know is values, not schema — never deleted"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
