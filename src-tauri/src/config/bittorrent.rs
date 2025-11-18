//! BitTorrent configuration module with persistent settings.
//!
//! Provides configuration management for BitTorrent operations including
//! network settings, rate limits, and DHT configuration with persistent storage.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_plugin_store::{Store, StoreBuilder};
use tracing::{info, warn};

/// BitTorrent configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitTorrentConfig {
    /// Network settings
    pub network: NetworkConfig,
    /// Rate limiting settings
    pub rate_limits: RateLimitConfig,
    /// DHT settings
    pub dht: DhtConfig,
    /// File management settings
    pub files: FileConfig,
    /// Peer connection settings
    pub peers: PeerConfig,
}

/// Network configuration for BitTorrent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// TCP listening port (0 for random)
    pub listening_port: u16,
    /// Enable UPnP port mapping
    pub enable_upnp: bool,
    /// Enable NAT-PMP port mapping
    pub enable_nat_pmp: bool,
    /// External IP address (optional)
    pub external_ip: Option<String>,
    /// Bind to specific network interface
    pub bind_interface: Option<String>,
    /// Enable IPv6
    pub enable_ipv6: bool,
    /// Proxy settings
    pub proxy: Option<ProxyConfig>,
}

/// Proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Proxy type (SOCKS5, HTTP)
    pub proxy_type: ProxyType,
    /// Proxy server address
    pub address: String,
    /// Proxy server port
    pub port: u16,
    /// Authentication (optional)
    pub auth: Option<ProxyAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyType {
    Socks5,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAuth {
    pub username: String,
    pub password: String,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Global download rate limit in bytes/sec (0 for unlimited)
    pub download_rate_limit: u64,
    /// Global upload rate limit in bytes/sec (0 for unlimited)
    pub upload_rate_limit: u64,
    /// Per-torrent download rate limit in bytes/sec (0 for unlimited)
    pub per_torrent_download_limit: u64,
    /// Per-torrent upload rate limit in bytes/sec (0 for unlimited)
    pub per_torrent_upload_limit: u64,
    /// Connection rate limiting
    pub connection_limits: ConnectionLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionLimits {
    /// Maximum number of connections per torrent
    pub max_connections_per_torrent: u32,
    /// Maximum number of upload slots per torrent
    pub max_uploads_per_torrent: u32,
    /// Global connection limit
    pub max_total_connections: u32,
}

/// DHT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtConfig {
    /// Enable DHT
    pub enabled: bool,
    /// DHT port (0 for same as listening port)
    pub port: u16,
    /// Bootstrap nodes
    pub bootstrap_nodes: Vec<String>,
    /// Enable local peer discovery
    pub enable_local_peer_discovery: bool,
    /// Enable peer exchange (PEX)
    pub enable_peer_exchange: bool,
}

/// File management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    /// Default download directory
    pub default_download_dir: PathBuf,
    /// Completed downloads directory
    pub completed_dir: Option<PathBuf>,
    /// Incomplete downloads directory
    pub incomplete_dir: Option<PathBuf>,
    /// Auto-manage files
    pub auto_manage: bool,
    /// Move completed files
    pub move_completed: bool,
    /// Check file integrity on completion
    pub check_integrity: bool,
    /// Pre-allocate disk space
    pub preallocate_files: bool,
}

/// Peer connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    /// Connection timeout in seconds
    pub connection_timeout: u32,
    /// Peer connection retry attempts
    pub max_retry_attempts: u32,
    /// Enable encryption
    pub enable_encryption: bool,
    /// Require encryption
    pub require_encryption: bool,
    /// Peer ID prefix
    pub peer_id_prefix: String,
    /// User agent string
    pub user_agent: String,
}

impl Default for BitTorrentConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            rate_limits: RateLimitConfig::default(),
            dht: DhtConfig::default(),
            files: FileConfig::default(),
            peers: PeerConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listening_port: 0, // Random port
            enable_upnp: true,
            enable_nat_pmp: true,
            external_ip: None,
            bind_interface: None,
            enable_ipv6: true,
            proxy: None,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            download_rate_limit: 0, // Unlimited
            upload_rate_limit: 0,   // Unlimited
            per_torrent_download_limit: 0,
            per_torrent_upload_limit: 0,
            connection_limits: ConnectionLimits::default(),
        }
    }
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            max_connections_per_torrent: 50,
            max_uploads_per_torrent: 4,
            max_total_connections: 200,
        }
    }
}

