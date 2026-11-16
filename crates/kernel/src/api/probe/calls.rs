use std::sync::Mutex;

use proto::error::ErrorInfo;
use proto::Contract;
use serde_json::{json, Value};

use super::contract::{Echo, EchoParams, Echoed, Shout};
use crate::api::{Chain, HostCalls, Reply};

/// One call that reached the host, as the router would have seen it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub chain: Vec<String>,
    pub channel: &'static str,
}

/// The probe's handlers standing in for the daemon's router: it answers the
/// probe's channels itself and records who called.
#[derive(Default)]
pub struct ProbeCalls {
    seen: Mutex<Vec<Recorded>>,
}

impl ProbeCalls {
    pub fn seen(&self) -> Vec<Recorded> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn answer(
        &self,
        chain: &Chain,
        channel: &'static str,
        params: Value,
    ) -> Result<Value, ErrorInfo> {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Recorded {
                chain: chain.plugins().to_vec(),
                channel,
            });
        let params: EchoParams =
            serde_json::from_value(params).map_err(|error| ErrorInfo::MalformedRequest {
                detail: error.to_string(),
            })?;
        let text = match channel {
            Echo::CHANNEL => params.text,
            Shout::CHANNEL => params.text.to_uppercase(),
            _ => {
                return Err(ErrorInfo::UnknownChannel {
                    channel: channel.to_owned(),
                })
            }
        };
        let length = text.chars().count() as u32;
        Ok(json!(Echoed { text, length }))
    }
}

impl HostCalls for ProbeCalls {
    fn call<'a>(&'a self, chain: &'a Chain, channel: &'static str, params: Value) -> Reply<'a> {
        Box::pin(async move { self.answer(chain, channel, params) })
    }

    fn run_job<'a>(&'a self, chain: &'a Chain, channel: &'static str, params: Value) -> Reply<'a> {
        Box::pin(async move { self.answer(chain, channel, params) })
    }
}
