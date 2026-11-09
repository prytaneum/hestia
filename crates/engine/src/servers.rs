//! Persistent Minecraft server store: each server lives at `<dir>/<slug>/` (the
//! name slugged; the id is opaque) — the `server.json` record beside `data/`,
//! the working directory the game
//! itself runs in (jar, `eula.txt`, `server.properties`, the world). The root
//! is reserved for managed content directories (`mods/`, `plugins/`,
//! `configs/`, `backups/`); every directory appears on demand. Listing scans
//! the parent — the disk is the registry.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::error::EntryKind;
use proto::keys::KeyRule;
use proto::minecraft::{Edition, ProvisionPhase, ProvisionProgress};
use proto::server::{PortProtocol, ServerPort, ServerProperty};
use proto::tasks::ServerTask;
use serde::{Deserialize, Serialize};

use crate::jvm::JavaSettings;
use crate::keys::invalid;
use crate::minecraft::launch::{self, LaunchPlan};
use crate::minecraft::materialize::{self, OnProgress};
use crate::minecraft::profile::{ServerProfile, ServerRuntime};
use crate::process::{Outcome, ProcessSupervisor, Task};
use crate::properties;
use crate::registry::{self, Entry, Store};
use crate::schema::Document;

// The key set the server's own pristine generation run emitted — the schema
// `config_set` validates against, kept beside the record because it describes
// the version hestia installed, not the values the game currently holds.
const SCHEMA: &str = "schema.properties";
const SCHEMA_RUN: &str = ".schema";
const DATA: &str = "data";
const GAME_PORT_BASE: u16 = 25565;
const RCON_PORT_BASE: u16 = 25575;
const BEDROCK_PORT_BASE: u16 = 19132;
const BEDROCK_SECOND_PORT_BASE: u16 = 19133;
const PORT_SPAN: u16 = 100;

// Pre-EULA servers (before 1.7.10) have no gate and would boot for real; the
// generation run is killed after this long and the file check decides.
const GENERATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

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
    pub game_port: u16,
    /// A Bedrock server's second UDP port (IPv6 under RakNet, the client
    /// transport under NetherNet); claimed at first start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_port: Option<u16>,
    /// Claimed at first start; internal, so it may be reallocated freely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rcon: Option<RconConfig>,
    /// Per-entry JVM tuning (memory, extra flags) injected at each start.
    #[serde(default)]
    pub jvm: JavaSettings,
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

impl Entry for ServerRecord {
    const KIND: EntryKind = EntryKind::Server;

    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

pub struct Servers {
    store: Store<ServerRecord>,
}

impl Servers {
    pub fn new(dir: PathBuf) -> Self {
        Servers {
            store: Store::new(dir),
        }
    }

    /// The server's directory, named for its current display name, so a rename
    /// moves it; the id stays the entry's stable internal key.
    pub fn server_dir(&self, record: &ServerRecord) -> PathBuf {
        self.store.entry_dir(record)
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
        self.store.list()
    }

    /// Find one server by id or name (any spelling that slugs the same).
    pub fn get(&self, reference: &str) -> Option<ServerRecord> {
        self.store.get(reference)
    }

    pub fn require(&self, reference: &str) -> Result<ServerRecord> {
        self.store.require(reference)
    }

    /// Every port the server binds under its current settings, the game port
    /// first — what a user forwards.
    pub fn ports(&self, record: &ServerRecord) -> Vec<ServerPort> {
        if Transport::of(&record.profile) == Transport::Tcp {
            return vec![ServerPort {
                port: record.game_port,
                protocol: PortProtocol::Tcp,
            }];
        }
        self.bedrock_transport(record)
            .ports(record.game_port, record.second_port)
    }

    pub fn bedrock_transport(&self, record: &ServerRecord) -> BedrockTransport {
        BedrockTransport::read(&self.data_dir(record).join(properties::PROPERTIES))
    }

