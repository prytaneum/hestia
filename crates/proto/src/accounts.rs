//! The accounts domain: Minecraft accounts signed in through Microsoft. Tokens
//! never cross the wire — the daemon keeps them; clients see uuid and name.

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    #[default]
    DeviceCode,
    Sisu,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Account {
    pub uuid: String,
    pub name: String,
    /// Stored, but its refresh token was rejected: cannot launch until re-login.
    pub needs_reauth: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AccountLoginBeginParams {
    pub method: LoginMethod,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AccountLoginBeginResult {
    pub id: String,
    pub method: LoginMethod,
    pub url: String,
    pub user_code: String,
    pub verification_uri: String,
}

pub struct AccountLoginBegin;
impl Contract for AccountLoginBegin {
    const CHANNEL: &'static str = "account.login.begin";
    type Params = AccountLoginBeginParams;
    type Result = AccountLoginBeginResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginCompleteParams {
    pub id: String,
    #[serde(default)]
    pub code: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AccountLoginCompleteResult {
    pub account: Account,
}

pub struct AccountLoginComplete;
impl Contract for AccountLoginComplete {
    const CHANNEL: &'static str = "account.login.complete";
    type Params = AccountLoginCompleteParams;
    type Result = AccountLoginCompleteResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AccountListResult {
    pub accounts: Vec<Account>,
    /// The account launches use when none is named.
    pub default_uuid: String,
}

pub struct AccountList;
impl Contract for AccountList {
    const CHANNEL: &'static str = "account.list";
    type Params = Empty;
    type Result = AccountListResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AccountSwitchParams {
    /// Name or uuid.
    pub account: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AccountSwitchResult {
    pub account: Account,
}

pub struct AccountSwitch;
impl Contract for AccountSwitch {
    const CHANNEL: &'static str = "account.switch";
    type Params = AccountSwitchParams;
    type Result = AccountSwitchResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct AccountRemoveParams {
    /// Name or uuid.
    pub account: String,
}

pub struct AccountRemove;
impl Contract for AccountRemove {
    const CHANNEL: &'static str = "account.remove";
    type Params = AccountRemoveParams;
    type Result = Empty;
}
