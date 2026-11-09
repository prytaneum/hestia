//! Mojang's Bedrock Dedicated Server — a native executable in a zip, with no
//! Java and no jar. Server side only.

use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use proto::minecraft::{Artifact, Edition, GameVersion};

use super::materialize::OnProgress;
use super::meta::bedrock;
use super::profile::{ServerProfile, ServerRuntime};
use super::provider::{
    FlavorInfo, FlavorProvider, InstallRequest, ResolveRequest, Side, SERVER_ONLY,
};

pub(crate) mod nbt;
pub(crate) mod world;

const ID: &str = "bedrock";

#[cfg(windows)]
pub const EXECUTABLE: &str = "bedrock_server.exe";
#[cfg(not(windows))]
pub const EXECUTABLE: &str = "bedrock_server";

pub const STAMP: &str = ".bds-version";
const STAGING: &str = ".bds-staging";

/// Top-level entries an update never overwrites once they exist.
const USER_OWNED: &[&str] = &[
    "server.properties",
    "allowlist.json",
    "permissions.json",
    "worlds",
    "config",
];

const INFO: FlavorInfo = FlavorInfo {
    id: ID,
    name: "Vanilla",
    summary: "Mojang's Bedrock Dedicated Server, for players on phones, consoles and Windows. Runs without Java.",
    edition: Edition::Bedrock,
    loads: None,
    requires: &[],
    sides: SERVER_ONLY,
};

pub struct Bedrock;

fn runtime(version: &str, url: String) -> ServerRuntime {
    ServerRuntime::Native {
        archive: Artifact {
            url,
            filename: format!("bedrock-server-{version}.zip"),
            ..Artifact::default()
        },
        executable: EXECUTABLE.to_string(),
        env: if cfg!(target_os = "linux") {
            vec![("LD_LIBRARY_PATH".to_string(), ".".to_string())]
        } else {
            Vec::new()
        },
    }
}

#[async_trait]
impl FlavorProvider for Bedrock {
    fn info(&self) -> &FlavorInfo {
        &INFO
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        bedrock::versions().await
    }

    async fn resolve_server(&self, request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        let url = bedrock::download_url(&request.version).await?;
        Ok(ServerProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: None,
            runtime: runtime(&request.version, url),
        })
    }

    async fn install(
        &self,
        side: Side,
        request: &InstallRequest<'_>,
        on: OnProgress<'_>,
    ) -> Result<()> {
        if side != Side::Server {
            return Ok(());
        }
        let root = request.root;
        let version = request.game_version;
        if std::fs::read_to_string(root.join(STAMP)).is_ok_and(|s| s.trim() == version)
            && root.join(EXECUTABLE).is_file()
        {
            return Ok(());
        }
        on.check()?;
        let archive = root.join(format!("bedrock-server-{version}.zip"));
        let (target, zip) = (root.to_path_buf(), archive.clone());
        let schema = request.schema.map(Path::to_path_buf);
        tokio::task::spawn_blocking(move || unpack(&zip, &target, schema.as_deref()))
            .await
            .context("the Bedrock install task panicked")??;
        on.check()?;
        crate::content::bedrock::record_shipped(root);
        std::fs::write(root.join(STAMP), version).context("cannot write the install stamp")?;
        let _ = std::fs::remove_file(&archive);
        tracing::info!(version, "bedrock server installed");
        Ok(())
    }
}

/// Everything in the zip lands over `root` except the user-owned entries that
/// already exist; directories merge, so packs a user added survive an update.
///
/// The zip's own `server.properties` is copied to `schema` first: on an update
/// the live file is the user's, so the shipped one is only visible here.
fn unpack(archive: &Path, root: &Path, schema: Option<&Path>) -> Result<()> {
    let staging = root.join(STAGING);
    let _ = std::fs::remove_dir_all(&staging);
    let merged = crate::extract::extract_archive(archive, &staging).and_then(|()| {
        let shipped = staging.join("server.properties");
        if let (Some(schema), true) = (schema, shipped.is_file()) {
            std::fs::copy(&shipped, schema).context("cannot store the properties schema")?;
        }
        for entry in std::fs::read_dir(&staging)? {
            let name = entry?.file_name();
            let owned = name.to_str().is_some_and(|n| USER_OWNED.contains(&n));
            if !(owned && root.join(&name).exists()) {
                merge(&staging.join(&name), &root.join(&name))?;
            }
        }
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&staging);
    merged?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = root.join(EXECUTABLE);
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("cannot make {} executable", executable.display()))?;
    }
    Ok(())
}

