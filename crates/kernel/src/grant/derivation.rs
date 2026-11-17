//! Deriving a grant from what a component imports, exports and its manifest.

use std::collections::{BTreeMap, BTreeSet};

use proto::plugin::{Dependency, Grant, GrantedPermission, PermissionScope, Program, ScopeKind};

use super::{
    deniable, endpoint, package_of, program, unversioned, Gate, GrantError, Permission, Table,
    KERNEL_PACKAGE,
};
use crate::commands::ScheduleDecl;
use crate::manifest::{Manifest, Scopes};
use crate::shape::Shape;

/// The scope the manifest writes; the entries scope is the user's.
fn manifest_scope(permission: &Permission) -> Option<ScopeKind> {
    permission.scope.filter(|kind| *kind != ScopeKind::Entries)
}

/// A permission is needed by importing or exporting an interface that `table`
/// maps to it (a topic's export needs its read permission). A `hestia:` package
/// other than the kernel's, imported or exported, is a requirement on the
/// built-in providing it. Pure: a program's digest is left empty, for [`pin`] to fill.
/// A program or endpoint written for another platform is left out, as the
/// sandbox does not grant it here.
pub fn derive(table: &Table, manifest: &Manifest, shape: &Shape) -> Result<Grant, GrantError> {
    derive_for(table, manifest, shape, Platforms::Here)
}

/// [`derive`] with the programs and endpoints of every platform, the same on
/// every machine: what a review of the plugin's reach needs. An entry that is
/// not valid for the platform it is written for is refused.
pub fn derive_everywhere(
    table: &Table,
    manifest: &Manifest,
    shape: &Shape,
) -> Result<Grant, GrantError> {
    derive_for(table, manifest, shape, Platforms::Every)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Platforms {
    Here,
    Every,
}

fn derive_for(
    table: &Table,
    manifest: &Manifest,
    shape: &Shape,
    platforms: Platforms,
) -> Result<Grant, GrantError> {
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
        let Some(package) = package_of(interface) else {
            continue;
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
            Gate::Unknown => return Err(GrantError::UnknownInterface(export.clone())),
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
                    platforms,
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
        dependencies: manifest
            .dependencies
            .iter()
            .map(|(id, range)| Dependency {
                id: id.clone(),
                range: range.clone(),
            })
            .collect(),
        schedules: manifest
            .schedules
            .iter()
            .map(ScheduleDecl::granted)
            .collect(),
    })
}

fn scope_of(
    name: &str,
    kind: ScopeKind,
    scopes: &Scopes,
    platforms: Platforms,
) -> Result<PermissionScope, GrantError> {
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
                if endpoint_here(&entry)? || platforms == Platforms::Every {
                    here.push(entry);
                }
            }
            scope.values = here;
        }
        ScopeKind::Programs => {
            for entry in scopes.programs.iter().filter(|e| {
                platforms == Platforms::Every
                    || (e.os.is_none_or(|os| os.is_current())
                        && program::Family::of_entry(e) == program::Family::HERE)
            }) {
                if let Err(reason) = program::check(&entry.path, program::Family::of_entry(entry)) {
                    return Err(GrantError::Program {
                        permission: name.to_owned(),
                        entry: entry.path.clone(),
                        reason: reason.to_owned(),
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

/// One manifest can list both forms.
fn endpoint_here(entry: &str) -> Result<bool, GrantError> {
    let unix = entry.starts_with('/');
    let pipe = endpoint::pipe_name(entry);
    if !unix && pipe.is_none() {
        return Err(GrantError::Endpoint {
            entry: entry.to_owned(),
            reason: format!("write an absolute path or a {} name", endpoint::PIPE_PREFIX),
        });
    }
    if pipe.is_some_and(|name| !endpoint::plain(name)) {
        return Err(GrantError::Endpoint {
            entry: entry.to_owned(),
            reason:
                "a pipe name is one plain segment, with no `.`, `..`, separators or trailing dots"
                    .into(),
        });
    }
    Ok(pipe.is_some() == cfg!(windows))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::grant::approval_digest;

    fn manifest() -> Manifest {
        Manifest::parse("id = \"t\"\nname = \"t\"\nversion = \"1\"\n").unwrap()
    }

    #[test]
    fn only_hestia_and_wasi_imports_and_known_kernel_interfaces_are_accepted() {
        let shape = |imports: &[&str], exports: &[&str]| Shape {
            imports: imports.iter().map(|s| s.to_string()).collect(),
            exports: exports.iter().map(|s| s.to_string()).collect(),
        };
        let table = crate::grant::kernel_table();
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

    pub(in crate::grant) fn derived(imports: &[&str], extra: &str) -> Result<Grant, GrantError> {
        let manifest = Manifest::parse(&format!(
            "id = \"t\"\nname = \"t\"\nversion = \"1\"\n{extra}"
        ))
        .unwrap();
        let shape = Shape {
            imports: imports.iter().map(|s| s.to_string()).collect(),
            exports: Vec::new(),
        };
        derive(&crate::grant::kernel_table(), &manifest, &shape)
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
        let tool = std::env::temp_dir().join(if cfg!(windows) { "tool.exe" } else { "tool" });
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
        let refused: [(&[&str], String, Check); 11] = [
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
            (
                &["hestia:kernel/ipc@0.1.0"],
                endpoints(r#"'\\.\pipe\x\..\hestia-hestiad'"#),
                |e| matches!(e, GrantError::Endpoint { .. }),
            ),
            (
                &["hestia:kernel/ipc@0.1.0"],
                endpoints(r#"'\\.\pipe\hestia-hestiad.'"#),
                |e| matches!(e, GrantError::Endpoint { .. }),
            ),
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
    fn the_grant_for_review_holds_every_platforms_programs_and_endpoints() {
        let manifest = |extra: &str| {
            Manifest::parse(&format!(
                "id = \"t\"\nname = \"t\"\nversion = \"1\"\n{extra}"
            ))
            .unwrap()
        };
        let shape = Shape {
            imports: vec![
                "hestia:kernel/programs@0.1.0".into(),
                "hestia:kernel/ipc@0.1.0".into(),
            ],
            exports: Vec::new(),
        };
        let everywhere = |extra: &str| {
            derive_everywhere(&crate::grant::kernel_table(), &manifest(extra), &shape)
        };
        let both = r#"
[permissions."process:spawn"]
programs = [{ path = '/usr/bin/a', os = 'linux' }, { path = 'C:\a.exe', os = 'windows' }]
[permissions."ipc:connect"]
endpoints = ['/run/a.sock', '\\.\pipe\a']
"#;
        let grant = everywhere(both).unwrap();
        let scopes: Vec<_> = grant
            .permissions
            .iter()
            .map(|p| p.scope.as_ref().unwrap())
            .collect();
        assert_eq!(scopes[0].values, ["/run/a.sock", r"\\.\pipe\a"]);
        let paths: Vec<_> = scopes[1].programs.iter().map(|p| &p.path[..]).collect();
        assert_eq!(paths, ["/usr/bin/a", r"C:\a.exe"]);
        assert!(matches!(
            everywhere(&both.replace("/usr/bin/a", "bin/a")),
            Err(GrantError::Program { .. })
        ));
    }
}
