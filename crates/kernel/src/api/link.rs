use std::sync::Arc;

use proto::job::Job;
use proto::Contract;
use serde_json::Value;

use super::calls::HostCalls;
use super::chain::Chain;
use super::error::plugin_error;
use crate::runtime::GuestError;

/// What one adapter call needs, owned so no borrow of the store crosses an await.
pub(crate) struct Link {
    pub calls: Arc<dyn HostCalls>,
    pub chain: Chain,
}

impl Link {
    pub async fn call<C: Contract>(&self, params: &C::Params) -> Result<C::Result, GuestError> {
        let params = encode(params)?;
        let reply = self
            .calls
            .call(&self.chain, C::CHANNEL, params)
            .await
            .map_err(plugin_error)?;
        decode(reply)
    }

    pub async fn run_job<C: Job>(&self, params: &C::Params) -> Result<C::Done, GuestError> {
        let params = encode(params)?;
        let reply = self
            .calls
            .run_job(&self.chain, C::CHANNEL, params)
            .await
            .map_err(plugin_error)?;
        decode(reply)
    }
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Value, GuestError> {
    serde_json::to_value(value).map_err(|error| GuestError::Invalid(error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, GuestError> {
    serde_json::from_value(value).map_err(|error| GuestError::Failed(error.to_string()))
}
