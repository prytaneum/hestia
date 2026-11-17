//! What a component asks for, read from its imports and exports, and the
//! digest the user's approval is bound to.

mod pin;
mod table;
pub(crate) mod versions;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proto::plugin::{Choice, Grant, GrantedPermission, PermissionScope, Program, ScopeKind};
use sha2::{Digest, Sha256};

use crate::commands::{self, ScheduleDecl};
use crate::manifest::{Manifest, Scopes};
use crate::runtime::{Denials, Shape};
pub(crate) use pin::digest;
pub use pin::{pin, Reserved};
pub use table::{Gate, Permission, Stability, Table};

#[derive(Debug, thiserror::Error)]
pub enum GrantError {
    #[error("the component imports '{0}', which is not an interface Hestia provides")]
    UnsupportedImport(String),
    #[error("'{0}' is not an interface of its package")]
    UnknownInterface(String),
    #[error("plugin.toml gives a scope to '{0}', which the component does not need")]
    ScopeNotNeeded(String),
    #[error("plugin.toml marks '{0}' optional, but the component does not need it")]
    OptionalNotNeeded(String),
    #[error("'{0}' cannot be optional: an interface it gates cannot be answered `denied`")]
    OptionalUndeniable(String),
    #[error("plugin.toml gives '{permission}' {given} scope, but it takes {takes}")]
    ScopeKind {
        permission: String,
        given: String,
        takes: String,
    },
    #[error("'{0}' needs a scope in plugin.toml")]
    ScopeMissing(String),
    #[error("'{0}' is not an optional permission of this plugin")]
    NotOptional(String),
    #[error("plugin.toml gives '{0}' unhashed hosts, but only hosts take them")]
    UnhashedNotTaken(String),
    #[error("plugin.toml gives '{1}' the unhashed host '{0}', which is not one of its hosts")]
    UnhashedOutsideHosts(String, String),
    #[error("plugin.toml grants an endpoint `{entry}`: {reason}")]
    Endpoint { entry: String, reason: String },
    #[error("plugin.toml grants '{permission}' the program `{entry}`: {reason}")]
    Program {
        permission: String,
        entry: String,
        reason: String,
    },
    #[error("plugin.toml declares {0}, but the component does not export them")]
    NotExported(&'static str),
    #[error("{what} is at API {asked}, but this Hestia provides {host}")]
    Api {
        what: String,
        asked: String,
        host: String,
    },
    #[error("plugin.toml says api {api}, but the component uses {import}")]
    ApiMismatch { api: String, import: String },
    #[error("'{interface}' is unstable ({feature}): only a plugin installed from a directory in developer mode may import it")]
    Unstable { interface: String, feature: String },
    #[error("'{interface}' arrived in {since}, but the plugin is built against {api}")]
    Since {
        interface: String,
        since: String,
        api: String,
    },
    #[error("plugin.toml grants '{permission}' {reason}")]
    Host {
        permission: String,
        reason: crate::vetting::HostError,
    },
}

const KERNEL_PACKAGE: &str = "hestia:kernel";

fn unversioned(name: &str) -> &str {
    name.split_once('@').map_or(name, |(base, _)| base)
}

/// Whether the sandbox can answer `denied` for the interface when its
/// permission is withheld: `wasi:` interfaces and `ipc`, which holds resources,
/// cannot, so a plugin importing one without the permission does not load.
pub(crate) fn deniable(import: &str) -> bool {
    let interface = unversioned(import);
    !interface.starts_with("wasi:") && interface != "hestia:kernel/ipc"
}

/// The scope the manifest writes; the entries scope is the user's.
fn manifest_scope(permission: &Permission) -> Option<ScopeKind> {
    permission.scope.filter(|kind| *kind != ScopeKind::Entries)
}

fn package_of(interface: &str) -> Option<&str> {
    interface.split_once('/').map(|(package, _)| package)
}

/// A permission is needed by importing an interface that `table` maps to it. A
/// `hestia:` package other than the kernel's is a requirement on the built-in
/// providing it. Pure: a program's digest is left empty, for [`pin`] to fill.
pub fn derive(table: &Table, manifest: &Manifest, shape: &Shape) -> Result<Grant, GrantError> {
    let mut needed: BTreeMap<&str, &Permission> = BTreeMap::new();
    let mut requires = BTreeSet::new();
    for import in &shape.imports {
        let interface = unversioned(import);
        if interface.starts_with("wasi:")
            && !package_of(interface).is_some_and(|p| table.knows_package(p))
        {
            continue;
        }
        let Some(package) =
            package_of(interface).filter(|p| p.starts_with("hestia:") || table.knows_package(p))
        else {
            return Err(GrantError::UnsupportedImport(import.clone()));
        };
        if package.starts_with("hestia:") && package != KERNEL_PACKAGE {
            requires.insert(package.to_owned());
        }
        if !table.knows_package(package) {
            continue;
        }
        match table.gate(interface) {
            Gate::Open => {}
            Gate::Needs(permission) => {
                needed.insert(permission.name, permission);
            }
            Gate::Unknown => return Err(GrantError::UnknownInterface(import.clone())),
        }
    }
    for export in &shape.exports {
        let interface = unversioned(export);
        if package_of(interface).is_some_and(|p| table.knows_package(p))
            && matches!(table.gate(interface), Gate::Unknown)
        {
            return Err(GrantError::UnknownInterface(export.clone()));
        }
    }
    for (name, scopes) in &manifest.permissions {
        let given_scope = scopes.kinds().next().is_some();
        match needed.get(name.as_str()) {
            None if scopes.optional && !given_scope => {
                return Err(GrantError::OptionalNotNeeded(name.clone()))
            }
            None => return Err(GrantError::ScopeNotNeeded(name.clone())),
            Some(permission) if manifest_scope(permission).is_none() && given_scope => {
                return Err(GrantError::ScopeNotNeeded(name.clone()))
            }
            Some(_) if scopes.optional => {
                let gated = |import: &String| matches!(table.gate(unversioned(import)), Gate::Needs(p) if p.name == name);
                if shape.imports.iter().any(|i| gated(i) && !deniable(i)) {
                    return Err(GrantError::OptionalUndeniable(name.clone()));
                }
            }
            Some(_) => {}
        }
    }
    let exported = |name: &str| shape.exports.iter().any(|e| unversioned(e) == name);
    if !manifest.commands.is_empty() && !exported("hestia:kernel/commands") {
        return Err(GrantError::NotExported("commands"));
    }
    if !manifest.schedules.is_empty() && !exported("hestia:kernel/on-schedule") {
        return Err(GrantError::NotExported("schedules"));
    }
    let mut permissions = Vec::new();
    for (name, permission) in needed {
        let scope = match manifest_scope(permission) {
            None => None,
            Some(kind) => {
                let scopes = manifest.permissions.get(name);
                if let Some(wrong) = scopes.and_then(|s| s.kinds().find(|k| *k != kind)) {
                    return Err(GrantError::ScopeKind {
                        permission: name.to_owned(),
                        given: wrong.to_string(),
                        takes: kind.to_string(),
                    });
                }
                Some(scope_of(
                    name,
                    kind,
                    scopes.ok_or_else(|| GrantError::ScopeMissing(name.to_owned()))?,
                )?)
            }
        };
        permissions.push(GrantedPermission {
            permission: name.to_owned(),
            tier: permission.tier,
            prompt: permission.prompt.to_owned(),
            scope,
            optional: manifest.permissions.get(name).is_some_and(|s| s.optional),
            entries: permission.scope == Some(ScopeKind::Entries),
        });
    }
    Ok(Grant {
        permissions,
        contributes: shape.exports.clone(),
        requires: requires.into_iter().collect(),
        schedules: manifest
            .schedules
            .iter()
            .map(ScheduleDecl::granted)
            .collect(),
    })
}

fn scope_of(name: &str, kind: ScopeKind, scopes: &Scopes) -> Result<PermissionScope, GrantError> {
    let unhashed = scopes.unhashed.as_slice();
    if scopes.listed(kind) == 0 {
        return Err(GrantError::ScopeMissing(name.to_owned()));
    }
    if kind != ScopeKind::Hosts && !unhashed.is_empty() {
        return Err(GrantError::UnhashedNotTaken(name.to_owned()));
    }
    let mut scope = PermissionScope {
        kind,
        values: scopes.values(kind).to_vec(),
        unhashed: unhashed.to_vec(),
        programs: Vec::new(),
    };
    match kind {
        ScopeKind::Hosts => {
            if let Some(stray) = unhashed
                .iter()
                .find(|host| !scope.values.iter().any(|h| h.eq_ignore_ascii_case(host)))
            {
                return Err(GrantError::UnhashedOutsideHosts(
                    stray.clone(),
                    name.to_owned(),
                ));
            }
            crate::vetting::Hosts::new(&scope.values, unhashed).map_err(|reason| {
                GrantError::Host {
                    permission: name.to_owned(),
                    reason,
                }
            })?;
        }
        ScopeKind::Endpoints => {
            let mut here = Vec::new();
            for entry in std::mem::take(&mut scope.values) {
                if endpoint_here(&entry)? {
                    here.push(entry);
                }
            }
            scope.values = here;
        }
        ScopeKind::Programs => {
            for entry in scopes
                .programs
                .iter()
                .filter(|e| e.os.is_none_or(|os| os.is_current()))
            {
                if !Path::new(&entry.path).is_absolute() {
                    return Err(GrantError::Program {
                        permission: name.to_owned(),
                        entry: entry.path.clone(),
                        reason: "write an absolute path".to_owned(),
                    });
                }
                scope.programs.push(Program {
                    path: entry.path.clone(),
                    args: entry.args.clone(),
                    sha256: String::new(),
                });
            }
        }
        ScopeKind::Paths | ScopeKind::Entries => {}
    }
    Ok(scope)
}

const PIPE_PREFIX: &str = r"\\.\pipe\";

/// Whether `entry`, an absolute unix path or a Windows pipe name, is the form
/// this platform connects to. One manifest can list both.
fn endpoint_here(entry: &str) -> Result<bool, GrantError> {
    let unix = entry.starts_with('/');
    let pipe = entry.len() > PIPE_PREFIX.len()
        && entry[..PIPE_PREFIX.len()].eq_ignore_ascii_case(PIPE_PREFIX);
    if !unix && !pipe {
        return Err(GrantError::Endpoint {
            entry: entry.to_owned(),
            reason: format!("write an absolute path or a {PIPE_PREFIX} name"),
        });
    }
    Ok(pipe == cfg!(windows))
}

/// Binds an approval to the package and what it reaches: the id, the exact
/// manifest and component bytes, each permission with its scope (hosts,
/// unhashed hosts, endpoints, and each program with its arguments and digest),
/// the packages it requires, whether each permission is optional, and for a
/// repository's package the repository and the author key that vouch for it.
/// Hestia's own properties (its prompt text, tiers and which permissions take the
/// entries scope) are left out, so changing them never invalidates an approval:
/// a permission that gains the entries scope reaches none until the user picks.
pub fn approval_digest(
    id: &str,
    manifest: &[u8],
    wasm: &[u8],
    grant: &Grant,
    signer: Option<(&str, &str)>,
) -> String {
    let mut hash = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    field(b"hestia-approval-v7");
    field(id.as_bytes());
    field(&Sha256::digest(manifest));
    field(&Sha256::digest(wasm));
    let mut permissions: Vec<_> = grant.permissions.iter().collect();
    permissions.sort_by(|a, b| a.permission.cmp(&b.permission));
    field(&(permissions.len() as u64).to_le_bytes());
    for permission in permissions {
        field(permission.permission.as_bytes());
        field(&[permission.optional as u8]);
        let scope = permission.scope.clone().unwrap_or_default();
        field(
            match permission.scope {
                Some(_) => scope.kind.to_string(),
                None => String::new(),
            }
            .as_bytes(),
        );
        for mut list in [scope.values, scope.unhashed] {
            list.sort_unstable();
            field(&(list.len() as u64).to_le_bytes());
            for value in list {
                field(value.as_bytes());
            }
        }
        let mut programs = scope.programs;
        programs.sort_unstable_by(|a, b| (&a.path, &a.args).cmp(&(&b.path, &b.args)));
        field(&(programs.len() as u64).to_le_bytes());
        for program in programs {
            field(program.path.as_bytes());
            field(&(program.args.len() as u64).to_le_bytes());
            for arg in &program.args {
                field(arg.as_bytes());
            }
            field(program.sha256.as_bytes());
        }
    }
    let mut requires: Vec<&str> = grant.requires.iter().map(String::as_str).collect();
    requires.sort_unstable();
    field(&(requires.len() as u64).to_le_bytes());
    for package in requires {
        field(package.as_bytes());
    }
    let mut schedules: Vec<_> = grant.schedules.iter().collect();
    schedules.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    field(&(schedules.len() as u64).to_le_bytes());
    for schedule in schedules {
        field(schedule.id.as_bytes());
        field(schedule.every.as_bytes());
    }
    let (repository, author) = signer.unwrap_or_default();
    field(repository.as_bytes());
    field(author.as_bytes());
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `new` asks for anything `old` did not: a permission, a scope entry
/// (a host, an endpoint, a host served without checksum, or a program whose
/// path, arguments or digest pin differ), a required package, an export, or a
/// schedule that is new or ticks more often. Dropping or narrowing never widens,
/// and a new optional permission arrives declined, so it does not widen either.
pub fn widens(old: &Grant, new: &Grant) -> bool {
    let more_packages = new
        .requires
        .iter()
        .any(|package| !old.requires.contains(package));
    let more_exports = new
        .contributes
        .iter()
        .any(|export| !old.contributes.contains(export));
    let more_schedules = new.schedules.iter().any(|schedule| {
        let every = commands::interval(&schedule.every);
        !old.schedules
            .iter()
            .any(|before| before.id == schedule.id && commands::interval(&before.every) <= every)
    });
    more_packages
        || more_exports
        || more_schedules
        || new.permissions.iter().any(|granted| {
            match old
                .permissions
                .iter()
                .find(|before| before.permission == granted.permission)
            {
                None => !granted.optional,
                Some(before) if before.optional && !granted.optional => true,
                Some(before) => scope_widens(before, granted),
            }
        })
}

/// Whether `now` reaches more than `before` did.
fn scope_widens(before: &GrantedPermission, now: &GrantedPermission) -> bool {
    match (&now.scope, &before.scope) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(now), Some(before)) => {
            now.kind != before.kind
                || now
                    .values
                    .iter()
                    .any(|value| !before.values.contains(value))
                || now
                    .unhashed
                    .iter()
                    .any(|host| !before.unhashed.contains(host))
                || now
                    .programs
                    .iter()
                    .any(|program| !before.programs.contains(program))
        }
    }
}

