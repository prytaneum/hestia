use std::sync::{Arc, Weak};

use async_trait::async_trait;
use engine::{CallChain, HostCalls, HostError};
use ipc::protocol::{decode_event, Request};
use proto::error::{code, ErrorInfo};
use proto::job::{JobEvent, JobStarted, JobState};
use proto::plugin::Permission;
use serde_json::Value;
use tokio::sync::mpsc::unbounded_channel;

use crate::runtime::managers::generate_job_id;
use crate::runtime::{Caller, HandlerContext, Router, Runtime};

/// Lends plugins the daemon's channels by routing their calls exactly as a
/// socket client's: same decoding, same account gate, same `changed` events.
/// Whether a plugin may call a channel is the engine's check, made before a
/// call reaches here.
pub struct RouterHost {
    router: Arc<Router>,
    runtime: Weak<Runtime>,
}

impl RouterHost {
    pub fn new(router: Arc<Router>, runtime: &Arc<Runtime>) -> Self {
        RouterHost {
            router,
            runtime: Arc::downgrade(runtime),
        }
    }

    fn runtime(&self) -> Result<Arc<Runtime>, HostError> {
        self.runtime
            .upgrade()
            .ok_or_else(|| HostError::Failed("the daemon is shutting down".into()))
    }

    async fn route(
        &self,
        runtime: Arc<Runtime>,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, HostError> {
        let (out, _) = unbounded_channel();
        let ctx = HandlerContext {
            runtime,
            conn_id: 0,
            out,
            caller: Caller::Plugin(plugin.to_string()),
            chain: chain.clone(),
        };
        let response = self
            .router
            .route(Request::new(channel, payload, None), ctx)
            .await;
        match response.error {
            None => Ok(response.payload),
            Some(error) => Err(match serde_json::from_value::<ErrorInfo>(error) {
                Ok(info) => host_error(info),
                Err(_) => HostError::Failed("the daemon answered with an unreadable error".into()),
            }),
        }
    }
}

#[async_trait]
impl HostCalls for RouterHost {
    fn permission(&self, channel: &str) -> Option<Permission> {
        self.router
            .plugin_access(channel)
            .map(|access| access.permission)
    }

