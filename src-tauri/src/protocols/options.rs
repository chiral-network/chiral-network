//! Unified Options and Result Structures for Protocol Manager
//!
//! This module defines the high-level structures for the unified download/upload API.
//! These structures provide a consistent interface across all protocols while allowing
//! protocol-specific configuration through the `protocol_specific` field.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// Re-export detection preferences from detection module
pub use crate::protocols::detection::DetectionPreferences;

/// Unified options for file transfer operations
///
/// This structure provides a protocol-agnostic interface for initiating downloads
/// or uploads. The protocol manager will automatically select the best protocol
/// based on the identifier and preferences, or use the explicitly specified protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferOptions {
    /// Output path for downloads (required for downloads)
    pub output_path: Option<PathBuf>,

    /// Explicitly specify which protocol to use (optional)
    /// If None, the protocol manager will auto-detect the best protocol
    /// Examples: "bittorrent", "http", "ftp", "ed2k"
    pub protocol: Option<String>,

    /// Enable multi-source download if multiple protocols are available
    pub enable_multi_source: bool,

    /// Maximum number of concurrent connections/peers
    pub max_connections: Option<usize>,

    /// Chunk size in bytes for chunked transfers
    pub chunk_size: Option<usize>,

    /// Enable encryption for the transfer
    pub encryption: bool,

    /// Bandwidth limit in bytes per second (0 or None = unlimited)
    pub bandwidth_limit: Option<u64>,

    /// Preferences for protocol auto-detection
    pub detection_preferences: Option<DetectionPreferences>,

    /// Protocol-specific options as key-value pairs
    /// Examples:
    /// - For BitTorrent: {"dht": "true", "pex": "true"}
    /// - For FTP: {"passive_mode": "true", "username": "user"}
    /// - For ED2K: {"server_url": "ed2k://...", "obfuscation": "true"}
    pub protocol_specific: HashMap<String, String>,

    /// Whether to announce to DHT for peer discovery (P2P protocols)
    pub announce_dht: bool,

    /// Maximum upload slots for seeding operations
    pub upload_slots: Option<usize>,
}

impl Default for FileTransferOptions {
    fn default() -> Self {
        Self {
            output_path: None,
            protocol: None,
            enable_multi_source: true,
            max_connections: Some(50),
            chunk_size: Some(256 * 1024), // 256KB default
            encryption: false,
            bandwidth_limit: None,
            detection_preferences: None,
            protocol_specific: HashMap::new(),
            announce_dht: true,
            upload_slots: Some(4),
        }
    }
}

impl FileTransferOptions {
    /// Create options optimized for speed
    pub fn optimized_for_speed() -> Self {
        Self {
            enable_multi_source: true,
            max_connections: Some(100),
            chunk_size: Some(512 * 1024), // Larger chunks
            encryption: false, // Disabled for speed
            ..Default::default()
        }
    }

    /// Create options optimized for privacy
    pub fn optimized_for_privacy() -> Self {
        Self {
            enable_multi_source: false, // Single source to reduce exposure
            max_connections: Some(10),
            encryption: true,
            detection_preferences: Some(DetectionPreferences {
                require_encryption: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Create options for low bandwidth scenarios
    pub fn optimized_for_bandwidth() -> Self {
        Self {
            enable_multi_source: false,
            max_connections: Some(5),
            chunk_size: Some(64 * 1024), // Smaller chunks
            bandwidth_limit: Some(512 * 1024), // 512 KB/s limit
            ..Default::default()
        }
    }
}

/// Result of a file transfer operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResult {
    /// Unique identifier for this transfer
    pub transfer_id: String,

    /// Protocol(s) used for the transfer
    pub protocols: Vec<String>,

    /// File identifier (URL, magnet link, ed2k link, etc.)
    pub identifier: String,

    /// Output path where file was saved (for downloads)
    pub output_path: Option<PathBuf>,

    /// Total bytes transferred
    pub bytes_transferred: u64,

    /// Time taken in seconds
    pub duration_seconds: f64,

    /// Average speed in bytes per second
    pub average_speed: f64,

    /// Whether the transfer completed successfully
    pub success: bool,

    /// Error message if the transfer failed
    pub error: Option<String>,

    /// Unix timestamp when transfer started
    pub started_at: u64,

    /// Unix timestamp when transfer completed
    pub completed_at: u64,
}

/// Real-time progress information for an active transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    /// Unique identifier for this transfer
    pub transfer_id: String,

    /// File identifier (URL, magnet link, ed2k link, etc.)
    pub identifier: String,

    /// Current transfer status
    pub status: TransferStatus,

    /// Protocol(s) currently being used
    pub active_protocols: Vec<String>,

    /// Bytes transferred so far
    pub bytes_transferred: u64,

    /// Total bytes to transfer
    pub total_bytes: u64,

    /// Percentage completed (0-100)
    pub percentage: f64,

    /// Current transfer speed in bytes per second
    pub current_speed: f64,

    /// Estimated time remaining in seconds
    pub eta_seconds: Option<u64>,

    /// Number of active connections/peers
    pub active_connections: usize,

    /// Number of total available sources
    pub total_sources: usize,

    /// Output path for the file (for downloads)
    pub output_path: Option<PathBuf>,

    /// Unix timestamp when transfer started
    pub started_at: u64,

    /// Additional protocol-specific information
    pub protocol_info: HashMap<String, serde_json::Value>,
}

impl TransferProgress {
    /// Calculate progress percentage
    pub fn calculate_percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.bytes_transferred as f64 / self.total_bytes as f64) * 100.0
        }
    }

    /// Check if transfer is complete
    pub fn is_complete(&self) -> bool {
        matches!(self.status, TransferStatus::Completed)
    }

    /// Check if transfer is active
    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            TransferStatus::Downloading | TransferStatus::Uploading | TransferStatus::Connecting
        )
    }

    /// Check if transfer has failed
    pub fn has_failed(&self) -> bool {
        matches!(self.status, TransferStatus::Failed)
    }
}

