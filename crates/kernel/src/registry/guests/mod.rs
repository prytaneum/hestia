//! The WASM half of the registry: reading a package, installing it, and
//! loading what is installed. Every change to a guest runs under
//! `Plugins::changes`, so installs, enables, disables and removals never
//! interleave.

mod install;
mod lifecycle;
mod load;

use std::path::Path;

use proto::error::IoOp;
use proto::plugin::Dependency;

use crate::commands::{CommandDecl, ScheduleDecl};
use crate::entries::Entries;
use crate::error::PluginError;
use crate::install::{Approval, Package, Source};
use crate::runtime::Loaded;
use crate::trust::TrustError;

const SECRETS: &str = "secrets";

/// A WASM plugin in the registry. `loaded` is missing when its package could
/// not be read or no longer derives what was approved.
pub(crate) struct Guest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: Source,
    granted: Vec<String>,
    /// The packages its imports need an enabled built-in to provide.
    pub requires: Vec<String>,
    /// The ids of the built-ins that provide `requires`.
    providers: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub commands: Vec<CommandDecl>,
    pub schedules: Vec<ScheduleDecl>,
    /// The optional permissions the user granted.
    pub optional: Vec<String>,
    /// Whether it holds a permission that takes the entries scope.
    pub scoped: bool,
    pub entries: Entries,
    pub loaded: Option<Loaded>,
    #[cfg(feature = "probe")]
    pub inbox: super::events::Inbox,
}

impl Guest {
    /// The ids of every plugin it needs: those it declares and the built-ins
    /// behind the packages it imports.
    pub fn needs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.dependencies.iter().map(|d| d.id.clone()).collect();
        for provider in &self.providers {
            if !ids.contains(provider) {
                ids.push(provider.clone());
            }
        }
        ids
    }

    fn unloaded(id: &str, record: Approval) -> Self {
        Guest {
            id: id.to_owned(),
            name: id.to_owned(),
            version: String::new(),
            source: record.source,
            granted: Vec::new(),
            requires: Vec::new(),
            providers: Vec::new(),
            dependencies: record.grant.dependencies,
            commands: Vec::new(),
            schedules: Vec::new(),
            optional: record.choice.optional,
            scoped: false,
            entries: Entries::new(record.choice.entries),
            loaded: None,
            #[cfg(feature = "probe")]
            inbox: Default::default(),
        }
    }
}

async fn read(dir: &Path) -> Result<Package, PluginError> {
    let dir = dir.to_owned();
    tokio::task::spawn_blocking(move || Package::read(&dir))
        .await
        .map_err(|error| PluginError::io(IoOp::Read, error))?
        .map_err(PluginError::from)
}

fn revoked(plugin: &str, version: &str, reason: &str) -> PluginError {
    TrustError::Revoked {
        plugin: plugin.to_owned(),
        version: version.to_owned(),
        reason: match reason.is_empty() {
            true => String::new(),
            false => format!(": {reason}"),
        },
    }
    .into()
}
