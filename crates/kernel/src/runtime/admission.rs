use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use tokio::sync::{AcquireError, OwnedSemaphorePermit, Semaphore};

use super::limits::Limits;

struct Slot {
    calls: Arc<Semaphore>,
    traps: VecDeque<Instant>,
}

/// Who may run now: bounded concurrency per plugin and overall, and the trap
/// window. Whether a plugin is enabled is the registry's, not this.
pub(super) struct Admission {
    limits: Limits,
    total: Arc<Semaphore>,
    plugins: Mutex<HashMap<String, Slot>>,
}

/// Held for the length of a call.
pub(super) struct Permit {
    _plugin: OwnedSemaphorePermit,
    _total: OwnedSemaphorePermit,
}

impl Admission {
    pub fn new(limits: Limits) -> Self {
        Admission {
            total: Arc::new(Semaphore::new(limits.calls_total)),
            limits,
            plugins: Mutex::default(),
        }
    }

    /// Waits for the plugin's own slot, then for one overall, so a plugin at
    /// its cap never holds an overall slot while it waits.
    pub async fn admit(&self, plugin: &str) -> Result<Permit, AcquireError> {
        let calls = self.slot(plugin, |slot| slot.calls.clone());
        let own = calls.acquire_owned().await?;
        let total = self.total.clone().acquire_owned().await?;
        Ok(Permit {
            _plugin: own,
            _total: total,
        })
    }

    /// Counts a trap; true when it reaches the limit, which also starts a new
    /// window.
    pub fn record_trap(&self, plugin: &str) -> bool {
        let now = Instant::now();
        let window = self.limits.trap_window;
        let limit = self.limits.traps_to_disable;
        self.slot(plugin, |slot| {
            slot.traps.push_back(now);
            while slot
                .traps
                .front()
                .is_some_and(|at| now.duration_since(*at) > window)
            {
                slot.traps.pop_front();
            }
            let reached = slot.traps.len() >= limit;
            if reached {
                slot.traps.clear();
            }
            reached
        })
    }

    fn slot<R>(&self, plugin: &str, use_slot: impl FnOnce(&mut Slot) -> R) -> R {
        let mut plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = plugins.entry(plugin.to_owned()).or_insert_with(|| Slot {
            calls: Arc::new(Semaphore::new(self.limits.calls_per_plugin)),
            traps: VecDeque::new(),
        });
        use_slot(slot)
    }
}
