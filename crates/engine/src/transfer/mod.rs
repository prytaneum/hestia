//! Instance import and export — moving an instance in and out of the launcher
//! as a single file.
//!
//! This is the instance answer to what backups are for a server, and it is a
//! different shape on purpose: a server's data is archived *in place*, on a
//! schedule, because a server is infrastructure that has to be recoverable. An
//! instance is something you play, share, and move between machines, so its
//! archive is one portable file that says what it is.
//!
//! # Adding a format
//!
//! One module beside this one and one line in [`formats`]. A format answers
//! three questions and nothing else:
//!
//! 1. **which archives are mine** — a [`Format::MARKER`] file, matched by name;
//! 2. **what does this one say it is** — [`Format::read`], which parses the
//!    archive's own manifest into a [`Blueprint`];
//! 3. **where do the files go** — [`Format::land`], given an instance that now
//!    exists on disk.
//!
//! What a format never does is create the instance. That is the launcher's
//! job, and the [`Recipe`] a blueprint carries says which of the three ways to
//! do it applies — a resolved record travelling in the archive, a game version
//! and loader that still have to be resolved, or a pack the modpack flow owns
//! end to end. Those three are a closed set: the flow matches on the recipe,
//! not on the format, so a fourth format costs the flow nothing.
//!
//! Export is the same shape with fewer moving parts: [`ExportFormat`] picks a
//! writer, and the rules for what an archive leaves out are shared
//! ([`exclude`]) so two exporters cannot drift on the question of what an
//! instance's files actually are.

pub(crate) mod archive;
pub(crate) mod contents;
pub(crate) mod curseforge;
pub(crate) mod exclude;
pub(crate) mod hestia;
pub(crate) mod mrpack;
pub(crate) mod pool;
pub(crate) mod prism;

use std::path::Path;

use anyhow::Result;
use proto::error::ErrorInfo;
use proto::minecraft::InstanceProfile;
use proto::transfer::{ExportFormat, ImportFormat};
use proto::warning::WarningInfo;

use crate::cancel::Job;
use crate::instances::InstanceRecord;
use crate::minecraft::launch::JavaSettings;
use archive::{Reader, Written};

/// Every archive format hestia recognises, in the order their markers are
/// preferred when two sit at the same depth. **This list is the registry** —
/// adding a format is a module beside this one and a line here.
fn formats() -> &'static [&'static dyn Format] {
    &[
        &hestia::Hestia,
        &mrpack::Mrpack,
        &prism::Prism,
        &curseforge::CurseForge,
    ]
}

/// What an archive says it is, before anything is written. Shown to whoever is
/// about to import it, and the reason inspecting is a separate call.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Descriptor {
    pub(crate) name: String,
    pub(crate) game_version: String,
    /// The loader's own name (`fabric`, `neoforge`), empty for vanilla.
    pub(crate) loader: String,
    pub(crate) loader_version: String,
}

/// How the launcher has to bring an archive's instance into existence. The
/// three variants are the three genuinely different routes, not one per
/// format: several formats share a route, and a new format picks one rather
/// than adding a fourth.
pub(crate) enum Recipe {
    /// The archive carries a resolved record — hestia's own export. Nothing to
    /// look up, so this route works with no network at all.
    Record(Box<InstanceRecord>),
    /// The archive names a game version and a loader, which resolve exactly as
    /// `instance create` resolves them.
    Resolve {
        /// The loader's name, empty for vanilla. Checked against the registered
        /// flavors, so an archive pinning one hestia has no flavor for is
        /// refused by name.
        loader: String,
        loader_version: Option<String>,
        /// Per-instance Java settings the archive carried, where its launcher
        /// stored any.
        jvm: JavaSettings,
    },
    /// The archive is a modpack, not an instance. The modpack flow already
    /// creates an entry from one and makes its mods ordinary pool items, so it
    /// owns this end to end and nothing here lands any files.
    Pack,
}

/// One archive, read: what it is and how to build it.
pub(crate) struct Blueprint {
    pub(crate) descriptor: Descriptor,
    pub(crate) recipe: Recipe,
}

