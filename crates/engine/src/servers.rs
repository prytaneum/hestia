//! Persistent Minecraft server store: each server lives at `<dir>/<slug>/` (the
//! name slugged; the id is opaque) — the `server.json` record beside `data/`,
//! the working directory the game
//! itself runs in (jar, `eula.txt`, `server.properties`, the world). The root
//! is reserved for managed content directories (`mods/`, `plugins/`,
//! `configs/`, `backups/`); every directory appears on demand. Listing scans
//! the parent — the disk is the registry.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use proto::keys::KeyRule;
use proto::minecraft::{ProvisionPhase, ProvisionProgress};
use proto::server::ServerProperty;
use proto::tasks::ServerTask;
use serde::{Deserialize, Serialize};

use crate::backup::BackupSettings;
use crate::keys::invalid;
use crate::minecraft::launch::{self, JavaSettings, LaunchPlan};
use crate::minecraft::materialize::{self, OnProgress};
use crate::minecraft::profile::ServerProfile;
use crate::process::{Outcome, ProcessSupervisor, Task};
use crate::properties;
use crate::registry;
use crate::schema::Document;

const PROPERTIES: &str = "server.properties";
// The key set the server's own pristine generation run emitted — the schema
// `config_set` validates against, kept beside the record because it describes
// the version hestia installed, not the values the game currently holds.
const SCHEMA: &str = "schema.properties";
const SCHEMA_RUN: &str = ".schema";
const DATA: &str = "data";
const GAME_PORT_BASE: u16 = 25565;
const RCON_PORT_BASE: u16 = 25575;
const PORT_SPAN: u16 = 100;

// Pre-EULA servers (before 1.7.10) have no gate and would boot for real; the
// generation run is killed after this long and the file check decides.
const GENERATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// `server.properties` keys hestia owns; a `config set` to any of them is
/// rejected (the game port is fixed at create, rcon is configured at start).
const MANAGED_PROPERTIES: &[&str] = &["server-port", "enable-rcon", "rcon.port", "rcon.password"];

/// Never sent to a front-end: nothing there needs the console's credential.
const SECRET_PROPERTIES: &[&str] = &["rcon.password"];

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RconConfig {
    pub port: u16,
    pub password: String,
}

/// Where a server is in its lifecycle. The disk is the registry, so this has to
/// survive a crash *and* say which kind of unfinished it is: a create that never
/// completed holds nothing of the user's and is an orphan to discard, while an
/// update that never completed belongs to a real server with a world and must be
/// left alone. A single `ready: bool` conflated the two, and a daemon killed
/// mid-create left a permanently un-startable entry nothing ever reconciled.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ServerPhase {
    /// The create job is still provisioning files; nothing here is the user's.
    #[default]
    Provisioning,
    /// Fully provisioned and startable.
    Ready,
    /// A version update is in flight over an entry that was ready — it has a
    /// world, so it is never discarded; updating again recovers it.
    Updating,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerRecord {
    pub id: String,
    pub name: String,
    pub created_unix: i64,
    #[serde(default)]
    pub phase: ServerPhase,
    /// Claimed at create and never moved — players connect to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_port: Option<u16>,
    /// Claimed at first start; internal, so it may be reallocated freely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rcon: Option<RconConfig>,
    /// Per-entry JVM tuning (memory, extra flags) injected at each start.
    #[serde(default)]
    pub jvm: JavaSettings,
    /// Scheduled-backup tuning (interval, retention); unset disables the
    /// schedule.
    #[serde(default)]
    pub backup: BackupSettings,
    /// What the daemon runs on a schedule while the server is up.
    #[serde(default)]
    pub tasks: Vec<ServerTask>,
    pub profile: ServerProfile,
}

impl Document for ServerRecord {
    const NAME: &'static str = "server.json";
}

impl ServerRecord {
    /// Whether the server can be started: fully provisioned, nothing in flight.
    pub fn ready(&self) -> bool {
        self.phase == ServerPhase::Ready
    }
}

pub struct Servers {
    dir: Mutex<PathBuf>,
    // Serializes scan-pick-persist port claims so two concurrent creates or
    // starts cannot claim the same port.
    claims: Mutex<()>,
}

