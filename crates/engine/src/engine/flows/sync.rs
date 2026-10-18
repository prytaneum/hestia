//! The sync flows: the reconcile a launch runs, the one its exit replays,
//! turning a unit on from a named instance, and the per-instance overrides.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Result};
use proto::sync::{
    InstanceSyncStatus, SyncConfig, SyncOption, SyncOverrides, SyncSource, SyncUnit,
};
use proto::warning::WarningInfo;

use crate::engine::Engine;
use crate::instances::InstanceRecord;
use crate::sync::Pass;

impl Engine {
    pub fn sync_config(&self) -> SyncConfig {
        self.sync.config()
    }

    pub(crate) fn instance_pass(&self, record: &InstanceRecord, data_dir: &Path) -> Pass {
        pass(record, data_dir)
    }

    /// Reconcile for a launching session and record what it reconciled, so the
    /// pass that runs when the session exits reconciles the same way.
    pub(crate) fn begin_instance_sync(&self, session: &str, pass: Pass) -> Vec<WarningInfo> {
        let warnings = self.sync.apply(&pass);
        self.sync.remember(session, pass);
        warnings
    }

    /// Skipped while the instance's other sessions run: they hold the same
    /// files open, and copying over a live game's `options.txt` would lose
    /// whatever it writes at its own exit.
    pub fn finish_instance_sync(&self, session: &str) {
        let Some(pass) = self.sync.recall(session) else {
            return;
        };
        let running = self.running_sessions(&pass.id);
        if running > 0 {
            tracing::debug!(
                instance = %pass.name,
                running,
                "leaving the sync pass to the last session out"
            );
            return;
        }
        for warning in self.sync.apply(&pass) {
            tracing::debug!(instance = %pass.name, warning = %warning, "sync at exit");
        }
    }

    /// Every instance that is not running takes the shared copy now, so a
    /// change reaches an instance that is never launched.
    pub fn reconcile_idle(&self) {
        for record in self.instances.list() {
            if self.running_sessions(&record.id) > 0 {
                continue;
            }
            let data_dir = self.instances.data_dir(&record);
            let pass = self.instance_pass(&record, &data_dir);
            for warning in self.sync.apply(&pass) {
                tracing::debug!(instance = %record.name, warning = %warning, "idle sync");
            }
        }
    }

    /// Which instances could start a unit's shared copy.
    pub fn sync_sources(&self, unit: SyncUnit) -> Vec<SyncSource> {
        self.instances
            .list()
            .into_iter()
            .map(|record| {
                let (present, modified_unix) = self
                    .sync
                    .source_state(unit, &self.instances.data_dir(&record));
                SyncSource {
                    id: record.id,
                    name: record.name,
                    present,
                    modified_unix,
                }
            })
            .collect()
    }

    /// Seed a unit from one instance and turn it on. Every other instance's
    /// agreement is recorded as its current content, so its first pass settles
    /// the shared copy's way rather than racing it.
    pub fn enable_sync_unit(&self, unit: SyncUnit, source: &str) -> Result<SyncConfig> {
        let records = self.instances.list();
        let source = match source.trim() {
            "" => self.only_candidate(unit, &records)?,
            reference => Some(self.instances.require(reference)?),
        };

        let seeded_from = match &source {
            Some(record) => {
                self.sync.seed(
                    unit,
                    &record.profile.game_version,
                    &self.instances.data_dir(record),
                )?;
                self.seed_packs(unit, record)?;
                record.name.clone()
            }
            None => String::new(),
        };
        for record in &records {
            self.sync
                .back_up(unit, &record.id, &self.instances.data_dir(record))?;
            self.sync.defer(
                unit,
                &record.id,
                &record.profile.game_version,
                &self.instances.data_dir(record),
            )?;
        }
        tracing::info!(%unit, source = %seeded_from, "sync unit enabled");
        let config = self.sync.enable(unit, &seeded_from)?;
        self.reconcile_idle();
        Ok(config)
    }

