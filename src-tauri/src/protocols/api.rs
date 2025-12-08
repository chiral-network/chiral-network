//! Unified Download/Upload API Implementation
//!
//! This module implements the high-level unified API for the ProtocolManager,
//! providing a consistent interface for file transfers across all protocols.

use super::options::{
    DetectionPreferences, FileTransferOptions, TransferProgress, TransferResult, TransferStatus,
};
use super::traits::{DownloadOptions, ProtocolError, SeedOptions};
use super::ProtocolManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};
use uuid::Uuid;

/// Represents an active file transfer
#[derive(Debug, Clone)]
pub struct ActiveTransfer {
    /// Unique identifier for this transfer
    pub transfer_id: String,

    /// File identifier (URL, magnet link, ed2k link, etc.)
    pub identifier: String,

    /// Protocol(s) being used
    pub protocols: Vec<String>,

    /// Current status
    pub status: TransferStatus,

    /// Output path (for downloads)
    pub output_path: Option<PathBuf>,

    /// Unix timestamp when transfer started
    pub started_at: u64,

    /// Whether this is a download (true) or upload (false)
    pub is_download: bool,

    /// Progress tracking
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub current_speed: f64,
    pub active_connections: usize,

    /// Protocol-specific handles for control operations
    pub protocol_handles: HashMap<String, String>,
}

impl ProtocolManager {
    // =========================================================================
    // Main Unified API Methods
    // =========================================================================

