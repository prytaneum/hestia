//! A server's scheduled tasks: a restart or a console command the daemon runs
//! on a schedule while the server is up. Scheduled *backups* are not tasks —
//! they carry retention and are configured through `server.config`.

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};
use crate::server::ServerRef;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    /// Stop the server and start it again.
    #[default]
    Restart,
    /// Send `command` to the console.
    Command,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerTask {
    /// Assigned by the daemon; empty on a task that is being created.
    pub id: String,
    pub action: TaskAction,
    /// The console command; unused by a restart.
    pub command: String,
    /// An interval (`30m`, `6h`, `1d`) or a daily time (`04:30`, the machine's
    /// local time).
    pub schedule: String,
    pub enabled: bool,
    /// When it last ran — or, before its first run, when it was saved, which
    /// is what an interval counts from.
    pub last_run_unix: i64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerTasksResult {
    pub tasks: Vec<ServerTask>,
}

pub struct ServerTaskList;
impl Contract for ServerTaskList {
    const CHANNEL: &'static str = "server.task.list";
    type Params = ServerRef;
    type Result = ServerTasksResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerTaskSaveParams {
    pub server: String,
    pub task: ServerTask,
}

/// Creates the task when its id is empty, replaces the one with that id
/// otherwise. Answers with the task as stored.
pub struct ServerTaskSave;
impl Contract for ServerTaskSave {
    const CHANNEL: &'static str = "server.task.save";
    type Params = ServerTaskSaveParams;
    type Result = ServerTask;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerTaskRemoveParams {
    pub server: String,
    pub id: String,
}

pub struct ServerTaskRemove;
impl Contract for ServerTaskRemove {
    const CHANNEL: &'static str = "server.task.remove";
    type Params = ServerTaskRemoveParams;
    type Result = Empty;
}
