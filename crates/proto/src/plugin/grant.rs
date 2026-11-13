use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::permission::{Permission, Tier};
use super::scope::{PermissionScope, ScopeKind};
use super::{Program, ScheduleContribution};

/// Everything a user approves, canonical and sorted: each permission with the
/// scope it reaches, and what the manifest contributes. Approval, widening and
/// re-approval compare grants, never bare permission lists.
///
/// A scope is the only shape a grant has, so validation, widening, the digest
/// and the approval prompt each walk it once and cover every kind by
/// construction ([0117](../../../../docs/decisions/0117-a-grant-is-permission-to-scope.md)).
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Grant {
    pub permissions: BTreeMap<Permission, PermissionScope>,
    /// What the manifest contributes and the user approves with it: hooks,
    /// events, schedules, the themes it restyles with and its secret settings.
    pub contributions: PermissionScope,
    /// The definition behind each entry of `process:spawn`'s `programs` scope.
    /// The entry is the definition rendered whole, so a changed definition is a
    /// new entry and widens; this map is how the host finds what to run.
    /// A widening leaves it empty.
    pub programs: BTreeMap<String, Program>,
}

impl Grant {
    /// What a user approves: hex sha256 of the canonical JSON of the grant
    /// together with the absolute path each `PATH` program is pinned to
    /// ([0101](../../../../docs/decisions/0101-plugins-spawn-only-declared-programs.md)).
    /// The only place the digest is computed.
    pub fn approval_digest(&self, program_paths: &BTreeMap<String, String>) -> String {
        let canonical = serde_json::to_vec(&serde_json::json!({
            "grant": self,
            "programPaths": program_paths,
        }))
        .expect("an approval is plain data");
        Sha256::digest(canonical)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn holds(&self, permission: Permission) -> bool {
        self.permissions.contains_key(&permission)
    }

    /// The entries of `kind` that `permission` reaches; none when the
    /// permission is not held, so an absent grant reaches nothing.
    pub fn entries(&self, permission: Permission, kind: ScopeKind) -> impl Iterator<Item = &str> {
        self.permissions
            .get(&permission)
            .into_iter()
            .flat_map(move |scope| scope.entries(kind))
    }

    /// Whether `permission` is held and its scope covers `item` of `kind`.
    pub fn covers(&self, permission: Permission, kind: ScopeKind, item: &str) -> bool {
        self.permissions
            .get(&permission)
            .is_some_and(|scope| scope.covers(kind, item))
    }

    /// The hosts `network:connect` reaches.
    pub fn hosts(&self) -> impl Iterator<Item = &str> {
        self.entries(Permission::NetworkConnect, ScopeKind::Hosts)
    }

    /// The hosts a flavor may download from without a checksum.
    pub fn unhashed(&self) -> impl Iterator<Item = &str> {
        self.entries(Permission::NetworkConnect, ScopeKind::Unhashed)
    }

    /// The local endpoint patterns `ipc:connect` reaches.
    pub fn endpoints(&self) -> impl Iterator<Item = &str> {
        self.entries(Permission::IpcConnect, ScopeKind::Endpoints)
    }

    /// The `-D` keys a launch hook may set; none without `launch:patch`.
    pub fn properties(&self) -> impl Iterator<Item = &str> {
        self.entries(Permission::LaunchPatch, ScopeKind::Properties)
    }

    /// The program `id`, when `process:spawn` is held.
    pub fn program(&self, id: &str) -> Option<&Program> {
        self.holds(Permission::ProcessSpawn)
            .then(|| self.programs.get(id))
            .flatten()
    }

    pub fn contributed(&self, kind: ScopeKind) -> impl Iterator<Item = &str> {
        self.contributions.entries(kind)
    }

    pub fn schedules(&self) -> impl Iterator<Item = ScheduleContribution> + '_ {
        self.contributed(ScopeKind::Schedules)
            .filter_map(ScheduleContribution::from_entry)
    }

    /// The permissions that reach beyond the sandbox.
    pub fn unrestricted(&self) -> Vec<Permission> {
        self.permissions
            .keys()
            .copied()
            .filter(|permission| permission.tier() == Tier::Unrestricted)
            .collect()
    }

    /// What this grant asks for beyond `installed`: a permission it adds whole,
    /// and for one both hold, the scope entries it adds. Narrowing is not a
    /// widening. It walks the maps, so a new scope kind is covered.
    pub fn widening_over(&self, installed: &Grant) -> Grant {
        let permissions = self
            .permissions
            .iter()
            .filter_map(
                |(permission, scope)| match installed.permissions.get(permission) {
                    None => Some((*permission, scope.clone())),
                    Some(held) => {
                        let added = scope.minus(held);
                        (!added.is_empty()).then_some((*permission, added))
                    }
                },
            )
            .collect();
        Grant {
            permissions,
            contributions: self.contributions.minus(&installed.contributions),
            programs: BTreeMap::new(),
        }
    }

    /// Whether this grant asks for anything `installed` lacks.
    pub fn widens(&self, installed: &Grant) -> bool {
        self.widening_over(installed) != Grant::default()
    }
}
