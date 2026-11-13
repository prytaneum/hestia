use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context, Result};

use super::repo::{store_meta, workspace_list, PluginDir, Repository, StoreMeta, PLUGINS_DIR};
use crate::ui::{self, View};

const SPEC: &str = "https://github.com/prytaneum/hestia/blob/main/docs/plugins/repository.md";
const MEMBERS_GLOB: &str = "plugins/*";
const LABEL_LENGTH: std::ops::RangeInclusive<usize> = 1..=4;

/// The valid plugins of a repository that follows the shape, and the folders
/// left out because they are not valid.
pub struct Checked {
    pub meta: StoreMeta,
    pub plugins: Vec<PluginDir>,
    pub skipped: Vec<String>,
}

/// One folder under `plugins/` and everything wrong with it.
pub struct PluginCheck {
    pub dir_name: String,
    pub plugin: Option<PluginDir>,
    pub problems: Vec<String>,
}

impl PluginCheck {
    pub fn is_valid(&self) -> bool {
        self.problems.is_empty()
    }
}

/// What a check found: problems of the repository as a whole, and a result for
/// each plugin. A plugin's problems never hide another plugin's result.
pub struct Inspection {
    pub meta: StoreMeta,
    pub repo_problems: Vec<String>,
    pub plugins: Vec<PluginCheck>,
}

impl Inspection {
    /// Keeps the named plugins, or all of them when `ids` is empty. A name that
    /// is not a folder under `plugins/` is an error.
    pub fn select(mut self, ids: &[String]) -> Result<Inspection> {
        if let Some(unknown) = ids
            .iter()
            .find(|id| !self.plugins.iter().any(|check| check.dir_name == **id))
        {
            let known: Vec<&str> = self
                .plugins
                .iter()
                .map(|check| check.dir_name.as_str())
                .collect();
            bail!(
                "no plugin '{unknown}' under {PLUGINS_DIR}/ (found: {})",
                known.join(", ")
            );
        }
        if !ids.is_empty() {
            self.plugins.retain(|check| ids.contains(&check.dir_name));
        }
        Ok(self)
    }

    /// Fails when the repository as a whole departs from the shape: nothing can
    /// be packed or published from it.
    pub fn require_shape(&self) -> Result<()> {
        if self.repo_problems.is_empty() {
            Ok(())
        } else {
            fail(&self.repo_problems)
        }
    }

    pub fn checked(&self) -> Checked {
        Checked {
            meta: self.meta.clone(),
            plugins: self
                .plugins
                .iter()
                .filter(|check| check.is_valid())
                .filter_map(|check| check.plugin.clone())
                .collect(),
            skipped: self
                .plugins
                .iter()
                .filter(|check| !check.is_valid())
                .map(|check| check.dir_name.clone())
                .collect(),
        }
    }

    pub fn invalid(&self) -> Vec<&PluginCheck> {
        self.plugins
            .iter()
            .filter(|check| !check.is_valid())
            .collect()
    }
}

pub fn run(dir: Option<&Path>, ids: &[String]) -> Result<()> {
    let repo = find(dir)?;
    let inspection = inspect(&repo)?.select(ids)?;
    for check in &inspection.plugins {
        let path = format!("{PLUGINS_DIR}/{}", check.dir_name);
        if check.is_valid() {
            ui::show(View::line(format!("ok: {path}")))?;
        } else {
            ui::show(View::line(failure_text(check)))?;
        }
    }
    let failed = inspection.invalid().len();
    if !inspection.repo_problems.is_empty() {
        inspection.require_shape()?;
    }
    if failed > 0 {
        bail!("{failed} of {} plugin(s) failed", inspection.plugins.len());
    }
    ui::show(View::line(format!(
        "ok: '{}', {} plugin(s)",
        inspection.meta.name,
        inspection.plugins.len()
    )))
}

/// Reports every invalid plugin and fails; `action` is what the run could not
/// do for them ("packed", "published").
pub fn fail_invalid(invalid: &[&PluginCheck], action: &str) -> Result<()> {
    if invalid.is_empty() {
        return Ok(());
    }
    for check in invalid {
        ui::show(View::warning(failure_text(check)))?;
    }
    let names: Vec<&str> = invalid
        .iter()
        .map(|check| check.dir_name.as_str())
        .collect();
    bail!(
        "not {action}: {} (fix them and run again)",
        names.join(", ")
    )
}

