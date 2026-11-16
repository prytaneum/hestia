//! The plugin registry.

use std::path::Path;

use proto::error::ErrorInfo;
use proto::job::JobStarted;
use proto::plugin::{
    PluginAnswer, PluginDisable, PluginEnable, PluginInspect, PluginInspectRelease, PluginInstall,
    PluginInstallRelease, PluginList, PluginRemove, PluginRepoAdd, PluginRepoList,
    PluginRepoRemove, PluginRun, PluginSecretSet, PluginUpdateApply, PluginUpdateInspect,
    PluginUpdates,
};
use proto::Empty;

use crate::runtime::{Caller, Channels};

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

    on.handle::<PluginRun, _, _>(|p, ctx| async move {
        let person = matches!(ctx.caller, Caller::Client);
        let id = ctx
            .runtime
            .start_plugin_command(&p.plugin, &p.command, &p.args, person, p.id)?;
        Ok(JobStarted { id })
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

    on.handle::<PluginRepoList, _, _>(|_: Empty, ctx| async move {
        Ok(ctx.runtime.plugins().repositories())
    });

    on.handle::<PluginRepoAdd, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .add_repository(&p.id, &p.url, &p.key)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginRepoRemove, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .remove_repository(&p.id)
            .await
            .map(|()| Empty {})
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginInspectRelease, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .inspect_release(&p.repository, &p.id, p.version.as_deref())
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginInstallRelease, _, _>(|p, ctx| async move {
        let release = p.release;
        ctx.runtime
            .plugins()
            .install_release(
                &release.repository,
                &release.id,
                release.version.as_deref(),
                &p.approval_digest,
            )
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginUpdates, _, _>(|_: Empty, ctx| async move {
        Ok(ctx.runtime.plugins().check_updates().await)
    });

    on.handle::<PluginUpdateInspect, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .inspect_update(&p.id)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginUpdateApply, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .update(&p.id, &p.approval_digest)
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
