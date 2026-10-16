//! The session's cross-model operations: the decisions that read the installed
//! state (the target), the browse selection (the catalogue), and the staged
//! batch (the cart) together. The two sub-models stay pure; the coupling lives
//! here.

use client::proto::content::{
    ContentAddItem, ContentAddSpec, ContentKind, ContentProject, InstalledContent,
};
use client::proto::error::{EntryKind, EntryTarget};

use super::cart::StagedRemoval;
use super::driver::{InstallJob, Removal, Request};
use super::{ContentSession, Mode, Overlay, Target};
use crate::commands::content::format::{
    version_picker, world_label, world_name as install_world_name,
};
use crate::ui::components::SelectList;

impl ContentSession {
    /// The target's index entry for a project, if it is installed. Local imports
    /// carry no project id and cannot be matched to a hit.
    pub(super) fn installed_entry(&self, project: &ContentProject) -> Option<&InstalledContent> {
        self.target.as_ref()?.installed.iter().find(|i| {
            !i.project_id.is_empty() && i.project_id == project.id && i.source == project.source
        })
    }

    /// The index entry a staged removal clears.
    pub(super) fn staged_record(&self, staged: &StagedRemoval) -> Option<&InstalledContent> {
        self.target
            .as_ref()?
            .installed
            .iter()
            .find(|i| i.project_id == staged.project_id)
    }

    /// The browse-time "already installed" marker: the installed version, or —
    /// for an instance's datapacks — the worlds it loads in.
    pub(super) fn installed_label(&self, project: &ContentProject) -> Option<String> {
        let entry = self.installed_entry(project)?;
        if self.instance_datapacks() {
            return Some(format!("in {}", world_label(entry)));
        }
        Some(entry.version_number.clone())
    }

    /// The review-time marker: what this install overwrites.
    pub(super) fn review_marker(&self, project: &ContentProject) -> Option<String> {
        let entry = self.installed_entry(project)?;
        Some(format!("replaces {}", entry.version_number))
    }

    pub(super) fn instance_datapacks(&self) -> bool {
        self.base.kind == ContentKind::DataPack
            && matches!(
                self.target,
                Some(Target {
                    entry: EntryTarget {
                        kind: EntryKind::Instance,
                        ..
                    },
                    ..
                })
            )
    }

    /// Space on a row. A plain row toggles in and out of the batch; an installed
    /// row cycles keep → reinstall → remove → keep.
    pub(super) fn toggle_chosen(&mut self) {
        let Some(hit) = self.catalogue.highlighted().cloned() else {
            return;
        };
        let installed = self.installed_entry(&hit).is_some();
        let chosen_pos = self.cart.chosen_pos(&hit.id);
        if installed {
            if let Some(pos) = chosen_pos {
                self.cart.chosen.remove(pos);
                self.stage_removal(&hit);
            } else if let Some(pos) = self.cart.removal_pos(&hit.id) {
                self.cart.removals.remove(pos);
            } else {
                self.cart.choose_latest(hit);
            }
        } else if let Some(pos) = chosen_pos {
            self.cart.chosen.remove(pos);
        } else {
            self.cart.choose_latest(hit);
        }
    }

    /// Stage a removal; a datapack loading in several worlds first asks which
    /// of them it should stop loading in.
    fn stage_removal(&mut self, hit: &ContentProject) {
        self.cart.stage_removal(hit.id.clone());
        let Some(worlds) = self
            .installed_entry(hit)
            .filter(|_| self.instance_datapacks())
            .map(|entry| self.entry_worlds(entry))
        else {
            return;
        };
        if worlds.len() > 1 {
            self.overlay = Some(Overlay::RemoveWorlds {
                project: hit.id.clone(),
                list: SelectList::new(worlds.clone()).with_checkboxes(),
                names: worlds,
            });
        }
    }

    /// The save worlds an installed datapack loads in, by folder name.
    fn entry_worlds(&self, entry: &InstalledContent) -> Vec<String> {
        if !entry.worlds.is_empty() {
            return entry
                .worlds
                .iter()
                .filter_map(|w| install_world_name(w))
                .map(str::to_string)
                .collect();
        }
        self.target
            .as_ref()
            .map(|t| t.worlds.clone())
            .unwrap_or_default()
    }

    pub(super) fn open_versions(&mut self, project: ContentProject) {
        let picker = self
            .catalogue
            .versions_for(&project.id)
            .map(|v| version_picker(v));
        if picker.is_none() {
            self.catalogue.request_versions(&self.base, &project);
        }
        self.overlay = Some(Overlay::Versions {
            project: Box::new(project),
            picker,
        });
    }

    pub(super) fn install(&mut self) {
        let Some(entry) = self.target.as_ref().map(|t| t.entry.clone()) else {
            return;
        };
        let items = self
            .cart
            .chosen
            .iter()
            .map(|c| ContentAddItem {
                project: c.project.id.clone(),
                version: c.version_id.clone(),
                ..ContentAddItem::default()
            })
            .collect();
        let removals = self
            .cart
            .removals
            .iter()
            .map(|staged| Removal {
                key: staged.project_id.clone(),
                worlds: staged.worlds.clone(),
                record: self.staged_record(staged).cloned(),
            })
            .collect();
        let spec = ContentAddSpec {
            kind: self.base.kind,
            source: self.base.source.clone(),
            items,
            worlds: self.cart.worlds.clone(),
        };
        let _ = self.requests.send(Request::Install(InstallJob {
            entry,
            spec,
            removals,
        }));
        self.mode = Mode::Installing { progress: None };
    }
}
