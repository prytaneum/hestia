//! What happens to content already in the pool: listing it, moving it to
//! another version, checking whether one exists, toggling it, and removing it.

use std::path::Path;

use anyhow::{bail, Context, Result};
use proto::content::{ContentKind, InstalledContent, UntrackedFile, VersionQuery, LOCAL_SOURCE};
use proto::minecraft::ProvisionPhase;

use super::entry::{content_loader, EntryContent};
use super::phase_progress;
use crate::content::{install, record, Identified, LocalFile};
use crate::engine::Engine;
use crate::minecraft::materialize::OnProgress;

impl Engine {
    /// Move matched platform items to a newer version — the newest compatible
    /// when `pin` is empty, or that exact version (id or number) when pinned.
    /// `references` empty takes every item of the kind; naming several takes
    /// exactly those, resolved before the first one moves.
    pub(super) async fn update_content(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
        references: &[String],
        pin: &str,
        on_progress: OnProgress<'_>,
    ) -> Result<Vec<InstalledContent>> {
        let index = install::load(&ctx.entry_dir);
        let targets = update_targets(&index, kind, references)?;
        if targets.is_empty() {
            bail!("nothing is installed");
        }
        let loader = content_loader(kind, &ctx.flavor);

        let mut updated = Vec::new();
        for item in targets {
            if item.project_id.is_empty() {
                continue;
            }
            on_progress.report(&phase_progress(ProvisionPhase::Resolving));
            let versions = self
                .content
                .versions(&VersionQuery {
                    source: item.source.clone(),
                    project: item.project_id.clone(),
                    loader: loader.clone(),
                    game_version: Some(ctx.game_version.clone()),
                })
                .await?;
            let version = match pin.is_empty() {
                true => install::pick_update(
                    &versions,
                    &ctx.game_version,
                    loader.as_deref(),
                    &item.version_id,
                ),
                false => {
                    install::pick_version(&versions, &ctx.game_version, loader.as_deref(), pin)
                }
            }
            .with_context(|| format!("cannot update '{}'", item.title))?
            .clone();
            if version.id == item.version_id {
                continue;
            }
            let release = self
                .fetch_release(ctx, item.kind, &item.title, &version, on_progress)
                .await?;
            let new_item = record::repin(&item, release);
            if new_item.filename != item.filename {
                install::remove_files(&ctx.entry_dir, &ctx.data_dir, &item, &ctx.worlds());
            }
            install::apply_files(&ctx.entry_dir, &ctx.data_dir, &new_item, &ctx.worlds())?;
            tracing::info!(
                title = %item.title,
                from = %item.version_number,
                to = %new_item.version_number,
                "content updated"
            );
            updated.push(new_item);
        }

        if !updated.is_empty() {
            let mut index = install::load(&ctx.entry_dir);
            for new_item in &updated {
                match index
                    .iter_mut()
                    .find(|i| i.kind == new_item.kind && i.project_id == new_item.project_id)
                {
                    Some(entry) => *entry = new_item.clone(),
                    None => index.push(new_item.clone()),
                }
            }
            install::save(&ctx.entry_dir, index)?;
        }
        Ok(updated)
    }

    pub(super) async fn content_matches(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
        reference: &str,
    ) -> Result<Vec<Identified>> {
        let item = installed_item(&install::load(&ctx.entry_dir), kind, reference)?;
        self.identified(ctx, &item).await
    }

    async fn identified(
        &self,
        ctx: &EntryContent,
        item: &InstalledContent,
    ) -> Result<Vec<Identified>> {
        let file = LocalFile::read(&install::managed_path(&ctx.entry_dir, item)?)?;
        Ok(self
            .content
            .identify(std::slice::from_ref(&file))
            .await
            .remove(&file.sha1)
            .unwrap_or_default())
    }

