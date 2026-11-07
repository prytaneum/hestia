//! What a plugin contributes under `contributes.providers`: content sources,
//! Minecraft flavors and archive formats. Each is the host's own provider trait over the
//! plugin's guest export, so the aggregates treat it like a built-in.

mod content;
mod flavor;
mod format;

use proto::error::ErrorInfo;

pub(super) use content::SourceOp;
pub use content::{connect_sources, register_sources, PluginContentProvider};
pub(super) use flavor::FlavorOp;
pub use flavor::{connect_flavors, register_flavors, PluginFlavorProvider};
pub(super) use format::FormatCall;
pub use format::{connect_formats, register_formats, PluginFormat};

fn call_failed(plugin: &str, detail: String) -> anyhow::Error {
    ErrorInfo::PluginCallFailed {
        id: plugin.to_string(),
        detail,
    }
    .into()
}
