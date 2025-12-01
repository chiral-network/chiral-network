//! Configuration management module.
//!
//! Provides persistent configuration management for various components
//! of the Chiral Network application.

use once_cell::sync::Lazy;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

pub mod bittorrent;

pub use bittorrent::{
    BitTorrentConfig, BitTorrentConfigManager, NetworkConfig, RateLimitConfig,
    DhtConfig, FileConfig, PeerConfig, ProxyConfig, ProxyType, ProxyAuth,
    ConnectionLimits,
    // Add the Tauri commands
    get_bittorrent_config, update_bittorrent_config, reset_bittorrent_config,
    update_network_config, update_rate_limits,
};

// ============================================================================
// Chain ID Configuration (from genesis.json)
// ============================================================================

/// Parse the chain ID from genesis.json
/// This is the single source of truth for the chain ID
fn parse_chain_id_from_genesis() -> Option<u64> {
    // Try multiple possible locations for genesis.json
    let possible_paths = [
        PathBuf::from("genesis.json"),
        PathBuf::from("../genesis.json"),
        PathBuf::from("../../genesis.json"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("genesis.json")))
            .unwrap_or_default(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().and_then(|p| p.parent()).map(|p| p.join("genesis.json")))
            .unwrap_or_default(),
    ];

    for path in &possible_paths {
        if path.exists() {
            if let Ok(mut file) = File::open(path) {
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_ok() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                        if let Some(chain_id) = json.get("config")
                            .and_then(|c| c.get("chainId"))
                            .and_then(|id| id.as_u64())
                        {
                            return Some(chain_id);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Get the chain ID with fallback order:
/// 1. Environment variable CHIRAL_CHAIN_ID
/// 2. genesis.json
/// 3. Default fallback (should never be reached if genesis.json exists)
pub fn get_chain_id() -> u64 {
    // First try environment variable
    if let Ok(chain_id_str) = std::env::var("CHIRAL_CHAIN_ID") {
        if let Ok(chain_id) = chain_id_str.parse() {
            return chain_id;
        }
    }
    
    // Then try genesis.json
    if let Some(chain_id) = parse_chain_id_from_genesis() {
        return chain_id;
    }
    
    // Fallback (should not be reached if genesis.json is properly deployed)
    eprintln!("Warning: Could not read chain ID from genesis.json, using fallback");
    98765
}

/// Global chain ID - lazily initialized from genesis.json or environment
pub static CHAIN_ID: Lazy<u64> = Lazy::new(get_chain_id);

/// Global network ID - same as chain ID by default, can be overridden via env var
pub static NETWORK_ID: Lazy<u64> = Lazy::new(|| {
    std::env::var("CHIRAL_NETWORK_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(get_chain_id)
});