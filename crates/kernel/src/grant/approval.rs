//! What the user approved: the digest an approval is bound to, whether a new
//! grant widens an old one, and what an optional choice carries across an update.

#[cfg(feature = "sandbox")]
use proto::plugin::Choice;
use proto::plugin::{Grant, GrantedPermission};
#[cfg(feature = "sandbox")]
use sha2::{Digest, Sha256};

#[cfg(feature = "sandbox")]
use super::GrantError;
use crate::commands;

#[cfg(feature = "sandbox")]
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
    hex::encode(hash.finalize())
}

/// Whether `new` asks for anything `old` did not: a permission, a scope entry
/// (a host, an endpoint, a host served without checksum, or a program whose
/// path, arguments or digest pin differ), a required package, a dependency, an
/// export, or a schedule that is new or ticks more often. Dropping or narrowing
/// never widens, and a new optional permission arrives declined, so it does not
/// widen either.
pub fn widens(old: &Grant, new: &Grant) -> bool {
    let more_packages = new
        .requires
        .iter()
        .any(|package| !old.requires.contains(package));
    let more_dependencies = new
        .dependencies
        .iter()
        .any(|dependency| !old.dependencies.contains(dependency));
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
        || more_dependencies
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

#[cfg(feature = "sandbox")]
/// The grant as the user left it: every required permission and the optional
/// ones they granted.
pub(crate) fn effective(grant: &Grant, choice: &Choice) -> Grant {
    let mut effective = grant.clone();
    effective
        .permissions
        .retain(|p| !p.optional || choice.optional.contains(&p.permission));
    effective
}

#[cfg(feature = "sandbox")]
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

#[cfg(feature = "sandbox")]
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

#[cfg(test)]
mod tests {
    use proto::plugin::{Dependency, GrantedSchedule, ScopeKind};

    use super::super::derivation::tests::derived;

    use super::*;
    #[test]
    fn rewording_the_prompt_or_tier_keeps_an_approval() {
        let mut grant = derived(&["hestia:kernel/secrets@0.1.0"], "").unwrap();
        let before = approval_digest("t", b"m", b"w", &grant, None);
        grant.permissions[0].prompt = "Hestia's new wording".into();
        grant.permissions[0].tier = proto::plugin::Tier::Unrestricted;
        assert_eq!(before, approval_digest("t", b"m", b"w", &grant, None));
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
            dependencies: vec![Dependency {
                id: "base".into(),
                range: "^1.0".into(),
            }],
            schedules: vec![GrantedSchedule {
                id: "nightly".into(),
                every: "1d".into(),
            }],
        };
        type Change = fn(&mut Grant);
        let widened: [(&str, Change); 13] = [
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
            ("a new dependency", |g| {
                g.dependencies.push(Dependency {
                    id: "extra".into(),
                    range: "^1.0".into(),
                })
            }),
            ("a dependency on another range", |g| {
                g.dependencies[0].range = "^2.0".into()
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
