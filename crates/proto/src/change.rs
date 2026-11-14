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
use crate::plugin::Permission;

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
    Plugins,
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
    /// The permission of the commonest scope; [`Topic::permission`] decides per event.
    const PLUGIN: Option<Permission> = Some(Permission::EntryRead);

    fn permission(payload: &serde_json::Value) -> Option<Permission> {
        let scope: Scope = serde_json::from_value(payload.get("scope")?.clone()).ok()?;
        scope.permission()
    }
}

impl Scope {
    /// What a plugin needs to be told that this scope changed. The event names
    /// the scope and the ids touched, never what changed in them.
    pub fn permission(self) -> Option<Permission> {
        match self {
            Scope::Entries | Scope::Sync | Scope::Screenshots => Some(Permission::EntryRead),
            Scope::Accounts => Some(Permission::AccountRead),
            Scope::Config => Some(Permission::ConfigRead),
            Scope::Java => Some(Permission::JavaRead),
            Scope::Skins | Scope::Cache | Scope::Plugins => None,
        }
    }
}
