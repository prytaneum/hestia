//! The plugin registry.

use proto::error::ErrorInfo;
use proto::plugin::{PluginDisable, PluginEnable, PluginList};
use proto::Empty;

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<PluginList, _, _>(|_: Empty, ctx| async move { Ok(ctx.runtime.plugins().list()) });

    on.handle::<PluginEnable, _, _>(|p, ctx| async move {
        ctx.runtime.plugins().enable(&p.id).map_err(ErrorInfo::from)
    });

    on.handle::<PluginDisable, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .disable(&p.id)
            .map_err(ErrorInfo::from)
    });
}
