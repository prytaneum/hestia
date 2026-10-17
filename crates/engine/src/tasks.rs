//! The scheduled-task vocabulary: how a task's schedule is written, and when
//! it comes due. Running one is the daemon's business; this only decides.

use std::time::Duration;

use anyhow::{bail, Result};
use proto::error::{ErrorInfo, Field, Reason};
use proto::tasks::{ServerTask, TaskAction};

const DAY_SECONDS: i64 = 86_400;

// A tighter schedule than this re-runs a task faster than it can meaningfully
// matter, and a backup keeps the server's saving paused too often.
const MIN_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Parse an interval: digits followed by one unit char (m/h/d), at least five
/// minutes.
fn parse_interval(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    let unit_seconds = match trimmed.chars().last()? {
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        _ => return None,
    };
    let count: u64 = trimmed[..trimmed.len() - 1]
        .parse()
        .ok()
        .filter(|n| *n > 0)?;
    Some(Duration::from_secs(count.saturating_mul(unit_seconds))).filter(|d| *d >= MIN_INTERVAL)
}

/// When a task runs: on an interval, or once a day at a local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    Every(Duration),
    Daily { hour: u32, minute: u32 },
}

impl Schedule {
    /// `30m` / `6h` / `1d`, or `HH:MM`.
    pub fn parse(value: &str) -> Result<Schedule> {
        let value = value.trim().to_ascii_lowercase();
        if let Some((hour, minute)) = value.split_once(':') {
            // `4:5` is a typo, not five past four.
            if minute.len() != 2 {
                return Err(malformed());
            }
            return match (hour.parse::<u32>(), minute.parse::<u32>()) {
                (Ok(hour), Ok(minute)) if hour < 24 && minute < 60 => {
                    Ok(Schedule::Daily { hour, minute })
                }
                _ => Err(malformed()),
            };
        }
        parse_interval(&value)
            .map(Schedule::Every)
            .ok_or_else(malformed)
    }

    /// Whether a task last run at `last_run` is due at `now`. `local_midnight`
    /// is the start of today in the machine's zone, which the caller knows and
    /// this crate does not.
    pub fn due(&self, last_run: i64, now: i64, local_midnight: i64) -> bool {
        match *self {
            Schedule::Every(interval) => now.saturating_sub(last_run) >= interval.as_secs() as i64,
            Schedule::Daily { hour, minute } => {
                let today = local_midnight + i64::from(hour) * 3600 + i64::from(minute) * 60;
                // Before today's slot, yesterday's is the one that may be owed.
                let slot = if now >= today {
                    today
                } else {
                    today - DAY_SECONDS
                };
                last_run < slot
            }
        }
    }
}

fn malformed() -> anyhow::Error {
    ErrorInfo::InvalidValue {
        field: Field::Schedule,
        reason: Reason::ScheduleFormat,
    }
    .into()
}

/// Check a task before it is stored and return it normalised.
pub fn checked(mut task: ServerTask) -> Result<ServerTask> {
    Schedule::parse(&task.schedule)?;
    task.schedule = task.schedule.trim().to_ascii_lowercase();
    task.command = task.command.trim().trim_start_matches('/').to_string();
    match task.action {
        TaskAction::Command if task.command.is_empty() => bail!(ErrorInfo::FieldRequired {
            field: Field::Command,
        }),
        TaskAction::Command => task.retention = None,
        TaskAction::Restart => {
            task.command.clear();
            task.retention = None;
        }
        TaskAction::Backup => {
            task.command.clear();
            if task.retention == Some(0) {
                bail!(ErrorInfo::InvalidValue {
                    field: Field::Retention,
                    reason: Reason::OutOfRange {
                        min: 1,
                        max: i64::from(u32::MAX),
                    },
                });
            }
        }
    }
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIDNIGHT: i64 = 1_000_000 * DAY_SECONDS;
    const HOUR: i64 = 3600;

    #[test]
    fn a_schedule_is_an_interval_or_a_daily_time() {
        assert_eq!(
            Schedule::parse("6h").unwrap(),
            Schedule::Every(Duration::from_secs(6 * 3600))
        );
        assert_eq!(
            Schedule::parse(" 04:30 ").unwrap(),
            Schedule::Daily {
                hour: 4,
                minute: 30
            }
        );
        for bad in ["", "soon", "1m", "24:00", "4:5", "12:60", "6"] {
            assert!(Schedule::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_interval_counts_from_the_last_run() {
        let every = Schedule::Every(Duration::from_secs(6 * 3600));
        assert!(!every.due(MIDNIGHT, MIDNIGHT + 5 * HOUR, MIDNIGHT));
        assert!(every.due(MIDNIGHT, MIDNIGHT + 6 * HOUR, MIDNIGHT));
    }

    #[test]
    fn a_daily_task_runs_once_per_slot() {
        let daily = Schedule::Daily { hour: 4, minute: 0 };
        let slot = MIDNIGHT + 4 * HOUR;
        // Saved yesterday evening: not due before the slot, due at it.
        let saved = MIDNIGHT - 2 * HOUR;
        assert!(!daily.due(saved, slot - 60, MIDNIGHT));
        assert!(daily.due(saved, slot, MIDNIGHT));
        // Ran at the slot: not again today.
        assert!(!daily.due(slot, slot + 10 * HOUR, MIDNIGHT));
        // The daemon was down over the slot: it is still owed afterwards.
        assert!(daily.due(saved, slot + 3 * HOUR, MIDNIGHT));
        // Saved after today's slot: it waits for tomorrow's.
        assert!(!daily.due(slot + HOUR, slot + 2 * HOUR, MIDNIGHT));
    }

    #[test]
    fn intervals_parse_with_units_and_a_floor() {
        assert_eq!(parse_interval("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_interval("1d"), Some(Duration::from_secs(86_400)));
        for bad in ["4m", "0h", "h", "90", "soon"] {
            assert_eq!(parse_interval(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn only_a_backup_keeps_a_retention_and_it_must_keep_one() {
        let task = |action, retention| ServerTask {
            action,
            retention,
            schedule: "1d".into(),
            ..Default::default()
        };
        assert_eq!(
            checked(task(TaskAction::Backup, Some(3)))
                .unwrap()
                .retention,
            Some(3)
        );
        assert!(checked(task(TaskAction::Backup, Some(0))).is_err());
        assert_eq!(
            checked(task(TaskAction::Restart, Some(3)))
                .unwrap()
                .retention,
            None
        );
    }

    #[test]
    fn a_command_task_needs_a_command_and_a_restart_keeps_none() {
        let task = |action, command: &str| ServerTask {
            action,
            command: command.into(),
            schedule: "1D".into(),
            ..Default::default()
        };
        assert!(checked(task(TaskAction::Command, "  ")).is_err());
        let say = checked(task(TaskAction::Command, "/say hi")).unwrap();
        assert_eq!(
            (say.command.as_str(), say.schedule.as_str()),
            ("say hi", "1d")
        );
        assert_eq!(
            checked(task(TaskAction::Restart, "stop")).unwrap().command,
            ""
        );
    }
}
