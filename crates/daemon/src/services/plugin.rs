//! Installed plugins: what is loaded and in which state.

use proto::plugin::{PluginDisable, PluginEnable, PluginGet, PluginList, PluginRemove};
use proto::Empty;

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<PluginList, _, _>(
        |_: Empty, ctx| async move { Ok(ctx.runtime.plugins().list().await) },
    );

    on.handle::<PluginGet, _, _>(|p, ctx| async move { ctx.runtime.plugins().info(&p.id).await });

    on.handle::<PluginEnable, _, _>(
        |p, ctx| async move { ctx.runtime.plugins().enable(&p.id).await },
    );

    on.handle::<PluginDisable, _, _>(
        |p, ctx| async move { ctx.runtime.plugins().disable(&p.id).await },
    );

    on.handle::<PluginRemove, _, _>(|p, ctx| async move {
        ctx.runtime.plugins().remove(&p.id)?;
        Ok(Empty {})
    });
}
