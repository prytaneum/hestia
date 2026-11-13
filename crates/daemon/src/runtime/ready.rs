//! Tells subscribers when a started server accepts players. The supervisor knows
//! a process is running, not that the game finished loading, so each start gets a
//! watcher that pings the game port until the server answers.

use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;

use proto::process::ProcessStartedEvent;
use proto::server::ServerReadyEvent;
use proto::Topic;
use serde_json::Value;
use tokio::time::Instant;

use super::Runtime;

const PROBE_EVERY: Duration = Duration::from_secs(2);
const GIVE_UP_AFTER: Duration = Duration::from_secs(10 * 60);
const LISTENER: &str = "server-ready";
const SERVER_PREFIX: &str = "server-";

/// Starts a watcher for every server process the supervisor reports started.
pub fn spawn_ready_watcher(runtime: &Arc<Runtime>) {
    let weak = Arc::downgrade(runtime);
    runtime.hub().listen(
        LISTENER,
        Arc::new(move |topic: &str, payload: &Value| {
            if topic != ProcessStartedEvent::TOPIC {
                return;
            }
            let Some(process) = payload.get("id").and_then(Value::as_str) else {
                return;
            };
            let Some(server) = process.strip_prefix(SERVER_PREFIX) else {
                return;
            };
            let Ok(handle) = tokio::runtime::Handle::try_current() else {
                return;
            };
            handle.spawn(watch(weak.clone(), process.to_string(), server.to_string()));
        }),
    );
}

async fn watch(runtime: Weak<Runtime>, process: String, server: String) {
    let probe = || {
        let runtime = runtime.clone();
        let server = server.clone();
        async move {
            match runtime.upgrade() {
                Some(runtime) => runtime.engine().server_ping(&server).await.is_ok(),
                None => false,
            }
        }
    };
    let alive = || {
        runtime
            .upgrade()
            .is_some_and(|runtime| runtime.process_running(&process))
    };
    if until_ready(probe, alive, PROBE_EVERY, GIVE_UP_AFTER).await {
        if let Some(runtime) = runtime.upgrade() {
            tracing::info!(%server, "server accepts players");
            runtime
                .hub()
                .publish_topic(&ServerReadyEvent { id: server });
        }
    } else {
        tracing::debug!(%server, "server did not become ready");
    }
}

/// Probes every `every` while `alive`, and gives up at `limit`.
async fn until_ready<P, Fut>(
    mut probe: P,
    alive: impl Fn() -> bool,
    every: Duration,
    limit: Duration,
) -> bool
where
    P: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + limit;
    while alive() && Instant::now() < deadline {
        if probe().await {
            return true;
        }
        tokio::time::sleep(every).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use super::*;

    const EVERY: Duration = Duration::from_secs(2);

    #[tokio::test(start_paused = true)]
    async fn ready_once_the_probe_answers() {
        let tries = AtomicU32::new(0);
        let ready = until_ready(
            || {
                let attempt = tries.fetch_add(1, Ordering::Relaxed);
                async move { attempt == 3 }
            },
            || true,
            EVERY,
            Duration::from_secs(60),
        )
        .await;
        assert!(ready);
        assert_eq!(tries.load(Ordering::Relaxed), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_at_the_limit() {
        let ready = until_ready(|| async { false }, || true, EVERY, Duration::from_secs(10)).await;
        assert!(!ready);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_when_the_process_is_gone() {
        let running = AtomicBool::new(true);
        let tries = AtomicU32::new(0);
        let ready = until_ready(
            || {
                tries.fetch_add(1, Ordering::Relaxed);
                running.store(false, Ordering::Relaxed);
                async { false }
            },
            || running.load(Ordering::Relaxed),
            EVERY,
            Duration::from_secs(60),
        )
        .await;
        assert!(!ready);
        assert_eq!(tries.load(Ordering::Relaxed), 1);
    }
}
