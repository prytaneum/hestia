//! The screenshots every instance has taken, read as one listing.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::Contract;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Screenshot {
    pub instance: String,
    pub instance_name: String,
    /// The file under the instance's `screenshots/`, which is its identity.
    pub file: String,
    pub path: PathBuf,
    pub taken_unix: i64,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ScreenshotListParams {
    /// One instance by name or id; empty reads every instance that shares them.
    pub instance: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ScreenshotListResult {
    /// Newest first.
    pub screenshots: Vec<Screenshot>,
}

pub struct ScreenshotList;
impl Contract for ScreenshotList {
    const CHANNEL: &'static str = "screenshot.list";
    type Params = ScreenshotListParams;
    type Result = ScreenshotListResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ScreenshotDeleteParams {
    pub instance: String,
    pub file: String,
}

/// Deletes the file where it is; nothing was ever copied.
pub struct ScreenshotDelete;
impl Contract for ScreenshotDelete {
    const CHANNEL: &'static str = "screenshot.delete";
    const CHANGES: Option<Scope> = Some(Scope::Screenshots);
    type Params = ScreenshotDeleteParams;
    type Result = ScreenshotListResult;
}
