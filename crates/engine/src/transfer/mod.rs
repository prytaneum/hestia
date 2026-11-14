//! Instance import and export — moving an instance in and out of the launcher
//! as a single file.
//!
//! This is the instance answer to what backups are for a server, and it is a
//! different shape on purpose: a server's data is archived *in place*, on a
//! schedule, because a server is infrastructure that has to be recoverable. An
//! instance is something you play, share, and move between machines, so its
//! archive is one portable file that says what it is.
//!
//! Hestia's own archive is the only instance format. Every other format is a
//! plugin's `pack` provider ([`crate::plugins::PluginPack`]), which declares the
//! members it reads and answers what the archive holds. [`detect`] ranks them
//! beside hestia's own and the modpack flow receives a pack archive already
//! open.
//!
//! Export is the same shape: [`Exporter`] picks a writer, and the rules for
//! what an archive leaves out are shared ([`exclude`]) so two exporters cannot
//! drift on the question of what an instance's files actually are.

pub(crate) mod archive;
pub(crate) mod contents;
pub(crate) mod exclude;
pub(crate) mod hestia;
pub(crate) mod plugin;
pub(crate) mod pool;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use proto::content::ResolvedModpack;
use proto::error::ErrorInfo;
use proto::provider::Provider;
use proto::sync::SyncOverrides;
use proto::transfer::ImportFormat;
use proto::warning::WarningInfo;

use crate::cancel::Job;
use crate::content::pack::shallowest;
use crate::instances::InstanceRecord;
use crate::jvm::JavaSettings;
use crate::minecraft::profile::InstanceProfile;
use crate::minecraft::Loaders;
use crate::plugins::PluginPack;
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
    pub(crate) jvm: JavaSettings,
    pub(crate) carried: Carried,
}

/// The identity of an instance that travels with its archive.
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

/// Where a landing puts files. The directory exists by the time a format is
/// asked to land, because the instance has already been registered.
pub(crate) struct Target<'a> {
    pub(crate) entry_dir: &'a Path,
}

#[derive(Clone)]
pub(crate) enum Kind {
    Hestia,
    Plugin(Arc<PluginPack>),
}

impl Kind {
    pub(crate) fn id(&self) -> ImportFormat {
        match self {
            Kind::Hestia => ImportFormat::Hestia,
            Kind::Plugin(pack) => {
                ImportFormat::Provider(Provider::new(pack.id(), &pack.info().name))
            }
        }
    }

    fn is_pack(&self) -> bool {
        matches!(self, Kind::Plugin(_))
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

/// The formats an archive could be, from its member names alone, shallowest
/// marker first. Detection is by marker rather than by extension because every
/// one of these formats is a zip and people rename them; someone handed an
/// archive should not have to know which launcher made it.
///
/// Depth has to come first: a manifest an archive happens to ship inside a
/// config directory is a file it uses, not what the archive is. The order below
/// only breaks a tie, so a plugin's format ranks after hestia's at equal depth,
/// and none can claim hestia's marker.
fn candidates(names: &[String], packs: &[Arc<PluginPack>]) -> Vec<Detected> {
    let mut found: Vec<Detected> = shallowest(names, hestia::MANIFEST)
        .map(|prefix| Detected {
            kind: Kind::Hestia,
            prefix,
        })
        .into_iter()
        .collect();
    found.extend(packs.iter().filter_map(|pack| {
        pack.root_of(names).map(|prefix| Detected {
            kind: Kind::Plugin(pack.clone()),
            prefix,
        })
    }));
    found.sort_by_key(|candidate| candidate.prefix.matches('/').count());
    found
}

/// Recognise an archive. A plugin's format that holds its declared members is
/// asked whether it claims the archive, given the text of those members, before
/// it wins; `packs_only` leaves the instance formats out, for a caller that
/// installs a modpack.
pub(crate) async fn detect(
    reader: &mut Reader,
    filename: &str,
    packs: &[Arc<PluginPack>],
    packs_only: bool,
) -> Result<Detected> {
    for candidate in candidates(&reader.names(), packs) {
        if packs_only && !candidate.kind.is_pack() {
            continue;
        }
        if let Kind::Plugin(pack) = &candidate.kind {
            if !pack.claims(reader, &candidate.prefix).await {
                tracing::debug!(pack = %pack.id(), "a pack format did not claim the archive");
                continue;
            }
        }
        return Ok(candidate);
    }
    Err(ErrorInfo::ArchiveUnrecognised {
        filename: filename.to_string(),
    }
    .into())
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

/// Who writes an export.
#[derive(Clone)]
pub(crate) enum Exporter {
    Hestia,
    Plugin(Arc<PluginPack>),
}

impl Exporter {
    /// The file extension an export of this format is given.
    pub(crate) fn extension(&self) -> &str {
        match self {
            Exporter::Hestia => "hestia",
            Exporter::Plugin(pack) => &pack.info().extension,
        }
    }

    pub(crate) fn id(&self) -> &str {
        match self {
            Exporter::Hestia => "hestia",
            Exporter::Plugin(pack) => pack.id(),
        }
    }
}

/// Write an instance out in one of the export formats.
pub(crate) async fn export(
    source: &Source<'_>,
    exporter: &Exporter,
    destination: &Path,
    loaders: &Loaders,
    job: &Job<'_>,
) -> Result<(Written, Vec<WarningInfo>)> {
    match exporter {
        Exporter::Hestia => hestia::export(source, destination, job).map(|w| (w, Vec::new())),
        Exporter::Plugin(pack) => plugin::export(source, pack, destination, loaders, job).await,
    }
}

/// The loader an instance profile runs, as an archive describes it: the
/// flavor's provider-id, so the archive does not depend on which plugin
/// supplied the flavor, and `vanilla` is the absence of one.
pub(crate) fn loader_of(profile: &InstanceProfile) -> String {
    match proto::naming::provider_id(&profile.flavor) {
        "vanilla" => String::new(),
        flavor => flavor.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader_of(list: &[&str]) -> (tempfile::TempDir, Reader) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in list {
            match name.ends_with('/') {
                true => zip.add_directory(*name, options).unwrap(),
                false => zip.start_file(*name, options).unwrap(),
            }
        }
        zip.finish().unwrap();
        let reader = Reader::open(&path).unwrap();
        (dir, reader)
    }

    #[tokio::test]
    async fn a_nested_hestia_archive_is_found_and_its_directory_reported() {
        let (_dir, mut reader) = reader_of(&[
            "Cozy/",
            "Cozy/hestia.instance.json",
            "Cozy/data/options.txt",
        ]);
        let detected = detect(&mut reader, "a", &[], false).await.unwrap();
        assert_eq!(detected.kind.id(), ImportFormat::Hestia);
        assert_eq!(detected.prefix, "Cozy/");
    }

    #[tokio::test]
    async fn without_a_pack_plugin_a_foreign_archive_is_unrecognised() {
        let (_dir, mut reader) = reader_of(&["modrinth.index.json", "overrides/config/x"]);
        assert!(detect(&mut reader, "a", &[], false).await.is_err());

        let (_dir, mut reader) = reader_of(&["hestia.instance.json"]);
        assert!(detect(&mut reader, "a", &[], true).await.is_err());
    }
}
