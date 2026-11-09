use proto::minecraft::{Artifact, Edition};
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
    pub runtime: ServerRuntime,
}

/// How a server runs: on a Java runtime from a jar, or as a native executable
/// unpacked from a downloaded archive. A variant, so Java-only machinery cannot
/// be applied to a server that has no Java.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerRuntime {
    Java {
        jar: ServerJar,
        launch: ServerLaunch,
        java_major: i32,
        /// The flavor's own recommended JVM flags, used only when the entry
        /// sets no `jvm-args` of its own and no global default exists.
        jvm_args: Vec<String>,
    },
    Native {
        archive: Artifact,
        /// Relative to the data directory.
        executable: String,
        env: Vec<(String, String)>,
    },
}

impl Default for ServerRuntime {
    fn default() -> Self {
        ServerRuntime::Java {
            jar: ServerJar::default(),
            launch: ServerLaunch::default(),
            java_major: 0,
            jvm_args: Vec::new(),
        }
    }
}

impl ServerRuntime {
    pub fn edition(&self) -> Edition {
        match self {
            ServerRuntime::Java { .. } => Edition::Java,
            ServerRuntime::Native { .. } => Edition::Bedrock,
        }
    }

    pub fn is_native(&self) -> bool {
        matches!(self, ServerRuntime::Native { .. })
    }

    pub fn java_major(&self) -> Option<i32> {
        match self {
            ServerRuntime::Java { java_major, .. } => Some(*java_major),
            ServerRuntime::Native { .. } => None,
        }
    }

    /// The file a version change replaces: the jar, or the archive.
    pub fn filename(&self) -> &str {
        match self {
            ServerRuntime::Java { jar, .. } => jar.filename(),
            ServerRuntime::Native { archive, .. } => &archive.filename,
        }
    }
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