    /// Register a new server: allocate its id from the name, claim its game
    /// port, create its directory, and write the (not yet ready) record.
    pub fn create(
        &self,
        name: &str,
        profile: ServerProfile,
        port: Option<u16>,
    ) -> Result<ServerRecord> {
        let transport = Transport::of(&profile);
        let record = self.store.register(name, |id| {
            Ok(ServerRecord {
                id,
                name: name.to_string(),
                created_unix: common::now_unix(),
                phase: ServerPhase::Provisioning,
                game_port: self.claim_game_port(port, transport)?,
                second_port: None,
                rcon: None,
                jvm: JavaSettings::default(),
                tasks: Vec::new(),
                profile,
            })
        })?;
        tracing::info!(id = %record.id, name, game_port = record.game_port, "server registered");
        Ok(record)
    }

    fn claim_game_port(&self, requested: Option<u16>, transport: Transport) -> Result<u16> {
        let claimed = self.claimed_ports(None);
        match requested {
            Some(port) if unavailable(&claimed, port, transport) => {
                bail!(proto::error::ErrorInfo::PortUnavailable { port })
            }
            Some(port) => Ok(port),
            None => allocate_port(transport.game_base(), &claimed, transport),
        }
    }

    /// Every port any server's record claims (game, IPv6 and rcon), except
    /// `exclude`'s own.
    fn claimed_ports(&self, exclude: Option<&str>) -> HashSet<u16> {
        self.list()
            .iter()
            .filter(|r| Some(r.id.as_str()) != exclude)
            .flat_map(|r| {
                [
                    Some(r.game_port),
                    r.second_port,
                    r.rcon.as_ref().map(|c| c.port),
                ]
            })
            .flatten()
            .collect()
    }

    /// Reconcile `server.properties` with the record's claimed ports before a
    /// spawn. The game port never moves (players depend on it) — an outside
    /// squatter is an error. The rcon port is internal and reallocates freely;
    /// a first start gains its rcon here.
    pub fn ensure_start_config(&self, id: &str) -> Result<ServerRecord> {
        self.store.mutate(id, |record| {
            let claimed = self.claimed_ports(Some(&record.id));

            let transport = Transport::of(&record.profile);
            let game_port = record.game_port;
            if unavailable(&claimed, game_port, transport) {
                bail!(proto::error::ErrorInfo::PortUnavailable { port: game_port });
            }

            let file = self.data_dir(record).join(properties::PROPERTIES);
            if transport == Transport::TcpAndUdp {
                let mut taken = claimed;
                taken.insert(game_port);
                let second = match record.second_port {
                    Some(port) if !unavailable(&taken, port, Transport::Udp) => port,
                    _ => allocate_port(BEDROCK_SECOND_PORT_BASE, &taken, Transport::Udp)?,
                };
                let netcode = BedrockTransport::read(&file);
                properties::merge_properties(&file, &netcode.property_entries(game_port, second))?;
                record.second_port = Some(second);
                return Ok(record.clone());
            }

            let rcon = match record.rcon.take() {
                Some(cfg) if !unavailable(&claimed, cfg.port, transport) => cfg,
                prior => RconConfig {
                    port: allocate_port(RCON_PORT_BASE, &claimed, transport)?,
                    password: prior.map(|c| c.password).unwrap_or_else(generate_password),
                },
            };

            properties::merge_properties(
                &file,
                &[
                    ("server-port", game_port.to_string()),
                    ("enable-rcon", "true".to_string()),
                    ("rcon.port", rcon.port.to_string()),
                    ("rcon.password", rcon.password.clone()),
                ],
            )?;
            record.rcon = Some(rcon);
            Ok(record.clone())
        })
    }

    /// Create the server's data directory and record the EULA acceptance the
    /// caller obtained from the user.
    pub fn provision(&self, record: &ServerRecord) -> Result<()> {
        let data = self.data_dir(record);
        std::fs::create_dir_all(&data)
            .with_context(|| format!("cannot create {}", data.display()))?;
        materialize::validate_filename(record.profile.runtime.filename())?;
        if Transport::of(&record.profile) == Transport::Tcp {
            std::fs::write(data.join("eula.txt"), "eula=true\n")
                .context("cannot write eula.txt")?;
        }
        tracing::info!(id = %record.id, "server provisioned");
        Ok(())
    }

