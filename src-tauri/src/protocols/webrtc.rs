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
use tokio::sync::RwLock;
use tracing::{debug, info};

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

    /// Extracts the file hash from a chiral:// identifier
    fn extract_hash(identifier: &str) -> Option<String> {
        if let Some(hash_part) = identifier.strip_prefix("chiral://sha256:") {
            Some(hash_part.to_string())
        } else {
            None
        }
    }

    /// Calculates SHA-256 hash of a file
    async fn calculate_file_hash(file_path: &PathBuf) -> Result<String, ProtocolError> {
        // Read file contents
        let data = fs::read(file_path).await.map_err(|e| {
            ProtocolError::FileNotFound(format!("Failed to read file: {}", e))
        })?;

        // Calculate SHA-256 hash
        let mut hasher = Sha256::new();
        hasher.update(&data);
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

    #[test]
    fn test_chiral_identifier_validation() {
        let handler = WebRtcProtocolHandler::new();

        // Valid identifiers
        assert!(handler.supports("chiral://sha256:abc123"));
        assert!(handler.supports("chiral://sha256:0123456789abcdef"));

        // Invalid identifiers
        assert!(!handler.supports("http://example.com/file.zip"));
        assert!(!handler.supports("magnet:?xt=urn:btih:..."));
        assert!(!handler.supports("chiral://md5:abc123")); // Wrong hash type
        assert!(!handler.supports("webrtc://sha256:abc123")); // Wrong scheme
    }

    #[test]
    fn test_hash_extraction() {
        assert_eq!(
            WebRtcProtocolHandler::extract_hash("chiral://sha256:abc123"),
            Some("abc123".to_string())
        );
        assert_eq!(
            WebRtcProtocolHandler::extract_hash("http://example.com"),
            None
        );
    }
}
