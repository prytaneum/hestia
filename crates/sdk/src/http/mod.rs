//! Outbound HTTPS to the hosts the manifest grants, over `wasi:http`.
//!
//! The plugin needs the `network:connect` permission and lists each host in
//! its `hosts`. Only `https://` URLs to named hosts are allowed. A refused
//! request is [`Error::Denied`]; a host that cannot be reached is
//! [`Error::Offline`]. The host does not follow redirects, so a `3xx` answer
//! comes back as it is and the plugin decides whether to request the `Location`
//! (which must be granted too).
//!
//! Build the address with [`Url`], describe the call with [`Request`], and turn
//! a failing status into the typed [`Error`] with [`Response::error_for_status`]:
//!
//! ```no_run
//! use hestia::http::{Request, Url};
//!
//! let url = Url::new("https://api.example.com")?
//!     .segments(["v1", "status"])
//!     .query("verbose", true);
//! let status: hestia::Value = Request::get(url).send_json()?;
//! # Ok::<(), hestia::Error>(())
//! ```
//!
//! [`get`] and [`post`] are the short forms when nothing else is set.

mod headers;
mod request;
mod response;
#[cfg(target_arch = "wasm32")]
mod transport;
mod url;

use crate::Error;

pub use headers::Headers;
pub use request::{Method, Request};
pub use response::Response;
pub use url::{encode, Url};

/// `GET` to `url`; any status is a response, see
/// [`Response::error_for_status`].
///
/// ```no_run
/// let reply = hestia::http::get("https://api.example.com/v1/status")?;
/// if reply.is_success() {
///     let status: hestia::Value = reply.json()?;
/// }
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn get(url: &str) -> Result<Response, Error> {
    Request::get(url).send()
}

/// `POST` of `body` as `content_type` to `url`.
///
/// ```no_run
/// let reply = hestia::http::post("https://api.example.com/echo", "text/plain", b"hi")?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn post(url: &str, content_type: &str, body: &[u8]) -> Result<Response, Error> {
    Request::post(url).body(content_type, body).send()
}
