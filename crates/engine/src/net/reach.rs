//! Whether the daemon can reach upstream, derived from the requests it actually
//! makes — a separate connectivity check could disagree with them, and the one
//! that matters is the one a launch is about to depend on.
//!
//! Process-global because the connection pool is: reachability is a property of
//! that pool's traffic, and the engine's fetch sites are free functions and
//! provider impls that hold no aggregate reference. `Engine::network()` is the
//! handle the daemon reaches it through.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use proto::net::{NetworkState, NetworkStatus};
use tokio::sync::watch;

/// Mojang's version manifest rather than a third-party connectivity endpoint: it
/// is a service the launcher already depends on, so probing it neither tells
/// anyone new that hestia is running nor reports online when the one host every
/// launch needs is down.
const PROBE_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// While offline, often — recovery should be noticed without the user acting.
const RETRY_EVERY: i64 = 10;
/// While online, only once traffic has gone quiet for this long.
const FRESH_FOR: i64 = 60;

pub fn network() -> &'static Network {
    static NETWORK: OnceLock<Network> = OnceLock::new();
    NETWORK.get_or_init(Network::new)
}

pub struct Network {
    state: Mutex<State>,
    tx: watch::Sender<NetworkStatus>,
}

struct State {
    status: NetworkStatus,
    last_probe_unix: i64,
}

impl Network {
    fn new() -> Self {
        let status = NetworkStatus::default();
        let (tx, _) = watch::channel(status.clone());
        Network {
            state: Mutex::new(State {
                status,
                last_probe_unix: 0,
            }),
            tx,
        }
    }

    pub fn status(&self) -> NetworkStatus {
        self.state.lock().unwrap().status.clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<NetworkStatus> {
        self.tx.subscribe()
    }

    /// Whether the user has pinned the daemon offline, so nothing should be
    /// attempted at all.
    pub fn pinned(&self) -> bool {
        self.state.lock().unwrap().status.offline_mode
    }

    pub fn set_offline_mode(&self, offline: bool) {
        self.publish_if_changed(|state| {
            state.status.offline_mode = offline;
            state.status.state = if offline {
                NetworkState::Offline
            } else {
                NetworkState::Unknown
            };
        });
    }

    pub fn observe_reachable(&self) {
        self.publish_if_changed(|state| {
            state.status.last_online_unix = common::now_unix();
            if !state.status.offline_mode {
                state.status.state = NetworkState::Online;
            }
        });
    }

    pub fn observe_unreachable(&self) {
        self.publish_if_changed(|state| {
            if !state.status.offline_mode {
                state.status.state = NetworkState::Offline;
            }
        });
    }

    /// Confirm the current state if it has gone stale. Called on a short tick by
    /// the daemon's watcher; the pacing lives here so the caller is just a clock.
    pub async fn refresh(&self) {
        let (status, last_probe) = {
            let state = self.state.lock().unwrap();
            (state.status.clone(), state.last_probe_unix)
        };
        if status.offline_mode {
            return;
        }
        let since_probe = common::now_unix().saturating_sub(last_probe);
        let due = match status.state {
            NetworkState::Unknown => true,
            NetworkState::Offline => since_probe >= RETRY_EVERY,
            NetworkState::Online => {
                common::now_unix().saturating_sub(status.last_online_unix) >= FRESH_FOR
                    && since_probe >= FRESH_FOR
            }
        };
        if due {
            self.probe().await;
        }
    }

    async fn probe(&self) {
        self.state.lock().unwrap().last_probe_unix = common::now_unix();
        let result = super::client()
            .head(PROBE_URL)
            .timeout(PROBE_TIMEOUT)
            .send()
            .await;
        match result {
            Err(error) if super::retry::is_transport(&error) => self.observe_unreachable(),
            _ => self.observe_reachable(),
        }
    }

    fn publish_if_changed(&self, change: impl FnOnce(&mut State)) {
        // `last_online_unix` moves on every successful request; only a state
        // change is worth an event, or a busy download would publish per chunk.
        let updated = {
            let mut state = self.state.lock().unwrap();
            let before = (state.status.state, state.status.offline_mode);
            change(&mut state);
            if (state.status.state, state.status.offline_mode) == before {
                return;
            }
            state.status.since_unix = common::now_unix();
            state.status.clone()
        };
        tracing::info!(
            state = ?updated.state,
            pinned = updated.offline_mode,
            "network reachability changed"
        );
        let _ = self.tx.send(updated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network() -> Network {
        Network::new()
    }

    #[test]
    fn nothing_attempted_yet_is_unknown_not_offline() {
        assert_eq!(network().status().state, NetworkState::Unknown);
    }

    #[test]
    fn a_failed_request_marks_offline_and_a_later_success_clears_it() {
        let net = network();
        net.observe_unreachable();
        assert_eq!(net.status().state, NetworkState::Offline);
        net.observe_reachable();
        assert_eq!(net.status().state, NetworkState::Online);
        assert!(net.status().last_online_unix > 0);
    }

    #[test]
    fn pinning_offline_survives_an_observation() {
        let net = network();
        net.set_offline_mode(true);
        assert!(net.pinned());
        // A request in flight when the pin landed must not un-pin it.
        net.observe_reachable();
        assert_eq!(net.status().state, NetworkState::Offline);
        assert!(net.status().offline_mode);
    }

    #[test]
    fn releasing_the_pin_returns_to_unknown_until_something_confirms() {
        let net = network();
        net.set_offline_mode(true);
        net.set_offline_mode(false);
        assert_eq!(net.status().state, NetworkState::Unknown);
    }

    #[tokio::test]
    async fn only_a_transition_is_published() {
        let net = network();
        let mut states = net.subscribe();
        net.observe_unreachable();
        assert_eq!(
            states.borrow_and_update().state,
            NetworkState::Offline,
            "the first observation is a change"
        );
        // A second identical observation would otherwise wake every front-end
        // once per request on a busy download.
        net.observe_unreachable();
        assert!(states.has_changed().is_ok_and(|changed| !changed));
    }
}
