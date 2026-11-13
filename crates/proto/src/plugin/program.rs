//! A program a plugin may run: what runs (its identity) and how it may be
//! invoked, kept apart so one program can serve several invocations
//! ([0120](../../../../docs/decisions/0120-a-programs-identity-is-separate-from-its-invocations.md)).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::manifest::{is_slug, ManifestError};
use super::scope::stays_inside;
use crate::minecraft::Requirement;

/// A program the plugin may run.
///
/// Identity is exactly one of `command` (a name on `PATH`, or a path inside
/// the plugin's data directory when it holds a `/`, which also needs `sha256`
/// per target triple) and `java` (a Java runtime Hestia manages).
///
/// Invocations are the argument lists the program may be called with. A program
/// with one writes its `args`, `env` and `path` at the top level; one with
/// several lists them as `invocations` and leaves the top level empty.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Program {
    pub id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java: Option<ProgramJava>,
    /// Per target triple; required for a program inside the data directory.
    pub sha256: BTreeMap<String, String>,
    /// What to tell the user when this program is not on the computer. Shown
    /// in errors and checks only; it grants nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<Requirement>,
    pub args: Vec<String>,
    pub env: BTreeSet<String>,
    pub path: Vec<String>,
    pub invocations: Vec<Invocation>,
}

/// One way a program may be called: argument patterns, the environment names it
/// may be given and the `PATH` programs whose directories form its `PATH`.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Invocation {
    /// A slug; empty for the one invocation of a program that declares its
    /// `args` at the top level.
    pub name: String,
    /// A literal token matches itself, `*` any one token, and a trailing `**`
    /// any rest, including none.
    pub args: Vec<String>,
    pub env: BTreeSet<String>,
    /// Ids of other `PATH` programs of the same manifest whose pinned
    /// directories become the child's `PATH`, and nothing else.
    pub path: Vec<String>,
}

/// The Java a `java` program runs on, from the runtimes Hestia manages.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(untagged)]
pub enum ProgramJava {
    /// Exactly this major.
    Major(u32),
    /// Whichever major of the range the call asks for.
    Range(JavaRange),
    /// The major the host uses for the Minecraft version the call is about.
    Profile(JavaProfile),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct JavaRange {
    pub min: u32,
    pub max: u32,
}

impl std::fmt::Display for JavaRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.min, self.max)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum JavaProfile {
    Profile,
}

/// Why a call's Java major was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JavaChoiceError {
    #[error("Java {asked} is outside what the program declares ({allowed})")]
    OutsideRange { asked: u32, allowed: JavaRange },
    #[error("the program runs on Java {allowed}; the call must say which major")]
    NeedsMajor { allowed: JavaRange },
    #[error("the program runs on the game version's Java, which the call does not choose")]
    ProfileTakesNone,
    #[error("the program runs on the game version's Java, and this call carries no game version")]
    NoGameVersion,
}

impl ProgramJava {
    /// The major a call runs on: `asked` is what the call named, `for_game` the
    /// major the host uses for the call's game version, when it has one.
    pub fn choose(
        &self,
        asked: Option<u32>,
        for_game: Option<u32>,
    ) -> Result<u32, JavaChoiceError> {
        let range = match self {
            ProgramJava::Major(major) => JavaRange {
                min: *major,
                max: *major,
            },
            ProgramJava::Range(range) => *range,
            ProgramJava::Profile(_) => {
                return match asked {
                    Some(_) => Err(JavaChoiceError::ProfileTakesNone),
                    None => for_game.ok_or(JavaChoiceError::NoGameVersion),
                };
            }
        };
        match asked {
            Some(asked) if (range.min..=range.max).contains(&asked) => Ok(asked),
            Some(asked) => Err(JavaChoiceError::OutsideRange {
                asked,
                allowed: range,
            }),
            None if range.min == range.max => Ok(range.min),
            None => Err(JavaChoiceError::NeedsMajor { allowed: range }),
        }
    }

    fn label(&self) -> String {
        match self {
            ProgramJava::Major(major) => format!("Hestia-managed Java {major}"),
            ProgramJava::Range(range) => format!("Hestia-managed Java {range}"),
            ProgramJava::Profile(_) => "Hestia-managed Java of the game version".to_string(),
        }
    }

    fn is_valid(&self) -> bool {
        let valid = |major: u32| (1..=MAX_JAVA_MAJOR).contains(&major);
        match self {
            ProgramJava::Major(major) => valid(*major),
            ProgramJava::Range(range) => {
                valid(range.min) && valid(range.max) && range.min <= range.max
            }
            ProgramJava::Profile(_) => true,
        }
    }
}

