//! What a component asks for, read from its imports and exports, and the
//! digest the user's approval is bound to.

mod pin;
mod table;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proto::plugin::{Grant, GrantedPermission, PermissionScope, Program, ScopeKind};
use sha2::{Digest, Sha256};

use crate::manifest::{Manifest, Scopes};
use crate::runtime::{Denials, Shape};
pub(crate) use pin::digest;
pub use pin::{pin, Reserved};
pub use table::{Gate, Permission, Table};

#[derive(Debug, thiserror::Error)]
pub enum GrantError {
    #[error("the component imports '{0}', which is not an interface Hestia provides")]
    UnsupportedImport(String),
    #[error("'{0}' is not an interface of its package")]
    UnknownInterface(String),
    #[error("plugin.toml gives a scope to '{0}', which the component does not need")]
    ScopeNotNeeded(String),
    #[error("plugin.toml gives '{permission}' {given} scope, but it takes {takes}")]
    ScopeKind {
        permission: String,
        given: String,
        takes: String,
    },
    #[error("'{0}' needs a scope in plugin.toml")]
    ScopeMissing(String),
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
    if let Some(unneeded) = manifest
        .permissions
        .keys()
        .find(|name| !needed.get(name.as_str()).is_some_and(|p| p.scope.is_some()))
    {
        return Err(GrantError::ScopeNotNeeded(unneeded.clone()));
    }
    let mut permissions = Vec::new();
    for (name, permission) in needed {
        let scope = match permission.scope {
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
        });
    }
    Ok(Grant {
        permissions,
        contributes: shape.exports.clone(),
        requires: requires.into_iter().collect(),
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
        ScopeKind::Paths => {}
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
/// and the packages it requires. Hestia's own prompt text and tiers are left
/// out, so rewording them never invalidates an approval.
pub fn approval_digest(id: &str, manifest: &[u8], wasm: &[u8], grant: &Grant) -> String {
    let mut hash = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    field(b"hestia-approval-v4");
    field(id.as_bytes());
    field(&Sha256::digest(manifest));
    field(&Sha256::digest(wasm));
    let mut permissions: Vec<_> = grant.permissions.iter().collect();
    permissions.sort_by(|a, b| a.permission.cmp(&b.permission));
    field(&(permissions.len() as u64).to_le_bytes());
    for permission in permissions {
        field(permission.permission.as_bytes());
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
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

        let digest = |grant: &Grant| approval_digest("t", b"m", b"w", grant);
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
}
