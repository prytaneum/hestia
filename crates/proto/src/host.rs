//! Facts about the machine the daemon runs on, for a front-end that must
//! offer only what that machine can honour, and for any plugin: the channel is
//! ambient, so everything it answers is harmless to hand out.

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
    /// `windows`, `osx` or `linux`: the vocabulary of a rule's `os.name`.
    pub os: String,
    /// `x86`, `x86_64` or `arm64`: the vocabulary of a rule's `os.arch`.
    pub arch: String,
    /// What joins the entries of a classpath or `PATH`: `:` or `;`.
    pub path_separator: String,
}

pub struct HostInfo;
impl Contract for HostInfo {
    const CHANNEL: &'static str = "host.info";
    const AMBIENT: bool = true;
    type Params = Empty;
    type Result = HostInfoResult;
}
