//! A dev build loads the frontend from `devUrl` (`http://localhost:1420`), and
//! WebKitGTK 2.54 refuses a custom-scheme load from an `http` page ("Unsafe
//! attempt to load URL … Domains, protocols and ports must match"), so every
//! image the app serves itself fails. Registering the scheme as CORS-enabled,
//! local or display-isolated does not lift it; the webview's CORS allowlist
//! does. A release page is served from `tauri://localhost` and is unaffected.

use tauri::WebviewWindow;

#[cfg(all(dev, target_os = "linux"))]
pub fn trust_dev_page(window: &WebviewWindow) {
    use webkit2gtk::WebViewExt;

    let allowed = [crate::proxy::SCHEME, crate::assets::SCHEME, "asset"]
        .map(|scheme| format!("{scheme}://*/*"));
    let applied = window.with_webview(move |webview| {
        let patterns: Vec<&str> = allowed.iter().map(String::as_str).collect();
        webview.inner().set_cors_allowlist(&patterns);
        tracing::debug!(?patterns, "dev page may load the app's own schemes");
    });
    if let Err(error) = applied {
        tracing::warn!(%error, "cannot reach the dev webview; images will not load");
    }
}

#[cfg(not(all(dev, target_os = "linux")))]
pub fn trust_dev_page(_: &WebviewWindow) {}
