//! Streams a URL to disk through a `.part` temp file, hashing incrementally when
//! a checksum is given and renaming into place only on success. With a cache, a
//! checksummed fetch is served from it when possible (re-verified on the way out)
//! and feeds it after a successful download.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use kernel::vetting::{Limits, Policy};
use proto::download::{Checksum, DownloadProgress};
use tokio::io::AsyncWriteExt;

use crate::cache::Cache;
use crate::checksum::Hasher;
use crate::net;

/// Why a transfer stopped, kept apart from the message so a caller that has to
/// answer typed (a plugin) can tell a missing file from a refusal. Always
/// attached to the [`ErrorInfo`](proto::error::ErrorInfo) of the same text, so
/// nothing that reads the failure that way changes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Fault {
    #[error("{detail}")]
    Status { status: u16, detail: String },
    #[error("{detail}")]
    TooLarge { detail: String },
}

fn typed(fault: Fault) -> anyhow::Error {
    let detail = fault.to_string();
    anyhow::Error::new(proto::error::ErrorInfo::DownloadFailed { detail }).context(fault)
}

pub type ProgressFn<'a> = dyn Fn(&DownloadProgress) -> Result<()> + Send + Sync + 'a;

/// The policy an artifact is fetched under, by its URL.
pub type PolicyFor<'a> = &'a (dyn Fn(&str) -> Arc<Policy> + Send + Sync);

/// Fetches under `policy`: the hosts, the size cap and the redirect rules are its.
pub struct Downloader<'a> {
    cache: Option<&'a Cache>,
    policy: Arc<Policy>,
}

impl<'a> Downloader<'a> {
    pub fn new(cache: Option<&'a Cache>, policy: Arc<Policy>) -> Self {
        Downloader { cache, policy }
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
        let _ = tokio::fs::remove_file(&part).await;
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
        let response = net::fetch(
            &self.policy,
            None,
            url,
            checksum.is_none(),
            Limits::DOWNLOAD,
        )
        .await?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Err(typed(Fault::Status {
                status,
                detail: format!("{url}: HTTP {status}"),
            }));
        }
        let total = response.content_length().unwrap_or(0);
        self.ensure_within_limit(url, total)?;

        let mut sink = Sink::create(part, checksum, total).await?;
        let stream = response.bytes_stream();
        futures_util::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| net::stream_failure(None, &e))?;
            sink.push(&chunk, on_progress).await?;
            self.ensure_within_limit(url, sink.written)?;
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

    fn ensure_within_limit(&self, url: &str, size: u64) -> Result<()> {
        let limit = Limits::DOWNLOAD.bytes;
        if size > limit {
            return Err(typed(Fault::TooLarge {
                detail: format!("{url}: larger than {limit} bytes"),
            }));
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
        // Exclusive, so a symlink a plugin planted at `part` is refused, not followed.
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(part)
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

#[cfg(test)]
mod tests {
    use kernel::vetting::Hosts;
    use proto::download::HashAlgorithm;
    use proto::error::ErrorInfo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const BODY: &[u8] = b"jar bytes";

    async fn serve() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0u8; 1024];
                let _ = stream.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    BODY.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(BODY).await;
            }
        });
        port
    }

    fn policy(unhashed: bool) -> Arc<Policy> {
        let hosts = Hosts::new::<&str>(&[], &[]).unwrap();
        Arc::new(Policy::new(hosts.with_loopback(unhashed)))
    }

    fn sha1(bytes: &[u8]) -> Checksum {
        let mut hasher = Hasher::new(HashAlgorithm::Sha1);
        hasher.update(bytes);
        Checksum {
            algorithm: HashAlgorithm::Sha1,
            hex: hasher.hex_digest(),
        }
    }

    fn detail(error: anyhow::Error) -> String {
        match error.downcast::<ErrorInfo>().expect("a typed error") {
            ErrorInfo::DownloadFailed { detail } => detail,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_download_is_vetted_and_verified_before_it_is_kept() {
        let port = serve().await;
        let url = format!("http://127.0.0.1:{port}/a.jar");
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.jar");
        let progress: &ProgressFn<'_> = &|_| Ok(());

        Downloader::new(None, policy(false))
            .fetch(&url, &target, Some(&sha1(BODY)), progress)
            .await
            .expect("a hashed file from a granted host");
        assert_eq!(std::fs::read(&target).unwrap(), BODY);

        let other = dir.path().join("b.jar");
        let mismatch = Downloader::new(None, policy(false))
            .fetch(&url, &other, Some(&sha1(b"other")), progress)
            .await
            .expect_err("checksum mismatch");
        assert!(detail(mismatch).contains("checksum mismatch"));
        assert!(!other.exists() && !common::atomic::staged(&other).exists());

        let unhashed = Downloader::new(None, policy(false))
            .fetch(&url, &other, None, progress)
            .await
            .expect_err("a host not declared unhashed");
        assert!(detail(unhashed).contains("unhashed"));

        Downloader::new(None, policy(true))
            .fetch(&url, &other, None, progress)
            .await
            .expect("a declared unhashed host");

        let core = Downloader::new(None, crate::net::hosts::mojang())
            .fetch(&url, &dir.path().join("c.jar"), Some(&sha1(BODY)), progress)
            .await
            .expect_err("a core policy never reaches loopback");
        assert!(detail(core).contains("only https"));
    }
}
