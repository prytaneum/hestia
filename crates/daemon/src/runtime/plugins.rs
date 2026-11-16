//! When the daemon checks plugin repositories. A revocation is the one thing
//! that must not wait for a person to ask, so every repository's index is
//! fetched once plugins are restored and then every few hours.

use std::sync::Arc;
use std::time::Duration;

use proto::change::{ChangedEvent, Scope};

use super::Runtime;

const INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub async fn watch_plugin_repositories(runtime: Arc<Runtime>) {
    let mut ticks = tokio::time::interval(INTERVAL);
    loop {
        ticks.tick().await;
        if runtime.engine().network().status().state.is_offline() {
            continue;
        }
        let found = runtime.plugins().check_updates().await;
        for revoked in &found.revoked {
            tracing::warn!(
                plugin = revoked.id,
                version = revoked.version,
                "an installed plugin version was revoked"
            );
        }
        if !found.revoked.is_empty() {
            runtime.hub().publish_topic(&ChangedEvent {
                scope: Scope::Plugins,
                ids: found.revoked.iter().map(|r| r.id.clone()).collect(),
            });
        }
    }
}
