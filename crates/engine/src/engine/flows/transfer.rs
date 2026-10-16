//! Instance import and export: the launcher side of the archive formats.
//!
//! Export is a read of one entry directory. Import is a **create** — it
//! registers a new instance and then fills it — so it owns the same
//! all-or-nothing discipline every other create has: an import that fails
//! partway removes the entry it had started, rather than leaving a registered
//! instance nobody asked for that cannot launch.
//!
//! Nothing here knows which formats exist. A format is read into a
//! [`Blueprint`] whose [`Recipe`] says which of three routes to the record
//! applies, and the match below is over those routes — so adding a format is a
//! module under `transfer/` and a line in its registry, and this file does not
//! change.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::content::{ContentFailure, LOCAL_SOURCE};
use proto::error::{ErrorInfo, Field, Reason};
use proto::modpack::{ModpackRef, ModpackTarget};
use proto::transfer::{ArchiveEntry, ArchiveInfo, ExportFormat, ImportFormat};
use proto::warning::WarningInfo;

use crate::cancel::Job;
use crate::engine::Engine;
use crate::instances::InstanceRecord;
use crate::registry;
use crate::schema;
use crate::transfer::archive::{self, Reader};
use crate::transfer::{
    self, contents, hestia, Blueprint, Descriptor, Detected, Recipe, Source, Target,
};

/// Where an export with no destination lands, under the data home.
const EXPORTS: &str = "exports";

fn detect(reader: &mut Reader, path: &Path) -> Result<Detected> {
    transfer::detect(&reader.names(), &archive::filename_of(path), |candidate| {
        candidate.format.claims(reader, &candidate.prefix)
    })
}

/// Documents set aside since `mark` that belong to this entry — scoped by path
/// so a concurrent operation's quarantine is not reported as this import's.
fn quarantined_under(mark: schema::notices::Mark, entry_dir: &Path) -> Vec<WarningInfo> {
    let prefix = entry_dir.display().to_string();
    schema::notices::since(mark)
        .into_iter()
        .filter(|warning| match warning {
            WarningInfo::DocumentQuarantined { path, .. } => path.starts_with(&prefix),
            _ => false,
        })
        .collect()
}

/// What an export produced.
pub struct ExportOutcome {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub files: u64,
    pub warnings: Vec<WarningInfo>,
}

/// What an import produced: the instance that now exists, and whatever was less
/// than perfect about getting there.
pub struct ImportOutcome {
    pub format: ImportFormat,
    pub record: InstanceRecord,
    pub failures: Vec<ContentFailure>,
    pub warnings: Vec<WarningInfo>,
}

impl Engine {
    /// Write an instance out as an archive. `destination` may name a file, name
    /// a directory to write a generated filename into, or be empty for the data
    /// home's own `exports/`.
    pub fn export_instance(
        &self,
        reference: &str,
        format: ExportFormat,
        destination: &str,
        exclude: &[String],
        job: &Job<'_>,
    ) -> Result<ExportOutcome> {
        let record = self.instance_record(reference)?;
        let entry_dir = self.instances.instance_dir(&record);
        let data_dir = self.instances.data_dir(&record);
        let exports = self.data_home().join(EXPORTS);
        let destination = export_destination(destination, &exports, &record, format)?;

        tracing::info!(
            instance = %record.name,
            format = format.as_str(),
            destination = %destination.display(),
            "exporting an instance"
        );
        let source = Source {
            record: &record,
            entry_dir: &entry_dir,
            data_dir: &data_dir,
            exclude,
        };
        let (written, warnings) = transfer::export(&source, format, &destination, job)?;
        tracing::info!(
            path = %written.path.display(),
            files = written.files,
            bytes = written.size_bytes,
            "instance exported"
        );
        Ok(ExportOutcome {
            path: written.path,
            size_bytes: written.size_bytes,
            files: written.files,
            warnings,
        })
    }

    /// What an export of this instance would carry, as a tree — what a caller
    /// picks its `exclude` paths out of.
    pub fn export_contents(&self, reference: &str) -> Result<Vec<ArchiveEntry>> {
        let record = self.instance_record(reference)?;
        Ok(contents::list(
            &self.instances.instance_dir(&record),
            &self.instances.data_dir(&record),
        ))
    }

    /// What an archive is, without importing it — so a front-end can name the
    /// instance it is about to create, and say what the file actually is.
    pub fn inspect_archive(&self, path: &str) -> Result<ArchiveInfo> {
        let path = daemon_local_path(path)?;
        let mut reader = Reader::open(&path)?;
        let detected = detect(&mut reader, &path)?;
        let blueprint = detected.format.read(&mut reader, &detected.prefix)?;
        let Descriptor {
            name,
            game_version,
            loader,
            loader_version,
        } = blueprint.descriptor;
        Ok(ArchiveInfo {
            format: detected.format.id(),
            name_taken: self.instance_name_taken(&name),
            name,
            game_version,
            loader,
            loader_version,
        })
    }

