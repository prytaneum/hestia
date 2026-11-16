//! Facts about the machine the daemon runs on, for a front-end that must
//! offer only what that machine can honour.

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct HostInfoResult {
    /// Installed physical memory, absent where the platform reports none. The
    /// ceiling the daemon enforces on a `memory` setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_memory_bytes: Option<u64>,
}

pub struct HostInfo;
impl Contract for HostInfo {
    const CHANNEL: &'static str = "host.info";
    type Params = Empty;
    type Result = HostInfoResult;
}
