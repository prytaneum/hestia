//! Preconditions shared by the domain registrars.
//!
//! Resolving an entry and refusing an operation that would race it are one act,
//! not a checklist each handler assembles: [`Intent`] names what the handler is
//! about to do and the table below decides which exclusions apply. A handler
//! that spells its own guards out is free to drift from its siblings — which is
//! how an instance content update came to race an export while an add could not.

use proto::error::{EntryKind, ErrorInfo, Field};
use proto::process::ProcessState;

use crate::runtime::{instance_process_id, server_process_id, HandlerContext};

/// What a handler is about to do to an entry.
pub(super) enum Intent {
    /// Reads it. Nothing is in the way.
    Read,
    /// Starts it. A server admits one process; an instance admits many
    /// sessions, so a running instance is not in its own way.
    Start,
    /// Rewrites its content pool or game directory.
    Mutate,
    /// Archives it. The one mutation that may run on a live entry — a backup
    /// flushes the world over rcon first — so it waits only on the jobs that
    /// write the tree it reads.
    Backup,
    /// Changes the entry itself — rename, remove, a version change.
    Lifecycle,
}

/// The exclusions one intent implies. Each is a job or process that owns the
/// entry's tree while it runs.
#[derive(Default, Clone, Copy)]
struct Exclusions {
    /// The entry must not be running: the JVM holds its jars open (locked on
    /// Windows), and changes only apply at the next start anyway.
    stopped: bool,
    provisioning: bool,
    update: bool,
    backup: bool,
    content: bool,
    modpack: bool,
    transfer: bool,
}

