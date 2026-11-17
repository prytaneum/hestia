//! The plugin registry.

use std::path::Path;

use proto::error::ErrorInfo;
use proto::job::JobStarted;
use proto::plugin::{
    Choice, PluginAnswer, PluginDisable, PluginEnable, PluginEntries, PluginInspect,
    PluginInspectRelease, PluginInstall, PluginInstallRelease, PluginList, PluginPermission,
    PluginRemove, PluginRepoAdd, PluginRepoList, PluginRepoRemove, PluginRun, PluginSecretSet,
    PluginUpdateApply, PluginUpdateInspect, PluginUpdates,
};
use proto::Empty;

use crate::runtime::entries::resolve_scope;
use crate::runtime::{Caller, Channels, HandlerContext};

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

/// The choice with the entries it names resolved to stable ids.
fn resolved(ctx: &HandlerContext, mut choice: Choice) -> Result<Choice, ErrorInfo> {
    choice.entries = resolve_scope(ctx.runtime.engine(), choice.entries)?;
    Ok(choice)
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
            .disable(&p.id, p.cascade)
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
        let choice = resolved(&ctx, p.choice)?;
        ctx.runtime
            .plugins()
            .install(dir, &p.approval_digest, choice)
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
        let choice = resolved(&ctx, p.choice)?;
        ctx.runtime
            .plugins()
            .install_release(
                &release.repository,
                &release.id,
                release.version.as_deref(),
                &p.approval_digest,
                choice,
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
        let scope = p
            .entries
            .map(|scope| resolve_scope(ctx.runtime.engine(), scope))
            .transpose()?;
        ctx.runtime
            .plugins()
            .update(&p.id, &p.approval_digest, scope)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginPermission, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .set_permission(&p.id, &p.permission, p.granted)
            .await
            .map_err(ErrorInfo::from)
    });

    on.handle::<PluginEntries, _, _>(|p, ctx| async move {
        let scope = resolve_scope(ctx.runtime.engine(), p.entries)?;
        ctx.runtime
            .plugins()
            .set_entries(&p.id, scope)
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