    /// For a pool that arrived without provenance, never one the user chose to keep local.
    pub(in crate::engine::flows) async fn recognise_pool(&self, entry_dir: &Path) -> Result<usize> {
        let mut index = install::load(entry_dir);
        let local = |item: &InstalledContent| item.source == LOCAL_SOURCE && !item.sha1.is_empty();
        let files: Vec<LocalFile> = index
            .iter()
            .filter(|item| local(item))
            .filter_map(|item| {
                Some(LocalFile {
                    path: install::managed_path(entry_dir, item).ok()?,
                    sha1: item.sha1.clone(),
                })
            })
            .collect();
        let found = self.content.identify(&files).await;
        let mut recognised = 0;
        for item in index.iter_mut().filter(|item| local(item)) {
            let Some(known) = found.get(&item.sha1).and_then(|matches| matches.first()) else {
                continue;
            };
            let (project, release) =
                known.recorded(item.kind, item.filename.clone(), item.sha1.clone());
            *item = record::resource(item, project, release);
            recognised += 1;
        }
        if recognised > 0 {
            install::save(entry_dir, index)?;
        }
        Ok(recognised)
    }

    /// Sources share no project ids, so the installed file's hash finds it there.
    pub(super) async fn move_content_source(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
        reference: &str,
        source: &str,
        pin: &str,
        on_progress: OnProgress<'_>,
    ) -> Result<Vec<InstalledContent>> {
        let item = installed_item(&install::load(&ctx.entry_dir), kind, reference)?;
        if item.source == source {
            return self
                .update_content(ctx, kind, &[reference.to_string()], pin, on_progress)
                .await;
        }
        on_progress.report(&phase_progress(ProvisionPhase::Resolving));
        let known = self
            .identified(ctx, &item)
            .await?
            .into_iter()
            .find(|m| m.version.source == source)
            .with_context(|| format!("'{}' is not published on {source}", item.title))?;
        let loader = content_loader(kind, &ctx.flavor);
        let versions = self
            .content
            .versions(&VersionQuery {
                source: source.to_string(),
                project: known.project.id.clone(),
                loader: loader.clone(),
                game_version: Some(ctx.game_version.clone()),
            })
            .await?;
        let version = install::pick_version(&versions, &ctx.game_version, loader.as_deref(), pin)
            .with_context(|| format!("cannot move '{}' to {source}", item.title))?
            .clone();
        let release = self
            .fetch_release(ctx, item.kind, &known.project.title, &version, on_progress)
            .await?;
        let moved = record::resource(
            &item,
            record::Project {
                kind: item.kind,
                project_id: known.project.id.clone(),
                slug: known.project.slug.clone(),
                title: known.project.title.clone(),
                icon_url: known.project.icon_url.clone(),
            },
            release,
        );
        if moved.filename != item.filename {
            install::remove_files(&ctx.entry_dir, &ctx.data_dir, &item, &ctx.worlds());
        }
        install::apply_files(&ctx.entry_dir, &ctx.data_dir, &moved, &ctx.worlds())?;
        tracing::info!(
            title = %moved.title,
            from = %item.source,
            to = %moved.source,
            version = %moved.version_number,
            "content source changed"
        );

        let mut index = install::load(&ctx.entry_dir);
        match index
            .iter_mut()
            .find(|i| i.kind == item.kind && i.filename == item.filename)
        {
            Some(entry) => *entry = moved.clone(),
            None => index.push(moved.clone()),
        }
        install::save(&ctx.entry_dir, index)?;
        Ok(vec![moved])
    }

