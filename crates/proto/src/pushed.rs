//! Events the host pushes to one plugin alone. They are never published to the
//! daemon's event hub, so no client and no other plugin receives them; a plugin
//! still subscribes in its manifest like any topic, and needs the permission
//! that gave it the thing the event is about.

use serde::{Deserialize, Serialize};

use crate::contract::Topic;
use crate::plugin::Permission;

/// A connection the plugin opened with `ipc.open` has bytes to read.
///
/// Edge-triggered: sent when data arrives while no call is waiting in
/// `receive`, then not again until the plugin has called `receive`, so a chatty
/// peer cannot flood the plugin's queue. A `receive` that leaves bytes behind
/// sends it again.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct IpcReadableEvent {
    /// The id `ipc.open` returned.
    pub connection: String,
}
impl Topic for IpcReadableEvent {
    const TOPIC: &'static str = "ipc.readable";
    const PLUGIN: Option<Permission> = Some(Permission::IpcConnect);
}

/// The peer of a connection the plugin opened closed it, or the connection
/// failed. Sent once per connection; bytes that arrived before the close can
/// still be read, and the `receive` after them answers `not-found`. Not sent
/// for a connection the plugin closed itself.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct IpcClosedEvent {
    /// The id `ipc.open` returned.
    pub connection: String,
}
impl Topic for IpcClosedEvent {
    const TOPIC: &'static str = "ipc.closed";
    const PLUGIN: Option<Permission> = Some(Permission::IpcConnect);
}

/// The topics only the owning plugin hears, which the hub must never carry to a
/// plugin: `plugin.schedule` is the third, defined with plugins.
pub const PRIVATE_TOPICS: &[&str] = &[
    IpcReadableEvent::TOPIC,
    IpcClosedEvent::TOPIC,
    crate::plugin::ScheduleEvent::TOPIC,
];

/// Whether `topic` is pushed to one plugin and never published to the hub.
pub fn is_private(topic: &str) -> bool {
    PRIVATE_TOPICS.contains(&topic)
}
