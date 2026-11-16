use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use proto::plugin::{PluginInfo, PluginKind, PluginState};
use serde::{Deserialize, Serialize};

use crate::builtin::{Builtin, Busy};
use crate::error::PluginError;
use crate::grant::Table;
use crate::install::Store;
use crate::runtime::Sandbox;
use common::schema::{self, Document};
use guests::Guest;

mod guests;

const FILE: &str = "plugins.json";

/// Only the disabled ids are kept, so a built-in added by an update starts
/// enabled.
#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    disabled: Vec<String>,
}

impl Document for Saved {
    const NAME: &'static str = FILE;
}

enum Kind {
    Builtin(Arc<dyn Builtin>),
    Guest(Arc<Guest>),
}

struct Slot {
    kind: Kind,
    state: PluginState,
}

impl Slot {
    fn builtin(plugin: Arc<dyn Builtin>) -> Self {
        Slot {
            kind: Kind::Builtin(plugin),
            state: PluginState::Disabled,
        }
    }

    fn id(&self) -> &str {
        match &self.kind {
            Kind::Builtin(plugin) => plugin.descriptor().id,
            Kind::Guest(guest) => &guest.id,
        }
    }

    fn requires(&self) -> &'static [&'static str] {
        match &self.kind {
            Kind::Builtin(plugin) => plugin.descriptor().requires,
            Kind::Guest(_) => &[],
        }
    }

    fn start(&self) -> anyhow::Result<()> {
        match &self.kind {
            Kind::Builtin(plugin) => plugin.enable(),
            Kind::Guest(_) => anyhow::bail!("a WASM plugin starts through its init"),
        }
    }

    fn stop(&self) -> Result<(), Busy> {
        match &self.kind {
            Kind::Builtin(plugin) => plugin.disable(),
            Kind::Guest(_) => Ok(()),
        }
    }
}

/// Every plugin: built-ins in dependency order, then installed WASM plugins.
pub struct Plugins {
    path: PathBuf,
    slots: Mutex<Vec<Slot>>,
    /// Held across every enable, disable, install and removal, `init` included,
    /// so none of them observes another half done. `slots` is only ever held
    /// briefly inside it.
    changes: tokio::sync::Mutex<()>,
    sandbox: Option<Arc<Sandbox>>,
    table: Table,
    store: Store,
}

impl Plugins {
    /// Orders `builtins` by their requirements and enables each one the user
    /// has not disabled. The registry keeps its state in `data_home`.
    ///
    /// WASM plugins load through `sandbox` once [`Plugins::restore`] runs;
    /// without one they are listed and never run. `table` says what their
    /// imports need.
    pub fn new(
        data_home: &Path,
        builtins: Vec<Arc<dyn Builtin>>,
        sandbox: Option<Arc<Sandbox>>,
        table: Table,
    ) -> Result<Self, PluginError> {
        let path = data_home.join(FILE);
        let ordered = order(builtins)?;
        let disabled: HashSet<String> = schema::load::<Saved>(&path)
            .map(|saved| saved.disabled.into_iter().collect())
            .unwrap_or_default();
        let mut slots: Vec<Slot> = ordered.into_iter().map(Slot::builtin).collect();
        for index in 0..slots.len() {
            if disabled.contains(slots[index].id()) {
                continue;
            }
            if let Err(error) = enable_at_start_up(&mut slots, index) {
                tracing::warn!(%error, "built-in plugin did not start");
            }
        }
        Ok(Plugins {
            path,
            slots: Mutex::new(slots),
            changes: tokio::sync::Mutex::new(()),
            sandbox,
            table,
            store: Store::new(data_home),
        })
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        self.slots().iter().map(info).collect()
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.slots()
            .iter()
            .any(|slot| slot.id() == id && slot.state == PluginState::Enabled)
    }

    /// Enables the plugin after everything it requires. A WASM plugin runs its
    /// `init`; one that reaches its trap limit stays disabled across restarts.
    pub async fn enable(&self, id: &str) -> Result<PluginInfo, PluginError> {
        let _changing = self.changes.lock().await;
        let guest = {
            let slots = self.slots();
            match &slots[find(&slots, id)?].kind {
                Kind::Guest(guest) => Some(guest.clone()),
                Kind::Builtin(_) => None,
            }
        };
        match guest {
            Some(guest) => {
                if !self.is_enabled(id) {
                    self.start_guest(&guest).await?;
                }
                let slots = self.slots();
                Ok(info(&slots[find(&slots, id)?]))
            }
            None => self.enable_builtin(id),
        }
    }

    fn enable_builtin(&self, id: &str) -> Result<PluginInfo, PluginError> {
        let mut slots = self.slots();
        let index = find(&slots, id)?;
        let outcome = enable_with_requirements(&mut slots, index);
        let saved = self.save(&slots);
        outcome?;
        saved?;
        tracing::info!(plugin = id, "plugin enabled");
        Ok(info(&slots[index]))
    }

