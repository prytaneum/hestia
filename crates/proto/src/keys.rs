use serde::{Deserialize, Serialize};

/// What a config value must look like; a key with no rule takes any string.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KeyKind {
    Boolean,
    Integer { min: i64, max: i64 },
    Choice { values: Vec<String> },
    Memory,
    Interval,
    JvmArgs,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct KeyRule {
    pub key: String,
    pub kind: KeyKind,
}
