//! Modpacks: installing one into a new or existing entry, moving it to another
//! published version, reading which pack an entry runs, and taking it back out.
//!
//! An install that *creates* its entry has nothing to guard — the entry does not
//! exist yet. One aimed at an existing entry is guarded exactly as a content
//! install is: stopped, and racing no backup, update or other content job.
//! Installing keeps a channel per side because a server install carries an EULA
//! and a port; every verb on an existing entry is one channel that names its
//! side in the target.

use std::path::Path;

use proto::error::{ErrorInfo, Field};
use proto::job::JobStarted;
use proto::modpack::{
    EntryModpackCheckUpdate, EntryModpackPreviewUpdate, EntryModpackRemove, EntryModpackStatus,
    EntryModpackUpdate, InstanceModpackInstall, ModpackRef, ModpackStatusResult, ModpackTarget,
    ModpackUpdateResult, ServerModpackInstall,
};

use super::guards::{entry_for, entry_held, instance_held, server_held, Hold, Intent};
use crate::runtime::{Channels, JobKind, ModpackJob};

pub(super) fn register(on: &mut Channels<'_>) {
    register_install(on);
    register_entry(on);
}

fn register_install(on: &mut Channels<'_>) {
    on.handle::<InstanceModpackInstall, _, _>(|p, ctx| async move {
        require_pack_ref(&p.pack)?;
        let (target, claim) = match &p.target {
            ModpackTarget::Create { name } => (ModpackTarget::Create { name: name.clone() }, None),
            ModpackTarget::Existing { entry } => {
                let (record, claim) = instance_held(&ctx, entry, Hold::stopped(JobKind::Modpack))?;
                (ModpackTarget::Existing { entry: record.id }, Some(claim))
            }
        };
        let id = ctx.runtime.jobs().start_modpack::<InstanceModpackInstall>(
            ModpackJob::InstallInstance {
                pack: p.pack,
                target,
            },
            p.id,
            claim,
        );
        Ok(JobStarted { id })
    });

    on.handle::<ServerModpackInstall, _, _>(|p, ctx| async move {
        require_pack_ref(&p.pack)?;
        let (target, claim) = match &p.target {
            ModpackTarget::Create { name } => {
                if !p.eula {
                    return Err(ErrorInfo::EulaRequired);
                }
                (ModpackTarget::Create { name: name.clone() }, None)
            }
            ModpackTarget::Existing { entry } => {
                let (record, claim) = server_held(&ctx, entry, Hold::stopped(JobKind::Modpack))?;
                (ModpackTarget::Existing { entry: record.id }, Some(claim))
            }
        };
        let id = ctx.runtime.jobs().start_modpack::<ServerModpackInstall>(
            ModpackJob::InstallServer {
                pack: p.pack,
                target,
                eula: p.eula,
                port: p.port,
            },
            p.id,
            claim,
        );
        Ok(JobStarted { id })
    });
}

fn register_entry(on: &mut Channels<'_>) {
    on.handle::<EntryModpackUpdate, _, _>(|p, ctx| async move {
        let (entry, claim) = entry_held(&ctx, &p.target, Hold::stopped(JobKind::Modpack))?;
        let id = ctx.runtime.jobs().start_modpack::<EntryModpackUpdate>(
            ModpackJob::Update { entry, to: p.to },
            p.id,
            Some(claim),
        );
        Ok(JobStarted { id })
    });

    on.handle::<EntryModpackPreviewUpdate, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        ctx.runtime
            .engine()
            .preview_entry_modpack_file(entry.entry_ref(), Path::new(&p.path))
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<EntryModpackStatus, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let pack = ctx
            .runtime
            .engine()
            .entry_modpack(entry.entry_ref())
            .map_err(crate::runtime::engine_error)?;
        Ok(ModpackStatusResult { pack })
    });

    on.handle::<EntryModpackCheckUpdate, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let update = ctx
            .runtime
            .engine()
            .entry_modpack_update(entry.entry_ref(), &p.game_version)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ModpackUpdateResult { update })
    });

    on.handle::<EntryModpackRemove, _, _>(|p, ctx| async move {
        let (entry, _claim) = entry_held(&ctx, &p.target, Hold::stopped(JobKind::Modpack))?;
        ctx.runtime
            .engine()
            .remove_entry_modpack(entry.entry_ref(), p.keep_content)
            .map_err(crate::runtime::engine_error)
    });
}

/// Exactly one selector, checked at the edge so a malformed reference never
/// becomes a job that fails a second later.
fn require_pack_ref(pack: &ModpackRef) -> Result<(), ErrorInfo> {
    let picked = [&pack.project, &pack.url, &pack.path]
        .iter()
        .filter(|s| !s.is_empty())
        .count();
    match picked {
        1 => Ok(()),
        0 => Err(ErrorInfo::FieldRequired {
            field: Field::Project,
        }),
        _ => Err(ErrorInfo::MutuallyExclusive {
            options: Field::CONTENT_SOURCES.to_vec(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_names_exactly_one_thing() {
        let project = ModpackRef {
            project: "fabulously-optimized".to_string(),
            ..ModpackRef::default()
        };
        assert!(require_pack_ref(&project).is_ok());
        assert!(require_pack_ref(&ModpackRef::default()).is_err());

        let both = ModpackRef {
            project: "fabulously-optimized".to_string(),
            path: "/tmp/pack.mrpack".to_string(),
            ..ModpackRef::default()
        };
        assert!(require_pack_ref(&both).is_err());
    }
}
