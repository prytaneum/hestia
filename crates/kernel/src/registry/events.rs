//! Topics and hooks: extension points a built-in's package defines for the
//! plugins that export them. A topic is delivered through the receiver's inbox
//! and the wake-up budget; a hook is folded over the plugins in id order.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use proto::plugin::PluginState;
use tokio::task::{JoinHandle, JoinSet};

use super::guests::Guest;
use super::{Kind, Plugins};
use crate::api::Chain;
use crate::grant::{unversioned, Gate};
use crate::runtime::Extension;

/// What a plugin may have waiting to hear; past it the oldest is dropped.
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

/// A plugin's events, oldest first.
#[derive(Default)]
pub(crate) struct Inbox {
    queue: Mutex<VecDeque<Arc<dyn Point>>>,
    draining: tokio::sync::Mutex<()>,
}

impl Inbox {
    fn push(&self, event: Arc<dyn Point>, plugin: &str) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if queue.len() >= INBOX {
            queue.pop_front();
            tracing::warn!(plugin, "inbox full; the oldest event was dropped");
        }
        queue.push_back(event);
    }

    fn pop(&self) -> Option<Arc<dyn Point>> {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
    }

    fn clear(&self) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

impl Plugins {
    /// Queues `event` for every enabled plugin that exports its interface, holds
    /// the permission the table gives it and has every entry it names in scope,
    /// and delivers in the background. Awaiting the handle waits for the
    /// delivery; dropping it does not stop it.
    pub(crate) fn publish(self: &Arc<Self>, event: Arc<dyn Point>) -> JoinHandle<()> {
        let hearers = self.exporters(&*event);
        for guest in &hearers {
            guest.inbox.push(event.clone(), &guest.id);
        }
        let plugins = self.clone();
        tokio::spawn(async move {
            let mut draining = JoinSet::new();
            for guest in hearers {
                let plugins = plugins.clone();
                draining.spawn(async move { plugins.drain(&guest).await });
            }
            while draining.join_next().await.is_some() {}
        })
    }

    /// Runs `hook` through every plugin that exports it, in id order. One that
    /// fails or runs out of time is skipped. Stops at the first that vetoes and
    /// answers its id.
    pub(crate) async fn fold(&self, hook: &dyn Hook) -> Option<String> {
        let mut guests = self.exporters(hook);
        guests.sort_by(|a, b| a.id.cmp(&b.id));
        let sandbox = self.sandbox().ok()?;
        for guest in guests {
            let Some((guest, loaded)) = self.running(&guest.id) else {
                continue;
            };
            let outcome = sandbox.extend(&loaded, Chain::default(), hook).await;
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

    async fn drain(&self, guest: &Guest) {
        let _draining = guest.inbox.draining.lock().await;
        let Ok(sandbox) = self.sandbox() else { return };
        while let Some(event) = guest.inbox.pop() {
            let Some((_, loaded)) = self.running(&guest.id) else {
                guest.inbox.clear();
                return;
            };
            if !sandbox.spend_wakeup(&guest.id) {
                tracing::warn!(plugin = guest.id, "no wake-ups left; event skipped");
                continue;
            }
            let told = sandbox.extend(&loaded, Chain::default(), &*event).await;
            if let Err(error) = self.settled(&guest.id, told) {
                tracing::warn!(plugin = guest.id, %error, "plugin failed an event");
            }
        }
    }

    fn exporters(&self, point: &dyn Point) -> Vec<Arc<Guest>> {
        let interface = point.interface();
        let permission = match self.table.gate(unversioned(interface)) {
            Gate::Needs(permission) => Some(permission.name),
            Gate::Open => None,
            Gate::Unknown => return Vec::new(),
        };
        self.slots()
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest)
                    if slot.state == PluginState::Enabled
                        && guest.loaded.as_ref().is_some_and(|loaded| {
                            loaded.exports(interface)
                                && permission.is_none_or(|name| loaded.holds(name))
                        })
                        && guest.entries.refused(point.entries()).is_none() =>
                {
                    Some(guest.clone())
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use wasmtime::component::{Instance, InstancePre};
    use wasmtime::Store;

    use super::*;
    use crate::runtime::extension::{Budget, Outcome};
    use crate::runtime::State;

    struct Nothing;

    impl Extension for Nothing {
        fn budget(&self) -> Budget {
            Budget::Event
        }

        fn call<'a>(
            &'a self,
            _: &'a mut Store<State>,
            _: &'a Instance,
            _: &'a InstancePre<State>,
        ) -> Outcome<'a> {
            Box::pin(async { Ok(Ok(())) })
        }
    }

    impl Point for Nothing {
        fn interface(&self) -> &'static str {
            "hestia:probe/entry-opened@0.1.0"
        }
    }

    #[test]
    fn an_inbox_holds_at_most_its_capacity() {
        let inbox = Inbox::default();
        for _ in 0..INBOX + 44 {
            inbox.push(Arc::new(Nothing), "ann");
        }
        let mut held = 0;
        while inbox.pop().is_some() {
            held += 1;
        }
        assert_eq!(held, INBOX);
    }
}
