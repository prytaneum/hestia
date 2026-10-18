//! `InstalledContent`'s fields have three owners: the project the item is an
//! install of, the release its file came from, and how the entry holds it. No
//! flow owns all three — an update writes only the release, a modpack re-supply
//! writes project and release over an item the entry already holds. Records are
//! assembled here so a flow cannot reset a group it does not own; [`assemble`]
//! is exhaustive, so a new field must be classified before it compiles.

use std::path::Path;

use proto::content::{ContentKind, InstalledContent};

use crate::content::icon;

#[derive(Debug, Clone)]
pub(crate) struct Project {
    pub kind: ContentKind,
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub icon_url: String,
}

impl Project {
    /// `archive` is the item's managed copy: with no project behind it, the
    /// only icon it can have is the one it carries.
    pub(crate) fn untracked(kind: ContentKind, title: String, archive: &Path) -> Self {
        Project {
            kind,
            project_id: String::new(),
            slug: String::new(),
            title,
            icon_url: icon::embedded(archive),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Release {
    pub source: String,
    pub version_id: String,
    pub version_number: String,
    pub filename: String,
    pub sha1: String,
    pub url: String,
}

impl Release {
    pub(crate) fn local(filename: String, sha1: String) -> Self {
        Release {
            source: proto::content::LOCAL_SOURCE.to_string(),
            version_id: String::new(),
            version_number: String::new(),
            filename,
            sha1,
            url: String::new(),
        }
    }
}

/// The mirror reads `enabled` and `disabled_worlds`
/// ([`crate::content::install::apply_files`]), so losing one does not just
/// misreport an item — it puts a disabled one back in the game's load dirs.
#[derive(Debug, Clone)]
pub(crate) struct Holding {
    pub worlds: Vec<String>,
    pub origin: String,
    pub enabled: bool,
    pub disabled_worlds: Vec<String>,
}

impl Holding {
    pub(crate) fn fresh(worlds: &[String]) -> Self {
        Holding {
            worlds: worlds.to_vec(),
            origin: String::new(),
            enabled: true,
            disabled_worlds: Vec::new(),
        }
    }
}

impl From<&InstalledContent> for Project {
    fn from(item: &InstalledContent) -> Self {
        Project {
            kind: item.kind,
            project_id: item.project_id.clone(),
            slug: item.slug.clone(),
            title: item.title.clone(),
            icon_url: item.icon_url.clone(),
        }
    }
}

impl From<&InstalledContent> for Release {
    fn from(item: &InstalledContent) -> Self {
        Release {
            source: item.source.clone(),
            version_id: item.version_id.clone(),
            version_number: item.version_number.clone(),
            filename: item.filename.clone(),
            sha1: item.sha1.clone(),
            url: item.url.clone(),
        }
    }
}

impl From<&InstalledContent> for Holding {
    fn from(item: &InstalledContent) -> Self {
        Holding {
            worlds: item.worlds.clone(),
            origin: item.origin.clone(),
            enabled: item.enabled,
            disabled_worlds: item.disabled_worlds.clone(),
        }
    }
}

/// The one place a record's fields are written; exhaustive on purpose.
pub(crate) fn assemble(project: Project, release: Release, holding: Holding) -> InstalledContent {
    InstalledContent {
        kind: project.kind,
        project_id: project.project_id,
        slug: project.slug,
        title: project.title,
        icon_url: project.icon_url,
        source: release.source,
        version_id: release.version_id,
        version_number: release.version_number,
        filename: release.filename,
        sha1: release.sha1,
        url: release.url,
        installed_unix: common::now_unix(),
        worlds: holding.worlds,
        origin: holding.origin,
        enabled: holding.enabled,
        disabled_worlds: holding.disabled_worlds,
    }
}

/// Move an item onto another release, keeping what it is and how it is held.
pub(crate) fn repin(item: &InstalledContent, release: Release) -> InstalledContent {
    assemble(Project::from(item), release, Holding::from(item))
}

pub(crate) fn resource(
    item: &InstalledContent,
    project: Project,
    release: Release,
) -> InstalledContent {
    assemble(project, release, Holding::from(item))
}

/// Put a resolved record where an existing one stood, under `holding`.
pub(crate) fn rehold(item: &InstalledContent, holding: Holding) -> InstalledContent {
    assemble(Project::from(item), Release::from(item), holding)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held() -> InstalledContent {
        assemble(
            Project {
                kind: ContentKind::DataPack,
                project_id: "sodium".to_string(),
                slug: "sodium".to_string(),
                title: "Sodium".to_string(),
                icon_url: "https://example.invalid/sodium.png".to_string(),
            },
            Release {
                source: "modrinth".to_string(),
                version_id: "v1".to_string(),
                version_number: "1.0.0".to_string(),
                filename: "sodium-1.0.0.jar".to_string(),
                sha1: "aaa".to_string(),
                url: "https://example.invalid/1.jar".to_string(),
            },
            Holding {
                worlds: vec!["saves/hardcore".to_string()],
                origin: "modpack:kitchen-sink".to_string(),
                enabled: false,
                disabled_worlds: vec!["saves/hardcore".to_string()],
            },
        )
    }

    fn moved() -> Release {
        Release {
            source: "modrinth".to_string(),
            version_id: "v2".to_string(),
            version_number: "2.0.0".to_string(),
            filename: "sodium-2.0.0.jar".to_string(),
            sha1: "bbb".to_string(),
            url: "https://example.invalid/2.jar".to_string(),
        }
    }

    #[test]
    fn repinning_moves_the_release_and_nothing_else() {
        let before = held();
        let after = repin(&before, moved());

        assert_eq!(after.version_id, "v2");
        assert_eq!(after.filename, "sodium-2.0.0.jar");
        assert_eq!(after.sha1, "bbb");

        assert_eq!(after.project_id, before.project_id);
        assert_eq!(after.title, before.title);
        assert_eq!(after.icon_url, before.icon_url);
        assert_eq!(after.origin, before.origin);
        assert!(!after.enabled);
        assert_eq!(after.worlds, before.worlds);
        assert_eq!(after.disabled_worlds, before.disabled_worlds);
    }

    #[test]
    fn reholding_keeps_the_new_release_under_the_old_holding() {
        let previous = held();
        let supplied = repin(&previous, moved());
        let resupplied = rehold(
            &supplied,
            Holding {
                origin: String::new(),
                ..Holding::from(&previous)
            },
        );

        assert_eq!(resupplied.version_id, "v2");
        assert!(!resupplied.enabled);
        assert_eq!(resupplied.worlds, previous.worlds);
        assert_eq!(resupplied.disabled_worlds, previous.disabled_worlds);
        assert!(resupplied.origin.is_empty());
    }
}
