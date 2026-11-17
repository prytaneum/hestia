use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hmac::{KeyInit, Mac};
use sha2::{Digest, Sha256};
use wasmtime::component::Component;
use wasmtime::Engine;

use super::error::SandboxError;

type Tag = hmac::Hmac<Sha256>;

const KEY_BYTES: usize = 32;
const TAG_BYTES: usize = 32;

/// Compiled components under `meta/plugins/`, one entry per plugin id. Loading
/// an entry runs it as native code in the daemon, so each is tagged with a
/// per-install key that never lives under `meta/`; an entry whose tag does not
/// cover this source, this engine and these bytes is recompiled, never loaded.
#[derive(Clone)]
pub(super) struct Cache {
    dir: PathBuf,
    key: Arc<[u8; KEY_BYTES]>,
}

impl Cache {
    pub fn open(dir: PathBuf, key_file: &Path) -> Result<Self, SandboxError> {
        Ok(Cache {
            dir,
            key: Arc::new(install_key(key_file)?),
        })
    }

    pub fn component(
        &self,
        engine: &Engine,
        plugin: &str,
        wasm: &[u8],
    ) -> Result<Component, SandboxError> {
        let path = self.path(plugin);
        let source = Sha256::digest(wasm);
        if let Some(compiled) = self.verified(&path, engine, &source) {
            // SAFETY: the tag proves this install's key signed these bytes as
            // the compilation of this source by an engine of this configuration.
            match unsafe { Component::deserialize(engine, &compiled) } {
                Ok(component) => return Ok(component),
                Err(error) => tracing::debug!(plugin, %error, "cached component is unusable"),
            }
        }
        let compiled = engine
            .precompile_component(wasm)
            .map_err(SandboxError::Load)?;
        let mut entry = self.tag(engine, &source, &compiled).to_vec();
        entry.extend_from_slice(&compiled);
        if let Err(error) = self.store(&path, &entry) {
            tracing::warn!(plugin, %error, "cannot cache compiled component");
        }
        // SAFETY: produced by `precompile_component` on this engine just above.
        unsafe { Component::deserialize(engine, &compiled) }.map_err(SandboxError::Load)
    }

    pub fn forget(&self, plugin: &str) {
        if let Err(error) = common::fsys::remove_file(self.path(plugin)) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(plugin, %error, "cannot remove cached component");
            }
        }
    }

    fn path(&self, plugin: &str) -> PathBuf {
        self.dir
            .join(format!("{}.cwasm", hex(&Sha256::digest(plugin))))
    }

    fn verified(&self, path: &Path, engine: &Engine, source: &[u8]) -> Option<Vec<u8>> {
        let mut entry = std::fs::read(path).ok()?;
        if entry.len() < TAG_BYTES {
            return None;
        }
        let compiled = entry.split_off(TAG_BYTES);
        self.mac(engine, source, &compiled)
            .verify_slice(&entry)
            .ok()
            .map(|()| compiled)
    }

    fn tag(&self, engine: &Engine, source: &[u8], compiled: &[u8]) -> [u8; TAG_BYTES] {
        self.mac(engine, source, compiled)
            .finalize()
            .into_bytes()
            .into()
    }

    fn mac(&self, engine: &Engine, source: &[u8], compiled: &[u8]) -> Tag {
        let mut mac = Tag::new_from_slice(&*self.key).expect("HMAC takes a key of any length");
        mac.update(source);
        engine
            .precompile_compatibility_hash()
            .hash(&mut MacHasher(&mut mac));
        mac.update(compiled);
        mac
    }

    fn store(&self, path: &Path, entry: &[u8]) -> std::io::Result<()> {
        common::fsys::create_private_dir(&self.dir)?;
        common::atomic::write_private(path, entry)
    }
}

/// Feeds a `Hash` value into the tag; `finish` is never read.
struct MacHasher<'a>(&'a mut Tag);

impl Hasher for MacHasher<'_> {
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn finish(&self) -> u64 {
        0
    }
}

/// Reads the install's key, or makes one. A key that cannot be saved still
/// serves this run; the next run recompiles.
fn install_key(file: &Path) -> Result<[u8; KEY_BYTES], SandboxError> {
    match std::fs::read(file) {
        Ok(bytes) => {
            if let Ok(key) = <[u8; KEY_BYTES]>::try_from(bytes) {
                return Ok(key);
            }
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!(%error, "cannot read the plugin cache key; using a key for this run only");
            return fresh_key();
        }
        Err(_) => {}
    }
    let key = fresh_key()?;
    let saved = file
        .parent()
        .map_or(Ok(()), common::fsys::create_private_dir)
        .and_then(|()| common::atomic::write_private(file, &key));
    if let Err(error) = saved {
        tracing::warn!(%error, "cannot save the plugin cache key");
    }
    Ok(key)
}

fn fresh_key() -> Result<[u8; KEY_BYTES], SandboxError> {
    let mut key = [0u8; KEY_BYTES];
    getrandom::fill(&mut key)
        .map_err(|error| SandboxError::Engine(wasmtime::format_err!("{error}")))?;
    Ok(key)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