impl Servers {
    pub fn new(dir: PathBuf) -> Self {
        Servers {
            dir: Mutex::new(dir),
            claims: Mutex::new(()),
        }
    }

    pub fn reload(&self, dir: PathBuf) {
        *self.dir.lock().unwrap() = dir;
    }

    pub fn dir(&self) -> PathBuf {
        self.dir.lock().unwrap().clone()
    }

    /// The server's directory, named for its current display name, so a rename
    /// moves it; the id stays the entry's stable internal key.
    pub fn server_dir(&self, record: &ServerRecord) -> PathBuf {
        self.dir()
            .join(registry::dir_name(&record.id, &record.name))
    }

    /// The server's working directory — everything the game itself reads and
    /// writes (jar, libraries, `eula.txt`, `server.properties`, the world).
    pub fn data_dir(&self, record: &ServerRecord) -> PathBuf {
        self.server_dir(record).join(DATA)
    }

    /// The `server.properties` schema derived for this server's version. Absent
    /// when the generation run could not produce one, in which case `config_set`
    /// validates nothing.
    pub fn schema_path(&self, record: &ServerRecord) -> PathBuf {
        self.server_dir(record).join(SCHEMA)
    }

    /// Whether this server has a validatable property schema. False means every
    /// unmanaged key is accepted — see [`Servers::config_set`].
    pub fn has_schema(&self, record: &ServerRecord) -> bool {
        self.schema_path(record).is_file()
    }

    pub fn list(&self) -> Vec<ServerRecord> {
        let mut records: Vec<ServerRecord> = registry::scan(&self.dir());
        records.sort_by(|a, b| a.name.cmp(&b.name));
        records
    }

    /// Find one server by id or name (any spelling that slugs the same).
    pub fn get(&self, reference: &str) -> Option<ServerRecord> {
        self.list()
            .into_iter()
            .find(|r| proto::naming::reference_matches(reference, &r.id, &r.name))
    }

    /// Register a new server: allocate its id from the name, claim its game
    /// port, create its directory, and write the (not yet ready) record.
    pub fn create(
        &self,
        name: &str,
        profile: ServerProfile,
        port: Option<u16>,
    ) -> Result<ServerRecord> {
        if registry::name_taken(name, self.list().iter().map(|r| r.name.as_str())) {
            bail!(proto::error::ErrorInfo::AlreadyExists {
                entry: proto::error::EntryKind::Server,
                name: name.to_string()
            });
        }
        registry::slugify(name)?;
        let id = registry::allocate_id(|id| self.get(id).is_some())?;
        let _claims = self.claims.lock().unwrap();
        let game_port = self.claim_game_port(port)?;
        let record = ServerRecord {
            id,
            name: name.to_string(),
            created_unix: registry::now_unix(),
            phase: ServerPhase::Provisioning,
            game_port: Some(game_port),
            rcon: None,
            jvm: JavaSettings::default(),
            backup: BackupSettings::default(),
            tasks: Vec::new(),
            profile,
        };
        let dir = self.server_dir(&record);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create {}", dir.display()))?;
        registry::write_record(&dir, &record)?;
        tracing::info!(id = %record.id, name, game_port, "server registered");
        Ok(record)
    }

    fn claim_game_port(&self, requested: Option<u16>) -> Result<u16> {
        let claimed = self.claimed_ports(None);
        match requested {
            Some(port) => {
                if claimed.contains(&port) {
                    bail!(proto::error::ErrorInfo::PortUnavailable { port });
                }
                if !can_bind(port) {
                    bail!(proto::error::ErrorInfo::PortUnavailable { port });
                }
                Ok(port)
            }
            None => allocate_port(GAME_PORT_BASE, &claimed),
        }
    }

    /// Every port any server's record claims (game and rcon), except
    /// `exclude`'s own.
    fn claimed_ports(&self, exclude: Option<&str>) -> HashSet<u16> {
        self.list()
            .iter()
            .filter(|r| Some(r.id.as_str()) != exclude)
            .flat_map(|r| {
                r.game_port
                    .into_iter()
                    .chain(r.rcon.as_ref().map(|c| c.port))
            })
            .collect()
    }

