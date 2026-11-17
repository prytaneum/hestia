//! The WASM sandbox: one wasmtime engine, a fresh store and instance per call,
//! bounded time, memory and concurrency, and a verified cache of compiled
//! components.

mod admission;
pub(crate) mod bindings;
mod cache;
mod data;
mod download;
mod error;
#[cfg(feature = "probe")]
pub(crate) mod extension;
mod host;
mod http;
mod ipc;
mod jobs;
mod limits;
mod messages;
mod notify;
mod pause;
mod permissions;
mod programs;
mod progress;
mod reach;
mod services;
mod settings;
mod shape;
mod shared;
mod stub;
mod ticker;
mod tree;
mod wakeups;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use proto::plugin::Grant;
use tokio::time::{error::Elapsed, timeout_at, Instant};
use wasmtime::component::{Component, HasSelf, InstancePre, Linker};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig, Store};

#[cfg(feature = "probe")]
use crate::api::Chain;
use crate::api::HostCalls;
use crate::entries::Entries;
use admission::Admission;
use bindings::exports::hestia::kernel::{
    commands, lifecycle, on_job_done, on_schedule, settings_changed,
};
use cache::Cache;
pub use error::{GuestError, SandboxError};
#[cfg(feature = "probe")]
pub(crate) use extension::{Budget, Extension};
use host::Call;
pub(crate) use host::State;
pub use jobs::{Invocation, JobOutcome};
pub use limits::Limits;
use limits::MemoryCap;
use reach::Reach;
pub use services::{Answer, NoticeLevel, Services};
pub use shape::{Shape, ShapeError};
use shared::Shared;
pub use stub::Denials;
use ticker::Ticker;

const CANCEL_POLL: Duration = Duration::from_millis(100);
const LIFECYCLE: &str = "hestia:kernel/lifecycle@0.1.0";
const SETTINGS_CHANGED: &str = "hestia:kernel/settings-changed@0.1.0";
const COMMANDS: &str = "hestia:kernel/commands@0.1.0";
const ON_SCHEDULE: &str = "hestia:kernel/on-schedule@0.1.0";
const ON_JOB_DONE: &str = "hestia:kernel/on-job-done@0.1.0";

