use std::sync::atomic::AtomicBool;
use std::sync::{Arc, PoisonError};

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use super::bindings::hestia::kernel::{app, log, types};
use super::http::Hooks;
use super::limits::MemoryCap;
use super::notify::Drawn;
use super::pause::Pause;
use super::reach::Reach;
use super::shared::Shared;
use crate::api::Chain;

const LOG_LINE_BYTES: usize = 4096;
const LOG_LINES_PER_CALL: usize = 1000;

/// What a call is for, set by whoever starts it.
#[derive(Default)]
pub(super) struct Call {
    /// Whether a person started the call; only such a call may ask them to confirm.
    pub person: bool,
    /// The job the call runs in, which its progress reports belong to.
    pub job: Option<String>,
    pub cancelled: Arc<AtomicBool>,
    pub pause: Arc<Pause>,
    /// The plugins whose calls led to this one, ending with the plugin running
    /// it once the call starts.
    pub chain: Chain,
}

/// The store data of one call. WASI sees one preopen, `/data`, no sockets and an
/// empty environment; the network is `wasi:http` through the plugin's policy.
pub(crate) struct State {
    pub(super) plugin: String,
    online: bool,
    log_lines: usize,
    pub(super) shared: Shared,
    pub(super) call: Call,
    /// The named locks this call holds, released when it ends however it ends.
    pub(super) held: Vec<String>,
    pub(super) drawn: Drawn,
    wasi: WasiCtx,
    table: ResourceTable,
    pub(super) memory: MemoryCap,
    pub(super) reach: Arc<Reach>,
    http: WasiHttpCtx,
    hooks: Hooks,
}

impl State {
    pub(super) fn new(
        plugin: &str,
        online: bool,
        memory: MemoryCap,
        shared: Shared,
        call: Call,
        reach: Arc<Reach>,
    ) -> Self {
        let mut wasi = WasiCtxBuilder::new();
        wasi.allow_tcp(false)
            .allow_udp(false)
            .allow_ip_name_lookup(false);
        if let Some(dir) = shared.services.data_dir(plugin) {
            let mounted = prepare(&shared, plugin, &dir).and_then(|()| {
                wasi.preopened_dir(&dir, "/data", FsPerms::ReadWrite)
                    .map(drop)
            });
            if let Err(error) = mounted {
                tracing::warn!(plugin, %error, "plugin data directory not mounted");
            }
        }
        State {
            plugin: plugin.to_owned(),
            online,
            log_lines: 0,
            shared,
            call,
            held: Vec::new(),
            drawn: Drawn::default(),
            wasi: wasi.build(),
            table: ResourceTable::new(),
            memory,
            http: WasiHttpCtx::new(),
            hooks: Hooks {
                plugin: plugin.to_owned(),
                reach: reach.clone(),
            },
            reach,
        }
    }
}

/// What an adapter of a built-in package calls through: the daemon's router,
/// with this call's chain, which already ends with this plugin.
#[cfg(feature = "probe")]
impl State {
    pub(crate) fn link(&self) -> Result<crate::api::Link, types::PluginError> {
        let calls = self
            .shared
            .calls
            .get()
            .cloned()
            .ok_or(types::PluginError::Unsupported)?;
        Ok(crate::api::Link {
            calls,
            chain: self.call.chain.clone(),
        })
    }
}

fn prepare(shared: &Shared, plugin: &str, dir: &std::path::Path) -> wasmtime::Result<()> {
    let mut mounted = shared
        .mounted
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !mounted.contains(plugin) {
        std::fs::create_dir_all(dir)?;
        mounted.insert(plugin.to_owned());
    }
    Ok(())
}

impl Drop for State {
    fn drop(&mut self) {
        let mut locks = self
            .shared
            .locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for name in self.held.drain(..) {
            locks.remove(&(self.plugin.clone(), name));
        }
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for State {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}

impl types::Host for State {}

impl log::Host for State {
    fn write(&mut self, level: log::Level, message: String) {
        self.log_lines += 1;
        let plugin = self.plugin.as_str();
        if self.log_lines > LOG_LINES_PER_CALL {
            if self.log_lines == LOG_LINES_PER_CALL + 1 {
                tracing::warn!(
                    plugin,
                    "plugin log lines past {LOG_LINES_PER_CALL} in one call are dropped"
                );
            }
            return;
        }
        let line = &message[..message.floor_char_boundary(LOG_LINE_BYTES)];
        match level {
            log::Level::Trace => tracing::trace!(target: "plugin", plugin, "{line:?}"),
            log::Level::Debug => tracing::debug!(target: "plugin", plugin, "{line:?}"),
            log::Level::Info => tracing::info!(target: "plugin", plugin, "{line:?}"),
            log::Level::Warn => tracing::warn!(target: "plugin", plugin, "{line:?}"),
            log::Level::Error => tracing::error!(target: "plugin", plugin, "{line:?}"),
        }
    }
}

impl app::Host for State {
    fn info(&mut self) -> app::AppInfo {
        app::AppInfo {
            name: common::app::NAME.to_owned(),
            version: common::app::VERSION.to_owned(),
            channel: common::app::CHANNEL.to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            online: self.online,
        }
    }
}
