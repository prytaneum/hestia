//! Providers: what fills one of the launcher's open registries (content
//! sources, flavors, archive formats, Java distributions), whether built in or
//! contributed by a plugin.
//!
//! [`Provider`] is the open form of a name a provider owns. Core vocabulary
//! stays a closed enum variant; anything a provider supplies is a `Provider`, so
//! adding one never edits an enum on the wire.
//!
//! [`ProviderKind`] and [`ProviderOp`] are what a plugin contributes and what
//! the host asks of it, as data: a kind names the permissions it needs, and an
//! operation pairs its name with typed arguments and reply, like a
//! [`Contract`](crate::Contract) in the other direction. The ABI carries every
//! operation of every kind through one `provider.invoke` export, so adding a
//! kind or an operation never changes the WIT
//! ([0106](../../../docs/decisions/0106-the-plugin-api-is-mechanisms-not-features.md)).

pub mod content;
pub mod flavor;
pub mod format;
pub mod java;

use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::plugin::Permission;

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

/// A kind of provider a plugin may contribute under `contributes.providers`.
pub trait ProviderKind {
    /// The `kind` a manifest names.
    const KIND: &'static str;
    /// What a plugin must hold to contribute one. A kind that needs
    /// `network:connect` also needs at least one granted host.
    const PERMISSIONS: &'static [Permission];
}

/// One operation the host asks of a provider of `Kind`. The guest answers
/// `unsupported` for an operation it does not implement; the host documents,
/// per operation, whether that is a fallback or a failure.
pub trait ProviderOp {
    type Kind: ProviderKind;
    const OP: &'static str;
    type Args: Serialize + DeserializeOwned;
    type Reply: Serialize + DeserializeOwned;
}

/// A kind by name, for checking a manifest without its types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindSpec {
    pub kind: &'static str,
    pub permissions: &'static [Permission],
}

const fn spec<K: ProviderKind>() -> KindSpec {
    KindSpec {
        kind: K::KIND,
        permissions: K::PERMISSIONS,
    }
}

/// Every kind this host offers.
pub const KINDS: &[KindSpec] = &[
    spec::<content::Source>(),
    spec::<flavor::Flavor>(),
    spec::<format::Format>(),
    spec::<java::Distribution>(),
];

/// The kind named `kind`, if this host offers it.
pub fn kind(kind: &str) -> Option<&'static KindSpec> {
    KINDS.iter().find(|spec| spec.kind == kind)
}
