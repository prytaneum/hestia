//! Long-running operations.
//!
//! A job's start channel answers at once with [`JobStarted`]; the work then
//! reports on the single `job` topic as [`JobEvent`]s carrying the job id, the
//! start channel that began it, and the [`JobState`] it reached. The start
//! contract declares what its progress and done payloads look like by
//! implementing [`Job`].
//!
//! Every job is identified by the client-supplied job id its events carry.
//! Cancellation reuses exactly that id, so there is **one** channel rather than
//! a `cancel` verb per domain: a front-end cancels the run it started, whatever
//! kind of run it was.
//!
//! A daemon job outlives the client that asked for it (see the "workloads
//! outlive the daemon" note in the architecture doc), so a disconnect must never
//! cancel anything. Cancelling is therefore an explicit act, like stopping a
//! workload — this channel is the only way it happens.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Topic};
use crate::error::ErrorInfo;

/// A start channel: answers with [`JobStarted`], then reports on the `job` topic.
pub trait Job: Contract<Result = JobStarted> {
    type Progress: Serialize + DeserializeOwned + Send + 'static;
    type Done: Serialize + DeserializeOwned + Send + 'static;
}

/// The reply to a job's start channel.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JobStarted {
    pub id: String,
}

/// Where a job is. A cancelled job is not a failure: nothing went wrong, and
/// what a cancel leaves behind is documented on each start contract.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState<P, D> {
    Progress(P),
    Done(D),
    Failed {
        /// The structured cause a front-end localizes from.
        error: ErrorInfo,
    },
    Cancelled,
}

/// One event of one job.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct JobEvent<P, D> {
    pub id: String,
    /// The start contract's `CHANNEL`, so an observer that did not start the job
    /// can still tell what kind it is.
    pub channel: String,
    #[serde(flatten)]
    pub state: JobState<P, D>,
}

/// The one topic every [`JobEvent`] publishes on, whatever its payload types.
pub const TOPIC: &str = "job";

impl<P, D> Topic for JobEvent<P, D> {
    const TOPIC: &'static str = TOPIC;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JobCancelParams {
    /// The job id its events carry.
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JobCancelResult {
    /// False when no job of that id is running — it already finished, or never
    /// existed. Not an error: asking to cancel something already over is a
    /// normal race, not a mistake.
    pub cancelled: bool,
}

pub struct JobCancel;
impl Contract for JobCancel {
    const CHANNEL: &'static str = "job.cancel";
    type Params = JobCancelParams;
    type Result = JobCancelResult;
}
