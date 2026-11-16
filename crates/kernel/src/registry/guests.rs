//! The WASM half of the registry: reading a package, installing it, and
//! loading what is installed. Every change to a guest runs under
//! `Plugins::changes`, so installs, enables, disables and removals never
//! interleave.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use proto::error::IoOp;
use proto::plugin::{Grant, Inspection, PluginInfo, PluginState};

use crate::trust::TrustError;

use super::{find, info, schema, Kind, Plugins, Saved, Slot};
use crate::error::{PackageError, PluginError};
use crate::grant;
use crate::install::{Approval, Package, Source};
use crate::runtime::{Loaded, Sandbox, SandboxError};

const SECRETS: &str = "secrets";

/// A WASM plugin in the registry. `loaded` is missing when its package could
/// not be read or no longer derives what was approved.
pub(crate) struct Guest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: Source,
    granted: Vec<String>,
    /// The packages its imports need an enabled built-in to provide.
    pub requires: Vec<String>,
    pub loaded: Option<Loaded>,
}

impl Guest {
    fn unloaded(id: &str, source: Source) -> Self {
        Guest {
            id: id.to_owned(),
            name: id.to_owned(),
            version: String::new(),
            source,
            granted: Vec::new(),
            requires: Vec::new(),
            loaded: None,
        }
    }
}

async fn read(dir: &Path) -> Result<Package, PluginError> {
    let dir = dir.to_owned();
    tokio::task::spawn_blocking(move || Package::read(&dir))
        .await
        .map_err(|error| PluginError::io(IoOp::Read, error))?
        .map_err(PluginError::from)
}

impl Plugins {
    /// Derives the grant of the package in `dir` without compiling or running
    /// any of it.
    pub async fn inspect(&self, dir: &Path) -> Result<Inspection, PluginError> {
        self.inspection(&read(dir).await?).await
    }

    /// Installs the package in `dir` if it still derives `approved`, then starts
    /// it. Only developer mode installs from a directory, which no signature
    /// vouches for. A plugin whose `init` fails stays installed, `failed`.
    pub async fn install(
        &self,
        dir: &Path,
        approved: &str,
        developer: bool,
    ) -> Result<PluginInfo, PluginError> {
        if !developer {
            return Err(PluginError::DeveloperMode);
        }
        let package = read(dir).await?;
        self.install_package(package, approved, Source::Dev).await
    }

    pub(super) async fn install_package(
        &self,
        package: Package,
        approved: &str,
        source: Source,
    ) -> Result<PluginInfo, PluginError> {
        let _changing = self.changes.lock().await;
        let inspection = self.inspection(&package).await?;
        let id = inspection.id.clone();
        if inspection.approval_digest != approved {
            return Err(PluginError::Changed(id));
        }
        self.require_provided(&inspection.grant.requires)?;
        if self.slots().iter().any(|slot| slot.id() == id) || self.store.dir(&id).exists() {
            return Err(PluginError::Exists(id));
        }
        let sandbox = self.sandbox()?.clone();
        let record = Approval {
            digest: inspection.approval_digest.clone(),
            grant: inspection.grant.clone(),
            source: source.clone(),
            revoked: None,
        };
        let store = self.store.clone();
        let (package, saved) = tokio::task::spawn_blocking(move || {
            let saved = store.save(&package, &record);
            (package, saved)
        })
        .await
        .map_err(|error| PluginError::io(IoOp::Write, error))?;
        saved?;
        let guest = match self
            .link(&sandbox, package, &inspection.grant, source)
            .await
        {
            Ok(guest) => guest,
            Err(error) => {
                if let Err(cleanup) = self.store.remove(&id) {
                    tracing::warn!(plugin = id, error = %cleanup, "cannot undo a failed install");
                }
                sandbox.forget(&id);
                return Err(error);
            }
        };
        let guest = self.add(guest, PluginState::Failed);
        if let Err(error) = self.start_guest(&guest).await {
            tracing::warn!(plugin = id, %error, "installed plugin did not start");
        }
        let slots = self.slots();
        Ok(info(&slots[find(&slots, &id)?]))
    }

