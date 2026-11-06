use std::time::Duration;

use futures_util::future::BoxFuture;

use proto::content::ContentKind;
use proto::error::EntryKind;
use serde::Serialize;

use super::{Hook, Verdict};
use crate::plugins::chain::CallChain;
use crate::plugins::imports::bindings::exports::hestia::plugin::content_hooks;
use crate::plugins::runtime::{CallError, LoadedPlugin};

/// One resolved file about to be installed, as a `before-content-install` hook
/// sees it: names and a hash, never the file's bytes.
#[derive(Debug, Clone)]
pub struct InstallContext {
    pub entry_id: String,
    pub entry_type: EntryKind,
    pub source: String,
    pub project_id: String,
    pub version_id: String,
    pub file_name: String,
    pub sha1: String,
    pub kind: ContentKind,
}

impl From<&InstallContext> for content_hooks::InstallContext {
    fn from(context: &InstallContext) -> Self {
        content_hooks::InstallContext {
            entry_id: context.entry_id.clone(),
            entry_type: context.entry_type.to_string(),
            source: context.source.clone(),
            project_id: context.project_id.clone(),
            version_id: context.version_id.clone(),
            file_name: context.file_name.clone(),
            sha1: context.sha1.clone(),
            kind: wire_name(context.kind),
        }
    }
}

/// The snake_case name a kind has on the wire, which is what a guest matches on.
fn wire_name(kind: impl Serialize) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The interceptor that runs on each file before it is installed. It can refuse
/// the file and nothing more, and it fails open like every hook: it is a
/// convenience, not a security scanner.
pub struct BeforeContentInstall;

impl Hook for BeforeContentInstall {
    const NAME: &'static str = "before-content-install";
    type Plugin = LoadedPlugin;
    type Context = InstallContext;
    type Patch = ();

    fn fold((): (), (): ()) {}

    fn invoke<'a>(
        plugin: &'a LoadedPlugin,
        context: &'a InstallContext,
        chain: &'a CallChain,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Verdict<()>, CallError>> {
        Box::pin(async move {
            let answer = plugin
                .before_content_install(chain, context.into(), timeout)
                .await?;
            Ok(match answer {
                Ok(()) => Verdict::Patch(()),
                Err(reason) => Verdict::Veto(reason),
            })
        })
    }
}
