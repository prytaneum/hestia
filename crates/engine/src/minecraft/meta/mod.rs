//! Upstream metadata client for the vanilla provider: Mojang's piston-meta
//! catalogue. Fetch + parse only; the provider assembles the results into
//! launch profiles.

pub mod bedrock;
pub mod mojang;

use anyhow::{Context, Result};
use proto::error::Service;
use serde_json::Value;

use crate::net;

async fn fetch_json(service: Service, url: &str) -> Result<Value> {
    let body = fetch_text(service, url).await?;
    serde_json::from_str(&body).with_context(|| format!("{url} returned malformed JSON"))
}

/// Fetch a catalogue document, falling back to the last good copy when the
/// network is gone. A version list changes rarely and a stale one is far more
/// useful than a failed page — front-ends report the offline state alongside,
/// so a cached answer is never mistaken for a live one.
async fn fetch_text(service: Service, url: &str) -> Result<String> {
    match net::get_text(service, url).await {
        Ok(body) => {
            net::store::save(url, &body);
            Ok(body)
        }
        Err(error) if net::is_offline(&error) => match net::store::load(url) {
            Some(body) => {
                tracing::info!(url, "offline: serving the cached catalogue");
                Ok(body)
            }
            None => Err(error),
        },
        Err(error) => Err(error),
    }
}
