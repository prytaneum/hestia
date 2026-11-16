//! The commands running on each plugin: what keeps it from being disabled or
//! removed, and how the host stops them when it cannot wait.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;

/// Stops every command, whatever its plugin.
const ALL: &str = "";

pub(super) struct Running {
    counts: Mutex<HashMap<String, usize>>,
    halt: broadcast::Sender<String>,
}

/// One running command; held for as long as it runs.
pub(super) struct Entry {
    running: Arc<Running>,
    plugin: String,
    halted: broadcast::Receiver<String>,
}

impl Running {
    pub fn new() -> Arc<Self> {
        Arc::new(Running {
            counts: Mutex::default(),
            halt: broadcast::channel(16).0,
        })
    }

    pub fn enter(self: &Arc<Self>, plugin: &str) -> Entry {
        let halted = self.halt.subscribe();
        *self.counts().entry(plugin.to_owned()).or_default() += 1;
        Entry {
            running: self.clone(),
            plugin: plugin.to_owned(),
            halted,
        }
    }

    pub fn any(&self, plugin: &str) -> bool {
        self.counts().get(plugin).is_some_and(|count| *count > 0)
    }

    pub fn halt(&self, plugin: &str) {
        let _ = self.halt.send(plugin.to_owned());
    }

    pub fn halt_all(&self) {
        self.halt(ALL);
    }

    fn counts(&self) -> std::sync::MutexGuard<'_, HashMap<String, usize>> {
        self.counts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Entry {
    /// Resolves when the host stops this command's plugin. A receiver that
    /// missed messages stops too: a stop must never be lost.
    pub async fn halted(&mut self) {
        loop {
            match self.halted.recv().await {
                Ok(plugin) if plugin == self.plugin || plugin == ALL => return,
                Ok(_) => {}
                Err(_) => return,
            }
        }
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        let mut counts = self.running.counts();
        if let Some(count) = counts.get_mut(&self.plugin) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.plugin);
            }
        }
    }
}
