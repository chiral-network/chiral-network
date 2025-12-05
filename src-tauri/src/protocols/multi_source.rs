//! Multi-Source Download Coordinator
//!
//! This module provides multi-source download capabilities integrated into
//! the ProtocolManager. It coordinates downloads from multiple protocols
//! simultaneously (BitTorrent, HTTP, FTP, ED2K) to maximize download speed
//! and reliability.

use super::traits::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};

/// Information about a download source
///
/// Represents a single source (protocol + identifier) that can provide
/// file chunks. Includes metadata for intelligent source selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceInfo {
    /// Protocol name (e.g., "bittorrent", "http")
    pub protocol: String,

    /// Protocol-specific identifier (URL, magnet link, etc.)
    pub identifier: String,

    /// Chunk IDs this source can provide (empty = all chunks)
    pub available_chunks: Vec<u32>,

    /// Average latency to this source in milliseconds
    pub latency_ms: Option<u64>,

    /// Source reputation score (0-100, higher is better)
    pub reputation: Option<u8>,
}

impl SourceInfo {
    /// Create a new source with default values
    pub fn new(protocol: String, identifier: String) -> Self {
        Self {
            protocol,
            identifier,
            available_chunks: Vec::new(),
            latency_ms: None,
            reputation: None,
        }
    }

    /// Calculate priority score for this source
    ///
    /// Higher score = higher priority for chunk assignment
    pub fn priority_score(&self) -> u32 {
        let mut score = 0u32;

        // Base protocol priority
        score += match self.protocol.as_str() {
            "bittorrent" => 100,
            "ed2k" => 75,
            "http" => 50,
            "ftp" => 25,
            _ => 10,
        };

        // Latency bonus (lower latency = higher score)
        if let Some(latency) = self.latency_ms {
            if latency < 50 {
                score += 50;
            } else if latency < 100 {
                score += 30;
            } else if latency < 200 {
                score += 10;
            }
        }

        // Reputation bonus
        if let Some(rep) = self.reputation {
            score += rep as u32;
        }

        score
    }
}

/// Assignment of a chunk to a specific source
#[derive(Debug, Clone)]
pub struct ChunkAssignment {
    /// Chunk ID
    pub chunk_id: u32,

    /// Source assigned to download this chunk
    pub source: SourceInfo,

    /// Byte offset in the file
    pub offset: u64,

    /// Size in bytes
    pub size: usize,
}

/// Coordinates downloads from multiple sources
pub struct MultiSourceCoordinator {
    /// Map of protocol name -> handler
    handlers: HashMap<String, Arc<dyn ProtocolHandler>>,

    /// Active downloads being coordinated
    active_downloads: Arc<RwLock<HashMap<String, MultiSourceDownload>>>,
}

/// Internal state for a multi-source download
struct MultiSourceDownload {
    /// Unique file identifier
    file_hash: String,

    /// Total file size in bytes
    total_size: u64,

    /// Size of each chunk in bytes
    chunk_size: usize,

    /// All chunks for this download
    chunks: Vec<ChunkInfo>,

    /// Available sources
    sources: Vec<SourceInfo>,

    /// Current chunk assignments
    assignments: HashMap<u32, ChunkAssignment>,

    /// Completed chunks (chunk_id -> data)
    completed_chunks: HashMap<u32, Vec<u8>>,

    /// Failed chunks that need retry
    failed_chunks: Vec<u32>,
}

/// Metadata about a file chunk
#[derive(Debug, Clone)]
struct ChunkInfo {
    /// Chunk identifier
    chunk_id: u32,

    /// Byte offset in file
    offset: u64,

    /// Chunk size in bytes
    size: usize,

    /// Expected hash (for verification)
    hash: String,
}

