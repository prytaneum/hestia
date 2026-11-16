//! Plugins that come from a repository: adding one, installing from it, checking
//! it for updates and applying what its signed index revokes. Everything it
//! learns comes from an index that verified under the repository's pinned key.

use proto::plugin::{
    Inspection, PluginInfo, PluginState, PluginUpdate, PluginUpdatesResult, RepositoryFailure,
    RepositoryInfo, UpdateInspection,
};

use super::{find, Kind, Plugins};
use crate::error::PluginError;
use crate::grant;
use crate::install::{Package, Source};
use crate::trust::{is_newer, pinned, Fetched, Release, Repository, TrustError};

fn repository_info(repository: &Repository, signer: &str) -> RepositoryInfo {
    RepositoryInfo {
        id: repository.id.clone(),
        url: repository.url.clone(),
        signer: signer.to_owned(),
    }
}

impl Plugins {
    pub fn repositories(&self) -> Vec<RepositoryInfo> {
        self.repositories
            .list()
            .iter()
            .map(|repository| {
                let signer = repository.signer(&self.official).map(|s| s.fingerprint());
                repository_info(repository, &signer.unwrap_or_default())
            })
            .collect()
    }

    /// Pins `key` for `id` once an index at `url` verifies under it. A key that
    /// stands for Hestia is refused: only Hestia's own repository may use it.
    pub async fn add_repository(
        &self,
        id: &str,
        url: &str,
        key: &str,
    ) -> Result<RepositoryInfo, PluginError> {
        if !crate::id::is_valid(id) {
            return Err(PluginError::UnknownRepository(id.to_owned()));
        }
        let signer = pinned(key, &self.official)?;
        let repository = Repository {
            id: id.to_owned(),
            url: url.to_owned(),
            key: signer.key().to_owned(),
            sequence: 0,
        };
        let fetched = Fetched::index(&repository, &signer).await?;
        let _changing = self.changes.lock().await;
        if self.repositories.get(id).is_some() {
            return Err(PluginError::RepositoryExists(id.to_owned()));
        }
        let repository = Repository {
            sequence: fetched.index.sequence,
            ..repository
        };
        self.repositories
            .put(repository.clone())
            .map_err(PluginError::Save)?;
        tracing::info!(
            repository = id,
            signer = signer.fingerprint(),
            "repository added"
        );
        Ok(repository_info(&repository, &signer.fingerprint()))
    }

    pub async fn remove_repository(&self, id: &str) -> Result<(), PluginError> {
        let _changing = self.changes.lock().await;
        match self.repositories.remove(id).map_err(PluginError::Save)? {
            true => Ok(()),
            false => Err(PluginError::UnknownRepository(id.to_owned())),
        }
    }

    pub async fn inspect_release(
        &self,
        repository: &str,
        id: &str,
        version: Option<&str>,
    ) -> Result<Inspection, PluginError> {
        let (package, _) = self.release(repository, id, version).await?;
        self.inspection(&package).await
    }

    /// Installs a release of a repository if it still derives `approved`.
    pub async fn install_release(
        &self,
        repository: &str,
        id: &str,
        version: Option<&str>,
        approved: &str,
    ) -> Result<PluginInfo, PluginError> {
        let (package, source) = self.release(repository, id, version).await?;
        self.install_package(package, approved, source).await
    }

