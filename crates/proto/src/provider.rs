//! The open form of a name that a provider owns: a flavor's upstream, a
//! content source, an archive format. Core vocabulary stays a closed enum
//! variant; anything a provider supplies is a [`Provider`], so adding one never
//! edits an enum on the wire.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A provider-owned thing, named by a stable `id` and shown as `name`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
}

impl Provider {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Provider {
            id: id.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}
