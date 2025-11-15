use crate::protocols::ProtocolHandler;
use async_trait::async_trait;
use librqbit::{AddTorrent, Session, ManagedTorrent, SessionOptions};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::{error, info, instrument, warn};
use thiserror::Error;

/// Custom error type for BitTorrent operations
#[derive(Debug, Error, Clone)]
pub enum BitTorrentError {
    /// Session initialization failed
    #[error("Failed to initialize BitTorrent session: {message}")]
    SessionInit { message: String },

    /// Invalid magnet link format
    #[error("Invalid magnet link format: {url}")]
    InvalidMagnetLink { url: String },

    /// Torrent file not found or invalid
    #[error("Torrent file error: {message}")]
    TorrentFileError { message: String },

    /// File system error (file not found, permission denied, etc.)
    #[error("File system error: {message}")]
    FileSystemError { message: String },

    /// Network/connection error
    #[error("Network error during BitTorrent operation: {message}")]
    NetworkError { message: String },

    /// Torrent parsing error
    #[error("Failed to parse torrent: {message}")]
    TorrentParsingError { message: String },

    /// Download timeout
    #[error("Download timed out after {timeout_secs} seconds")]
    DownloadTimeout { timeout_secs: u64 },

    /// Seeding operation failed
    #[error("Seeding failed: {message}")]
    SeedingError { message: String },

    /// Torrent handle unavailable
    #[error("Torrent handle is not available")]
    HandleUnavailable,

    /// Generic I/O error
    #[error("I/O error: {message}")]
    IoError {
        message: String,
    },

    /// Configuration error
    #[error("Configuration error: {message}")]
    ConfigError { message: String },

    /// Torrent already exists
    #[error("Torrent already exists: {info_hash}")]
    TorrentExists { info_hash: String },

    /// Unknown error from librqbit
    #[error("Unknown BitTorrent error: {message}")]
    Unknown { message: String },
}

impl From<std::io::Error> for BitTorrentError {
    fn from(err: std::io::Error) -> Self {
        BitTorrentError::IoError { message: err.to_string() }
    }
}

impl BitTorrentError {
    /// Convert to user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            BitTorrentError::SessionInit { .. } => {
                "Failed to start BitTorrent engine. Please check your download directory permissions.".to_string()
            }
            BitTorrentError::InvalidMagnetLink { .. } => {
                "The magnet link format is invalid. Please check the link and try again.".to_string()
            }
            BitTorrentError::TorrentFileError { .. } => {
                "The torrent file is invalid or corrupted. Please try a different torrent file.".to_string()
            }
            BitTorrentError::FileSystemError { .. } => {
                "File system error occurred. Please check file permissions and available disk space.".to_string()
            }
            BitTorrentError::NetworkError { .. } => {
                "Network connection failed. Please check your internet connection and firewall settings.".to_string()
            }
            BitTorrentError::TorrentParsingError { .. } => {
                "Failed to parse the torrent. The torrent file may be corrupted or incompatible.".to_string()
            }
            BitTorrentError::DownloadTimeout { timeout_secs } => {
                format!("Download timed out after {} seconds. No peers may be available for this torrent.", timeout_secs)
            }
            BitTorrentError::SeedingError { .. } => {
                "Failed to start seeding. Please check that the file exists and is accessible.".to_string()
            }
            BitTorrentError::HandleUnavailable => {
                "Torrent is no longer available. It may have been removed or completed.".to_string()
            }
            BitTorrentError::IoError { .. } => {
                "File system operation failed. Please check permissions and available disk space.".to_string()
            }
            BitTorrentError::ConfigError { .. } => {
                "BitTorrent configuration error. Please check your settings.".to_string()
            }
            BitTorrentError::TorrentExists { .. } => {
                "This torrent is already being downloaded or seeded.".to_string()
            }
            BitTorrentError::Unknown { .. } => {
                "An unexpected error occurred. Please try again or contact support if the issue persists.".to_string()
            }
        }
    }

    /// Get error category for logging/analytics
    pub fn category(&self) -> &'static str {
        match self {
            BitTorrentError::SessionInit { .. } => "session",
            BitTorrentError::InvalidMagnetLink { .. } => "validation",
            BitTorrentError::TorrentFileError { .. } => "validation",
            BitTorrentError::FileSystemError { .. } => "filesystem",
            BitTorrentError::NetworkError { .. } => "network",
            BitTorrentError::TorrentParsingError { .. } => "parsing",
            BitTorrentError::DownloadTimeout { .. } => "timeout",
            BitTorrentError::SeedingError { .. } => "seeding",
            BitTorrentError::HandleUnavailable => "state",
            BitTorrentError::IoError { .. } => "filesystem",
            BitTorrentError::ConfigError { .. } => "config",
            BitTorrentError::TorrentExists { .. } => "state",
            BitTorrentError::Unknown { .. } => "unknown",
        }
    }
}

