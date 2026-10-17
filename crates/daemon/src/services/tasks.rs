//! A server's scheduled tasks. Storing one is all a handler does — the
//! scheduler loop in `runtime/scheduler.rs` is what runs them.

use proto::error::ErrorInfo;
use proto::tasks::{ServerTaskList, ServerTaskRemove, ServerTaskSave};
use proto::Empty;

use super::guards::{server_for, Intent};
use crate::runtime::{now_unix, Channels};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerTaskList, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        Ok(record.tasks)
    });

    on.handle::<ServerTaskSave, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        ctx.runtime
            .engine()
            .servers()
            .save_task(&record.id, p.task, now_unix())
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ServerTaskRemove, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        match ctx
            .runtime
            .engine()
            .servers()
            .remove_task(&record.id, &p.id)
        {
            Ok(true) => Ok(Empty {}),
            Ok(false) => Err(ErrorInfo::TaskNotFound { id: p.id }),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });
}
