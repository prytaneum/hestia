//! Turning a pack *reference* into something installable: which archive it is,
//! which flavor it needs, and what the catalogue knows about the files inside.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use proto::content::{
    ContentKind, ContentProject, ContentVersion, ResolvedModpack, VersionQuery, LOCAL_SOURCE,
};
use proto::error::ErrorInfo;
use proto::minecraft::{Artifact, ConfigEntry, ProvisionPhase};
use proto::modpack::{InstalledModpack, ModpackMove, ModpackRef};
use proto::provider::pack::PackRead;
use uuid::Uuid;

use super::super::content::entry::{content_target, EntryContent};
use super::super::phase_progress;
use super::{pack_display_name, same_pack, FileIdentity, Supplied};
use crate::cancel::Job;
use crate::content::{install, pack, LocalFile};
use crate::engine::Engine;
use crate::jvm::{JVM_ARGS_KEY, MEMORY_KEY};
use crate::minecraft::{materialize, Side};
use crate::plugins::PluginPack;
use crate::transfer::archive::Reader;
use crate::transfer::{Detected, Kind};

const STAGING: &str = "tmp";

/// A pack ready to install: its manifest, the project detail it has when it came
/// from a catalogue, and the archive its `overrides/` live in.
pub(crate) type FetchedPack = (ResolvedModpack, Option<ContentProject>, pack::Archive);

/// `versions` is newest-first; a pre-release installed ahead of the newest release is kept.
pub(super) fn update_target<'a>(
    versions: &'a [ContentVersion],
    installed: &str,
) -> Option<&'a ContentVersion> {
    install::pick_newest(versions, |_| true, installed)
}

/// A pack already on `game_version` is only offered what was published after the version it runs.
pub(super) fn fitting<'a>(
    versions: &'a [ContentVersion],
    installed: &str,
    game_version: &str,
) -> Option<&'a ContentVersion> {
    let targets = |v: &&ContentVersion| v.game_versions.iter().any(|g| g == game_version);
    let reach = versions
        .iter()
        .position(|v| v.id == installed && targets(&v))
        .unwrap_or(versions.len());
    install::pick_newest(&versions[..reach], |v| targets(&v), "")
}

impl Engine {
    /// A pack can only be installed into an entry that already runs what it
    /// pins: both are baked into the entry's resolved profile, and neither can
    /// be changed in place (a version update stays within one flavor).
    pub(super) fn ensure_entry_matches(
        &self,
        ctx: &EntryContent,
        resolved: &ResolvedModpack,
    ) -> Result<()> {
        let side = ctx.side;
        let pack_flavor = self.pack_flavor(resolved, side)?;
        if pack_flavor == ctx.flavor && resolved.game_version == ctx.game_version {
            return Ok(());
        }
        bail!(ErrorInfo::ModpackEntryMismatch {
            entry: side.into(),
            flavor: ctx.flavor.clone(),
            game_version: ctx.game_version.clone(),
            pack_flavor,
            pack_game_version: resolved.game_version.clone(),
        })
    }

    /// The flavor a loader name stands for. The registry is the table: a
    /// loader name is hestia's flavor id where the flavor exists, else the
    /// loader a flavor declares, so adding a flavor needs no edit here. `None` is a loader no flavor answers to, for
    /// the caller to refuse by name rather than silently install as vanilla.
    pub(in crate::engine) fn supported_flavor(&self, side: Side, loader: &str) -> Option<String> {
        let wanted = match loader.is_empty() {
            true => "vanilla",
            false => loader,
        };
        self.minecraft.flavor_of_loader(side, wanted)
    }

    pub(super) fn pack_flavor(&self, resolved: &ResolvedModpack, side: Side) -> Result<String> {
        let loader = resolved.loader.as_deref().unwrap_or_default();
        match self.supported_flavor(side, loader) {
            Some(flavor) => Ok(flavor),
            None => bail!(ErrorInfo::ModpackLoaderUnsupported {
                loader: loader.to_string()
            }),
        }
    }

