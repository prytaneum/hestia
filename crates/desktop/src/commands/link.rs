//! `hestia://add-repo?url=<index url>` links.
//!
//! A repository's website offers one click to add it. The link only reaches
//! the add-repository confirmation in the webview; nothing is added until the
//! user confirms there. Only the installers register the scheme, so a link
//! never arrives from an AppImage or a portable archive.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State, Url};
use tauri_plugin_deep_link::DeepLinkExt;

/// The topic the webview listens on for a link opened while it was running.
pub const REPOSITORY_LINK: &str = "repository-link";

const SCHEME: &str = "hestia";
const ACTION: &str = "add-repo";

/// A validated index URL the shell was given before the webview could hear it.
/// Taken exactly once.
#[derive(Default)]
pub struct PendingRepository(Mutex<Option<String>>);

impl PendingRepository {
    fn set(&self, url: String) {
        *self.0.lock().unwrap() = Some(url);
    }

    fn take(&self) -> Option<String> {
        self.0.lock().unwrap().take()
    }
}

/// The https index URL a `hestia://add-repo?url=` link carries. Any other
/// scheme, host, path or non-https target is refused before it can reach a
/// request.
pub fn repository_from(link: &Url) -> Option<String> {
    if link.scheme() != SCHEME
        || link.host_str() != Some(ACTION)
        || !matches!(link.path(), "" | "/")
    {
        return None;
    }
    let target = link
        .query_pairs()
        .find_map(|(key, value)| (key == "url").then(|| value.into_owned()))?;
    let target = Url::parse(&target).ok()?;
    (target.scheme() == "https" && target.host_str().is_some()).then(|| target.into())
}

fn deliver(app: &AppHandle, links: &[Url]) {
    for link in links {
        let Some(url) = repository_from(link) else {
            tracing::debug!("ignoring a link that is not an https add-repo link");
            continue;
        };
        tracing::info!(url, "a repository link was opened");
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        if let Err(error) = app.emit(REPOSITORY_LINK, &url) {
            tracing::warn!(%error, "cannot deliver the repository link; holding it");
            app.state::<PendingRepository>().set(url);
        }
    }
}

/// Receive links while running, and hold the one the app was launched with
/// until the webview asks for it.
pub fn listen(app: &AppHandle) {
    let deep_link = app.deep_link();
    match deep_link.get_current() {
        Ok(Some(links)) => {
            if let Some(url) = links.iter().find_map(repository_from) {
                app.state::<PendingRepository>().set(url);
            }
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "cannot read the launch link"),
    }
    let handle = app.clone();
    deep_link.on_open_url(move |event| deliver(&handle, &event.urls()));
}

/// The repository link the app was launched with, cleared as it is read.
#[tauri::command]
pub fn pending_repository(pending: State<'_, PendingRepository>) -> Option<String> {
    pending.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(link: &str) -> Option<String> {
        repository_from(&Url::parse(link).unwrap())
    }

    #[test]
    fn an_https_index_url_passes() {
        assert_eq!(
            parse("hestia://add-repo?url=https%3A%2F%2Fexample.com%2Findex.json"),
            Some("https://example.com/index.json".to_string())
        );
        assert_eq!(
            parse("hestia://add-repo/?url=https://example.com/index.json"),
            Some("https://example.com/index.json".to_string())
        );
    }

    #[test]
    fn anything_but_https_is_refused() {
        assert_eq!(
            parse("hestia://add-repo?url=http://example.com/i.json"),
            None
        );
        assert_eq!(parse("hestia://add-repo?url=file:///etc/passwd"), None);
        assert_eq!(parse("hestia://add-repo?url=javascript:alert(1)"), None);
        assert_eq!(parse("hestia://add-repo?url=https://"), None);
        assert_eq!(parse("hestia://add-repo?url=not-a-url"), None);
        assert_eq!(parse("hestia://add-repo"), None);
    }

    #[test]
    fn other_hosts_paths_and_schemes_are_ignored() {
        let target = "?url=https://example.com/i.json";
        assert_eq!(parse(&format!("hestia://install{target}")), None);
        assert_eq!(parse(&format!("hestia://add-repo/extra{target}")), None);
        assert_eq!(parse(&format!("https://add-repo{target}")), None);
    }

    #[test]
    fn a_pending_link_is_read_once() {
        let pending = PendingRepository::default();
        assert_eq!(pending.take(), None);
        pending.set("https://example.com/i.json".to_string());
        assert!(pending.take().is_some());
        assert_eq!(pending.take(), None);
    }
}
