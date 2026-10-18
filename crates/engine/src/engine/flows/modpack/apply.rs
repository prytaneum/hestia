//! Putting a resolved pack onto an entry: which of its files go where, what is
//! downloaded, what is written into the game directory, and how the result
//! reconciles against whatever pack was installed before.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use proto::content::{
    ContentFailure, ContentKind, ContentProject, InstalledContent, ModpackFile, ResolvedModpack,
};
use proto::minecraft::ProvisionPhase;
use proto::modpack::{InstalledModpack, ModpackOverride};
use proto::warning::WarningInfo;

use super::super::content::entry::{content_target, EntryContent};
use super::super::download::Download;
use super::{pack_display_name, FileIdentity, Supplied};
use crate::cancel::Job;
use crate::config::ModpackSettings;
use crate::content::pool::Pool;
use crate::content::{exclude, install, modpack, pack, record};
use crate::engine::Engine;
use crate::minecraft::{materialize, Side};

impl Engine {
    /// The install itself, once the entry exists: refuse a pack the entry
    /// cannot run, download its managed files, write its game-directory files,
    /// and reconcile against whatever pack was there before.
    pub(super) async fn apply_pack(
        &self,
        ctx: &EntryContent,
        resolved: &ResolvedModpack,
        project: Option<&ContentProject>,
        archive: &mut pack::Archive,
        job: &Job<'_>,
    ) -> Result<(InstalledModpack, Vec<ContentFailure>, Vec<WarningInfo>)> {
        self.ensure_entry_matches(ctx, resolved)?;
        let previous = modpack::load(&ctx.entry_dir);
        let side = pack::Side::from(ctx.side);
        let accepts = self.accepted_kinds(ctx);
        let settings = self.config().settings().modpack;
        let inclusion = self.pack_inclusion(ctx, project, &settings);
        let mut warnings = Vec::new();

        let mut managed = Vec::new();
        let mut loose = Vec::new();
        let mut excluded = Vec::new();
        for file in &resolved.files {
            let wanted = side.wants(file);
            if !inclusion.includes(&file.path, wanted) {
                // A file the pack never wanted here is simply the other side's;
                // one it did want and we held back is worth saying.
                if wanted {
                    excluded.push(file.artifact.filename.clone());
                }
                continue;
            }
            match managed_kind(&file.path) {
                // A kind this flavor's loader cannot manage still belongs on
                // disk where the pack put it: the pack said the side wants it,
                // and hestia's own pool model is not a reason to drop a file.
                Some(kind) if accepts.contains(&kind) => managed.push((kind, file)),
                _ => loose.push(file),
            }
        }
        if !excluded.is_empty() {
            excluded.sort();
            warnings.push(WarningInfo::ModpackFilesExcluded {
                count: excluded.len() as u32,
                files: excluded,
            });
        }

        let patterns = exclude::OverridePatterns::new(&settings.overrides_exclusions);
        let (mut supplied, mut failures) =
            self.fetch_pack_content(ctx, &managed, resolved, job).await;
        let mut overrides = self
            .fetch_loose_files(ctx, &loose, previous.as_ref(), &patterns, job)
            .await;
        let mut kept: Vec<String> = Vec::new();

        job.check()?;
        let owned: HashMap<&str, &ModpackOverride> = previous
            .as_ref()
            .map(|p| p.overrides.iter().map(|o| (o.path.as_str(), o)).collect())
            .unwrap_or_default();
        // A first install has nothing of the user's to protect, so the pack's
        // own files win outright, as docker-mc-server's do. Once a pack is
        // installed, an edited file is the user's and the hash record is what
        // tells the two apart.
        let installing = previous.is_none();
        let data_dir = ctx.data_dir.clone();
        let pooled = |path: &str| managed_kind(path).filter(|kind| accepts.contains(kind));
        let written = archive.extract_overrides(side, job, |path| {
            if patterns.excludes(path) {
                return None;
            }
            // Pool content like an index file, so it goes where the mirror reads.
            if let Some(kind) = pooled(path) {
                return content_target(ctx, kind, filename(path)).ok();
            }
            let writable = installing
                || match owned.get(path) {
                    // Known to the previous pack: only overwrite what is still
                    // byte-for-byte what we wrote.
                    Some(previous) => modpack::ours(&data_dir, previous),
                    // Unknown: an existing file was not put there by a pack, so
                    // it is the user's and is not ours to replace.
                    None => !data_dir.join(path).exists(),
                };
            if !writable {
                kept.push(path.to_string());
            }
            writable.then(|| data_dir.join(path))
        })?;
        for file in written {
            match pooled(&file.path) {
                Some(kind) => {
                    let filename = filename(&file.path).to_string();
                    // Written last, so it wins a name the index also supplied.
                    supplied.retain(|s| !(s.kind == kind && s.filename == filename));
                    supplied.push(Supplied {
                        kind,
                        filename,
                        sha1: file.sha1,
                        url: String::new(),
                        identity: None,
                    });
                }
                None => overrides.push(ModpackOverride {
                    path: file.path,
                    sha1: file.sha1,
                }),
            }
        }

        job.check()?;
        self.recognise(ctx, &resolved.source, &mut supplied).await;
        let items: Vec<InstalledContent> = supplied
            .into_iter()
            .map(|file| pack_item(file, &ctx.entry_dir))
            .collect();

        let mut pack = InstalledModpack {
            source: resolved.source.clone(),
            project_id: resolved.project_id.clone(),
            slug: project.map(|p| p.slug.clone()).unwrap_or_default(),
            name: pack_display_name(resolved, project),
            version_id: resolved.version_id.clone(),
            version_number: resolved.version_number.clone(),
            game_version: resolved.game_version.clone(),
            loader: resolved.loader.clone().unwrap_or_default(),
            loader_version: resolved.loader_version.clone().unwrap_or_default(),
            icon_url: project.map(|p| p.icon_url.clone()).unwrap_or_default(),
            installed_unix: common::now_unix(),
            files: Vec::new(),
            overrides,
        };

        let (recorded, mut unplaced) =
            Self::merge_pack_items(ctx, &pack, items, previous.as_ref())?;
        pack.files = recorded;
        failures.append(&mut unplaced);
        if let Some(previous) = &previous {
            let stale = stale_overrides(previous, &pack);
            let (_, mut left) = modpack::remove_overrides(&ctx.data_dir, &stale);
            kept.append(&mut left);
        }
        if !kept.is_empty() {
            kept.sort();
            kept.dedup();
            warnings.push(WarningInfo::ModpackOverridesKept {
                count: kept.len() as u32,
                paths: kept,
            });
        }
        modpack::save(&ctx.entry_dir, &pack)?;
        tracing::info!(
            entry = %ctx.entry_dir.display(),
            pack = %pack.name,
            version = %pack.version_number,
            files = pack.files.len(),
            overrides = pack.overrides.len(),
            failures = failures.len(),
            "modpack installed"
        );
        Ok((pack, failures, warnings))
    }

