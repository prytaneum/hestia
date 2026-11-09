//! An entry's own icon, kept in the entry's directory beside its record. It
//! therefore moves with a rename, travels in an export and goes with a remove,
//! with nothing keyed by the entry's id to keep in step.
//!
//! The image is stored as `icon.<ext>`, the extension taken from its bytes. A
//! generated icon also records what composed it in `icon.json`; a picked one
//! has none, which is how the two are told apart.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use image::ImageFormat;
use proto::error::ErrorInfo;
use proto::icon::{EntryIcon, IconBackground, IconGenerator};

use crate::net;
use crate::schema::{self, Document};

const STEM: &str = "icon";
/// What an icon may be, and the extension each is stored under — in the order
/// a directory holding more than one is read.
const FORMATS: [(ImageFormat, &str); 4] = [
    (ImageFormat::Png, "png"),
    (ImageFormat::Jpeg, "jpg"),
    (ImageFormat::WebP, "webp"),
    (ImageFormat::Gif, "gif"),
];
const MAX_MEGABYTES: u64 = 10;
pub(crate) const MAX_BYTES: u64 = MAX_MEGABYTES * 1024 * 1024;
const MAX_SYMBOL_LEN: usize = 64;
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

impl Document for IconGenerator {
    const NAME: &'static str = "icon.json";
}

/// The entry's icon, or `None` when it wears none. A generator that does not
/// hold is dropped: it came off disk, perhaps out of somebody's archive, and
/// its colors end up in a stylesheet.
pub(crate) fn read(entry_dir: &Path) -> Option<EntryIcon> {
    let path = stored(entry_dir).next()?;
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let revision = modified
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let generator =
        schema::load::<IconGenerator>(&entry_dir.join(IconGenerator::NAME)).filter(well_formed);
    Some(EntryIcon {
        path,
        revision,
        generator,
    })
}

/// Make `image` the entry's icon, recording `generator` beside it when it
/// composed the image and clearing any earlier one when it did not.
pub(crate) fn write(
    entry_dir: &Path,
    image: &[u8],
    generator: Option<&IconGenerator>,
) -> Result<EntryIcon> {
    let extension = extension_of(image)?;
    if generator.is_some_and(|g| !well_formed(g)) {
        bail!(ErrorInfo::MalformedRequest {
            detail: "the icon generator names a malformed color or symbol".to_string(),
        });
    }
    let target = entry_dir.join(format!("{STEM}.{extension}"));
    common::atomic::write(&target, image)
        .with_context(|| format!("cannot write {}", target.display()))?;
    for stale in stored(entry_dir).filter(|path| *path != target) {
        remove(&stale)?;
    }
    let sidecar = entry_dir.join(IconGenerator::NAME);
    match generator {
        Some(generator) => schema::save(&sidecar, generator)?,
        None if sidecar.exists() => remove(&sidecar)?,
        None => {}
    }
    tracing::info!(entry = %entry_dir.display(), extension, bytes = image.len(), generated = generator.is_some(), "entry icon set");
    read(entry_dir).context("the icon just written cannot be read back")
}

/// Read an image file the caller named, refusing one too large to be an icon
/// before reading it whole.
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>> {
    let not_found = || ErrorInfo::FileNotFound {
        path: path.display().to_string(),
    };
    let meta = std::fs::metadata(path).map_err(|_| not_found())?;
    if !meta.is_file() {
        bail!(not_found());
    }
    if meta.len() > MAX_BYTES {
        bail!(too_large());
    }
    std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))
}

/// Download an icon a source published, bounded like a picked one.
pub(crate) async fn fetch(url: &str) -> Result<Vec<u8>> {
    let parsed = reqwest::Url::parse(url).context("the icon url is not a url")?;
    if parsed.scheme() != "https" {
        bail!("the icon url is not https");
    }
    let request = net::client().get(parsed).timeout(FETCH_TIMEOUT);
    let mut response = net::send(None, request)
        .await?
        .error_for_status()
        .context("the icon could not be downloaded")?;
    if response.content_length().is_some_and(|len| len > MAX_BYTES) {
        bail!(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| net::stream_failure(None, &e))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > MAX_BYTES {
            bail!(too_large());
        }
    }
    Ok(bytes)
}

fn extension_of(image: &[u8]) -> Result<&'static str> {
    if image.len() as u64 > MAX_BYTES {
        bail!(too_large());
    }
    let format = image::guess_format(image).map_err(|_| ErrorInfo::IconNotAnImage)?;
    FORMATS
        .iter()
        .find(|(known, _)| *known == format)
        .map(|(_, extension)| *extension)
        .ok_or_else(|| ErrorInfo::IconNotAnImage.into())
}

fn stored(entry_dir: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    FORMATS
        .iter()
        .map(move |(_, extension)| entry_dir.join(format!("{STEM}.{extension}")))
        .filter(|path| path.is_file())
}

fn remove(path: &Path) -> Result<()> {
    std::fs::remove_file(path).with_context(|| format!("cannot remove {}", path.display()))
}

fn too_large() -> ErrorInfo {
    ErrorInfo::IconTooLarge {
        megabytes: MAX_MEGABYTES,
    }
}

fn well_formed(generator: &IconGenerator) -> bool {
    let colors_hold = match &generator.background {
        IconBackground::Color { value } => is_hex_color(value),
        IconBackground::LinearTopDownGradient {
            top_color,
            bottom_color,
        } => is_hex_color(top_color) && is_hex_color(bottom_color),
    };
    let symbol = &generator.symbol;
    colors_hold
        && !symbol.is_empty()
        && symbol.len() <= MAX_SYMBOL_LEN
        && symbol
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_hex_color(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const GIF: &[u8] = b"GIF89a\x01\0\x01\0";

    fn generator() -> IconGenerator {
        IconGenerator {
            background: IconBackground::LinearTopDownGradient {
                top_color: "#a1b2c3".to_string(),
                bottom_color: "#000000".to_string(),
            },
            symbol: "grass_block".to_string(),
        }
    }

    #[test]
    fn a_picked_image_replaces_a_generated_icon_and_its_generator() {
        let dir = tempfile::tempdir().unwrap();
        let generated = write(dir.path(), PNG, Some(&generator())).unwrap();
        assert_eq!(generated.generator, Some(generator()));

        let picked = write(dir.path(), GIF, None).unwrap();
        assert_eq!(picked.path, dir.path().join("icon.gif"));
        assert_eq!(picked.generator, None);
        assert!(!dir.path().join("icon.png").exists());
        assert!(!dir.path().join(IconGenerator::NAME).exists());
    }

    #[test]
    fn an_icon_is_judged_by_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let error = write(
            dir.path(),
            b"<svg xmlns='http://www.w3.org/2000/svg'/>",
            None,
        )
        .unwrap_err()
        .downcast::<ErrorInfo>()
        .unwrap();
        assert_eq!(error, ErrorInfo::IconNotAnImage);
        assert!(read(dir.path()).is_none());
    }

    #[test]
    fn a_generator_that_would_reach_a_stylesheet_unchecked_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), PNG, Some(&generator())).unwrap();
        let hostile = IconGenerator {
            background: IconBackground::Color {
                value: "red), url(https://example.invalid/track".to_string(),
            },
            symbol: "grass_block".to_string(),
        };
        schema::save(&dir.path().join(IconGenerator::NAME), &hostile).unwrap();
        assert_eq!(read(dir.path()).unwrap().generator, None);
        assert!(write(dir.path(), PNG, Some(&hostile)).is_err());
    }
}
