// download_scheduler.rs
// Example integration of unified download source abstraction
// This module demonstrates how to use DownloadSource in scheduling and logging

use crate::download_source::{
    BitTorrentSourceInfo, DownloadSource, Ed2kSourceInfo, FtpSourceInfo, HttpSourceInfo,
    P2pSourceInfo,
};
use crate::file_transfer::FileTransferService;
use crate::ftp_client;
use crate::http_download::HttpDownloadClient;
use crate::protocols::ed2k::Ed2kProtocolHandler;
use crate::protocols::traits::{DownloadOptions, ProtocolHandler};
use crate::transfer_events::{
    calculate_eta, calculate_progress, current_timestamp_ms, SourceInfo, SourceType,
    TransferCompletedEvent, TransferEventBus, TransferFailedEvent, TransferProgressEvent,
    TransferStartedEvent, ErrorCategory,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

/// Represents a scheduled download task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    /// Unique task identifier
    pub task_id: String,

    /// File hash or identifier
    pub file_hash: String,

    /// File name
    pub file_name: String,

    /// Available download sources
    pub sources: Vec<DownloadSource>,

    /// Task status
    pub status: DownloadTaskStatus,

    /// Priority (higher is more important)
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadTaskStatus {
    Pending,
    Scheduled,
    Downloading,
    Paused,
    Completed,
    Failed,
}

/// Download scheduler that manages tasks with different source types
pub struct DownloadScheduler {
    tasks: HashMap<String, DownloadTask>,
    file_transfer_service: Option<Arc<FileTransferService>>,
    event_bus: Option<Arc<TransferEventBus>>,
}

impl DownloadScheduler {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            file_transfer_service: None,
            event_bus: None,
        }
    }

    pub fn with_file_transfer_service(file_transfer_service: Arc<FileTransferService>) -> Self {
        Self {
            tasks: HashMap::new(),
            file_transfer_service: Some(file_transfer_service),
            event_bus: None,
        }
    }

    pub fn with_event_bus(event_bus: Arc<TransferEventBus>) -> Self {
        Self {
            tasks: HashMap::new(),
            file_transfer_service: None,
            event_bus: Some(event_bus),
        }
    }

    pub fn with_file_transfer_service_and_event_bus(
        file_transfer_service: Arc<FileTransferService>,
        event_bus: Arc<TransferEventBus>
    ) -> Self {
        Self {
            tasks: HashMap::new(),
            file_transfer_service: Some(file_transfer_service),
            event_bus: Some(event_bus),
        }
    }

    /// Add a new download task with multiple sources
    pub fn add_task(&mut self, task: DownloadTask) {
        info!(
            task_id = %task.task_id,
            file_hash = %task.file_hash,
            sources_count = task.sources.len(),
            "Adding new download task"
        );

        // Log information about each source
        for (idx, source) in task.sources.iter().enumerate() {
            debug!(
                task_id = %task.task_id,
                source_idx = idx,
                source_type = source.source_type(),
                source_display = %source,
                supports_encryption = source.supports_encryption(),
                priority_score = source.priority_score(),
                "Source available for download"
            );
        }

        self.tasks.insert(task.task_id.clone(), task);
    }

    /// Select the best source for a download task
    pub fn select_best_source(&self, task_id: &str) -> Option<DownloadSource> {
        let task = self.tasks.get(task_id)?;

        if task.sources.is_empty() {
            warn!(task_id = %task_id, "No sources available for task");
            return None;
        }

        // Sort sources by priority score (highest first)
        let mut sources_with_scores: Vec<_> = task
            .sources
            .iter()
            .map(|s| (s.clone(), s.priority_score()))
            .collect();

        sources_with_scores.sort_by(|a, b| b.1.cmp(&a.1));

        let best_source = sources_with_scores[0].0.clone();

        info!(
            task_id = %task_id,
            source_type = best_source.source_type(),
            source = %best_source,
            priority_score = best_source.priority_score(),
            "Selected best source for download"
        );

        Some(best_source)
    }

    /// Handle source-specific download logic (placeholder)
    pub fn start_download(&self, task_id: &str, source: &DownloadSource) -> Result<(), String> {
        info!(
            task_id = %task_id,
            source_type = source.source_type(),
            "Starting download from source"
        );

        match source {
            DownloadSource::P2p(info) => {
                self.handle_p2p_download(task_id, info)
            }
            DownloadSource::Http(info) => {
                self.handle_http_download(task_id, info)
            }
            DownloadSource::Ftp(info) => {
                self.handle_ftp_download(task_id, info)
            }
            DownloadSource::Ed2k(info) => {
                self.handle_ed2k_download(task_id, info)
            }
            DownloadSource::BitTorrent(info) => self.handle_bittorrent_download(task_id, info),
        }
    }

    // Placeholder handlers for different source types
    fn handle_p2p_download(&self, task_id: &str, info: &P2pSourceInfo) -> Result<(), String> {
        info!(
            task_id = %task_id,
            peer_id = %info.peer_id,
            protocol = ?info.protocol,
            "Initiating P2P download"
        );

        // Check if FileTransferService is available
        let file_transfer_service = self.file_transfer_service.as_ref()
            .ok_or_else(|| "FileTransferService not available".to_string())?;

        // Get task to determine file hash
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Task not found: {}", task_id))?;

        // Construct output path (use file name from task)
        let file_name = &task.file_name;
        let output_path = format!("./downloads/{}", file_name);

        // Create downloads directory if it doesn't exist
        if let Some(parent) = std::path::Path::new(&output_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create download directory: {}", e))?;
        }

        // Spawn async task to initiate P2P download
        let peer_id = info.peer_id.clone();
        let file_hash = task.file_hash.clone();
        let output_path_clone = output_path.clone();
        let file_transfer_service_clone = Arc::clone(file_transfer_service);
        let task_id_clone = task_id.to_string();

        tokio::spawn(async move {
            match file_transfer_service_clone.initiate_p2p_download(
                file_hash.clone(),
                peer_id.clone(),
                output_path_clone.clone(),
            ).await {
                Ok(_) => {
                    info!(
                        task_id = %task_id_clone,
                        peer_id = %peer_id,
                        file_hash = %file_hash,
                        output = %output_path_clone,
                        "P2P download initiated successfully"
                    );
                }
                Err(e) => {
                    error!(
                        error = %e,
                        task_id = %task_id_clone,
                        peer_id = %peer_id,
                        file_hash = %file_hash,
                        "P2P download initiation failed"
                    );
                }
            }
        });

        Ok(())
    }

    fn handle_http_download(&self, task_id: &str, info: &HttpSourceInfo) -> Result<(), String> {
        info!(
            task_id = %task_id,
            url = %info.url,
            verify_ssl = info.verify_ssl,
            "Initiating HTTP download"
        );

        // Get task to determine output path
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Task not found: {}", task_id))?;

        // Construct output path (use file name from task)
        let file_name = &task.file_name;
        let output_path = PathBuf::from(format!("./downloads/{}", file_name));

        // Create downloads directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create download directory: {}", e))?;
        }

        // Spawn async task to download file
        let url = info.url.clone();
        let file_hash_clone = task.file_hash.clone();
        let output_path_clone = output_path.clone();

        tokio::spawn(async move {
            let client = HttpDownloadClient::new();

            // Create progress channel for monitoring (optional)
            let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<crate::http_download::HttpDownloadProgress>(10);

            // Spawn progress monitor
            let file_hash_for_progress = file_hash_clone.clone();
            let progress_handle = tokio::spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    debug!(
                        file_hash = %file_hash_for_progress,
                        downloaded = progress.bytes_downloaded,
                        total = progress.bytes_total,
                        status = ?progress.status,
                        "HTTP download progress"
                    );
                }
            });

            // Perform the HTTP download
            match client.download_file(
                &url,
                &file_hash_clone,
                &output_path_clone,
                Some(progress_tx),
            ).await {
                Ok(_) => {
                    info!(
                        file_hash = %file_hash_clone,
                        output = ?output_path_clone,
                        "HTTP download completed successfully"
                    );
                }
                Err(e) => {
                    error!(
                        error = %e,
                        url = %url,
                        file_hash = %file_hash_clone,
                        "HTTP download failed"
                    );
                }
            }

            // Wait for progress monitor to finish
            let _ = progress_handle.await;
        });

        Ok(())
    }

    fn handle_ftp_download(&self, task_id: &str, info: &FtpSourceInfo) -> Result<(), String> {
        info!(
            task_id = %task_id,
            url = %info.url,
            username = ?info.username,
            passive_mode = info.passive_mode,
            use_ftps = info.use_ftps,
            "Initiating FTP download"
        );

        // Get task to determine output path and file size
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Task not found: {}", task_id))?;

        // Construct output path (use file name from task or URL)
        let file_name = &task.file_name;
        let output_path = PathBuf::from(format!("./downloads/{}", file_name));

        // Create downloads directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create download directory: {}", e))?;
        }

        // Clone event bus for async task
        let event_bus = self.event_bus.as_ref()
            .ok_or_else(|| "TransferEventBus not available".to_string())?
            .clone();

        // Clone task data for async task
        let task_id_clone = task_id.to_string();
        let file_hash_clone = task.file_hash.clone();
        let file_name_clone = task.file_name.clone();
        let info_clone = info.clone();
        let output_path_clone = output_path.clone();

        tokio::spawn(async move {
            // Emit started event
            let started_event = TransferStartedEvent {
                transfer_id: task_id_clone.clone(),
                file_hash: file_hash_clone.clone(),
                protocol: "FTP".to_string(),
                file_name: file_name_clone.clone(),
                file_size: 0, // We'll get this from the download
                total_chunks: 1, // FTP downloads are treated as single chunk
                chunk_size: 0,
                started_at: current_timestamp_ms(),
                available_sources: vec![SourceInfo {
                    id: format!("ftp:{}", info_clone.url),
                    source_type: SourceType::Ftp,
                    address: info_clone.url.clone(),
                    reputation: None,
                    estimated_speed_bps: None,
                    latency_ms: None,
                    location: None,
                }],
                selected_sources: vec![format!("ftp:{}", info_clone.url)],
            };
            event_bus.emit_started(started_event);

            let start_time = Instant::now();
            let progress_state = Arc::new(std::sync::Mutex::new((start_time, 0u64))); // (last_progress_update, last_downloaded_bytes)
            let task_id_for_callback = task_id_clone.clone();
            let event_bus_for_callback = Arc::clone(&event_bus);

            // Create progress callback
            let progress_state_clone = Arc::clone(&progress_state);
            let progress_callback: ftp_client::ProgressCallback = Box::new(move |downloaded: u64, total: u64| {
                let now = Instant::now();

                let mut state = progress_state_clone.lock().unwrap();
                let (last_progress_update, last_downloaded_bytes) = *state;

                // Throttle progress updates to every 100ms to avoid overwhelming the UI
                if now.duration_since(last_progress_update).as_millis() >= 100 || downloaded == total {
                    let elapsed_secs = start_time.elapsed().as_secs_f64();
                    let speed = if elapsed_secs > 0.0 {
                        downloaded as f64 / elapsed_secs
                    } else {
                        0.0
                    };

                    let remaining = total.saturating_sub(downloaded);
                    let eta = calculate_eta(remaining, speed);

                    let progress_event = TransferProgressEvent {
                        transfer_id: task_id_for_callback.clone(),
                        protocol: "FTP".to_string(),
                        downloaded_bytes: downloaded,
                        total_bytes: total,
                        completed_chunks: if total > 0 && downloaded >= total { 1 } else { 0 },
                        total_chunks: 1,
                        progress_percentage: calculate_progress(downloaded, total),
                        download_speed_bps: speed,
                        upload_speed_bps: 0.0,
                        eta_seconds: eta,
                        active_sources: 1,
                        timestamp: current_timestamp_ms(),
                    };

                    event_bus_for_callback.emit_progress(progress_event);

                    *state = (now, downloaded);
                }
            });

            // Perform the download with progress tracking
            match ftp_client::download_from_ftp_with_progress(
                &info_clone,
                &output_path_clone,
                progress_callback
            ).await {
                Ok(bytes) => {
                    let duration_seconds = start_time.elapsed().as_secs_f64();
                    let average_speed = if duration_seconds > 0.0 {
                        bytes as f64 / duration_seconds
                    } else {
                        0.0
                    };

                    // Emit completed event
                    let completed_event = TransferCompletedEvent {
                        transfer_id: task_id_clone.clone(),
                        file_hash: file_hash_clone.clone(),
                        protocol: "FTP".to_string(),
                        file_name: file_name_clone.clone(),
                        file_size: bytes,
                        output_path: output_path_clone.to_string_lossy().to_string(),
                        completed_at: current_timestamp_ms(),
                        duration_seconds: duration_seconds as u64,
                        average_speed_bps: average_speed,
                        total_chunks: 1,
                        sources_used: vec![crate::transfer_events::SourceSummary {
                            source_id: format!("ftp:{}", info_clone.url),
                            source_type: SourceType::Ftp,
                            chunks_provided: 1,
                            bytes_provided: bytes,
                            average_speed_bps: average_speed,
                            connection_duration_seconds: duration_seconds as u64,
                        }],
                    };
                    event_bus.emit_completed(completed_event);

                    info!(
                        bytes = bytes,
                        output = ?output_path_clone,
                        "FTP download completed successfully"
                    );
                }
                Err(e) => {
                    // Get the last downloaded bytes from the progress state
                    let downloaded_bytes = progress_state.lock().unwrap().1;

                    // Emit failed event
                    let failed_event = TransferFailedEvent {
                        transfer_id: task_id_clone.clone(),
                        file_hash: file_hash_clone.clone(),
                        protocol: "FTP".to_string(),
                        failed_at: current_timestamp_ms(),
                        error: e.to_string(),
                        error_category: ErrorCategory::Network,
                        downloaded_bytes,
                        total_bytes: 0, // We don't know the total if download failed
                        retry_possible: true,
                    };
                    event_bus.emit_failed(failed_event);

                    error!(
                        error = %e,
                        url = %info_clone.url,
                        "FTP download failed"
                    );
                }
            }
        });

        Ok(())
    }

    fn handle_ed2k_download(&self, task_id: &str, info: &Ed2kSourceInfo) -> Result<(), String> {
        info!(
            task_id = %task_id,
            server_url = %info.server_url,
            file_hash = %info.file_hash,
            file_size = info.file_size,
            "Initiating Ed2k download"
        );

        // Get task to determine output path
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| format!("Task not found: {}", task_id))?;

        // Construct output path
        let file_name = &task.file_name;
        let output_path = PathBuf::from(format!("./downloads/{}", file_name));

        // Create downloads directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create download directory: {}", e))?;
        }

        // Build ed2k:// link from source info
        let ed2k_link = format!(
            "ed2k://|file|{}|{}|{}|/",
            file_name,
            info.file_size,
            info.file_hash.to_uppercase()
        );

        // Clone data for async task
        let server_url = info.server_url.clone();
        let file_hash_clone = task.file_hash.clone();

        // Spawn async task to download file using Ed2kProtocolHandler
        tokio::spawn(async move {
            // Create ED2K protocol handler with the server URL
            let handler = Ed2kProtocolHandler::new(server_url);

            // Configure download options
            let options = DownloadOptions {
                output_path: output_path.clone(),
                max_peers: Some(5),
                chunk_size: None,
                encryption: false,
                bandwidth_limit: None,
            };

            // Start the download
            match handler.download(&ed2k_link, options).await {
                Ok(handle) => {
                    info!(
                        file_hash = %file_hash_clone,
                        identifier = %handle.identifier,
                        "ED2K download started successfully"
                    );

                    // Monitor progress until completion
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                        match handler.get_download_progress(&handle.identifier).await {
                            Ok(progress) => {
                                debug!(
                                    file_hash = %file_hash_clone,
                                    downloaded = progress.downloaded_bytes,
                                    total = progress.total_bytes,
                                    speed = progress.download_speed,
                                    status = ?progress.status,
                                    "ED2K download progress"
                                );

                                // Check if download is complete or failed
                                match progress.status {
                                    crate::protocols::traits::DownloadStatus::Completed => {
                                        info!(
                                            file_hash = %file_hash_clone,
                                            output = ?output_path,
                                            "ED2K download completed successfully"
                                        );
                                        break;
                                    }
                                    crate::protocols::traits::DownloadStatus::Failed => {
                                        error!(
                                            file_hash = %file_hash_clone,
                                            "ED2K download failed"
                                        );
                                        break;
                                    }
                                    crate::protocols::traits::DownloadStatus::Cancelled => {
                                        info!(
                                            file_hash = %file_hash_clone,
                                            "ED2K download cancelled"
                                        );
                                        break;
                                    }
                                    _ => continue,
                                }
                            }
                            Err(e) => {
                                error!(
                                    error = %e,
                                    file_hash = %file_hash_clone,
                                    "Failed to get ED2K download progress"
                                );
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        error = %e,
                        file_hash = %file_hash_clone,
                        "ED2K download failed to start"
                    );
                }
            }
        });

        Ok(())
    }

    fn handle_bittorrent_download(
        &self,
        task_id: &str,
        info: &BitTorrentSourceInfo,
    ) -> Result<(), String> {
        info!(
            task_id = %task_id,
            magnet_uri = %info.magnet_uri,
            "Initiating BitTorrent download (placeholder)"
        );
        warn!("BitTorrent downloads are not yet fully implemented");
        Ok(())
    }

    /// Get statistics about source types in use
    pub fn get_source_statistics(&self) -> SourceStatistics {
        let mut stats = SourceStatistics::default();

        for task in self.tasks.values() {
            for source in &task.sources {
                match source {
                    DownloadSource::P2p(_) => stats.p2p_count += 1,
                    DownloadSource::Http(_) => stats.http_count += 1,
                    DownloadSource::Ftp(_) => stats.ftp_count += 1,
                    DownloadSource::Ed2k(_) => stats.ed2k_count += 1,
                    DownloadSource::BitTorrent(_) => stats.bittorrent_count += 1,
                }
            }
        }

        info!(
            p2p_sources = stats.p2p_count,
            http_sources = stats.http_count,
            ftp_sources = stats.ftp_count,
            "Current source statistics"
        );

        stats
    }

    /// Display all tasks with their sources
    pub fn display_tasks(&self) {
        info!(total_tasks = self.tasks.len(), "Current download tasks");

        for task in self.tasks.values() {
            info!(
                task_id = %task.task_id,
                file_name = %task.file_name,
                status = ?task.status,
                priority = task.priority,
                sources_count = task.sources.len(),
                "Task details"
            );

            for source in &task.sources {
                debug!(
                    task_id = %task.task_id,
                    source = %source,
                    "  └─ Source available"
                );
            }
        }
    }
}

