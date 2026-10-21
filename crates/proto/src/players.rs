//! Who is on a server and who may be: the players online, and the whitelist,
//! operator and ban lists the server keeps beside its world.

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};
use crate::plugin::Permission;
use crate::server::ServerRef;

/// One of the lists a server keeps a player on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PlayerList {
    #[default]
    Whitelist,
    Ops,
    Banned,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerEntry {
    pub name: String,
    pub uuid: String,
    /// Why, for a ban that gave a reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerPlayersResult {
    /// Who is online right now; empty for a stopped server.
    pub online: Vec<PlayerEntry>,
    pub whitelist: Vec<PlayerEntry>,
    pub ops: Vec<PlayerEntry>,
    pub banned: Vec<PlayerEntry>,
}

/// Answers for a stopped server too: the lists are the server's own files.
pub struct ServerPlayers;
impl Contract for ServerPlayers {
    const CHANNEL: &'static str = "server.players";
    const PLUGIN: Option<Permission> = Some(Permission::EntryRead);
    type Params = ServerRef;
    type Result = ServerPlayersResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerPlayerSetParams {
    pub server: String,
    pub player: String,
    pub list: PlayerList,
    /// Put the player on the list, or take them off it.
    pub listed: bool,
}

/// Goes through the running server, which resolves the name and rewrites its
/// own file — so it needs the server up.
pub struct ServerPlayerSet;
impl Contract for ServerPlayerSet {
    const CHANNEL: &'static str = "server.players.set";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = ServerPlayerSetParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerPlayerKickParams {
    pub server: String,
    pub player: String,
}

pub struct ServerPlayerKick;
impl Contract for ServerPlayerKick {
    const CHANNEL: &'static str = "server.players.kick";
    const PLUGIN: Option<Permission> = Some(Permission::EntryWrite);
    type Params = ServerPlayerKickParams;
    type Result = Empty;
}
