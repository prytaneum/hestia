//! What the interceptor hooks receive and may answer. See [`Plugin::before_launch`](crate::Plugin::before_launch)
//! and [`Plugin::before_server_start`](crate::Plugin::before_server_start), which answer the same patch.

use crate::bindings::exports::hestia::plugin::launch_hooks as wit;

pub use crate::bindings::exports::hestia::plugin::content_hooks::InstallContext;

pub use wit::{LaunchContext, ServerContext};

/// Arguments a launch hook asks to add. The host keeps only an allowlist and
/// drops the rest with a warning.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LaunchPatch {
    /// `-XX:` tuning flags, and `-D<key>=<value>` for keys the manifest declares
    /// under `[launch] properties`.
    pub extra_jvm_args: Vec<String>,
    /// `--width <n>`, `--height <n>` and `--fullscreen`; ignored for a server start.
    pub extra_game_args: Vec<String>,
}

impl From<LaunchPatch> for wit::LaunchPatch {
    fn from(patch: LaunchPatch) -> Self {
        wit::LaunchPatch {
            extra_jvm_args: patch.extra_jvm_args,
            extra_game_args: patch.extra_game_args,
        }
    }
}
