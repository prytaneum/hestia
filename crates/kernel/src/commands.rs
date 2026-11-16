//! The commands and schedules a manifest declares, and the checks of what
//! callers pass them. The values are JSON because the manifest, not the WIT,
//! declares their shape.

use std::collections::HashSet;
use std::time::Duration;

use proto::plugin::{ArgKind, ArgSpec, CommandSpec, GrantedSchedule};
use serde::Deserialize;
use serde_json::Value;

use crate::manifest::is_plain;

const NAME_MAX: usize = 32;
const SUMMARY_MAX: usize = 200;
const ARGS_MAX: usize = 16;
const COMMANDS_MAX: usize = 32;
const SCHEDULES_MAX: usize = 8;
const ARGS_BYTES: usize = 64 * 1024;
const MINUTE: u64 = 60;

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct CommandDecl {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub args: Vec<ArgDecl>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ArgDecl {
    pub name: String,
    #[serde(default)]
    pub kind: ArgKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub summary: String,
}

/// A schedule ticks every `every`: a whole number of minutes, hours or days
/// (`30m`, `6h`, `1d`), counted from its last run.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDecl {
    pub id: String,
    pub every: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DeclError {
    #[error("a command, argument or schedule name must be 1-{NAME_MAX} lowercase letters, digits or '-', not starting with '-': '{0}'")]
    Name(String),
    #[error("'{0}' is declared twice")]
    Duplicate(String),
    #[error("command '{0}' declares more than {ARGS_MAX} arguments")]
    TooManyArgs(String),
    #[error("a plugin may declare at most {COMMANDS_MAX} commands and {SCHEDULES_MAX} schedules")]
    TooMany,
    #[error("'{0}' is a name the CLI's parser keeps for itself")]
    Reserved(String),
    #[error("a summary must be at most {SUMMARY_MAX} characters with no control characters")]
    Summary,
    #[error("schedule '{0}' must repeat every whole number of minutes, hours or days, such as 30m, 6h or 1d")]
    Every(String),
}

fn accepts(kind: ArgKind, value: &Value) -> bool {
    match kind {
        ArgKind::String => value.is_string(),
        ArgKind::Integer => value.is_i64() || value.is_u64(),
        ArgKind::Boolean => value.is_boolean(),
    }
}

fn name_ok(name: &str) -> bool {
    (1..=NAME_MAX).contains(&name.len())
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn unique<'a>(names: impl Iterator<Item = &'a str>) -> Result<(), DeclError> {
    let mut seen = HashSet::new();
    for name in names {
        if !name_ok(name) {
            return Err(DeclError::Name(name.to_owned()));
        }
        if !seen.insert(name) {
            return Err(DeclError::Duplicate(name.to_owned()));
        }
    }
    Ok(())
}

/// Names clap defines on every command it parses, so a declaration may not take them.
fn reserved<'a>(mut names: impl Iterator<Item = &'a str>, taken: &[&str]) -> Result<(), DeclError> {
    match names.find(|name| taken.contains(name)) {
        Some(name) => Err(DeclError::Reserved(name.to_owned())),
        None => Ok(()),
    }
}

pub(crate) fn validate(
    commands: &[CommandDecl],
    schedules: &[ScheduleDecl],
) -> Result<(), DeclError> {
    if commands.len() > COMMANDS_MAX || schedules.len() > SCHEDULES_MAX {
        return Err(DeclError::TooMany);
    }
    unique(commands.iter().map(|c| c.name.as_str()))?;
    unique(schedules.iter().map(|s| s.id.as_str()))?;
    reserved(commands.iter().map(|c| c.name.as_str()), &["help"])?;
    for command in commands {
        unique(command.args.iter().map(|a| a.name.as_str()))?;
        reserved(
            command.args.iter().map(|a| a.name.as_str()),
            &["help", "version"],
        )?;
        if command.args.len() > ARGS_MAX {
            return Err(DeclError::TooManyArgs(command.name.clone()));
        }
        let summaries =
            std::iter::once(&command.summary).chain(command.args.iter().map(|a| &a.summary));
        if summaries
            .into_iter()
            .any(|s| !s.is_empty() && !is_plain(s, SUMMARY_MAX))
        {
            return Err(DeclError::Summary);
        }
    }
    match schedules.iter().find(|s| s.interval().is_none()) {
        Some(bad) => Err(DeclError::Every(bad.id.clone())),
        None => Ok(()),
    }
}

/// The length of an `every` such as `30m`: digits and one ASCII unit, nothing else.
pub(crate) fn interval(every: &str) -> Option<Duration> {
    let (digits, unit) = [("m", MINUTE), ("h", MINUTE * 60), ("d", MINUTE * 60 * 24)]
        .into_iter()
        .find_map(|(suffix, seconds)| Some((every.strip_suffix(suffix)?, seconds)))?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let count: u64 = digits.parse().ok().filter(|n| *n > 0)?;
    count.checked_mul(unit).map(Duration::from_secs)
}

impl ScheduleDecl {
    pub fn interval(&self) -> Option<Duration> {
        interval(&self.every)
    }

    pub fn granted(&self) -> GrantedSchedule {
        GrantedSchedule {
            id: self.id.clone(),
            every: self.every.clone(),
        }
    }
}

impl CommandDecl {
    pub fn spec(&self) -> CommandSpec {
        CommandSpec {
            name: self.name.clone(),
            summary: self.summary.clone(),
            args: self
                .args
                .iter()
                .map(|arg| ArgSpec {
                    name: arg.name.clone(),
                    kind: arg.kind,
                    required: arg.required,
                    summary: arg.summary.clone(),
                })
                .collect(),
        }
    }

    /// `args` must be a JSON object that names only declared arguments, each
    /// of its declared kind, with every required one present.
    pub fn check(&self, args: &str) -> Result<(), String> {
        if args.len() > ARGS_BYTES {
            return Err(format!("arguments are larger than {ARGS_BYTES} bytes"));
        }
        let given: Value = serde_json::from_str(if args.is_empty() { "{}" } else { args })
            .map_err(|error| format!("arguments are not JSON: {error}"))?;
        let Value::Object(given) = given else {
            return Err("arguments must be a JSON object".into());
        };
        if let Some(unknown) = given
            .keys()
            .find(|key| !self.args.iter().any(|arg| &arg.name == *key))
        {
            return Err(format!("'{}' takes no argument '{unknown}'", self.name));
        }
        for arg in &self.args {
            match given.get(&arg.name) {
                None if arg.required => {
                    return Err(format!("'{}' needs '{}'", self.name, arg.name))
                }
                Some(value) if !accepts(arg.kind, value) => {
                    return Err(format!("'{}' must be a {}", arg.name, arg.kind))
                }
                _ => {}
            }
        }
        Ok(())
    }
}
