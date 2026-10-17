use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::change::Scope;
use crate::contract::{Contract, Empty};

/// Reserved keys the daemon routes to their own subsystem instead of the settings
/// store: the data-directory pointer and the login registration.
pub const HOME_KEY: &str = "home";
pub const AUTOSTART_KEY: &str = "autostart";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ConfigGetParams {
    pub key: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigGetResult {
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub value: Value,
}

pub struct ConfigGet;
impl Contract for ConfigGet {
    const CHANNEL: &'static str = "config.get";
    type Params = ConfigGetParams;
    type Result = ConfigGetResult;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetParams {
    pub key: String,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub value: Value,
}

pub struct ConfigSet;
impl Contract for ConfigSet {
    const CHANNEL: &'static str = "config.set";
    const CHANGES: Option<Scope> = Some(Scope::Config);
    type Params = ConfigSetParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigListResult {
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
    pub entries: Value,
}

pub struct ConfigList;
impl Contract for ConfigList {
    const CHANNEL: &'static str = "config.list";
    type Params = Empty;
    type Result = ConfigListResult;
}
