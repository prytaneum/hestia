//! The `flavor` provider kind: a Minecraft distribution. A flavor decides the
//! code the game runs, so the host vets every profile before acting on it: each
//! artifact must carry a sha1 and sit on one of the plugin's network hosts, or
//! be named as built: a flavor's optional `build` produces those files locally,
//! in an area of its own, and the host places them.
//!
//! What a flavor adds to the vanilla profile may be declarative: a library as a
//! Maven coordinate the host turns into a path, a URL and a checksum, and
//! libraries and arguments gated by Mojang-style rules the host evaluates. The
//! host resolves all of it before vetting, so what persists is always the plain
//! [`Library`].

use serde::{Deserialize, Serialize};

use super::{ProviderKind, ProviderOp};
use crate::contract::Empty;
use crate::minecraft::GameVersion;
use crate::plugin::Permission;
use crate::profile::{FlavorDescription, InstanceProfile, Library, ServerJar, ServerProfile};
use crate::rules::Rule;

pub struct Flavor;
impl ProviderKind for Flavor {
    const KIND: &'static str = "flavor";
    const PERMISSIONS: &'static [Permission] =
        &[Permission::FlavorProvide, Permission::NetworkConnect];
}

/// What the host knows of the machine and the Java the game will run on, so a
/// guest never probes the platform. Filled in by the host on every query that
/// carries it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Target {
    /// `windows`, `osx` or `linux`: the vocabulary of a rule's `os.name`.
    pub os: String,
    /// `x86`, `x86_64` or `arm64`: the vocabulary of a rule's `os.arch`.
    pub arch: String,
    /// The Java major the host runs this version with, when known. A `build`
    /// knows it; a `resolve` has not yet resolved the vanilla profile that
    /// decides it, so it is absent there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_major: Option<i32>,
    /// What joins the entries of a classpath: `:` or `;`.
    pub path_separator: String,
}

/// One game version.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct GameQuery {
    pub game: String,
    pub target: Target,
}

/// A game version and, for a flavor with a loader, the loader build.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ResolveQuery {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub target: Target,
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
    pub target: Target,
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

/// A library named by its Maven coordinate: the host derives the path under the
/// libraries root and the download URL from the coordinate and the repository.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Maven {
    /// `group:artifact:version[:classifier][@ext]`; the extension defaults to `jar`.
    pub coordinate: String,
    /// The repository's base URL, with or without a trailing slash.
    pub repository: String,
    /// The repository publishes `<file>.sha1` beside each file. When the
    /// library names no checksum, the host fetches it, through the plugin's own
    /// network grant, and verifies the download against it.
    pub sha1_sidecar: bool,
}

/// A library a flavor adds: the plain [`Library`], or a [`Maven`] one the host
/// completes (`name`, `path`, the artifact's `url` and `filename` stay empty
/// then, and a given `artifact.checksum` outranks the sidecar), gated by
/// `rules`. A library the rules disallow on this machine is dropped.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct LibrarySpec {
    #[serde(flatten)]
    pub library: Library,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maven: Option<Maven>,
    pub rules: Vec<Rule>,
}

impl From<Library> for LibrarySpec {
    fn from(library: Library) -> Self {
        LibrarySpec {
            library,
            ..LibrarySpec::default()
        }
    }
}

/// Launch arguments gated by `rules`; dropped when they disallow this machine.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct RuledArgs {
    pub value: Vec<String>,
    pub rules: Vec<Rule>,
}

/// One launch argument: a plain string, or [`RuledArgs`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Ruled(RuledArgs),
}

impl From<&str> for Argument {
    fn from(value: &str) -> Self {
        Argument::Plain(value.to_string())
    }
}

impl From<String> for Argument {
    fn from(value: String) -> Self {
        Argument::Plain(value)
    }
}

/// What a flavor adds to the vanilla client of the requested game version. The
/// host resolves the vanilla profile itself and merges these over it: a library
/// replaces a vanilla one of the same Maven coordinate, the arguments follow
/// vanilla's, and a non-empty `main_class` replaces vanilla's.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ClientAddition {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub libraries: Vec<LibrarySpec>,
    pub main_class: String,
    pub jvm_args: Vec<Argument>,
    pub game_args: Vec<Argument>,
}

/// What a flavor adds to the vanilla server of the requested game version. The
/// host takes the Java major from vanilla; `jar` replaces vanilla's server jar
/// and is absent for a flavor that runs it unchanged.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerAddition {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jar: Option<ServerJar>,
    pub jvm_args: Vec<Argument>,
}

/// A client profile in full, or built on the vanilla one.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ClientResolution {
    Full(Box<InstanceProfile>),
    OnVanilla(ClientAddition),
}

/// A server profile in full, or built on the vanilla one.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ServerResolution {
    Full(Box<ServerProfile>),
    OnVanilla(ServerAddition),
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

/// The launch profile of a server, in full or as what it adds to vanilla's. The
/// jar is a download or named as built.
pub struct ResolveServer;
impl ProviderOp for ResolveServer {
    type Kind = Flavor;
    const OP: &'static str = "resolve-server";
    type Args = ResolveQuery;
    type Reply = ServerResolution;
}

/// The launch profile of a client, under the same rules as [`ResolveServer`]; a
/// library may be named as built.
pub struct ResolveClient;
impl ProviderOp for ResolveClient {
    type Kind = Flavor;
    const OP: &'static str = "resolve-client";
    type Args = ResolveQuery;
    type Reply = ClientResolution;
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
