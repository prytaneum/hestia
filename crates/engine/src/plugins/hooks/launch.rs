use proto::hook::{self, HookPoint};

use super::Hook;
use crate::plugins::patch::LaunchPatch;
use crate::plugins::runtime::LoadedPlugin;

/// Just before a game launch. A plugin's answer is held to its own grant and
/// the allowlist before it is folded in.
pub struct BeforeLaunch;

impl Hook for BeforeLaunch {
    type Point = hook::BeforeLaunch;
    type Plugin = LoadedPlugin;
    type Patch = LaunchPatch;

    fn vet(plugin: &LoadedPlugin, patch: <Self::Point as HookPoint>::Patch) -> LaunchPatch {
        LaunchPatch::vetted(
            plugin.id(),
            &plugin.grant().launch_properties,
            patch.extra_jvm_args,
            patch.extra_game_args,
        )
    }

    fn fold(folded: LaunchPatch, next: LaunchPatch) -> LaunchPatch {
        folded.then(next)
    }
}

/// Just before a server is spawned: the same allowlist as [`BeforeLaunch`],
/// minus the game arguments.
pub struct BeforeServerStart;

impl Hook for BeforeServerStart {
    type Point = hook::BeforeServerStart;
    type Plugin = LoadedPlugin;
    type Patch = LaunchPatch;

    fn vet(plugin: &LoadedPlugin, patch: <Self::Point as HookPoint>::Patch) -> LaunchPatch {
        LaunchPatch::vetted_for_server(
            plugin.id(),
            &plugin.grant().launch_properties,
            patch.extra_jvm_args,
            patch.extra_game_args,
        )
    }

    fn fold(folded: LaunchPatch, next: LaunchPatch) -> LaunchPatch {
        folded.then(next)
    }
}
