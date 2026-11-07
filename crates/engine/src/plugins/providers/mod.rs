//! What a plugin contributes under `contributes.providers`: content sources
//! and Minecraft flavors. Each is the host's own provider trait over the
//! plugin's guest export, so the aggregates treat it like a built-in.

mod content;
mod flavor;

use proto::error::ErrorInfo;

pub(super) use content::SourceOp;
pub use content::{connect_sources, register_sources, PluginContentProvider};
pub(super) use flavor::FlavorOp;
pub use flavor::{connect_flavors, register_flavors, PluginFlavorProvider};

fn call_failed(plugin: &str, detail: String) -> anyhow::Error {
    ErrorInfo::PluginCallFailed {
        id: plugin.to_string(),
        detail,
    }
    .into()
}
