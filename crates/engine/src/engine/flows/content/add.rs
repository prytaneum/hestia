//! Installing content into an entry: a platform project (with its required
//! dependencies), a source page URL, or a local file. A batch is per-item — a
//! selector that cannot be installed records a failure and the rest proceeds.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use proto::content::{
    ContentAddItem, ContentAddSpec, ContentFailure, ContentKind, ContentProject, DependencyKind,
    InstalledContent, VersionQuery,
};
use proto::error::ErrorInfo;
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

use super::entry::{content_loader, content_target, datapack_worlds, side_gate, EntryContent};
use super::phase_progress;
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
                        options: vec!["a project".into(), "a url".into(), "a file".into()],
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
        // A path that cannot be hashed is left for the import itself to refuse,
        // which names why.
        let local: Vec<Option<LocalFile>> = files
            .iter()
            .map(|item| LocalFile::read(Path::new(&item.path)).ok())
            .collect();
        let hashed: Vec<LocalFile> = local.iter().flatten().cloned().collect();
        let identified = if hashed.is_empty() {
            HashMap::new()
        } else {
            on_progress.report(&phase_progress(ProvisionPhase::Resolving));
            self.content.identify(&hashed).await
        };
        for (item, file) in files.into_iter().zip(&local) {
            let known = file.as_ref().and_then(|f| identified.get(&f.sha1));
            match add_file_content(ctx, spec.kind, item, &worlds, known) {
                Ok(installed) => items.push(installed),
                Err(e) => failures.push(failure(&item.path, "", crate::error_info(e))),
            }
        }
        let (mut platform_items, mut platform_failures) = self
            .add_platform_content(ctx, spec.kind, roots, &worlds, on_progress)
            .await;
        items.append(&mut platform_items);
        failures.append(&mut platform_failures);

        let entry_worlds = ctx.worlds();
        let mut index = install::load(&ctx.entry_dir);
        for item in &items {
            if let Some(pos) = install::replaces(&index, item) {
                let old = index.remove(pos);
                if old.filename != item.filename {
                    install::remove_files(&ctx.entry_dir, &ctx.data_dir, &old, &entry_worlds);
                }
            }
            index.push(item.clone());
        }
        install::save(&ctx.entry_dir, index)?;
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
        let loader = content_loader(kind, &ctx.flavor);

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

        let mut finished = 0u64;
        while let Some(node) = queue.pop() {
            let versions = match self
                .content
                .versions(&VersionQuery {
                    source: node.source.clone(),
                    project: node.project.id.clone(),
                    loader: loader.clone(),
                    game_version: Some(ctx.game_version.clone()),
                })
                .await
            {
                Ok(versions) => versions,
                Err(e) => {
                    failures.push(failure(
                        &node.given,
                        &node.project.title,
                        crate::error_info(e),
                    ));
                    continue;
                }
            };
            let version = match install::pick_version(
                &versions,
                &ctx.game_version,
                loader.as_deref(),
                &node.pin,
            ) {
                Ok(version) => version.clone(),
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

            // Label each forwarded event with which unit of the batch this
            // is: the queue length is live, so `items` grows as dependency
            // resolution discovers more work.
            let title = node.project.title.clone();
            let item = finished + 1;
            let known = item + queue.len() as u64;
            let relabel = move |p: &ProvisionProgress| {
                let mut progress = p.clone();
                progress.detail = title.clone();
                progress.item = item;
                progress.items = known;
                on_progress.report(&progress);
            };
            // The relabelled view keeps the same cancel token: per-item install
            // is the longest part of a batch, and must stay interruptible.
            let labeled = crate::cancel::Job::new(&relabel, on_progress.cancel());
            labeled.report(&phase_progress(ProvisionPhase::Content));
            match self
                .install_version_file(ctx, kind, &node.project, &version, worlds, &labeled)
                .await
            {
                Ok(installed) => items.push(installed),
                Err(e) => failures.push(failure(
                    &node.given,
                    &node.project.title,
                    crate::error_info(e),
                )),
            }
            finished += 1;
        }
        (items, failures)
    }

    pub(super) async fn install_version_file(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
        project: &ContentProject,
        version: &proto::content::ContentVersion,
        worlds: &[String],
        on_progress: OnProgress<'_>,
    ) -> Result<InstalledContent> {
        let release = self
            .fetch_release(ctx, kind, &project.title, version, on_progress)
            .await?;
        let item = record::assemble(
            record::Project {
                kind,
                project_id: project.id.clone(),
                slug: project.slug.clone(),
                title: project.title.clone(),
                icon_url: project.icon_url.clone(),
            },
            release,
            record::Holding::fresh(worlds),
        );
        install::apply_files(&ctx.entry_dir, &ctx.data_dir, &item, &ctx.worlds())?;
        Ok(item)
    }

    /// `kind` is the *requested* kind, not the project's: Modrinth types
    /// datapacks as mod projects, so `project.kind` would route a datapack into
    /// `mods/`.
    pub(super) async fn fetch_release(
        &self,
        ctx: &EntryContent,
        kind: ContentKind,
        title: &str,
        version: &proto::content::ContentVersion,
        on_progress: OnProgress<'_>,
    ) -> Result<record::Release> {
        let file = install::primary_file(version)?;
        // CurseForge lists files whose author opted out of distribution.
        if file.artifact.url.is_empty() {
            bail!(ErrorInfo::ContentDownloadBlocked {
                title: title.to_string(),
                source: version.source.clone(),
            });
        }
        materialize::validate_filename(&file.artifact.filename)?;
        let managed = content_target(ctx, kind, &file.artifact.filename)?;
        materialize::ensure_artifact(
            Some(&self.cache),
            &file.artifact,
            &managed,
            ProvisionPhase::Content,
            on_progress,
        )
        .await?;
        Ok(record::Release {
            source: version.source.clone(),
            version_id: version.id.clone(),
            version_number: version.version_number.clone(),
            filename: file.artifact.filename.clone(),
            sha1: file
                .artifact
                .checksum
                .as_ref()
                .map(|c| c.hex.clone())
                .unwrap_or_default(),
            url: file.artifact.url.clone(),
        })
    }
}

