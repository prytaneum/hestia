//! An archive format identified by `replay.cfg`. The manifest is `key=value`
//! lines; `files/<path>` members land at `data/<path>`. Two values of `flag`
//! make it misbehave: `escape` answers a path outside the instance, `ghost`
//! names a member the archive does not hold.

use hestia::proto::format::{FormatBlueprint, FormatCopy, FormatInfo, FormatKind, FormatWritten};
use hestia::{Error, Plugin};

const FORMAT: &str = "replay";
const MARKER: &str = "replay.cfg";

struct Replay;

fn check(format: &str) -> Result<(), Error> {
    match format {
        FORMAT => Ok(()),
        other => Err(Error::NotFound(format!("unknown format `{other}`"))),
    }
}

impl Plugin for Replay {
    fn format_info(format: &str) -> Result<FormatInfo, Error> {
        check(format)?;
        Ok(FormatInfo {
            name: "Replay".into(),
            kind: FormatKind::Instance,
            marker: MARKER.into(),
        })
    }

    fn format_read(
        format: &str,
        manifest: &[u8],
        _files: &[String],
    ) -> Result<FormatBlueprint, Error> {
        check(format)?;
        let mut blueprint = FormatBlueprint::default();
        for line in String::from_utf8_lossy(manifest).lines() {
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

    fn format_land(
        format: &str,
        blueprint: FormatBlueprint,
        files: &[String],
    ) -> Result<Vec<FormatCopy>, Error> {
        check(format)?;
        let mut copies: Vec<FormatCopy> = files
            .iter()
            .filter_map(|file| file.strip_prefix("files/"))
            .map(|rest| FormatCopy {
                archive_path: format!("files/{rest}"),
                entry_path: rest.to_string(),
            })
            .collect();
        match blueprint.loader_version.as_str() {
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

    fn format_write(
        format: &str,
        blueprint: FormatBlueprint,
        files: &[String],
    ) -> Result<FormatWritten, Error> {
        check(format)?;
        let mapping = files
            .iter()
            .map(|file| FormatCopy {
                archive_path: format!("files/{file}"),
                entry_path: match blueprint.name.as_str() {
                    "escape" => "../evil.txt".into(),
                    _ => file.clone(),
                },
            })
            .collect();
        Ok(FormatWritten {
            manifest: format!("name={}\n", blueprint.name).into_bytes(),
            mapping,
        })
    }
}

hestia::plugin!(Replay);
