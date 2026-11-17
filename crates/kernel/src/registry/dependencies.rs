//! Plugins that need other plugins: resolving what an install would bring,
//! checking that what a plugin needs is there, and the order they start in.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use proto::plugin::{Dependency, Inspection, PluginState};
use semver::VersionReq;

use super::guests::Guest;
use super::releases::download;
use super::{info, Kind, Plugins};
use crate::error::{PackageError, PluginError};
use crate::grant;
use crate::install::{Package, Source};
use crate::trust::{accepts, Fetched, Signer};

/// A chain of dependencies is at most this many plugins long, not counting the
/// one being installed. It bounds the recursion.
const DEPTH_MAX: usize = 8;

/// One prompt brings at most this many plugins besides the one asked for.
const MEMBERS_MAX: usize = 32;

/// What one resolution has seen: the chain being walked, what it brings so far,
/// and each repository's index, fetched once (`None` when it could not be).
#[derive(Default)]
struct Walk {
    path: Vec<String>,
    members: Vec<Member>,
    indexes: HashMap<String, Option<Fetched>>,
}

/// A plugin an install brings because another needs it.
pub(super) struct Member {
    pub package: Package,
    pub source: Source,
    pub inspection: Inspection,
}

/// What approving an install covers: the plugin and, in install order, each
/// plugin it brings. `inspection` carries the plan and the digest of all of it;
/// `own` is the plugin's digest alone, which is what it is stored under.
pub(super) struct Plan {
    pub inspection: Inspection,
    pub own: String,
    pub members: Vec<Member>,
}

type Resolving<'a> = Pin<Box<dyn Future<Output = Result<(), PluginError>> + Send + 'a>>;

impl Plugins {
    /// Reads `package` and resolves everything it declares it needs, before any
    /// of it runs. Fetches indexes, so it never runs under `changes`.
    pub(super) async fn plan(
        &self,
        package: &Package,
        source: &Source,
    ) -> Result<Plan, PluginError> {
        let mut inspection = self.inspection(package, source).await?;
        let own = inspection.approval_digest.clone();
        let mut walk = Walk {
            path: vec![inspection.id.clone()],
            ..Walk::default()
        };
        let dependencies = inspection.grant.dependencies.clone();
        self.resolve(&dependencies, source, &mut walk).await?;
        let members = walk.members;
        let digests: Vec<&str> = members
            .iter()
            .map(|member| member.inspection.approval_digest.as_str())
            .collect();
        inspection.approval_digest = grant::plan_digest(&own, &digests);
        inspection.plan = members
            .iter()
            .map(|member| member.inspection.clone())
            .collect();
        Ok(Plan {
            inspection,
            own,
            members,
        })
    }