    /// Resolve a pack reference — a project, a source page URL, or a local
    /// `.mrpack` — into everything installing needs.
    pub(super) async fn fetch_pack(
        &self,
        reference: &ModpackRef,
        installed: &str,
        job: &Job<'_>,
    ) -> Result<FetchedPack> {
        let picked = [&reference.project, &reference.url, &reference.path]
            .iter()
            .filter(|s| !s.is_empty())
            .count();
        if picked != 1 {
            bail!(ErrorInfo::MutuallyExclusive {
                options: proto::error::Field::CONTENT_SOURCES.to_vec(),
            });
        }
        job.report(&phase_progress(ProvisionPhase::Resolving));

        if !reference.path.is_empty() {
            return self.read_pack_file(Path::new(&reference.path)).await;
        }

        let (source, project_ref, pin) = match reference.url.is_empty() {
            false => {
                let (source, parsed) =
                    self.content
                        .parse_url(&reference.url)
                        .await
                        .ok_or_else(|| ErrorInfo::UnsupportedContentUrl {
                            url: reference.url.clone(),
                        })?;
                let pin = parsed.version.unwrap_or_else(|| reference.version.clone());
                (source, parsed.project, pin)
            }
            true => (
                reference.source.clone(),
                reference.project.clone(),
                reference.version.clone(),
            ),
        };

        let project = self
            .content
            .project(&source, &project_ref, Some(ContentKind::Modpack))
            .await?;
        if !project.kinds.contains(&ContentKind::Modpack) && project.kind != ContentKind::Modpack {
            bail!(ErrorInfo::ContentKindMismatch {
                title: project.title.clone(),
                actual: project.kind,
                expected: ContentKind::Modpack,
            });
        }
        let version = self
            .pick_pack_version(&source, &project.id, &pin, installed)
            .await?;
        let artifact = &install::primary_file(&version)?.artifact;
        if artifact.url.is_empty() {
            bail!(ErrorInfo::ContentDownloadBlocked {
                title: project.title.clone(),
                source: version.source.clone(),
            });
        }
        let (archive, mut resolved) = self
            .download_pack(&version.source, artifact, job)
            .await
            .map_err(|e| match e.downcast_ref::<ErrorInfo>() {
                Some(ErrorInfo::ArchiveUnrecognised { .. }) => ErrorInfo::NotAModpack {
                    reference: version.id.clone(),
                }
                .into(),
                _ => e,
            })?;
        resolved.source = version.source;
        resolved.project_id = version.project_id;
        resolved.version_id = version.id;
        resolved.version_number = version.version_number;
        Ok((resolved, Some(project), archive))
    }

    async fn download_pack(
        &self,
        source: &str,
        artifact: &Artifact,
        job: &Job<'_>,
    ) -> Result<(pack::Archive, ResolvedModpack)> {
        let staged = self
            .pack_staging()
            .join(format!("{}.pack", Uuid::now_v7().simple()));
        let policy = self.content.download_policy(source)?;
        let downloaded = materialize::ensure_artifact(
            Some(&self.cache),
            policy,
            artifact,
            &staged,
            ProvisionPhase::Content,
            None,
            job,
        )
        .await;
        let opened = match downloaded {
            Ok(()) => self.open_pack_file(&staged).await,
            Err(e) => Err(e),
        };
        if let Err(e) = std::fs::remove_file(&staged) {
            tracing::debug!(path = %staged.display(), error = %e, "no modpack staging file to remove");
        }
        opened
    }

    pub(crate) fn pack_staging(&self) -> PathBuf {
        self.data_home().join(STAGING)
    }

    /// Opens a zip that is a pack and reads what it holds: the launcher's own
    /// `.mrpack`, or whichever pack format a plugin contributes.
    pub(crate) async fn open_pack_file(
        &self,
        path: &Path,
    ) -> Result<(pack::Archive, ResolvedModpack)> {
        let reader = Reader::open(path)?;
        let detected = self.detect_archive(&reader, path, true).await?;
        self.open_pack(reader, detected).await
    }

    /// Reads a detected pack archive. A plugin's pack names its files itself,
    /// or hands a manifest to the content source that owns it, and the files
    /// are fetched under the policy of whichever vetted them.
    pub(crate) async fn open_pack(
        &self,
        mut reader: Reader,
        detected: Detected,
    ) -> Result<(pack::Archive, ResolvedModpack)> {
        match detected.kind {
            Kind::Mrpack => {
                let mut archive = pack::Archive::mrpack(reader, detected.prefix);
                let mut resolved = ResolvedModpack {
                    source: LOCAL_SOURCE.to_string(),
                    ..archive.index(&self.minecraft.loaders())?
                };
                self.content.recognise_files(&mut resolved.files).await;
                Ok((archive, resolved))
            }
            Kind::Plugin(plugin) => {
                let read = plugin.read(&mut reader, &detected.prefix).await?;
                let trees = read.overrides.clone();
                let config = [(MEMORY_KEY, &read.memory), (JVM_ARGS_KEY, &read.jvm_args)]
                    .into_iter()
                    .filter_map(|(key, value)| {
                        value.as_ref().map(|value| ConfigEntry {
                            key: key.to_string(),
                            value: value.clone(),
                        })
                    })
                    .collect();
                let (mut resolved, policy) = self.resolve_read(&plugin, read).await?;
                let loaders = self.minecraft.loaders();
                if let Some(flavor) = resolved
                    .loader
                    .as_deref()
                    .and_then(|key| loaders.flavor_of(plugin.id(), key))
                {
                    resolved.loader = Some(flavor.to_string());
                }
                Ok((
                    pack::Archive::contributed(reader, detected.prefix, trees, policy, config),
                    resolved,
                ))
            }
            Kind::Instance(_) => Err(ErrorInfo::ArchiveUnrecognised {
                filename: String::new(),
            }
            .into()),
        }
    }

