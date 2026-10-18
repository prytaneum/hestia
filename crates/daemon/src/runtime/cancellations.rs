use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Every cancellable job currently running, keyed by its job id — the same id
/// its progress and terminal events carry, so `job.cancel` needs nothing else to
/// find it.
///
/// The registration guard releases on drop, exactly as [`super::Claim`] does, so a job
/// that panics leaves no token behind for a later job to inherit.
#[derive(Clone, Default)]
pub struct Cancellations {
    running: Arc<Mutex<HashMap<String, engine::Cancel>>>,
}

impl Cancellations {
    /// Register `id` and hand back its token plus the guard that unregisters it.
    pub(super) fn register(&self, id: &str) -> (engine::Cancel, Registered) {
        let cancel = engine::Cancel::new();
        self.running
            .lock()
            .unwrap()
            .insert(id.to_string(), cancel.clone());
        (
            cancel,
            Registered {
                running: self.running.clone(),
                id: id.to_string(),
            },
        )
    }

    pub fn any_running(&self) -> bool {
        !self.running.lock().unwrap().is_empty()
    }

    /// Raise `id`'s cancel flag. False when no such job is running — it already
    /// finished, or never existed; a normal race, not an error.
    pub fn cancel(&self, id: &str) -> bool {
        match self.running.lock().unwrap().get(id) {
            Some(cancel) => {
                cancel.cancel();
                tracing::info!(job = %id, "job cancellation requested");
                true
            }
            None => {
                tracing::debug!(job = %id, "nothing to cancel");
                false
            }
        }
    }
}

pub(super) struct Registered {
    running: Arc<Mutex<HashMap<String, engine::Cancel>>>,
    id: String,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.running.lock().unwrap().remove(&self.id);
    }
}
