//! Per-entry JVM tuning: the memory and extra-flag settings, their validation
//! (memory is checked against the host's installed amount) and merging with the
//! launcher-wide defaults.

use anyhow::Result;
use proto::error::Reason;
use proto::keys::{KeyKind, KeyRule};
use serde::{Deserialize, Serialize};

use crate::keys::{self, invalid};

/// The reserved per-entry JVM setting keys, shared by the server and instance
/// `config` surfaces.
pub const MEMORY_KEY: &str = "memory";
pub const JVM_ARGS_KEY: &str = "jvm-args";

/// Per-entry JVM tuning stored on a server/instance record: a single memory
/// value driving both `-Xms`/`-Xmx`, plus extra flags injected at launch.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct JavaSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub jvm_args: Vec<String>,
}

impl JavaSettings {
    pub(crate) fn rules() -> Vec<KeyRule> {
        vec![
            keys::rule(MEMORY_KEY, KeyKind::Memory),
            keys::rule(JVM_ARGS_KEY, KeyKind::JvmArgs),
        ]
    }

    /// The current value of a JVM key, or `None` (outer) when `key` is not one
    /// of the reserved JVM keys — the signal to fall through to properties. The
    /// inner `None` means the key is a JVM key but unset.
    pub fn get(&self, key: &str) -> Option<Option<String>> {
        match key {
            MEMORY_KEY => Some(self.memory.clone()),
            JVM_ARGS_KEY => Some(if self.jvm_args.is_empty() {
                None
            } else {
                Some(self.jvm_args.join(" "))
            }),
            _ => None,
        }
    }

