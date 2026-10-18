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
//! One module beside this one and one line in [`detect`]. A format answers
//! three questions and nothing else:
//!
//! 1. **which archives are mine** — a [`Format::marker`] file, matched by name;
//! 2. **what does this one say it is** — [`Format::read`], which parses the
//!    archive's own manifest into a [`Blueprint`];
//! 3. **where do the files go** — [`Format::land`], given an instance that now
//!    exists on disk.
//!
//! What a format never does is create the instance. That is the launcher's
//! job, and the [`Recipe`] a blueprint carries is everything it needs: a loader
//! to resolve with the descriptor's game version, and the identity some formats
//! carry. The flow reads the recipe, not the format, so another format costs
//! the flow nothing.
//!
//! A modpack is not an instance format. Whether a zip is one, in which format
//! and at what root is [`crate::content::pack`]'s single answer; [`detect`]
//! ranks that candidate beside the instance formats and the modpack flow
//! receives the archive already open.
//!
//! Export is the same shape with fewer moving parts: [`ExportFormat`] picks a
//! writer, and the rules for what an archive leaves out are shared
//! ([`exclude`]) so two exporters cannot drift on the question of what an
//! instance's files actually are.

pub(crate) mod archive;
pub(crate) mod contents;
pub(crate) mod exclude;
pub(crate) mod hestia;
pub(crate) mod mrpack;
pub(crate) mod pool;
pub(crate) mod prism;

use std::path::Path;

use anyhow::Result;
use proto::content::ResolvedModpack;
use proto::error::ErrorInfo;
use proto::sync::SyncOverrides;
use proto::transfer::{ExportFormat, ImportFormat};
use proto::warning::WarningInfo;

use crate::cancel::Job;
use crate::content::pack;
use crate::content::pack::shallowest;
use crate::instances::InstanceRecord;
use crate::jvm::JavaSettings;
use crate::minecraft::profile::InstanceProfile;
use archive::{Reader, Written};

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

/// How the launcher brings an archive's instance into existence: the game
/// version and loader resolve exactly as `instance create` resolves them.
pub(crate) struct Recipe {
    /// Per-instance Java settings the archive carried, where its launcher
    /// stored any.
    pub(crate) jvm: JavaSettings,
    pub(crate) carried: Option<Carried>,
}

/// The identity of an instance that travels with its archive; hestia's own
/// export is the only format that has one.
pub(crate) struct Carried {
    pub(crate) created_unix: i64,
    pub(crate) last_played_unix: Option<i64>,
    pub(crate) playtime_seconds: i64,
    pub(crate) sharing: SyncOverrides,
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

    /// Parse the archive's manifest. Reads only — nothing exists on disk yet.
    fn read(&self, reader: &mut Reader, prefix: &str) -> Result<Blueprint>;

    /// Put the archive's files into the instance that now exists.
    fn land(
        &self,
        reader: &mut Reader,
        prefix: &str,
        target: &Target<'_>,
        job: &Job<'_>,
    ) -> Result<Landed>;
}

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Instance(&'static dyn Format),
    Pack(pack::Format),
}

impl Kind {
    pub(crate) fn id(self) -> ImportFormat {
        match self {
            Kind::Instance(format) => format.id(),
            Kind::Pack(format) => format.id(),
        }
    }
}

/// A recognised archive: what it is, and the directory inside it the
/// contents start at (empty at the root). A zip made by right-clicking an
/// instance folder has everything one level down, which is the usual shape of a
/// hand-made export.
pub(crate) struct Detected {
    pub(crate) kind: Kind,
    pub(crate) prefix: String,
}

/// Recognise an archive from its member names. Detection is by marker rather
/// than by extension because every one of these formats is a zip and people
/// rename them; someone handed an archive should not have to know which
/// launcher made it.
///
/// The **shallowest** marker wins, and the order below only breaks a tie.
/// Depth has to come first: a pack index a Prism instance happens to ship
/// inside its `config/` is a file that instance uses, not what the archive is.
///
/// `claims_manifest` is the pack module's question about a generic
/// `manifest.json`, answered by whoever holds the archive open.
pub(crate) fn detect(
    names: &[String],
    filename: &str,
    claims_manifest: impl FnMut(&str) -> bool,
) -> Result<Detected> {
    let instance = |format: &'static dyn Format| {
        shallowest(names, format.marker()).map(|prefix| Detected {
            kind: Kind::Instance(format),
            prefix,
        })
    };
    let pack = pack::locate(names, claims_manifest).map(|(format, root)| Detected {
        kind: Kind::Pack(format),
        prefix: root,
    });
    let mut candidates: Vec<Detected> = [instance(&hestia::Hestia), pack, instance(&prism::Prism)]
        .into_iter()
        .flatten()
        .collect();
    candidates.sort_by_key(|candidate| candidate.prefix.matches('/').count());
    candidates.into_iter().next().ok_or_else(|| {
        ErrorInfo::ArchiveUnrecognised {
            filename: filename.to_string(),
        }
        .into()
    })
}

impl From<ResolvedModpack> for Descriptor {
    fn from(pack: ResolvedModpack) -> Self {
        Descriptor {
            name: pack.name,
            game_version: pack.game_version,
            loader: pack.loader.unwrap_or_default(),
            loader_version: pack.loader_version.unwrap_or_default(),
        }
    }
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
        assert_eq!(hestia.kind.id(), ImportFormat::Hestia);
        assert!(hestia.prefix.is_empty());

        let mrpack = detect(
            &names(&["modrinth.index.json", "overrides/config/x"]),
            "a",
            |_| true,
        )
        .unwrap();
        assert_eq!(mrpack.kind.id(), ImportFormat::Mrpack);

        let prism = detect(&names(&["instance.cfg", "mmc-pack.json"]), "a", |_| true).unwrap();
        assert_eq!(prism.kind.id(), ImportFormat::Prism);
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
        assert_eq!(detected.kind.id(), ImportFormat::Prism);
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
        assert_eq!(detected.kind.id(), ImportFormat::Prism);
        assert_eq!(detected.prefix, "");
    }

    #[test]
    fn a_manifest_that_is_not_a_pack_falls_through_to_the_next_candidate() {
        let archive = names(&["manifest.json", "pack/instance.cfg"]);

        let claimed = detect(&archive, "a", |_| true).unwrap();
        assert_eq!(claimed.kind.id(), ImportFormat::CurseForge);

        let disowned = detect(&archive, "a", |_| false).unwrap();
        assert_eq!(disowned.kind.id(), ImportFormat::Prism);
    }

    #[test]
    fn the_shallowest_pack_marker_wins_whichever_format_it_is() {
        let archive = names(&["manifest.json", "overrides/config/modrinth.index.json"]);
        let detected = detect(&archive, "a", |_| true).unwrap();
        assert_eq!(detected.kind.id(), ImportFormat::CurseForge);
    }

    #[test]
    fn an_archive_with_no_marker_is_refused_by_name() {
        let error = detect(&names(&["random.txt"]), "holiday-photos.zip", |_| true)
            .err()
            .expect("an unmarked archive is not importable");
        assert!(error.to_string().contains("holiday-photos.zip"));
    }
}
