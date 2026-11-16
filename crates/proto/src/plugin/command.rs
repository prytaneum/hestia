//! A plugin's commands: what its manifest declares and the job that runs one.

use serde::{Deserialize, Serialize};

use crate::contract::Contract;
use crate::job::{Job, JobStarted};

/// The kind of value a command argument takes.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ArgKind {
    #[default]
    String,
    Integer,
    Boolean,
}

impl std::fmt::Display for ArgKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ArgKind::String => "string",
            ArgKind::Integer => "integer",
            ArgKind::Boolean => "boolean",
        })
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct ArgSpec {
    pub name: String,
    pub kind: ArgKind,
    pub required: bool,
    pub summary: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct CommandSpec {
    pub name: String,
    pub summary: String,
    pub args: Vec<ArgSpec>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginRunParams {
    /// The job id its events carry, and the scope the notices, questions and
    /// links of the run are delivered in; empty generates one.
    pub id: String,
    pub plugin: String,
    pub command: String,
    /// A JSON object of the command's declared arguments.
    pub args: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginRunProgress {
    pub current: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginRunDone {}

/// Runs one of a plugin's commands as a person's request, so it may ask the
/// user to confirm. A job: a cancel is raised for the plugin to see between
/// steps, and a plugin still running 5 s later is dropped. Either way the job
/// is `cancelled`: what the plugin already wrote to its own files stays, and the
/// named locks it held are released. The command's own hour is the only other
/// stop.
pub struct PluginRun;
impl Contract for PluginRun {
    const CHANNEL: &'static str = "plugin.run";
    type Params = PluginRunParams;
    type Result = JobStarted;
}

impl Job for PluginRun {
    type Progress = PluginRunProgress;
    type Done = PluginRunDone;
}
