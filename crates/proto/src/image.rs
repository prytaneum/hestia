//! Remote images, fetched by the daemon so the webview never contacts a host
//! itself.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::contract::Contract;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ImageFetchParams {
    pub url: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ImageFetchResult {
    /// Decided by sniffing the bytes, never by what the host claimed.
    pub mime: String,
    /// The image, base64-encoded; at most 10 MiB before encoding.
    pub bytes: String,
}

/// An `https` image from a public host, at most 10 MiB, served from the
/// daemon's cache when it was fetched before or the network is down. Anything
/// that is not a png, jpeg, gif, webp, avif or svg is refused.
pub struct ImageFetch;
impl Contract for ImageFetch {
    const CHANNEL: &'static str = "image.fetch";
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = ImageFetchParams;
    type Result = ImageFetchResult;
}
