use std::sync::Arc;

use proto::access::{EntryAccess, JobAccess};
use serde_json::Value;

use super::calls::HostCalls;
use super::chain::Chain;
use super::error::plugin_error;
use crate::runtime::{GuestError, PluginLog};

/// What one adapter call needs, owned so no borrow of the store crosses an await.
/// `permission` is the one that gates the interface calling, if any, which a
/// refusal of the router names.
pub(crate) struct Link {
    pub calls: Arc<dyn HostCalls>,
    pub chain: Chain,
    pub permission: Option<&'static str>,
    pub log: PluginLog,
    /// The WIT interface and function calling, which a developer-mode denial names.
    pub site: (&'static str, &'static str),
}

impl Link {
    fn refused(&self, info: proto::error::ErrorInfo) -> GuestError {
        let error = plugin_error(info, self.permission);
        if let GuestError::Denied(permission) = &error {
            self.log.denied(self.site.0, self.site.1, permission);
        }
        error
    }

    pub async fn call<C: EntryAccess>(&self, params: &C::Params) -> Result<C::Result, GuestError> {
        let reply = self
            .calls
            .call(
                &self.chain,
                C::CHANNEL,
                encode(params)?,
                &C::touches(params),
            )
            .await
            .map_err(|info| self.refused(info))?;
        let result: C::Result = decode(reply)?;
        self.calls.created(&self.chain, &C::creates(&result)).await;
        Ok(result)
    }

    pub async fn run_job<C: JobAccess>(&self, params: &C::Params) -> Result<C::Done, GuestError> {
        let reply = self
            .calls
            .run_job(
                &self.chain,
                C::CHANNEL,
                encode(params)?,
                &C::touches(params),
            )
            .await
            .map_err(|info| self.refused(info))?;
        let done: C::Done = decode(reply)?;
        self.calls
            .created(&self.chain, &C::creates_done(&done))
            .await;
        Ok(done)
    }
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Value, GuestError> {
    serde_json::to_value(value).map_err(|error| GuestError::Invalid(error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, GuestError> {
    serde_json::from_value(value).map_err(|error| GuestError::Failed(error.to_string()))
}
