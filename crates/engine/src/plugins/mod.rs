//! The plugin registry and the built-ins it lists.

mod builtin;
mod error;
mod registry;

pub use builtin::{all, Builtin, Busy, Descriptor};
pub use error::PluginError;
pub use registry::Plugins;