fn merge(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            merge(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::rename(from, to).with_context(|| format!("cannot place {}", to.display()))
    }
}

/// Top-level names a backup leaves out and a restore carries over: the
/// executable, its shared libraries and the stamp, which belong together.
pub fn binaries(data: &Path) -> Vec<String> {
    let libraries = std::fs::read_dir(data)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".dll") || n.contains(".so"));
    [
        EXECUTABLE.to_string(),
        STAMP.to_string(),
        crate::content::bedrock::SHIPPED.to_string(),
    ]
    .into_iter()
    .chain(libraries)
    .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn zip_of(path: &Path, files: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn the_runtime_names_the_platform_executable() {
        let ServerRuntime::Native {
            archive,
            executable,
            env,
        } = runtime("1.21.0.3", "https://x/y.zip".into())
        else {
            panic!("bedrock is native");
        };
        assert_eq!(archive.filename, "bedrock-server-1.21.0.3.zip");
        assert!(archive.checksum.is_none());
        assert_eq!(executable, EXECUTABLE);
        assert_eq!(env.is_empty(), !cfg!(target_os = "linux"));
    }

    #[test]
    fn an_update_keeps_what_the_user_owns() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let zip = root.join("v1.zip");
        let schema = root.join("schema.properties");
        zip_of(
            &zip,
            &[
                (EXECUTABLE, "old"),
                ("server.properties", "default"),
                ("behavior_packs/vanilla/a.json", "1"),
            ],
        );
        unpack(&zip, root, Some(&schema)).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("server.properties")).unwrap(),
            "default"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(EXECUTABLE)).unwrap(),
            "old"
        );

        std::fs::write(root.join("server.properties"), "mine").unwrap();
        std::fs::create_dir_all(root.join("worlds/Bedrock level")).unwrap();
        std::fs::write(root.join("worlds/Bedrock level/db"), "world").unwrap();
        std::fs::create_dir_all(root.join("behavior_packs/custom")).unwrap();
        std::fs::write(root.join("behavior_packs/custom/p.json"), "pack").unwrap();
        zip_of(
            &zip,
            &[
                (EXECUTABLE, "new"),
                ("server.properties", "default"),
                ("worlds/ignored", "x"),
                ("behavior_packs/vanilla/a.json", "2"),
            ],
        );
        unpack(&zip, root, Some(&schema)).unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join(EXECUTABLE)).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("server.properties")).unwrap(),
            "mine"
        );
        assert!(root.join("worlds/Bedrock level/db").is_file());
        assert!(!root.join("worlds/ignored").exists());
        assert!(root.join("behavior_packs/custom/p.json").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("behavior_packs/vanilla/a.json")).unwrap(),
            "2"
        );
        assert!(!root.join(STAGING).exists());
        assert_eq!(std::fs::read_to_string(&schema).unwrap(), "default");
    }

    #[test]
    fn an_update_seeds_the_keys_it_introduces_and_keeps_the_users_values() {
        use crate::properties::{read_properties, read_property, seed_properties};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (zip, schema) = (root.join("bds.zip"), root.join("schema.properties"));
        std::fs::write(root.join("server.properties"), "gamemode=creative\n").unwrap();
        zip_of(
            &zip,
            &[
                (EXECUTABLE, "x"),
                ("server.properties", "gamemode=survival\nnew-key=on\n"),
            ],
        );
        unpack(&zip, root, Some(&schema)).unwrap();
        seed_properties(&root.join("server.properties"), &read_properties(&schema)).unwrap();

        let live = root.join("server.properties");
        assert_eq!(
            read_property(&live, "gamemode").as_deref(),
            Some("creative")
        );
        assert_eq!(read_property(&live, "new-key").as_deref(), Some("on"));
        assert_eq!(
            read_property(&schema, "gamemode").as_deref(),
            Some("survival")
        );
    }

    #[test]
    fn binaries_name_the_executable_and_libraries_only() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["libx.so", "y.dll", "server.properties"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        let names = binaries(dir.path());
        assert!(names.contains(&EXECUTABLE.to_string()));
        assert!(names.contains(&"libx.so".to_string()) && names.contains(&"y.dll".to_string()));
        assert!(!names.contains(&"server.properties".to_string()));
    }
}