    /// Loads every installed package, one at a time so a client's change waits
    /// for at most one `init`. One that cannot be read or no longer derives
    /// what was approved is listed `failed` (or `disabled`, if the user turned
    /// it off) and never runs.
    pub async fn restore(&self) {
        let disabled: HashSet<String> = schema::load::<Saved>(&self.path)
            .map(|saved| saved.disabled.into_iter().collect())
            .unwrap_or_default();
        let store = self.store.clone();
        let ids = tokio::task::spawn_blocking(move || store.ids())
            .await
            .unwrap_or_default();
        for id in ids {
            let _changing = self.changes.lock().await;
            if self.slots().iter().any(|slot| slot.id() == id) {
                tracing::warn!(plugin = id, "an installed plugin shares a built-in's id");
                continue;
            }
            let wanted = !disabled.contains(&id);
            match self.load_guest(&id).await {
                Ok(guest) if wanted => {
                    let unprovided = self.require_provided(&guest.requires);
                    let guest = self.add(guest, PluginState::Failed);
                    if let Err(error) = unprovided {
                        tracing::warn!(plugin = id, %error, "installed plugin did not start");
                        continue;
                    }
                    if let Err(error) = self.start_guest(&guest).await {
                        tracing::warn!(plugin = id, %error, "installed plugin did not start");
                    }
                }
                Ok(guest) => {
                    self.add(guest, PluginState::Disabled);
                }
                Err(error) => {
                    tracing::warn!(plugin = id, %error, "installed plugin is not loaded");
                    let state = match wanted {
                        true => PluginState::Failed,
                        false => PluginState::Disabled,
                    };
                    let source = self.store.record(&id).unwrap_or_default().source;
                    self.add(Guest::unloaded(&id, source), state);
                }
            }
        }
    }

    /// Puts `package` in place of the installed version, the caller holding
    /// `changes`. One that fails to link or to start is rolled back: the
    /// previous files return and the previous version keeps its place and state.
    pub(super) async fn replace(
        &self,
        package: Package,
        inspection: &Inspection,
        source: Source,
    ) -> Result<PluginInfo, PluginError> {
        let id = inspection.id.as_str();
        let sandbox = self.sandbox()?.clone();
        let (old, was) = {
            let slots = self.slots();
            let slot = &slots[find(&slots, id)?];
            match &slot.kind {
                Kind::Guest(guest) => (guest.clone(), slot.state),
                Kind::Builtin(_) => return Err(PluginError::Builtin(id.to_owned())),
            }
        };
        let record = Approval {
            digest: inspection.approval_digest.clone(),
            grant: inspection.grant.clone(),
            source: source.clone(),
            revoked: None,
        };
        let store = self.store.clone();
        let (package, swapped) = tokio::task::spawn_blocking(move || {
            let swapped = store.swap(&package, &record);
            (package, swapped)
        })
        .await
        .map_err(|error| PluginError::io(IoOp::Write, error))?;
        swapped?;
        let rollback = || {
            if let Err(error) = self.store.rollback(id) {
                tracing::error!(plugin = id, %error, "cannot restore the previous version");
            }
            sandbox.forget(id);
        };
        let guest = match self
            .link(&sandbox, package, &inspection.grant, source)
            .await
        {
            Ok(guest) => Arc::new(guest),
            Err(error) => {
                tracing::warn!(plugin = id, %error, "updated plugin did not load; rolled back");
                rollback();
                return Err(error);
            }
        };
        self.place(id, guest.clone(), None)?;
        if was != PluginState::Disabled {
            if let Err(error) = self.start_guest(&guest).await {
                tracing::warn!(plugin = id, %error, "updated plugin did not start; rolled back");
                rollback();
                self.place(id, old, Some(was))?;
                return Err(error);
            }
        }
        self.store.commit(id);
        tracing::info!(plugin = id, version = guest.version, "plugin updated");
        let slots = self.slots();
        Ok(info(&slots[find(&slots, id)?]))
    }

    /// Replaces what the slot of `id` runs, and its state when one is given.
    fn place(
        &self,
        id: &str,
        guest: Arc<Guest>,
        state: Option<PluginState>,
    ) -> Result<(), PluginError> {
        let mut slots = self.slots();
        let index = find(&slots, id)?;
        slots[index].kind = Kind::Guest(guest);
        if let Some(state) = state {
            slots[index].state = state;
            self.save(&slots)?;
        }
        Ok(())
    }

    /// Deletes an installed plugin and its compiled cache entry. A built-in
    /// cannot be removed.
    pub async fn remove(&self, id: &str) -> Result<(), PluginError> {
        let _changing = self.changes.lock().await;
        {
            let slots = self.slots();
            if matches!(slots[find(&slots, id)?].kind, Kind::Builtin(_)) {
                return Err(PluginError::Builtin(id.to_owned()));
            }
        }
        let (store, owned) = (self.store.clone(), id.to_owned());
        tokio::task::spawn_blocking(move || store.remove(&owned))
            .await
            .map_err(|error| PluginError::io(IoOp::Remove, error))??;
        if let Some(sandbox) = &self.sandbox {
            sandbox.forget(id);
        }
        let mut slots = self.slots();
        let index = find(&slots, id)?;
        slots.remove(index);
        self.save(&slots)?;
        tracing::info!(plugin = id, "plugin removed");
        Ok(())
    }

