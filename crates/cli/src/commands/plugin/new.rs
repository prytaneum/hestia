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
const WORKFLOWS: [(&str, &str); 3] = [
    ("ci.yml", include_str!("templates/ci.yml.tmpl")),
    ("index.yml", include_str!("templates/index.yml.tmpl")),
    ("release.yml", include_str!("templates/release.yml.tmpl")),
];

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
    scaffold_repository(&root, id)?;
    ui::show(View::line(format!(
        "created {id}/ with plugin '{id}'; cd {id} && hestia plugin pack"
    )))
}

fn scaffold_repository(root: &Path, id: &str) -> Result<()> {
    let workflows = root.join(".github").join("workflows");
    std::fs::create_dir_all(root.join(PLUGINS_DIR))?;
    std::fs::create_dir_all(&workflows)?;
    write(&root.join(CRATE_MANIFEST), &WORKSPACE.replace("{name}", id))?;
    write(&root.join(".gitignore"), GITIGNORE)?;
    write(&root.join("README.md"), README)?;
    for (name, content) in WORKFLOWS {
        write(&workflows.join(name), content)?;
    }
    let repo = Repository::containing(root).context("the new repository is not recognisable")?;
    scaffold_plugin(&repo, id)
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
    template.replace("{id}", id)
}

fn write(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_repository_carries_the_template_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("hello");

        scaffold_repository(&root, "hello").unwrap();

        for file in [
            "Cargo.toml",
            ".gitignore",
            "README.md",
            ".github/workflows/ci.yml",
            ".github/workflows/index.yml",
            ".github/workflows/release.yml",
            "plugins/hello/plugin.toml",
            "plugins/hello/Cargo.toml",
            "plugins/hello/src/lib.rs",
        ] {
            assert!(root.join(file).is_file(), "{file} is missing");
        }
    }
}
