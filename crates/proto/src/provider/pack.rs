//! The pack format vocabulary: an archive format that carries a game version, a
//! loader and the files to run it with, such as a Modrinth or CurseForge pack
//! or another launcher's instance. The host owns every archive: the guest is
//! handed the member names and the text of the members it declared, and every
//! path, URL and tree it answers is vetted before a byte is copied or fetched.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::content::{InstalledContent, ResolvedModpack};

/// What a pack format says of itself. Asked once, when the plugin is enabled.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PackInfo {
    pub name: String,
    /// The archive members it reads, relative to the directory the pack starts
    /// at: a path (`manifest.json`) or a pattern whose `*` stands for any run
    /// of characters inside one path segment (`*.cfg`, `.minecraft/*.json`). An
    /// archive none of them matches is not this format's, and the host reads
    /// the matching members as text for [`Read`].
    pub members: Vec<String>,
    /// The extension an export of this format is given, which is what makes it
    /// one the user can export to. Empty for a format that can only be read.
    pub extension: String,
}

/// Whether `pattern` is a [`PackInfo::members`] entry: a relative path of
/// segments with no `.` or `..`, no backslash and no control character.
pub fn is_member_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && !pattern.contains('\\')
        && !pattern.chars().any(char::is_control)
        && pattern
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// Whether the archive member `name` is one `pattern` declares.
pub fn member_matches(pattern: &str, name: &str) -> bool {
    let (mut patterns, mut names) = (pattern.split('/'), name.split('/'));
    loop {
        match (patterns.next(), names.next()) {
            (None, None) => return true,
            (Some(pattern), Some(name)) if segment_matches(pattern, name) => {}
            _ => return false,
        }
    }
}

fn segment_matches(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else {
        return false;
    };
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let mut parts = parts.peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return rest.ends_with(part);
        }
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.is_empty()
}

/// The member names of an archive and the text of the members the format
/// declared, relative to the directory the pack would start at, so a format can
/// look inside before it claims. A declared member that is not UTF-8 text, is
/// over the size bound or would pass the total bound is left out of `members`,
/// and the archive is not refused for it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct ClaimQuery {
    pub files: Vec<String>,
    pub members: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Claim {
    pub claimed: bool,
}

/// The archive's member names and the text of the members the format declared,
/// all relative to the directory the pack starts at. A declared member that is
/// not UTF-8 text refuses the archive before the guest is asked.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct ReadQuery {
    pub files: Vec<String>,
    pub members: BTreeMap<String, String>,
    /// The loader keys the registered flavors declare for this format in their
    /// `interchange` map, so a reader can pick the component or dependency a
    /// flavor actually claims. Keys only: which flavor owns one is the host's.
    pub loaders: Vec<String>,
}

/// A manifest a content source resolves into the pack's files.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct ManifestDelegate {
    /// The id of a content source that offers `resolve-manifest`.
    pub source: String,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub manifest: serde_json::Value,
}

/// Directories of the archive, relative to the pack's root, whose files are
/// written into the game directory as they are. Within a side the trees are
/// written in the order listed, so a later one wins a path an earlier one also
/// names; `shared` is written first.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct OverrideTrees {
    pub shared: Vec<String>,
    pub client: Vec<String>,
    pub server: Vec<String>,
}

/// What the archive is: the pack it describes, the trees it writes straight
/// into the game directory, and, when the pack names its files in a form only
/// a content source understands, which source resolves them.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PackRead {
    /// The pack's own name, game version, loader and files. Its `source`,
    /// `projectId` and `versionId` are the host's to set, and so is the
    /// identity of each file. With `resolve` set, `files` must be empty.
    pub pack: ResolvedModpack,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolve: Option<ManifestDelegate>,
    pub overrides: OverrideTrees,
    /// The pack's own memory, in the form the `memory` setting takes. Applied
    /// only to an instance the install creates; one the setting rejects is
    /// dropped with a warning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// The pack's own extra JVM flags, in the form the `jvm-args` setting
    /// takes. Applied only to an instance the install creates; flags the setting
    /// rejects are dropped with a warning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jvm_args: Option<String>,
    /// An image of the pack's own, as a path inside the archive relative to the
    /// directory the pack starts at. Applied only to an instance the install
    /// creates, held to the rules an imported launcher icon is: a path that
    /// stays inside the pack, an image the launcher can show, at most 10 MiB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// The instance an export writes.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PackInstance {
    pub name: String,
    pub game_version: String,
    /// The loader's own name (`fabric`, `neoforge`), empty for vanilla.
    pub loader: String,
    pub loader_version: String,
}

/// One item of the instance's content pool, and where its file is in the
/// game directory.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PoolItem {
    /// Relative to the game directory, such as `mods/sodium.jar`.
    pub path: String,
    /// Whether it names a download and a checksum, so a manifest can refer to
    /// it instead of carrying it.
    pub referenceable: bool,
    /// The file's size in bytes. Zero unless `referenceable`.
    pub size: u64,
    /// The file's SHA-512 as lowercase hex, beside the SHA-1 `item` carries.
    /// Empty unless `referenceable`.
    pub sha512: String,
    pub item: InstalledContent,
}

/// What an export has at hand. `files` are the game directory's own files the
/// export would carry, and `content` the pool's; a file is carried only if the
/// answer's `copies` name it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct WriteQuery {
    pub instance: PackInstance,
    pub files: Vec<String>,
    pub content: Vec<PoolItem>,
}

/// A file the guest writes into the archive.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PackMember {
    pub path: String,
    pub text: String,
}

/// A file of the instance the host copies into the archive.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PackCopy {
    /// A path of [`WriteQuery::files`] or of a [`PoolItem`].
    pub entry_path: String,
    pub archive_path: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PackWritten {
    pub members: Vec<PackMember>,
    pub copies: Vec<PackCopy>,
    /// What the export could not carry faithfully, in operational English.
    pub warnings: Vec<String>,
}
