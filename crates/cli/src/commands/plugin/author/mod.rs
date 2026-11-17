//! The commands of a plugin author: they work on a project directory and need
//! no daemon, except `dev`, which installs the project into one.

mod check;
mod dev;
mod inspect;
mod key;
mod lock;
mod pack;
mod project;
mod publish;
mod scaffold;
mod suite;
mod world;

#[cfg(test)]
mod tests;

use anyhow::Result;
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

pub async fn run(cmd: AuthorCmd) -> Result<()> {
    match cmd {
        AuthorCmd::New(args) => scaffold::run(args),
        AuthorCmd::Check(args) => check::run(args),
        AuthorCmd::World(args) => world::run(args),
        AuthorCmd::Dev(args) => dev::run(args).await,
        AuthorCmd::Test(args) => suite::run(args),
        AuthorCmd::Pack(args) => pack::run(args),
        AuthorCmd::Publish(args) => publish::run(args),
    }
}