    /// The corrections in force over this pack's own `env` declarations.
    /// Server-side only: the shipped table names client mods, so applying it to
    /// an instance would strip a modpack of exactly the mods it exists for.
    fn pack_inclusion(
        &self,
        ctx: &EntryContent,
        project: Option<&ContentProject>,
        settings: &ModpackSettings,
    ) -> exclude::Inclusion {
        if ctx.side != Side::Server {
            return exclude::Inclusion::none();
        }
        exclude::Inclusion::new(
            project.map(|p| p.slug.as_str()).unwrap_or_default(),
            settings.default_excludes,
            &settings.exclude_files,
            &settings.force_include_files,
        )
    }

    /// Download the pack's managed-kind files into the entry's managed
    /// directories. Each is recorded with the ids the resolved pack carries, so
    /// a pack's mods are ordinary tracked items.
    async fn fetch_pack_content(
        &self,
        ctx: &EntryContent,
        files: &[(ContentKind, &ModpackFile)],
        resolved: &ResolvedModpack,
        job: &Job<'_>,
    ) -> (Vec<Supplied>, Vec<ContentFailure>) {
        let mut named = HashSet::new();
        let mut queued: Vec<(Supplied, &ModpackFile)> = files
            .iter()
            .filter(|(_, file)| named.insert(file.path.as_str()))
            .map(|(kind, file)| {
                let pending = Supplied {
                    kind: *kind,
                    filename: file.artifact.filename.clone(),
                    sha1: String::new(),
                    url: file.artifact.url.clone(),
                    identity: FileIdentity::of(file),
                };
                (pending, *file)
            })
            .collect();
        let mut identities: Vec<&mut FileIdentity> = queued
            .iter_mut()
            .filter_map(|(pending, _)| pending.identity.as_mut())
            .collect();
        self.hydrate(&mut identities).await;

        let mut failures = Vec::new();
        let mut downloads = Vec::new();
        for (pending, file) in queued {
            let label = pending.label();
            let planned = materialize::validate_filename(&file.artifact.filename).and_then(|()| {
                Ok(Download {
                    artifact: file.artifact.clone(),
                    target: content_target(ctx, pending.kind, &file.artifact.filename)?,
                    label: label.clone(),
                    source: resolved.source.clone(),
                })
            });
            match planned {
                Ok(download) => downloads.push(((pending, file, label), download)),
                Err(e) => failures.push(ContentFailure {
                    item: file.path.clone(),
                    title: label,
                    error: crate::error_info(e),
                }),
            }
        }

        let mut supplied = Vec::new();
        for ((mut pending, file, label), fetched) in self
            .download_batch(downloads, ProvisionPhase::Content, job)
            .await
        {
            match fetched {
                Ok(sha1) => {
                    pending.sha1 = sha1;
                    supplied.push(pending);
                }
                Err(e) => failures.push(ContentFailure {
                    item: file.path.clone(),
                    title: label,
                    error: crate::error_info(e),
                }),
            }
        }
        (supplied, failures)
    }

