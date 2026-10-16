use proto::minecraft::Artifact;
use serde::{Deserialize, Serialize};

/// A classpath dependency, resolved to its download and its path under the
/// libraries root (Maven layout).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Library {
    pub name: String,
    pub path: String,
    pub artifact: Artifact,
}

/// A library the game loads through `java.library.path` rather than the
/// classpath: a pre-1.19 manifest ships one classifier jar per OS
/// (`downloads.classifiers["natives-linux"]`) for the launcher to unpack.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Native {
    pub library: Library,
    /// Archive path prefixes the manifest's `extract.exclude` names.
    pub exclude: Vec<String>,
}

/// The asset index a client version pins (`assetIndex` in a vanilla profile).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub artifact: Artifact,
    pub total_size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ServerJar {
    Download(Artifact),
    /// Named but not downloadable: the flavor's install hook builds this file.
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

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ServerLaunch {
    #[default]
    Jar,
    /// Relative to the data directory: the file names its libraries relatively.
    ArgsFile(String),
}

/// The resolved launch profile for a Minecraft *server*.
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

/// The resolved launch profile for a Minecraft *client* (instance).
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
