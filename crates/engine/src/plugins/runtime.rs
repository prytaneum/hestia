use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use proto::plugin::{Grant, PageContext, PluginSource, DATA_MOUNT};
use proto::ui::{Action, Effect, UiError, UiNode};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::time::{timeout_at, Instant};
use wasmtime::component::{HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig, Store};
use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};
use wasmtime_wasi_http::WasiHttpCtx;

use super::cache::ComponentCache;
use super::chain::{CallChain, ChainError};
use super::http::PluginHttp;
use super::imports::bindings::{self, hestia::plugin::types::PluginError};
use super::imports::{CommandRun, Gate, HostCalls, PluginState};
use super::ipc::Connections;
use super::output::{Captured, PluginLog};
use super::process::Programs;
use super::settings::PluginSettings;
use super::store::PluginRecord;
use crate::checksum::hex;
use crate::process::ProcessSupervisor;

pub const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(2);
pub const UI_TIMEOUT: Duration = Duration::from_secs(10);
pub const PROVIDER_TIMEOUT: Duration = Duration::from_secs(30);
/// The deadline of a `plugin.run` command: it is a job the user can cancel, so
/// it may outlast an ordinary call by far.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// How long a cancelled command may keep running before its call is dropped.
pub const CANCEL_GRACE: Duration = Duration::from_secs(10);

const CANCEL_POLL: Duration = Duration::from_millis(50);
const EPOCH_TICK: Duration = Duration::from_millis(10);
const POOL_SLOTS: u32 = 16;
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
const TABLE_ELEMENTS: usize = 20_000;
const CALLS_PER_PLUGIN: usize = 4;
const TRAP_LIMIT: usize = 3;
const TRAP_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("the record has no wasm hash to check the component against")]
    MissingHash,
    #[error(
        "the component does not match its recorded hash (expected {expected}, found {actual})"
    )]
    HashMismatch { expected: String, actual: String },
    #[error("cannot load the component: {0}")]
    Component(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error("the plugin was disabled after repeated traps")]
    Disabled,
    #[error("no free slot to run the plugin")]
    Busy,
    #[error("the plugin did not answer within {0:?}")]
    Timeout(Duration),
    #[error("the command was cancelled and did not stop in time")]
    Cancelled,
    #[error("the plugin trapped: {0}")]
    Trap(String),
    #[error("cannot prepare the plugin's sandbox: {0}")]
    Setup(String),
    #[error("{0}")]
    Guest(String),
    #[error("the plugin does not implement `{0}`")]
    Unsupported(String),
    #[error("the page `{0}` returned a document outside the UI vocabulary")]
    InvalidUi(String),
}

/// Counters kept for diagnosis.
#[derive(Debug, Clone, Copy, Default)]
pub struct PluginStats {
    pub calls: u64,
    pub traps: u64,
    pub timeouts: u64,
}

/// Advances the engine's epoch from a thread of its own. A tokio task would
/// not do: a guest spinning on a current-thread runtime is exactly when the
/// tick has to arrive, and nothing else would run to deliver it.
struct Ticker {
    stopped: Arc<AtomicBool>,
}

impl Ticker {
    fn start(engine: Engine) -> std::io::Result<Self> {
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        std::thread::Builder::new()
            .name("plugin-epoch".into())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(EPOCH_TICK);
                    engine.increment_epoch();
                }
            })?;
        Ok(Ticker { stopped })
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// The one wasmtime engine of the daemon. Its pooling allocator is shared by
/// every plugin, so the pool is also the global cap on plugin memory.
pub struct PluginRuntime {
    engine: Engine,
    linker: Arc<Linker<PluginState>>,
    pool: Arc<Semaphore>,
    host: Arc<dyn HostCalls>,
    processes: Arc<ProcessSupervisor>,
    ticker: Arc<Ticker>,
    cache: ComponentCache,
}

