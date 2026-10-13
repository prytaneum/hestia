//! Modpack install, update and removal, for a new or an existing entry.
//!
//! A pack is three different things at once, and each goes where it belongs
//! rather than into a parallel store of its own:
//!
//! - its **loader and game version** become the entry's flavor and version, so
//!   creating from a pack is the ordinary create with those filled in;
//! - its **files under a managed kind directory** — named by the index or
//!   shipped in an override tree — become ordinary pool items tagged
//!   `modpack:<project>`, so the launch-time mirror, the backup heal,
//!   `content list` and per-item update all work on them unchanged;
//! - everything else it ships — the rest of its `overrides/`, plus any index
//!   file outside a managed kind directory — is written straight into the game
//!   directory and recorded in `modpack.json` with the hash it was written with.
//!
//! Only the third needs a record, and only because those files are the ones the
//! user edits: the hash is what lets an update replace a file the pack still
//! owns while leaving a tweaked one exactly as it was found.
//!
//! This module is the per-side seam; [`resolve`] turns a reference into an
//! archive and [`apply`] puts it onto an entry.

mod apply;
mod resolve;

use std::path::Path;

use anyhow::Result;
use proto::content::{
    ContentFailure, ContentKind, ContentProject, InstalledContent, ResolvedModpack,
};
use proto::error::{EntryKind, ErrorInfo};
use proto::modpack::{
    InstalledModpack, ModpackFilePreview, ModpackMove, ModpackRef, ModpackRemoveResult,
    ModpackTarget, ModpackUpdate,
};
use proto::warning::WarningInfo;

use super::content::entry::{EntryContent, EntryRef, EntrySide};
use crate::cancel::Job;
use crate::content::{install, modpack, pack};
use crate::engine::{Engine, ServerCreateSpec, ServerUpdateSpec};

/// What a finished install or update produced. `entry` is the entry's id —
/// which, for a create, the caller has no other way to learn.
pub struct ModpackOutcome {
    pub entry: String,
    pub entry_name: String,
    pub pack: InstalledModpack,
    pub failures: Vec<ContentFailure>,
    pub warnings: Vec<WarningInfo>,
}

/// The release a source knows one pack file to be. A pack names its files by
/// URL and hash alone, so a file no source recognises has none — and installs
/// anyway, just untracked.
struct FileIdentity {
    source: String,
    project_id: String,
    version_id: String,
    /// Empty, like `project`, when the catalogue could not be reached.
    version_number: String,
    project: Option<ContentProject>,
}

struct Supplied {
    kind: ContentKind,
    filename: String,
    sha1: String,
    url: String,
    identity: Option<FileIdentity>,
}

impl Supplied {
    /// What to call the file while it downloads and in the pool: the project's
    /// title where there is one, the filename otherwise.
    fn label(&self) -> String {
        self.identity
            .as_ref()
            .and_then(|identity| identity.project.as_ref())
            .map(|project| project.title.clone())
            .unwrap_or_else(|| self.filename.clone())
    }
}

impl Engine {
    /// Install a pack into a new or existing instance.
    pub async fn install_instance_modpack(
        &self,
        reference: &ModpackRef,
        target: &ModpackTarget,
        job: &Job<'_>,
    ) -> Result<ModpackOutcome> {
        let (resolved, project, mut archive) = self.fetch_pack(reference, job).await?;
        let entry = match target {
            ModpackTarget::Create { name } => {
                let flavor = self.pack_flavor(&resolved, EntrySide::Client)?;
                job.check()?;
                self.create_instance(
                    &pack_entry_name(name, &resolved),
                    &flavor,
                    &resolved.game_version,
                    resolved.loader_version.clone(),
                    &[],
                )
                .await?
                .id
            }
            ModpackTarget::Existing { entry } => entry.clone(),
        };

        let outcome = self
            .apply_to(
                EntryRef::Instance(&entry),
                &resolved,
                project.as_ref(),
                &mut archive,
                job,
            )
            .await;
        if outcome.is_err() && matches!(target, ModpackTarget::Create { .. }) {
            // A create that could not be filled leaves nothing, exactly as a
            // failed or cancelled plain create does.
            let _ = self.remove_instance(&entry);
        }
        outcome
    }

