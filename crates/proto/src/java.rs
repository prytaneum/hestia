use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};
use crate::job::{Job, JobStarted};

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JavaRelease {
    pub major: i32,
    pub lts: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JavaRuntime {
    pub vendor: String,
    pub major: i32,
    pub release_name: String,
    pub home: PathBuf,
    pub executable: PathBuf,
    /// Whether an existing server or instance launches with this major.
    pub in_use: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum JavaInstallPhase {
    Resolving,
    Downloading,
    Extracting,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallProgress {
    pub phase: JavaInstallPhase,
    #[serde(default)]
    pub current: u64,
    #[serde(default)]
    pub total: u64,
}

impl JavaInstallProgress {
    /// Completion of the current phase in `0.0..=1.0`; a phase with unknown
    /// extent (`total == 0`) reports 0. Keyed on with the phase by the daemon's
    /// progress coalescing.
    pub fn ratio(&self) -> f64 {
        if self.total > 0 {
            (self.current as f64 / self.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JavaReleasesResult {
    pub releases: Vec<JavaRelease>,
}

pub struct JavaReleases;
impl Contract for JavaReleases {
    const CHANNEL: &'static str = "java.releases";
    type Params = Empty;
    type Result = JavaReleasesResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JavaListResult {
    pub runtimes: Vec<JavaRuntime>,
}

pub struct JavaList;
impl Contract for JavaList {
    const CHANNEL: &'static str = "java.list";
    type Params = Empty;
    type Result = JavaListResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JavaInstallParams {
    pub major: i32,
    pub id: String,
    pub force: bool,
}

/// A job. A cancel registers nothing: the install stages into `.staging` and
/// only a rename makes it real, so a cancelled one leaves no runtime behind.
pub struct JavaInstall;
impl Contract for JavaInstall {
    const CHANNEL: &'static str = "java.install";
    type Params = JavaInstallParams;
    type Result = JobStarted;
}

impl Job for JavaInstall {
    type Progress = JavaInstallProgress;
    type Done = JavaInstallDone;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct JavaUninstallParams {
    pub major: i32,
}

pub struct JavaUninstall;
impl Contract for JavaUninstall {
    const CHANNEL: &'static str = "java.uninstall";
    type Params = JavaUninstallParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallDone {
    pub runtime: JavaRuntime,
    #[serde(default)]
    pub already_installed: bool,
}
