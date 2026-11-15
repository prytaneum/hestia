//! Whether installed content has a newer compatible version. An answer the
//! update cache holds inside its TTL is used as is; the rest are asked in bulk,
//! one request per source, game version and loader however many entries share
//! them; and only an item the bulk answer says has moved is resolved exactly,
//! so the release-first, never-backwards rule of an update decides the version.

use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::Result;
use proto::content::{
    ContentDone, ContentFailure, ContentUpdate, ContentVersion, EntryUpdates, InstalledContent,
};
use proto::error::{EntryKind, EntryTarget};
use proto::minecraft::{Edition, ProvisionPhase};
use proto::warning::WarningInfo;

use super::super::batch::gather_ordered;
use super::add::failure;
use super::entry::{EntryContent, EntryRef};
use super::phase_progress;
use crate::content::provider::{InstalledFile, LatestQuery};
use crate::content::updates::{Latest, UpdateKey};
use crate::content::{install, modpack};
use crate::engine::Engine;
use crate::minecraft::materialize::OnProgress;
use crate::plugins::CallChain;

/// One installed item asked about under its entry's game version and loaders.
struct Probe {
    game_version: String,
    loaders: Vec<String>,
    item: InstalledContent,
}

impl Probe {
    fn key(&self) -> UpdateKey {
        UpdateKey::of(&self.item, &self.game_version, &self.loaders)
    }
}

/// What a check answered for one entry and kind.
pub struct Checked {
    pub updates: Vec<ContentUpdate>,
    /// Items that gained an update no earlier check had reported.
    pub fresh: u32,
}

/// What a check over every entry found.
pub struct Swept {
    /// Only the entries with at least one update.
    pub entries: Vec<EntryUpdates>,
    pub fresh: u32,
}

impl Engine {
    fn probe(&self, ctx: &EntryContent, item: InstalledContent) -> Probe {
        Probe {
            game_version: ctx.game_version.clone(),
            loaders: self.content_loaders(item.kind, ctx),
            item,
        }
    }

    /// For each platform-sourced item of the kind, the newest compatible
    /// version against the current pin. `refresh` asks upstream even inside the
    /// TTL. An item whose versions cannot be resolved is skipped, not fatal.
    pub async fn check_entry_updates(
        &self,
        entry: EntryRef<'_>,
        kind: proto::content::ContentKind,
        refresh: bool,
    ) -> Result<Checked> {
        let ctx = self.content_ctx(entry)?;
        let probes = platform_items(&ctx)
            .into_iter()
            .filter(|item| item.kind == kind)
            .map(|item| self.probe(&ctx, item))
            .collect();
        let (answers, fresh) = self.latest_versions(probes, refresh).await;
        let updates = answers
            .into_iter()
            .filter_map(|(probe, latest)| match latest {
                Ok(latest) => Some(content_update(probe.item, &latest)),
                Err(e) => {
                    tracing::warn!(title = %probe.item.title, error = %format!("{e:#}"), "update check failed");
                    None
                }
            })
            .collect();
        Ok(Checked { updates, fresh })
    }

    /// Check every platform-sourced item of every ready Java entry in one pass,
    /// so entries on the same game version and loader share their requests.
    pub async fn check_all_updates(&self) -> Swept {
        let entries = self.update_targets();
        let mut probes = Vec::new();
        let mut owners = Vec::new();
        for (index, (_, ctx)) in entries.iter().enumerate() {
            for item in platform_items(ctx) {
                probes.push(self.probe(ctx, item));
                owners.push(index);
            }
        }
        let (answers, fresh) = self.latest_versions(probes, false).await;
        let mut found: Vec<Vec<ContentUpdate>> = vec![Vec::new(); entries.len()];
        for ((probe, latest), owner) in answers.into_iter().zip(owners) {
            if let Ok(latest @ Latest::Newer(_)) = latest {
                found[owner].push(content_update(probe.item, &latest));
            }
        }
        let entries = entries
            .into_iter()
            .zip(found)
            .filter(|(_, updates)| !updates.is_empty())
            .map(|((target, ctx), updates)| EntryUpdates {
                target,
                name: ctx.name,
                updates,
            })
            .collect();
        tracing::info!(fresh, "content update check finished");
        Swept { entries, fresh }
    }