    /// Install a pack into a new or existing server. A pack is client-shaped by
    /// convention, so the server side takes what the pack marks as its own —
    /// `env.server`, plus `server-overrides/` in place of `client-overrides/`.
    pub async fn install_server_modpack(
        &self,
        reference: &ModpackRef,
        target: &ModpackTarget,
        eula: bool,
        port: Option<u16>,
        job: &Job<'_>,
    ) -> Result<ModpackOutcome> {
        let (resolved, project, mut archive) = self.fetch_pack(reference, job).await?;
        let entry = match target {
            ModpackTarget::Create { name } => {
                let flavor = self.pack_flavor(&resolved, EntrySide::Server)?;
                job.check()?;
                self.provision_server(
                    ServerCreateSpec {
                        name: pack_entry_name(name, &resolved),
                        flavor,
                        version: resolved.game_version.clone(),
                        loader_version: resolved.loader_version.clone(),
                        port,
                        config: Vec::new(),
                        eula,
                    },
                    job,
                )
                .await?
                .0
                .id
            }
            ModpackTarget::Existing { entry } => entry.clone(),
        };

        let outcome = self
            .apply_to(
                EntryRef::Server(&entry),
                &resolved,
                project.as_ref(),
                &mut archive,
                job,
            )
            .await;
        if outcome.is_err() && matches!(target, ModpackTarget::Create { .. }) {
            let _ = self.servers.remove(&entry);
        }
        outcome
    }

    /// Move an instance's pack to another published version. A pack update
    /// carries the game version with it — that is what updating a pack means —
    /// so the entry's own version moves too, behind the same downgrade gate the
    /// plain version update uses.
    pub async fn update_entry_modpack(
        &self,
        target: EntryRef<'_>,
        to: &ModpackMove,
        job: &Job<'_>,
    ) -> Result<ModpackOutcome> {
        let (entry, ctx) = self.content_ctx(target)?;
        let current = require_pack(&ctx, entry_kind(target), &entry.name)?;
        let (resolved, project, mut archive) = self.fetch_update(&current, to, job).await?;
        let allow_downgrade = to.allow_downgrade;

        // A pack that moved game version carries its entry with it, through the
        // same in-place update a plain version change uses.
        if resolved.game_version != entry.game_version {
            match target {
                EntryRef::Instance(_) => {
                    self.update_instance(
                        &entry.id,
                        &resolved.game_version,
                        resolved.loader_version.clone(),
                        allow_downgrade,
                    )
                    .await?;
                }
                EntryRef::Server(_) => {
                    self.update_server(
                        ServerUpdateSpec {
                            server: entry.id.clone(),
                            version: resolved.game_version.clone(),
                            loader_version: resolved.loader_version.clone(),
                            allow_downgrade,
                        },
                        job,
                    )
                    .await?;
                }
            }
        }
        self.apply_to(
            same_entry(target, &entry.id),
            &resolved,
            project.as_ref(),
            &mut archive,
            job,
        )
        .await
    }

    /// What [`Self::update_entry_modpack`] would do with a pack file, so a
    /// front-end can ask about another pack or a downgrade before it starts.
    pub async fn preview_entry_modpack_file(
        &self,
        target: EntryRef<'_>,
        path: &Path,
    ) -> Result<ModpackFilePreview> {
        let (entry, ctx) = self.content_ctx(target)?;
        let current = require_pack(&ctx, entry_kind(target), &entry.name)?;
        let fetched = self.read_pack_file(path).await?;
        let versions = match ctx.side {
            EntrySide::Server => self.minecraft.server_versions(&ctx.flavor).await?,
            EntrySide::Client => self.minecraft.instance_versions(&ctx.flavor).await?,
        };
        let (resolved, project, _) = &fetched;
        Ok(ModpackFilePreview {
            name: pack_display_name(resolved, project.as_ref()),
            version_number: resolved.version_number.clone(),
            game_version: resolved.game_version.clone(),
            same_pack: same_pack(&current, &fetched),
            downgrade: proto::minecraft::downgrade_between(
                &versions,
                &ctx.game_version,
                &resolved.game_version,
            ) == Some(true),
        })
    }

