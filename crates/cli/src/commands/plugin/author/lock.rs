//! `plugin.grant`, the committed record of what the built component asks for.
//! Hestia never reads it: the host derives the grant itself, so the file only
//! lets a review see, and the build refuse, a grant that grew unnoticed.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use kernel::{Manifest, Shape, KERNEL_PACKAGE};
use proto::plugin::{Grant, ScopeKind};
use serde::Deserialize;

use super::inspect::api_of;
use super::project::LOCK;

#[derive(Deserialize, Default, PartialEq, Eq, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Lock {
    api: Option<String>,
    requires: Vec<String>,
    /// The interfaces it imports from the packages that are not the kernel's:
    /// the CLI cannot see the permissions of a built-in's package, so what
    /// changes there shows here until the merged table replaces this.
    imports: Vec<String>,
    permissions: BTreeMap<String, Permit>,
    dependencies: BTreeMap<String, String>,
    schedules: BTreeMap<String, String>,
    contributes: Contributes,
}

#[derive(Deserialize, Default, PartialEq, Eq, Debug)]
#[serde(default, deny_unknown_fields)]
struct Contributes {
    exports: Vec<String>,
}

#[derive(Deserialize, Default, PartialEq, Eq, Debug)]
#[serde(default, deny_unknown_fields)]
struct Permit {
    optional: bool,
    entries: bool,
    hosts: Vec<String>,
    unhashed: Vec<String>,
    paths: Vec<String>,
    endpoints: Vec<String>,
    programs: Vec<Program>,
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(deny_unknown_fields)]
struct Program {
    path: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    os: Option<String>,
}

fn sorted<T: Ord>(mut items: Vec<T>) -> Vec<T> {
    items.sort();
    items
}

impl Lock {
    /// What `grant` asks for, as built from `manifest` and `shape`. The grant
    /// is derived for every platform, and the programs are read from the
    /// manifest to keep the `os` each is written for, so the file is the same
    /// on every machine.
    pub fn of(manifest: &Manifest, grant: &Grant, shape: &Shape) -> Lock {
        let permissions = grant
            .permissions
            .iter()
            .map(|granted| {
                let mut permit = Permit {
                    optional: granted.optional,
                    entries: granted.entries,
                    ..Permit::default()
                };
                if let Some(scope) = &granted.scope {
                    let values = sorted(scope.values.clone());
                    match scope.kind {
                        ScopeKind::Hosts => {
                            permit.hosts = values;
                            permit.unhashed = sorted(scope.unhashed.clone());
                        }
                        ScopeKind::Paths => permit.paths = values,
                        ScopeKind::Endpoints => permit.endpoints = values,
                        ScopeKind::Programs => {
                            permit.programs = sorted(
                                manifest
                                    .permissions
                                    .get(&granted.permission)
                                    .into_iter()
                                    .flat_map(|scopes| &scopes.programs)
                                    .map(|program| Program {
                                        path: program.path.clone(),
                                        args: program.args.clone(),
                                        os: program.os.map(|os| os.name().to_owned()),
                                    })
                                    .collect(),
                            )
                        }
                        ScopeKind::Entries => {}
                    }
                }
                (granted.permission.clone(), permit)
            })
            .collect();
        let kernel_prefix = format!("{KERNEL_PACKAGE}/");
        let imports = shape
            .imports
            .iter()
            .filter(|import| import.starts_with("hestia:") && !import.starts_with(&kernel_prefix))
            .cloned()
            .collect();
        Lock {
            api: api_of(shape),
            requires: sorted(grant.requires.clone()),
            imports: sorted(imports),
            permissions,
            dependencies: grant
                .dependencies
                .iter()
                .map(|dependency| (dependency.id.clone(), dependency.range.clone()))
                .collect(),
            schedules: grant
                .schedules
                .iter()
                .map(|schedule| (schedule.id.clone(), schedule.every.clone()))
                .collect(),
            contributes: Contributes {
                exports: sorted(grant.contributes.clone()),
            },
        }
    }

