//! Every outbound HTTP request the engine makes goes through here: one pooled
//! client with real timeouts, a bounded retry, one classification of what went
//! wrong, and one place that observes whether the machine can reach upstream.
//!
//! A fetch written anywhere else would be a request with no timeout — the shape
//! that turns a dropped connection into a job that hangs until it is cancelled.

pub(crate) mod guard;
pub(crate) mod image;
mod reach;
mod retry;
pub(crate) mod store;

pub(crate) use reach::network;
pub use reach::Network;

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use proto::error::{ErrorInfo, Service};
use reqwest::{RequestBuilder, Response};
use serde::de::DeserializeOwned;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A dropped connection usually stops delivering rather than resetting, which
/// without this reads as a transfer that is merely very slow — forever.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// The whole-request budget for a metadata call. A download sets none: a large
/// artifact on a slow line is not a failure, and `READ_TIMEOUT` already catches
/// a stalled one.
const META_TIMEOUT: Duration = Duration::from_secs(45);

/// One pooled client keeps connections alive across requests — a fresh client
/// per request pays a TCP + TLS handshake for every one of the thousands of
/// small fetches an asset materialisation makes.
pub(crate) fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| builder().build().expect("default reqwest client builds"))
}

/// For a URL a user typed rather than one a catalogue vouches for: a redirect
/// may not leave https, so a link cannot be downgraded in flight.
pub(crate) fn https_only_client() -> &'static reqwest::Client {
    const MAX_REDIRECTS: usize = 5;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("redirected away from https")
            } else if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        });
        builder()
            .redirect(policy)
            .build()
            .expect("https-only reqwest client builds")
    })
}

fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(common::app::user_agent())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
}

/// Send `request`, retrying what is worth retrying and recording what the
/// outcome says about reachability. The response is returned whatever its
/// status — a 404 is the caller's business, and the network is up either way.
pub async fn send(service: Option<Service>, request: RequestBuilder) -> Result<Response> {
    if network().pinned() {
        return Err(ErrorInfo::OfflineMode.into());
    }
    let mut next = Some(request);
    for attempt in 1..=retry::MAX_ATTEMPTS {
        let request = next.take().expect("the loop always leaves a request");
        // A multipart or streaming body cannot be replayed, so it gets one shot.
        let replay = request.try_clone();
        match request.send().await {
            Ok(response) => {
                network().observe_reachable();
                match retry::backoff_for(&response, attempt).zip(replay) {
                    Some((delay, replay)) => {
                        tracing::debug!(
                            status = response.status().as_u16(),
                            attempt,
                            "upstream asked to be retried"
                        );
                        tokio::time::sleep(delay).await;
                        next = Some(replay);
                    }
                    None => return Ok(response),
                }
            }
            Err(error) => {
                if !retry::is_transport(&error) {
                    network().observe_reachable();
                    return Err(upstream(service, &error));
                }
                match replay.filter(|_| attempt < retry::MAX_ATTEMPTS) {
                    Some(replay) => {
                        tracing::debug!(attempt, %error, "request did not reach upstream");
                        tokio::time::sleep(retry::delay(attempt)).await;
                        next = Some(replay);
                    }
                    // Only once the retries are spent, so a single blip does not
                    // flip every front-end to offline and back.
                    None => {
                        network().observe_unreachable();
                        return Err(offline(service));
                    }
                }
            }
        }
    }
    network().observe_unreachable();
    Err(offline(service))
}

/// Reject a non-2xx response as an upstream failure.
pub fn require_success(service: Service, response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(ErrorInfo::Upstream {
        service,
        detail: format!("HTTP {}", status.as_u16()),
    }
    .into())
}

pub async fn get_json<T: DeserializeOwned>(service: Service, url: &str) -> Result<T> {
    let response = get(service, url).await?;
    read_body(service, response.json::<T>().await)
}

pub async fn get_text(service: Service, url: &str) -> Result<String> {
    let response = get(service, url).await?;
    read_body(service, response.text().await)
}

pub async fn get(service: Service, url: &str) -> Result<Response> {
    tracing::debug!(url, %service, "GET");
    let response = send(Some(service), client().get(url).timeout(META_TIMEOUT)).await?;
    require_success(service, response)
}

/// A body that stops arriving mid-read is the same network drop as one that
/// never started, so it lands on the same state and the same error.
fn read_body<T>(service: Service, result: reqwest::Result<T>) -> Result<T> {
    result.map_err(|error| {
        if retry::is_transport(&error) {
            network().observe_unreachable();
            return offline(Some(service));
        }
        upstream(Some(service), &error)
    })
}

/// Classify a failure that happened while streaming a response body, which the
/// download path drives itself rather than through [`send`].
pub fn stream_failure(service: Option<Service>, error: &reqwest::Error) -> anyhow::Error {
    if retry::is_transport(error) {
        network().observe_unreachable();
        return offline(service);
    }
    upstream(service, error)
}

pub fn offline(service: Option<Service>) -> anyhow::Error {
    ErrorInfo::Offline { service }.into()
}

/// Whether a failure was the network rather than the answer — either kind, so
/// a caller with a cached fallback serves it in offline mode too.
pub fn is_offline(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ErrorInfo>()
        .is_some_and(|info| matches!(info, ErrorInfo::Offline { .. } | ErrorInfo::OfflineMode))
}

fn upstream(service: Option<Service>, error: &reqwest::Error) -> anyhow::Error {
    match service {
        Some(service) => ErrorInfo::Upstream {
            service,
            detail: error.to_string(),
        }
        .into(),
        None => ErrorInfo::DownloadFailed {
            detail: error.to_string(),
        }
        .into(),
    }
}
