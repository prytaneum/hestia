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

use anyhow::{Context, Result};
use proto::content::{
    ContentFailure, ContentKind, ContentProject, InstalledContent, ModpackFile, ResolvedModpack,
};
use proto::error::{EntryKind, ErrorInfo};
use proto::modpack::{
    InstalledModpack, ModpackFilePreview, ModpackFit, ModpackMove, ModpackRef, ModpackRemoveResult,
    ModpackTarget, ModpackUpdate, PackPolicy,
};
use proto::warning::WarningInfo;

use super::content::entry::{EntryContent, EntryRef, EntrySide};
use crate::cancel::Job;
use crate::content::{install, modpack, pack};
use crate::engine::{Engine, ServerCreateSpec, ServerUpdateSpec};
use crate::instances::InstanceRecord;
use crate::registry;
use crate::servers::ServerRecord;

/// What a finished install or update produced. `entry` is the entry's id —
/// which, for a create, the caller has no other way to learn.
pub struct ModpackOutcome {
    pub entry: String,
    pub entry_name: String,
    pub pack: InstalledModpack,
    pub failures: Vec<ContentFailure>,
    pub warnings: Vec<WarningInfo>,
}

/// What a game-version change produced: the entry as it now stands, and what
/// moving its pack degraded on.
pub struct VersionChange<R> {
    pub record: R,
    pub warnings: Vec<WarningInfo>,
    pub failures: Vec<ContentFailure>,
}

enum PackStep {
    Moved(Box<ModpackOutcome>),
    Stayed(Vec<WarningInfo>),
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

impl FileIdentity {
    fn of(file: &ModpackFile) -> Option<Self> {
        (!file.source.is_empty() && !file.version_id.is_empty()).then(|| FileIdentity {
            source: file.source.clone(),
            project_id: file.project_id.clone(),
            version_id: file.version_id.clone(),
            version_number: file.version_number.clone(),
            project: file.project.clone(),
        })
    }
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
        let fetched = self.fetch_pack(reference, "", job).await?;
        self.install_instance_pack(fetched, target, job).await
    }

