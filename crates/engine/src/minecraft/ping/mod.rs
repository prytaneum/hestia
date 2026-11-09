//! Server status pings, one module per edition.

mod bedrock;
mod java;

use anyhow::Result;
use proto::minecraft::Edition;
use proto::server::ServerPingResult;

pub use java::{ping_address, split_address};

/// Ping a managed server on loopback at its game port.
pub async fn ping(edition: Edition, port: u16) -> Result<ServerPingResult> {
    match edition {
        Edition::Java => java::ping(port).await,
        Edition::Bedrock => bedrock::ping(port).await,
    }
}
