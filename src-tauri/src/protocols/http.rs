//! HTTP Protocol Handler
//!
//! Wraps the existing HTTP download functionality to implement the enhanced ProtocolHandler trait.
//! Supports TransferEventBus integration for UI progress tracking.

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus,
    ProtocolCapabilities, ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
};
use crate::transfer_events::{
    current_timestamp_ms, DisconnectReason, ErrorCategory,
    SourceConnectedEvent, SourceDisconnectedEvent, SourceInfo, SourceSummary,
    SourceType, TransferCanceledEvent, TransferCompletedEvent, TransferEventBus,
    TransferFailedEvent, TransferProgressEvent, TransferStartedEvent,
};
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{info, warn, error};

/// HTTP protocol handler implementing the enhanced ProtocolHandler trait
pub struct HttpProtocolHandler {
    /// HTTP client for making requests
    client: Client,
    /// Track active downloads
    active_downloads: Arc<Mutex<HashMap<String, HttpDownloadState>>>,
    /// Track download progress
    download_progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
    /// Optional event bus for emitting transfer events to frontend
    event_bus: Option<Arc<TransferEventBus>>,
}

/// Internal state for an HTTP download
struct HttpDownloadState {
    url: String,
    output_path: PathBuf,
    started_at: u64,
    status: DownloadStatus,
    cancel_token: tokio::sync::watch::Sender<bool>,
    /// File name extracted from URL
    file_name: String,
    /// Total file size (if known)
    total_bytes: u64,
}

