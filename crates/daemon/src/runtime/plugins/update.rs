//! Applying a plugin update: the swap is the engine's; this loads the new build
//! beside the running one, so calls in flight finish on the old code and new
//! calls take the new, and watches the first minute for a trap.

use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use engine::{Cancel, LoadedPlugin, UpdateOutcome, UpdateRequest};
use proto::plugin::{PluginInstallPhase, PluginRolledBackEvent, PluginUpdateDone};
use proto::warning::WarningInfo;
use tokio::task::spawn_blocking;
use tokio::time::Instant;

use super::{Activation, Plugins};

const TRAP_WATCH: Duration = Duration::from_secs(60);
const TRAP_POLL: Duration = Duration::from_secs(1);

impl Plugins {
    /// Stages and verifies the newest build, swaps it in and loads it. A build
    /// that fails to load is swapped back out and the done event warns of it.
    pub async fn update(
        self: &Arc<Self>,
        plugin: &str,
        approve: &str,
        cancel: &Cancel,
        on_phase: &(dyn Fn(PluginInstallPhase) + Sync),
    ) -> anyhow::Result<PluginUpdateDone> {
        let sandbox = self
            .sandbox
            .get()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("the plugin runtime is not running"))?;
        let downloads = self.engine.plugin_downloads();
        let token = self.engine.stage_plugin_update(plugin).await?;
        let staged = downloads.staged(&token)?;
        let outcome = self
            .engine
            .plugins()
            .apply_update(
                UpdateRequest {
                    id: plugin,
                    staged: &staged,
                    approve,
                },
                cancel,
                on_phase,
                |wasm| sandbox.check(wasm).map_err(Into::into),
                |record| async move { self.activate_update(&record).await },
            )
            .await;
        downloads.discard(&token);

