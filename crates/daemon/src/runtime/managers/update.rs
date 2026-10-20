use engine::CallChain;
use proto::update::{UpdateDone, UpdateDownload};

use super::job::{Jobs, Spec};

impl Jobs {
    /// Start the signed installer download off-thread. Returns the job id.
    pub fn start_update_download(&self, id: String, chain: CallChain) -> String {
        let spec = Spec {
            id,
            prefix: "update",
            claim: None,
            subject: None,
            chain,
        };

        self.run::<UpdateDownload, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let report = reporter.checked();
                let (path, version) = engine.download_update(&report).await?;
                Ok(UpdateDone { path, version })
            })
        })
    }
}
