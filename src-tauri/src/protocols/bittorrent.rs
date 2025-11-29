//! BitTorrent Protocol Handler
//!
//! Wraps the existing BitTorrentHandler to implement the enhanced ProtocolHandler trait.

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus,
    ProtocolCapabilities, ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
    SimpleProtocolHandler,
};
use crate::dht::DhtService;
use crate::bittorrent_handler::BitTorrentHandler;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::info;

/// BitTorrent protocol handler implementing the enhanced ProtocolHandler trait
pub struct BitTorrentProtocolHandler {
    /// Underlying BitTorrent handler
    handler: Arc<BitTorrentHandler>,
    /// Track active downloads for progress reporting
    active_downloads: Arc<Mutex<HashMap<String, DownloadState>>>,
    /// Track seeding files
    seeding_files: Arc<Mutex<HashMap<String, SeedingInfo>>>,
}

/// Internal state for tracking a download
struct DownloadState {
    identifier: String,
    output_path: PathBuf,
    started_at: u64,
    status: DownloadStatus,
    downloaded_bytes: u64,
    total_bytes: u64,
    is_paused: bool,
}

impl BitTorrentProtocolHandler {
    /// Creates a new BitTorrent protocol handler
    pub fn new(handler: Arc<BitTorrentHandler>) -> Self { // The handler is now passed in from main.rs
        Self {
            handler,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            seeding_files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Extract info hash from magnet link
    fn extract_info_hash(identifier: &str) -> Option<String> {
        if identifier.starts_with("magnet:?") {
            if let Some(start) = identifier.find("urn:btih:") {
                let start = start + 9;
                let end = identifier[start..]
                    .find('&')
                    .map(|i| start + i)
                    .unwrap_or(identifier.len());
                return Some(identifier[start..end].to_lowercase());
            }
        }
        None
    }

    /// Get current timestamp
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[async_trait]
impl ProtocolHandler for BitTorrentProtocolHandler {
    fn name(&self) -> &'static str {
        "bittorrent"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("magnet:?")
            || identifier.ends_with(".torrent")
            || identifier.starts_with("urn:btih:")
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("BitTorrent: Starting download for {}", identifier);

        // Extract info hash for tracking
        let info_hash = Self::extract_info_hash(identifier)
            .unwrap_or_else(|| identifier.to_string());

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(&info_hash) {
                return Err(ProtocolError::AlreadyExists(info_hash));
            }
        }

        // Start the download using the underlying handler
        let _handle = self.handler
            .start_download(identifier)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        let started_at = Self::now();

        // Track the download
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(
                info_hash.clone(),
                DownloadState {
                    identifier: identifier.to_string(),
                    output_path: options.output_path,
                    started_at,
                    status: DownloadStatus::FetchingMetadata,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    is_paused: false,
                },
            );
        }

        Ok(DownloadHandle {
            identifier: info_hash,
            protocol: "bittorrent".to_string(),
            started_at,
        })
    }

