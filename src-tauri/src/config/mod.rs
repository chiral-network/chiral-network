//! Configuration management module.
//!
//! Provides persistent configuration management for various components
//! of the Chiral Network application.

pub mod bittorrent;

pub use bittorrent::{
    BitTorrentConfig, BitTorrentConfigManager, NetworkConfig, RateLimitConfig,
    DhtConfig, FileConfig, PeerConfig, ProxyConfig, ProxyType, ProxyAuth,
    ConnectionLimits,
    // Add the Tauri commands
    get_bittorrent_config, update_bittorrent_config, reset_bittorrent_config,
    update_network_config, update_rate_limits,
};