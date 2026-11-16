use proto::plugin::{Grant, ProgramPin, ScopeKind};

use crate::grant::program::Invocation;
use crate::vetting::{Hosts, Policy};

/// Where a plugin may go, read from the scopes the user approved: the hosts it
/// may reach, the invocations it may run and the endpoints it may connect to.
/// What is not listed here is refused, so the default reaches nothing.
pub(super) struct Reach {
    pub policy: Policy,
    pub programs: Vec<Invocation>,
    pub pins: Vec<ProgramPin>,
    pub endpoints: Vec<String>,
}

impl Default for Reach {
    fn default() -> Self {
        let nothing = Hosts::new::<&str>(&[], &[]).expect("an empty host list is valid");
        Reach {
            policy: Policy::new(nothing),
            programs: Vec::new(),
            pins: Vec::new(),
            endpoints: Vec::new(),
        }
    }
}

impl Reach {
    pub fn of(grant: &Grant) -> Self {
        let mut reach = Reach::default();
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
                    reach.programs = scope
                        .values
                        .iter()
                        .filter_map(|entry| Invocation::parse(entry))
                        .collect();
                    reach.pins = scope.pins.clone();
                }
                ("ipc:connect", ScopeKind::Endpoints) => reach.endpoints = scope.values.clone(),
                _ => {}
            }
        }
        reach
    }
}
