//! Installing a package from a directory or a plan, and undoing a failed install.

use std::future::Future;
use std::path::Path;

use proto::error::IoOp;
use proto::plugin::{Choice, Inspection, PluginInfo, PluginState, SignerInfo};

use super::super::dependencies::{Member, Plan};
use super::super::{find, info, Kind, Plugins};
use super::*;
use crate::entries::Entries;
use crate::error::{PackageError, PluginError};
use crate::grant;
use crate::install::{Approval, Package, Source};
use crate::trust::Signer;

impl Plugins {
    /// Derives the grant of the package in `dir` without compiling or running
    /// any of it.
    pub async fn inspect(&self, dir: &Path) -> Result<Inspection, PluginError> {
        Ok(self.plan(&read(dir).await?, &Source::Dev).await?.inspection)
    }

    /// Installs the package in `dir` if it still derives `approved`, then starts
    /// it. Only developer mode installs from a directory, which no signature
    /// vouches for. A plugin whose `init` fails stays installed, `failed`.
    ///
    /// A directory whose id is already a dev install replaces it as an update
    /// does: settings, secrets and schedule runs stay, the granted optional
    /// permissions are carried, and `choice` is ignored.
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
        if self.is_dev(&package.manifest.id) {
            return self.reload(package, approved).await;
        }
        self.install_package(package, approved, Source::Dev, choice)
            .await
    }

    fn is_dev(&self, id: &str) -> bool {
        self.slots().iter().any(|slot| {
            slot.id() == id
                && matches!(&slot.kind, Kind::Guest(guest) if guest.source == Source::Dev)
        })
    }

    async fn reload(&self, package: Package, approved: &str) -> Result<PluginInfo, PluginError> {
        let plan = self.plan(&package, &Source::Dev).await?;
        let _changing = self.changes.lock().await;
        if plan.inspection.approval_digest != approved {
            return Err(PluginError::Changed(plan.inspection.id.clone()));
        }
        self.still_holds(&plan, true)?;
        let Plan {
            inspection,
            own,
            members,
        } = plan;
        self.with_members(
            members,
            (&inspection.id, &own),
            self.replace(package, &inspection, &own, Source::Dev, None),
        )
        .await
    }

    pub(in crate::registry) async fn install_package(
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
    pub(in crate::registry) async fn with_members<T>(
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

    /// The grant of `package` with its programs pinned, read off the async
    /// executor.
    pub(in crate::registry) async fn inspection(
        &self,
        package: &Package,
        source: &Source,
    ) -> Result<Inspection, PluginError> {
        let manifest = &package.manifest;
        let warnings = grant::check_versions(
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
}
