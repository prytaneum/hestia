use std::sync::LazyLock;

use proto::error::Reason;
use proto::keys::{KeyKind, KeyRule};

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
}
