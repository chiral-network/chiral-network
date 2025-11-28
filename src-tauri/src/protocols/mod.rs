//! Protocol Handlers Module
//!
//! This module provides a unified interface for different file transfer protocols
//! used in the Chiral Network. Each protocol implements the `ProtocolHandler` trait,
//! allowing for consistent handling of downloads and seeding operations.
//!
//! ## Supported Protocols
//!
//! - **BitTorrent**: Magnet links and .torrent files, with DHT support
//! - **HTTP/HTTPS**: Direct file downloads with range request support
//! - **FTP/FTPS**: FTP server downloads with resume capability
//! - **ED2K**: eDonkey2000 protocol with chunk-based downloads
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::protocols::{ProtocolManager, DownloadOptions};
//!
//! // Create a protocol manager
//! let mut manager = ProtocolManager::new();
//!
//! // Register handlers
//! manager.register(Arc::new(HttpProtocolHandler::new()?));
//! manager.register(Arc::new(BitTorrentProtocolHandler::with_download_directory(dir).await?));
//!
//! // Download a file
//! let handle = manager.download(
//!     "[https://example.com/file.zip](https://example.com/file.zip)",
//!     DownloadOptions::default(),
//! ).await?;
//! ```

pub mod traits;
pub mod bittorrent;
pub mod http;
pub mod ftp;
pub mod ed2k;
pub mod seeding;
pub mod detection;

// Re-export commonly used types
pub use traits::{
    // ProtocolManager
    ProtocolHandler,
    ProtocolCapabilities,
    ProtocolError,
    DownloadHandle,
    DownloadOptions,
    DownloadProgress,
    DownloadStatus,
    SeedOptions,
    SeedingInfo,
    // Legacy exports for backward compatibility
    SimpleProtocolHandler,
    SimpleProtocolManager,
};

// Re-export detection types
pub use detection::{DetectionPreferences, ProtocolDetector};

use crate::protocols::seeding::{SeedingEntry, SeedingRegistry};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn};

// Re-export legacy trait with the old name for backward compatibility
// This allows existing code like bittorrent_handler.rs to continue working
#[doc(hidden)]
#[deprecated(note = "Use SimpleProtocolHandler or ProtocolHandler instead")]
pub use traits::SimpleProtocolHandler as LegacyProtocolHandler;

pub use bittorrent::BitTorrentProtocolHandler;
pub use http::HttpProtocolHandler;
pub use ftp::FtpProtocolHandler;
pub use ed2k::Ed2kProtocolHandler;

/// Manages multiple protocol handlers
///
/// Routes downloads and seeds to the appropriate handler based on the identifier.
pub struct ProtocolManager {
    handlers: Vec<std::sync::Arc<dyn ProtocolHandler>>,
    simple_handlers: Vec<std::sync::Arc<dyn SimpleProtocolHandler>>,
    seeding_registry: SeedingRegistry,
}