    async fn resolve_read(
        &self,
        plugin: &Arc<PluginPack>,
        read: PackRead,
    ) -> Result<(ResolvedModpack, Option<Arc<crate::plugins::vouch::Policy>>)> {
        let Some(delegate) = read.resolve else {
            let mut resolved = ResolvedModpack {
                source: LOCAL_SOURCE.to_string(),
                ..read.pack
            };
            self.content.recognise_files(&mut resolved.files).await;
            return Ok((resolved, Some(plugin.policy())));
        };
        let theirs = self
            .content
            .resolve_manifest(&delegate.source, &delegate.manifest)
            .await?;
        let policy = self.content.download_policy(&theirs.source)?;
        let own = read.pack;
        let resolved = ResolvedModpack {
            source: theirs.source,
            project_id: theirs.project_id,
            version_id: theirs.version_id,
            version_number: first_filled(own.version_number, theirs.version_number),
            name: first_filled(own.name, theirs.name),
            summary: first_filled(own.summary, theirs.summary),
            game_version: first_filled(own.game_version, theirs.game_version),
            loader: own.loader.or(theirs.loader),
            loader_version: own.loader_version.or(theirs.loader_version),
            files: theirs.files,
        };
        Ok((resolved, policy))
    }

    pub(super) async fn read_pack_file(&self, path: &Path) -> Result<FetchedPack> {
        let (archive, resolved) = self.open_pack_file(path).await?;
        self.read_pack(archive, resolved, path).await
    }

    pub(crate) async fn read_pack(
        &self,
        archive: pack::Archive,
        mut resolved: ResolvedModpack,
        path: &Path,
    ) -> Result<FetchedPack> {
        let project = self.published_pack(path, &mut resolved).await;
        if resolved.name.is_empty() {
            resolved.name = match &project {
                Some(project) => project.title.clone(),
                None => path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            };
        }
        Ok((resolved, project, archive))
    }