impl PluginRuntime {
    /// `cache_dir` holds the compiled components (`meta/plugins/`); `processes`
    /// supervises what plugins spawn.
    pub fn new(
        host: Arc<dyn HostCalls>,
        cache_dir: PathBuf,
        processes: Arc<ProcessSupervisor>,
    ) -> anyhow::Result<Self> {
        let mut pooling = PoolingAllocationConfig::new();
        pooling
            .total_component_instances(POOL_SLOTS)
            .total_core_instances(POOL_SLOTS * 16)
            .total_memories(POOL_SLOTS)
            .total_tables(POOL_SLOTS * 4)
            .total_stacks(POOL_SLOTS)
            .max_core_instances_per_component(16)
            .max_memories_per_component(1)
            .max_tables_per_component(4)
            .table_elements(TABLE_ELEMENTS)
            .max_memory_size(MEMORY_LIMIT);
        let mut config = Config::new();
        config
            .epoch_interruption(true)
            .concurrency_support(false)
            .memory_init_cow(true)
            .allocation_strategy(InstanceAllocationStrategy::Pooling(pooling));
        let engine = Engine::new(&config).map_err(|error| anyhow::anyhow!("{error:#}"))?;

        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        bindings::Plugin::add_to_linker::<PluginState, HasSelf<PluginState>>(
            &mut linker,
            |state| state,
        )
        .map_err(|error| anyhow::anyhow!("{error:#}"))?;

        let ticker = Arc::new(Ticker::start(engine.clone())?);
        Ok(PluginRuntime {
            cache: ComponentCache::new(cache_dir, &engine),
            engine,
            linker: Arc::new(linker),
            pool: Arc::new(Semaphore::new(POOL_SLOTS as usize)),
            host,
            processes,
            ticker,
        })
    }

