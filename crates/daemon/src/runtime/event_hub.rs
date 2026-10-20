//! Fans daemon events out to subscribed client connections (via the
//! `events.subscribe` channel). Each subscriber holds the sender for its
//! connection's writer task; a closed connection is pruned on the next publish.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ipc::protocol::{encode_event, Event};
use proto::Topic;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

struct Sub {
    conn_id: u64,
    out: UnboundedSender<String>,
    // Filter to one id (matched against the event payload's "id"); None
    // subscribes to every event. An entry key also covers the session keys
    // beneath it, so following an entry outlives its processes.
    filter: Option<String>,
}

#[derive(Default)]
pub struct EventHub {
    subs: Mutex<Vec<Sub>>,
    last_subscriber_id: AtomicU64,
}

impl EventHub {
    /// A subscriber id no other connection or job watch holds.
    pub fn next_subscriber_id(&self) -> u64 {
        self.last_subscriber_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn subscribe(&self, conn_id: u64, out: UnboundedSender<String>, filter: Option<String>) {
        tracing::debug!(
            conn = conn_id,
            filter = filter.as_deref(),
            "event subscription added"
        );
        self.subs.lock().unwrap().push(Sub {
            conn_id,
            out,
            filter,
        });
    }

    /// Whether any live subscriber takes every event (unfiltered) — only these
    /// receive an idless broadcast like `process.metrics`.
    pub fn has_broadcast_subscriber(&self) -> bool {
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|s| !s.out.is_closed());
        subs.iter().any(|s| s.filter.is_none())
    }

    pub fn unsubscribe(&self, conn_id: u64) {
        let mut subs = self.subs.lock().unwrap();
        let before = subs.len();
        subs.retain(|s| s.conn_id != conn_id);
        if subs.len() < before {
            tracing::debug!(conn = conn_id, "event subscriptions removed");
        }
    }

    /// Publish a topic as its own payload; one that fails to serialise is
    /// logged and dropped.
    pub fn publish_topic<T: Topic + Serialize>(&self, topic: &T) {
        match serde_json::to_value(topic) {
            Ok(payload) => self.publish(&Event {
                topic: T::TOPIC.to_string(),
                payload,
            }),
            Err(e) => tracing::warn!(topic = T::TOPIC, error = %e, "event not serialisable"),
        }
    }

    /// Deliver `event` to every matching subscriber, pruning any whose connection
    /// has gone away.
    pub fn publish(&self, event: &Event) {
        let id = event.payload.get("id").and_then(Value::as_str);
        tracing::trace!(topic = %event.topic, id, "publishing event");
        let frame = encode_event(event);
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|sub| {
            if let Some(filter) = &sub.filter {
                if !id.is_some_and(|id| proto::naming::process_in_scope(filter, id)) {
                    return !sub.out.is_closed();
                }
            }
            sub.out.send(frame.clone()).is_ok()
        });
    }
}

/// The hub as the engine's supervisor sees it: a place to publish, with no
/// knowledge of the socket behind it.
impl engine::ProcessEvents for EventHub {
    fn publish(&self, topic: &str, payload: Value) {
        EventHub::publish(
            self,
            &Event {
                topic: topic.to_string(),
                payload,
            },
        );
    }
}
