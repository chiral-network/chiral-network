//! WebRTC Protocol Handler
//!
//! This module implements P2P file transfers using WebRTC data channels,
//! replacing the legacy Bitswap protocol for Chiral Network downloads.
//!
//! ## Protocol Flow (4 Steps)
//!
//! 1. **Locate**: Query DHT for file metadata and available peers
//! 2. **Handshake**: Establish WebRTC connection and negotiate terms (price, size)
//! 3. **Download**: Transfer file chunks via WebRTC data channel
//! 4. **Pay**: Submit payment transaction to seeder's wallet (bulk payment after download)
//!
//! ## Identifier Format
//!
//! Uses `chiral://` scheme for P2P downloads:
//! - Format: `chiral://sha256:<file_hash>`
//! - Example: `chiral://sha256:abc123def456...`

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus, ProtocolCapabilities,
    ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// WebRTC Protocol Handler for Chiral Network P2P file transfers
pub struct WebRtcProtocolHandler {
    /// Reference to the WebRTC service for managing connections
    webrtc_service: Arc<RwLock<Option<Arc<crate::webrtc_service::WebRTCService>>>>,
}

impl WebRtcProtocolHandler {
    /// Creates a new WebRTC protocol handler
    pub fn new() -> Self {
        info!("Initializing WebRTC protocol handler");
        Self {
            webrtc_service: Arc::new(RwLock::new(None)),
        }
    }

    /// Sets the WebRTC service instance
    pub async fn set_webrtc_service(&self, service: Arc<crate::webrtc_service::WebRTCService>) {
        let mut svc = self.webrtc_service.write().await;
        *svc = Some(service);
        info!("WebRTC service attached to protocol handler");
    }

    /// Checks if the identifier is a valid chiral:// URL
    fn is_chiral_identifier(identifier: &str) -> bool {
        identifier.starts_with("chiral://sha256:")
    }

    /// Validates that a string is a valid SHA-256 hash (64 hexadecimal characters)
    fn is_valid_sha256_hash(hash: &str) -> bool {
        hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Extracts the file hash from a chiral:// identifier and validates format
    fn extract_hash(identifier: &str) -> Option<String> {
        if let Some(hash_part) = identifier.strip_prefix("chiral://sha256:") {
            // Validate hash format (must be 64 hex characters)
            if Self::is_valid_sha256_hash(hash_part) {
                Some(hash_part.to_string())
            } else {
                warn!("Invalid SHA-256 hash format in identifier: {}", identifier);
                None
            }
        } else {
            None
        }
    }

    /// Calculates SHA-256 hash of a file using streaming to avoid loading entire file into memory
    async fn calculate_file_hash(file_path: &PathBuf) -> Result<String, ProtocolError> {
        // Open file for streaming
        let mut file = fs::File::open(file_path).await.map_err(|e| {
            ProtocolError::FileNotFound(format!("Failed to open file: {}", e))
        })?;

        // Calculate hash in chunks to avoid memory issues with large files
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 8192]; // 8KB buffer

        loop {
            let bytes_read = file.read(&mut buffer).await.map_err(|e| {
                ProtocolError::Internal(format!("Failed to read file during hashing: {}", e))
            })?;

            if bytes_read == 0 {
                break; // EOF reached
            }

            hasher.update(&buffer[..bytes_read]);
        }

        let result = hasher.finalize();
        Ok(hex::encode(result))
    }

    /// Creates a chiral:// identifier from a file hash
    fn create_identifier(file_hash: &str) -> String {
        format!("chiral://sha256:{}", file_hash)
    }
}

