//! A pack format identified by `replay.json`, whose members it answers almost
//! verbatim: the manifest names the files, the override trees and, optionally,
//! a content source to resolve a manifest through, so a test scripts every
//! answer a pack can give. An archive holding `replay.skip` is not claimed. An
//! export names every file of the instance, plus one the export does not hold
//! when the instance is called `escape`.

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
    Ok(Claim {
        claimed: !query.files.iter().any(|file| file == "replay.skip"),
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
    Ok(PackRead {
        pack: ResolvedModpack {
            name: text(&manifest, "name"),
            game_version: text(&manifest, "gameVersion"),
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
    Ok(PackWritten {
        members: vec![PackMember {
            path: MARKER.into(),
            text: format!("{{\"name\":\"{}\"}}", query.instance.name),
        }],
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
