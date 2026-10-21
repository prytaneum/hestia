//! A server's game directory as a file tree. Every path is relative to that
//! directory and `/`-separated on every platform; the empty path is its root.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};
use crate::plugin::Permission;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub dir: bool,
    /// Zero for a directory.
    pub size: u64,
    pub modified_unix: i64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerFileParams {
    pub server: String,
    pub path: String,
}

/// Directories first, then by name.
pub struct ServerFilesList;
impl Contract for ServerFilesList {
    const CHANNEL: &'static str = "server.files.list";
    const PLUGIN: Option<Permission> = Some(Permission::EntryRead);
    type Params = ServerFileParams;
    type Result = Vec<FileEntry>;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct FileReadResult {
    pub content: String,
}

/// Reads a text file whole. A binary file or one past the editable size is
/// refused rather than truncated.
pub struct ServerFilesRead;
impl Contract for ServerFilesRead {
    const CHANNEL: &'static str = "server.files.read";
    type Params = ServerFileParams;
    type Result = FileReadResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerFileWriteParams {
    pub server: String,
    pub path: String,
    pub content: String,
}

/// Creates the file or replaces its content.
pub struct ServerFilesWrite;
impl Contract for ServerFilesWrite {
    const CHANNEL: &'static str = "server.files.write";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    type Params = ServerFileWriteParams;
    type Result = Empty;
}

pub struct ServerFilesMkdir;
impl Contract for ServerFilesMkdir {
    const CHANNEL: &'static str = "server.files.mkdir";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    type Params = ServerFileParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerFileTransferParams {
    pub server: String,
    pub from: String,
    pub to: String,
}

/// Renames or moves; never replaces what is already at `to`.
pub struct ServerFilesMove;
impl Contract for ServerFilesMove {
    const CHANNEL: &'static str = "server.files.move";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    /// A world is copied or archived inside one call, so every verb that moves
    /// a whole tree waits far longer than a plain request.
    const TIMEOUT: Duration = Duration::from_secs(600);
    type Params = ServerFileTransferParams;
    type Result = Empty;
}

pub struct ServerFilesCopy;
impl Contract for ServerFilesCopy {
    const CHANNEL: &'static str = "server.files.copy";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    const TIMEOUT: Duration = Duration::from_secs(600);
    type Params = ServerFileTransferParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerFilesRemoveParams {
    pub server: String,
    pub paths: Vec<String>,
}

pub struct ServerFilesRemove;
impl Contract for ServerFilesRemove {
    const CHANNEL: &'static str = "server.files.remove";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    const TIMEOUT: Duration = Duration::from_secs(600);
    type Params = ServerFilesRemoveParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerFilesImportParams {
    pub server: String,
    /// The directory the sources land in.
    pub path: String,
    /// Absolute paths on the daemon's machine, files or directories.
    pub sources: Vec<String>,
}

/// Copies files from elsewhere on the machine into the tree — what a hosted
/// panel calls an upload.
pub struct ServerFilesImport;
impl Contract for ServerFilesImport {
    const CHANNEL: &'static str = "server.files.import";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    const TIMEOUT: Duration = Duration::from_secs(600);
    type Params = ServerFilesImportParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerFilesCompressParams {
    pub server: String,
    pub paths: Vec<String>,
    /// The `.zip` to write.
    pub to: String,
}

pub struct ServerFilesCompress;
impl Contract for ServerFilesCompress {
    const CHANNEL: &'static str = "server.files.compress";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    const TIMEOUT: Duration = Duration::from_secs(600);
    type Params = ServerFilesCompressParams;
    type Result = Empty;
}

/// Unpacks the `.zip` or `.tar.gz` at `from` into the directory `to`.
pub struct ServerFilesExtract;
impl Contract for ServerFilesExtract {
    const CHANNEL: &'static str = "server.files.extract";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    const TIMEOUT: Duration = Duration::from_secs(600);
    type Params = ServerFileTransferParams;
    type Result = Empty;
}
