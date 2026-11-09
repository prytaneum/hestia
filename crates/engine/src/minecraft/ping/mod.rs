//! Server status pings, one module per edition.

mod java;

pub use java::{ping, ping_address, split_address};
