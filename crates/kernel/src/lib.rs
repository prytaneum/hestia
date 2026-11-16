//! The plugin kernel: the registry every plugin, built-in or not, is listed in.

mod builtin;
mod error;
mod registry;

pub use builtin::{Builtin, Busy, Descriptor};
pub use error::PluginError;
pub use registry::Plugins;
