//! Conventions for the typed wire contracts. A call `Contract` names its channel
//! once and pairs it with the `Params`/`Result` payload shapes; both sides of the
//! socket marshal through one definition and cannot drift — a disagreement is a
//! compile error. Serde derive is the marshalling layer (replacing the C++
//! `kFields` codec).

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::plugin::Permission;

/// How long a call waits for its reply unless its contract says otherwise; a
/// wedged handler can't hang the caller forever.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Binds a channel string to its request/response payload types.
pub trait Contract {
    const CHANNEL: &'static str;
    /// How long a caller waits for the reply. A contract whose handler blocks on
    /// the network or the disk overrides it; `scripts/lib/gen-channels.py` reads
    /// the override as `Duration::from_secs(<int> [* <int>]…)` into the
    /// front-end's table, so keep that form.
    const TIMEOUT: Duration = DEFAULT_TIMEOUT;
    /// The state a successful call changes. The daemon publishes it as a
    /// [`ChangedEvent`](crate::change::ChangedEvent) so other clients refresh;
    /// a read leaves it `None`, and a job reports on the `job` topic instead.
    const CHANGES: Option<Scope> = None;
    /// The permission a plugin needs to call this channel. `None` makes the
    /// channel unreachable from plugins, so a new contract is closed until it is
    /// classified ([0092](../../docs/decisions/0092-a-contract-names-the-permission-a-plugin-needs.md)).
    const PLUGIN: Option<Permission> = None;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;
}

/// A topic string for an unsolicited daemon→client event; the implementing type
/// is its own payload.
pub trait Topic {
    const TOPIC: &'static str;
    /// The permission a plugin needs to receive this event; `None` withholds it.
    const PLUGIN: Option<Permission> = None;
}

/// An empty payload: serializes to `{}` and decodes from it, matching the C++
/// `proto::Empty`.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
pub struct Empty {}
