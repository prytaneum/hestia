use ipc::errors::IpcError;

use crate::session::Session;

pub struct Cache<'a> {
    pub(crate) session: &'a Session,
}

impl Cache<'_> {
    pub async fn info(&self) -> Result<proto::cache::CacheInfoResult, IpcError> {
        self.session
            .call::<proto::cache::CacheInfo>(&proto::Empty {})
            .await
    }

    pub async fn list(&self) -> Result<Vec<proto::cache::CacheEntry>, IpcError> {
        self.session
            .call::<proto::cache::CacheList>(&proto::Empty {})
            .await
    }

    pub async fn clear(&self) -> Result<proto::cache::CacheUsage, IpcError> {
        self.session
            .call::<proto::cache::CacheClear>(&proto::Empty {})
            .await
    }
}
