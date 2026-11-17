use serde::{Deserialize, Serialize};

use std::time::Duration;

use super::registry::PluginInfo;
use crate::change::Scope;
use crate::contract::Contract;

/// The entries a plugin reaches: every one, or the listed stable ids (`ids` is
/// empty when `all` is set). What the user picks at approval; an entry the
/// plugin creates is added. A client names entries by reference; the daemon
/// resolves them to ids before they reach the kernel.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryScope {
    pub all: bool,
    pub ids: Vec<String>,
}

impl EntryScope {
    /// The most ids a scope lists; past it a client asks for every entry instead.
    pub const MAX_IDS: usize = 256;

    /// `all` with no ids, or the ids once each.
    pub fn normalised(mut self) -> Self {
        match self.all {
            true => self.ids.clear(),
            false => {
                let mut seen = std::collections::HashSet::new();
                self.ids.retain(|id| seen.insert(id.clone()));
            }
        }
        self
    }

    pub fn allows(&self, id: &str) -> bool {
        self.all || self.ids.iter().any(|held| held == id)
    }
}

/// What the user decides at approval beyond the grant itself. The default
/// grants no optional permission and no entry.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Choice {
    /// The optional permissions the user granted.
    pub optional: Vec<String>,
    pub entries: EntryScope,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginPermissionParams {
    pub id: String,
    pub permission: String,
    /// `true` grants the optional permission, `false` revokes it.
    pub granted: bool,
}

/// Grants or revokes one optional permission of an installed plugin. The plugin
/// is relinked at once. A revoke halts the plugin's running commands; a grant is
/// refused while it has work running. A call already in flight keeps the link it
/// began with until it ends.
pub struct PluginPermission;
impl Contract for PluginPermission {
    const CHANNEL: &'static str = "plugin.permission";
    const TIMEOUT: Duration = Duration::from_secs(60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginPermissionParams;
    type Result = PluginInfo;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginEntriesParams {
    pub id: String,
    /// Entries by id or name; the daemon resolves and refuses what it cannot.
    pub entries: EntryScope,
}

/// Sets the entries an installed plugin may reach, replacing the scope. Takes
/// effect for the next call; a person's choice, never a plugin's.
pub struct PluginEntries;
impl Contract for PluginEntries {
    const CHANNEL: &'static str = "plugin.entries";
    const TIMEOUT: Duration = Duration::from_secs(60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginEntriesParams;
    type Result = PluginInfo;
}
