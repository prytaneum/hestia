use super::bindings::hestia::kernel::{data, types::PluginError};
use super::host::State;

const LOCKS_PER_CALL: usize = 16;
const NAME_BYTES: usize = 128;

impl data::Host for State {
    fn try_lock(&mut self, name: String) -> Result<bool, PluginError> {
        if name.is_empty() || name.len() > NAME_BYTES {
            return Err(PluginError::Invalid("lock name".into()));
        }
        if self.held.contains(&name) {
            return Ok(true);
        }
        if self.held.len() >= LOCKS_PER_CALL {
            return Err(PluginError::Busy);
        }
        let taken = self
            .shared
            .locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((self.plugin.clone(), name.clone()));
        if taken {
            self.held.push(name);
        }
        Ok(taken)
    }

    fn unlock(&mut self, name: String) {
        if let Some(at) = self.held.iter().position(|held| *held == name) {
            self.held.swap_remove(at);
            self.shared
                .locks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&(self.plugin.clone(), name));
        }
    }
}