    /// The file's text. Parsing it back gives the same lock.
    pub fn render(&self) -> String {
        let mut text = String::from(
            "# plugin.grant - written by `hestia plugin check --update-grant`, committed, never edited by hand\n",
        );
        if let Some(api) = &self.api {
            let _ = writeln!(text, "api = {}", quote(api));
        }
        for (name, values) in [("requires", &self.requires), ("imports", &self.imports)] {
            if !values.is_empty() {
                let _ = writeln!(text, "{name} = {}", list(values));
            }
        }
        text.push_str("\n[permissions]\n");
        for (name, permit) in &self.permissions {
            let _ = writeln!(text, "{} = {}", quote(name), permit.render());
        }
        for (title, entries) in [
            ("dependencies", &self.dependencies),
            ("schedules", &self.schedules),
        ] {
            if !entries.is_empty() {
                let _ = writeln!(text, "\n[{title}]");
                for (id, value) in entries {
                    let _ = writeln!(text, "{} = {}", quote(id), quote(value));
                }
            }
        }
        let _ = write!(
            text,
            "\n[contributes]\nexports = {}\n",
            list(&self.contributes.exports)
        );
        text
    }
}

impl Permit {
    fn render(&self) -> String {
        let mut fields = Vec::new();
        if self.optional {
            fields.push("optional = true".to_owned());
        }
        if self.entries {
            fields.push("entries = true".to_owned());
        }
        for (name, values) in [
            ("hosts", &self.hosts),
            ("unhashed", &self.unhashed),
            ("paths", &self.paths),
            ("endpoints", &self.endpoints),
        ] {
            if !values.is_empty() {
                fields.push(format!("{name} = {}", list(values)));
            }
        }
        if !self.programs.is_empty() {
            let programs: Vec<String> = self
                .programs
                .iter()
                .map(|program| {
                    let os = program
                        .os
                        .as_ref()
                        .map_or_else(String::new, |os| format!(", os = {}", quote(os)));
                    format!(
                        "{{ path = {}, args = {}{os} }}",
                        quote(&program.path),
                        list(&program.args)
                    )
                })
                .collect();
            fields.push(format!("programs = [{}]", programs.join(", ")));
        }
        match fields.is_empty() {
            true => "{}".to_owned(),
            false => format!("{{ {} }}", fields.join(", ")),
        }
    }
}

/// A TOML basic string. JSON's escaping leaves U+007F bare, which TOML refuses.
pub fn quote(text: &str) -> String {
    let mut quoted = String::from('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(quoted, "\\u{:04X}", c as u32);
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

fn list(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| quote(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// The lines only one side has: `-` for the locked file, `+` for the build.
fn diff(locked: &str, built: &str) -> String {
    let only = |from: &str, other: &str, mark: char| -> Vec<String> {
        let mut left: Vec<&str> = other.lines().collect();
        from.lines()
            .filter(|line| match left.iter().position(|l| l == line) {
                Some(at) => {
                    left.swap_remove(at);
                    false
                }
                None => !line.is_empty(),
            })
            .map(|line| format!("{mark} {line}"))
            .collect()
    };
    [only(locked, built, '-'), only(built, locked, '+')]
        .concat()
        .join("\n")
}

/// What `built` asks for that `before` did not and the reverse, as a diff.
pub fn changes(before: &Lock, built: &Lock) -> String {
    diff(&before.render(), &built.render())
}

pub fn write(dir: &Path, lock: &Lock) -> std::io::Result<()> {
    common::atomic::write(&dir.join(LOCK), lock.render().as_bytes())
}

/// Why the file in `dir` is not `built`, or nothing when it is.
pub fn mismatch(dir: &Path, built: &Lock) -> Option<String> {
    let rewrite = "run `hestia plugin check --update-grant` if the change is intended";
    let text = match std::fs::read_to_string(dir.join(LOCK)) {
        Ok(text) => text,
        Err(error) => return Some(format!("{LOCK} cannot be read ({error}); {rewrite}")),
    };
    let locked: Lock = match toml::from_str(&text) {
        Ok(locked) => locked,
        Err(error) => return Some(format!("{LOCK} is malformed: {error}")),
    };
    (locked != *built).then(|| {
        format!(
            "the built component asks for something other than {LOCK} (- locked, + built):\n{}\n{rewrite}",
            diff(&locked.render(), &built.render())
        )
    })
}
