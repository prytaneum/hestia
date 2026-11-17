//! The WASM half of the registry: reading a package, installing it, and
//! loading what is installed. Every change to a guest runs under
//! `Plugins::changes`, so installs, enables, disables and removals never
//! interleave.

use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use proto::error::IoOp;
use proto::plugin::{
    Choice, Dependency, EntryScope, Grant, Inspection, PluginInfo, PluginState, SignerInfo,
};

use super::dependencies::{dependency_order, Member, Plan};
use super::{find, info, schema, Kind, Plugins, Saved, Slot};
use crate::commands::{CommandDecl, ScheduleDecl};
use crate::entries::Entries;
use crate::error::{PackageError, PluginError};
use crate::grant;
use crate::install::{Approval, Package, Source};
use crate::runtime::{Loaded, Sandbox, SandboxError};
use crate::trust::{Signer, TrustError};

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
    /// The ids of the built-ins that provide `requires`.
    providers: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub commands: Vec<CommandDecl>,
    pub schedules: Vec<ScheduleDecl>,
    /// The optional permissions the user granted.
    pub optional: Vec<String>,
    /// Whether it holds a permission that takes the entries scope.
    pub scoped: bool,
    pub entries: Entries,
    pub loaded: Option<Loaded>,
    #[cfg(feature = "probe")]
    pub inbox: super::events::Inbox,
}

