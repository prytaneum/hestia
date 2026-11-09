//! Hook points: the moments an operation offers every plugin that declares the
//! point a chance to adjust or refuse it. A point is data, like a [`Contract`](crate::Contract):
//! its name, the permissions it needs, what the plugin is shown and what it may
//! answer. The ABI carries any point through one `hooks.intercept` export, so
//! adding a point never changes the WIT
//! ([0106](../../../docs/decisions/0106-the-plugin-api-is-mechanisms-not-features.md)).
//!
//! A hook is customisation that fails open, never a control
//! ([0095](../../../docs/decisions/0095-interceptors-are-bounded-and-fail-open.md)).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::contract::Empty;
use crate::error::EntryKind;
use crate::plugin::Permission;

/// One hook point. The host shows each declaring plugin the `Context` and reads
/// back a `Patch`, or the plugin's veto.
pub trait HookPoint {
    /// The name a manifest lists in `contributes.hooks`.
    const NAME: &'static str;
    /// What a plugin must hold to declare the point.
    const PERMISSIONS: &'static [Permission];
    type Context: Serialize + DeserializeOwned;
    /// A plugin with nothing to add answers the default.
    type Patch: Serialize + DeserializeOwned + Default;
}

/// A point by name, for checking a manifest without its types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointSpec {
    pub name: &'static str,
    pub permissions: &'static [Permission],
}

const fn spec<P: HookPoint>() -> PointSpec {
    PointSpec {
        name: P::NAME,
        permissions: P::PERMISSIONS,
    }
}

/// Every point this host offers.
pub const POINTS: &[PointSpec] = &[
    spec::<BeforeLaunch>(),
    spec::<BeforeServerStart>(),
    spec::<BeforeContentInstall>(),
];

/// The point named `name`, if this host offers it.
pub fn point(name: &str) -> Option<&'static PointSpec> {
    POINTS.iter().find(|point| point.name == name)
}

/// What a launch is about to run. There is no access token and no raw command
/// line: a hook adjusts a launch, it does not see its secrets.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct LaunchContext {
    pub instance_id: String,
    pub instance_name: String,
    pub game_version: String,
    pub flavor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub java_path: String,
    /// The configured heap size, e.g. `4G`; absent when left to the JVM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// What the launch joins on start: `world:<folder>`, `server:<address>` or
    /// `realm:<id>`; absent for the title screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_play: Option<String>,
    pub profile_name: String,
    pub profile_uuid: String,
}

/// What a server start is about to run.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerContext {
    pub id: String,
    pub name: String,
    pub version: String,
    pub flavor: String,
    pub port: u16,
    /// The configured heap size, e.g. `4G`; absent when left to the JVM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

/// One file about to land in an entry: an install, each required dependency, a
/// local import, an update or version change, or a modpack's file. Names and a
/// hash, never the file's bytes: a convenience check, not a security scanner.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct InstallContext {
    pub entry_id: String,
    pub entry_type: EntryKind,
    /// `local` for an imported file no source recognises; `project_id` and
    /// `version_id` are then empty.
    pub source: String,
    pub project_id: String,
    pub version_id: String,
    pub file_name: String,
    /// Empty when the source publishes no sha1 for the file.
    pub sha1: String,
    pub kind: ContentKind,
}

/// Arguments a launch or server-start hook asks to add. The host keeps only an
/// allowlist (tuning `-XX:` flags, `-D` keys the manifest declares, `--width`,
/// `--height`, `--fullscreen`) and drops the rest with a warning. `wrapper`
/// names one of the plugin's own `[[programs]]`, which needs `process:spawn`; the
/// host runs the game through its pinned path, or drops the request with a
/// warning when the name is unknown or the permission missing.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct LaunchPatch {
    pub extra_jvm_args: Vec<String>,
    /// Ignored for a server start: a server has no window.
    pub extra_game_args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrapper: Option<String>,
}

/// Runs just before the game is spawned.
pub struct BeforeLaunch;
impl HookPoint for BeforeLaunch {
    const NAME: &'static str = "before-launch";
    const PERMISSIONS: &'static [Permission] = &[Permission::LaunchPatch, Permission::AccountRead];
    type Context = LaunchContext;
    type Patch = LaunchPatch;
}

/// Runs just before a server is spawned.
pub struct BeforeServerStart;
impl HookPoint for BeforeServerStart {
    const NAME: &'static str = "before-server-start";
    const PERMISSIONS: &'static [Permission] = &[Permission::LaunchPatch, Permission::AccountRead];
    type Context = ServerContext;
    type Patch = LaunchPatch;
}

/// Runs before each file is written into an entry. A veto refuses that file only.
pub struct BeforeContentInstall;
impl HookPoint for BeforeContentInstall {
    const NAME: &'static str = "before-content-install";
    const PERMISSIONS: &'static [Permission] = &[Permission::ContentRead];
    type Context = InstallContext;
    type Patch = Empty;
}
