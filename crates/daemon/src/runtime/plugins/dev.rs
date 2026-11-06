//! Reloading dev plugins: a plugin run from a folder is polled once a second and
//! reloaded when its manifest, component or themes change. A reload builds a new
//! component and swaps the slot, so calls already running finish on the old one.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use proto::plugin::{PluginSource, PluginState};
use tokio::task::spawn_blocking;

use super::Plugins;

const POLL: Duration = Duration::from_secs(1);

impl Plugins {
    /// Polls dev plugins until the daemon shuts down. Plain mtime polling keeps
    /// a file watcher out of the dependency tree.
    pub fn watch_dev(self: &Arc<Self>) {
        let plugins = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut stamps = HashMap::new();
            let mut tick = tokio::time::interval(POLL);
            loop {
                tick.tick().await;
                let Some(plugins) = plugins.upgrade() else {
                    break;
                };
                if plugins.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                plugins.reload_changed(&mut stamps).await;
            }
        });
    }

    async fn reload_changed(&self, stamps: &mut HashMap<String, SystemTime>) {
        let engine = self.engine.clone();
        let Ok(current) = spawn_blocking(move || {
            engine
                .plugins()
                .list()
                .into_iter()
                .filter(|record| record.enabled && record.source == PluginSource::Dev)
                .filter_map(|record| {
                    let stamp = engine.plugins().dev_stamp(&record.id)?;
                    Some((record.id, stamp))
                })
                .collect::<Vec<_>>()
        })
        .await
        else {
            return;
        };
        stamps.retain(|id, _| current.iter().any(|(live, _)| live == id));
        for (id, stamp) in current {
            if stamps
                .insert(id.clone(), stamp)
                .is_some_and(|seen| seen != stamp)
            {
                self.reload(id).await;
            }
        }
    }

    async fn reload(&self, id: String) {
        tracing::info!(plugin = %id, "dev plugin changed, reloading");
        let engine = self.engine.clone();
        let target = id.clone();
        match spawn_blocking(move || engine.plugins().refresh_dev(&target)).await {
            Ok(Ok(record)) => self.activate(record).await,
            Ok(Err(error)) => {
                let message = format!("{error:#}");
                tracing::warn!(plugin = %id, error = %message, "dev plugin cannot be reloaded");
                self.settle(&id, PluginState::Failed, Some(message), None);
            }
            Err(error) => tracing::error!(plugin = %id, %error, "dev reload task failed"),
        }
    }
}