impl Default for DhtConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 0, // Same as listening port
            bootstrap_nodes: vec![
                "router.bittorrent.com:6881".to_string(),
                "dht.transmissionbt.com:6881".to_string(),
                "router.utorrent.com:6881".to_string(),
            ],
            enable_local_peer_discovery: true,
            enable_peer_exchange: true,
        }
    }
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            default_download_dir: std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("./"))
                .join("downloads"),
            completed_dir: None,
            incomplete_dir: None,
            auto_manage: true,
            move_completed: false,
            check_integrity: true,
            preallocate_files: false,
        }
    }
}

impl Default for PeerConfig {
    fn default() -> Self {
        Self {
            connection_timeout: 30,
            max_retry_attempts: 3,
            enable_encryption: true,
            require_encryption: false,
            peer_id_prefix: "-CN0001-".to_string(), // Chiral Network v0.0.1
            user_agent: "ChiralNetwork/0.0.1".to_string(),
        }
    }
}

/// Configuration manager for BitTorrent settings
pub struct BitTorrentConfigManager {
    store: Arc<Store<tauri::Wry>>,
    config: BitTorrentConfig,
}

impl BitTorrentConfigManager {
    const CONFIG_KEY: &'static str = "bittorrent_config";

    /// Create a new configuration manager
    pub async fn new(app_handle: AppHandle) -> Result<Self> {
        info!("Initializing BitTorrent configuration manager");

        let store = StoreBuilder::new(&app_handle, "bittorrent-config.json")
            .build()?;

        let mut manager = Self {
            store,
            config: BitTorrentConfig::default(),
        };

        // Load existing configuration or create default
        manager.load_config().await?;
        Ok(manager)
    }

    /// Load configuration from persistent storage
    pub async fn load_config(&mut self) -> Result<()> {
        match self.store.get(Self::CONFIG_KEY) {
            Some(value) => {
                match serde_json::from_value::<BitTorrentConfig>(value.clone()) {
                    Ok(config) => {
                        self.config = config;
                        info!("Loaded BitTorrent configuration from storage");
                    }
                    Err(e) => {
                        warn!("Failed to parse stored configuration: {}. Using defaults.", e);
                        self.config = BitTorrentConfig::default();
                        self.save_config().await?;
                    }
                }
            }
            None => {
                info!("No stored configuration found. Creating default configuration.");
                self.config = BitTorrentConfig::default();
                self.save_config().await?;
            }
        }
        Ok(())
    }

    /// Save configuration to persistent storage
    pub async fn save_config(&self) -> Result<()> {
        let config_value = serde_json::to_value(&self.config)
            .map_err(|e| anyhow!("Failed to serialize config: {}", e))?;

        self.store.set(Self::CONFIG_KEY, config_value);
        
        self.store.save()
            .map_err(|e| anyhow!("Failed to save config: {}", e))?;

        info!("Saved BitTorrent configuration to storage");
        Ok(())
    }

    /// Get current configuration
    pub fn get_config(&self) -> &BitTorrentConfig {
        &self.config
    }

    /// Update configuration
    pub async fn update_config(&mut self, new_config: BitTorrentConfig) -> Result<()> {
        // Validate configuration
        self.validate_config(&new_config)?;
        
        self.config = new_config;
        self.save_config().await?;
        info!("Updated BitTorrent configuration");
        Ok(())
    }

    /// Update network configuration
    pub async fn update_network_config(&mut self, network_config: NetworkConfig) -> Result<()> {
        self.validate_network_config(&network_config)?;
        self.config.network = network_config;
        self.save_config().await
    }

    /// Update rate limit configuration
    pub async fn update_rate_limits(&mut self, rate_limits: RateLimitConfig) -> Result<()> {
        self.config.rate_limits = rate_limits;
        self.save_config().await
    }

