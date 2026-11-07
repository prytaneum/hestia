use proto::hook::{self, HookPoint};

use super::Hook;
use crate::plugins::runtime::LoadedPlugin;

/// Before each file is installed. It can refuse the file and nothing more, and
/// it fails open like every hook: a convenience, not a security scanner.
pub struct BeforeContentInstall;

impl Hook for BeforeContentInstall {
    type Point = hook::BeforeContentInstall;
    type Plugin = LoadedPlugin;
    type Patch = ();

    fn vet(_: &LoadedPlugin, _: <Self::Point as HookPoint>::Patch) {}

    fn fold((): (), (): ()) {}
}