    /// The index files that are not managed-kind content — a pack shipping its
    /// own `config/…` by URL rather than in `overrides/`. They go straight into
    /// the game directory and are recorded like any other pack-owned file.
    async fn fetch_loose_files(
        &self,
        ctx: &EntryContent,
        files: &[&ModpackFile],
        previous: Option<&InstalledModpack>,
        patterns: &exclude::OverridePatterns,
        job: &Job<'_>,
    ) -> Vec<ModpackOverride> {
        let owned: HashMap<&str, &ModpackOverride> = previous
            .map(|p| p.overrides.iter().map(|o| (o.path.as_str(), o)).collect())
            .unwrap_or_default();
        let installing = previous.is_none();
        let mut named = HashSet::new();
        let mut wanted = Vec::new();
        for file in files {
            if patterns.excludes(&file.path) || !named.insert(file.path.as_str()) {
                continue;
            }
            let target = match materialize::safe_join(&ctx.data_dir, &file.path) {
                Ok(path) => path,
                Err(e) => {
                    tracing::warn!(path = %file.path, error = %e, "unsafe modpack file path");
                    continue;
                }
            };
            let writable = installing
                || match owned.get(file.path.as_str()) {
                    Some(previous) => modpack::ours(&ctx.data_dir, previous),
                    None => !target.exists(),
                };
            if writable {
                wanted.push((*file, target));
            }
        }

        let downloads: Vec<(&ModpackFile, Download)> = wanted
            .into_iter()
            .map(|(file, target)| {
                let download = Download {
                    artifact: file.artifact.clone(),
                    target,
                    label: file.path.clone(),
                    source: String::new(),
                };
                (file, download)
            })
            .collect();

        let mut out = Vec::new();
        for (file, fetched) in self
            .download_batch(downloads, ProvisionPhase::Overrides, job)
            .await
        {
            match fetched {
                Ok(sha1) => out.push(ModpackOverride {
                    path: file.path.clone(),
                    sha1,
                }),
                Err(e) => {
                    tracing::warn!(path = %file.path, error = %e, "cannot fetch a modpack file")
                }
            }
        }
        out
    }

    /// Fold the pack's items into the entry's content index, tagged with the
    /// pack's origin, and drop the previous pack's items that this one no
    /// longer ships. A user re-installing one of them by hand clears the tag and
    /// takes ownership, so a tagged item is only ever removed here. Returns the
    /// filenames recorded and the items that could not be placed.
    fn merge_pack_items(
        ctx: &EntryContent,
        pack: &InstalledModpack,
        items: Vec<InstalledContent>,
        previous: Option<&InstalledModpack>,
    ) -> Result<(Vec<String>, Vec<ContentFailure>)> {
        let origin = modpack::origin(pack);
        let mut pool = Pool::open(&ctx.entry_dir, &ctx.data_dir, ctx.worlds());
        let mut recorded = Vec::new();
        let mut failures = Vec::new();
        for mut item in items {
            item.origin = origin.clone();
            let item = match pool.holder_of(&item) {
                // A user-installed copy keeps its own (untagged) ownership:
                // the pack supplying the same file does not take it over.
                Some(old) => record::rehold(
                    &item,
                    record::Holding {
                        origin: match old.origin.is_empty() {
                            true => String::new(),
                            false => origin.clone(),
                        },
                        ..record::Holding::from(old)
                    },
                ),
                None => item,
            };
            match pool.upsert(item.clone()) {
                Ok(()) => recorded.push(item.filename),
                Err(e) => failures.push(ContentFailure {
                    item: item.filename,
                    title: item.title,
                    error: crate::error_info(e),
                }),
            }
        }
        if let Some(previous) = previous {
            let previous_origin = modpack::origin(previous);
            let keeping: HashSet<&str> = recorded.iter().map(String::as_str).collect();
            pool.drop_where(|i| {
                i.origin == previous_origin && !keeping.contains(i.filename.as_str())
            });
        }
        pool.commit()?;
        Ok((recorded, failures))
    }
}

