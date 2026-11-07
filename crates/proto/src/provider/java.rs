//! The `java` provider kind: a Java distribution. A runtime is native code the
//! launcher executes, so the guest only names a build: the host downloads it,
//! only from the plugin's network hosts and only against a sha256, and extracts
//! it itself ([0105](../../../../docs/decisions/0105-a-java-provider-only-resolves.md)).

use serde::{Deserialize, Serialize};

use super::{ProviderKind, ProviderOp};
use crate::contract::Empty;
use crate::plugin::Permission;

pub struct Distribution;
impl ProviderKind for Distribution {
    const KIND: &'static str = "java";
    const PERMISSIONS: &'static [Permission] =
        &[Permission::JavaProvide, Permission::NetworkConnect];
}

/// A feature version for one platform: `os` is `linux`, `windows` or `mac`,
/// `arch` is `x64` or `aarch64`.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct TargetQuery {
    pub major: i32,
    pub os: String,
    pub arch: String,
}

/// One downloadable build of a runtime.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct JavaBuild {
    /// A plain name (letters, digits, `-`, `_`); it names the install directory.
    pub vendor: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// `tar.gz` or `zip`.
    pub archive: String,
}

/// The feature versions the distribution offers.
pub struct Majors;
impl ProviderOp for Majors {
    type Kind = Distribution;
    const OP: &'static str = "majors";
    type Args = Empty;
    type Reply = Vec<i32>;
}

/// The build of a feature version for a platform; `None` when there is none.
pub struct Resolve;
impl ProviderOp for Resolve {
    type Kind = Distribution;
    const OP: &'static str = "resolve";
    type Args = TargetQuery;
    type Reply = Option<JavaBuild>;
}
