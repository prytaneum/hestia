//! Shared settings across instances: a catalogue of units, the shared copy of
//! each under `<data_home>/shared/`, and the reconcile a launch runs.

mod catalogue;
mod document;
mod history;
mod hotbars;
mod options;
pub mod packs;
mod reconcile;
mod servers;
mod state;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use proto::sync::{SyncConfig, SyncOption, SyncOverrides, SyncUnit, UnitState, UnitStatus};
use proto::warning::WarningInfo;

use state::Catalogue;

const BASELINES: &str = ".baselines";

const BACKUPS: &str = ".backups";

#[derive(Clone)]
pub struct Pass {
    pub id: String,
    pub name: String,
    pub game_version: String,
    pub data_dir: PathBuf,
    pub overrides: SyncOverrides,
}

pub struct Sync {
    dir: PathBuf,
    sessions: Mutex<HashMap<String, Pass>>,
}

impl Sync {
    pub fn new(dir: PathBuf) -> Self {
        Sync {
            dir,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn config(&self) -> SyncConfig {
        let shared = self.dir();
        Catalogue::load(shared).to_config(shared)
    }

    pub fn enabled(&self, unit: SyncUnit) -> bool {
        Catalogue::load(self.dir()).enabled(unit)
    }

    pub fn enable(&self, unit: SyncUnit, seeded_from: &str) -> Result<SyncConfig> {
        let shared = self.dir();
        let mut catalogue = Catalogue::load(shared);
        catalogue.enable(unit, seeded_from);
        if unit == SyncUnit::Options {
            let mut pinned = catalogue.unsynced().clone();
            pinned.extend(options::LOCAL_BY_DEFAULT.iter().map(|key| key.to_string()));
            catalogue.set_unsynced(pinned);
        }
        catalogue.save(shared)?;
        Ok(catalogue.to_config(shared))
    }

    pub fn disable(&self, unit: SyncUnit) -> Result<SyncConfig> {
        let shared = self.dir();
        let mut catalogue = Catalogue::load(shared);
        catalogue.disable(unit);
        catalogue.save(shared)?;
        Ok(catalogue.to_config(shared))
    }

    pub fn set_unsynced(&self, keys: BTreeSet<String>) -> Result<SyncConfig> {
        let shared = self.dir();
        let mut catalogue = Catalogue::load(shared);
        catalogue.set_unsynced(keys);
        catalogue.save(shared)?;
        Ok(catalogue.to_config(shared))
    }

    pub fn seed(&self, unit: SyncUnit, game_version: &str, from: &Path) -> Result<()> {
        let Some(file) = catalogue::file(unit) else {
            return Ok(());
        };
        let store = in_era(self.dir(), catalogue::era(unit, game_version));
        std::fs::create_dir_all(&store)
            .with_context(|| format!("cannot create {}", store.display()))?;
        let source = from.join(file);
        if source.is_file() {
            reconcile::copy_file(&source, &store.join(file))?;
        }
        Ok(())
    }

    pub fn back_up(&self, unit: SyncUnit, id: &str, data_dir: &Path) -> Result<()> {
        let Some(file) = catalogue::file(unit) else {
            return Ok(());
        };
        let source = data_dir.join(file);
        if !source.is_file() {
            return Ok(());
        }
        let kept = self.dir().join(BACKUPS).join(id);
        let staging = kept.join(format!("{file}.part"));
        reconcile::copy_file(&source, &staging)?;
        std::fs::rename(&staging, kept.join(file))
            .with_context(|| format!("cannot keep a copy of {}", source.display()))?;
        for stamped in std::fs::read_dir(&kept).into_iter().flatten().flatten() {
            if stamped.path().is_dir() {
                let _ = std::fs::remove_file(stamped.path().join(file));
                let _ = std::fs::remove_dir(stamped.path());
            }
        }
        Ok(())
    }

    pub fn defer(
        &self,
        unit: SyncUnit,
        id: &str,
        game_version: &str,
        data_dir: &Path,
    ) -> Result<()> {
        let Some(file) = catalogue::file(unit) else {
            return Ok(());
        };
        let agreed = in_era(
            &self.dir().join(BASELINES).join(id),
            catalogue::era(unit, game_version),
        );
        reconcile::defer_to_store(&agreed.join(file), &data_dir.join(file))
    }

    pub fn agreed_packs(&self, id: &str, kind: proto::content::ContentKind) -> Vec<packs::Pack> {
        packs::Library::load(&self.dir().join(BASELINES).join(id)).of(kind)
    }

    pub fn agree_packs(
        &self,
        id: &str,
        kind: proto::content::ContentKind,
        agreed: &[packs::Pack],
    ) -> Result<()> {
        let dir = self.dir().join(BASELINES).join(id);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create {}", dir.display()))?;
        let mut library = packs::Library::load(&dir);
        library.replace(kind, agreed.to_vec());
        library.save(&dir)
    }

    pub fn library(&self) -> packs::Library {
        packs::Library::load(self.dir())
    }

    pub fn save_library(&self, library: &packs::Library) -> Result<()> {
        let shared = self.dir();
        std::fs::create_dir_all(shared)
            .with_context(|| format!("cannot create {}", shared.display()))?;
        library.save(shared)
    }

    pub fn source_state(&self, unit: SyncUnit, data_dir: &Path) -> (bool, Option<i64>) {
        let Some(file) = catalogue::file(unit) else {
            return (false, None);
        };
        let path = data_dir.join(file);
        let modified = std::fs::metadata(&path)
            .ok()
            .and_then(|meta| common::modified_unix(&meta));
        (path.is_file(), modified)
    }

    /// Best-effort per unit: refusing to launch over one unreadable file would
    /// be worse than launching unshared, so a skip is returned rather than
    /// raised.
    pub fn apply(&self, pass: &Pass) -> Vec<WarningInfo> {
        let shared = self.dir();
        let catalogue = Catalogue::load(shared);
        let mut warnings = Vec::new();
        for &unit in catalogue::ALL {
            if !catalogue.enabled(unit) || !pass.overrides.shares(unit) {
                continue;
            }
            if catalogue::file(unit).is_none() {
                continue;
            }
            if !catalogue::supports(unit, &pass.game_version) {
                tracing::info!(instance = %pass.name, version = %pass.game_version, %unit, "sync skipped a unit this version cannot share");
                warnings.push(WarningInfo::SyncUnitUnsupported {
                    instance: pass.name.clone(),
                    unit,
                    requires: catalogue::requirement(unit),
                });
                continue;
            }
            if let Err(e) = self.settle(unit, pass, shared, &catalogue) {
                let detail = format!("{e:#}");
                tracing::warn!(%unit, error = %detail, "sync skipped a unit");
                warnings.push(WarningInfo::SyncUnitSkipped { unit, detail });
            }
        }
        warnings
    }

    fn settle(
        &self,
        unit: SyncUnit,
        pass: &Pass,
        shared: &Path,
        catalogue: &Catalogue,
    ) -> Result<()> {
        let era = catalogue::era(unit, &pass.game_version);
        let store = in_era(shared, era);
        let baselines = in_era(&shared.join(BASELINES).join(&pass.id), era);
        std::fs::create_dir_all(&baselines)
            .with_context(|| format!("cannot create {}", baselines.display()))?;
        let Some(file) = catalogue::file(unit) else {
            return Ok(());
        };
        let agreed = baselines.join(file);
        let interrupted = baselines.join(format!("{file}.pending"));
        if interrupted.exists() {
            reconcile::defer_to_store(&agreed, &pass.data_dir.join(file))?;
        }
        std::fs::write(&interrupted, [])
            .with_context(|| format!("cannot write {}", interrupted.display()))?;
        let settled = match unit {
            SyncUnit::Options => options::merge(
                &baselines.join(file),
                &store.join(file),
                &pass.data_dir.join(file),
                &excluded_keys(catalogue, pass),
                &pass.game_version,
            ),
            SyncUnit::Servers => servers::merge(&baselines, &store, &pass.data_dir),
            SyncUnit::Commands => history::merge(
                &baselines.join(file),
                &store.join(file),
                &pass.data_dir.join(file),
            ),
            SyncUnit::Hotbars => hotbars::merge(
                &baselines.join(file),
                &store.join(file),
                &pass.data_dir.join(file),
            ),
            SyncUnit::Screenshots | SyncUnit::ResourcePacks => Ok(()),
        };
        if settled.is_ok() {
            let _ = std::fs::remove_file(&interrupted);
        }
        settled
    }

    pub fn status(&self, pass: &Pass) -> Vec<UnitStatus> {
        let shared = self.dir();
        let catalogue = Catalogue::load(shared);
        catalogue::ALL
            .iter()
            .map(|&unit| UnitStatus {
                unit,
                state: self.state(unit, pass, shared, &catalogue),
            })
            .collect()
    }

    fn state(
        &self,
        unit: SyncUnit,
        pass: &Pass,
        shared: &Path,
        catalogue: &Catalogue,
    ) -> UnitState {
        if !catalogue.enabled(unit) {
            return UnitState::Off;
        }
        if !pass.overrides.shares(unit) {
            return UnitState::Overridden;
        }
        if !catalogue::supports(unit, &pass.game_version) {
            return UnitState::Unsupported;
        }
        let Some(file) = catalogue::file(unit) else {
            return UnitState::Synced;
        };
        let agreed = in_era(
            &shared.join(BASELINES).join(&pass.id),
            catalogue::era(unit, &pass.game_version),
        )
        .join(file);
        match agreed.exists() {
            true => UnitState::Synced,
            false => UnitState::Pending,
        }
    }

    pub fn options(&self) -> Vec<SyncOption> {
        let shared = self.dir();
        let catalogue = Catalogue::load(shared);
        options::read(&shared.join(catalogue::OPTIONS))
            .into_iter()
            .map(|(key, value)| SyncOption {
                synced: !catalogue.unsynced().contains(&key),
                key,
                value,
            })
            .collect()
    }

    pub fn set_option(&self, key: &str, value: &str) -> Result<()> {
        let path = self.dir().join(catalogue::OPTIONS);
        options::set(&path, key, value)
    }

    pub fn remember(&self, session: &str, pass: Pass) {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.to_string(), pass);
    }

    pub fn recall(&self, session: &str) -> Option<Pass> {
        self.sessions.lock().unwrap().remove(session)
    }

    pub fn forget(&self, id: &str) {
        let dir = self.dir().join(BASELINES).join(id);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!(instance = id, error = %e, "cannot drop the sync baselines");
            }
        }
    }
}

