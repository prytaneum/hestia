use crate::content::ContentKind;
use crate::minecraft::Artifact;
use serde::{Deserialize, Serialize};

/// A classpath dependency, resolved to its download and its path under the
/// libraries root (Maven layout).
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Library {
    pub name: String,
    pub path: String,
    pub artifact: Artifact,
    /// Produced locally by the flavor's build rather than downloaded: the
    /// artifact names nothing to fetch, and the file must exist once the build
    /// has run.
    pub built: bool,
    /// Whether the library is on the game's `-cp`. One that is not is still
    /// materialised, for a loader that finds it by path.
    pub classpath: bool,
}

impl Default for Library {
    fn default() -> Self {
        Library {
            name: String::new(),
            path: String::new(),
            artifact: Artifact::default(),
            built: false,
            classpath: true,
        }
    }
}

/// A library the game loads through `java.library.path` rather than the
/// classpath: a pre-1.19 manifest ships one classifier jar per OS
/// (`downloads.classifiers["natives-linux"]`) for the launcher to unpack.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Native {
    pub library: Library,
    /// Archive path prefixes the manifest's `extract.exclude` names.
    pub exclude: Vec<String>,
}

/// The asset index a client version pins (`assetIndex` in a vanilla profile).
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub artifact: Artifact,
    pub total_size: u64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ServerJar {
    Download(Artifact),
    /// Named but not downloadable: the flavor's `build` produces this file.
    Built {
        filename: String,
    },
}

impl ServerJar {
    pub fn filename(&self) -> &str {
        match self {
            ServerJar::Download(artifact) => &artifact.filename,
            ServerJar::Built { filename } => filename,
        }
    }
}

impl Default for ServerJar {
    fn default() -> Self {
        ServerJar::Download(Artifact::default())
    }
}

/// The argument file a server launches from, with the files other operating
/// systems use in its place. Every path is relative to the data directory: the
/// file names its libraries relatively.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ArgsFile {
    pub path: String,
    pub alternatives: Vec<OsArgsFile>,
}

/// The argument file for one operating system, named as a rule's `os.name`
/// (`windows`, `osx` or `linux`).
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct OsArgsFile {
    pub os: String,
    pub path: String,
}

impl ArgsFile {
    /// The file to launch from on `os`: its alternative, else the default.
    pub fn for_os(&self, os: &str) -> &str {
        self.alternatives
            .iter()
            .find(|alternative| alternative.os == os)
            .map_or(self.path.as_str(), |alternative| alternative.path.as_str())
    }

    /// Every file the profile may launch from.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.path.as_str()).chain(
            self.alternatives
                .iter()
                .map(|alternative| alternative.path.as_str()),
        )
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ServerLaunch {
    #[default]
    Jar,
    ArgsFile(ArgsFile),
}

/// The resolved launch profile for a Minecraft *server*.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerProfile {
    pub flavor: String,
    pub game_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub jar: ServerJar,
    pub launch: ServerLaunch,
    pub java_major: i32,
    /// The flavor's own recommended JVM flags, used only when the entry sets no
    /// `jvm-args` of its own and no global default exists.
    pub jvm_args: Vec<String>,
}

impl ServerProfile {
    /// The files the profile names as built: the jar's filename when it is.
    pub fn built(&self) -> Vec<&str> {
        match &self.jar {
            ServerJar::Built { filename } => vec![filename],
            ServerJar::Download(_) => Vec::new(),
        }
    }
}

/// The resolved launch profile for a Minecraft *client* (instance).
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceProfile {
    pub flavor: String,
    pub game_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub client: Artifact,
    pub libraries: Vec<Library>,
    /// Empty from 1.19 on, where natives are ordinary rule-gated libraries.
    pub natives: Vec<Native>,
    pub asset_index: AssetIndex,
    pub java_major: i32,
    pub main_class: String,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

impl InstanceProfile {
    /// The paths of the libraries the profile names as built.
    pub fn built(&self) -> Vec<&str> {
        self.libraries
            .iter()
            .chain(self.natives.iter().map(|native| &native.library))
            .filter(|library| library.built)
            .map(|library| library.path.as_str())
            .collect()
    }
}

/// What a plugin's flavor says of itself: the `info` of the `flavor` export.
/// The host registers it as `<plugin>.<id>`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct FlavorDescription {
    pub name: String,
    pub summary: String,
    /// The content kind the flavor's own loader consumes; none for a flavor that loads nothing.
    pub loads: Option<ContentKind>,
    pub server: bool,
    pub client: bool,
    /// Ids of the plugin's own `[[programs]]` the flavor needs on this computer
    /// before it can build or install. The host checks them when it lists the
    /// flavor, so a front-end can say what is missing before a build starts.
    pub requires: Vec<String>,
    /// The key under which a Modrinth pack (`.mrpack`) names this flavor's
    /// loader in its `dependencies`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mrpack_key: Option<String>,
    /// The component uid under which a Prism or MultiMC instance names this
    /// flavor's loader.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prism_uid: Option<String>,
}
