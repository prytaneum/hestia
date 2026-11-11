//! Which state a successful call changed, for the clients that did not make it.
//!
//! A contract that mutates names the [`Scope`] it touches by overriding
//! [`Contract::CHANGES`]; the daemon's registrar publishes one [`ChangedEvent`]
//! after the handler succeeds, so a handler never announces anything itself.
//! A job reports on the `job` topic instead.
//!
//! [`Contract::CHANGES`]: crate::contract::Contract::CHANGES

use serde::{Deserialize, Serialize};

use crate::contract::Topic;
use crate::plugin::Permission;

/// A body of state a front-end renders and another client can change. Coarse on
/// purpose: a reader refetches the scope, so the event names no entry.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Servers and instances: their records and everything kept beside them
    /// (settings, content, backups, tasks, players).
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
    /// only the scope, never what changed in it.
    pub fn permission(self) -> Option<Permission> {
        match self {
            Scope::Entries | Scope::Sync | Scope::Screenshots => Some(Permission::EntryRead),
            Scope::Accounts => Some(Permission::AccountRead),
            Scope::Config => Some(Permission::ConfigWrite),
            Scope::Java => Some(Permission::JavaRead),
            Scope::Skins | Scope::Cache | Scope::Plugins => None,
        }
    }
}