    pub fn entry_modpack(&self, entry: EntryRef<'_>) -> Result<Option<InstalledModpack>> {
        let (_, ctx) = self.content_ctx(entry)?;
        Ok(modpack::load(&ctx.entry_dir))
    }

    /// Whether the entry's pack has a newer published version, over the same
    /// pick an unpinned [`Self::update_entry_modpack`] makes. `None` when there
    /// is nothing to check: no pack, or one no source published.
    pub async fn entry_modpack_update(&self, entry: EntryRef<'_>) -> Result<Option<ModpackUpdate>> {
        let (_, ctx) = self.content_ctx(entry)?;
        let Some(current) = modpack::load(&ctx.entry_dir) else {
            return Ok(None);
        };
        if current.project_id.is_empty() {
            return Ok(None);
        }
        let latest = self
            .pick_pack_version(&current.source, &current.project_id, "")
            .await?;
        Ok(Some(ModpackUpdate {
            updatable: latest.id != current.version_id,
            current_version_id: current.version_id,
            current_version_number: current.version_number,
            latest_version_id: latest.id,
            latest_version_number: latest.version_number,
        }))
    }

    /// Take the pack out of an entry — with its content, or `keep_content` to
    /// drop only the pack and leave everything it installed as the user's own.
    pub fn remove_entry_modpack(
        &self,
        target: EntryRef<'_>,
        keep_content: bool,
    ) -> Result<ModpackRemoveResult> {
        let (entry, ctx) = self.content_ctx(target)?;
        let pack = require_pack(&ctx, entry_kind(target), &entry.name)?;
        match keep_content {
            true => release_pack(&ctx, &pack),
            false => remove_pack(&ctx, &pack),
        }
    }

    async fn apply_to(
        &self,
        target: EntryRef<'_>,
        resolved: &ResolvedModpack,
        project: Option<&ContentProject>,
        archive: &mut pack::Archive,
        job: &Job<'_>,
    ) -> Result<ModpackOutcome> {
        let (entry, ctx) = self.content_ctx(target)?;
        let (pack, failures, warnings) = self
            .apply_pack(&ctx, resolved, project, archive, job)
            .await?;
        Ok(ModpackOutcome {
            entry: entry.id,
            entry_name: entry.name,
            pack,
            failures,
            warnings,
        })
    }
}

fn entry_kind(entry: EntryRef<'_>) -> EntryKind {
    match entry {
        EntryRef::Server(_) => EntryKind::Server,
        EntryRef::Instance(_) => EntryKind::Instance,
    }
}

/// The same side, named by id — an entry resolved by name is re-resolved by the
/// id it turned out to have.
fn same_entry<'a>(entry: EntryRef<'_>, id: &'a str) -> EntryRef<'a> {
    match entry {
        EntryRef::Server(_) => EntryRef::Server(id),
        EntryRef::Instance(_) => EntryRef::Instance(id),
    }
}

fn require_pack(ctx: &EntryContent, entry: EntryKind, name: &str) -> Result<InstalledModpack> {
    modpack::load(&ctx.entry_dir).ok_or_else(|| {
        ErrorInfo::ModpackNotInstalled {
            entry,
            name: name.to_string(),
        }
        .into()
    })
}

