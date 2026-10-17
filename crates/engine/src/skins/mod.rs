//! The skin library: PNG textures the user saved, kept under
//! `<data_home>/skins/` as `<key>.png` blobs beside a `library.json` index —
//! the disk is the registry, as with `java`. A row's key is Mojang's texture
//! hash once Mojang has seen the texture (an upload response reports it, and
//! the row is re-keyed to match), so equipped-detection at list time is a key
//! comparison. The Mojang profile operations live in `mojang`, the vanilla
//! default-skin table in `defaults`.

pub(crate) mod defaults;
pub(crate) mod mojang;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use proto::skins::SkinVariant;
use serde::{Deserialize, Serialize};

use self::mojang::Profile;
use crate::registry;
use crate::schema::Document;

// Absorbs bursts of skin.list reads; Mojang's profile API rate-limits hard.
const PROFILE_TTL: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub variant: SkinVariant,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LibraryFile {
    #[serde(default)]
    skins: Vec<LibraryEntry>,
}

impl Document for LibraryFile {
    const NAME: &'static str = "library.json";
}

pub struct Skins {
    dir: PathBuf,
    profiles: Mutex<HashMap<String, (Instant, Profile)>>,
}

impl Skins {
    pub fn new(dir: PathBuf) -> Self {
        Skins {
            dir,
            profiles: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn cached_profile(&self, account: &str) -> Option<Profile> {
        let profiles = self.profiles.lock().unwrap();
        let (stored, profile) = profiles.get(account)?;
        (stored.elapsed() < PROFILE_TTL).then(|| profile.clone())
    }

    pub(crate) fn store_profile(&self, account: &str, profile: Profile) {
        self.profiles
            .lock()
            .unwrap()
            .insert(account.to_string(), (Instant::now(), profile));
    }

    pub(crate) fn invalidate_profile(&self, account: &str) {
        self.profiles.lock().unwrap().remove(account);
    }

    fn dir(&self) -> &Path {
        &self.dir
    }

    fn blob_path(&self, key: &str) -> PathBuf {
        self.dir().join(format!("{key}.png"))
    }

    pub fn list(&self) -> Vec<LibraryEntry> {
        load(self.dir()).skins
    }

    pub fn entry(&self, key: &str) -> Option<LibraryEntry> {
        self.list().into_iter().find(|e| e.key == key)
    }

    pub fn texture(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(key);
        fs::read(&path).with_context(|| format!("cannot read skin texture {}", path.display()))
    }

    /// Save `png` under a known texture key (Mojang's hash), validating the
    /// dimensions. An existing row with the same key is updated in place;
    /// otherwise the entry is inserted newest-first.
    pub fn add_keyed(
        &self,
        key: &str,
        png: &[u8],
        variant: SkinVariant,
        name: &str,
    ) -> Result<LibraryEntry> {
        validate_skin_png(png)?;
        let dir = self.dir();
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        fs::write(self.blob_path(key), png)?;

        let mut file = load(dir);
        let entry = LibraryEntry {
            key: key.to_string(),
            name: name.trim().to_string(),
            variant,
        };
        match file.skins.iter_mut().find(|e| e.key == key) {
            Some(existing) => {
                if !entry.name.is_empty() {
                    existing.name = entry.name.clone();
                }
                existing.variant = variant;
            }
            None => file.skins.insert(0, entry.clone()),
        }
        save(dir, &file)?;
        tracing::info!(key, "skin saved to the library");
        Ok(entry)
    }

    /// Re-key a row (and its blob) to the texture hash Mojang reported for the
    /// same PNG. A no-op when the keys already agree.
    pub fn rekey(&self, from: &str, to: &str) -> Result<()> {
        if from == to {
            return Ok(());
        }
        let dir = self.dir();
        let mut file = load(dir);
        let Some(entry) = file.skins.iter_mut().find(|e| e.key == from) else {
            return Ok(());
        };
        entry.key = to.to_string();
        // The target may already exist (the same texture added twice); the
        // rename overwrite plus a duplicate sweep collapses them into one row.
        fs::rename(self.blob_path(from), self.blob_path(to))?;
        let mut seen = false;
        file.skins.retain(|e| {
            let duplicate = e.key == to && std::mem::replace(&mut seen, true);
            !duplicate
        });
        save(dir, &file)?;
        tracing::debug!(from, to, "skin re-keyed to Mojang's texture hash");
        Ok(())
    }

    /// Record the model variant Mojang reports for a saved texture.
    pub fn sync_variant(&self, key: &str, variant: SkinVariant) -> Result<()> {
        let dir = self.dir();
        let mut file = load(dir);
        let Some(entry) = file.skins.iter_mut().find(|e| e.key == key) else {
            return Ok(());
        };
        if entry.variant == variant {
            return Ok(());
        }
        entry.variant = variant;
        save(dir, &file)
    }

    /// Rewrite a row's label and variant; `None` when no row matches.
    pub fn update(
        &self,
        key: &str,
        name: &str,
        variant: SkinVariant,
    ) -> Result<Option<LibraryEntry>> {
        let dir = self.dir();
        let mut file = load(dir);
        let Some(entry) = file.skins.iter_mut().find(|e| e.key == key) else {
            return Ok(None);
        };
        entry.name = name.trim().to_string();
        entry.variant = variant;
        let updated = entry.clone();
        save(dir, &file)?;
        tracing::info!(key, "skin library entry updated");
        Ok(Some(updated))
    }

    pub fn remove(&self, key: &str) -> Result<bool> {
        let dir = self.dir();
        let mut file = load(dir);
        let before = file.skins.len();
        file.skins.retain(|e| e.key != key);
        if file.skins.len() == before {
            return Ok(false);
        }
        save(dir, &file)?;
        let _ = fs::remove_file(self.blob_path(key));
        tracing::info!(key, "skin removed from the library");
        Ok(true)
    }
}

/// Reject anything that is not a skin-shaped PNG: 64×64, or the legacy 64×32.
pub(crate) fn validate_skin_png(png: &[u8]) -> Result<()> {
    let Some((width, height)) = png_dimensions(png) else {
        bail!(proto::error::ErrorInfo::TextureNotPng);
    };
    if width != 64 || !(height == 64 || height == 32) {
        bail!(proto::error::ErrorInfo::TextureSizeUnsupported { width, height });
    }
    Ok(())
}

/// Width/height from the PNG signature + IHDR header; `None` when the bytes
/// are not a PNG. Full decoding is Mojang's job — only the shape is checked.
fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 24 || data[..8] != SIGNATURE || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
    Some((width, height))
}

fn load(dir: &std::path::Path) -> LibraryFile {
    registry::read_record(dir).unwrap_or_default()
}

fn save(dir: &std::path::Path, file: &LibraryFile) -> Result<()> {
    registry::write_record(dir, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        data.extend_from_slice(&13u32.to_be_bytes());
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[8, 6, 0, 0, 0]);
        data
    }

