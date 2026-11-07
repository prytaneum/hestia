//! The Java provider half of a plugin's guest exports. See the `java_*`
//! methods of [`Plugin`](crate::Plugin).

use serde::{Deserialize, Serialize};

#[doc(hidden)]
#[derive(Deserialize)]
pub struct ResolveArgs {
    pub major: i32,
    pub os: String,
    pub arch: String,
}

/// One downloadable build of a Java runtime, as the host expects it. The host
/// downloads `url`, checks it against `sha256` and extracts it as `archive`.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct JavaBuild {
    /// A plain name (letters, digits, `-`, `_`); it names the install directory.
    pub vendor: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// `tar.gz` or `zip`.
    pub archive: String,
}
