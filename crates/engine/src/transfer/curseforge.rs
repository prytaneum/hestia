//! The CurseForge pack format, as an instance archive. `manifest.json` is a
//! name any archive may carry, so [`Format::claims`] reads it before deciding.

use anyhow::Result;
use proto::transfer::ImportFormat;
use serde_json::Value;

use super::archive::Reader;
use super::{Blueprint, Descriptor, Format, Landed, Recipe, Target};
use crate::cancel::Job;
use crate::content::curseforge;

const MANIFEST: &str = "manifest.json";

pub(crate) struct CurseForge;

fn manifest(reader: &mut Reader, prefix: &str) -> Result<Value> {
    let text = reader.read_text(&format!("{prefix}{MANIFEST}"))?;
    serde_json::from_str(&text).map_err(|e| {
        proto::error::ErrorInfo::ArchiveInvalid {
            format: "curseforge".to_string(),
            detail: format!("{MANIFEST} is malformed: {e}"),
        }
        .into()
    })
}

impl Format for CurseForge {
    fn id(&self) -> ImportFormat {
        ImportFormat::CurseForge
    }

    fn marker(&self) -> &'static str {
        MANIFEST
    }

    fn claims(&self, reader: &mut Reader, prefix: &str) -> bool {
        manifest(reader, prefix).is_ok_and(|manifest| curseforge::is_pack_manifest(&manifest))
    }

    fn read(&self, reader: &mut Reader, prefix: &str) -> Result<Blueprint> {
        let pack = curseforge::parse_manifest(&manifest(reader, prefix)?)?;
        Ok(Blueprint {
            descriptor: Descriptor {
                name: pack.name,
                game_version: pack.game_version,
                loader: pack.loader.unwrap_or_default(),
                loader_version: pack.loader_version.unwrap_or_default(),
            },
            recipe: Recipe::Pack,
        })
    }

    fn land(
        &self,
        _reader: &mut Reader,
        _prefix: &str,
        _target: &Target<'_>,
        _job: &Job<'_>,
    ) -> Result<Landed> {
        Ok(Landed::default())
    }
}