impl Exclusions {
    fn of(side: EntryKind, intent: &Intent) -> Exclusions {
        let base = Exclusions::default();
        match (side, intent) {
            (_, Intent::Read) => base,
            (EntryKind::Server, Intent::Start) => Exclusions {
                stopped: true,
                backup: true,
                ..base
            },
            (EntryKind::Server, Intent::Backup) => Exclusions {
                update: true,
                content: true,
                modpack: true,
                ..base
            },
            // Instances have no backups; `instance.export` is what they have.
            (EntryKind::Instance, Intent::Backup) => Exclusions {
                content: true,
                modpack: true,
                transfer: true,
                ..base
            },
            (EntryKind::Server, Intent::Mutate) => Exclusions {
                stopped: true,
                backup: true,
                update: true,
                content: true,
                modpack: true,
                ..base
            },
            (EntryKind::Server, Intent::Lifecycle) => Exclusions {
                stopped: true,
                provisioning: true,
                backup: true,
                update: true,
                content: true,
                modpack: true,
                ..base
            },
            // A launch re-mirrors the pool into `data/`, which is exactly the
            // tree an export is reading.
            (EntryKind::Instance, Intent::Start) => Exclusions {
                transfer: true,
                ..base
            },
            (EntryKind::Instance, Intent::Mutate | Intent::Lifecycle) => Exclusions {
                stopped: true,
                content: true,
                modpack: true,
                transfer: true,
                ..base
            },
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
    let process_id = server_process_id(&record.id);
    let running = is_running(ctx, &process_id);
    gate(
        ctx,
        Exclusions::of(EntryKind::Server, &intent),
        Gated {
            side: EntryKind::Server,
            id: &record.id,
            name: &record.name,
            key: &process_id,
            running,
        },
    )?;
    Ok(record)
}

/// Resolve an instance and refuse anything `intent` may not race.
pub(super) fn instance_for(
    ctx: &HandlerContext,
    reference: &str,
    intent: Intent,
) -> Result<engine::InstanceRecord, ErrorInfo> {
    let record = find_instance(ctx, reference)?;
    let key = instance_process_id(&record.id);
    // An instance runs its sessions under `instance-<id>_<seq>`, never under
    // the entry key itself — so "is it running" is a question about its
    // sessions, and asking the supervisor for the entry key always says no.
    let running = ctx.runtime.instance_running(&record.id);
    gate(
        ctx,
        Exclusions::of(EntryKind::Instance, &intent),
        Gated {
            side: EntryKind::Instance,
            id: &record.id,
            name: &record.name,
            key: &key,
            running,
        },
    )?;
    Ok(record)
}

struct Gated<'a> {
    side: EntryKind,
    id: &'a str,
    name: &'a str,
    /// The entry key every job manager is keyed by (`server-<id>` /
    /// `instance-<id>`).
    key: &'a str,
    running: bool,
}

fn gate(ctx: &HandlerContext, what: Exclusions, entry: Gated<'_>) -> Result<(), ErrorInfo> {
    let named = || entry.name.to_string();
    if what.stopped && entry.running {
        return Err(ErrorInfo::EntryRunning {
            entry: entry.side,
            name: named(),
        });
    }
    if what.provisioning && ctx.runtime.server_creates().in_flight(entry.name) {
        return Err(ErrorInfo::Provisioning { name: named() });
    }
    if what.update && ctx.runtime.server_updates().in_flight(entry.id) {
        return Err(ErrorInfo::UpdateInProgress { name: named() });
    }
    if what.backup && ctx.runtime.backups().in_flight(entry.key) {
        return Err(ErrorInfo::BackupInProgress { name: named() });
    }
    if what.content && ctx.runtime.content_jobs().in_flight(entry.key) {
        return Err(ErrorInfo::ContentInProgress { name: named() });
    }
    // A pack rewrites both the pool and the game directory, so it is held apart
    // from everything else the same way a content job is.
    if what.modpack && ctx.runtime.modpack_jobs().in_flight(entry.key) {
        return Err(ErrorInfo::ContentInProgress { name: named() });
    }
    // An export reads the whole entry tree, so anything that rewrites it would
    // put a state in the archive that never existed on disk. An *import* is
    // never in the way: it creates the entry it fills.
    if what.transfer && ctx.runtime.transfers().in_flight(entry.key) {
        return Err(ErrorInfo::ExportInProgress { name: named() });
    }
    Ok(())
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

pub(super) fn is_running(ctx: &HandlerContext, process_id: &str) -> bool {
    ctx.runtime
        .processes()
        .status(process_id)
        .is_some_and(|info| info.state == ProcessState::Running)
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
    use proto::error::EntryKind;

    use super::{require_content_items, require_item_names, Exclusions, Intent};

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
    fn accepts_a_batch_of_single_selector_items() {
        let spec = ContentAddSpec {
            items: vec![project_item("sodium"), project_item("lithium")],
            ..ContentAddSpec::default()
        };
        assert!(require_content_items(&spec).is_ok());
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

    #[test]
    fn a_backup_runs_on_a_live_entry_but_waits_for_the_jobs_that_write_it() {
        let what = Exclusions::of(EntryKind::Server, &Intent::Backup);
        assert!(!what.stopped, "a backup flushes the world over rcon first");
        assert!(what.content && what.modpack && what.update);
    }

    #[test]
    fn a_read_excludes_nothing() {
        for side in [EntryKind::Server, EntryKind::Instance] {
            let what = Exclusions::of(side, &Intent::Read);
            assert!(!what.stopped && !what.backup && !what.content && !what.transfer);
        }
    }

    #[test]
    fn every_mutation_waits_for_the_jobs_that_own_the_tree() {
        for intent in [Intent::Mutate, Intent::Lifecycle] {
            let server = Exclusions::of(EntryKind::Server, &intent);
            assert!(server.stopped && server.content && server.modpack && server.backup);
            let instance = Exclusions::of(EntryKind::Instance, &intent);
            assert!(instance.stopped && instance.content && instance.modpack);
            assert!(
                instance.transfer,
                "an export reads the tree a mutation rewrites"
            );
        }
    }

    #[test]
    fn only_a_server_is_in_its_own_way_when_starting() {
        assert!(Exclusions::of(EntryKind::Server, &Intent::Start).stopped);
        assert!(
            !Exclusions::of(EntryKind::Instance, &Intent::Start).stopped,
            "an instance runs several sessions at once"
        );
    }

    #[test]
    fn backups_and_updates_are_a_server_concern_and_transfers_an_instance_one() {
        let server = Exclusions::of(EntryKind::Server, &Intent::Lifecycle);
        assert!(server.backup && server.update && server.provisioning && !server.transfer);
        let instance = Exclusions::of(EntryKind::Instance, &Intent::Lifecycle);
        assert!(instance.transfer && !instance.backup && !instance.update);
    }
}
