//! The launcher's own Discord Rich Presence connection
//! ([0059](../../../../docs/decisions/0059-discord-presence-is-a-daemon-loop.md)).
//! It moves out of the core once a plugin built on `ipc:connect` replaces it
//! ([0106](../../../../docs/decisions/0106-the-plugin-api-is-mechanisms-not-features.md)).
//!
//! Callers only state what they want shown; the daemon's presence thread calls
//! [`Presence::tick`], which diffs that against what was last published. Every
//! call into [`Discord`] is a blocking socket write whose peer is a process
//! Hestia does not control, so the connection lives behind its own lock and a
//! caller on an async worker never waits on it.

mod discord;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use proto::presence::Activity;

/// Discord's documented limit is 5 activity updates per 20 seconds.
const RATE_LIMIT: usize = 5;
const RATE_WINDOW: Duration = Duration::from_secs(20);

/// How many ticks a failed connect is left alone for. Discord not running is
/// the ordinary case for a resident daemon, and probing its socket every tick
/// for the hours that lasts buys nothing.
const RETRY_TICKS: u32 = 6;

pub trait Discord: Send {
    fn connect(&mut self) -> Result<()>;
    fn set(&mut self, card: &Activity) -> Result<()>;
    fn clear(&mut self) -> Result<()>;
    fn close(&mut self);
}

struct Offline;

impl Discord for Offline {
    fn connect(&mut self) -> Result<()> {
        bail!("no discord application configured")
    }

    fn set(&mut self, _: &Activity) -> Result<()> {
        bail!("not connected")
    }

    fn clear(&mut self) -> Result<()> {
        bail!("not connected")
    }

    fn close(&mut self) {}
}

struct Link {
    client: Box<dyn Discord>,
    connected: bool,
    idle_ticks: u32,
    published: Option<Activity>,
    sent: VecDeque<Instant>,
}

impl Link {
    /// `set` and `clear` panic in the crate behind [`Discord`] when it never
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
        self.published = None;
        true
    }

    fn under_rate_limit(&mut self, now: Instant) -> bool {
        while self
            .sent
            .front()
            .is_some_and(|sent| now.duration_since(*sent) >= RATE_WINDOW)
        {
            self.sent.pop_front();
        }
        self.sent.len() < RATE_LIMIT
    }

    fn sync(&mut self, wanted: Option<&Activity>, now: Instant) {
        if self.published.as_ref() == wanted {
            return;
        }
        match wanted {
            Some(card) => {
                if !self.ready() {
                    return;
                }
                if !self.under_rate_limit(now) {
                    tracing::debug!("discord presence rate limited; retrying next tick");
                    return;
                }
                if self.client.set(card).is_err() {
                    self.disconnect();
                    return;
                }
                self.sent.push_back(now);
                self.published = Some(card.clone());
            }
            None => {
                if !self.ready() {
                    return;
                }
                if self.client.clear().is_err() {
                    self.disconnect();
                    return;
                }
                self.published = None;
            }
        }
    }

    fn disconnect(&mut self) {
        tracing::debug!("discord presence disconnected");
        self.client.close();
        self.connected = false;
        self.published = None;
    }
}

/// What the launcher shows on Discord. The decision loop sets the core card;
/// a plugin sets its own. While any plugin card is set the newest wins, and
/// clearing it restores whatever is under it.
pub struct Presence {
    wanted: Mutex<Option<Activity>>,
    link: Mutex<Link>,
    stopped: AtomicBool,
}

impl Presence {
    pub fn new(client: Box<dyn Discord>) -> Self {
        Presence {
            wanted: Mutex::default(),
            link: Mutex::new(Link {
                client,
                connected: false,
                idle_ticks: 0,
                published: None,
                sent: VecDeque::new(),
            }),
            stopped: AtomicBool::new(false),
        }
    }

    /// The service over Discord's application, or a connection that never
    /// succeeds when the build names none.
    pub fn over_discord() -> Self {
        if common::app::DISCORD_APP_ID.is_empty() {
            tracing::debug!("no discord application configured; presence disabled");
            return Presence::new(Box::new(Offline));
        }
        Presence::new(Box::new(discord::Ipc::new(common::app::DISCORD_APP_ID)))
    }

    pub fn set_core(&self, card: Option<Activity>) {
        *self.wanted.lock().unwrap() = card;
    }

    /// Publishes what is wanted if it differs from what is shown. `enabled`
    /// is the user's `discord.enabled`: off, nothing is shown whoever asks.
    pub fn tick(&self, enabled: bool) {
        self.tick_at(enabled, Instant::now());
    }

    fn tick_at(&self, enabled: bool, now: Instant) {
        let card = {
            let wanted = self.wanted.lock().unwrap();
            enabled.then(|| wanted.clone()).flatten()
        };
        self.link.lock().unwrap().sync(card.as_ref(), now);
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    /// Clears what is shown and ends the ticks. Set before the clear, so a
    /// tick already in flight cannot re-publish over it on its way out.
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.link.lock().unwrap().sync(None, Instant::now());
    }
}

#[cfg(test)]
mod tests;
