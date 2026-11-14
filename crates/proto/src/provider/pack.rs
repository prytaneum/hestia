//! The `pack` provider kind: an archive format that carries a game version, a
//! loader and the files to run it with, such as a Modrinth or CurseForge pack
//! or another launcher's instance. The host owns every archive: the guest is
//! handed the member names and the text of the members it declared, and every
//! path, URL and tree it answers is vetted before a byte is copied or fetched.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ProviderKind, ProviderOp};
use crate::content::{InstalledContent, ResolvedModpack};
use crate::contract::Empty;
use crate::plugin::Permission;

pub struct Pack;
impl ProviderKind for Pack {
    const KIND: &'static str = "pack";
    const PERMISSIONS: &'static [Permission] = &[Permission::PackProvide];
}

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

/// The member names of an archive, relative to the directory the pack would
/// start at.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct ClaimQuery {
    pub files: Vec<String>,
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

/// Describes the format.
pub struct Info;
impl ProviderOp for Info {
    type Kind = Pack;
    const OP: &'static str = "info";
    type Args = Empty;
    type Reply = PackInfo;
}

/// Whether the archive is this format's, once a declared member is present.
/// Optional: unsupported claims it.
pub struct Claims;
impl ProviderOp for Claims {
    type Kind = Pack;
    const OP: &'static str = "claims";
    type Args = ClaimQuery;
    type Reply = Claim;
}

/// Reads the archive into the pack it describes. A file named here is held to
/// the same rule as any download a plugin names: its path stays inside the
/// entry, and its URL is on one of the plugin's network hosts and verified by
/// a checksum, or empty for a file its author withholds. A tree that escapes
/// the pack's directory, or a delegation to a source that is not there,
/// refuses the answer whole.
pub struct Read;
impl ProviderOp for Read {
    type Kind = Pack;
    const OP: &'static str = "read";
    type Args = ReadQuery;
    type Reply = PackRead;
}

/// Writes an export: the manifests the host stores, and which of the instance's
/// files go beside them. Optional while [`PackInfo::extension`] is empty.
pub struct Write;
impl ProviderOp for Write {
    type Kind = Pack;
    const OP: &'static str = "write";
    type Args = WriteQuery;
    type Reply = PackWritten;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_matches_whole_segments() {
        assert!(member_matches("manifest.json", "manifest.json"));
        assert!(!member_matches("manifest.json", "pack/manifest.json"));
        assert!(member_matches("*.cfg", "instance.cfg"));
        assert!(member_matches(".minecraft/*.json", ".minecraft/a.json"));
        assert!(member_matches("a*c*e", "abcde"));
        assert!(!member_matches("*.cfg", "dir/instance.cfg"));
        assert!(!member_matches("*.cfg", "instance.cfg.bak"));
        assert!(!member_matches("a*b", "a"));
    }

    #[test]
    fn a_pattern_is_a_plain_relative_path() {
        for good in ["manifest.json", "*.cfg", "a/b/*.json"] {
            assert!(is_member_pattern(good), "{good}");
        }
        for bad in ["", "/abs", "a//b", "../x", "a/./b", "a\\b", "a/"] {
            assert!(!is_member_pattern(bad), "{bad}");
        }
    }
}