    pub(crate) async fn install_instance_pack(
        &self,
        (resolved, project, mut archive): resolve::FetchedPack,
        target: &ModpackTarget,
        job: &Job<'_>,
    ) -> Result<ModpackOutcome> {
        let entry = match target {
            ModpackTarget::Create { name } => {
                let flavor = self.pack_flavor(&resolved, EntrySide::Client)?;
                job.check()?;
                self.create_instance(
                    &pack_entry_name(name, &resolved, |n| self.instance_name_taken(n)),
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
        let (resolved, project, mut archive) = self.fetch_pack(reference, "", job).await?;
        let entry = match target {
            ModpackTarget::Create { name } => {
                let flavor = self.pack_flavor(&resolved, EntrySide::Server)?;
                job.check()?;
                let servers = self.servers.list();
                let taken =
                    |n: &str| registry::name_taken(n, servers.iter().map(|r| r.name.as_str()));
                self.provision_server(
                    ServerCreateSpec {
                        name: pack_entry_name(name, &resolved, taken),
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

    /// Move a server to another game version, taking its modpack to the build
    /// for that version when `pack` says to and the pack has one.
    pub async fn update_server_with_pack(
        &self,
        spec: ServerUpdateSpec,
        pack: PackPolicy,
        job: &Job<'_>,
    ) -> Result<VersionChange<ServerRecord>> {
        let moved = self
            .move_pack_with_version(
                EntryRef::Server(&spec.server),
                &spec.version,
                pack,
                spec.allow_downgrade,
                job,
            )
            .await?;
        match moved {
            PackStep::Moved(outcome) => Ok(VersionChange {
                record: self
                    .servers
                    .get(&spec.server)
                    .with_context(|| format!("unknown server: {}", spec.server))?,
                warnings: outcome.warnings,
                failures: outcome.failures,
            }),
            PackStep::Stayed(mut warnings) => {
                let (record, updated) = self.update_server(spec, job).await?;
                warnings.extend(updated);
                Ok(VersionChange {
                    record,
                    warnings,
                    failures: Vec::new(),
                })
            }
        }
    }

    /// The instance twin of [`Self::update_server_with_pack`].
    pub async fn update_instance_with_pack(
        &self,
        reference: &str,
        version: &str,
        loader_version: Option<String>,
        allow_downgrade: bool,
        pack: PackPolicy,
        job: &Job<'_>,
    ) -> Result<VersionChange<InstanceRecord>> {
        let moved = self
            .move_pack_with_version(
                EntryRef::Instance(reference),
                version,
                pack,
                allow_downgrade,
                job,
            )
            .await?;
        match moved {
            PackStep::Moved(outcome) => Ok(VersionChange {
                record: self
                    .instances
                    .get(reference)
                    .with_context(|| format!("unknown instance: {reference}"))?,
                warnings: outcome.warnings,
                failures: outcome.failures,
            }),
            PackStep::Stayed(warnings) => Ok(VersionChange {
                record: self
                    .update_instance(reference, version, loader_version, allow_downgrade)
                    .await?,
                warnings,
                failures: Vec::new(),
            }),
        }
    }

    /// Runs the pack update that carries the entry to `version` when the pack
    /// is to follow and has a build for it; otherwise says why the pack stays.
    async fn move_pack_with_version(
        &self,
        target: EntryRef<'_>,
        version: &str,
        pack: PackPolicy,
        allow_downgrade: bool,
        job: &Job<'_>,
    ) -> Result<PackStep> {
        let (_, ctx) = self.content_ctx(target)?;
        let current = match (pack, modpack::load(&ctx.entry_dir)) {
            (PackPolicy::Move, Some(current)) if current.game_version != version => current,
            _ => return Ok(PackStep::Stayed(Vec::new())),
        };
        let fitting = match current.project_id.is_empty() {
            true => None,
            false => self
                .entry_modpack_update(target, version)
                .await?
                .and_then(|update| update.fitting),
        };
        let Some(fit) = fitting else {
            return Ok(PackStep::Stayed(vec![WarningInfo::PackNotMoved {
                pack: current.name,
                version_number: current.version_number,
                pack_game_version: current.game_version,
                target: version.to_string(),
            }]));
        };
        let to = ModpackMove {
            version: fit.version_id,
            allow_downgrade,
            ..ModpackMove::default()
        };
        let outcome = self.update_entry_modpack(target, &to, job).await?;
        Ok(PackStep::Moved(Box::new(outcome)))
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
        let versions = self
            .minecraft
            .versions(ctx.side.side(), &ctx.flavor)
            .await?;
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
    pub async fn entry_modpack_update(
        &self,
        entry: EntryRef<'_>,
        game_version: &str,
    ) -> Result<Option<ModpackUpdate>> {
        let (_, ctx) = self.content_ctx(entry)?;
        let Some(current) = modpack::load(&ctx.entry_dir) else {
            return Ok(None);
        };
        if current.project_id.is_empty() {
            return Ok(None);
        }
        let game_version = match game_version.is_empty() {
            true => ctx.game_version.as_str(),
            false => game_version,
        };
        let versions = self
            .pack_versions(&current.source, &current.project_id)
            .await?;
        let latest = resolve::update_target(&versions, &current.version_id).ok_or_else(|| {
            ErrorInfo::VersionNotFound {
                reference: current.project_id.clone(),
            }
        })?;
        let fitting =
            resolve::fitting(&versions, &current.version_id, game_version).map(|v| ModpackFit {
                version_id: v.id.clone(),
                version_number: v.version_number.clone(),
                channel: v.channel,
                game_version: game_version.to_string(),
            });
        Ok(Some(ModpackUpdate {
            updatable: latest.id != current.version_id,
            latest_version_id: latest.id.clone(),
            latest_version_number: latest.version_number.clone(),
            current_version_id: current.version_id,
            current_version_number: current.version_number,
            fitting,
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

/// A name the user did not choose is made unique, so it is never what fails the create.
fn pack_entry_name(
    given: &str,
    resolved: &ResolvedModpack,
    taken: impl Fn(&str) -> bool,
) -> String {
    if !given.trim().is_empty() {
        return given.trim().to_string();
    }
    let own = resolved.name.clone();
    if !taken(&own) {
        return own;
    }
    (2..100)
        .map(|n| format!("{own} ({n})"))
        .find(|candidate| !taken(candidate))
        .unwrap_or(own)
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
    use proto::content::{ContentVersion, ReleaseChannel};

    use super::*;

    #[test]
    fn an_entry_takes_the_packs_name_only_when_none_was_given() {
        let resolved = ResolvedModpack {
            name: "Fabulously Optimized".to_string(),
            ..ResolvedModpack::default()
        };
        assert_eq!(
            pack_entry_name("", &resolved, |_| false),
            "Fabulously Optimized"
        );
        assert_eq!(
            pack_entry_name("  my pack ", &resolved, |_| true),
            "my pack"
        );
    }

    #[test]
    fn a_defaulted_name_steps_past_the_entries_already_using_it() {
        let resolved = ResolvedModpack {
            name: "Fabulously Optimized".to_string(),
            ..ResolvedModpack::default()
        };
        let existing = ["Fabulously Optimized", "Fabulously Optimized (2)"];
        assert_eq!(
            pack_entry_name("", &resolved, |n| existing.contains(&n)),
            "Fabulously Optimized (3)"
        );
    }

    fn published(id: &str, channel: ReleaseChannel, game: &str) -> ContentVersion {
        ContentVersion {
            id: id.to_string(),
            channel,
            game_versions: vec![game.to_string()],
            ..ContentVersion::default()
        }
    }

    fn catalogue() -> Vec<ContentVersion> {
        vec![
            published("15-beta", ReleaseChannel::Beta, "26.3"),
            published("14.1.1", ReleaseChannel::Release, "26.2"),
            published("14.1.0", ReleaseChannel::Release, "26.2"),
        ]
    }

    #[test]
    fn an_unpinned_update_takes_the_newest_release_but_never_moves_back() {
        let versions = catalogue();
        let target = |installed| resolve::update_target(&versions, installed).map(|v| v.id.clone());
        assert_eq!(target("14.1.0").as_deref(), Some("14.1.1"));
        assert_eq!(target("15-beta").as_deref(), Some("15-beta"));
    }

    #[test]
    fn a_pack_offers_the_version_built_for_the_game_version_asked_about() {
        let versions = catalogue();
        let fit =
            |installed, game| resolve::fitting(&versions, installed, game).map(|v| v.id.clone());
        assert_eq!(fit("14.1.1", "26.3").as_deref(), Some("15-beta"));
        assert_eq!(fit("14.1.0", "26.2").as_deref(), Some("14.1.1"));
        assert_eq!(fit("14.1.1", "26.2"), None);
        assert_eq!(fit("14.1.1", "26.4"), None);
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
