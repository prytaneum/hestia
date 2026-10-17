//! The canonical daemon warning — a degraded outcome of an operation that
//! nonetheless succeeded. Structured exactly like [`crate::error::ErrorInfo`]:
//! nobody authors prose at a call site, the English text (`Display`) is a
//! projection of the variant, and a front-end renders its own localized string
//! from the tag + typed fields.
//!
//! A warning exists because the alternative is a daemon log line, which the
//! person who ran the operation never sees — so an operation that half-worked
//! reported unqualified success. Anything the user would want to know about a
//! result belongs here, on the result.
//!
//! Every variant carries a [`WarningInfo::hint`] beside its headline: a
//! degraded outcome the user cannot act on is just noise, so the remediation is
//! part of the type rather than something each front-end invents.

use std::fmt;

use serde::{Deserialize, Serialize};

/// One degraded outcome. The `kind` tag is the wire discriminant; front-ends
/// switch on it exhaustively.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WarningInfo {
    /// The schema-generation run produced nothing, so this server's property
    /// keys cannot be validated — every unmanaged key will be accepted.
    PropertiesSchemaMissing { name: String },
    /// A synced unit was left instance-local because this instance's game
    /// version is older than the unit needs: the file either does not exist at
    /// that version or is written in a spelling the other instances corrupt.
    /// `requires` is the version that would share it.
    SyncUnitUnsupported {
        instance: String,
        unit: crate::sync::SyncUnit,
        requires: String,
    },
    /// A synced unit could not be reconciled at all, so this launch runs on
    /// whatever the instance already had. `detail` is operational English,
    /// shown as secondary text.
    SyncUnitSkipped {
        unit: crate::sync::SyncUnit,
        detail: String,
    },
    /// Game-directory files the pack owns were left as the user edited them, so
    /// this entry is not running the pack's own configuration for them.
    /// `count` alongside `paths` so the headline is one interpolation rather
    /// than a joined list a front-end would have to shorten itself.
    ModpackOverridesKept { count: u32, paths: Vec<String> },
    /// The pack declared client-only mods as server-compatible, so they were
    /// held back — a correction over the pack's own `env`, which packs get
    /// wrong routinely. Named individually: which mod was dropped is exactly
    /// what someone debugging a pack needs, and the list is short.
    ModpackFilesExcluded { count: u32, files: Vec<String> },
    /// Pool items an `.mrpack` export could not name as downloads — a local
    /// import, or a file from a source the pack format cannot reference — so
    /// they were embedded in the archive instead. The export is complete and
    /// installs correctly; it is only no longer a pack Modrinth would accept
    /// for publishing.
    ExportFilesEmbedded { count: u32, files: Vec<String> },
    /// Content an import took from the archive's own bytes rather than from a
    /// source: it is installed and loads, but carries no provenance, so it can
    /// never be updated in place.
    ImportFilesUntracked { count: u32, files: Vec<String> },
    /// Tracked content the archive neither carried nor named a download for, so
    /// its record was dropped: the instance imports with the content list it
    /// actually has.
    ImportContentMissing { count: u32, files: Vec<String> },
    /// A file hestia keeps could not be read as the document it should be —
    /// written by a newer build, hand-edited into something that no longer
    /// decodes, or damaged — so it was renamed aside and whatever it held is
    /// back at its defaults. `path` is where the original is now, because the
    /// only useful thing about an unreadable file is where to find it.
    DocumentQuarantined {
        document: String,
        path: String,
        /// Operational English naming the schema problem, shown as secondary
        /// text: the version it declared, or why it would not parse.
        detail: String,
    },
    /// The multiplayer list was edited while a session had the instance open.
    /// `servers.dat` belongs to the running game, which holds the list in
    /// memory and writes the whole file back when it exits — so that copy wins
    /// and this edit is lost with it.
    ServerListInUse { instance: String, sessions: u32 },
    /// The session was launched offline on purpose, so it carries no usable
    /// token.
    LaunchedOffline { account: String },
    /// The account's token was due for rotation and Microsoft could not be
    /// reached, so the session runs on the last one the launcher holds.
    SessionNotVerified { account: String },
    /// The update finished (or failed) while the server was stopped for it, and
    /// starting it again failed. `detail` is operational English, shown as
    /// secondary text.
    ServerNotRestarted { server: String, detail: String },
    /// A version change left the entry's modpack where it was because the pack
    /// has no version built for the new game version.
    PackNotMoved {
        pack: String,
        version_number: String,
        pack_game_version: String,
        target: String,
    },
}

