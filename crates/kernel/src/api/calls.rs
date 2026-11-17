use std::future::Future;
use std::pin::Pin;

use proto::error::{EntryTarget, ErrorInfo};
use serde_json::Value;

use super::chain::Chain;

pub type Reply<'a> = Pin<Box<dyn Future<Output = Result<Value, ErrorInfo>> + Send + 'a>>;

pub type Settled<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// How an adapter reaches the daemon's handlers: the same router a front-end
/// uses, as the plugin that ends `chain`. The kernel builds the chain and the
/// router applies its gates (owner enabled, account, and the `entries` scope of
/// every plugin in `chain` over `touched`); the adapter only converts.
pub trait HostCalls: Send + Sync {
    fn call<'a>(
        &'a self,
        chain: &'a Chain,
        channel: &'static str,
        params: Value,
        touched: &'a [EntryTarget],
    ) -> Reply<'a>;

    fn run_job<'a>(
        &'a self,
        chain: &'a Chain,
        channel: &'static str,
        params: Value,
        touched: &'a [EntryTarget],
    ) -> Reply<'a>;

    fn created<'a>(&'a self, chain: &'a Chain, created: &'a [EntryTarget]) -> Settled<'a>;
}