/// Why no invocation of a program fits a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvocationMismatch {
    /// No invocation's patterns match the arguments.
    Args,
    /// An invocation matches the arguments but does not declare this variable.
    Env(String),
}

/// What an approval pins for a `PATH` program: where it was found and the
/// sha256 of the file there. A different path or content is a different pin.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct ProgramPin {
    pub path: String,
    pub sha256: String,
}

const SHELLS: [&str; 12] = [
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "cmd",
    "powershell",
    "pwsh",
    "wscript",
];
const MAX_JAVA_MAJOR: u32 = 99;
const ANY_REST: &str = "**";
const ANY_ONE: &str = "*";
const MAX_INVOCATIONS: usize = 16;
const NAME_LENGTH: std::ops::RangeInclusive<usize> = 1..=40;

impl Invocation {
    /// Whether `args` match the declared patterns.
    pub fn permits(&self, args: &[String]) -> bool {
        let (fixed, rest) = match self.args.split_last() {
            Some((last, fixed)) if last == ANY_REST => (fixed, true),
            _ => (&self.args[..], false),
        };
        let length = if rest {
            args.len() >= fixed.len()
        } else {
            args.len() == fixed.len()
        };
        length
            && fixed
                .iter()
                .zip(args)
                .all(|(pattern, arg)| pattern == ANY_ONE || pattern == arg)
    }

    /// This invocation on one line, each list as JSON so no argument can pass
    /// for another field.
    pub fn entry(&self) -> String {
        let mut entry = String::new();
        if !self.name.is_empty() {
            entry.push_str(&format!("{}: ", self.name));
        }
        entry.push_str(&format!("args {}", serde_json::json!(self.args)));
        if !self.env.is_empty() {
            entry.push_str(&format!(" env {}", serde_json::json!(self.env)));
        }
        if !self.path.is_empty() {
            entry.push_str(&format!(" path {}", serde_json::json!(self.path)));
        }
        entry
    }

    fn is_empty(&self) -> bool {
        self.args.is_empty() && self.env.is_empty() && self.path.is_empty()
    }

    fn validate(&self, program: &str) -> Result<(), ManifestError> {
        let last = self.args.len().saturating_sub(1);
        if self
            .args
            .iter()
            .enumerate()
            .any(|(at, arg)| arg.contains('\0') || (arg == ANY_REST && at != last))
        {
            return Err(ManifestError::BadProgram(program.to_string()));
        }
        match self.env.iter().find(|name| !is_program_env(name)) {
            Some(name) => Err(ManifestError::BadProgramEnv(name.clone())),
            None => Ok(()),
        }
    }
}

impl Program {
    /// The entry a grant lists for this program: its whole definition on one
    /// line, identity first and then each invocation. Two programs that differ
    /// in identity or in any invocation have different entries, which is what
    /// makes a changed definition widen.
    pub fn entry(&self) -> String {
        let mut entry = format!("{}: {}", self.id, self.identity_label());
        if !self.sha256.is_empty() {
            entry.push_str(&format!(" sha256 {}", serde_json::json!(self.sha256)));
        }
        for invocation in self.invocations() {
            entry.push_str(" | ");
            entry.push_str(&invocation.entry());
        }
        entry
    }

    /// Every way the program may be called: the declared `invocations`, or the
    /// one the top-level `args`, `env` and `path` make.
    pub fn invocations(&self) -> Vec<Invocation> {
        if self.invocations.is_empty() {
            return vec![self.sugar()];
        }
        self.invocations.clone()
    }

    fn sugar(&self) -> Invocation {
        Invocation {
            name: String::new(),
            args: self.args.clone(),
            env: self.env.clone(),
            path: self.path.clone(),
        }
    }

    /// The invocation a call fits: the first whose patterns match `args` and
    /// which declares every name in `env`. Invocations that overlap on `args`
    /// should differ in them, since the first one that fits is the one used.
    pub fn matching(
        &self,
        args: &[String],
        env: &[&str],
    ) -> Result<Invocation, InvocationMismatch> {
        let mut refused = None;
        for invocation in self.invocations() {
            if !invocation.permits(args) {
                continue;
            }
            match env.iter().find(|name| !invocation.env.contains(**name)) {
                None => return Ok(invocation),
                Some(name) => {
                    refused.get_or_insert_with(|| (*name).to_string());
                }
            }
        }
        Err(refused.map_or(InvocationMismatch::Args, InvocationMismatch::Env))
    }

    /// A command with a slash is a path inside the plugin's data directory;
    /// one without is a name resolved on `PATH`.
    pub fn in_data_dir(&self) -> bool {
        self.java.is_none() && self.command.contains('/')
    }

