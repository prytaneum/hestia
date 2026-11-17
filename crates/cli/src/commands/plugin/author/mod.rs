//! The commands of a plugin author: they work on a project directory and need
//! no daemon, except `dev`, which installs the project into one.

mod check;
mod dev;
mod inspect;
mod key;
mod lock;
mod log;
mod pack;
mod project;
mod publish;
mod repo;
mod scaffold;
mod suite;
mod world;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum AuthorCmd {
    /// Start a plugin project from a template
    New(scaffold::NewArgs),
    /// Check the manifest, the component and plugin.grant, reporting every problem
    Check(check::CheckArgs),
    /// What a component imports, exports and asks for
    World(world::WorldArgs),
    /// Install a project as a dev install, rebuilding and reloading it on change
    Dev(dev::DevArgs),
    /// Run the project's own tests
    Test(suite::TestArgs),
    /// Build the .hplugin package
    Pack(pack::PackArgs),
    /// Add a signed package to a repository's index
    Publish(publish::PublishArgs),
}

/// Runs a command that builds or reads files off the async executor.
async fn blocking(command: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    tokio::task::spawn_blocking(command)
        .await
        .context("the command stopped")?
}

pub async fn run(cmd: AuthorCmd) -> Result<()> {
    match cmd {
        AuthorCmd::New(args) => blocking(move || scaffold::run(args)).await,
        AuthorCmd::Check(args) => blocking(move || check::run(args)).await,
        AuthorCmd::World(args) => blocking(move || world::run(args)).await,
        AuthorCmd::Dev(args) => dev::run(args).await,
        AuthorCmd::Test(args) => blocking(move || suite::run(args)).await,
        AuthorCmd::Pack(args) => blocking(move || pack::run(args)).await,
        AuthorCmd::Publish(args) => blocking(move || publish::run(args)).await,
    }
}