/// Events sent by the BitTorrent download monitor
#[derive(Debug)]
pub enum BitTorrentEvent {
    /// Download progress update
    Progress { downloaded: u64, total: u64 },
    /// Download has completed successfully
    Completed,
    /// Download has failed
    Failed(BitTorrentError),
}

/// Convert BitTorrentError to String for compatibility with ProtocolHandler trait
impl From<BitTorrentError> for String {
    fn from(error: BitTorrentError) -> Self {
        error.user_message()
    }
}

/// BitTorrent protocol handler implementing the ProtocolHandler trait.
/// This handler manages BitTorrent downloads and seeding operations using librqbit.
#[derive(Clone)]
pub struct BitTorrentHandler {
    rqbit_session: Arc<Session>,
    download_directory: std::path::PathBuf,
}

impl BitTorrentHandler {
    /// Creates a new BitTorrentHandler with the specified download directory.
    pub async fn new(download_directory: std::path::PathBuf) -> Result<Self, BitTorrentError> {
        Self::new_with_port_range(download_directory, None).await
    }

    /// Creates a new BitTorrentHandler with a specific port range to avoid conflicts.
    pub async fn new_with_port_range(
        download_directory: std::path::PathBuf,
        listen_port_range: Option<std::ops::Range<u16>>,
    ) -> Result<Self, BitTorrentError> {
        let mut opts = SessionOptions::default();
        
        // Set port range if provided (helps run multiple instances)
        if let Some(range) = listen_port_range {
            opts.listen_port_range = Some(range);
        }
        
        // Use instance-specific DHT config if available (for multiple instances)
        // The DHT state file will be stored in the download directory
        opts.dht_config = Some(librqbit::dht::PersistentDhtConfig {
            config_filename: Some(download_directory.join("dht.json")),
            dump_interval: Some(Duration::from_secs(300)), // Save DHT state every 5 minutes
        });
        
        let session = Session::new_with_opts(download_directory.clone(), opts).await.map_err(|e| {
            BitTorrentError::SessionInit {
                message: format!("Failed to create session: {}", e),
            }
        })?;
        
        info!(
            "Initializing BitTorrentHandler with download directory: {:?}",
            download_directory
        );
        Ok(Self {
            rqbit_session: session,
            download_directory,
        })
    }

    /// Starts a download and returns a handle to the torrent.
    /// This method is non-blocking.
    pub async fn start_download(
        &self,
        identifier: &str,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        info!("Starting BitTorrent download for: {}", identifier);

        let add_torrent = if identifier.starts_with("magnet:") {
            Self::validate_magnet_link(identifier).map_err(|e| {
                error!("Magnet link validation failed: {}", e);
                e
            })?;
            AddTorrent::from_url(identifier)
        } else {
            Self::validate_torrent_file(identifier).map_err(|e| {
                error!("Torrent file validation failed: {}", e);
                e
            })?;
            AddTorrent::from_local_filename(identifier).map_err(|e| {
                error!("Failed to load torrent file: {}", e);
                BitTorrentError::TorrentFileError {
                    message: format!("Cannot read torrent file {}: {}", identifier, e),
                }
            })?
        };

        let add_torrent_response = self
            .rqbit_session
            .add_torrent(add_torrent, None)
            .await
            .map_err(|e| {
                error!("Failed to add torrent to session: {}", e);
                Self::map_generic_error(e)
            })?;

        let handle = add_torrent_response
            .into_handle()
            .ok_or(BitTorrentError::HandleUnavailable)?;

        Ok(handle)
    }

