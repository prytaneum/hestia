//! A server's game directory as a file tree. Every operation is filesystem
//! work of unbounded size — a world is copied, an archive unpacked — so each
//! runs off the async runtime.

use engine::FileTree;
use proto::error::{ErrorInfo, Field};
use proto::files::{
    FileListResult, FileReadResult, ServerFilesCompress, ServerFilesCopy, ServerFilesExtract,
    ServerFilesImport, ServerFilesList, ServerFilesMkdir, ServerFilesMove, ServerFilesRead,
    ServerFilesRemove, ServerFilesWrite,
};
use proto::Empty;

use super::guards::{server_for, Intent};
use crate::runtime::{Channels, HandlerContext};

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ServerFilesList, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Read)?;
        let entries = blocking(move || tree.list(&p.path)).await?;
        Ok(FileListResult { entries })
    });

    on.handle::<ServerFilesRead, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Read)?;
        let content = blocking(move || tree.read(&p.path)).await?;
        Ok(FileReadResult { content })
    });

    on.handle::<ServerFilesWrite, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
        blocking(move || tree.write(&p.path, &p.content)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesMkdir, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
        blocking(move || tree.mkdir(&p.path)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesMove, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
        blocking(move || tree.rename(&p.from, &p.to)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesCopy, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
        blocking(move || tree.copy(&p.from, &p.to)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesRemove, _, _>(|p, ctx| async move {
        require_some(&p.paths)?;
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
        blocking(move || tree.remove(&p.paths)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesImport, _, _>(|p, ctx| async move {
        require_some(&p.sources)?;
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
        blocking(move || tree.import(&p.path, &p.sources)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesCompress, _, _>(|p, ctx| async move {
        require_some(&p.paths)?;
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
        blocking(move || tree.compress(&p.paths, &p.to)).await?;
        Ok(Empty {})
    });

    on.handle::<ServerFilesExtract, _, _>(|p, ctx| async move {
        let tree = tree(&ctx, &p.server, Intent::Edit)?;
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