    /// Whether the program is a name on `PATH`, pinned at approval.
    pub fn on_path(&self) -> bool {
        self.java.is_none() && !self.command.is_empty() && !self.command.contains('/')
    }

    /// What runs, as the grant shows it.
    pub fn identity_label(&self) -> String {
        match &self.java {
            Some(java) => java.label(),
            None => self.command.clone(),
        }
    }

    pub(super) fn collect_errors(&self, declared: &[Program], errors: &mut Vec<ManifestError>) {
        match &self.java {
            Some(java) => {
                if !self.command.is_empty() {
                    errors.push(ManifestError::ProgramIdentity(self.id.clone()));
                }
                if !java.is_valid() {
                    errors.push(ManifestError::BadProgramJava(self.id.clone()));
                }
                if !self.sha256.is_empty() {
                    errors.push(ManifestError::BadProgram(self.id.clone()));
                }
            }
            None if self.command.is_empty() => {
                errors.push(ManifestError::ProgramIdentity(self.id.clone()));
            }
            None => self.collect_command_errors(errors),
        }
        self.collect_invocation_errors(declared, errors);
        if self
            .requires
            .as_ref()
            .is_some_and(|r| !is_valid_requirement(r))
        {
            errors.push(ManifestError::BadProgramRequires(self.id.clone()));
        }
    }

    fn collect_command_errors(&self, errors: &mut Vec<ManifestError>) {
        let bad = || ManifestError::BadProgram(self.id.clone());
        let name = self.command.rsplit('/').next().unwrap_or_default();
        let stem = name
            .strip_suffix(".exe")
            .unwrap_or(name)
            .to_ascii_lowercase();
        if SHELLS.contains(&stem.as_str()) || self.command.contains('\0') {
            errors.push(bad());
        } else if self.in_data_dir() {
            if !stays_inside(&self.command) {
                errors.push(bad());
            }
            if self.sha256.is_empty() {
                errors.push(ManifestError::ProgramNeedsHash(self.id.clone()));
            } else {
                let hashes_ok = self.sha256.iter().all(|(triple, hash)| {
                    !triple.is_empty()
                        && triple
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                        && hash.len() == 64
                        && hash
                            .bytes()
                            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                });
                if !hashes_ok {
                    errors.push(bad());
                }
            }
        } else if !(1..=64).contains(&name.len())
            || name.starts_with(['.', '-'])
            || !name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
            })
        {
            errors.push(bad());
        }
    }

    fn collect_invocation_errors(&self, declared: &[Program], errors: &mut Vec<ManifestError>) {
        if !self.invocations.is_empty() {
            let mut names = BTreeSet::new();
            let named = self.invocations.iter().all(|invocation| {
                is_slug(&invocation.name, NAME_LENGTH) && names.insert(&invocation.name)
            });
            if !self.sugar().is_empty() || !named || self.invocations.len() > MAX_INVOCATIONS {
                errors.push(ManifestError::BadProgramInvocations(self.id.clone()));
            }
        }
        for invocation in self.invocations() {
            errors.extend(invocation.validate(&self.id).err());
            errors.extend(self.validate_path(&invocation, declared).err());
        }
    }

    fn validate_path(
        &self,
        invocation: &Invocation,
        declared: &[Program],
    ) -> Result<(), ManifestError> {
        let mut seen = BTreeSet::new();
        for id in &invocation.path {
            let names_path_program = *id != self.id
                && declared
                    .iter()
                    .any(|other| other.id == *id && other.on_path());
            if !names_path_program || !seen.insert(id) {
                return Err(ManifestError::BadProgramPath {
                    program: self.id.clone(),
                    entry: id.clone(),
                });
            }
        }
        Ok(())
    }
}

fn is_valid_requirement(requirement: &Requirement) -> bool {
    (1..=80).contains(&requirement.name.chars().count())
        && !requirement.name.chars().any(char::is_control)
        && requirement.url.starts_with("https://")
        && requirement.url.len() <= 512
        && !requirement
            .url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
}

/// Whether `name` may be declared in a program's `env`: a plain variable name
/// that is not one that loads code or redirects the program's own lookups.
/// No declaration unlocks a refused one.
pub fn is_program_env(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    (1..=128).contains(&name.len())
        && !name.starts_with(|first: char| first.is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !upper.starts_with("LD_")
        && !upper.starts_with("DYLD_")
        && upper != "PATH"
        && !upper
            .find("JAVA")
            .is_some_and(|at| upper[at..].contains("OPTIONS"))
}
