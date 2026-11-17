//! The `entries` scope: which servers and instances a plugin may reach. The user
//! picks it at approval; the router checks the entries a call touches against
//! it, so a handler never does, and an entry the plugin creates joins it. The
//! ids are the entries' stable ids; the daemon resolves what a call names.

use std::sync::{Arc, Mutex};

use proto::error::ErrorInfo;
use proto::plugin::EntryScope;

use crate::api::Chain;
use crate::registry::Plugins;

/// One plugin's scope, shared by the guest the registry lists and the calls
/// that read it, so an entry created mid-run is seen at once.
#[derive(Clone, Default)]
pub struct Entries(Arc<Mutex<EntryScope>>);

impl Entries {
    pub fn new(scope: EntryScope) -> Self {
        Entries(Arc::new(Mutex::new(scope.normalised())))
    }

    pub(crate) fn set(&self, scope: EntryScope) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = scope.normalised();
    }

    pub fn get(&self) -> EntryScope {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The first of `ids` outside the scope.
    pub(crate) fn refused<'a>(&self, ids: &'a [String]) -> Option<&'a str> {
        let scope = self.0.lock().unwrap_or_else(|e| e.into_inner());
        ids.iter().map(String::as_str).find(|id| !scope.allows(id))
    }

    pub(crate) fn add(&self, ids: &[String]) {
        let mut scope = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            if scope.allows(id) {
                continue;
            }
            if scope.ids.len() >= EntryScope::MAX_IDS {
                tracing::warn!(id, "the entries scope is full; the entry stays out of it");
                break;
            }
            scope.ids.push(id.clone());
        }
    }
}

/// Refuses a call that touches an entry outside the scope of any plugin in
/// `chain`: a plugin cannot reach through another what it may not reach itself.
/// A call that touches nothing passes. `touched` holds stable ids.
pub fn guard(
    plugins: &Plugins,
    chain: &Chain,
    channel: &str,
    touched: &[String],
) -> Result<(), ErrorInfo> {
    if touched.is_empty() {
        return Ok(());
    }
    for plugin in chain.plugins() {
        if let Some(entry) = plugins.refused_entry(plugin, touched) {
            tracing::warn!(plugin, entry, channel, "entry outside the plugin's scope");
            return Err(ErrorInfo::EntryOutOfScope {
                plugin: plugin.clone(),
                entry,
            });
        }
    }
    Ok(())
}

/// Adds the entries a call of the plugin ending `chain` made to its scope.
pub async fn record(plugins: &Plugins, chain: &Chain, created: &[String]) {
    if let (Some(plugin), false) = (chain.caller(), created.is_empty()) {
        plugins.entries_created(plugin, created).await;
    }
}
