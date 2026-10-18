//! Per-entry content management: install from a platform project, a source page
//! URL, or a local file; list, remove, and update what is installed. The managed
//! directory under the entry root is the source of truth; `data/` holds a mirror.
//!
//! This module is the seam between the two sides and the work: one verb each,
//! taking an [`entry::EntryRef`] that resolves into one [`entry::EntryContent`]
//! whichever side it names. The work lives beside it — [`entry`] for what an
//! entry is and accepts, [`add`] for installing, [`manage`] for everything
//! already installed.

mod add;
pub(crate) mod entry;
mod manage;

use std::path::Path;

use anyhow::{bail, Result};
use proto::content::{
    ContentAddSpec, ContentFailure, ContentKind, ContentMatch, InstalledContent, UntrackedFile,
};

pub(in crate::engine) use self::entry::EntryRef;
use self::entry::{Entry, EntryContent};
use self::manage::{list_content, remove_content, set_enabled, update_from_file};
use super::phase_progress;
use crate::content::install;
use crate::engine::Engine;
use crate::minecraft::materialize::OnProgress;
use crate::minecraft::Side;

impl Engine {
    /// Install a batch of content into an entry — each item a platform project,
    /// a direct URL, or a local file. Returns everything installed (items plus
    /// required dependencies) and, per item that could not be installed, a
    /// failure; the batch continues past them.
    pub async fn add_entry_content(
        &self,
        entry: EntryRef<'_>,
        spec: &ContentAddSpec,
        on_progress: OnProgress<'_>,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>)> {
        let (_, ctx) = self.content_ctx(entry)?;
        self.ensure_accepts(&ctx, spec.kind)?;
        self.add_content(&ctx, spec, on_progress).await
    }

    /// An entry's installed items of one kind, plus untracked filenames found
    /// in its game directory.
    pub fn entry_content(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
    ) -> Result<(Vec<InstalledContent>, Vec<UntrackedFile>)> {
        let (_, ctx) = self.content_ctx(entry)?;
        Ok(list_content(&ctx, kind))
    }

    /// Uninstall the named items (each matched by project id, slug, filename,
    /// or title) — their managed copies and every mirror of them. A non-empty
    /// `worlds` instead narrows a datapack to the worlds it keeps loading in.
    /// Returns how many items were uninstalled outright, which a world-narrowed
    /// removal leaves at zero.
    pub fn remove_entry_content(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
        items: &[String],
        worlds: &[String],
    ) -> Result<usize> {
        let (_, ctx) = self.content_ctx(entry)?;
        let index = install::load(&ctx.entry_dir);
        resolve_all(&index, kind, items)?;
        index
            .iter()
            .filter(|i| i.kind == kind && items.iter().any(|item| install::matches(i, item)))
            .try_for_each(refuse_owned)?;
        let removed = remove_content(&ctx, kind, items, worlds)?;
        Ok(removed.len())
    }

    /// Update one local item from the file at `path`, keeping its enabled
    /// state, world targeting and disables. A tracked item is refused.
    pub fn update_entry_content_from_file(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
        item: &str,
        path: &Path,
    ) -> Result<InstalledContent> {
        let (_, ctx) = self.content_ctx(entry)?;
        update_from_file(&ctx, kind, item, path)
    }

    /// Move platform-sourced items to their newest compatible version — the
    /// named items, or every item of the kind when `items` is empty. Returns
    /// what actually changed and, per item that could not move, a failure; the
    /// batch continues past them.
    pub async fn update_entry_content(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
        items: &[String],
        on_progress: OnProgress<'_>,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>)> {
        let (_, ctx) = self.content_ctx(entry)?;
        self.update_content(&ctx, kind, items, "", on_progress)
            .await
    }

    /// Re-pin one named platform item to a specific published `version` (id or
    /// number), re-installing that version like an update.
    pub async fn set_entry_content_version(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
        item: &str,
        version: &str,
        source: &str,
        on_progress: OnProgress<'_>,
    ) -> Result<Vec<InstalledContent>> {
        if item.is_empty() || version.is_empty() {
            bail!(proto::error::ErrorInfo::FieldsRequired {
                fields: vec![proto::error::Field::Item, proto::error::Field::Version]
            });
        }
        let (_, ctx) = self.content_ctx(entry)?;
        match source.is_empty() {
            true => {
                self.repin_content(&ctx, kind, item, version, on_progress)
                    .await
            }
            false => {
                self.move_content_source(&ctx, kind, item, source, version, on_progress)
                    .await
            }
        }
    }

