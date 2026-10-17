//! The content-addressed download cache.

use proto::cache::{
    CacheClear, CacheEntry, CacheInfo, CacheInfoResult, CacheList, CacheListResult, CacheUsage,
};
use proto::Empty;

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<CacheInfo, _, _>(|_: Empty, ctx| async move {
        let cache = ctx.runtime.engine().cache();
        let usage = cache.usage();
        Ok(CacheInfoResult {
            path: cache.dir().to_path_buf(),
            usage: CacheUsage {
                entries: usage.entries,
                bytes: usage.bytes,
            },
        })
    });

    on.handle::<CacheList, _, _>(|_: Empty, ctx| async move {
        let entries = ctx
            .runtime
            .engine()
            .cache()
            .entries()
            .into_iter()
            .map(|e| CacheEntry {
                checksum: e.checksum,
                size: e.size,
            })
            .collect();
        Ok(CacheListResult { entries })
    });

    on.handle::<CacheClear, _, _>(|_: Empty, ctx| async move {
        let freed = ctx.runtime.engine().cache().clear();
        tracing::info!(
            entries = freed.entries,
            bytes = freed.bytes,
            "cache cleared"
        );
        Ok(CacheUsage {
            entries: freed.entries,
            bytes: freed.bytes,
        })
    });
}
