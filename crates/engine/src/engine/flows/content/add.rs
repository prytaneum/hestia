//! Installing content into an entry: a platform project (with its required
//! dependencies), a source page URL, or a local file. A batch is per-item — a
//! selector that cannot be installed records a failure and the rest proceeds.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::content::{
    ContentAddItem, ContentAddSpec, ContentFailure, ContentKind, ContentProject, ContentVersion,
    DependencyKind, InstalledContent, LOCAL_SOURCE,
};
use proto::error::ErrorInfo;
use proto::minecraft::ProvisionPhase;

use super::super::download::Download;
use super::entry::{content_target, datapack_worlds, side_gate, EntryContent};
use super::phase_progress;
use crate::content::pool::Pool;
use crate::content::{inspect, install, record, Identified, LocalFile};
use crate::engine::Engine;
use crate::minecraft::materialize::{self, OnProgress};

/// One platform selector of a batch, resolved from its item: where it came
/// from (`given`, for failure reporting), which source serves it, and the
/// version pin.
struct PlatformRoot {
    given: String,
    source: String,
    project: String,
    pin: String,
}

/// A BFS node: a fetched project awaiting version resolution and install.
struct Node {
    given: String,
    source: String,
    pin: String,
    project: ContentProject,
}

impl Engine {
    pub(super) async fn add_content(
        &self,
        ctx: &EntryContent,
        spec: &ContentAddSpec,
        on_progress: OnProgress<'_>,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>)> {
        install::kind_dir(spec.kind)?;
        if spec.items.is_empty() {
            bail!(proto::error::ErrorInfo::NothingToDo {
                what: proto::error::Task::Install
            });
        }
        let worlds = datapack_worlds(ctx, spec)?;

        let mut failures = Vec::new();
        let mut roots = Vec::new();
        let mut files = Vec::new();
        for item in &spec.items {
            let picked = [&item.project, &item.url, &item.path]
                .iter()
                .filter(|s| !s.is_empty())
                .count();
            if picked != 1 {
                failures.push(failure(
                    item_label(item),
                    "",
                    ErrorInfo::MutuallyExclusive {
                        options: proto::error::Field::CONTENT_SOURCES.to_vec(),
                    },
                ));
                continue;
            }
            if !item.url.is_empty() {
                match self.content.parse_url(&item.url) {
                    Some((source, parsed)) => roots.push(PlatformRoot {
                        given: item.url.clone(),
                        source,
                        pin: parsed.version.unwrap_or_else(|| item.version.clone()),
                        project: parsed.project,
                    }),
                    None => failures.push(failure(
                        &item.url,
                        "",
                        ErrorInfo::UnsupportedContentUrl {
                            url: item.url.clone(),
                        },
                    )),
                }
            } else if !item.project.is_empty() {
                roots.push(PlatformRoot {
                    given: item.project.clone(),
                    source: match item.source.is_empty() {
                        true => spec.source.clone(),
                        false => item.source.clone(),
                    },
                    project: item.project.clone(),
                    pin: item.version.clone(),
                });
            } else {
                files.push(item);
            }
        }

        let mut items = Vec::new();
        let tracked_from = |item: &ContentAddItem| match item.source.is_empty() {
            true => spec.source.clone(),
            false => item.source.clone(),
        };
        // An untracked import is looked up too: a match is not recorded, but it
        // lends its icon to an archive that carries none.
        let local: Vec<Option<LocalFile>> = files
            .iter()
            .map(|item| LocalFile::read(Path::new(&item.path)).ok())
            .collect();
        let hashed: Vec<LocalFile> = local.iter().flatten().cloned().collect();
        if !hashed.is_empty() {
            on_progress.report(&phase_progress(ProvisionPhase::Resolving));
        }
        let identified = self.content.identify(&hashed).await;
        for (item, file) in files.into_iter().zip(&local) {
            let matches = file
                .as_ref()
                .and_then(|f| identified.get(&f.sha1))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let untracked = tracked_from(item) == LOCAL_SOURCE;
            let known = match untracked {
                true => None,
                false => chosen_match(matches, &tracked_from(item)),
            };
            if !untracked && !matches.is_empty() && known.is_none() {
                tracing::warn!(
                    file = %item.path,
                    source = %tracked_from(item),
                    "the chosen source does not recognise this file; importing it untracked"
                );
            }
            match add_file_content(ctx, spec.kind, item, &worlds, known, matches) {
                Ok(installed) => items.push(installed),
                Err(e) => failures.push(failure(&item.path, "", crate::error_info(e))),
            }
        }
        let (mut platform_items, mut platform_failures) = self
            .add_platform_content(ctx, spec.kind, roots, &worlds, on_progress)
            .await;
        items.append(&mut platform_items);
        failures.append(&mut platform_failures);

        let mut pool = Pool::open(&ctx.entry_dir, &ctx.data_dir, ctx.worlds());
        let mut recorded = Vec::new();
        for item in items {
            match pool.upsert(item.clone()) {
                Ok(()) => recorded.push(item),
                Err(e) => failures.push(failure(&item.filename, &item.title, crate::error_info(e))),
            }
        }
        pool.commit()?;
        let items = recorded;
        for item in &items {
            tracing::info!(
                entry = %ctx.entry_dir.display(),
                kind = ?item.kind,
                title = %item.title,
                filename = %item.filename,
                version = %item.version_number,
                "content installed"
            );
        }
        for fail in &failures {
            tracing::warn!(
                entry = %ctx.entry_dir.display(),
                item = %fail.item,
                error = %fail.error,
                "content install failed"
            );
        }
        Ok((items, failures))
    }

