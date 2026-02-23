// Library exports for testing
pub mod protocols;
pub mod analytics;
pub mod bandwidth;
pub mod config; 
pub mod control_plane;
pub mod multi_source_download;
pub mod download_restart;
pub mod p2p_download_recovery;
pub mod transfer_events;

// Connection retry and resilience framework
pub mod connection_retry;

// Download source abstraction
pub mod download_source;
pub mod download_scheduler;
pub mod download_persistence;
pub mod ftp_client;
pub mod ftp_bookmarks;
pub mod ed2k_client;
pub mod http_download;
pub mod bittorrent_handler;
pub mod chiral_bittorrent_extension;
pub mod download_paths;

// Required modules for multi_source_download
pub mod dht;
pub mod file_transfer;
pub mod ftp_downloader;
#[cfg(feature = "ftp-server")]
pub mod ftp_server;

/// No-op FTP server stub used when the `ftp-server` feature is disabled.
/// Provides the same public API so all callers compile unchanged.
#[cfg(not(feature = "ftp-server"))]
pub mod ftp_server {
    use std::path::PathBuf;

    pub struct FtpServer {
        root_dir: PathBuf,
        port: u16,
    }

    impl FtpServer {
        pub fn new(root_dir: PathBuf, port: u16) -> Self {
            Self { root_dir, port }
        }
        pub fn port(&self) -> u16 { self.port }
        pub fn root_dir(&self) -> &PathBuf { &self.root_dir }
        pub async fn is_running(&self) -> bool { false }
        pub async fn start(&self) -> Result<(), String> { Ok(()) }
        pub async fn stop(&self) -> Result<(), String> { Ok(()) }
        pub fn get_file_url(&self, file_name: &str) -> String {
            format!("ftp://localhost:{}/{}", self.port, file_name)
        }
        pub async fn add_file(&self, _src: &PathBuf, file_name: &str) -> Result<String, String> {
            Ok(self.get_file_url(file_name))
        }
        pub async fn add_file_data(&self, _data: &[u8], file_name: &str) -> Result<String, String> {
            Ok(self.get_file_url(file_name))
        }
        pub async fn remove_file(&self, _file_name: &str) -> Result<(), String> { Ok(()) }
    }
}
pub mod peer_selection;
pub mod peer_cache;
pub mod peer_cache_runtime;
pub mod webrtc_service;

// Required modules for encryption and keystore functionality
pub mod encryption;
pub mod keystore;
pub mod manager;

// P2P chunk network - real network integration for recovery
pub mod p2p_chunk_network;

// Proxy latency optimization module
pub mod proxy_latency;

// Stream authentication module
pub mod stream_auth;
// Reputation system
pub mod reputation;
// Payment checkpoint module
pub mod payment_checkpoint;

// Logger module for file-based logging
pub mod logger;

// Chunk scheduler for multi-peer downloads
pub mod chunk_scheduler;

// Ethereum/Geth integration
pub mod ethereum;
pub mod geth_downloader;
pub mod geth_bootstrap;
