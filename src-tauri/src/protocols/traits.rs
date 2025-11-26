//! Enhanced Protocol Handler Traits
//!
//! This module defines the core traits for protocol handlers in the Chiral Network.
//! Each protocol (BitTorrent, HTTP, FTP, ED2K) implements these traits to provide
//! a consistent interface for file downloads and seeding operations.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Options for initiating a download
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadOptions {
    /// Output path for the downloaded file
    pub output_path: PathBuf,
    /// Maximum number of peers to connect to (for P2P protocols)
    pub max_peers: Option<usize>,
    /// Chunk size in bytes (for chunked downloads)
    pub chunk_size: Option<usize>,
    /// Enable encryption for transfer
    pub encryption: bool,
    /// Bandwidth limit in bytes per second (0 = unlimited)
    pub bandwidth_limit: Option<u64>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("."),
            max_peers: None,
            chunk_size: None,
            encryption: false,
            bandwidth_limit: None,
        }
    }
}

/// Options for seeding a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedOptions {
    /// Announce to DHT for peer discovery
    pub announce_dht: bool,
    /// Enable encryption for transfers
    pub enable_encryption: bool,
    /// Maximum upload slots
    pub upload_slots: Option<usize>,
}

impl Default for SeedOptions {
    fn default() -> Self {
        Self {
            announce_dht: true,
            enable_encryption: false,
            upload_slots: None,
        }
    }
}

/// Progress information for an ongoing download
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total bytes to download
    pub total_bytes: u64,
    /// Current download speed in bytes per second
    pub download_speed: f64,
    /// Estimated time remaining in seconds
    pub eta_seconds: Option<u64>,
    /// Number of active peers/connections
    pub active_peers: usize,
    /// Download status
    pub status: DownloadStatus,
}

/// Status of a download
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DownloadStatus {
    /// Fetching metadata (magnet resolution, etc.)
    FetchingMetadata,
    /// Actively downloading
    Downloading,
    /// Download is paused
    Paused,
    /// Assembling chunks into final file
    Assembling,
    /// Download completed successfully
    Completed,
    /// Download failed
    Failed,
    /// Download was cancelled
    Cancelled,
}

/// Information about a file being seeded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedingInfo {
    /// Identifier for others to download (magnet link, URL, ed2k link, etc.)
    pub identifier: String,
    /// Path to the file being seeded
    pub file_path: PathBuf,
    /// Protocol name
    pub protocol: String,
    /// Number of active peers downloading
    pub active_peers: usize,
    /// Total bytes uploaded
    pub bytes_uploaded: u64,
}

/// Handle returned when starting a download
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadHandle {
    /// Unique identifier for this download
    pub identifier: String,
    /// Protocol used
    pub protocol: String,
    /// Unix timestamp when download started
    pub started_at: u64,
}

/// Capabilities supported by a protocol handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolCapabilities {
    /// Can seed files to other peers
    pub supports_seeding: bool,
    /// Can pause and resume downloads
    pub supports_pause_resume: bool,
    /// Can download from multiple sources simultaneously
    pub supports_multi_source: bool,
    /// Supports encrypted transfers
    pub supports_encryption: bool,
    /// Uses DHT for peer discovery
    pub supports_dht: bool,
}

impl Default for ProtocolCapabilities {
    fn default() -> Self {
        Self {
            supports_seeding: false,
            supports_pause_resume: true,
            supports_multi_source: false,
            supports_encryption: false,
            supports_dht: false,
        }
    }
}

/// Errors that can occur during protocol operations
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum ProtocolError {
    /// Network-related error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// File not found
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Invalid identifier format
    #[error("Invalid identifier: {0}")]
    InvalidIdentifier(String),

    /// Protocol-specific error
    #[error("Protocol error: {0}")]
    ProtocolSpecific(String),

    /// Operation timed out
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Operation not supported by this protocol
    #[error("Operation not supported")]
    NotSupported,

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Download not found
    #[error("Download not found: {0}")]
    DownloadNotFound(String),

    /// Already exists
    #[error("Already exists: {0}")]
    AlreadyExists(String),
}

