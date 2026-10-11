use serde::{Deserialize, Serialize};

use crate::contract::Contract;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct EventsSubscribeParams {
    /// Empty subscribes to every event.
    #[serde(default)]
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EventsSubscribeResult {
    pub subscribed: bool,
}

pub struct EventsSubscribe;
impl Contract for EventsSubscribe {
    const CHANNEL: &'static str = "events.subscribe";
    type Params = EventsSubscribeParams;
    type Result = EventsSubscribeResult;
}
