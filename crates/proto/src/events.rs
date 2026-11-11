//! Subscribing to daemon events, and which events a plugin may receive.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{Contract, Topic};
use crate::plugin::Permission;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct EventsSubscribeParams {
    /// Empty subscribes to every event.
    #[serde(default)]
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EventsSubscribeResult {
    pub subscribed: bool,
}

pub struct EventsSubscribe;
impl Contract for EventsSubscribe {
    const CHANNEL: &'static str = "events.subscribe";
    type Params = EventsSubscribeParams;
    type Result = EventsSubscribeResult;
}

/// What a plugin needs to receive one event of `topic`.
#[derive(Clone, Copy)]
pub struct TopicAccess {
    pub topic: &'static str,
    pub permission: fn(&Value) -> Option<Permission>,
}

const fn access<T: Topic>() -> TopicAccess {
    TopicAccess {
        topic: T::TOPIC,
        permission: T::permission,
    }
}

/// Every topic the daemon publishes; `tests/permission.rs` fails one that is
/// missing. The `job` and `plugin.state` topics name no permission here: the
/// daemon admits a job event by the permission of the channel that started it,
/// and cuts a plugin state event to its id and state. `plugin.schedule` names none
/// and is never published to the hub, so only the plugin that declared it hears it.
pub const TOPICS: &[TopicAccess] = &[
    access::<crate::change::ChangedEvent>(),
    access::<crate::job::JobEvent<(), ()>>(),
    access::<crate::net::NetworkStatus>(),
    access::<crate::plugin::Outdated>(),
    access::<crate::plugin::PluginLogEvent>(),
    access::<crate::plugin::PluginStateEvent>(),
    access::<crate::plugin::ScheduleEvent>(),
    access::<crate::process::ProcessExitEvent>(),
    access::<crate::process::ProcessMetricsEvent>(),
    access::<crate::process::ProcessOutputEvent>(),
    access::<crate::process::ProcessStartedEvent>(),
];

/// The permission a plugin needs for this event, or `None` when no plugin may
/// receive it.
pub fn topic_permission(topic: &str, payload: &Value) -> Option<Permission> {
    TOPICS
        .iter()
        .find(|access| access.topic == topic)
        .and_then(|access| (access.permission)(payload))
}
