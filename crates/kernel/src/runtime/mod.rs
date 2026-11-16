//! The WASM sandbox: one wasmtime engine, a fresh store and instance per call,
//! bounded time, memory and concurrency, and a verified cache of compiled
//! components.

mod admission;
mod bindings;
mod cache;
mod error;
mod host;
mod limits;
mod ticker;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::time::{error::Elapsed, timeout_at, Instant};
use wasmtime::component::{HasSelf, InstancePre, Linker};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig, Store};

use admission::Admission;
use bindings::exports::hestia::kernel::lifecycle;
use cache::Cache;
pub use error::{GuestError, SandboxError};
use host::State;
pub use limits::Limits;
use limits::MemoryCap;
use ticker::Ticker;

const LIFECYCLE: &str = "hestia:kernel/lifecycle@0.1.0";

/// Runs plugin components. Calls need a tokio runtime; dropping a call's future
/// stops the guest.
pub struct Sandbox {
    engine: Engine,
    linker: Linker<State>,
    cache: Cache,
    admission: Admission,
    ticker: Ticker,
    limits: Limits,
    online: Arc<AtomicBool>,
}

/// A component ready to run. Each export is found on its own; one the component
/// does not have is not provided.
#[derive(Clone)]
pub struct Loaded {
    plugin: String,
    pre: InstancePre<State>,
    lifecycle: Option<lifecycle::GuestIndices>,
}

impl Loaded {
    /// Whether the component exports `lifecycle`.
    pub fn provides_lifecycle(&self) -> bool {
        self.lifecycle.is_some()
    }
}

impl Sandbox {
    /// `cache_dir` holds compiled components (`meta/plugins/` of the data home);
    /// `key_file` holds the key that vouches for them and must not be under
    /// `meta/`, which downloads write to.
    pub fn new(cache_dir: PathBuf, key_file: &Path, limits: Limits) -> Result<Self, SandboxError> {
        let engine = Engine::new(&engine_config(&limits)).map_err(SandboxError::Engine)?;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(SandboxError::Engine)?;
        bindings::Host_::add_to_linker::<State, HasSelf<State>>(&mut linker, |state| state)
            .map_err(SandboxError::Engine)?;
        let ticker =
            Ticker::start(engine.clone()).map_err(|error| SandboxError::Engine(error.into()))?;
        Ok(Sandbox {
            cache: Cache::open(cache_dir, key_file)?,
            admission: Admission::new(limits.clone()),
            engine,
            linker,
            ticker,
            limits,
            online: Arc::new(AtomicBool::new(true)),
        })
    }

    /// What `app.info` reports as network status to calls that start later.
    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::Relaxed);
    }

    /// Reads and compiles `file` off the async executor, or takes it from the
    /// cache. An export that is present but malformed fails the load.
    pub async fn load(&self, plugin: &str, file: &Path) -> Result<Loaded, SandboxError> {
        let (engine, cache) = (self.engine.clone(), self.cache.clone());
        let (id, file) = (plugin.to_owned(), file.to_owned());
        let component = tokio::task::spawn_blocking(move || {
            let wasm = std::fs::read(&file).map_err(|error| SandboxError::Load(error.into()))?;
            cache.component(&engine, &id, &wasm)
        })
        .await
        .map_err(|error| SandboxError::Load(wasmtime::format_err!("{error}")))??;
        let pre = self
            .linker
            .instantiate_pre(&component)
            .map_err(SandboxError::Load)?;
        let lifecycle = match component.get_export_index(None, LIFECYCLE) {
            Some(_) => Some(lifecycle::GuestIndices::new(&pre).map_err(SandboxError::Load)?),
            None => None,
        };
        tracing::info!(plugin, lifecycle = lifecycle.is_some(), "component loaded");
        Ok(Loaded {
            plugin: plugin.to_owned(),
            pre,
            lifecycle,
        })
    }

    /// Runs the plugin's `lifecycle.init`; a component without one succeeds.
    pub async fn init(&self, loaded: &Loaded) -> Result<(), SandboxError> {
        let Some(indices) = &loaded.lifecycle else {
            return Ok(());
        };
        let plugin = loaded.plugin.as_str();
        let deadline = Instant::now() + self.limits.init_timeout;
        let _permit = match timeout_at(deadline, self.admission.admit(plugin)).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) | Err(_) => return Err(SandboxError::Saturated(plugin.to_owned())),
        };
        let _ticking = self.ticker.running();
        let mut store = self.store(plugin);
        let outcome = timeout_at(deadline, async {
            let instance = loaded.pre.instantiate_async(&mut store).await?;
            indices
                .load(&mut store, &instance)?
                .call_init(&mut store)
                .await
        })
        .await;
        self.settle(plugin, outcome, store.data().memory.exceeded)
    }

    fn store(&self, plugin: &str) -> Store<State> {
        let online = self.online.load(Ordering::Relaxed);
        let memory = MemoryCap::new(self.limits.memory_bytes);
        let mut store = Store::new(&self.engine, State::new(plugin, online, memory));
        store.limiter(|state| &mut state.memory);
        store.set_epoch_deadline(1);
        store.epoch_deadline_async_yield_and_update(1);
        store
    }

    /// Runs in the same poll that produced the outcome, so a caller dropping the
    /// call cannot skip the trap count.
    fn settle(
        &self,
        plugin: &str,
        outcome: Result<wasmtime::Result<Result<(), GuestError>>, Elapsed>,
        memory_exceeded: bool,
    ) -> Result<(), SandboxError> {
        let failure = match outcome {
            Ok(Ok(Ok(()))) => return Ok(()),
            Ok(Ok(Err(error))) => return Err(SandboxError::Guest(error)),
            Err(_) => SandboxError::Timeout,
            Ok(Err(_)) if memory_exceeded => SandboxError::Memory,
            Ok(Err(error)) => SandboxError::Trapped(format!("{error:#}")),
        };
        tracing::warn!(plugin, %failure, "plugin call failed");
        if self.admission.record_trap(plugin) {
            tracing::warn!(plugin, "plugin reached its trap limit");
            return Err(SandboxError::TrapLimit(Box::new(failure)));
        }
        Err(failure)
    }
}

fn engine_config(limits: &Limits) -> Config {
    let slots = limits.calls_total as u32;
    let mut pool = PoolingAllocationConfig::new();
    pool.total_component_instances(slots)
        .total_core_instances(slots * 4)
        .total_memories(slots * 2)
        .total_tables(slots * 2)
        .max_memory_size(limits.memory_bytes);
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .epoch_interruption(true)
        .allocation_strategy(InstanceAllocationStrategy::Pooling(pool));
    config
}
