//! chiral_provider — run a Chiral resource provider (storage / container /
//! inference). Configured via `CHIRAL_PROVIDER_*` environment variables; see
//! `provider_daemon::ProviderConfig::from_env`.
//!
//! Requires a multi-threaded Tokio runtime (the on-chain `RpcChainVerifier` uses
//! `block_in_place`), which `#[tokio::main]` provides by default.

use chiral_network::provider_daemon::{self, ProviderConfig};

#[tokio::main]
async fn main() {
    let config = match ProviderConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[chiral-provider] config error: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "[chiral-provider] class={:?} endpoint={} control={} data={}",
        config.class, config.endpoint, config.control_bind, config.data_bind
    );
    if let Err(e) = provider_daemon::run(config).await {
        eprintln!("[chiral-provider] error: {e}");
        std::process::exit(1);
    }
}