    /// Seed the live file with the keys the shipped schema has and it lacks.
    pub(crate) fn seed_from_schema(&self, record: &ServerRecord) {
        let schema = properties::read_properties(&self.schema_path(record));
        let live = self.data_dir(record).join(properties::PROPERTIES);
        if let Err(e) = properties::seed_properties(&live, &schema) {
            tracing::warn!(id = %record.id, error = format!("{e:#}"), "cannot seed server.properties");
        }
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
        match self
            .generate_schema(record, java, processes, on_progress)
            .await
        {
            Ok(schema) if schema.is_empty() => {
                tracing::warn!(id = %record.id, "the schema run wrote no server.properties");
            }
            Ok(schema) => {
                let keys = schema.len();
                if let Err(e) = properties::seed_properties(
                    &self.data_dir(record).join(properties::PROPERTIES),
                    &schema,
                ) {
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
        let pristine = scratch.join(properties::PROPERTIES);
        let schema = properties::read_properties(&pristine);
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
        let ServerRuntime::Java { jar, .. } = &record.profile.runtime else {
            bail!("only a Java server has a properties schema to derive");
        };
        let plan =
            launch::server_schema_plan(jar, java, &self.data_dir(record), scratch, &record.jvm);
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
        self.store.mutate(id, |record| {
            materialize::validate_filename(profile.runtime.filename())?;
            let previous_jar = record.profile.runtime.filename().to_string();
            record.profile = profile;
            record.phase = ServerPhase::Updating;
            Ok((record.clone(), previous_jar))
        })
    }

    pub fn finish_update(&self, record: &ServerRecord, previous_jar: &str) -> Result<ServerRecord> {
        if previous_jar != record.profile.runtime.filename() {
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
        self.store.mutate(id, |record| {
            record.phase = phase;
            Ok(record.clone())
        })
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
    /// new slug. The id is stable, so ports, rcon, the process, and JVM
    /// settings are untouched. The caller guarantees it is stopped and not busy.
    pub fn rename(&self, reference: &str, new_name: &str) -> Result<ServerRecord> {
        self.store.rename(reference, new_name)
    }

    /// Store a scheduled task: a new one when its id is empty, else in place
    /// of the one it names. A new task, or one whose schedule moved, counts
    /// from `now` — otherwise an interval would fire at once and a daily time
    /// already past would too.
    pub fn save_task(&self, id: &str, task: ServerTask, now: i64) -> Result<ServerTask> {
        self.store.mutate(id, |record| {
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
            tracing::info!(server = %record.id, task = %task.id, schedule = %task.schedule, "task saved");
            Ok(task)
        })
    }

    /// Returns false when the server has no task with that id.
    pub fn remove_task(&self, id: &str, task_id: &str) -> Result<bool> {
        self.store.mutate(id, |record| {
            let before = record.tasks.len();
            record.tasks.retain(|task| task.id != task_id);
            Ok(record.tasks.len() != before)
        })
    }

    /// Stamp a task as run, so its schedule counts from here.
    pub fn mark_task_run(&self, id: &str, task_id: &str, now: i64) -> Result<()> {
        self.store.mutate(id, |record| {
            if let Some(task) = record.tasks.iter_mut().find(|task| task.id == task_id) {
                task.last_run_unix = now;
            }
            Ok(())
        })
    }

    /// Delete a server's directory (jar, world and all). Returns false when no
    /// server matches.
    pub fn remove(&self, reference: &str) -> Result<bool> {
        self.store.remove(reference)
    }

    /// `java` is the resolved runtime a Java server launches on; a native
    /// server has none and ignores it.
    pub fn launch_plan(
        &self,
        record: &ServerRecord,
        java: Option<&Path>,
        jvm: &JavaSettings,
    ) -> Result<LaunchPlan> {
        let dir = self.data_dir(record);
        match &record.profile.runtime {
            ServerRuntime::Java { jar, launch, .. } => {
                let java = java.context("a Java server needs a resolved Java runtime")?;
                Ok(launch::server_plan(jar, launch, java, &dir, jvm))
            }
            ServerRuntime::Native {
                executable, env, ..
            } => Ok(launch::native_plan(executable, env, &dir)),
        }
    }

    fn require_java_key(record: &ServerRecord, key: &str) -> Result<()> {
        if record.jvm.get(key).is_some() && record.profile.runtime.java_major().is_none() {
            bail!(proto::error::ErrorInfo::ConfigKeyNotApplicable {
                key: key.to_string()
            });
        }
        Ok(())
    }

    /// Read one setting: a reserved JVM key from the record, or any other key
    /// from `server.properties`. `Ok(None)` means the key is not set.
    pub fn config_get(&self, id: &str, key: &str) -> Result<Option<String>> {
        let record = self.store.require(id)?;
        Self::require_java_key(&record, key)?;
        if let Some(value) = record.jvm.get(key) {
            return Ok(value);
        }
        Ok(properties::read_property(
            &self.data_dir(&record).join(properties::PROPERTIES),
            key,
        ))
    }

    /// Write one setting: a reserved JVM key onto the record, or a
    /// `server.properties` key through to the file. A property key must exist in
    /// the **schema** the server itself generated for its version — not in the
    /// live file, which also carries keys no current version knows — so a typo
    /// cannot silently drift the file. A server with no derived schema accepts
    /// any unmanaged key rather than rejecting every one. The hestia-managed
    /// keys are rejected. A property value must satisfy the curated rule for its
    /// key; a key the table does not know takes any string. An empty value
    /// clears a JVM key. Settings take effect on the next start.
    pub fn config_set(&self, id: &str, key: &str, value: &str) -> Result<()> {
        self.store.mutate(id, |record| {
            Self::require_java_key(record, key)?;
            if record.jvm.set(key, value)? {
                return Ok(());
            }
            if properties::MANAGED.contains(&key) {
                bail!(
                    "'{key}' is managed by hestia (the game port is fixed at create with -p; \
                     rcon is configured automatically)"
                );
            }
            let file = self.data_dir(record).join(properties::PROPERTIES);
            let schema = self.schema_path(record);
            if schema.is_file() {
                if properties::read_property(&schema, key).is_none() {
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
            properties::validate(record.profile.runtime.edition(), key, value)
                .map_err(invalid(key))?;
            if record.profile.runtime.edition() == Edition::Java
                && properties::HARDCORE_KEYS.contains(&key)
            {
                properties::check_hardcore_invariant(&file, key, value)?;
            }
            properties::merge_properties(&file, &[(key, value.to_string())])
        })
    }

    /// Every rule a server's settings are checked against.
    pub fn config_rules(edition: Edition) -> Vec<KeyRule> {
        let mut rules = match edition {
            Edition::Java => JavaSettings::rules(),
            Edition::Bedrock => Vec::new(),
        };
        rules.extend(properties::rules(edition).iter().cloned());
        rules
    }

    /// The reserved JVM settings (always shown) followed by every current
    /// `server.properties` entry.
    pub fn config_list(&self, id: &str) -> Result<Vec<(String, String)>> {
        let record = self.store.require(id)?;
        let mut entries = match record.profile.runtime.java_major() {
            Some(_) => record.jvm.entries(),
            None => Vec::new(),
        };
        entries.extend(properties::read_properties(
            &self.data_dir(&record).join(properties::PROPERTIES),
        ));
        Ok(entries)
    }

    /// Every `server.properties` key this server knows: the ones its version's
    /// schema defines, plus any the live file carries beyond it, each with the
    /// value in effect.
    pub fn properties(&self, id: &str) -> Result<Vec<ServerProperty>> {
        let record = self.store.require(id)?;
        Ok(properties::describe_properties(
            &properties::read_properties(&self.data_dir(&record).join(properties::PROPERTIES)),
            &properties::read_properties(&self.schema_path(&record)),
        ))
    }
}

/// What a server's listeners speak: a Java server one TCP port, a Bedrock
/// server's game port both (TCP under NetherNet, UDP under RakNet).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    Tcp,
    Udp,
    TcpAndUdp,
}

impl Transport {
    fn of(profile: &ServerProfile) -> Self {
        match profile.runtime {
            ServerRuntime::Java { .. } => Transport::Tcp,
            ServerRuntime::Native { .. } => Transport::TcpAndUdp,
        }
    }

    fn game_base(self) -> u16 {
        match self {
            Transport::Tcp => GAME_PORT_BASE,
            Transport::Udp | Transport::TcpAndUdp => BEDROCK_PORT_BASE,
        }
    }
}

/// The `transport` a Bedrock server's `server.properties` selects. It decides
/// which ports the game binds, so it is re-read at every start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BedrockTransport {
    Nethernet,
    Raknet,
}

impl BedrockTransport {
    pub fn read(properties_file: &Path) -> Self {
        match properties::read_property(properties_file, "transport").as_deref() {
            Some("raknet") => Self::Raknet,
            _ => Self::Nethernet,
        }
    }

    /// NetherNet's dual-stack socket listens on TCP `server-port` and takes its
    /// client UDP port from `server-udp-ports`; `server-portv6` is ignored
    /// there but kept coherent. RakNet binds UDP `server-port` and `-portv6`.
    fn property_entries(self, game: u16, second: u16) -> Vec<(&'static str, String)> {
        let mut entries = vec![
            ("server-port", game.to_string()),
            ("server-portv6", second.to_string()),
        ];
        if self == Self::Nethernet {
            entries.push(("server-udp-ports", second.to_string()));
        }
        entries
    }

    fn ports(self, game: u16, second: Option<u16>) -> Vec<ServerPort> {
        let game_protocol = match self {
            Self::Nethernet => PortProtocol::Tcp,
            Self::Raknet => PortProtocol::Udp,
        };
        let game = ServerPort {
            port: game,
            protocol: game_protocol,
        };
        let second = second.map(|port| ServerPort {
            port,
            protocol: PortProtocol::Udp,
        });
        std::iter::once(game).chain(second).collect()
    }
}

// Listeners bind all interfaces, so probe the same way.
fn can_bind(port: u16, transport: Transport) -> bool {
    match transport {
        Transport::Tcp => std::net::TcpListener::bind(("0.0.0.0", port)).is_ok(),
        Transport::Udp => std::net::UdpSocket::bind(("0.0.0.0", port)).is_ok(),
        Transport::TcpAndUdp => can_bind(port, Transport::Tcp) && can_bind(port, Transport::Udp),
    }
}

fn unavailable(claimed: &HashSet<u16>, port: u16, transport: Transport) -> bool {
    claimed.contains(&port) || !can_bind(port, transport)
}

fn allocate_port(base: u16, claimed: &HashSet<u16>, transport: Transport) -> Result<u16> {
    (base..base.saturating_add(PORT_SPAN))
        .find(|port| !unavailable(claimed, *port, transport))
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

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[test]
    fn nethernet_maps_the_second_port_to_udp_ports() {
        let entries = BedrockTransport::Nethernet.property_entries(19132, 19133);
        assert_eq!(
            entries,
            vec![
                ("server-port", "19132".to_string()),
                ("server-portv6", "19133".to_string()),
                ("server-udp-ports", "19133".to_string()),
            ]
        );
    }

    #[test]
    fn raknet_binds_two_udp_ports() {
        let entries = BedrockTransport::Raknet.property_entries(19132, 19133);
        assert_eq!(entries.len(), 2);
        let ports = BedrockTransport::Raknet.ports(19132, Some(19133));
        assert!(ports.iter().all(|p| p.protocol == PortProtocol::Udp));
        let ports = BedrockTransport::Nethernet.ports(19132, Some(19133));
        assert_eq!(ports[0].protocol, PortProtocol::Tcp);
        assert_eq!(ports[1].protocol, PortProtocol::Udp);
    }

    #[test]
    fn transport_defaults_to_nethernet() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("server.properties");
        assert_eq!(BedrockTransport::read(&file), BedrockTransport::Nethernet);
        std::fs::write(&file, "transport=raknet\n").unwrap();
        assert_eq!(BedrockTransport::read(&file), BedrockTransport::Raknet);
    }

    #[test]
    fn a_bedrock_game_port_needs_tcp_and_udp_free() {
        let tcp = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = tcp.local_addr().unwrap().port();
        assert!(unavailable(&HashSet::new(), port, Transport::TcpAndUdp));
        let claimed = HashSet::from([port.wrapping_add(1)]);
        assert!(unavailable(&claimed, port.wrapping_add(1), Transport::Udp));
    }
}
