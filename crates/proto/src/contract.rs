//! Conventions for the typed wire contracts. A call `Contract` names its channel
//! once and pairs it with the `Params`/`Result` payload shapes; both sides of the
//! socket marshal through one definition and cannot drift — a disagreement is a
//! compile error. Serde derive is the marshalling layer (replacing the C++
//! `kFields` codec).

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::plugin::{Permission, ScopeKind, Touched};

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
    /// Any plugin may call this channel with no permission: its whole result is
    /// a harmless fact about the app or the machine, never a path under the
    /// user's home, an account, an entry, a setting or a secret. Exclusive with
    /// [`Self::PLUGIN`], and never part of a grant
    /// ([0126](../../docs/decisions/0126-an-ambient-channel-needs-no-permission.md)).
    const AMBIENT: bool = false;
    /// The scope kind of [`Self::PLUGIN`] that narrows this channel for a
    /// plugin, with [`Self::touches`] naming what a call reaches. The plugin
    /// gate refuses a call whose touched entries the caller's scope does not
    /// cover, so a narrowing is declared here and never hand-written in a
    /// handler.
    const NARROWED_BY: Option<ScopeKind> = None;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;

    /// The entries of [`Self::NARROWED_BY`] this call reaches, such as the path
    /// it reads.
    fn touches(params: &Self::Params) -> Vec<String> {
        let _ = params;
        Vec::new()
    }

    /// What the plugin gate checks for this channel, read from its raw payload.
    fn touched(payload: &serde_json::Value) -> Touched {
        match Self::NARROWED_BY {
            None => Touched::Unscoped,
            Some(kind) => match serde_json::from_value::<Self::Params>(payload.clone()) {
                Ok(params) => Touched::Items(kind, Self::touches(&params)),
                Err(_) => Touched::Malformed,
            },
        }
    }
}

/// A topic string for an unsolicited daemon→client event; the implementing type
/// is its own payload.
pub trait Topic {
    const TOPIC: &'static str;
    /// The permission a plugin needs to receive this event; `None` withholds it.
    /// A topic must be classified or explicitly denied, like a contract.
    const PLUGIN: Option<Permission> = None;

    /// The permission one event needs, read from its payload. Only a topic whose
    /// events differ in what they reveal overrides it.
    fn permission(payload: &serde_json::Value) -> Option<Permission> {
        let _ = payload;
        Self::PLUGIN
    }
}

/// An empty payload: serializes to `{}` and decodes from it, matching the C++
/// `proto::Empty`.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
pub struct Empty {}
