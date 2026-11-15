//! Chiral inference exchange: explicit-network settlement, prepaid inference and signed discovery.
pub mod app;
pub mod auth;
pub mod billing_store;
pub mod chain_client;
pub mod chain_rpc_api;
pub mod consumer;
pub mod contracts;
pub mod dht;
pub mod engine_runtime;
pub mod error;
mod file_lock;
pub mod inference_backend;
pub mod inference_gateway;
pub mod network;
pub mod offer_registry;
pub mod performance;
pub mod protocol;
pub mod provider_config;
pub mod rating_api;
pub mod rating_storage;
pub mod reputation;
pub mod rpc_client;
pub mod version;
pub mod wallet;

#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "desktop")]
pub fn run() {
    desktop::run();
}
