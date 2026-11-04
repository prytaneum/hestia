use std::path::Path;

use anyhow::{bail, Context, Result};
use proto::plugin::Manifest;

use super::repo::{Repository, CRATE_MANIFEST, MANIFEST, PLUGINS_DIR};
use crate::ui::{self, View};

const WORKSPACE: &str = include_str!("templates/workspace.toml.tmpl");
const GITIGNORE: &str = include_str!("templates/gitignore.tmpl");
const README: &str = include_str!("templates/README.md.tmpl");
const PLUGIN_MANIFEST: &str = include_str!("templates/plugin.toml.tmpl");
const CRATE: &str = include_str!("templates/crate.toml.tmpl");
const LIB: &str = include_str!("templates/lib.rs.tmpl");

/// Adds `plugins/<id>/` to the repository around the current directory, or
/// creates a new repository in `./<id>/` when there is none.
pub fn run(id: &str) -> Result<()> {
    validate_id(id)?;
    let cwd = std::env::current_dir()?;
    if let Some(repo) = Repository::containing(&cwd) {
        scaffold_plugin(&repo, id)?;
        return ui::show(View::line(format!("added {PLUGINS_DIR}/{id}")));
    }

    let root = cwd.join(id);
    if root.exists() {
        bail!("{} already exists", root.display());
    }
    std::fs::create_dir_all(root.join(PLUGINS_DIR))?;
    write(&root.join(CRATE_MANIFEST), WORKSPACE)?;
    write(&root.join(".gitignore"), GITIGNORE)?;
    write(&root.join("README.md"), README)?;
    let repo = Repository::containing(&root).context("the new repository is not recognisable")?;
    scaffold_plugin(&repo, id)?;
    ui::show(View::line(format!(
        "created {id}/ with plugin '{id}'; cd {id} && hestia plugin pack"
    )))
}

fn validate_id(id: &str) -> Result<()> {
    Manifest {
        id: id.to_string(),
        version: "0.1.0".to_string(),
        ..Manifest::default()
    }
    .validate()?;
    if !id.starts_with(|first: char| first.is_ascii_lowercase()) {
        bail!("plugin id `{id}` must start with a letter: it is also the crate name");
    }
    Ok(())
}

fn scaffold_plugin(repo: &Repository, id: &str) -> Result<()> {
    let dir = repo.plugin_path(id);
    if dir.exists() {
        bail!("{PLUGINS_DIR}/{id} already exists");
    }
    std::fs::create_dir_all(dir.join("src"))?;
    write(&dir.join(MANIFEST), &render(PLUGIN_MANIFEST, id))?;
    write(&dir.join(CRATE_MANIFEST), &render(CRATE, id))?;
    write(&dir.join("src").join("lib.rs"), &render(LIB, id))?;
    tracing::info!(plugin = id, path = %dir.display(), "plugin scaffolded");
    Ok(())
}

fn render(template: &str, id: &str) -> String {
    template
        .replace("{id}", id)
        .replace("{sdk_tag}", &format!("v{}", common::app::VERSION))
}

fn write(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("cannot write {}", path.display()))
}