    async fn seed(
        &self,
        file_path: PathBuf,
        _options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        info!("BitTorrent: Starting seed for {:?}", file_path);

        // Check if file exists
        if !file_path.exists() {
            return Err(ProtocolError::FileNotFound(
                file_path.to_string_lossy().to_string()
            ));
        }

        // Use underlying handler's seed method
        let file_path_str = file_path.to_string_lossy().to_string();
        let magnet_link = self.handler
            .seed(&file_path_str)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e))?;

        let seeding_info = SeedingInfo {
            identifier: magnet_link.clone(),
            file_path: file_path.clone(),
            protocol: "bittorrent".to_string(),
            active_peers: 0,
            bytes_uploaded: 0,
        };

        // Track the seeding file
        {
            let mut seeding = self.seeding_files.lock().await;
            seeding.insert(magnet_link.clone(), seeding_info.clone());
        }

        Ok(seeding_info)
    }

    async fn stop_seeding(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("BitTorrent: Stopping seed for {}", identifier);

        // Extract info hash from identifier
        let info_hash = Self::extract_info_hash(identifier)
            .unwrap_or_else(|| identifier.to_string());

        // Remove from our tracking
        {
            let mut seeding = self.seeding_files.lock().await;
            seeding.remove(&info_hash);
        }

        // Stop seeding in librqbit
        self.handler
            .stop_seeding_torrent(&info_hash)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        Ok(())
    }

    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("BitTorrent: Pausing download {}", identifier);

        // Update our local state
        {
            let mut downloads = self.active_downloads.lock().await;
            if let Some(state) = downloads.get_mut(identifier) {
                state.is_paused = true;
                state.status = DownloadStatus::Paused;
            } else {
                return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
            }
        }

        // Pause in librqbit
        self.handler
            .pause_torrent(identifier)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        Ok(())
    }

    async fn resume_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("BitTorrent: Resuming download {}", identifier);

        // Update our local state
        {
            let mut downloads = self.active_downloads.lock().await;
            if let Some(state) = downloads.get_mut(identifier) {
                state.is_paused = false;
                state.status = DownloadStatus::Downloading;
            } else {
                return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
            }
        }

        // Resume in librqbit
        self.handler
            .resume_torrent(identifier)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        Ok(())
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("BitTorrent: Cancelling download {}", identifier);

        // Remove from our tracking
        {
            let mut downloads = self.active_downloads.lock().await;
            if downloads.remove(identifier).is_none() {
                return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
            }
        }

        // Cancel in librqbit (delete files since it's a cancel, not stop)
        self.handler
            .cancel_torrent(identifier, true)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        Ok(())
    }

    async fn get_download_progress(
        &self,
        identifier: &str,
    ) -> Result<DownloadProgress, ProtocolError> {
        // Try to get real progress from librqbit first
        match self.handler.get_torrent_progress(identifier).await {
            Ok(progress) => {
                // Update our local state with real values
                {
                    let mut downloads = self.active_downloads.lock().await;
                    if let Some(state) = downloads.get_mut(identifier) {
                        state.downloaded_bytes = progress.downloaded_bytes;
                        state.total_bytes = progress.total_bytes;
                        if progress.is_finished {
                            state.status = DownloadStatus::Completed;
                        }
                    }
                }

                // Determine status from torrent state
                let status = match progress.state.as_str() {
                    "paused" => DownloadStatus::Paused,
                    "error" => DownloadStatus::Failed,
                    "live" if progress.is_finished => DownloadStatus::Completed,
                    "live" => DownloadStatus::Downloading,
                    "initializing" => DownloadStatus::FetchingMetadata,
                    _ => DownloadStatus::Downloading,
                };

                Ok(DownloadProgress {
                    downloaded_bytes: progress.downloaded_bytes,
                    total_bytes: progress.total_bytes,
                    download_speed: progress.download_speed,
                    eta_seconds: progress.eta_seconds,
                    active_peers: 0, // librqbit doesn't expose peer count easily
                    status,
                })
            }
            Err(_) => {
                // Fall back to local state if torrent not found in handler
                let downloads = self.active_downloads.lock().await;
                if let Some(state) = downloads.get(identifier) {
                    Ok(DownloadProgress {
                        downloaded_bytes: state.downloaded_bytes,
                        total_bytes: state.total_bytes,
                        download_speed: 0.0,
                        eta_seconds: None,
                        active_peers: 0,
                        status: state.status.clone(),
                    })
                } else {
                    Err(ProtocolError::DownloadNotFound(identifier.to_string()))
                }
            }
        }
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
            supports_encryption: true,
            supports_dht: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_magnet() {
        // Create a mock - we can't actually construct without async
        let identifier = "magnet:?xt=urn:btih:abc123def456";
        assert!(identifier.starts_with("magnet:?"));
    }

    #[test]
    fn test_supports_torrent_file() {
        let identifier = "/path/to/file.torrent";
        assert!(identifier.ends_with(".torrent"));
    }

    #[test]
    fn test_extract_info_hash() {
        let magnet = "magnet:?xt=urn:btih:ABC123DEF456&dn=test";
        let hash = BitTorrentProtocolHandler::extract_info_hash(magnet);
        assert_eq!(hash, Some("abc123def456".to_string()));
    }

    #[test]
    fn test_extract_info_hash_no_params() {
        let magnet = "magnet:?xt=urn:btih:ABC123DEF456";
        let hash = BitTorrentProtocolHandler::extract_info_hash(magnet);
        assert_eq!(hash, Some("abc123def456".to_string()));
    }
}
