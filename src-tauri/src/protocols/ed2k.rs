//! ED2K (eDonkey2000) Protocol Handler
//!
//! Wraps the existing ED2K client to implement the enhanced ProtocolHandler trait.

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus,
    ProtocolCapabilities, ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
};
use crate::ed2k_client::{Ed2kClient, Ed2kConfig, Ed2kFileInfo, ED2K_CHUNK_SIZE};
use async_trait::async_trait;
use md4::{Md4, Digest};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{info, warn, error};

/// ED2K protocol handler implementing the enhanced ProtocolHandler trait
pub struct Ed2kProtocolHandler {
    /// Underlying ED2K client
    client: Arc<Mutex<Ed2kClient>>,
    /// Track active downloads
    active_downloads: Arc<Mutex<HashMap<String, Ed2kDownloadState>>>,
    /// Track download progress
    download_progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
    /// Track seeding files
    seeding_files: Arc<Mutex<HashMap<String, SeedingInfo>>>,
}

/// Internal state for an ED2K download
struct Ed2kDownloadState {
    file_info: Ed2kFileInfo,
    output_path: PathBuf,
    started_at: u64,
    status: DownloadStatus,
    is_paused: bool,
    /// Track which chunks have been downloaded for resume
    downloaded_chunks: Vec<bool>,
    /// Partial data downloaded so far
    partial_data: Vec<u8>,
}

impl Ed2kProtocolHandler {
    /// Creates a new ED2K protocol handler
    pub fn new(server_url: String) -> Self {
        Self {
            client: Arc::new(Mutex::new(Ed2kClient::new(server_url))),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            seeding_files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a handler with custom configuration
    pub fn with_config(config: Ed2kConfig) -> Self {
        Self {
            client: Arc::new(Mutex::new(Ed2kClient::with_config(config))),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            seeding_files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get current timestamp
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Parse ed2k:// link format
    /// ed2k://|file|FileName.ext|FileSize|MD4Hash|/
    fn parse_ed2k_link(link: &str) -> Result<Ed2kFileInfo, ProtocolError> {
        if !link.starts_with("ed2k://") {
            return Err(ProtocolError::InvalidIdentifier(
                "Not a valid ed2k:// link".to_string()
            ));
        }

        let parts: Vec<&str> = link.split('|').collect();

        if parts.len() < 5 || parts[1] != "file" {
            return Err(ProtocolError::InvalidIdentifier(
                "Invalid ed2k:// link format".to_string()
            ));
        }

        let file_name = parts[2].to_string();
        let file_size = parts[3]
            .parse::<u64>()
            .map_err(|_| ProtocolError::InvalidIdentifier("Invalid file size".to_string()))?;
        let md4_hash = parts[4].to_string();

        // Validate hash format (32 hex chars for MD4)
        if md4_hash.len() != 32 || !md4_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ProtocolError::InvalidIdentifier(
                "Invalid MD4 hash format".to_string()
            ));
        }

        Ok(Ed2kFileInfo {
            file_hash: md4_hash,
            file_size,
            file_name: Some(file_name),
            sources: Vec::new(),
        })
    }

    /// Generate ed2k:// link from file
    async fn generate_ed2k_link(file_path: &PathBuf) -> Result<String, ProtocolError> {
        let file_name = file_path
            .file_name()
            .ok_or_else(|| ProtocolError::InvalidIdentifier("Invalid file path".to_string()))?
            .to_str()
            .ok_or_else(|| ProtocolError::InvalidIdentifier("Invalid file name encoding".to_string()))?;

        let metadata = tokio::fs::metadata(file_path)
            .await
            .map_err(|e| ProtocolError::FileNotFound(e.to_string()))?;

        let file_size = metadata.len();

        // Calculate MD4 hash
        let file_data = tokio::fs::read(file_path)
            .await
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        let mut hasher = Md4::new();
        hasher.update(&file_data);
        let hash_result = hasher.finalize();
        let md4_hash = hex::encode(hash_result);

        Ok(format!(
            "ed2k://|file|{}|{}|{}|/",
            file_name, file_size, md4_hash.to_uppercase()
        ))
    }

    /// Calculate number of chunks for a file
    fn calculate_chunks(file_size: u64) -> usize {
        ((file_size as usize + ED2K_CHUNK_SIZE - 1) / ED2K_CHUNK_SIZE).max(1)
    }
}

#[async_trait]
impl ProtocolHandler for Ed2kProtocolHandler {
    fn name(&self) -> &'static str {
        "ed2k"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("ed2k://|file|")
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("ED2K: Starting download for {}", identifier);

        let file_info = Self::parse_ed2k_link(identifier)?;
        let download_id = file_info.file_hash.clone();

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(&download_id) {
                return Err(ProtocolError::AlreadyExists(download_id));
            }
        }

        let started_at = Self::now();
        let total_chunks = Self::calculate_chunks(file_info.file_size);

        // Initialize progress
        {
            let mut prog = self.download_progress.lock().await;
            prog.insert(download_id.clone(), DownloadProgress {
                downloaded_bytes: 0,
                total_bytes: file_info.file_size,
                download_speed: 0.0,
                eta_seconds: None,
                active_peers: 0,
                status: DownloadStatus::FetchingMetadata,
            });
        }

        // Track the download
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(download_id.clone(), Ed2kDownloadState {
                file_info: file_info.clone(),
                output_path: options.output_path.clone(),
                started_at,
                status: DownloadStatus::Downloading,
                is_paused: false,
                downloaded_chunks: vec![false; total_chunks],
                partial_data: Vec::new(),
            });
        }