    pub async fn entry_content_matches(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
        item: &str,
    ) -> Result<Vec<ContentMatch>> {
        let (_, ctx) = self.content_ctx(entry)?;
        Ok(self
            .content_matches(&ctx, kind, item)
            .await?
            .into_iter()
            .map(ContentMatch::from)
            .collect())
    }

    /// Enable or disable installed items matching `item`; a non-empty `worlds`
    /// scopes a datapack toggle to those save worlds. Returns the number of
    /// matched entries — zero means nothing matched. The entry must be stopped
    /// (enforced at the service boundary).
    pub fn enable_entry_content(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
        item: &str,
        enabled: bool,
        worlds: &[String],
    ) -> Result<usize> {
        let (_, ctx) = self.content_ctx(entry)?;
        set_enabled(&ctx, kind, item, enabled, worlds)
    }

    /// For each platform-sourced item of the kind, resolve the newest
    /// compatible version and report whether it differs from the current pin.
    /// An item whose versions cannot be resolved is skipped, not fatal.
    pub async fn check_entry_updates(
        &self,
        entry: EntryRef<'_>,
        kind: ContentKind,
    ) -> Result<Vec<proto::content::ContentUpdate>> {
        let (_, ctx) = self.content_ctx(entry)?;
        self.content_updates(&ctx, kind).await
    }

    /// Resolve an entry into the shape every content and pack flow works
    /// against. The one place the two sides are told apart.
    pub(in crate::engine::flows) fn content_ctx(
        &self,
        entry: EntryRef<'_>,
    ) -> Result<(Entry, EntryContent)> {
        match entry {
            EntryRef::Server(reference) => {
                let record = self.ready_server(reference)?;
                let ctx = EntryContent {
                    entry_dir: self.servers.server_dir(&record),
                    data_dir: self.servers.data_dir(&record),
                    game_version: record.profile.game_version.clone(),
                    flavor: record.profile.flavor.clone(),
                    side: Side::Server,
                };
                Ok((
                    Entry {
                        id: record.id,
                        name: record.name,
                        game_version: record.profile.game_version,
                    },
                    ctx,
                ))
            }
            EntryRef::Instance(reference) => {
                let record = self.instances.require(reference)?;
                let ctx = EntryContent {
                    entry_dir: self.instances.instance_dir(&record),
                    data_dir: self.instances.data_dir(&record),
                    game_version: record.profile.game_version.clone(),
                    flavor: record.profile.flavor.clone(),
                    side: Side::Client,
                };
                Ok((
                    Entry {
                        id: record.id,
                        name: record.name,
                        game_version: record.profile.game_version,
                    },
                    ctx,
                ))
            }
        }
    }
}

/// Check every reference names something installed, before any of them is
/// acted on. An empty batch means "all of the kind" and has nothing to check.
pub(in crate::engine::flows) fn resolve_all(
    index: &[InstalledContent],
    kind: ContentKind,
    items: &[String],
) -> Result<()> {
    for item in items {
        if !index
            .iter()
            .any(|i| i.kind == kind && install::matches(i, item))
        {
            bail!(proto::error::ErrorInfo::ContentNotFound {
                reference: item.clone()
            });
        }
    }
    Ok(())
}

/// Content something else installed is owned by that thing: a local change to it
/// would be undone at the next apply or pack update, so it is refused and the
/// user is pointed at whatever owns it.
fn refuse_owned(item: &InstalledContent) -> Result<()> {
    if item.origin.is_empty() {
        return Ok(());
    }
    bail!(
        "'{}' was installed by {}; change it there instead",
        item.title,
        origin_owner(&item.origin),
    )
}

/// Who an `origin` tag says installed an item, for a refusal that names where
/// to go instead.
fn origin_owner(origin: &str) -> String {
    match origin.split_once(':') {
        Some(("modpack", name)) => format!("modpack '{name}'"),
        Some((scope, name)) => format!("{scope} '{name}'"),
        None => format!("'{origin}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_whatever_owns_the_item() {
        assert_eq!(origin_owner("modpack:1KVo5zza"), "modpack '1KVo5zza'");
        assert_eq!(origin_owner("hand-placed"), "'hand-placed'");
    }
}
