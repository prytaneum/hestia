//! Instance import and export.
//!
//! Export is guarded exactly as a backup is — stopped, and racing no content,
//! pack or transfer job — because it reads the same tree those write. Import
//! guards nothing: it creates the entry it fills, so there is no entry yet for
//! anything to race.

use proto::error::{ErrorInfo, Field};
use proto::job::JobStarted;
use proto::transfer::{
    ExportContentsResult, InstanceExport, InstanceExportContents, InstanceImport,
    InstanceImportInspect,
};

use super::guards::{instance_for, Intent};
use crate::runtime::{Channels, TransferJob};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<InstanceExport, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Mutate)?;
        let started = ctx.runtime.transfers().start(
            TransferJob::Export {
                instance_id: record.id,
                format: p.format,
                destination: p.destination,
                exclude: p.exclude,
            },
            p.id,
        );
        started
            .map(|id| JobStarted { id })
            .ok_or(ErrorInfo::ExportInProgress { name: record.name })
    });

    on.handle::<InstanceExportContents, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        let entries = ctx
            .runtime
            .engine()
            .export_contents(&record.id)
            .map_err(crate::runtime::engine_error)?;
        Ok(ExportContentsResult { entries })
    });

    on.handle::<InstanceImport, _, _>(|p, ctx| async move {
        require_path(&p.path)?;
        let started = ctx.runtime.transfers().start(
            TransferJob::Import {
                path: p.path,
                name: p.name,
            },
            p.id,
        );
        started
            .map(|id| JobStarted { id })
            .ok_or(ErrorInfo::ImportInProgress)
    });

    on.handle::<InstanceImportInspect, _, _>(|p, ctx| async move {
        require_path(&p.path)?;
        ctx.runtime
            .engine()
            .inspect_archive(&p.path)
            .map_err(crate::runtime::engine_error)
    });
}

fn require_path(path: &str) -> Result<(), ErrorInfo> {
    match path.trim().is_empty() {
        true => Err(ErrorInfo::FieldRequired { field: Field::Path }),
        false => Ok(()),
    }
}
