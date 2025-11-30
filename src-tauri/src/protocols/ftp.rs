//! FTP Protocol Handler
//!
//! Wraps the existing FTP download functionality to implement the enhanced ProtocolHandler trait.
//! Supports TransferEventBus integration for UI progress tracking.

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus,
    ProtocolCapabilities, ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
};
use crate::ftp_downloader::{FtpDownloader, FtpCredentials, FtpDownloadConfig};
use crate::transfer_events::{
    current_timestamp_ms, ChunkCompletedEvent, DisconnectReason, ErrorCategory,
    SourceConnectedEvent, SourceDisconnectedEvent, SourceInfo, SourceSummary,
    SourceType, TransferCanceledEvent, TransferCompletedEvent, TransferEventBus,
    TransferFailedEvent, TransferPausedEvent, TransferStartedEvent, PauseReason,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tokio::sync::Mutex;
use tracing::{info, warn};
use url::Url;

/// FTP protocol handler implementing the enhanced ProtocolHandler trait
pub struct FtpProtocolHandler {
    /// Underlying FTP downloader (wrapped in Arc for sharing across async tasks)
    downloader: Arc<FtpDownloader>,
    /// Track active downloads
    active_downloads: Arc<Mutex<HashMap<String, FtpDownloadState>>>,
    /// Track download progress
    download_progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
    /// Optional event bus for emitting transfer events to frontend
    event_bus: Option<Arc<TransferEventBus>>,
}

/// Internal state for an FTP download
struct FtpDownloadState {
    url: String,
    output_path: PathBuf,
    started_at: u64,
    status: DownloadStatus,
    credentials: Option<FtpCredentials>,
    is_paused: bool,
    /// File name for event reporting
    file_name: String,
    /// File size (if known)
    file_size: u64,
}

impl FtpProtocolHandler {
    /// Creates a new FTP protocol handler with default config (no event bus)
    pub fn new() -> Self {
        Self {
            downloader: Arc::new(FtpDownloader::new()),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: None,
        }
    }

    /// Creates a handler with custom configuration (no event bus)
    pub fn with_config(config: FtpDownloadConfig) -> Self {
        Self {
            downloader: Arc::new(FtpDownloader::with_config(config)),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: None,
        }
    }

    /// Creates a handler with event bus for UI integration
    pub fn with_event_bus(app_handle: AppHandle) -> Self {
        Self {
            downloader: Arc::new(FtpDownloader::new()),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: Some(Arc::new(TransferEventBus::new(app_handle))),
        }
    }

    /// Creates a handler with both custom configuration and event bus
    pub fn with_config_and_event_bus(config: FtpDownloadConfig, app_handle: AppHandle) -> Self {
        Self {
            downloader: Arc::new(FtpDownloader::with_config(config)),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: Some(Arc::new(TransferEventBus::new(app_handle))),
        }
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

    /// Generate a unique ID for tracking downloads
    fn generate_id(url: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        format!("ftp-{:x}", hasher.finish())
    }

    /// Extract file name from URL path
    fn extract_file_name(url: &Url) -> String {
        url.path_segments()
            .and_then(|segments| segments.last())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown_file".to_string())
    }

    /// Parse FTP URL and extract credentials if present
    fn parse_ftp_url(url: &str) -> Result<(String, Option<FtpCredentials>), ProtocolError> {
        let parsed = Url::parse(url)
            .map_err(|e| ProtocolError::InvalidIdentifier(e.to_string()))?;

        let credentials = if !parsed.username().is_empty() {
            Some(FtpCredentials {
                username: parsed.username().to_string(),
                password: parsed.password().unwrap_or("").to_string(),
            })
        } else {
            None
        };

        Ok((url.to_string(), credentials))
    }
}

impl Default for FtpProtocolHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolHandler for FtpProtocolHandler {
    fn name(&self) -> &'static str {
        "ftp"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("ftp://") || identifier.starts_with("ftps://")
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("FTP: Starting download for {}", identifier);

        let download_id = Self::generate_id(identifier);

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(&download_id) {
                return Err(ProtocolError::AlreadyExists(download_id));
            }
        }

        // Parse URL and extract credentials
        let (url, credentials) = Self::parse_ftp_url(identifier)?;

        let parsed_url = Url::parse(&url)
            .map_err(|e| ProtocolError::InvalidIdentifier(e.to_string()))?;

        let file_name = Self::extract_file_name(&parsed_url);
        let started_at = Self::now();
        let source_id = format!("ftp-{}", parsed_url.host_str().unwrap_or("unknown"));

        // Initialize progress
        {
            let mut prog = self.download_progress.lock().await;
            prog.insert(download_id.clone(), DownloadProgress {
                downloaded_bytes: 0,
                total_bytes: 0,
                download_speed: 0.0,
                eta_seconds: None,
                active_peers: 1, // FTP has "1 peer" (the server)
                status: DownloadStatus::FetchingMetadata,
            });
        }

        // Track the download
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(download_id.clone(), FtpDownloadState {
                url: url.clone(),
                output_path: options.output_path.clone(),
                started_at,
                status: DownloadStatus::Downloading,
                credentials: credentials.clone(),
                is_paused: false,
                file_name: file_name.clone(),
                file_size: 0,
            });
        }

        // Clone necessary values for the spawned task
        let downloader = self.downloader.clone();
        let progress = self.download_progress.clone();
        let active_downloads = self.active_downloads.clone();
        let id = download_id.clone();
        let output_path = options.output_path.clone();
        let creds = credentials.clone();
        let event_bus = self.event_bus.clone();
        let task_source_id = source_id.clone();
        let task_file_name = file_name.clone();

        // Spawn download task
        tokio::spawn(async move {
            let start_time = Instant::now();

            // Connect to FTP server
            let mut stream = match downloader.connect_and_login(&parsed_url, creds).await {
                Ok(s) => s,
                Err(e) => {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.status = DownloadStatus::Failed;
                    }
                    // Emit failed event
                    if let Some(ref bus) = event_bus {
                        bus.emit_failed(TransferFailedEvent {
                            transfer_id: id.clone(),
                            file_hash: id.clone(),
                            failed_at: current_timestamp_ms(),
                            error: format!("FTP connection failed: {}", e),
                            error_category: ErrorCategory::Network,
                            downloaded_bytes: 0,
                            total_bytes: 0,
                            retry_possible: true,
                        });
                    }
                    tracing::error!("FTP connection failed: {}", e);
                    return;
                }
            };

            // Get remote path
            let remote_path = parsed_url.path();

            // Get file size
            let file_size = match downloader.get_file_size(&mut stream, remote_path).await {
                Ok(size) => {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.total_bytes = size;
                        p.status = DownloadStatus::Downloading;
                    }
                    // Update state with file size
                    let mut downloads = active_downloads.lock().await;
                    if let Some(state) = downloads.get_mut(&id) {
                        state.file_size = size;
                    }
                    size
                }
                Err(e) => {
                    tracing::warn!("Could not get file size: {}", e);
                    0
                }
            };

            // Create source info for events
            let source_info = SourceInfo {
                id: task_source_id.clone(),
                source_type: SourceType::Ftp,
                address: parsed_url.to_string(),
                reputation: None,
                estimated_speed_bps: None,
                latency_ms: None,
                location: None,
            };

            // Emit started event
            if let Some(ref bus) = event_bus {
                bus.emit_started(TransferStartedEvent {
                    transfer_id: id.clone(),
                    file_hash: id.clone(),
                    file_name: task_file_name.clone(),
                    file_size,
                    total_chunks: 1, // FTP downloads as single chunk
                    chunk_size: file_size as usize,
                    started_at: current_timestamp_ms(),
                    available_sources: vec![source_info.clone()],
                    selected_sources: vec![task_source_id.clone()],
                });

                // Emit source connected event
                bus.emit_source_connected(SourceConnectedEvent {
                    transfer_id: id.clone(),
                    source_id: task_source_id.clone(),
                    source_type: SourceType::Ftp,
                    source_info: source_info.clone(),
                    connected_at: current_timestamp_ms(),
                    assigned_chunks: vec![0], // Single chunk
                });
            }

            // Download the file
            match downloader.download_full(&mut stream, remote_path).await {
                Ok(data) => {
                    let download_duration_secs = start_time.elapsed().as_secs();
                    let download_speed = if download_duration_secs > 0 {
                        data.len() as f64 / download_duration_secs as f64
                    } else {
                        data.len() as f64
                    };

                    // Write to file
                    if let Err(e) = tokio::fs::write(&output_path, &data).await {
                        let mut prog = progress.lock().await;
                        if let Some(p) = prog.get_mut(&id) {
                            p.status = DownloadStatus::Failed;
                        }
                        // Emit failed event
                        if let Some(ref bus) = event_bus {
                            bus.emit_failed(TransferFailedEvent {
                                transfer_id: id.clone(),
                                file_hash: id.clone(),
                                failed_at: current_timestamp_ms(),
                                error: format!("Failed to write file: {}", e),
                                error_category: ErrorCategory::Filesystem,
                                downloaded_bytes: data.len() as u64,
                                total_bytes: file_size,
                                retry_possible: false,
                            });
                        }
                        tracing::error!("Failed to write file: {}", e);
                        return;
                    }

                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.downloaded_bytes = data.len() as u64;
                        p.download_speed = download_speed;
                        p.status = DownloadStatus::Completed;
                    }

                    // Emit completion events
                    if let Some(ref bus) = event_bus {
                        // Emit chunk completed (single chunk for FTP)
                        bus.emit_chunk_completed(ChunkCompletedEvent {
                            transfer_id: id.clone(),
                            chunk_id: 0,
                            chunk_size: data.len(),
                            source_id: task_source_id.clone(),
                            source_type: SourceType::Ftp,
                            completed_at: current_timestamp_ms(),
                            download_duration_ms: start_time.elapsed().as_millis() as u64,
                            verified: true,
                        });

                        // Emit source disconnected (successful)
                        bus.emit_source_disconnected(SourceDisconnectedEvent {
                            transfer_id: id.clone(),
                            source_id: task_source_id.clone(),
                            source_type: SourceType::Ftp,
                            disconnected_at: current_timestamp_ms(),
                            reason: DisconnectReason::Completed,
                            chunks_completed: 1,
                            will_retry: false,
                        });

                        // Emit transfer completed
                        bus.emit_completed(TransferCompletedEvent {
                            transfer_id: id.clone(),
                            file_hash: id.clone(),
                            file_name: task_file_name.clone(),
                            file_size: data.len() as u64,
                            output_path: output_path.to_string_lossy().to_string(),
                            completed_at: current_timestamp_ms(),
                            duration_seconds: download_duration_secs,
                            average_speed_bps: download_speed,
                            total_chunks: 1,
                            sources_used: vec![SourceSummary {
                                source_id: task_source_id.clone(),
                                source_type: SourceType::Ftp,
                                chunks_provided: 1,
                                bytes_provided: data.len() as u64,
                                average_speed_bps: download_speed,
                                connection_duration_seconds: download_duration_secs,
                            }],
                        });
                    }

                    tracing::info!("FTP download completed: {} bytes in {} seconds", data.len(), download_duration_secs);
                }
                Err(e) => {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.status = DownloadStatus::Failed;
                    }
                    // Emit failed event
                    if let Some(ref bus) = event_bus {
                        bus.emit_source_disconnected(SourceDisconnectedEvent {
                            transfer_id: id.clone(),
                            source_id: task_source_id.clone(),
                            source_type: SourceType::Ftp,
                            disconnected_at: current_timestamp_ms(),
                            reason: DisconnectReason::NetworkError,
                            chunks_completed: 0,
                            will_retry: false,
                        });

                        bus.emit_failed(TransferFailedEvent {
                            transfer_id: id.clone(),
                            file_hash: id.clone(),
                            failed_at: current_timestamp_ms(),
                            error: format!("FTP download failed: {}", e),
                            error_category: ErrorCategory::Network,
                            downloaded_bytes: 0,
                            total_bytes: file_size,
                            retry_possible: true,
                        });
                    }
                    tracing::error!("FTP download failed: {}", e);
                }
            }

            // Disconnect
            let _ = downloader.disconnect(&mut stream).await;
        });

        Ok(DownloadHandle {
            identifier: download_id,
            protocol: "ftp".to_string(),
            started_at,
        })
    }

    async fn seed(
        &self,
        file_path: PathBuf,
        _options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        info!("FTP: Seeding not directly supported - would need FTP server");

        // FTP "seeding" would mean uploading to an FTP server
        // This requires server configuration which we don't have here
        warn!("FTP seeding requires an FTP server to be configured");

        // Return a placeholder - in a real implementation, this would upload to a configured server
        Ok(SeedingInfo {
            identifier: format!("ftp://chiral-node/{}", file_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string())),
            file_path,
            protocol: "ftp".to_string(),
            active_peers: 0,
            bytes_uploaded: 0,
        })
    }

    async fn stop_seeding(&self, identifier: &str) -> Result<(), ProtocolError> {
        warn!("FTP: stop_seeding - {}", identifier);
        // Would delete file from FTP server
        Ok(())
    }

    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("FTP: Pausing download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.get_mut(identifier) {
            state.is_paused = true;
            state.status = DownloadStatus::Paused;

            let downloaded_bytes = {
                let mut prog = self.download_progress.lock().await;
                if let Some(p) = prog.get_mut(identifier) {
                    p.status = DownloadStatus::Paused;
                    p.downloaded_bytes
                } else {
                    0
                }
            };

            // Emit paused event
            if let Some(ref bus) = self.event_bus {
                bus.emit_paused(TransferPausedEvent {
                    transfer_id: identifier.to_string(),
                    paused_at: Self::now_ms(),
                    reason: PauseReason::UserRequested,
                    can_resume: true,
                    downloaded_bytes,
                    total_bytes: state.file_size,
                });
            }

            // FTP supports REST command for resume, so pause is viable
            // The actual pause would need to close the connection and track position
            warn!("FTP: pause requires reconnection to resume with REST command");
            Ok(())
        } else {
            Err(ProtocolError::DownloadNotFound(identifier.to_string()))
        }
    }

    async fn resume_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("FTP: Resuming download {}", identifier);

        let downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.get(identifier) {
            if !state.is_paused {
                return Err(ProtocolError::ProtocolSpecific(
                    "Download is not paused".to_string()
                ));
            }

            // Would need to reconnect and use REST command to resume
            // This requires tracking the bytes already downloaded
            warn!("FTP: resume_download would use REST command - not fully implemented");
            Err(ProtocolError::NotSupported)
        } else {
            Err(ProtocolError::DownloadNotFound(identifier.to_string()))
        }
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("FTP: Cancelling download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.remove(identifier) {
            let downloaded_bytes = {
                let mut prog = self.download_progress.lock().await;
                if let Some(p) = prog.get_mut(identifier) {
                    p.status = DownloadStatus::Cancelled;
                    p.downloaded_bytes
                } else {
                    0
                }
            };

            // Emit canceled event
            if let Some(ref bus) = self.event_bus {
                bus.emit_canceled(TransferCanceledEvent {
                    transfer_id: identifier.to_string(),
                    canceled_at: Self::now_ms(),
                    downloaded_bytes,
                    total_bytes: state.file_size,
                    keep_partial: false,
                });
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
        // Would list files on configured FTP server
        Ok(Vec::new())
    }

    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities {
            supports_seeding: true,  // Via upload to FTP server
            supports_pause_resume: true,  // Via REST command
            supports_multi_source: false,
            supports_encryption: true,  // FTPS
            supports_dht: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_ftp() {
        let handler = FtpProtocolHandler::new();
        assert!(handler.supports("ftp://ftp.example.com/file.zip"));
        assert!(handler.supports("ftps://secure.example.com/file.zip"));
        assert!(!handler.supports("http://example.com/file.zip"));
        assert!(!handler.supports("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn test_parse_ftp_url_no_creds() {
        let (url, creds) = FtpProtocolHandler::parse_ftp_url("ftp://example.com/file.zip").unwrap();
        assert_eq!(url, "ftp://example.com/file.zip");
        assert!(creds.is_none());
    }

    #[test]
    fn test_parse_ftp_url_with_creds() {
        let (_, creds) = FtpProtocolHandler::parse_ftp_url("ftp://user:pass@example.com/file.zip").unwrap();
        assert!(creds.is_some());
        let c = creds.unwrap();
        assert_eq!(c.username, "user");
        assert_eq!(c.password, "pass");
    }

    #[test]
    fn test_generate_id() {
        let id1 = FtpProtocolHandler::generate_id("ftp://example.com/file.zip");
        let id2 = FtpProtocolHandler::generate_id("ftp://example.com/file.zip");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("ftp-"));
    }
}