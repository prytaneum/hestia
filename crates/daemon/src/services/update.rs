//! Self-update: the released-version check, the signed artifact download, and
//! applying it.

use proto::job::JobStarted;
use proto::update::{UpdateApply, UpdateApplyResult, UpdateCheck, UpdateDownload};
use proto::Empty;

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<UpdateCheck, _, _>(|_: Empty, ctx| async move {
        ctx.runtime
            .engine()
            .check_update()
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<UpdateDownload, _, _>(|params, ctx| async move {
        Ok(JobStarted {
            id: ctx
                .runtime
                .jobs()
                .start_update_download(params.id, ctx.chain.clone()),
        })
    });

    on.handle::<UpdateApply, _, _>(|params, ctx| async move {
        let runtime = ctx.runtime.clone();
        tokio::task::spawn_blocking(move || runtime.engine().update().apply(&params.path))
            .await
            .map_err(|e| crate::runtime::engine_error(anyhow::anyhow!("{e}")))?
            .map(|applied| UpdateApplyResult {
                relaunches: applied.relaunches,
            })
            .map_err(crate::runtime::engine_error)
    });
}