    /// Reconcile `server.properties` with the record's claimed ports before a
    /// spawn. The game port never moves (players depend on it) — an outside
    /// squatter is an error. The rcon port is internal and reallocates freely;
    /// a record from before ports existed gains both here.
    pub fn ensure_start_config(&self, id: &str) -> Result<ServerRecord> {
        let _claims = self.claims.lock().unwrap();
        let mut record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        let claimed = self.claimed_ports(Some(&record.id));

        let game_port = match record.game_port {
            Some(port) => port,
            None => allocate_port(GAME_PORT_BASE, &claimed)?,
        };
        if claimed.contains(&game_port) || !can_bind(game_port) {
            bail!(proto::error::ErrorInfo::PortUnavailable { port: game_port });
        }

        let rcon = match record.rcon.take() {
            Some(cfg) if !claimed.contains(&cfg.port) && can_bind(cfg.port) => cfg,
            prior => RconConfig {
                port: allocate_port(RCON_PORT_BASE, &claimed)?,
                password: prior.map(|c| c.password).unwrap_or_else(generate_password),
            },
        };

        merge_properties(
            &self.data_dir(&record).join(PROPERTIES),
            &[
                ("server-port", game_port.to_string()),
                ("enable-rcon", "true".to_string()),
                ("rcon.port", rcon.port.to_string()),
                ("rcon.password", rcon.password.clone()),
            ],
        )?;
        record.game_port = Some(game_port);
        record.rcon = Some(rcon);
        registry::write_record(&self.server_dir(&record), &record)?;
        Ok(record)
    }

    /// Create the server's data directory and record the EULA acceptance the
    /// caller obtained from the user.
    pub fn provision(&self, record: &ServerRecord) -> Result<()> {
        let data = self.data_dir(record);
        std::fs::create_dir_all(&data)
            .with_context(|| format!("cannot create {}", data.display()))?;
        materialize::validate_filename(record.profile.jar.filename())?;
        std::fs::write(data.join("eula.txt"), "eula=true\n").context("cannot write eula.txt")?;
        tracing::info!(id = %record.id, "server provisioned");
        Ok(())
    }

    /// Run the server once to derive this version's `server.properties`
    /// **schema** and seed the live file with any key it is missing.
    /// Best-effort: a server with no schema accepts any property key rather
    /// than rejecting every one, so a failure here degrades validation instead
    /// of failing the operation — the caller reports it by asking
    /// [`Servers::has_schema`].
    pub(crate) async fn derive_properties_schema(
        &self,
        record: &ServerRecord,
        java: &Path,
        processes: &ProcessSupervisor,
        on_progress: OnProgress<'_>,
    ) {
        on_progress.report(&ProvisionProgress {
            phase: ProvisionPhase::Server,
            current: 0,
            total: 0,
            detail: "generating server.properties".into(),
            ..ProvisionProgress::default()
        });
        self.derive_schema(record, java, processes, on_progress)
            .await;
    }

    async fn derive_schema(
        &self,
        record: &ServerRecord,
        java: &Path,
        processes: &ProcessSupervisor,
        job: OnProgress<'_>,
    ) {
        match self.generate_schema(record, java, processes, job).await {
            Ok(schema) if schema.is_empty() => {
                tracing::warn!(id = %record.id, "the schema run wrote no server.properties");
            }
            Ok(schema) => {
                let keys = schema.len();
                if let Err(e) = seed_properties(&self.data_dir(record).join(PROPERTIES), &schema) {
                    tracing::warn!(id = %record.id, error = format!("{e:#}"), "cannot seed server.properties");
                }
                tracing::info!(id = %record.id, keys, "server.properties schema derived");
            }
            Err(e) => {
                tracing::warn!(id = %record.id, error = format!("{e:#}"), "server.properties schema generation failed");
            }
        }
    }