impl Default for DownloadScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default, Serialize)]
pub struct SourceStatistics {
    pub p2p_count: usize,
    pub http_count: usize,
    pub ftp_count: usize,
    pub ed2k_count: usize,
    pub bittorrent_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_with_mixed_sources() {
        let mut scheduler = DownloadScheduler::new();

        let task = DownloadTask {
            task_id: "task1".to_string(),
            file_hash: "QmTest123".to_string(),
            file_name: "test_file.zip".to_string(),
            sources: vec![
                DownloadSource::P2p(P2pSourceInfo {
                    peer_id: "12D3KooWPeer1".to_string(),
                    multiaddr: None,
                    reputation: Some(90),
                    supports_encryption: true,
                    protocol: Some("webrtc".to_string()),
                }),
                DownloadSource::Http(HttpSourceInfo {
                    url: "https://cdn.example.com/file.zip".to_string(),
                    auth_header: None,
                    verify_ssl: true,
                    headers: None,
                    timeout_secs: Some(30),
                }),
                DownloadSource::Ftp(FtpSourceInfo {
                    url: "ftp://ftp.example.com/pub/file.zip".to_string(),
                    username: Some("anonymous".to_string()),
                    encrypted_password: None,
                    passive_mode: true,
                    use_ftps: false,
                    timeout_secs: Some(60),
                }),
            ],
            status: DownloadTaskStatus::Pending,
            priority: 100,
        };

        scheduler.add_task(task);

        // Should select P2P as best source (highest priority)
        let best_source = scheduler.select_best_source("task1").unwrap();
        assert_eq!(best_source.source_type(), "P2P");

        // Check statistics
        let stats = scheduler.get_source_statistics();
        assert_eq!(stats.p2p_count, 1);
        assert_eq!(stats.http_count, 1);
        assert_eq!(stats.ftp_count, 1);
    }

    #[test]
    fn test_ftp_source_recognition() {
        let ftp_source = DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://files.example.org/data.tar.gz".to_string(),
            username: Some("user".to_string()),
            encrypted_password: Some("encrypted_pass_base64".to_string()),
            passive_mode: true,
            use_ftps: true,
            timeout_secs: Some(120),
        });

        assert_eq!(ftp_source.source_type(), "FTP");
        assert_eq!(ftp_source.display_name(), "FTP: files.example.org");
        assert!(ftp_source.supports_encryption()); // FTPS enabled
        assert_eq!(ftp_source.priority_score(), 25);
    }
}
