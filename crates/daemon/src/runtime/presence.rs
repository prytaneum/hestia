//! The decision loop of Discord Rich Presence: what is being played, handed to
//! the engine's presence service, which owns the connection
//! ([0059](../../../../docs/decisions/0059-discord-presence-is-a-daemon-loop.md),
//! [0106](../../../../docs/decisions/0106-the-plugin-api-is-mechanisms-not-features.md)).
//!
//! It runs on its own thread rather than a tokio task: the tick publishes
//! through a blocking socket write whose peer is a process hestia does not
//! control.

use std::sync::Arc;
use std::time::Duration;

use engine::{InstanceProfile, Presence, Side};
use proto::naming;
use proto::presence::Activity;
use proto::process::ProcessState;

use super::Runtime;

const TICK: Duration = Duration::from_secs(5);

const IDLE: &str = "Idling…";

fn idle() -> Activity {
    card(IDLE.to_string(), None, None)
}

fn card(details: String, state: Option<String>, since: Option<i64>) -> Activity {
    Activity {
        details: Some(details),
        state,
        start: since,
        large_image: Some(common::app::DISCORD_LARGE_IMAGE.to_string()),
        large_text: Some(common::app::NAME.to_string()),
        ..Activity::default()
    }
}

/// Set by automated tests: several throwaway daemons reaching for the one
/// Discord client at once leaves some of them blocked in its handshake, and a
/// test daemon has nothing to publish anyway.
fn suppressed() -> bool {
    std::env::var_os("HESTIA_NO_PRESENCE").is_some_and(|value| !value.is_empty() && value != "0")
}

/// Start publishing presence, returning the service the shutdown path clears
/// through. `None` when the build names no Discord application.
pub fn spawn_presence_updater(runtime: Arc<Runtime>) -> Option<Arc<Presence>> {
    if common::app::DISCORD_APP_ID.is_empty() {
        tracing::debug!("no discord application configured; presence disabled");
        return None;
    }
    if suppressed() {
        tracing::debug!("HESTIA_NO_PRESENCE set; not publishing discord presence");
        return None;
    }
    let presence = runtime.engine().presence().clone();
    let worker = presence.clone();
    std::thread::Builder::new()
        .name("discord-presence".to_string())
        .spawn(move || {
            while !worker.stopped() {
                tick(&runtime, &worker);
                std::thread::sleep(TICK);
            }
        })
        .map_err(|e| tracing::warn!("cannot start discord presence: {e}"))
        .ok()?;
    Some(presence)
}

fn tick(runtime: &Runtime, presence: &Presence) {
    let enabled = runtime.engine().config().settings().discord.enabled;
    presence.set_core(enabled.then(|| playing(runtime).unwrap_or_else(idle)));
    presence.tick(enabled);
}

/// The newest running session. Concurrent sessions collapse to one: Discord
/// shows a single activity.
fn playing(runtime: &Runtime) -> Option<Activity> {
    let (id, started) = runtime
        .processes()
        .list()
        .into_iter()
        .filter(|p| p.state == ProcessState::Running)
        .filter_map(|p| naming::instance_id_of_session(&p.id).map(|id| (id, p.started_unix)))
        .max_by_key(|(_, started)| *started)?;
    let record = runtime.engine().instances().get(&id)?;
    Some(card(
        record.name,
        Some(describe(runtime, &record.profile)),
        Some(started),
    ))
}

fn describe(runtime: &Runtime, profile: &InstanceProfile) -> String {
    let flavor = runtime
        .engine()
        .minecraft()
        .flavors(Side::Client)
        .into_iter()
        .find(|f| f.id == profile.flavor)
        .map(|f| f.name)
        .unwrap_or_else(|| profile.flavor.clone());
    format!("{flavor} {}", profile.game_version)
}
