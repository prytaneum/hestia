//! The reachability watcher: keeps the engine's network state honest while the
//! launcher is idle, and pushes every transition to subscribed front-ends.
//!
//! Two loops rather than one. The tick asks the engine to confirm a state that
//! has gone stale — it decides for itself whether a probe is due — while the
//! watch on its state channel forwards transitions the moment they happen,
//! including the ones a real request observed between ticks.

use std::sync::Arc;
use std::time::Duration;

use proto::net::NetworkStatus;

use super::Runtime;

const TICK: Duration = Duration::from_secs(5);

pub fn spawn_network_watcher(runtime: Arc<Runtime>) {
    let refresher = runtime.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            refresher.engine().network().refresh().await;
        }
    });

    tokio::spawn(async move {
        let mut states = runtime.engine().network().subscribe();
        loop {
            if states.changed().await.is_err() {
                return;
            }
            let status = states.borrow_and_update().clone();
            publish(&runtime, &status);
        }
    });
}

fn publish(runtime: &Runtime, status: &NetworkStatus) {
    runtime.hub().publish_topic(status);
}
