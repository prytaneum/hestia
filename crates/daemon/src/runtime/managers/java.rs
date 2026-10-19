use proto::java::{JavaInstall, JavaInstallDone};

use super::job::{Jobs, Spec};
use crate::runtime::JobKind;

impl Jobs {
    /// Start an install off-thread, one per release line at a time. Returns the
    /// job id, or `None` if that line is already installing.
    pub fn start_java_install(&self, major: i32, id: String, force: bool) -> Option<String> {
        let claim = self
            .claims
            .claim(format!("java-{major}"), JobKind::Install)
            .ok()?;
        let spec = Spec {
            id,
            prefix: "java-install",
            claim: Some(claim),
            subject: None,
        };

        Some(self.run::<JavaInstall, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let cancel = reporter.cancel();
                let report = reporter.checked();
                let outcome = engine
                    .java()
                    .install(major, force, cancel, move |p| {
                        let _ = report(p);
                    })
                    .await?;
                Ok(JavaInstallDone {
                    runtime: outcome.runtime,
                    already_installed: outcome.already_installed,
                })
            })
        }))
    }
}
