//! Preconditions shared by the domain registrars.
//!
//! Resolving an entry and refusing an operation that would race it are one act,
//! not a checklist each handler assembles. An entry's tree has one lock
//! ([`Claims`](crate::runtime::Claims)): a handler that changes the entry takes
//! its claim here and moves it into the job it starts, so the check and the
//! claim are a single step and a refusal names the job that holds the entry.

use engine::Side;
use proto::error::{EntryKind, EntryTarget, ErrorInfo, Field};

use crate::runtime::{
    create_key, instance_process_id, require_account, server_process_id, Claim, HandlerContext,
    JobEntry, JobKind,
};

/// What a handler that holds nothing is about to do to an entry.
pub(super) enum Intent {
    /// Reads it. Nothing is in the way.
    Read,
    /// Works inside it — a file edit, an instance launch — which any job that
    /// owns the tree would race. Allowed on a live entry: what the game holds
    /// open is its own to refuse.
    Idle,
}

/// What a handler that changes an entry takes: the claim, and whether the entry
/// must also be stopped (the JVM holds its jars open, locked on Windows, and a
/// change only applies at the next start anyway). A backup is the one claim
/// that may be taken live: it flushes the world over rcon first.
pub(super) struct Hold {
    kind: JobKind,
    stopped: bool,
}

impl Hold {
    pub(super) fn stopped(kind: JobKind) -> Self {
        Hold {
            kind,
            stopped: true,
        }
    }

    pub(super) fn live(kind: JobKind) -> Self {
        Hold {
            kind,
            stopped: false,
        }
    }
}

/// The side a shared verb names, refusing an instance without an account: the
/// router's prefix gate cannot see a side carried in the payload.
pub(super) fn side_for(ctx: &HandlerContext, kind: EntryKind) -> Result<Side, ErrorInfo> {
    match kind {
        EntryKind::Server => Ok(Side::Server),
        EntryKind::Instance => {
            require_account(ctx)?;
            Ok(Side::Client)
        }
    }
}

/// Resolve the entry a shared verb is aimed at, applying the guards of its side.
pub(super) fn entry_for(
    ctx: &HandlerContext,
    target: &EntryTarget,
    intent: Intent,
) -> Result<JobEntry, ErrorInfo> {
    match side_for(ctx, target.kind)? {
        Side::Server => Ok(server_for(ctx, &target.entry, intent)?.into()),
        Side::Client => Ok(instance_for(ctx, &target.entry, intent)?.into()),
    }
}

/// [`entry_for`], taking the entry's claim.
pub(super) fn entry_held(
    ctx: &HandlerContext,
    target: &EntryTarget,
    hold: Hold,
) -> Result<(JobEntry, Claim), ErrorInfo> {
    match side_for(ctx, target.kind)? {
        Side::Server => {
            let (record, claim) = server_held(ctx, &target.entry, hold)?;
            Ok((record.into(), claim))
        }
        Side::Client => {
            let (record, claim) = instance_held(ctx, &target.entry, hold)?;
            Ok((record.into(), claim))
        }
    }
}

/// Resolve a server and refuse anything `intent` may not race.
pub(super) fn server_for(
    ctx: &HandlerContext,
    reference: &str,
    intent: Intent,
) -> Result<engine::ServerRecord, ErrorInfo> {
    let record = find_server(ctx, reference)?;
    if let Intent::Idle = intent {
        refuse_provisioning(ctx, &record)?;
        refuse_held(
            ctx,
            EntryKind::Server,
            &server_process_id(&record.id),
            &record.name,
        )?;
    }
    Ok(record)
}

/// Resolve a server and take its claim, or refuse with whatever holds it.
pub(super) fn server_held(
    ctx: &HandlerContext,
    reference: &str,
    hold: Hold,
) -> Result<(engine::ServerRecord, Claim), ErrorInfo> {
    let record = find_server(ctx, reference)?;
    refuse_provisioning(ctx, &record)?;
    let process_id = server_process_id(&record.id);
    let claim = take(ctx, EntryKind::Server, &process_id, &record.name, hold.kind)?;
    if hold.stopped && ctx.runtime.process_running(&process_id) {
        return Err(ErrorInfo::EntryRunning {
            entry: EntryKind::Server,
            name: record.name,
        });
    }
    Ok((record, claim))
}

