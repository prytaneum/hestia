//! The shared pack library, reconciled into one instance through the pool.

use anyhow::{Context, Result};
use proto::content::{ContentAddItem, ContentAddSpec, ContentKind, InstalledContent};
use proto::sync::SyncUnit;
use proto::warning::WarningInfo;

use crate::cancel::{Cancel, Job};
use crate::engine::Engine;
use crate::instances::InstanceRecord;
use crate::sync::packs::{self, Action, Pack};

use super::content::EntryRef;

const UNITS: &[(SyncUnit, ContentKind)] = &[(SyncUnit::ResourcePacks, ContentKind::ResourcePack)];

fn kind_of(unit: SyncUnit) -> Option<ContentKind> {
    UNITS
        .iter()
        .find(|(candidate, _)| *candidate == unit)
        .map(|(_, kind)| *kind)
}

impl Engine {
    pub async fn reconcile_packs(&self, record: &InstanceRecord) -> Vec<WarningInfo> {
        let mut warnings = Vec::new();
        for &(unit, kind) in UNITS {
            if !self.sync.enabled(unit) || !record.sharing.shares(unit) {
                continue;
            }
            if let Err(e) = self.settle_packs(record, kind).await {
                let detail = format!("{e:#}");
                tracing::warn!(instance = %record.name, %unit, error = %detail, "pack sync skipped");
                warnings.push(WarningInfo::SyncUnitSkipped { unit, detail });
            }
        }
        warnings
    }

    pub async fn reconcile_idle_packs(&self) {
        for record in self.instances.list() {
            if self.running_sessions(&record.id) > 0 {
                continue;
            }
            self.reconcile_packs(&record).await;
        }
    }

    /// The named instance's packs become the library, so enabling the unit
    /// starts from someone rather than from whoever reconciles first.
    pub fn seed_packs(&self, unit: SyncUnit, record: &InstanceRecord) -> Result<()> {
        let Some(kind) = kind_of(unit) else {
            return Ok(());
        };
        let held = self.held_packs(record, kind)?;
        let mut library = self.sync.library();
        library.replace(kind, held);
        self.sync.save_library(&library)
    }

    pub fn shared_packs(&self) -> Vec<Pack> {
        self.sync.library().packs
    }

    pub fn set_shared_pack(&self, reference: &str, enabled: bool) -> Result<Vec<Pack>> {
        self.change_library(reference, |packs, at| packs[at].enabled = enabled)
    }

    pub fn remove_shared_pack(&self, reference: &str) -> Result<Vec<Pack>> {
        self.change_library(reference, |packs, at| {
            packs.remove(at);
        })
    }

    fn change_library(
        &self,
        reference: &str,
        change: impl FnOnce(&mut Vec<Pack>, usize),
    ) -> Result<Vec<Pack>> {
        let mut library = self.sync.library();
        let at = library
            .packs
            .iter()
            .position(|pack| pack.answers_to(reference))
            .with_context(|| format!("no shared pack '{reference}'"))?;
        change(&mut library.packs, at);
        self.sync.save_library(&library)?;
        Ok(library.packs)
    }

    async fn settle_packs(&self, record: &InstanceRecord, kind: ContentKind) -> Result<()> {
        let mine = self.held_packs(record, kind)?;
        let mut library = self.sync.library();
        let agreed = self.sync.agreed_packs(&record.id, kind);
        let (shared, actions) = packs::settle(&library.of(kind), &agreed, &mine);

        for action in actions {
            self.act_on_pack(record, kind, action).await?;
        }
        library.replace(kind, shared.clone());
        self.sync.save_library(&library)?;

        let held = self.held_packs(record, kind)?;
        let held_here: Vec<Pack> = shared
            .into_iter()
            .filter(|pack| held.iter().any(|here| here.identity() == pack.identity()))
            .collect();
        self.sync.agree_packs(&record.id, kind, &held_here)
    }

    fn held_packs(&self, record: &InstanceRecord, kind: ContentKind) -> Result<Vec<Pack>> {
        let installed = self.entry_content(EntryRef::Instance(&record.id), kind)?.0;
        Ok(installed
            .iter()
            .filter(is_the_players)
            .map(as_pack)
            .collect())
    }

    async fn act_on_pack(
        &self,
        record: &InstanceRecord,
        kind: ContentKind,
        action: Action,
    ) -> Result<()> {
        let entry = EntryRef::Instance(&record.id);
        match action {
            Action::Install(pack) => {
                let spec = ContentAddSpec {
                    kind,
                    source: pack.source.clone(),
                    items: vec![ContentAddItem {
                        project: pack.project.clone(),
                        source: pack.source.clone(),
                        ..ContentAddItem::default()
                    }],
                    worlds: Vec::new(),
                };
                let cancel = Cancel::new();
                let quiet = |_: &proto::minecraft::ProvisionProgress| {};
                let job = Job::new(&quiet, &cancel);
                let reason = match self.add_entry_content(entry, &spec, &job).await {
                    Ok((_, failed)) => failed.first().map(|failure| failure.error.to_string()),
                    Err(e) => Some(format!("{e:#}")),
                };
                if let Some(reason) = reason {
                    tracing::info!(
                        instance = %record.name,
                        pack = %pack.title,
                        %reason,
                        "pack not installed here"
                    );
                }
                Ok(())
            }
            Action::Remove(pack) => {
                self.remove_entry_content(entry, kind, &[selector(&pack)], &[])?;
                Ok(())
            }
            Action::Enable(pack, enabled) => {
                self.enable_entry_content(entry, kind, &selector(&pack), enabled, &[])?;
                Ok(())
            }
        }
    }
}

/// Content a modpack supplied belongs to the pack, not to the player's library.
fn is_the_players(item: &&InstalledContent) -> bool {
    item.origin.is_empty()
}

fn as_pack(item: &InstalledContent) -> Pack {
    Pack {
        kind: item.kind,
        source: item.source.clone(),
        project: item.project_id.clone(),
        title: item.title.clone(),
        filename: item.filename.clone(),
        enabled: item.enabled,
    }
}

fn selector(pack: &Pack) -> String {
    match pack.project.is_empty() {
        true => pack.filename.clone(),
        false => pack.project.clone(),
    }
}
