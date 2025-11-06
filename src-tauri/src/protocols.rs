use async_trait::async_trait;
use std::sync::Arc;

/// A trait for handling a specific download/upload protocol like BitTorrent or HTTP.
#[async_trait]
pub trait ProtocolHandler: Send + Sync {
    /// Returns the name of the protocol (e.g., "bittorrent", "http").
    fn name(&self) -> &'static str;

    /// Determines if this handler can process the given identifier (e.g., a URL or magnet link).
    fn supports(&self, identifier: &str) -> bool;

    /// Initiates a download for the given identifier.
    async fn download(&self, identifier: &str) -> Result<(), String>;

    /// Starts seeding a file and returns an identifier (e.g., magnet link) for others to use.
    async fn seed(&self, file_path: &str) -> Result<String, String>;
}

/// Manages multiple protocol handlers to abstract away the download/upload mechanism.
pub struct ProtocolManager {
    handlers: Vec<Arc<dyn ProtocolHandler>>,
}

impl ProtocolManager {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Adds a new protocol handler to the manager.
    pub fn register(&mut self, handler: Arc<dyn ProtocolHandler>) {
        self.handlers.push(handler);
    }

    /// Delegates a download to the appropriate handler.
    pub async fn download(&self, identifier: &str) -> Result<(), String> {
        // Find a handler that supports the identifier (e.g., a magnet link).
        for handler in &self.handlers {
            if handler.supports(identifier) {
                return handler.download(identifier).await;
            }
        }
        Err(format!(
            "No protocol handler found for identifier: {}",
            identifier
        ))
    }

    /// Delegates seeding to the appropriate handler.
    pub async fn seed(&self, file_path: &str) -> Result<String, String> {
        // For seeding, we might try the first available handler or one that matches the file type.
        // For now, we'll try the first one that can handle it.
        // A more advanced implementation could select a handler based on configuration.
        if let Some(handler) = self.handlers.first() {
            return handler.seed(file_path).await;
        }
        Err(format!(
            "No protocol handler available to seed file: {}",
            file_path
        ))
    }
}