    /// Import an archive as a new instance. `name` overrides the one the
    /// archive carries.
    pub async fn import_instance(
        &self,
        path: &str,
        name: &str,
        job: &Job<'_>,
    ) -> Result<ImportOutcome> {
        let path = daemon_local_path(path)?;
        let mut reader = Reader::open(&path)?;
        let detected = detect(&mut reader, &path)?;
        let format = detected.format.id();
        tracing::info!(path = %path.display(), format = format.as_str(), "importing an instance");

        let Blueprint { descriptor, recipe } =
            detected.format.read(&mut reader, &detected.prefix)?;
        let name = pick_name(name, &descriptor.name);

        // A pack is not an instance: the modpack flow creates the entry, fetches
        // every file from its source and records the provenance. Nothing here
        // could improve on that by unpacking the archive itself.
        if matches!(recipe, Recipe::Pack) {
            return self.import_pack(&path, &name, format, job).await;
        }

        let record = self.record_for(recipe, &name, &descriptor).await?;
        let entry_dir = self.instances.instance_dir(&record);
        let data_dir = self.instances.data_dir(&record);
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("cannot create {}", data_dir.display()))?;

        let landed = detected.format.land(
            &mut reader,
            &detected.prefix,
            &Target {
                entry_dir: &entry_dir,
                data_dir: &data_dir,
            },
            job,
        );
        let landed = self.unwind_on_failure(&record, landed)?;
        tracing::info!(
            id = %record.id,
            name = %record.name,
            files = landed.files,
            "instance imported"
        );

        let mark = schema::notices::mark();
        crate::content::validate(&entry_dir);
        let restored = self.restore_tracked_content(&entry_dir, job).await;
        let mut warnings = self.unwind_on_failure(&record, restored)?;
        warnings.extend(self.untracked_adoptions(&entry_dir, &landed.adopted).await);
        warnings.extend(quarantined_under(mark, &entry_dir));

        Ok(ImportOutcome {
            format,
            record,
            failures: Vec::new(),
            warnings,
        })
    }

    /// A lookup that cannot run leaves them untracked rather than failing an import that has already landed.
    async fn untracked_adoptions(&self, entry_dir: &Path, adopted: &[String]) -> Vec<WarningInfo> {
        if adopted.is_empty() {
            return Vec::new();
        }
        if let Err(e) = self.recognise_pool(entry_dir).await {
            tracing::warn!(error = %format!("{e:#}"), "cannot record recognised imports");
        }
        let files: Vec<String> = crate::content::install::load(entry_dir)
            .into_iter()
            .filter(|item| item.source == LOCAL_SOURCE && adopted.contains(&item.filename))
            .map(|item| item.filename)
            .collect();
        tracing::info!(
            adopted = adopted.len(),
            untracked = files.len(),
            "imported content looked up"
        );
        match files.is_empty() {
            true => Vec::new(),
            false => vec![WarningInfo::ImportFilesUntracked {
                count: files.len() as u32,
                files,
            }],
        }
    }

    /// Register the instance an archive describes. The three routes are the
    /// whole of what a format can ask the launcher for.
    async fn record_for(
        &self,
        recipe: Recipe,
        name: &str,
        descriptor: &Descriptor,
    ) -> Result<InstanceRecord> {
        let template = match recipe {
            // The carried profile was resolved for the exporting host's OS, so
            // only the instance's identity is kept and the profile is resolved
            // again here.
            Recipe::Record(record) => {
                let flavor = self.archive_flavor(&descriptor.loader)?;
                let loader_version =
                    Some(descriptor.loader_version.clone()).filter(|version| !version.is_empty());
                let profile = self
                    .minecraft
                    .resolve_instance(&flavor, &descriptor.game_version, loader_version)
                    .await?;
                InstanceRecord {
                    name: name.to_string(),
                    profile,
                    ..*record
                }
            }
            // A game version and a loader, resolved exactly as a create does.
            Recipe::Resolve {
                loader,
                loader_version,
                jvm,
            } => {
                let flavor = self.archive_flavor(&loader)?;
                let profile = self
                    .minecraft
                    .resolve_instance(&flavor, &descriptor.game_version, loader_version)
                    .await?;
                InstanceRecord {
                    id: String::new(),
                    name: name.to_string(),
                    created_unix: registry::now_unix(),
                    last_played_unix: None,
                    playtime_seconds: 0,
                    jvm,
                    sharing: Default::default(),
                    profile,
                }
            }
            Recipe::Pack => unreachable!("a pack is installed by the modpack flow"),
        };
        self.instances.adopt(name, template)
    }

    /// A pack reaches the ordinary modpack install, which creates the entry and
    /// makes its mods updatable pool items.
    async fn import_pack(
        &self,
        path: &Path,
        name: &str,
        format: ImportFormat,
        job: &Job<'_>,
    ) -> Result<ImportOutcome> {
        let outcome = self
            .install_instance_modpack(
                &ModpackRef {
                    path: path.to_string_lossy().into_owned(),
                    ..ModpackRef::default()
                },
                &ModpackTarget::Create {
                    name: name.to_string(),
                },
                job,
            )
            .await?;
        let record = self
            .instances
            .get(&outcome.entry)
            .with_context(|| format!("instance '{}' vanished after import", outcome.entry))?;
        Ok(ImportOutcome {
            format,
            record,
            failures: outcome.failures,
            warnings: outcome.warnings,
        })
    }

    /// The flavor an archive's loader names, refused by name when there is
    /// none — the registry is the table, exactly as it is for a modpack, so a
    /// flavor added later needs no edit here.
    fn archive_flavor(&self, loader: &str) -> Result<String> {
        let wanted = match loader.is_empty() {
            true => "vanilla",
            false => loader,
        };
        match self
            .minecraft
            .flavors(crate::minecraft::Side::Client)
            .iter()
            .any(|f| f.id == wanted)
        {
            true => Ok(wanted.to_string()),
            false => Err(ErrorInfo::ArchiveUnsupported {
                loader: wanted.to_string(),
            }
            .into()),
        }
    }

    /// Roll back a half-built import. A registered instance whose files never
    /// landed is worse than no instance: it lists, it cannot launch, and the
    /// user did not ask for it.
    fn unwind_on_failure<T>(&self, record: &InstanceRecord, outcome: Result<T>) -> Result<T> {
        if outcome.is_err() {
            if let Err(e) = self.remove_instance(&record.id) {
                tracing::warn!(id = %record.id, error = %e, "cannot remove a half-imported instance");
            }
        }
        outcome
    }

    pub(super) fn instance_name_taken(&self, name: &str) -> bool {
        registry::name_taken(name, self.instances.list().iter().map(|r| r.name.as_str()))
    }
}