    /// Fetches every repository's index, which disables an installed version it
    /// revokes, and lists what is newer than what is installed.
    pub async fn check_updates(&self) -> PluginUpdatesResult {
        let mut result = PluginUpdatesResult::default();
        for repository in self.repositories.list() {
            let fetched = match self.fetch(&repository).await {
                Ok((fetched, _)) => fetched,
                Err(error) => {
                    tracing::warn!(repository = repository.id, %error, "repository not checked");
                    result.failed.push(RepositoryFailure {
                        repository: repository.id,
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            for (id, installed) in self.installed_from(&repository.id) {
                let newer = fetched
                    .index
                    .latest(&id)
                    .filter(|release| is_newer(&release.version, &installed));
                if let Some(release) = newer {
                    result.updates.push(PluginUpdate {
                        id,
                        repository: repository.id.clone(),
                        installed,
                        available: release.version.clone(),
                    });
                }
            }
        }
        result
    }

    /// What updating `id` would install and whether it asks for more than the
    /// user approved.
    pub async fn inspect_update(&self, id: &str) -> Result<UpdateInspection, PluginError> {
        let update = self.prepare_update(id).await?;
        Ok(UpdateInspection {
            installed: update.installed,
            inspection: update.inspection,
            widened: update.widened,
        })
    }

    /// Replaces `id` with its update if it still derives `approved`. A widened
    /// update is only ever applied this way, never on its own.
    pub async fn update(&self, id: &str, approved: &str) -> Result<PluginInfo, PluginError> {
        let update = self.prepare_update(id).await?;
        if update.inspection.approval_digest != approved {
            return Err(PluginError::Changed(id.to_owned()));
        }
        let _changing = self.changes.lock().await;
        self.replace(update.package, &update.inspection, update.source)
            .await
    }

    async fn prepare_update(&self, id: &str) -> Result<Update, PluginError> {
        let (repository, signer, installed) = {
            let slots = self.slots();
            match &slots[find(&slots, id)?].kind {
                Kind::Guest(guest) => match &guest.source {
                    Source::Repository { repository, signer } => {
                        (repository.clone(), signer.clone(), guest.version.clone())
                    }
                    Source::Dev => return Err(PluginError::NotFromRepository(id.to_owned())),
                },
                Kind::Builtin(_) => return Err(PluginError::NotFromRepository(id.to_owned())),
            }
        };
        let pinned = self.repository(&repository)?;
        let (fetched, current) = self.fetch(&pinned).await?;
        if current != signer {
            return Err(TrustError::Signer(repository).into());
        }
        let release = fetched
            .index
            .latest(id)
            .filter(|release| is_newer(&release.version, &installed))
            .ok_or_else(|| PluginError::UpToDate(id.to_owned()))?;
        let package = download(&fetched, release).await?;
        let inspection = self.inspection(&package).await?;
        let approved = self.store.record(id).unwrap_or_default().grant;
        Ok(Update {
            installed,
            widened: grant::widens(&approved, &inspection.grant),
            package,
            inspection,
            source: Source::Repository {
                repository,
                signer: current,
            },
        })
    }

    fn repository(&self, id: &str) -> Result<Repository, PluginError> {
        self.repositories
            .get(id)
            .ok_or_else(|| PluginError::UnknownRepository(id.to_owned()))
    }

    async fn release(
        &self,
        repository: &str,
        id: &str,
        version: Option<&str>,
    ) -> Result<(Package, Source), PluginError> {
        let (fetched, signer) = self.fetch(&self.repository(repository)?).await?;
        let release = match version {
            Some(version) => fetched
                .index
                .release(id, version)
                .filter(|release| fetched.index.revocation(id, &release.version).is_none()),
            None => fetched.index.latest(id),
        }
        .ok_or_else(|| PluginError::NoRelease {
            repository: repository.to_owned(),
            id: id.to_owned(),
        })?;
        let source = Source::Repository {
            repository: repository.to_owned(),
            signer,
        };
        Ok((download(&fetched, release).await?, source))
    }

    /// The index of `repository`, verified under its pinned key and no older
    /// than the last one seen, with the key's fingerprint. It records the sequence and disables what the
    /// index revokes among the plugins installed from this repository.
    async fn fetch(&self, repository: &Repository) -> Result<(Fetched, String), PluginError> {
        let signer = repository.signer(&self.official)?;
        let fetched = Fetched::index(repository, &signer).await?;
        let _changing = self.changes.lock().await;
        if let Some(mut known) = self.repositories.get(&repository.id) {
            if fetched.index.sequence > known.sequence {
                known.sequence = fetched.index.sequence;
                self.repositories.put(known).map_err(PluginError::Save)?;
            }
        }
        self.revoke_listed(&repository.id, &fetched);
        Ok((fetched, signer.fingerprint()))
    }

    fn installed_from(&self, repository: &str) -> Vec<(String, String)> {
        self.slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest) => match &guest.source {
                    Source::Repository {
                        repository: from, ..
                    } if from == repository => Some((guest.id.clone(), guest.version.clone())),
                    _ => None,
                },
                Kind::Builtin(_) => None,
            })
            .collect()
    }

    /// Marks each installed version the index revokes and stops it if it runs.
    /// The mark persists, so it stays off after a restart and cannot be enabled.
    fn revoke_listed(&self, repository: &str, fetched: &Fetched) {
        for (id, version) in self.installed_from(repository) {
            let Some(revocation) = fetched.index.revocation(&id, &version) else {
                continue;
            };
            tracing::warn!(
                plugin = id,
                version,
                repository,
                "installed version revoked"
            );
            if let Err(error) = self.store.revoke(&id, &revocation.reason) {
                tracing::error!(plugin = id, %error, "cannot record the revocation");
                continue;
            }
            let mut slots = self.slots();
            if let Ok(index) = find(&slots, &id) {
                if slots[index].state == PluginState::Enabled {
                    slots[index].state = PluginState::Failed;
                }
            }
        }
    }
}

struct Update {
    installed: String,
    widened: bool,
    package: Package,
    inspection: Inspection,
    source: Source,
}

/// A package of a verified index: both files held to its digests, and named
/// what the index says they are, so a listed version cannot carry another.
async fn download(fetched: &Fetched, release: &Release) -> Result<Package, PluginError> {
    let manifest = fetched.manifest(&release.manifest).await?;
    let component = fetched.component(&release.component).await?;
    let package = tokio::task::spawn_blocking(move || Package::new(manifest, component))
        .await
        .map_err(|error| PluginError::io(proto::error::IoOp::Read, error))??;
    if package.manifest.id != release.id || package.manifest.version != release.version {
        return Err(TrustError::Malformed(format!(
            "the package says it is '{}' {}, the index lists '{}' {}",
            package.manifest.id, package.manifest.version, release.id, release.version
        ))
        .into());
    }
    Ok(package)
}
