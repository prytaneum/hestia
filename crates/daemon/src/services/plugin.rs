//! Installed plugins: what is loaded and in which state.

use proto::error::{EntryKind, ErrorInfo};
use proto::job::{JobStarted, JobSubject};
use proto::plugin::{
    CommandScope, PluginAsset, PluginDisable, PluginEnable, PluginGet, PluginInspect,
    PluginInstall, PluginList, PluginRemove, PluginRun,
};
use proto::Empty;

use super::guards::{instance_for, server_for, Intent};

use crate::runtime::Channels;
use crate::runtime::Package;

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

    on.handle::<PluginInspect, _, _>(|p, ctx| async move {
        let package = Package::fetch(ctx.runtime.engine(), &p).await?;
        ctx.runtime.plugins().inspect(&package).await
    });

    on.handle::<PluginInstall, _, _>(|p, ctx| async move {
        let package = Package::held(ctx.runtime.engine(), &p)?;
        let plugin = ctx
            .runtime
            .engine()
            .plugins()
            .inspect(package.path())
            .map_err(crate::runtime::engine_error)?
            .manifest
            .id;
        match ctx.runtime.jobs().start_plugin_install(
            ctx.runtime.plugins().clone(),
            &plugin,
            p.id,
            package,
            p.approve,
            ctx.chain.clone(),
        ) {
            Some(id) => Ok(JobStarted { id }),
            None => Err(ErrorInfo::PluginInstallInProgress { id: plugin }),
        }
    });

    on.handle::<PluginAsset, _, _>(|p, ctx| async move {
        if !ctx.runtime.plugins().is_live(&p.plugin) {
            return Err(ErrorInfo::PluginUnavailable { id: p.plugin });
        }
        let runtime = ctx.runtime.clone();
        tokio::task::spawn_blocking(move || runtime.engine().plugins().asset(&p.plugin, &p.path))
            .await
            .map_err(|e| crate::runtime::engine_error(anyhow::anyhow!("{e}")))?
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<PluginRun, _, _>(|p, ctx| async move {
        let plugins = ctx.runtime.plugins();
        let (command, loaded) = plugins.command(&p.plugin, &p.command).await?;
        let scoped = match (command.scope, p.entry.as_deref()) {
            (CommandScope::Server, Some(reference)) => {
                let record = server_for(&ctx, reference, Intent::Idle)?;
                Some((EntryKind::Server, record.id))
            }
            (CommandScope::Instance, Some(reference)) => {
                let record = instance_for(&ctx, reference, Intent::Idle)?;
                Some((EntryKind::Instance, record.id))
            }
            _ => None,
        };
        let args = engine::command_args(
            &p.plugin,
            &command,
            scoped
                .as_ref()
                .map(|(_, id)| id.as_str())
                .or(p.entry.as_deref()),
            &p.args,
        )?;
        let subject = scoped.map(|(kind, id)| JobSubject { kind, id });
        let id = ctx.runtime.jobs().start_plugin_run(
            loaded,
            p.command,
            args,
            p.id,
            subject,
            ctx.chain.clone(),
        );
        Ok(JobStarted { id })
    });
}
