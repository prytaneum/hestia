//! Maps a channel name to a typed handler and routes a decoded request to it. An
//! unknown channel or a handler failure becomes a protocol-level error response,
//! so the caller always gets a well-formed `Response` back.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ipc::protocol::{Request, Response};
use proto::change::{ChangedEvent, Scope};
use proto::error::ErrorInfo;
use proto::job::JobStarted;
use proto::plugin::{Permission, Touched};
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

/// How a plugin may reach a channel its contract opened to plugins.
#[derive(Clone, Copy)]
pub struct PluginAccess {
    pub permission: Permission,
    /// What a call's payload touches, for the narrowing its contract declares.
    pub touched: fn(&Value) -> Touched,
    /// The channel starts a job (it answers `JobStarted`), so `host.run-job`
    /// can follow it to its end.
    pub job: bool,
}

#[derive(Default)]
pub struct Router {
    handlers: HashMap<String, Handler>,
    plugin_access: HashMap<String, PluginAccess>,
    ambient: HashSet<String>,
}

impl Router {
    pub fn on(&mut self, channel: &str, handler: Handler) {
        self.handlers.insert(channel.to_string(), handler);
    }

    /// `None` for a channel a plugin may not reach, including an unknown one.
    pub fn plugin_access(&self, channel: &str) -> Option<PluginAccess> {
        self.plugin_access.get(channel).copied()
    }

    /// Whether any plugin may call `channel` with no permission.
    pub fn plugin_ambient(&self, channel: &str) -> bool {
        self.ambient.contains(channel)
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
            match self.handlers.get(&request.channel) {
                Some(handler) => {
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
        assert!(
            !(C::AMBIENT && C::PLUGIN.is_some()),
            "`{}` is both ambient and behind a permission",
            C::CHANNEL
        );
        if C::AMBIENT {
            self.router.ambient.insert(C::CHANNEL.to_string());
        }
        if let Some(permission) = C::PLUGIN {
            self.router.plugin_access.insert(
                C::CHANNEL.to_string(),
                PluginAccess {
                    permission,
                    touched: C::touched,
                    job: TypeId::of::<C::Result>() == TypeId::of::<JobStarted>(),
                },
            );
        }
        self.router.on(
            C::CHANNEL,
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
    use std::sync::{Arc, Mutex};

    use engine::CallChain;
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
            chain: CallChain::default(),
            changed: Changed::default(),
        };
        let response = crate::services::make_router()
            .route(Request::new(channel, payload, None), ctx)
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);
        response.payload
    }

    #[tokio::test]
    async fn an_entry_call_names_the_stable_id_however_the_caller_spelled_it() {
        let home = tempfile::tempdir().unwrap();
        let engine = engine::Engine::new(Some(home.path()));
        let record = engine
            .servers()
            .create("My Server", ServerProfile::default(), None)
            .unwrap();
        engine.servers().mark_ready(&record.id).unwrap();
        let runtime = Arc::new(Runtime::over(engine, home.path().join("d.log"), false));
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
        let runtime = Arc::new(Runtime::over(engine, home.path().join("d.log"), false));
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

    #[test]
    fn an_ambient_channel_is_open_to_plugins_without_a_permission() {
        let router = crate::services::make_router();
        for channel in ["app.info", "host.info", "net.status"] {
            assert!(router.plugin_ambient(channel), "{channel}");
            assert!(router.plugin_access(channel).is_none(), "{channel}");
        }
        assert!(!router.plugin_ambient("server.list"));
        assert!(!router.plugin_ambient("daemon.status"));
        assert!(router.plugin_access("server.list").is_some());
    }
}
