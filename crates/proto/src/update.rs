//! Self-update: the released-version check, the signed artifact download, and
//! applying it. Every front-end goes through these channels — the daemon is the
//! only thing that reads the release manifest or verifies a signature.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};
use crate::download::DownloadProgress;
use crate::job::{Job, JobStarted};

/// Which release feed a build follows. One manifest per channel: the channel
/// picks the document, version precedence decides whether it is an upgrade.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    /// Also the last path segment of the feed this channel is served from.
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateChannel::Stable => "stable",
            UpdateChannel::Beta => "beta",
        }
    }

    pub fn parse(value: &str) -> Option<UpdateChannel> {
        match value {
            "stable" => Some(UpdateChannel::Stable),
            "beta" => Some(UpdateChannel::Beta),
            _ => None,
        }
    }
}

/// How this copy of Hestia was installed, which decides both the artifact the
/// manifest is asked for and how it is applied.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum UpdateInstall {
    /// The Windows NSIS installer — the setup executable re-runs over itself.
    Nsis,
    AppImage,
    Deb,
    Rpm,
    /// A portable archive, a distro package Hestia did not place, or a build
    /// run straight out of `target/`. Nothing here may be replaced in place.
    #[default]
    Unmanaged,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    /// The artifact matching this install, or the platform's default one when
    /// the manifest carries nothing more specific.
    pub url: String,
    /// Whether `update.download` + `update.apply` can install this in place.
    /// False on an unmanaged layout, or when the manifest has no artifact for
    /// how this copy was installed — a front-end then offers `url` instead.
    pub applicable: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current: String,
    pub install: UpdateInstall,
    /// The feed this answer came from.
    pub channel: UpdateChannel,
    pub available: Option<UpdateInfo>,
}

pub struct UpdateCheck;
impl Contract for UpdateCheck {
    const CHANNEL: &'static str = "update.check";
    type Params = Empty;
    type Result = UpdateCheckResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateDownloadParams {
    pub id: String,
}

pub struct UpdateDownload;
impl Contract for UpdateDownload {
    const CHANNEL: &'static str = "update.download";
    type Params = UpdateDownloadParams;
    type Result = JobStarted;
}

impl Job for UpdateDownload {
    type Progress = DownloadProgress;
    type Done = UpdateDone;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateApplyParams {
    /// The staged artifact, as reported by `update.done`. Checked to be the
    /// file this daemon downloaded — a client cannot name an arbitrary path.
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateApplyResult {
    /// Whether the installer brings Hestia back up by itself. The Windows
    /// installer stops the daemon, replaces it and starts it again, so a
    /// front-end need only exit; a Linux package or AppImage is replaced in
    /// place and nothing is restarted for it.
    pub relaunches: bool,
}

pub struct UpdateApply;
impl Contract for UpdateApply {
    const CHANNEL: &'static str = "update.apply";
    type Params = UpdateApplyParams;
    type Result = UpdateApplyResult;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct UpdateDone {
    pub path: PathBuf,
    pub version: String,
}
