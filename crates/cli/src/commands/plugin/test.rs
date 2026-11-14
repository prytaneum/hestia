use anyhow::{bail, Result};

use super::cargo::{self, Metadata};
use super::check;
use super::repo::PLUGINS_DIR;
use super::sdk::{Sdk, SdkOption};

/// Runs the native tests of the named plugins of the repository around the
/// current directory, or of all of them. A plugin with no crate has none.
pub fn run(ids: &[String], sdk: &SdkOption) -> Result<()> {
    let sdk = Sdk::resolve(sdk)?;
    let repo = check::find(None)?;
    let inspection = check::inspect(&repo)?;
    inspection.require_shape()?;
    let inspection = inspection.select(ids)?;
    let crates: Vec<_> = inspection
        .plugins
        .iter()
        .filter_map(|check| check.plugin.as_ref())
        .filter(|plugin| plugin.has_crate)
        .collect();
    if crates.is_empty() {
        bail!("no plugin crates under {PLUGINS_DIR}/ to test");
    }
    let metadata = Metadata::read(repo.root(), sdk.as_ref())?;
    let packages = crates
        .iter()
        .map(|plugin| metadata.package_name(&plugin.path))
        .collect::<Result<Vec<_>>>()?;
    cargo::test(repo.root(), &packages, sdk.as_ref())
}
