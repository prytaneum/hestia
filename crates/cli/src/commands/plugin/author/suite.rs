//! `hestia plugin test`: the project's own tests, natively.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;

use super::project::cargo;

#[derive(Args)]
pub struct TestArgs {
    /// The project directory
    #[arg(default_value = ".")]
    dir: PathBuf,
    /// Passed on to `cargo test`
    #[arg(last = true)]
    args: Vec<String>,
}

pub fn run(args: TestArgs) -> Result<()> {
    let status = cargo(&args.dir)
        .arg("test")
        .args(&args.args)
        .status()
        .context("cannot run cargo")?;
    match status.success() {
        true => Ok(()),
        false => bail!("the tests failed"),
    }
}