impl MultiSourceCoordinator {
    /// Create a new multi-source coordinator
    pub fn new(handlers: HashMap<String, Arc<dyn ProtocolHandler>>) -> Self {
        info!("Initializing MultiSourceCoordinator with {} handlers", handlers.len());
        Self {
            handlers,
            active_downloads: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a multi-source download
    ///
    /// Coordinates downloading from multiple sources simultaneously,
    /// assigning different chunks to different sources for parallel downloads.
    ///
    /// # Arguments
    ///
    /// * `sources` - Available sources to download from
    /// * `output_path` - Where to save the complete file
    /// * `total_size` - Total file size in bytes
    /// * `chunk_size` - Size of each chunk in bytes
    ///
    /// # Returns
    ///
    /// A handle that can be used to track download progress
    pub async fn download_multi_source(
        &self,
        sources: Vec<SourceInfo>,
        output_path: PathBuf,
        total_size: u64,
        chunk_size: usize,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!(
            "Starting multi-source download from {} sources (size: {} bytes)",
            sources.len(),
            total_size
        );

        if sources.is_empty() {
            return Err(ProtocolError::Internal(
                "No sources provided for multi-source download".to_string(),
            ));
        }

        // Calculate chunks
        let chunks = self.calculate_chunks(total_size, chunk_size);
        info!("Split file into {} chunks", chunks.len());

        // Generate unique identifier for this download
        let file_hash = format!("multi_{}", uuid::Uuid::new_v4());

        // Create download state
        let download = MultiSourceDownload {
            file_hash: file_hash.clone(),
            total_size,
            chunk_size,
            chunks: chunks.clone(),
            sources: sources.clone(),
            assignments: HashMap::new(),
            completed_chunks: HashMap::new(),
            failed_chunks: Vec::new(),
        };

        // Store download state
        {
            let mut downloads = self.active_downloads.write().await;
            downloads.insert(file_hash.clone(), download);
        }

        // Assign chunks to sources
        let assignments = self.assign_chunks_to_sources(&sources, &chunks).await?;
        info!("Assigned chunks to {} sources", assignments.len());

        // Update assignments in download state
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(&file_hash) {
                for (source, chunk_ids) in &assignments {
                    for &chunk_id in chunk_ids {
                        if let Some(chunk) = chunks.iter().find(|c| c.chunk_id == chunk_id) {
                            download.assignments.insert(
                                chunk_id,
                                ChunkAssignment {
                                    chunk_id,
                                    source: source.clone(),
                                    offset: chunk.offset,
                                    size: chunk.size,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Spawn download tasks for each source
        for (source, chunk_ids) in assignments {
            let handler = self
                .handlers
                .get(&source.protocol)
                .ok_or_else(|| {
                    ProtocolError::Internal(format!("No handler for protocol: {}", source.protocol))
                })?
                .clone();

            let file_hash_clone = file_hash.clone();
            let active_downloads = self.active_downloads.clone();
            let output_path_clone = output_path.clone();

            tokio::spawn(async move {
                debug!(
                    "Starting download from {} for {} chunks",
                    source.protocol,
                    chunk_ids.len()
                );

                for chunk_id in chunk_ids {
                    match Self::download_chunk(
                        handler.clone(),
                        &source,
                        chunk_id,
                        &output_path_clone,
                    )
                    .await
                    {
                        Ok(data) => {
                            debug!("Completed chunk {} from {}", chunk_id, source.protocol);
                            // Store completed chunk
                            let mut downloads = active_downloads.write().await;
                            if let Some(download) = downloads.get_mut(&file_hash_clone) {
                                download.completed_chunks.insert(chunk_id, data);
                            }
                        }
                        Err(e) => {
                            error!(
                                "Failed to download chunk {} from {}: {}",
                                chunk_id, source.protocol, e
                            );
                            // Mark chunk as failed for retry
                            let mut downloads = active_downloads.write().await;
                            if let Some(download) = downloads.get_mut(&file_hash_clone) {
                                if !download.failed_chunks.contains(&chunk_id) {
                                    download.failed_chunks.push(chunk_id);
                                }
                            }
                        }
                    }
                }
            });
        }

        // Monitor progress and assemble file when complete
        let coordinator_clone = self.clone();
        let file_hash_clone = file_hash.clone();
        let output_path_clone = output_path.clone();
        tokio::spawn(async move {
            if let Err(e) = coordinator_clone
                .monitor_and_assemble(file_hash_clone.clone(), output_path_clone)
                .await
            {
                error!("Failed to assemble file {}: {}", file_hash_clone, e);
            }
        });

        Ok(DownloadHandle {
            identifier: file_hash,
            protocol: "multi-source".to_string(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }

    /// Assign chunks to sources based on availability and performance
    ///
    /// Uses a priority-based algorithm to distribute chunks across sources.
    /// Higher priority sources get more chunks.
    async fn assign_chunks_to_sources(
        &self,
        sources: &[SourceInfo],
        chunks: &[ChunkInfo],
    ) -> Result<HashMap<SourceInfo, Vec<u32>>, ProtocolError> {
        let mut assignments: HashMap<SourceInfo, Vec<u32>> = HashMap::new();

        if sources.is_empty() || chunks.is_empty() {
            return Ok(assignments);
        }

        // Sort sources by priority score (highest first)
        let mut sorted_sources = sources.to_vec();
        sorted_sources.sort_by(|a, b| b.priority_score().cmp(&a.priority_score()));

        // Calculate chunks per source based on priority
        let total_priority: u32 = sorted_sources.iter().map(|s| s.priority_score()).sum();

        let mut chunk_index = 0;
        for source in &sorted_sources {
            // Calculate how many chunks this source should get
            let source_priority = source.priority_score() as f64;
            let chunk_count =
                ((source_priority / total_priority as f64) * chunks.len() as f64).ceil() as usize;

            let mut source_chunks = Vec::new();
            for _ in 0..chunk_count {
                if chunk_index >= chunks.len() {
                    break;
                }
                source_chunks.push(chunks[chunk_index].chunk_id);
                chunk_index += 1;
            }

            if !source_chunks.is_empty() {
                debug!(
                    "Assigned {} chunks to {} (priority: {})",
                    source_chunks.len(),
                    source.protocol,
                    source.priority_score()
                );
                assignments.insert(source.clone(), source_chunks);
            }
        }

        // Assign any remaining chunks using round-robin
        while chunk_index < chunks.len() {
            for source in &sorted_sources {
                if chunk_index >= chunks.len() {
                    break;
                }
                assignments
                    .entry(source.clone())
                    .or_insert_with(Vec::new)
                    .push(chunks[chunk_index].chunk_id);
                chunk_index += 1;
            }
        }

        Ok(assignments)
    }

    /// Download a single chunk from a source
    ///
    /// Note: This is a simplified implementation. In production, protocol handlers
    /// should support byte-range requests for efficient chunk downloads.
    async fn download_chunk(
        handler: Arc<dyn ProtocolHandler>,
        source: &SourceInfo,
        chunk_id: u32,
        output_path: &PathBuf,
    ) -> Result<Vec<u8>, ProtocolError> {
        debug!(
            "Downloading chunk {} from {} ({})",
            chunk_id, source.protocol, source.identifier
        );

        // Create temporary path for this chunk
        let temp_path = output_path.with_extension(format!("chunk_{}.tmp", chunk_id));

        // Download to temporary file
        // Note: This downloads the full file. In production, use byte-range requests.
        handler
            .download(
                &source.identifier,
                DownloadOptions {
                    output_path: temp_path.clone(),
                    max_peers: Some(1),
                    chunk_size: None,
                    encryption: false,
                    bandwidth_limit: None,
                },
            )
            .await?;

        // Read chunk data from temporary file
        let data = tokio::fs::read(&temp_path)
            .await
            .map_err(|e| ProtocolError::Internal(format!("Failed to read chunk file: {}", e)))?;

        // Clean up temporary file
        let _ = tokio::fs::remove_file(&temp_path).await;

        debug!("Successfully downloaded chunk {} ({} bytes)", chunk_id, data.len());
        Ok(data)
    }

    /// Calculate chunk metadata for a file
    fn calculate_chunks(&self, total_size: u64, chunk_size: usize) -> Vec<ChunkInfo> {
        let total_chunks = ((total_size as f64) / (chunk_size as f64)).ceil() as u32;
        let mut chunks = Vec::new();

        for chunk_id in 0..total_chunks {
            let offset = (chunk_id as u64) * (chunk_size as u64);
            let size = if chunk_id == total_chunks - 1 {
                // Last chunk may be smaller
                (total_size - offset) as usize
            } else {
                chunk_size
            };

            chunks.push(ChunkInfo {
                chunk_id,
                offset,
                size,
                hash: String::new(), // TODO: Calculate or retrieve chunk hash
            });
        }

        chunks
    }

    /// Monitor download progress and assemble file when complete
    ///
    /// Runs in a loop checking if all chunks are downloaded, then assembles
    /// the final file from chunks.
    async fn monitor_and_assemble(
        &self,
        file_hash: String,
        output_path: PathBuf,
    ) -> Result<(), ProtocolError> {
        info!("Starting monitor for download: {}", file_hash);

        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;

            let downloads = self.active_downloads.read().await;
            let download = match downloads.get(&file_hash) {
                Some(d) => d,
                None => {
                    warn!("Download {} no longer active", file_hash);
                    break;
                }
            };

            let total_chunks = download.chunks.len();
            let completed = download.completed_chunks.len();
            let failed = download.failed_chunks.len();

            debug!(
                "Download progress: {}/{} chunks completed, {} failed",
                completed, total_chunks, failed
            );

            // Check if all chunks completed
            if completed == total_chunks {
                drop(downloads); // Release read lock

                info!("All chunks completed, assembling file: {}", file_hash);

                // Assemble file from chunks
                self.assemble_file(&file_hash, &output_path).await?;

                // Remove from active downloads
                let mut downloads = self.active_downloads.write().await;
                downloads.remove(&file_hash);

                info!("Multi-source download completed: {}", file_hash);
                break;
            }

            // Check if download has stalled (no progress and has failures)
            if failed > 0 && failed + completed == total_chunks {
                error!(
                    "Download {} has stalled: {} failed chunks",
                    file_hash, failed
                );
                // TODO: Implement retry logic for failed chunks
                break;
            }
        }

        Ok(())
    }

    /// Assemble complete file from downloaded chunks
    async fn assemble_file(
        &self,
        file_hash: &str,
        output_path: &PathBuf,
    ) -> Result<(), ProtocolError> {
        info!("Assembling file: {} -> {:?}", file_hash, output_path);

        let downloads = self.active_downloads.read().await;
        let download = downloads.get(file_hash).ok_or_else(|| {
            ProtocolError::Internal(format!("Download not found: {}", file_hash))
        })?;

        // Create output file
        let mut file = tokio::fs::File::create(output_path)
            .await
            .map_err(|e| ProtocolError::Internal(format!("Failed to create output file: {}", e)))?;

        use tokio::io::AsyncWriteExt;

        // Write chunks in order
        for chunk_id in 0..download.chunks.len() as u32 {
            let data = download.completed_chunks.get(&chunk_id).ok_or_else(|| {
                ProtocolError::Internal(format!("Missing chunk {} during assembly", chunk_id))
            })?;

            file.write_all(data)
                .await
                .map_err(|e| ProtocolError::Internal(format!("Failed to write chunk: {}", e)))?;

            debug!("Wrote chunk {} ({} bytes)", chunk_id, data.len());
        }

        file.flush()
            .await
            .map_err(|e| ProtocolError::Internal(format!("Failed to flush file: {}", e)))?;

        info!("File assembly completed: {:?}", output_path);
        Ok(())
    }

    /// Get current download progress
    pub async fn get_progress(&self, file_hash: &str) -> Option<(usize, usize)> {
        let downloads = self.active_downloads.read().await;
        downloads.get(file_hash).map(|d| {
            let total = d.chunks.len();
            let completed = d.completed_chunks.len();
            (completed, total)
        })
    }

    /// Cancel an active download
    pub async fn cancel_download(&self, file_hash: &str) -> Result<(), ProtocolError> {
        let mut downloads = self.active_downloads.write().await;
        if downloads.remove(file_hash).is_some() {
            info!("Cancelled download: {}", file_hash);
            Ok(())
        } else {
            Err(ProtocolError::DownloadNotFound(file_hash.to_string()))
        }
    }
}

// Make MultiSourceCoordinator cloneable for spawning tasks
impl Clone for MultiSourceCoordinator {
    fn clone(&self) -> Self {
        Self {
            handlers: self.handlers.clone(),
            active_downloads: self.active_downloads.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_info_priority() {
        let bittorrent = SourceInfo::new("bittorrent".to_string(), "magnet:...".to_string());
        let http = SourceInfo::new("http".to_string(), "https://...".to_string());

        assert!(bittorrent.priority_score() > http.priority_score());
    }

    #[test]
    fn test_source_info_with_latency() {
        let mut fast = SourceInfo::new("http".to_string(), "https://fast".to_string());
        fast.latency_ms = Some(30);

        let mut slow = SourceInfo::new("http".to_string(), "https://slow".to_string());
        slow.latency_ms = Some(150);

        assert!(fast.priority_score() > slow.priority_score());
    }

    #[test]
    fn test_calculate_chunks() {
        let coordinator = MultiSourceCoordinator::new(HashMap::new());
        let chunks = coordinator.calculate_chunks(1000, 300);

        assert_eq!(chunks.len(), 4); // ceil(1000/300) = 4
        assert_eq!(chunks[0].size, 300);
        assert_eq!(chunks[3].size, 100); // Last chunk is smaller
    }

    #[tokio::test]
    async fn test_chunk_assignment() {
        let coordinator = MultiSourceCoordinator::new(HashMap::new());

        let sources = vec![
            SourceInfo::new("bittorrent".to_string(), "magnet:1".to_string()),
            SourceInfo::new("http".to_string(), "https://example.com".to_string()),
        ];

        let chunks = coordinator.calculate_chunks(1000, 250);
        let assignments = coordinator
            .assign_chunks_to_sources(&sources, &chunks)
            .await
            .unwrap();

        // Should have assignments for both sources
        assert!(assignments.len() > 0);

        // Total chunks assigned should equal input chunks
        let total_assigned: usize = assignments.values().map(|v| v.len()).sum();
        assert_eq!(total_assigned, chunks.len());
    }
}
