//! Skin and cape management for a signed-in account — the desktop's skin
//! picker. Every change relays to Mojang with the account's token, so the
//! handlers stay thin over the engine's skin flows.

use proto::skins::{
    CapeClear, CapeEquip, SkinAdd, SkinEquip, SkinList, SkinListResult, SkinRemove, SkinReset,
    SkinUpdate,
};
use proto::Empty;

use proto::error::ErrorInfo;

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<SkinList, _, _>(|p, ctx| async move {
        let (skins, capes) = ctx
            .runtime
            .engine()
            .list_skins(&p.account)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(SkinListResult { skins, capes })
    });

    on.handle::<SkinAdd, _, _>(|p, ctx| async move {
        tracing::info!(name = %p.name, variant = ?p.variant, "skin upload started");
        let skin = ctx
            .runtime
            .engine()
            .add_skin(&p.account, &p.name, p.variant, &p.data)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(skin)
    });

    on.handle::<SkinEquip, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .equip_skin(&p.account, &p.key)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(Empty {})
    });

    on.handle::<SkinReset, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .reset_skin(&p.account)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(Empty {})
    });

    // No library pre-check: the flow adopts an equipped skin the library does
    // not yet record, which is how the current skin becomes editable.
    on.handle::<SkinUpdate, _, _>(|p, ctx| async move {
        let skin = ctx
            .runtime
            .engine()
            .update_skin(&p.account, &p.key, &p.name, p.variant)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(skin)
    });

    on.handle::<SkinRemove, _, _>(|p, ctx| async move {
        match ctx.runtime.engine().skins().remove(&p.key) {
            Ok(true) => Ok(Empty {}),
            Ok(false) => Err(ErrorInfo::SkinNotFound { key: p.key.clone() }),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });

    on.handle::<CapeEquip, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .equip_cape(&p.account, &p.cape)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(Empty {})
    });

    on.handle::<CapeClear, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .clear_cape(&p.account)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(Empty {})
    });
}
