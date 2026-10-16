//! The per-server scheduled-backup vocabulary: the two reserved config keys,
//! how their values parse, and what they mean when unset. Separate from the
//! archive lifecycle beside it — one is a config concern, the other is I/O over
//! a tar.

use std::time::Duration;

use anyhow::Result;
use proto::error::Reason;
use proto::keys::{KeyKind, KeyRule};
use serde::{Deserialize, Serialize};

use crate::keys::{self, invalid};

/// The reserved per-server scheduled-backup setting keys.
pub const INTERVAL_KEY: &str = "backup-interval";
pub const RETENTION_KEY: &str = "backup-retention";

fn retention_kind() -> KeyKind {
    KeyKind::Integer {
        min: 1,
        max: i64::from(u32::MAX),
    }
}

/// Scheduled archives kept when no `backup-retention` is set.
pub const DEFAULT_RETENTION: usize = 7;

// A tighter schedule than this re-archives the world faster than it can
// meaningfully change and keeps the server's saving paused too often.
const MIN_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Per-server scheduled-backup tuning stored on the record: how often the
/// daemon archives the running server and how many scheduled archives to keep
/// (manual and pre-update backups are never pruned).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct BackupSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention: Option<u32>,
}

impl BackupSettings {
    pub(crate) fn rules() -> Vec<KeyRule> {
        vec![
            keys::rule(INTERVAL_KEY, KeyKind::Interval),
            keys::rule(RETENTION_KEY, retention_kind()),
        ]
    }

    /// The current value of a backup key, or `None` (outer) when `key` is not
    /// one of the reserved backup keys. The inner `None` means unset.
    pub fn get(&self, key: &str) -> Option<Option<String>> {
        match key {
            INTERVAL_KEY => Some(self.interval.clone()),
            RETENTION_KEY => Some(self.retention.map(|n| n.to_string())),
            _ => None,
        }
    }

    /// Apply a backup key. `Ok(false)` means `key` is not a backup key (fall
    /// through); an empty value clears the setting (an empty interval disables
    /// scheduled backups); an invalid value is `Err`.
    pub fn set(&mut self, key: &str, value: &str) -> Result<bool> {
        match key {
            INTERVAL_KEY => {
                self.interval = if value.trim().is_empty() {
                    None
                } else {
                    let normalized = value.trim().to_ascii_lowercase();
                    parse_interval(&normalized).map_err(invalid(key))?;
                    Some(normalized)
                };
                Ok(true)
            }
            RETENTION_KEY => {
                self.retention = if value.trim().is_empty() {
                    None
                } else {
                    keys::validate(&retention_kind(), value.trim()).map_err(invalid(key))?;
                    Some(value.trim().parse()?)
                };
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
                INTERVAL_KEY.to_string(),
                self.interval.clone().unwrap_or_default(),
            ),
            (
                RETENTION_KEY.to_string(),
                self.retention.map(|n| n.to_string()).unwrap_or_default(),
            ),
        ]
    }

    /// The parsed schedule, `None` when scheduled backups are disabled (or the
    /// stored value no longer parses).
    pub fn interval(&self) -> Option<Duration> {
        self.interval
            .as_deref()
            .and_then(|v| parse_interval(v).ok())
    }

    pub fn retention(&self) -> usize {
        self.retention
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_RETENTION)
    }
}

/// Parse a schedule interval: digits followed by one unit char (m/h/d),
/// at least five minutes.
pub fn parse_interval(value: &str) -> Result<Duration, Reason> {
    let trimmed = value.trim();
    let unit_seconds = match trimmed.chars().last() {
        Some('m') => 60,
        Some('h') => 3600,
        Some('d') => 86400,
        _ => return Err(Reason::IntervalFormat),
    };
    let digits = &trimmed[..trimmed.len() - 1];
    let count: u64 = digits
        .parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or(Reason::IntervalFormat)?;
    let interval = Duration::from_secs(count.saturating_mul(unit_seconds));
    if interval < MIN_INTERVAL {
        return Err(Reason::IntervalTooShort);
    }
    Ok(interval)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_parse_with_units() {
        assert_eq!(parse_interval("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_interval("6h").unwrap(), Duration::from_secs(21_600));
        assert_eq!(parse_interval("1d").unwrap(), Duration::from_secs(86_400));
        assert!(parse_interval("4m").is_err());
        assert!(parse_interval("0h").is_err());
        assert!(parse_interval("h").is_err());
        assert!(parse_interval("90").is_err());
        assert!(parse_interval("soon").is_err());
    }

    #[test]
    fn settings_validate_and_round_trip_keys() {
        let mut settings = BackupSettings::default();
        assert!(!settings.set("motd", "hi").unwrap());
        assert!(settings.set(INTERVAL_KEY, "6H").unwrap());
        assert_eq!(settings.interval(), Some(Duration::from_secs(21_600)));
        assert!(settings.set(RETENTION_KEY, "3").unwrap());
        assert_eq!(settings.retention(), 3);
        assert!(settings.set(INTERVAL_KEY, "soon").is_err());
        assert!(settings.set(RETENTION_KEY, "0").is_err());
        assert!(settings.set(INTERVAL_KEY, "").unwrap());
        assert_eq!(settings.interval(), None);
        assert_eq!(settings.get(RETENTION_KEY), Some(Some("3".to_string())));
    }
}
