//! Maps a channel name to a typed handler and routes a decoded request to it. An
//! unknown channel or a handler failure becomes a protocol-level error response,
//! so the caller always gets a well-formed `Response` back.

use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ipc::protocol::{Request, Response};
use proto::change::{ChangedEvent, Scope};
use proto::error::ErrorInfo;
use proto::job::JobStarted;
use proto::plugin::Permission;
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
#[derive(Debug, Clone, Copy)]
pub struct PluginAccess {
    pub permission: Permission,
    /// The channel starts a job (it answers `JobStarted`), so `host.run-job`
    /// can follow it to its end.
    pub job: bool,
}

#[derive(Default)]
pub struct Router {
    handlers: HashMap<String, Handler>,
    plugin_access: HashMap<String, PluginAccess>,
}

impl Router {
    pub fn on(&mut self, channel: &str, handler: Handler) {
        self.handlers.insert(channel.to_string(), handler);
    }

    /// `None` for a channel a plugin may not reach, including an unknown one.
    pub fn plugin_access(&self, channel: &str) -> Option<PluginAccess> {
        self.plugin_access.get(channel).copied()
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

fn publish_changed(runtime: &Runtime, scope: Scope) {
    runtime.hub().publish_topic(&ChangedEvent { scope });
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
        if let Some(permission) = C::PLUGIN {
            self.router.plugin_access.insert(
                C::CHANNEL.to_string(),
                PluginAccess {
                    permission,
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
                    match f(params, ctx).await {
                        Ok(result) => {
                            let payload = serde_json::to_value(result).unwrap_or(Value::Null);
                            if let Some(scope) = C::CHANGES {
                                publish_changed(&runtime, scope);
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
    use super::requires_account;

    #[test]
    fn gates_instance_and_sync_channels() {
        for channel in [
            "instance.launch",
            "instance.list",
            "instance.modpack.install",
            "sync.set",
            "instance.sync.adopt",
        ] {
            assert!(requires_account(channel), "{channel} should be gated");
        }
    }

    #[test]
    fn leaves_other_channels_open() {
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
