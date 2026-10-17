use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};
use crate::download::Checksum;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    #[serde(flatten)]
    pub checksum: Checksum,
    #[serde(default)]
    pub size: u64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CacheUsage {
    pub entries: u64,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CacheInfoResult {
    pub path: PathBuf,
    #[serde(flatten)]
    pub usage: CacheUsage,
}

pub struct CacheInfo;
impl Contract for CacheInfo {
    const CHANNEL: &'static str = "cache.info";
    type Params = Empty;
    type Result = CacheInfoResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CacheListResult {
    pub entries: Vec<CacheEntry>,
}

pub struct CacheList;
impl Contract for CacheList {
    const CHANNEL: &'static str = "cache.list";
    type Params = Empty;
    type Result = CacheListResult;
}

pub struct CacheClear;
impl Contract for CacheClear {
    const CHANNEL: &'static str = "cache.clear";
    const CHANGES: Option<Scope> = Some(Scope::Cache);
    type Params = Empty;
    type Result = CacheUsage;
}
