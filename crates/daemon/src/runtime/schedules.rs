//! The clock of WASM plugins' declared schedules, checked once a minute, the
//! finest a schedule can be.

use std::sync::Arc;
use std::time::Duration;

use super::Runtime;

const TICK: Duration = Duration::from_secs(60);

pub fn spawn_plugin_schedules(runtime: Arc<Runtime>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            runtime
                .plugins()
                .run_due(common::now_unix().max(0) as u64)
                .await;
        }
    });
}