    /// Verifies `wasm` against the record's hash, compiles it and links it
    /// once. Compiling is CPU-heavy: call from `spawn_blocking`.
    pub fn load(
        &self,
        record: &PluginRecord,
        wasm: &[u8],
        data_dir: PathBuf,
        settings: PluginSettings,
    ) -> Result<LoadedPlugin, LoadError> {
        let expected = record
            .wasm_sha256
            .as_deref()
            .ok_or(LoadError::MissingHash)?;
        let actual = hex(&Sha256::digest(wasm));
        if actual != expected {
            return Err(LoadError::HashMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
        let pre = self.link(expected, wasm)?;
        tracing::info!(plugin = %record.id, "plugin loaded");
        let id: Arc<str> = Arc::from(record.id.as_str());
        let grant = Arc::new(record.grant.clone());
        let programs = Arc::new(Programs {
            plugin: id.clone(),
            grant: grant.clone(),
            resolved: record.program_paths.clone(),
            bin_dir: data_dir.with_file_name("bin"),
            data_dir: data_dir.clone(),
            supervisor: self.processes.clone(),
        });
        let ipc = Arc::new(Connections::new(id.clone(), grant.clone()));
        Ok(LoadedPlugin {
            ipc,
            id,
            engine: self.engine.clone(),
            pre,
            host: self.host.clone(),
            grant,
            programs,
            data_dir,
            settings: Arc::new(settings),
            log: PluginLog::new(
                &record.id,
                (record.source == PluginSource::Dev).then(|| self.host.clone()),
            ),
            pool: self.pool.clone(),
            concurrent: Semaphore::new(CALLS_PER_PLUGIN),
            calls: AtomicU64::new(0),
            traps: AtomicU64::new(0),
            timeouts: AtomicU64::new(0),
            recent_traps: Mutex::new(VecDeque::new()),
            disabled: AtomicBool::new(false),
            _ticker: self.ticker.clone(),
        })
    }

    /// Compiles and links without running any guest code. CPU-heavy.
    pub fn check(&self, wasm: &[u8]) -> Result<(), LoadError> {
        self.link(&hex(&Sha256::digest(wasm)), wasm).map(drop)
    }

    fn link(
        &self,
        sha256: &str,
        wasm: &[u8],
    ) -> Result<bindings::PluginPre<PluginState>, LoadError> {
        let component = self
            .cache
            .component(&self.engine, sha256, wasm)
            .map_err(|error| LoadError::Component(format!("{error:#}")))?;
        let instance_pre = self
            .linker
            .instantiate_pre(&component)
            .map_err(|error| LoadError::Component(format!("{error:#}")))?;
        bindings::PluginPre::new(instance_pre)
            .map_err(|error| LoadError::Component(format!("{error:#}")))
    }
}

/// An enabled plugin. Every call runs on a fresh store that is dropped
/// afterwards, so a trap, timeout or cancel loses only that call and no guest
/// memory is shared between calls.
pub struct LoadedPlugin {
    id: Arc<str>,
    engine: Engine,
    pre: bindings::PluginPre<PluginState>,
    host: Arc<dyn HostCalls>,
    grant: Arc<Grant>,
    programs: Arc<Programs>,
    ipc: Arc<Connections>,
    data_dir: PathBuf,
    settings: Arc<PluginSettings>,
    log: Arc<PluginLog>,
    pool: Arc<Semaphore>,
    concurrent: Semaphore,
    calls: AtomicU64,
    traps: AtomicU64,
    timeouts: AtomicU64,
    recent_traps: Mutex<VecDeque<std::time::Instant>>,
    disabled: AtomicBool,
    _ticker: Arc<Ticker>,
}

impl LoadedPlugin {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// True once repeated traps took the plugin out of service; enabling it
    /// again loads a fresh one.
    pub fn is_disabled(&self) -> bool {
        self.disabled.load(Ordering::Relaxed)
    }

    pub fn stats(&self) -> PluginStats {
        PluginStats {
            calls: self.calls.load(Ordering::Relaxed),
            traps: self.traps.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
        }
    }

    /// Runs the plugin's `lifecycle.init`.
    pub async fn init(&self, chain: &CallChain, timeout: Duration) -> Result<(), CallError> {
        let outcome = self
            .guarded(chain, timeout, None, |store, instance| {
                Box::pin(instance.hestia_plugin_lifecycle().call_init(store))
            })
            .await?;
        outcome.map_err(guest_error)
    }

    /// Runs the plugin's `settings-changed.on-settings-changed` for `key`.
    pub async fn on_settings_changed(
        &self,
        chain: &CallChain,
        key: &str,
        timeout: Duration,
    ) -> Result<(), CallError> {
        let key = key.to_string();
        self.guarded(chain, timeout, None, move |store, instance| {
            Box::pin(async move {
                instance
                    .hestia_plugin_settings_changed()
                    .call_on_settings_changed(store, &key)
                    .await
            })
        })
        .await
    }

    /// Runs the plugin's `commands.run` under `timeout`; `args` is the object
    /// the command declares. A cancel in `control` is only asked of the guest:
    /// after `control.grace` the call is dropped with [`CallError::Cancelled`].
    pub async fn run_command(
        &self,
        chain: &CallChain,
        command: &str,
        args: &Value,
        control: CommandRun,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        let (command, args) = (command.to_string(), args.to_string());
        let reply = self
            .guarded(chain, timeout, Some(control), move |store, instance| {
                Box::pin(async move {
                    instance
                        .hestia_plugin_commands()
                        .call_run(store, &command, &args)
                        .await
                })
            })
            .await?
            .map_err(guest_error)?;
        serde_json::from_str(&reply)
            .map_err(|error| CallError::Guest(format!("the command's result is not JSON: {error}")))
    }

    /// Runs the plugin's `ui.render` under [`UI_TIMEOUT`] and checks the tree
    /// against the UI vocabulary.
    pub async fn render_page(
        &self,
        chain: &CallChain,
        page: &str,
        context: &PageContext,
    ) -> Result<UiNode, CallError> {
        let (name, context) = (page.to_string(), context_json(context));
        let reply = self
            .guarded(chain, UI_TIMEOUT, None, move |store, instance| {
                Box::pin(async move {
                    instance
                        .hestia_plugin_ui()
                        .call_render(store, &name, &context)
                        .await
                })
            })
            .await?
            .map_err(guest_error)?;
        self.decode_ui(page, &reply, UiNode::validate)
    }

    /// Runs the plugin's `ui.action` under [`UI_TIMEOUT`] and checks the effects
    /// against the UI vocabulary.
    pub async fn page_action(
        &self,
        chain: &CallChain,
        page: &str,
        action: &Action,
        values: &Value,
        context: &PageContext,
    ) -> Result<Vec<Effect>, CallError> {
        let (name, action, values, context) = (
            page.to_string(),
            serde_json::to_string(action).expect("an action is plain data"),
            values.to_string(),
            context_json(context),
        );
        let reply = self
            .guarded(chain, UI_TIMEOUT, None, move |store, instance| {
                Box::pin(async move {
                    instance
                        .hestia_plugin_ui()
                        .call_action(store, &name, &action, &values, &context)
                        .await
                })
            })
            .await?
            .map_err(guest_error)?;
        self.decode_ui(page, &reply, |effects: &Vec<Effect>| {
            effects.iter().try_for_each(Effect::validate)
        })
    }

    fn decode_ui<T: DeserializeOwned>(
        &self,
        page: &str,
        reply: &str,
        validate: impl FnOnce(&T) -> Result<(), UiError>,
    ) -> Result<T, CallError> {
        let invalid = |error: &dyn std::fmt::Display| {
            tracing::warn!(plugin = %self.id, %page, %error, "plugin returned an invalid UI document");
            CallError::InvalidUi(page.to_string())
        };
        let document = serde_json::from_str(reply).map_err(|error| invalid(&error))?;
        validate(&document).map_err(|error| invalid(&error))?;
        Ok(document)
    }

    /// Runs the plugin's `events.on-event` under `timeout`.
    pub async fn on_event(
        &self,
        chain: &CallChain,
        topic: &str,
        payload: &Value,
        timeout: Duration,
    ) -> Result<(), CallError> {
        let (topic, payload) = (topic.to_string(), payload.to_string());
        self.guarded(chain, timeout, None, move |store, instance| {
            Box::pin(async move {
                instance
                    .hestia_plugin_events()
                    .call_on_event(store, &topic, &payload)
                    .await
            })
        })
        .await
    }

    /// Runs the hook point `point` under `timeout` with its context as JSON. The
    /// inner error is the plugin's veto reason; the answer is the point's patch
    /// as JSON.
    pub(super) async fn intercept(
        &self,
        chain: &CallChain,
        point: &str,
        context: &str,
        timeout: Duration,
    ) -> Result<Result<String, String>, CallError> {
        let (point, context) = (point.to_string(), context.to_string());
        self.guarded(chain, timeout, None, move |store, instance| {
            Box::pin(async move {
                instance
                    .hestia_plugin_hooks()
                    .call_intercept(store, &point, &context)
                    .await
            })
        })
        .await
    }

    /// Asks the operation `op` of the contributed provider `id` of `kind` under
    /// `timeout`; `args` and the reply are JSON. An operation the guest does not
    /// implement is [`CallError::Unsupported`].
    pub(super) async fn provide(
        &self,
        chain: &CallChain,
        kind: &str,
        id: &str,
        op: &str,
        args: &str,
        timeout: Duration,
    ) -> Result<String, CallError> {
        let (kind, id, op, args) = (
            kind.to_string(),
            id.to_string(),
            op.to_string(),
            args.to_string(),
        );
        self.guarded(chain, timeout, None, move |store, instance| {
            Box::pin(async move {
                instance
                    .hestia_plugin_provider()
                    .call_invoke(store, &kind, &id, &op, &args)
                    .await
            })
        })
        .await?
        .map_err(guest_error)
    }

    /// The path a `PATH` program was pinned to at approval, when this plugin
    /// holds `process:spawn` and the file is still there.
    pub fn pinned_program(&self, id: &str) -> Option<PathBuf> {
        self.programs.wrapper(id)
    }

    /// What the user approved for this plugin.
    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    /// Runs `run` on a fresh store under one wall-clock budget that covers
    /// waiting for a slot, guest compute and awaited host calls. The epoch
    /// deadline only makes a spinning guest yield to the executor; the timeout
    /// is what cuts it off.
    pub(super) async fn guarded<T, F>(
        &self,
        chain: &CallChain,
        timeout: Duration,
        control: Option<CommandRun>,
        run: F,
    ) -> Result<T, CallError>
    where
        F: for<'a> FnOnce(
            &'a mut Store<PluginState>,
            &'a bindings::Plugin,
        ) -> BoxFuture<'a, wasmtime::Result<T>>,
    {
        if self.is_disabled() {
            return Err(CallError::Disabled);
        }
        let chain = chain.enter(&self.id)?;
        let started = std::time::Instant::now();
        let deadline = Instant::now() + timeout;
        let _own_slot = acquire(&self.concurrent, deadline).await?;
        let _pool_slot = acquire(&self.pool, deadline).await?;
        self.calls.fetch_add(1, Ordering::Relaxed);

        let control = control.map(Arc::new);
        let mut store = self
            .new_store(chain, control.clone())
            .map_err(|error| CallError::Setup(format!("{error:#}")))?;
        let attempt = async {
            let instance = self.pre.instantiate_async(&mut store).await?;
            run(&mut store, &instance).await
        };
        let stopped = async {
            match &control {
                Some(control) => {
                    while !control.cancel.is_cancelled() {
                        tokio::time::sleep(CANCEL_POLL).await;
                    }
                    tokio::time::sleep(control.grace).await;
                }
                None => std::future::pending().await,
            }
        };
        let attempt = async {
            tokio::select! {
                result = timeout_at(deadline, attempt) => Some(result),
                () = stopped => None,
            }
        };
        let outcome = match attempt.await {
            None => {
                tracing::info!(plugin = %self.id, "plugin command dropped after its cancel");
                Err(CallError::Cancelled)
            }
            Some(Err(_)) => {
                self.timeouts.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(plugin = %self.id, ?timeout, "plugin call timed out");
                Err(CallError::Timeout(timeout))
            }
            Some(Ok(Err(error))) => {
                let message = format!("{error:#}");
                tracing::warn!(plugin = %self.id, error = %message, "plugin trapped");
                self.record_trap();
                Err(CallError::Trap(message))
            }
            Some(Ok(Ok(value))) => {
                tracing::debug!(
                    plugin = %self.id,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "plugin call finished"
                );
                Ok(value)
            }
        };
        self.log.report_dropped();
        outcome
    }

    fn new_store(
        &self,
        chain: CallChain,
        run: Option<Arc<CommandRun>>,
    ) -> wasmtime::Result<Store<PluginState>> {
        let mut wasi = WasiCtxBuilder::new();
        wasi.stdout(Captured::stdout(self.log.clone()))
            .stderr(Captured::stderr(self.log.clone()))
            .allow_tcp(false)
            .allow_udp(false)
            .allow_ip_name_lookup(false)
            .preopened_dir(&self.data_dir, DATA_MOUNT, FsPerms::ReadWrite)?;
        let limits: StoreLimits = StoreLimitsBuilder::new()
            .memory_size(MEMORY_LIMIT)
            .memories(1)
            .tables(4)
            .table_elements(TABLE_ELEMENTS)
            .instances(16)
            .trap_on_grow_failure(true)
            .build();
        let state = PluginState {
            run,
            wasi: wasi.build(),
            http: WasiHttpCtx::new(),
            http_hooks: PluginHttp {
                plugin: self.id.clone(),
                grant: self.grant.clone(),
                log: self.log.clone(),
            },
            table: ResourceTable::new(),
            limits,
            log: self.log.clone(),
            settings: self.settings.clone(),
            programs: self.programs.clone(),
            ipc: self.ipc.clone(),
            gate: Arc::new(Gate {
                plugin: self.id.clone(),
                host: self.host.clone(),
                grant: self.grant.clone(),
                chain,
                log: self.log.clone(),
            }),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_epoch_deadline(1);
        store.epoch_deadline_async_yield_and_update(1);
        Ok(store)
    }

    fn record_trap(&self) {
        self.traps.fetch_add(1, Ordering::Relaxed);
        let now = std::time::Instant::now();
        let mut recent = self.recent_traps.lock().unwrap();
        recent.push_back(now);
        while recent
            .front()
            .is_some_and(|trapped| now.duration_since(*trapped) > TRAP_WINDOW)
        {
            recent.pop_front();
        }
        if recent.len() >= TRAP_LIMIT && !self.disabled.swap(true, Ordering::Relaxed) {
            tracing::error!(plugin = %self.id, "plugin disabled after repeated traps");
            self.ipc.close_all();
        }
    }
}

async fn acquire(
    semaphore: &Semaphore,
    deadline: Instant,
) -> Result<SemaphorePermit<'_>, CallError> {
    match timeout_at(deadline, semaphore.acquire()).await {
        Ok(Ok(permit)) => Ok(permit),
        _ => Err(CallError::Busy),
    }
}

fn context_json(context: &PageContext) -> String {
    serde_json::to_string(context).expect("a page context is plain data")
}

fn guest_error(error: PluginError) -> CallError {
    match error {
        PluginError::Unsupported(message) => CallError::Unsupported(message),
        PluginError::PermissionDenied(message)
        | PluginError::NotFound(message)
        | PluginError::Invalid(message)
        | PluginError::Failed(message) => CallError::Guest(message),
    }
}

#[cfg(test)]
mod tests;
