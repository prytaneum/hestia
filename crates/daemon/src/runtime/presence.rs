//! Discord Rich Presence: what is being played, published to the user's own
//! Discord client over that client's local IPC socket.
//!
//! A tick loop rather than hooks on the launch and exit paths — Discord
//! starting after the daemon, sessions re-adopted across a restart, and the
//! rate limit on activity updates all resolve the same way when the loop diffs
//! against what it last published.
//!
//! It runs on its own thread rather than a tokio task: every call into the
//! Discord client is a blocking socket write whose peer is a process hestia
//! does not control.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use discord_rich_presence::activity::{Activity, Assets, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use engine::{InstanceProfile, Side};
use proto::naming;
use proto::process::ProcessState;

use super::Runtime;

const TICK: Duration = Duration::from_secs(5);

/// How many ticks a failed connect is left alone for. Discord not running is
/// the ordinary case for a resident daemon, and probing its socket every tick
/// for the hours that lasts buys nothing.
const RETRY_TICKS: u32 = 6;

const IDLE: &str = "Idling…";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Card {
    details: String,
    state: String,
    since: Option<i64>,
}

impl Card {
    fn idle() -> Self {
        Card {
            details: IDLE.to_string(),
            state: String::new(),
            since: None,
        }
    }
}

pub struct Presence {
    link: Mutex<Link>,
    stopped: AtomicBool,
}

impl Presence {
    pub fn shutdown(&self) {
        // Set before the clear, so a tick already in flight cannot re-publish
        // over it on its way out.
        self.stopped.store(true, Ordering::Relaxed);
        self.link.lock().unwrap().clear();
    }
}

struct Link {
    client: DiscordIpcClient,
    connected: bool,
    idle_ticks: u32,
    published: Option<Card>,
}

impl Link {
    /// `set_activity` and `clear_activity` panic when the client has never
    /// connected, and `hestiad` is built with `panic = "abort"` — so a failed
    /// connect has to gate every later call rather than merely be logged.
    fn ready(&mut self) -> bool {
        if self.connected {
            return true;
        }
        if self.idle_ticks > 0 {
            self.idle_ticks -= 1;
            return false;
        }
        if self.client.connect().is_err() {
            self.idle_ticks = RETRY_TICKS;
            return false;
        }
        tracing::debug!("discord presence connected");
        self.connected = true;
        self.idle_ticks = 0;
        // A fresh connection holds no activity.
        self.published = None;
        true
    }

    fn publish(&mut self, card: Card) {
        if self.published.as_ref() == Some(&card) || !self.ready() {
            return;
        }
        let assets = Assets::new()
            .large_image(common::app::DISCORD_LARGE_IMAGE)
            .large_text(common::app::NAME);
        let mut activity = Activity::new().details(&card.details).assets(assets);
        if !card.state.is_empty() {
            activity = activity.state(&card.state);
        }
        if let Some(since) = card.since {
            activity = activity.timestamps(Timestamps::new().start(since));
        }
        if self.client.set_activity(activity).is_err() {
            self.disconnect();
            return;
        }
        self.published = Some(card);
    }

    fn clear(&mut self) {
        if self.published.is_none() || !self.ready() {
            return;
        }
        if self.client.clear_activity().is_err() {
            self.disconnect();
            return;
        }
        self.published = None;
    }

    fn disconnect(&mut self) {
        tracing::debug!("discord presence disconnected");
        let _ = self.client.close();
        self.connected = false;
        self.published = None;
    }
}

/// Set by automated tests: several throwaway daemons reaching for the one
/// Discord client at once leaves some of them blocked in its handshake, and a
/// test daemon has nothing to publish anyway.
fn suppressed() -> bool {
    std::env::var_os("HESTIA_NO_PRESENCE").is_some_and(|value| !value.is_empty() && value != "0")
}

/// Start publishing presence, returning the handle the shutdown path clears
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
    let presence = Arc::new(Presence {
        link: Mutex::new(Link {
            client: DiscordIpcClient::new(common::app::DISCORD_APP_ID),
            connected: false,
            idle_ticks: 0,
            published: None,
        }),
        stopped: AtomicBool::new(false),
    });
    let worker = presence.clone();
    std::thread::Builder::new()
        .name("discord-presence".to_string())
        .spawn(move || {
            while !worker.stopped.load(Ordering::Relaxed) {
                tick(&runtime, &worker);
                std::thread::sleep(TICK);
            }
        })
        .map_err(|e| tracing::warn!("cannot start discord presence: {e}"))
        .ok()?;
    Some(presence)
}

fn tick(runtime: &Runtime, presence: &Presence) {
    let mut link = presence.link.lock().unwrap();
    if !runtime.engine().config().settings().discord.enabled {
        link.clear();
        return;
    }
    link.publish(playing(runtime).unwrap_or_else(Card::idle));
}

/// The newest running session. Concurrent sessions collapse to one: Discord
/// shows a single activity.
fn playing(runtime: &Runtime) -> Option<Card> {
    let (id, started) = runtime
        .processes()
        .list()
        .into_iter()
        .filter(|p| p.state == ProcessState::Running)
        .filter_map(|p| naming::instance_id_of_session(&p.id).map(|id| (id, p.started_unix)))
        .max_by_key(|(_, started)| *started)?;
    let record = runtime.engine().instances().get(&id)?;
    Some(Card {
        details: record.name,
        state: describe(runtime, &record.profile),
        since: Some(started),
    })
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