/// The export a call runs.
enum Entry<'a> {
    Init(&'a lifecycle::GuestIndices),
    SettingChanged(&'a settings_changed::GuestIndices, &'a str, &'a str),
    #[cfg(feature = "probe")]
    Extension(&'a dyn Extension),
    Command(&'a commands::GuestIndices, &'a str, &'a str),
    Tick(&'a on_schedule::GuestIndices, &'a str),
    JobDone(&'a on_job_done::GuestIndices, &'a String, &'a JobOutcome),
}

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
    shared: Shared,
}

/// A component ready to run. Each export is found on its own; one the component
/// does not have is not provided.
#[derive(Clone)]
pub struct Loaded {
    plugin: String,
    pre: InstancePre<State>,
    reach: Arc<Reach>,
    lifecycle: Option<lifecycle::GuestIndices>,
    settings_changed: Option<settings_changed::GuestIndices>,
    commands: Option<commands::GuestIndices>,
    on_schedule: Option<on_schedule::GuestIndices>,
    on_job_done: Option<on_job_done::GuestIndices>,
}

/// A compiled component that is not linked yet.
pub struct Opened {
    plugin: String,
    component: Component,
}

impl Loaded {
    /// Whether the component exports `lifecycle`.
    pub fn provides_lifecycle(&self) -> bool {
        self.lifecycle.is_some()
    }

    /// Whether the component exports `interface` (`package/name@version`).
    #[cfg(feature = "probe")]
    pub(crate) fn exports(&self, interface: &str) -> bool {
        self.pre
            .component()
            .get_export_index(None, interface)
            .is_some()
    }

    /// Whether the plugin holds `permission` (an optional one only once granted).
    #[cfg(feature = "probe")]
    pub(crate) fn holds(&self, permission: &str) -> bool {
        self.reach.granted.iter().any(|held| held == permission)
    }

    /// Whether the component exports `settings-changed`.
    pub fn provides_settings_changed(&self) -> bool {
        self.settings_changed.is_some()
    }

    pub fn provides_on_schedule(&self) -> bool {
        self.on_schedule.is_some()
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
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)
            .map_err(SandboxError::Engine)?;
        bindings::Host_::add_to_linker::<State, HasSelf<State>>(&mut linker, |state| state)
            .map_err(SandboxError::Engine)?;
        crate::api::link(&mut linker)?;
        let ticker =
            Ticker::start(engine.clone()).map_err(|error| SandboxError::Engine(error.into()))?;
        let shared = Shared::new(Arc::new(services::Unavailable));
        #[cfg(feature = "probe")]
        let shared = Shared {
            events: Arc::new(wakeups::Wakeups::new(
                limits.event_burst,
                limits.event_refill_per_second,
            )),
            ..shared
        };
        Ok(Sandbox {
            cache: Cache::open(cache_dir, key_file)?,
            admission: Admission::new(limits.clone()),
            engine,
            linker,
            ticker,
            limits,
            online: Arc::new(AtomicBool::new(true)),
            shared,
        })
    }

    /// Gives plugins the files, grants and front-ends of the host around the sandbox.
    pub fn with_services(mut self, services: Arc<dyn Services>) -> Self {
        self.shared.services = services;
        self
    }

    /// The router the adapters of built-in packages call through.
    pub fn set_calls(&self, calls: Arc<dyn HostCalls>) {
        if self.shared.calls.set(calls).is_err() {
            tracing::warn!("the host calls were already set");
        }
    }

    /// What `app.info` reports as network status to calls that start later.
    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::Relaxed);
    }

    /// Compiles `wasm` off the async executor, or takes it from the cache.
    /// The caller passes the bytes it checked, so what runs is what was
    /// approved.
    pub async fn open(&self, plugin: &str, wasm: Vec<u8>) -> Result<Opened, SandboxError> {
        let (engine, cache, id) = (self.engine.clone(), self.cache.clone(), plugin.to_owned());
        let component = tokio::task::spawn_blocking(move || cache.component(&engine, &id, &wasm))
            .await
            .map_err(|error| SandboxError::Load(wasmtime::format_err!("{error}")))??;
        Ok(Opened {
            plugin: plugin.to_owned(),
            component,
        })
    }

    /// Links `opened`, each denied import to a stub that answers `denied`; a
    /// denied import the sandbox cannot answer (`grant::deniable`) fails the
    /// load. An export that is present but malformed fails the load. Calls reach
    /// only the hosts, programs and endpoints `grant` lists, and the entries
    /// `entries` holds now, which the registry keeps current.
    pub fn link(
        &self,
        opened: &Opened,
        denials: &Denials,
        grant: &Grant,
        entries: Entries,
    ) -> Result<Loaded, SandboxError> {
        if let Some((import, permission)) = denials
            .iter()
            .find(|(name, _)| !crate::grant::deniable(name))
        {
            return Err(SandboxError::Load(wasmtime::format_err!(
                "'{import}' needs '{permission}', which is not granted"
            )));
        }
        let mut linker = self.linker.clone();
        stub::link_denials(&mut linker, &self.engine, &opened.component, denials)?;
        let pre = linker
            .instantiate_pre(&opened.component)
            .map_err(SandboxError::Load)?;
        crate::api::check_exports(&opened.component, &pre)?;
        let lifecycle = match opened.component.get_export_index(None, LIFECYCLE) {
            Some(_) => Some(lifecycle::GuestIndices::new(&pre).map_err(SandboxError::Load)?),
            None => None,
        };
        let settings_changed = match opened.component.get_export_index(None, SETTINGS_CHANGED) {
            Some(_) => Some(settings_changed::GuestIndices::new(&pre).map_err(SandboxError::Load)?),
            None => None,
        };
        let commands = match opened.component.get_export_index(None, COMMANDS) {
            Some(_) => Some(commands::GuestIndices::new(&pre).map_err(SandboxError::Load)?),
            None => None,
        };
        let on_schedule = match opened.component.get_export_index(None, ON_SCHEDULE) {
            Some(_) => Some(on_schedule::GuestIndices::new(&pre).map_err(SandboxError::Load)?),
            None => None,
        };
        let on_job_done = match opened.component.get_export_index(None, ON_JOB_DONE) {
            Some(_) => Some(on_job_done::GuestIndices::new(&pre).map_err(SandboxError::Load)?),
            None => None,
        };
        tracing::info!(
            plugin = opened.plugin,
            lifecycle = lifecycle.is_some(),
            settings_changed = settings_changed.is_some(),
            commands = commands.is_some(),
            on_schedule = on_schedule.is_some(),
            on_job_done = on_job_done.is_some(),
            denied = denials.len(),
            "component loaded"
        );
        let mut reach = Reach::of(grant);
        reach.entries = entries;
        Ok(Loaded {
            plugin: opened.plugin.clone(),
            pre,
            reach: Arc::new(reach),
            lifecycle,
            settings_changed,
            commands,
            on_schedule,
            on_job_done,
        })
    }

    /// `open` and `link` with nothing denied.
    pub async fn load(&self, plugin: &str, wasm: Vec<u8>) -> Result<Loaded, SandboxError> {
        self.link(
            &self.open(plugin, wasm).await?,
            &Denials::new(),
            &Grant::default(),
            Entries::default(),
        )
    }

    /// Drops the plugin's compiled cache entry.
    pub fn forget(&self, plugin: &str) {
        self.cache.forget(plugin);
    }

    /// Runs the plugin's `lifecycle.init`; a component without one succeeds.
    pub async fn init(&self, loaded: &Loaded) -> Result<(), SandboxError> {
        let Some(indices) = &loaded.lifecycle else {
            return Ok(());
        };
        let budget = self.limits.init_timeout;
        self.run(loaded, budget, Call::default(), Entry::Init(indices))
            .await
    }

    /// Stores a setting the user changed, then tells the plugin if it listens.
    /// A plugin that fails the notice keeps the stored value.
    pub async fn change_setting(
        &self,
        loaded: &Loaded,
        key: &str,
        value: &str,
    ) -> Result<(), SandboxError> {
        settings::save_setting(&self.shared, &loaded.plugin, key, value)
            .map_err(SandboxError::Setting)?;
        let Some(indices) = &loaded.settings_changed else {
            return Ok(());
        };
        let person = Call {
            person: true,
            ..Call::default()
        };
        let budget = self.limits.event_timeout;
        self.run(
            loaded,
            budget,
            person,
            Entry::SettingChanged(indices, key, value),
        )
        .await
    }

    /// One call into an extension point a built-in's package defines, run
    /// like any other call; `chain` is the plugins that led to it.
    #[cfg(feature = "probe")]
    pub(crate) async fn extend(
        &self,
        loaded: &Loaded,
        chain: Chain,
        extension: &dyn Extension,
    ) -> Result<(), SandboxError> {
        let call = Call {
            chain,
            ..Call::default()
        };
        let budget = match extension.budget() {
            Budget::Event => self.limits.event_timeout,
            Budget::Hook => self.limits.hook_timeout,
        };
        self.run(loaded, budget, call, Entry::Extension(extension))
            .await
    }

    /// Runs the plugin's `commands.run` for up to the command budget. A person's
    /// call may ask them to confirm; a call in a job reports its progress.
    pub async fn command(
        &self,
        loaded: &Loaded,
        command: &str,
        args: &str,
        invocation: Invocation,
    ) -> Result<(), SandboxError> {
        let Some(indices) = &loaded.commands else {
            return Err(SandboxError::Unprovided("commands"));
        };
        let budget = self.limits.command_timeout;
        let call = invocation.into_call();
        self.run(loaded, budget, call, Entry::Command(indices, command, args))
            .await
    }

    /// Tells the plugin a schedule it declared ticked; a plugin that does not
    /// listen succeeds.
    pub async fn tick(&self, loaded: &Loaded, schedule: &str) -> Result<(), SandboxError> {
        let Some(indices) = &loaded.on_schedule else {
            return Ok(());
        };
        let budget = self.limits.event_timeout;
        self.run(
            loaded,
            budget,
            Call::default(),
            Entry::Tick(indices, schedule),
        )
        .await
    }

    /// Tells the plugin a job it started ended; a plugin that does not listen
    /// succeeds.
    pub async fn job_done(
        &self,
        loaded: &Loaded,
        job: &String,
        outcome: &JobOutcome,
    ) -> Result<(), SandboxError> {
        let Some(indices) = &loaded.on_job_done else {
            return Ok(());
        };
        let budget = self.limits.event_timeout;
        self.run(
            loaded,
            budget,
            Call::default(),
            Entry::JobDone(indices, job, outcome),
        )
        .await
    }

    /// Spends one of the plugin's wake-ups: what a schedule tick or a job the
    /// plugin starts costs. `false` when it has none left.
    pub fn spend_wakeup(&self, plugin: &str) -> bool {
        self.shared.wakeups.spend(plugin)
    }

    /// Spends one of the plugin's event deliveries, a budget apart from its
    /// wake-ups; otherwise says how long until one is refilled.
    #[cfg(feature = "probe")]
    pub(crate) fn spend_event(&self, plugin: &str) -> Result<(), Duration> {
        self.shared.events.take(plugin)
    }

    /// Stores a secret the user entered for the plugin.
    pub fn store_secret(&self, plugin: &str, key: &str, value: &str) -> Result<(), SandboxError> {
        settings::save_secret(&self.shared, plugin, key, value).map_err(SandboxError::Setting)
    }

    /// One call under the sandbox's bounds: a slot, a fresh store, and `budget`
    /// of wall clock, paused while the call waits on a person.
    async fn run(
        &self,
        loaded: &Loaded,
        budget: Duration,
        call: Call,
        entry: Entry<'_>,
    ) -> Result<(), SandboxError> {
        let plugin = loaded.plugin.as_str();
        let call = Call {
            chain: call.chain.enter(plugin)?,
            ..call
        };
        let start = Instant::now();
        let _permit = match timeout_at(start + budget, self.admission.admit(plugin)).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) | Err(_) => return Err(SandboxError::Saturated(plugin.to_owned())),
        };
        let _ticking = self.ticker.running();
        let pause = call.pause.clone();
        let cancelled = matches!(entry, Entry::Command(..)).then(|| call.cancelled.clone());
        let mut store = self.store(plugin, call, loaded.reach.clone());
        let outcome = {
            let mut guest = pin!(async {
                let instance = loaded.pre.instantiate_async(&mut store).await?;
                match entry {
                    Entry::Init(indices) => {
                        indices
                            .load(&mut store, &instance)?
                            .call_init(&mut store)
                            .await
                    }
                    #[cfg(feature = "probe")]
                    Entry::Extension(extension) => {
                        extension.call(&mut store, &instance, &loaded.pre).await
                    }
                    Entry::SettingChanged(indices, key, value) => {
                        indices
                            .load(&mut store, &instance)?
                            .call_changed(&mut store, key, value)
                            .await
                    }
                    Entry::Command(indices, command, args) => {
                        indices
                            .load(&mut store, &instance)?
                            .call_run(&mut store, command, args)
                            .await
                    }
                    Entry::Tick(indices, schedule) => {
                        indices
                            .load(&mut store, &instance)?
                            .call_tick(&mut store, schedule)
                            .await
                    }
                    Entry::JobDone(indices, job, outcome) => {
                        indices
                            .load(&mut store, &instance)?
                            .call_finished(&mut store, job, outcome)
                            .await
                    }
                }
            });
            let timed = async {
                loop {
                    let deadline = start + budget + pause.paused();
                    match timeout_at(deadline, &mut guest).await {
                        Err(_) if start + budget + pause.paused() > Instant::now() => continue,
                        outcome => break outcome,
                    }
                }
            };
            tokio::select! {
                outcome = timed => outcome,
                () = dropped_after_cancel(cancelled, self.limits.cancel_grace) => {
                    tracing::warn!(plugin, "command ignored its cancel; call dropped");
                    return Err(SandboxError::Stopped);
                }
            }
        };
        self.settle(plugin, outcome, store.data().memory.exceeded)
    }

    fn store(&self, plugin: &str, call: Call, reach: Arc<Reach>) -> Store<State> {
        let online = self.online.load(Ordering::Relaxed);
        let memory = MemoryCap::new(self.limits.memory_bytes);
        let mut store = Store::new(
            &self.engine,
            State::new(plugin, online, memory, self.shared.clone(), call, reach),
        );
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

/// Resolves once `cancelled` has been raised for `grace`; never for a call
/// that has no flag to watch. The flag is polled because the job's cancel is a
/// bare atomic shared with every other kind of job.
async fn dropped_after_cancel(cancelled: Option<Arc<AtomicBool>>, grace: Duration) {
    let Some(cancelled) = cancelled else {
        return std::future::pending().await;
    };
    while !cancelled.load(Ordering::Relaxed) {
        tokio::time::sleep(CANCEL_POLL).await;
    }
    tokio::time::sleep(grace).await;
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