/// Resolve an instance and refuse anything `intent` may not race.
pub(super) fn instance_for(
    ctx: &HandlerContext,
    reference: &str,
    intent: Intent,
) -> Result<engine::InstanceRecord, ErrorInfo> {
    let record = find_instance(ctx, reference)?;
    if let Intent::Idle = intent {
        refuse_held(
            ctx,
            EntryKind::Instance,
            &instance_process_id(&record.id),
            &record.name,
        )?;
    }
    Ok(record)
}

/// Resolve an instance and take its claim, or refuse with whatever holds it.
pub(super) fn instance_held(
    ctx: &HandlerContext,
    reference: &str,
    hold: Hold,
) -> Result<(engine::InstanceRecord, Claim), ErrorInfo> {
    let record = find_instance(ctx, reference)?;
    let claim = take(
        ctx,
        EntryKind::Instance,
        &instance_process_id(&record.id),
        &record.name,
        hold.kind,
    )?;
    // An instance runs its sessions under `instance-<id>_<seq>`, never under
    // the entry key itself — so "is it running" is a question about its
    // sessions, and asking the supervisor for the entry key always says no.
    if hold.stopped && ctx.runtime.instance_running(&record.id) {
        return Err(ErrorInfo::EntryRunning {
            entry: EntryKind::Instance,
            name: record.name,
        });
    }
    Ok((record, claim))
}

fn take(
    ctx: &HandlerContext,
    side: EntryKind,
    key: &str,
    name: &str,
    kind: JobKind,
) -> Result<Claim, ErrorInfo> {
    ctx.runtime
        .claims()
        .claim(key.to_string(), kind)
        .map_err(|holder| busy(holder, side, name))
}

fn refuse_held(
    ctx: &HandlerContext,
    side: EntryKind,
    key: &str,
    name: &str,
) -> Result<(), ErrorInfo> {
    match ctx.runtime.claims().holder(key) {
        Some(holder) => Err(busy(holder, side, name)),
        None => Ok(()),
    }
}

fn refuse_provisioning(
    ctx: &HandlerContext,
    record: &engine::ServerRecord,
) -> Result<(), ErrorInfo> {
    match ctx.runtime.claims().holder(&create_key(&record.name)) {
        Some(_) => Err(ErrorInfo::Provisioning {
            name: record.name.clone(),
        }),
        None => Ok(()),
    }
}

fn busy(holder: JobKind, side: EntryKind, name: &str) -> ErrorInfo {
    let name = name.to_string();
    match holder {
        JobKind::Launch => ErrorInfo::EntryRunning { entry: side, name },
        JobKind::Install => ErrorInfo::Provisioning { name },
        JobKind::Update => ErrorInfo::UpdateInProgress { name },
        JobKind::Backup => ErrorInfo::BackupInProgress { name },
        JobKind::Content | JobKind::Modpack => ErrorInfo::ContentInProgress { name },
        JobKind::Export => ErrorInfo::ExportInProgress { name },
    }
}

pub(super) fn find_server(
    ctx: &HandlerContext,
    reference: &str,
) -> Result<engine::ServerRecord, ErrorInfo> {
    ctx.runtime
        .engine()
        .servers()
        .get(reference)
        .ok_or_else(|| ErrorInfo::EntryNotFound {
            entry: EntryKind::Server,
            reference: reference.to_string(),
        })
}

pub(super) fn find_instance(
    ctx: &HandlerContext,
    reference: &str,
) -> Result<engine::InstanceRecord, ErrorInfo> {
    ctx.runtime
        .engine()
        .instances()
        .get(reference)
        .ok_or_else(|| ErrorInfo::EntryNotFound {
            entry: EntryKind::Instance,
            reference: reference.to_string(),
        })
}

/// Resolve a server for a read and refuse unless it is up.
pub(super) fn server_running(
    ctx: &HandlerContext,
    reference: &str,
) -> Result<engine::ServerRecord, ErrorInfo> {
    let record = server_for(ctx, reference, Intent::Read)?;
    require_running(ctx, &record)?;
    Ok(record)
}

