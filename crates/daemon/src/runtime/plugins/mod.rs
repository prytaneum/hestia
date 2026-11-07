//! The daemon's side of plugins: which installed plugins are loaded, in what
//! state, and the transitions between states. The sandbox itself is the
//! engine's [`PluginRuntime`]; the channels a plugin may call are lent to it by
//! [`RouterHost`].

mod dev;
mod events;
mod host;
mod package;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use engine::{
    connect_flavors, connect_formats, connect_java, connect_sources, is_official_signer,
    register_flavors, register_formats, register_java, register_sources, CallChain, Cancel, Engine,
    HostCalls, Listings, LoadedPlugin, PluginContentProvider, PluginFlavorProvider, PluginFormat,
    PluginJavaProvider, PluginRecord, PluginRuntime, PluginStats, Roster, SettingsError,
    EVENT_TIMEOUT,
};
use proto::error::ErrorInfo;
use proto::plugin::{
    CommandContribution, Manifest, PageContribution, PluginInfo, PluginInspection,
    PluginInstallPhase, PluginPage, PluginState, PluginStateEvent, PluginTheme, SettingValue,
};
use tokio::task::{spawn_blocking, JoinSet};

use super::EventHub;
use crate::runtime::router::ServiceResult;
use events::Delivery;

pub use host::RouterHost;
pub use package::Package;

const INIT_TIMEOUT: Duration = Duration::from_secs(10);
const SAFE_MODE_NOTE: &str = "not loaded: the daemon is in safe mode";

struct Slot {
    state: PluginState,
    error: Option<String>,
    loaded: Option<Arc<LoadedPlugin>>,
    _delivery: Option<Delivery>,
}

/// What an enabled plugin adds to the engine's registries.
#[derive(Default)]
struct Contributions {
    sources: Vec<PluginContentProvider>,
    flavors: Vec<PluginFlavorProvider>,
    formats: Vec<PluginFormat>,
    java: Vec<PluginJavaProvider>,
}

enum Activation {
    /// `None` for a package with no component, such as a theme-only one.
    Ready(Option<Arc<LoadedPlugin>>, Contributions),
    NeedsApproval,
}

pub struct Plugins {
    engine: Arc<Engine>,
    hub: Arc<EventHub>,
    sandbox: OnceLock<Arc<PluginRuntime>>,
    host: OnceLock<Arc<dyn HostCalls>>,
    slots: Mutex<HashMap<String, Slot>>,
    shutting_down: AtomicBool,
    safe_mode: bool,
}

impl Plugins {
    pub fn new(engine: Arc<Engine>, hub: Arc<EventHub>, safe_mode: bool) -> Self {
        Plugins {
            engine,
            hub,
            safe_mode,
            sandbox: OnceLock::new(),
            host: OnceLock::new(),
            slots: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
        }
    }

    /// No third-party plugin loads this run.
    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    /// Creates the sandbox that plugins run in; `host` is what they may call.
    pub fn attach(&self, host: Arc<dyn HostCalls>) -> anyhow::Result<()> {
        let sandbox = PluginRuntime::new(
            host.clone(),
            self.engine.plugin_cache_dir(),
            self.engine.processes().clone(),
            self.engine.presence().clone(),
        )?;
        self.host
            .set(host)
            .map_err(|_| anyhow::anyhow!("the plugin runtime is already attached"))?;
        self.sandbox
            .set(Arc::new(sandbox))
            .map_err(|_| anyhow::anyhow!("the plugin runtime is already attached"))
    }

    /// Loads every enabled plugin concurrently. One that fails is marked failed
    /// and the others carry on.
    pub async fn load_enabled(self: &Arc<Self>) {
        let mut loads = JoinSet::new();
        for record in self
            .engine
            .plugins()
            .list()
            .into_iter()
            .filter(|record| record.enabled)
        {
            let plugins = self.clone();
            loads.spawn(async move { plugins.activate(record).await });
        }
        while let Some(joined) = loads.join_next().await {
            if let Err(error) = joined {
                tracing::error!(%error, "plugin load task failed");
            }
        }
    }

