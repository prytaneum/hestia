//! The `flavor` provider kind: a Minecraft distribution. A flavor decides the
//! code the game runs, so the host vets every profile before acting on it: each
//! artifact must carry a sha1 and sit on one of the plugin's network hosts, or
//! be named as built: a flavor's optional `build` produces those files locally,
//! in an area of its own, and the host places them.

use serde::{Deserialize, Serialize};

use super::{ProviderKind, ProviderOp};
use crate::contract::Empty;
use crate::minecraft::GameVersion;
use crate::plugin::Permission;
use crate::profile::{FlavorDescription, InstanceProfile, ServerProfile};

pub struct Flavor;
impl ProviderKind for Flavor {
    const KIND: &'static str = "flavor";
    const PERMISSIONS: &'static [Permission] =
        &[Permission::FlavorProvide, Permission::NetworkConnect];
}

/// One game version.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct GameQuery {
    pub game: String,
}

/// A game version and, for a flavor with a loader, the loader build.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ResolveQuery {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
}

/// Which half of the game a build is for.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum BuildSide {
    #[default]
    Server,
    Client,
}

/// What a `build` is asked to produce files for. `vanilla_jar` is the guest
/// path of the side's vanilla jar under the read-only `/vanilla` mount, absent
/// when the side has none (a server whose jar is itself built).
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct BuildQuery {
    pub side: BuildSide,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vanilla_jar: Option<String>,
}

/// One file a build produced: `path` is a guest path under `/build`, `target`
/// what it becomes. For a server, the filename of the profile's built jar; for a
/// client, the `path` of one of the profile's built libraries.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct BuildOutput {
    pub path: String,
    pub target: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct BuildDone {
    pub outputs: Vec<BuildOutput>,
}

/// Describes the flavor. Asked once, when the plugin is enabled.
pub struct Info;
impl ProviderOp for Info {
    type Kind = Flavor;
    const OP: &'static str = "info";
    type Args = Empty;
    type Reply = FlavorDescription;
}

/// The game versions the flavor supports, newest first.
pub struct Versions;
impl ProviderOp for Versions {
    type Kind = Flavor;
    const OP: &'static str = "versions";
    type Args = Empty;
    type Reply = Vec<GameVersion>;
}

/// The loader builds for a game version, newest first. Optional: unsupported
/// reads as none.
pub struct LoaderVersions;
impl ProviderOp for LoaderVersions {
    type Kind = Flavor;
    const OP: &'static str = "loader-versions";
    type Args = GameQuery;
    type Reply = Vec<String>;
}

/// The launch profile of a server. The jar is a download or named as built.
pub struct ResolveServer;
impl ProviderOp for ResolveServer {
    type Kind = Flavor;
    const OP: &'static str = "resolve-server";
    type Args = ResolveQuery;
    type Reply = ServerProfile;
}

/// The launch profile of a client, under the same rules as [`ResolveServer`]; a
/// library may be named as built.
pub struct ResolveClient;
impl ProviderOp for ResolveClient {
    type Kind = Flavor;
    const OP: &'static str = "resolve-client";
    type Args = ResolveQuery;
    type Reply = InstanceProfile;
}

/// Builds what the profile names but cannot download. Optional: unsupported
/// means the flavor has nothing to build. Idempotent and cheap once done, since
/// a client install runs on every launch: the guest checks its own outputs
/// under `/build` first. The guest writes only there; the host places each
/// output.
pub struct Build;
impl ProviderOp for Build {
    type Kind = Flavor;
    const OP: &'static str = "build";
    type Args = BuildQuery;
    type Reply = BuildDone;
}
