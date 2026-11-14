//! Hub events on their way to a plugin's `on-event`. What a plugin may receive is
//! decided here, once per event, before anything is queued: the manifest's globs
//! pick the topics, and the plugin's permissions pick which of those it may see.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use engine::{CallChain, CallError, HostCalls, LoadedPlugin, Pushed, EVENT_TIMEOUT};
use proto::events::topic_permission;
use proto::glob::topic_matches;
use proto::plugin::{Grant, PluginStateEvent, ScheduleEvent, ScopeKind};
use proto::pushed::{is_private, IpcClosedEvent, IpcReadableEvent};
use proto::Topic;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, MissedTickBehavior};

use super::budget::{Budget, Verdict, Wake};
use crate::runtime::EventHub;

const QUEUE_CAPACITY: usize = 256;
const WARN_EVERY: Duration = Duration::from_secs(60);
const TIMEOUTS_IN_A_ROW: u32 = 3;

/// The payload `plugin` may receive for this event, or `None` to withhold it.
/// Every topic is classified in `proto::events::TOPICS`; a topic none classifies,
/// such as process output, never reaches a plugin. A `job` event goes only to a
/// plugin granted the permission of the channel that started the job;
/// `plugin.state` needs none but is cut to its id and state. A topic pushed to
/// one plugin alone is never admitted from the hub, whoever claims to have
/// published it.
pub fn admit(grant: &Grant, host: &dyn HostCalls, topic: &str, payload: &Value) -> Option<Value> {
    if is_private(topic) || !subscribes(grant, topic) {
        return None;
    }
    if topic == PluginStateEvent::TOPIC {
        return Some(json!({ "id": payload.get("id"), "state": payload.get("state") }));
    }
    let needed = if topic == proto::job::TOPIC {
        host.permission(payload.get("channel")?.as_str()?)
    } else {
        topic_permission(topic, payload)
    };
    needed
        .is_some_and(|permission| grant.holds(permission))
        .then(|| payload.clone())
}

fn subscribes(grant: &Grant, topic: &str) -> bool {
    grant
        .contributed(ScopeKind::Events)
        .any(|pattern| topic_matches(pattern, topic))
}

/// Whether the plugin may be handed an event the host pushed to it: it must
/// subscribe to the topic and hold the permission the topic names.
fn admit_pushed(grant: &Grant, topic: &str) -> bool {
    subscribes(grant, topic)
        && topic_permission(topic, &Value::Null).is_some_and(|permission| grant.holds(permission))
}

/// A plugin's pending events: bounded, and when full the oldest is dropped, so a
/// slow plugin falls behind on history instead of growing the daemon.
struct Inbox {
    plugin: String,
    queue: Mutex<VecDeque<(String, Value)>>,
    ready: Notify,
    dropped: AtomicU64,
    last_warned: Mutex<Option<Instant>>,
    budget: Mutex<Budget>,
}

impl Inbox {
    fn new(plugin: &str) -> Self {
        Inbox {
            plugin: plugin.to_string(),
            queue: Mutex::default(),
            ready: Notify::new(),
            dropped: AtomicU64::new(0),
            last_warned: Mutex::new(None),
            budget: Mutex::new(Budget::new(Instant::now())),
        }
    }

    fn judge(&self, wake: Wake) -> Verdict {
        self.budget.lock().unwrap().judge(wake, Instant::now())
    }

