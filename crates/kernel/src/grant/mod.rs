//! What a component asks for, read from its imports and exports, and the
//! digest the user's approval is bound to.

mod table;

use std::collections::{BTreeMap, BTreeSet};

use proto::plugin::{Grant, GrantedPermission, PermissionScope, ScopeKind};
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
        if interface.starts_with("wasi:") {
            continue;
        }
        let Some(package) = package_of(interface).filter(|p| p.starts_with("hestia:")) else {
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
                if values.is_empty() {
                    return Err(GrantError::ScopeMissing(name.to_owned()));
                }
                if kind == ScopeKind::Hosts {
                    crate::vetting::Hosts::new(values, &[] as &[String]).map_err(|reason| {
                        GrantError::Host {
                            permission: name.to_owned(),
                            reason,
                        }
                    })?;
                }
                Some(PermissionScope {
                    kind,
                    values: values.to_vec(),
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
    field(b"hestia-approval-v2");
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
}