#[async_trait]
impl ProtocolHandler for WebRtcProtocolHandler {
    fn name(&self) -> &'static str {
        "webrtc"
    }

    fn supports(&self, identifier: &str) -> bool {
        Self::is_chiral_identifier(identifier)
    }

    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities {
            supports_seeding: true,
            supports_pause_resume: true,
            supports_multi_source: true,
            supports_encryption: true,
            supports_dht: true,
        }
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("WebRTC download requested for: {}", identifier);

        // Validate identifier
        if !self.supports(identifier) {
            return Err(ProtocolError::InvalidIdentifier(format!(
                "Invalid chiral:// identifier: {}",
                identifier
            )));
        }

        // Extract file hash
        let file_hash = Self::extract_hash(identifier).ok_or_else(|| {
            ProtocolError::InvalidIdentifier("Failed to extract hash from identifier".to_string())
        })?;

        debug!("Extracted file hash: {}", file_hash);

        // TODO: Implement 4-step download flow (Phase 2)
        // 1. Locate file and peers via DHT
        // 2. Handshake with selected peer
        // 3. Download chunks via WebRTC
        // 4. Process payment

        // Placeholder implementation
        Err(ProtocolError::Internal(
            "WebRTC download not yet implemented".to_string(),
        ))
    }

    async fn seed(
        &self,
        file_path: PathBuf,
        _options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        info!("WebRTC seeding requested for: {:?}", file_path);

        // Validate file exists
        if !file_path.exists() {
            return Err(ProtocolError::FileNotFound(format!(
                "File not found: {:?}",
                file_path
            )));
        }

        // Step 1: Calculate file hash
        let file_hash = Self::calculate_file_hash(&file_path).await?;
        debug!("Calculated file hash: {}", file_hash);

        // Step 2: Create chiral:// identifier
        let identifier = Self::create_identifier(&file_hash);
        info!("Created identifier: {}", identifier);

        // Step 3: Get file metadata
        let metadata = fs::metadata(&file_path).await.map_err(|e| {
            ProtocolError::Internal(format!("Failed to read file metadata: {}", e))
        })?;
        let file_size = metadata.len();

        // TODO (Phase 2): Integrate with DHT service to publish file metadata
        // For now, we just register locally and assume DHT integration will come later

        // Step 4: Return seeding info
        let seeding_info = SeedingInfo {
            identifier: identifier.clone(),
            file_path,
            protocol: "webrtc".to_string(),
            active_peers: 0, // No peers connected yet
            bytes_uploaded: 0, // No data uploaded yet
        };

        info!(
            "WebRTC seeding started: {} ({} bytes, hash: {})",
            identifier, file_size, file_hash
        );

        Ok(seeding_info)
    }

    async fn stop_seeding(&self, _identifier: &str) -> Result<(), ProtocolError> {
        info!("WebRTC stop seeding requested");

        // TODO: Implement stop seeding
        // 1. Remove file from seeding list
        // 2. Unpublish metadata from DHT

        Ok(())
    }

    async fn pause_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
        // TODO: Implement pause download
        Ok(())
    }

    async fn resume_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
        // TODO: Implement resume download
        Ok(())
    }

    async fn cancel_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
        // TODO: Implement cancel download
        Ok(())
    }

    async fn get_download_progress(&self, _identifier: &str) -> Result<DownloadProgress, ProtocolError> {
        // TODO: Implement progress tracking
        Ok(DownloadProgress {
            downloaded_bytes: 0,
            total_bytes: 0,
            download_speed: 0.0,
            eta_seconds: None,
            active_peers: 0,
            status: DownloadStatus::FetchingMetadata,
        })
    }

    async fn list_seeding(&self) -> Result<Vec<SeedingInfo>, ProtocolError> {
        // TODO: Implement list seeding
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_chiral_identifier_validation() {
        let handler = WebRtcProtocolHandler::new();

        // Valid identifier (64 hex characters)
        let valid_hash = "a".repeat(64);
        assert!(handler.supports(&format!("chiral://sha256:{}", valid_hash)));

        // Invalid identifiers
        assert!(!handler.supports("http://example.com/file.zip"));
        assert!(!handler.supports("magnet:?xt=urn:btih:..."));
        assert!(!handler.supports("chiral://md5:abc123")); // Wrong hash type
        assert!(!handler.supports("webrtc://sha256:abc123")); // Wrong scheme
        assert!(!handler.supports("chiral://sha256:abc")); // Too short (not 64 chars)
        assert!(!handler.supports("chiral://sha256:")); // Empty hash
    }

    #[test]
    fn test_hash_validation() {
        // Valid SHA-256 hashes (64 hex characters)
        assert!(WebRtcProtocolHandler::is_valid_sha256_hash(&"a".repeat(64)));
        assert!(WebRtcProtocolHandler::is_valid_sha256_hash(&"0123456789abcdef".repeat(4)));
        assert!(WebRtcProtocolHandler::is_valid_sha256_hash("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));

        // Invalid hashes
        assert!(!WebRtcProtocolHandler::is_valid_sha256_hash("abc")); // Too short
        assert!(!WebRtcProtocolHandler::is_valid_sha256_hash(&"a".repeat(65))); // Too long
        assert!(!WebRtcProtocolHandler::is_valid_sha256_hash(&"g".repeat(64))); // Invalid hex char
        assert!(!WebRtcProtocolHandler::is_valid_sha256_hash("xyz123")); // Invalid chars
        assert!(!WebRtcProtocolHandler::is_valid_sha256_hash("")); // Empty
    }

    #[test]
    fn test_hash_extraction() {
        // Valid extraction (with proper hash format)
        let valid_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            WebRtcProtocolHandler::extract_hash(&format!("chiral://sha256:{}", valid_hash)),
            Some(valid_hash.to_string())
        );

        // Invalid extractions
        assert_eq!(
            WebRtcProtocolHandler::extract_hash("chiral://sha256:abc"), // Too short
            None
        );
        assert_eq!(
            WebRtcProtocolHandler::extract_hash("chiral://sha256:"), // Empty
            None
        );
        assert_eq!(
            WebRtcProtocolHandler::extract_hash("http://example.com"),
            None
        );
        assert_eq!(
            WebRtcProtocolHandler::extract_hash(&format!("chiral://sha256:{}", "g".repeat(64))), // Invalid hex
            None
        );
    }

    #[test]
    fn test_identifier_creation() {
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let identifier = WebRtcProtocolHandler::create_identifier(hash);
        assert_eq!(identifier, format!("chiral://sha256:{}", hash));
    }

    #[tokio::test]
    async fn test_file_hash_calculation() {
        // Create a temporary file with known content
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Hello, World!").unwrap();
        temp_file.flush().unwrap();

        // Calculate hash
        let hash = WebRtcProtocolHandler::calculate_file_hash(&temp_file.path().to_path_buf())
            .await
            .unwrap();

        // Expected SHA-256 hash of "Hello, World!"
        let expected_hash = "dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f";
        assert_eq!(hash, expected_hash);
    }

    #[tokio::test]
    async fn test_file_hash_empty_file() {
        // Create an empty temporary file
        let temp_file = NamedTempFile::new().unwrap();

        // Calculate hash
        let hash = WebRtcProtocolHandler::calculate_file_hash(&temp_file.path().to_path_buf())
            .await
            .unwrap();

        // Expected SHA-256 hash of empty string
        let expected_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(hash, expected_hash);
    }

    #[tokio::test]
    async fn test_file_hash_nonexistent_file() {
        let result = WebRtcProtocolHandler::calculate_file_hash(&PathBuf::from("/nonexistent/file.txt")).await;
        assert!(result.is_err());
        match result {
            Err(ProtocolError::FileNotFound(_)) => {},
            _ => panic!("Expected FileNotFound error"),
        }
    }

    #[test]
    fn test_capabilities() {
        let handler = WebRtcProtocolHandler::new();
        let caps = handler.capabilities();

        assert!(caps.supports_seeding);
        assert!(caps.supports_pause_resume);
        assert!(caps.supports_multi_source);
        assert!(caps.supports_encryption);
        assert!(caps.supports_dht);
    }
}
