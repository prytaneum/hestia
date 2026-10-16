//! Turning a pack *reference* into something installable: which archive it is,
//! which flavor it needs, and what the catalogue knows about the files inside.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use proto::content::{
    ContentKind, ContentProject, ContentVersion, ModpackFile, ReleaseChannel, ResolvedModpack,
    VersionQuery, LOCAL_SOURCE,
};
use proto::error::{ArchiveFault, EntryKind, ErrorInfo};
use proto::minecraft::{Artifact, ProvisionPhase};
use proto::modpack::{InstalledModpack, ModpackMove, ModpackRef};
use uuid::Uuid;

use super::super::content::entry::{content_target, EntryContent, EntrySide};
use super::super::phase_progress;
use super::{pack_display_name, same_pack, FileIdentity, Supplied};
use crate::cancel::Job;
use crate::content::provider::FileRef;
use crate::content::{install, pack, LocalFile};
use crate::engine::Engine;
use crate::minecraft::materialize;

const STAGING: &str = "tmp";

/// A pack ready to install: its manifest, the project detail it has when it came
/// from a catalogue, and the archive its `overrides/` live in.
pub(super) type FetchedPack = (ResolvedModpack, Option<ContentProject>, pack::Archive);

pub(super) struct Located {
    source: &'static str,
    file: FileRef,
}

