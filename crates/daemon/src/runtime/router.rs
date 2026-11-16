//! Maps a channel name to a typed handler and routes a decoded request to it. An
//! unknown channel or a handler failure becomes a protocol-level error response,
//! so the caller always gets a well-formed `Response` back.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ipc::protocol::{Request, Response};
use proto::change::{ChangedEvent, Scope};
use proto::error::ErrorInfo;
use proto::Contract;
use serde_json::Value;
use tracing::Instrument;

use super::{HandlerContext, Runtime};

/// A handler's typed failure is the canonical `ErrorInfo` — the same value the
/// socket carries and a front-end localizes.
pub type ServiceResult<T> = Result<T, ErrorInfo>;

/// Serialize an `ErrorInfo` into an error `Response`.
pub fn error_response(info: ErrorInfo) -> Response {
    Response::failure(serde_json::to_value(&info).unwrap_or(Value::Null))
}

/// Channels locked until a Minecraft account is signed in: you cannot use
/// Minecraft you don't own.
fn requires_account(channel: &str) -> bool {
    channel.starts_with("instance.") || channel.starts_with("sync.")
}

/// Refuse the caller unless a Minecraft account is signed in.
pub fn require_account(ctx: &HandlerContext) -> ServiceResult<()> {
    if ctx.runtime.engine().accounts().has_account() {
        return Ok(());
    }
    tracing::warn!("rejected: no signed-in account");
    Err(ErrorInfo::SignInRequired)
}

fn is_polled(channel: &str) -> bool {
    matches!(
        channel,
        "daemon.status" | "health.ping" | "process.list" | "process.status"
    )
}

type BoxFuture = Pin<Box<dyn Future<Output = Response> + Send>>;
type Handler = Arc<dyn Fn(Request, HandlerContext) -> BoxFuture + Send + Sync>;

struct Route {
    owner: Option<&'static str>,
    handler: Handler,
}

#[derive(Default)]
pub struct Router {
    routes: HashMap<String, Route>,
}

impl Router {
    pub fn on(&mut self, channel: &str, owner: Option<&'static str>, handler: Handler) {
        self.routes
            .insert(channel.to_string(), Route { owner, handler });
    }

    pub async fn route(&self, request: Request, ctx: HandlerContext) -> Response {
        let polled = is_polled(&request.channel);
        let span = tracing::info_span!(
            "req",
            channel = %request.channel,
            id = request.id.unwrap_or_default(),
            caller = %ctx.caller
        );
        async move {
            match self.routes.get(&request.channel) {
                Some(Route { owner, handler }) => {
                    if let Some(owner) = owner {
                        if !ctx.runtime.engine().plugins().is_enabled(owner) {
                            tracing::warn!(plugin = owner, "rejected: owner plugin is not enabled");
                            return error_response(ErrorInfo::PluginDisabled {
                                plugin: owner.to_string(),
                            });
                        }
                    }
                    if requires_account(&request.channel) {
                        if let Err(refusal) = require_account(&ctx) {
                            return error_response(refusal);
                        }
                    }
                    let started = std::time::Instant::now();
                    let response = handler(request, ctx).await;
                    let elapsed_ms = started.elapsed().as_millis() as u64;
                    match (&response.error, polled) {
                        (Some(err), _) => {
                            tracing::warn!(error = %err, elapsed_ms, "request failed")
                        }
                        (None, true) => tracing::trace!(elapsed_ms, "request ok"),
                        (None, false) => tracing::debug!(elapsed_ms, "request ok"),
                    }
                    response
                }
                None => {
                    tracing::warn!("no handler for channel");
                    error_response(ErrorInfo::UnknownChannel {
                        channel: request.channel.clone(),
                    })
                }
            }
        }
        .instrument(span)
        .await
    }
}

fn publish_changed(runtime: &Runtime, scope: Scope, ids: Vec<String>) {
    runtime.hub().publish_topic(&ChangedEvent { scope, ids });
}

/// The registrar handed to every service: binds a typed contract handler onto the
/// router. The channel name and payload shapes come from the contract, so a
/// service physically cannot drift from the client SDK.
pub struct Channels<'r> {
    router: &'r mut Router,
}

impl<'r> Channels<'r> {
    pub fn new(router: &'r mut Router) -> Self {
        Channels { router }
    }