/// The grant as the user left it: every required permission and the optional
/// ones they granted.
pub(crate) fn effective(grant: &Grant, choice: &Choice) -> Grant {
    let mut effective = grant.clone();
    effective
        .permissions
        .retain(|p| !p.optional || choice.optional.contains(&p.permission));
    effective
}

/// What an update keeps of `choice`, which was made against `old`: an optional
/// permission stays granted only if it was optional before and still is, with a
/// scope no wider; one that was required and became optional stays granted, since
/// the update's approval covered it. Anything else arrives declined, so a
/// permission dropped and added back never returns with what nobody approved.
pub(crate) fn carry(old: &Grant, new: &Grant, choice: &Choice) -> Choice {
    let held = |name: &str| old.permissions.iter().find(|p| p.permission == name);
    let optional = new
        .permissions
        .iter()
        .filter(|now| now.optional)
        .filter(|now| match held(&now.permission) {
            Some(before) if before.optional => {
                choice.optional.contains(&now.permission) && !scope_widens(before, now)
            }
            Some(_) => true,
            None => false,
        })
        .map(|now| now.permission.clone())
        .collect();
    Choice {
        optional,
        entries: choice.entries.clone(),
    }
}

/// Refuses a choice that grants a permission the plugin does not mark optional.
pub(crate) fn check_choice(grant: &Grant, choice: &Choice) -> Result<(), GrantError> {
    match choice.optional.iter().find(|name| {
        !grant
            .permissions
            .iter()
            .any(|p| p.optional && &p.permission == *name)
    }) {
        Some(name) => Err(GrantError::NotOptional(name.clone())),
        None => Ok(()),
    }
}