    /// Stops serving plugins: nothing can start a call on them afterwards, and
    /// calls already running end at their own deadlines.
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.slots.lock().unwrap().clear();
    }

    pub async fn list(&self) -> Vec<PluginInfo> {
        let listings = self.listings().await;
        let mut infos = Vec::new();
        for record in self.engine.plugins().list() {
            infos.push(self.info_with(record, &listings).await);
        }
        infos
    }

    pub async fn info(&self, id: &str) -> ServiceResult<PluginInfo> {
        let mut info = self.info_of(self.record(id)?).await;
        let engine = self.engine.clone();
        let id = id.to_string();
        info.data_bytes = spawn_blocking(move || {
            engine
                .plugins()
                .data_dir(&id)
                .map_or(0, |dir| engine::dir_size(&dir))
        })
        .await
        .ok();
        Ok(info)
    }

    pub async fn enable(&self, id: &str) -> ServiceResult<PluginInfo> {
        let mut record = self.record(id)?;
        self.engine
            .plugins()
            .set_enabled(id, true)
            .map_err(internal)?;
        record.enabled = true;
        self.activate(record.clone()).await;
        Ok(self.info_of(record).await)
    }

    /// Calls already running keep their own stores and finish on them.
    pub async fn disable(&self, id: &str) -> ServiceResult<PluginInfo> {
        let mut record = self.record(id)?;
        self.engine
            .plugins()
            .set_enabled(id, false)
            .map_err(internal)?;
        record.enabled = false;
        self.settle(
            id,
            PluginState::Disabled,
            None,
            None,
            Contributions::default(),
        );
        self.engine.processes().stop_owned(&engine::owner_of(id));
        Ok(self.info_of(record).await)
    }

    /// A downloaded package is kept for the install only if it is a plugin.
    pub async fn inspect(&self, package: &Package) -> ServiceResult<PluginInspection> {
        let inspected = self.engine.plugins().inspect(package.path());
        if inspected.is_err() {
            self.discard(package);
        }
        let inspection = inspected.map_err(super::engine_error)?;
        let installed_grant = inspection
            .installed
            .as_ref()
            .map(|record| record.grant.clone())
            .unwrap_or_default();
        let replaces = match inspection.installed {
            Some(record) => Some(self.info_of(record).await),
            None => None,
        };
        let widened = inspection.grant.widening_over(&installed_grant);
        Ok(PluginInspection {
            unrestricted: widened.unrestricted(),
            widened,
            manifest: inspection.manifest,
            grant: inspection.grant,
            grant_digest: inspection.grant_digest,
            replaces,
            program_paths: inspection
                .program_paths
                .into_iter()
                .map(|(id, path)| (id, path.display().to_string()))
                .collect(),
            staged: package.token().map(str::to_string),
        })
    }

    /// Blocking: compiles the component on the calling thread.
    pub fn install(
        &self,
        package: &Package,
        approve: &str,
        cancel: &Cancel,
        on_phase: &dyn Fn(PluginInstallPhase),
    ) -> anyhow::Result<PluginRecord> {
        let sandbox = self
            .sandbox
            .get()
            .cloned()
            .context("the plugin runtime is not running")?;
        let validate = |wasm: &[u8]| sandbox.check(wasm).map_err(Into::into);
        let record = match package {
            Package::Local(path) => self
                .engine
                .plugins()
                .install(path, approve, cancel, on_phase, validate)?,
            Package::Dev(path) => self.engine.plugins().install_dev(path, approve, validate)?,
            Package::Downloaded { staged, .. } => self
                .engine
                .plugins()
                .install_downloaded(staged, approve, cancel, on_phase, validate)?,
        };
        self.discard(package);
        Ok(record)
    }

    fn discard(&self, package: &Package) {
        if let Some(token) = package.token() {
            self.engine.plugin_downloads().discard(token);
        }
    }

    pub async fn installed(&self, record: PluginRecord) -> PluginInfo {
        if record.enabled {
            self.activate(record.clone()).await;
        }
        self.info_of(record).await
    }

    /// The command `name` of `id`'s manifest and the plugin that runs it.
    pub async fn command(
        &self,
        id: &str,
        name: &str,
    ) -> ServiceResult<(CommandContribution, Arc<LoadedPlugin>)> {
        let manifest = self.manifest(id).await?;
        let command = engine::find_command(&manifest, name)?.clone();
        Ok((command, self.running(id)?))
    }

    /// The plugin's declared settings; a secret reports only whether it is set.
    pub async fn settings(&self, id: &str) -> ServiceResult<Vec<SettingValue>> {
        self.record(id)?;
        let engine = self.engine.clone();
        let plugin = id.to_string();
        spawn_blocking(move || engine.plugins().settings(&plugin).map(|s| s.list()))
            .await
            .map_err(|error| internal(error.into()))?
            .map_err(internal)
    }

    /// Stores a setting, then tells the plugin if it is running. A guest that
    /// fails to take the news does not undo the change.
    pub async fn set_setting(
        &self,
        id: &str,
        key: &str,
        value: serde_json::Value,
        chain: &CallChain,
    ) -> ServiceResult<()> {
        self.record(id)?;
        let engine = self.engine.clone();
        let (plugin, name) = (id.to_string(), key.to_string());
        spawn_blocking(move || engine.plugins().set_setting(&plugin, &name, value))
            .await
            .map_err(|error| internal(error.into()))?
            .map_err(|error| match error {
                SettingsError::Store(error) => internal(error),
                other => ErrorInfo::PluginSettingInvalid {
                    plugin: id.to_string(),
                    key: key.to_string(),
                    detail: other.to_string(),
                },
            })?;
        if let Ok(loaded) = self.running(id) {
            if let Err(error) = loaded.on_settings_changed(chain, key, EVENT_TIMEOUT).await {
                tracing::warn!(plugin = %id, %key, %error, "plugin did not take a settings change");
            }
        }
        Ok(())
    }

    /// The page `name` of `id`'s manifest and the plugin that draws it.
    pub async fn page(
        &self,
        id: &str,
        name: &str,
    ) -> ServiceResult<(PageContribution, Arc<LoadedPlugin>)> {
        let manifest = self.manifest(id).await?;
        let page = manifest
            .contributes
            .pages
            .iter()
            .find(|page| page.id == name)
            .ok_or_else(|| ErrorInfo::PluginPageInvalid {
                plugin: id.to_string(),
                page: name.to_string(),
                detail: "no such page".into(),
            })?
            .clone();
        Ok((page, self.running(id)?))
    }

    /// Every page of the plugins that are running.
    pub async fn pages(&self) -> Vec<PluginPage> {
        let mut pages = Vec::new();
        for info in self.list().await {
            if info.state != PluginState::Enabled || info.manifest.wasm.is_none() {
                continue;
            }
            let plugin = info.manifest.id;
            pages.extend(
                info.manifest
                    .contributes
                    .pages
                    .into_iter()
                    .map(|page| PluginPage {
                        plugin: plugin.clone(),
                        page,
                    }),
            );
        }
        pages
    }

    /// Every theme of the plugins that are enabled, running or not.
    pub async fn themes(&self) -> Vec<PluginTheme> {
        self.list()
            .await
            .into_iter()
            .filter(|info| info.state == PluginState::Enabled)
            .flat_map(|info| {
                let plugin = info.manifest.id;
                info.manifest
                    .contributes
                    .themes
                    .into_iter()
                    .map(move |theme| PluginTheme {
                        plugin: plugin.clone(),
                        id: theme.id,
                        name: theme.name,
                        appearance: theme.appearance,
                        file: theme.file,
                    })
            })
            .collect()
    }

    async fn manifest(&self, id: &str) -> ServiceResult<Manifest> {
        self.record(id)?;
        let engine = self.engine.clone();
        let plugin = id.to_string();
        spawn_blocking(move || engine.plugins().read_manifest(&plugin))
            .await
            .map_err(|error| internal(error.into()))?
            .map_err(internal)
    }

    fn running(&self, id: &str) -> ServiceResult<Arc<LoadedPlugin>> {
        self.slots
            .lock()
            .unwrap()
            .get(id)
            .filter(|slot| slot.state == PluginState::Enabled)
            .and_then(|slot| slot.loaded.clone())
            .filter(|loaded| !loaded.is_disabled())
            .ok_or_else(|| ErrorInfo::PluginUnavailable { id: id.to_string() })
    }

    /// Whether `id` is live, so its package may be served: a package changed on
    /// disk settles as needs-approval, and its files must not reach the webview.
    pub fn is_live(&self, id: &str) -> bool {
        self.slots
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|slot| slot.state == PluginState::Enabled)
    }

    /// The programs the plugin started are stopped first: a running agent would
    /// outlive the plugin that owns it, and on Windows hold its files open.
    pub async fn remove(&self, id: &str) -> ServiceResult<()> {
        self.record(id)?;
        let processes = self.engine.processes();
        processes
            .stop_and_wait(&processes.stop_owned(&engine::owner_of(id)))
            .await;
        self.engine.plugins().remove(id).map_err(internal)?;
        self.slots.lock().unwrap().remove(id);
        self.engine.content().unregister(id);
        self.engine.minecraft().unregister(id);
        Ok(())
    }

    fn record(&self, id: &str) -> ServiceResult<PluginRecord> {
        self.engine
            .plugins()
            .get(id)
            .ok_or_else(|| ErrorInfo::PluginNotFound { id: id.to_string() })
    }

    fn wanted(&self, id: &str) -> bool {
        !self.shutting_down.load(Ordering::SeqCst)
            && self
                .engine
                .plugins()
                .get(id)
                .is_some_and(|record| record.enabled)
    }

    /// Blocking reads of the cached indexes, off the async threads.
    async fn listings(&self) -> Listings {
        let engine = self.engine.clone();
        spawn_blocking(move || engine.listings())
            .await
            .unwrap_or_default()
    }

    async fn info_of(&self, record: PluginRecord) -> PluginInfo {
        let listings = if record.signer.is_some() {
            self.listings().await
        } else {
            Listings::default()
        };
        self.info_with(record, &listings).await
    }

    async fn info_with(&self, record: PluginRecord, listings: &Listings) -> PluginInfo {
        let obsolete = listings.obsolete(&record);
        let engine = self.engine.clone();
        let id = record.id.clone();
        let manifest = spawn_blocking(move || engine.plugins().read_manifest(&id))
            .await
            .unwrap_or_else(|error| Err(error.into()));
        let (state, last_error, stats) = {
            let slots = self.slots.lock().unwrap();
            match slots.get(&record.id) {
                Some(slot)
                    if slot
                        .loaded
                        .as_ref()
                        .is_some_and(|loaded| loaded.is_disabled()) =>
                {
                    (
                        PluginState::Failed,
                        Some("disabled after repeated traps".to_string()),
                        stats_of(slot),
                    )
                }
                Some(slot) => (slot.state, slot.error.clone(), stats_of(slot)),
                None if record.enabled => (PluginState::Enabled, None, PluginStats::default()),
                None => (PluginState::Disabled, None, PluginStats::default()),
            }
        };
        let (manifest, state, last_error) = match manifest {
            Ok(manifest) => (manifest, state, last_error),
            Err(error) => (
                Manifest {
                    id: record.id.clone(),
                    ..Manifest::default()
                },
                PluginState::Failed,
                Some(format!("{error:#}")),
            ),
        };
        PluginInfo {
            manifest,
            state,
            source: record.source,
            url: record.url,
            official: is_official_signer(record.signer.as_deref()),
            signer: record.signer,
            repository: record.repository,
            obsolete,
            sha256: record.wasm_sha256.unwrap_or_default(),
            unrestricted: record.grant.unrestricted(),
            grant: record.grant,
            last_error,
            calls: stats.calls,
            traps: stats.traps,
            timeouts: stats.timeouts,
            data_bytes: None,
        }
    }

    async fn activate(&self, record: PluginRecord) {
        let id = record.id.clone();
        if self.safe_mode {
            tracing::info!(plugin = %id, "plugin not loaded in safe mode");
            self.settle(
                &id,
                PluginState::Disabled,
                Some(SAFE_MODE_NOTE.into()),
                None,
                Contributions::default(),
            );
            return;
        }
        match self.try_activate(&record).await {
            Ok(Activation::Ready(loaded, contributions)) => {
                self.settle(&id, PluginState::Enabled, None, loaded, contributions)
            }
            Ok(Activation::NeedsApproval) => {
                tracing::warn!(plugin = %id, "plugin asks for more than was approved");
                self.settle(
                    &id,
                    PluginState::NeedsApproval,
                    None,
                    None,
                    Contributions::default(),
                )
            }
            Err(error) => {
                let message = format!("{error:#}");
                tracing::warn!(plugin = %id, error = %message, "plugin failed to load");
                self.settle(
                    &id,
                    PluginState::Failed,
                    Some(message),
                    None,
                    Contributions::default(),
                )
            }
        }
    }

    async fn try_activate(&self, record: &PluginRecord) -> anyhow::Result<Activation> {
        let engine = self.engine.clone();
        let id = record.id.clone();
        let package = spawn_blocking(move || engine.plugins().read_package(&id)).await??;
        if package.manifest.grant().digest() != record.grant_digest {
            return Ok(Activation::NeedsApproval);
        }
        let Some(wasm) = package.wasm else {
            return Ok(Activation::Ready(None, Contributions::default()));
        };
        let sandbox = self
            .sandbox
            .get()
            .cloned()
            .context("the plugin runtime is not running")?;
        let data_dir = self.engine.plugins().data_dir(&record.id)?;
        let record = record.clone();
        let engine = self.engine.clone();
        let loaded = spawn_blocking(move || {
            let settings = engine.plugins().settings(&record.id)?;
            anyhow::Ok(sandbox.load(&record, &wasm, data_dir, settings)?)
        })
        .await??;
        loaded.init(&CallChain::default(), INIT_TIMEOUT).await?;
        let loaded = Arc::new(loaded);
        let contributions = Contributions {
            sources: connect_sources(&loaded, &package.manifest).await,
            flavors: connect_flavors(&loaded, &package.manifest).await,
            formats: connect_formats(&loaded, &package.manifest).await,
            java: connect_java(&loaded, &package.manifest),
        };
        Ok(Activation::Ready(Some(loaded), contributions))
    }

    /// Loading takes seconds, so a plugin can be disabled, removed or the
    /// daemon stopped meanwhile; checked under the slots lock, a result for
    /// one that is no longer wanted is dropped instead of going live.
    fn settle(
        &self,
        id: &str,
        state: PluginState,
        error: Option<String>,
        loaded: Option<Arc<LoadedPlugin>>,
        contributions: Contributions,
    ) {
        let delivery = match (state, &loaded, self.host.get()) {
            (PluginState::Enabled, Some(loaded), Some(host)) => {
                Delivery::start(self.hub.clone(), host.clone(), loaded.clone())
            }
            _ => None,
        };
        {
            let mut slots = self.slots.lock().unwrap();
            if state != PluginState::Disabled && !self.wanted(id) {
                tracing::info!(plugin = %id, "plugin was disabled or removed while loading");
                return;
            }
            self.engine.content().unregister(id);
            self.engine.minecraft().unregister(id);
            self.engine.unregister_formats(id);
            self.engine.java().unregister(id);
            if state == PluginState::Enabled {
                let official = self
                    .engine
                    .plugins()
                    .get(id)
                    .is_some_and(|record| is_official_signer(record.signer.as_deref()));
                register_sources(self.engine.content(), id, official, contributions.sources);
                register_flavors(self.engine.minecraft(), id, official, contributions.flavors);
                register_formats(&self.engine, id, official, contributions.formats);
                register_java(self.engine.java(), id, official, contributions.java);
            }
            slots.insert(
                id.to_string(),
                Slot {
                    state,
                    error: error.clone(),
                    loaded,
                    _delivery: delivery,
                },
            );
        }
        self.hub.publish_topic(&PluginStateEvent {
            id: id.to_string(),
            state,
            error,
        });
    }
}

