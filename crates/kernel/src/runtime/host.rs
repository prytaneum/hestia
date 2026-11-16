use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use super::bindings::hestia::kernel::{app, log, types};
use super::limits::MemoryCap;

const LOG_LINE_BYTES: usize = 4096;

/// The store data of one call. WASI sees no preopen, no network and an empty
/// environment.
pub(super) struct State {
    plugin: String,
    online: bool,
    wasi: WasiCtx,
    table: ResourceTable,
    pub memory: MemoryCap,
}

impl State {
    pub fn new(plugin: &str, online: bool, memory: MemoryCap) -> Self {
        State {
            plugin: plugin.to_owned(),
            online,
            wasi: WasiCtxBuilder::new()
                .allow_tcp(false)
                .allow_udp(false)
                .allow_ip_name_lookup(false)
                .build(),
            table: ResourceTable::new(),
            memory,
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

impl types::Host for State {}

impl log::Host for State {
    fn write(&mut self, level: log::Level, message: String) {
        let line = &message[..message.floor_char_boundary(LOG_LINE_BYTES)];
        let plugin = self.plugin.as_str();
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
