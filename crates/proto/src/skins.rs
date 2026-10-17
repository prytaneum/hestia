//! The skins domain: profile skins and capes for a signed-in Minecraft account,
//! plus the daemon's local skin library. Desktop-facing — deliberately no CLI
//! surface. Textures cross the wire as URLs (Mojang-hosted) or data URLs
//! (library blobs); uploads carry the PNG as base64.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SkinVariant {
    #[default]
    Classic,
    Slim,
}

/// How the daemon knows about a skin: a vanilla default, a library entry, or
/// the account's currently equipped texture that neither covers.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SkinSource {
    #[default]
    Default,
    Library,
    External,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Skin {
    /// The texture hash — the stable identity a library row and an equip name.
    pub key: String,
    pub name: String,
    pub variant: SkinVariant,
    /// An https texture URL, or a data URL for a library blob.
    pub texture: String,
    pub source: SkinSource,
    pub equipped: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Cape {
    pub id: String,
    pub name: String,
    /// The Mojang-hosted texture URL.
    pub texture: String,
    pub equipped: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinListParams {
    /// Name or uuid; empty uses the default account.
    pub account: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinListResult {
    /// Library entries, then the vanilla defaults, then — only when neither
    /// covers it — the account's equipped external skin. At most one entry is
    /// `equipped`.
    pub skins: Vec<Skin>,
    /// The capes the account owns; at most one is `equipped`.
    pub capes: Vec<Cape>,
}

pub struct SkinList;
impl Contract for SkinList {
    const CHANNEL: &'static str = "skin.list";
    /// Every call that touches Mojang takes 30 s: a stale account token is
    /// rotated through Microsoft inline, which multiplies the round trips.
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = SkinListParams;
    type Result = SkinListResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinAddParams {
    /// Name or uuid; empty uses the default account.
    pub account: String,
    /// An optional label for the library entry.
    pub name: String,
    pub variant: SkinVariant,
    /// The skin PNG (64×64, or the legacy 64×32), base64-encoded.
    pub data: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinAddResult {
    pub skin: Skin,
}

pub struct SkinAdd;
impl Contract for SkinAdd {
    const CHANNEL: &'static str = "skin.add";
    const CHANGES: Option<Scope> = Some(Scope::Skins);
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = SkinAddParams;
    type Result = SkinAddResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinEquipParams {
    /// Name or uuid; empty uses the default account.
    pub account: String,
    /// A library or default skin key from `skin.list`.
    pub key: String,
}

pub struct SkinEquip;
impl Contract for SkinEquip {
    const CHANNEL: &'static str = "skin.equip";
    const CHANGES: Option<Scope> = Some(Scope::Skins);
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = SkinEquipParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinResetParams {
    /// Name or uuid; empty uses the default account.
    pub account: String,
}

pub struct SkinReset;
impl Contract for SkinReset {
    const CHANNEL: &'static str = "skin.reset";
    const CHANGES: Option<Scope> = Some(Scope::Skins);
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = SkinResetParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinUpdateParams {
    /// Name or uuid; empty uses the default account.
    pub account: String,
    /// The library entry to update.
    pub key: String,
    /// The new label; empty clears it.
    pub name: String,
    pub variant: SkinVariant,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinUpdateResult {
    /// The updated library entry. A label-only update never touches Mojang,
    /// so `equipped` here is authoritative only from `skin.list`.
    pub skin: Skin,
}

pub struct SkinUpdate;
impl Contract for SkinUpdate {
    const CHANNEL: &'static str = "skin.update";
    const CHANGES: Option<Scope> = Some(Scope::Skins);
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = SkinUpdateParams;
    type Result = SkinUpdateResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SkinRemoveParams {
    /// The library entry to remove. The equipped Mojang skin is untouched.
    pub key: String,
}

pub struct SkinRemove;
impl Contract for SkinRemove {
    const CHANNEL: &'static str = "skin.remove";
    const CHANGES: Option<Scope> = Some(Scope::Skins);
    type Params = SkinRemoveParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CapeEquipParams {
    /// Name or uuid; empty uses the default account.
    pub account: String,
    /// A cape id from `skin.list`.
    pub cape: String,
}

pub struct CapeEquip;
impl Contract for CapeEquip {
    const CHANNEL: &'static str = "cape.equip";
    const CHANGES: Option<Scope> = Some(Scope::Skins);
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = CapeEquipParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CapeClearParams {
    /// Name or uuid; empty uses the default account.
    pub account: String,
}

pub struct CapeClear;
impl Contract for CapeClear {
    const CHANNEL: &'static str = "cape.clear";
    const CHANGES: Option<Scope> = Some(Scope::Skins);
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = CapeClearParams;
    type Result = Empty;
}