    /// Update DHT configuration
    pub async fn update_dht_config(&mut self, dht_config: DhtConfig) -> Result<()> {
        self.config.dht = dht_config;
        self.save_config().await
    }

    /// Update file configuration
    pub async fn update_file_config(&mut self, file_config: FileConfig) -> Result<()> {
        self.validate_file_config(&file_config)?;
        self.config.files = file_config;
        self.save_config().await
    }

    /// Update peer configuration
    pub async fn update_peer_config(&mut self, peer_config: PeerConfig) -> Result<()> {
        self.validate_peer_config(&peer_config)?;
        self.config.peers = peer_config;
        self.save_config().await
    }

    /// Reset configuration to defaults
    pub async fn reset_to_defaults(&mut self) -> Result<()> {
        self.config = BitTorrentConfig::default();
        self.save_config().await?;
        info!("Reset BitTorrent configuration to defaults");
        Ok(())
    }

    /// Validate complete configuration
    fn validate_config(&self, config: &BitTorrentConfig) -> Result<()> {
        self.validate_network_config(&config.network)?;
        self.validate_file_config(&config.files)?;
        self.validate_peer_config(&config.peers)?;
        Ok(())
    }

    /// Validate network configuration
    fn validate_network_config(&self, _config: &NetworkConfig) -> Result<()> {
        // Note: listening_port is u16, so it's always <= 65535
        // No additional validation needed for port range
        Ok(())
    }

    /// Validate file configuration
    fn validate_file_config(&self, config: &FileConfig) -> Result<()> {
        // Skip validation if directory doesn't exist yet - it can be created later
        if config.default_download_dir.to_string_lossy().is_empty() {
            return Err(anyhow!("Default download directory cannot be empty"));
        }
        Ok(())
    }

    /// Validate peer configuration
    fn validate_peer_config(&self, config: &PeerConfig) -> Result<()> {
        if config.connection_timeout == 0 {
            return Err(anyhow!("Connection timeout must be greater than 0"));
        }
        if config.peer_id_prefix.len() > 8 {
            return Err(anyhow!("Peer ID prefix must be 8 characters or less"));
        }
        Ok(())
    }
}

/// Tauri commands for configuration management
#[tauri::command]
pub async fn get_bittorrent_config(
    config_manager: tauri::State<'_, tokio::sync::Mutex<BitTorrentConfigManager>>
) -> Result<BitTorrentConfig, String> {
    let manager = config_manager.lock().await;
    Ok(manager.get_config().clone())
}

#[tauri::command]
pub async fn update_bittorrent_config(
    config: BitTorrentConfig,
    config_manager: tauri::State<'_, tokio::sync::Mutex<BitTorrentConfigManager>>
) -> Result<(), String> {
    let mut manager = config_manager.lock().await;
    manager.update_config(config).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reset_bittorrent_config(
    config_manager: tauri::State<'_, tokio::sync::Mutex<BitTorrentConfigManager>>
) -> Result<(), String> {
    let mut manager = config_manager.lock().await;
    manager.reset_to_defaults().await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_network_config(
    network_config: NetworkConfig,
    config_manager: tauri::State<'_, tokio::sync::Mutex<BitTorrentConfigManager>>
) -> Result<(), String> {
    let mut manager = config_manager.lock().await;
    manager.update_network_config(network_config).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_rate_limits(
    rate_limits: RateLimitConfig,
    config_manager: tauri::State<'_, tokio::sync::Mutex<BitTorrentConfigManager>>
) -> Result<(), String> {
    let mut manager = config_manager.lock().await;
    manager.update_rate_limits(rate_limits).await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BitTorrentConfig::default();
        assert_eq!(config.network.listening_port, 0);
        assert!(config.dht.enabled);
        assert!(config.rate_limits.download_rate_limit == 0); // Unlimited
    }

    #[test]
    fn test_config_serialization() {
        let config = BitTorrentConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: BitTorrentConfig = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(config.network.listening_port, deserialized.network.listening_port);
        assert_eq!(config.dht.enabled, deserialized.dht.enabled);
    }
}