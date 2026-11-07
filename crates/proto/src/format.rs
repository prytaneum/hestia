//! What a plugin's `archive-format` export says and answers, as JSON. The host
//! owns every archive: the guest sees the manifest bytes and the member names,
//! and answers with a blueprint and with which members go where.

use serde::{Deserialize, Serialize};

/// Whether an archive is an instance or a modpack.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum FormatKind {
    #[default]
    Instance,
    Pack,
}

/// What a plugin's format says of itself: the `info` of the `archive-format`
/// export. The host registers it as `<plugin>.<id>`.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct FormatInfo {
    pub name: String,
    pub kind: FormatKind,
    /// The archive member whose presence identifies the format, e.g. `instance.cfg`.
    pub marker: String,
}

/// An instance as an archive describes it: the answer of `read`, and the
/// description `write` is given.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct FormatBlueprint {
    pub name: String,
    pub game_version: String,
    /// The loader's own name (`fabric`, `neoforge`), empty for vanilla.
    pub loader: String,
    pub loader_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    pub jvm_args: Vec<String>,
}

/// One file the host copies. On import `archive_path` is a member of the
/// archive and `entry_path` is relative to the instance's game directory; on export
/// the direction is reversed.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct FormatCopy {
    pub archive_path: String,
    pub entry_path: String,
}

/// The answer of `write`: the manifest the host stores at the format's marker,
/// and the files it copies beside it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct FormatWritten {
    pub manifest: Vec<u8>,
    pub mapping: Vec<FormatCopy>,
}
