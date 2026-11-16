//! Which state a successful call changed, for the clients that did not make it.
//!
//! A contract that mutates names the [`Scope`] it touches by overriding
//! [`Contract::CHANGES`]; the daemon's registrar publishes one [`ChangedEvent`]
//! after the handler succeeds, so a handler never announces anything itself.
//! A job reports on the `job` topic instead.
//!
//! The event also names the entities the call touched, when the handler knows
//! them, so a reader can refetch one entry instead of the whole scope.
//!
//! [`Contract::CHANGES`]: crate::contract::Contract::CHANGES

use serde::{Deserialize, Serialize};

use crate::contract::Topic;

/// A body of state a front-end renders and another client can change. Coarse on
/// purpose: a reader refetches the scope, and [`ChangedEvent::ids`] only narrows it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Servers and instances: their records and everything kept beside them
    /// (settings, content, backups, tasks).
    Entries,
    Accounts,
    Skins,
    Config,
    Java,
    Sync,
    Screenshots,
    Cache,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChangedEvent {
    pub scope: Scope,
    /// The stable ids the call touched: entry ids for `entries`, plugin ids for
    /// `plugins`. Empty means the whole scope.
    #[serde(default)]
    pub ids: Vec<String>,
}
impl Topic for ChangedEvent {
    const TOPIC: &'static str = "changed";
}
