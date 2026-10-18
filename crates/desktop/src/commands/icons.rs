//! Custom entry icons, copied into the hestia data home.
//!
//! A picked image is copied to `<data_home>/icons/<entry-id>.<ext>` so it
//! survives the original file moving; the disk is the registry (no index).
//! The webview loads them over the asset protocol, whose scope is widened to
//! the icons directory at each call — the data home can move at runtime.
//!
//! An entry created from a modpack inherits the pack's icon: `icon_fetch`
//! downloads it into the same directory, sniffing the format rather than
//! trusting the URL or the server's content type.
//!
//! Entries can instead wear a *generated* icon: the webview bakes a PNG from a
//! background and a symbol sprite and sends the bytes, saved as
//! `<entry-id>.png`. The configuration that produced it is kept beside it as an
//! `<entry-id>.json` sidecar so the icon editor can reopen on the current look.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use image::ImageFormat;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::bridge::CallError;

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];
const MAX_SIDECAR_BYTES: usize = 4096;
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// One entry's stored icon; `mtime` doubles as the cache-busting version.
#[derive(Serialize, Clone)]
pub struct IconEntry {
    pub path: String,
    pub mtime: u64,
}

fn icons_dir() -> PathBuf {
    common::paths::data_home(None).join("icons")
}

// Entry ids are slug + hex tag; anything else could escape the icons dir.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn allow_assets(app: &AppHandle, dir: &Path) {
    let _ = app.asset_protocol_scope().allow_directory(dir, false);
}

fn entry_for(path: &Path) -> Option<IconEntry> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(IconEntry {
        path: path.to_string_lossy().into_owned(),
        mtime,
    })
}

fn stored_icons(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let id = path.file_stem()?.to_str()?.to_string();
            let icon = path.is_file() && has_image_extension(&path);
            icon.then_some((id, path))
        })
        .collect()
}

fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn store(app: &AppHandle, entry_id: &str, ext: &str, bytes: &[u8]) -> Result<IconEntry, CallError> {
    let dir = icons_dir();
    std::fs::create_dir_all(&dir)?;
    remove_stored(&dir, entry_id);
    let target = dir.join(format!("{entry_id}.{ext}"));
    std::fs::write(&target, bytes)?;
    allow_assets(app, &dir);
    entry_for(&target).ok_or_else(|| CallError::other("cannot read the stored icon"))
}

fn remove_stored(dir: &Path, id: &str) {
    for (stored_id, path) in stored_icons(dir) {
        if stored_id == id {
            let _ = std::fs::remove_file(path);
        }
    }
    let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
}

#[tauri::command]
pub fn icons_list(app: AppHandle) -> Result<BTreeMap<String, IconEntry>, CallError> {
    let dir = icons_dir();
    allow_assets(&app, &dir);
    Ok(stored_icons(&dir)
        .into_iter()
        .filter_map(|(id, path)| entry_for(&path).map(|entry| (id, entry)))
        .collect())
}

#[tauri::command]
pub fn icon_set(
    app: AppHandle,
    entry_id: String,
    source_path: String,
) -> Result<IconEntry, CallError> {
    if !valid_id(&entry_id) {
        return Err(CallError::other("invalid entry id"));
    }
    let source = PathBuf::from(&source_path);
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| CallError::other("unsupported image type"))?;
    let size = std::fs::metadata(&source)?.len();
    if size > MAX_BYTES {
        return Err(CallError::other("image is larger than 10 MB"));
    }
    let bytes = std::fs::read(&source)?;
    let entry = store(&app, &entry_id, &ext, &bytes)?;
    tracing::info!(entry_id, ext, size, "icon set");
    Ok(entry)
}