// =============================================================================
// Legacy Traits (for backward compatibility with existing handlers)
// =============================================================================

/// Simple protocol handler trait (legacy)
///
/// This trait provides a simpler interface for backward compatibility with
/// existing protocol handlers like BitTorrentHandler.
#[async_trait]
pub trait SimpleProtocolHandler: Send + Sync {
    /// Returns the name of the protocol (e.g., "bittorrent", "http").
    fn name(&self) -> &'static str;

    /// Determines if this handler can process the given identifier.
    fn supports(&self, identifier: &str) -> bool;

    /// Initiates a download for the given identifier.
    async fn download(&self, identifier: &str) -> Result<(), String>;

    /// Starts seeding a file and returns an identifier for others to use.
    async fn seed(&self, file_path: &str) -> Result<String, String>;
}

/// Manages multiple simple protocol handlers (legacy)
pub struct SimpleProtocolManager {
    handlers: Vec<std::sync::Arc<dyn SimpleProtocolHandler>>,
}

impl SimpleProtocolManager {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn register(&mut self, handler: std::sync::Arc<dyn SimpleProtocolHandler>) {
        self.handlers.push(handler);
    }

    pub async fn download(&self, identifier: &str) -> Result<(), String> {
        for handler in &self.handlers {
            if handler.supports(identifier) {
                return handler.download(identifier).await;
            }
        }
        Err(format!("No protocol handler found for: {}", identifier))
    }

    pub async fn seed(&self, file_path: &str) -> Result<String, String> {
        if let Some(handler) = self.handlers.first() {
            return handler.seed(file_path).await;
        }
        Err(format!("No protocol handler available for: {}", file_path))
    }
}

impl Default for SimpleProtocolManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Enhanced Protocol Handler Trait
// =============================================================================

/// Core trait that all protocol handlers must implement
///
/// This trait provides a unified interface for different download protocols
/// (BitTorrent, HTTP, FTP, ED2K) while allowing protocol-specific capabilities.
#[async_trait]
pub trait ProtocolHandler: Send + Sync {
    /// Returns the protocol name (e.g., "bittorrent", "http", "ftp", "ed2k")
    fn name(&self) -> &'static str;

    /// Checks if this handler can process the given identifier
    ///
    /// Examples:
    /// - BitTorrent: `magnet:?xt=urn:btih:...` or `.torrent` file path
    /// - HTTP: `http://` or `https://`
    /// - FTP: `ftp://` or `ftps://`
    /// - ED2K: `ed2k://|file|...`
    fn supports(&self, identifier: &str) -> bool;

    /// Initiates a download
    ///
    /// Returns a handle that can be used to track and control the download.
    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError>;

    /// Starts seeding a file
    ///
    /// Returns information including the identifier others can use to download.
    async fn seed(
        &self,
        file_path: PathBuf,
        options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError>;

    /// Stops seeding a file
    async fn stop_seeding(&self, identifier: &str) -> Result<(), ProtocolError>;

    /// Pauses an ongoing download
    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError>;

    /// Resumes a paused download
    async fn resume_download(&self, identifier: &str) -> Result<(), ProtocolError>;

    /// Cancels and removes a download
    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError>;

    /// Gets current download progress
    async fn get_download_progress(
        &self,
        identifier: &str,
    ) -> Result<DownloadProgress, ProtocolError>;

    /// Lists all currently seeding files for this protocol
    async fn list_seeding(&self) -> Result<Vec<SeedingInfo>, ProtocolError>;

    /// Returns the capabilities of this protocol handler
    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_options_default() {
        let opts = DownloadOptions::default();
        assert!(!opts.encryption);
        assert!(opts.max_peers.is_none());
    }

    #[test]
    fn test_protocol_capabilities_default() {
        let caps = ProtocolCapabilities::default();
        assert!(!caps.supports_seeding);
        assert!(caps.supports_pause_resume);
    }

    #[test]
    fn test_protocol_error_display() {
        let err = ProtocolError::NetworkError("connection refused".to_string());
        assert!(err.to_string().contains("connection refused"));
    }
}
