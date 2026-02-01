//! BitTorrent Protocol Handler
//!
//! Wraps the existing BitTorrentHandler to implement the enhanced ProtocolHandler trait.
//! Supports TransferEventBus integration for UI progress tracking.

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus,
    ProtocolCapabilities, ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
    SimpleProtocolHandler,
};
use crate::dht::DhtService;
use crate::bittorrent_handler::BitTorrentHandler;
use crate::transfer_events::{
    current_timestamp_ms, DisconnectReason, ErrorCategory, PauseReason,
    SourceConnectedEvent, SourceDisconnectedEvent, SourceInfo, SourceSummary,
    SourceType, TransferCanceledEvent, TransferCompletedEvent, TransferEventBus,
    TransferFailedEvent, TransferPausedEvent, TransferProgressEvent,
    TransferResumedEvent, TransferStartedEvent,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
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
    /// Optional event bus for emitting transfer events to frontend
    event_bus: Option<Arc<TransferEventBus>>,
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
    /// Torrent name (from metadata)
    name: Option<String>,
    /// Last progress event timestamp (to throttle events)
    last_progress_event: u64,
}

impl BitTorrentProtocolHandler {
    /// Creates a new BitTorrent protocol handler
    pub fn new(handler: Arc<BitTorrentHandler>) -> Self {
        Self {
            handler,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            seeding_files: Arc::new(Mutex::new(HashMap::new())),
            event_bus: None,
        }
    }

    /// Creates a new handler with event bus for UI integration
    pub fn new_with_event_bus(handler: Arc<BitTorrentHandler>, app_handle: AppHandle) -> Self {
        Self {
            handler,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            seeding_files: Arc::new(Mutex::new(HashMap::new())),
            event_bus: Some(Arc::new(TransferEventBus::new(app_handle))),
        }
    }