/// Where a landing puts files. Both directories exist by the time a format is
/// asked to land, because the instance has already been registered.
pub(crate) struct Target<'a> {
    pub(crate) entry_dir: &'a Path,
    pub(crate) data_dir: &'a Path,
}

/// What a landing produced.
#[derive(Default)]
pub(crate) struct Landed {
    pub(crate) files: u64,
    /// Filenames of pool items taken from the archive's own bytes.
    pub(crate) adopted: Vec<String>,
}

/// One archive format hestia can read. Implementations are stateless — they
/// are consulted through a `&'static` in [`formats`] — and know nothing about
/// the engine: an import is a parse and a file copy, and everything that needs
/// the launcher is expressed as the [`Recipe`] the blueprint carries.
pub(crate) trait Format: Send + Sync {
    /// What this format is called on the wire.
    fn id(&self) -> ImportFormat;

    /// The archive-relative file whose presence identifies this format. Marker
    /// names must be specific: finding one inside somebody else's archive has
    /// to mean the archive *is* this, which is why hestia's own is
    /// `hestia.instance.json` rather than `instance.json`.
    fn marker(&self) -> &'static str;

    /// For a marker name too generic to identify the format by presence alone.
    fn claims(&self, _reader: &mut Reader, _prefix: &str) -> bool {
        true
    }

    /// Parse the archive's manifest. Reads only — nothing exists on disk yet.
    fn read(&self, reader: &mut Reader, prefix: &str) -> Result<Blueprint>;

    /// Put the archive's files into the instance that now exists. A format
    /// whose recipe is [`Recipe::Pack`] never gets here.
    fn land(
        &self,
        reader: &mut Reader,
        prefix: &str,
        target: &Target<'_>,
        job: &Job<'_>,
    ) -> Result<Landed>;
}

/// A recognised archive: which format, and the directory inside it the
/// contents start at (empty at the root). A zip made by right-clicking an
/// instance folder has everything one level down, which is the usual shape of a
/// hand-made export.
pub(crate) struct Detected {
    pub(crate) format: &'static dyn Format,
    pub(crate) prefix: String,
}

/// Recognise an archive from its member names. Detection is by marker rather
/// than by extension because every one of these formats is a zip and people
/// rename them; someone handed an archive should not have to know which
/// launcher made it.
///
/// The **shallowest** marker wins, and the registry order only breaks a tie.
/// Depth has to come first: a pack index a Prism instance happens to ship
/// inside its `config/` is a file that instance uses, not what the archive is.
///
/// `claims` is asked of each candidate in that order, so a format whose marker
/// name is not specific can read the file before the archive is called its.
pub(crate) fn detect(
    names: &[String],
    filename: &str,
    mut claims: impl FnMut(&Detected) -> bool,
) -> Result<Detected> {
    let mut candidates: Vec<(usize, Detected)> = formats()
        .iter()
        .enumerate()
        .filter_map(|(rank, format)| {
            shallowest(names, format.marker()).map(|prefix| {
                (
                    rank,
                    Detected {
                        format: *format,
                        prefix,
                    },
                )
            })
        })
        .collect();
    candidates.sort_by_key(|(rank, candidate)| (depth(&candidate.prefix), *rank));
    candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .find(|candidate| claims(candidate))
        .ok_or_else(|| {
            ErrorInfo::ArchiveUnrecognised {
                filename: filename.to_string(),
            }
            .into()
        })
}

/// The directory prefix of the shallowest member named `marker`, so a nested
/// export is found but a marker buried deeper never outranks one at the root.
fn shallowest(names: &[String], marker: &str) -> Option<String> {
    names
        .iter()
        .filter_map(|name| match name.as_str() == marker {
            true => Some(String::new()),
            false => name
                .strip_suffix(marker)
                .filter(|prefix| prefix.ends_with('/'))
                .map(str::to_string),
        })
        .min_by_key(|prefix| depth(prefix))
}

fn depth(prefix: &str) -> usize {
    prefix.matches('/').count()
}

/// What one export needs to know about the instance it is writing.
pub(crate) struct Source<'a> {
    pub(crate) record: &'a InstanceRecord,
    pub(crate) entry_dir: &'a Path,
    pub(crate) data_dir: &'a Path,
    /// Entry-relative paths the caller asked to leave out as well.
    pub(crate) exclude: &'a [String],
}

