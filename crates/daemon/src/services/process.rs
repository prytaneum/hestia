//! The raw supervisor surface: start, stop, and inspect any supervised process.

use proto::error::ErrorInfo;
use proto::process::{
    ProcessList, ProcessListResult, ProcessLogs, ProcessLogsResult, ProcessStart,
    ProcessStartResult, ProcessStatus, ProcessStop,
};
use proto::Empty;

use crate::runtime::{Channels, StartError};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ProcessStart, _, _>(|spec, ctx| async move {
        match ctx.runtime.processes().start(spec).await {
            Ok(info) => Ok(ProcessStartResult {
                id: info.id,
                pid: info.pid,
            }),
            Err(StartError::EmptyProgram) => Err(ErrorInfo::FieldRequired {
                field: proto::error::Field::Program,
            }),
            Err(StartError::InvalidId(_)) => Err(ErrorInfo::InvalidValue {
                field: proto::error::Field::ProcessId,
                reason: proto::error::Reason::ProcessIdCharset,
            }),
            Err(e @ StartError::Spawn { .. }) => Err(ErrorInfo::Internal {
                detail: format!("cannot spawn process: {e}"),
            }),
        }
    });

    on.handle::<ProcessStop, _, _>(|p, ctx| async move {
        if ctx.runtime.processes().stop(&p.id) {
            Ok(Empty {})
        } else {
            Err(ErrorInfo::ProcessNotFound { id: p.id.clone() })
        }
    });

    on.handle::<ProcessList, _, _>(|_: Empty, ctx| async move {
        Ok(ProcessListResult {
            processes: ctx.runtime.processes().list(),
        })
    });

    on.handle::<ProcessStatus, _, _>(|p, ctx| async move {
        ctx.runtime
            .processes()
            .status(&p.id)
            .ok_or_else(|| ErrorInfo::ProcessNotFound { id: p.id.clone() })
    });

    on.handle::<ProcessLogs, _, _>(|p, ctx| async move {
        match ctx.runtime.processes().logs(&p.id, p.tail) {
            Some(lines) => Ok(ProcessLogsResult { lines }),
            None => Err(ErrorInfo::ProcessNotFound { id: p.id.clone() }),
        }
    });
}
