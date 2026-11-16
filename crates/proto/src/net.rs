//! Network reachability — whether the daemon can reach upstream at all. A
//! launcher is mostly network-bound, so this is a state the whole system reads
//! rather than a conclusion each caller re-derives from its own failed request.

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty, Topic};

/// Whether outbound requests are reaching upstream.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum NetworkState {
    Online,
    Offline,
    /// Nothing has been attempted yet. Distinct from offline: the daemon has no
    /// grounds to claim either.
    #[default]
    Unknown,
}

impl NetworkState {
    pub fn is_offline(self) -> bool {
        matches!(self, NetworkState::Offline)
    }
}

/// The reachability readout: the state, why it is that state, and since when.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct NetworkStatus {
    pub state: NetworkState,
    /// The user pinned the daemon offline (`network.offline`); nothing is
    /// attempted at all while this is set.
    pub offline_mode: bool,
    /// Unix seconds the current state was entered, 0 while unknown.
    pub since_unix: i64,
    /// Unix seconds of the last request that succeeded, 0 if none has.
    pub last_online_unix: i64,
}

pub struct NetStatus;
impl Contract for NetStatus {
    const CHANNEL: &'static str = "net.status";
    type Params = Empty;
    type Result = NetworkStatus;
}

/// Published on every transition; the readout is its own event payload, so the
/// pushed and polled shapes cannot drift.
impl Topic for NetworkStatus {
    const TOPIC: &'static str = "net.state";
}
