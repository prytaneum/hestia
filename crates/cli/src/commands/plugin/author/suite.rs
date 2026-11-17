//! `hestia plugin test`: the project's own tests, natively.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args;

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
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .arg("test")
        .args(&args.args)
        .current_dir(&args.dir)
        .status()
        .context("cannot run cargo")?;
    match status.success() {
        true => Ok(()),
        false => bail!("the tests failed"),
    }
}