impl ProtocolManager {
    /// Creates a new protocol manager
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            simple_handlers: Vec::new(),
            seeding_registry: SeedingRegistry::new(),
        }
    }

    /// Registers an enhanced protocol handler
    pub fn register(&mut self, handler: std::sync::Arc<dyn ProtocolHandler>) {
        info!("Registering protocol handler: {}", handler.name());
        self.handlers.push(handler);
    }

    /// Create a protocol detector from current handlers
    fn create_detector(&self) -> ProtocolDetector {
        let mut detector_handlers: HashMap<String, std::sync::Arc<dyn ProtocolHandler>> = HashMap::new();

        for handler in &self.handlers {
            detector_handlers.insert(handler.name().to_string(), handler.clone());
        }

        ProtocolDetector::new(detector_handlers)
    }

    /// Finds a handler that supports the given identifier
    pub fn find_handler(&self, identifier: &str) -> Option<&dyn ProtocolHandler> {
        self.handlers
            .iter()
            .find(|h| h.supports(identifier))
            .map(|h| h.as_ref())
    }

    /// Finds a simple handler that supports the given identifier
    fn find_simple_handler(&self, identifier: &str) -> Option<&dyn SimpleProtocolHandler> {
        self.simple_handlers
            .iter()
            .find(|h| h.supports(identifier))
            .map(|h| h.as_ref())
    }

    /// Initiates a download using the appropriate handler
    pub async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        let handler = self
            .find_handler(identifier)
            .ok_or_else(|| ProtocolError::InvalidIdentifier(
                format!("No handler found for: {}", identifier)
            ))?;

        handler.download(identifier, options).await
    }

    /// Starts seeding using the specified protocol
    pub async fn seed(
        &self,
        protocol: &str,
        file_path: PathBuf,
        options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        let handler = self
            .handlers
            .iter()
            .find(|h| h.name() == protocol)
            .ok_or_else(|| ProtocolError::InvalidIdentifier(
                format!("Unknown protocol: {}", protocol)
            ))?;

        handler.seed(file_path, options).await
    }

    /// Lists all handlers and their capabilities
    pub fn list_protocols(&self) -> Vec<(&'static str, ProtocolCapabilities)> {
        self.handlers
            .iter()
            .map(|h| (h.name(), h.capabilities()))
            .collect()
    }

    // =========================================================================
    // Legacy Methods (for backward compatibility)
    // =========================================================================

    /// Initiates a download using the appropriate handler (legacy API)
    /// This is a backward-compatible wrapper that tries both enhanced and simple handlers.
    #[deprecated(note = "Use download() with DownloadOptions instead")]
    pub async fn download_simple(&self, identifier: &str) -> Result<(), String> {
        // First try enhanced handlers
        if let Some(handler) = self.find_handler(identifier) {
            return handler.download(identifier, DownloadOptions::default())
                .await
                .map(|_| ())
                .map_err(|e| e.to_string());
        }

        // Then try simple handlers
        if let Some(handler) = self.find_simple_handler(identifier) {
            return handler.download(identifier).await;
        }

        Err(format!("No handler found for: {}", identifier))
    }

    /// Starts seeding a file (legacy API)
    /// This is a backward-compatible wrapper that tries both enhanced and simple handlers.
    #[deprecated(note = "Use seed() with protocol and SeedOptions instead")]
    pub async fn seed_simple(&self, file_path: &str) -> Result<String, String> {
        // Try enhanced handlers first
        for handler in &self.handlers {
            if handler.capabilities().supports_seeding {
                match handler.seed(PathBuf::from(file_path), SeedOptions::default()).await {
                    Ok(info) => return Ok(info.identifier),
                    Err(_) => continue,
                }
            }
        }

        // Then try simple handlers
        for handler in &self.simple_handlers {
            match handler.seed(file_path).await {
                Ok(identifier) => return Ok(identifier),
                Err(_) => continue,
            }
        }

        Err(format!("No handler available to seed: {}", file_path))
    }

    // =========================================================================
    // --- New Methods for Centralized Seeding ---
    // =========================================================================

    /// Seed a file on multiple protocols simultaneously and register it.
    pub async fn seed_file_multi_protocol(
        &self,
        file_path: PathBuf,
        protocols: Vec<String>, // e.g., ["bittorrent", "ed2k"]
        options: SeedOptions,
    ) -> Result<HashMap<String, SeedingInfo>, ProtocolError> {
        info!("Seeding file on protocols: {:?}", protocols);

        if !file_path.exists() {
            return Err(ProtocolError::FileNotFound(
                file_path.to_string_lossy().to_string(),
            ));
        }

        let mut results = HashMap::new();
        // Use SHA-256 as the unique file identifier
        let file_hash = self.calculate_file_hash(&file_path).await?;

        for protocol_name in protocols {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == protocol_name) {
                if !handler.capabilities().supports_seeding {
                    warn!("Protocol {} does not support seeding.", protocol_name);
                    continue;
                }
                
                match handler.seed(file_path.clone(), options.clone()).await {
                    Ok(seeding_info) => {
                        // Add to registry
                        self.seeding_registry
                            .add_seeding(
                                file_hash.clone(),
                                file_path.clone(),
                                protocol_name.clone(),
                                seeding_info.clone(),
                            )
                            .await
                            .map_err(|e| ProtocolError::Internal(e))?;

                        results.insert(protocol_name, seeding_info);
                    }
                    Err(e) => {
                        warn!("Failed to seed on {}: {}", protocol_name, e);
                    }
                }
            } else {
                warn!("No handler found for protocol: {}", protocol_name);
            }
        }

        if results.is_empty() {
            Err(ProtocolError::Internal(
                "Failed to seed on any protocol".to_string(),
            ))
        } else {
            Ok(results)
        }
    }

    /// Stop seeding a file on all protocols it's registered with.
    pub async fn stop_seeding_all(&self, file_hash: &str) -> Result<(), ProtocolError> {
        info!("Stopping seeding for file hash: {}", file_hash);

        // Get seeding entry to find which protocols are active
        let entries = self.seeding_registry.entries.read().await;
        let entry = match entries.get(file_hash) {
            Some(entry) => entry.clone(), // Clone to release lock
            None => {
                // Using DownloadNotFound as it's the closest existing variant
                return Err(ProtocolError::DownloadNotFound(
                    "File not found in seeding registry".to_string(),
                ));
            }
        };
        drop(entries); // Release read lock

        // Stop on each active protocol
        for (protocol_name, seeding_info) in entry.protocols.iter() {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == protocol_name) {
                // Use the protocol-specific identifier (e.g., magnet link) to stop
                if let Err(e) = handler.stop_seeding(&seeding_info.identifier).await {
                    warn!(
                        "Failed to stop seeding on {}: {}",
                        protocol_name, e
                    );
                }
            }
        }

        // Remove from registry
        self.seeding_registry.remove_seeding(file_hash).await;

        Ok(())
    }

    /// List all files currently being seeded.
    pub async fn list_seeding_files(&self) -> Vec<SeedingEntry> {
        self.seeding_registry.list_all().await
    }

    /// Calculate file hash (SHA-256)
    pub async fn calculate_file_hash(&self, file_path: &PathBuf) -> Result<String, ProtocolError> {
        let data = tokio::fs::read(file_path)
            .await
            .map_err(|e| ProtocolError::FileNotFound(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(&data);
        Ok(hex::encode(hasher.finalize()))
    }

    // =========================================================================
    // --- Protocol Auto-Detection Methods (Task 4) ---
    // =========================================================================

    /// Detect all protocols that can handle the given identifier
    ///
    /// Returns a list of protocol names (e.g., "bittorrent", "http") that
    /// support the given identifier.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let protocols = manager.detect_protocols("magnet:?xt=urn:btih:...");
    /// // Returns: ["bittorrent"]
    ///
    /// let protocols = manager.detect_protocols("https://example.com/file.zip");
    /// // Returns: ["http"]
    /// ```
    pub fn detect_protocols(&self, identifier: &str) -> Vec<String> {
        info!("Detecting protocols for identifier: {}", identifier);
        let detector = self.create_detector();
        detector.detect_all(identifier)
    }

    /// Detect the best protocol for the given identifier based on preferences
    ///
    /// This method filters protocols by user preferences (encryption, seeding support, etc.)
    /// and returns the highest priority protocol that matches all requirements.
    ///
    /// # Arguments
    ///
    /// * `identifier` - The file identifier (URL, magnet link, ed2k link, etc.)
    /// * `preferences` - User preferences for filtering protocols
    ///
    /// # Returns
    ///
    /// The name of the best matching protocol, or `None` if no protocol matches.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use crate::protocols::DetectionPreferences;
    ///
    /// let prefs = DetectionPreferences {
    ///     require_encryption: true,
    ///     require_seeding: false,
    ///     ..Default::default()
    /// };
    ///
    /// let best = manager.detect_best_protocol("ed2k://|file|...", prefs);
    /// ```
    pub fn detect_best_protocol(
        &self,
        identifier: &str,
        preferences: DetectionPreferences,
    ) -> Option<String> {
        info!("Detecting best protocol for identifier with preferences");
        let detector = self.create_detector();
        detector.detect_best(identifier, preferences)
    }
}


impl Default for ProtocolManager {
    fn default() -> Self {
        Self::new()
    }
}