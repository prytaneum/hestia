//! The catalogue as it is persisted: `<shared>/sync.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use proto::sync::{SyncConfig, SyncUnit, SyncUnitConfig};
use serde::{Deserialize, Serialize};

use super::catalogue;
use common::schema::{self, Document};

const FILE: &str = "sync.json";

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Catalogue {
    units: BTreeMap<SyncUnit, UnitRecord>,
    unsynced: BTreeSet<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
struct UnitRecord {
    enabled: bool,
    seeded_from: String,
}

impl Document for Catalogue {
    const NAME: &'static str = FILE;
}

impl Catalogue {
    pub fn load(dir: &Path) -> Catalogue {
        schema::load(&dir.join(FILE)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        Ok(schema::save(&dir.join(FILE), self)?)
    }

    pub fn enabled(&self, unit: SyncUnit) -> bool {
        self.units.get(&unit).is_some_and(|record| record.enabled)
    }

    pub fn unsynced(&self) -> &BTreeSet<String> {
        &self.unsynced
    }

    pub fn set_unsynced(&mut self, keys: BTreeSet<String>) {
        self.unsynced = keys;
    }

    pub fn enable(&mut self, unit: SyncUnit, seeded_from: &str) {
        self.units.insert(
            unit,
            UnitRecord {
                enabled: true,
                seeded_from: seeded_from.to_string(),
            },
        );
    }

    pub fn disable(&mut self, unit: SyncUnit) {
        self.units.entry(unit).or_default().enabled = false;
    }

    pub fn to_config(&self, shared_dir: &Path) -> SyncConfig {
        SyncConfig {
            shared_dir: shared_dir.to_path_buf(),
            units: catalogue::ALL
                .iter()
                .map(|unit| {
                    let record = self.units.get(unit).cloned().unwrap_or_default();
                    SyncUnitConfig {
                        unit: *unit,
                        enabled: record.enabled,
                        seeded_from: record.seeded_from,
                    }
                })
                .collect(),
            unsynced: self.unsynced.clone(),
        }
    }
}