    /// Refused while an enabled plugin requires it or it has work running.
    pub async fn disable(&self, id: &str) -> Result<PluginInfo, PluginError> {
        let _changing = self.changes.lock().await;
        let mut slots = self.slots();
        let index = find(&slots, id)?;
        match slots[index].state {
            PluginState::Disabled => return Ok(info(&slots[index])),
            PluginState::Failed => {}
            PluginState::Enabled => {
                let dependents: Vec<String> = slots
                    .iter()
                    .filter(|slot| {
                        slot.state == PluginState::Enabled && slot.requires().contains(&id)
                    })
                    .map(|slot| slot.id().to_string())
                    .collect();
                if !dependents.is_empty() {
                    return Err(PluginError::Required {
                        plugin: id.to_string(),
                        dependents,
                    });
                }
                slots[index]
                    .stop()
                    .map_err(|_| PluginError::Busy(id.to_string()))?;
            }
        }
        slots[index].state = PluginState::Disabled;
        self.save(&slots)?;
        tracing::info!(plugin = id, "plugin disabled");
        Ok(info(&slots[index]))
    }

    fn slots(&self) -> MutexGuard<'_, Vec<Slot>> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, slots: &[Slot]) -> Result<(), PluginError> {
        let disabled = slots
            .iter()
            .filter(|slot| slot.state == PluginState::Disabled)
            .map(|slot| slot.id().to_string())
            .collect();
        schema::save(&self.path, &Saved { disabled }).map_err(PluginError::Save)
    }
}

fn info(slot: &Slot) -> PluginInfo {
    match &slot.kind {
        Kind::Builtin(plugin) => {
            let descriptor = plugin.descriptor();
            PluginInfo {
                id: descriptor.id.to_string(),
                name: descriptor.name.to_string(),
                kind: PluginKind::Builtin,
                version: common::app::VERSION.to_string(),
                state: slot.state,
                provides: descriptor.provides.map(str::to_string),
                requires: descriptor
                    .requires
                    .iter()
                    .map(|id| id.to_string())
                    .collect(),
            }
        }
        Kind::Guest(guest) => PluginInfo {
            id: guest.id.clone(),
            name: guest.name.clone(),
            kind: PluginKind::Wasm,
            version: guest.version.clone(),
            state: slot.state,
            provides: None,
            requires: Vec::new(),
        },
    }
}

fn find(slots: &[Slot], id: &str) -> Result<usize, PluginError> {
    slots
        .iter()
        .position(|slot| slot.id() == id)
        .ok_or_else(|| PluginError::NotFound(id.to_string()))
}

/// Start-up enables only what the user left on, and never a requirement they
/// turned off: a plugin whose requirement is not enabled stays failed. Not
/// persisted, so it recovers once the requirement is enabled and the plugin is
/// enabled again.
fn enable_at_start_up(slots: &mut [Slot], index: usize) -> Result<(), PluginError> {
    for required in slots[index].requires() {
        let required_index = find(slots, required)?;
        if slots[required_index].state != PluginState::Enabled {
            tracing::warn!(
                plugin = slots[index].id(),
                requirement = required,
                "requirement is not enabled"
            );
            slots[index].state = PluginState::Failed;
            return Ok(());
        }
    }
    enable_with_requirements(slots, index)
}

/// Requirements come first in `slots`, so recursing into them enables in
/// dependency order. A requirement that fails leaves the dependent failed too.
fn enable_with_requirements(slots: &mut [Slot], index: usize) -> Result<(), PluginError> {
    if slots[index].state == PluginState::Enabled {
        return Ok(());
    }
    let id = slots[index].id().to_string();
    for required in slots[index].requires() {
        let required_index = find(slots, required)?;
        if let Err(error) = enable_with_requirements(slots, required_index) {
            slots[index].state = PluginState::Failed;
            return Err(error);
        }
    }
    match slots[index].start() {
        Ok(()) => {
            slots[index].state = PluginState::Enabled;
            Ok(())
        }
        Err(source) => {
            slots[index].state = PluginState::Failed;
            Err(PluginError::Failed { plugin: id, source })
        }
    }
}

fn order(builtins: Vec<Arc<dyn Builtin>>) -> Result<Vec<Arc<dyn Builtin>>, PluginError> {
    let mut seen = HashSet::new();
    for plugin in &builtins {
        let id = plugin.descriptor().id;
        if !seen.insert(id) {
            return Err(PluginError::Invalid(format!("duplicate id '{id}'")));
        }
    }
    for plugin in &builtins {
        let descriptor = plugin.descriptor();
        if let Some(unknown) = descriptor.requires.iter().find(|id| !seen.contains(*id)) {
            return Err(PluginError::Invalid(format!(
                "'{}' requires unknown '{unknown}'",
                descriptor.id
            )));
        }
    }
    let mut remaining = builtins;
    let mut ordered: Vec<Arc<dyn Builtin>> = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let placed: HashSet<&str> = ordered.iter().map(|p| p.descriptor().id).collect();
        let Some(next) = remaining.iter().position(|plugin| {
            plugin
                .descriptor()
                .requires
                .iter()
                .all(|required| placed.contains(required))
        }) else {
            return Err(PluginError::Invalid("requirement cycle".into()));
        };
        ordered.push(remaining.remove(next));
    }
    Ok(ordered)
}