    /// Each dependency must be installed and enabled at an accepted version, or
    /// be listed by an added repository, which then joins the plan after what it
    /// needs itself. What an installed dependency needs is walked too, so a
    /// loop through installed plugins is found.
    fn resolve<'a>(
        &'a self,
        dependencies: &'a [Dependency],
        from: &'a Source,
        walk: &'a mut Walk,
    ) -> Resolving<'a> {
        Box::pin(async move {
            let dependent = walk.path.last().cloned().unwrap_or_default();
            for dependency in dependencies {
                if walk.path.contains(&dependency.id) {
                    return Err(PackageError::DependencyCycle(format!(
                        "{} -> {}",
                        walk.path.join(" -> "),
                        dependency.id
                    ))
                    .into());
                }
                if walk.path.len() > DEPTH_MAX {
                    return Err(PackageError::DependencyDepth(DEPTH_MAX).into());
                }
                let missing = || PluginError::MissingDependency {
                    plugin: dependent.clone(),
                    dependency: dependency.id.clone(),
                    range: dependency.range.clone(),
                };
                if let Some(member) = walk
                    .members
                    .iter()
                    .find(|member| member.inspection.id == dependency.id)
                {
                    match accepts(&dependency.range, &member.inspection.version) {
                        true => continue,
                        false => return Err(missing()),
                    }
                }
                if self.installed(&dependency.id) {
                    self.dependency_ready(&dependent, dependency)?;
                    let nested = self.dependencies_of(&dependency.id);
                    walk.path.push(dependency.id.clone());
                    self.resolve(&nested, from, walk).await?;
                    walk.path.pop();
                    continue;
                }
                if walk.members.len() >= MEMBERS_MAX {
                    return Err(PackageError::TooManyDependencies(MEMBERS_MAX).into());
                }
                let (package, source) = self
                    .find_release(dependency, from, &mut walk.indexes)
                    .await?
                    .ok_or_else(missing)?;
                let inspection = self.inspection(&package, &source).await?;
                self.require_provided(&inspection.grant.requires)?;
                let nested = inspection.grant.dependencies.clone();
                walk.path.push(dependency.id.clone());
                self.resolve(&nested, &source, walk).await?;
                walk.path.pop();
                walk.members.push(Member {
                    package,
                    source,
                    inspection,
                });
            }
            Ok(())
        })
    }

    /// The newest release some repository lists for `dependency`, the one that
    /// vouched for the dependent first. A repository that cannot be reached, or
    /// whose release cannot be fetched, is skipped.
    async fn find_release(
        &self,
        dependency: &Dependency,
        from: &Source,
        indexes: &mut HashMap<String, Option<Fetched>>,
    ) -> Result<Option<(Package, Source)>, PluginError> {
        let Ok(range) = VersionReq::parse(&dependency.range) else {
            return Ok(None);
        };
        let mut repositories = self.repositories.list().map_err(PluginError::Load)?;
        if let Source::Repository { repository, .. } = from {
            repositories.sort_by_key(|listed| listed.id != *repository);
        }
        for repository in repositories {
            if !indexes.contains_key(&repository.id) {
                let fetched = match self.fetch(&repository).await {
                    Ok((fetched, _)) => Some(fetched),
                    Err(error) => {
                        tracing::warn!(repository = repository.id, %error, "dependency not looked up");
                        None
                    }
                };
                indexes.insert(repository.id.clone(), fetched);
            }
            let Some(fetched) = indexes[&repository.id].as_ref() else {
                continue;
            };
            let Some((listing, release)) = fetched.index.matching(&dependency.id, &range) else {
                continue;
            };
            let author = Signer::parse(&listing.author)?;
            let package = match download(fetched, listing, release, &author).await {
                Ok(package) => package,
                Err(error) => {
                    tracing::warn!(repository = repository.id, %error, "dependency not downloaded");
                    continue;
                }
            };
            let source = Source::Repository {
                repository: repository.id.clone(),
                repository_key: repository.key.clone(),
                author: author.key().to_owned(),
            };
            return Ok(Some((package, source)));
        }
        Ok(None)
    }

    /// What the installed plugin `id` declares in `[dependencies]`.
    fn dependencies_of(&self, id: &str) -> Vec<Dependency> {
        self.guests()
            .into_iter()
            .find(|guest| guest.id == id)
            .map(|guest| guest.dependencies.clone())
            .unwrap_or_default()
    }

    fn installed(&self, id: &str) -> bool {
        self.slots().iter().any(|slot| slot.id() == id)
    }

    /// Whether `dependency`, which `plugin` needs, is installed at a version its
    /// range accepts and enabled.
    pub(super) fn dependency_ready(
        &self,
        plugin: &str,
        dependency: &Dependency,
    ) -> Result<(), PluginError> {
        let slots = self.slots();
        let held = slots
            .iter()
            .find(|slot| slot.id() == dependency.id)
            .filter(|slot| accepts(&dependency.range, &info(slot).version));
        match held {
            None => Err(PluginError::MissingDependency {
                plugin: plugin.to_owned(),
                dependency: dependency.id.clone(),
                range: dependency.range.clone(),
            }),
            Some(slot) if slot.state != PluginState::Enabled => {
                Err(PluginError::NotEnabled(dependency.id.clone()))
            }
            Some(_) => Ok(()),
        }
    }

    /// Whether what `plan` was resolved against still holds, once `changes` is
    /// held: nothing it brings is installed, and everything else it needs still
    /// is, and enabled. It also holds everything that can still refuse the call,
    /// so nothing is installed for a plan that cannot finish.
    pub(super) fn still_holds(&self, plan: &Plan, updating: bool) -> Result<(), PluginError> {
        let top = &plan.inspection;
        let changed = || PluginError::Changed(top.id.clone());
        let planned: Vec<&str> = plan
            .members
            .iter()
            .map(|member| member.inspection.id.as_str())
            .collect();
        for member in &plan.members {
            let id = &member.inspection.id;
            if self.installed(id) || self.store.dir(id).exists() {
                return Err(changed());
            }
        }
        if !updating && (self.installed(&top.id) || self.store.dir(&top.id).exists()) {
            return Err(PluginError::Exists(top.id.clone()));
        }
        let inspections = plan
            .members
            .iter()
            .map(|member| &member.inspection.grant)
            .chain([&top.grant]);
        for dependency in inspections.flat_map(|grant| &grant.dependencies) {
            if !planned.contains(&dependency.id.as_str()) {
                self.dependency_ready(&top.id, dependency)
                    .map_err(|_| changed())?;
            }
        }
        for grant in plan
            .members
            .iter()
            .map(|member| &member.inspection.grant)
            .chain([&top.grant])
        {
            self.require_provided(&grant.requires)?;
        }
        if updating {
            if self.running.any(&top.id) {
                return Err(PluginError::Busy(top.id.clone()));
            }
            self.accepted(&top.id, &top.version)?;
        }
        Ok(())
    }

    /// Refuses `version` of `id` when an installed plugin's range rejects it.
    pub(super) fn accepted(&self, id: &str, version: &str) -> Result<(), PluginError> {
        let dependents = self.dependents_rejecting(id, version);
        match dependents.is_empty() {
            true => Ok(()),
            false => Err(PluginError::Rejected {
                version: version.to_owned(),
                dependents,
            }),
        }
    }

    /// The installed plugins that declare `id` as a dependency, whatever their
    /// state.
    pub(super) fn dependents_of(&self, id: &str) -> Vec<String> {
        self.guests()
            .into_iter()
            .filter(|guest| guest.dependencies.iter().any(|d| d.id == id))
            .map(|guest| guest.id.clone())
            .collect()
    }

    /// The installed plugins that declare `id` and whose range rejects `version`.
    pub(super) fn dependents_rejecting(&self, id: &str, version: &str) -> Vec<String> {
        self.guests()
            .into_iter()
            .filter(|guest| {
                guest
                    .dependencies
                    .iter()
                    .any(|d| d.id == id && !accepts(&d.range, version))
            })
            .map(|guest| guest.id.clone())
            .collect()
    }

    fn guests(&self) -> Vec<Arc<Guest>> {
        self.slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest) => Some(guest.clone()),
                Kind::Builtin(_) => None,
            })
            .collect()
    }

    /// Every installed plugin `guest` needs, what each needs first.
    pub(super) fn needed_by(&self, guest: &Guest) -> Vec<Arc<Guest>> {
        let installed = self.guests();
        let mut order = Vec::new();
        let mut seen = HashSet::from([guest.id.clone()]);
        visit(&installed, guest, &mut seen, &mut order);
        order
    }
}

