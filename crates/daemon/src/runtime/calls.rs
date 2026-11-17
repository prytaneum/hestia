//! What a built-in's adapter calls through: the router, as the plugin.

use std::sync::{Arc, Weak};

use ipc::protocol::Request;
use kernel::{Chain, HostCalls, Reply, Settled};
use proto::error::{EntryTarget, ErrorInfo};
use proto::job::{JobEvent, JobStarted, JobState};
use serde_json::Value;
use tokio::sync::mpsc::unbounded_channel;

use super::entries::stable_id;
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

    fn runtime(&self) -> Result<Arc<Runtime>, ErrorInfo> {
        self.runtime.upgrade().ok_or_else(|| ErrorInfo::Internal {
            detail: "the daemon is stopping".into(),
        })
    }

    /// Refuses a call that touches an entry no plugin in `chain` may reach, or
    /// one that resolves to none. The router never learns of entries: only here
    /// does a call carry what it touches.
    fn gate(&self, chain: &Chain, channel: &str, touched: &[EntryTarget]) -> Result<(), ErrorInfo> {
        if touched.is_empty() {
            return Ok(());
        }
        let runtime = self.runtime()?;
        let ids = touched
            .iter()
            .map(|target| stable_id(runtime.engine(), target))
            .collect::<Result<Vec<_>, _>>()?;
        kernel::entries::guard(runtime.plugins(), chain, channel, &ids)
    }

    async fn record(&self, chain: &Chain, created: &[EntryTarget]) {
        let Ok(runtime) = self.runtime() else {
            return;
        };
        let ids: Vec<String> = created
            .iter()
            .filter_map(|target| match stable_id(runtime.engine(), target) {
                Ok(id) => Some(id),
                Err(error) => {
                    tracing::warn!(%error, "a call reported an entry it made that does not exist");
                    None
                }
            })
            .collect();
        kernel::entries::record(runtime.plugins(), chain, &ids).await;
    }

    async fn dispatch(
        &self,
        chain: &Chain,
        channel: &str,
        params: Value,
    ) -> Result<Value, ErrorInfo> {
        let runtime = self.runtime()?;
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
        let runtime = self.runtime()?;
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
    fn call<'a>(
        &'a self,
        chain: &'a Chain,
        channel: &'static str,
        params: Value,
        touched: &'a [EntryTarget],
    ) -> Reply<'a> {
        Box::pin(async move {
            self.gate(chain, channel, touched)?;
            self.dispatch(chain, channel, params).await
        })
    }

    fn run_job<'a>(
        &'a self,
        chain: &'a Chain,
        channel: &'static str,
        params: Value,
        touched: &'a [EntryTarget],
    ) -> Reply<'a> {
        Box::pin(async move {
            self.gate(chain, channel, touched)?;
            self.finish(chain, channel, params).await
        })
    }

    fn created<'a>(&'a self, chain: &'a Chain, created: &'a [EntryTarget]) -> Settled<'a> {
        Box::pin(async move {
            if !created.is_empty() {
                self.record(chain, created).await;
            }
        })
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

        let who = calls
            .call(&chain, "test.who", json!({}), &[])
            .await
            .unwrap();
        assert_eq!(who, json!("plugin:a>b"));

        let done = calls
            .run_job(&chain, "test.work", json!({}), &[])
            .await
            .unwrap();
        assert_eq!(done, json!({ "text": "finished" }));

        let missing = calls.call(&chain, "test.none", json!({}), &[]).await;
        assert!(matches!(missing, Err(ErrorInfo::UnknownChannel { .. })));
    }

    use proto::access::{EntryAccess, JobAccess};
    use proto::error::{EntryKind, EntryTarget};
    use proto::plugin::{Choice, EntryScope};
    use proto::profile::ServerProfile;
    use proto::Contract;

    #[derive(Serialize, Deserialize, Default, Clone, Debug)]
    #[serde(default)]
    struct Entry {
        name: String,
    }

    fn server(name: &str) -> Vec<EntryTarget> {
        vec![EntryTarget::new(EntryKind::Server, name)]
    }

    struct Open;
    impl proto::Contract for Open {
        const CHANNEL: &'static str = "test.open";
        type Params = Entry;
        type Result = Entry;
    }
    impl EntryAccess for Open {
        fn touches(params: &Entry) -> Vec<EntryTarget> {
            server(&params.name)
        }
        fn creates(_: &Entry) -> Vec<EntryTarget> {
            Vec::new()
        }
    }

    struct Make;
    impl proto::Contract for Make {
        const CHANNEL: &'static str = "test.make";
        type Params = Entry;
        type Result = Entry;
    }
    impl EntryAccess for Make {
        fn touches(_: &Entry) -> Vec<EntryTarget> {
            Vec::new()
        }
        fn creates(result: &Entry) -> Vec<EntryTarget> {
            server(&result.name)
        }
    }

    struct Build;
    impl proto::Contract for Build {
        const CHANNEL: &'static str = "test.build";
        type Params = Entry;
        type Result = JobStarted;
    }
    impl proto::job::Job for Build {
        type Progress = ();
        type Done = Entry;
    }
    impl EntryAccess for Build {
        fn touches(_: &Entry) -> Vec<EntryTarget> {
            Vec::new()
        }
        fn creates(_: &JobStarted) -> Vec<EntryTarget> {
            Vec::new()
        }
    }
    impl JobAccess for Build {
        fn creates_done(done: &Entry) -> Vec<EntryTarget> {
            server(&done.name)
        }
    }

    fn make_server(engine: &engine::Engine, name: &str) -> String {
        let record = engine
            .servers()
            .create(name, Default::default(), ServerProfile::default(), None)
            .unwrap();
        engine.servers().mark_ready(&record.id).unwrap();
        record.id
    }

    /// What an adapter's `Link` does around a router call.
    async fn call<C: EntryAccess>(
        calls: &RouterCalls,
        chain: &Chain,
        params: &C::Params,
    ) -> Result<C::Result, ErrorInfo> {
        let reply = calls
            .call(
                chain,
                C::CHANNEL,
                serde_json::to_value(params).unwrap(),
                &C::touches(params),
            )
            .await?;
        let result: C::Result = serde_json::from_value(reply).unwrap();
        calls.created(chain, &C::creates(&result)).await;
        Ok(result)
    }

    async fn install(runtime: &Runtime, id: &str, entries: EntryScope) {
        let dir = runtime.engine().data_home().join(format!("pkg-{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\n"),
        )
        .unwrap();
        std::fs::write(dir.join("plugin.wasm"), b"\0asm\x0d\0\x01\0").unwrap();
        let digest = runtime
            .plugins()
            .inspect(&dir)
            .await
            .unwrap()
            .approval_digest;
        let choice = Choice {
            entries,
            ..Choice::default()
        };
        runtime
            .plugins()
            .install(&dir, &digest, choice)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_plugin_reaches_only_the_entries_every_plugin_in_its_chain_may_and_owns_what_it_creates(
    ) {
        let home = tempfile::tempdir().unwrap();
        let engine = engine::Engine::new(Some(home.path()));
        let (alpha, beta) = (make_server(&engine, "Alpha"), make_server(&engine, "Beta"));
        let runtime = Arc::new(Runtime::over(engine, home.path().join("d.log")));
        runtime.plugins().set_developer(true).await;
        let only_alpha = EntryScope {
            all: false,
            ids: vec![alpha.clone()],
        };
        let everything = EntryScope {
            all: true,
            ids: Vec::new(),
        };
        install(&runtime, "narrow", only_alpha).await;
        install(&runtime, "wide", everything).await;

        let mut router = Router::default();
        let mut on = Channels::new(&mut router);
        on.handle::<Open, _, _>(|p, _| async move { Ok(p) });
        on.handle::<Make, _, _>(|p, ctx| async move {
            make_server(ctx.runtime.engine(), &p.name);
            Ok(p)
        });
        on.handle::<Build, _, _>(|p, ctx| async move {
            let runtime = ctx.runtime.clone();
            tokio::spawn(async move {
                make_server(runtime.engine(), &p.name);
                let done: JobEvent<(), Entry> = JobEvent {
                    id: "j1".into(),
                    channel: "test.build".into(),
                    subject: None,
                    state: JobState::Done(p),
                };
                runtime.hub().publish(&ipc::protocol::Event {
                    topic: proto::job::TOPIC.to_owned(),
                    payload: serde_json::to_value(done).unwrap(),
                });
            });
            Ok(JobStarted { id: "j1".into() })
        });
        let calls = RouterCalls::new(Arc::new(router), &runtime);
        let enter = |plugins: &[&str]| {
            plugins.iter().fold(Chain::default(), |chain, plugin| {
                chain.enter(plugin).unwrap()
            })
        };
        let (narrow, wide) = (enter(&["narrow"]), enter(&["wide"]));
        let open = |name: &str| Entry { name: name.into() };

        for (chain, entry, reaches) in [
            (&narrow, "Alpha", true),
            (&narrow, "alpha", true),
            (&narrow, "Beta", false),
            (&wide, "Beta", true),
            (&enter(&["wide", "narrow"]), "Beta", false),
            (&enter(&["narrow", "wide"]), "Beta", false),
            (&enter(&["wide", "narrow"]), "Alpha", true),
        ] {
            let answer = call::<Open>(&calls, chain, &open(entry)).await;
            match (reaches, answer) {
                (true, Ok(_)) => {}
                (false, Err(ErrorInfo::EntryOutOfScope { plugin, entry })) => {
                    assert_eq!(plugin, "narrow");
                    assert_eq!(entry, beta);
                }
                (_, other) => panic!("{chain:?} {entry}: {other:?}"),
            }
        }
        assert!(matches!(
            call::<Open>(&calls, &wide, &open("Nothing")).await,
            Err(ErrorInfo::EntryNotFound { .. })
        ));

        call::<Make>(&calls, &narrow, &open("Gamma")).await.unwrap();
        call::<Open>(&calls, &narrow, &open("Gamma")).await.unwrap();
        let done = calls
            .run_job(
                &narrow,
                Build::CHANNEL,
                json!({ "name": "Delta" }),
                &Build::touches(&open("Delta")),
            )
            .await
            .unwrap();
        let done: Entry = serde_json::from_value(done).unwrap();
        calls.created(&narrow, &Build::creates_done(&done)).await;
        call::<Open>(&calls, &narrow, &open("Delta")).await.unwrap();
        let ids = ["Alpha", "Gamma", "Delta", "Beta"].map(|name| {
            let target = EntryTarget::new(EntryKind::Server, name);
            stable_id(runtime.engine(), &target).unwrap()
        });
        assert_eq!(
            runtime.plugins().refused_entry("narrow", &ids),
            Some(beta),
            "what it made is its own; nothing else was added"
        );
    }
}
