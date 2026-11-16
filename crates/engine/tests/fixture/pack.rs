//! A native pack format identified by `replay.json`, whose members it answers
//! almost verbatim: the manifest names the files, the override trees and,
//! optionally, a content source to resolve a manifest through, plus its
//! `memory`, `jvmArgs`, `loader` and `icon`, so a test scripts every answer a
//! pack can give. An archive holding `replay.skip`, or whose manifest says
//! `"decline": true`, is not claimed. An export names every file of the
//! instance and stores what the host said of the content pool in `pool.json`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use engine::{Hosts, Loaders, PackProvider, Policy, Reader, Vetted};
use proto::content::{ModpackFile, ResolvedModpack};
use proto::download::{Checksum, HashAlgorithm};
use proto::minecraft::Artifact;
use proto::provider::pack::{
    ManifestDelegate, OverrideTrees, PackCopy, PackInfo, PackMember, PackRead, PackWritten,
    WriteQuery,
};
use serde_json::Value;

pub const OWNER: &str = "replay-pack";
pub const ID: &str = "replay-pack.replay";
const MARKER: &str = "replay.json";

pub struct Replay {
    info: PackInfo,
    policy: Arc<Policy>,
}

impl Replay {
    pub fn new() -> Replay {
        let hosts = Hosts {
            connect: vec!["127.0.0.1".into()],
            unhashed: Vec::new(),
        };
        Replay {
            info: PackInfo {
                name: "Replay".into(),
                members: vec![MARKER.into()],
                extension: "replay".into(),
            },
            policy: Arc::new(Policy::new(&hosts)),
        }
    }
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

fn list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn file(entry: &Value) -> ModpackFile {
    let path = text(entry, "path");
    ModpackFile {
        artifact: Artifact {
            url: text(entry, "url"),
            filename: path.rsplit('/').next().unwrap_or_default().to_string(),
            checksum: entry["sha1"].as_str().map(|hex| Checksum {
                algorithm: HashAlgorithm::Sha1,
                hex: hex.to_string(),
            }),
            ..Artifact::default()
        },
        path,
        ..ModpackFile::default()
    }
}

fn manifest(reader: &mut Reader, root: &str) -> Result<Value> {
    let bytes = reader.read_capped(&format!("{root}{MARKER}"), 1024 * 1024)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[async_trait]
impl PackProvider for Replay {
    fn id(&self) -> &str {
        ID
    }

    fn info(&self) -> &PackInfo {
        &self.info
    }

    fn policy(&self) -> Arc<Policy> {
        self.policy.clone()
    }

    fn root_of(&self, names: &[String]) -> Option<String> {
        names
            .iter()
            .filter_map(|name| name.strip_suffix(MARKER))
            .min_by_key(|root| root.matches('/').count())
            .map(str::to_string)
    }

    async fn claims(&self, reader: &mut Reader, root: &str) -> bool {
        let skipped = reader
            .names()
            .iter()
            .any(|name| name == &format!("{root}replay.skip"));
        let declined = manifest(reader, root)
            .map(|manifest| manifest["decline"] == true)
            .unwrap_or(false);
        !skipped && !declined
    }

    async fn read(&self, reader: &mut Reader, root: &str, loaders: &Loaders) -> Result<Vetted> {
        let manifest = manifest(reader, root)?;
        let files = manifest["files"]
            .as_array()
            .map(|entries| entries.iter().map(file).collect())
            .unwrap_or_default();
        let trees = &manifest["trees"];
        let name = match manifest["echoLoaders"] == true {
            true => loaders.keys_of(ID).join(","),
            false => text(&manifest, "name"),
        };
        let loader = manifest["loader"]
            .as_str()
            .map(|key| loaders.flavor_of(ID, key).unwrap_or(key).to_string());
        Ok(Vetted {
            read: PackRead {
                pack: ResolvedModpack {
                    name,
                    version_number: text(&manifest, "versionId"),
                    game_version: text(&manifest, "gameVersion"),
                    loader,
                    files,
                    ..ResolvedModpack::default()
                },
                resolve: manifest.get("resolve").map(|resolve| ManifestDelegate {
                    source: text(resolve, "source"),
                    manifest: resolve["manifest"].clone(),
                }),
                overrides: OverrideTrees {
                    shared: list(&trees["shared"]),
                    client: list(&trees["client"]),
                    server: list(&trees["server"]),
                },
                memory: manifest["memory"].as_str().map(str::to_string),
                jvm_args: manifest["jvmArgs"].as_str().map(str::to_string),
                icon: manifest["icon"].as_str().map(str::to_string),
            },
            warnings: Vec::new(),
        })
    }

    async fn write(&self, query: &WriteQuery) -> Result<PackWritten> {
        let copies = query
            .files
            .iter()
            .chain(query.content.iter().map(|item| &item.path))
            .map(|path| PackCopy {
                entry_path: path.clone(),
                archive_path: format!("overrides/{path}"),
            })
            .collect();
        let pool: Vec<Value> = query
            .content
            .iter()
            .map(|item| {
                serde_json::json!({
                    "path": item.path,
                    "referenceable": item.referenceable,
                    "size": item.size,
                    "sha512": item.sha512,
                })
            })
            .collect();
        Ok(PackWritten {
            members: vec![
                PackMember {
                    path: MARKER.into(),
                    text: format!("{{\"name\":\"{}\"}}", query.instance.name),
                },
                PackMember {
                    path: "pool.json".into(),
                    text: Value::Array(pool).to_string(),
                },
            ],
            copies,
            warnings: vec!["replay keeps no references to content".into()],
        })
    }
}
