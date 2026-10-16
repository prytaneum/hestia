use std::sync::Arc;

use engine::{Engine, JavaInstallOutcome};
use proto::java::{
    JavaInstallCancelledEvent, JavaInstallDoneEvent, JavaInstallErrorEvent, JavaInstallProgress,
    JavaInstallProgressEvent,
};

use super::job::{progress_event, settle, Cancellations, Runner, Spec};
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
            progress: progress_event(|id, progress: &JavaInstallProgress| {
                JavaInstallProgressEvent {
                    id,
                    progress: progress.clone(),
                }
            }),
            done: settle(|id, outcome: JavaInstallOutcome| JavaInstallDoneEvent {
                id,
                runtime: outcome.runtime,
                already_installed: outcome.already_installed,
            }),
            cancelled: Some(settle(|id, ()| JavaInstallCancelledEvent { id })),
            error: settle(|id, e| JavaInstallErrorEvent {
                id,
                error: crate::runtime::engine_error(e),
            }),
        };

        self.runner.start(spec, move |engine, reporter| {
            Box::pin(async move {
                let cancel = reporter.cancel();
                let report = reporter.checked();
                engine
                    .java()
                    .install(major, force, cancel, move |p| {
                        let _ = report(p);
                    })
                    .await
            })
        })
    }
}
