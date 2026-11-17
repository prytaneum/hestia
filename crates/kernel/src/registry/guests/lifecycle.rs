//! What happens to an installed guest over time: restore at start, update, developer mode, removal, `init`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use proto::error::IoOp;
use proto::plugin::{EntryScope, Inspection, PluginInfo, PluginState};

use super::super::dependencies::dependency_order;
use super::super::{disabled_ids, find, info, Kind, Plugins};
use super::*;
use crate::error::{PackageError, PluginError};
use crate::grant;
use crate::install::{Approval, Package, Source};
use crate::runtime::{Sandbox, SandboxError};

impl Plugins {
    /// Loads every installed package, one at a time so a client's change waits
    /// for at most one `init`. One that cannot be read or no longer derives
    /// what was approved is listed `failed` (or `disabled`, if the user turned
    /// it off) and never runs.
    pub async fn restore(&self) {
        let disabled = match disabled_ids(&self.path) {
            Ok(disabled) => disabled,
            Err(error) => {
                tracing::error!(%error, "installed plugins not loaded");
                return;
            }
        };
        let store = self.store.clone();
        let ids = tokio::task::spawn_blocking(move || {
            // Rolls back an update that never finished, which `recover` reads.
            store.ids();
            store.recover();
            let needs = |id: &str| -> Vec<String> {
                store
                    .record(id)
                    .map(|record| {
                        record
                            .grant
                            .dependencies
                            .into_iter()
                            .map(|d| d.id)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            dependency_order(store.ids(), needs)
        })
        .await
        .unwrap_or_default();
        for id in ids {
            let _changing = self.changes.lock().await;
            if self.slots().iter().any(|slot| slot.id() == id) {
                tracing::warn!(plugin = id, "an installed plugin shares a built-in's id");
                continue;
            }
            let wanted = !disabled.contains(&id);
            match self.load_guest(&id, None).await {
                Ok(guest) if wanted => {
                    let guest = self.add(guest, PluginState::Failed);
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
                    let record = self.store.record(&id).unwrap_or_default();
                    self.add(Guest::unloaded(&id, record), state);
                }
            }
        }
    }

    /// Puts `package` in place of the installed version, the caller holding
    /// `changes`. One that fails to link or to start is rolled back: the
    /// previous files return and the previous version keeps its place and state.
    pub(in crate::registry) async fn replace(
        &self,
        package: Package,
        inspection: &Inspection,
        own: &str,
        source: Source,
        scope: Option<EntryScope>,
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
        let approved = self.store.record(id).unwrap_or_default();
        let choice = grant::carry(&approved.grant, &inspection.grant, &approved.choice);
        let record = Approval {
            digest: own.to_owned(),
            grant: inspection.grant.clone(),
            choice: choice.clone(),
            source: source.clone(),
            revoked: None,
        };
        let (store, live, chosen) = (self.store.clone(), old.entries.clone(), scope.clone());
        let (package, swapped) = tokio::task::spawn_blocking(move || {
            let swapped = store.swap(&package, || {
                let mut record = record;
                record.choice.entries = chosen
                    .map(EntryScope::normalised)
                    .unwrap_or_else(|| live.get());
                record
            });
            (package, swapped)
        })
        .await
        .map_err(|error| PluginError::io(IoOp::Write, error))?;
        swapped?;
        let guest = match self
            .link(
                &sandbox,
                package,
                &inspection.grant,
                source,
                &choice,
                old.entries.clone(),
            )
            .await
        {
            Ok(guest) => Arc::new(guest),
            Err(error) => {
                tracing::warn!(plugin = id, %error, "updated plugin did not load; rolled back");
                self.roll_back(&sandbox, id).await;
                return Err(error);
            }
        };
        self.place(id, guest.clone(), None)?;
        if was != PluginState::Disabled {
            if let Err(error) = self.start_guest(&guest).await {
                tracing::warn!(plugin = id, %error, "updated plugin did not start; rolled back");
                self.roll_back(&sandbox, id).await;
                self.place(id, old, Some(was))?;
                return Err(error);
            }
        }
        if let Some(scope) = scope {
            guest.entries.set(scope);
        }
        self.store.commit(id);
        tracing::info!(plugin = id, version = guest.version, "plugin updated");
        let slots = self.slots();
        Ok(info(&slots[find(&slots, id)?]))
    }

    /// Brings back the version an update set aside and drops the compiled one.
    async fn roll_back(&self, sandbox: &Sandbox, id: &str) {
        let (store, owned) = (self.store.clone(), id.to_owned());
        match tokio::task::spawn_blocking(move || store.rollback(&owned)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::error!(plugin = id, %error, "cannot restore the previous version")
            }
            Err(error) => {
                tracing::error!(plugin = id, %error, "cannot restore the previous version")
            }
        }
        sandbox.forget(id);
    }

    /// Whether a plugin installed from a directory may run.
    pub(in crate::registry) fn developer(&self) -> bool {
        self.developer.load(Ordering::Relaxed)
    }

    /// Developer mode changed. Turning it off fails every running dev install
    /// at once; turning it on starts the ones it stopped. A plugin the user
    /// disabled stays disabled either way. Returns the ids whose state changed.
    pub async fn set_developer(&self, on: bool) -> Vec<String> {
        let _changing = self.changes.lock().await;
        self.developer.store(on, Ordering::Relaxed);
        let devs: Vec<(Arc<Guest>, PluginState)> = self
            .slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest) if guest.source == Source::Dev => {
                    Some((guest.clone(), slot.state))
                }
                _ => None,
            })
            .collect();
        tracing::info!(
            developer = on,
            plugins = devs.len(),
            "developer mode changed"
        );
        let mut changed = Vec::new();
        for (guest, state) in devs {
            match (on, state) {
                (false, PluginState::Enabled) => {
                    let mut slots = self.slots();
                    if let Ok(index) = find(&slots, &guest.id) {
                        slots[index].state = PluginState::Failed;
                    }
                    self.running.halt(&guest.id);
                }
                (true, PluginState::Failed) if guest.loaded.is_some() => {
                    if let Err(error) = self.start_guest(&guest).await {
                        tracing::warn!(plugin = guest.id, %error, "plugin did not start");
                    }
                }
                _ => {}
            }
            let slots = self.slots();
            if find(&slots, &guest.id).is_ok_and(|index| slots[index].state != state) {
                changed.push(guest.id.clone());
            }
        }
        changed
    }

    /// Replaces what the slot of `id` runs, and its state when one is given.
    pub(in crate::registry) fn place(
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
        self.remove_installed(id).await
    }

    /// [`Plugins::remove`], the caller holding `changes`. Refused while an
    /// installed plugin declares it as a dependency.
    pub(in crate::registry) async fn remove_installed(&self, id: &str) -> Result<(), PluginError> {
        {
            let slots = self.slots();
            if matches!(slots[find(&slots, id)?].kind, Kind::Builtin(_)) {
                return Err(PluginError::Builtin(id.to_owned()));
            }
            if self.running.any(id) {
                return Err(PluginError::Busy(id.to_owned()));
            }
        }
        let dependents = self.dependents_of(id);
        if !dependents.is_empty() {
            return Err(PluginError::Required {
                plugin: id.to_owned(),
                dependents,
            });
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
    pub async fn store_secret(&self, id: &str, key: &str, value: &str) -> Result<(), PluginError> {
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
            .await
            .map_err(|error| PluginError::Secret(error.to_string()))?;
        tracing::info!(plugin = id, key, "secret stored");
        Ok(())
    }

    /// Runs `init` with `changes` held. Success enables the plugin; a failure
    /// leaves it `failed`, and one that reaches the trap limit is disabled and
    /// stays so after a restart.
    pub(in crate::registry) async fn start_guest(&self, guest: &Guest) -> Result<(), PluginError> {
        if guest.source == Source::Dev && !self.developer() {
            return Err(PluginError::DeveloperMode);
        }
        if let Some(reason) = self
            .store
            .record(&guest.id)
            .and_then(|record| record.revoked)
        {
            return Err(revoked(&guest.id, &guest.version, &reason));
        }
        if let Err(error) = self.dependencies_ready(guest) {
            let mut slots = self.slots();
            let index = find(&slots, &guest.id)?;
            slots[index].state = match error {
                PluginError::MissingDependency { .. }
                | PluginError::Package(PackageError::Unprovided(_)) => {
                    PluginState::MissingDependency
                }
                _ => PluginState::Failed,
            };
            return Err(error);
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
