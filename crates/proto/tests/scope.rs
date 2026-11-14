//! The scope table: every permission decides which scope kinds narrow it, and
//! widening, the digest and validation read that one table, so a new permission
//! or kind that is not decided fails here.

use std::collections::BTreeSet;

use proto::plugin::{
    Grant, Manifest, ManifestError, Permission, PermissionScope, ScopeKind, Tier, Touched,
};
use proto::Contract;

const CONTRIBUTION_KINDS: [ScopeKind; 5] = [
    ScopeKind::Hooks,
    ScopeKind::Events,
    ScopeKind::Schedules,
    ScopeKind::Secrets,
    ScopeKind::Themes,
];

fn valid() -> Manifest {
    Manifest {
        id: "tunnel".into(),
        version: "1.2.3".into(),
        api: Some("^1.0.0".into()),
        wasm: Some("plugin.wasm".into()),
        ..Manifest::default()
    }
}

#[cfg(feature = "ts")]
#[test]
fn every_permission_is_in_the_table() {
    use ts_rs::{Config, TS};

    let declared = Permission::decl(&Config::default());
    let named: BTreeSet<String> = declared
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    let tabled: BTreeSet<String> = Permission::ALL.iter().map(ToString::to_string).collect();
    assert_eq!(
        named, tabled,
        "a permission is missing from Permission::ALL, or listed twice"
    );
    assert_eq!(Permission::ALL.len(), tabled.len());
}

#[test]
fn every_permission_decides_its_scope_kinds() {
    for permission in Permission::ALL {
        let spec = permission.spec();
        assert_eq!(spec.tier, permission.tier(), "{permission}");
        let distinct: BTreeSet<_> = spec.scopes.iter().collect();
        assert_eq!(distinct.len(), spec.scopes.len(), "{permission}");
        assert!(
            !spec.requires_scope || !spec.scopes.is_empty(),
            "{permission} requires a scope it cannot take"
        );
        for kind in spec.scopes {
            assert!(
                !CONTRIBUTION_KINDS.contains(kind),
                "{permission} takes `{kind}`, which a manifest never declares"
            );
        }
    }
}

#[test]
fn every_scope_kind_is_declared_by_a_permission_or_derived_from_contributions() {
    for kind in ScopeKind::ALL {
        let taken = Permission::ALL
            .iter()
            .any(|permission| permission.spec().scopes.contains(&kind));
        assert_eq!(!taken, CONTRIBUTION_KINDS.contains(&kind), "{kind}");
    }
}

#[test]
fn a_scope_round_trips_and_an_unknown_kind_is_refused() {
    let mut scope = PermissionScope::of(ScopeKind::Hosts, ["api.example.com", "*.example.org"]);
    scope.extend(ScopeKind::Unhashed, ["api.example.com"]);
    let json = serde_json::to_string(&scope).unwrap();
    assert_eq!(
        serde_json::from_str::<PermissionScope>(&json).unwrap(),
        scope
    );
    assert!(serde_json::from_str::<PermissionScope>(r#"{"everything":["*"]}"#).is_err());

    let manifest = Manifest {
        permissions: [(Permission::NetworkConnect, scope)].into(),
        ..valid()
    };
    let json = serde_json::to_string(&manifest).unwrap();
    assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), manifest);
}

fn carriers() -> Vec<(Option<Permission>, ScopeKind)> {
    let mut carriers: Vec<_> = Permission::ALL
        .iter()
        .flat_map(|permission| {
            permission
                .spec()
                .scopes
                .iter()
                .map(move |kind| (Some(*permission), *kind))
        })
        .collect();
    carriers.extend(CONTRIBUTION_KINDS.map(|kind| (None, kind)));
    carriers
}

fn grant_with(carrier: Option<Permission>, kind: ScopeKind, entries: &[&str]) -> Grant {
    let scope = PermissionScope::of(kind, entries.iter().copied());
    match carrier {
        Some(permission) => Grant {
            permissions: [(permission, scope)].into(),
            ..Grant::default()
        },
        None => Grant {
            contributions: scope,
            ..Grant::default()
        },
    }
}

#[test]
fn adding_an_entry_of_any_kind_widens_and_removing_one_does_not() {
    for (carrier, kind) in carriers() {
        let narrow = grant_with(carrier, kind, &["a"]);
        let wide = grant_with(carrier, kind, &["a", "b"]);
        let label = format!("{carrier:?} {kind}");
        assert!(wide.widens(&narrow), "{label}");
        assert!(!narrow.widens(&wide), "{label}");
        assert!(!wide.widens(&wide), "{label}");
        let added = wide.widening_over(&narrow);
        let scope = match carrier {
            Some(permission) => added.permissions.get(&permission).cloned().unwrap(),
            None => added.contributions,
        };
        assert_eq!(scope, PermissionScope::of(kind, ["b"]), "{label}");
    }
}