    /// Resolve every platform root (and, for mods, required dependencies —
    /// breadth-first under one visited set, so a dependency shared across the
    /// batch installs once) and download each pick into the managed directory.
    /// A node that fails records a per-item failure and the batch continues.
    async fn add_platform_content(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
        roots: Vec<PlatformRoot>,
        worlds: &[String],
        on_progress: OnProgress<'_>,
    ) -> (Vec<InstalledContent>, Vec<ContentFailure>) {
        let mut items = Vec::new();
        let mut failures = Vec::new();
        if roots.is_empty() {
            return (items, failures);
        }
        on_progress.report(&phase_progress(ProvisionPhase::Resolving));

        let mut visited: HashSet<String> = install::load(&ctx.entry_dir)
            .into_iter()
            .map(|i| i.project_id)
            .filter(|p| !p.is_empty())
            .collect();

        // An explicitly named root installs even when already present (a
        // reinstall/re-pin); only duplicates within the batch collapse.
        let mut queued = HashSet::new();
        let mut queue = Vec::new();
        for root in roots {
            let project = match self
                .content
                .project(&root.source, &root.project, Some(kind))
                .await
            {
                Ok(project) => project,
                Err(e) => {
                    failures.push(failure(&root.given, "", crate::error_info(e)));
                    continue;
                }
            };
            if !kind_matches(kind, project.kind) {
                failures.push(failure(
                    &root.given,
                    &project.title,
                    ErrorInfo::ContentKindMismatch {
                        title: project.title.clone(),
                        actual: project.kind,
                        expected: kind,
                    },
                ));
                continue;
            }
            if let Err(e) = side_gate(kind, &project, ctx.side) {
                failures.push(failure(&root.given, &project.title, crate::error_info(e)));
                continue;
            }
            if !queued.insert(project.id.clone()) {
                continue;
            }
            visited.insert(project.id.clone());
            queue.push(Node {
                given: root.given,
                source: root.source,
                pin: root.pin,
                project,
            });
        }

        let mut planned = Vec::new();
        while let Some(node) = queue.pop() {
            if on_progress.check().is_err() {
                break;
            }
            let version = match self
                .target_version(ctx, kind, &node.source, &node.project.id, "", &node.pin)
                .await
            {
                Ok(version) => version,
                Err(e) => {
                    failures.push(failure(
                        &node.given,
                        &node.project.title,
                        crate::error_info(e),
                    ));
                    continue;
                }
            };

            if kind == ContentKind::Mod {
                for dep in &version.dependencies {
                    if dep.kind != DependencyKind::Required {
                        continue;
                    }
                    if dep.project_id.is_empty() {
                        tracing::warn!(
                            of = %node.project.title,
                            version_id = %dep.version_id,
                            "required dependency names no project; skipping"
                        );
                        continue;
                    }
                    if !visited.insert(dep.project_id.clone()) {
                        continue;
                    }
                    let dep_project = match self
                        .content
                        .project(&node.source, &dep.project_id, Some(kind))
                        .await
                    {
                        Ok(project) => project,
                        Err(e) => {
                            failures.push(failure(&dep.project_id, "", crate::error_info(e)));
                            continue;
                        }
                    };
                    if side_gate(kind, &dep_project, ctx.side).is_err() {
                        tracing::warn!(
                            dependency = %dep_project.title,
                            of = %node.project.title,
                            "required dependency does not support this side; skipping"
                        );
                        continue;
                    }
                    queue.push(Node {
                        given: dep_project.slug.clone(),
                        source: node.source.clone(),
                        pin: String::new(),
                        project: dep_project,
                    });
                }
            }

            planned.push((node, version));
        }

        let mut downloads = Vec::new();
        for (node, version) in planned {
            match plan_release(ctx, kind, &node.project.title, &version) {
                Ok(download) => downloads.push(((node, version), download)),
                Err(e) => failures.push(failure(
                    &node.given,
                    &node.project.title,
                    crate::error_info(e),
                )),
            }
        }
        let outcomes = self
            .download_batch(downloads, ProvisionPhase::Content, on_progress)
            .await;
        for ((node, version), fetched) in outcomes {
            match fetched.and_then(|sha1| release_of(&version, sha1)) {
                Ok(release) => items.push(record::assemble(
                    record::Project {
                        kind,
                        project_id: node.project.id.clone(),
                        slug: node.project.slug.clone(),
                        title: node.project.title.clone(),
                        icon_url: node.project.icon_url.clone(),
                    },
                    release,
                    record::Holding::fresh(worlds),
                )),
                Err(e) => failures.push(failure(
                    &node.given,
                    &node.project.title,
                    crate::error_info(e),
                )),
            }
        }
        (items, failures)
    }

