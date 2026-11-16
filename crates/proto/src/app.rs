use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AppInfoResult {
    pub name: String,
    pub version: String,
    pub id: String,
    pub vendor: String,
    pub channel: String,
}

pub struct AppInfo;
impl Contract for AppInfo {
    const CHANNEL: &'static str = "app.info";
    type Params = Empty;
    type Result = AppInfoResult;
}