    /// Monitors a torrent download and sends progress events.
    pub async fn monitor_download(
        &self,
        handle: Arc<ManagedTorrent>,
        event_tx: mpsc::Sender<BitTorrentEvent>,
    ) {
        let mut interval = time::interval(Duration::from_secs(1));
        let mut no_progress_count = 0;
        const MAX_NO_PROGRESS_ITERATIONS: u32 = 300; // 5 minutes with 1-second intervals

        loop {
            interval.tick().await;
            let stats = handle.stats();
            let downloaded = stats.progress_bytes;
            let total = stats.total_bytes;

            if event_tx.is_closed() {
                error!("Failed to send progress event, receiver dropped.");
                return;
            }

            if let Err(_) = event_tx
                .send(BitTorrentEvent::Progress { downloaded, total })
                .await
            {
                error!("Failed to send progress event, receiver dropped.");
                return;
            }

            // Check for completion
            if total > 0 && downloaded >= total {
                info!("Download completed for torrent");
                let _ = event_tx.send(BitTorrentEvent::Completed).await;
                return;
            }

            // Check for timeout (no progress for extended period)
            if downloaded == 0 {
                no_progress_count += 1;
                if no_progress_count >= MAX_NO_PROGRESS_ITERATIONS {
                    error!(
                        "Download timeout: no progress after {} seconds",
                        MAX_NO_PROGRESS_ITERATIONS
                    );
                    let _ = event_tx
                        .send(BitTorrentEvent::Failed(BitTorrentError::DownloadTimeout {
                            timeout_secs: MAX_NO_PROGRESS_ITERATIONS as u64,
                        }))
                        .await;
                    return;
                }
            } else {
                no_progress_count = 0; // Reset counter when progress is made
            }
        }
    }