    /// Register a handler for contract `C`: decode `C::Params` (a malformed
    /// payload answers `bad_request`), invoke `f`, and encode the returned
    /// `C::Result`. The handler returns an `ErrorInfo` variant for a typed failure.
    pub fn handle<C, F, Fut>(&mut self, f: F)
    where
        C: Contract + 'static,
        F: Fn(C::Params, HandlerContext) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = ServiceResult<C::Result>> + Send + 'static,
    {
        self.router.on(
            C::CHANNEL,
            C::OWNER,
            Arc::new(move |req: Request, ctx: HandlerContext| {
                let f = f.clone();
                Box::pin(async move {
                    let params: C::Params = match serde_json::from_value(req.payload) {
                        Ok(p) => p,
                        Err(e) => {
                            return error_response(ErrorInfo::MalformedRequest {
                                detail: e.to_string(),
                            })
                        }
                    };
                    let runtime = ctx.runtime.clone();
                    let changed = ctx.changed.clone();
                    match f(params, ctx).await {
                        Ok(result) => {
                            let payload = serde_json::to_value(result).unwrap_or(Value::Null);
                            if let Some(scope) = C::CHANGES {
                                publish_changed(&runtime, scope, changed.ids());
                            }
                            Response::success(payload)
                        }
                        Err(info) => error_response(info),
                    }
                })
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use ipc::protocol::Request;
    use proto::change::ChangedEvent;
    use proto::profile::ServerProfile;
    use serde_json::{json, Value};
    use tokio::sync::mpsc::unbounded_channel;

    use super::requires_account;
    use crate::runtime::{Caller, Changed, HandlerContext, Runtime};

    fn changes(runtime: &Runtime) -> Arc<Mutex<Vec<ChangedEvent>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        runtime.hub().listen(
            "test",
            Arc::new(move |topic: &str, payload: &Value| {
                if topic == "changed" {
                    sink.lock()
                        .unwrap()
                        .push(serde_json::from_value(payload.clone()).unwrap());
                }
            }),
        );
        seen
    }

    async fn call(runtime: &Arc<Runtime>, channel: &str, payload: Value) -> Value {
        let (out, _) = unbounded_channel();
        let ctx = HandlerContext {
            runtime: runtime.clone(),
            conn_id: 0,
            out,
            caller: Caller::Client,
            changed: Changed::default(),
        };
        let response = crate::services::make_router()
            .route(Request::new(channel, payload, None), ctx)
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);
        response.payload
    }

    struct Ghost;
    impl proto::Contract for Ghost {
        const CHANNEL: &'static str = "ghost.ping";
        const OWNER: Option<&'static str> = Some("ghost");
        type Params = proto::Empty;
        type Result = proto::Empty;
    }

    #[tokio::test]
    async fn a_channel_whose_owner_is_not_enabled_answers_plugin_disabled_and_never_runs() {
        let home = tempfile::tempdir().unwrap();
        let engine = engine::Engine::new(Some(home.path()));
        let runtime = Arc::new(Runtime::over(engine, home.path().join("d.log")));
        let ran = Arc::new(AtomicBool::new(false));
        let witness = ran.clone();
        let mut router = super::Router::default();
        super::Channels::new(&mut router).handle::<Ghost, _, _>(move |_, _| {
            let witness = witness.clone();
            async move {
                witness.store(true, Ordering::SeqCst);
                Ok(proto::Empty {})
            }
        });
        let (out, _) = unbounded_channel();
        let ctx = HandlerContext {
            runtime,
            conn_id: 0,
            out,
            caller: Caller::Client,
            changed: Changed::default(),
        };

        let response = router
            .route(Request::new("ghost.ping", json!({}), None), ctx)
            .await;

        let error = response.error.expect("refused");
        assert_eq!(error["kind"], "plugin_disabled");
        assert_eq!(error["plugin"], "ghost");
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn an_entry_call_names_the_stable_id_however_the_caller_spelled_it() {
        let home = tempfile::tempdir().unwrap();
        let engine = engine::Engine::new(Some(home.path()));
        let record = engine
            .servers()
            .create(
                "My Server",
                Default::default(),
                ServerProfile::default(),
                None,
            )
            .unwrap();
        engine.servers().mark_ready(&record.id).unwrap();
        let runtime = Arc::new(Runtime::over(engine, home.path().join("d.log")));
        let seen = changes(&runtime);

        let renamed = call(
            &runtime,
            "entry.rename",
            json!({ "target": { "kind": "server", "entry": "My Server" }, "name": "Other" }),
        )
        .await;

        let new_id = renamed["id"].as_str().unwrap();
        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 1);
        let mut expected = vec![record.id.clone(), new_id.to_string()];
        expected.sort();
        expected.dedup();
        assert_eq!(events[0].ids, expected);
    }

    #[tokio::test]
    async fn a_call_that_resolves_nothing_changes_the_whole_scope() {
        let home = tempfile::tempdir().unwrap();
        let engine = engine::Engine::new(Some(home.path()));
        let runtime = Arc::new(Runtime::over(engine, home.path().join("d.log")));
        let seen = changes(&runtime);

        call(
            &runtime,
            "config.set",
            json!({ "key": "instance.multi-session", "value": false }),
        )
        .await;

        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].ids.is_empty());
    }

    #[test]
    fn only_instance_and_sync_channels_are_gated_on_an_account() {
        for channel in [
            "instance.launch",
            "instance.list",
            "instance.modpack.install",
            "sync.set",
            "instance.sync.adopt",
        ] {
            assert!(requires_account(channel), "{channel} should be gated");
        }
        for channel in [
            "account.list",
            "server.start",
            "server.modpack.install",
            "content.add",
            "modpack.status",
            "content.search",
            "java.install",
        ] {
            assert!(!requires_account(channel), "{channel} should be open");
        }
    }
}
