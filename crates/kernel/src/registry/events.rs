//! Topics and hooks: extension points a built-in's package defines for the
//! plugins that export them. A topic is delivered through the receiver's inbox
//! at the pace of its event budget; a hook is folded over the plugins in id
//! order.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use proto::plugin::PluginState;
use tokio::sync::Notify;

use super::guests::Guest;
use super::{Kind, Plugins};
use crate::api::Chain;
use crate::grant::{unversioned, Gate};
use crate::runtime::Extension;

/// What a plugin may have waiting to hear; past it the oldest is dropped. An
/// event otherwise waits for the plugin's event budget to refill.
const INBOX: usize = 256;

/// One interface a plugin exports for the host to call.
pub(crate) trait Point: Extension {
    /// `package/interface@version`, as the component exports it.
    fn interface(&self) -> &'static str;

    /// The stable ids of the entries the call is about. A plugin hears or
    /// customises it only if its `entries` scope holds every one.
    fn entries(&self) -> &[String] {
        &[]
    }
}

/// A point whose plugins each may stop the rest.
pub(crate) trait Hook: Point {
    fn vetoed(&self) -> bool;
}

/// A plugin's events, oldest first, and the one task that delivers them.
#[derive(Default)]
pub(crate) struct Inbox {
    queue: Mutex<VecDeque<Arc<dyn Point>>>,
    scheduled: AtomicBool,
    dropped: AtomicUsize,
    idle: Notify,
}

impl Inbox {
    fn queue(&self) -> std::sync::MutexGuard<'_, VecDeque<Arc<dyn Point>>> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, event: Arc<dyn Point>, plugin: &str) {
        let mut queue = self.queue();
        if queue.len() >= INBOX {
            queue.pop_front();
            if self.dropped.fetch_add(1, Ordering::Relaxed) == 0 {
                tracing::warn!(plugin, "inbox full; the oldest events are dropped");
            }
        }
        queue.push_back(event);
    }

    fn pop(&self) -> Option<Arc<dyn Point>> {
        self.queue().pop_front()
    }

    fn clear(&self) -> usize {
        let mut queue = self.queue();
        let cleared = queue.len();
        queue.clear();
        cleared
    }

    /// Whether the caller must start the task that delivers.
    fn schedule(&self) -> bool {
        !self.scheduled.swap(true, Ordering::AcqRel)
    }
}

impl Plugins {
    /// Queues `event` for every enabled plugin that exports its interface, holds
    /// the permission the table gives it and has every entry it names in scope,
    /// and delivers in the background, one task per inbox. Each event is
    /// checked again just before it is delivered.
    pub(crate) fn publish(self: &Arc<Self>, event: Arc<dyn Point>) {
        for guest in self.exporters(&*event) {
            guest.inbox.push(event.clone(), &guest.id);
            if guest.inbox.schedule() {
                let plugins = self.clone();
                tokio::spawn(async move { plugins.drain(guest).await });
            }
        }
    }

    /// Resolves once every inbox has delivered what it held.
    pub async fn settle(&self) {
        let guests: Vec<_> = self
            .slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest) => Some(guest.clone()),
                _ => None,
            })
            .collect();
        for guest in guests {
            loop {
                let idle = guest.inbox.idle.notified();
                if !guest.inbox.scheduled.load(Ordering::Acquire) {
                    break;
                }
                idle.await;
            }
        }
    }

    /// Runs `hook` through every plugin that exports it, in id order, for a call
    /// made through `chain`. One that fails or runs out of time is skipped. Stops
    /// at the first that vetoes and answers its id.
    pub(crate) async fn fold(&self, hook: &dyn Hook, chain: &Chain) -> Option<String> {
        let mut guests = self.exporters(hook);
        guests.sort_by(|a, b| a.id.cmp(&b.id));
        let sandbox = self.sandbox().ok()?;
        for guest in guests {
            let Some((guest, loaded)) = self.running(&guest.id) else {
                continue;
            };
            let outcome = sandbox.extend(&loaded, chain.clone(), hook).await;
            if let Err(error) = self.settled(&guest.id, outcome) {
                tracing::warn!(plugin = guest.id, %error, "hook skipped");
                continue;
            }
            if hook.vetoed() {
                tracing::info!(plugin = guest.id, "hook vetoed");
                return Some(guest.id.clone());
            }
        }
        None
    }

    async fn drain(&self, guest: Arc<Guest>) {
        loop {
            self.deliver(&guest).await;
            let dropped = guest.inbox.dropped.swap(0, Ordering::Relaxed);
            if dropped > 0 {
                tracing::warn!(
                    plugin = guest.id,
                    dropped,
                    "events were dropped from a full inbox"
                );
            }
            guest.inbox.scheduled.store(false, Ordering::Release);
            if guest.inbox.queue().is_empty() || !guest.inbox.schedule() {
                break;
            }
        }
        guest.inbox.idle.notify_waiters();
    }

    /// Delivers the inbox one event at a time, each when the plugin's event
    /// budget has one. A guest that is no longer the running one loses what was
    /// queued for it; one that no longer hears an event skips that event alone.
    async fn deliver(&self, guest: &Arc<Guest>) {
        let Ok(sandbox) = self.sandbox() else {
            guest.inbox.clear();
            return;
        };
        while let Some(event) = guest.inbox.pop() {
            while let Err(wait) = sandbox.spend_event(&guest.id) {
                tokio::time::sleep(wait).await;
            }
            let Some((running, loaded)) = self.running(&guest.id) else {
                guest.inbox.clear();
                return;
            };
            if !Arc::ptr_eq(&running, guest) {
                guest.inbox.clear();
                return;
            }
            if !self.hears(guest, &*event) {
                tracing::debug!(plugin = guest.id, "event no longer heard; skipped");
                continue;
            }
            let told = sandbox.extend(&loaded, Chain::default(), &*event).await;
            if let Err(error) = self.settled(&guest.id, told) {
                tracing::warn!(plugin = guest.id, %error, "plugin failed an event");
            }
        }
    }

    /// Whether `guest` exports the point's interface, holds the permission the
    /// table gives it and has every entry it names in scope.
    fn hears(&self, guest: &Guest, point: &dyn Point) -> bool {
        let interface = point.interface();
        let permission = match self.table.gate(unversioned(interface)) {
            Gate::Needs(permission) => Some(permission.name),
            Gate::Open => None,
            Gate::Unknown => return false,
        };
        guest.loaded.as_ref().is_some_and(|loaded| {
            loaded.exports(interface) && permission.is_none_or(|name| loaded.holds(name))
        }) && guest.entries.refused(point.entries()).is_none()
    }

    fn exporters(&self, point: &dyn Point) -> Vec<Arc<Guest>> {
        self.slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest)
                    if slot.state == PluginState::Enabled && self.hears(guest, point) =>
                {
                    Some(guest.clone())
                }
                _ => None,
            })
            .collect()
    }
}
