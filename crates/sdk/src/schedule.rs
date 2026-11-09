//! Hearing the plugin's own `contributes.schedules`. A tick arrives through
//! [`Plugin::on_event`](crate::Plugin::on_event) like any event, from the host
//! to this plugin only.

use proto::plugin::ScheduleEvent;
use proto::Topic;
use serde_json::Value;

/// The schedule id when `topic` and `payload` are a tick, else `None`.
///
/// ```no_run
/// # use hestia::{Plugin, Value};
/// # struct Poller;
/// impl Plugin for Poller {
///     fn on_event(topic: &str, payload: Value) {
///         if hestia::schedule::tick(topic, &payload) == Some("poll") {
///             hestia::log::info("polling");
///         }
///     }
/// }
/// ```
pub fn tick<'a>(topic: &str, payload: &'a Value) -> Option<&'a str> {
    if topic != ScheduleEvent::TOPIC {
        return None;
    }
    payload.get("id")?.as_str()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_a_schedule_topic_is_a_tick() {
        let payload = json!({ "id": "poll" });
        assert_eq!(tick("plugin.schedule", &payload), Some("poll"));
        assert_eq!(tick("plugin.state", &payload), None);
        assert_eq!(tick("plugin.schedule", &json!({})), None);
    }
}
