//! `hestia plugin world`: what a component imports, exports and asks for.

use anyhow::Result;
use clap::Args;

use super::check::report;
use super::inspect::analyze;
use super::project::Where;
use crate::ui::{self, View};

#[derive(Args)]
pub struct WorldArgs {
    #[command(flatten)]
    source: Where,
}

pub fn run(args: WorldArgs) -> Result<()> {
    let source = args.source.load()?;
    let analysis = analyze(&kernel::table(), &source.manifest, &source.wasm, true);
    if let Some(shape) = &analysis.shape {
        let rows = [("import", &shape.imports), ("export", &shape.exports)]
            .into_iter()
            .flat_map(|(kind, names)| {
                names
                    .iter()
                    .map(move |name| vec![kind.to_owned(), name.clone()])
            })
            .collect();
        ui::show(View::table("interfaces", ["", "INTERFACE"], rows))?;
    }
    if let Some(grant) = &analysis.grant {
        let rows = grant
            .permissions
            .iter()
            .map(|p| {
                let optional = if p.optional { "optional" } else { "required" };
                vec![p.permission.clone(), optional.to_owned(), p.prompt.clone()]
            })
            .collect();
        match grant.permissions.is_empty() {
            true => ui::show(View::note("it asks for no permissions"))?,
            false => ui::show(View::table(
                "permissions",
                ["PERMISSION", "", "ALLOWS"],
                rows,
            ))?,
        }
        let wanted: Vec<String> = grant
            .requires
            .iter()
            .cloned()
            .chain(
                grant
                    .dependencies
                    .iter()
                    .map(|dependency| format!("{} {}", dependency.id, dependency.range)),
            )
            .collect();
        if !wanted.is_empty() {
            ui::show(View::line(format!("it needs: {}", wanted.join(", "))))?;
        }
    }
    report(&analysis)
}
