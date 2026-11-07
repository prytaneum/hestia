//! Interceptors: a plugin's chance to adjust or refuse an operation just before
//! it happens. Bounded and fail-open
//! ([0095](../../../../docs/decisions/0095-interceptors-are-bounded-and-fail-open.md)):
//! a plugin that times out, traps or cannot run is skipped with a warning, and
//! only an explicit veto stops the operation.

use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use futures_util::future::BoxFuture;
use proto::error::ErrorInfo;
use proto::hook::HookPoint;
use proto::warning::WarningInfo;

mod content;
mod launch;

pub use content::BeforeContentInstall;
pub use launch::{BeforeLaunch, BeforeServerStart};

use super::chain::CallChain;
use super::runtime::{CallError, LoadedPlugin, HOOK_TIMEOUT};

/// What `Hooks::run` yields when no plugin vetoed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HookOutcome<Patch> {
    pub patch: Patch,
    pub warnings: Vec<WarningInfo>,
}

/// How the host acts on one hook point: what it keeps of a plugin's patch, and
/// how patches combine. A flow names it as the type argument of [`Hooks::run`];
/// the wire shape is its [`HookPoint`].
pub trait Hook: 'static {
    type Point: HookPoint;
    type Plugin: Participant;
    type Patch: Default + Send;

    /// What the host keeps of `plugin`'s answer.
    fn vet(plugin: &Self::Plugin, patch: <Self::Point as HookPoint>::Patch) -> Self::Patch;

    /// Merges the next plugin's patch into the ones before it.
    fn fold(folded: Self::Patch, next: Self::Patch) -> Self::Patch;
}

/// What `Hooks` needs of a plugin: whether to call it, and the call.
pub trait Participant: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn declares(&self, point: &str) -> bool;
    fn is_disabled(&self) -> bool;

    /// Runs `point` with `context` as JSON under `timeout`. The inner error is
    /// a veto; the answer is the point's patch as JSON.
    fn intercept<'a>(
        &'a self,
        point: &'a str,
        context: &'a str,
        chain: &'a CallChain,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Result<String, String>, CallError>>;
}

impl Participant for LoadedPlugin {
    fn id(&self) -> &str {
        LoadedPlugin::id(self)
    }

    fn declares(&self, point: &str) -> bool {
        self.grant().hooks.contains(point)
    }

    fn is_disabled(&self) -> bool {
        LoadedPlugin::is_disabled(self)
    }

    fn intercept<'a>(
        &'a self,
        point: &'a str,
        context: &'a str,
        chain: &'a CallChain,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Result<String, String>, CallError>> {
        Box::pin(LoadedPlugin::intercept(
            self, chain, point, context, timeout,
        ))
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

    /// Calls every enabled plugin that declares `H`'s point, ordered by plugin
    /// id, and folds their vetted patches in that order. An answer that is not
    /// the point's patch counts as a failed hook. No entry claim is involved:
    /// callers run hooks before taking one.
    pub async fn run<H: Hook<Plugin = P>>(
        &self,
        context: &<H::Point as HookPoint>::Context,
        chain: &CallChain,
    ) -> Result<HookOutcome<H::Patch>, ErrorInfo> {
        let name = H::Point::NAME;
        let mut plugins = self
            .roster
            .get()
            .and_then(Weak::upgrade)
            .map(|roster| roster.loaded())
            .unwrap_or_default();
        plugins.retain(|plugin| plugin.declares(name) && !plugin.is_disabled());
        plugins.sort_by(|a, b| a.id().cmp(b.id()));

        let mut outcome = HookOutcome {
            patch: H::Patch::default(),
            warnings: Vec::new(),
        };
        if plugins.is_empty() {
            return Ok(outcome);
        }
        let context = serde_json::to_string(context).expect("a hook context is plain data");
        for plugin in plugins {
            let failed = |outcome: &mut HookOutcome<H::Patch>, error: &dyn std::fmt::Display| {
                tracing::warn!(plugin = plugin.id(), hook = name, %error, "hook skipped");
                outcome.warnings.push(WarningInfo::PluginHookFailed {
                    plugin: plugin.id().to_string(),
                    hook: name.to_string(),
                });
            };
            match plugin.intercept(name, &context, chain, HOOK_TIMEOUT).await {
                Ok(Ok(reply)) => {
                    match serde_json::from_str::<Option<<H::Point as HookPoint>::Patch>>(&reply) {
                        Ok(patch) => {
                            let vetted = H::vet(&plugin, patch.unwrap_or_default());
                            outcome.patch = H::fold(std::mem::take(&mut outcome.patch), vetted);
                        }
                        Err(error) => failed(&mut outcome, &format!("unreadable patch: {error}")),
                    }
                }
                Ok(Err(reason)) => {
                    tracing::info!(plugin = plugin.id(), hook = name, "plugin vetoed");
                    return Err(ErrorInfo::PluginVetoed {
                        plugin: plugin.id().to_string(),
                        hook: name.to_string(),
                        reason,
                    });
                }
                Err(error) => failed(&mut outcome, &error),
            }
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