    /// Records a wake-up the budget held back; the log hears of it at most once
    /// a minute.
    fn throttled(&self, plugin: &LoadedPlugin, wake: Wake) {
        plugin.record_throttled();
        let due = self.budget.lock().unwrap().note_throttled(Instant::now());
        if due {
            tracing::warn!(
                plugin = %self.plugin,
                ?wake,
                "plugin is being woken faster than its call budget allows"
            );
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

    /// Skipped while a tick of `id` still waits, so a plugin slower than its
    /// schedule holds at most one pending tick per schedule.
    fn push_tick(&self, id: &str) {
        let payload = json!({ "id": id });
        let waiting = self
            .queue
            .lock()
            .unwrap()
            .iter()
            .any(|(topic, queued)| topic == ScheduleEvent::TOPIC && *queued == payload);
        if waiting {
            tracing::debug!(plugin = %self.plugin, schedule = id, "schedule tick skipped: the last is still waiting");
        } else {
            self.push(ScheduleEvent::TOPIC, payload);
        }
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

/// A plugin's subscription to the hub, its schedules, and the tasks serving
/// them. Dropping it ends them all, which is how disabling, removing or
/// reloading a plugin stops delivery.
pub struct Delivery {
    key: String,
    hub: Arc<EventHub>,
    tasks: Vec<JoinHandle<()>>,
}

impl Delivery {
    /// `None` when the plugin subscribes to nothing and schedules nothing.
    pub fn start(
        hub: Arc<EventHub>,
        host: Arc<dyn HostCalls>,
        loaded: Arc<LoadedPlugin>,
    ) -> Option<Delivery> {
        let subscribed = loaded.grant();
        if subscribed.contributed(ScopeKind::Events).next().is_none()
            && subscribed
                .contributed(ScopeKind::Schedules)
                .next()
                .is_none()
        {
            return None;
        }
        let key = format!("plugin:{}:{}", loaded.id(), hub.next_subscriber_id());
        let inbox = Arc::new(Inbox::new(loaded.id()));
        let grant = Arc::new(loaded.grant().clone());
        let queue = inbox.clone();
        hub.listen(
            &key,
            Arc::new({
                let grant = grant.clone();
                move |topic, payload| {
                    if let Some(payload) = admit(&grant, host.as_ref(), topic, payload) {
                        queue.push(topic, payload);
                    }
                }
            }),
        );
        let mut tasks: Vec<_> = Vec::new();
        if admit_pushed(&grant, IpcReadableEvent::TOPIC)
            || admit_pushed(&grant, IpcClosedEvent::TOPIC)
        {
            tasks.push(tokio::spawn(forward(
                inbox.clone(),
                grant.clone(),
                loaded.pushed(),
            )));
        }
        tasks.extend(grant.schedules().map(|schedule| {
            tokio::spawn(tick(
                inbox.clone(),
                schedule.id.clone(),
                Duration::from_secs(schedule.every),
            ))
        }));
        let ticks: Vec<_> = tasks.iter().map(JoinHandle::abort_handle).collect();
        let hub_for_drain = hub.clone();
        let key_for_drain = key.clone();
        tasks.push(tokio::spawn(async move {
            drain(inbox, loaded, hub_for_drain, key_for_drain).await;
            ticks.iter().for_each(|tick| tick.abort());
        }));
        Some(Delivery { key, hub, tasks })
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        self.hub.unlisten(&self.key);
        self.tasks.iter().for_each(JoinHandle::abort);
    }
}

/// Moves what the plugin's own connections report into its inbox, once it has
/// subscribed to the topic and holds the permission.
async fn forward(inbox: Arc<Inbox>, grant: Arc<Grant>, mut pushed: UnboundedReceiver<Pushed>) {
    while let Some(event) = pushed.recv().await {
        if admit_pushed(&grant, event.topic) {
            inbox.push(event.topic, event.payload);
        }
    }
}

/// The first tick is one full period away; a tick missed while the machine slept
/// is skipped, never replayed.
async fn tick(inbox: Arc<Inbox>, id: String, every: Duration) {
    let mut timer = interval_at(tokio::time::Instant::now() + every, every);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        inbox.push_tick(&id);
    }
}

/// The one place a host-initiated call is paid for. A tick over budget is
/// dropped; an event waits for a token while the queue behind it keeps dropping
/// its oldest.
async fn within_budget(inbox: &Inbox, plugin: &LoadedPlugin, wake: Wake) -> bool {
    let mut held = false;
    loop {
        match inbox.judge(wake) {
            Verdict::Run => return true,
            Verdict::Skip => {
                inbox.throttled(plugin, wake);
                return false;
            }
            Verdict::Wait(pause) => {
                if !std::mem::replace(&mut held, true) {
                    inbox.throttled(plugin, wake);
                }
                tokio::time::sleep(pause).await;
            }
        }
    }
}

/// Serial, in order, each event on a fresh store. Stops for good, until the
/// plugin is loaded again, after [`TIMEOUTS_IN_A_ROW`] timeouts.
async fn drain(inbox: Arc<Inbox>, plugin: Arc<LoadedPlugin>, hub: Arc<EventHub>, key: String) {
    let chain = CallChain::default();
    let mut timeouts = 0;
    loop {
        let (topic, payload) = inbox.pop().await;
        let wake = match topic.as_str() {
            ScheduleEvent::TOPIC => Wake::Tick,
            _ => Wake::Event,
        };
        if !within_budget(&inbox, &plugin, wake).await {
            continue;
        }
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
    use proto::error::ErrorInfo;
    use proto::plugin::{Permission, PermissionScope, Touched};

    use super::*;

    struct Channels;

    #[async_trait]
    impl HostCalls for Channels {
        fn permission(&self, channel: &str) -> Option<Permission> {
            (channel == "content.search").then_some(Permission::ContentRead)
        }

        fn touched(&self, _channel: &str, _payload: &Value) -> Touched {
            Touched::Unscoped
        }

        async fn call(
            &self,
            _: &str,
            _: &CallChain,
            _: &str,
            _: Value,
        ) -> Result<Value, ErrorInfo> {
            unreachable!()
        }

        async fn run_job(
            &self,
            _: &str,
            _: &CallChain,
            _: &str,
            _: Value,
        ) -> Result<Value, ErrorInfo> {
            unreachable!()
        }
    }

    fn grant(events: &[&str], permissions: &[Permission]) -> Grant {
        Grant {
            contributions: PermissionScope::of(ScopeKind::Events, events.iter().copied()),
            permissions: permissions
                .iter()
                .map(|permission| (*permission, PermissionScope::default()))
                .collect(),
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
        let everything = grant(&["net.*", "process.*"], &[Permission::ProcessRead]);
        assert!(admitted(&everything, "process.unknown", json!({})).is_none());
        assert!(admitted(&everything, "net.state", json!({})).is_none());
        let online = grant(&["net.*"], &[Permission::NetworkConnect]);
        assert!(admitted(&online, "net.state", json!({})).is_some());
    }

    #[test]
    fn a_change_is_admitted_by_the_permission_of_its_scope() {
        let changed = |scope: &str| json!({ "scope": scope });
        let entries = grant(&["changed"], &[Permission::EntryRead]);
        assert!(admitted(&entries, "changed", changed("entries")).is_some());
        assert!(admitted(&entries, "changed", changed("accounts")).is_none());
        assert!(admitted(&entries, "changed", changed("plugins")).is_none());
        assert!(admitted(&entries, "changed", json!({})).is_none());
    }

    #[test]
    fn sign_in_and_server_ready_need_their_read_permissions() {
        let who = json!({ "uuid": "u", "name": "Steve" });
        let accounts = grant(&["account.*"], &[Permission::AccountRead]);
        assert!(admitted(&accounts, "account.signed_in", who.clone()).is_some());
        assert!(admitted(&accounts, "account.signed_out", who.clone()).is_some());
        assert!(admitted(&grant(&["account.*"], &[]), "account.signed_in", who).is_none());

        let ready = json!({ "id": "smp" });
        let entries = grant(&["server.ready"], &[Permission::EntryRead]);
        assert!(admitted(&entries, "server.ready", ready.clone()).is_some());
        assert!(admitted(&grant(&["server.ready"], &[]), "server.ready", ready).is_none());
    }

    #[test]
    fn a_pushed_topic_is_never_admitted_from_the_hub() {
        let owner = grant(&["ipc.*"], &[Permission::IpcConnect]);
        let readable = json!({ "connection": "1" });
        assert!(admitted(&owner, "ipc.readable", readable.clone()).is_none());
        assert!(admitted(&owner, "ipc.closed", readable).is_none());
    }

    #[test]
    fn a_pushed_event_needs_the_subscription_and_the_permission() {
        let both = grant(&["ipc.readable"], &[Permission::IpcConnect]);
        assert!(admit_pushed(&both, "ipc.readable"));
        assert!(!admit_pushed(&both, "ipc.closed"));
        assert!(admit_pushed(
            &grant(&["ipc.*"], &[Permission::IpcConnect]),
            "ipc.closed"
        ));
        assert!(!admit_pushed(&grant(&["ipc.*"], &[]), "ipc.readable"));
        assert!(!admit_pushed(
            &grant(&["server.*"], &[Permission::IpcConnect]),
            "ipc.readable"
        ));
    }

    #[tokio::test]
    async fn only_admitted_pushed_events_reach_the_inbox() {
        let inbox = Arc::new(Inbox::new("demo"));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let event = |topic| Pushed {
            topic,
            payload: json!({ "connection": "1" }),
        };
        sender.send(event("ipc.readable")).unwrap();
        sender.send(event("ipc.closed")).unwrap();
        drop(sender);

        let listening = Arc::new(grant(&["ipc.closed"], &[Permission::IpcConnect]));
        forward(inbox.clone(), listening, receiver).await;

        let queue = inbox.queue.lock().unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].0, "ipc.closed");
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
    fn a_waiting_tick_is_not_queued_again() {
        let inbox = Inbox::new("demo");
        inbox.push_tick("poll");
        inbox.push_tick("poll");
        inbox.push_tick("sync");
        assert_eq!(inbox.queue.lock().unwrap().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_schedule_ticks_each_period_and_skips_what_it_missed() {
        let inbox = Arc::new(Inbox::new("demo"));
        let every = Duration::from_secs(30);
        let task = tokio::spawn(tick(inbox.clone(), "poll".into(), every));
        tokio::time::sleep(Duration::from_secs(29)).await;
        assert!(inbox.queue.lock().unwrap().is_empty());
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(inbox.pop().await.0, "plugin.schedule");
        tokio::time::sleep(every * 10).await;
        assert!(inbox.queue.lock().unwrap().len() <= 1);
        task.abort();
    }

    #[test]
    fn a_schedule_tick_is_never_admitted_from_the_hub() {
        let payload = json!({ "id": "poll" });
        let listening = grant(&["plugin.*", "plugin.schedule"], &[]);
        assert!(admitted(&listening, "plugin.schedule", payload).is_none());
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
}
