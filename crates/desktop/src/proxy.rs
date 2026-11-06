//! The `hestia-image` scheme: `hestia-image://localhost/fetch?u=<encoded url>`
//! (`http://hestia-image.localhost/…` on WebView2) answers with a remote image
//! the daemon fetched through `image.fetch`.
//!
//! Shell plumbing like the bridge: all policy (https only, public hosts, size,
//! sniffing, caching) is the daemon's, so the webview never contacts an image
//! host itself.

use base64::Engine as _;
use client::proto::image::{ImageFetch, ImageFetchParams};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder};

use crate::bridge::{self, Bridge, CallError};

pub const SCHEME: &str = "hestia-image";

pub(crate) const SVG: &str = "image/svg+xml";
/// An SVG opened directly (not through `<img>`) must not run script.
pub(crate) const SVG_CSP: &str = "script-src 'none'";
const MAX_AGE: &str = "private, max-age=86400";

pub fn serve(
    ctx: UriSchemeContext<'_, tauri::Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let uri = request.uri().to_string();
    tauri::async_runtime::spawn(async move {
        responder.respond(respond(&app, &uri).await);
    });
}

async fn respond(app: &AppHandle, uri: &str) -> Response<Vec<u8>> {
    let Some(url) = remote_url(uri) else {
        return status(StatusCode::BAD_REQUEST);
    };
    match fetch(app, url).await {
        Ok((mime, bytes)) => image(&mime, bytes),
        Err(error) => {
            tracing::debug!(?error, "remote image not served");
            status(StatusCode::BAD_GATEWAY)
        }
    }
}

fn remote_url(uri: &str) -> Option<String> {
    reqwest::Url::parse(uri)
        .ok()?
        .query_pairs()
        .find_map(|(key, value)| (key == "u").then(|| value.into_owned()))
}

async fn fetch(app: &AppHandle, url: String) -> Result<(String, Vec<u8>), CallError> {
    let bridge = app.state::<Bridge>();
    let client = bridge::acquire(app, &bridge).await?;
    let fetched = client
        .session()
        .call::<ImageFetch>(&ImageFetchParams { url })
        .await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(fetched.bytes)
        .map_err(|error| CallError::other(error.to_string()))?;
    Ok((fetched.mime, bytes))
}

fn image(mime: &str, bytes: Vec<u8>) -> Response<Vec<u8>> {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, MAX_AGE)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        // WebGL refuses a cross-origin texture without it (the 3D skin preview).
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    if mime == SVG {
        response = response.header(header::CONTENT_SECURITY_POLICY, SVG_CSP);
    }
    response
        .body(bytes)
        .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR))
}

pub(crate) fn status(code: StatusCode) -> Response<Vec<u8>> {
    let mut response = Response::new(Vec::new());
    *response.status_mut() = code;
    response
}
