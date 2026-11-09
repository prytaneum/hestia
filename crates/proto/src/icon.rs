//! An entry's own icon: one image kept in the entry's directory, so it moves,
//! exports and is removed with the entry it belongs to.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::Contract;
use crate::error::EntryTarget;

/// The icon an entry wears, read where it lies.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryIcon {
    pub path: PathBuf,
    /// Changes whenever the image does, so a front-end that caches by URL can
    /// append it.
    pub revision: u64,
    /// What composed it; `None` for an image somebody picked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<IconGenerator>,
}

/// The background and symbol a generated icon was baked from — what lets an
/// editor reopen on the icon as it looks. The symbol is a front-end catalogue
/// id the daemon does not interpret.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct IconGenerator {
    pub background: IconBackground,
    pub symbol: String,
}

/// Colors are `#rrggbb`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum IconBackground {
    Color {
        value: String,
    },
    LinearTopDownGradient {
        top_color: String,
        bottom_color: String,
    },
}

impl Default for IconBackground {
    fn default() -> Self {
        IconBackground::Color {
            value: String::new(),
        }
    }
}

/// Where a new icon comes from.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum IconSource {
    /// An image file on this machine, by absolute path. The daemon copies it,
    /// so the icon survives the original moving.
    File { path: String },
    /// An image the front-end composed, base64-encoded, and what composed it.
    Generated {
        image: String,
        generator: IconGenerator,
    },
}

impl Default for IconSource {
    fn default() -> Self {
        IconSource::File {
            path: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryIconSetParams {
    pub target: EntryTarget,
    pub source: IconSource,
}

/// Replaces whatever icon the entry wore. The image must be a PNG, JPEG, WebP
/// or GIF of at most 10 MB, recognised by its bytes rather than its name.
pub struct EntryIconSet;
impl Contract for EntryIconSet {
    const CHANNEL: &'static str = "entry.icon.set";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = EntryIconSetParams;
    type Result = EntryIcon;
}