impl WarningInfo {
    /// What the user can do about it — the whole point of surfacing a warning
    /// rather than logging it. Rendered as secondary text under the headline;
    /// a front-end localizes it from the same tag + fields.
    pub fn hint(&self) -> String {
        use WarningInfo::*;
        match self {
            PropertiesSchemaMissing { name } => format!(
                "any key is accepted until it can be derived again, so check spelling yourself; \
                 `hestia server {name} update <version>` re-derives it"
            ),
            SyncUnitUnsupported { unit, requires, .. } => format!(
                "{unit} stays this instance's own — sharing it needs Minecraft {requires} or newer"
            ),
            SyncUnitSkipped { unit, .. } => {
                format!("check the instance's file permissions, then launch again to share {unit}")
            }
            ModpackOverridesKept { .. } => {
                "delete a file under `data/` to take the pack's version of it at the next update"
                    .to_string()
            }
            ModpackFilesExcluded { .. } => {
                "they would break a server; `config set modpack.force-include-files <name>` \
                 installs one anyway, and `modpack.default-excludes false` trusts the pack"
                    .to_string()
            }
            ExportFilesEmbedded { .. } => {
                "nothing to do unless you mean to publish the pack — reinstall those files from a \
                 source first if you do"
                    .to_string()
            }
            ImportFilesUntracked { .. } => {
                "reinstall them from a source (`mod add <name>`) to make them updatable".to_string()
            }
            ImportContentMissing { .. } => {
                "install them again from a source (`mod add <name>`) if you still want them"
                    .to_string()
            }
            ServerListInUse { .. } => {
                "close the game, then make the change again — or add the server from the in-game \
                 multiplayer screen instead"
                    .to_string()
            }
            LaunchedOffline { .. } | SessionNotVerified { .. } => {
                "singleplayer works as usual; launch again once you are back online to play \
                 multiplayer"
                    .to_string()
            }
            ServerNotRestarted { server, .. } => {
                format!("start it yourself once the cause is fixed: `hestia start {server}`")
            }
            PackNotMoved { .. } => {
                "its mods may not load; move the pack yourself when a build for this version is published"
                    .to_string()
            }
            DocumentQuarantined { path, .. } => format!(
                "nothing was deleted — the file is at `{path}`; update hestia if it came from a \
                 newer version, or delete it once you have what you need from it"
            ),
        }
    }
}

impl fmt::Display for WarningInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use WarningInfo::*;
        match self {
            PropertiesSchemaMissing { name } => write!(
                f,
                "'{name}' has no property schema: its server.properties keys cannot be validated"
            ),
            SyncUnitUnsupported {
                instance,
                unit,
                requires,
            } => write!(
                f,
                "'{instance}' cannot share {unit}: that needs Minecraft {requires} or newer"
            ),
            SyncUnitSkipped { unit, detail } => {
                write!(f, "{unit} could not be synced: {detail}")
            }
            ModpackOverridesKept { count, .. } => write!(
                f,
                "{count} file(s) you had edited were kept instead of the modpack's"
            ),
            ModpackFilesExcluded { count, .. } => write!(
                f,
                "{count} client-only mod(s) the pack called server-compatible were not installed"
            ),
            ExportFilesEmbedded { count, .. } => write!(
                f,
                "{count} file(s) had no download to reference and were embedded in the archive"
            ),
            ImportFilesUntracked { count, .. } => write!(
                f,
                "{count} file(s) came from the archive itself, so they cannot be updated"
            ),
            ImportContentMissing { count, .. } => write!(
                f,
                "{count} file(s) were not in the archive and had no download, so they were left out"
            ),
            ServerListInUse { instance, sessions } => write!(
                f,
                "'{instance}' has {sessions} session(s) open: the running game will overwrite this \
                 change to the multiplayer list when it exits"
            ),
            LaunchedOffline { account } => {
                write!(
                    f,
                    "playing offline as '{account}' — this session is not signed in"
                )
            }
            SessionNotVerified { account } => write!(
                f,
                "could not reach Microsoft to refresh '{account}', so this session runs on the \
                 stored sign-in"
            ),
            ServerNotRestarted { server, detail } => {
                write!(f, "'{server}' was stopped for the update and could not be started again: {detail}")
            }
            PackNotMoved {
                pack,
                version_number,
                pack_game_version,
                target,
            } => write!(
                f,
                "the modpack '{pack}' {version_number} is built for Minecraft {pack_game_version} \
                 and has no version for {target}"
            ),
            DocumentQuarantined {
                document, detail, ..
            } => write!(
                f,
                "{document} could not be read and was set aside: {detail}"
            ),
        }
    }
}