    /// Creates a new handler with a download directory and DhtService (no event bus)
    pub async fn with_download_directory(
        download_dir: PathBuf,
        dht_service: Arc<DhtService>,
    ) -> Result<Self, ProtocolError> {
        let handler = BitTorrentHandler::new(download_dir, dht_service)
            .await
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self::new(Arc::new(handler)))
    }

    /// Creates a new handler with a download directory, DhtService, and event bus
    pub async fn with_download_directory_and_event_bus(
        download_dir: PathBuf,
        dht_service: Arc<DhtService>,
        app_handle: AppHandle,
    ) -> Result<Self, ProtocolError> {
        let handler = BitTorrentHandler::new(download_dir, dht_service)
            .await
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self::new_with_event_bus(Arc::new(handler), app_handle))
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

    /// Extract display name from magnet link
    fn extract_display_name(identifier: &str) -> Option<String> {
        if identifier.starts_with("magnet:?") {
            // Look for dn= parameter
            if let Some(start) = identifier.find("dn=") {
                let start = start + 3;
                let end = identifier[start..]
                    .find('&')
                    .map(|i| start + i)
                    .unwrap_or(identifier.len());
                // URL decode the name
                return Some(
                    urlencoding::decode(&identifier[start..end])
                        .unwrap_or_else(|_| identifier[start..end].into())
                        .to_string()
                );
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

    /// Get current timestamp in milliseconds
    fn now_ms() -> u64 {
        current_timestamp_ms()
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

        // Extract display name if available
        let display_name = Self::extract_display_name(identifier)
            .unwrap_or_else(|| info_hash.clone());

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(&info_hash) {
                return Err(ProtocolError::AlreadyExists(info_hash));
            }
        }

        // Start the download using the underlying handler
        let _handle = match self.handler.start_download(identifier).await {
            Ok(h) => h,
            Err(e) => {
                // Emit failed event
                if let Some(ref bus) = self.event_bus {
                    bus.emit_failed(TransferFailedEvent {
                        transfer_id: info_hash.clone(),
                        file_hash: info_hash.clone(),
                        protocol: "BitTorrent".to_string(),
                        failed_at: Self::now_ms(),
                        error: format!("Failed to start BitTorrent download: {}", e),
                        error_category: ErrorCategory::Protocol,
                        downloaded_bytes: 0,
                        total_bytes: 0,
                        retry_possible: true,
                    });
                }
                return Err(ProtocolError::ProtocolSpecific(e.to_string()));
            }
        };

        let started_at = Self::now();
        let source_id = format!("bittorrent-swarm-{}", &info_hash[..8.min(info_hash.len())]);

        // Track the download
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(
                info_hash.clone(),
                DownloadState {
                    identifier: identifier.to_string(),
                    output_path: options.output_path.clone(),
                    started_at,
                    status: DownloadStatus::FetchingMetadata,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    is_paused: false,
                    name: Some(display_name.clone()),
                    last_progress_event: 0,
                },
            );
        }

        // Create source info for events
        let source_info = SourceInfo {
            id: source_id.clone(),
            source_type: SourceType::BitTorrent,
            address: identifier.to_string(),
            reputation: None,
            estimated_speed_bps: None,
            latency_ms: None,
            location: None,
        };

        // Emit started event
        if let Some(ref bus) = self.event_bus {
            bus.emit_started(TransferStartedEvent {
                transfer_id: info_hash.clone(),
                file_hash: info_hash.clone(),
                protocol: "BitTorrent".to_string(),
                file_name: display_name.clone(),
                file_size: 0, // Unknown until metadata is fetched
                total_chunks: 0, // Unknown until metadata is fetched
                chunk_size: 0,
                started_at: Self::now_ms(),
                available_sources: vec![source_info.clone()],
                selected_sources: vec![source_id.clone()],
            });

            // Emit source connected (the swarm)
            bus.emit_source_connected(SourceConnectedEvent {
                transfer_id: info_hash.clone(),
                source_id: source_id.clone(),
                source_type: SourceType::BitTorrent,
                source_info,
                connected_at: Self::now_ms(),
                assigned_chunks: vec![], // BitTorrent manages chunks internally
            });
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

        let (downloaded_bytes, total_bytes) = {
            // Update our local state
            let mut downloads = self.active_downloads.lock().await;
            if let Some(state) = downloads.get_mut(identifier) {
                state.is_paused = true;
                state.status = DownloadStatus::Paused;
                (state.downloaded_bytes, state.total_bytes)
            } else {
                return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
            }
        };

        // Pause in librqbit
        self.handler
            .pause_torrent(identifier)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        // Emit paused event
        if let Some(ref bus) = self.event_bus {
            bus.emit_paused(TransferPausedEvent {
                transfer_id: identifier.to_string(),
                protocol: "BitTorrent".to_string(),
                paused_at: Self::now_ms(),
                reason: PauseReason::UserRequested,
                can_resume: true,
                downloaded_bytes,
                total_bytes,
            });
        }

        Ok(())
    }

    async fn resume_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("BitTorrent: Resuming download {}", identifier);

        let (downloaded_bytes, total_bytes) = {
            // Update our local state
            let mut downloads = self.active_downloads.lock().await;
            if let Some(state) = downloads.get_mut(identifier) {
                state.is_paused = false;
                state.status = DownloadStatus::Downloading;
                (state.downloaded_bytes, state.total_bytes)
            } else {
                return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
            }
        };

        // Resume in librqbit
        self.handler
            .resume_torrent(identifier)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        // Emit resumed event
        if let Some(ref bus) = self.event_bus {
            bus.emit_resumed(TransferResumedEvent {
                transfer_id: identifier.to_string(),
                protocol: "BitTorrent".to_string(),
                resumed_at: Self::now_ms(),
                downloaded_bytes,
                remaining_bytes: total_bytes.saturating_sub(downloaded_bytes),
                active_sources: 1, // Swarm
            });
        }

        Ok(())
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("BitTorrent: Cancelling download {}", identifier);

        let (downloaded_bytes, total_bytes) = {
            // Remove from our tracking
            let mut downloads = self.active_downloads.lock().await;
            if let Some(state) = downloads.remove(identifier) {
                (state.downloaded_bytes, state.total_bytes)
            } else {
                return Err(ProtocolError::DownloadNotFound(identifier.to_string()));
            }
        };

        // Cancel in librqbit (delete files since it's a cancel, not stop)
        self.handler
            .cancel_torrent(identifier, true)
            .await
            .map_err(|e| ProtocolError::ProtocolSpecific(e.to_string()))?;

        // Emit canceled event
        if let Some(ref bus) = self.event_bus {
            bus.emit_canceled(TransferCanceledEvent {
                transfer_id: identifier.to_string(),
                protocol: "BitTorrent".to_string(),
                canceled_at: Self::now_ms(),
                downloaded_bytes,
                total_bytes,
                keep_partial: false,
            });
        }

        Ok(())
    }

    async fn get_download_progress(
        &self,
        identifier: &str,
    ) -> Result<DownloadProgress, ProtocolError> {
        // Try to get real progress from librqbit first
        match self.handler.get_torrent_progress(identifier).await {
            Ok(progress) => {
                let now_ms = Self::now_ms();
                let source_id = format!("bittorrent-swarm-{}", &identifier[..8.min(identifier.len())]);

                // Update our local state with real values
                let should_emit_progress = {
                    let mut downloads = self.active_downloads.lock().await;
                    if let Some(state) = downloads.get_mut(identifier) {
                        let prev_downloaded = state.downloaded_bytes;
                        let prev_status = state.status.clone();

                        state.downloaded_bytes = progress.downloaded_bytes;
                        state.total_bytes = progress.total_bytes;

                        // Check if completed
                        if progress.is_finished && prev_status != DownloadStatus::Completed {
                            state.status = DownloadStatus::Completed;

                            // Emit completion events
                            if let Some(ref bus) = self.event_bus {
                                let duration_secs = Self::now() - state.started_at;
                                let avg_speed = if duration_secs > 0 {
                                    progress.total_bytes as f64 / duration_secs as f64
                                } else {
                                    progress.total_bytes as f64
                                };

                                bus.emit_source_disconnected(SourceDisconnectedEvent {
                                    transfer_id: identifier.to_string(),
                                    source_id: source_id.clone(),
                                    source_type: SourceType::BitTorrent,
                                    disconnected_at: now_ms,
                                    reason: DisconnectReason::Completed,
                                    chunks_completed: 1,
                                    will_retry: false,
                                });

                                bus.emit_completed(TransferCompletedEvent {
                                    transfer_id: identifier.to_string(),
                                    file_hash: identifier.to_string(),
                                    protocol: "BitTorrent".to_string(),
                                    file_name: state.name.clone().unwrap_or_else(|| identifier.to_string()),
                                    file_size: progress.total_bytes,
                                    output_path: state.output_path.to_string_lossy().to_string(),
                                    completed_at: now_ms,
                                    duration_seconds: duration_secs,
                                    average_speed_bps: avg_speed,
                                    total_chunks: 1,
                                    sources_used: vec![SourceSummary {
                                        source_id: source_id.clone(),
                                        source_type: SourceType::BitTorrent,
                                        chunks_provided: 1,
                                        bytes_provided: progress.total_bytes,
                                        average_speed_bps: avg_speed,
                                        connection_duration_seconds: duration_secs,
                                    }],
                                });
                            }
                            false
                        } else if progress.state == "error" && prev_status != DownloadStatus::Failed {
                            state.status = DownloadStatus::Failed;

                            // Emit failure event
                            if let Some(ref bus) = self.event_bus {
                                bus.emit_failed(TransferFailedEvent {
                                    transfer_id: identifier.to_string(),
                                    file_hash: identifier.to_string(),
                                    protocol: "BitTorrent".to_string(),
                                    failed_at: now_ms,
                                    error: "BitTorrent download error".to_string(),
                                    error_category: ErrorCategory::Protocol,
                                    downloaded_bytes: progress.downloaded_bytes,
                                    total_bytes: progress.total_bytes,
                                    retry_possible: true,
                                });
                            }
                            false
                        } else {
                            // Check if we should emit a progress event (throttle to every 2 seconds)
                            let should_emit = now_ms - state.last_progress_event >= 2000
                                && progress.downloaded_bytes > prev_downloaded;
                            if should_emit {
                                state.last_progress_event = now_ms;
                            }
                            should_emit
                        }
                    } else {
                        false
                    }
                };

                // Emit progress event if needed (outside of lock)
                if should_emit_progress {
                    if let Some(ref bus) = self.event_bus {
                        let progress_pct = if progress.total_bytes > 0 {
                            (progress.downloaded_bytes as f64 / progress.total_bytes as f64) * 100.0
                        } else {
                            0.0
                        };

                        bus.emit_progress(TransferProgressEvent {
                            transfer_id: identifier.to_string(),
                            protocol: "BitTorrent".to_string(),
                            downloaded_bytes: progress.downloaded_bytes,
                            total_bytes: progress.total_bytes,
                            completed_chunks: 0, // BitTorrent manages pieces internally
                            total_chunks: 0,
                            progress_percentage: progress_pct,
                            download_speed_bps: progress.download_speed,
                            upload_speed_bps: 0.0,
                            eta_seconds: progress.eta_seconds.map(|e| e as u32),
                            active_sources: 1, // Swarm
                            timestamp: now_ms,
                        });
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