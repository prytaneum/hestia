//! What a plugin contributes under `contributes.providers`. Each kind is the
//! host's own registry trait over [`Guest`], which asks any operation of
//! `proto::provider` through the one `provider.invoke` export, so the
//! aggregates treat a plugin's provider like a built-in.
//!
//! Adding a kind: its vocabulary in `proto::provider`, an adapter module here,
//! and one line in each of [`Contributions::connect`], [`Contributions::register`]
//! and [`unregister`]. The ABI and the daemon do not change.

mod build;
mod content;
mod flavor;
mod format;
mod java;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use proto::error::{ErrorInfo, Service};
use proto::plugin::Manifest;
use proto::provider::Provider;
use proto::provider::{ProviderKind, ProviderOp};

pub use content::PluginContentProvider;
pub use flavor::PluginFlavorProvider;
pub use format::PluginFormat;
pub use java::PluginJavaProvider;

use crate::engine::Engine;
use crate::plugins::chain::CallChain;
use crate::plugins::context::CallContext;
use crate::plugins::runtime::{CallError, LoadedPlugin, ProviderCall, PROVIDER_TIMEOUT};

/// The guest end of one contributed provider: the plugin and the id its
/// manifest gives the provider. Registered as `<plugin>.<id>`, or as the bare
/// `<id>` when the plugin is official.
pub(super) struct Guest {
    plugin: Arc<LoadedPlugin>,
    kind: &'static str,
    local: String,
    id: String,
    service: Service,
}

impl Guest {
    fn new<K: ProviderKind>(
        plugin: Arc<LoadedPlugin>,
        manifest: &Manifest,
        local: &str,
        official: bool,
    ) -> Self {
        let id = if official {
            local.to_string()
        } else {
            format!("{}.{local}", plugin.id())
        };
        let name = if manifest.name.is_empty() {
            &manifest.id
        } else {
            &manifest.name
        };
        Guest {
            service: Service::Provider(Provider::new(&id, name)),
            id,
            kind: K::KIND,
            local: local.to_string(),
            plugin,
        }
    }

    /// The id the provider is registered under.
    fn id(&self) -> &str {
        &self.id
    }

    fn plugin(&self) -> &LoadedPlugin {
        &self.plugin
    }

    /// Asks `O`; `None` when the guest does not implement it.
    async fn try_ask<O: ProviderOp>(&self, args: &O::Args) -> Result<Option<O::Reply>> {
        self.try_ask_within::<O>(args, PROVIDER_TIMEOUT, CallContext::default())
            .await
    }

    /// [`Self::try_ask`] under `timeout`, with what `context` lends the call.
    async fn try_ask_within<O: ProviderOp>(
        &self,
        args: &O::Args,
        timeout: Duration,
        context: CallContext,
    ) -> Result<Option<O::Reply>> {
        debug_assert_eq!(<O::Kind as ProviderKind>::KIND, self.kind);
        let kind = self.kind;
        let args = serde_json::to_string(args).expect("provider arguments are plain data");
        let call = ProviderCall {
            kind,
            id: &self.local,
            op: O::OP,
            args: &args,
        };
        let reply = match self
            .plugin
            .provide(&CallChain::default(), call, timeout, context)
            .await
        {
            Ok(reply) => reply,
            Err(CallError::Unsupported(_)) => return Ok(None),
            Err(CallError::Unreachable(detail)) => {
                tracing::warn!(plugin = self.plugin.id(), provider = %self.local, kind, op = O::OP, %detail, "provider upstream unreachable");
                return Err(ErrorInfo::Offline {
                    service: Some(self.service.clone()),
                }
                .into());
            }
            Err(error) => return Err(self.failed(format!("{}: {error}", O::OP))),
        };
        serde_json::from_str(&reply).map(Some).map_err(|error| {
            tracing::warn!(plugin = self.plugin.id(), provider = %self.local, kind, op = O::OP, %error, "malformed provider reply");
            self.failed(format!("{} returned an unreadable reply", O::OP))
        })
    }

    /// Asks `O`, which the guest must implement.
    async fn ask<O: ProviderOp>(&self, args: &O::Args) -> Result<O::Reply> {
        self.try_ask::<O>(args)
            .await?
            .ok_or_else(|| self.failed(format!("{} is not implemented", O::OP)))
    }

    fn failed(&self, detail: String) -> anyhow::Error {
        ErrorInfo::PluginCallFailed {
            id: self.plugin.id().to_string(),
            detail: format!("{} `{}`: {detail}", self.kind, self.local),
        }
        .into()
    }
}

/// The ids of `manifest`'s contributions of kind `K`.
fn contributed<K: ProviderKind>(manifest: &Manifest) -> impl Iterator<Item = &str> {
    manifest
        .contributes
        .providers
        .iter()
        .filter(|contribution| contribution.kind == K::KIND)
        .map(|contribution| contribution.id.as_str())
}

/// Connects each contribution of kind `K` with `connect`. One that fails to
/// describe itself is logged and left out; the plugin stays enabled.
async fn connect_each<K: ProviderKind, T, F, Fut>(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
    official: bool,
    connect: F,
) -> Vec<T>
where
    F: Fn(Guest) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut connected = Vec::new();
    for local in contributed::<K>(manifest) {
        match connect(Guest::new::<K>(plugin.clone(), manifest, local, official)).await {
            Ok(provider) => connected.push(provider),
            Err(error) => tracing::warn!(
                plugin = plugin.id(),
                kind = K::KIND,
                provider = %local,
                error = %format!("{error:#}"),
                "provider not registered"
            ),
        }
    }
    connected
}

/// Logs the outcome of registering one provider.
fn registered(owner: &str, kind: &str, id: &str, outcome: Result<(), impl std::fmt::Display>) {
    match outcome {
        Ok(()) => tracing::info!(plugin = owner, kind, provider = %id, "provider registered"),
        Err(error) => {
            tracing::warn!(plugin = owner, kind, provider = %id, %error, "provider refused")
        }
    }
}

/// What one enabled plugin adds to the engine's registries.
#[derive(Default)]
pub struct Contributions {
    official: bool,
    sources: Vec<PluginContentProvider>,
    flavors: Vec<PluginFlavorProvider>,
    formats: Vec<PluginFormat>,
    java: Vec<PluginJavaProvider>,
}

impl Contributions {
    /// Connects every provider `manifest` contributes. `official` is whether
    /// the plugin's key is the official one, which alone may take bare ids.
    pub async fn connect(plugin: &Arc<LoadedPlugin>, manifest: &Manifest, official: bool) -> Self {
        Contributions {
            official,
            sources: content::connect(plugin, manifest, official).await,
            flavors: flavor::connect(plugin, manifest, official).await,
            formats: format::connect(plugin, manifest, official).await,
            java: java::connect(plugin, manifest, official),
        }
    }

    /// The archive formats among them, for a caller that drives a format
    /// itself instead of registering it.
    pub fn into_formats(self) -> Vec<PluginFormat> {
        self.formats
    }

    /// Adds every provider to the engine under `owner`.
    pub fn register(self, engine: &Engine, owner: &str) {
        let official = self.official;
        content::register(engine, owner, official, self.sources);
        flavor::register(engine, owner, official, self.flavors);
        format::register(engine, owner, official, self.formats);
        java::register(engine, owner, official, self.java);
    }
}

/// Takes everything `owner` contributed out of the engine's registries.
pub fn unregister(engine: &Engine, owner: &str) {
    engine.content().unregister(owner);
    engine.minecraft().unregister(owner);
    engine.unregister_formats(owner);
    engine.java().unregister(owner);
}