#[test]
fn adding_a_permission_widens_whatever_its_scope() {
    for permission in Permission::ALL {
        let held = Grant::default();
        let asked = Grant {
            permissions: [(permission, PermissionScope::default())].into(),
            ..Grant::default()
        };
        assert!(asked.widens(&held), "{permission}");
        assert!(!held.widens(&asked), "{permission}");
        assert_eq!(
            asked.widening_over(&held).unrestricted().is_empty(),
            permission.tier() == Tier::Sandboxed,
            "{permission}"
        );
    }
}

#[test]
fn the_digest_changes_with_any_scope_entry() {
    let none = Default::default();
    for (carrier, kind) in carriers() {
        let one = grant_with(carrier, kind, &["a"]).approval_digest(&none);
        let two = grant_with(carrier, kind, &["a", "b"]).approval_digest(&none);
        let other = grant_with(carrier, kind, &["c"]).approval_digest(&none);
        let empty = Grant::default().approval_digest(&none);
        let label = format!("{carrier:?} {kind}");
        assert!(one != two && one != other && one != empty, "{label}");
    }
}

#[test]
fn a_kind_a_permission_does_not_take_is_refused_for_every_pair() {
    for permission in Permission::ALL {
        for kind in ScopeKind::ALL {
            if permission.spec().scopes.contains(&kind) {
                continue;
            }
            let mut manifest = valid();
            manifest
                .permissions
                .insert(permission, PermissionScope::of(kind, ["a"]));
            let errors = manifest.validate().unwrap_err();
            assert!(
                errors.contains(&ManifestError::ScopeNotAccepted { permission, kind }),
                "{permission} {kind}: {errors}"
            );
        }
    }
}

#[test]
fn a_required_scope_may_not_be_empty() {
    for permission in Permission::ALL {
        let spec = permission.spec();
        let mut manifest = valid();
        manifest
            .permissions
            .insert(permission, PermissionScope::default());
        let verdict = manifest.validate();
        match spec.scopes.first().filter(|_| spec.requires_scope) {
            Some(kind) => assert!(
                verdict
                    .unwrap_err()
                    .contains(&ManifestError::ScopeRequired {
                        permission,
                        kind: *kind
                    }),
                "{permission}"
            ),
            None => assert!(verdict.is_ok(), "{permission}"),
        }
    }
}

#[test]
fn every_entry_of_every_declared_kind_is_checked_and_all_errors_are_reported() {
    let mut manifest = valid();
    manifest.permissions.insert(
        Permission::NetworkConnect,
        PermissionScope::of(ScopeKind::Hosts, ["10.0.0.1", "[::1]"]),
    );
    manifest.permissions.insert(
        Permission::IpcConnect,
        PermissionScope::of(ScopeKind::Endpoints, ["*", "a b"]),
    );
    manifest.permissions.insert(
        Permission::LaunchPatch,
        PermissionScope::of(ScopeKind::Properties, ["java.library.path"]),
    );
    let errors = manifest.validate().unwrap_err();
    for expected in [
        ManifestError::BadNetworkHost("10.0.0.1".into()),
        ManifestError::BadNetworkHost("[::1]".into()),
        ManifestError::BadIpcEndpoint("*".into()),
        ManifestError::BadIpcEndpoint("a b".into()),
        ManifestError::BadLaunchProperty("java.library.path".into()),
    ] {
        assert!(errors.contains(&expected), "{expected:?} missing: {errors}");
    }
}

#[test]
fn an_absent_grant_reaches_nothing() {
    let grant = Grant {
        permissions: [(Permission::ContentRead, PermissionScope::default())].into(),
        ..Grant::default()
    };
    assert_eq!(grant.hosts().count(), 0);
    assert_eq!(grant.endpoints().count(), 0);
    assert_eq!(grant.properties().count(), 0);
    assert!(grant.program("git").is_none());
    for kind in ScopeKind::ALL {
        for permission in Permission::ALL {
            assert!(!grant.covers(permission, kind, "anything"));
        }
    }
}

#[test]
fn a_scope_that_is_not_the_permissions_is_not_reached() {
    let grant = Grant {
        permissions: [(
            Permission::EntryRead,
            PermissionScope::of(ScopeKind::Hosts, ["example.com"]),
        )]
        .into(),
        ..Grant::default()
    };
    assert_eq!(grant.hosts().count(), 0);
}

#[test]
fn the_files_channel_declares_what_it_touches() {
    use proto::files::{ServerFileParams, ServerFilesRead};

    assert_eq!(ServerFilesRead::NARROWED_BY, Some(ScopeKind::Files));
    let touched = ServerFilesRead::touched(&serde_json::json!({
        "server": "smp",
        "path": "ops.json",
    }));
    assert_eq!(
        touched,
        Touched::Items(ScopeKind::Files, vec!["ops.json".into()])
    );
    assert_eq!(
        ServerFilesRead::touched(&serde_json::json!({ "path": 3 })),
        Touched::Malformed
    );
    let params = ServerFileParams {
        server: "smp".into(),
        path: "a".into(),
    };
    assert_eq!(ServerFilesRead::touches(&params), vec!["a".to_string()]);
    assert_eq!(
        proto::files::ServerFilesList::touched(&serde_json::json!({})),
        Touched::Unscoped
    );
}
