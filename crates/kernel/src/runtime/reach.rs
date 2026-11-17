use proto::plugin::{Grant, Program, ScopeKind};

use crate::entries::Entries;
use crate::vetting::{Hosts, Policy};

/// Where a plugin may go, read from the scopes the user approved: the hosts it
/// may reach, the invocations it may run and the endpoints it may connect to.
/// What is not listed here is refused, so the default reaches nothing.
pub(super) struct Reach {
    pub policy: Policy,
    pub programs: Vec<Program>,
    pub endpoints: Vec<String>,
    /// The permissions the plugin holds, which `permissions.granted` answers from.
    pub granted: Vec<String>,
    pub entries: Entries,
}

impl Default for Reach {
    fn default() -> Self {
        let nothing = Hosts::new::<&str>(&[], &[]).expect("an empty host list is valid");
        Reach {
            policy: Policy::new(nothing),
            programs: Vec::new(),
            endpoints: Vec::new(),
            granted: Vec::new(),
            entries: Entries::default(),
        }
    }
}

impl Reach {
    pub fn of(grant: &Grant) -> Self {
        let mut reach = Reach {
            granted: grant
                .permissions
                .iter()
                .map(|p| p.permission.clone())
                .collect(),
            ..Reach::default()
        };
        for granted in &grant.permissions {
            let Some(scope) = &granted.scope else {
                continue;
            };
            match (granted.permission.as_str(), scope.kind) {
                ("network:connect", ScopeKind::Hosts) => {
                    match Hosts::new(&scope.values, &scope.unhashed) {
                        Ok(hosts) => reach.policy = Policy::new(hosts),
                        Err(error) => {
                            tracing::warn!(%error, "granted hosts are not valid; none are reachable")
                        }
                    }
                }
                ("process:spawn", ScopeKind::Programs) => {
                    reach.programs = scope.programs.clone();
                }
                ("ipc:connect", ScopeKind::Endpoints) => reach.endpoints = scope.values.clone(),
                _ => {}
            }
        }
        reach
    }
}