    /// Apply a JVM key. `Ok(false)` means `key` is not a JVM key (fall through
    /// to properties); an empty value clears the setting; an invalid value is
    /// `Err`.
    pub fn set(&mut self, key: &str, value: &str) -> Result<bool> {
        match key {
            MEMORY_KEY => {
                self.memory = if value.trim().is_empty() {
                    None
                } else {
                    Some(normalize_memory(value).map_err(invalid(key))?)
                };
                Ok(true)
            }
            JVM_ARGS_KEY => {
                self.jvm_args = parse_jvm_args(value).map_err(invalid(key))?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Both reserved keys with their current values (empty when unset), so a
    /// `config list` always shows what is settable.
    pub fn entries(&self) -> Vec<(String, String)> {
        vec![
            (
                MEMORY_KEY.to_string(),
                self.memory.clone().unwrap_or_default(),
            ),
            (JVM_ARGS_KEY.to_string(), self.jvm_args.join(" ")),
        ]
    }

    /// These settings with unset fields filled from `defaults` — how the
    /// launcher-wide `defaults.*` config keys reach an entry that never set
    /// its own.
    pub fn or_defaults(&self, defaults: &JavaSettings) -> JavaSettings {
        JavaSettings {
            memory: self.memory.clone().or_else(|| defaults.memory.clone()),
            jvm_args: if self.jvm_args.is_empty() {
                defaults.jvm_args.clone()
            } else {
                self.jvm_args.clone()
            },
        }
    }

    /// The JVM flags these settings inject: the memory pair first, then the
    /// extra args.
    pub fn flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        if let Some(memory) = &self.memory {
            flags.push(format!("-Xms{memory}"));
            flags.push(format!("-Xmx{memory}"));
        }
        flags.extend(self.jvm_args.iter().cloned());
        flags
    }
}

/// Validate and normalise a memory value: digits followed by one unit char
/// (k/m/g, case-insensitive), the number nonzero, the unit upper-cased and
/// carried as far up the ladder as it divides exactly — an import that speaks
/// megabytes (Prism's `MaxMemAlloc=8192`) stores the `8G` the user chose. An
/// amount above this machine's installed memory is rejected.
pub(crate) fn normalize_memory(value: &str) -> Result<String, Reason> {
    normalize_memory_within(value, crate::host::total_memory())
}

fn normalize_memory_within(value: &str, limit: Option<u64>) -> Result<String, Reason> {
    let trimmed = value.trim();
    let unit = trimmed
        .chars()
        .last()
        .filter(|c| "kmgKMG".contains(*c))
        .ok_or(Reason::MemoryFormat)?;
    let digits = &trimmed[..trimmed.len() - unit.len_utf8()];
    let valid = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.bytes().any(|b| b != b'0');
    if !valid {
        return Err(Reason::MemoryFormat);
    }
    let unit = unit.to_ascii_uppercase();
    if limit.is_some_and(|limit| bytes(digits, unit).is_none_or(|bytes| bytes > limit)) {
        return Err(Reason::MemoryExceedsHost);
    }
    Ok(promote(digits, unit))
}

const LADDER: [char; 3] = ['K', 'M', 'G'];

/// The amount in bytes, or `None` when it does not fit a `u64`.
fn bytes(digits: &str, unit: char) -> Option<u64> {
    let rung = LADDER.iter().position(|u| *u == unit)? as u32;
    digits
        .parse::<u64>()
        .ok()?
        .checked_mul(1024u64.checked_pow(rung + 1)?)
}

/// Carry an amount up the unit ladder while it divides exactly. A number too
/// large for `u64` is left as written — the JVM is the one that rejects it.
fn promote(digits: &str, unit: char) -> String {
    let Ok(mut amount) = digits.parse::<u64>() else {
        return format!("{digits}{unit}");
    };
    let mut at = LADDER
        .iter()
        .position(|u| *u == unit)
        .unwrap_or(LADDER.len());
    while at + 1 < LADDER.len() && amount % 1024 == 0 {
        amount /= 1024;
        at += 1;
    }
    format!("{amount}{}", LADDER.get(at).copied().unwrap_or(unit))
}

/// Split JVM args on whitespace; every token must start with `-`.
pub(crate) fn parse_jvm_args(value: &str) -> Result<Vec<String>, Reason> {
    let mut args = Vec::new();
    for token in value.split_whitespace() {
        if !token.starts_with('-') {
            return Err(Reason::JvmArgsPrefix);
        }
        args.push(token.to_string());
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(memory: Option<&str>, jvm_args: &[&str]) -> JavaSettings {
        JavaSettings {
            memory: memory.map(str::to_string),
            jvm_args: jvm_args.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn or_defaults_fills_only_unset_fields() {
        let defaults = settings(Some("8G"), &["-XX:+UseZGC"]);
        let merged = settings(None, &[]).or_defaults(&defaults);
        assert_eq!(merged.memory.as_deref(), Some("8G"));
        assert_eq!(merged.jvm_args, ["-XX:+UseZGC"]);

        let own = settings(Some("2G"), &["-XX:+UseG1GC"]).or_defaults(&defaults);
        assert_eq!(own.memory.as_deref(), Some("2G"));
        assert_eq!(own.jvm_args, ["-XX:+UseG1GC"]);
    }

    #[test]
    fn memory_validation_normalizes_accepts_and_rejects() {
        let mut s = JavaSettings::default();
        assert!(s.set(MEMORY_KEY, "4g").unwrap());
        assert_eq!(s.memory.as_deref(), Some("4G"));
        assert_eq!(s.flags(), ["-Xms4G", "-Xmx4G"]);
        assert!(s.set(MEMORY_KEY, "2048M").unwrap());
        assert_eq!(s.memory.as_deref(), Some("2G"));
        // Only an exact multiple is promoted; the rest keeps its own unit.
        assert!(s.set(MEMORY_KEY, "6656M").unwrap());
        assert_eq!(s.memory.as_deref(), Some("6656M"));
        assert!(s.set(MEMORY_KEY, "1048576k").unwrap());
        assert_eq!(s.memory.as_deref(), Some("1G"));
        // Empty clears.
        assert!(s.set(MEMORY_KEY, "").unwrap());
        assert_eq!(s.memory, None);
        for bad in ["4", "G", "0G", "4GB", "4.5G", "-4G", "four"] {
            assert!(s.set(MEMORY_KEY, bad).is_err(), "'{bad}' must be rejected");
        }
    }

    #[test]
    fn memory_above_the_installed_amount_is_rejected() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let limit = Some(16 * GIB - 1);
        assert_eq!(normalize_memory_within("15G", limit).unwrap(), "15G");
        assert_eq!(normalize_memory_within("15360M", limit).unwrap(), "15G");
        for over in ["16G", "16384M", "32G", "99999999999999999999G"] {
            assert!(
                normalize_memory_within(over, limit).is_err(),
                "'{over}' must be rejected"
            );
        }
        assert_eq!(
            normalize_memory_within("16G", Some(16 * GIB)).unwrap(),
            "16G"
        );
        assert_eq!(normalize_memory_within("64G", None).unwrap(), "64G");
    }

    #[test]
    fn jvm_args_split_validate_and_clear() {
        let mut s = JavaSettings::default();
        assert!(s.set(JVM_ARGS_KEY, "-XX:+UseG1GC  -Xmn1G").unwrap());
        assert_eq!(s.jvm_args, ["-XX:+UseG1GC", "-Xmn1G"]);
        assert_eq!(
            s.get(JVM_ARGS_KEY),
            Some(Some("-XX:+UseG1GC -Xmn1G".into()))
        );
        assert!(s.set(JVM_ARGS_KEY, "notaflag").is_err());
        assert!(s.set(JVM_ARGS_KEY, "").unwrap());
        assert!(s.jvm_args.is_empty());
        assert_eq!(s.get(JVM_ARGS_KEY), Some(None));
    }
}
