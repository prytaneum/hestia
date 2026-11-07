//! Archive formats contributed by plugins. A plugin's `archive-format` export
//! is wrapped in a [`PluginFormat`], which the transfer flow ranks beside the
//! core formats. The host owns every archive: the guest is handed the manifest
//! and the member names, and everything it answers is vetted here before a
//! byte is copied. An answer with one refused path is refused whole.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use proto::error::{ArchiveFault, ArchiveFormat, ErrorInfo};
use proto::format::{FormatBlueprint, FormatCopy, FormatInfo, FormatKind, FormatWritten};
use proto::plugin::{Manifest, FORMAT_PROVIDER};
use serde::de::DeserializeOwned;

use super::call_failed;
use crate::content::pack::is_safe_path;
use crate::engine::Engine;
use crate::plugins::chain::CallChain;
use crate::plugins::runtime::{LoadedPlugin, PROVIDER_TIMEOUT};
use crate::transfer::archive::Reader;
use crate::transfer::{hestia, pool, Blueprint, Descriptor, Landed, Recipe, Target};
use crate::{cancel::Job, jvm::JavaSettings};

/// The most manifest bytes a guest is handed.
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

/// One call of the `archive-format` export.
pub(in crate::plugins) enum FormatCall {
    Info,
    Read {
        manifest: Vec<u8>,
        files: Vec<String>,
    },
    Land {
        blueprint: String,
        files: Vec<String>,
    },
    Write {
        blueprint: String,
        files: Vec<String>,
    },
}

/// Why a guest's answer was refused.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error("`{0}` is not a path inside the instance")]
    Unsafe(String),
    #[error("`{0}` is not a member of the archive")]
    Unlisted(String),
    #[error("the manifest is over {MAX_MANIFEST_BYTES} bytes")]
    Oversized,
}

fn vet(
    copies: &[FormatCopy],
    listed: impl Fn(&FormatCopy) -> &str,
    files: &[String],
) -> Result<(), Refusal> {
    let known: HashSet<&str> = files.iter().map(String::as_str).collect();
    for copy in copies {
        for path in [&copy.archive_path, &copy.entry_path] {
            if !is_safe_path(path) {
                return Err(Refusal::Unsafe(path.clone()));
            }
        }
        if !known.contains(listed(copy)) {
            return Err(Refusal::Unlisted(listed(copy).to_string()));
        }
    }
    Ok(())
}

impl From<&FormatBlueprint> for Blueprint {
    fn from(wire: &FormatBlueprint) -> Self {
        Blueprint {
            descriptor: Descriptor {
                name: wire.name.clone(),
                game_version: wire.game_version.clone(),
                loader: wire.loader.clone(),
                loader_version: wire.loader_version.clone(),
            },
            recipe: Recipe {
                jvm: JavaSettings {
                    memory: wire.memory.clone(),
                    jvm_args: wire.jvm_args.clone(),
                },
                carried: None,
            },
        }
    }
}

/// A format a plugin contributes, registered as `<plugin>.<id>`.
pub struct PluginFormat {
    id: String,
    local: String,
    plugin: Arc<LoadedPlugin>,
    info: FormatInfo,
}