impl HttpProtocolHandler {
    /// Creates a new HTTP protocol handler (no event bus)
    pub fn new() -> Result<Self, ProtocolError> {
        let client = Client::builder()
            .user_agent("Chiral-Network/1.0")
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self {
            client,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: None,
        })
    }

    /// Creates a handler with custom timeout (no event bus)
    pub fn with_timeout(timeout_secs: u64) -> Result<Self, ProtocolError> {
        let client = Client::builder()
            .user_agent("Chiral-Network/1.0")
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self {
            client,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: None,
        })
    }

    /// Creates a handler with event bus for UI integration
    pub fn with_event_bus(app_handle: AppHandle) -> Result<Self, ProtocolError> {
        let client = Client::builder()
            .user_agent("Chiral-Network/1.0")
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self {
            client,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: Some(Arc::new(TransferEventBus::new(app_handle))),
        })
    }

    /// Creates a handler with custom timeout and event bus
    pub fn with_timeout_and_event_bus(timeout_secs: u64, app_handle: AppHandle) -> Result<Self, ProtocolError> {
        let client = Client::builder()
            .user_agent("Chiral-Network/1.0")
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self {
            client,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
            event_bus: Some(Arc::new(TransferEventBus::new(app_handle))),
        })
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
        format!("http-{:x}", hasher.finish())
    }

    /// Extract file name from URL
    fn extract_file_name(url: &str) -> String {
        url.rsplit('/')
            .next()
            .and_then(|s| s.split('?').next())
            .filter(|s| !s.is_empty())
            .unwrap_or("download")
            .to_string()
    }

    /// Download file with progress tracking and event emission
    async fn download_with_progress(
        client: Client,
        url: String,
        output_path: PathBuf,
        progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
        active_downloads: Arc<Mutex<HashMap<String, HttpDownloadState>>>,
        download_id: String,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
        event_bus: Option<Arc<TransferEventBus>>,
        file_name: String,
    ) -> Result<(), ProtocolError> {
        let start_time = Instant::now();
        let source_id = format!("http-{}", url.split('/').nth(2).unwrap_or("unknown"));

        // Initial HEAD request to get content length
        let head_response = match client.head(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                if let Some(ref bus) = event_bus {
                    bus.emit_failed(TransferFailedEvent {
                        transfer_id: download_id.clone(),
                        file_hash: download_id.clone(),
                        failed_at: current_timestamp_ms(),
                        error: format!("Failed to connect: {}", e),
                        error_category: ErrorCategory::Network,
                        downloaded_bytes: 0,
                        total_bytes: 0,
                        retry_possible: true,
                    });
                }
                return Err(ProtocolError::NetworkError(e.to_string()));
            }
        };

        let total_bytes = head_response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        // Update progress with total size
        {
            let mut prog = progress.lock().await;
            if let Some(p) = prog.get_mut(&download_id) {
                p.total_bytes = total_bytes;
                p.status = DownloadStatus::Downloading;
            }
        }

        // Update state with total size
        {
            let mut downloads = active_downloads.lock().await;
            if let Some(state) = downloads.get_mut(&download_id) {
                state.total_bytes = total_bytes;
            }
        }

        // Create source info
        let source_info = SourceInfo {
            id: source_id.clone(),
            source_type: SourceType::Http,
            address: url.clone(),
            reputation: None,
            estimated_speed_bps: None,
            latency_ms: None,
            location: None,
        };

        // Emit started and source connected events
        if let Some(ref bus) = event_bus {
            bus.emit_started(TransferStartedEvent {
                transfer_id: download_id.clone(),
                file_hash: download_id.clone(),
                file_name: file_name.clone(),
                file_size: total_bytes,
                total_chunks: 1,
                chunk_size: total_bytes as usize,
                started_at: current_timestamp_ms(),
                available_sources: vec![source_info.clone()],
                selected_sources: vec![source_id.clone()],
            });

            bus.emit_source_connected(SourceConnectedEvent {
                transfer_id: download_id.clone(),
                source_id: source_id.clone(),
                source_type: SourceType::Http,
                source_info,
                connected_at: current_timestamp_ms(),
                assigned_chunks: vec![0],
            });
        }

        // Start download
        let response = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                if let Some(ref bus) = event_bus {
                    bus.emit_failed(TransferFailedEvent {
                        transfer_id: download_id.clone(),
                        file_hash: download_id.clone(),
                        failed_at: current_timestamp_ms(),
                        error: format!("Download request failed: {}", e),
                        error_category: ErrorCategory::Network,
                        downloaded_bytes: 0,
                        total_bytes,
                        retry_possible: true,
                    });
                }
                return Err(ProtocolError::NetworkError(e.to_string()));
            }
        };

        if !response.status().is_success() {
            let error_msg = format!("HTTP {} for {}", response.status(), url);
            if let Some(ref bus) = event_bus {
                bus.emit_failed(TransferFailedEvent {
                    transfer_id: download_id.clone(),
                    file_hash: download_id.clone(),
                    failed_at: current_timestamp_ms(),
                    error: error_msg.clone(),
                    error_category: ErrorCategory::Network,
                    downloaded_bytes: 0,
                    total_bytes,
                    retry_possible: true,
                });
            }
            return Err(ProtocolError::NetworkError(error_msg));
        }

        // Create output file
        let mut file = match File::create(&output_path).await {
            Ok(f) => f,
            Err(e) => {
                if let Some(ref bus) = event_bus {
                    bus.emit_failed(TransferFailedEvent {
                        transfer_id: download_id.clone(),
                        file_hash: download_id.clone(),
                        failed_at: current_timestamp_ms(),
                        error: format!("Failed to create file: {}", e),
                        error_category: ErrorCategory::Filesystem,
                        downloaded_bytes: 0,
                        total_bytes,
                        retry_possible: false,
                    });
                }
                return Err(ProtocolError::Internal(e.to_string()));
            }
        };

        let mut downloaded_bytes: u64 = 0;
        let mut stream = response.bytes_stream();
        let mut last_progress_event: u64 = 0;

        use futures::StreamExt;

        loop {
            tokio::select! {
                // Check for cancellation
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        // Cancelled
                        let mut prog = progress.lock().await;
                        if let Some(p) = prog.get_mut(&download_id) {
                            p.status = DownloadStatus::Cancelled;
                        }
                        if let Some(ref bus) = event_bus {
                            bus.emit_canceled(TransferCanceledEvent {
                                transfer_id: download_id.clone(),
                                canceled_at: current_timestamp_ms(),
                                downloaded_bytes,
                                total_bytes,
                                keep_partial: false,
                            });
                        }
                        return Err(ProtocolError::Internal("Download cancelled".to_string()));
                    }
                }
                // Process next chunk
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            if let Err(e) = file.write_all(&bytes).await {
                                if let Some(ref bus) = event_bus {
                                    bus.emit_failed(TransferFailedEvent {
                                        transfer_id: download_id.clone(),
                                        file_hash: download_id.clone(),
                                        failed_at: current_timestamp_ms(),
                                        error: format!("Failed to write: {}", e),
                                        error_category: ErrorCategory::Filesystem,
                                        downloaded_bytes,
                                        total_bytes,
                                        retry_possible: false,
                                    });
                                }
                                return Err(ProtocolError::Internal(e.to_string()));
                            }

                            downloaded_bytes += bytes.len() as u64;

                            // Update progress
                            let elapsed = start_time.elapsed().as_secs_f64();
                            let speed = if elapsed > 0.0 {
                                downloaded_bytes as f64 / elapsed
                            } else {
                                0.0
                            };

                            let eta = if speed > 0.0 && total_bytes > downloaded_bytes {
                                Some(((total_bytes - downloaded_bytes) as f64 / speed) as u64)
                            } else {
                                None
                            };

                            let now_ms = current_timestamp_ms();

                            let mut prog = progress.lock().await;
                            if let Some(p) = prog.get_mut(&download_id) {
                                p.downloaded_bytes = downloaded_bytes;
                                p.download_speed = speed;
                                p.eta_seconds = eta;
                            }
                            drop(prog);

                            // Emit progress event (throttled to every 2 seconds)
                            if let Some(ref bus) = event_bus {
                                if now_ms - last_progress_event >= 2000 {
                                    last_progress_event = now_ms;
                                    let progress_pct = if total_bytes > 0 {
                                        (downloaded_bytes as f64 / total_bytes as f64) * 100.0
                                    } else {
                                        0.0
                                    };

                                    bus.emit_progress(TransferProgressEvent {
                                        transfer_id: download_id.clone(),
                                        downloaded_bytes,
                                        total_bytes,
                                        completed_chunks: 0,
                                        total_chunks: 1,
                                        progress_percentage: progress_pct,
                                        download_speed_bps: speed,
                                        upload_speed_bps: 0.0,
                                        eta_seconds: eta.map(|e| e as u32),
                                        active_sources: 1,
                                        timestamp: now_ms,
                                    });
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let mut prog = progress.lock().await;
                            if let Some(p) = prog.get_mut(&download_id) {
                                p.status = DownloadStatus::Failed;
                            }
                            if let Some(ref bus) = event_bus {
                                bus.emit_failed(TransferFailedEvent {
                                    transfer_id: download_id.clone(),
                                    file_hash: download_id.clone(),
                                    failed_at: current_timestamp_ms(),
                                    error: format!("Network error: {}", e),
                                    error_category: ErrorCategory::Network,
                                    downloaded_bytes,
                                    total_bytes,
                                    retry_possible: true,
                                });
                            }
                            return Err(ProtocolError::NetworkError(e.to_string()));
                        }
                        None => {
                            // Download complete
                            break;
                        }
                    }
                }
            }
        }

        file.flush()
            .await
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        // Mark as completed
        let duration_secs = start_time.elapsed().as_secs();
        let avg_speed = if duration_secs > 0 {
            downloaded_bytes as f64 / duration_secs as f64
        } else {
            downloaded_bytes as f64
        };

        let mut prog = progress.lock().await;
        if let Some(p) = prog.get_mut(&download_id) {
            p.status = DownloadStatus::Completed;
            p.downloaded_bytes = downloaded_bytes;
        }
        drop(prog);

        // Emit completion events
        if let Some(ref bus) = event_bus {
            bus.emit_source_disconnected(SourceDisconnectedEvent {
                transfer_id: download_id.clone(),
                source_id: source_id.clone(),
                source_type: SourceType::Http,
                disconnected_at: current_timestamp_ms(),
                reason: DisconnectReason::Completed,
                chunks_completed: 1,
                will_retry: false,
            });

            bus.emit_completed(TransferCompletedEvent {
                transfer_id: download_id.clone(),
                file_hash: download_id.clone(),
                file_name,
                file_size: downloaded_bytes,
                output_path: output_path.to_string_lossy().to_string(),
                completed_at: current_timestamp_ms(),
                duration_seconds: duration_secs,
                average_speed_bps: avg_speed,
                total_chunks: 1,
                sources_used: vec![SourceSummary {
                    source_id,
                    source_type: SourceType::Http,
                    chunks_provided: 1,
                    bytes_provided: downloaded_bytes,
                    average_speed_bps: avg_speed,
                    connection_duration_seconds: duration_secs,
                }],
            });
        }

        info!("HTTP: Download completed: {} bytes in {} seconds", downloaded_bytes, duration_secs);
        Ok(())
    }
}