    /// An archive no source published keeps no project, which is what leaves it without an update path.
    async fn published_pack(
        &self,
        path: &Path,
        resolved: &mut ResolvedModpack,
    ) -> Option<ContentProject> {
        let file = match LocalFile::read(path) {
            Ok(file) => file,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot hash a modpack archive");
                return None;
            }
        };
        let known = self
            .content
            .identify(std::slice::from_ref(&file))
            .await
            .remove(&file.sha1)?
            .into_iter()
            .next()?;
        tracing::info!(
            source = %known.version.source,
            project = %known.project.id,
            version = %known.version.version_number,
            "local modpack recognised"
        );
        resolved.source = known.version.source;
        resolved.project_id = known.project.id.clone();
        resolved.version_id = known.version.id;
        resolved.version_number = known.version.version_number;
        Some(known.project)
    }

    /// The pack an update moves to: the same project at another (or the newest)
    /// published version.
    pub(super) async fn fetch_update(
        &self,
        current: &InstalledModpack,
        to: &ModpackMove,
        job: &Job<'_>,
    ) -> Result<FetchedPack> {
        if !to.path.is_empty() {
            if !to.version.is_empty() {
                bail!(ErrorInfo::MutuallyExclusive {
                    options: vec![proto::error::Field::Version, proto::error::Field::Path],
                });
            }
            job.report(&phase_progress(ProvisionPhase::Resolving));
            let fetched = self.read_pack_file(Path::new(&to.path)).await?;
            if !to.allow_other_pack && !same_pack(current, &fetched) {
                bail!(ErrorInfo::ModpackDifferentPack {
                    installed: current.name.clone(),
                    given: pack_display_name(&fetched.0, fetched.1.as_ref()),
                });
            }
            return Ok(fetched);
        }
        if current.project_id.is_empty() {
            bail!(ErrorInfo::UnsupportedOperation {
                reason: proto::error::Unsupported::ModpackNotUpdatable
            });
        }
        self.fetch_pack(
            &ModpackRef {
                source: current.source.clone(),
                project: current.project_id.clone(),
                version: to.version.clone(),
                ..ModpackRef::default()
            },
            &current.version_id,
            job,
        )
        .await
    }

    /// Unfiltered by game version: a pack states its game version rather than being selected for one.
    pub(super) async fn pack_versions(
        &self,
        source: &str,
        project: &str,
    ) -> Result<Vec<ContentVersion>> {
        self.content
            .versions(&VersionQuery {
                source: source.to_string(),
                project: project.to_string(),
                ..VersionQuery::default()
            })
            .await
    }

    /// `installed` is empty for a fresh install.
    pub(super) async fn pick_pack_version(
        &self,
        source: &str,
        project: &str,
        pin: &str,
        installed: &str,
    ) -> Result<ContentVersion> {
        let versions = self.pack_versions(source, project).await?;
        let picked: Option<&ContentVersion> = match pin.is_empty() {
            false => versions
                .iter()
                .find(|v| v.id == pin || v.version_number == pin),
            true => update_target(&versions, installed),
        };
        // A paged listing may not carry the pin a URL named; ask for it by id.
        if picked.is_none() && !pin.is_empty() {
            let pinned = [pin.to_string()];
            if let Ok(found) = self.content.versions_by_id(source, &pinned).await {
                if let Some(version) = found.into_iter().next() {
                    return Ok(version);
                }
            }
        }
        picked.cloned().ok_or_else(|| {
            ErrorInfo::VersionNotFound {
                reference: match pin.is_empty() {
                    true => project.to_string(),
                    false => pin.to_string(),
                },
            }
            .into()
        })
    }

    /// Where several sources publish a file the pack's own wins, keeping its pool on the source it updates from.
    pub(super) async fn recognise(
        &self,
        ctx: &EntryContent,
        pack_source: &str,
        supplied: &mut [Supplied],
    ) {
        let unknown: Vec<LocalFile> = supplied
            .iter()
            .filter(|file| file.identity.is_none() && !file.sha1.is_empty())
            .filter_map(|file| {
                Some(LocalFile {
                    path: content_target(ctx, file.kind, &file.filename).ok()?,
                    sha1: file.sha1.clone(),
                })
            })
            .collect();
        let found = self.content.identify(&unknown).await;
        for file in supplied.iter_mut().filter(|file| file.identity.is_none()) {
            let Some(matches) = found.get(&file.sha1) else {
                continue;
            };
            let Some(known) = matches
                .iter()
                .find(|m| m.version.source == pack_source)
                .or_else(|| matches.first())
            else {
                continue;
            };
            file.url = known.file.artifact.url.clone();
            file.identity = Some(FileIdentity {
                source: known.version.source.clone(),
                project_id: known.project.id.clone(),
                version_id: known.version.id.clone(),
                version_number: known.version.version_number.clone(),
                project: Some(known.project.clone()),
            });
        }
    }

    /// Bulk lookups for the version numbers and projects the pack did not
    /// already carry, so the pool reads like ordinary installed content.
    /// Best-effort: a pack still installs when the catalogue is unreachable.
    pub(super) async fn hydrate(&self, identities: &mut [&mut FileIdentity]) {
        let sources: BTreeSet<String> = identities.iter().map(|i| i.source.clone()).collect();
        for source in &sources {
            let versions = wanted(
                identities
                    .iter()
                    .filter(|i| &i.source == source && i.version_number.is_empty())
                    .map(|i| i.version_id.clone()),
            );
            let versions = match versions.is_empty() {
                true => Vec::new(),
                false => self
                    .content
                    .versions_by_id(source, &versions)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(source, error = %e, count = versions.len(), "cannot look up modpack versions");
                        Vec::new()
                    }),
            };
            let projects = wanted(
                identities
                    .iter()
                    .filter(|i| &i.source == source && i.project.is_none())
                    .map(|i| i.project_id.clone()),
            );
            let projects = match projects.is_empty() {
                true => Vec::new(),
                false => self
                    .content
                    .projects(source, &projects)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(source, error = %e, count = projects.len(), "cannot look up modpack projects");
                        Vec::new()
                    }),
            };
            for identity in identities.iter_mut().filter(|i| &i.source == source) {
                if identity.version_number.is_empty() {
                    if let Some(version) = versions.iter().find(|v| v.id == identity.version_id) {
                        identity.version_number = version.version_number.clone();
                    }
                }
                if identity.project.is_none() {
                    identity.project = projects
                        .iter()
                        .find(|p| p.id == identity.project_id)
                        .cloned();
                }
            }
        }
    }
}

fn first_filled(own: String, theirs: String) -> String {
    match own.is_empty() {
        true => theirs,
        false => own,
    }
}

fn wanted(ids: impl Iterator<Item = String>) -> Vec<String> {
    let mut ids: Vec<String> = ids.filter(|id| !id.is_empty()).collect();
    ids.sort();
    ids.dedup();
    ids
}