fn filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn pack_item(file: Supplied, entry_dir: &Path) -> InstalledContent {
    let title = file.label();
    let Supplied {
        kind,
        filename,
        sha1,
        url,
        identity,
    } = file;
    let (project, release) = match identity {
        Some(identity) => (
            record::Project {
                kind,
                project_id: identity.project_id,
                slug: identity
                    .project
                    .as_ref()
                    .map(|p| p.slug.clone())
                    .unwrap_or_default(),
                title,
                icon_url: identity.project.map(|p| p.icon_url).unwrap_or_default(),
            },
            record::Release {
                source: identity.source,
                version_id: identity.version_id,
                version_number: identity.version_number,
                filename,
                sha1,
                url,
            },
        ),
        // The pack's source here would let `update` re-pin it to a project it never belonged to.
        None => (
            record::Project::untracked(
                kind,
                title,
                &entry_dir
                    .join(install::kind_dir(kind).unwrap_or_default())
                    .join(&filename),
            ),
            record::Release {
                url,
                ..record::Release::local(filename, sha1)
            },
        ),
    };
    record::assemble(project, release, record::Holding::fresh(&[]))
}

/// The kind a pack file's path names, when it is exactly one of the managed
/// load directories' own content files. A nested path under one of them (or
/// any other directory) is a game-directory file instead — the managed dirs
/// are flat — and so is a sidecar a mod wrote beside a pack.
pub(super) fn managed_kind(path: &str) -> Option<ContentKind> {
    let mut parts = path.split('/');
    let dir = parts.next()?;
    let file = parts.next()?;
    if file.is_empty() || parts.next().is_some() {
        return None;
    }
    let kind = match dir {
        "mods" => ContentKind::Mod,
        "plugins" => ContentKind::Plugin,
        "resourcepacks" => ContentKind::ResourcePack,
        "shaderpacks" => ContentKind::Shader,
        _ => return None,
    };
    install::is_content_file(kind, file).then_some(kind)
}

/// The previous pack's game-directory files that the new one no longer ships.
pub(super) fn stale_overrides(
    previous: &InstalledModpack,
    current: &InstalledModpack,
) -> Vec<ModpackOverride> {
    let keeping: HashSet<&str> = current.overrides.iter().map(|o| o.path.as_str()).collect();
    previous
        .overrides
        .iter()
        .filter(|o| !keeping.contains(o.path.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_load_dir_file_is_managed_content() {
        assert_eq!(managed_kind("mods/sodium.jar"), Some(ContentKind::Mod));
        assert_eq!(
            managed_kind("plugins/essentials.jar"),
            Some(ContentKind::Plugin)
        );
        assert_eq!(
            managed_kind("resourcepacks/cozy.zip"),
            Some(ContentKind::ResourcePack)
        );
        assert_eq!(
            managed_kind("shaderpacks/bsl.zip"),
            Some(ContentKind::Shader)
        );
    }

    #[test]
    fn a_sidecar_beside_a_pack_is_a_game_directory_file() {
        assert_eq!(managed_kind("resourcepacks/cozy.zip.rpo"), None);
        assert_eq!(managed_kind("shaderpacks/bsl.zip.txt"), None);
        assert_eq!(managed_kind("shaderpacks/error_logs.txt"), None);
        assert_eq!(managed_kind("resourcepacks/nested.mrpack"), None);
        assert_eq!(managed_kind("mods/readme.txt"), None);
    }

    #[test]
    fn anything_else_is_a_game_directory_file() {
        // The managed dirs are flat, so a nested path cannot be mirrored.
        assert_eq!(managed_kind("mods/1.21/sodium.jar"), None);
        assert_eq!(managed_kind("config/sodium.json"), None);
        assert_eq!(managed_kind("options.txt"), None);
        assert_eq!(managed_kind("mods"), None);
        assert_eq!(managed_kind("mods/"), None);
    }

    fn pack(overrides: &[&str]) -> InstalledModpack {
        InstalledModpack {
            overrides: overrides
                .iter()
                .map(|p| ModpackOverride {
                    path: p.to_string(),
                    sha1: "hash".to_string(),
                })
                .collect(),
            ..InstalledModpack::default()
        }
    }

    #[test]
    fn an_update_marks_only_the_files_the_new_pack_dropped() {
        let stale = stale_overrides(
            &pack(&["config/a.toml", "config/b.toml"]),
            &pack(&["config/a.toml", "config/c.toml"]),
        );
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].path, "config/b.toml");
    }
}
