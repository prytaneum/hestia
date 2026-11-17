//! Reading an installed package back and linking it against what was approved.

use std::sync::Arc;

use proto::plugin::{Choice, Grant, PluginState};

use super::super::{Kind, Plugins, Slot};
use super::*;
use crate::entries::Entries;
use crate::error::{PackageError, PluginError};
use crate::grant;
use crate::install::{Package, Source};
use crate::runtime::Sandbox;

impl Plugins {
    pub(in crate::registry) fn sandbox(&self) -> Result<&Arc<Sandbox>, PluginError> {
        self.sandbox.as_ref().ok_or(PluginError::NoSandbox)
    }

    fn providers(&self, requires: &[String]) -> Vec<String> {
        self.slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Builtin(plugin) => plugin
                    .descriptor()
                    .provides
                    .filter(|package| requires.iter().any(|r| r == package))
                    .map(|_| slot.id().to_owned()),
                Kind::Guest(_) => None,
            })
            .collect()
    }

    pub(in crate::registry) fn require_provided(
        &self,
        requires: &[String],
    ) -> Result<(), PluginError> {
        let slots = self.slots();
        let provided = |package: &str| {
            slots.iter().any(|slot| {
                slot.state == PluginState::Enabled
                    && matches!(&slot.kind, Kind::Builtin(plugin)
                        if plugin.descriptor().provides == Some(package))
            })
        };
        match requires.iter().find(|package| !provided(package)) {
            Some(package) => Err(PackageError::Unprovided(package.clone()).into()),
            None => Ok(()),
        }
    }

    /// Reads an installed package and links it if it still derives the digest
    /// the user approved.
    /// `live` is the scope the plugin runs with now, when the guest replaces one
    /// that is running.
    pub(in crate::registry) async fn load_guest(
        &self,
        id: &str,
        live: Option<Entries>,
    ) -> Result<Guest, PluginError> {
        let sandbox = self.sandbox()?.clone();
        let package = read(&self.store.dir(id)).await?;
        if package.manifest.id != id {
            return Err(PackageError::Renamed(package.manifest.id).into());
        }
        let record = self.store.record(id).unwrap_or_default();
        let inspection = self.inspection(&package, &record.source).await?;
        if record.digest != inspection.approval_digest {
            return Err(PluginError::Changed(id.to_owned()));
        }
        if let Some(reason) = record.revoked {
            return Err(revoked(id, &package.manifest.version, &reason));
        }
        self.link(
            &sandbox,
            package,
            &inspection.grant,
            record.source,
            &record.choice,
            live.unwrap_or_else(|| Entries::new(record.choice.entries.clone())),
        )
        .await
    }

    /// Compiles the very bytes the digest covered, each ungranted import
    /// linked to the denied stub.
    pub(super) async fn link(
        &self,
        sandbox: &Sandbox,
        package: Package,
        grant: &Grant,
        source: Source,
        choice: &Choice,
        entries: Entries,
    ) -> Result<Guest, PluginError> {
        let effective = grant::effective(grant, choice);
        let granted: Vec<String> = effective
            .permissions
            .iter()
            .map(|granted| granted.permission.clone())
            .collect();
        let names: Vec<&str> = granted.iter().map(String::as_str).collect();
        let denials = grant::denials(&self.table, &package.shape, &names);
        let Package { manifest, wasm, .. } = package;
        let opened = sandbox
            .open(&manifest.id, wasm)
            .await
            .map_err(PackageError::from)?;
        let loaded = sandbox
            .link(&opened, &denials, &effective, entries.clone())
            .map_err(PackageError::from)?;
        Ok(Guest {
            id: manifest.id,
            name: manifest.name.trim().to_owned(),
            version: manifest.version,
            source,
            granted,
            requires: grant.requires.clone(),
            providers: self.providers(&grant.requires),
            dependencies: grant.dependencies.clone(),
            commands: manifest.commands,
            schedules: manifest.schedules,
            optional: effective
                .permissions
                .iter()
                .filter(|granted| granted.optional)
                .map(|granted| granted.permission.clone())
                .collect(),
            scoped: effective.permissions.iter().any(|granted| granted.entries),
            entries,
            loaded: Some(loaded),
            #[cfg(feature = "probe")]
            inbox: Default::default(),
        })
    }

    pub(in crate::registry) fn add(&self, guest: Guest, state: PluginState) -> Arc<Guest> {
        let guest = Arc::new(guest);
        self.slots().push(Slot {
            kind: Kind::Guest(guest.clone()),
            state,
        });
        guest
    }

    pub(super) fn dependencies_ready(&self, guest: &Guest) -> Result<(), PluginError> {
        self.require_provided(&guest.requires)?;
        guest
            .dependencies
            .iter()
            .try_for_each(|dependency| self.dependency_ready(&guest.id, dependency))
    }
}