    /// Run the server once in a throwaway directory to make it write the
    /// complete `server.properties` for exactly its version, and store that
    /// pristine file as the schema. The run has no `eula.txt`, so the gate stops
    /// it right after the write, before it binds ports or generates a world; and
    /// it has no properties file to round-trip, so what it writes is the keys
    /// this version knows rather than a rewrite of the values the user set.
    async fn generate_schema(
        &self,
        record: &ServerRecord,
        java: &Path,
        processes: &ProcessSupervisor,
        job: OnProgress<'_>,
    ) -> Result<Vec<(String, String)>> {
        let scratch = self.server_dir(record).join(SCHEMA_RUN);
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch)
            .with_context(|| format!("cannot create {}", scratch.display()))?;
        let generated = self
            .run_schema_generation(record, java, &scratch, processes, job)
            .await;
        let pristine = scratch.join(PROPERTIES);
        let schema = read_properties(&pristine);
        if !schema.is_empty() {
            std::fs::copy(&pristine, self.schema_path(record))
                .context("cannot store the properties schema")?;
        }
        let _ = std::fs::remove_dir_all(&scratch);
        generated?;
        Ok(schema)
    }

    async fn run_schema_generation(
        &self,
        record: &ServerRecord,
        java: &Path,
        scratch: &Path,
        processes: &ProcessSupervisor,
        job: OnProgress<'_>,
    ) -> Result<()> {
        let plan = launch::server_schema_plan(
            &record.profile,
            java,
            &self.data_dir(record),
            scratch,
            &record.jvm,
        );
        let outcome = processes
            .run(
                Task {
                    id: format!("schema-{}", record.id),
                    program: &plan.program,
                    args: plan.args.clone(),
                    cwd: plan.cwd.clone(),
                    phase: ProvisionPhase::Server,
                    narrates: crate::process::silent,
                    deadline: Some(GENERATE_TIMEOUT),
                },
                job,
            )
            .await?;
        match outcome {
            Outcome::TimedOut => {
                tracing::debug!(id = %record.id, "server.properties schema run timed out (no EULA gate?)");
            }
            Outcome::Exited(code) => {
                tracing::debug!(id = %record.id, ?code, "server.properties schema run exited");
            }
        }
        Ok(())
    }

    /// Move a server onto a freshly resolved profile, under the `updating`
    /// gate: a crash anywhere before [`Servers::finish_update`] leaves a record
    /// that says so rather than one that looks finished, and updating again
    /// recovers it. Name, ports, JVM settings, and the world on disk are
    /// untouched. Returns the swapped record and the jar it replaces.
    pub fn begin_update(&self, id: &str, profile: ServerProfile) -> Result<(ServerRecord, String)> {
        let mut record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        materialize::validate_filename(profile.jar.filename())?;
        let previous_jar = record.profile.jar.filename().to_string();
        record.profile = profile;
        record.phase = ServerPhase::Updating;
        registry::write_record(&self.server_dir(&record), &record)?;
        Ok((record, previous_jar))
    }

    pub fn finish_update(&self, record: &ServerRecord, previous_jar: &str) -> Result<ServerRecord> {
        if previous_jar != record.profile.jar.filename() {
            let _ = std::fs::remove_file(self.data_dir(record).join(previous_jar));
        }
        tracing::info!(
            id = %record.id,
            version = %record.profile.game_version,
            loader = ?record.profile.loader_version,
            "server updated"
        );
        self.mark_ready(&record.id)
    }

    pub fn mark_ready(&self, id: &str) -> Result<ServerRecord> {
        self.mark_phase(id, ServerPhase::Ready)
    }

    /// Move a server to another lifecycle phase, persisted immediately — the
    /// disk is the registry, and startup recovery reads exactly this.
    pub fn mark_phase(&self, id: &str, phase: ServerPhase) -> Result<ServerRecord> {
        let mut record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        record.phase = phase;
        registry::write_record(&self.server_dir(&record), &record)?;
        Ok(record)
    }

    /// Reconcile records a crash left mid-lifecycle. No job survives a restart,
    /// so anything still `Provisioning` belongs to a create that will never
    /// finish — an entry that never existed as far as the user is concerned, and
    /// which otherwise persists forever as an un-startable orphan holding a port
    /// claim. It is discarded, the same conclusion `provision_server` reaches
    /// when the create fails while the daemon is alive.
    ///
    /// An `Updating` record is left alone: it was a real, ready server before
    /// the update began, so its world is on disk. Updating it again recovers it.
    /// Returns the names discarded.
    pub fn reconcile(&self) -> Vec<String> {
        let mut discarded = Vec::new();
        for record in self.list() {
            match record.phase {
                ServerPhase::Ready => {}
                ServerPhase::Updating => tracing::warn!(
                    id = %record.id,
                    name = %record.name,
                    "server was mid-update when the daemon stopped; update it again to finish"
                ),
                ServerPhase::Provisioning => {
                    tracing::warn!(
                        id = %record.id,
                        name = %record.name,
                        "discarding a server whose create never finished"
                    );
                    match self.remove(&record.id) {
                        Ok(_) => discarded.push(record.name),
                        Err(e) => tracing::warn!(
                            id = %record.id,
                            error = format!("{e:#}"),
                            "cannot discard an unprovisioned server"
                        ),
                    }
                }
            }
        }
        discarded
    }

    /// Rename a server: rewrite the display name and move its directory to the
    /// new slug. The id is stable, so ports, rcon, the process, and JVM/backup
    /// settings are untouched. The caller guarantees it is stopped and not busy.
    pub fn rename(&self, reference: &str, new_name: &str) -> Result<ServerRecord> {
        let _claims = self.claims.lock().unwrap();
        let mut record = self
            .get(reference)
            .with_context(|| format!("unknown server: {reference}"))?;
        if registry::name_taken(
            new_name,
            self.list()
                .iter()
                .filter(|r| r.id != record.id)
                .map(|r| r.name.as_str()),
        ) {
            bail!(proto::error::ErrorInfo::AlreadyExists {
                entry: proto::error::EntryKind::Server,
                name: new_name.to_string()
            });
        }
        registry::slugify(new_name)?;
        let old_dir = self.server_dir(&record);
        record.name = new_name.to_string();
        let new_dir = self.server_dir(&record);
        if new_dir != old_dir && old_dir.exists() {
            std::fs::rename(&old_dir, &new_dir).with_context(|| {
                format!("cannot move {} to {}", old_dir.display(), new_dir.display())
            })?;
        }
        registry::write_record(&new_dir, &record)?;
        tracing::info!(id = %record.id, name = %new_name, "server renamed");
        Ok(record)
    }

    /// Store a scheduled task: a new one when its id is empty, else in place
    /// of the one it names. A new task, or one whose schedule moved, counts
    /// from `now` — otherwise an interval would fire at once and a daily time
    /// already past would too.
    pub fn save_task(&self, id: &str, task: ServerTask, now: i64) -> Result<ServerTask> {
        let _claims = self.claims.lock().unwrap();
        let mut record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        let mut task = crate::tasks::checked(task)?;
        match record.tasks.iter_mut().find(|stored| stored.id == task.id) {
            Some(stored) => {
                task.last_run_unix = match stored.schedule == task.schedule {
                    true => stored.last_run_unix,
                    false => now,
                };
                *stored = task.clone();
            }
            None => {
                task.id = registry::allocate_id(|candidate| {
                    record.tasks.iter().any(|stored| stored.id == candidate)
                })?;
                task.last_run_unix = now;
                record.tasks.push(task.clone());
            }
        }
        registry::write_record(&self.server_dir(&record), &record)?;
        tracing::info!(server = %record.id, task = %task.id, schedule = %task.schedule, "task saved");
        Ok(task)
    }

    /// Returns false when the server has no task with that id.
    pub fn remove_task(&self, id: &str, task_id: &str) -> Result<bool> {
        let _claims = self.claims.lock().unwrap();
        let mut record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        let before = record.tasks.len();
        record.tasks.retain(|task| task.id != task_id);
        if record.tasks.len() == before {
            return Ok(false);
        }
        registry::write_record(&self.server_dir(&record), &record)?;
        Ok(true)
    }

    /// Stamp a task as run, so its schedule counts from here.
    pub fn mark_task_run(&self, id: &str, task_id: &str, now: i64) -> Result<()> {
        let _claims = self.claims.lock().unwrap();
        let mut record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        if let Some(task) = record.tasks.iter_mut().find(|task| task.id == task_id) {
            task.last_run_unix = now;
            registry::write_record(&self.server_dir(&record), &record)?;
        }
        Ok(())
    }

    /// Delete a server's directory (jar, world and all). Returns false when no
    /// server matches.
    pub fn remove(&self, reference: &str) -> Result<bool> {
        let Some(record) = self.get(reference) else {
            return Ok(false);
        };
        let dir = self.server_dir(&record);
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("cannot remove {}", dir.display()))?;
        tracing::info!(id = %record.id, "server removed");
        Ok(true)
    }

    pub fn launch_plan(
        &self,
        record: &ServerRecord,
        java: &Path,
        jvm: &JavaSettings,
    ) -> LaunchPlan {
        launch::server_plan(&record.profile, java, &self.data_dir(record), jvm)
    }

    /// Read one setting: a reserved JVM or backup key from the record, or any
    /// other key from `server.properties`. `Ok(None)` means the key is not set.
    pub fn config_get(&self, id: &str, key: &str) -> Result<Option<String>> {
        let record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        if let Some(value) = record.jvm.get(key).or_else(|| record.backup.get(key)) {
            return Ok(value);
        }
        Ok(read_property(&self.data_dir(&record).join(PROPERTIES), key))
    }

    /// Write one setting: a reserved JVM or backup key onto the record, or a
    /// `server.properties` key through to the file. A property key must exist in
    /// the **schema** the server itself generated for its version — not in the
    /// live file, which also carries keys no current version knows — so a typo
    /// cannot silently drift the file. A server with no derived schema accepts
    /// any unmanaged key rather than rejecting every one. The hestia-managed
    /// keys are rejected. A property value must satisfy the curated rule for its
    /// key; a key the table does not know takes any string. An empty value
    /// clears a JVM key. Settings take effect on the next start.
    pub fn config_set(&self, id: &str, key: &str, value: &str) -> Result<()> {
        let _claims = self.claims.lock().unwrap();
        let mut record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        if record.jvm.set(key, value)? || record.backup.set(key, value)? {
            registry::write_record(&self.server_dir(&record), &record)?;
            return Ok(());
        }
        if MANAGED_PROPERTIES.contains(&key) {
            bail!(
                "'{key}' is managed by hestia (the game port is fixed at create with -p; \
                 rcon is configured automatically)"
            );
        }
        let properties = self.data_dir(&record).join(PROPERTIES);
        let schema = self.schema_path(&record);
        if schema.is_file() {
            if read_property(&schema, key).is_none() {
                bail!(proto::error::ErrorInfo::ConfigKeyUnknown {
                    key: key.to_string()
                });
            }
        } else {
            tracing::debug!(
                id = %record.id,
                key,
                "no properties schema to validate against; accepting the key"
            );
        }
        properties::validate(key, value).map_err(invalid(key))?;
        if HARDCORE_KEYS.contains(&key) {
            check_hardcore_invariant(&properties, key, value)?;
        }
        merge_properties(&properties, &[(key, value.to_string())])
    }

    /// Every rule a server's settings are checked against.
    pub fn config_rules() -> Vec<KeyRule> {
        let mut rules = JavaSettings::rules();
        rules.extend(BackupSettings::rules());
        rules.extend(properties::rules().iter().cloned());
        rules
    }

    /// The reserved JVM and backup settings (always shown) followed by every
    /// current `server.properties` entry.
    pub fn config_list(&self, id: &str) -> Result<Vec<(String, String)>> {
        let record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        let mut entries = record.jvm.entries();
        entries.extend(record.backup.entries());
        entries.extend(read_properties(&self.data_dir(&record).join(PROPERTIES)));
        Ok(entries)
    }

    /// Every `server.properties` key this server knows: the ones its version's
    /// schema defines, plus any the live file carries beyond it, each with the
    /// value in effect.
    pub fn properties(&self, id: &str) -> Result<Vec<ServerProperty>> {
        let record = self
            .get(id)
            .with_context(|| format!("unknown server: {id}"))?;
        Ok(describe_properties(
            &read_properties(&self.data_dir(&record).join(PROPERTIES)),
            &read_properties(&self.schema_path(&record)),
        ))
    }
}

