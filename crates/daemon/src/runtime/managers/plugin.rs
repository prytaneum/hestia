use std::path::PathBuf;
use std::sync::Arc;

use engine::CallChain;
use proto::plugin::{PluginInstall, PluginInstallDone, PluginInstallProgress};

use super::job::{Jobs, Spec};
use crate::runtime::{JobKind, Plugins};

impl Jobs {
    /// `None` if `plugin` is already installing.
    pub fn start_plugin_install(
        &self,
        plugins: Arc<Plugins>,
        plugin: &str,
        id: String,
        path: PathBuf,
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
                let record = plugins.install(&path, &approve, reporter.cancel(), &|phase| {
                    let _ = report(&PluginInstallProgress { phase });
                })?;
                Ok(PluginInstallDone {
                    plugin: plugins.installed(record).await,
                })
            })
        }))
    }
}