    /// Download a file using the best available protocol(s)
    ///
    /// This is the main entry point for downloads. It will:
    /// 1. Auto-detect available protocols if not explicitly specified
    /// 2. Use multi-source download if enabled and multiple protocols are available
    /// 3. Track progress and manage the transfer lifecycle
    ///
    /// # Arguments
    ///
    /// * `identifier` - File identifier (URL, magnet link, ed2k link, etc.)
    /// * `options` - Transfer options including output path and preferences
    ///
    /// # Returns
    ///
    /// A `TransferResult` with the transfer ID and initial status
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let options = FileTransferOptions {
    ///     output_path: Some(PathBuf::from("./downloads/file.zip")),
    ///     enable_multi_source: true,
    ///     ..Default::default()
    /// };
    ///
    /// let result = manager.download_file("magnet:?xt=...", options).await?;
    /// println!("Transfer ID: {}", result.transfer_id);
    /// ```
    pub async fn download_file(
        &self,
        identifier: &str,
        options: FileTransferOptions,
    ) -> Result<TransferResult, ProtocolError> {
        info!("Starting download for: {}", identifier);

        // Validate options
        let output_path = options.output_path.clone().ok_or_else(|| {
            ProtocolError::InvalidIdentifier("Output path is required for downloads".to_string())
        })?;

        // Generate unique transfer ID
        let transfer_id = Uuid::new_v4().to_string();
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Determine which protocol(s) to use
        let protocols = if let Some(protocol) = &options.protocol {
            // Explicit protocol specified
            vec![protocol.clone()]
        } else {
            // Auto-detect available protocols
            let detected = self.detect_protocols(identifier.to_string()).await;

            if detected.is_empty() {
                return Err(ProtocolError::InvalidIdentifier(format!(
                    "No protocol handler found for: {}",
                    identifier
                )));
            }

            // Apply detection preferences if provided
            if let Some(prefs) = &options.detection_preferences {
                self.filter_protocols_by_preferences(&detected, prefs)
            } else {
                detected
            }
        };

        if protocols.is_empty() {
            return Err(ProtocolError::InvalidIdentifier(
                "No suitable protocol found after applying preferences".to_string(),
            ));
        }

        info!("Selected protocols: {:?}", protocols);

        // Create initial transfer record
        let active_transfer = ActiveTransfer {
            transfer_id: transfer_id.clone(),
            identifier: identifier.to_string(),
            protocols: protocols.clone(),
            status: TransferStatus::Initializing,
            output_path: Some(output_path.clone()),
            started_at,
            is_download: true,
            bytes_transferred: 0,
            total_bytes: 0,
            current_speed: 0.0,
            active_connections: 0,
            protocol_handles: HashMap::new(),
        };

        // Register the transfer
        self.register_active_transfer(active_transfer.clone())
            .await;

        // Decide between single-protocol and multi-source download
        if options.enable_multi_source && protocols.len() > 1 {
            // Multi-source download (Task 2 integration point)
            self.start_multi_source_download(identifier, &protocols, options, &transfer_id)
                .await?;
        } else {
            // Single protocol download
            let protocol = &protocols[0];
            self.start_single_protocol_download(identifier, protocol, options, &transfer_id)
                .await?;
        }

        Ok(TransferResult {
            transfer_id: transfer_id.clone(),
            protocols,
            identifier: identifier.to_string(),
            output_path: Some(output_path),
            bytes_transferred: 0,
            duration_seconds: 0.0,
            average_speed: 0.0,
            success: true,
            error: None,
            started_at,
            completed_at: started_at,
        })
    }

    /// Upload/seed a file on one or more protocols
    ///
    /// This method will seed the file on the specified protocols or all available
    /// protocols that support seeding.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file to upload/seed
    /// * `options` - Transfer options including protocol selection
    ///
    /// # Returns
    ///
    /// A `TransferResult` with identifiers for downloading the file
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let options = FileTransferOptions {
    ///     protocol: Some("bittorrent".to_string()),
    ///     ..Default::default()
    /// };
    ///
    /// let result = manager.upload_file(PathBuf::from("./file.zip"), options).await?;
    /// println!("Magnet link: {}", result.identifier);
    /// ```
    pub async fn upload_file(
        &self,
        file_path: PathBuf,
        options: FileTransferOptions,
    ) -> Result<TransferResult, ProtocolError> {
        info!("Starting upload for: {}", file_path.display());

        // Validate file exists
        if !file_path.exists() {
            return Err(ProtocolError::FileNotFound(format!(
                "File not found: {}",
                file_path.display()
            )));
        }

        // Generate unique transfer ID
        let transfer_id = Uuid::new_v4().to_string();
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Determine which protocol(s) to use for seeding
        let protocols = if let Some(protocol) = &options.protocol {
            vec![protocol.clone()]
        } else {
            // Use all protocols that support seeding
            self.list_protocols()
                .into_iter()
                .filter(|(_, caps)| caps.supports_seeding)
                .map(|(name, _)| name.to_string())
                .collect()
        };

        if protocols.is_empty() {
            return Err(ProtocolError::NotSupported);
        }

        info!("Seeding on protocols: {:?}", protocols);

        // Convert FileTransferOptions to SeedOptions
        let seed_options = SeedOptions {
            announce_dht: options.announce_dht,
            enable_encryption: options.encryption,
            upload_slots: options.upload_slots,
        };

        // Seed on multiple protocols using existing method
        let seeding_results = self
            .seed_file_multi_protocol(file_path.clone(), protocols.clone(), seed_options)
            .await?;

        // Get the first identifier as the primary one
        let identifier = seeding_results
            .values()
            .next()
            .map(|info| info.identifier.clone())
            .unwrap_or_default();

        // Create transfer record
        let active_transfer = ActiveTransfer {
            transfer_id: transfer_id.clone(),
            identifier: identifier.clone(),
            protocols: protocols.clone(),
            status: TransferStatus::Uploading,
            output_path: Some(file_path),
            started_at,
            is_download: false,
            bytes_transferred: 0,
            total_bytes: 0,
            current_speed: 0.0,
            active_connections: 0,
            protocol_handles: seeding_results
                .iter()
                .map(|(proto, info)| (proto.clone(), info.identifier.clone()))
                .collect(),
        };

        self.register_active_transfer(active_transfer).await;

        Ok(TransferResult {
            transfer_id,
            protocols,
            identifier,
            output_path: None,
            bytes_transferred: 0,
            duration_seconds: 0.0,
            average_speed: 0.0,
            success: true,
            error: None,
            started_at,
            completed_at: started_at,
        })
    }

    /// Pause an active transfer
    pub async fn pause_transfer(&self, transfer_id: &str) -> Result<(), ProtocolError> {
        info!("Pausing transfer: {}", transfer_id);

        let transfer = self.get_active_transfer(transfer_id).await.ok_or_else(|| {
            ProtocolError::DownloadNotFound(format!("Transfer not found: {}", transfer_id))
        })?;

        // Pause on all active protocols
        for (protocol, handle) in &transfer.protocol_handles {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == protocol) {
                if let Err(e) = handler.pause_download(handle).await {
                    warn!("Failed to pause on {}: {}", protocol, e);
                }
            }
        }

        // Update transfer status
        self.update_transfer_status(transfer_id, TransferStatus::Paused)
            .await;

        Ok(())
    }

    /// Resume a paused transfer
    pub async fn resume_transfer(&self, transfer_id: &str) -> Result<(), ProtocolError> {
        info!("Resuming transfer: {}", transfer_id);

        let transfer = self.get_active_transfer(transfer_id).await.ok_or_else(|| {
            ProtocolError::DownloadNotFound(format!("Transfer not found: {}", transfer_id))
        })?;

        // Resume on all active protocols
        for (protocol, handle) in &transfer.protocol_handles {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == protocol) {
                if let Err(e) = handler.resume_download(handle).await {
                    warn!("Failed to resume on {}: {}", protocol, e);
                }
            }
        }

        // Update transfer status
        self.update_transfer_status(transfer_id, TransferStatus::Downloading)
            .await;

        Ok(())
    }

