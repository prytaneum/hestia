//! Opening a folder or a link with the host's default handler. The plugin's own
//! commands start that handler with the shell's environment, which inside an
//! AppImage is not the host's ([`crate::appimage`]).

use std::ffi::OsStr;
use std::path::PathBuf;

/// The schemes `tauri-plugin-opener` opens by default, kept so a link in an
/// untrusted description cannot reach an arbitrary scheme handler.
const URL_SCHEMES: [&str; 4] = ["http", "https", "mailto", "tel"];

#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    hand_off(path.as_os_str())
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    let url = reqwest::Url::parse(&url).map_err(|e| format!("not a link: {e}"))?;
    if !URL_SCHEMES.contains(&url.scheme()) {
        return Err(format!("{}: links are not opened", url.scheme()));
    }
    hand_off(OsStr::new(url.as_str()))
}

fn hand_off(target: &OsStr) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let opened = match crate::appimage::host_env() {
        Some(env) => crate::appimage::open(target, &env),
        None => open::that_detached(target),
    };
    #[cfg(not(target_os = "linux"))]
    let opened = open::that_detached(target);

    opened.map_err(|e| {
        tracing::warn!(error = %e, "cannot hand the target to the host");
        e.to_string()
    })
}
