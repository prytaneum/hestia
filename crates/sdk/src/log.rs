//! Lines in the daemon's log, tagged with the plugin's id. Anything a plugin
//! prints to stdout or stderr lands there too.
//!
//! ```no_run
//! hestia::log::info("tunnel is up");
//! hestia::log::warn(&format!("retrying in {}s", 5));
//! ```

#[cfg(target_arch = "wasm32")]
pub use crate::bindings::hestia::plugin::log::{debug, error, info, trace, warn};
#[cfg(not(target_arch = "wasm32"))]
pub use crate::mock::log::{debug, error, info, trace, warn};
