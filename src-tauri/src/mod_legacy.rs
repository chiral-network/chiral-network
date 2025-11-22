use crate::protocols::bittorrent::BitTorrentHandler;
use async_trait::async_trait;
use std::sync::Arc;

pub mod bittorrent;

/// A common interface for all protocol handlers.
#[async_trait]
pub trait ProtocolHandler: Send + Sync {
    async fn download(&self, identifier: &str) -> Result<(), String>;
    async fn seed(&self, file_path: &str) -> Result<String, String>; // Returns identifier
}

/// Manages available protocol handlers and delegates tasks.
pub struct ProtocolManager {
    bittorrent: Arc<BitTorrentHandler>,
    // You can add other handlers here later, e.g., for HTTP, IPFS, etc.
    // http: Arc<HttpHandler>,
}

impl ProtocolManager {
    /// Creates a new ProtocolManager and initializes all handlers.
    pub fn new() -> Self {
        Self {
            bittorrent: Arc::new(BitTorrentHandler),
        }
    }

    /// Identifies the protocol from an identifier and calls the appropriate handler for downloading.
    pub async fn download(&self, identifier: &str) -> Result<(), String> {
        // Simple logic: if it's a magnet link or .torrent file, use BitTorrent.
        if identifier.starts_with("magnet:") || identifier.ends_with(".torrent") {
            self.bittorrent.download(identifier).await
        } else {
            Err("Unsupported protocol or invalid identifier.".to_string())
        }
    }

    /// Delegates seeding to the appropriate handler.
    pub async fn seed(&self, file_path: &str) -> Result<String, String> {
        // For now, we assume all seeding is done via BitTorrent.
        // This can be expanded later if other seeding protocols are added.
        self.bittorrent.seed(file_path).await
    }
}