#[tauri::command]
pub async fn icon_fetch(
    app: AppHandle,
    entry_id: String,
    url: String,
) -> Result<IconEntry, CallError> {
    if !valid_id(&entry_id) {
        return Err(CallError::other("invalid entry id"));
    }
    let url = reqwest::Url::parse(&url).map_err(|_| CallError::other("invalid icon url"))?;
    if url.scheme() != "https" {
        return Err(CallError::other("icon url must be https"));
    }
    let bytes = download_capped(url).await?;
    let ext = sniff_extension(&bytes)?;
    let entry = store(&app, &entry_id, ext, &bytes)?;
    tracing::info!(entry_id, ext, byte_len = bytes.len(), "icon fetched");
    Ok(entry)
}

async fn download_capped(url: reqwest::Url) -> Result<Vec<u8>, CallError> {
    let too_large = || CallError::other("image is larger than 10 MB");
    let client = reqwest::Client::builder()
        .user_agent(common::app::user_agent())
        .timeout(FETCH_TIMEOUT)
        .build()?;
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)?;
    if response.content_length().is_some_and(|len| len > MAX_BYTES) {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > MAX_BYTES {
            return Err(too_large());
        }
    }
    Ok(bytes)
}

fn sniff_extension(bytes: &[u8]) -> Result<&'static str, CallError> {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Png) => Ok("png"),
        Ok(ImageFormat::Jpeg) => Ok("jpg"),
        Ok(ImageFormat::WebP) => Ok("webp"),
        Ok(ImageFormat::Gif) => Ok("gif"),
        _ => Err(CallError::other("unsupported image type")),
    }
}

#[tauri::command]
pub fn icon_remove(entry_id: String) -> Result<(), CallError> {
    if !valid_id(&entry_id) {
        return Err(CallError::other("invalid entry id"));
    }
    tracing::info!(entry_id, "icon removed");
    remove_stored(&icons_dir(), &entry_id);
    Ok(())
}

#[tauri::command]
pub fn icon_set_bytes(
    app: AppHandle,
    entry_id: String,
    bytes: Vec<u8>,
    config: Value,
) -> Result<IconEntry, CallError> {
    if !valid_id(&entry_id) {
        return Err(CallError::other("invalid entry id"));
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Err(CallError::other("image is larger than 10 MB"));
    }
    let ext = sniff_extension(&bytes)?;
    let sidecar = sidecar_json(&config)?;
    let entry = store(&app, &entry_id, ext, &bytes)?;
    std::fs::write(icons_dir().join(format!("{entry_id}.json")), sidecar)?;
    tracing::info!(entry_id, ext, byte_len = bytes.len(), "generated icon set");
    Ok(entry)
}

/// The stored generation config, or `null` when the icon was picked from a
/// file instead — the sidecar's only reader.
#[tauri::command]
pub fn icon_config(entry_id: String) -> Result<Option<Value>, CallError> {
    if !valid_id(&entry_id) {
        return Err(CallError::other("invalid entry id"));
    }
    let path = icons_dir().join(format!("{entry_id}.json"));
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn sidecar_json(config: &Value) -> Result<Vec<u8>, CallError> {
    let encoded = serde_json::to_vec(config)?;
    if !config.is_object() || encoded.len() > MAX_SIDECAR_BYTES {
        return Err(CallError::other("invalid icon config"));
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::{sidecar_json, sniff_extension, MAX_SIDECAR_BYTES};
    use serde_json::json;

    #[test]
    fn fetched_icon_extension_comes_from_the_bytes() {
        assert_eq!(sniff_extension(b"\x89PNG\r\n\x1a\n").unwrap(), "png");
        assert_eq!(sniff_extension(b"\xff\xd8\xff\xe0").unwrap(), "jpg");
        assert_eq!(sniff_extension(b"GIF89a").unwrap(), "gif");
        assert!(sniff_extension(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").is_err());
    }

    #[test]
    fn sidecar_must_be_a_small_object() {
        assert!(sidecar_json(&json!({ "symbol": "grass_block" })).is_ok());
        assert!(sidecar_json(&json!("grass_block")).is_err());
        assert!(sidecar_json(&json!({ "symbol": "x".repeat(MAX_SIDECAR_BYTES) })).is_err());
    }
}
