// src-tauri/src/protocols/seeding.rs

use super::traits::SeedingInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Represents a single file being seeded, potentially across multiple protocols.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedingEntry {
    /// Local path to the file
    pub file_path: PathBuf,
    /// A unique hash of the file (e.g., SHA-256) used as the primary key
    pub file_hash: String,
    /// Size of the file in bytes
    pub file_size: u64,
    /// Map of protocol name -> protocol-specific seeding info (like magnet link)
    pub protocols: HashMap<String, SeedingInfo>,
    /// Unix timestamp when seeding first started
    pub started_at: u64,
    /// Total bytes uploaded across all protocols for this file
    pub total_uploaded: u64,
}

/// Manages all active seeding entries in a thread-safe way.
pub struct SeedingRegistry {
    /// Map of file_hash -> SeedingEntry
    pub entries: Arc<RwLock<HashMap<String, SeedingEntry>>>,
}

impl SeedingRegistry {
    /// Creates a new, empty seeding registry.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Adds a file to the seeding registry for a specific protocol.
    /// If the file is already registered, it adds the new protocol to the existing entry.
    pub async fn add_seeding(
        &self,
        file_hash: String,
        file_path: PathBuf,
        protocol: String,
        seeding_info: SeedingInfo,
    ) -> Result<(), String> {
        let mut entries = self.entries.write().await;

        let file_size = match tokio::fs::metadata(&file_path).await {
            Ok(meta) => meta.len(),
            Err(e) => {
                warn!(
                    "Failed to get metadata for seeding file {:?}: {}",
                    file_path, e
                );
                return Err(e.to_string());
            }
        };

        let entry = entries.entry(file_hash.clone()).or_insert_with(|| {
            info!("Creating new seeding entry for file hash: {}", file_hash);
            SeedingEntry {
                file_path: file_path.clone(),
                file_hash: file_hash.clone(),
                file_size,
                protocols: HashMap::new(),
                started_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                total_uploaded: 0,
            }
        });

        // Add or update the protocol-specific info
        entry.protocols.insert(protocol, seeding_info);
        Ok(())
    }

    /// Removes a file from the seeding registry entirely (stops seeding on all protocols).
    pub async fn remove_seeding(&self, file_hash: &str) {
        let mut entries = self.entries.write().await;
        if entries.remove(file_hash).is_some() {
            info!("Removed seeding entry for file hash: {}", file_hash);
        }
    }

    /// Returns a list of all currently seeding files.
    pub async fn list_all(&self) -> Vec<SeedingEntry> {
        let entries = self.entries.read().await;
        entries.values().cloned().collect()
    }

    /// Updates the total uploaded bytes for a specific file.
    /// This would be called periodically by a monitoring task.
    pub async fn update_stats(&self, file_hash: &str, bytes_uploaded_delta: u64) {
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(file_hash) {
            entry.total_uploaded += bytes_uploaded_delta;
        }
    }
}