impl PluginFormat {
    /// Asks the guest to describe the format `local` of its manifest.
    pub async fn connect(plugin: Arc<LoadedPlugin>, local: &str) -> Result<Self> {
        let mut format = PluginFormat {
            id: format!("{}.{local}", plugin.id()),
            local: local.to_string(),
            info: FormatInfo::default(),
            plugin,
        };
        let info: FormatInfo = format.ask(FormatCall::Info).await?;
        if info.kind != FormatKind::Instance {
            return Err(format.failed("only instance formats are supported".into()));
        }
        if !is_safe_path(&info.marker) || info.marker == hestia::MANIFEST {
            return Err(format.failed(format!("`{}` is not a usable marker", info.marker)));
        }
        format.info = info;
        Ok(format)
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn info(&self) -> &FormatInfo {
        &self.info
    }

    async fn ask<T: DeserializeOwned>(&self, call: FormatCall) -> Result<T> {
        let reply = self
            .plugin
            .archive_format(&CallChain::default(), &self.local, call, PROVIDER_TIMEOUT)
            .await
            .map_err(|error| self.failed(error.to_string()))?;
        serde_json::from_str(&reply).map_err(|error| {
            tracing::warn!(plugin = self.plugin.id(), format = %self.local, %error, "malformed format reply");
            self.failed("returned an unreadable reply".into())
        })
    }

    fn failed(&self, detail: String) -> anyhow::Error {
        call_failed(
            self.plugin.id(),
            format!("format `{}`: {detail}", self.local),
        )
    }

    fn refused(&self, refusal: Refusal) -> anyhow::Error {
        tracing::warn!(plugin = self.plugin.id(), format = %self.local, %refusal, "format answer refused");
        self.failed(format!("answer refused: {refusal}"))
    }

    /// The archive's members under `prefix`, named relative to it.
    fn members(reader: &Reader, prefix: &str) -> Vec<String> {
        reader
            .names()
            .into_iter()
            .filter(|name| !name.ends_with('/'))
            .filter_map(|name| name.strip_prefix(prefix).map(str::to_string))
            .collect()
    }

    /// Hands the guest the manifest and the member names, and returns the
    /// instance it describes.
    pub(crate) async fn read(&self, reader: &mut Reader, prefix: &str) -> Result<FormatBlueprint> {
        let marker = format!("{prefix}{}", self.info.marker);
        let manifest = reader.read_capped(&marker, MAX_MANIFEST_BYTES).map_err(|error| {
            tracing::warn!(member = %marker, error = %format!("{error:#}"), "cannot read a format's manifest");
            ErrorInfo::ArchiveInvalid {
                format: ArchiveFormat::Zip,
                reason: ArchiveFault::ManifestMalformed,
            }
        })?;
        let files = Self::members(reader, prefix);
        self.ask(FormatCall::Read { manifest, files }).await
    }

    /// Asks the guest where the archive's files go and copies exactly those.
    pub(crate) async fn land(
        &self,
        reader: &mut Reader,
        prefix: &str,
        blueprint: &FormatBlueprint,
        target: &Target<'_>,
        job: &Job<'_>,
    ) -> Result<Landed> {
        let files = Self::members(reader, prefix);
        let copies: Vec<FormatCopy> = self
            .ask(FormatCall::Land {
                blueprint: serde_json::to_string(blueprint)?,
                files: files.clone(),
            })
            .await?;
        vet(&copies, |copy| &copy.archive_path, &files).map_err(|refusal| self.refused(refusal))?;
        for copy in &copies {
            job.check()?;
            let destination = target.data_dir.join(&copy.entry_path);
            reader
                .copy_member(&format!("{prefix}{}", copy.archive_path), &destination)
                .with_context(|| format!("cannot land {}", copy.archive_path))?;
        }
        let adopted = pool::adopt(target.entry_dir, target.data_dir)?;
        Ok(Landed {
            files: copies.len() as u64,
            adopted,
        })
    }

    /// Asks the guest for the manifest of an export and the files to carry.
    /// `files` are paths relative to the instance's game directory; the answer is
    /// vetted but the host's exporter does the writing.
    pub async fn write(
        &self,
        blueprint: &FormatBlueprint,
        files: Vec<String>,
    ) -> Result<FormatWritten> {
        let written: FormatWritten = self
            .ask(FormatCall::Write {
                blueprint: serde_json::to_string(blueprint)?,
                files: files.clone(),
            })
            .await?;
        if written.manifest.len() > MAX_MANIFEST_BYTES {
            return Err(self.refused(Refusal::Oversized));
        }
        vet(&written.mapping, |copy| &copy.entry_path, &files)
            .map_err(|refusal| self.refused(refusal))?;
        Ok(written)
    }
}

/// Connects every format the manifest contributes. One that fails to describe
/// itself is logged and left out; the plugin stays enabled.
pub async fn connect_formats(plugin: &Arc<LoadedPlugin>, manifest: &Manifest) -> Vec<PluginFormat> {
    let mut formats = Vec::new();
    for contribution in &manifest.contributes.providers {
        if contribution.kind != FORMAT_PROVIDER {
            continue;
        }
        match PluginFormat::connect(plugin.clone(), &contribution.id).await {
            Ok(format) => formats.push(format),
            Err(error) => tracing::warn!(
                plugin = plugin.id(),
                format = %contribution.id,
                error = %format!("{error:#}"),
                "format not registered"
            ),
        }
    }
    formats
}

/// Adds `formats` to `engine` under `owner`. `official` is whether the plugin's
/// key is the official one; see [`Roster::register`](crate::roster).
pub fn register_formats(engine: &Engine, owner: &str, official: bool, formats: Vec<PluginFormat>) {
    for format in formats {
        let id = format.id.clone();
        match engine
            .formats()
            .register(owner, official, &id, Arc::new(format))
        {
            Ok(()) => tracing::info!(plugin = owner, format = %id, "format registered"),
            Err(error) => tracing::warn!(plugin = owner, format = %id, %error, "format refused"),
        }
    }
}