    /// Validate magnet link format
    fn validate_magnet_link(url: &str) -> Result<(), BitTorrentError> {
        if !url.starts_with("magnet:?xt=urn:btih:") {
            return Err(BitTorrentError::InvalidMagnetLink {
                url: url.to_string(),
            });
        }

        // Extract info hash to validate length
        if let Some(hash_start) = url.find("urn:btih:") {
            let hash_start = hash_start + 9; // Length of "urn:btih:"
            let hash_end = url[hash_start..]
                .find('&')
                .unwrap_or(url.len() - hash_start)
                + hash_start;
            let hash = &url[hash_start..hash_end];

            // Check hash length (40 chars for SHA-1, 64 for SHA-256)
            if hash.len() != 40 && hash.len() != 64 {
                return Err(BitTorrentError::InvalidMagnetLink {
                    url: url.to_string(),
                });
            }

            // Check if hash contains only hex characters
            if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(BitTorrentError::InvalidMagnetLink {
                    url: url.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Validate torrent file path
    fn validate_torrent_file(path: &str) -> Result<(), BitTorrentError> {
        let file_path = Path::new(path);

        if !file_path.exists() {
            return Err(BitTorrentError::TorrentFileError {
                message: format!("Torrent file not found: {}", path),
            });
        }

        if !file_path.is_file() {
            return Err(BitTorrentError::TorrentFileError {
                message: format!("Path is not a file: {}", path),
            });
        }

        if !path.ends_with(".torrent") {
            return Err(BitTorrentError::TorrentFileError {
                message: format!("File does not have .torrent extension: {}", path),
            });
        }

        Ok(())
    }

    /// Map generic errors to our custom error type
    fn map_generic_error(error: impl std::fmt::Display) -> BitTorrentError {
        let error_msg = error.to_string();

        if error_msg.contains("network") || error_msg.contains("connection") {
            BitTorrentError::NetworkError {
                message: error_msg,
            }
        } else if error_msg.contains("timeout") {
            BitTorrentError::DownloadTimeout { timeout_secs: 30 }
        } else if error_msg.contains("parse") || error_msg.contains("invalid") {
            BitTorrentError::TorrentParsingError {
                message: error_msg.clone(),
            }
        } else {
            BitTorrentError::Unknown {
                message: error_msg,
            }
        }
    }
}

#[async_trait]
impl ProtocolHandler for BitTorrentHandler {
    fn name(&self) -> &'static str {
        "bittorrent"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("magnet:") || identifier.ends_with(".torrent")
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn download(&self, identifier: &str) -> Result<(), String> {
        let handle = self.start_download(identifier).await?;
        let (tx, mut rx) = mpsc::channel(10);

        let self_arc = Arc::new(self.clone());
        tokio::spawn(async move {
            self_arc.monitor_download(handle, tx).await;
        });

        while let Some(event) = rx.recv().await {
            match event {
                BitTorrentEvent::Completed => return Ok(()),
                BitTorrentEvent::Failed(e) => return Err(e.into()),
                _ => {}
            }
        }
        Err("Monitoring channel closed unexpectedly.".to_string())
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn seed(&self, file_path: &str) -> Result<String, String> {
        info!("Starting to seed file: {}", file_path);

        let path = Path::new(file_path);
        if !path.exists() {
            let error = BitTorrentError::FileSystemError {
                message: format!("File does not exist: {}", file_path),
            };
            error!("Seeding failed: {}", error);
            return Err(error.into());
        }

        if !path.is_file() {
            let error = BitTorrentError::FileSystemError {
                message: format!("Path is not a file: {}", file_path),
            };
            error!("Seeding failed: {}", error);
            return Err(error.into());
        }

        // TODO: Implement actual seeding with rqbit
        // This would involve:
        // 1. Creating a .torrent file with proper metadata
        // 2. Adding the torrent to the session for seeding
        // 3. Returning the actual magnet link

        warn!("Seeding functionality not yet implemented");
        Err(BitTorrentError::SeedingError {
            message: "Seeding functionality is not yet implemented".to_string(),
        }
        .into())
    }
}

// Helper functions for error mapping and validation
impl BitTorrentHandler {
    /// Check if string is a valid magnet link
    pub fn is_magnet_link(url: &str) -> bool {
        Self::validate_magnet_link(url).is_ok()
    }

    /// Check if path points to a valid torrent file
    pub fn is_torrent_file(path: &str) -> bool {
        Self::validate_torrent_file(path).is_ok()
    }

    /// Extract info hash from magnet link
    pub fn extract_info_hash(magnet: &str) -> Option<String> {
        if let Ok(_) = Self::validate_magnet_link(magnet) {
            if let Some(hash_start) = magnet.find("urn:btih:") {
                let hash_start = hash_start + 9;
                let hash_end = magnet[hash_start..]
                    .find('&')
                    .unwrap_or(magnet.len() - hash_start)
                    + hash_start;
                Some(magnet[hash_start..hash_end].to_string())
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;
    use std::io::Write;

    fn create_test_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
        let file_path = dir.join(name);
        let mut file = File::create(&file_path).unwrap();
        write!(file, "{}", content).unwrap();
        file_path
    }

    #[test]
    fn test_validate_magnet_link_valid() {
        assert!(BitTorrentHandler::validate_magnet_link(
            "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678"
        )
        .is_ok());
        assert!(BitTorrentHandler::validate_magnet_link(
            "magnet:?xt=urn:btih:ABCDEF1234567890ABCDEF1234567890ABCDEF12&dn=test"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_magnet_link_invalid() {
        assert!(BitTorrentHandler::validate_magnet_link("http://example.com").is_err());
        assert!(BitTorrentHandler::validate_magnet_link("magnet:?xt=urn:btih:invalid").is_err());
        assert!(BitTorrentHandler::validate_magnet_link("magnet:?xt=urn:btih:123").is_err());
        // Too short
    }

    #[test]
    fn test_validate_torrent_file() {
        let temp_dir = tempdir().unwrap();
        let torrent_path = create_test_file(temp_dir.path(), "test.torrent", "content");

        assert!(
            BitTorrentHandler::validate_torrent_file(torrent_path.to_str().unwrap()).is_ok()
        );
        assert!(BitTorrentHandler::validate_torrent_file("/nonexistent/file.torrent").is_err());

        let txt_path = create_test_file(temp_dir.path(), "test.txt", "content");
        assert!(BitTorrentHandler::validate_torrent_file(txt_path.to_str().unwrap()).is_err());
    }

    #[test]
    fn test_error_user_messages() {
        let error = BitTorrentError::InvalidMagnetLink {
            url: "invalid".to_string(),
        };
        assert!(error.user_message().contains("magnet link format"));

        let error = BitTorrentError::NetworkError {
            message: "connection failed".to_string(),
        };
        assert!(error.user_message().contains("Network connection failed"));
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(
            BitTorrentError::InvalidMagnetLink {
                url: "test".to_string()
            }
            .category(),
            "validation"
        );
        assert_eq!(
            BitTorrentError::NetworkError {
                message: "test".to_string()
            }
            .category(),
            "network"
        );
        assert_eq!(
            BitTorrentError::FileSystemError {
                message: "test".to_string()
            }
            .category(),
            "filesystem"
        );
    }
}