use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use proto::plugin::{PluginInfo, PluginKind, PluginState};
use serde::{Deserialize, Serialize};

use crate::builtin::Builtin;
use crate::error::PluginError;
use common::schema::{self, Document};

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

struct Slot {
    plugin: Arc<dyn Builtin>,
    state: PluginState,
}

impl Slot {
    fn id(&self) -> &'static str {
        self.plugin.descriptor().id
    }
}

/// Every plugin, in dependency order: a plugin's requirements sit before it.
pub struct Plugins {
    path: PathBuf,
    slots: Mutex<Vec<Slot>>,
}

impl Plugins {
    /// Orders `builtins` by their requirements and enables each one the user
    /// has not disabled. The registry keeps its state in `data_home`.
    pub fn new(data_home: &Path, builtins: Vec<Arc<dyn Builtin>>) -> Result<Self, PluginError> {
        let path = data_home.join(FILE);
        let ordered = order(builtins)?;
        let disabled: HashSet<String> = schema::load::<Saved>(&path)
            .map(|saved| saved.disabled.into_iter().collect())
            .unwrap_or_default();
        let mut slots: Vec<Slot> = ordered
            .into_iter()
            .map(|plugin| Slot {
                plugin,
                state: PluginState::Disabled,
            })
            .collect();
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

    /// Enables the plugin after everything it requires.
    pub fn enable(&self, id: &str) -> Result<PluginInfo, PluginError> {
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
    pub fn disable(&self, id: &str) -> Result<PluginInfo, PluginError> {
        let mut slots = self.slots();
        let index = find(&slots, id)?;
        match slots[index].state {
            PluginState::Disabled => return Ok(info(&slots[index])),
            PluginState::Failed => {}
            PluginState::Enabled => {
                let dependents: Vec<String> = slots
                    .iter()
                    .filter(|slot| {
                        slot.state == PluginState::Enabled
                            && slot.plugin.descriptor().requires.contains(&id)
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
                    .plugin
                    .disable()
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
    let descriptor = slot.plugin.descriptor();
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
    for required in slots[index].plugin.descriptor().requires {
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
    let id = slots[index].id();
    for required in slots[index].plugin.descriptor().requires {
        let required_index = find(slots, required)?;
        if let Err(error) = enable_with_requirements(slots, required_index) {
            slots[index].state = PluginState::Failed;
            return Err(error);
        }
    }
    match slots[index].plugin.enable() {
        Ok(()) => {
            slots[index].state = PluginState::Enabled;
            Ok(())
        }
        Err(source) => {
            slots[index].state = PluginState::Failed;
            Err(PluginError::Failed {
                plugin: id.to_string(),
                source,
            })
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
