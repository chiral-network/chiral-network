//! Protocol Handlers Module
//!
//! This module provides a unified interface for different file transfer protocols
//! used in the Chiral Network. Each protocol implements the `ProtocolHandler` trait,
//! allowing for consistent handling of downloads and seeding operations.
//!
//! ## Supported Protocols
//!
//! - **WebRTC**: P2P file transfers using WebRTC data channels (chiral:// scheme)
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
pub mod webrtc;
pub mod bittorrent;
pub mod http;
pub mod ftp;
pub mod ed2k;
pub mod seeding;
pub mod detection;
pub mod multi_source;
pub mod options;
pub mod api;

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

// Re-export unified API types
pub use options::{
    FileTransferOptions,
    TransferProgress,
    TransferResult,
    TransferStatus,
    DetectionPreferences,
};

pub use api::ActiveTransfer;

// Re-export multi-source types
pub use multi_source::{MultiSourceCoordinator, SourceInfo, ChunkAssignment};

use crate::protocols::seeding::{SeedingEntry, SeedingRegistry};
use detection::ProtocolDetector;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// Re-export legacy trait with the old name for backward compatibility
// This allows existing code like bittorrent_handler.rs to continue working
#[doc(hidden)]
#[deprecated(note = "Use SimpleProtocolHandler or ProtocolHandler instead")]
pub use traits::SimpleProtocolHandler as LegacyProtocolHandler;

pub use webrtc::WebRtcProtocolHandler;
pub use bittorrent::BitTorrentProtocolHandler;
pub use http::HttpProtocolHandler;
pub use ftp::FtpProtocolHandler;
pub use ed2k::Ed2kProtocolHandler;

/// Manages multiple protocol handlers
///
/// Routes downloads and seeds to the appropriate handler based on the identifier.
pub struct ProtocolManager {
    handlers: Vec<Arc<dyn ProtocolHandler>>,
    simple_handlers: Vec<Arc<dyn SimpleProtocolHandler>>,
    seeding_registry: SeedingRegistry,
    detector: ProtocolDetector,
    multi_source: MultiSourceCoordinator,
    /// Active file transfers (downloads and uploads)
    /// Maps transfer_id -> ActiveTransfer
    pub(crate) active_transfers: Arc<RwLock<HashMap<String, ActiveTransfer>>>,
}

impl ProtocolManager {
    /// Creates a new protocol manager
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            simple_handlers: Vec::new(),
            seeding_registry: SeedingRegistry::new(),
            detector: ProtocolDetector::new(),
            multi_source: MultiSourceCoordinator::new(HashMap::new()),
            active_transfers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers an enhanced protocol handler
    pub fn register(&mut self, handler: Arc<dyn ProtocolHandler>) {
        let name = handler.name().to_string();
        info!("Registering protocol handler: {}", name);
        self.handlers.push(handler);

        // Rebuild multi-source coordinator with updated handlers
        self.rebuild_multi_source();
    }

    /// Rebuild multi-source coordinator with current handlers
    fn rebuild_multi_source(&mut self) {
        let mut handlers_map: HashMap<String, Arc<dyn ProtocolHandler>> = HashMap::new();

        for handler in &self.handlers {
            handlers_map.insert(handler.name().to_string(), handler.clone());
        }

        self.multi_source = MultiSourceCoordinator::new(handlers_map);
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

    /// Initiates a download with automatic multi-source detection
    ///
    /// This method automatically detects available sources and uses multi-source
    /// download when beneficial (multiple sources and max_peers > 1).
    pub async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("Starting download for identifier: {}", identifier);

        // Discover all available sources for this identifier
        let sources = self.discover_sources(identifier).await?;
        info!("Found {} source(s) for download", sources.len());

        // Check if multi-source download is beneficial
        let use_multi_source = sources.len() > 1
            && options.max_peers.unwrap_or(1) > 1
            && options.chunk_size.is_some();

        if use_multi_source {
            info!("Using multi-source download with {} sources", sources.len());

            // Estimate total size (TODO: improve this by querying metadata)
            let total_size = 10 * 1024 * 1024; // Default 10MB if unknown
            let chunk_size = options.chunk_size.unwrap_or(256 * 1024);

            self.multi_source.download_multi_source(
                sources,
                options.output_path,
                total_size,
                chunk_size,
            ).await
        } else {
            info!("Using single-source download");

            // Single-source download - use traditional method
            let handler = self
                .find_handler(identifier)
                .ok_or_else(|| ProtocolError::InvalidIdentifier(
                    format!("No handler found for: {}", identifier)
                ))?;

            handler.download(identifier, options).await
        }
    }