/// Status of a file transfer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    /// Initializing transfer (detecting protocols, fetching metadata)
    Initializing,

    /// Connecting to sources
    Connecting,

    /// Actively downloading
    Downloading,

    /// Actively uploading/seeding
    Uploading,

    /// Transfer is paused
    Paused,

    /// Assembling chunks into final file
    Assembling,

    /// Transfer completed successfully
    Completed,

    /// Transfer failed
    Failed,

    /// Transfer was cancelled by user
    Cancelled,

    /// Queued for transfer
    Queued,
}

impl std::fmt::Display for TransferStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransferStatus::Initializing => write!(f, "Initializing"),
            TransferStatus::Connecting => write!(f, "Connecting"),
            TransferStatus::Downloading => write!(f, "Downloading"),
            TransferStatus::Uploading => write!(f, "Uploading"),
            TransferStatus::Paused => write!(f, "Paused"),
            TransferStatus::Assembling => write!(f, "Assembling"),
            TransferStatus::Completed => write!(f, "Completed"),
            TransferStatus::Failed => write!(f, "Failed"),
            TransferStatus::Cancelled => write!(f, "Cancelled"),
            TransferStatus::Queued => write!(f, "Queued"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_transfer_options_default() {
        let opts = FileTransferOptions::default();
        assert!(opts.enable_multi_source);
        assert_eq!(opts.max_connections, Some(50));
        assert_eq!(opts.chunk_size, Some(256 * 1024));
    }

    #[test]
    fn test_optimized_presets() {
        let speed_opts = FileTransferOptions::optimized_for_speed();
        assert!(speed_opts.enable_multi_source);
        assert!(!speed_opts.encryption);

        let privacy_opts = FileTransferOptions::optimized_for_privacy();
        assert!(privacy_opts.encryption);
        assert!(!privacy_opts.enable_multi_source);

        let bandwidth_opts = FileTransferOptions::optimized_for_bandwidth();
        assert!(bandwidth_opts.bandwidth_limit.is_some());
        assert_eq!(bandwidth_opts.max_connections, Some(5));
    }

    #[test]
    fn test_transfer_progress_percentage() {
        let mut progress = TransferProgress {
            transfer_id: "test".to_string(),
            identifier: "test".to_string(),
            status: TransferStatus::Downloading,
            active_protocols: vec!["http".to_string()],
            bytes_transferred: 50,
            total_bytes: 100,
            percentage: 0.0,
            current_speed: 1024.0,
            eta_seconds: Some(10),
            active_connections: 1,
            total_sources: 1,
            output_path: None,
            started_at: 0,
            protocol_info: HashMap::new(),
        };

        progress.percentage = progress.calculate_percentage();
        assert_eq!(progress.percentage, 50.0);
        assert!(progress.is_active());
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_transfer_status_display() {
        assert_eq!(TransferStatus::Downloading.to_string(), "Downloading");
        assert_eq!(TransferStatus::Completed.to_string(), "Completed");
        assert_eq!(TransferStatus::Failed.to_string(), "Failed");
    }
}
