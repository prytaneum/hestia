//! The `sync.*` channels: the catalogue, what can seed it, the shared options,
//! and each instance's standing. The reconcile itself runs in the launch flow.

use proto::sync::{
    InstanceSyncKeys, InstanceSyncKeysParams, InstanceSyncUnit, InstanceSyncUnitParams,
    SyncDisable, SyncDisableParams, SyncEnable, SyncEnableParams, SyncGet, SyncKeys,
    SyncKeysParams, SyncOptionSet, SyncOptionSetParams, SyncOptionsGet, SyncOptionsResult,
    SyncPackRemove, SyncPackRemoveParams, SyncPackSet, SyncPackSetParams, SyncPacks,
    SyncPacksResult, SyncSources, SyncSourcesParams, SyncStatus,
};
use proto::Empty;

use super::guards::{instance_held, Hold};
use crate::runtime::{Channels, JobKind};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<SyncGet, _, _>(
        |_: Empty, ctx| async move { Ok(ctx.runtime.engine().sync_config()) },
    );

    on.handle::<SyncSources, _, _>(|p: SyncSourcesParams, ctx| async move {
        Ok(ctx.runtime.engine().sync_sources(p.unit))
    });

    on.handle::<SyncEnable, _, _>(|p: SyncEnableParams, ctx| async move {
        ctx.runtime
            .engine()
            .enable_sync_unit(p.unit, &p.source)
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<SyncDisable, _, _>(|p: SyncDisableParams, ctx| async move {
        ctx.runtime
            .engine()
            .disable_sync_unit(p.unit)
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<SyncKeys, _, _>(|p: SyncKeysParams, ctx| async move {
        ctx.runtime
            .engine()
            .set_sync_unsynced(p.unsynced)
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<SyncPacks, _, _>(|_: Empty, ctx| async move {
        Ok(SyncPacksResult {
            packs: ctx.runtime.engine().shared_packs(),
        })
    });

    on.handle::<SyncPackSet, _, _>(|p: SyncPackSetParams, ctx| async move {
        let packs = ctx
            .runtime
            .engine()
            .set_shared_pack(&p.pack, p.enabled)
            .map_err(crate::runtime::engine_error)?;
        Ok(SyncPacksResult { packs })
    });

    on.handle::<SyncPackRemove, _, _>(|p: SyncPackRemoveParams, ctx| async move {
        let packs = ctx
            .runtime
            .engine()
            .remove_shared_pack(&p.pack)
            .map_err(crate::runtime::engine_error)?;
        Ok(SyncPacksResult { packs })
    });

    on.handle::<SyncOptionsGet, _, _>(|_: Empty, ctx| async move {
        Ok(SyncOptionsResult {
            options: ctx.runtime.engine().sync_options(),
        })
    });

    on.handle::<SyncOptionSet, _, _>(|p: SyncOptionSetParams, ctx| async move {
        let options = ctx
            .runtime
            .engine()
            .set_sync_option(&p.key, &p.value)
            .map_err(crate::runtime::engine_error)?;
        Ok(SyncOptionsResult { options })
    });

    on.handle::<SyncStatus, _, _>(
        |_: Empty, ctx| async move { Ok(ctx.runtime.engine().sync_status()) },
    );

    on.handle::<InstanceSyncUnit, _, _>(|p: InstanceSyncUnitParams, ctx| async move {
        let (record, _claim) = instance_held(&ctx, &p.instance, Hold::stopped(JobKind::Content))?;
        ctx.runtime
            .engine()
            .set_instance_sync_unit(&record.id, p.unit, p.shared)
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<InstanceSyncKeys, _, _>(|p: InstanceSyncKeysParams, ctx| async move {
        let (record, _claim) = instance_held(&ctx, &p.instance, Hold::stopped(JobKind::Content))?;
        ctx.runtime
            .engine()
            .set_instance_sync_keys(&record.id, p.unsynced)
            .map_err(crate::runtime::engine_error)
    });
}