/// Write an instance out in one of the export formats.
pub(crate) fn export(
    source: &Source<'_>,
    format: ExportFormat,
    destination: &Path,
    job: &Job<'_>,
) -> Result<(Written, Vec<WarningInfo>)> {
    match format {
        ExportFormat::Hestia => hestia::export(source, destination, job).map(|w| (w, Vec::new())),
        ExportFormat::Mrpack => mrpack::export(source, destination, job),
    }
}

/// The loader an instance profile runs, as an archive describes it: the flavor
/// *is* the loader's name, and `vanilla` is the absence of one.
pub(crate) fn loader_of(profile: &InstanceProfile) -> String {
    match profile.flavor.as_str() {
        "vanilla" => String::new(),
        flavor => flavor.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn each_format_is_recognised_by_its_own_marker() {
        let hestia = detect(
            &names(&["hestia.instance.json", "data/options.txt"]),
            "a",
            |_| true,
        )
        .unwrap();
        assert_eq!(hestia.format.id(), ImportFormat::Hestia);
        assert!(hestia.prefix.is_empty());

        let mrpack = detect(
            &names(&["modrinth.index.json", "overrides/config/x"]),
            "a",
            |_| true,
        )
        .unwrap();
        assert_eq!(mrpack.format.id(), ImportFormat::Mrpack);

        let prism = detect(&names(&["instance.cfg", "mmc-pack.json"]), "a", |_| true).unwrap();
        assert_eq!(prism.format.id(), ImportFormat::Prism);
    }

    #[test]
    fn a_nested_instance_is_found_and_its_directory_reported() {
        let detected = detect(
            &names(&[
                "My Pack/",
                "My Pack/instance.cfg",
                "My Pack/.minecraft/options.txt",
            ]),
            "a",
            |_| true,
        )
        .unwrap();
        assert_eq!(detected.format.id(), ImportFormat::Prism);
        assert_eq!(detected.prefix, "My Pack/");
    }

    #[test]
    fn a_marker_at_the_root_outranks_one_buried_deeper() {
        let detected = detect(
            &names(&["instance.cfg", "nested/deeper/instance.cfg"]),
            "a",
            |_| true,
        )
        .unwrap();
        assert_eq!(detected.prefix, "");
    }

    #[test]
    fn a_pack_index_the_instance_merely_ships_does_not_masquerade_as_the_pack() {
        let detected = detect(
            &names(&[
                "instance.cfg",
                "mmc-pack.json",
                "config/modrinth.index.json",
            ]),
            "a",
            |_| true,
        )
        .unwrap();
        assert_eq!(detected.format.id(), ImportFormat::Prism);
        assert_eq!(detected.prefix, "");
    }

    #[test]
    fn a_marker_its_format_disowns_falls_through_to_the_next_candidate() {
        let archive = names(&["manifest.json", "pack/instance.cfg"]);

        let claimed = detect(&archive, "a", |_| true).unwrap();
        assert_eq!(claimed.format.id(), ImportFormat::CurseForge);

        let disowned = detect(&archive, "a", |candidate| {
            candidate.format.id() != ImportFormat::CurseForge
        })
        .unwrap();
        assert_eq!(disowned.format.id(), ImportFormat::Prism);
    }

    #[test]
    fn an_archive_with_no_marker_is_refused_by_name() {
        let error = detect(&names(&["random.txt"]), "holiday-photos.zip", |_| true)
            .err()
            .expect("an unmarked archive is not importable");
        assert!(error.to_string().contains("holiday-photos.zip"));
    }

    #[test]
    fn every_registered_format_has_a_distinct_marker_and_id() {
        let markers: Vec<&str> = formats().iter().map(|f| f.marker()).collect();
        let ids: Vec<ImportFormat> = formats().iter().map(|f| f.id()).collect();
        for (index, marker) in markers.iter().enumerate() {
            assert!(
                !markers[index + 1..].contains(marker),
                "two formats claim the marker {marker}"
            );
            assert!(
                !ids[index + 1..].contains(&ids[index]),
                "two formats claim the id {:?}",
                ids[index]
            );
        }
    }
}