        Ok(match outcome? {
            UpdateOutcome::Applied { record, from } => {
                self.watch_for_traps(&record.id);
                PluginUpdateDone {
                    plugin: self.info_of(record).await,
                    from,
                    warnings: staged.warnings,
                }
            }
            UpdateOutcome::RolledBack {
                record,
                version,
                error,
            } => {
                let mut warnings = staged.warnings;
                warnings.push(WarningInfo::PluginUpdateRolledBack {
                    id: record.id.clone(),
                    version,
                    error,
                });
                let from = self.read_version(&record.id).await;
                PluginUpdateDone {
                    plugin: self.info_of(record).await,
                    from,
                    warnings,
                }
            }
        })
    }

    /// Settles only on success: a failed load leaves the old build serving
    /// until the engine has put its package back.
    async fn activate_update(&self, record: &engine::PluginRecord) -> anyhow::Result<()> {
        if self.safe_mode {
            self.activate(record.clone()).await;
            return Ok(());
        }
        match self.try_activate(record).await? {
            Activation::Ready(loaded, contributions) => {
                self.settle(
                    &record.id,
                    proto::plugin::PluginState::Enabled,
                    None,
                    loaded,
                    contributions,
                );
                Ok(())
            }
            Activation::NeedsApproval => bail!("the update asks for more than was approved"),
        }
    }

    async fn read_version(&self, id: &str) -> String {
        let engine = self.engine.clone();
        let id = id.to_string();
        spawn_blocking(move || engine.plugins().read_manifest(&id))
            .await
            .ok()
            .and_then(Result::ok)
            .map(|manifest| manifest.version)
            .unwrap_or_default()
    }

    /// A plugin that traps in the first minute after an update goes back to
    /// the version it came from. Ends quietly when the plugin is replaced,
    /// disabled or removed meanwhile.
    fn watch_for_traps(self: &Arc<Self>, id: &str) {
        let Some(updated) = self.loaded_of(id) else {
            return;
        };
        let plugins = Arc::downgrade(self);
        let id = id.to_string();
        tokio::spawn(async move {
            let deadline = Instant::now() + TRAP_WATCH;
            while Instant::now() < deadline {
                tokio::time::sleep(TRAP_POLL).await;
                let Some(plugins) = plugins.upgrade() else {
                    return;
                };
                if !plugins
                    .loaded_of(&id)
                    .is_some_and(|live| Arc::ptr_eq(&live, &updated))
                {
                    return;
                }
                if updated.is_disabled() || updated.stats().traps > 0 {
                    plugins.roll_back_trapped(&id).await;
                    return;
                }
            }
        });
    }

    fn loaded_of(&self, id: &str) -> Option<Arc<LoadedPlugin>> {
        self.slots
            .lock()
            .unwrap()
            .get(id)
            .and_then(|slot| slot.loaded.clone())
    }

    async fn roll_back_trapped(&self, id: &str) {
        let engine = self.engine.clone();
        let target = id.to_string();
        let rolled_back = self.read_version(id).await;
        let restored = spawn_blocking(move || engine.plugins().rollback(&target)).await;
        match restored {
            Ok(Ok(record)) => {
                tracing::warn!(plugin = %id, "updated plugin trapped in its first minute; rolled back");
                self.engine.processes().stop_owned(&engine::owner_of(id));
                self.activate(record).await;
                self.hub.publish_topic(&PluginRolledBackEvent {
                    id: id.to_string(),
                    version: rolled_back,
                    restored: self.read_version(id).await,
                });
            }
            Ok(Err(error)) => {
                tracing::error!(plugin = %id, error = %format!("{error:#}"), "cannot roll back a trapping update");
            }
            Err(error) => tracing::error!(plugin = %id, %error, "rollback task failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::event_hub::EventHub;
    use engine::{Engine, PluginRecord};
    use tokio::sync::mpsc::unbounded_channel;

    fn manifest(version: &str) -> String {
        format!(
            "id = \"demo\"\nversion = \"{version}\"\napi = \"^{}\"\nwasm = \"plugin.wasm\"\n",
            proto::plugin::API_VERSION
        )
    }

    fn rolled_back_events(
        events: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    ) -> Vec<serde_json::Value> {
        let mut found = Vec::new();
        while let Ok(frame) = events.try_recv() {
            let event: serde_json::Value = serde_json::from_str(&frame).unwrap();
            if event["event"] == "plugin.rolled_back" {
                found.push(event);
            }
        }
        found
    }

    #[tokio::test]
    async fn a_rollback_publishes_both_versions() {
        let home = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(Some(home.path())));
        let hub = Arc::new(EventHub::default());
        let plugins = Plugins::new(engine.clone(), hub.clone(), false);
        let record = PluginRecord {
            id: "demo".into(),
            enabled: true,
            ..PluginRecord::default()
        };
        engine.plugins().save(&record).unwrap();
        let dir = engine.plugins().plugin_dir("demo").unwrap();
        std::fs::copy(dir.join("plugin.json"), dir.join("plugin.prev.json")).unwrap();
        for (package, version) in [("package", "2.0.0"), ("package.prev", "1.0.0")] {
            std::fs::create_dir_all(dir.join(package)).unwrap();
            std::fs::write(dir.join(package).join("plugin.toml"), manifest(version)).unwrap();
        }
        let (out, mut events) = unbounded_channel();
        hub.subscribe(1, out, None);

        plugins.roll_back_trapped("demo").await;

        let event = rolled_back_events(&mut events)
            .pop()
            .expect("a rollback event");
        assert_eq!(
            event["payload"],
            serde_json::json!({"id": "demo", "version": "2.0.0", "restored": "1.0.0"})
        );
    }

    #[tokio::test]
    async fn a_failed_rollback_publishes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(Some(home.path())));
        let hub = Arc::new(EventHub::default());
        let plugins = Plugins::new(engine, hub.clone(), false);
        let (out, mut events) = unbounded_channel();
        hub.subscribe(1, out, None);

        plugins.roll_back_trapped("demo").await;

        assert!(rolled_back_events(&mut events).is_empty());
    }
}
