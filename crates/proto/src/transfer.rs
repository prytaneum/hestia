//! Instance import and export: moving an instance out of the launcher as a
//! single archive, and bringing one back — from hestia's own export or from
//! another launcher's.
//!
//! Export is the instance side of what backups are for a server, which is why
//! instances have no backup channels of their own. Import is deliberately
//! *format-detecting* rather than format-declared: the caller names a file, the
//! daemon reads its marker and answers with what it found, so a person who was
//! handed a `.mrpack` and a person who zipped a Prism instance both run the
//! same command.
//!
//! Both are jobs — an archive is an arbitrary number of files and an import may
//! download every mod a pack names — so the call answers with a job id and the
//! outcome arrives as an event, like every other long-running operation.

use serde::{Deserialize, Serialize};

use crate::content::ContentFailure;
use crate::contract::{Contract, Topic};
use crate::error::ErrorInfo;
use crate::instance::InstanceInfo;
use crate::minecraft::ProvisionProgress;
use crate::warning::WarningInfo;

/// What an export writes. `Hestia` is the full-fidelity archive — the one that
/// round-trips an instance whole; `Mrpack` is the portable one other launchers
/// read, and carries only what can be expressed as a Modrinth pack.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    #[default]
    Hestia,
    Mrpack,
}

impl ExportFormat {
    /// The file extension an export of this format is given.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Hestia => "hestia",
            ExportFormat::Mrpack => "mrpack",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ExportFormat::Hestia => "hestia",
            ExportFormat::Mrpack => "mrpack",
        }
    }

    pub fn parse(value: &str) -> Option<ExportFormat> {
        match value {
            "hestia" => Some(ExportFormat::Hestia),
            "mrpack" | "modrinth" => Some(ExportFormat::Mrpack),
            _ => None,
        }
    }
}

/// What an archive turned out to be, recognised from a marker file inside it
/// rather than from its extension — the extension is whatever the person who
/// sent it happened to save it as.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ImportFormat {
    /// hestia's own export (`hestia.instance.json`).
    #[default]
    Hestia,
    /// A Modrinth pack (`modrinth.index.json`) — installed through the ordinary
    /// modpack path, so its mods join the pool as updatable content.
    Mrpack,
    /// A Prism Launcher / MultiMC / PolyMC instance (`instance.cfg`).
    Prism,
    /// A CurseForge pack (`manifest.json`), installed like a Modrinth one.
    #[serde(rename = "curseforge")]
    CurseForge,
}

