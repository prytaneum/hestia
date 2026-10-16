//! The shared pack library: which packs the instances hold, not their files.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use proto::content::ContentKind;
pub use proto::sync::SharedPack as Pack;
use serde::{Deserialize, Serialize};

use crate::schema::{self, Document};

const FILE: &str = "packs.json";

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Library {
    pub packs: Vec<Pack>,
}

impl Document for Library {
    const NAME: &'static str = FILE;
}

impl Library {
    pub fn load(dir: &Path) -> Library {
        schema::load(&dir.join(FILE)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        schema::save(&dir.join(FILE), self)
    }

    pub fn of(&self, kind: ContentKind) -> Vec<Pack> {
        self.packs
            .iter()
            .filter(|pack| pack.kind == kind)
            .cloned()
            .collect()
    }

    pub fn replace(&mut self, kind: ContentKind, packs: Vec<Pack>) {
        self.packs.retain(|pack| pack.kind != kind);
        self.packs.extend(packs);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Install(Pack),
    Remove(Pack),
    Enable(Pack, bool),
}

/// What the instance must do, and what the library becomes. A pack only one
/// side has settles the way the agreement says it moved: gained here means the
/// others gain it, gone here means the others lose it.
pub fn settle(library: &[Pack], agreed: &[Pack], installed: &[Pack]) -> (Vec<Pack>, Vec<Action>) {
    let identities: BTreeSet<String> = library
        .iter()
        .chain(installed.iter())
        .map(Pack::identity)
        .collect();
    let mut shared = Vec::new();
    let mut actions = Vec::new();

    for identity in identities {
        let shared_pack = find(library, &identity);
        let local = find(installed, &identity);
        let known = find(agreed, &identity).is_some();
        match (shared_pack, local) {
            (Some(pack), Some(here)) => {
                let enabled = match known {
                    true if here.enabled != enabled_when_agreed(agreed, &identity) => here.enabled,
                    _ => pack.enabled,
                };
                if here.enabled != enabled {
                    actions.push(Action::Enable(here.clone(), enabled));
                }
                shared.push(Pack {
                    enabled,
                    ..pack.clone()
                });
            }
            (Some(pack), None) if !known => {
                actions.push(Action::Install(pack.clone()));
                shared.push(pack.clone());
            }
            (Some(_), None) => {}
            (None, Some(here)) => match known {
                true => actions.push(Action::Remove(here.clone())),
                false => shared.push(here.clone()),
            },
            (None, None) => {}
        }
    }
    (shared, actions)
}

fn find<'a>(packs: &'a [Pack], identity: &str) -> Option<&'a Pack> {
    packs.iter().find(|pack| pack.identity() == identity)
}

fn enabled_when_agreed(agreed: &[Pack], identity: &str) -> bool {
    find(agreed, identity).is_some_and(|pack| pack.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(project: &str, enabled: bool) -> Pack {
        Pack {
            kind: ContentKind::ResourcePack,
            source: "modrinth".to_string(),
            project: project.to_string(),
            title: project.to_string(),
            filename: format!("{project}.zip"),
            enabled,
        }
    }

    #[test]
    fn a_pack_the_library_has_and_the_instance_does_not_is_installed() {
        let (shared, actions) = settle(&[pack("cozy", true)], &[], &[]);
        assert_eq!(actions, vec![Action::Install(pack("cozy", true))]);
        assert_eq!(shared, vec![pack("cozy", true)]);
    }

    #[test]
    fn a_pack_the_instance_installed_itself_joins_the_library() {
        let (shared, actions) = settle(&[], &[], &[pack("cozy", true)]);
        assert!(actions.is_empty());
        assert_eq!(shared, vec![pack("cozy", true)]);
    }

    #[test]
    fn removing_a_pack_here_removes_it_from_the_library() {
        let agreed = vec![pack("cozy", true)];
        let (shared, actions) = settle(&[pack("cozy", true)], &agreed, &[]);
        assert!(actions.is_empty());
        assert!(shared.is_empty());
    }

    #[test]
    fn a_pack_removed_elsewhere_is_removed_here() {
        let agreed = vec![pack("cozy", true)];
        let (shared, actions) = settle(&[], &agreed, &[pack("cozy", true)]);
        assert_eq!(actions, vec![Action::Remove(pack("cozy", true))]);
        assert!(shared.is_empty());
    }

    #[test]
    fn disabling_a_pack_here_disables_it_everywhere() {
        let agreed = vec![pack("cozy", true)];
        let (shared, actions) = settle(&[pack("cozy", true)], &agreed, &[pack("cozy", false)]);
        assert!(actions.is_empty());
        assert_eq!(shared, vec![pack("cozy", false)]);
    }

    #[test]
    fn a_pack_disabled_elsewhere_is_disabled_here() {
        let agreed = vec![pack("cozy", true)];
        let (shared, actions) = settle(&[pack("cozy", false)], &agreed, &[pack("cozy", true)]);
        assert_eq!(actions, vec![Action::Enable(pack("cozy", true), false)]);
        assert_eq!(shared, vec![pack("cozy", false)]);
    }

    #[test]
    fn a_file_install_is_known_by_its_filename() {
        let local = Pack {
            source: "file".to_string(),
            project: String::new(),
            ..pack("cozy", true)
        };
        assert!(local.identity().ends_with("file:cozy.zip"));
    }
}