fn visit(
    installed: &[Arc<Guest>],
    guest: &Guest,
    seen: &mut HashSet<String>,
    order: &mut Vec<Arc<Guest>>,
) {
    for dependency in &guest.dependencies {
        let Some(needed) = installed.iter().find(|other| other.id == dependency.id) else {
            continue;
        };
        if seen.insert(needed.id.clone()) {
            visit(installed, needed, seen, order);
            order.push(needed.clone());
        }
    }
}

/// `ids` with each one after the ones `needs` names for it, so start-up enables
/// what a plugin needs first. A loop is cut where it is found.
pub(super) fn dependency_order(
    ids: Vec<String>,
    needs: impl Fn(&str) -> Vec<String>,
) -> Vec<String> {
    fn place(
        id: &str,
        ids: &[String],
        needs: &dyn Fn(&str) -> Vec<String>,
        done: &mut Vec<String>,
        visiting: &mut HashSet<String>,
    ) {
        if done.iter().any(|placed| placed == id) || !visiting.insert(id.to_owned()) {
            return;
        }
        for needed in needs(id).iter().filter(|n| ids.contains(n)) {
            place(needed, ids, needs, done, visiting);
        }
        done.push(id.to_owned());
    }
    let mut done = Vec::new();
    let mut visiting = HashSet::new();
    for id in &ids {
        place(id, &ids, &needs, &mut done, &mut visiting);
    }
    done
}