/// Each import that needs a permission the grant lacks, to that permission.
pub fn denials(table: &Table, shape: &Shape, granted: &[&str]) -> Denials {
    shape
        .imports
        .iter()
        .filter_map(|import| match table.gate(unversioned(import)) {
            Gate::Needs(permission) if !granted.contains(&permission.name) => {
                Some((import.clone(), permission.name.to_owned()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proto::plugin::GrantedSchedule;

    use super::*;

    fn manifest() -> Manifest {
        Manifest::parse("id = \"t\"\nname = \"t\"\nversion = \"1\"\n").unwrap()
    }

    #[test]
    fn only_hestia_and_wasi_imports_and_known_kernel_interfaces_are_accepted() {
        let shape = |imports: &[&str], exports: &[&str]| Shape {
            imports: imports.iter().map(|s| s.to_string()).collect(),
            exports: exports.iter().map(|s| s.to_string()).collect(),
        };
        let table = Table::kernel();
        let refused = [
            (shape(&["foo:bar/baz@1.0.0"], &[]), "foo:bar"),
            (shape(&["just-a-function"], &[]), "a bare name"),
            (shape(&["hestia:kernel/nope@0.1.0"], &[]), "unknown import"),
            (shape(&[], &["hestia:kernel/nope@0.1.0"]), "unknown export"),
        ];
        for (shape, case) in refused {
            let error = derive(&table, &manifest(), &shape).unwrap_err();
            assert!(
                matches!(
                    error,
                    GrantError::UnsupportedImport(_) | GrantError::UnknownInterface(_)
                ),
                "{case}: {error}"
            );
        }

        let grant = derive(
            &table,
            &manifest(),
            &shape(
                &[
                    "wasi:io/streams@0.2.6",
                    "hestia:kernel/secrets@0.1.0",
                    "hestia:servers/read@0.1.0",
                ],
                &["hestia:kernel/lifecycle@0.1.0"],
            ),
        )
        .unwrap();
        let permissions: Vec<_> = grant.permissions.iter().map(|p| &p.permission).collect();
        assert_eq!(permissions, ["secrets"]);
        assert_eq!(grant.requires, ["hestia:servers"]);
    }

    fn derived(imports: &[&str], extra: &str) -> Result<Grant, GrantError> {
        let manifest = Manifest::parse(&format!(
            "id = \"t\"\nname = \"t\"\nversion = \"1\"\n{extra}"
        ))
        .unwrap();
        let shape = Shape {
            imports: imports.iter().map(|s| s.to_string()).collect(),
            exports: Vec::new(),
        };
        derive(&Table::kernel(), &manifest, &shape)
    }

    #[test]
    fn an_optional_mark_belongs_to_a_permission_the_component_needs() {
        let optional = "[permissions.\"secrets\"]\noptional = true\n";
        let secrets = ["hestia:kernel/secrets@0.1.0"];
        assert!(matches!(
            derived(&[], optional),
            Err(GrantError::OptionalNotNeeded(_))
        ));
        let grant = derived(&secrets, optional).unwrap();
        assert!(grant.permissions[0].optional);
        assert!(!derived(&secrets, "").unwrap().permissions[0].optional);
        let wasi = ["wasi:http/types@0.2.0"];
        let network =
            "[permissions.\"network:connect\"]\noptional = true\nhosts = [\"a.example\"]\n";
        assert!(matches!(
            derived(&wasi, network),
            Err(GrantError::OptionalUndeniable(_))
        ));
        let scoped = "[permissions.\"secrets\"]\nhosts = [\"a.example\"]\n";
        assert!(matches!(
            derived(&secrets, scoped),
            Err(GrantError::ScopeNotNeeded(_))
        ));
    }

    #[test]
    fn network_programs_and_endpoints_each_need_their_permission_and_scope() {
        let tool = std::env::temp_dir().join("tool");
        let tool = tool.display().to_string();
        let http = ["wasi:http/types@0.2.6", "wasi:http/outgoing-handler@0.2.6"];
        let hosts = |list: &str| format!("[permissions.\"network:connect\"]\nhosts = [{list}]\n");
        let programs = |entries: &str| {
            format!(
                "[permissions.\"process:spawn\"]\nprograms = [{}]\n",
                entries.replace("TOOL", &tool)
            )
        };
        let endpoints =
            |list: &str| format!("[permissions.\"ipc:connect\"]\nendpoints = [{list}]\n");
        let spawn = ["hestia:kernel/programs@0.1.0"];
        let other_os = if cfg!(windows) { "linux" } else { "windows" };

        type Check = fn(&GrantError) -> bool;
        let refused: [(&[&str], String, Check); 9] = [
            (&http, String::new(), |e| matches!(e, GrantError::ScopeMissing(_))),
            (&http, hosts("\"1.2.3.4\""), |e| matches!(e, GrantError::Host { .. })),
            (&http, hosts("\"*.com\""), |e| matches!(e, GrantError::Host { .. })),
            (
                &["hestia:kernel/download@0.1.0"],
                "[permissions.\"network:connect\"]\nhosts = [\"a.example\"]\nunhashed = [\"b.example\"]\n".into(),
                |e| matches!(e, GrantError::UnhashedOutsideHosts(..)),
            ),
            (&["wasi:http/proxy@0.2.6"], String::new(), |e| {
                matches!(e, GrantError::UnknownInterface(_))
            }),
            (&spawn, programs("{ path = 'relative/tool' }"), |e| {
                matches!(e, GrantError::Program { .. })
            }),
            (
                &spawn,
                format!("{}unhashed = [\"a.example\"]\n", programs("{ path = 'TOOL' }")),
                |e| matches!(e, GrantError::UnhashedNotTaken(_)),
            ),
            (&["hestia:kernel/ipc@0.1.0"], endpoints("'relative.sock'"), |e| {
                matches!(e, GrantError::Endpoint { .. })
            }),
            (&["hestia:kernel/ipc@0.1.0"], String::new(), |e| {
                matches!(e, GrantError::ScopeMissing(_))
            }),
        ];
        for (imports, extra, expected) in refused {
            let error = derived(imports, &extra).unwrap_err();
            assert!(expected(&error), "{imports:?} {extra:?}: {error}");
        }

        let grant = derived(&http, &hosts("\"api.example\", \"*.cdn.example\"")).unwrap();
        let [network] = &grant.permissions[..] else {
            panic!("one permission, got {:?}", grant.permissions)
        };
        assert_eq!(network.permission, "network:connect");
        assert_eq!(network.tier, proto::plugin::Tier::Sandboxed);
        assert!(grant.requires.is_empty(), "wasi:http is not a requirement");
        assert!(derived(&["wasi:io/streams@0.2.6"], "")
            .unwrap()
            .permissions
            .is_empty());

        let entries = format!(
            "{{ path = 'TOOL', args = ['--list'] }}, {{ path = 'relative', os = '{other_os}' }}"
        );
        let spawning = derived(&spawn, &programs(&entries)).unwrap();
        let [spawn_permission] = &spawning.permissions[..] else {
            panic!("one permission")
        };
        assert_eq!(spawn_permission.tier, proto::plugin::Tier::Unrestricted);
        let kept = &spawn_permission.scope.as_ref().unwrap().programs;
        assert_eq!(
            kept,
            &[Program {
                path: tool.clone(),
                args: vec!["--list".into()],
                sha256: String::new()
            }],
            "only this platform's entries are granted, and none is pinned by derive"
        );
        let elsewhere = derived(
            &spawn,
            &programs(&format!("{{ path = 'x', os = '{other_os}' }}")),
        );
        assert!(elsewhere.unwrap().permissions[0]
            .scope
            .as_ref()
            .unwrap()
            .programs
            .is_empty());

        let both = endpoints(r#"'/run/a.sock', '\\.\pipe\a'"#);
        let reached = derived(&["hestia:kernel/ipc@0.1.0"], &both).unwrap();
        let here = if cfg!(windows) {
            r"\\.\pipe\a"
        } else {
            "/run/a.sock"
        };
        assert_eq!(
            reached.permissions[0].scope.as_ref().unwrap().values,
            [here]
        );

        let digest = |grant: &Grant| approval_digest("t", b"m", b"w", grant, None);
        let mut pinned = spawning.clone();
        pinned.permissions[0].scope.as_mut().unwrap().programs[0].sha256 = "1".repeat(64);
        type Change = fn(&mut PermissionScope);
        let changes: [Change; 3] = [
            |scope| scope.programs[0].sha256 = "0".repeat(64),
            |scope| scope.programs[0].args.push("--more".into()),
            |scope| scope.unhashed.push("api.example".into()),
        ];
        for change in changes {
            let mut moved = pinned.clone();
            change(moved.permissions[0].scope.as_mut().unwrap());
            assert_ne!(
                digest(&pinned),
                digest(&moved),
                "a changed scope needs a new approval"
            );
        }
    }

    #[test]
    fn pinning_hashes_each_program_and_refuses_what_is_hestias_or_not_a_plain_file() {
        let dir = tempfile::tempdir().unwrap();
        let data = crate::install::sandbox_root(dir.path());
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir(dir.path().join("java")).unwrap();
        let (tool, own, inside) = (
            dir.path().join("java/tool"),
            dir.path().join("own"),
            data.join("x"),
        );
        for file in [&tool, &own, &inside] {
            std::fs::write(file, b"v1").unwrap();
        }
        let granted = |path: &Path, args: &[&str]| Program {
            path: path.to_str().unwrap().to_owned(),
            args: args.iter().map(|a| a.to_string()).collect(),
            sha256: String::new(),
        };
        let grant = |programs: Vec<Program>, endpoints: &[&str]| Grant {
            permissions: vec![
                GrantedPermission {
                    permission: "process:spawn".into(),
                    scope: Some(PermissionScope {
                        kind: ScopeKind::Programs,
                        programs,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                GrantedPermission {
                    permission: "ipc:connect".into(),
                    scope: Some(PermissionScope {
                        kind: ScopeKind::Endpoints,
                        values: endpoints.iter().map(|e| e.to_string()).collect(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let reserved = Reserved {
            directories: vec![data.clone()],
            programs: vec![own.clone()],
            endpoints: vec!["/run/hestiad.sock".into()],
        };

        let mut pinned = grant(
            vec![granted(&tool, &[]), granted(&tool, &["-v"])],
            &["/run/a.sock"],
        );
        pin(&mut pinned, &reserved).unwrap();
        let expected: String = Sha256::digest(b"v1")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let programs = &pinned.permissions[0].scope.as_ref().unwrap().programs;
        assert!(programs.iter().all(|program| program.sha256 == expected));

        let mut refused = vec![
            (grant(vec![granted(&own, &[])], &[]), "a Hestia binary"),
            (
                grant(vec![granted(&inside, &[])], &[]),
                "a file in a plugin's sandbox",
            ),
            (
                grant(vec![granted(&dir.path().join("none"), &[])], &[]),
                "a missing file",
            ),
            (grant(vec![granted(dir.path(), &[])], &[]), "a directory"),
            (
                grant(Vec::new(), &["/run/hestiad.sock"]),
                "Hestia's own endpoint",
            ),
        ];
        #[cfg(unix)]
        {
            let fifo = dir.path().join("fifo");
            let name = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            refused.push((
                grant(vec![granted(&fifo, &[])], &[]),
                "a pipe, which would block",
            ));
        }
        for (mut grant, case) in refused {
            assert!(pin(&mut grant, &reserved).is_err(), "{case}");
        }
    }

    #[test]
    fn anything_the_user_would_approve_again_widens_a_grant() {
        use proto::plugin::{GrantedPermission, PermissionScope, Program};

        let program = |path: &str, args: &[&str], sha256: &str| Program {
            path: path.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            sha256: sha256.into(),
        };
        let permission =
            |name: &str, kind, values: &[&str], programs: Vec<Program>| GrantedPermission {
                permission: name.into(),
                scope: Some(PermissionScope {
                    kind,
                    values: values.iter().map(|value| value.to_string()).collect(),
                    programs,
                    ..PermissionScope::default()
                }),
                ..GrantedPermission::default()
            };
        let before = Grant {
            permissions: vec![
                permission("net", ScopeKind::Hosts, &["a.example"], Vec::new()),
                permission(
                    "run",
                    ScopeKind::Programs,
                    &[],
                    vec![program("/bin/t", &[], "aa")],
                ),
                permission("ipc", ScopeKind::Endpoints, &["/run/a.sock"], Vec::new()),
            ],
            contributes: vec!["hestia:servers/flavor".into()],
            requires: vec!["hestia:servers".into()],
            schedules: vec![GrantedSchedule {
                id: "nightly".into(),
                every: "1d".into(),
            }],
        };
        type Change = fn(&mut Grant);
        let widened: [(&str, Change); 11] = [
            ("a new schedule", |g| {
                g.schedules.push(GrantedSchedule {
                    id: "extra".into(),
                    every: "1d".into(),
                })
            }),
            ("a schedule that ticks more often", |g| {
                g.schedules[0].every = "1h".into()
            }),
            ("a new permission", |g| {
                g.permissions.push(permission_named("extra"))
            }),
            ("a new required package", |g| {
                g.requires.push("hestia:content".into())
            }),
            ("a new export", |g| {
                g.contributes.push("hestia:servers/on-stopped".into())
            }),
            ("a new host", |g| {
                g.permissions[0]
                    .scope
                    .as_mut()
                    .unwrap()
                    .values
                    .push("b.example".into())
            }),
            ("a host that may now serve unchecked files", |g| {
                g.permissions[0]
                    .scope
                    .as_mut()
                    .unwrap()
                    .unhashed
                    .push("a.example".into())
            }),
            ("a new endpoint", |g| {
                g.permissions[2]
                    .scope
                    .as_mut()
                    .unwrap()
                    .values
                    .push("/run/b.sock".into())
            }),
            ("a program with another digest", |g| {
                g.permissions[1].scope.as_mut().unwrap().programs[0].sha256 = "bb".into()
            }),
            ("a program with other arguments", |g| {
                g.permissions[1].scope.as_mut().unwrap().programs[0]
                    .args
                    .push("-x".into())
            }),
            ("a program at another path", |g| {
                g.permissions[1].scope.as_mut().unwrap().programs[0].path = "/bin/u".into()
            }),
        ];
        for (case, change) in widened {
            let mut new = before.clone();
            change(&mut new);
            assert!(widens(&before, &new), "{case}");
        }
        assert!(!widens(&before, &before.clone()));
        let optional = |g: &mut Grant| {
            let mut extra = permission_named("extra");
            extra.optional = true;
            g.permissions.push(extra);
        };
        let mut with_optional = before.clone();
        optional(&mut with_optional);
        assert!(
            !widens(&before, &with_optional),
            "a new optional permission arrives declined and never widens"
        );
        let mut required_now = with_optional.clone();
        required_now.permissions.last_mut().unwrap().optional = false;
        assert!(
            widens(&with_optional, &required_now),
            "an optional permission turned required may have been declined"
        );
        assert!(!widens(&before, &Grant::default()), "dropping never widens");
        let mut slower = before.clone();
        slower.schedules[0].every = "7d".into();
        assert!(!widens(&before, &slower), "a rarer schedule never widens");
    }

    fn permission_named(name: &str) -> proto::plugin::GrantedPermission {
        proto::plugin::GrantedPermission {
            permission: name.into(),
            ..Default::default()
        }
    }
}
