use std::path::PathBuf;
use std::sync::Arc;

use engine::CallChain;
use proto::plugin::{PluginInstall, PluginInstallProgress};

use super::job::{Jobs, Spec};
use crate::runtime::Plugins;

impl Jobs {
    pub fn start_plugin_install(
        &self,
        plugins: Arc<Plugins>,
        id: String,
        path: PathBuf,
        approve: String,
        chain: CallChain,
    ) -> String {
        let spec = Spec {
            id,
            prefix: "plugin-install",
            claim: None,
            subject: None,
            chain,
        };

        self.run::<PluginInstall, _>(spec, move |_, reporter| {
            Box::pin(async move {
                let report = reporter.checked();
                let record = plugins.install(&path, &approve, reporter.cancel(), &|phase| {
                    let _ = report(&PluginInstallProgress { phase });
                })?;
                Ok(plugins.installed(record).await)
            })
        })
    }
}
