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
pub mod ftp_server;
pub mod peer_selection;
pub mod peer_cache;
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

// Ethereum/Geth integration
pub mod ethereum;
pub mod geth_downloader;
pub mod geth_bootstrap;