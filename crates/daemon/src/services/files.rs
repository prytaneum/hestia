//! A server's game directory as a file tree. Every operation is filesystem
//! work of unbounded size — a world is copied, an archive unpacked — so each
//! runs off the async runtime.

use engine::FileTree;
use proto::error::{ErrorInfo, Field, FileFault};
use proto::files::{
    FileReadResult, ServerFilesCompress, ServerFilesCopy, ServerFilesExtract, ServerFilesImport,
    ServerFilesList, ServerFilesMkdir, ServerFilesMove, ServerFilesRead, ServerFilesRemove,
    ServerFilesWrite,
};
use proto::Empty;

use super::guards::{server_for, Intent};
use crate::runtime::{Caller, Channels, HandlerContext};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerFilesList, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Read)?;
        let entries = blocking(move || tree.list(&p.path)).await?;
        Ok(entries)
    });

    on.handle::<ServerFilesRead, _, _>(|p, ctx| async move {
        if !readable_by(&ctx, &p.path) {
            tracing::warn!(caller = %ctx.caller, path = %p.path, "plugin refused a file its grant does not list");
            return Err(ErrorInfo::FileRejected {
                path: p.path,
                reason: FileFault::OutsideEntry,
            });
        }
        let tree = tree(&ctx, &p.server, Intent::Read)?;
        let content = blocking(move || tree.read(&p.path)).await?;
        Ok(FileReadResult { content })
    });

    on.handle::<ServerFilesWrite, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.write(&p.path, &p.content)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesMkdir, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.mkdir(&p.path)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesMove, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.rename(&p.from, &p.to)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesCopy, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.copy(&p.from, &p.to)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesRemove, _, _>(|p, ctx| async move {
        require_some(&p.paths)?;
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.remove(&p.paths)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesImport, _, _>(|p, ctx| async move {
        require_some(&p.sources)?;
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.import(&p.path, &p.sources)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesCompress, _, _>(|p, ctx| async move {
        require_some(&p.paths)?;
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.compress(&p.paths, &p.to)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesExtract, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Idle)?;
        blocking(move || tree.extract(&p.from, &p.to)).await?;
        Ok(Empty {})
    });
}

fn tree(ctx: &HandlerContext, server: &str, intent: Intent) -> Result<FileTree, ErrorInfo> {
    let record = server_for(ctx, server, intent)?;
    Ok(FileTree::new(
        ctx.runtime.engine().servers().data_dir(&record),
    ))
}

/// A plugin reads only the files its grant lists, by exact path: the game
/// directory also holds `server.properties`, which carries the RCON password.
fn readable_by(ctx: &HandlerContext, path: &str) -> bool {
    match &ctx.caller {
        Caller::Client => true,
        Caller::Plugin(id) => ctx.runtime.plugins().grants_file(id, path),
    }
}

fn require_some(paths: &[String]) -> Result<(), ErrorInfo> {
    if paths.is_empty() {
        return Err(ErrorInfo::FieldRequired { field: Field::Path });
    }
    Ok(())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, ErrorInfo> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ErrorInfo::Internal {
            detail: format!("file task failed: {e}"),
        })?
        .map_err(crate::runtime::engine_error)
}
