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
use proto::modpack::{
    EntryModpackCheckUpdate, EntryModpackPreviewUpdate, EntryModpackRemove, EntryModpackStatus,
    EntryModpackUpdate, InstanceModpackInstall, ModpackJobResult, ModpackRef, ModpackStatusResult,
    ModpackTarget, ModpackUpdateResult, ServerModpackInstall,
};

use super::guards::{entry_for, instance_for, server_for, Intent};
use crate::runtime::{Channels, ModpackJob};

pub(super) fn register(on: &mut Channels<'_>) {
    register_install(on);
    register_entry(on);
}

fn register_install(on: &mut Channels<'_>) {
    on.handle::<InstanceModpackInstall, _, _>(|p, ctx| async move {
        require_pack_ref(&p.pack)?;
        let target = match &p.target {
            ModpackTarget::Create { name } => ModpackTarget::Create { name: name.clone() },
            ModpackTarget::Existing { entry } => {
                let record = instance_for(&ctx, entry, Intent::Mutate)?;
                ModpackTarget::Existing {
                    entry: record.id.clone(),
                }
            }
        };
        let busy = target_name(&target);
        start(
            ctx.runtime.modpack_jobs().start(
                ModpackJob::InstallInstance {
                    pack: p.pack,
                    target,
                },
                p.id,
            ),
            busy,
        )
    });

    on.handle::<ServerModpackInstall, _, _>(|p, ctx| async move {
        require_pack_ref(&p.pack)?;
        let target = match &p.target {
            ModpackTarget::Create { name } => {
                if !p.eula {
                    return Err(ErrorInfo::EulaRequired);
                }
                ModpackTarget::Create { name: name.clone() }
            }
            ModpackTarget::Existing { entry } => {
                let record = server_for(&ctx, entry, Intent::Mutate)?;
                ModpackTarget::Existing {
                    entry: record.id.clone(),
                }
            }
        };
        let busy = target_name(&target);
        start(
            ctx.runtime.modpack_jobs().start(
                ModpackJob::InstallServer {
                    pack: p.pack,
                    target,
                    eula: p.eula,
                    port: p.port,
                },
                p.id,
            ),
            busy,
        )
    });
}

fn register_entry(on: &mut Channels<'_>) {
    on.handle::<EntryModpackUpdate, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Mutate)?;
        start(
            ctx.runtime.modpack_jobs().start(
                ModpackJob::Update {
                    entry: entry.handle,
                    to: p.to,
                },
                p.id,
            ),
            entry.name,
        )
    });

    on.handle::<EntryModpackPreviewUpdate, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        ctx.runtime
            .engine()
            .preview_entry_modpack_file(entry.handle.entry_ref(), Path::new(&p.path))
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<EntryModpackStatus, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let pack = ctx
            .runtime
            .engine()
            .entry_modpack(entry.handle.entry_ref())
            .map_err(crate::runtime::engine_error)?;
        Ok(ModpackStatusResult { pack })
    });

    on.handle::<EntryModpackCheckUpdate, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let update = ctx
            .runtime
            .engine()
            .entry_modpack_update(entry.handle.entry_ref(), &p.game_version)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ModpackUpdateResult { update })
    });

    on.handle::<EntryModpackRemove, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Mutate)?;
        ctx.runtime
            .engine()
            .remove_entry_modpack(entry.handle.entry_ref(), p.keep_content)
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

/// What a busy refusal names — the entry for an install into one, the pack
/// itself when the job would have created its own entry.
fn target_name(target: &ModpackTarget) -> String {
    match target {
        ModpackTarget::Create { name } => name.clone(),
        ModpackTarget::Existing { entry } => entry.clone(),
    }
}

fn start(started: Option<String>, name: String) -> Result<ModpackJobResult, ErrorInfo> {
    match started {
        Some(id) => Ok(ModpackJobResult { id }),
        None => Err(ErrorInfo::ContentInProgress { name }),
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
