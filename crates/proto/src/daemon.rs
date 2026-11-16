use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};
use crate::warning::WarningInfo;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct DaemonStatusResult {
    pub pid: i64,
    pub version: String,
    pub uptime_seconds: i64,
    pub home: PathBuf,
    pub log: PathBuf,
    /// Documents this daemon could not read and set aside since it started. Not
    /// caused by any one request, so they have no result of their own to ride
    /// out on and are reported here instead.
    pub quarantined: Vec<WarningInfo>,
}

pub struct DaemonStatus;
impl Contract for DaemonStatus {
    const CHANNEL: &'static str = "daemon.status";
    type Params = Empty;
    type Result = DaemonStatusResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct DaemonStopParams {
    /// When false (the default), supervised processes keep running — the daemon
    /// is restartable under live workloads, and stopping one is always a
    /// deliberate act.
    ///
    /// This is a two-valued instruction because by the time a caller reaches the
    /// channel it has decided. Deciding is the *front-end's* job: with workloads
    /// running there is a third meaning — "ask me" — which the CLI resolves by
    /// prompting (or refusing, when piped) before calling. A front-end that
    /// sends `false` while a server is running is asserting that the user meant
    /// to leave it running.
    pub stop_processes: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct DaemonStopResult {
    pub stopping: bool,
}

pub struct DaemonStop;
impl Contract for DaemonStop {
    const CHANNEL: &'static str = "daemon.stop";
    type Params = DaemonStopParams;
    type Result = DaemonStopResult;
}
