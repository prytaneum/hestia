//! Third-party content: the source catalogue (search, project, versions) and
//! the per-entry install surface, one channel per verb for servers and
//! instances alike.

use proto::content::{
    ContentInspect, ContentJobResult, ContentListResult, ContentMatchesResult, ContentProjectGet,
    ContentResolveUrl, ContentSearch, ContentSources, ContentUpdatesResult, ContentVersions,
    ContentVersionsResult, EntryContentAdd, EntryContentCheckUpdates, EntryContentEnable,
    EntryContentList, EntryContentMatches, EntryContentRemove, EntryContentSetVersion,
    EntryContentUpdate, SourcesResult,
};
use proto::error::{ErrorInfo, Field};
use proto::Empty;

use super::guards::{
    entry_for, require_content_items, require_item_names, require_one_for_file, Intent,
};
use crate::runtime::{Channels, ContentJob};

pub(super) fn register(on: &mut Channels<'_>) {
    register_sources(on);
    register_entry(on);
}

fn register_sources(on: &mut Channels<'_>) {
    on.handle::<ContentSources, _, _>(|_: Empty, ctx| async move {
        Ok(SourcesResult {
            sources: ctx.runtime.engine().content().sources(),
        })
    });

    on.handle::<ContentSearch, _, _>(|q, ctx| async move {
        ctx.runtime
            .engine()
            .content()
            .search(&q)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ContentProjectGet, _, _>(|p, ctx| async move {
        if p.project.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: Field::Project,
            });
        }
        ctx.runtime
            .engine()
            .content()
            .project(&p.source, &p.project, p.kind)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ContentResolveUrl, _, _>(|p, ctx| async move {
        if p.url.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Url });
        }
        ctx.runtime
            .engine()
            .content()
            .resolve_url(&p.url)
            .await
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ContentVersions, _, _>(|q, ctx| async move {
        if q.project.is_empty() {
            return Err(ErrorInfo::FieldRequired {
                field: Field::Project,
            });
        }
        let versions = ctx
            .runtime
            .engine()
            .content()
            .versions(&q)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentVersionsResult { versions })
    });

    on.handle::<ContentInspect, _, _>(|p, ctx| async move {
        if p.path.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Path });
        }
        Ok(ctx.runtime.engine().content().inspect(&p.path).await)
    });
}

fn register_entry(on: &mut Channels<'_>) {
    on.handle::<EntryContentAdd, _, _>(|p, ctx| async move {
        require_content_items(&p.spec)?;
        let entry = entry_for(&ctx, &p.target, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::Add {
                entry: entry.handle,
                spec: p.spec,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress { name: entry.name }),
        }
    });

    on.handle::<EntryContentList, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let (items, untracked) = ctx
            .runtime
            .engine()
            .entry_content(entry.handle.entry_ref(), p.kind)
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentListResult { items, untracked })
    });

    on.handle::<EntryContentRemove, _, _>(|p, ctx| async move {
        if p.items.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Item });
        }
        require_item_names(&p.items)?;
        let entry = entry_for(&ctx, &p.target, Intent::Mutate)?;
        ctx.runtime
            .engine()
            .remove_entry_content(entry.handle.entry_ref(), p.kind, &p.items, &p.worlds)
            .map(|_| Empty {})
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<EntryContentUpdate, _, _>(|p, ctx| async move {
        require_item_names(&p.items)?;
        require_one_for_file(&p.items, &p.path)?;
        let entry = entry_for(&ctx, &p.target, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::Update {
                entry: entry.handle,
                kind: p.kind,
                items: p.items,
                path: p.path,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress { name: entry.name }),
        }
    });

    on.handle::<EntryContentEnable, _, _>(|p, ctx| async move {
        if p.item.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Item });
        }
        let entry = entry_for(&ctx, &p.target, Intent::Mutate)?;
        match ctx.runtime.engine().enable_entry_content(
            entry.handle.entry_ref(),
            p.kind,
            &p.item,
            p.enabled,
            &p.worlds,
        ) {
            Ok(0) => Err(ErrorInfo::ContentNotFound {
                reference: p.item.clone(),
            }),
            Ok(_) => Ok(Empty {}),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });

    on.handle::<EntryContentCheckUpdates, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let updates = ctx
            .runtime
            .engine()
            .check_entry_updates(entry.handle.entry_ref(), p.kind)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentUpdatesResult { updates })
    });

    on.handle::<EntryContentMatches, _, _>(|p, ctx| async move {
        let entry = entry_for(&ctx, &p.target, Intent::Read)?;
        let matches = ctx
            .runtime
            .engine()
            .entry_content_matches(entry.handle.entry_ref(), p.kind, &p.item)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentMatchesResult { matches })
    });

    on.handle::<EntryContentSetVersion, _, _>(|p, ctx| async move {
        if p.item.is_empty() || p.version.is_empty() {
            return Err(ErrorInfo::FieldsRequired {
                fields: vec![Field::Item, Field::Version],
            });
        }
        let entry = entry_for(&ctx, &p.target, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::SetVersion {
                entry: entry.handle,
                kind: p.kind,
                item: p.item,
                version: p.version,
                source: p.source,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress { name: entry.name }),
        }
    });
}
