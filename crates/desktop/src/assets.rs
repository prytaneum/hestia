//! The `hestia-plugin` scheme: `hestia-plugin://localhost/<id>/<path>`
//! (`http://hestia-plugin.localhost/<id>/<path>` on WebView2, and the
//! `hestia-plugin://<id>/<path>` spelling) answers with a file of an enabled
//! plugin's package, read by the daemon through `plugin.asset`.
//!
//! Shell plumbing beside the image proxy: which plugin, which directory and how
//! large is the daemon's call, so the webview never reads a plugin's files and
//! plugin content never shares the app's origin.

use base64::Engine as _;
use client::proto::plugin::{PluginAsset, PluginAssetParams};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder};

use crate::bridge::{self, Bridge, CallError};
use crate::proxy::{self, SVG, SVG_CSP};

pub const SCHEME: &str = "hestia-plugin";

const HTML: &str = "text/html";

/// A plugin's page runs in an opaque-origin frame, so `'self'` matches nothing:
/// its own files are named by scheme. No network, no nested frames, no forms;
/// `sandbox` keeps the origin opaque even if the file is opened on its own.
const HTML_CSP: &str = "sandbox allow-scripts; default-src 'none'; \
    script-src 'unsafe-inline' hestia-plugin: http://hestia-plugin.localhost; \
    style-src 'unsafe-inline' hestia-plugin: http://hestia-plugin.localhost; \
    img-src hestia-plugin: http://hestia-plugin.localhost data: blob:; \
    font-src hestia-plugin: http://hestia-plugin.localhost data:; \
    connect-src hestia-plugin: http://hestia-plugin.localhost; \
    base-uri 'none'; form-action 'none'; frame-src 'none'";

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
    let Some((plugin, path)) = target(uri) else {
        return proxy::status(StatusCode::BAD_REQUEST);
    };
    match fetch(app, plugin, path).await {
        Ok((mime, bytes)) => asset(&mime, bytes),
        Err(error) => {
            tracing::debug!(?error, "plugin asset not served");
            proxy::status(StatusCode::NOT_FOUND)
        }
    }
}

fn target(uri: &str) -> Option<(String, String)> {
    let url = reqwest::Url::parse(uri).ok()?;
    let path = url.path().trim_start_matches('/');
    let (plugin, path) = match url.host_str() {
        Some(host) if host != "localhost" && !host.ends_with(".localhost") => (host, path),
        _ => path.split_once('/')?,
    };
    (!plugin.is_empty() && !path.is_empty()).then(|| (plugin.to_string(), percent_decode(path)))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| text.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                at += 3;
            }
            None => {
                decoded.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

async fn fetch(
    app: &AppHandle,
    plugin: String,
    path: String,
) -> Result<(String, Vec<u8>), CallError> {
    let bridge = app.state::<Bridge>();
    let client = bridge::acquire(app, &bridge).await?;
    let fetched = client
        .session()
        .call::<PluginAsset>(&PluginAssetParams { plugin, path })
        .await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(fetched.bytes)
        .map_err(|error| CallError::other(error.to_string()))?;
    Ok((fetched.mime, bytes))
}

fn asset(mime: &str, bytes: Vec<u8>) -> Response<Vec<u8>> {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        // A font loaded by a stylesheet is fetched cross-origin in CORS mode.
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    if mime == SVG {
        response = response.header(header::CONTENT_SECURITY_POLICY, SVG_CSP);
    }
    if mime == HTML {
        response = response.header(header::CONTENT_SECURITY_POLICY, HTML_CSP);
    }
    response
        .body(bytes)
        .unwrap_or_else(|_| proxy::status(StatusCode::INTERNAL_SERVER_ERROR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_names_the_plugin_and_path() {
        for uri in [
            "hestia-plugin://localhost/demo/assets/icon.png",
            "http://hestia-plugin.localhost/demo/assets/icon.png",
            "hestia-plugin://demo/assets/icon.png",
        ] {
            assert_eq!(
                target(uri),
                Some(("demo".into(), "assets/icon.png".into())),
                "{uri}"
            );
        }
    }

    #[test]
    fn a_page_is_served_sandboxed_without_network() {
        let response = asset(HTML, Vec::new());
        let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(csp.starts_with("sandbox allow-scripts;"));
        assert!(!csp.contains("allow-same-origin") && !csp.contains("https:"));
    }

    #[test]
    fn a_path_is_percent_decoded() {
        let (_, path) = target("hestia-plugin://localhost/demo/assets/a%20b.png").unwrap();
        assert_eq!(path, "assets/a b.png");
    }

    #[test]
    fn a_uri_without_a_file_is_refused() {
        assert_eq!(target("hestia-plugin://localhost/demo"), None);
        assert_eq!(target("hestia-plugin://localhost/"), None);
    }
}
