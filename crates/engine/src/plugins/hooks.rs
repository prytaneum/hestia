//! Interceptors: a plugin's chance to adjust or refuse an operation just before
//! it happens. Bounded and fail-open
//! ([0095](../../../../docs/decisions/0095-interceptors-are-bounded-and-fail-open.md)):
//! a plugin that times out, traps or cannot run is skipped with a warning, and
//! only an explicit veto stops the operation.

use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use futures_util::future::BoxFuture;
use proto::error::ErrorInfo;
use proto::warning::WarningInfo;

mod launch;

pub use launch::{BeforeLaunch, LaunchContext};

use super::chain::CallChain;
use super::runtime::{CallError, LoadedPlugin, HOOK_TIMEOUT};

/// What a plugin answered: a patch to fold in, or a refusal with its reason.
pub enum Verdict<Patch> {
    Patch(Patch),
    Veto(String),
}

/// What `Hooks::run` yields when no plugin vetoed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HookOutcome<Patch> {
    pub patch: Patch,
    pub warnings: Vec<WarningInfo>,
}

/// One kind of interceptor. A flow names it as the type argument of
/// [`Hooks::run`]; the real guest call lives in the implementation.
pub trait Hook: 'static {
    /// The name a manifest lists in `contributes.hooks`; see `proto::plugin::HOOKS`.
    const NAME: &'static str;
    type Plugin: Participant;
    type Context: Sync;
    type Patch: Default + Send;

    /// Merges the next plugin's patch into the ones before it.
    fn fold(folded: Self::Patch, next: Self::Patch) -> Self::Patch;

    /// Runs the hook in one plugin under `timeout`.
    fn invoke<'a>(
        plugin: &'a Self::Plugin,
        context: &'a Self::Context,
        chain: &'a CallChain,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Verdict<Self::Patch>, CallError>>;
}

/// What `Hooks` needs to know of a plugin to decide whether to call it.
pub trait Participant: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn declares(&self, hook: &str) -> bool;
    fn is_disabled(&self) -> bool;
}

impl Participant for LoadedPlugin {
    fn id(&self) -> &str {
        LoadedPlugin::id(self)
    }

    fn declares(&self, hook: &str) -> bool {
        self.grant().hooks.contains(hook)
    }

    fn is_disabled(&self) -> bool {
        LoadedPlugin::is_disabled(self)
    }
}

/// The loaded plugins, as the owner of their lifecycle knows them.
pub trait Roster<P>: Send + Sync {
    fn loaded(&self) -> Vec<Arc<P>>;
}

pub struct Hooks<P> {
    roster: OnceLock<Weak<dyn Roster<P>>>,
}

impl<P> Default for Hooks<P> {
    fn default() -> Self {
        Hooks {
            roster: OnceLock::new(),
        }
    }
}

impl<P: Participant> Hooks<P> {
    /// Held weakly: the roster's owner already holds the engine that holds this.
    pub fn attach<R: Roster<P> + 'static>(&self, roster: &Arc<R>) {
        let roster: Weak<R> = Arc::downgrade(roster);
        let roster: Weak<dyn Roster<P>> = roster;
        if self.roster.set(roster).is_err() {
            tracing::warn!("the hook roster is already attached");
        }
    }

    /// Calls every enabled plugin that declares `H`, ordered by plugin id, and
    /// folds their patches in that order. No entry claim is involved: callers
    /// run hooks before taking one.
    pub async fn run<H: Hook<Plugin = P>>(
        &self,
        context: &H::Context,
        chain: &CallChain,
    ) -> Result<HookOutcome<H::Patch>, ErrorInfo> {
        let mut plugins = self
            .roster
            .get()
            .and_then(Weak::upgrade)
            .map(|roster| roster.loaded())
            .unwrap_or_default();
        plugins.retain(|plugin| plugin.declares(H::NAME) && !plugin.is_disabled());
        plugins.sort_by(|a, b| a.id().cmp(b.id()));

        let mut outcome = HookOutcome {
            patch: H::Patch::default(),
            warnings: Vec::new(),
        };
        for plugin in plugins {
            match H::invoke(&plugin, context, chain, HOOK_TIMEOUT).await {
                Ok(Verdict::Patch(patch)) => {
                    outcome.patch = H::fold(std::mem::take(&mut outcome.patch), patch);
                }
                Ok(Verdict::Veto(reason)) => {
                    tracing::info!(plugin = plugin.id(), hook = H::NAME, "plugin vetoed");
                    return Err(ErrorInfo::PluginVetoed {
                        plugin: plugin.id().to_string(),
                        hook: H::NAME.to_string(),
                        reason,
                    });
                }
                Err(error) => {
                    tracing::warn!(plugin = plugin.id(), hook = H::NAME, %error, "hook skipped");
                    outcome.warnings.push(WarningInfo::PluginHookFailed {
                        plugin: plugin.id().to_string(),
                        hook: H::NAME.to_string(),
                    });
                }
            }
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
