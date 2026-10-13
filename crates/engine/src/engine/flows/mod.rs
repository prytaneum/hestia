//! The cross-subsystem flows composed over the `Engine` aggregate — one module
//! per concern, each an `impl Engine` block, so the aggregate itself stays the
//! wiring and nothing more.

mod backup;
mod batch;
mod console;
mod content;
pub use content::entry::EntryRef;
mod flavors;
mod instance;

pub use instance::LaunchRequest;
mod modpack;

pub use modpack::ModpackOutcome;
mod multiplayer;

pub use multiplayer::ServerListWrite;
mod packs;
mod players;
mod recent;
mod screenshots;
mod server;
mod skins;
mod sync;
mod transfer;
mod update;

pub use transfer::{ExportOutcome, ImportOutcome};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

use super::Engine;
use crate::minecraft::materialize::OnProgress;

impl Engine {
    /// The installed runtime for `major`, installing it (through the cache) when
    /// missing.
    async fn ensure_java(&self, major: i32, on_progress: OnProgress<'_>) -> Result<PathBuf> {
        let detail = format!("java {major}");
        let outcome = self
            .java
            .install(
                major,
                false,
                Some(&self.cache),
                on_progress.cancel(),
                |jp| {
                    on_progress.report(&ProvisionProgress {
                        phase: ProvisionPhase::Java,
                        current: jp.current,
                        total: jp.total,
                        detail: detail.clone(),
                        ..ProvisionProgress::default()
                    });
                },
            )
            .await?;
        Ok(outcome.runtime.executable)
    }

    fn installed_java(&self, major: i32) -> Result<PathBuf> {
        self.java
            .installed()
            .into_iter()
            .find(|r| r.major == major)
            .map(|r| r.executable)
            .with_context(|| {
                format!("java {major} is not installed (run `hestia java install {major}`)")
            })
    }

    /// The stored record a reference names, as the typed refusal every flow
    /// answers an unknown instance with.
    fn instance_record(&self, reference: &str) -> Result<crate::instances::InstanceRecord> {
        self.instances.get(reference).ok_or_else(|| {
            proto::error::ErrorInfo::EntryNotFound {
                entry: proto::error::EntryKind::Instance,
                reference: reference.to_string(),
            }
            .into()
        })
    }
}

/// Reject an unconfirmed downgrade. The direction comes from the flavor's own
/// newest-first catalogue; a version the catalogue no longer lists is
/// undecidable and passes (the front-end still confirms what it can detect).
fn guard_downgrade(
    data: &str,
    name: &str,
    from: &str,
    to: &str,
    versions: &[proto::minecraft::GameVersion],
    allowed: bool,
) -> Result<()> {
    if !allowed && proto::minecraft::downgrade_between(versions, from, to) == Some(true) {
        anyhow::bail!(
            "moving '{name}' from {from} back to {to} is a downgrade, and Minecraft cannot \
             load {data} written by a newer version; confirm the downgrade to proceed"
        );
    }
    Ok(())
}

fn effective_name(name: &str, flavor: &str, version: &str) -> String {
    if name.trim().is_empty() {
        format!("{flavor}-{version}")
    } else {
        name.trim().to_string()
    }
}

/// The root for launcher-managed shared game files (versions, libraries,
/// assets, natives) — the Modrinth layout, keeping the data home itself to
/// user-facing entries and launcher internals. Also where a flavor that builds
/// its own artifacts works, since that build belongs to no single entry.
fn meta_dir(home: &Path) -> PathBuf {
    home.join("meta")
}

fn phase_progress(phase: ProvisionPhase) -> ProvisionProgress {
    ProvisionProgress {
        phase,
        ..ProvisionProgress::default()
    }
}