    /// Stores a secret the user entered for a WASM plugin that asks for the
    /// `secrets` permission. The value never reaches a log.
    pub fn store_secret(&self, id: &str, key: &str, value: &str) -> Result<(), PluginError> {
        let asks = {
            let slots = self.slots();
            match &slots[find(&slots, id)?].kind {
                Kind::Builtin(_) => return Err(PluginError::Builtin(id.to_owned())),
                Kind::Guest(guest) => guest.granted.iter().any(|p| p == SECRETS),
            }
        };
        if !asks {
            return Err(PluginError::Secret(format!(
                "plugin '{id}' does not ask for the {SECRETS} permission"
            )));
        }
        self.sandbox()?
            .store_secret(id, key, value)
            .map_err(|error| PluginError::Secret(error.to_string()))?;
        tracing::info!(plugin = id, key, "secret stored");
        Ok(())
    }

    fn sandbox(&self) -> Result<&Arc<Sandbox>, PluginError> {
        self.sandbox.as_ref().ok_or(PluginError::NoSandbox)
    }

    /// The grant of `package` with its programs pinned, read off the async
    /// executor.
    pub(super) async fn inspection(&self, package: &Package) -> Result<Inspection, PluginError> {
        let manifest = &package.manifest;
        let mut grant =
            grant::derive(&self.table, manifest, &package.shape).map_err(PackageError::from)?;
        let reserved = self.reserved.clone();
        let grant =
            tokio::task::spawn_blocking(move || grant::pin(&mut grant, &reserved).map(|()| grant))
                .await
                .map_err(|error| PluginError::io(IoOp::Read, error))?
                .map_err(PackageError::from)?;
        Ok(Inspection {
            id: manifest.id.clone(),
            name: manifest.name.trim().to_owned(),
            version: manifest.version.clone(),
            approval_digest: grant::approval_digest(
                &manifest.id,
                &package.manifest_bytes,
                &package.wasm,
                &grant,
            ),
            grant,
        })
    }

    fn require_provided(&self, requires: &[String]) -> Result<(), PluginError> {
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
    async fn load_guest(&self, id: &str) -> Result<Guest, PluginError> {
        let sandbox = self.sandbox()?.clone();
        let package = read(&self.store.dir(id)).await?;
        if package.manifest.id != id {
            return Err(PackageError::Renamed(package.manifest.id).into());
        }
        let inspection = self.inspection(&package).await?;
        let record = self.store.record(id).unwrap_or_default();
        if record.digest != inspection.approval_digest {
            return Err(PluginError::Changed(id.to_owned()));
        }
        if let Some(reason) = record.revoked {
            return Err(revoked(id, &package.manifest.version, &reason));
        }
        self.link(&sandbox, package, &inspection.grant, record.source)
            .await
    }

    /// Compiles the very bytes the digest covered, each ungranted import
    /// linked to the denied stub.
    async fn link(
        &self,
        sandbox: &Sandbox,
        package: Package,
        grant: &Grant,
        source: Source,
    ) -> Result<Guest, PluginError> {
        let granted: Vec<String> = grant
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
            .link(&opened, &denials, grant)
            .map_err(PackageError::from)?;
        Ok(Guest {
            id: manifest.id,
            name: manifest.name.trim().to_owned(),
            version: manifest.version,
            source,
            granted,
            requires: grant.requires.clone(),
            loaded: Some(loaded),
        })
    }

    fn add(&self, guest: Guest, state: PluginState) -> Arc<Guest> {
        let guest = Arc::new(guest);
        self.slots().push(Slot {
            kind: Kind::Guest(guest.clone()),
            state,
        });
        guest
    }

    /// Runs `init` with `changes` held. Success enables the plugin; a failure
    /// leaves it `failed`, and one that reaches the trap limit is disabled and
    /// stays so after a restart.
    pub(super) async fn start_guest(&self, guest: &Guest) -> Result<(), PluginError> {
        if let Some(reason) = self
            .store
            .record(&guest.id)
            .and_then(|record| record.revoked)
        {
            return Err(revoked(&guest.id, &guest.version, &reason));
        }
        let outcome = match (&guest.loaded, self.sandbox()) {
            (Some(loaded), Ok(sandbox)) => sandbox.init(loaded).await,
            _ => {
                return Err(PluginError::Failed {
                    plugin: guest.id.clone(),
                    source: anyhow::anyhow!("its package is not loaded"),
                })
            }
        };
        let mut slots = self.slots();
        let index = find(&slots, &guest.id)?;
        slots[index].state = match &outcome {
            Ok(()) => PluginState::Enabled,
            Err(SandboxError::TrapLimit(_)) => PluginState::Disabled,
            Err(_) => PluginState::Failed,
        };
        let saved = self.save(&slots);
        match outcome {
            Ok(()) => {
                saved?;
                tracing::info!(plugin = guest.id, "plugin enabled");
                Ok(())
            }
            Err(source) => Err(PluginError::Failed {
                plugin: guest.id.clone(),
                source: source.into(),
            }),
        }
    }
}

fn revoked(plugin: &str, version: &str, reason: &str) -> PluginError {
    TrustError::Revoked {
        plugin: plugin.to_owned(),
        version: version.to_owned(),
        reason: match reason.is_empty() {
            true => String::new(),
            false => format!(": {reason}"),
        },
    }
    .into()
}
