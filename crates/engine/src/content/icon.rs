//! The icon an archive carries for itself, for content no source recognises.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::imageops::FilterType;
use image::{ImageFormat, ImageReader, Limits};
use serde_json::Value;
use zip::ZipArchive;

const DATA_URI_PREFIX: &str = "data:image/png;base64,";
const PACK_ICON: &str = "pack.png";
const THUMBNAIL_SIDE: u32 = 64;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ICON_SIDE: u32 = 2048;
const MAX_DECODE_BYTES: u64 = 32 * 1024 * 1024;

type Declared = fn(&str) -> Option<String>;

// In the order a multi-loader jar should be read: the first manifest that
// names an icon wins.
const MANIFESTS: [(&str, Declared); 5] = [
    ("fabric.mod.json", fabric_icon),
    ("quilt.mod.json", quilt_icon),
    ("META-INF/neoforge.mods.toml", forge_logo),
    ("META-INF/mods.toml", forge_logo),
    ("mcmod.info", legacy_logo),
];

/// A PNG data URI of the archive's own icon, shrunk to a thumbnail; empty when
/// it carries none. The archive is untrusted, so the image is re-encoded rather
/// than passed through and every read is bounded.
pub(crate) fn embedded(path: &Path) -> String {
    match read(path) {
        Ok(Some(icon)) => icon,
        Ok(None) => String::new(),
        Err(e) => {
            tracing::debug!(
                file = %path.display(),
                error = %format!("{e:#}"),
                "cannot read the archive's embedded icon"
            );
            String::new()
        }
    }
}

fn read(path: &Path) -> Result<Option<String>> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut archive = ZipArchive::new(file).context("not a readable archive")?;
    let Some(entry) = icon_entry(&mut archive) else {
        return Ok(None);
    };
    let bytes = read_entry(&mut archive, &entry, MAX_ICON_BYTES)?;
    thumbnail(&bytes).map(Some)
}

fn icon_entry(archive: &mut ZipArchive<File>) -> Option<String> {
    let declared = MANIFESTS.iter().find_map(|(manifest, icon)| {
        let bytes = read_entry(archive, manifest, MAX_MANIFEST_BYTES).ok()?;
        icon(&String::from_utf8_lossy(&bytes))
    });
    declared
        .map(|entry| {
            entry
                .trim_start_matches("./")
                .trim_start_matches('/')
                .to_string()
        })
        .filter(|entry| archive.index_for_name(entry).is_some())
        .or_else(|| {
            archive
                .index_for_name(PACK_ICON)
                .map(|_| PACK_ICON.to_string())
        })
}

fn read_entry(archive: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let entry = archive.by_name(name)?;
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("'{name}' is larger than {limit} bytes");
    }
    Ok(bytes)
}

fn thumbnail(bytes: &[u8]) -> Result<String> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_ICON_SIDE);
    limits.max_image_height = Some(MAX_ICON_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    reader.limits(limits);
    let image = reader.decode().context("the icon is not a readable PNG")?;
    let image = match image.width().max(image.height()) > THUMBNAIL_SIDE {
        true => image.resize(THUMBNAIL_SIDE, THUMBNAIL_SIDE, FilterType::Triangle),
        false => image,
    };
    let mut png = Vec::new();
    image.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)?;
    Ok(format!("{DATA_URI_PREFIX}{}", STANDARD.encode(png)))
}

fn fabric_icon(manifest: &str) -> Option<String> {
    let manifest: Value = serde_json::from_str(manifest).ok()?;
    sized_icon(manifest.get("icon")?)
}

fn quilt_icon(manifest: &str) -> Option<String> {
    let manifest: Value = serde_json::from_str(manifest).ok()?;
    sized_icon(manifest.get("quilt_loader")?.get("metadata")?.get("icon")?)
}