    pub(super) async fn content_updates(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
    ) -> Result<Vec<proto::content::ContentUpdate>> {
        let items: Vec<InstalledContent> = install::load(&ctx.entry_dir)
            .into_iter()
            .filter(|i| i.kind == kind && !i.project_id.is_empty())
            .collect();
        let loader = content_loader(kind, &ctx.flavor);
        let mut out = Vec::new();
        for item in items {
            let versions = match self
                .content
                .versions(&VersionQuery {
                    source: item.source.clone(),
                    project: item.project_id.clone(),
                    loader: loader.clone(),
                    game_version: Some(ctx.game_version.clone()),
                })
                .await
            {
                Ok(versions) => versions,
                Err(e) => {
                    tracing::warn!(title = %item.title, error = %format!("{e:#}"), "update check failed");
                    continue;
                }
            };
            let Ok(latest) = install::pick_update(
                &versions,
                &ctx.game_version,
                loader.as_deref(),
                &item.version_id,
            ) else {
                continue;
            };
            out.push(proto::content::ContentUpdate {
                filename: item.filename,
                project_id: item.project_id,
                current_version_id: item.version_id.clone(),
                current_version_number: item.version_number.clone(),
                latest_version_id: latest.id.clone(),
                latest_version_number: latest.version_number.clone(),
                updatable: latest.id != item.version_id,
            });
        }
        Ok(out)
    }
}

fn installed_item(
    index: &[InstalledContent],
    kind: ContentKind,
    reference: &str,
) -> Result<InstalledContent> {
    index
        .iter()
        .find(|i| i.kind == kind && install::matches(i, reference))
        .cloned()
        .ok_or_else(|| {
            proto::error::ErrorInfo::ContentNotFound {
                reference: reference.to_string(),
            }
            .into()
        })
}

/// The items an update acts on: every one of the kind for an empty batch, or
/// exactly the named ones — each resolved before the first one moves, so a bad
/// reference changes nothing.
fn update_targets(
    index: &[InstalledContent],
    kind: ContentKind,
    references: &[String],
) -> Result<Vec<InstalledContent>> {
    if references.is_empty() {
        return Ok(index.iter().filter(|i| i.kind == kind).cloned().collect());
    }
    super::resolve_all(index, kind, references)?;
    let mut targets: Vec<InstalledContent> = Vec::new();
    for reference in references {
        let matched = index
            .iter()
            .filter(|i| i.kind == kind && install::matches(i, reference));
        for item in matched {
            if item.project_id.is_empty() {
                bail!(
                    "'{}' was installed from a {} and cannot be updated",
                    item.filename,
                    item.source
                );
            }
            if !targets.iter().any(|t| t.filename == item.filename) {
                targets.push(item.clone());
            }
        }
    }
    Ok(targets)
}

pub(super) fn list_content(
    ctx: &EntryContent,
    kind: ContentKind,
) -> (Vec<InstalledContent>, Vec<UntrackedFile>) {
    let items: Vec<InstalledContent> = install::load(&ctx.entry_dir)
        .into_iter()
        .filter(|i| i.kind == kind)
        .collect();
    let untracked = install::untracked(&ctx.data_dir, kind, &items, &ctx.worlds());
    (items, untracked)
}

/// Uninstall every item matching one of `references`: the managed copy and
/// every mirror of it. A non-empty `worlds` (datapacks only) instead drops
/// those worlds from each item's selection, uninstalling it only when none is
/// left.
pub(super) fn remove_content(
    ctx: &EntryContent,
    kind: ContentKind,
    references: &[String],
    worlds: &[String],
) -> Result<Vec<InstalledContent>> {
    if !worlds.is_empty() && kind != ContentKind::DataPack {
        bail!(proto::error::ErrorInfo::UnsupportedOperation {
            reason: proto::error::Unsupported::DatapacksPerWorld
        });
    }
    let entry_worlds = ctx.worlds();
    let (matched, mut kept): (Vec<_>, Vec<_>) =
        install::load(&ctx.entry_dir).into_iter().partition(|i| {
            i.kind == kind
                && references
                    .iter()
                    .any(|reference| install::matches(i, reference))
        });
    if matched.is_empty() {
        return Ok(matched);
    }
    let mut removed = Vec::new();
    for mut item in matched {
        let remaining = keep_worlds(&item, worlds, &entry_worlds);
        if !remaining.is_empty() {
            item.worlds = remaining;
            install::apply_files(&ctx.entry_dir, &ctx.data_dir, &item, &entry_worlds)?;
            tracing::info!(
                entry = %ctx.entry_dir.display(),
                title = %item.title,
                worlds = ?item.worlds,
                "datapack narrowed to fewer worlds"
            );
            kept.push(item);
            continue;
        }
        install::remove_files(&ctx.entry_dir, &ctx.data_dir, &item, &entry_worlds);
        tracing::info!(
            entry = %ctx.entry_dir.display(),
            kind = ?item.kind,
            title = %item.title,
            filename = %item.filename,
            "content removed"
        );
        removed.push(item);
    }
    install::save(&ctx.entry_dir, kept)?;
    Ok(removed)
}

