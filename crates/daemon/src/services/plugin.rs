//! Installed plugins: what is loaded and in which state.

use engine::CallError;
use proto::catalogue::{
    PluginCatalogue, PluginRepoAdd, PluginRepoList, PluginRepoRefresh, PluginRepoRemove,
};
use proto::error::{EntryKind, ErrorInfo};
use proto::job::{JobStarted, JobSubject};
use proto::plugin::{
    CommandScope, PageContext, PageContribution, PageSlot, PluginAsset, PluginDisable,
    PluginEnable, PluginGet, PluginInspect, PluginInstall, PluginList, PluginPages, PluginRemove,
    PluginRun, PluginSettingsGet, PluginSettingsSet, PluginThemes, PluginUiAction, PluginUiRender,
};
use proto::Empty;

use super::guards::{instance_for, server_for, Intent};

use crate::runtime::Channels;
use crate::runtime::HandlerContext;
use crate::runtime::Package;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<PluginList, _, _>(
        |_: Empty, ctx| async move { Ok(ctx.runtime.plugins().list().await) },
    );

    on.handle::<PluginGet, _, _>(|p, ctx| async move { ctx.runtime.plugins().info(&p.id).await });

    on.handle::<PluginSettingsGet, _, _>(|p, ctx| async move {
        ctx.runtime.plugins().settings(&p.id).await
    });

    on.handle::<PluginSettingsSet, _, _>(|p, ctx| async move {
        ctx.runtime
            .plugins()
            .set_setting(&p.id, &p.key, p.value, &ctx.chain)
            .await?;
        Ok(Empty {})
    });

    on.handle::<PluginEnable, _, _>(
        |p, ctx| async move { ctx.runtime.plugins().enable(&p.id).await },
    );

    on.handle::<PluginDisable, _, _>(
        |p, ctx| async move { ctx.runtime.plugins().disable(&p.id).await },
    );

    on.handle::<PluginRemove, _, _>(|p, ctx| async move {
        ctx.runtime.plugins().remove(&p.id).await?;
        Ok(Empty {})
    });

    on.handle::<PluginRepoAdd, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .add_repository(&p.url, p.confirm)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<PluginRepoList, _, _>(|_: Empty, ctx| async move {
        Ok(ctx.runtime.engine().repositories())
    });

    on.handle::<PluginRepoRemove, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .remove_repository(&p.url)
            .map_err(crate::runtime::engine_error)?;
        Ok(Empty {})
    });

    on.handle::<PluginRepoRefresh, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .refresh_repositories(p.url.as_deref())
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<PluginCatalogue, _, _>(|p, ctx| async move {
        ctx.runtime
            .engine()
            .catalogue(&p)
            .await
            .map_err(crate::runtime::engine_error)
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

    on.handle::<PluginPages, _, _>(|_: Empty, ctx| async move {
        Ok(ctx.runtime.plugins().pages().await)
    });

    on.handle::<PluginThemes, _, _>(|_: Empty, ctx| async move {
        Ok(ctx.runtime.plugins().themes().await)
    });

    on.handle::<PluginUiRender, _, _>(|p, ctx| async move {
        let (page, loaded) = ctx.runtime.plugins().page(&p.plugin, &p.page).await?;
        let context = page_context(&ctx, &p.plugin, &page, p.context)?;
        loaded
            .render_page(&ctx.chain, &p.page, &context)
            .await
            .map_err(|error| ui_error(&p.plugin, error))
    });

    on.handle::<PluginUiAction, _, _>(|p, ctx| async move {
        let (page, loaded) = ctx.runtime.plugins().page(&p.plugin, &p.page).await?;
        let context = page_context(&ctx, &p.plugin, &page, p.context)?;
        loaded
            .page_action(&ctx.chain, &p.page, &p.action, &p.values, &context)
            .await
            .map_err(|error| ui_error(&p.plugin, error))
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

/// An `entry` page receives the id of the entry it sits on, resolved here so a
/// plugin never sees a name that could mean another entry; any other page gets none.
fn page_context(
    ctx: &HandlerContext,
    plugin: &str,
    page: &PageContribution,
    context: PageContext,
) -> Result<PageContext, ErrorInfo> {
    if page.slot != PageSlot::Entry {
        return Ok(PageContext::default());
    }
    let reference = context.entry.ok_or_else(|| ErrorInfo::PluginPageInvalid {
        plugin: plugin.to_string(),
        page: page.id.clone(),
        detail: "an entry page needs the entry it sits on".into(),
    })?;
    let id = match page.kind.unwrap_or_default() {
        EntryKind::Server => server_for(ctx, &reference, Intent::Read)?.id,
        EntryKind::Instance => instance_for(ctx, &reference, Intent::Read)?.id,
    };
    Ok(PageContext { entry: Some(id) })
}

fn ui_error(plugin: &str, error: CallError) -> ErrorInfo {
    match error {
        CallError::InvalidUi(page) => ErrorInfo::PluginUiInvalid {
            plugin: plugin.to_string(),
            page,
        },
        other => ErrorInfo::PluginCallFailed {
            id: plugin.to_string(),
            detail: other.to_string(),
        },
    }
}
