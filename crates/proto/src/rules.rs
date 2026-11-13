//! Mojang-style `rules`: what gates a library or a launch argument on the
//! machine it runs on. The host evaluates them, the same way for the game's own
//! profile and for a plugin's.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    #[default]
    Allow,
    Disallow,
}

/// The platform a rule names. Every field left out matches any value.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct OsRule {
    /// `windows`, `osx` or `linux`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `x86`, `x86_64` or `arm64`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// Accepted for Mojang's manifests, which use it for one Windows release;
    /// the host does not evaluate it, so it matches every version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// One rule of a list: `action` applies when the rule matches. A rule that
/// names `features` (demo mode, a custom resolution) is not applicable to a
/// launcher that enables none, and is skipped.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Rule {
    pub action: RuleAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<std::collections::BTreeMap<String, bool>>,
}