/// The worlds an item still loads in once `drop` are taken from it; empty means
/// nothing is left, so the item goes. An "every world" selection materialises to
/// the entry's current worlds first.
fn keep_worlds(item: &InstalledContent, drop: &[String], entry_worlds: &[String]) -> Vec<String> {
    if drop.is_empty() {
        return Vec::new();
    }
    let current = match item.worlds.is_empty() {
        true => entry_worlds,
        false => &item.worlds,
    };
    current
        .iter()
        .filter(|world| !drop.iter().any(|named| install::world_name(world) == named))
        .cloned()
        .collect()
}

/// Toggle every index entry matching `reference`, applying the filesystem side
/// immediately. With no `worlds` the item itself flips; with worlds (datapacks
/// only) just those are scoped in or out, leaving the rest as they are. Returns
/// how many entries matched (regardless of whether anything moved), so the
/// caller can distinguish "nothing matched" from "already in that state".
pub(super) fn set_enabled(
    ctx: &EntryContent,
    kind: ContentKind,
    reference: &str,
    enabled: bool,
    scope: &[String],
) -> Result<usize> {
    if !scope.is_empty() && kind != ContentKind::DataPack {
        bail!(proto::error::ErrorInfo::UnsupportedOperation {
            reason: proto::error::Unsupported::DatapacksPerWorld
        });
    }
    let entry_worlds = ctx.worlds();
    let mut index = install::load(&ctx.entry_dir);
    let mut matched = 0usize;
    for item in index.iter_mut() {
        if item.kind != kind || !install::matches(item, reference) {
            continue;
        }
        matched += 1;
        scope_enabled(item, enabled, scope, &entry_worlds);
        install::apply_files(&ctx.entry_dir, &ctx.data_dir, item, &entry_worlds)?;
        tracing::info!(
            entry = %ctx.entry_dir.display(),
            title = %item.title,
            filename = %item.filename,
            enabled,
            worlds = ?scope,
            "content enabled state changed"
        );
    }
    if matched > 0 {
        install::save(&ctx.entry_dir, index)?;
    }
    Ok(matched)
}

