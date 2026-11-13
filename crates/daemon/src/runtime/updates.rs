//! When the daemon checks installed content for updates. There is no timer: a
//! check runs when its answer matters — at startup, when the network comes
//! back, as an instance launches, and when a front-end opens an entry — and
//! the engine's TTL keeps one that comes soon after another from asking
//! upstream at all.

use std::sync::Arc;

use proto::content::{ContentUpdatesFoundEvent, EntryContentUpdate};
use proto::error::EntryKind;
use proto::net::NetworkState;

use super::{server_process_id, ContentJob, JobEntry, JobKind, Runtime};

pub fn spawn_update_watcher(runtime: Arc<Runtime>) {
    tokio::spawn(async move {
        let mut states = runtime.engine().network().subscribe();
        let mut was_online = states.borrow_and_update().state == NetworkState::Online;
        runtime.check_updates().await;
        loop {
            if states.changed().await.is_err() {
                return;
            }
            let online = states.borrow_and_update().state == NetworkState::Online;
            if online && !was_online {
                runtime.check_updates().await;
            }
            was_online = online;
        }
    });
}

impl Runtime {
    /// Check every entry, tell front-ends what is new, and apply what
    /// auto-update may to the servers that are stopped.
    pub async fn check_updates(&self) {
        if self.engine().network().status().state.is_offline() {
            return;
        }
        let swept = self.engine().check_all_updates().await;
        self.announce_updates(swept.fresh);
        for record in self.engine().servers().list() {
            self.auto_update_server(JobEntry::from(record));
        }
    }

    /// Run [`Runtime::check_updates`] off the caller's task.
    pub fn spawn_update_check(self: &Arc<Self>) {
        let runtime = self.clone();
        tokio::spawn(async move { runtime.check_updates().await });
    }

    pub fn announce_updates(&self, fresh: u32) {
        if fresh > 0 {
            self.hub()
                .publish_topic(&ContentUpdatesFoundEvent { fresh });
        }
    }

    /// Start an auto-update for a server that is stopped and idle and has
    /// updates auto-update may apply. A running or busy one is caught by a
    /// later check; an instance updates as it launches instead.
    pub fn auto_update_server(&self, entry: JobEntry) {
        if entry.kind != EntryKind::Server
            || !self.engine().config().settings().content.auto_update
            || self.engine().auto_updatable(entry.entry_ref()) == 0
        {
            return;
        }
        let process_id = server_process_id(&entry.id);
        let Ok(claim) = self.claims().claim(process_id.clone(), JobKind::Content) else {
            return;
        };
        if self.process_running(&process_id) {
            return;
        }
        tracing::info!(server = %entry.id, "auto-updating a stopped server's content");
        self.jobs().start_content::<EntryContentUpdate>(
            ContentJob::AutoUpdate { entry },
            String::new(),
            claim,
        );
    }
}