/// Take the pack out of an entry: its pool items and its game-directory files,
/// leaving anything the user has since edited. The entry itself stays — a pack
/// is content an entry carries, not its identity.
fn remove_pack(ctx: &EntryContent, pack: &InstalledModpack) -> Result<ModpackRemoveResult> {
    let origin = modpack::origin(pack);
    let worlds = ctx.worlds();
    let mut index = install::load(&ctx.entry_dir);
    let dropped: Vec<InstalledContent> = index
        .iter()
        .filter(|i| i.origin == origin)
        .cloned()
        .collect();
    for item in &dropped {
        install::remove_files(&ctx.entry_dir, &ctx.data_dir, item, &worlds);
    }
    index.retain(|i| i.origin != origin);
    install::save(&ctx.entry_dir, index)?;

    let (removed_overrides, kept) = modpack::remove_overrides(&ctx.data_dir, &pack.overrides);
    modpack::clear(&ctx.entry_dir);
    tracing::info!(
        entry = %ctx.entry_dir.display(),
        pack = %pack.name,
        files = dropped.len(),
        overrides = removed_overrides,
        kept = kept.len(),
        "modpack removed"
    );
    Ok(ModpackRemoveResult {
        removed_files: dropped.len() as u32,
        removed_overrides,
        kept,
        ..ModpackRemoveResult::default()
    })
}

/// Forget the pack and keep what it installed: its pool items lose the origin
/// tag, so they are removable and syncable like anything the user added, and
/// the game-directory files it wrote simply stop being recorded.
fn release_pack(ctx: &EntryContent, pack: &InstalledModpack) -> Result<ModpackRemoveResult> {
    let origin = modpack::origin(pack);
    let mut index = install::load(&ctx.entry_dir);
    let mut released = 0;
    for item in index.iter_mut().filter(|i| i.origin == origin) {
        item.origin.clear();
        released += 1;
    }
    install::save(&ctx.entry_dir, index)?;
    modpack::clear(&ctx.entry_dir);
    tracing::info!(
        entry = %ctx.entry_dir.display(),
        pack = %pack.name,
        files = released,
        "modpack released, content kept"
    );
    Ok(ModpackRemoveResult {
        released_files: released,
        ..ModpackRemoveResult::default()
    })
}

fn pack_entry_name(given: &str, resolved: &ResolvedModpack) -> String {
    match given.trim().is_empty() {
        false => given.trim().to_string(),
        true => resolved.name.clone(),
    }
}

fn pack_display_name(resolved: &ResolvedModpack, project: Option<&ContentProject>) -> String {
    let from_project = project.map(|p| p.title.as_str()).unwrap_or_default();
    match (resolved.name.is_empty(), from_project.is_empty()) {
        (false, _) => resolved.name.clone(),
        (true, false) => from_project.to_string(),
        (true, true) => "modpack".to_string(),
    }
}

/// Whether a pack file holds the pack an entry runs. A pack file carries no
/// id, so the project decides where a source recognises both and the name —
/// free text an author may change — is all there is otherwise.
fn same_pack(current: &InstalledModpack, (resolved, project, _): &resolve::FetchedPack) -> bool {
    if !current.project_id.is_empty() && !resolved.project_id.is_empty() {
        return current.source == resolved.source && current.project_id == resolved.project_id;
    }
    let given = pack_display_name(resolved, project.as_ref());
    current.name.trim().eq_ignore_ascii_case(given.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_takes_the_packs_name_only_when_none_was_given() {
        let resolved = ResolvedModpack {
            name: "Fabulously Optimized".to_string(),
            ..ResolvedModpack::default()
        };
        assert_eq!(pack_entry_name("", &resolved), "Fabulously Optimized");
        assert_eq!(pack_entry_name("  my pack ", &resolved), "my pack");
    }

    #[test]
    fn a_pack_with_no_name_of_its_own_takes_the_projects() {
        let project = ContentProject {
            title: "Cozy".to_string(),
            ..ContentProject::default()
        };
        assert_eq!(
            pack_display_name(&ResolvedModpack::default(), Some(&project)),
            "Cozy"
        );
        assert_eq!(
            pack_display_name(&ResolvedModpack::default(), None),
            "modpack"
        );
    }
}