    /// Every entry's updates as earlier checks left them — no request made, and
    /// an answer past its TTL still listed.
    pub fn known_updates(&self) -> Vec<EntryUpdates> {
        self.update_targets()
            .into_iter()
            .filter_map(|(target, ctx)| {
                let updates: Vec<ContentUpdate> = platform_items(&ctx)
                    .into_iter()
                    .filter_map(|item| {
                        let probe = self.probe(&ctx, item);
                        match self.content_updates.known(&probe.key())? {
                            latest @ Latest::Newer(_) => Some(content_update(probe.item, &latest)),
                            Latest::Current => None,
                        }
                    })
                    .collect();
                (!updates.is_empty()).then_some(EntryUpdates {
                    target,
                    name: ctx.name,
                    updates,
                })
            })
            .collect()
    }

    /// How many items auto-update would move in the entry, from what earlier
    /// checks found — no request made.
    pub fn auto_updatable(&self, entry: EntryRef<'_>) -> usize {
        let Ok(ctx) = self.content_ctx(entry) else {
            return 0;
        };
        auto_items(&ctx)
            .into_iter()
            .filter(|item| {
                let probe = self.probe(&ctx, item.clone());
                matches!(
                    self.content_updates.known(&probe.key()),
                    Some(Latest::Newer(_))
                )
            })
            .count()
    }

    /// Move every item auto-update may touch to its newest compatible version:
    /// platform-sourced, not held, and not installed by a modpack, whose own
    /// version decides its mods.
    pub async fn auto_update_entry(
        &self,
        entry: EntryRef<'_>,
        on_progress: OnProgress<'_>,
    ) -> Result<ContentDone> {
        let ctx = self.content_ctx(entry)?;
        let items = auto_items(&ctx);
        if items.is_empty() {
            return Ok(ContentDone::default());
        }
        on_progress.report(&phase_progress(ProvisionPhase::Resolving));
        let (planned, failures) = self.newer_versions(&ctx, items).await;
        on_progress.check()?;
        if planned.is_empty() && failures.is_empty() {
            return Ok(ContentDone::default());
        }
        tracing::info!(entry = %ctx.name, updates = planned.len(), "auto-updating content");
        self.install_updates(
            &ctx,
            planned,
            failures,
            false,
            &CallChain::default(),
            on_progress,
        )
        .await
    }

    /// Auto-update an instance as it launches. Items that could not move ride
    /// on the launch as a warning — the game still starts on what it has —
    /// and only a cancel stops the launch.
    pub(in crate::engine::flows) async fn auto_update_for_launch(
        &self,
        instance: &str,
        on_progress: OnProgress<'_>,
    ) -> Result<Vec<WarningInfo>> {
        match self
            .auto_update_entry(EntryRef::Instance(instance), on_progress)
            .await
        {
            Ok(done) => {
                let mut warnings = done.warnings;
                if !done.failures.is_empty() {
                    warnings.push(WarningInfo::ContentNotUpdated {
                        count: done.failures.len() as u32,
                        files: done.failures.into_iter().map(|f| f.title).collect(),
                    });
                }
                Ok(warnings)
            }
            Err(e) if crate::is_cancelled(&e) => Err(e),
            Err(e) => {
                tracing::warn!(instance, error = %format!("{e:#}"), "auto-update failed; launching as installed");
                Ok(Vec::new())
            }
        }
    }

    /// The items of `items` with a newer version, paired with it, and a
    /// failure for each one that could not be resolved.
    pub(super) async fn newer_versions(
        &self,
        ctx: &EntryContent,
        items: Vec<InstalledContent>,
    ) -> (Vec<(InstalledContent, ContentVersion)>, Vec<ContentFailure>) {
        let probes = items
            .into_iter()
            .map(|item| self.probe(ctx, item))
            .collect();
        let (answers, _) = self.latest_versions(probes, false).await;
        let mut planned = Vec::new();
        let mut failures = Vec::new();
        for (probe, latest) in answers {
            match latest {
                Ok(Latest::Newer(version)) => planned.push((probe.item, *version)),
                Ok(Latest::Current) => {}
                Err(e) => failures.push(failure(
                    &probe.item.filename,
                    &probe.item.title,
                    crate::error_info(e),
                )),
            }
        }
        (planned, failures)
    }

