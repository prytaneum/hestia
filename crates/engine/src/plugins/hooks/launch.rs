use std::time::Duration;

use futures_util::future::BoxFuture;

use super::{Hook, Verdict};
use crate::plugins::chain::CallChain;
use crate::plugins::imports::bindings::exports::hestia::plugin::launch_hooks;
use crate::plugins::patch::LaunchPatch;
use crate::plugins::runtime::{CallError, LoadedPlugin};

/// What a launch is about to run, as a `before-launch` hook sees it. It holds
/// no access token and no command line.
#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub instance_id: String,
    pub instance_name: String,
    pub game_version: String,
    pub flavor: String,
    pub loader_version: Option<String>,
    pub java_path: String,
    pub memory: Option<String>,
    pub quick_play: Option<String>,
    pub profile_name: String,
    pub profile_uuid: String,
}

impl From<&LaunchContext> for launch_hooks::LaunchContext {
    fn from(context: &LaunchContext) -> Self {
        launch_hooks::LaunchContext {
            instance_id: context.instance_id.clone(),
            instance_name: context.instance_name.clone(),
            game_version: context.game_version.clone(),
            flavor: context.flavor.clone(),
            loader_version: context.loader_version.clone(),
            java_path: context.java_path.clone(),
            memory: context.memory.clone(),
            quick_play: context.quick_play.clone(),
            profile_name: context.profile_name.clone(),
            profile_uuid: context.profile_uuid.clone(),
        }
    }
}

/// The interceptor that runs just before a game launch. A plugin's answer is
/// held to its own grant and the allowlist before it is folded in; an error
/// from the guest is a veto, because a guest that merely fails traps instead.
pub struct BeforeLaunch;

impl Hook for BeforeLaunch {
    const NAME: &'static str = "before-launch";
    type Plugin = LoadedPlugin;
    type Context = LaunchContext;
    type Patch = LaunchPatch;

    fn fold(folded: LaunchPatch, next: LaunchPatch) -> LaunchPatch {
        folded.then(next)
    }

    fn invoke<'a>(
        plugin: &'a LoadedPlugin,
        context: &'a LaunchContext,
        chain: &'a CallChain,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Verdict<LaunchPatch>, CallError>> {
        Box::pin(async move {
            let answer = plugin.before_launch(chain, context.into(), timeout).await?;
            Ok(match answer {
                Ok(patch) => Verdict::Patch(LaunchPatch::vetted(
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
