use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use proto::plugin::Grant;
use sha2::{Digest, Sha256};
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::time::{timeout_at, Instant};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig, Store};
use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

use super::chain::{CallChain, ChainError};
use super::imports::bindings::{self, hestia::plugin::types::PluginError};
use super::imports::{Gate, HostCalls, PluginState};
use super::output::{Captured, PluginLog};
use super::store::PluginRecord;
use crate::checksum::hex;

pub const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(2);
pub const UI_TIMEOUT: Duration = Duration::from_secs(10);
pub const PROVIDER_TIMEOUT: Duration = Duration::from_secs(30);

const EPOCH_TICK: Duration = Duration::from_millis(10);
const POOL_SLOTS: u32 = 16;
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
const CALLS_PER_PLUGIN: usize = 4;
const TRAP_LIMIT: usize = 3;
const TRAP_WINDOW: Duration = Duration::from_secs(10 * 60);
const DATA_MOUNT: &str = "/data";

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
    #[error("the plugin trapped: {0}")]
    Trap(String),
    #[error("cannot prepare the plugin's sandbox: {0}")]
    Setup(String),
    #[error("{0}")]
    Guest(String),
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
    ticker: Arc<Ticker>,
}

impl PluginRuntime {
    pub fn new(host: Arc<dyn HostCalls>) -> anyhow::Result<Self> {
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
        bindings::Plugin::add_to_linker::<PluginState, HasSelf<PluginState>>(
            &mut linker,
            |state| state,
        )
        .map_err(|error| anyhow::anyhow!("{error:#}"))?;

        let ticker = Arc::new(Ticker::start(engine.clone())?);
        Ok(PluginRuntime {
            engine,
            linker: Arc::new(linker),
            pool: Arc::new(Semaphore::new(POOL_SLOTS as usize)),
            host,
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
        let component = Component::new(&self.engine, wasm)
            .map_err(|error| LoadError::Component(format!("{error:#}")))?;
        let instance_pre = self
            .linker
            .instantiate_pre(&component)
            .map_err(|error| LoadError::Component(format!("{error:#}")))?;
        let pre = bindings::PluginPre::new(instance_pre)
            .map_err(|error| LoadError::Component(format!("{error:#}")))?;
        tracing::info!(plugin = %record.id, "plugin loaded");
        Ok(LoadedPlugin {
            id: Arc::from(record.id.as_str()),
            engine: self.engine.clone(),
            pre,
            host: self.host.clone(),
            grant: Arc::new(record.grant.clone()),
            data_dir,
            log: PluginLog::new(&record.id),
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
    data_dir: PathBuf,
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
            .guarded(chain, timeout, |store, instance| {
                Box::pin(instance.hestia_plugin_lifecycle().call_init(store))
            })
            .await?;
        outcome.map_err(|error| CallError::Guest(describe(error)))
    }

    /// Runs `run` on a fresh store under one wall-clock budget that covers
    /// waiting for a slot, guest compute and awaited host calls. The epoch
    /// deadline only makes a spinning guest yield to the executor; the timeout
    /// is what cuts it off.
    async fn guarded<T, F>(
        &self,
        chain: &CallChain,
        timeout: Duration,
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

        let mut store = self
            .new_store(chain)
            .map_err(|error| CallError::Setup(format!("{error:#}")))?;
        let attempt = async {
            let instance = self.pre.instantiate_async(&mut store).await?;
            run(&mut store, &instance).await
        };
        match timeout_at(deadline, attempt).await {
            Err(_) => {
                self.timeouts.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(plugin = %self.id, ?timeout, "plugin call timed out");
                Err(CallError::Timeout(timeout))
            }
            Ok(Err(error)) => {
                let message = format!("{error:#}");
                tracing::warn!(plugin = %self.id, error = %message, "plugin trapped");
                self.record_trap();
                Err(CallError::Trap(message))
            }
            Ok(Ok(value)) => {
                tracing::debug!(
                    plugin = %self.id,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "plugin call finished"
                );
                Ok(value)
            }
        }
    }

    fn new_store(&self, chain: CallChain) -> wasmtime::Result<Store<PluginState>> {
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
            .table_elements(100_000)
            .instances(16)
            .trap_on_grow_failure(true)
            .build();
        let state = PluginState {
            wasi: wasi.build(),
            table: ResourceTable::new(),
            limits,
            log: self.log.clone(),
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

fn describe(error: PluginError) -> String {
    match error {
        PluginError::PermissionDenied(message)
        | PluginError::NotFound(message)
        | PluginError::Invalid(message)
        | PluginError::Failed(message) => message,
    }
}

#[cfg(test)]
mod tests;
