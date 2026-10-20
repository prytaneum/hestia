//! The daemon-internal aggregate root; front-ends reach it only over IPC. Adding
//! a domain = a module, a member, and a getter here.
//!
//! The aggregate owns the subsystems and nothing else. The cross-subsystem flows
//! composed over them — provisioning, launching, backups, content — live in
//! `flows`, one module apiece, each an `impl Engine` block.

mod flows;

pub use flows::{
    EntryRef, ExportOutcome, ImportOutcome, LaunchRequest, ModpackOutcome, ServerListWrite,
    VersionChange,
};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use proto::minecraft::ConfigEntry;

use crate::accounts::Accounts;
use crate::cache::Cache;
use crate::config::Config;
use crate::console::Consoles;
use crate::content::Content;
use crate::instances::Instances;
use crate::java::Java;
use crate::minecraft::Minecraft;
use crate::plugins::Plugins;
use crate::process::ProcessSupervisor;
use crate::servers::Servers;
use crate::skins::Skins;
use crate::sync::Sync;
use crate::update::Update;

/// Everything a server create needs from the caller — the engine-side input to
/// `provision_server` (EULA assertion and job ids are daemon concerns).
#[derive(Debug, Clone, Default)]
pub struct ServerCreateSpec {
    pub name: String,
    pub flavor: String,
    pub version: String,
    pub loader_version: Option<String>,
    pub port: Option<u16>,
    pub config: Vec<ConfigEntry>,
    /// The caller confirms the user accepted the Minecraft EULA. A field rather
    /// than a doc comment, so the obligation is one a caller must fill in.
    pub eula: bool,
}

/// Everything a server update needs from the caller — the engine-side input to
/// `update_server` (the downgrade confirmation is obtained by the front-end).
#[derive(Debug, Clone, Default)]
pub struct ServerUpdateSpec {
    pub server: String,
    pub version: String,
    pub loader_version: Option<String>,
    pub allow_downgrade: bool,
}

/// Where the catalogue cache lives — under `meta/`, with the rest of the files
/// a reclaim is free to delete.
fn catalogue_dir(home: &Path) -> PathBuf {
    flows::meta_dir(home).join("catalogue")
}

pub struct Engine {
    data_home: PathBuf,
    config: Config,
    cache: Cache,
    java: Java,
    accounts: Accounts,
    minecraft: Minecraft,
    content: Content,
    servers: Servers,
    instances: Instances,
    plugins: Plugins,
    skins: Skins,
    sync: Sync,
    update: Update,
    processes: Arc<ProcessSupervisor>,
    consoles: Consoles,
}

impl Engine {
    pub fn new(override_home: Option<&Path>) -> Self {
        Engine::over(override_home, Minecraft::new(), Content::new())
    }

    /// Build over a given provider registry. The seam a test crosses to drive
    /// the cross-subsystem flows — provisioning, launching, content, packs —
    /// against fixtures rather than upstream.
    pub fn over(override_home: Option<&Path>, minecraft: Minecraft, content: Content) -> Self {
        let data_home = common::paths::data_home(override_home);
        tracing::info!(home = %data_home.display(), "engine data home");
        let config = Config::new(common::paths::config_path(Some(&data_home)));
        let cache = Cache::new(data_home.join("cache"));
        let java = Java::new(data_home.join("java"));
        let accounts = Accounts::new(data_home.join("accounts.json"));
        let servers = Servers::new(data_home.join("servers"));
        let instances = Instances::new(data_home.join("instances"));
        let plugins = Plugins::new(data_home.join("plugins"));
        let skins = Skins::new(data_home.join("skins"));
        let sync = Sync::new(data_home.join("shared"));
        crate::net::store::set_root(catalogue_dir(&data_home));
        let update = Update::new(data_home.join("updates"));
        let processes = Arc::new(ProcessSupervisor::new(data_home.join("processes")));
        let engine = Engine {
            data_home,
            config,
            cache,
            java,
            accounts,
            minecraft,
            content,
            servers,
            instances,
            plugins,
            skins,
            sync,
            update,
            processes,
            consoles: Consoles::default(),
        };
        engine.apply_settings();
        engine
    }

    pub fn data_home(&self) -> &Path {
        &self.data_home
    }

    /// Everything a restart invalidates. No job survives a daemon stop, so at
    /// startup any half-finished state on disk belongs to a job that will never
    /// come back to finish it: temp artifacts have no owner, and a record still
    /// mid-create is an entry that never came into existence. Call once the data
    /// home is settled, before serving.
    pub fn recover(&self) {
        self.reclaim_temp();
        self.servers.reconcile();
    }

    /// Reclaim every abandoned temp artifact in the data home. A `.part` or
    /// `.staging` artifact is only valid while its job holds the matching
    /// in-flight claim, and no claim survives a restart — so at startup each of
    /// these belongs to a job that will never finish.
    fn reclaim_temp(&self) {
        let mut freed = self.java.reclaim();
        freed += self.plugins.reclaim();
        freed += crate::reclaim::contents(&self.pack_staging());
        freed += crate::reclaim::suffixed(&self.plugin_cache_dir(), common::atomic::SUFFIX);
        for record in self.servers.list() {
            let dir = self.servers.server_dir(&record);
            freed += crate::backup::reclaim(&dir);
            freed += crate::backup::recover(&dir, &self.servers.data_dir(&record));
        }
        if !freed.is_empty() {
            tracing::info!(
                entries = freed.entries,
                bytes = freed.bytes,
                "reclaimed abandoned temp artifacts"
            );
        }
    }

    /// Where compiled plugin components are cached.
    pub fn plugin_cache_dir(&self) -> PathBuf {
        flows::meta_dir(&self.data_home).join("plugins")
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Set a value, then re-apply the settings a subsystem holds its own copy
    /// of, so a change lands on the running daemon.
    pub fn set_config(
        &self,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), crate::config::ConfigError> {
        self.config.set(key, value)?;
        self.apply_settings();
        Ok(())
    }

    fn apply_settings(&self) {
        let settings = self.config.settings();
        self.content.configure(&settings.content);
        crate::net::network().set_offline_mode(settings.network.offline);
    }

    /// Reachability, as the daemon reads and publishes it.
    pub fn network(&self) -> &'static crate::net::Network {
        crate::net::network()
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    pub fn java(&self) -> &Java {
        &self.java
    }

    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }

    pub fn minecraft(&self) -> &Minecraft {
        &self.minecraft
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn servers(&self) -> &Servers {
        &self.servers
    }

    pub fn instances(&self) -> &Instances {
        &self.instances
    }

    pub fn plugins(&self) -> &Plugins {
        &self.plugins
    }

    pub fn skins(&self) -> &Skins {
        &self.skins
    }

    pub fn sync(&self) -> &Sync {
        &self.sync
    }

    pub fn processes(&self) -> &Arc<ProcessSupervisor> {
        &self.processes
    }

    pub fn update(&self) -> &Update {
        &self.update
    }
}
