//! Remote images for the webview, fetched here so the webview never contacts a
//! host itself (and so a strict `img-src` can stay strict).
//!
//! The URL comes from untrusted content, so every fetch goes through the
//! guarded client and its own checks: https only, a host name rather than an
//! address, no credentials, at most [`MAX_REDIRECTS`] hops each re-checked, a
//! size cap while streaming, and a body that must sniff as an image. What
//! arrives is stored in the content-addressed cache by its SHA-256, with a small
//! URL index beside it, so a repeat view — or an offline one — is served from
//! disk.

mod index;
mod sniff;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use proto::download::{Checksum, HashAlgorithm};
use proto::error::ErrorInfo;
use reqwest::Url;
use sha2::{Digest, Sha256};

use super::{guard, network, stream_failure};
use crate::cache::{Cache, IMAGE_INDEX};
use index::{Entry, Index};

const MAX_BYTES: usize = 10 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct Image {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub struct ImageProxy {
    client: reqwest::Client,
    cache: Cache,
    index: Index,
    #[cfg(test)]
    local: Option<std::net::SocketAddr>,
}

impl ImageProxy {
    pub fn new(cache_dir: PathBuf) -> Self {
        ImageProxy {
            client: guard::client().clone(),
            index: Index::new(cache_dir.join(IMAGE_INDEX)),
            cache: Cache::new(cache_dir),
            #[cfg(test)]
            local: None,
        }
    }

    /// The image at `url`: from the cache when it was fetched before, otherwise
    /// from the network. A cached copy is served even when offline.
    pub async fn fetch(&self, url: &str) -> Result<Image> {
        let mut target = Url::parse(url).map_err(|_| refused("not a valid URL"))?;
        target.set_fragment(None);
        self.check_target(&target)?;

        if let Some(hit) = self.cached(target.as_str()) {
            tracing::debug!(host = target.host_str(), "image served from cache");
            return Ok(hit);
        }
        if network().pinned() {
            return Err(ErrorInfo::OfflineMode.into());
        }

        let host = target.host_str().unwrap_or_default().to_string();
        let image = tokio::time::timeout(TOTAL_TIMEOUT, self.download(target.clone()))
            .await
            .map_err(|_| refused("the host took too long to answer"))??;
        tracing::debug!(host, bytes = image.bytes.len(), mime = %image.mime, "image fetched");
        self.remember(target.as_str(), &image);
        Ok(image)
    }

    fn cached(&self, url: &str) -> Option<Image> {
        let entry = self.index.lookup(url)?;
        let bytes = self.cache.read_verified(&sha256_checksum(entry.sha256))?;
        Some(Image {
            mime: entry.mime,
            bytes,
        })
    }

    fn remember(&self, url: &str, image: &Image) {
        let sha256 = crate::checksum::hex(&Sha256::digest(&image.bytes));
        self.cache
            .store_bytes(&image.bytes, &sha256_checksum(sha256.clone()));
        let released = self.index.insert(
            url,
            Entry {
                sha256,
                mime: image.mime.clone(),
                fetched_unix: common::now_unix(),
            },
        );
        for sha256 in released {
            self.cache.evict(&sha256_checksum(sha256));
        }
    }

    fn check_target(&self, url: &Url) -> Result<()> {
        #[cfg(test)]
        if self.local.is_some_and(|local| {
            url.host_str() == Some(&local.ip().to_string()) && url.port() == Some(local.port())
        }) {
            return Ok(());
        }
        if url.scheme() != "https" {
            return Err(refused("only https images are fetched"));
        }
        let Some(host) = url.host_str() else {
            return Err(refused("the URL has no host"));
        };
        if guard::is_ip_literal(host) {
            return Err(refused("an IP address is not a host name"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(refused("the URL carries credentials"));
        }
        Ok(())
    }

    /// Redirects are walked here, not by the client, so that a hop to `http://`
    /// or to an address is stopped by the same checks as the first request.
    async fn download(&self, start: Url) -> Result<Image> {
        let mut url = start;
        for _ in 0..=MAX_REDIRECTS {
            self.check_target(&url)?;
            let mut response = self
                .client
                .get(url.clone())
                .send()
                .await
                .map_err(classify)?;
            network().observe_reachable();
            let status = response.status();
            tracing::debug!(
                host = url.host_str(),
                status = status.as_u16(),
                "image response"
            );

            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| refused("a redirect named no location"))?;
                url = url
                    .join(location)
                    .map_err(|_| refused("a redirect named an invalid location"))?;
                url.set_fragment(None);
                continue;
            }
            if !status.is_success() {
                return Err(refused(format!(
                    "the host answered HTTP {}",
                    status.as_u16()
                )));
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_BYTES as u64)
            {
                return Err(refused("the image is larger than 10 MiB"));
            }

            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(classify)? {
                if bytes.len() + chunk.len() > MAX_BYTES {
                    return Err(refused("the image is larger than 10 MiB"));
                }
                bytes.extend_from_slice(&chunk);
            }
            let mime =
                sniff::mime(&bytes).ok_or_else(|| refused("the response is not an image"))?;
            return Ok(Image {
                mime: mime.to_string(),
                bytes,
            });
        }
        Err(refused("too many redirects"))
    }
}

fn sha256_checksum(hex: String) -> Checksum {
    Checksum {
        algorithm: HashAlgorithm::Sha256,
        hex,
    }
}

fn refused(reason: impl std::fmt::Display) -> anyhow::Error {
    ErrorInfo::DownloadFailed {
        detail: format!("image refused: {reason}"),
    }
    .into()
}

/// A request the guard stopped is an answer, not a dropped network, and
/// reqwest's own message would carry the full URL.
fn classify(error: reqwest::Error) -> anyhow::Error {
    if guard::was_refused(&error) {
        return refused("the host resolves to a non-public address");
    }
    stream_failure(None, &error.without_url())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n pixels";

    /// A plain-HTTP origin on loopback, which stands in for the internet: the
    /// proxy is told this one address is reachable and everything else follows
    /// its production rules.
    struct Origin {
        addr: std::net::SocketAddr,
        hits: Arc<AtomicUsize>,
    }

    impl Origin {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("addr");
            let hits = Arc::new(AtomicUsize::new(0));
            let counted = hits.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    counted.fetch_add(1, Ordering::SeqCst);
                    let mut request = vec![0u8; 2048];
                    let read = socket.read(&mut request).await.unwrap_or(0);
                    let path = String::from_utf8_lossy(&request[..read])
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_string();
                    let (head, body): (&str, &[u8]) = match path.as_str() {
                        "/logo.png" => ("200 OK", PNG),
                        "/page" => ("200 OK", b"<html>hello</html>"),
                        _ => (
                            "302 Found\r\nLocation: http://cdn.example.test/logo.png",
                            b"",
                        ),
                    };
                    let response = format!(
                        "HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.write_all(body).await;
                }
            });
            Origin { addr, hits }
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}{path}", self.addr)
        }

        fn proxy(&self, cache: PathBuf) -> ImageProxy {
            ImageProxy {
                client: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .no_proxy()
                    .build()
                    .expect("client"),
                local: Some(self.addr),
                ..ImageProxy::new(cache)
            }
        }
    }

    fn detail(error: &anyhow::Error) -> String {
        match error.downcast_ref::<ErrorInfo>() {
            Some(ErrorInfo::DownloadFailed { detail }) => detail.clone(),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_url_the_proxy_must_not_follow_is_refused_before_any_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let proxy = ImageProxy::new(dir.path().to_path_buf());
        for url in [
            "http://example.test/logo.png",
            "ftp://example.test/logo.png",
            "https://127.0.0.1/logo.png",
            "https://[::1]/logo.png",
            "https://user:secret@example.test/logo.png",
            "https://localhost/logo.png",
        ] {
            let error = proxy.fetch(url).await.expect_err(url);
            assert!(
                detail(&error).starts_with("image refused"),
                "{url}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn a_redirect_to_plain_http_is_refused() {
        let origin = Origin::start().await;
        let dir = tempfile::tempdir().expect("temp dir");
        let proxy = origin.proxy(dir.path().to_path_buf());

        let error = proxy
            .fetch(&origin.url("/moved"))
            .await
            .expect_err("refused");

        assert!(detail(&error).contains("https"), "{error}");
        assert_eq!(origin.hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_response_that_is_not_an_image_is_refused_and_not_kept() {
        let origin = Origin::start().await;
        let dir = tempfile::tempdir().expect("temp dir");
        let proxy = origin.proxy(dir.path().to_path_buf());

        let error = proxy
            .fetch(&origin.url("/page"))
            .await
            .expect_err("refused");

        assert!(detail(&error).contains("not an image"), "{error}");
        assert_eq!(Cache::new(dir.path().to_path_buf()).usage().bytes, 0);
    }

    #[tokio::test]
    async fn a_repeat_fetch_is_served_from_the_cache_until_it_is_damaged_or_cleared() {
        let origin = Origin::start().await;
        let dir = tempfile::tempdir().expect("temp dir");
        let proxy = origin.proxy(dir.path().to_path_buf());
        let cache = Cache::new(dir.path().to_path_buf());
        let url = origin.url("/logo.png");

        let first = proxy.fetch(&url).await.expect("fetched");
        let second = proxy.fetch(&url).await.expect("cached");
        assert_eq!(
            (first.bytes.as_slice(), second.mime.as_str()),
            (PNG, "image/png")
        );
        assert_eq!(second.bytes, first.bytes);
        assert_eq!(origin.hits.load(Ordering::SeqCst), 1);
        assert!(
            cache.usage().bytes > PNG.len() as u64,
            "the index is counted"
        );

        let blob = cache.entries().remove(0).checksum;
        std::fs::write(cache.lookup(&blob).expect("blob"), b"damaged").expect("damage");
        proxy.fetch(&url).await.expect("refetched");
        assert_eq!(origin.hits.load(Ordering::SeqCst), 2);

        cache.clear();
        assert_eq!(cache.usage().bytes, 0);
        proxy.fetch(&url).await.expect("refetched after clear");
        assert_eq!(origin.hits.load(Ordering::SeqCst), 3);
    }
}