/// Import a local file: copy it into the managed directory and mirror it into
/// the game's load dirs. One a source recognises (`known`) is recorded as that
/// release, so it updates like any installed item; one nobody recognises has no
/// provenance beyond the hash and can never update. Either way only the file
/// asked for is installed — its dependencies are not resolved.
fn add_file_content(
    ctx: &EntryContent,
    kind: ContentKind,
    item: &ContentAddItem,
    worlds: &[String],
    known: Option<&Identified>,
) -> Result<InstalledContent> {
    let source = Path::new(&item.path);
    if !source.is_file() {
        bail!("no file at {}", source.display());
    }
    let filename = if !item.filename.is_empty() {
        item.filename.clone()
    } else if let Some(known) = known {
        known.file.artifact.filename.clone()
    } else {
        source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    materialize::validate_filename(&filename)?;
    // Reject an unreadable archive or a modpack; the detected kind is advisory
    // (the requested `kind` wins, so an override installs where asked).
    match inspect::classify(source)? {
        inspect::Detected::Kind(detected) if detected != kind => tracing::warn!(
            file = %filename,
            ?detected,
            requested = ?kind,
            "importing a local file under a kind that differs from its detected type"
        ),
        _ => {}
    }
    let managed = content_target(ctx, kind, &filename)?;
    if let Some(parent) = managed.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::copy(source, &managed)
        .with_context(|| format!("cannot import {}", source.display()))?;
    let sha1 = install::sha1_file(&managed)?;
    let (project, release) = match known {
        Some(known) => (
            record::Project {
                kind,
                project_id: known.project.id.clone(),
                slug: known.project.slug.clone(),
                title: known.project.title.clone(),
                icon_url: known.project.icon_url.clone(),
            },
            record::Release {
                source: known.version.source.clone(),
                version_id: known.version.id.clone(),
                version_number: known.version.version_number.clone(),
                filename,
                sha1,
                url: known.file.artifact.url.clone(),
            },
        ),
        None => (
            record::Project::untracked(kind, filename.clone()),
            record::Release::local(filename, sha1),
        ),
    };
    let installed = record::assemble(project, release, record::Holding::fresh(worlds));
    install::apply_files(&ctx.entry_dir, &ctx.data_dir, &installed, &ctx.worlds())?;
    Ok(installed)
}

/// Whether a fetched project satisfies the requested kind. Datapacks accept
/// `Mod` projects: Modrinth has no datapack project type — a datapack is a
/// mod-typed project whose versions carry the `datapack` loader.
fn kind_matches(requested: ContentKind, project: ContentKind) -> bool {
    requested == project || (requested == ContentKind::DataPack && project == ContentKind::Mod)
}

fn failure(item: impl Into<String>, title: impl Into<String>, error: ErrorInfo) -> ContentFailure {
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
