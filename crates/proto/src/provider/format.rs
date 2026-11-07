//! The `format` provider kind: an archive format for importing and exporting
//! instances. The host owns every archive: the guest is handed the manifest
//! text and the member names, and the host vets every path it answers.

use serde::{Deserialize, Serialize};

use super::{ProviderKind, ProviderOp};
use crate::contract::Empty;
use crate::format::{FormatBlueprint, FormatCopy, FormatInfo, FormatWritten};
use crate::plugin::Permission;

pub struct Format;
impl ProviderKind for Format {
    const KIND: &'static str = "format";
    const PERMISSIONS: &'static [Permission] = &[Permission::FormatProvide];
}

/// The archive's manifest, the member named by the format's marker, and the
/// archive's member names. A manifest that is not UTF-8 text is refused by the
/// host before the guest is asked.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct ManifestQuery {
    pub manifest: String,
    pub files: Vec<String>,
}

/// An instance as an archive describes it, and the files at hand: archive
/// members on import, paths relative to the instance on export.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct BlueprintQuery {
    pub blueprint: FormatBlueprint,
    pub files: Vec<String>,
}

/// Describes the format. Asked once, when the plugin is enabled.
pub struct Info;
impl ProviderOp for Info {
    type Kind = Format;
    const OP: &'static str = "info";
    type Args = Empty;
    type Reply = FormatInfo;
}

/// Reads the manifest into the instance it describes.
pub struct Read;
impl ProviderOp for Read {
    type Kind = Format;
    const OP: &'static str = "read";
    type Args = ManifestQuery;
    type Reply = FormatBlueprint;
}

/// Names the archive members the host copies into the new instance, and where.
/// One path that escapes, or names a member not in `files`, refuses the answer.
pub struct Land;
impl ProviderOp for Land {
    type Kind = Format;
    const OP: &'static str = "land";
    type Args = BlueprintQuery;
    type Reply = Vec<FormatCopy>;
}

/// Writes an export's manifest and names the instance files the host copies
/// into the archive.
pub struct Write;
impl ProviderOp for Write {
    type Kind = Format;
    const OP: &'static str = "write";
    type Args = BlueprintQuery;
    type Reply = FormatWritten;
}