/// What the catalogue knows about the pack's files, keyed by source since
/// sources share no ids. Empty where one could not be reached.
#[derive(Default)]
pub(super) struct Catalogue {
    projects: HashMap<(&'static str, String), ContentProject>,
    /// Version id → version number, so a pack's mod lists a version like every
    /// other installed item.
    versions: HashMap<(&'static str, String), String>,
}

impl Catalogue {
    pub(super) fn identity(&self, located: Located) -> FileIdentity {
        let Located { source, file } = located;
        FileIdentity {
            source: source.to_string(),
            project: self
                .projects
                .get(&(source, file.project_id.clone()))
                .cloned(),
            version_number: self
                .versions
                .get(&(source, file.version_id.clone()))
                .cloned()
                .unwrap_or_default(),
            project_id: file.project_id,
            version_id: file.version_id,
        }
    }
}

/// `versions` is newest-first; a pre-release installed ahead of the newest release is kept.
pub(super) fn update_target<'a>(
    versions: &'a [ContentVersion],
    installed: &str,
) -> Option<&'a ContentVersion> {
    let newest = versions
        .iter()
        .find(|v| v.channel == ReleaseChannel::Release)
        .or_else(|| versions.first())?;
    let ahead = versions
        .iter()
        .take_while(|v| v.id != newest.id)
        .find(|v| v.id == installed);
    Some(ahead.unwrap_or(newest))
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
    let mut candidates = versions[..reach].iter().filter(targets);
    candidates
        .clone()
        .find(|v| v.channel == ReleaseChannel::Release)
        .or_else(|| candidates.next())
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
            entry: match side {
                EntrySide::Server => EntryKind::Server,
                EntrySide::Client => EntryKind::Instance,
            },
            flavor: ctx.flavor.clone(),
            game_version: ctx.game_version.clone(),
            pack_flavor,
            pack_game_version: resolved.game_version.clone(),
        })
    }

    /// The flavor a pack's loader names. The registry is the table: a pack's
    /// loader name *is* hestia's flavor id where the flavor exists, so adding a
    /// flavor needs no edit here — and a pack pinning one that does not exist is
    /// refused by name rather than silently installed as vanilla.
    pub(super) fn pack_flavor(
        &self,
        resolved: &ResolvedModpack,
        side: EntrySide,
    ) -> Result<String> {
        let flavors = self.minecraft.flavors(side.side());
        let wanted = resolved
            .loader
            .as_deref()
            .filter(|l| !l.is_empty())
            .unwrap_or("vanilla");
        match flavors.iter().any(|f| f.id == wanted) {
            true => Ok(wanted.to_string()),
            false => bail!(ErrorInfo::ModpackLoaderUnsupported {
                loader: wanted.to_string()
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
                let (source, parsed) = self.content.parse_url(&reference.url).ok_or_else(|| {
                    ErrorInfo::UnsupportedContentUrl {
                        url: reference.url.clone(),
                    }
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
        let mut archive = self.download_pack(artifact, job).await?;
        let mut resolved = self.pack_manifest(&mut archive).await.map_err(|e| {
            match e.downcast_ref::<ErrorInfo>() {
                Some(ErrorInfo::ModpackInvalid {
                    reason: ArchiveFault::ManifestMissing,
                }) => ErrorInfo::NotAModpack {
                    reference: version.id.clone(),
                }
                .into(),
                _ => e,
            }
        })?;
        resolved.source = version.source;
        resolved.project_id = version.project_id;
        resolved.version_id = version.id;
        resolved.version_number = version.version_number;
        Ok((resolved, Some(project), archive))
    }

    async fn download_pack(&self, artifact: &Artifact, job: &Job<'_>) -> Result<pack::Archive> {
        let staged = self
            .pack_staging()
            .join(format!("{}.pack", Uuid::now_v7().simple()));
        let downloaded = materialize::ensure_artifact(
            Some(&self.cache),
            artifact,
            &staged,
            ProvisionPhase::Content,
            job,
        )
        .await;
        let archive = downloaded.and_then(|()| pack::Archive::read(&staged));
        if let Err(e) = std::fs::remove_file(&staged) {
            tracing::debug!(path = %staged.display(), error = %e, "no modpack staging file to remove");
        }
        archive
    }

    pub(crate) fn pack_staging(&self) -> PathBuf {
        self.data_home().join(STAGING)
    }

    async fn pack_manifest(&self, archive: &mut pack::Archive) -> Result<ResolvedModpack> {
        Ok(match archive.platform_manifest()? {
            Some((source, manifest)) => self.content.resolve_manifest(source, &manifest).await?,
            None => ResolvedModpack {
                source: LOCAL_SOURCE.to_string(),
                ..archive.index()?
            },
        })
    }

    pub(super) async fn read_pack_file(&self, path: &Path) -> Result<FetchedPack> {
        let mut archive = pack::Archive::read(path)?;
        let mut resolved = self.pack_manifest(&mut archive).await?;
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

    /// Which platform file each managed pack file is, from its download URL.
    pub(super) fn identify<'a>(
        &self,
        files: &[(ContentKind, &'a ModpackFile)],
    ) -> HashMap<&'a str, Located> {
        let mut out = HashMap::new();
        for (_, file) in files {
            if let Some((source, file_ref)) = self.content.parse_file_url(&file.artifact.url) {
                out.insert(
                    file.path.as_str(),
                    Located {
                        source,
                        file: file_ref,
                    },
                );
            }
        }
        out
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

    /// Two bulk lookups for everything the pack's files belong to — versions by
    /// id for the version *numbers*, projects by id for the titles and icons —
    /// so the pool reads like ordinary installed content rather than a hundred
    /// bare filenames. Best-effort in both halves: a pack still installs when
    /// the catalogue is unreachable, it just reads less well.
    ///
    /// Versions go first: a source whose URLs name only the file (CurseForge)
    /// gets its `refs` completed from the answer before the projects are asked.
    pub(super) async fn hydrate(&self, refs: &mut HashMap<&str, Located>) -> Catalogue {
        let sources: BTreeSet<&'static str> = refs.values().map(|r| r.source).collect();
        let mut catalogue = Catalogue::default();
        for source in sources {
            let mut wanted: Vec<String> = refs
                .values()
                .filter(|r| r.source == source)
                .map(|r| r.file.version_id.clone())
                .filter(|id| !id.is_empty())
                .collect();
            wanted.sort();
            wanted.dedup();
            let found = match self.content.versions_by_id(source, &wanted).await {
                Ok(found) => found,
                Err(e) => {
                    tracing::warn!(source, error = %e, count = wanted.len(), "cannot look up modpack versions");
                    Vec::new()
                }
            };
            for reference in refs.values_mut().filter(|r| r.source == source) {
                if !reference.file.project_id.is_empty() {
                    continue;
                }
                if let Some(version) = found.iter().find(|v| v.id == reference.file.version_id) {
                    reference.file.project_id = version.project_id.clone();
                }
            }

            let mut projects: Vec<String> = refs
                .values()
                .filter(|r| r.source == source)
                .map(|r| r.file.project_id.clone())
                .filter(|id| !id.is_empty())
                .collect();
            projects.sort();
            projects.dedup();
            if !projects.is_empty() {
                match self.content.projects(source, &projects).await {
                    Ok(found) => catalogue
                        .projects
                        .extend(found.into_iter().map(|p| ((source, p.id.clone()), p))),
                    Err(e) => {
                        tracing::warn!(source, error = %e, count = projects.len(), "cannot look up modpack projects");
                    }
                }
            }
            catalogue.versions.extend(
                found
                    .into_iter()
                    .map(|v| ((source, v.id), v.version_number)),
            );
        }
        catalogue
    }
}
