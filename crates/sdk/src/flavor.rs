//! The decoding half of a flavor's guest exports. See the `flavor_*` methods
//! of [`Plugin`](crate::Plugin).

use serde::Deserialize;

#[doc(hidden)]
#[derive(Deserialize)]
pub struct GameArgs {
    pub game: String,
}

#[doc(hidden)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveArgs {
    pub version: String,
    pub loader_version: Option<String>,
}
