//! The `flavor` provider kind: a Minecraft distribution. A flavor decides the
//! code the game runs, so the host vets every profile before acting on it: each
//! artifact must carry a sha1 and sit on one of the plugin's network hosts.

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

/// The launch profile of a server. The jar must be a download: a plugin cannot
/// build one.
pub struct ResolveServer;
impl ProviderOp for ResolveServer {
    type Kind = Flavor;
    const OP: &'static str = "resolve-server";
    type Args = ResolveQuery;
    type Reply = ServerProfile;
}

/// The launch profile of a client, under the same rules as [`ResolveServer`].
pub struct ResolveClient;
impl ProviderOp for ResolveClient {
    type Kind = Flavor;
    const OP: &'static str = "resolve-client";
    type Args = ResolveQuery;
    type Reply = InstanceProfile;
}