pub(super) fn require_running(
    ctx: &HandlerContext,
    record: &engine::ServerRecord,
) -> Result<(), ErrorInfo> {
    match ctx.runtime.process_running(&server_process_id(&record.id)) {
        true => Ok(()),
        false => Err(ErrorInfo::NotRunning {
            entry: EntryKind::Server,
            name: record.name.clone(),
        }),
    }
}

pub(super) fn require_content_items(
    spec: &proto::content::ContentAddSpec,
) -> Result<(), ErrorInfo> {
    if spec.items.is_empty() {
        return Err(ErrorInfo::NothingToDo {
            what: proto::error::Task::Install,
        });
    }
    for item in &spec.items {
        let picked = [&item.project, &item.url, &item.path]
            .iter()
            .filter(|s| !s.is_empty())
            .count();
        if picked != 1 {
            return Err(ErrorInfo::MutuallyExclusive {
                options: proto::error::Field::CONTENT_SOURCES.to_vec(),
            });
        }
    }
    if !spec.worlds.is_empty() && spec.kind != proto::content::ContentKind::DataPack {
        return Err(ErrorInfo::UnsupportedOperation {
            reason: proto::error::Unsupported::WorldsForDatapacksOnly,
        });
    }
    Ok(())
}

/// A blank reference compares equal to every local-file item's empty project
/// id, so it would silently select those; it is never a name. An empty batch is
/// legal — it means every item of the kind.
pub(super) fn require_item_names(items: &[String]) -> Result<(), ErrorInfo> {
    match items.iter().any(|item| item.is_empty()) {
        true => Err(ErrorInfo::FieldRequired { field: Field::Item }),
        false => Ok(()),
    }
}

pub(super) fn require_one_for_file(items: &[String], path: &str) -> Result<(), ErrorInfo> {
    match path.is_empty() || items.len() == 1 {
        true => Ok(()),
        false => Err(ErrorInfo::UnsupportedOperation {
            reason: proto::error::Unsupported::FileUpdatesOneItem,
        }),
    }
}

pub(super) fn require_backup(
    backups: anyhow::Result<Vec<proto::backup::BackupInfo>>,
    reference: &str,
) -> Result<(), ErrorInfo> {
    let backups = backups.map_err(crate::runtime::engine_error)?;
    if backups.iter().any(|b| b.id == reference) {
        Ok(())
    } else {
        Err(ErrorInfo::BackupNotFound {
            reference: reference.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use proto::content::{ContentAddItem, ContentAddSpec, ContentKind};

    use super::{require_content_items, require_item_names};

    #[test]
    fn a_blank_reference_in_a_batch_is_refused() {
        assert!(require_item_names(&["sodium".to_string()]).is_ok());
        assert!(require_item_names(&[]).is_ok(), "empty means every item");
        assert!(require_item_names(&["sodium".to_string(), String::new()]).is_err());
    }

    fn project_item(project: &str) -> ContentAddItem {
        ContentAddItem {
            project: project.to_string(),
            ..ContentAddItem::default()
        }
    }

    #[test]
    fn rejects_an_empty_batch() {
        assert!(require_content_items(&ContentAddSpec::default()).is_err());
    }

    #[test]
    fn rejects_an_item_with_no_or_multiple_selectors() {
        let empty = ContentAddSpec {
            items: vec![ContentAddItem::default()],
            ..ContentAddSpec::default()
        };
        assert!(require_content_items(&empty).is_err());

        let mut both = project_item("sodium");
        both.url = "https://modrinth.com/mod/sodium".to_string();
        let spec = ContentAddSpec {
            items: vec![both],
            ..ContentAddSpec::default()
        };
        assert!(require_content_items(&spec).is_err());
    }

    #[test]
    fn rejects_worlds_on_non_datapack_kinds() {
        let spec = ContentAddSpec {
            kind: ContentKind::Mod,
            items: vec![project_item("sodium")],
            worlds: vec!["world".to_string()],
            ..ContentAddSpec::default()
        };
        assert!(require_content_items(&spec).is_err());

        let spec = ContentAddSpec {
            kind: ContentKind::DataPack,
            items: vec![project_item("terralith")],
            worlds: vec!["world".to_string()],
            ..ContentAddSpec::default()
        };
        assert!(require_content_items(&spec).is_ok());
    }
}