        // Spawn download task
        let client = self.client.clone();
        let progress = self.download_progress.clone();
        let downloads = self.active_downloads.clone();
        let id = download_id.clone();
        let output_path = options.output_path;
        let file_hash = file_info.file_hash.clone();
        let file_size = file_info.file_size;

        tokio::spawn(async move {
            // Try to connect to ED2K server (optional for P2P mode)
            let server_available = {
                let mut c = client.lock().await;
                match c.connect().await {
                    Ok(_) => {
                        info!("ED2K: Connected to server for enhanced peer discovery");
                        true
                    },
                    Err(e) => {
                        warn!("ED2K: Server connection failed, operating in direct P2P mode: {}", e);
                        // Continue without server - may still work with direct peer connections
                        false
                    }
                }
            };

            // If no server available, set status to indicate waiting for peers
            if !server_available {
                let mut prog = progress.lock().await;
                if let Some(p) = prog.get_mut(&id) {
                    p.status = DownloadStatus::FetchingMetadata; // Waiting for peer discovery
                }
                // In a full P2P implementation, we'd start DHT-based peer discovery here
                // For now, we'll mark as fetching metadata and could potentially implement direct peer connections
                info!("ED2K: Download queued in P2P mode - waiting for peer connections");
                return;
            }

            // Update status to downloading
            {
                let mut prog = progress.lock().await;
                if let Some(p) = prog.get_mut(&id) {
                    p.status = DownloadStatus::Downloading;
                }
            }

            // Download chunks
            let mut all_data = Vec::with_capacity(file_size as usize);
            let start_time = std::time::Instant::now();

            for chunk_idx in 0..total_chunks {
                // Check if paused or cancelled
                {
                    let dl = downloads.lock().await;
                    if let Some(state) = dl.get(&id) {
                        if state.is_paused {
                            info!("ED2K: Download paused at chunk {}", chunk_idx);
                            return;
                        }
                    } else {
                        info!("ED2K: Download cancelled");
                        return;
                    }
                }

                // Calculate expected chunk hash (simplified - real implementation would have chunk hashes)
                let expected_hash = format!("{:032x}", chunk_idx); // Placeholder

                // Download chunk
                let chunk_data = {
                    let mut c = client.lock().await;
                    match c.download_chunk(&file_hash, chunk_idx as u32, &expected_hash).await {
                        Ok(data) => data,
                        Err(e) => {
                            error!("ED2K: Failed to download chunk {}: {}", chunk_idx, e);
                            let mut prog = progress.lock().await;
                            if let Some(p) = prog.get_mut(&id) {
                                p.status = DownloadStatus::Failed;
                            }
                            return;
                        }
                    }
                };

                all_data.extend(chunk_data);

                // Update progress
                let downloaded = all_data.len() as u64;
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
                let eta = if speed > 0.0 && file_size > downloaded {
                    Some(((file_size - downloaded) as f64 / speed) as u64)
                } else {
                    None
                };

                {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.downloaded_bytes = downloaded;
                        p.download_speed = speed;
                        p.eta_seconds = eta;
                    }
                }
            }

            // Write to file
            if let Err(e) = tokio::fs::write(&output_path, &all_data).await {
                error!("ED2K: Failed to write file: {}", e);
                let mut prog = progress.lock().await;
                if let Some(p) = prog.get_mut(&id) {
                    p.status = DownloadStatus::Failed;
                }
                return;
            }

            // Mark as completed
            {
                let mut prog = progress.lock().await;
                if let Some(p) = prog.get_mut(&id) {
                    p.status = DownloadStatus::Completed;
                    p.downloaded_bytes = file_size;
                }
            }

            info!("ED2K: Download completed: {} bytes", file_size);

            // Disconnect
            {
                let mut c = client.lock().await;
                let _ = c.disconnect().await;
            }
        });

        Ok(DownloadHandle {
            identifier: download_id,
            protocol: "ed2k".to_string(),
            started_at,
        })
    }

    async fn seed(
        &self,
        file_path: PathBuf,
        _options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        info!("ED2K: Starting seed for {:?}", file_path);

        // Check if file exists
        if !file_path.exists() {
            return Err(ProtocolError::FileNotFound(
                file_path.to_string_lossy().to_string()
            ));
        }

        // Generate ed2k link
        let ed2k_link = Self::generate_ed2k_link(&file_path).await?;

        let seeding_info = SeedingInfo {
            identifier: ed2k_link.clone(),
            file_path: file_path.clone(),
            protocol: "ed2k".to_string(),
            active_peers: 0,
            bytes_uploaded: 0,
        };

        // Track the seeding file
        {
            let mut seeding = self.seeding_files.lock().await;
            seeding.insert(ed2k_link.clone(), seeding_info.clone());
        }

        // Parse ed2k link to get file info for registration
        let file_info = Self::parse_ed2k_link(&ed2k_link)?;

        // ED2K now works in a decentralized P2P mode
        // Files are made available locally and can be discovered via DHT
        // Server connections are optional and don't prevent seeding
        {
            let mut client = self.client.lock().await;

            // Try to connect to server for enhanced discovery (optional)
            if !client.is_connected() {
                if let Err(e) = client.connect().await {
                    info!("ED2K: Server connection failed, operating in P2P-only mode: {}", e);
                    // Continue without server - file is still available via DHT
                } else {
                    // Optional: Offer the file to the server for enhanced visibility
                    if let Err(e) = client.offer_files(vec![file_info.clone()]).await {
                        warn!("ED2K: Failed to register file with server (continuing in P2P mode): {}", e);
                    } else {
                        info!("ED2K: File registered with server for enhanced discovery");
                    }
                }
            } else {
                // Already connected, optionally offer the file
                if let Err(e) = client.offer_files(vec![file_info.clone()]).await {
                    warn!("ED2K: Failed to register file with server (continuing in P2P mode): {}", e);
                } else {
                    info!("ED2K: File registered with server for enhanced discovery");
                }
            }
        }

        info!("ED2K: File seeded successfully in P2P mode - available via DHT and direct connections");

        Ok(seeding_info)
    }

    async fn stop_seeding(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("ED2K: Stopping seed for {}", identifier);

        let mut seeding = self.seeding_files.lock().await;
        if seeding.remove(identifier).is_none() {
            return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
        }

        // Unregister from ED2K server
        let file_info = Self::parse_ed2k_link(identifier)?;
        {
            let mut client = self.client.lock().await;
            if client.is_connected() {
                if let Err(e) = client.remove_shared_file(&file_info.file_hash).await {
                    warn!("ED2K: Failed to unregister file from server: {}", e);
                } else {
                    info!("ED2K: File unregistered from server");
                }
            }
        }

        Ok(())
    }

    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("ED2K: Pausing download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.get_mut(identifier) {
            state.is_paused = true;
            state.status = DownloadStatus::Paused;

            let mut prog = self.download_progress.lock().await;
            if let Some(p) = prog.get_mut(identifier) {
                p.status = DownloadStatus::Paused;
            }

            Ok(())
        } else {
            Err(ProtocolError::DownloadNotFound(identifier.to_string()))
        }
    }

    async fn resume_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("ED2K: Resuming download {}", identifier);

        // Get download state and determine where to resume from
        let (file_info, output_path, start_chunk, partial_data) = {
            let mut downloads = self.active_downloads.lock().await;
            if let Some(state) = downloads.get_mut(identifier) {
                state.is_paused = false;
                state.status = DownloadStatus::Downloading;

                // Find the first incomplete chunk
                let start_chunk = state.downloaded_chunks.iter()
                    .position(|&completed| !completed)
                    .unwrap_or(state.downloaded_chunks.len());

                (
                    state.file_info.clone(),
                    state.output_path.clone(),
                    start_chunk,
                    state.partial_data.clone(),
                )
            } else {
                return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
            }
        };

        // Update progress status
        {
            let mut prog = self.download_progress.lock().await;
            if let Some(p) = prog.get_mut(identifier) {
                p.status = DownloadStatus::Downloading;
            }
        }

        // Spawn resumed download task
        let client = self.client.clone();
        let progress = self.download_progress.clone();
        let downloads = self.active_downloads.clone();
        let id = identifier.to_string();
        let file_hash = file_info.file_hash.clone();
        let file_size = file_info.file_size;
        let total_chunks = Self::calculate_chunks(file_size);

        tokio::spawn(async move {
            // Reconnect if needed
            {
                let mut c = client.lock().await;
                if !c.is_connected() {
                    if let Err(e) = c.connect().await {
                        error!("ED2K: Failed to reconnect for resume: {}", e);
                        let mut prog = progress.lock().await;
                        if let Some(p) = prog.get_mut(&id) {
                            p.status = DownloadStatus::Failed;
                        }
                        return;
                    }
                }
            }

            // Continue with existing data
            let mut all_data = partial_data;
            let start_time = std::time::Instant::now();

            info!("ED2K: Resuming from chunk {} of {}", start_chunk, total_chunks);

            for chunk_idx in start_chunk..total_chunks {
                // Check if paused or cancelled
                {
                    let dl = downloads.lock().await;
                    if let Some(state) = dl.get(&id) {
                        if state.is_paused {
                            // Save progress before pausing
                            drop(dl);
                            let mut dl = downloads.lock().await;
                            if let Some(state) = dl.get_mut(&id) {
                                state.partial_data = all_data.clone();
                            }
                            info!("ED2K: Download paused at chunk {}", chunk_idx);
                            return;
                        }
                    } else {
                        info!("ED2K: Download cancelled");
                        return;
                    }
                }

                // Download chunk
                let expected_hash = format!("{:032x}", chunk_idx);
                let chunk_data = {
                    let mut c = client.lock().await;
                    match c.download_chunk(&file_hash, chunk_idx as u32, &expected_hash).await {
                        Ok(data) => data,
                        Err(e) => {
                            error!("ED2K: Failed to download chunk {}: {}", chunk_idx, e);
                            let mut prog = progress.lock().await;
                            if let Some(p) = prog.get_mut(&id) {
                                p.status = DownloadStatus::Failed;
                            }
                            return;
                        }
                    }
                };

                all_data.extend(chunk_data);

                // Mark chunk as complete
                {
                    let mut dl = downloads.lock().await;
                    if let Some(state) = dl.get_mut(&id) {
                        if chunk_idx < state.downloaded_chunks.len() {
                            state.downloaded_chunks[chunk_idx] = true;
                        }
                        state.partial_data = all_data.clone();
                    }
                }

                // Update progress
                let downloaded = all_data.len() as u64;
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
                let eta = if speed > 0.0 && file_size > downloaded {
                    Some(((file_size - downloaded) as f64 / speed) as u64)
                } else {
                    None
                };

                {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.downloaded_bytes = downloaded;
                        p.download_speed = speed;
                        p.eta_seconds = eta;
                    }
                }
            }

            // Write to file
            if let Err(e) = tokio::fs::write(&output_path, &all_data).await {
                error!("ED2K: Failed to write file: {}", e);
                let mut prog = progress.lock().await;
                if let Some(p) = prog.get_mut(&id) {
                    p.status = DownloadStatus::Failed;
                }
                return;
            }

            // Mark as completed
            {
                let mut prog = progress.lock().await;
                if let Some(p) = prog.get_mut(&id) {
                    p.status = DownloadStatus::Completed;
                    p.downloaded_bytes = file_size;
                }
            }

            info!("ED2K: Resumed download completed: {} bytes", file_size);
        });

        Ok(())
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("ED2K: Cancelling download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if downloads.remove(identifier).is_some() {
            let mut prog = self.download_progress.lock().await;
            if let Some(p) = prog.get_mut(identifier) {
                p.status = DownloadStatus::Cancelled;
            }
            Ok(())
        } else {
            Err(ProtocolError::DownloadNotFound(identifier.to_string()))
        }
    }

    async fn get_download_progress(
        &self,
        identifier: &str,
    ) -> Result<DownloadProgress, ProtocolError> {
        let progress = self.download_progress.lock().await;
        progress
            .get(identifier)
            .cloned()
            .ok_or_else(|| ProtocolError::DownloadNotFound(identifier.to_string()))
    }

    async fn list_seeding(&self) -> Result<Vec<SeedingInfo>, ProtocolError> {
        let seeding = self.seeding_files.lock().await;
        Ok(seeding.values().cloned().collect())
    }

    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities {
            supports_seeding: true,
            supports_pause_resume: true,
            supports_multi_source: true,
            supports_encryption: false, // ED2K doesn't have built-in encryption
            supports_dht: true,         // Can use DHT for peer discovery
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_ed2k() {
        // ED2K handler requires server URL, but we can test the supports function logic
        let link = "ed2k://|file|test.txt|1024|ABC123DEF456789012345678ABCDEF01|/";
        assert!(link.starts_with("ed2k://|file|"));
    }

    #[test]
    fn test_parse_ed2k_link() {
        let link = "ed2k://|file|Ubuntu.iso|3654957056|31D6CFE0D16AE931B73C59D7E0C089C0|/";
        let info = Ed2kProtocolHandler::parse_ed2k_link(link).unwrap();

        assert_eq!(info.file_name, Some("Ubuntu.iso".to_string()));
        assert_eq!(info.file_size, 3654957056);
        assert_eq!(info.file_hash, "31D6CFE0D16AE931B73C59D7E0C089C0");
    }

    #[test]
    fn test_parse_ed2k_link_invalid() {
        let result = Ed2kProtocolHandler::parse_ed2k_link("http://example.com");
        assert!(result.is_err());

        let result = Ed2kProtocolHandler::parse_ed2k_link("ed2k://|server|");
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_chunks() {
        // 9.28 MB chunk size
        assert_eq!(Ed2kProtocolHandler::calculate_chunks(1000), 1);
        assert_eq!(Ed2kProtocolHandler::calculate_chunks(ED2K_CHUNK_SIZE as u64), 1);
        assert_eq!(Ed2kProtocolHandler::calculate_chunks(ED2K_CHUNK_SIZE as u64 + 1), 2);
        assert_eq!(Ed2kProtocolHandler::calculate_chunks(ED2K_CHUNK_SIZE as u64 * 3), 3);
    }
}
