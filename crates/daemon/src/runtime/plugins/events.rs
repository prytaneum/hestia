//! Hub events on their way to a plugin's `on-event`. What a plugin may receive is
//! decided here, once per event, before anything is queued: the manifest's globs
//! pick the topics, and the plugin's permissions pick which of those it may see.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use engine::{CallChain, CallError, HostCalls, LoadedPlugin, EVENT_TIMEOUT};
use proto::glob::topic_matches;
use proto::plugin::{Grant, Permission, PluginStateEvent};
use proto::process::{
    ProcessExitEvent, ProcessMetricsEvent, ProcessOutputEvent, ProcessStartedEvent,
};
use proto::Topic;
use serde_json::{json, Value};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::runtime::EventHub;

const QUEUE_CAPACITY: usize = 256;
const WARN_EVERY: Duration = Duration::from_secs(60);
const TIMEOUTS_IN_A_ROW: u32 = 3;

/// Topics no manifest or grant can reach. A client prints its own session id to
/// stdout, so process output is secret-bearing.
const NEVER: &[&str] = &[ProcessOutputEvent::TOPIC];

/// The topics that name a permission; every other topic is withheld.
const TOPIC_PERMISSIONS: [(&str, Option<Permission>); 3] = [
    (ProcessStartedEvent::TOPIC, ProcessStartedEvent::PLUGIN),
    (ProcessExitEvent::TOPIC, ProcessExitEvent::PLUGIN),
    (ProcessMetricsEvent::TOPIC, ProcessMetricsEvent::PLUGIN),
];

/// The payload `plugin` may receive for this event, or `None` to withhold it.
/// A `job` event goes only to a plugin granted the permission of the channel
/// that started the job; `plugin.state` needs none but is cut to its id and state.
pub fn admit(grant: &Grant, host: &dyn HostCalls, topic: &str, payload: &Value) -> Option<Value> {
    if NEVER.contains(&topic)
        || !grant
            .events
            .iter()
            .any(|pattern| topic_matches(pattern, topic))
    {
        return None;
    }
    if topic == PluginStateEvent::TOPIC {
        return Some(json!({ "id": payload.get("id"), "state": payload.get("state") }));
    }
    let needed = if topic == proto::job::TOPIC {
        host.permission(payload.get("channel")?.as_str()?)
    } else {
        TOPIC_PERMISSIONS
            .iter()
            .find(|(known, _)| *known == topic)
            .and_then(|(_, permission)| *permission)
    };
    needed
        .is_some_and(|permission| grant.permissions.contains(&permission))
        .then(|| payload.clone())
}

/// A plugin's pending events: bounded, and when full the oldest is dropped, so a
/// slow plugin falls behind on history instead of growing the daemon.
struct Inbox {
    plugin: String,
    queue: Mutex<VecDeque<(String, Value)>>,
    ready: Notify,
    dropped: AtomicU64,
    last_warned: Mutex<Option<Instant>>,
}

impl Inbox {
    fn new(plugin: &str) -> Self {
        Inbox {
            plugin: plugin.to_string(),
            queue: Mutex::default(),
            ready: Notify::new(),
            dropped: AtomicU64::new(0),
            last_warned: Mutex::new(None),
        }
    }

    fn push(&self, topic: &str, payload: Value) {
        let overflowed = {
            let mut queue = self.queue.lock().unwrap();
            let overflowed = queue.len() >= QUEUE_CAPACITY;
            if overflowed {
                queue.pop_front();
            }
            queue.push_back((topic.to_string(), payload));
            overflowed
        };
        if overflowed {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.warn_dropped();
        }
        self.ready.notify_one();
    }

    fn warn_dropped(&self) {
        let mut last = self.last_warned.lock().unwrap();
        if last.is_none_or(|warned| warned.elapsed() >= WARN_EVERY) {
            *last = Some(Instant::now());
            tracing::warn!(
                plugin = %self.plugin,
                dropped = self.dropped(),
                "plugin is not keeping up with events, dropping the oldest"
            );
        }
    }

    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    async fn pop(&self) -> (String, Value) {
        loop {
            if let Some(event) = self.queue.lock().unwrap().pop_front() {
                return event;
            }
            self.ready.notified().await;
        }
    }
}

/// A plugin's subscription to the hub and the task serving it. Dropping it ends
/// both, which is how disabling, removing or reloading a plugin stops delivery.
pub struct Delivery {
    key: String,
    hub: Arc<EventHub>,
    task: JoinHandle<()>,
}

impl Delivery {
    /// `None` when the plugin subscribes to nothing.
    pub fn start(
        hub: Arc<EventHub>,
        host: Arc<dyn HostCalls>,
        loaded: Arc<LoadedPlugin>,
    ) -> Option<Delivery> {
        if loaded.grant().events.is_empty() {
            return None;
        }
        let key = format!("plugin:{}:{}", loaded.id(), hub.next_subscriber_id());
        let inbox = Arc::new(Inbox::new(loaded.id()));
        let grant = Arc::new(loaded.grant().clone());
        let queue = inbox.clone();
        hub.listen(
            &key,
            Arc::new(move |topic, payload| {
                if let Some(payload) = admit(&grant, host.as_ref(), topic, payload) {
                    queue.push(topic, payload);
                }
            }),
        );
        let task = tokio::spawn(drain(inbox, loaded, hub.clone(), key.clone()));
        Some(Delivery { key, hub, task })
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        self.hub.unlisten(&self.key);
        self.task.abort();
    }
}