impl Guest {
    /// The ids of every plugin it needs: those it declares and the built-ins
    /// behind the packages it imports.
    pub fn needs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.dependencies.iter().map(|d| d.id.clone()).collect();
        for provider in &self.providers {
            if !ids.contains(provider) {
                ids.push(provider.clone());
            }
        }
        ids
    }

    fn unloaded(id: &str, record: Approval) -> Self {
        Guest {
            id: id.to_owned(),
            name: id.to_owned(),
            version: String::new(),
            source: record.source,
            granted: Vec::new(),
            requires: Vec::new(),
            providers: Vec::new(),
            dependencies: record.grant.dependencies,
            commands: Vec::new(),
            schedules: Vec::new(),
            optional: record.choice.optional,
            scoped: false,
            entries: Entries::new(record.choice.entries),
            loaded: None,
            #[cfg(feature = "probe")]
            inbox: Default::default(),
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
        Ok(self.plan(&read(dir).await?, &Source::Dev).await?.inspection)
    }

    /// Installs the package in `dir` if it still derives `approved`, then starts
    /// it. Only developer mode installs from a directory, which no signature
    /// vouches for. A plugin whose `init` fails stays installed, `failed`.
    pub async fn install(
        &self,
        dir: &Path,
        approved: &str,
        choice: Choice,
    ) -> Result<PluginInfo, PluginError> {
        if !self.developer() {
            return Err(PluginError::DeveloperMode);
        }
        let package = read(dir).await?;
        self.install_package(package, approved, Source::Dev, choice)
            .await
    }

    pub(super) async fn install_package(
        &self,
        package: Package,
        approved: &str,
        source: Source,
        choice: Choice,
    ) -> Result<PluginInfo, PluginError> {
        let plan = self.plan(&package, &source).await?;
        let _changing = self.changes.lock().await;
        if plan.inspection.approval_digest != approved {
            return Err(PluginError::Changed(plan.inspection.id.clone()));
        }
        self.still_holds(&plan, false)?;
        grant::check_choice(&plan.inspection.grant, &choice).map_err(PackageError::from)?;
        let Plan {
            inspection,
            own,
            members,
        } = plan;
        self.with_members(
            members,
            (&inspection.id, &own),
            self.install_one(package, &inspection, &own, source, choice),
        )
        .await
    }

    /// Installs what a plan brings, then runs `top`, the caller holding
    /// `changes`. If either fails, everything installed for this call is removed
    /// again with its data. The plan is on disk meanwhile, so a crash is undone
    /// at the next start.
    pub(super) async fn with_members<T>(
        &self,
        members: Vec<Member>,
        (plugin, digest): (&str, &str),
        top: impl Future<Output = Result<T, PluginError>>,
    ) -> Result<T, PluginError> {
        if members.is_empty() {
            return top.await;
        }
        self.store.begin(
            plugin,
            digest,
            members.iter().map(|m| m.inspection.id.clone()).collect(),
        )?;
        let mut installed = Vec::new();
        let outcome = match self.install_members(members, &mut installed).await {
            Ok(()) => top.await,
            Err(error) => Err(error),
        };
        match &outcome {
            Ok(_) => self.store.finish(),
            Err(_) => {
                if self.undo(&installed).await {
                    self.store.finish();
                }
            }
        }
        outcome
    }

    /// Installs what a plan brings, in order, the caller holding `changes`,
    /// noting each id in `installed` as it starts. One that fails, or does not
    /// start, is an error.
    async fn install_members(
        &self,
        members: Vec<Member>,
        installed: &mut Vec<String>,
    ) -> Result<(), PluginError> {
        for member in members {
            let id = member.inspection.id.clone();
            self.ensure_absent(&id)?;
            installed.push(id.clone());
            let info = self
                .install_one(
                    member.package,
                    &member.inspection,
                    &member.inspection.approval_digest,
                    member.source,
                    Choice::default(),
                )
                .await?;
            if info.state != PluginState::Enabled {
                return Err(PluginError::Failed {
                    plugin: id,
                    source: anyhow::anyhow!("it did not start"),
                });
            }
        }
        Ok(())
    }

    /// Refuses an id that is installed or a built-in's, so a plan never undoes
    /// a plugin it did not create.
    fn ensure_absent(&self, id: &str) -> Result<(), PluginError> {
        match self.slots().iter().any(|slot| slot.id() == id) || self.store.dir(id).exists() {
            true => Err(PluginError::Exists(id.to_owned())),
            false => Ok(()),
        }
    }

    /// Installs `package` and starts it, the caller holding `changes`.
    /// `own` is the digest it is stored under.
    async fn install_one(
        &self,
        package: Package,
        inspection: &Inspection,
        own: &str,
        source: Source,
        choice: Choice,
    ) -> Result<PluginInfo, PluginError> {
        let id = inspection.id.clone();
        self.ensure_absent(&id)?;
        let sandbox = self.sandbox()?.clone();
        let record = Approval {
            digest: own.to_owned(),
            grant: inspection.grant.clone(),
            choice: choice.clone(),
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
            .link(
                &sandbox,
                package,
                &inspection.grant,
                source,
                &choice,
                Entries::new(choice.entries.clone()),
            )
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
        self.start_schedules(&guest, common::now_unix().max(0) as u64);
        if let Err(error) = self.start_guest(&guest).await {
            tracing::warn!(plugin = id, %error, "installed plugin did not start");
        }
        let slots = self.slots();
        Ok(info(&slots[find(&slots, &id)?]))
    }

    /// Removes what an install brought before it failed, last first, with its
    /// data, the caller holding `changes`. Whether all of it is gone.
    async fn undo(&self, installed: &[String]) -> bool {
        let mut clean = true;
        for id in installed.iter().rev() {
            let undone = match self.remove_installed(id).await {
                Ok(()) => self.store.discard(id),
                Err(PluginError::NotFound(_)) => self.store.discard(id),
                Err(error) => Err(error),
            };
            if let Err(error) = undone {
                tracing::warn!(plugin = id, %error, "cannot undo a dependency of a failed install");
                clean = false;
            }
        }
        clean
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
    pub(super) async fn replace(
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
    pub(super) fn developer(&self) -> bool {
        self.developer.load(Ordering::Relaxed)
    }

    /// Developer mode changed. Turning it off fails every running dev install
    /// at once; turning it on starts the ones it stopped. A plugin the user
    /// disabled stays disabled either way. Returns the ids whose state changed.
    pub async fn set_developer(&self, on: bool) -> Vec<String> {
        let _changing = self.changes.lock().await;
        self.developer.store(on, Ordering::Relaxed);
        if let Some(sandbox) = &self.sandbox {
            sandbox.set_developer(on);
        }
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
    pub(super) fn place(
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
    pub(super) async fn remove_installed(&self, id: &str) -> Result<(), PluginError> {
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

    pub(super) fn sandbox(&self) -> Result<&Arc<Sandbox>, PluginError> {
        self.sandbox.as_ref().ok_or(PluginError::NoSandbox)
    }

    /// The grant of `package` with its programs pinned, read off the async
    /// executor.
    pub(super) async fn inspection(
        &self,
        package: &Package,
        source: &Source,
    ) -> Result<Inspection, PluginError> {
        let manifest = &package.manifest;
        let warnings = grant::versions::check(
            &self.table,
            manifest,
            &package.shape,
            *source == Source::Dev,
        )
        .map_err(PackageError::from)?;
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
                source.signer(),
            ),
            signer: self.signer_info(source),
            warnings,
            grant,
            plan: Vec::new(),
        })
    }

    /// Who vouches for a package of `source`, as the prompt shows it.
    fn signer_info(&self, source: &Source) -> Option<SignerInfo> {
        let (repository, author) = source.signer()?;
        let author = Signer::parse(author).ok()?;
        Some(SignerInfo {
            repository: repository.to_owned(),
            fingerprint: author.fingerprint(),
            official: self.official.iter().any(|own| own.same_key(&author)),
        })
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

    pub(super) fn require_provided(&self, requires: &[String]) -> Result<(), PluginError> {
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
    pub(super) async fn load_guest(
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
    async fn link(
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

    pub(super) fn add(&self, guest: Guest, state: PluginState) -> Arc<Guest> {
        let guest = Arc::new(guest);
        self.slots().push(Slot {
            kind: Kind::Guest(guest.clone()),
            state,
        });
        guest
    }

    fn dependencies_ready(&self, guest: &Guest) -> Result<(), PluginError> {
        self.require_provided(&guest.requires)?;
        guest
            .dependencies
            .iter()
            .try_for_each(|dependency| self.dependency_ready(&guest.id, dependency))
    }

    /// Runs `init` with `changes` held. Success enables the plugin; a failure
    /// leaves it `failed`, and one that reaches the trap limit is disabled and
    /// stays so after a restart.
    pub(super) async fn start_guest(&self, guest: &Guest) -> Result<(), PluginError> {
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
