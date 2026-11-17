//! `hestia plugin check`: the manifest, the component and `plugin.grant`, held
//! to the rules Hestia applies at install.

use anyhow::{bail, Result};
use clap::Args;

use super::inspect::{review, Analysis};
use super::project::{Where, LOCK};
use crate::ui::{self, View};

#[derive(Args)]
pub struct CheckArgs {
    #[command(flatten)]
    source: Where,
    /// Rewrite plugin.grant to what the built component asks for
    #[arg(long)]
    update_grant: bool,
}

/// Prints what an analysis found: warnings, then every error, failing on any.
pub fn report(analysis: &Analysis) -> Result<()> {
    for warning in &analysis.warnings {
        ui::show(View::note(warning))?;
    }
    for error in &analysis.errors {
        ui::show(View::warning(error))?;
    }
    match analysis.errors.len() {
        0 => Ok(()),
        1 => bail!("1 problem"),
        count => bail!("{count} problems"),
    }
}

pub fn run(args: CheckArgs) -> Result<()> {
    let source = args.source.load()?;
    let analysis = review(&kernel::table(), &source, false, args.update_grant);
    report(&analysis)?;
    ui::show(View::line(match args.update_grant {
        true => format!("wrote {LOCK}"),
        false => "ok".to_owned(),
    }))
}