    /// Discover all available sources for a file identifier
    ///
    /// Checks each registered protocol to see if it supports the identifier,
    /// and returns a list of potential download sources.
    async fn discover_sources(
        &self,
        identifier: &str,
    ) -> Result<Vec<SourceInfo>, ProtocolError> {
        let mut sources = Vec::new();

        // Check each protocol to see if it supports this identifier
        for handler in &self.handlers {
            if handler.supports(identifier) {
                let source = SourceInfo {
                    protocol: handler.name().to_string(),
                    identifier: identifier.to_string(),
                    available_chunks: Vec::new(), // TODO: Query actual chunk availability
                    latency_ms: None,            // TODO: Measure latency
                    reputation: None,             // TODO: Get reputation from reputation system
                };

                debug!("Found source: {} for identifier", handler.name());
                sources.push(source);
            }
        }

        // TODO: Query DHT for additional sources
        // TODO: Parse protocol-specific sources (e.g., trackers in magnet links)
        // TODO: Check seeding registry for local peers

        if sources.is_empty() {
            return Err(ProtocolError::InvalidIdentifier(
                format!("No sources found for: {}", identifier)
            ));
        }

        Ok(sources)
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

    /// Get capabilities for a specific protocol
    ///
    /// Returns the capabilities of the specified protocol handler,
    /// or None if the protocol is not registered.
    pub fn get_capabilities(&self, protocol: &str) -> Option<ProtocolCapabilities> {
        self.handlers
            .iter()
            .find(|h| h.name() == protocol)
            .map(|h| h.capabilities())
    }

    /// Get best protocol handler for an identifier
    ///
    /// Uses priority ordering to select the best handler that supports
    /// the given identifier. Priority: WebRTC > BitTorrent > ED2K > HTTP > FTP
    pub fn get_best_handler(
        &self,
        identifier: &str,
    ) -> Result<Arc<dyn ProtocolHandler>, ProtocolError> {
        // Priority order
        let priority = ["webrtc", "bittorrent", "ed2k", "http", "ftp"];

        for protocol in &priority {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == *protocol) {
                if handler.supports(identifier) {
                    return Ok(handler.clone());
                }
            }
        }

        Err(ProtocolError::InvalidIdentifier(
            format!("No handler supports: {}", identifier)
        ))
    }

    /// Upload/seed file on specified protocols
    ///
    /// Seeds a file on multiple protocols simultaneously.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file to seed
    /// * `protocols` - List of protocol names to seed on (e.g., ["bittorrent", "ed2k"])
    /// * `options` - Seeding options
    ///
    /// # Returns
    ///
    /// A map of protocol name to seeding information for successful seeds
    pub async fn upload(
        &self,
        file_path: PathBuf,
        protocols: Vec<String>,
        options: SeedOptions,
    ) -> Result<HashMap<String, SeedingInfo>, ProtocolError> {
        let mut results = HashMap::new();

        info!("Seeding file on {} protocol(s)", protocols.len());

        for protocol_name in protocols {
            if let Some(handler) = self.handlers.iter().find(|h| h.name() == protocol_name) {
                match handler.seed(file_path.clone(), options.clone()).await {
                    Ok(info) => {
                        debug!("Successfully seeded on {}", protocol_name);
                        results.insert(protocol_name, info);
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
                "Failed to seed on any protocol".to_string()
            ))
        } else {
            Ok(results)
        }
    }

    // =========================================================================
    // Legacy Methods (for backward compatibility)
    // ==========================================================================

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

    /// Returns all protocols that can serve the file
    pub async fn detect_protocols(&self, file_identifier: String) -> Vec<String> {
        let mut map: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        for handler in &self.handlers {
            map.insert(handler.name().to_string(), handler.as_ref());
        }
    
        self.detector.detect_all(&file_identifier, &map).await
    }
    

    /// Returns the best protocol for downloading the file
    pub async fn detect_best_protocol(&self, file_identifier: String) -> Option<String> {
        let mut map: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        for handler in &self.handlers {
            map.insert(handler.name().to_string(), handler.as_ref());
        }

        self.detector.detect_best(&file_identifier, &map).await
    }

    /// Returns the best protocol for downloading the file with custom preferences
    ///
    /// This method allows filtering protocols based on required capabilities
    /// such as seeding support, encryption, or pause/resume functionality.
    ///
    /// # Arguments
    ///
    /// * `file_identifier` - The file identifier (URL, magnet link, ed2k link, etc.)
    /// * `preferences` - User preferences for filtering protocols
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let prefs = DetectionPreferences {
    ///     require_encryption: true,
    ///     ..Default::default()
    /// };
    /// let best = manager.detect_best_protocol_with_preferences(
    ///     "magnet:?xt=urn:btih:...".to_string(),
    ///     prefs
    /// ).await;
    /// ```
    pub async fn detect_best_protocol_with_preferences(
        &mut self,
        file_identifier: String,
        preferences: DetectionPreferences,
    ) -> Option<String> {
        // Update detector preferences
        self.detector.set_priority(preferences);

        let mut map: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        for handler in &self.handlers {
            map.insert(handler.name().to_string(), handler.as_ref());
        }

        self.detector.detect_best(&file_identifier, &map).await
    }
}


impl Default for ProtocolManager {
    fn default() -> Self {
        Self::new()
    }
}