/// Fabric and Quilt name either one path or a map of pixel width to path.
fn sized_icon(icon: &Value) -> Option<String> {
    match icon {
        Value::String(path) => Some(path.clone()),
        Value::Object(sizes) => sizes
            .iter()
            .max_by_key(|(size, _)| size.parse::<u32>().unwrap_or(0))
            .and_then(|(_, path)| path.as_str())
            .map(str::to_string),
        _ => None,
    }
}

fn forge_logo(manifest: &str) -> Option<String> {
    let manifest: toml::Table = manifest.parse().ok()?;
    let of_mod = || manifest.get("mods")?.as_array()?.first()?.get("logoFile");
    manifest
        .get("logoFile")
        .or_else(of_mod)?
        .as_str()
        .map(str::to_string)
}

/// `mcmod.info` is a bare list of mods, or from its second revision an object
/// holding that list.
fn legacy_logo(manifest: &str) -> Option<String> {
    let manifest: Value = serde_json::from_str(manifest).ok()?;
    let mods = manifest.get("modList").unwrap_or(&manifest);
    let logo = mods.as_array()?.first()?.get("logoFile")?.as_str()?;
    (!logo.is_empty()).then(|| logo.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use image::{DynamicImage, GenericImageView};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    use super::*;

    fn png(side: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::new_rgba8(side, side)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn archive(entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let path = tempfile::Builder::new()
            .prefix("hestia-icon-")
            .suffix(".jar")
            .tempfile()
            .expect("temp file");
        let mut zip = ZipWriter::new(File::create(path.path()).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn side_of(icon: &str) -> u32 {
        let png = STANDARD
            .decode(icon.strip_prefix(DATA_URI_PREFIX).expect("a PNG data URI"))
            .unwrap();
        image::load_from_memory(&png).unwrap().dimensions().0
    }

    #[test]
    fn each_manifest_leads_to_the_icon_it_declares() {
        let manifests: [(&str, &str); 6] = [
            ("fabric.mod.json", r#"{"icon":"assets/demo/icon.png"}"#),
            (
                "fabric.mod.json",
                r#"{"icon":{"16":"small.png","128":"assets/demo/icon.png"}}"#,
            ),
            (
                "quilt.mod.json",
                r#"{"quilt_loader":{"metadata":{"icon":"/assets/demo/icon.png"}}}"#,
            ),
            (
                "META-INF/mods.toml",
                "[[mods]]\nmodId=\"demo\"\nlogoFile=\"assets/demo/icon.png\"\n",
            ),
            (
                "META-INF/neoforge.mods.toml",
                "logoFile=\"assets/demo/icon.png\"\n[[mods]]\nmodId=\"demo\"\n",
            ),
            ("mcmod.info", r#"[{"logoFile":"assets/demo/icon.png"}]"#),
        ];
        let icon = png(8);
        for (manifest, body) in manifests {
            let jar = archive(&[(manifest, body.as_bytes()), ("assets/demo/icon.png", &icon)]);
            assert_eq!(side_of(&embedded(jar.path())), 8, "{manifest}: {body}");
        }
    }

    #[test]
    fn a_pack_is_read_by_its_pack_png_and_shrunk() {
        let icon = png(256);
        let pack = archive(&[("pack.mcmeta", b"{}"), ("pack.png", &icon)]);
        assert_eq!(side_of(&embedded(pack.path())), THUMBNAIL_SIDE);
    }

    #[test]
    fn an_archive_without_a_usable_icon_has_none() {
        let dangling = archive(&[("fabric.mod.json", br#"{"icon":"missing.png"}"#)]);
        assert_eq!(embedded(dangling.path()), "");

        let not_an_image = archive(&[("pack.png", b"<svg/>")]);
        assert_eq!(embedded(not_an_image.path()), "");

        let oversized = png(MAX_ICON_SIDE + 1);
        let bomb = archive(&[("pack.png", &oversized)]);
        assert_eq!(embedded(bomb.path()), "");
    }
}
