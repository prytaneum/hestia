//! `server.files.*` — a server's game directory as a file tree.

use std::time::Duration;

use proto::files::{
    FileEntry, ServerFileParams, ServerFileTransferParams, ServerFileWriteParams,
    ServerFilesCompress, ServerFilesCompressParams, ServerFilesCopy, ServerFilesExtract,
    ServerFilesImport, ServerFilesImportParams, ServerFilesList, ServerFilesMkdir, ServerFilesMove,
    ServerFilesRead, ServerFilesRemove, ServerFilesRemoveParams, ServerFilesWrite,
};

use crate::{IpcError, Session};

/// A world is copied or archived inside one call, so anything moving whole
/// trees waits far longer than a plain request.
const TREE_TIMEOUT: Duration = Duration::from_secs(600);

pub struct Files<'a> {
    pub(crate) session: &'a Session,
}

impl Files<'_> {
    pub async fn list(&self, server: &str, path: &str) -> Result<Vec<FileEntry>, IpcError> {
        Ok(self
            .session
            .call::<ServerFilesList>(&at(server, path))
            .await?
            .entries)
    }

    pub async fn read(&self, server: &str, path: &str) -> Result<String, IpcError> {
        Ok(self
            .session
            .call::<ServerFilesRead>(&at(server, path))
            .await?
            .content)
    }

    pub async fn write(&self, server: &str, path: &str, content: &str) -> Result<(), IpcError> {
        let params = ServerFileWriteParams {
            server: server.to_string(),
            path: path.to_string(),
            content: content.to_string(),
        };
        self.session.call::<ServerFilesWrite>(&params).await?;
        Ok(())
    }

    pub async fn mkdir(&self, server: &str, path: &str) -> Result<(), IpcError> {
        self.session
            .call::<ServerFilesMkdir>(&at(server, path))
            .await?;
        Ok(())
    }

    pub async fn rename(&self, server: &str, from: &str, to: &str) -> Result<(), IpcError> {
        self.session
            .call_with_timeout::<ServerFilesMove>(&between(server, from, to), TREE_TIMEOUT)
            .await?;
        Ok(())
    }

    pub async fn copy(&self, server: &str, from: &str, to: &str) -> Result<(), IpcError> {
        self.session
            .call_with_timeout::<ServerFilesCopy>(&between(server, from, to), TREE_TIMEOUT)
            .await?;
        Ok(())
    }

    pub async fn remove(&self, server: &str, paths: &[String]) -> Result<(), IpcError> {
        let params = ServerFilesRemoveParams {
            server: server.to_string(),
            paths: paths.to_vec(),
        };
        self.session
            .call_with_timeout::<ServerFilesRemove>(&params, TREE_TIMEOUT)
            .await?;
        Ok(())
    }

    /// Copy `sources` — absolute paths on the daemon's machine — into the
    /// directory at `path`.
    pub async fn import(
        &self,
        server: &str,
        path: &str,
        sources: &[String],
    ) -> Result<(), IpcError> {
        let params = ServerFilesImportParams {
            server: server.to_string(),
            path: path.to_string(),
            sources: sources.to_vec(),
        };
        self.session
            .call_with_timeout::<ServerFilesImport>(&params, TREE_TIMEOUT)
            .await?;
        Ok(())
    }

    pub async fn compress(&self, server: &str, paths: &[String], to: &str) -> Result<(), IpcError> {
        let params = ServerFilesCompressParams {
            server: server.to_string(),
            paths: paths.to_vec(),
            to: to.to_string(),
        };
        self.session
            .call_with_timeout::<ServerFilesCompress>(&params, TREE_TIMEOUT)
            .await?;
        Ok(())
    }

    pub async fn extract(&self, server: &str, from: &str, to: &str) -> Result<(), IpcError> {
        self.session
            .call_with_timeout::<ServerFilesExtract>(&between(server, from, to), TREE_TIMEOUT)
            .await?;
        Ok(())
    }
}

fn at(server: &str, path: &str) -> ServerFileParams {
    ServerFileParams {
        server: server.to_string(),
        path: path.to_string(),
    }
}

fn between(server: &str, from: &str, to: &str) -> ServerFileTransferParams {
    ServerFileTransferParams {
        server: server.to_string(),
        from: from.to_string(),
        to: to.to_string(),
    }
}