    async fn call(
        &self,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, HostError> {
        let runtime = self.runtime()?;
        self.route(runtime, plugin, chain, channel, payload).await
    }

    async fn run_job(
        &self,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, HostError> {
        if !self
            .router
            .plugin_access(channel)
            .is_some_and(|access| access.job)
        {
            return Err(HostError::Invalid(format!(
                "`{channel}` does not start a job"
            )));
        }
        let Value::Object(mut fields) = payload else {
            return Err(HostError::Invalid(
                "a job payload must be a JSON object".into(),
            ));
        };
        let runtime = self.runtime()?;
        let job_id = generate_job_id(&format!("plugin-{plugin}"));
        fields.insert("id".into(), Value::String(job_id.clone()));

        // Subscribed before the job starts, so a fast job cannot finish unseen.
        let (frames, mut events) = unbounded_channel();
        let mut watch = Watch {
            runtime: runtime.clone(),
            conn_id: runtime.hub().next_subscriber_id(),
            job_id: job_id.clone(),
            finished: false,
        };
        runtime
            .hub()
            .subscribe(watch.conn_id, frames, Some(job_id.clone()));

        let started = match self
            .route(runtime, plugin, chain, channel, Value::Object(fields))
            .await
        {
            Ok(started) => started,
            Err(error) => {
                watch.finished = true;
                return Err(error);
            }
        };
        let started: JobStarted = serde_json::from_value(started)
            .map_err(|error| HostError::Failed(format!("unexpected job reply: {error}")))?;
        if started.id != job_id {
            watch.runtime.cancellations().cancel(&started.id);
            return Err(HostError::Failed(format!(
                "`{channel}` did not take the job id it was given"
            )));
        }

        while let Some(frame) = events.recv().await {
            if let Some(outcome) = job_outcome(&frame, &job_id) {
                watch.finished = true;
                return outcome;
            }
        }
        Err(HostError::Failed("the job's events stopped".into()))
    }
}

/// Follows one job for a plugin's call. Dropping it before the job reports an
/// end (the call timed out, was cancelled or its store was dropped) cancels the
/// job.
struct Watch {
    runtime: Arc<Runtime>,
    conn_id: u64,
    job_id: String,
    finished: bool,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.runtime.hub().unsubscribe(self.conn_id);
        if !self.finished {
            tracing::info!(job = %self.job_id, "plugin call ended first, cancelling its job");
            self.runtime.cancellations().cancel(&self.job_id);
        }
    }
}

/// The job's end as the plugin sees it; `None` for a frame that is progress or
/// belongs to something else.
fn job_outcome(frame: &str, job_id: &str) -> Option<Result<Value, HostError>> {
    let frame: Value = serde_json::from_str(frame).ok()?;
    let event = decode_event(&frame);
    if event.topic != proto::job::TOPIC {
        return None;
    }
    let event: JobEvent<Value, Value> = serde_json::from_value(event.payload).ok()?;
    if event.id != job_id {
        return None;
    }
    match event.state {
        JobState::Progress(_) => None,
        JobState::Done(done) => Some(Ok(done)),
        JobState::Failed { error } => Some(Err(host_error(error))),
        JobState::Cancelled => Some(Err(HostError::Failed("the job was cancelled".into()))),
    }
}

fn host_error(info: ErrorInfo) -> HostError {
    let message = info.to_string();
    match info.code() {
        code::NOT_FOUND => HostError::NotFound(message),
        code::BAD_REQUEST => HostError::Invalid(message),
        _ => HostError::Failed(message),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use ipc::protocol::Event;
    use proto::job::{Job, JobStarted};
    use proto::Contract;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::runtime::Channels;

    #[derive(Serialize, Deserialize, Default)]
    #[serde(default)]
    struct FixtureParams {
        id: String,
        hang: bool,
    }

    #[derive(Serialize, Deserialize)]
    struct FixtureDone {
        value: u32,
    }

    struct Fixture;
    impl Contract for Fixture {
        const CHANNEL: &'static str = "fixture.job";
        const PLUGIN: Option<Permission> = Some(Permission::ContentRead);
        type Params = FixtureParams;
        type Result = JobStarted;
    }
    impl Job for Fixture {
        type Progress = ();
        type Done = FixtureDone;
    }

    struct Harness {
        host: RouterHost,
        /// The host holds the runtime weakly, so the test keeps it alive.
        _runtime: Arc<Runtime>,
        cancelled: Arc<AtomicBool>,
        seen: Arc<Mutex<Option<(String, bool)>>>,
        _home: tempfile::TempDir,
    }

    fn harness(router: Router) -> Harness {
        let home = tempfile::tempdir().expect("temp home");
        let engine = engine::Engine::new(Some(home.path()));
        let runtime = Arc::new(Runtime::over(engine, home.path().join("hestiad.log")));
        let host = RouterHost::new(Arc::new(router), &runtime);
        Harness {
            host,
            _runtime: runtime,
            cancelled: Arc::default(),
            seen: Arc::default(),
            _home: home,
        }
    }

    fn fixture_router(
        cancelled: Arc<AtomicBool>,
        seen: Arc<Mutex<Option<(String, bool)>>>,
    ) -> Router {
        let mut router = Router::default();
        Channels::new(&mut router).handle::<Fixture, _, _>(move |p, ctx| {
            let cancelled = cancelled.clone();
            let seen = seen.clone();
            async move {
                *seen.lock().unwrap() =
                    Some((ctx.caller.to_string(), ctx.chain.enter("demo").is_err()));
                let (cancel, registered) = ctx.runtime.cancellations().register(&p.id);
                let id = p.id.clone();
                tokio::spawn(async move {
                    let _registered = registered;
                    let state = if p.hang {
                        while !cancel.is_cancelled() {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                        cancelled.store(true, Ordering::SeqCst);
                        JobState::<(), FixtureDone>::Cancelled
                    } else {
                        JobState::Done(FixtureDone { value: 7 })
                    };
                    let event = JobEvent {
                        id: p.id,
                        channel: Fixture::CHANNEL.to_string(),
                        subject: None,
                        state,
                    };
                    ctx.runtime.hub().publish(&Event {
                        topic: proto::job::TOPIC.to_string(),
                        payload: serde_json::to_value(&event).unwrap(),
                    });
                });
                Ok(JobStarted { id })
            }
        });
        router
    }

    fn fixture() -> Harness {
        let cancelled = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(None));
        let mut harness = harness(fixture_router(cancelled.clone(), seen.clone()));
        harness.cancelled = cancelled;
        harness.seen = seen;
        harness
    }

    #[tokio::test]
    async fn run_job_returns_the_done_payload_of_a_routed_job() {
        let harness = fixture();
        let chain = CallChain::default().enter("demo").unwrap();
        let done = harness
            .host
            .run_job("demo", &chain, "fixture.job", json!({}))
            .await
            .expect("job done");
        assert_eq!(done, json!({ "value": 7 }));
        let (caller, chain_carried) = harness.seen.lock().unwrap().clone().unwrap();
        assert_eq!(caller, "plugin:demo");
        assert!(
            chain_carried,
            "the handler should see the plugin on the chain"
        );
        assert!(!harness.cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_call_that_ends_first_cancels_its_job() {
        let harness = fixture();
        let chain = CallChain::default();
        let call = harness
            .host
            .run_job("demo", &chain, "fixture.job", json!({ "hang": true }));
        assert!(tokio::time::timeout(Duration::from_millis(100), call)
            .await
            .is_err());
        for _ in 0..200 {
            if harness.cancelled.load(Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the job was not cancelled after its plugin call ended");
    }

    #[tokio::test]
    async fn run_job_refuses_a_channel_that_starts_no_job() {
        let harness = harness(crate::services::make_router());
        let error = harness
            .host
            .run_job("demo", &CallChain::default(), "instance.list", json!({}))
            .await
            .expect_err("instance.list is not a job");
        assert!(matches!(error, HostError::Invalid(_)));
    }

    #[tokio::test]
    async fn a_plugin_call_meets_the_account_gate_like_a_client() {
        let harness = harness(crate::services::make_router());
        let error = harness
            .host
            .call("demo", &CallChain::default(), "instance.list", json!({}))
            .await
            .expect_err("no account is signed in");
        assert!(matches!(&error, HostError::Failed(message) if message.contains("sign in")));
    }
}