impl Roster<LoadedPlugin> for Plugins {
    fn loaded(&self) -> Vec<Arc<LoadedPlugin>> {
        self.slots
            .lock()
            .unwrap()
            .values()
            .filter(|slot| slot.state == PluginState::Enabled)
            .filter_map(|slot| slot.loaded.clone())
            .collect()
    }
}

fn stats_of(slot: &Slot) -> PluginStats {
    slot.loaded
        .as_ref()
        .map(|loaded| loaded.stats())
        .unwrap_or_default()
}

fn internal(error: anyhow::Error) -> ErrorInfo {
    ErrorInfo::Internal {
        detail: format!("{error:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugins_over(home: &std::path::Path) -> (Plugins, Arc<Engine>) {
        let engine = Arc::new(Engine::new(Some(home)));
        let plugins = Plugins::new(engine.clone(), Arc::new(EventHub::default()), false);
        (plugins, engine)
    }

    fn install(engine: &Engine, enabled: bool) {
        engine
            .plugins()
            .save(&PluginRecord {
                id: "demo".into(),
                enabled,
                ..PluginRecord::default()
            })
            .expect("save record");
    }

    #[tokio::test]
    async fn a_load_that_finishes_after_disable_does_not_go_live() {
        let home = tempfile::tempdir().unwrap();
        let (plugins, engine) = plugins_over(home.path());
        install(&engine, true);
        plugins.disable("demo").await.unwrap();

        plugins.settle(
            "demo",
            PluginState::Enabled,
            None,
            None,
            Contributions::default(),
        );

        assert_eq!(
            plugins.slots.lock().unwrap()["demo"].state,
            PluginState::Disabled
        );
    }

    #[tokio::test]
    async fn only_an_enabled_slot_is_live() {
        let home = tempfile::tempdir().unwrap();
        let (plugins, engine) = plugins_over(home.path());
        install(&engine, true);
        assert!(!plugins.is_live("demo"));

        plugins.settle(
            "demo",
            PluginState::NeedsApproval,
            None,
            None,
            Contributions::default(),
        );
        assert!(!plugins.is_live("demo"));

        plugins.settle(
            "demo",
            PluginState::Enabled,
            None,
            None,
            Contributions::default(),
        );
        assert!(plugins.is_live("demo"));
    }

    #[tokio::test]
    async fn a_load_that_finishes_after_shutdown_does_not_go_live() {
        let home = tempfile::tempdir().unwrap();
        let (plugins, engine) = plugins_over(home.path());
        install(&engine, true);
        plugins.shutdown();

        plugins.settle(
            "demo",
            PluginState::Failed,
            Some("late".into()),
            None,
            Contributions::default(),
        );

        assert!(plugins.slots.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn safe_mode_loads_nothing_and_says_why() {
        let home = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(Some(home.path())));
        let plugins = Arc::new(Plugins::new(
            engine.clone(),
            Arc::new(EventHub::default()),
            true,
        ));
        install(&engine, true);

        plugins.load_enabled().await;

        let slots = plugins.slots.lock().unwrap();
        assert_eq!(slots["demo"].state, PluginState::Disabled);
        assert_eq!(slots["demo"].error.as_deref(), Some(SAFE_MODE_NOTE));
        assert!(slots["demo"].loaded.is_none());
    }
}
