use std::sync::Arc;

use engine::{CallChain, CallError, CommandRun, LoadedPlugin, CANCEL_GRACE, COMMAND_TIMEOUT};
use proto::error::ErrorInfo;
use proto::job::JobSubject;
use proto::plugin::{
    PluginInstall, PluginInstallDone, PluginInstallProgress, PluginRun, PluginRunDone,
};
use serde_json::Value;
use tokio::sync::mpsc::unbounded_channel;

use super::job::{Jobs, Spec};
use crate::runtime::Package;
use crate::runtime::{JobKind, Plugins};

impl Jobs {
    /// `None` if `plugin` is already installing.
    pub fn start_plugin_install(
        &self,
        plugins: Arc<Plugins>,
        plugin: &str,
        id: String,
        package: Package,
        approve: String,
        chain: CallChain,
    ) -> Option<String> {
        let claim = self
            .claims
            .claim(format!("plugin-{plugin}"), JobKind::Install)
            .ok()?;
        let spec = Spec {
            id,
            prefix: "plugin-install",
            claim: Some(claim),
            subject: None,
            chain,
        };

        Some(self.run::<PluginInstall, _>(spec, move |_, reporter| {
            Box::pin(async move {
                let report = reporter.checked();
                let record = plugins.install(&package, &approve, reporter.cancel(), &|phase| {
                    let _ = report(&PluginInstallProgress { phase });
                })?;
                Ok(PluginInstallDone {
                    plugin: plugins.installed(record).await,
                })
            })
        }))
    }

    /// Runs one plugin command. The entry, if any, is not claimed: the plugin may
    /// or may not change it, so a command that must exclude other work takes that
    /// itself.
    pub fn start_plugin_run(
        &self,
        loaded: Arc<LoadedPlugin>,
        command: String,
        args: Value,
        id: String,
        subject: Option<JobSubject>,
        chain: CallChain,
    ) -> String {
        let spec = Spec {
            id,
            prefix: "plugin-run",
            claim: None,
            subject,
            chain: chain.clone(),
        };

        self.run::<PluginRun, _>(spec, move |_, reporter| {
            Box::pin(async move {
                let cancel = reporter.cancel().clone();
                let report = reporter.checked();
                let (progress, mut reports) = unbounded_channel();
                let control = CommandRun {
                    cancel: cancel.clone(),
                    progress,
                    grace: CANCEL_GRACE,
                };
                let call = loaded.run_command(&chain, &command, &args, control, COMMAND_TIMEOUT);
                tokio::pin!(call);
                let outcome = loop {
                    tokio::select! {
                        outcome = &mut call => break outcome,
                        Some(progress) = reports.recv() => {
                            let _ = report(&progress);
                        }
                    }
                };
                match outcome {
                    Ok(output) => Ok(PluginRunDone { output }),
                    Err(error) if cancel.is_cancelled() => {
                        tracing::info!(plugin = %loaded.id(), %command, %error, "plugin command stopped by its cancel");
                        Err(engine::Cancelled.into())
                    }
                    Err(CallError::Cancelled) => Err(engine::Cancelled.into()),
                    Err(error) => Err(ErrorInfo::PluginCallFailed {
                        id: loaded.id().to_string(),
                        detail: error.to_string(),
                    }
                    .into()),
                }
            })
        })
    }
}