impl Default for HttpProtocolHandler {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP handler")
    }
}

#[async_trait]
impl ProtocolHandler for HttpProtocolHandler {
    fn name(&self) -> &'static str {
        "http"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("http://") || identifier.starts_with("https://")
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("HTTP: Starting download for {}", identifier);

        let download_id = Self::generate_id(identifier);
        let file_name = Self::extract_file_name(identifier);

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(&download_id) {
                return Err(ProtocolError::AlreadyExists(download_id));
            }
        }

        let started_at = Self::now();

        // Create cancellation channel
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        // Initialize progress
        {
            let mut prog = self.download_progress.lock().await;
            prog.insert(download_id.clone(), DownloadProgress {
                downloaded_bytes: 0,
                total_bytes: 0,
                download_speed: 0.0,
                eta_seconds: None,
                active_peers: 1, // HTTP has "1 peer" (the server)
                status: DownloadStatus::FetchingMetadata,
            });
        }

        // Track the download
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(download_id.clone(), HttpDownloadState {
                url: identifier.to_string(),
                output_path: options.output_path.clone(),
                started_at,
                status: DownloadStatus::Downloading,
                cancel_token: cancel_tx,
                file_name: file_name.clone(),
                total_bytes: 0,
            });
        }

        // Spawn download task
        let client = self.client.clone();
        let url = identifier.to_string();
        let output_path = options.output_path;
        let progress = self.download_progress.clone();
        let active_downloads = self.active_downloads.clone();
        let id = download_id.clone();
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::download_with_progress(
                client,
                url,
                output_path,
                progress,
                active_downloads,
                id,
                cancel_rx,
                event_bus,
                file_name,
            ).await {
                error!("HTTP download failed: {}", e);
            }
        });

        Ok(DownloadHandle {
            identifier: download_id,
            protocol: "http".to_string(),
            started_at,
        })
    }

    async fn seed(
        &self,
        _file_path: PathBuf,
        _options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        // HTTP doesn't support traditional seeding
        // Would need to run an HTTP server
        warn!("HTTP: Seeding not supported");
        Err(ProtocolError::NotSupported)
    }

    async fn stop_seeding(&self, _identifier: &str) -> Result<(), ProtocolError> {
        Err(ProtocolError::NotSupported)
    }

    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        // HTTP doesn't easily support pause without range requests
        // For now, we cancel and would need to resume with range request
        warn!("HTTP: pause_download - cancelling download (resume requires range request support)");
        self.cancel_download(identifier).await
    }

    async fn resume_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
        // Would need to track partial file and use Range header
        warn!("HTTP: resume_download not yet implemented");
        Err(ProtocolError::NotSupported)
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("HTTP: Cancelling download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.remove(identifier) {
            // Signal cancellation
            let _ = state.cancel_token.send(true);

            // Emit canceled event
            if let Some(ref bus) = self.event_bus {
                bus.emit_canceled(TransferCanceledEvent {
                    transfer_id: identifier.to_string(),
                    canceled_at: Self::now_ms(),
                    downloaded_bytes: 0, // Will be updated by download task
                    total_bytes: state.total_bytes,
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
        // HTTP doesn't support seeding
        Ok(Vec::new())
    }

    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities {
            supports_seeding: false,
            supports_pause_resume: false, // Could be true with range request implementation
            supports_multi_source: true,  // Can download same file from multiple URLs
            supports_encryption: true,    // HTTPS
            supports_dht: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_http() {
        let handler = HttpProtocolHandler::new().unwrap();
        assert!(handler.supports("http://example.com/file.zip"));
        assert!(handler.supports("https://example.com/file.zip"));
        assert!(!handler.supports("ftp://example.com/file.zip"));
        assert!(!handler.supports("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn test_generate_id() {
        let id1 = HttpProtocolHandler::generate_id("http://example.com/file.zip");
        let id2 = HttpProtocolHandler::generate_id("http://example.com/file.zip");
        let id3 = HttpProtocolHandler::generate_id("http://other.com/file.zip");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert!(id1.starts_with("http-"));
    }
}