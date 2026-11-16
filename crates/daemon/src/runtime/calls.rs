//! What a built-in's adapter calls through: the router, as the plugin.

use std::sync::{Arc, Weak};

use ipc::protocol::Request;
use kernel::{Chain, HostCalls, Reply};
use proto::error::ErrorInfo;
use proto::job::{JobEvent, JobStarted, JobState};
use serde_json::Value;
use tokio::sync::mpsc::unbounded_channel;

use super::{Caller, Changed, HandlerContext, Router, Runtime};

/// Holds the runtime weakly: the runtime owns the sandbox that owns this.
pub struct RouterCalls {
    router: Arc<Router>,
    runtime: Weak<Runtime>,
}

impl RouterCalls {
    pub fn new(router: Arc<Router>, runtime: &Arc<Runtime>) -> Self {
        RouterCalls {
            router,
            runtime: Arc::downgrade(runtime),
        }
    }

    async fn dispatch(
        &self,
        chain: &Chain,
        channel: &str,
        params: Value,
    ) -> Result<Value, ErrorInfo> {
        let runtime = self.runtime.upgrade().ok_or_else(|| ErrorInfo::Internal {
            detail: "the daemon is stopping".into(),
        })?;
        let (out, _) = unbounded_channel();
        let ctx = HandlerContext {
            runtime,
            conn_id: 0,
            out,
            caller: Caller::Plugin(chain.clone()),
            changed: Changed::default(),
        };
        let response = self
            .router
            .route(Request::new(channel, params, None), ctx)
            .await;
        match response.error {
            None => Ok(response.payload),
            Some(error) => {
                Err(
                    serde_json::from_value(error).unwrap_or_else(|e| ErrorInfo::Internal {
                        detail: format!("unreadable error: {e}"),
                    }),
                )
            }
        }
    }

    /// The listener is in place before the job starts, so none of its events
    /// can be missed. Dropping the call (the guest's budget ran out) cancels the
    /// job, so no work outlives the plugin that wanted it.
    async fn finish(
        &self,
        chain: &Chain,
        channel: &'static str,
        params: Value,
    ) -> Result<Value, ErrorInfo> {
        let runtime = self.runtime.upgrade().ok_or_else(|| ErrorInfo::Internal {
            detail: "the daemon is stopping".into(),
        })?;
        let (events, mut arrived) = unbounded_channel::<Value>();
        let mut running = Running::listen(runtime, channel, events);
        let started = self.dispatch(chain, channel, params).await?;
        let JobStarted { id } =
            serde_json::from_value(started).map_err(|e| ErrorInfo::Internal {
                detail: format!("not a job: {e}"),
            })?;
        running.job = Some(id.clone());
        while let Some(event) = arrived.recv().await {
            if event.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                continue;
            }
            let Ok(event) = serde_json::from_value::<JobEvent<Value, Value>>(event) else {
                continue;
            };
            let settled = match event.state {
                JobState::Progress(_) => continue,
                JobState::Done(done) => Ok(done),
                JobState::Failed { error } => Err(error),
                JobState::Cancelled => Err(ErrorInfo::JobCancelled {
                    channel: channel.to_owned(),
                }),
            };
            running.job = None;
            return settled;
        }
        Err(ErrorInfo::Internal {
            detail: "the job's events stopped".into(),
        })
    }
}

/// The hub listener of one plugin-started job, and the job until it settles.
struct Running {
    runtime: Arc<Runtime>,
    key: String,
    job: Option<String>,
}

impl Running {
    fn listen(
        runtime: Arc<Runtime>,
        channel: &'static str,
        events: tokio::sync::mpsc::UnboundedSender<Value>,
    ) -> Self {
        let key = format!("plugin-job-{}", runtime.hub().next_subscriber_id());
        runtime.hub().listen(
            &key,
            Arc::new(move |topic: &str, payload: &Value| {
                let ours = payload.get("channel").and_then(Value::as_str) == Some(channel);
                if topic == proto::job::TOPIC && ours {
                    let _ = events.send(payload.clone());
                }
            }),
        );
        Running {
            runtime,
            key,
            job: None,
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.runtime.hub().unlisten(&self.key);
        if let Some(job) = &self.job {
            tracing::info!(job, "a plugin stopped waiting; cancelling its job");
            self.runtime.cancellations().cancel(job);
        }
    }
}

impl HostCalls for RouterCalls {
    fn call<'a>(&'a self, chain: &'a Chain, channel: &'static str, params: Value) -> Reply<'a> {
        Box::pin(self.dispatch(chain, channel, params))
    }

    fn run_job<'a>(&'a self, chain: &'a Chain, channel: &'static str, params: Value) -> Reply<'a> {
        Box::pin(self.finish(chain, channel, params))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::runtime::Channels;

    struct Who;
    impl proto::Contract for Who {
        const CHANNEL: &'static str = "test.who";
        type Params = proto::Empty;
        type Result = String;
    }

    #[derive(Serialize, Deserialize)]
    struct Finished {
        text: String,
    }

    struct Work;
    impl proto::Contract for Work {
        const CHANNEL: &'static str = "test.work";
        type Params = proto::Empty;
        type Result = JobStarted;
    }
    impl proto::job::Job for Work {
        type Progress = ();
        type Done = Finished;
    }

    #[tokio::test]
    async fn a_plugin_reaches_handlers_as_itself_and_waits_for_a_job_to_finish() {
        let home = tempfile::tempdir().unwrap();
        let engine = engine::Engine::new(Some(home.path()));
        let runtime = Arc::new(Runtime::over(engine, home.path().join("d.log")));
        let mut router = Router::default();
        let mut on = Channels::new(&mut router);
        on.handle::<Who, _, _>(|_, ctx| async move { Ok(ctx.caller.to_string()) });
        on.handle::<Work, _, _>(|_, ctx| async move {
            let runtime = ctx.runtime.clone();
            tokio::spawn(async move {
                let done: JobEvent<(), Finished> = JobEvent {
                    id: "j1".into(),
                    channel: "test.work".into(),
                    subject: None,
                    state: JobState::Done(Finished {
                        text: "finished".into(),
                    }),
                };
                runtime.hub().publish(&ipc::protocol::Event {
                    topic: proto::job::TOPIC.to_owned(),
                    payload: serde_json::to_value(done).unwrap(),
                });
            });
            Ok(JobStarted { id: "j1".into() })
        });
        let calls = RouterCalls::new(Arc::new(router), &runtime);
        let chain = Chain::default().enter("a").unwrap().enter("b").unwrap();

        let who = calls.call(&chain, "test.who", json!({})).await.unwrap();
        assert_eq!(who, json!("plugin:a>b"));

        let done = calls.run_job(&chain, "test.work", json!({})).await.unwrap();
        assert_eq!(done, json!({ "text": "finished" }));

        let missing = calls.call(&chain, "test.none", json!({})).await;
        assert!(matches!(missing, Err(ErrorInfo::UnknownChannel { .. })));
    }
}
