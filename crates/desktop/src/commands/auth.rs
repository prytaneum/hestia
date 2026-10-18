//! Microsoft sign-in over the sisu flow, orchestrated shell-side.
//!
//! Unlike every other daemon call — which the frontend makes through the
//! generic `ipc_call` bridge — sign-in cannot live purely in the frontend:
//! it opens a webview at Microsoft's sign-in page and must read that
//! external, cross-origin webview's URL to catch the `?code=` redirect, and
//! only the Rust side can read a cross-origin webview's location. So this one
//! command drives the two daemon calls (`account.login.begin` →
//! `account.login.complete`) around a native sign-in window, exactly as
//! Modrinth's launcher does.

use std::time::{Duration, Instant};

use client::proto::accounts::{Account, LoginMethod};
use tauri::{AppHandle, Manager, State, UserAttentionType, WebviewUrl, WebviewWindowBuilder};

use crate::bridge::{acquire, Bridge, CallError};

/// The MSA desktop reply URL the sisu flow redirects to with the OAuth code
/// (the engine's `REPLY_URL`). Reaching it means the user has signed in.
const REDIRECT_PREFIX: &str = "https://login.live.com/oauth20_desktop.srf";
const SIGNIN_LABEL: &str = "signin";
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const FLOW_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Sign in a Microsoft account. Returns the stored account, or `None` when the
/// user closes the sign-in window before completing (a cancel, not an error).
#[tauri::command]
pub async fn account_login_sisu(
    app: AppHandle,
    bridge: State<'_, Bridge>,
) -> Result<Option<Account>, CallError> {
    let client = acquire(&app, &bridge).await?;
    tracing::info!("sisu sign-in started");
    let begin = client.accounts().begin_login(LoginMethod::Sisu).await?;
    if begin.url.is_empty() {
        return Err(CallError::other("daemon returned no sign-in URL"));
    }

    if let Some(existing) = app.get_webview_window(SIGNIN_LABEL) {
        let _ = existing.close();
    }
    let url = begin
        .url
        .parse()
        .map_err(|_| CallError::other("could not parse the sign-in URL"))?;
    let window = WebviewWindowBuilder::new(&app, SIGNIN_LABEL, WebviewUrl::External(url))
        .title("Sign in to Microsoft")
        .inner_size(520.0, 720.0)
        .decorations(false)
        .focused(true)
        .center()
        .build()
        .map_err(|e| CallError::other(format!("could not open the sign-in window: {e}")))?;
    let _ = window.set_focus();
    let _ = window.request_user_attention(Some(UserAttentionType::Critical));

    let started = Instant::now();
    loop {
        if started.elapsed() >= FLOW_TIMEOUT {
            tracing::warn!("sisu sign-in timed out");
            let _ = window.close();
            return Err(CallError::other("sign-in timed out"));
        }
        // A closed window's title read errors — the user cancelled the flow.
        if window.title().is_err() {
            tracing::info!("sisu sign-in cancelled");
            return Ok(None);
        }
        if let Ok(current) = window.url() {
            if current.as_str().starts_with(REDIRECT_PREFIX) {
                if let Some(code) = current
                    .query_pairs()
                    .find(|(key, _)| key == "code")
                    .map(|(_, value)| value.into_owned())
                {
                    let _ = window.close();
                    let account = client.accounts().complete_login(&begin.id, &code).await?;
                    tracing::info!(name = %account.name, "sisu sign-in complete");
                    return Ok(Some(account));
                }
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
