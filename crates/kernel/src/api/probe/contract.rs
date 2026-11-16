use proto::job::{Job, JobStarted};
use proto::Contract;
use serde::{Deserialize, Serialize};

use super::ID;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct EchoParams {
    pub text: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Echoed {
    pub text: String,
    pub length: u32,
}

/// A typed call.
pub struct Echo;
impl Contract for Echo {
    const CHANNEL: &'static str = "probe.echo";
    const OWNER: Option<&'static str> = Some(ID);
    type Params = EchoParams;
    type Result = Echoed;
}

/// A job: answers the shouted text when it is done.
pub struct Shout;
impl Contract for Shout {
    const CHANNEL: &'static str = "probe.shout";
    const OWNER: Option<&'static str> = Some(ID);
    type Params = EchoParams;
    type Result = JobStarted;
}
impl Job for Shout {
    type Progress = ();
    type Done = Echoed;
}