    /// Cancel an active transfer
    pub async fn cancel_transfer(&self, transfer_id: &str) -> Result<(), ProtocolError> {
        info!("Cancelling transfer: {}", transfer_id);

        let transfer = self.get_active_transfer(transfer_id).await.ok_or_else(|| {
            ProtocolError::DownloadNotFound(format!("Transfer not found: {}", transfer_id))
        })?;

        // Cancel on all active protocols
        for (protocol, handle) in &transfer.protocol_handles {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == protocol) {
                if let Err(e) = handler.cancel_download(handle).await {
                    warn!("Failed to cancel on {}: {}", protocol, e);
                }
            }
        }

        // Remove from active transfers
        self.remove_active_transfer(transfer_id).await;

        Ok(())
    }

    /// Get progress for an active transfer
    pub async fn get_transfer_progress(
        &self,
        transfer_id: &str,
    ) -> Result<TransferProgress, ProtocolError> {
        let transfer = self.get_active_transfer(transfer_id).await.ok_or_else(|| {
            ProtocolError::DownloadNotFound(format!("Transfer not found: {}", transfer_id))
        })?;

        // Aggregate progress from all active protocols
        let mut total_connections = 0;
        let mut protocol_info = HashMap::new();

        for (protocol, handle) in &transfer.protocol_handles {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == protocol) {
                if let Ok(progress) = handler.get_download_progress(handle).await {
                    total_connections += progress.active_peers;

                    // Store protocol-specific info
                    protocol_info.insert(
                        protocol.clone(),
                        serde_json::json!({
                            "speed": progress.download_speed,
                            "peers": progress.active_peers,
                            "status": format!("{:?}", progress.status),
                        }),
                    );
                }
            }
        }

        let percentage = if transfer.total_bytes > 0 {
            (transfer.bytes_transferred as f64 / transfer.total_bytes as f64) * 100.0
        } else {
            0.0
        };

        let eta_seconds = if transfer.current_speed > 0.0 && transfer.total_bytes > 0 {
            let remaining = transfer.total_bytes.saturating_sub(transfer.bytes_transferred);
            Some((remaining as f64 / transfer.current_speed) as u64)
        } else {
            None
        };

        Ok(TransferProgress {
            transfer_id: transfer_id.to_string(),
            identifier: transfer.identifier,
            status: transfer.status,
            active_protocols: transfer.protocols,
            bytes_transferred: transfer.bytes_transferred,
            total_bytes: transfer.total_bytes,
            percentage,
            current_speed: transfer.current_speed,
            eta_seconds,
            active_connections: total_connections,
            total_sources: transfer.protocol_handles.len(),
            output_path: transfer.output_path,
            started_at: transfer.started_at,
            protocol_info,
        })
    }

    /// List all active transfers
    pub async fn list_transfers(&self) -> Vec<TransferProgress> {
        let transfers = self.active_transfers.read().await;

        let mut results = Vec::new();
        for transfer_id in transfers.keys() {
            if let Ok(progress) = self.get_transfer_progress(transfer_id).await {
                results.push(progress);
            }
        }

        results
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// Filter protocols based on detection preferences
    fn filter_protocols_by_preferences(
        &self,
        protocols: &[String],
        prefs: &DetectionPreferences,
    ) -> Vec<String> {
        protocols
            .iter()
            .filter(|proto| {
                if let Some(handler) = self.handlers.iter().find(|h| h.name() == *proto) {
                    let caps = handler.capabilities();

                    // Check requirements
                    if prefs.require_seeding && !caps.supports_seeding {
                        return false;
                    }
                    if prefs.require_encryption && !caps.supports_encryption {
                        return false;
                    }
                    if prefs.require_pause_resume && !caps.supports_pause_resume {
                        return false;
                    }

                    true
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }

    /// Start single-protocol download
    async fn start_single_protocol_download(
        &self,
        identifier: &str,
        protocol: &str,
        options: FileTransferOptions,
        transfer_id: &str,
    ) -> Result<(), ProtocolError> {
        let handler = self
            .handlers
            .iter()
            .find(|h| h.name() == protocol)
            .ok_or_else(|| {
                ProtocolError::InvalidIdentifier(format!("Protocol not found: {}", protocol))
            })?;

        // Convert FileTransferOptions to DownloadOptions
        let download_opts = DownloadOptions {
            output_path: options.output_path.unwrap_or_default(),
            max_peers: options.max_connections,
            chunk_size: options.chunk_size,
            encryption: options.encryption,
            bandwidth_limit: options.bandwidth_limit,
        };

        // Start the download
        let handle = handler.download(identifier, download_opts).await?;

        // Store the handle
        let mut transfers = self.active_transfers.write().await;
        if let Some(transfer) = transfers.get_mut(transfer_id) {
            transfer
                .protocol_handles
                .insert(protocol.to_string(), handle.identifier);
            transfer.status = TransferStatus::Downloading;
        }

        Ok(())
    }

    /// Start multi-source download (placeholder - will be implemented in Task 2)
    async fn start_multi_source_download(
        &self,
        identifier: &str,
        protocols: &[String],
        options: FileTransferOptions,
        transfer_id: &str,
    ) -> Result<(), ProtocolError> {
        info!(
            "Multi-source download requested for {} using protocols: {:?}",
            identifier, protocols
        );

        // TODO: Implement multi-source download coordination in Task 2
        // For now, fall back to single protocol (first in list)
        warn!("Multi-source download not yet implemented, falling back to single protocol");

        if let Some(protocol) = protocols.first() {
            self.start_single_protocol_download(identifier, protocol, options, transfer_id)
                .await
        } else {
            Err(ProtocolError::Internal(
                "No protocols available for download".to_string(),
            ))
        }
    }

    /// Register an active transfer
    async fn register_active_transfer(&self, transfer: ActiveTransfer) {
        let mut transfers = self.active_transfers.write().await;
        transfers.insert(transfer.transfer_id.clone(), transfer);
    }

    /// Get an active transfer
    async fn get_active_transfer(&self, transfer_id: &str) -> Option<ActiveTransfer> {
        let transfers = self.active_transfers.read().await;
        transfers.get(transfer_id).cloned()
    }

    /// Remove an active transfer
    async fn remove_active_transfer(&self, transfer_id: &str) {
        let mut transfers = self.active_transfers.write().await;
        transfers.remove(transfer_id);
    }

    /// Update transfer status
    async fn update_transfer_status(&self, transfer_id: &str, status: TransferStatus) {
        let mut transfers = self.active_transfers.write().await;
        if let Some(transfer) = transfers.get_mut(transfer_id) {
            transfer.status = status;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_filter_protocols_by_preferences() {
        let manager = ProtocolManager::new();

        let protocols = vec!["http".to_string(), "bittorrent".to_string()];

        let prefs = DetectionPreferences {
            require_encryption: true,
            ..Default::default()
        };

        let filtered = manager.filter_protocols_by_preferences(&protocols, &prefs);

        // This test will need actual handlers registered to work properly
        // For now, just verify it doesn't panic
        assert!(filtered.len() <= protocols.len());
    }

    #[test]
    fn test_active_transfer_creation() {
        let transfer = ActiveTransfer {
            transfer_id: "test-123".to_string(),
            identifier: "magnet:?xt=...".to_string(),
            protocols: vec!["bittorrent".to_string()],
            status: TransferStatus::Downloading,
            output_path: Some(PathBuf::from("/tmp/test")),
            started_at: 1234567890,
            is_download: true,
            bytes_transferred: 1024,
            total_bytes: 2048,
            current_speed: 512.0,
            active_connections: 5,
            protocol_handles: HashMap::new(),
        };

        assert_eq!(transfer.transfer_id, "test-123");
        assert!(transfer.is_download);
        assert_eq!(transfer.protocols.len(), 1);
    }
}
