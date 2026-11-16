//! What a component asks for, read from its imports and exports, and the
//! digest the user's approval is bound to.

pub(crate) mod program;
mod table;

use std::collections::{BTreeMap, BTreeSet};

use proto::plugin::{Grant, GrantedPermission, PermissionScope, ProgramPin, ScopeKind};
use sha2::{Digest, Sha256};

use crate::manifest::Manifest;
use crate::runtime::{Denials, Shape};
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
/// package other than the kernel's, or one the table does not know, is a
/// requirement on the plugin providing it.
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
        let known = table.knows_package(package);
        if !known || package != KERNEL_PACKAGE {
            requires.insert(package.to_owned());
        }
        if !known {
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
                let values = scopes.map(|s| s.values(kind)).unwrap_or_default();
                let unhashed = scopes.map(|s| s.unhashed.as_slice()).unwrap_or_default();
                if values.is_empty() {
                    return Err(GrantError::ScopeMissing(name.to_owned()));
                }
                if kind != ScopeKind::Hosts && !unhashed.is_empty() {
                    return Err(GrantError::UnhashedNotTaken(name.to_owned()));
                }
                if kind == ScopeKind::Hosts {
                    crate::vetting::Hosts::new(values, unhashed).map_err(|reason| {
                        GrantError::Host {
                            permission: name.to_owned(),
                            reason,
                        }
                    })?;
                }
                let pins = match kind {
                    ScopeKind::Programs => pin_programs(name, values)?,
                    _ => Vec::new(),
                };
                Some(PermissionScope {
                    kind,
                    values: values.to_vec(),
                    unhashed: unhashed.to_vec(),
                    pins,
                })
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

/// The file each declared invocation runs, as it is now.
fn pin_programs(permission: &str, entries: &[String]) -> Result<Vec<ProgramPin>, GrantError> {
    let refuse = |entry: &str, reason: String| GrantError::Program {
        permission: permission.to_owned(),
        entry: entry.to_owned(),
        reason,
    };
    let mut pins: Vec<ProgramPin> = Vec::new();
    for entry in entries {
        let invocation = program::Invocation::parse(entry).ok_or_else(|| {
            refuse(
                entry,
                "write an absolute path, then each argument after a single space".to_owned(),
            )
        })?;
        if pins.iter().any(|pin| pin.path == invocation.program) {
            continue;
        }
        let sha256 = program::digest(std::path::Path::new(&invocation.program))
            .map_err(|error| refuse(entry, format!("cannot read it: {error}")))?;
        pins.push(ProgramPin {
            path: invocation.program,
            sha256,
        });
    }
    Ok(pins)
}

/// Binds an approval to the package and what it reaches: the id, the exact
/// manifest and component bytes, each permission with its scope, and the
/// packages it requires. Hestia's own prompt text and tiers are left out, so
/// rewording them never invalidates an approval.
pub fn approval_digest(id: &str, manifest: &[u8], wasm: &[u8], grant: &Grant) -> String {
    let mut hash = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    field(b"hestia-approval-v3");
    field(id.as_bytes());
    field(&Sha256::digest(manifest));
    field(&Sha256::digest(wasm));
    let mut permissions: Vec<_> = grant.permissions.iter().collect();
    permissions.sort_by(|a, b| a.permission.cmp(&b.permission));
    field(&(permissions.len() as u64).to_le_bytes());
    for permission in permissions {
        field(permission.permission.as_bytes());
        let mut values: Vec<&str> = match &permission.scope {
            Some(scope) => {
                field(scope.kind.to_string().as_bytes());
                scope.values.iter().map(String::as_str).collect()
            }
            None => {
                field(b"");
                Vec::new()
            }
        };
        values.sort_unstable();
        field(&(values.len() as u64).to_le_bytes());
        for value in values {
            field(value.as_bytes());
        }
        let (mut unhashed, mut pins): (Vec<&str>, Vec<(&str, &str)>) = match &permission.scope {
            Some(scope) => (
                scope.unhashed.iter().map(String::as_str).collect(),
                scope
                    .pins
                    .iter()
                    .map(|pin| (pin.path.as_str(), pin.sha256.as_str()))
                    .collect(),
            ),
            None => Default::default(),
        };
        unhashed.sort_unstable();
        field(&(unhashed.len() as u64).to_le_bytes());
        for host in unhashed {
            field(host.as_bytes());
        }
        pins.sort_unstable();
        field(&(pins.len() as u64).to_le_bytes());
        for (path, sha256) in pins {
            field(path.as_bytes());
            field(sha256.as_bytes());
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
        let exe = std::env::current_exe().unwrap();
        let exe = exe.display();
        let http = ["wasi:http/types@0.2.6", "wasi:http/outgoing-handler@0.2.6"];
        let hosts = |list: &str| format!("[permissions.\"network:connect\"]\nhosts = [{list}]\n");
        let programs =
            |entry: &str| format!("[permissions.\"process:spawn\"]\nprograms = ['{entry}']\n");

        type Check = fn(&GrantError) -> bool;
        let refused: [(&[&str], String, Check); 9] = [
            (&http, String::new(), |e| matches!(e, GrantError::ScopeMissing(_))),
            (&http, hosts("\"1.2.3.4\""), |e| matches!(e, GrantError::Host { .. })),
            (&http, hosts("\"*.com\""), |e| matches!(e, GrantError::Host { .. })),
            (
                &["hestia:kernel/download@0.1.0"],
                "[permissions.\"network:connect\"]\nhosts = [\"a.example\"]\nunhashed = [\"1.2.3.4\"]\n".into(),
                |e| matches!(e, GrantError::Host { .. }),
            ),
            (&["wasi:http/proxy@0.2.6"], String::new(), |e| {
                matches!(e, GrantError::UnknownInterface(_))
            }),
            (&["hestia:kernel/programs@0.1.0"], programs("relative/tool --x"), |e| {
                matches!(e, GrantError::Program { .. })
            }),
            (&["hestia:kernel/programs@0.1.0"], programs("/no/such/program"), |e| {
                matches!(e, GrantError::Program { .. })
            }),
            (&["hestia:kernel/programs@0.1.0"], programs(&format!("{exe}  --double-space")), |e| {
                matches!(e, GrantError::Program { .. })
            }),
            (
                &["hestia:kernel/programs@0.1.0"],
                format!("[permissions.\"process:spawn\"]\nprograms = ['{exe}']\nunhashed = [\"a.example\"]\n"),
                |e| matches!(e, GrantError::UnhashedNotTaken(_)),
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
        assert!(derived(&["wasi:io/streams@0.2.6"], "")
            .unwrap()
            .permissions
            .is_empty());

        let program = programs(&format!("{exe} --list"));
        let pinned = derived(&["hestia:kernel/programs@0.1.0"], &program).unwrap();
        let [spawn] = &pinned.permissions[..] else {
            panic!("one permission")
        };
        assert_eq!(spawn.tier, proto::plugin::Tier::Unrestricted);
        let pins = &spawn.scope.as_ref().unwrap().pins;
        assert_eq!(pins.len(), 1);
        assert_eq!(
            pins[0].sha256,
            program::digest(std::path::Path::new(&pins[0].path)).unwrap()
        );

        let mut moved = pinned.clone();
        moved.permissions[0].scope.as_mut().unwrap().pins[0].sha256 = "0".repeat(64);
        let digest = |grant: &Grant| approval_digest("t", b"m", b"w", grant);
        assert_ne!(
            digest(&pinned),
            digest(&moved),
            "a moved pin needs a new approval"
        );
        let mut widened = grant.clone();
        widened.permissions[0].scope.as_mut().unwrap().unhashed = vec!["api.example".into()];
        assert_ne!(digest(&grant), digest(&widened));
    }
}