impl ImportFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportFormat::Hestia => "hestia",
            ImportFormat::Mrpack => "mrpack",
            ImportFormat::Prism => "prism",
            ImportFormat::CurseForge => "curseforge",
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceExportParams {
    /// Instance name or id.
    pub instance: String,
    pub format: ExportFormat,
    /// Absolute, daemon-local destination file. Empty writes into the data
    /// home's `exports/` under a generated name — the daemon's own directory is
    /// the only place it can be sure it may write.
    pub destination: String,
    /// Extra entry-relative paths to leave out, on top of the regenerable and
    /// transient ones an export always skips.
    pub exclude: Vec<String>,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceImportParams {
    /// An absolute, daemon-local archive path. The daemon reads the file, so
    /// only a path it can see is meaningful.
    pub path: String,
    /// Display name for the new instance; empty takes the archive's own.
    pub name: String,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

/// The immediate answer of an export/import call: the job whose events carry
/// the outcome.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct TransferJobResult {
    pub id: String,
}

/// What an archive says about itself, without importing it: enough for a
/// front-end to name the instance it is about to create and to say what the
/// archive actually is before anything is written.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ArchiveInfo {
    pub format: ImportFormat,
    /// The instance/pack name the archive carries; empty when it names none.
    pub name: String,
    pub game_version: String,
    /// The loader the archive pins, empty for vanilla.
    pub loader: String,
    pub loader_version: String,
    /// Whether a name is already taken by an existing instance, so a front-end
    /// can ask for another one before starting a job that would be refused.
    pub name_taken: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ArchiveRef {
    pub path: String,
}

/// One node of what an export *would* carry, so a caller can leave part of it
/// out knowingly. Derived from the same file plan the export writes, so the two
/// cannot disagree about what an instance's files are.
///
/// The tree stops a few levels down (deep enough to name a single world, not so
/// deep that a config tree becomes thousands of nodes): a directory at the
/// bottom is a leaf carrying the total of everything inside it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ArchiveEntry {
    /// Entry-relative path — what `InstanceExportParams::exclude` takes.
    pub path: String,
    /// The last segment, for display.
    pub name: String,
    pub directory: bool,
    /// Bytes this node contributes to the archive; for a directory, everything
    /// under it that the export would carry.
    pub size_bytes: u64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ExportContentsResult {
    /// Sorted by path, so a caller can build the tree in one pass.
    pub entries: Vec<ArchiveEntry>,
}

pub struct InstanceExport;
impl Contract for InstanceExport {
    const CHANNEL: &'static str = "instance.export";
    type Params = InstanceExportParams;
    type Result = TransferJobResult;
}

pub struct InstanceExportContents;
impl Contract for InstanceExportContents {
    const CHANNEL: &'static str = "instance.export.contents";
    type Params = crate::instance::InstanceRef;
    type Result = ExportContentsResult;
}

pub struct InstanceImport;
impl Contract for InstanceImport {
    const CHANNEL: &'static str = "instance.import";
    type Params = InstanceImportParams;
    type Result = TransferJobResult;
}

pub struct InstanceImportInspect;
impl Contract for InstanceImportInspect {
    const CHANNEL: &'static str = "instance.import.inspect";
    type Params = ArchiveRef;
    type Result = ArchiveInfo;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ExportProgressEvent {
    pub id: String,
    #[serde(flatten)]
    pub progress: ProvisionProgress,
}
impl Topic for ExportProgressEvent {
    const TOPIC: &'static str = "instance.export.progress";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ExportDoneEvent {
    pub id: String,
    /// Where the archive was written — the daemon picked it when the request
    /// named no destination, so this is the only place the caller learns it.
    pub path: String,
    pub size_bytes: u64,
    pub files: u64,
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}
impl Topic for ExportDoneEvent {
    const TOPIC: &'static str = "instance.export.done";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ExportErrorEvent {
    pub id: String,
    /// The structured cause a front-end localizes from.
    pub error: ErrorInfo,
}
impl Topic for ExportErrorEvent {
    const TOPIC: &'static str = "instance.export.error";
}

/// An export stopped at the caller's request. The archive it was writing is
/// discarded — an export is written through a `.part` and renamed, so a
/// cancelled one leaves nothing behind.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ExportCancelledEvent {
    pub id: String,
}
impl Topic for ExportCancelledEvent {
    const TOPIC: &'static str = "instance.export.cancelled";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ImportProgressEvent {
    pub id: String,
    #[serde(flatten)]
    pub progress: ProvisionProgress,
}
impl Topic for ImportProgressEvent {
    const TOPIC: &'static str = "instance.import.progress";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ImportDoneEvent {
    pub id: String,
    pub format: ImportFormat,
    pub instance: InstanceInfo,
    /// Per-item failures of an import that installs content (a pack's mods):
    /// the rest of the instance landed, these did not.
    #[serde(default)]
    pub failures: Vec<ContentFailure>,
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}
impl Topic for ImportDoneEvent {
    const TOPIC: &'static str = "instance.import.done";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ImportErrorEvent {
    pub id: String,
    /// The structured cause a front-end localizes from.
    pub error: ErrorInfo,
}
impl Topic for ImportErrorEvent {
    const TOPIC: &'static str = "instance.import.error";
}

/// An import stopped at the caller's request. A half-built instance is removed
/// rather than left registered: an entry that never finished importing is not
/// one the user asked for.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ImportCancelledEvent {
    pub id: String,
}
impl Topic for ImportCancelledEvent {
    const TOPIC: &'static str = "instance.import.cancelled";
}