fn in_era(root: &Path, era: &str) -> PathBuf {
    match era.is_empty() {
        true => root.to_path_buf(),
        false => root.join(era),
    }
}

fn excluded_keys(catalogue: &Catalogue, pass: &Pass) -> BTreeSet<String> {
    catalogue
        .unsynced()
        .iter()
        .cloned()
        .chain(pass.overrides.unsynced.iter().cloned())
        .chain(
            catalogue::NEVER_SHARED_KEYS
                .iter()
                .map(|key| key.to_string()),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;

    fn temp_dir(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-sync-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn sharing(shared: &Path) -> Sync {
        let sync = Sync::new(shared.to_path_buf());
        for &unit in catalogue::ALL {
            sync.enable(unit, "test").unwrap();
        }
        sync
    }

    fn pass(name: &str, data_dir: &Path) -> Pass {
        Pass {
            id: name.to_string(),
            name: name.to_string(),
            game_version: "1.21.4".to_string(),
            data_dir: data_dir.to_path_buf(),
            overrides: SyncOverrides::default(),
        }
    }

    /// Stamped a known distance in the past, so a pass that stamps `now` where
    /// it should have stamped nothing is unambiguous.
    fn write_at(path: &Path, contents: &str, seconds_ago: u64) {
        write_bytes_at(path, contents.as_bytes(), seconds_ago)
    }

    fn write_bytes_at(path: &Path, contents: &[u8], seconds_ago: u64) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
        let when = SystemTime::now() - Duration::from_secs(seconds_ago);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    fn hotbar_holding(id: &str) -> Vec<u8> {
        let mut stack = HashMap::new();
        stack.insert("id".to_string(), fastnbt::Value::String(id.to_string()));
        let mut root: HashMap<String, fastnbt::Value> = HashMap::new();
        root.insert(
            "0".to_string(),
            fastnbt::Value::List(vec![fastnbt::Value::Compound(stack)]),
        );
        fastnbt::to_bytes(&root).unwrap()
    }

    fn holds(path: &Path, id: &str) -> bool {
        let Ok(bytes) = fs::read(path) else {
            return false;
        };
        String::from_utf8_lossy(&bytes).contains(id)
    }

    #[test]
    fn a_unit_is_off_until_it_is_enabled() {
        let base = temp_dir("off");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("options.txt"), "guiScale:3\n").unwrap();

        let sync = Sync::new(shared.clone());
        sync.apply(&pass("test", &data));
        assert!(!data.join("options.txt").exists());

        sync.enable(SyncUnit::Options, "other").unwrap();
        sync.apply(&pass("test", &data));
        assert!(fs::read_to_string(data.join("options.txt"))
            .unwrap()
            .contains("guiScale:3"));
    }

    #[test]
    fn seeding_starts_the_shared_copy_from_one_instance() {
        let base = temp_dir("seed");
        let shared = base.path().join("shared");
        let source = base.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("options.txt"), "fov:90\n").unwrap();

        Sync::new(shared.clone())
            .seed(SyncUnit::Options, "1.21.4", &source)
            .unwrap();

        assert!(fs::read_to_string(shared.join("options.txt"))
            .unwrap()
            .contains("fov:90"));
    }

    #[test]
    fn what_the_shared_copy_lands_on_is_kept_first() {
        let base = temp_dir("backup");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        write_at(&data.join("options.txt"), "guiScale:4\n", 100);

        let sync = sharing(&shared);
        let kept = shared.join(BACKUPS).join("test");
        write_at(&kept.join("1700000000").join("options.txt"), "old\n", 50);
        write_at(&kept.join("1700000000").join("servers.dat"), "other\n", 50);
        sync.back_up(SyncUnit::Options, "test", &data).unwrap();

        assert!(fs::read_to_string(kept.join("options.txt"))
            .unwrap()
            .contains("guiScale:4"));
        assert!(!kept.join("1700000000").join("options.txt").exists());
        assert!(kept.join("1700000000").join("servers.dat").exists());
        assert!(!kept.join("options.txt.part").exists());
    }

    #[test]
    fn a_pass_that_died_mid_write_settles_the_shared_copys_way() {
        let base = temp_dir("interrupted");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        let sync = sharing(&shared);

        write_at(&shared.join("options.txt"), "guiScale:1\n", 300);
        sync.apply(&pass("test", &data));

        write_at(&shared.join("options.txt"), "guiScale:2\n", 200);
        write_at(&data.join("options.txt"), "guiScale:9\n", 100);
        fs::write(
            shared
                .join(BASELINES)
                .join("test")
                .join("options.txt.pending"),
            [],
        )
        .unwrap();
        sync.apply(&pass("test", &data));

        assert!(fs::read_to_string(data.join("options.txt"))
            .unwrap()
            .contains("guiScale:2"));
    }

    #[test]
    fn deferring_lets_the_shared_copy_win() {
        let base = temp_dir("defer");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        write_at(&shared.join("options.txt"), "guiScale:1\n", 300);
        write_at(&data.join("options.txt"), "guiScale:4\n", 100);

        let sync = sharing(&shared);
        sync.defer(SyncUnit::Options, "test", "1.21.4", &data)
            .unwrap();
        sync.apply(&pass("test", &data));

        assert!(fs::read_to_string(data.join("options.txt"))
            .unwrap()
            .contains("guiScale:1"));
    }

    /// The drift the baseline exists to stop: an instance that changed nothing
    /// must not outrank one that did, however the clock falls.
    #[test]
    fn an_idle_instance_cannot_revert_another_ones_edit() {
        let base = temp_dir("drift");
        let shared = base.path().join("shared");
        let a = base.path().join("a");
        let b = base.path().join("b");
        let sync = sharing(&shared);

        write_at(&shared.join("options.txt"), "guiScale:1\n", 300);
        sync.apply(&pass("a", &a));
        sync.apply(&pass("b", &b));

        write_at(&b.join("options.txt"), "guiScale:4\n", 100);
        sync.apply(&pass("a", &a));
        sync.apply(&pass("b", &b));
        sync.apply(&pass("a", &a));

        assert!(fs::read_to_string(a.join("options.txt"))
            .unwrap()
            .contains("guiScale:4"));
    }

    #[test]
    fn two_instances_changing_different_keys_both_survive() {
        let base = temp_dir("keys");
        let shared = base.path().join("shared");
        let a = base.path().join("a");
        let b = base.path().join("b");
        let sync = sharing(&shared);

        write_at(&shared.join("options.txt"), "guiScale:1\nfov:70\n", 300);
        sync.apply(&pass("a", &a));
        sync.apply(&pass("b", &b));

        write_at(&a.join("options.txt"), "guiScale:3\nfov:70\n", 200);
        write_at(&b.join("options.txt"), "guiScale:1\nfov:90\n", 100);
        sync.apply(&pass("a", &a));
        sync.apply(&pass("b", &b));
        sync.apply(&pass("a", &a));

        let merged = fs::read_to_string(a.join("options.txt")).unwrap();
        assert!(
            merged.contains("guiScale:3"),
            "a's change survives: {merged}"
        );
        assert!(merged.contains("fov:90"), "b's change reaches a: {merged}");
    }

    #[test]
    fn what_a_machine_may_disagree_about_starts_pinned() {
        let base = temp_dir("defaults");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        write_at(
            &shared.join("options.txt"),
            "renderDistance:32\nfov:90\n",
            300,
        );

        let sync = sharing(&shared);
        sync.apply(&pass("test", &data));

        let local = fs::read_to_string(data.join("options.txt")).unwrap();
        assert!(local.contains("fov:90"));
        assert!(!local.contains("renderDistance"));
    }

    #[test]
    fn a_key_one_instance_pins_stays_out_of_its_copy_only() {
        let base = temp_dir("pin");
        let shared = base.path().join("shared");
        let a = base.path().join("a");
        let b = base.path().join("b");
        let sync = sharing(&shared);

        write_at(&shared.join("options.txt"), "guiScale:1\n", 300);
        write_at(&a.join("options.txt"), "guiScale:4\n", 100);

        let mut pinned = pass("a", &a);
        pinned.overrides.unsynced.insert("guiScale".to_string());
        sync.apply(&pinned);
        sync.apply(&pass("b", &b));

        assert!(fs::read_to_string(a.join("options.txt"))
            .unwrap()
            .contains("guiScale:4"));
        assert!(
            fs::read_to_string(shared.join("options.txt"))
                .unwrap()
                .contains("guiScale:1"),
            "pinning on one instance must not strip the key for the others"
        );
        assert!(fs::read_to_string(b.join("options.txt"))
            .unwrap()
            .contains("guiScale:1"));
    }

    #[test]
    fn pack_selection_stays_instance_local() {
        let base = temp_dir("packs");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(
            data.join("options.txt"),
            "guiScale:2\nresourcePacks:[\"cozy\"]\n",
        )
        .unwrap();

        sharing(&shared).apply(&pass("test", &data));

        let stored = fs::read_to_string(shared.join("options.txt")).unwrap();
        assert!(stored.contains("guiScale:2"));
        assert!(!stored.contains("resourcePacks"));
        assert!(fs::read_to_string(data.join("options.txt"))
            .unwrap()
            .contains("resourcePacks"));
    }

    #[test]
    fn an_instance_that_excludes_a_unit_keeps_its_own() {
        let base = temp_dir("excluded");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        write_at(&shared.join("options.txt"), "guiScale:1\n", 300);
        write_at(&data.join("options.txt"), "guiScale:4\n", 100);

        let sync = sharing(&shared);
        let mut opted_out = pass("test", &data);
        opted_out.overrides.excluded.insert(SyncUnit::Options);
        sync.apply(&opted_out);

        assert!(fs::read_to_string(data.join("options.txt"))
            .unwrap()
            .contains("guiScale:4"));
        assert_eq!(
            sync.status(&opted_out)
                .iter()
                .find(|status| status.unit == SyncUnit::Options)
                .map(|status| status.state),
            Some(UnitState::Overridden)
        );
    }

    #[test]
    fn the_command_history_is_a_union_of_both_sides() {
        let base = temp_dir("history");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        write_at(
            &shared.join("command_history.txt"),
            "/gamemode creative\n",
            300,
        );
        write_at(&data.join("command_history.txt"), "/time set day\n", 100);

        sharing(&shared).apply(&pass("test", &data));

        let merged = fs::read_to_string(data.join("command_history.txt")).unwrap();
        assert!(merged.contains("/gamemode creative"));
        assert!(merged.contains("/time set day"));
    }

    #[test]
    fn a_missing_file_is_restored_rather_than_deleted_from_the_store() {
        let base = temp_dir("nodelete");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        let sync = sharing(&shared);

        let saved = hotbar_holding("minecraft:stone");
        write_bytes_at(&shared.join("components").join("hotbar.nbt"), &saved, 300);
        sync.apply(&pass("test", &data));
        fs::remove_file(data.join("hotbar.nbt")).unwrap();
        sync.apply(&pass("test", &data));

        assert!(holds(
            &shared.join("components").join("hotbar.nbt"),
            "minecraft:stone"
        ));
        assert!(holds(&data.join("hotbar.nbt"), "minecraft:stone"));
    }

    #[test]
    fn a_legacy_instance_shares_what_its_version_can_hold() {
        let base = temp_dir("era");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("options.txt"), "guiScale:3\n").unwrap();
        fs::create_dir_all(shared.join("legacy")).unwrap();
        fs::write(
            shared.join("legacy").join("hotbar.nbt"),
            hotbar_holding("minecraft:stone"),
        )
        .unwrap();

        let legacy = Pass {
            game_version: "1.12.2".to_string(),
            ..pass("old", &data)
        };
        let warnings = sharing(&shared).apply(&legacy);

        assert!(fs::read_to_string(data.join("options.txt"))
            .unwrap()
            .contains("guiScale:3"));
        assert!(data.join("hotbar.nbt").exists());
        assert!(warnings
            .iter()
            .any(|warning| matches!(warning, WarningInfo::SyncUnitUnsupported { .. })));
    }

    #[test]
    fn hotbars_are_kept_apart_across_the_item_format_break() {
        let base = temp_dir("eras");
        let shared = base.path().join("shared");
        let modern_dir = base.path().join("modern");
        let old_dir = base.path().join("old");
        let sync = sharing(&shared);

        let modern = pass("modern", &modern_dir);
        let old = Pass {
            game_version: "1.20.4".to_string(),
            ..pass("old", &old_dir)
        };
        write_bytes_at(
            &modern_dir.join("hotbar.nbt"),
            &hotbar_holding("minecraft:stone"),
            200,
        );
        write_bytes_at(
            &old_dir.join("hotbar.nbt"),
            &hotbar_holding("minecraft:torch"),
            100,
        );
        sync.apply(&modern);
        sync.apply(&old);
        sync.apply(&modern);

        assert!(holds(
            &shared.join("components").join("hotbar.nbt"),
            "minecraft:stone"
        ));
        assert!(holds(
            &shared.join("legacy").join("hotbar.nbt"),
            "minecraft:torch"
        ));
        assert!(!holds(&modern_dir.join("hotbar.nbt"), "minecraft:torch"));
        assert!(!holds(&old_dir.join("hotbar.nbt"), "minecraft:stone"));
    }

    #[test]
    fn a_unit_is_gated_by_the_version_that_first_writes_its_file() {
        assert!(catalogue::supports(SyncUnit::Options, "1.12.2"));
        assert!(catalogue::supports(SyncUnit::Commands, "23w14a"));
        assert!(!catalogue::supports(SyncUnit::Commands, "1.20.1"));
        assert!(catalogue::supports(SyncUnit::Commands, "1.20.2"));
        assert!(!catalogue::supports(SyncUnit::Hotbars, "1.11.2"));
        assert!(catalogue::supports(SyncUnit::Hotbars, "1.12"));
        assert!(catalogue::supports(SyncUnit::Servers, "1.7.10"));
    }

    #[test]
    fn a_merge_leaves_everything_it_did_not_settle_alone() {
        let base = temp_dir("shape");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        let sync = sharing(&shared);

        write_at(&shared.join("options.txt"), "guiScale:1\n", 300);
        write_at(
            &data.join("options.txt"),
            "# mine\r\nfov:70\r\nguiScale:1\r\n",
            100,
        );
        sync.apply(&pass("test", &data));

        assert_eq!(
            fs::read_to_string(data.join("options.txt")).unwrap(),
            "# mine\r\nfov:70\r\nguiScale:1\r\n"
        );
    }

    #[test]
    fn a_key_that_describes_the_file_or_the_machine_never_travels() {
        let base = temp_dir("never");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(
            data.join("options.txt"),
            "guiScale:2\nversion:4325\nlastServer:example.net\noverrideWidth:1920\n",
        )
        .unwrap();

        sharing(&shared).apply(&pass("test", &data));

        let stored = fs::read_to_string(shared.join("options.txt")).unwrap();
        assert!(stored.contains("guiScale:2"));
        for key in ["version", "lastServer", "overrideWidth"] {
            assert!(!stored.contains(key), "{key} must not reach the store");
        }
    }

    #[test]
    fn forget_drops_only_that_instances_agreements() {
        let base = temp_dir("forget");
        let shared = base.path().join("shared");
        let a = base.path().join("a");
        let b = base.path().join("b");
        let sync = sharing(&shared);

        write_at(&shared.join("options.txt"), "guiScale:1\n", 300);
        sync.apply(&pass("a", &a));
        sync.apply(&pass("b", &b));
        assert!(shared.join(BASELINES).join("a").exists());

        sync.forget("a");
        assert!(!shared.join(BASELINES).join("a").exists());
        assert!(shared.join(BASELINES).join("b").exists());
    }

    #[test]
    fn editing_the_shared_options_reaches_the_next_launch() {
        let base = temp_dir("edit");
        let shared = base.path().join("shared");
        let data = base.path().join("data");
        let sync = sharing(&shared);

        sync.set_option("fov", "90").unwrap();
        sync.apply(&pass("test", &data));

        assert!(fs::read_to_string(data.join("options.txt"))
            .unwrap()
            .contains("fov:90"));
        assert!(sync
            .options()
            .iter()
            .any(|option| option.key == "fov" && option.synced));
    }
}
