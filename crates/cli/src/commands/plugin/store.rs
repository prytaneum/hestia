//! `hestia plugin repo …` — the plugin repositories (stores) the daemon trusts.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::catalogue::{RepositoryAdded, RepositoryInfo};

use crate::ui::{self, Spinner, View};

#[derive(Subcommand)]
pub enum StoreCmd {
    /// Trust a repository by its signing key and list what it offers
    Add {
        /// `owner/repo`, `github.com/owner/repo` or an https URL of its hestia-index.json
        url: String,
        #[arg(short = 'y', long, help = "Trust the key shown without asking")]
        yes: bool,
    },
    /// The configured repositories
    #[command(alias = "ls")]
    List,
    /// Stop using a repository; plugins installed from it keep running, without updates
    #[command(alias = "rm")]
    Remove {
        /// Label, name or index URL
        repository: String,
        #[arg(short = 'y', long, help = "Remove without the confirmation prompt")]
        yes: bool,
    },
    /// Fetch the index again
    Refresh {
        /// Label, name or index URL; all of them by default
        repository: Option<String>,
    },
}

pub async fn run(cmd: StoreCmd, plugins: client::Plugin<'_>) -> Result<()> {
    match cmd {
        StoreCmd::Add { url, yes } => add(&plugins, &url, yes).await.map(drop),
        StoreCmd::List => list(&plugins).await,
        StoreCmd::Remove { repository, yes } => remove(&plugins, &repository, yes).await,
        StoreCmd::Refresh { repository } => refresh(&plugins, repository.as_deref()).await,
    }
}

/// Fetches the index, shows what is being trusted and asks before saving.
/// Returns the repository, saved or already configured.
pub async fn add(plugins: &client::Plugin<'_>, url: &str, yes: bool) -> Result<RepositoryInfo> {
    let found = {
        let _spinner = Spinner::start("fetching the index");
        plugins.repo_add(url, false).await?
    };
    show_found(&found)?;
    if found.configured {
        ui::show(View::note("already added"))?;
        return Ok(found.repository);
    }
    if !yes {
        let trust = ui::confirm(
            &format!(
                "trust key {}? plugins it signs can be installed and updated",
                found.repository.key_id
            ),
            "trust",
            "cancel",
        )
        .context("pass --yes to trust the key shown")?;
        if !trust {
            bail!("add cancelled");
        }
    }
    let saved = plugins.repo_add(url, true).await?;
    ui::show(View::line(format!(
        "added '{}' [{}]",
        saved.repository.name, saved.repository.label
    )))?;
    Ok(saved.repository)
}

fn show_found(found: &RepositoryAdded) -> Result<()> {
    let repository = &found.repository;
    ui::show(View::detail([
        ("Name", repository.name.clone()),
        ("Label", repository.label.clone()),
        ("Website", repository.website.clone()),
        ("Key id", repository.key_id.clone()),
        ("Plugins", repository.plugins.to_string()),
        ("Index", repository.url.clone()),
    ]))
}

async fn list(plugins: &client::Plugin<'_>) -> Result<()> {
    let repositories = plugins.repo_list().await?;
    if repositories.is_empty() {
        return ui::show(View::note(
            "no plugin repositories; add one with `hestia plugin repo add`",
        ));
    }
    ui::show(View::table(
        "Plugin repositories",
        ["LABEL", "NAME", "KEY ID", "PLUGINS", "WEBSITE"],
        repositories
            .into_iter()
            .map(|r| vec![r.label, r.name, r.key_id, r.plugins.to_string(), r.website])
            .collect(),
    ))
}

async fn remove(plugins: &client::Plugin<'_>, reference: &str, yes: bool) -> Result<()> {
    let repository = resolve(&plugins.repo_list().await?, reference)?;
    if !yes {
        let remove = ui::confirm(
            &format!(
                "plugins installed from '{}' keep running but get no updates",
                repository.name
            ),
            "remove",
            "cancel",
        )
        .context("pass --yes to remove without confirming")?;
        if !remove {
            bail!("remove cancelled");
        }
    }
    plugins.repo_remove(&repository.url).await?;
    ui::show(View::line(format!("removed '{}'", repository.name)))
}

async fn refresh(plugins: &client::Plugin<'_>, reference: Option<&str>) -> Result<()> {
    let url = match reference {
        Some(reference) => Some(resolve(&plugins.repo_list().await?, reference)?.url),
        None => None,
    };
    let refreshed = {
        let _spinner = Spinner::start("refreshing");
        plugins.repo_refresh(url.as_deref()).await?
    };
    for repository in &refreshed.repositories {
        ui::show(View::line(format!(
            "{} [{}]: {} plugin(s), version {}",
            repository.name, repository.label, repository.plugins, repository.version
        )))?;
    }
    ui::show_warnings(&refreshed.warnings)
}

/// The one repository a label, name or index URL names.
pub fn resolve(repositories: &[RepositoryInfo], reference: &str) -> Result<RepositoryInfo> {
    let matches: Vec<&RepositoryInfo> = repositories
        .iter()
        .filter(|r| {
            r.url == reference
                || r.label.eq_ignore_ascii_case(reference)
                || r.name.eq_ignore_ascii_case(reference)
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!("no repository matches '{reference}'; see `hestia plugin repo list`"),
        many => bail!(
            "'{reference}' matches {} repositories; use the index URL: {}",
            many.len(),
            many.iter()
                .map(|r| r.url.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(label: &str, name: &str, url: &str) -> RepositoryInfo {
        RepositoryInfo {
            label: label.to_string(),
            name: name.to_string(),
            url: url.to_string(),
            ..RepositoryInfo::default()
        }
    }

    #[test]
    fn a_reference_names_one_repository_by_label_name_or_url() {
        let all = [
            repository("SMP", "Sample", "https://a.example/i.json"),
            repository("ACME", "Acme tools", "https://b.example/i.json"),
        ];

        for reference in ["smp", "Sample", "https://a.example/i.json"] {
            assert_eq!(resolve(&all, reference).unwrap().label, "SMP");
        }
        assert!(resolve(&all, "nothing").is_err());
    }

    #[test]
    fn an_ambiguous_label_asks_for_the_url() {
        let all = [
            repository("SMP", "One", "https://a.example/i.json"),
            repository("SMP", "Two", "https://b.example/i.json"),
        ];

        let error = resolve(&all, "SMP").unwrap_err().to_string();

        assert!(error.contains("https://a.example/i.json"), "{error}");
    }
}
