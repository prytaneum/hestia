//! Which outbound failures are worth another attempt, and how long to wait.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Response;

pub const MAX_ATTEMPTS: u32 = 3;
const BASE_DELAY: Duration = Duration::from_millis(400);
const MAX_DELAY: Duration = Duration::from_secs(5);

/// Whether the request failed on the way out rather than being answered. These
/// are the network-drop family: no route, DNS, a refused or timed-out connect,
/// and a body that stopped arriving mid-transfer.
pub fn is_transport(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request() || error.is_body()
}

/// How long to wait before retrying an answered-but-failed response, or `None`
/// when its status is the caller's to handle.
pub fn backoff_for(response: &Response, attempt: u32) -> Option<Duration> {
    if attempt >= MAX_ATTEMPTS {
        return None;
    }
    let status = response.status();
    if status.as_u16() != 429 && !status.is_server_error() {
        return None;
    }
    Some(retry_after(response).unwrap_or_else(|| delay(attempt)))
}

pub fn delay(attempt: u32) -> Duration {
    let window = BASE_DELAY
        .saturating_mul(1 << attempt.saturating_sub(1).min(4))
        .min(MAX_DELAY);
    window + Duration::from_millis(jitter_ms(window))
}

fn retry_after(response: &Response) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds.min(MAX_DELAY.as_secs())))
}

// Spreads the retries of a burst that failed together — the thousands of
// concurrent asset fetches a materialize makes would otherwise resume in lockstep.
fn jitter_ms(window: Duration) -> u64 {
    let span = (window.as_millis() as u64 / 2).max(1);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
        % span
}
