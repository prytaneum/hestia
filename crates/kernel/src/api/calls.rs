use std::future::Future;
use std::pin::Pin;

use proto::error::ErrorInfo;
use serde_json::Value;

use super::chain::Chain;

pub type Reply<'a> = Pin<Box<dyn Future<Output = Result<Value, ErrorInfo>> + Send + 'a>>;

/// How an adapter reaches the daemon's handlers: the same router a front-end
/// uses, as the plugin that ends `chain`. The kernel builds the chain and the
/// router applies its gates (owner enabled, account); the adapter only converts.
pub trait HostCalls: Send + Sync {
    fn call<'a>(&'a self, chain: &'a Chain, channel: &'static str, params: Value) -> Reply<'a>;

    /// Starts the job `channel` names and answers its `done` payload.
    fn run_job<'a>(&'a self, chain: &'a Chain, channel: &'static str, params: Value) -> Reply<'a>;
}
