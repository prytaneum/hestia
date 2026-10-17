use std::sync::Arc;

use engine::Engine;
use proto::java::{JavaInstall, JavaInstallDone};

use super::job::{Cancellations, Runner, Spec};
use crate::runtime::EventHub;

pub struct JavaInstallManager {
    runner: Runner<i32>,
}

impl JavaInstallManager {
    pub fn new(engine: Arc<Engine>, hub: Arc<EventHub>, cancellations: Cancellations) -> Self {
        JavaInstallManager {
            runner: Runner::new(engine, hub, cancellations),
        }
    }

    /// Start an install off-thread, one per release line at a time. Returns the
    /// job id, or `None` if that line is already installing.
    pub fn start(&self, major: i32, id: String, force: bool) -> Option<String> {
        let spec = Spec {
            id,
            prefix: "java-install",
            key: Some(major),
        };

        self.runner
            .start::<JavaInstall, _>(spec, move |engine, reporter| {
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
            })
    }
}