    /// One version's file, fetched on its own. `kind` is the *requested* kind,
    /// not the project's: Modrinth types datapacks as mod projects, so
    /// `project.kind` would route a datapack into `mods/`.
    pub(super) async fn fetch_release(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
        title: &str,
        version: &ContentVersion,
        on_progress: OnProgress<'_>,
    ) -> Result<record::Release> {
        let download = plan_release(ctx, kind, title, version)?;
        let sha1 = self
            .download(&download, ProvisionPhase::Content, on_progress)
            .await?;
        release_of(version, sha1)
    }
}

pub(super) fn plan_release(
    ctx: &EntryContent,
    kind: ContentKind,
    title: &str,
    version: &ContentVersion,
) -> Result<Download> {
    let artifact = install::primary_file(version)?.artifact.clone();
    materialize::validate_filename(&artifact.filename)?;
    Ok(Download {
        target: content_target(ctx, kind, &artifact.filename)?,
        artifact,
        label: title.to_string(),
        source: version.source.clone(),
    })
}

pub(super) fn release_of(version: &ContentVersion, sha1: String) -> Result<record::Release> {
    let artifact = &install::primary_file(version)?.artifact;
    Ok(record::Release {
        source: version.source.clone(),
        version_id: version.id.clone(),
        version_number: version.version_number.clone(),
        filename: artifact.filename.clone(),
        sha1,
        url: artifact.url.clone(),
    })
}

/// Import a local file: copy it into the managed directory and mirror it into
/// the game's load dirs.
fn add_file_content(
    ctx: &EntryContent,
    kind: ContentKind,
    item: &ContentAddItem,
    worlds: &[String],
    known: Option<&Identified>,
    recognised: &[Identified],
) -> Result<InstalledContent> {
    let source = Path::new(&item.path);
    let filename = if !item.filename.is_empty() {
        item.filename.clone()
    } else if let Some(known) = known {
        known.file.artifact.filename.clone()
    } else {
        file_name(source)
    };
    let (managed, sha1) = import_file(ctx, kind, source, &filename)?;
    let (project, release) = match known {
        Some(known) => known.recorded(kind, filename, sha1),
        None => {
            let mut project = record::Project::untracked(kind, filename.clone(), &managed);
            if project.icon_url.is_empty() {
                project.icon_url = borrowed_icon(recognised);
            }
            (project, record::Release::local(filename, sha1))
        }
    };
    Ok(record::assemble(
        project,
        release,
        record::Holding::fresh(worlds),
    ))
}

/// Copy a local file into the managed directory as `filename`, returning the
/// managed path and its sha1. The kind it is filed under is the caller's: only
/// an unreadable archive or a modpack is refused.
pub(super) fn import_file(
    ctx: &EntryContent,
    kind: ContentKind,
    source: &Path,
    filename: &str,
) -> Result<(PathBuf, String)> {
    if !source.is_file() {
        bail!("no file at {}", source.display());
    }
    materialize::validate_filename(filename)?;
    match inspect::classify(source)? {
        inspect::Detected::Kind(detected) if detected != kind => tracing::warn!(
            file = %filename,
            ?detected,
            requested = ?kind,
            "importing a local file under a kind that differs from its detected type"
        ),
        _ => {}
    }
    let managed = content_target(ctx, kind, filename)?;
    if let Some(parent) = managed.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    // Copying a file onto itself truncates it to nothing.
    if !same_file(source, &managed) {
        std::fs::copy(source, &managed)
            .with_context(|| format!("cannot import {}", source.display()))?;
    }
    let sha1 = install::sha1_file(&managed)?;
    Ok((managed, sha1))
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The icon of a project a source knows this file as, for an import that stays
/// untracked — a shader pack or a plugin has no icon of its own to read.
fn borrowed_icon(recognised: &[Identified]) -> String {
    recognised
        .iter()
        .map(|identified| &identified.project.icon_url)
        .find(|icon| !icon.is_empty())
        .cloned()
        .unwrap_or_default()
}

fn chosen_match<'a>(matches: &'a [Identified], source: &str) -> Option<&'a Identified> {
    match source.is_empty() {
        true => matches.first(),
        false => matches.iter().find(|m| m.version.source == source),
    }
}

/// Whether a fetched project satisfies the requested kind. Datapacks accept
/// `Mod` projects: Modrinth has no datapack project type — a datapack is a
/// mod-typed project whose versions carry the `datapack` loader.
fn kind_matches(requested: ContentKind, project: ContentKind) -> bool {
    requested == project || (requested == ContentKind::DataPack && project == ContentKind::Mod)
}

pub(super) fn failure(
    item: impl Into<String>,
    title: impl Into<String>,
    error: ErrorInfo,
) -> ContentFailure {
    ContentFailure {
        item: item.into(),
        title: title.into(),
        error,
    }
}

/// The selector an item names, for failure reporting on malformed items.
fn item_label(item: &ContentAddItem) -> String {
    [&item.project, &item.url, &item.path]
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "(empty item)".to_string())
}
