use std::time::Duration;

use futures_util::future::BoxFuture;

use super::{Hook, Verdict};
use crate::plugins::chain::CallChain;
use crate::plugins::imports::bindings::exports::hestia::plugin::launch_hooks;
use crate::plugins::patch::LaunchPatch;
use crate::plugins::runtime::{CallError, LoadedPlugin};

/// What a server start is about to run, as a `before-server-start` hook sees it.
#[derive(Debug, Clone)]
pub struct ServerContext {
    pub id: String,
    pub name: String,
    pub version: String,
    pub flavor: String,
    pub port: u16,
    pub memory: Option<String>,
}

impl From<&ServerContext> for launch_hooks::ServerContext {
    fn from(context: &ServerContext) -> Self {
        launch_hooks::ServerContext {
            id: context.id.clone(),
            name: context.name.clone(),
            version: context.version.clone(),
            flavor: context.flavor.clone(),
            port: context.port,
            memory: context.memory.clone(),
        }
    }
}

/// The interceptor that runs just before a server is spawned. Its patch is held
/// to the same allowlist as [`BeforeLaunch`](super::BeforeLaunch)'s, minus the
/// game arguments.
pub struct BeforeServerStart;

impl Hook for BeforeServerStart {
    const NAME: &'static str = "before-server-start";
    type Plugin = LoadedPlugin;
    type Context = ServerContext;
    type Patch = LaunchPatch;

    fn fold(folded: LaunchPatch, next: LaunchPatch) -> LaunchPatch {
        folded.then(next)
    }

    fn invoke<'a>(
        plugin: &'a LoadedPlugin,
        context: &'a ServerContext,
        chain: &'a CallChain,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Verdict<LaunchPatch>, CallError>> {
        Box::pin(async move {
            let answer = plugin
                .before_server_start(chain, context.into(), timeout)
                .await?;
            Ok(match answer {
                Ok(patch) => Verdict::Patch(LaunchPatch::vetted_for_server(
                    plugin.id(),
                    &plugin.grant().launch_properties,
                    patch.extra_jvm_args,
                    patch.extra_game_args,
                )),
                Err(reason) => Verdict::Veto(reason),
            })
        })
    }
}
