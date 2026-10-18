//! Streams a URL to disk through a `.part` temp file, hashing incrementally when
//! a checksum is given and renaming into place only on success. With a cache, a
//! checksummed fetch is served from it when possible (re-verified on the way out)
//! and feeds it after a successful download.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use proto::download::{Checksum, DownloadProgress};
use tokio::io::AsyncWriteExt;

use crate::cache::Cache;
use crate::checksum::Hasher;
use crate::net;

pub type ProgressFn<'a> = dyn Fn(&DownloadProgress) -> Result<()> + Send + Sync + 'a;

pub struct Downloader<'a> {
    cache: Option<&'a Cache>,
}

impl<'a> Downloader<'a> {
    pub fn new(cache: Option<&'a Cache>) -> Self {
        Downloader { cache }
    }

    /// Fetch `url` to `destination`. Errors on a network error, a non-2xx status,
    /// or a checksum mismatch; the `.part` file is removed on every failure.
    pub async fn fetch(
        &self,
        url: &str,
        destination: &Path,
        checksum: Option<&Checksum>,
        on_progress: &ProgressFn<'_>,
    ) -> Result<()> {
        if url.is_empty() {
            bail!(proto::error::ErrorInfo::FieldRequired {
                field: proto::error::Field::Url
            });
        }
        if let Some(c) = checksum {
            if !c.is_valid() {
                bail!(
                    "invalid checksum '{}': expected {} hex characters",
                    c.hex,
                    c.algorithm.hex_digest_length()
                );
            }
        }
        if let Some(parent) = destination.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let part = common::atomic::staged(destination);
        let outcome = self.fill_part(url, &part, checksum, on_progress).await;
        let from_cache = match outcome {
            Ok(from_cache) => from_cache,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e);
            }
        };

        tokio::fs::rename(&part, destination)
            .await
            .with_context(|| format!("cannot move {} into place", part.display()))?;

        if from_cache {
            return Ok(());
        }
        if let (Some(cache), Some(checksum)) = (self.cache, checksum) {
            cache.store(destination, checksum);
        }
        tracing::debug!(url, dest = %destination.display(), "download complete");
        Ok(())
    }

    async fn fill_part(
        &self,
        url: &str,
        part: &Path,
        checksum: Option<&Checksum>,
        on_progress: &ProgressFn<'_>,
    ) -> Result<bool> {
        if let (Some(cache), Some(checksum)) = (self.cache, checksum) {
            if serve_from_cache(cache, part, checksum, on_progress).await? {
                tracing::debug!(hex = %checksum.hex, "cache hit");
                return Ok(true);
            }
        }
        tracing::debug!(url, "downloading");
        self.stream_to_file(url, part, checksum, on_progress)
            .await?;
        Ok(false)
    }

    async fn stream_to_file(
        &self,
        url: &str,
        part: &Path,
        checksum: Option<&Checksum>,
        on_progress: &ProgressFn<'_>,
    ) -> Result<()> {
        let response = net::send(None, net::client().get(url)).await?;
        if !response.status().is_success() {
            bail!(proto::error::ErrorInfo::DownloadFailed {
                detail: format!("{url}: HTTP {}", response.status().as_u16()),
            });
        }
        let total = response.content_length().unwrap_or(0);

        let mut sink = Sink::create(part, checksum, total).await?;
        let stream = response.bytes_stream();
        futures_util::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| net::stream_failure(None, &e))?;
            sink.push(&chunk, on_progress).await?;
        }

        if let (Some(actual), Some(checksum)) = (sink.finish().await?, checksum) {
            let expected = checksum.hex.to_lowercase();
            if actual != expected {
                bail!(proto::error::ErrorInfo::DownloadFailed {
                    detail: format!(
                        "checksum mismatch for {url}: expected {expected}, got {actual}"
                    )
                });
            }
        }
        Ok(())
    }
}

struct Sink {
    file: tokio::fs::File,
    hasher: Option<Hasher>,
    written: u64,
    total: u64,
}

impl Sink {
    async fn create(part: &Path, checksum: Option<&Checksum>, total: u64) -> Result<Self> {
        let file = tokio::fs::File::create(part)
            .await
            .with_context(|| format!("cannot open {} for writing", part.display()))?;
        Ok(Sink {
            file,
            hasher: checksum.map(|c| Hasher::new(c.algorithm)),
            written: 0,
            total,
        })
    }

    async fn push(&mut self, chunk: &[u8], on_progress: &ProgressFn<'_>) -> Result<()> {
        self.file.write_all(chunk).await?;
        if let Some(h) = self.hasher.as_mut() {
            h.update(chunk);
        }
        self.written += chunk.len() as u64;
        on_progress(&DownloadProgress {
            downloaded: self.written,
            total: self.total,
        })
    }

    async fn finish(mut self) -> Result<Option<String>> {
        self.file.flush().await?;
        Ok(self.hasher.map(|h| h.hex_digest()))
    }
}

/// Copy a cached blob into `part`, re-hashing on the way out. A blob that no
/// longer matches its key is evicted and the caller falls back to the network.
async fn serve_from_cache(
    cache: &Cache,
    part: &Path,
    checksum: &Checksum,
    on_progress: &ProgressFn<'_>,
) -> Result<bool> {
    let Some(blob) = cache.lookup(checksum) else {
        return Ok(false);
    };
    let total = std::fs::metadata(&blob).map(|m| m.len()).unwrap_or(0);

    let mut input = tokio::fs::File::open(&blob).await.map_err(|e| anyhow!(e))?;
    let mut sink = Sink::create(part, Some(checksum), total).await?;
    let mut buf = vec![0u8; 64 * 1024];

    use tokio::io::AsyncReadExt;
    loop {
        let n = input.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        sink.push(&buf[..n], on_progress).await?;
    }

    if sink.finish().await?.as_deref() != Some(checksum.hex.to_lowercase().as_str()) {
        tracing::warn!(hex = %checksum.hex, "cache blob is corrupt; evicting and refetching");
        cache.evict(checksum);
        let _ = tokio::fs::remove_file(part).await;
        return Ok(false);
    }
    Ok(true)
}
