//! The plugin registry.

use std::path::Path;

use proto::error::ErrorInfo;
use proto::plugin::{
    PluginAnswer, PluginDisable, PluginEnable, PluginInspect, PluginInstall, PluginList,
    PluginRemove, PluginSecretSet,
};
use proto::Empty;

use crate::runtime::Channels;

/// A relative path would resolve against the daemon's working directory, which
/// is not the client's.
fn package_dir(path: &str) -> Result<&Path, ErrorInfo> {
    let dir = Path::new(path);
    match dir.is_absolute() {
        true => Ok(dir),
        false => Err(ErrorInfo::MalformedRequest {
            detail: "the package path must be absolute".into(),
        }),
    }
}

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<PluginList, _, _>(|_: Empty, ctx| async move { Ok(ctx.runtime.plugins().list()) });

    on.handle::<PluginAnswer, _, _>(|p, ctx| async move {
        ctx.runtime
            .surface()
            .answer(&p.request, &p.plugin, p.accepted);
        Ok(Empty {})
    });

    on.handle::<PluginEnable, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .enable(&p.id)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginDisable, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .disable(&p.id)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginInspect, _, _>(|p, ctx| async move {
        let dir = package_dir(&p.path)?;
        ctx.runtime
            .plugins()
            .inspect(dir)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginInstall, _, _>(|p, ctx| async move {
        let dir = package_dir(&p.path)?;
        ctx.runtime
            .plugins()
            .install(dir, &p.approval_digest)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginRemove, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .remove(&p.id)
            .await
            .map(|()| Empty {})
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginSecretSet, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .store_secret(&p.id, &p.key, &p.value)
            .map(|()| Empty {})
            .map_err(ErrorInfo::from)
    });
}
