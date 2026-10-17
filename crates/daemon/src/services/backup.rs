//! Server backups: create and restore are jobs over the `BackupManager`, list
//! and remove answer inline. Backups are a server feature — instances have
//! none — `instance.export` is what they have instead.

use proto::backup::{
    BackupListResult, ServerBackupCreate, ServerBackupList, ServerBackupRemove, ServerBackupRestore,
};
use proto::error::ErrorInfo;
use proto::job::JobStarted;
use proto::Empty;

use super::guards::{is_running, require_backup, server_for, Intent};
use crate::runtime::{server_process_id, BackupJob, Channels};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerBackupCreate, _, _>(|p, ctx| async move {
        // A backup is the one mutation that may run on a live server: it
        // flushes the world over rcon first. So it gates on the jobs that own
        // the tree, not on the process.
        let record = server_for(&ctx, &p.server, Intent::Backup)?;
        if !record.ready() {
            return Err(ErrorInfo::Provisioning {
                name: record.name.clone(),
            });
        }
        let live = is_running(&ctx, &server_process_id(&record.id));
        match ctx.runtime.backups().start::<ServerBackupCreate>(
            BackupJob::ServerBackup {
                server_id: record.id,
                live,
            },
            p.id,
        ) {
            Some(id) => Ok(JobStarted { id }),
            None => Err(ErrorInfo::BackupInProgress {
                name: record.name.clone(),
            }),
        }
    });

    on.handle::<ServerBackupList, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let backups = ctx
            .runtime
            .engine()
            .server_backups(&record.id)
            .map_err(crate::runtime::engine_error)?;
        Ok(BackupListResult { backups })
    });

    on.handle::<ServerBackupRestore, _, _>(|p, ctx| async move {
        if p.backup.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: proto::error::Field::Backup,
            });
        }
        let record = server_for(&ctx, &p.server, Intent::Mutate)?;
        require_backup(ctx.runtime.engine().server_backups(&record.id), &p.backup)?;
        match ctx.runtime.backups().start::<ServerBackupRestore>(
            BackupJob::ServerRestore {
                server_id: record.id,
                backup: p.backup,
            },
            p.id,
        ) {
            Some(id) => Ok(JobStarted { id }),
            None => Err(ErrorInfo::BackupInProgress {
                name: record.name.clone(),
            }),
        }
    });

    on.handle::<ServerBackupRemove, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        match ctx
            .runtime
            .engine()
            .remove_server_backup(&record.id, &p.backup)
        {
            Ok(true) => Ok(Empty {}),
            Ok(false) => Err(ErrorInfo::BackupNotFound {
                reference: p.backup.clone(),
            }),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });
}
