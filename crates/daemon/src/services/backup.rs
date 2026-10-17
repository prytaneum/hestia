//! Server backups: create and restore are jobs, list
//! and remove answer inline. Backups are a server feature — instances have
//! none — `instance.export` is what they have instead.

use proto::backup::{
    ServerBackupCreate, ServerBackupList, ServerBackupRemove, ServerBackupRestore,
};
use proto::error::ErrorInfo;
use proto::job::JobStarted;
use proto::Empty;

use super::guards::{is_running, require_backup, server_for, server_held, Hold, Intent};
use crate::runtime::{server_process_id, BackupJob, Channels, JobKind};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerBackupCreate, _, _>(|p, ctx| async move {
        let (record, claim) = server_held(&ctx, &p.server, Hold::live(JobKind::Backup))?;
        if !record.ready() {
            return Err(ErrorInfo::Provisioning { name: record.name });
        }
        let live = is_running(&ctx, &server_process_id(&record.id));
        let id = ctx.runtime.jobs().start_backup::<ServerBackupCreate>(
            BackupJob::ServerBackup {
                server_id: record.id,
                live,
                scheduled_retention: None,
            },
            p.id,
            claim,
        );
        Ok(JobStarted { id })
    });

    on.handle::<ServerBackupList, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let backups = ctx
            .runtime
            .engine()
            .server_backups(&record.id)
            .map_err(crate::runtime::engine_error)?;
        Ok(backups)
    });

    on.handle::<ServerBackupRestore, _, _>(|p, ctx| async move {
        if p.backup.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: proto::error::Field::Backup,
            });
        }
        let (record, claim) = server_held(&ctx, &p.server, Hold::stopped(JobKind::Backup))?;
        require_backup(ctx.runtime.engine().server_backups(&record.id), &p.backup)?;
        let id = ctx.runtime.jobs().start_backup::<ServerBackupRestore>(
            BackupJob::ServerRestore {
                server_id: record.id,
                backup: p.backup,
            },
            p.id,
            claim,
        );
        Ok(JobStarted { id })
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
