use std::sync::Arc;

use engine::Engine;
use proto::update::{UpdateDone, UpdateDownload};

use super::job::{Cancellations, Runner, Spec};
use crate::runtime::EventHub;

pub struct UpdateManager {
    runner: Runner<String>,
}

impl UpdateManager {
    pub fn new(engine: Arc<Engine>, hub: Arc<EventHub>, cancellations: Cancellations) -> Self {
        UpdateManager {
            runner: Runner::new(engine, hub, cancellations),
        }
    }

    /// Start the signed installer download off-thread. Returns the job id.
    pub fn start(&self, id: String) -> String {
        let spec = Spec {
            id,
            prefix: "update",
            key: None,
        };

        self.runner
            .start::<UpdateDownload, _>(spec, move |engine, reporter| {
                Box::pin(async move {
                    let report = reporter.checked();
                    let (path, version) = engine.download_update(&report).await?;
                    Ok(UpdateDone { path, version })
                })
            })
            .expect("a keyless job is never refused")
    }
}