/// Apply a toggle to one item: item-wide with no `scope`, else only to the
/// named worlds. Enabling a world also turns the item itself back on, since a
/// wholly disabled pack loads nowhere.
fn scope_enabled(
    item: &mut InstalledContent,
    enabled: bool,
    scope: &[String],
    entry_worlds: &[String],
) {
    if scope.is_empty() {
        item.enabled = enabled;
        if enabled {
            item.disabled_worlds.clear();
        }
        return;
    }
    let targets: Vec<String> = install::target_worlds(item, entry_worlds)
        .iter()
        .filter(|world| {
            scope
                .iter()
                .any(|named| install::world_name(world) == named)
        })
        .cloned()
        .collect();
    if enabled {
        item.enabled = true;
        item.disabled_worlds
            .retain(|world| !targets.contains(world));
    } else {
        for world in targets {
            if !item.disabled_worlds.contains(&world) {
                item.disabled_worlds.push(world);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mod_item(project: &str, filename: &str) -> InstalledContent {
        InstalledContent {
            kind: ContentKind::Mod,
            project_id: project.to_string(),
            slug: project.to_string(),
            filename: filename.to_string(),
            ..InstalledContent::default()
        }
    }

    #[test]
    fn a_batch_takes_each_named_item_once() {
        let index = vec![
            mod_item("sodium", "sodium-1.jar"),
            mod_item("iris", "iris-1.jar"),
            mod_item("lithium", "lithium-1.jar"),
        ];
        let targets = update_targets(
            &index,
            ContentKind::Mod,
            &["sodium".to_string(), "iris-1.jar".to_string()],
        )
        .expect("resolve");
        assert_eq!(
            targets.iter().map(|i| &i.project_id).collect::<Vec<_>>(),
            vec!["sodium", "iris"]
        );

        let deduped = update_targets(
            &index,
            ContentKind::Mod,
            &["sodium".to_string(), "sodium-1.jar".to_string()],
        )
        .expect("resolve");
        assert_eq!(deduped.len(), 1, "one item named twice is one target");
    }

    #[test]
    fn an_empty_batch_takes_every_item_of_the_kind() {
        let index = vec![
            mod_item("sodium", "sodium-1.jar"),
            InstalledContent {
                kind: ContentKind::Shader,
                ..InstalledContent::default()
            },
        ];
        let targets = update_targets(&index, ContentKind::Mod, &[]).expect("resolve");
        assert_eq!(targets.len(), 1);
    }

    #[test]
    fn a_batch_naming_one_bad_item_moves_none_of_them() {
        let index = vec![mod_item("sodium", "sodium-1.jar")];
        assert!(
            update_targets(
                &index,
                ContentKind::Mod,
                &["sodium".to_string(), "nope".to_string()]
            )
            .is_err(),
            "a reference matching nothing refuses the whole batch"
        );

        let with_file = vec![
            mod_item("sodium", "sodium-1.jar"),
            InstalledContent {
                kind: ContentKind::Mod,
                filename: "hand-dropped.jar".to_string(),
                source: "file".to_string(),
                ..InstalledContent::default()
            },
        ];
        assert!(
            update_targets(
                &with_file,
                ContentKind::Mod,
                &["sodium".to_string(), "hand-dropped.jar".to_string()]
            )
            .is_err(),
            "a local file has no version to move to"
        );
        assert_eq!(
            update_targets(&with_file, ContentKind::Mod, &[])
                .expect("sweep")
                .len(),
            2,
            "the whole-kind sweep still skips it later, rather than refusing"
        );
    }

    #[test]
    fn scoping_a_world_off_leaves_the_rest_loaded() {
        let entry_worlds = vec!["saves/one".to_string(), "saves/two".to_string()];
        let mut item = InstalledContent {
            kind: ContentKind::DataPack,
            enabled: true,
            ..InstalledContent::default()
        };

        scope_enabled(&mut item, false, &["two".to_string()], &entry_worlds);
        assert_eq!(item.disabled_worlds, vec!["saves/two".to_string()]);
        assert!(item.enabled, "the item itself stays on");

        scope_enabled(&mut item, true, &["two".to_string()], &entry_worlds);
        assert!(item.disabled_worlds.is_empty());
    }

    #[test]
    fn removing_named_worlds_keeps_the_rest() {
        let entry_worlds = vec!["saves/one".to_string(), "saves/two".to_string()];
        let item = InstalledContent {
            kind: ContentKind::DataPack,
            ..InstalledContent::default()
        };
        assert_eq!(
            keep_worlds(&item, &["two".to_string()], &entry_worlds),
            vec!["saves/one".to_string()]
        );
        assert!(
            keep_worlds(
                &item,
                &["one".to_string(), "two".to_string()],
                &entry_worlds
            )
            .is_empty(),
            "no world left means the item goes"
        );
        assert!(
            keep_worlds(&item, &[], &entry_worlds).is_empty(),
            "an unscoped removal takes the item"
        );
    }
}
