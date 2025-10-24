// Library exports for testing
pub mod analytics;
pub mod protocols;
pub mod multi_source_download;

// Required modules for multi_source_download
pub mod dht;
pub mod file_transfer;
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
