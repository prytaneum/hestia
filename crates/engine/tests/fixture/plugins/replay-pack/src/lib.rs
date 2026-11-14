//! A pack format identified by `replay.json`, whose members it answers almost
//! verbatim: the manifest names the files, the override trees and, optionally,
//! a content source to resolve a manifest through, and optionally its `memory`
//! and `jvmArgs`, so a test scripts every
//! answer a pack can give. An archive holding `replay.skip`, or whose
//! `replay.json` says `"decline": true`, is not claimed. A manifest naming a
//! `loader` or an `icon` has them answered, and one with `"echoLoaders": true`
//! is named for the loader keys the host offered. An export names every file of
//! the instance, plus one the export does not hold when the instance is called
//! `escape`, and stores what the host said of the content pool in `pool.json`.

use hestia::proto::content::{ModpackFile, ResolvedModpack};
use hestia::proto::download::{Checksum, HashAlgorithm};
use hestia::proto::minecraft::Artifact;
use hestia::proto::provider::pack::{
    Claim, ClaimQuery, Claims, Info, ManifestDelegate, OverrideTrees, PackCopy, PackInfo,
    PackMember, PackRead, PackWritten, Read, ReadQuery, Write, WriteQuery,
};
use hestia::serde_json::{self, Value};
use hestia::{Error, Plugin, Provide, Provided};

const PACK: &str = "replay";
const MARKER: &str = "replay.json";

struct Replay;

fn check(pack: &str) -> Result<(), Error> {
    match pack {
        PACK => Ok(()),
        other => Err(Error::NotFound(format!("unknown pack `{other}`"))),
    }
}

fn info(pack: &str) -> Result<PackInfo, Error> {
    check(pack)?;
    Ok(PackInfo {
        name: "Replay".into(),
        members: vec![MARKER.into()],
        extension: "replay".into(),
    })
}

fn claims(pack: &str, query: ClaimQuery) -> Result<Claim, Error> {
    check(pack)?;
    let declined = query
        .members
        .get(MARKER)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .is_some_and(|manifest| manifest["decline"] == true);
    Ok(Claim {
        claimed: !declined && !query.files.iter().any(|file| file == "replay.skip"),
    })
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

fn read(pack: &str, query: ReadQuery) -> Result<PackRead, Error> {
    check(pack)?;
    let manifest: Value = serde_json::from_str(
        query
            .members
            .get(MARKER)
            .ok_or_else(|| Error::Invalid("the archive has no replay.json".into()))?,
    )?;
    let files = manifest["files"]
        .as_array()
        .map(|entries| entries.iter().map(file).collect())
        .unwrap_or_default();
    let trees = &manifest["trees"];
    let name = match manifest["echoLoaders"] == true {
        true => query.loaders.join(","),
        false => text(&manifest, "name"),
    };
    Ok(PackRead {
        pack: ResolvedModpack {
            name,
            version_number: text(&manifest, "versionId"),
            game_version: text(&manifest, "gameVersion"),
            loader: manifest["loader"].as_str().map(str::to_string),
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
    })
}

fn write(pack: &str, query: WriteQuery) -> Result<PackWritten, Error> {
    check(pack)?;
    let mut copies: Vec<PackCopy> = query
        .files
        .iter()
        .chain(query.content.iter().map(|item| &item.path))
        .map(|path| PackCopy {
            entry_path: path.clone(),
            archive_path: format!("overrides/{path}"),
        })
        .collect();
    if query.instance.name == "escape" {
        copies.push(PackCopy {
            entry_path: "../../etc/passwd".into(),
            archive_path: "overrides/passwd".into(),
        });
    }
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

impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|pack, _| info(pack))
            .on::<Claims>(claims)
            .on::<Read>(read)
            .on::<Write>(write)
            .finish()
    }
}

hestia::plugin!(Replay);
