//! Hearing daemon events. The host calls [`Plugin::event`](crate::Plugin::event)
//! for each event matching a glob of the manifest's `contributes.events` that
//! the plugin's permissions allow; route each topic to a handler with
//! [`Events::on`], typed by the topic in [`proto`](crate::proto). An event no
//! handler takes is ignored, and one whose payload does not decode is logged and
//! dropped: an event has no answer to fail with.
//!
//! A tick of one of the manifest's `contributes.schedules` is the
//! [`ScheduleEvent`](crate::proto::plugin::ScheduleEvent) topic.
//!
//! ```no_run
//! use hestia::proto::plugin::ScheduleEvent;
//! use hestia::proto::process::ProcessExitEvent;
//! use hestia::{Events, Plugin};
//!
//! struct Watcher;
//!
//! impl Plugin for Watcher {
//!     fn event(call: Events) {
//!         call.on::<ProcessExitEvent>(|exit| {
//!             hestia::log::info(&format!("{} exited", exit.id));
//!         })
//!         .on::<ScheduleEvent>(|tick| {
//!             if tick.id == "poll" {
//!                 hestia::log::info("polling");
//!             }
//!         })
//!         .glob("server.*", |topic, _payload| {
//!             hestia::log::debug(topic);
//!         })
//!         .finish()
//!     }
//! }
//!
//! hestia::plugin!(Watcher);
//! ```

use proto::Topic;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// One event the host is delivering, waiting for the handler that takes it.
pub struct Events {
    topic: String,
    payload: String,
    taken: bool,
}

impl Events {
    #[doc(hidden)]
    pub fn new(topic: String, payload: String) -> Self {
        Events {
            topic,
            payload,
            taken: false,
        }
    }

    /// The topic being delivered.
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Gives the event to `handler` when it is topic `T`; the first handler
    /// that matches takes it. A payload that does not decode as `T` is logged
    /// with [`log::warn`](crate::log::warn) and dropped.
    pub fn on<T: Topic + DeserializeOwned>(mut self, handler: impl FnOnce(T)) -> Self {
        if self.taken || self.topic != T::TOPIC {
            return self;
        }
        self.taken = true;
        match serde_json::from_str(&self.payload) {
            Ok(event) => handler(event),
            Err(error) => {
                crate::log::warn(&format!("unreadable `{}` payload: {error}", self.topic))
            }
        }
        self
    }

    /// Gives the event to `handler` as its topic and raw JSON when the topic
    /// matches `pattern`: an exact topic, a prefix such as `server.*` (the
    /// topics beneath `server.`, not `server`), or `*` for every topic. Use it for
    /// a glob the manifest declares; put typed [`on`](Events::on) handlers
    /// before it, since the first match takes the event.
    pub fn glob(mut self, pattern: &str, handler: impl FnOnce(&str, Value)) -> Self {
        if self.taken || !matches(pattern, &self.topic) {
            return self;
        }
        self.taken = true;
        let payload = serde_json::from_str(&self.payload).unwrap_or_default();
        handler(&self.topic, payload);
        self
    }

    /// Ends the chain; an event no handler took is dropped.
    pub fn finish(self) {}
}

fn matches(pattern: &str, topic: &str) -> bool {
    match pattern.strip_suffix(".*") {
        Some(prefix) => topic
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1),
        None => pattern == "*" || pattern == topic,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use proto::plugin::ScheduleEvent;
    use serde_json::json;

    use super::*;

    fn event(topic: &str, payload: Value) -> Events {
        Events::new(topic.into(), payload.to_string())
    }

    #[test]
    fn globs_follow_the_manifest_rules() {
        assert!(matches("process.exit", "process.exit"));
        assert!(!matches("process.exit", "process.started"));
        assert!(matches("process.*", "process.exit"));
        assert!(matches("process.*", "process.a.b"));
        assert!(!matches("process.*", "process"));
        assert!(!matches("process.*", "processes.exit"));
        assert!(!matches("process.*", "process."));
        assert!(matches("*", "anything.at.all"));
    }

    #[test]
    fn a_typed_handler_takes_its_topic() {
        let seen = RefCell::new(None);
        event("plugin.schedule", json!({ "id": "poll" }))
            .on::<ScheduleEvent>(|tick| *seen.borrow_mut() = Some(tick.id))
            .finish();
        assert_eq!(seen.into_inner().as_deref(), Some("poll"));
    }

    #[test]
    fn only_the_first_matching_handler_runs() {
        let calls = RefCell::new(Vec::new());
        event("plugin.schedule", json!({ "id": "a" }))
            .on::<ScheduleEvent>(|_| calls.borrow_mut().push("typed"))
            .glob("plugin.*", |_, _| calls.borrow_mut().push("glob"))
            .finish();
        assert_eq!(calls.into_inner(), ["typed"]);
    }

    #[test]
    fn an_untaken_event_is_ignored() {
        let calls = RefCell::new(0);
        event("job", json!({}))
            .on::<ScheduleEvent>(|_| *calls.borrow_mut() += 1)
            .glob("server.*", |_, _| *calls.borrow_mut() += 1)
            .finish();
        assert_eq!(calls.into_inner(), 0);
    }

    #[test]
    fn a_glob_handler_gets_the_topic_and_the_raw_payload() {
        let seen = RefCell::new(None);
        event("server.created", json!({ "n": 1 }))
            .glob("server.*", |topic, payload| {
                *seen.borrow_mut() = Some((topic.to_string(), payload))
            })
            .finish();
        assert_eq!(
            seen.into_inner(),
            Some(("server.created".to_string(), json!({ "n": 1 })))
        );
    }
}
