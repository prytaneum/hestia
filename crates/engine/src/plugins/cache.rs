use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use wasmtime::component::Component;
use wasmtime::Engine;

use crate::checksum::hex;

/// `DefaultHasher` output changes between Rust releases, which would rename
/// every cached artifact.
struct StableHasher(Sha256);

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn finish(&self) -> u64 {
        let digest = self.0.clone().finalize();
        u64::from_be_bytes(digest[..8].try_into().expect("sha256 yields 32 bytes"))
    }
}

/// Compiled components under `meta/plugins/`, regenerable like the rest of
/// `meta/` ([0054](../../../../docs/decisions/0054-meta-root-for-materialised-files.md)).
/// The artifact's own sha256 guards against corruption only; a local writer of
/// the data home is out of scope ([0091](../../../../docs/decisions/0091-plugins-are-wasm-components.md)).
pub(super) struct ComponentCache {
    dir: PathBuf,
    stamp: String,
}

impl ComponentCache {
    pub(super) fn new(dir: PathBuf, engine: &Engine) -> Self {
        let mut hasher = StableHasher(Sha256::new());
        engine.precompile_compatibility_hash().hash(&mut hasher);
        Self::stamped(dir, hasher.finish())
    }

    fn stamped(dir: PathBuf, config_hash: u64) -> Self {
        ComponentCache {
            dir,
            stamp: format!("{config_hash:016x}"),
        }
    }

    /// `wasm` must already be verified against `wasm_sha256`. Falls back to
    /// compiling whenever the cached artifact is missing or unusable.
    pub(super) fn component(
        &self,
        engine: &Engine,
        wasm_sha256: &str,
        wasm: &[u8],
    ) -> wasmtime::Result<Component> {
        let artifact = self.dir.join(format!("{wasm_sha256}-{}.cwasm", self.stamp));
        let digest = self
            .dir
            .join(format!("{wasm_sha256}-{}.cwasm.sha256", self.stamp));
        if let Some(component) = Self::cached(engine, &artifact, &digest) {
            return Ok(component);
        }
        let component = Component::new(engine, wasm)?;
        if let Err(error) = self.store(&component, &artifact, &digest) {
            tracing::warn!(artifact = %artifact.display(), error = %format!("{error:#}"), "cannot cache the compiled plugin");
            discard(&artifact, &digest);
        }
        Ok(component)
    }

    fn cached(engine: &Engine, artifact: &Path, digest: &Path) -> Option<Component> {
        let bytes = std::fs::read(artifact).ok()?;
        let recorded = std::fs::read_to_string(digest).unwrap_or_default();
        if hex(&Sha256::digest(&bytes)) != recorded.trim() {
            tracing::warn!(artifact = %artifact.display(), "cached plugin artifact is corrupt; recompiling");
            discard(artifact, digest);
            return None;
        }
        // SAFETY: `deserialize` trusts its input to be wasmtime's own output.
        // The bytes were written by this daemon and match their recorded hash,
        // which rules out corruption; tampering by a writer of the data home
        // is out of scope (0091).
        match unsafe { Component::deserialize(engine, &bytes) } {
            Ok(component) => Some(component),
            Err(error) => {
                tracing::warn!(artifact = %artifact.display(), error = %format!("{error:#}"), "cached plugin artifact is unusable; recompiling");
                discard(artifact, digest);
                None
            }
        }
    }

    fn store(&self, component: &Component, artifact: &Path, digest: &Path) -> anyhow::Result<()> {
        let bytes = component
            .serialize()
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        std::fs::create_dir_all(&self.dir)?;
        common::atomic::write_private(artifact, &bytes)?;
        common::atomic::write_private(digest, hex(&Sha256::digest(&bytes)).as_bytes())?;
        Ok(())
    }
}

fn discard(artifact: &Path, digest: &Path) {
    let _ = std::fs::remove_file(artifact);
    let _ = std::fs::remove_file(digest);
}

#[cfg(test)]
mod tests {
    use super::*;

    const WASM: &str = "(component)";

    fn artifacts(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "cwasm"))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_truncated_artifact_is_replaced_by_a_fresh_compile() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::default();
        let cache = ComponentCache::stamped(dir.path().into(), 0);
        cache.component(&engine, "abc", WASM.as_bytes()).unwrap();
        let artifact = artifacts(dir.path()).remove(0);
        let whole = std::fs::read(&artifact).unwrap();
        std::fs::write(&artifact, &whole[..whole.len() / 2]).unwrap();

        cache.component(&engine, "abc", WASM.as_bytes()).unwrap();

        assert_eq!(std::fs::read(&artifact).unwrap(), whole);
    }

    #[test]
    fn a_different_compatibility_hash_misses_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::default();
        ComponentCache::stamped(dir.path().into(), 1)
            .component(&engine, "abc", WASM.as_bytes())
            .unwrap();
        ComponentCache::stamped(dir.path().into(), 2)
            .component(&engine, "abc", WASM.as_bytes())
            .unwrap();

        assert_eq!(artifacts(dir.path()).len(), 2);
    }
}