fn pick_name(requested: &str, from_archive: &str) -> String {
    match requested.trim().is_empty() {
        false => requested.trim().to_string(),
        true => from_archive.trim().to_string(),
    }
}

/// Where an export writes: the caller's file, the caller's directory plus a
/// generated name, or the data home's `exports/`.
fn export_destination(
    destination: &str,
    exports: &Path,
    record: &InstanceRecord,
    format: ExportFormat,
) -> Result<PathBuf> {
    if destination.trim().is_empty() {
        return Ok(hestia::default_destination(exports, record, format));
    }
    let path = daemon_local_path(destination)?;
    match path.is_dir() {
        true => Ok(hestia::default_destination(&path, record, format)),
        false => Ok(path),
    }
}

/// A path a client named for the daemon to read or write. It must be absolute:
/// the two are different processes and need not share a working directory, so a
/// relative path means something different on each side of the socket.
fn daemon_local_path(value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value.trim());
    if path.as_os_str().is_empty() {
        return Err(ErrorInfo::FieldRequired { field: Field::Path }.into());
    }
    if !path.is_absolute() {
        tracing::debug!(path = %path.display(), "rejected a relative path");
        return Err(ErrorInfo::InvalidValue {
            field: Field::Path,
            reason: Reason::AbsolutePath,
        }
        .into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str) -> InstanceRecord {
        InstanceRecord {
            id: "abc".to_string(),
            name: name.to_string(),
            ..InstanceRecord::default()
        }
    }

    #[test]
    fn an_empty_destination_lands_in_the_data_homes_exports() {
        let exports = Path::new("/home/u/.hestia/exports");
        let path = export_destination("", exports, &record("Cozy"), ExportFormat::Hestia).unwrap();
        assert!(path.starts_with(exports));
        assert_eq!(path.extension().unwrap(), "hestia");
        assert!(path.to_string_lossy().contains("cozy"));
    }

    #[test]
    fn a_named_file_is_taken_as_given() {
        let dest = std::env::temp_dir().join("share/cozy.mrpack");
        let dest_str = dest.to_str().unwrap().to_string();
        let path = export_destination(
            &dest_str,
            Path::new("/exports"),
            &record("Cozy"),
            ExportFormat::Mrpack,
        )
        .unwrap();
        assert_eq!(path, dest);
    }

    #[test]
    fn a_relative_path_is_refused_because_the_daemon_is_a_different_process() {
        let error = export_destination(
            "./cozy.hestia",
            Path::new("/exports"),
            &record("Cozy"),
            ExportFormat::Hestia,
        )
        .unwrap_err();
        assert!(error.to_string().contains("absolute"), "{error}");
        assert!(daemon_local_path("   ").is_err());
    }

    #[test]
    fn the_requested_name_wins_over_the_archives_own() {
        assert_eq!(pick_name(" Mine ", "Cozy Pack"), "Mine");
        assert_eq!(pick_name("", "Cozy Pack"), "Cozy Pack");
        assert_eq!(pick_name("   ", "Cozy Pack"), "Cozy Pack");
    }
}
