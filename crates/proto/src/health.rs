use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PingResult {
    pub status: String,
    pub pid: i32,
}

pub struct Ping;
impl Contract for Ping {
    const CHANNEL: &'static str = "health.ping";
    type Params = Empty;
    type Result = PingResult;
}