/// Parse `server.properties` into key/value pairs, skipping blank and comment
/// lines. Values are kept verbatim; the split is on the first `=`.
fn read_properties(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

/// The union of the schema's keys and the live file's, ordered by key, each
/// with the live value (the schema's default when the file has none).
fn describe_properties(
    live: &[(String, String)],
    schema: &[(String, String)],
) -> Vec<ServerProperty> {
    let value_of = |pairs: &[(String, String)], key: &str| {
        pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    let hardcore = value_of(live, "hardcore").is_some_and(|v| prop_is_true(&v));
    let keys: BTreeSet<&String> = schema.iter().chain(live).map(|(key, _)| key).collect();

    keys.into_iter()
        .map(|key| {
            let default = value_of(schema, key);
            let secret = SECRET_PROPERTIES.contains(&key.as_str());
            ServerProperty {
                key: key.clone(),
                value: match secret {
                    true => String::new(),
                    false => value_of(live, key)
                        .or_else(|| default.clone())
                        .unwrap_or_default(),
                },
                default: default.filter(|_| !secret),
                managed: MANAGED_PROPERTIES.contains(&key.as_str()),
                secret,
                locked: hardcore && HARDCORE_PINNED.contains(&key.as_str()),
            }
        })
        .collect()
}

fn read_property(path: &Path, key: &str) -> Option<String> {
    read_properties(path)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// The `server.properties` keys hardcore mode interlocks: enabling it forces
/// `difficulty=hard` and `gamemode=survival`.
const HARDCORE_KEYS: &[&str] = &["hardcore", "difficulty", "gamemode"];

/// The two of those hardcore fixes while it is on.
const HARDCORE_PINNED: &[&str] = &["difficulty", "gamemode"];

fn prop_is_true(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("true")
}

// Modern versions serialize these as names; pre-1.13 used the numeric ids
// (difficulty 3 = hard, gamemode 0 = survival), so accept both.
fn prop_is_hard(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("hard") || value == "3"
}

fn prop_is_survival(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("survival") || value == "0"
}

/// Enforce Minecraft's rule that hardcore forces `difficulty=hard` and
/// `gamemode=survival`: reject a `config set` to any of the three keys that
/// would leave them contradicting, so the file can never drift into a state
/// the game silently overrides at runtime. `key`/`value` is the pending edit.
fn check_hardcore_invariant(properties: &Path, key: &str, value: &str) -> Result<()> {
    let effective = |name: &str| -> Option<String> {
        if name == key {
            Some(value.to_string())
        } else {
            read_property(properties, name)
        }
    };
    if !effective("hardcore").as_deref().is_some_and(prop_is_true) {
        return Ok(());
    }
    let bad_difficulty = effective("difficulty").is_some_and(|v| !prop_is_hard(&v));
    let bad_gamemode = effective("gamemode").is_some_and(|v| !prop_is_survival(&v));
    if !bad_difficulty && !bad_gamemode {
        return Ok(());
    }
    let locked = |value: &str| proto::error::ErrorInfo::HardcoreLocked {
        key: key.to_string(),
        value: value.to_string(),
    };
    bail!(match key {
        "difficulty" => locked("hard"),
        "gamemode" => locked("survival"),
        _ => proto::error::ErrorInfo::HardcoreRequires {
            settings: [
                bad_difficulty.then_some("difficulty=hard"),
                bad_gamemode.then_some("gamemode=survival"),
            ]
            .into_iter()
            .flatten()
            .map(str::to_string)
            .collect(),
        },
    });
}

/// The server's world directory name (`level-name`, default `world`), read from
/// `server.properties` in `data_dir`. This is where the server keeps its world,
/// and so where datapacks install.
pub(crate) fn level_name(data_dir: &Path) -> String {
    read_property(&data_dir.join(PROPERTIES), "level-name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "world".to_string())
}

// Both the game and rcon listeners bind all interfaces, so probe the same way.
fn can_bind(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

fn allocate_port(base: u16, claimed: &HashSet<u16>) -> Result<u16> {
    (base..base.saturating_add(PORT_SPAN))
        .find(|port| !claimed.contains(port) && can_bind(*port))
        .with_context(|| format!("no free port in {base}..{}", base.saturating_add(PORT_SPAN)))
}

// Vanilla has no rcon bind-address setting — the listener is reachable from
// the network, so the password is the only barrier. Never log it.
fn generate_password() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("system RNG must be available for the rcon password");
    bytes
        .iter()
        .map(|b| CHARSET[(*b as usize) % CHARSET.len()] as char)
        .collect()
}

/// Add the keys the file does not yet carry, leaving every present key at its
/// current value. This is how a version update introduces the keys its schema
/// added without touching what the user set — and how a fresh server's file
/// starts out as the full schema. Keys no current version knows stay in the
/// file: it holds values, and deleting lines the user or a mod may own is worse
/// than the drift.
fn seed_properties(path: &Path, schema: &[(String, String)]) -> Result<()> {
    let present: HashSet<String> = read_properties(path).into_iter().map(|(k, _)| k).collect();
    let missing: Vec<(&str, String)> = schema
        .iter()
        .filter(|(key, _)| !present.contains(key))
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    merge_properties(path, &missing)
}

/// Rewrite `entries` into the properties file, preserving every other line
/// (user edits included) and appending keys not yet present. The data
/// directory appears on demand with the file.
fn merge_properties(path: &Path, entries: &[(&str, String)]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    for (key, value) in entries {
        let prefix = format!("{key}=");
        let entry = format!("{key}={value}");
        match lines
            .iter_mut()
            .find(|l| l.trim_start().starts_with(&prefix))
        {
            Some(line) => *line = entry,
            None => lines.push(entry),
        }
    }
    std::fs::write(path, lines.join("\n") + "\n")
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn properties_are_whatever_the_schema_and_the_file_carry() {
        let described = describe_properties(
            &schema(&[("motd", "mine"), ("retired", "leftover")]),
            &schema(&[("motd", "A Minecraft Server"), ("pvp", "true")]),
        );
        let row = |key: &str| described.iter().find(|p| p.key == key).unwrap();

        assert_eq!(
            described.iter().map(|p| &p.key).collect::<Vec<_>>(),
            ["motd", "pvp", "retired"]
        );
        assert_eq!(row("motd").value, "mine");
        assert_eq!(row("motd").default.as_deref(), Some("A Minecraft Server"));
        assert_eq!(row("pvp").value, "true");
        assert_eq!(row("retired").default, None);
    }

    #[test]
    fn a_managed_key_is_flagged_and_the_credential_is_withheld() {
        let pairs = schema(&[("server-port", "25565"), ("rcon.password", "hunter2")]);
        let described = describe_properties(&pairs, &pairs);
        let row = |key: &str| described.iter().find(|p| p.key == key).unwrap();

        assert!(row("server-port").managed);
        assert_eq!(row("server-port").value, "25565");
        assert!(row("rcon.password").secret);
        assert_eq!(row("rcon.password").value, "");
        assert_eq!(row("rcon.password").default, None);
    }

    #[test]
    fn hardcore_locks_what_it_pins_only_while_it_is_on() {
        let on = schema(&[("hardcore", "true"), ("difficulty", "hard"), ("motd", "")]);
        let described = describe_properties(&on, &[]);
        let locked =
            |list: &[ServerProperty], key: &str| list.iter().find(|p| p.key == key).unwrap().locked;
        assert!(locked(&described, "difficulty"));
        assert!(!locked(&described, "motd"));

        let off = schema(&[("hardcore", "false"), ("difficulty", "easy")]);
        assert!(!locked(&describe_properties(&off, &[]), "difficulty"));
    }

    #[test]
    fn seeding_adds_new_keys_and_keeps_every_existing_value() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(PROPERTIES);

        // A fresh server starts out as the whole schema.
        seed_properties(&path, &schema(&[("motd", "A Minecraft Server")])).unwrap();
        assert_eq!(
            read_property(&path, "motd").as_deref(),
            Some("A Minecraft Server")
        );

        // The user changes a value and the file keeps a key from an older
        // version; an update's schema adds one key.
        merge_properties(&path, &[("motd", "mine".to_string())]).unwrap();
        merge_properties(&path, &[("retired", "leftover".to_string())]).unwrap();
        seed_properties(
            &path,
            &schema(&[("motd", "A Minecraft Server"), ("new-key", "default")]),
        )
        .unwrap();

        assert_eq!(read_property(&path, "motd").as_deref(), Some("mine"));
        assert_eq!(read_property(&path, "new-key").as_deref(), Some("default"));
        assert_eq!(
            read_property(&path, "retired").as_deref(),
            Some("leftover"),
            "a key the new version does not know is values, not schema — never deleted"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
