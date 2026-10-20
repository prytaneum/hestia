//! Server backup contracts: archive a server's `data/` directory on demand,
//! list/restore/remove the stored archives. Create and restore are jobs — the
//! call answers with the job id and progress/done/error arrive as events, like
//! the other provisioning jobs. Servers additionally back up on a schedule and
//! before a version update — the schedule is a `Backup` task on the server
//! (`crate::tasks`), not a channel of its own. Backups are a **server** feature: instances have none — an
//! instance travels as one archive instead (`crate::transfer`).

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};
use crate::job::{Job, JobStarted};
use crate::minecraft::ProvisionProgress;
use crate::plugin::Permission;
use crate::server::ServerRef;

/// Why a backup was taken. Encoded into the archive's id, so the disk stays
/// the registry.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum BackupKind {
    #[default]
    Manual,
    Scheduled,
    /// Taken automatically before a version update.
    Update,
}

impl BackupKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BackupKind::Manual => "manual",
            BackupKind::Scheduled => "scheduled",
            BackupKind::Update => "update",
        }
    }

    pub fn parse(value: &str) -> Option<BackupKind> {
        match value {
            "manual" => Some(BackupKind::Manual),
            "scheduled" => Some(BackupKind::Scheduled),
            "update" => Some(BackupKind::Update),
            _ => None,
        }
    }
}

/// One stored backup archive.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct BackupInfo {
    pub id: String,
    pub kind: BackupKind,
    pub created_unix: i64,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerBackupCreateParams {
    /// Server name or id.
    pub server: String,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerBackupRestoreParams {
    /// Server name or id.
    pub server: String,
    /// Backup id (`server.backup.list` names them).
    pub backup: String,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

/// Names one backup of one server (remove).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerBackupRef {
    pub server: String,
    pub backup: String,
}

/// A job. A cancel reclaims the archive being written.
pub struct ServerBackupCreate;
impl Contract for ServerBackupCreate {
    const CHANNEL: &'static str = "server.backup.create";
    const PLUGIN: Option<Permission> = Some(Permission::EntriesWrite);
    type Params = ServerBackupCreateParams;
    type Result = JobStarted;
}

pub struct ServerBackupList;
impl Contract for ServerBackupList {
    const CHANNEL: &'static str = "server.backup.list";
    const PLUGIN: Option<Permission> = Some(Permission::EntriesRead);
    type Params = ServerRef;
    type Result = Vec<BackupInfo>;
}

/// A job. A cancel discards the staging directory, so the live data is
/// untouched.
pub struct ServerBackupRestore;
impl Contract for ServerBackupRestore {
    const CHANNEL: &'static str = "server.backup.restore";
    const PLUGIN: Option<Permission> = Some(Permission::EntriesWrite);
    type Params = ServerBackupRestoreParams;
    type Result = JobStarted;
}

pub struct ServerBackupRemove;
impl Contract for ServerBackupRemove {
    const CHANNEL: &'static str = "server.backup.remove";
    const PLUGIN: Option<Permission> = Some(Permission::EntriesWrite);
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = ServerBackupRef;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct BackupDone {
    pub backup: BackupInfo,
}

impl Job for ServerBackupCreate {
    type Progress = ProvisionProgress;
    type Done = BackupDone;
}

impl Job for ServerBackupRestore {
    type Progress = ProvisionProgress;
    type Done = BackupDone;
}