/// Serial, in order, each event on a fresh store. Stops for good, until the
/// plugin is loaded again, after [`TIMEOUTS_IN_A_ROW`] timeouts.
async fn drain(inbox: Arc<Inbox>, plugin: Arc<LoadedPlugin>, hub: Arc<EventHub>, key: String) {
    let chain = CallChain::default();
    let mut timeouts = 0;
    loop {
        let (topic, payload) = inbox.pop().await;
        match plugin
            .on_event(&chain, &topic, &payload, EVENT_TIMEOUT)
            .await
        {
            Ok(()) => timeouts = 0,
            Err(CallError::Timeout(_)) => {
                timeouts += 1;
                tracing::warn!(plugin = %plugin.id(), %topic, timeouts, "plugin timed out on an event");
                if timeouts >= TIMEOUTS_IN_A_ROW {
                    tracing::error!(plugin = %plugin.id(), "events stopped: the plugin keeps timing out");
                    break;
                }
            }
            Err(CallError::Disabled) => break,
            Err(error) => {
                timeouts = 0;
                tracing::warn!(plugin = %plugin.id(), %topic, %error, "plugin failed on an event");
            }
        }
    }
    hub.unlisten(&key);
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use engine::HostError;

    use super::*;

    struct Channels;

    #[async_trait]
    impl HostCalls for Channels {
        fn permission(&self, channel: &str) -> Option<Permission> {
            (channel == "content.search").then_some(Permission::ContentRead)
        }

        async fn call(
            &self,
            _: &str,
            _: &CallChain,
            _: &str,
            _: Value,
        ) -> Result<Value, HostError> {
            unreachable!()
        }

        async fn run_job(
            &self,
            _: &str,
            _: &CallChain,
            _: &str,
            _: Value,
        ) -> Result<Value, HostError> {
            unreachable!()
        }
    }

    fn grant(events: &[&str], permissions: &[Permission]) -> Grant {
        Grant {
            events: events.iter().map(|e| e.to_string()).collect(),
            permissions: permissions.iter().copied().collect(),
            ..Grant::default()
        }
    }

    fn admitted(grant: &Grant, topic: &str, payload: Value) -> Option<Value> {
        admit(grant, &Channels, topic, &payload)
    }

    #[test]
    fn a_topic_needs_a_matching_glob_and_its_permission() {
        let exit = json!({ "id": "smp", "success": true });
        let both = grant(&["process.*"], &[Permission::ProcessRead]);
        assert_eq!(
            admitted(&both, "process.exit", exit.clone()),
            Some(exit.clone())
        );
        assert!(admitted(
            &grant(&["server.*"], &[Permission::ProcessRead]),
            "process.exit",
            exit.clone()
        )
        .is_none());
        assert!(admitted(&grant(&["process.*"], &[]), "process.exit", exit).is_none());
    }

    #[test]
    fn unclassified_topics_are_withheld_even_when_subscribed() {
        let everything = grant(
            &["net.*", "changed", "process.*"],
            &[Permission::ProcessRead],
        );
        assert!(admitted(&everything, "net.state", json!({})).is_none());
        assert!(admitted(&everything, "changed", json!({})).is_none());
        assert!(admitted(&everything, "process.unknown", json!({})).is_none());
    }

    #[test]
    fn process_output_is_never_delivered() {
        let all = grant(
            &["process.*", "process.output"],
            &[Permission::ProcessRead, Permission::ProcessControl],
        );
        assert!(admitted(&all, "process.output", json!({ "id": "a", "lines": [] })).is_none());
    }

    #[test]
    fn a_job_is_delivered_only_for_a_channel_the_plugin_holds() {
        let job = |channel: &str| json!({ "id": "j1", "channel": channel, "state": "done" });
        let content = grant(&["job"], &[Permission::ContentRead]);
        assert!(admitted(&content, "job", job("content.search")).is_some());
        assert!(admitted(&content, "job", job("java.install")).is_none());
        assert!(admitted(&content, "job", job("plugin.run")).is_none());
        assert!(admitted(&content, "job", json!({ "id": "j1" })).is_none());
        assert!(admitted(&grant(&["job"], &[]), "job", job("content.search")).is_none());
    }

    #[test]
    fn plugin_state_needs_no_permission_and_carries_only_id_and_state() {
        let event = json!({ "id": "demo", "state": "failed", "error": "/home/me/secret failed" });
        let delivered = admitted(&grant(&["plugin.*"], &[]), "plugin.state", event.clone());
        assert_eq!(delivered, Some(json!({ "id": "demo", "state": "failed" })));
        assert!(admitted(&grant(&["server.*"], &[]), "plugin.state", event).is_none());
    }

    #[test]
    fn a_full_queue_drops_the_oldest_and_counts_it() {
        let inbox = Inbox::new("demo");
        for n in 0..QUEUE_CAPACITY + 44 {
            inbox.push("server.tick", json!(n));
        }
        assert_eq!(inbox.dropped(), 44);
        let queue = inbox.queue.lock().unwrap();
        assert_eq!(queue.len(), QUEUE_CAPACITY);
        assert_eq!(queue.front().unwrap().1, json!(44));
        assert_eq!(queue.back().unwrap().1, json!(QUEUE_CAPACITY + 43));
    }

    #[tokio::test]
    async fn pop_waits_for_a_push_in_order() {
        let inbox = Arc::new(Inbox::new("demo"));
        let waiting = tokio::spawn({
            let inbox = inbox.clone();
            async move { (inbox.pop().await, inbox.pop().await) }
        });
        inbox.push("a", json!(1));
        inbox.push("b", json!(2));
        let (first, second) = waiting.await.unwrap();
        assert_eq!((first.0.as_str(), second.0.as_str()), ("a", "b"));
    }
}
