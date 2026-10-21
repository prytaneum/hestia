//! hestia's own archive format — portable and compact.
//!
//! It is the entry directory itself: the content pool, the modpack record, the
//! game directory, everything but what regenerates. A pool file a source can
//! serve again stays out (see [`install::referenceable`]); import fetches it
//! from its record. The record travels beside them in `hestia.instance.json`,
//! which is both the manifest and the marker. The record's identity travels;
//! its resolved profile is host-specific (library rules are filtered by the
//! exporting OS), so it is left out and import resolves the game and loader
//! versions on its own host.
//!
//! The marker is not called `instance.json` deliberately. A marker file is a
//! claim about what an archive is, so it has to be specific enough that finding
//! it inside someone else's zip means something.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::error::{ArchiveFault, ArchiveFormat, ErrorInfo};
use proto::transfer::{ExportFormat, ImportFormat};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::archive::{self, Reader, Writer, Written};
use super::exclude::Rules;
use super::{Blueprint, Carried, Descriptor, Format, Landed, Recipe, Source, Target};
use crate::cancel::Job;
use crate::content::install;
use crate::instances::InstanceRecord;
use crate::minecraft::profile::InstanceProfile;
use crate::registry;
use crate::schema::{self, Document};

pub(crate) const MANIFEST: &str = "hestia.instance.json";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Manifest {
    pub(crate) exported_unix: i64,
    /// Which build wrote it — the first thing worth knowing about an archive
    /// that will not read back.
    pub(crate) exported_by: String,
    /// The exported instance's record, carried stamped rather than inlined as a
    /// typed field so its own schema stamp is checked on the way in. Its `id` is
    /// not honoured on import.
    pub(crate) instance: Value,
}

impl Document for Manifest {
    const NAME: &'static str = MANIFEST;
}

impl Manifest {
    fn record(&self) -> Result<InstanceRecord> {
        schema::decode::<InstanceRecord>(self.instance.clone())
            .map_err(|e| schema::archive_error(ArchiveFormat::Hestia, e).into())
    }
}

pub(crate) struct Hestia;

impl Format for Hestia {
    fn id(&self) -> ImportFormat {
        ImportFormat::Hestia
    }

    fn marker(&self) -> &'static str {
        MANIFEST
    }

    fn read(&self, reader: &mut Reader, prefix: &str) -> Result<Blueprint> {
        let record = manifest(reader, prefix)?.record()?;
        if record.profile.game_version.is_empty() {
            return Err(invalid(ArchiveFault::NoGameVersion).into());
        }
        let profile = &record.profile;
        let loader = super::loader_of(profile);
        let descriptor = Descriptor {
            name: record.name.clone(),
            game_version: profile.game_version.clone(),
            loader,
            loader_version: profile.loader_version.clone().unwrap_or_default(),
        };
        let recipe = Recipe {
            jvm: record.jvm,
            carried: Some(Carried {
                created_unix: record.created_unix,
                last_played_unix: record.last_played_unix,
                playtime_seconds: record.playtime_seconds,
                sharing: record.sharing,
            }),
        };
        Ok(Blueprint { descriptor, recipe })
    }

    fn land(
        &self,
        reader: &mut Reader,
        prefix: &str,
        target: &Target<'_>,
        job: &Job<'_>,
    ) -> Result<Landed> {
        // The record is registered before this runs, so a member named like it
        // would replace it; exports never carry one.
        let files = reader.extract_under(prefix, target.entry_dir, job, &|relative| {
            if relative == InstanceRecord::NAME {
                tracing::debug!(member = relative, "skipping a record the importer owns");
            }
            relative != MANIFEST && relative != InstanceRecord::NAME
        })?;
        Ok(Landed {
            files,
            adopted: Vec::new(),
        })
    }
}

fn manifest(reader: &mut Reader, prefix: &str) -> Result<Manifest> {
    let text = reader
        .read_text(&format!("{prefix}{MANIFEST}"))
        .map_err(|e| {
            tracing::warn!(error = %e, "cannot read {MANIFEST}");
            invalid(ArchiveFault::ManifestMissing)
        })?;
    let value = serde_json::from_str(&text).map_err(|e| {
        tracing::warn!(error = %e, "{MANIFEST} is malformed");
        invalid(ArchiveFault::ManifestMalformed)
    })?;
    schema::decode::<Manifest>(value)
        .map_err(|e| schema::archive_error(ArchiveFormat::Hestia, e).into())
}

