//! What a project's manifest and component say, through the kernel's own
//! rules: the same parse, shape, version gates and grant derivation Hestia
//! applies at install.

use kernel::{Manifest, Shape, Table, KERNEL_PACKAGE};
use proto::plugin::Grant;

use super::lock::{self, Lock};
use super::project::Source;

#[derive(Default)]
pub struct Analysis {
    pub manifest: Option<Manifest>,
    pub shape: Option<Shape>,
    pub grant: Option<Grant>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// The problems that can be found independently of the others: the manifest,
/// the component's shape, and the version gates and grant derivation, each of
/// which stops at its first problem. `dev` allows the unstable interfaces a dev
/// install may use. The grant holds every platform's programs and endpoints.
pub fn analyze(table: &Table, manifest: &[u8], wasm: &[u8], dev: bool) -> Analysis {
    let mut analysis = Analysis::default();
    match std::str::from_utf8(manifest)
        .map_err(|_| "plugin.toml is not UTF-8 text".to_owned())
        .and_then(|text| Manifest::parse(text).map_err(|error| error.to_string()))
    {
        Ok(parsed) => analysis.manifest = Some(parsed),
        Err(error) => analysis.errors.push(error),
    }
    match Shape::read(wasm) {
        Ok(shape) => analysis.shape = Some(shape),
        Err(error) => analysis.errors.push(error.to_string()),
    }
    let (Some(manifest), Some(shape)) = (&analysis.manifest, &analysis.shape) else {
        return analysis;
    };
    match kernel::check_versions(table, manifest, shape, dev) {
        Ok(warnings) => {
            analysis.warnings = warnings
                .iter()
                .map(|warning| format!("{warning}: {}", warning.hint()))
                .collect()
        }
        Err(error) => analysis.errors.push(error.to_string()),
    }
    match kernel::derive_everywhere(table, manifest, shape) {
        Ok(grant) => analysis.grant = Some(grant),
        Err(error) => analysis.errors.push(error.to_string()),
    }
    analysis
}

/// The version of the API the component imports, which `api` in the manifest
/// must name.
pub fn api_of(shape: &Shape) -> Option<String> {
    shape
        .imports
        .iter()
        .chain(&shape.exports)
        .filter(|name| {
            name.strip_prefix(KERNEL_PACKAGE)
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .find_map(|name| name.split_once('@').map(|(_, version)| version.to_owned()))
}

/// The lock the built component calls for.
pub fn lock_of(analysis: &Analysis) -> Option<Lock> {
    let (manifest, grant, shape) = (
        analysis.manifest.as_ref()?,
        analysis.grant.as_ref()?,
        analysis.shape.as_ref()?,
    );
    Some(Lock::of(manifest, grant, shape))
}

/// [`analyze`] of a project, with the lockfile held to the grant (or, with
/// `update`, rewritten to it): every problem, the lockfile's included, in one
/// list.
pub fn review(table: &Table, source: &Source, dev: bool, update: bool) -> Analysis {
    let mut analysis = analyze(table, &source.manifest, &source.wasm, dev);
    let Some(lock) = lock_of(&analysis) else {
        return analysis;
    };
    if update {
        if let Err(error) = lock::write(&source.dir, &lock) {
            analysis
                .errors
                .push(format!("cannot write plugin.grant: {error}"));
        }
    } else if let Some(mismatch) = lock::mismatch(&source.dir, &lock) {
        analysis.errors.push(mismatch);
    }
    analysis
}