    /// Each probe's newest compatible version, in the order given, and how many
    /// gained an update no earlier answer reported. Bulk answers that the
    /// installed version is current settle there; one naming another version is
    /// resolved exactly; a source without a bulk lookup resolves every item.
    async fn latest_versions(
        &self,
        probes: Vec<Probe>,
        refresh: bool,
    ) -> (Vec<(Probe, Result<Latest>)>, u32) {
        let now = Instant::now();
        let mut answers: Vec<Option<Result<Latest>>> = probes.iter().map(|_| None).collect();
        let mut groups: BTreeMap<(&str, &str, &[String]), Vec<usize>> = BTreeMap::new();
        for (index, probe) in probes.iter().enumerate() {
            let cached = (!refresh)
                .then(|| self.content_updates.fresh(&probe.key(), now))
                .flatten();
            match cached {
                Some(latest) => answers[index] = Some(Ok(latest)),
                None => groups
                    .entry((
                        probe.item.source.as_str(),
                        probe.game_version.as_str(),
                        probe.loaders.as_slice(),
                    ))
                    .or_default()
                    .push(index),
            }
        }

        let mut fresh = 0u32;
        let mut settle =
            |index: usize, latest: Latest, answers: &mut Vec<Option<Result<Latest>>>| {
                if self
                    .content_updates
                    .record(probes[index].key(), latest.clone(), now)
                {
                    fresh += 1;
                }
                answers[index] = Some(Ok(latest));
            };

        let mut exact = Vec::new();
        for ((source, game_version, loaders), members) in groups {
            let files: Vec<InstalledFile> = members
                .iter()
                .map(|&i| InstalledFile {
                    project_id: probes[i].item.project_id.clone(),
                    sha1: probes[i].item.sha1.clone(),
                })
                .collect();
            let query = LatestQuery {
                game_version: game_version.to_string(),
                loaders: loaders.to_vec(),
                files,
            };
            match self.content.latest(source, &query).await {
                Ok(Some(newest)) => {
                    for index in members {
                        let item = &probes[index].item;
                        match newest.get(&item.project_id) {
                            Some(id) if *id != item.version_id => exact.push(index),
                            _ => settle(index, Latest::Current, &mut answers),
                        }
                    }
                }
                Ok(None) => exact.extend(members),
                Err(e) => {
                    tracing::warn!(source, error = %format!("{e:#}"), "bulk update lookup failed; resolving one by one");
                    exact.extend(members);
                }
            }
        }

        let resolved = gather_ordered(
            exact,
            |_| None,
            |index| {
                let probe = &probes[index];
                async move {
                    let version = self
                        .version_for(
                            &probe.game_version,
                            &probe.loaders,
                            &probe.item.source,
                            &probe.item.project_id,
                            &probe.item.version_id,
                            "",
                        )
                        .await;
                    (index, version)
                }
            },
        )
        .await;
        for (index, version) in resolved {
            match version {
                Ok(version) if version.id == probes[index].item.version_id => {
                    settle(index, Latest::Current, &mut answers)
                }
                Ok(version) => settle(index, Latest::Newer(Box::new(version)), &mut answers),
                Err(e) => answers[index] = Some(Err(e)),
            }
        }

        let answers = probes
            .into_iter()
            .zip(answers)
            .map(|(probe, answer)| (probe, answer.expect("every probe is answered")))
            .collect();
        (answers, fresh)
    }

    /// Every ready Java entry, as the target a client names it by and the
    /// content view a check works against.
    fn update_targets(&self) -> Vec<(EntryTarget, EntryContent)> {
        let servers = self
            .servers
            .list()
            .into_iter()
            .map(|record| (EntryKind::Server, record.id));
        let instances = self
            .instances
            .list()
            .into_iter()
            .map(|record| (EntryKind::Instance, record.id));
        servers
            .chain(instances)
            .filter_map(|(kind, id)| {
                let entry = match kind {
                    EntryKind::Server => EntryRef::Server(&id),
                    EntryKind::Instance => EntryRef::Instance(&id),
                };
                let ctx = self.content_ctx(entry).ok()?;
                (ctx.edition == Edition::Java).then(|| (EntryTarget::new(kind, id.clone()), ctx))
            })
            .collect()
    }
}

fn platform_items(ctx: &EntryContent) -> Vec<InstalledContent> {
    install::load(&ctx.entry_dir)
        .into_iter()
        .filter(|item| !item.project_id.is_empty())
        .collect()
}

fn auto_items(ctx: &EntryContent) -> Vec<InstalledContent> {
    platform_items(ctx)
        .into_iter()
        .filter(|item| !item.held && !modpack::is_pack_origin(&item.origin))
        .collect()
}

fn content_update(item: InstalledContent, latest: &Latest) -> ContentUpdate {
    let (latest_id, latest_number) = match latest {
        Latest::Newer(version) => (version.id.clone(), version.version_number.clone()),
        Latest::Current => (item.version_id.clone(), item.version_number.clone()),
    };
    ContentUpdate {
        kind: item.kind,
        title: item.title,
        held: item.held,
        updatable: matches!(latest, Latest::Newer(_)),
        filename: item.filename,
        project_id: item.project_id,
        current_version_id: item.version_id,
        current_version_number: item.version_number,
        latest_version_id: latest_id,
        latest_version_number: latest_number,
    }
}