fn invalid(reason: ArchiveFault) -> ErrorInfo {
    ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Hestia,
        reason,
    }
}

/// The path an export writes to when the caller named none: the instance's
/// slug and the time it was taken, so repeated exports never collide.
pub(crate) fn default_destination(
    dir: &Path,
    record: &InstanceRecord,
    format: ExportFormat,
) -> PathBuf {
    let slug = registry::dir_name(&record.id, &record.name);
    let stamp = registry::utc_stamp(common::now_unix());
    dir.join(format!("{slug}-{stamp}.{}", format.extension()))
}

pub(crate) fn export(source: &Source<'_>, destination: &Path, job: &Job<'_>) -> Result<Written> {
    let profile = &source.record.profile;
    let identity = InstanceRecord {
        profile: InstanceProfile {
            flavor: profile.flavor.clone(),
            game_version: profile.game_version.clone(),
            loader_version: profile.loader_version.clone(),
            ..InstanceProfile::default()
        },
        ..source.record.clone()
    };
    let manifest = Manifest {
        exported_unix: common::now_unix(),
        exported_by: format!("{} {}", common::app::NAME, common::app::VERSION),
        instance: schema::encode(&identity)?,
    };
    let encoded = serde_json::to_vec_pretty(&schema::encode(&manifest)?)
        .context("the manifest serializes")?;
    let rules = Rules::new(source.entry_dir, source.data_dir, source.exclude);
    let tracked = install::load(source.entry_dir);
    let referenced = install::referenceable_paths(&tracked);
    let tracked_count = tracked.len();
    let kept: Vec<_> = tracked
        .into_iter()
        .filter(|item| install::relative_path(item).is_some_and(|path| rules.keeps(&path)))
        .collect();
    tracing::debug!(
        kept = kept.len(),
        dropped = tracked_count - kept.len(),
        "tracked content in the archive"
    );

    let mut writer = Writer::create(destination)?;
    writer.add_bytes(MANIFEST, &encoded)?;
    if tracked_count > 0 {
        writer.add_bytes(install::INDEX, &install::encode(kept)?)?;
    }
    writer.add_all(
        &archive::plan(source.entry_dir, "", &|relative| {
            rules.keeps(relative) && !referenced.contains(relative)
        }),
        job,
    )?;
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::profile::InstanceProfile;
    use proto::content::InstalledContent;

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-native-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn write(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn record() -> InstanceRecord {
        InstanceRecord {
            id: "0192abc".to_string(),
            name: "Cozy".to_string(),
            created_unix: 1_700_000_000,
            last_played_unix: Some(1_800_000_000),
            playtime_seconds: 3600,
            jvm: Default::default(),
            sharing: Default::default(),
            profile: InstanceProfile {
                flavor: "fabric".to_string(),
                game_version: "1.21.1".to_string(),
                loader_version: Some("0.16.9".to_string()),
                java_major: 21,
                ..Default::default()
            },
        }
    }

    fn job_for(cancel: &crate::cancel::Cancel) -> Job<'_> {
        static NOOP: fn(&proto::minecraft::ProvisionProgress) = |_| {};
        Job::new(&NOOP, cancel)
    }

    /// Export an instance tree, then read it back the way an import does.
    #[test]
    fn an_export_round_trips_through_the_manifest_and_the_tree() {
        let dir = temp("roundtrip");
        let entry_dir = dir.path().join("instances/cozy");
        let data_dir = entry_dir.join("data");
        write(&entry_dir, "content.json", r#"{"items":[]}"#);
        write(&entry_dir, "mods/sodium.jar", "jar bytes");
        write(&data_dir, "options.txt", "lang:en");
        write(&data_dir, "saves/world/level.dat", "nbt");
        write(&entry_dir, "logs/session-1.log", "noise");
        write(&data_dir, "crash-reports/crash.txt", "noise");

        let record = record();
        let archive_path = dir.path().join("cozy.hestia");
        let cancel = crate::cancel::Cancel::new();
        let job = job_for(&cancel);
        export(
            &Source {
                record: &record,
                entry_dir: &entry_dir,
                data_dir: &data_dir,
                exclude: &[],
            },
            &archive_path,
            &job,
        )
        .unwrap();

        let mut reader = Reader::open(&archive_path).unwrap();
        let blueprint = Hestia.read(&mut reader, "").unwrap();
        assert_eq!(blueprint.descriptor.name, "Cozy");
        assert_eq!(blueprint.descriptor.game_version, "1.21.1");
        assert_eq!(blueprint.descriptor.loader, "fabric");
        let carried = blueprint
            .recipe
            .carried
            .expect("hestia carries an identity");
        assert_eq!(carried.playtime_seconds, 3600);
        let written = manifest(&mut reader, "").unwrap().record().unwrap();
        assert_eq!(
            written.profile.java_major, 0,
            "the host-resolved profile stays out of the manifest"
        );

        let restored = temp("restored");
        let target = Target {
            entry_dir: restored.path(),
            data_dir: &restored.path().join("data"),
        };
        Hestia.land(&mut reader, "", &target, &job).unwrap();

        assert!(restored.path().join("mods/sodium.jar").is_file());
        assert_eq!(
            std::fs::read_to_string(restored.path().join("data/options.txt")).unwrap(),
            "lang:en"
        );
        assert!(restored.path().join("data/saves/world/level.dat").is_file());
        assert!(
            !restored.path().join("logs").exists(),
            "session logs regenerate"
        );
        assert!(!restored.path().join("data/crash-reports").exists());
        assert!(
            !restored.path().join(MANIFEST).exists(),
            "the manifest is the record, not a file the instance keeps"
        );
        assert!(
            !restored.path().join("instance.json").exists(),
            "the record is written by the store when the instance is registered"
        );
    }

    #[test]
    fn an_export_embeds_a_local_item_and_references_a_tracked_one() {
        let dir = temp("referenced");
        let entry_dir = dir.path().join("instances/cozy");
        let data_dir = entry_dir.join("data");
        write(&entry_dir, "mods/local.jar", "handmade");
        write(&entry_dir, "mods/sodium.jar", "served");
        install::save(
            &entry_dir,
            vec![
                InstalledContent {
                    filename: "local.jar".to_string(),
                    ..Default::default()
                },
                InstalledContent {
                    filename: "sodium.jar".to_string(),
                    url: "https://cdn.modrinth.com/sodium.jar".to_string(),
                    sha1: "abc".to_string(),
                    ..Default::default()
                },
            ],
        )
        .unwrap();

        let archive_path = dir.path().join("cozy.hestia");
        let cancel = crate::cancel::Cancel::new();
        let job = job_for(&cancel);
        export(
            &Source {
                record: &record(),
                entry_dir: &entry_dir,
                data_dir: &data_dir,
                exclude: &[],
            },
            &archive_path,
            &job,
        )
        .unwrap();

        let restored = temp("referenced-restored");
        let target = Target {
            entry_dir: restored.path(),
            data_dir: &restored.path().join("data"),
        };
        let mut reader = Reader::open(&archive_path).unwrap();
        Hestia.land(&mut reader, "", &target, &job).unwrap();

        assert!(restored.path().join("mods/local.jar").is_file());
        assert!(!restored.path().join("mods/sodium.jar").exists());
        assert!(
            restored.path().join("content.json").is_file(),
            "the record of the omitted file travels so import can fetch it"
        );
    }

    #[test]
    fn excluding_a_pool_file_drops_its_record_from_the_archive() {
        let dir = temp("excluded");
        let entry_dir = dir.path().join("instances/cozy");
        let data_dir = entry_dir.join("data");
        write(&entry_dir, "mods/dropped.jar", "handmade");
        write(&entry_dir, "mods/kept.jar", "handmade");
        let tracked = |filename: &str| InstalledContent {
            filename: filename.to_string(),
            ..Default::default()
        };
        install::save(
            &entry_dir,
            vec![tracked("dropped.jar"), tracked("kept.jar")],
        )
        .unwrap();

        let imported_names = |exclude: &[String]| {
            let archive_path = dir.path().join("cozy.hestia");
            let cancel = crate::cancel::Cancel::new();
            let job = job_for(&cancel);
            export(
                &Source {
                    record: &record(),
                    entry_dir: &entry_dir,
                    data_dir: &data_dir,
                    exclude,
                },
                &archive_path,
                &job,
            )
            .unwrap();

            let restored = temp("excluded-restored");
            let target = Target {
                entry_dir: restored.path(),
                data_dir: &restored.path().join("data"),
            };
            let mut reader = Reader::open(&archive_path).unwrap();
            Hestia.land(&mut reader, "", &target, &job).unwrap();
            let names: Vec<String> = install::load(restored.path())
                .into_iter()
                .map(|item| item.filename)
                .collect();
            (names, restored)
        };

        let (names, restored) = imported_names(&["mods/dropped.jar".to_string()]);
        assert_eq!(names, ["kept.jar"]);
        assert!(!restored.path().join("mods/dropped.jar").exists());
        assert!(restored.path().join("mods/kept.jar").is_file());

        let (names, _) = imported_names(&["mods".to_string()]);
        assert!(names.is_empty(), "excluding the directory drops every mod");
    }

    #[test]
    fn an_archive_cannot_replace_the_registered_record() {
        let dir = temp("hostile-record");
        let path = dir.path().join("hostile.hestia");
        let mut writer = Writer::create(&path).unwrap();
        writer.add_bytes(MANIFEST, b"{}").unwrap();
        writer.add_bytes(InstanceRecord::NAME, b"hostile").unwrap();
        writer.add_bytes("mods/a.jar", b"jar").unwrap();
        writer.finish().unwrap();

        let restored = temp("hostile-restored");
        std::fs::write(restored.path().join(InstanceRecord::NAME), "registered").unwrap();
        let target = Target {
            entry_dir: restored.path(),
            data_dir: &restored.path().join("data"),
        };
        let cancel = crate::cancel::Cancel::new();
        Hestia
            .land(
                &mut Reader::open(&path).unwrap(),
                "",
                &target,
                &job_for(&cancel),
            )
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(restored.path().join(InstanceRecord::NAME)).unwrap(),
            "registered"
        );
        assert!(restored.path().join("mods/a.jar").is_file());
    }

    /// Write an archive holding only `manifest`, as raw JSON.
    fn archive_of(path: &Path, manifest: &Value) {
        let mut writer = Writer::create(path).unwrap();
        writer
            .add_bytes(MANIFEST, &serde_json::to_vec(manifest).unwrap())
            .unwrap();
        writer.finish().unwrap();
    }

    fn read_error(path: &Path) -> ErrorInfo {
        match Hestia.read(&mut Reader::open(path).unwrap(), "") {
            Ok(_) => panic!("the archive was accepted"),
            Err(e) => e.downcast::<ErrorInfo>().expect("a typed archive error"),
        }
    }

    #[test]
    fn a_newer_manifest_schema_is_refused_rather_than_guessed_at() {
        let dir = temp("newer-manifest");
        let path = dir.path().join("future.hestia");
        let mut manifest = schema::encode(&Manifest {
            instance: schema::encode(&record()).unwrap(),
            ..Default::default()
        })
        .unwrap();
        manifest[schema::FIELD] = Value::from(Manifest::VERSION + 1);
        archive_of(&path, &manifest);

        assert!(matches!(read_error(&path), ErrorInfo::ArchiveTooNew { .. }));
    }

    /// The record travels stamped with its own version, so an archive whose
    /// manifest this build reads fine can still carry a record it cannot.
    #[test]
    fn a_newer_record_schema_inside_a_readable_manifest_is_refused_too() {
        let dir = temp("newer-record");
        let path = dir.path().join("future-record.hestia");
        let mut instance = schema::encode(&record()).unwrap();
        instance[schema::FIELD] = Value::from(InstanceRecord::VERSION + 1);
        archive_of(
            &path,
            &schema::encode(&Manifest {
                instance,
                ..Default::default()
            })
            .unwrap(),
        );

        assert!(matches!(read_error(&path), ErrorInfo::ArchiveTooNew { .. }));
    }

    #[test]
    fn an_unstamped_manifest_reads_as_the_baseline() {
        let dir = temp("legacy-manifest");
        let path = dir.path().join("legacy.hestia");
        archive_of(
            &path,
            &serde_json::json!({ "instance": serde_json::to_value(record()).unwrap() }),
        );

        let blueprint = Hestia.read(&mut Reader::open(&path).unwrap(), "").unwrap();

        assert_eq!(blueprint.descriptor.name, "Cozy");
        assert_eq!(blueprint.descriptor.game_version, "1.21.1");
    }

    #[test]
    fn a_manifest_pinning_no_version_is_invalid() {
        let dir = temp("no-version");
        let path = dir.path().join("broken.hestia");
        let mut record = record();
        record.profile.game_version = String::new();
        archive_of(
            &path,
            &schema::encode(&Manifest {
                instance: schema::encode(&record).unwrap(),
                ..Default::default()
            })
            .unwrap(),
        );

        assert!(matches!(
            read_error(&path),
            ErrorInfo::ArchiveInvalid { .. }
        ));
    }
}
