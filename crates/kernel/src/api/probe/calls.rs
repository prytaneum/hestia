use std::sync::Mutex;

use proto::error::{EntryTarget, ErrorInfo};
use proto::Contract;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::contract::{Build, Create, Echo, EchoParams, Echoed, EntryRef, Open, Shout};
use crate::api::{Chain, HostCalls, Reply, Settled};

/// One call that reached the host, as the router would have seen it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub chain: Vec<String>,
    pub channel: &'static str,
}

/// The probe's handlers standing in for the daemon's router: it answers the
/// probe's channels itself and records who called. It
/// applies no entries gate; that is the daemon's.
#[derive(Default)]
pub struct ProbeCalls {
    seen: Mutex<Vec<Recorded>>,
    touched: Mutex<Vec<EntryTarget>>,
    created: Mutex<Vec<EntryTarget>>,
}

fn decode<T: DeserializeOwned>(params: Value) -> Result<T, ErrorInfo> {
    serde_json::from_value(params).map_err(|error| ErrorInfo::MalformedRequest {
        detail: error.to_string(),
    })
}

impl ProbeCalls {
    pub fn seen(&self) -> Vec<Recorded> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Every entry a call said it touches.
    pub fn touched(&self) -> Vec<EntryTarget> {
        self.touched
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Every entry a call said it made, whether it was a plain call or a job.
    pub fn created(&self) -> Vec<EntryTarget> {
        self.created
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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
        match channel {
            Open::CHANNEL | Create::CHANNEL | Build::CHANNEL => {
                Ok(json!(decode::<EntryRef>(params)?))
            }
            Echo::CHANNEL | Shout::CHANNEL => {
                let params: EchoParams = decode(params)?;
                let text = match channel {
                    Echo::CHANNEL => params.text,
                    _ => params.text.to_uppercase(),
                };
                let length = text.chars().count() as u32;
                Ok(json!(Echoed { text, length }))
            }
            _ => Err(ErrorInfo::UnknownChannel {
                channel: channel.to_owned(),
            }),
        }
    }
}

impl HostCalls for ProbeCalls {
    fn call<'a>(
        &'a self,
        chain: &'a Chain,
        channel: &'static str,
        params: Value,
        touched: &'a [EntryTarget],
    ) -> Reply<'a> {
        Box::pin(async move {
            self.touched
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(touched);
            self.answer(chain, channel, params)
        })
    }

    fn run_job<'a>(
        &'a self,
        chain: &'a Chain,
        channel: &'static str,
        params: Value,
        touched: &'a [EntryTarget],
    ) -> Reply<'a> {
        Box::pin(async move {
            self.touched
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(touched);
            self.answer(chain, channel, params)
        })
    }

    fn created<'a>(&'a self, _: &'a Chain, created: &'a [EntryTarget]) -> Settled<'a> {
        Box::pin(async move {
            self.created
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(created);
        })
    }
}
