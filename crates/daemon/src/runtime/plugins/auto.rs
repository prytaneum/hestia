//! Unattended updates: after a scheduled check, the official plugins whose
//! update asks for nothing new go through the same apply path as `plugin.update`.

use std::sync::Arc;

use engine::Cancel;
use proto::error::ErrorInfo;
use proto::plugin::{Outdated, PluginAutoUpdatedEvent};

use super::Plugins;
use crate::runtime::{JobKind, Runtime};

impl Plugins {
    /// Applies what [`engine::Engine::auto_update_candidates`] allows and
    /// publishes `plugin.auto_updated` for each success. Returns the ids that
    /// were updated.
    pub async fn auto_update(
        self: &Arc<Self>,
        outdated: &Outdated,
        runtime: &Runtime,
    ) -> Vec<String> {
        let mut updated = Vec::new();
        for offer in self.engine.auto_update_candidates(outdated) {
            let Ok(_claim) = runtime
                .claims()
                .claim(format!("plugin-{}", offer.id), JobKind::Install)
            else {
                tracing::debug!(plugin = %offer.id, "plugin busy; auto-update deferred");
                continue;
            };
            match self.update(&offer.id, "", &Cancel::new(), &|_| {}).await {
                Ok(done) if done.plugin.manifest.version != done.from => {
                    tracing::info!(plugin = %offer.id, from = %done.from, to = %done.plugin.manifest.version, "plugin auto-updated");
                    self.hub.publish_topic(&PluginAutoUpdatedEvent {
                        id: offer.id.clone(),
                        from: done.from,
                        to: done.plugin.manifest.version,
                    });
                    updated.push(offer.id);
                }
                Ok(done) => {
                    tracing::warn!(plugin = %offer.id, version = %done.plugin.manifest.version, "auto-update rolled back; kept the previous build");
                }
                Err(error) => match error.downcast_ref::<ErrorInfo>() {
                    Some(ErrorInfo::PluginUpdateNeedsApproval { .. }) => {
                        tracing::info!(plugin = %offer.id, "update asks for more than was approved; left for the user");
                    }
                    _ => {
                        tracing::warn!(plugin = %offer.id, error = format!("{error:#}"), "auto-update failed");
                    }
                },
            }
        }
        updated
    }
}
