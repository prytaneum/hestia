//! Third-party content: the source catalogue (search, project, versions) and
//! the per-entry install surface for servers and instances.

use proto::content::{
    ContentInspect, ContentJobResult, ContentListResult, ContentMatchesResult, ContentProjectGet,
    ContentResolveUrl, ContentSearch, ContentSources, ContentUpdatesResult, ContentVersions,
    ContentVersionsResult, InstanceContentAdd, InstanceContentCheckUpdates, InstanceContentEnable,
    InstanceContentList, InstanceContentMatches, InstanceContentRemove, InstanceContentSetVersion,
    InstanceContentUpdate, ServerContentAdd, ServerContentCheckUpdates, ServerContentEnable,
    ServerContentList, ServerContentMatches, ServerContentRemove, ServerContentSetVersion,
    ServerContentUpdate, SourcesResult,
};
use proto::error::{ErrorInfo, Field};
use proto::Empty;

use super::guards::{
    instance_for, require_content_items, require_item_names, require_one_for_file, server_for,
    Intent,
};
use crate::runtime::{Channels, ContentJob, JobEntry};

pub(super) fn register(on: &mut Channels<'_>) {
    register_sources(on);
    register_server(on);
    register_instance(on);
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

fn register_server(on: &mut Channels<'_>) {
    on.handle::<ServerContentAdd, _, _>(|p, ctx| async move {
        require_content_items(&p.spec)?;
        let record = server_for(&ctx, &p.server, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::Add {
                entry: JobEntry::server(record.id),
                spec: p.spec,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress {
                name: record.name.clone(),
            }),
        }
    });

    on.handle::<ServerContentList, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let (items, untracked) = ctx
            .runtime
            .engine()
            .entry_content(engine::EntryRef::Server(&record.id), p.kind)
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentListResult { items, untracked })
    });

    on.handle::<ServerContentRemove, _, _>(|p, ctx| async move {
        if p.items.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Item });
        }
        require_item_names(&p.items)?;
        let record = server_for(&ctx, &p.server, Intent::Mutate)?;
        ctx.runtime
            .engine()
            .remove_entry_content(
                engine::EntryRef::Server(&record.id),
                p.kind,
                &p.items,
                &p.worlds,
            )
            .map(|_| Empty {})
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<ServerContentUpdate, _, _>(|p, ctx| async move {
        require_item_names(&p.items)?;
        require_one_for_file(&p.items, &p.path)?;
        let record = server_for(&ctx, &p.server, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::Update {
                entry: JobEntry::server(record.id),
                kind: p.kind,
                items: p.items,
                path: p.path,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress {
                name: record.name.clone(),
            }),
        }
    });

    on.handle::<ServerContentEnable, _, _>(|p, ctx| async move {
        if p.item.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Item });
        }
        let record = server_for(&ctx, &p.server, Intent::Mutate)?;
        match ctx.runtime.engine().enable_entry_content(
            engine::EntryRef::Server(&record.id),
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

    on.handle::<ServerContentCheckUpdates, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let updates = ctx
            .runtime
            .engine()
            .check_entry_updates(engine::EntryRef::Server(&record.id), p.kind)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentUpdatesResult { updates })
    });

    on.handle::<ServerContentMatches, _, _>(|p, ctx| async move {
        let record = server_for(&ctx, &p.server, Intent::Read)?;
        let matches = ctx
            .runtime
            .engine()
            .entry_content_matches(engine::EntryRef::Server(&record.id), p.kind, &p.item)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentMatchesResult { matches })
    });

    on.handle::<ServerContentSetVersion, _, _>(|p, ctx| async move {
        if p.item.is_empty() || p.version.is_empty() {
            return Err(ErrorInfo::FieldsRequired {
                fields: vec![Field::Item, Field::Version],
            });
        }
        let record = server_for(&ctx, &p.server, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::SetVersion {
                entry: JobEntry::server(record.id),
                kind: p.kind,
                item: p.item,
                version: p.version,
                source: p.source,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress {
                name: record.name.clone(),
            }),
        }
    });
}

fn register_instance(on: &mut Channels<'_>) {
    on.handle::<InstanceContentAdd, _, _>(|p, ctx| async move {
        require_content_items(&p.spec)?;
        let record = instance_for(&ctx, &p.instance, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::Add {
                entry: JobEntry::instance(record.id),
                spec: p.spec,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress {
                name: record.name.clone(),
            }),
        }
    });

    on.handle::<InstanceContentList, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        let (items, untracked) = ctx
            .runtime
            .engine()
            .entry_content(engine::EntryRef::Instance(&record.id), p.kind)
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentListResult { items, untracked })
    });

    on.handle::<InstanceContentRemove, _, _>(|p, ctx| async move {
        if p.items.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Item });
        }
        require_item_names(&p.items)?;
        let record = instance_for(&ctx, &p.instance, Intent::Mutate)?;
        ctx.runtime
            .engine()
            .remove_entry_content(
                engine::EntryRef::Instance(&record.id),
                p.kind,
                &p.items,
                &p.worlds,
            )
            .map(|_| Empty {})
            .map_err(crate::runtime::engine_error)
    });

    on.handle::<InstanceContentUpdate, _, _>(|p, ctx| async move {
        require_item_names(&p.items)?;
        require_one_for_file(&p.items, &p.path)?;
        let record = instance_for(&ctx, &p.instance, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::Update {
                entry: JobEntry::instance(record.id),
                kind: p.kind,
                items: p.items,
                path: p.path,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress {
                name: record.name.clone(),
            }),
        }
    });

    on.handle::<InstanceContentEnable, _, _>(|p, ctx| async move {
        if p.item.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Item });
        }
        let record = instance_for(&ctx, &p.instance, Intent::Mutate)?;
        match ctx.runtime.engine().enable_entry_content(
            engine::EntryRef::Instance(&record.id),
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

    on.handle::<InstanceContentCheckUpdates, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        let updates = ctx
            .runtime
            .engine()
            .check_entry_updates(engine::EntryRef::Instance(&record.id), p.kind)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentUpdatesResult { updates })
    });

    on.handle::<InstanceContentMatches, _, _>(|p, ctx| async move {
        let record = instance_for(&ctx, &p.instance, Intent::Read)?;
        let matches = ctx
            .runtime
            .engine()
            .entry_content_matches(engine::EntryRef::Instance(&record.id), p.kind, &p.item)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(ContentMatchesResult { matches })
    });

    on.handle::<InstanceContentSetVersion, _, _>(|p, ctx| async move {
        if p.item.is_empty() || p.version.is_empty() {
            return Err(ErrorInfo::FieldsRequired {
                fields: vec![Field::Item, Field::Version],
            });
        }
        let record = instance_for(&ctx, &p.instance, Intent::Mutate)?;
        match ctx.runtime.content_jobs().start(
            ContentJob::SetVersion {
                entry: JobEntry::instance(record.id),
                kind: p.kind,
                item: p.item,
                version: p.version,
                source: p.source,
            },
            p.id,
        ) {
            Some(id) => Ok(ContentJobResult { id }),
            None => Err(ErrorInfo::ContentInProgress {
                name: record.name.clone(),
            }),
        }
    });
}