    #[test]
    fn accepts_modern_and_legacy_skin_dimensions() {
        assert!(validate_skin_png(&png(64, 64)).is_ok());
        assert!(validate_skin_png(&png(64, 32)).is_ok());
        assert!(validate_skin_png(&png(64, 48)).is_err());
        assert!(validate_skin_png(&png(128, 64)).is_err());
        assert!(validate_skin_png(b"not a png").is_err());
    }

    #[test]
    fn profile_cache_stores_and_invalidates() {
        let dir = tempfile::tempdir().expect("temp dir");
        let skins = Skins::new(dir.path().to_path_buf());
        assert!(skins.cached_profile("u1").is_none());
        skins.store_profile("u1", Profile::default());
        assert!(skins.cached_profile("u1").is_some());
        assert!(skins.cached_profile("u2").is_none());
        skins.invalidate_profile("u1");
        assert!(skins.cached_profile("u1").is_none());
    }

    #[test]
    fn library_round_trip_and_rekey() {
        let dir = tempfile::tempdir().expect("temp dir");
        let skins = Skins::new(dir.path().join("skins"));

        let entry = skins
            .add_keyed("local-key", &png(64, 64), SkinVariant::Slim, "My Skin")
            .unwrap();
        assert_eq!(skins.list().len(), 1);
        assert_eq!(skins.texture(&entry.key).unwrap(), png(64, 64));

        skins.rekey(&entry.key, "mojang-key").unwrap();
        assert!(skins.entry(&entry.key).is_none());
        let rekeyed = skins.entry("mojang-key").unwrap();
        assert_eq!(rekeyed.name, "My Skin");
        assert_eq!(rekeyed.variant, SkinVariant::Slim);
        assert_eq!(skins.texture("mojang-key").unwrap(), png(64, 64));

        skins
            .sync_variant("mojang-key", SkinVariant::Classic)
            .unwrap();
        assert_eq!(
            skins.entry("mojang-key").unwrap().variant,
            SkinVariant::Classic
        );

        let updated = skins
            .update("mojang-key", "  Renamed  ", SkinVariant::Slim)
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.variant, SkinVariant::Slim);
        assert!(skins
            .update("missing-key", "x", SkinVariant::Classic)
            .unwrap()
            .is_none());

        assert!(skins.remove("mojang-key").unwrap());
        assert!(!skins.remove("mojang-key").unwrap());
        assert!(skins.list().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
