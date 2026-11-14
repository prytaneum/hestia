//! An archive format identified by `replay.cfg`. The manifest is `key=value`
//! lines; `files/<path>` members land at `data/<path>`. Two values of `flag`
//! make it misbehave: `escape` answers a path outside the instance, `ghost`
//! names a member the archive does not hold.

use hestia::proto::format::{FormatBlueprint, FormatCopy, FormatInfo, FormatKind, FormatWritten};
use hestia::proto::provider::format::{BlueprintQuery, Info, Land, ManifestQuery, Read, Write};
use hestia::{Error, Plugin, Provide, Provided};

const FORMAT: &str = "replay";
const MARKER: &str = "replay.cfg";

struct Replay;

fn check(format: &str) -> Result<(), Error> {
    match format {
        FORMAT => Ok(()),
        other => Err(Error::NotFound(format!("unknown format `{other}`"))),
    }
}

fn info(format: &str) -> Result<FormatInfo, Error> {
    check(format)?;
    Ok(FormatInfo {
        name: "Replay".into(),
        kind: FormatKind::Instance,
        marker: MARKER.into(),
    })
}

fn read(format: &str, query: ManifestQuery) -> Result<FormatBlueprint, Error> {
    check(format)?;
    let mut blueprint = FormatBlueprint::default();
    for line in query.manifest.lines() {
        match line.split_once('=') {
            Some(("name", value)) => blueprint.name = value.into(),
            Some(("version", value)) => blueprint.game_version = value.into(),
            Some(("memory", value)) => blueprint.memory = Some(value.into()),
            Some(("flag", value)) => blueprint.loader_version = value.into(),
            _ => {}
        }
    }
    Ok(blueprint)
}

fn land(format: &str, query: BlueprintQuery) -> Result<Vec<FormatCopy>, Error> {
    check(format)?;
    let mut copies: Vec<FormatCopy> = query
        .files
        .iter()
        .filter_map(|file| file.strip_prefix("files/"))
        .map(|rest| FormatCopy {
            archive_path: format!("files/{rest}"),
            entry_path: rest.to_string(),
        })
        .collect();
    match query.blueprint.loader_version.as_str() {
        "escape" => copies.push(FormatCopy {
            archive_path: "files/options.txt".into(),
            entry_path: "../evil.txt".into(),
        }),
        "ghost" => copies.push(FormatCopy {
            archive_path: "files/ghost.txt".into(),
            entry_path: "ghost.txt".into(),
        }),
        _ => {}
    }
    Ok(copies)
}

fn write(format: &str, query: BlueprintQuery) -> Result<FormatWritten, Error> {
    check(format)?;
    let mapping = query
        .files
        .iter()
        .map(|file| FormatCopy {
            archive_path: format!("files/{file}"),
            entry_path: match query.blueprint.name.as_str() {
                "escape" => "../evil.txt".into(),
                _ => file.clone(),
            },
        })
        .collect();
    Ok(FormatWritten {
        manifest: format!("name={}\n", query.blueprint.name).into_bytes(),
        mapping,
    })
}

impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|format, _| info(format))
            .on::<Read>(read)
            .on::<Land>(land)
            .on::<Write>(write)
            .finish()
    }
}

hestia::plugin!(Replay);
