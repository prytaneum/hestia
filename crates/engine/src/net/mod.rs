//! Every outbound HTTP request the engine makes goes through here: the kernel's
//! network policy vets it, then one place retries what is worth retrying,
//! classifies what went wrong and observes whether the machine can reach upstream.
//!
//! A fetch written anywhere else would be a request with no vetting and no
//! timeout — the shape that turns a dropped connection into a job that hangs
//! until it is cancelled.

mod answer;
pub(crate) mod hosts;
pub(crate) mod image;
mod reach;
mod retry;
pub(crate) mod store;

pub(crate) use answer::{call, Answer};
pub(crate) use reach::network;
pub use reach::Network;

use anyhow::Result;
use kernel::vetting::{self, Failure, Limits, Policy, Refusal};
use proto::error::{ErrorInfo, Service};
use reqwest::{Method, RequestBuilder, Response};
use serde::de::DeserializeOwned;

/// The refusal as an [`ErrorInfo`] with the typed reason attached, so a caller
/// that answers a plugin can tell a refusal from a transfer failure.
fn refused(reason: Refusal) -> anyhow::Error {
    anyhow::Error::new(ErrorInfo::from(reason.clone())).context(reason)
}

/// A request to an API host `policy` declares, under the request limits; send
/// it with [`call`] so its body is read under them too.
pub(crate) fn request(policy: &Policy, method: Method, url: &str) -> Result<RequestBuilder> {
    policy
        .request(method, url, Limits::REQUEST)
        .map_err(refused)
}

/// `url` under `policy`, redirects vetted on every hop, each hop sent through
/// [`send`]. The body is read with [`read`] or streamed under `limits.bytes`.
pub(crate) async fn fetch(
    policy: &Policy,
    service: Option<Service>,
    url: &str,
    unhashed: bool,
    limits: Limits,
) -> Result<Response> {
    policy
        .get(url, unhashed, limits, |request| {
            send(service.clone(), request)
        })
        .await
        .map_err(|failure| match failure {
            Failure::Refused(reason) => refused(reason),
            Failure::Send(error) => error,
        })
}

/// A body read under `limits.bytes`.
pub(crate) async fn read(
    service: Option<Service>,
    response: Response,
    limits: Limits,
) -> Result<Vec<u8>> {
    vetting::read(response, limits)
        .await
        .map_err(|failure| match failure {
            Failure::Refused(reason) => refused(reason),
            Failure::Send(error) => stream_failure(service, &error),
        })
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
                if vetting::was_refused(&error) {
                    network().observe_reachable();
                    return Err(ErrorInfo::DownloadFailed {
                        detail: "the host resolves to a non-public address".into(),
                    }
                    .into());
                }
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
    Err(status_failure(service, status))
}

fn status_failure(service: Service, status: reqwest::StatusCode) -> anyhow::Error {
    ErrorInfo::Upstream {
        service,
        detail: format!("HTTP {}", status.as_u16()),
    }
    .into()
}

pub async fn get_json<T: DeserializeOwned>(
    service: Service,
    policy: &Policy,
    url: &str,
) -> Result<T> {
    let body = get_bytes(service.clone(), policy, url).await?;
    serde_json::from_slice(&body).map_err(|error| answer::malformed(service, error))
}

pub async fn get_text(service: Service, policy: &Policy, url: &str) -> Result<String> {
    let body = get_bytes(service.clone(), policy, url).await?;
    String::from_utf8(body).map_err(|error| answer::malformed(service, error))
}

async fn get_bytes(service: Service, policy: &Policy, url: &str) -> Result<Vec<u8>> {
    tracing::debug!(url, %service, "GET");
    let response = fetch(policy, Some(service.clone()), url, true, Limits::REQUEST).await?;
    let response = require_success(service.clone(), response)?;
    read(Some(service), response, Limits::REQUEST).await
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

#[cfg(test)]
mod tests {
    use kernel::vetting::Hosts;

    use super::*;

    #[tokio::test]
    async fn a_resolver_refusal_is_a_typed_failure_and_not_a_dropped_network() {
        let policy = Policy::new(Hosts::open_unhashed());
        let started = std::time::Instant::now();
        let error = fetch(&policy, None, "https://localhost/", true, Limits::REQUEST)
            .await
            .expect_err("localhost resolves to loopback");

        assert!(!is_offline(&error));
        assert!(matches!(
            error.downcast_ref::<ErrorInfo>(),
            Some(ErrorInfo::DownloadFailed { detail }) if detail.contains("non-public")
        ));
        assert!(
            started.elapsed() < retry::delay(1),
            "refusal is not retried"
        );
    }
}