    fn only_candidate(
        &self,
        unit: SyncUnit,
        records: &[InstanceRecord],
    ) -> Result<Option<InstanceRecord>> {
        let mut candidates: Vec<&InstanceRecord> = records
            .iter()
            .filter(|record| {
                self.sync
                    .source_state(unit, &self.instances.data_dir(record))
                    .0
            })
            .collect();
        if candidates.len() > 1 {
            bail!(proto::error::ErrorInfo::SyncSourceRequired {
                unit,
                candidates: candidates.iter().map(|r| r.name.clone()).collect(),
            });
        }
        Ok(candidates.pop().cloned())
    }

    pub fn disable_sync_unit(&self, unit: SyncUnit) -> Result<SyncConfig> {
        tracing::info!(%unit, "sync unit disabled");
        self.sync.disable(unit)
    }

    pub fn set_sync_unsynced(&self, keys: BTreeSet<String>) -> Result<SyncConfig> {
        let config = self.sync.set_unsynced(keys)?;
        self.reconcile_idle();
        Ok(config)
    }

    pub fn sync_options(&self) -> Vec<SyncOption> {
        self.sync.options()
    }

    pub fn set_sync_option(&self, key: &str, value: &str) -> Result<Vec<SyncOption>> {
        if !self.sync.enabled(SyncUnit::Options) {
            bail!(proto::error::ErrorInfo::SyncUnitDisabled {
                unit: SyncUnit::Options
            });
        }
        self.sync.set_option(key, value)?;
        self.reconcile_idle();
        Ok(self.sync.options())
    }

    pub fn sync_status(&self) -> Vec<InstanceSyncStatus> {
        self.instances
            .list()
            .into_iter()
            .map(|record| self.instance_sync_status(&record))
            .collect()
    }

    pub fn set_instance_sync_unit(
        &self,
        reference: &str,
        unit: SyncUnit,
        shared: Option<bool>,
    ) -> Result<InstanceSyncStatus> {
        self.update_overrides(reference, |overrides| match shared {
            Some(true) | None => {
                overrides.excluded.remove(&unit);
            }
            Some(false) => {
                overrides.excluded.insert(unit);
            }
        })
    }

    pub fn set_instance_sync_keys(
        &self,
        reference: &str,
        unsynced: BTreeSet<String>,
    ) -> Result<InstanceSyncStatus> {
        self.update_overrides(reference, |overrides| overrides.unsynced = unsynced.clone())
    }

    fn update_overrides(
        &self,
        reference: &str,
        mutate: impl FnOnce(&mut SyncOverrides),
    ) -> Result<InstanceSyncStatus> {
        let record = self.instances.require(reference)?;
        let mut overrides = record.sharing.clone();
        mutate(&mut overrides);
        let record = self.instances.set_overrides(&record.id, overrides)?;
        tracing::info!(instance = %record.name, "instance sync overrides changed");
        if self.running_sessions(&record.id) == 0 {
            let data_dir = self.instances.data_dir(&record);
            let pass = self.instance_pass(&record, &data_dir);
            self.sync.apply(&pass);
        }
        Ok(self.instance_sync_status(&record))
    }

    fn instance_sync_status(&self, record: &InstanceRecord) -> InstanceSyncStatus {
        let data_dir = self.instances.data_dir(record);
        let pass = pass(record, &data_dir);
        InstanceSyncStatus {
            id: record.id.clone(),
            name: record.name.clone(),
            units: self.sync.status(&pass),
            unsynced: record.sharing.unsynced.clone(),
        }
    }
}

fn pass(record: &InstanceRecord, data_dir: &Path) -> Pass {
    Pass {
        id: record.id.clone(),
        name: record.name.clone(),
        game_version: record.profile.game_version.clone(),
        data_dir: data_dir.to_path_buf(),
        overrides: record.sharing.clone(),
    }
}
