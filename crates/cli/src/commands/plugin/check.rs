use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Context, Result};

use super::repo::{store_meta, workspace_list, PluginDir, Repository, StoreMeta, PLUGINS_DIR};
use crate::ui::{self, View};

const SPEC: &str = "https://github.com/prytaneum/hestia/blob/main/docs/plugins/repository.md";
const MEMBERS_GLOB: &str = "plugins/*";
const LABEL_LENGTH: std::ops::RangeInclusive<usize> = 1..=4;

/// A repository that follows the shape.
pub struct Checked {
    pub meta: StoreMeta,
    pub plugins: Vec<PluginDir>,
}

pub fn run(dir: Option<&Path>) -> Result<()> {
    let repo = find(dir)?;
    let checked = check(&repo)?;
    ui::show(View::line(format!(
        "ok: '{}', {} plugin(s)",
        checked.meta.name,
        checked.plugins.len()
    )))
}

pub fn find(dir: Option<&Path>) -> Result<Repository> {
    let dir = match dir {
        Some(dir) => dir.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repository::containing(&dir).with_context(|| {
        format!("not inside a plugin repository (a Cargo.toml beside a {PLUGINS_DIR}/ folder)")
    })
}

/// Every way the repository departs from the shape, in one error.
pub fn check(repo: &Repository) -> Result<Checked> {
    let mut problems = Vec::new();
    let workspace = repo.workspace()?;

    let members = workspace_list(&workspace, "members");
    if members != [MEMBERS_GLOB] {
        problems.push(format!(
            "the workspace `members` must be [\"{MEMBERS_GLOB}\"], found {members:?}"
        ));
    }
    let meta = match store_meta(&workspace) {
        Some(Ok(meta)) => {
            check_meta(&meta, &mut problems);
            meta
        }
        Some(Err(error)) => {
            problems.push(format!("{error:#}"));
            StoreMeta::default()
        }
        None => {
            problems.push(
                "the workspace Cargo.toml has no [workspace.metadata.hestia] with name, label, website and key"
                    .to_string(),
            );
            StoreMeta::default()
        }
    };

    let plugins = match repo.plugins() {
        Ok(plugins) => plugins,
        Err(error) => {
            problems.push(format!("{error:#}"));
            return fail(problems);
        }
    };
    if plugins.is_empty() {
        problems.push(format!(
            "no plugins under {PLUGINS_DIR}/; add one with `hestia plugin new <id>`"
        ));
    }
    let excluded: BTreeSet<String> = workspace_list(&workspace, "exclude").into_iter().collect();
    let mut ids = BTreeSet::new();
    for plugin in &plugins {
        check_plugin(plugin, &excluded, &mut problems);
        if !ids.insert(plugin.manifest.id.as_str()) {
            problems.push(format!("id '{}' is declared twice", plugin.manifest.id));
        }
    }
    if problems.is_empty() {
        Ok(Checked { meta, plugins })
    } else {
        fail(problems)
    }
}

fn fail<T>(problems: Vec<String>) -> Result<T> {
    let list: String = problems
        .iter()
        .map(|problem| format!("\n  - {problem}"))
        .collect();
    bail!("the repository does not follow the shape:{list}\nsee {SPEC}")
}

fn check_meta(meta: &StoreMeta, problems: &mut Vec<String>) {
    if meta.name.trim().is_empty() {
        problems.push("[workspace.metadata.hestia] needs a `name`".to_string());
    }
    if !LABEL_LENGTH.contains(&meta.label.chars().count()) {
        problems.push("[workspace.metadata.hestia] `label` must be 1 to 4 characters".to_string());
    }
    let host = meta.website.strip_prefix("https://").unwrap_or_default();
    if host.is_empty() || host.starts_with('/') || host.contains(char::is_whitespace) {
        problems.push("[workspace.metadata.hestia] `website` must be an https URL".to_string());
    }
    if meta.key.trim().is_empty() {
        problems.push("[workspace.metadata.hestia] needs the signing `key`".to_string());
    } else if minisign::PublicKey::from_base64(meta.key.trim()).is_err() {
        problems.push(
            "[workspace.metadata.hestia] `key` is not a minisign public key; run `hestia plugin keygen`"
                .to_string(),
        );
    }
}

fn check_plugin(plugin: &PluginDir, excluded: &BTreeSet<String>, problems: &mut Vec<String>) {
    let dir = &plugin.dir_name;
    let path = format!("{PLUGINS_DIR}/{dir}");
    if plugin.manifest.id != *dir {
        problems.push(format!(
            "{path} declares id '{}'; the folder name must equal the id",
            plugin.manifest.id
        ));
    }
    let mut manifest = plugin.manifest.clone();
    // A source manifest never carries `api`: `pack` writes it from the resolved SDK.
    if manifest.wasm.is_some() && manifest.api.is_none() {
        manifest.api = Some(format!("^{}", proto::plugin::API_VERSION));
    }
    if let Err(error) = manifest.validate() {
        problems.push(format!("{path}/plugin.toml: {error}"));
    }
    match (plugin.has_crate, excluded.contains(&path)) {
        (false, false) => problems.push(format!(
            "{path} has no Cargo.toml, so it is not a crate: add \"{path}\" to the workspace `exclude`"
        )),
        (true, true) => problems.push(format!(
            "{path} has a Cargo.toml but is listed in the workspace `exclude`; remove it from the list"
        )),
        _ => {}
    }
    if manifest.wasm.is_some() && !plugin.has_crate {
        problems.push(format!(
            "{path} declares a wasm component but has no Cargo.toml"
        ));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const META: &str = "\n[workspace.metadata.hestia]\nname = \"Sample\"\nlabel = \"SMP\"\nwebsite = \"https://example.com\"\nkey = \"RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\"\n";

    pub fn repo_with(workspace_extra: &str, exclude: &[&str]) -> (tempfile::TempDir, Repository) {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(PLUGINS_DIR)).unwrap();
        let exclude = format!("{exclude:?}");
        std::fs::write(
            root.path().join("Cargo.toml"),
            format!("[workspace]\nresolver = \"2\"\nmembers = [\"plugins/*\"]\nexclude = {exclude}\n{workspace_extra}"),
        )
        .unwrap();
        let repo = Repository::containing(root.path()).unwrap();
        (root, repo)
    }

    pub fn add_plugin(repo: &Repository, dir: &str, id: &str, crate_toml: bool) {
        let path = repo.plugin_path(dir);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("plugin.toml"),
            format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\n"),
        )
        .unwrap();
        if crate_toml {
            std::fs::write(path.join("Cargo.toml"), "").unwrap();
        }
    }

    pub fn valid_meta() -> &'static str {
        META
    }

    fn message(repo: &Repository) -> String {
        format!("{:#}", check(repo).err().expect("check must fail"))
    }

    #[test]
    fn a_valid_repository_passes() {
        let (_root, repo) = repo_with(META, &["plugins/themes"]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "themes", "themes", false);

        let checked = check(&repo).unwrap();

        assert_eq!(checked.meta.label, "SMP");
        assert_eq!(checked.plugins.len(), 2);
    }

    #[test]
    fn a_folder_that_differs_from_its_id_is_refused() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "bb", true);

        assert!(message(&repo).contains("folder name must equal the id"));
    }

    #[test]
    fn a_duplicate_id_is_refused() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "bb", "aa", true);

        assert!(message(&repo).contains("id 'aa' is declared twice"));
    }

    #[test]
    fn a_workspace_without_the_members_glob_is_refused() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(PLUGINS_DIR)).unwrap();
        std::fs::write(
            root.path().join("Cargo.toml"),
            format!("[workspace]\nmembers = []\n{META}"),
        )
        .unwrap();
        let repo = Repository::containing(root.path()).unwrap();
        add_plugin(&repo, "aa", "aa", true);

        assert!(message(&repo).contains("`members` must be"));
    }

    #[test]
    fn a_theme_only_plugin_must_be_excluded_and_a_crate_must_not() {
        let (_root, repo) = repo_with(META, &["plugins/aa"]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "themes", "themes", false);

        let message = message(&repo);

        assert!(message.contains("add \"plugins/themes\" to the workspace `exclude`"));
        assert!(message.contains("remove it from the list"));
    }

    #[test]
    fn missing_or_invalid_store_metadata_is_refused() {
        let (_root, repo) = repo_with("", &[]);
        add_plugin(&repo, "aa", "aa", true);
        assert!(message(&repo).contains("no [workspace.metadata.hestia]"));

        let (_root, repo) = repo_with(
            "\n[workspace.metadata.hestia]\nname = \"S\"\nlabel = \"TOOLONG\"\nwebsite = \"http://x.io\"\n",
            &[],
        );
        add_plugin(&repo, "aa", "aa", true);
        let message = message(&repo);
        assert!(message.contains("`label` must be 1 to 4"));
        assert!(message.contains("`website` must be an https URL"));
        assert!(message.contains("signing `key`"));
    }
}