fn failure_text(check: &PluginCheck) -> String {
    let list: String = check
        .problems
        .iter()
        .map(|problem| format!("\n  - {problem}"))
        .collect();
    format!("{PLUGINS_DIR}/{}:{list}", check.dir_name)
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

/// Every way the repository and each of its plugins depart from the shape.
pub fn inspect(repo: &Repository) -> Result<Inspection> {
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

    let scanned = match repo.scan() {
        Ok(scanned) => scanned,
        Err(error) => {
            problems.push(format!("{error:#}"));
            Vec::new()
        }
    };
    let excluded: BTreeSet<String> = workspace_list(&workspace, "exclude").into_iter().collect();
    let mut plugins: Vec<PluginCheck> = scanned
        .into_iter()
        .map(|found| match found {
            Ok(plugin) => {
                let mut problems = Vec::new();
                check_plugin(&plugin, &excluded, &mut problems);
                PluginCheck {
                    dir_name: plugin.dir_name.clone(),
                    plugin: Some(plugin),
                    problems,
                }
            }
            Err(broken) => PluginCheck {
                dir_name: broken.dir_name,
                plugin: None,
                problems: vec![broken.reason],
            },
        })
        .collect();
    flag_duplicate_ids(&mut plugins);
    Ok(Inspection {
        meta,
        repo_problems: problems,
        plugins,
    })
}

fn flag_duplicate_ids(plugins: &mut [PluginCheck]) {
    let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
    for check in plugins.iter() {
        if let Some(plugin) = &check.plugin {
            *by_id.entry(plugin.manifest.id.clone()).or_default() += 1;
        }
    }
    for check in plugins.iter_mut() {
        let Some(plugin) = &check.plugin else {
            continue;
        };
        if by_id[&plugin.manifest.id] > 1 {
            check
                .problems
                .push(format!("id '{}' is declared twice", plugin.manifest.id));
        }
    }
}

fn fail<T>(problems: &[String]) -> Result<T> {
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
    if !meta.next.trim().is_empty() && minisign::PublicKey::from_base64(meta.next.trim()).is_err() {
        problems
            .push("[workspace.metadata.hestia] `next` is not a minisign public key".to_string());
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
    if let Err(errors) = manifest.validate() {
        problems.extend(
            errors
                .iter()
                .map(|error| format!("{path}/plugin.toml: {error}")),
        );
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

    fn message(repo: &Repository) -> String {
        let inspection = inspect(repo).unwrap();
        let plugin_problems = inspection
            .plugins
            .iter()
            .flat_map(|check| check.problems.clone());
        let all: Vec<String> = inspection
            .repo_problems
            .iter()
            .cloned()
            .chain(plugin_problems)
            .collect();
        assert!(!all.is_empty(), "inspection must find a problem");
        all.join("\n")
    }

    #[test]
    fn a_valid_repository_passes() {
        let (_root, repo) = repo_with(META, &["plugins/themes"]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "themes", "themes", false);

        let inspection = inspect(&repo).unwrap();

        assert!(inspection.repo_problems.is_empty());
        assert!(inspection.invalid().is_empty());
        let checked = inspection.checked();
        assert_eq!(checked.meta.label, "SMP");
        assert_eq!(checked.plugins.len(), 2);
        assert!(checked.skipped.is_empty());
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

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn one_invalid_plugin_does_not_hide_the_others() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "bb", "Bad Id", true);
        add_plugin(&repo, "cc", "cc", true);

        let inspection = inspect(&repo).unwrap();

        assert!(inspection.repo_problems.is_empty());
        let verdicts: Vec<(&str, bool)> = inspection
            .plugins
            .iter()
            .map(|check| (check.dir_name.as_str(), check.is_valid()))
            .collect();
        assert_eq!(verdicts, [("aa", true), ("bb", false), ("cc", true)]);
        let checked = inspection.checked();
        let valid: Vec<&str> = checked
            .plugins
            .iter()
            .map(|plugin| plugin.dir_name.as_str())
            .collect();
        assert_eq!(valid, ["aa", "cc"]);
        assert_eq!(checked.skipped, ["bb"]);
    }

    #[test]
    fn every_manifest_mistake_of_a_plugin_is_listed() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "Bad Id", true);
        std::fs::write(
            repo.plugin_path("aa").join("plugin.toml"),
            "id = \"Bad Id\"\nname = \"x\"\nversion = \"one\"\n",
        )
        .unwrap();

        let inspection = inspect(&repo).unwrap();

        let problems = &inspection.plugins[0].problems;
        assert!(problems.iter().any(|problem| problem.contains("plugin id")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("not a semantic version")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("folder name must equal the id")));
    }

    #[test]
    fn an_unreadable_manifest_is_that_plugins_problem_only() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "bb", "bb", true);
        std::fs::write(repo.plugin_path("bb").join("plugin.toml"), "id = [").unwrap();

        let inspection = inspect(&repo).unwrap();

        assert!(inspection.plugins[0].is_valid());
        assert!(!inspection.plugins[1].is_valid());
        assert!(inspection.plugins[1].plugin.is_none());
    }

    #[test]
    fn both_plugins_sharing_an_id_are_flagged() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "bb", "aa", true);

        let inspection = inspect(&repo).unwrap();

        assert!(inspection
            .plugins
            .iter()
            .all(|check| check.problems.iter().any(|p| p.contains("declared twice"))));
    }

    #[test]
    fn selecting_by_name_keeps_only_those_and_refuses_unknown_names() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "bb", "bb", true);

        let selected = inspect(&repo).unwrap().select(&ids(&["bb"])).unwrap();
        assert_eq!(selected.plugins.len(), 1);
        assert_eq!(selected.plugins[0].dir_name, "bb");

        let all = inspect(&repo).unwrap().select(&[]).unwrap();
        assert_eq!(all.plugins.len(), 2);

        let error = inspect(&repo)
            .unwrap()
            .select(&ids(&["aa", "zz"]))
            .err()
            .expect("an unknown name is an error");
        assert!(error.to_string().contains("no plugin 'zz'"));
    }

    #[test]
    fn failing_the_invalid_plugins_names_each_of_them() {
        let (_root, repo) = repo_with(META, &[]);
        add_plugin(&repo, "aa", "Bad", true);
        add_plugin(&repo, "bb", "Worse", true);
        let inspection = inspect(&repo).unwrap();

        let error = fail_invalid(&inspection.invalid(), "packed").unwrap_err();

        assert_eq!(
            error.to_string(),
            "not packed: aa, bb (fix them and run again)"
        );
        assert!(fail_invalid(&[], "packed").is_ok());
    }
}
