//! Plugins that come from a repository: adding one, installing from it, checking
//! it for updates and applying what its signed index revokes. The index says
//! which key each plugin's author signs with and the digest of each release;
//! the release itself is a `.hplugin` that author signed.

use proto::error::IoOp;
use proto::plugin::{
    Choice, EntryScope, Inspection, PluginInfo, PluginRevoked, PluginState, PluginUpdate,
    PluginUpdatesResult, RepositoryFailure, RepositoryInfo, UpdateInspection,
};

use super::dependencies::Plan;
use super::{find, Kind, Plugins};
use crate::error::PluginError;
use crate::grant;
use crate::install::{Package, Source};
use crate::trust::{
    is_newer, pinned, same_key, Fetched, Hplugin, Listing, Release, Repository, Signer, TrustError,
};

fn repository_info(repository: &Repository, signer: &str) -> RepositoryInfo {
    RepositoryInfo {
        id: repository.id.clone(),
        url: repository.url.clone(),
        signer: signer.to_owned(),
    }
}

/// Runs file work off the async executor.
async fn blocking<T: Send + 'static>(
    operation: IoOp,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, PluginError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| PluginError::io(operation, error))
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
            return Err(PluginError::InvalidId(id.to_owned()));
        }
        let signer = pinned(key, &self.official)?;
        let repository = Repository {
            id: id.to_owned(),
            url: url.to_owned(),
            key: signer.key().to_owned(),
            sequence: 0,
        };
        let fetched = Fetched::index(&repository, &signer, self.policy_for).await?;
        let _changing = self.changes.lock().await;
        if self.repositories.get(id).is_some() {
            return Err(PluginError::RepositoryExists(id.to_owned()));
        }
        let repository = Repository {
            sequence: fetched.index.sequence,
            ..repository
        };
        let (repositories, added) = (self.repositories.clone(), repository.clone());
        blocking(IoOp::Write, move || repositories.put(added))
            .await?
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
        let (repositories, owned) = (self.repositories.clone(), id.to_owned());
        match blocking(IoOp::Write, move || repositories.remove(&owned))
            .await?
            .map_err(PluginError::Save)?
        {
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
        let (package, source) = self.release(repository, id, version).await?;
        Ok(self.plan(&package, &source).await?.inspection)
    }

    /// Installs a release of a repository if it still derives `approved`.
    pub async fn install_release(
        &self,
        repository: &str,
        id: &str,
        version: Option<&str>,
        approved: &str,
        choice: Choice,
    ) -> Result<PluginInfo, PluginError> {
        let (package, source) = self.release(repository, id, version).await?;
        self.install_package(package, approved, source, choice)
            .await
    }

    /// Fetches every repository's index, together, which disables an installed
    /// version it revokes, and lists what is newer than what is installed.
    pub async fn check_updates(&self) -> PluginUpdatesResult {
        let mut fetches = Vec::new();
        for repository in self.repositories.list() {
            let (asked, policy_for) = (repository.clone(), self.policy_for);
            let signer = repository.signer(&self.official);
            let fetch =
                tokio::spawn(async move { Fetched::index(&asked, &signer?, policy_for).await });
            fetches.push((repository, fetch));
        }
        let mut result = PluginUpdatesResult::default();
        for (repository, fetch) in fetches {
            let fetched = match fetch.await {
                Ok(fetched) => fetched.map_err(PluginError::from),
                Err(error) => Err(PluginError::io(IoOp::Read, error)),
            };
            let fetched = match fetched {
                Ok(fetched) => self.accept(&repository, fetched).await,
                Err(error) => Err(error),
            };
            let (fetched, revoked) = match fetched {
                Ok(accepted) => accepted,
                Err(error) => {
                    tracing::warn!(repository = repository.id, %error, "repository not checked");
                    result.failed.push(RepositoryFailure {
                        repository: repository.id,
                        detail: error.to_string(),
                    });
                    continue;
                }
            };
            result.revoked.extend(revoked);
            for (id, installed) in self.installed_from(&repository) {
                let newer = fetched
                    .index
                    .latest(&id)
                    .map(|(_, release)| release)
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
            inspection: update.plan.inspection,
            widened: update.widened,
        })
    }

    /// Replaces `id` with its update if it still derives `approved`. A widened
    /// update is only ever applied this way, never on its own. What it newly
    /// needs is installed first, and removed again if the update fails.
    pub async fn update(
        &self,
        id: &str,
        approved: &str,
        scope: Option<EntryScope>,
    ) -> Result<PluginInfo, PluginError> {
        let update = self.prepare_update(id).await?;
        if update.plan.inspection.approval_digest != approved {
            return Err(PluginError::Changed(id.to_owned()));
        }
        let _changing = self.changes.lock().await;
        self.still_holds(&update.plan, true)?;
        let Plan {
            inspection,
            own,
            members,
        } = update.plan;
        self.with_members(
            members,
            (&inspection.id, &own),
            self.replace(update.package, &inspection, &own, update.source, scope),
        )
        .await
    }

    /// The newest release of `id`, signed by the author who signed the
    /// installed version and listed by the repository that vouched for it.
    async fn prepare_update(&self, id: &str) -> Result<Update, PluginError> {
        let (installed, source) = {
            let slots = self.slots();
            match &slots[find(&slots, id)?].kind {
                Kind::Guest(guest) if guest.source != Source::Dev => {
                    (guest.version.clone(), guest.source.clone())
                }
                _ => return Err(PluginError::NotFromRepository(id.to_owned())),
            }
        };
        let Source::Repository {
            repository: from,
            author,
            ..
        } = &source
        else {
            return Err(PluginError::NotFromRepository(id.to_owned()));
        };
        let pinned = self.repository(from)?;
        if !source.vouched_by(&pinned) {
            return Err(TrustError::RepositoryKey(from.clone()).into());
        }
        let (fetched, _) = self.fetch(&pinned).await?;
        let (listing, release) = fetched
            .index
            .latest(id)
            .filter(|(_, release)| is_newer(&release.version, &installed))
            .ok_or_else(|| PluginError::UpToDate(id.to_owned()))?;
        if !same_key(&listing.author, author) {
            return Err(TrustError::AuthorKey(id.to_owned()).into());
        }
        let package = download(&fetched, listing, release, &Signer::parse(author)?).await?;
        let plan = self.plan(&package, &source).await?;
        self.accepted(id, &plan.inspection.version)?;
        let approved = self.store.record(id).unwrap_or_default().grant;
        Ok(Update {
            installed,
            widened: grant::widens(&approved, &plan.inspection.grant) || !plan.members.is_empty(),
            package,
            plan,
            source,
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
        let pinned = self.repository(repository)?;
        let (fetched, _) = self.fetch(&pinned).await?;
        let (listing, release) = match version {
            Some(version) => fetched.index.release(id, version),
            None => fetched.index.latest(id),
        }
        .ok_or_else(|| PluginError::NoRelease {
            repository: repository.to_owned(),
            id: id.to_owned(),
        })?;
        let author = Signer::parse(&listing.author)?;
        let package = download(&fetched, listing, release, &author).await?;
        let source = Source::Repository {
            repository: repository.to_owned(),
            repository_key: pinned.key,
            author: author.key().to_owned(),
        };
        Ok((package, source))
    }

    /// The index of `repository`, verified under its pinned key and no older
    /// than the last one seen. It records the sequence and disables what the
    /// index revokes among the plugins installed from this repository.
    pub(super) async fn fetch(
        &self,
        repository: &Repository,
    ) -> Result<(Fetched, Vec<PluginRevoked>), PluginError> {
        let signer = repository.signer(&self.official)?;
        let fetched = Fetched::index(repository, &signer, self.policy_for).await?;
        self.accept(repository, fetched).await
    }

    /// Takes in an index that verified under `repository`'s key, unless the
    /// repository was removed or pinned to another key while it was fetched.
    async fn accept(
        &self,
        repository: &Repository,
        fetched: Fetched,
    ) -> Result<(Fetched, Vec<PluginRevoked>), PluginError> {
        let _changing = self.changes.lock().await;
        let known = self.repository(&repository.id)?;
        if !same_key(&known.key, &repository.key) {
            return Err(TrustError::RepositoryKey(repository.id.clone()).into());
        }
        if fetched.index.sequence > known.sequence {
            let repositories = self.repositories.clone();
            let seen = Repository {
                sequence: fetched.index.sequence,
                ..known
            };
            blocking(IoOp::Write, move || repositories.put(seen))
                .await?
                .map_err(PluginError::Save)?;
        }
        let revoked = self.revoke_listed(repository, &fetched).await?;
        Ok((fetched, revoked))
    }

    fn installed_from(&self, repository: &Repository) -> Vec<(String, String)> {
        self.slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest) if guest.source.vouched_by(repository) => {
                    Some((guest.id.clone(), guest.version.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Marks each installed version the index revokes and stops it if it runs.
    /// The mark persists, so it stays off after a restart and cannot be enabled.
    async fn revoke_listed(
        &self,
        repository: &Repository,
        fetched: &Fetched,
    ) -> Result<Vec<PluginRevoked>, PluginError> {
        let listed: Vec<PluginRevoked> = self
            .installed_from(repository)
            .into_iter()
            .filter_map(|(id, version)| {
                let revocation = fetched.index.revocation(&id, &version)?;
                Some(PluginRevoked {
                    id,
                    version,
                    repository: repository.id.clone(),
                    reason: revocation.reason.clone(),
                })
            })
            .collect();
        if listed.is_empty() {
            return Ok(listed);
        }
        let (store, marks) = (self.store.clone(), listed.clone());
        let done = blocking(IoOp::Write, move || {
            marks
                .into_iter()
                .filter(|revoked| match store.amend(&revoked.id, |record| record.revoked = Some(revoked.reason.clone())) {
                    Ok(()) => true,
                    Err(error) => {
                        tracing::error!(plugin = revoked.id, %error, "cannot record the revocation");
                        false
                    }
                })
                .collect::<Vec<_>>()
        })
        .await?;
        let mut slots = self.slots();
        for revoked in &done {
            tracing::warn!(
                plugin = revoked.id,
                version = revoked.version,
                repository = revoked.repository,
                "installed version revoked"
            );
            if let Ok(index) = find(&slots, &revoked.id) {
                if slots[index].state == PluginState::Enabled {
                    slots[index].state = PluginState::Failed;
                }
            }
        }
        Ok(done)
    }
}

struct Update {
    installed: String,
    widened: bool,
    package: Package,
    plan: Plan,
    source: Source,
}

/// A release of a verified index: the `.hplugin` held to the index's digest
/// and signed by `author` as the plugin and version the index lists, whose
/// manifest says the same.
pub(super) async fn download(
    fetched: &Fetched,
    listing: &Listing,
    release: &Release,
    author: &Signer,
) -> Result<Package, PluginError> {
    let bytes = fetched.archive(release).await?;
    let (id, version, author) = (listing.id.clone(), release.version.clone(), author.clone());
    blocking(IoOp::Read, move || {
        let archive = Hplugin::read(&bytes)?;
        archive.verify(&author, &id, &version)?;
        let package = Package::new(archive.manifest, archive.wasm)?;
        if package.manifest.id != id || package.manifest.version != version {
            return Err(PluginError::Trust(TrustError::Malformed(format!(
                "the package says it is '{}' {}, the index lists '{id}' {version}",
                package.manifest.id, package.manifest.version
            ))));
        }
        Ok(package)
    })
    .await?
}
