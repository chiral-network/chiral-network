// Library exports for testing
pub mod analytics;
pub mod bandwidth;
pub mod control_plane;
pub mod multi_source_download;
pub mod download_restart;

// Download source abstraction
pub mod download_source;
pub mod download_scheduler;
pub mod download_persistence;
pub mod ftp_client;
pub mod ed2k_client;

// Required modules for multi_source_download
pub mod dht;
pub mod file_transfer;
pub mod ftp_downloader;
pub mod peer_selection;
pub mod webrtc_service;

// Required modules for encryption and keystore functionality
pub mod encryption;
pub mod keystore;
pub mod manager;

// Proxy latency optimization module
pub mod proxy_latency;

// Stream authentication module
pub mod stream_auth;
// Reputation system
pub mod reputation;

// Logger module for file-based logging
pub mod logger;
