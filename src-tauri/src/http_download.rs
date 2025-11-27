use crate::transfer_events::{
    calculate_eta, calculate_progress, current_timestamp_ms, ChunkCompletedEvent,
    SourceConnectedEvent, SourceDisconnectedEvent, SourceInfo, SourceType,
    TransferCompletedEvent, TransferEventBus, TransferFailedEvent, TransferPriority,
    TransferProgressEvent, TransferQueuedEvent, TransferStartedEvent, DisconnectReason,
    ErrorCategory, SourceSummary,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tauri::AppHandle;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Semaphore};

/// HTTP Download Client for fetching files via HTTP Range requests
///
/// Simplified Architecture (Range-based, no manifest):
/// 1. Fetch file metadata from seeder's HTTP endpoint
/// 2. Calculate byte ranges (e.g., 0-256KB, 256KB-512KB, etc.)
/// 3. Download chunks in parallel using Range headers
/// 4. Reassemble chunks into final file
///
/// This approach uses HTTP Range requests (RFC 7233) to download file chunks
/// dynamically, without requiring pre-chunking or manifest endpoints.
///
/// Example Range request:
/// GET /files/{hash}
/// Range: bytes=0-262143
///
/// Server responds with:
/// HTTP/1.1 206 Partial Content
/// Content-Range: bytes 0-262143/1048576
/// Content-Length: 262144

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpDownloadProgress {
    pub file_hash: String,
    pub chunks_total: usize,
    pub chunks_downloaded: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    FetchingMetadata,
    Downloading,
    Assembling,
    Completed,
    Failed,
}

/// File metadata from HTTP server (matches HttpFileMetadata in http_server.rs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpFileMetadata {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub encrypted: bool,
}

/// Represents a byte range to download
#[derive(Debug, Clone)]
struct ByteRange {
    start: u64,
    end: u64,
    index: usize,
}

/// Chunk size for Range requests (256 KB)
const CHUNK_SIZE: u64 = 256 * 1024;

/// Maximum concurrent chunk downloads per HTTP source
///
/// Set conservatively to 5 because:
/// - Multi-source downloads will have multiple HTTP sources running in parallel
/// - Total concurrency = MAX_CONCURRENT_CHUNKS × number_of_sources
/// - Example: 3 sources × 5 chunks each = 15 concurrent downloads total
/// - Prevents network/bandwidth saturation
/// - Leaves headroom for WebRTC/Bitswap downloads happening simultaneously
const MAX_CONCURRENT_CHUNKS: usize = 5;

/// Configuration for a download with event bus integration
#[derive(Clone)]
pub struct HttpDownloadConfig {
    /// Unique transfer identifier for event tracking
    pub transfer_id: String,
    /// File name for display purposes
    pub file_name: String,
    /// Transfer priority
    pub priority: TransferPriority,
}

#[derive(Clone)]
pub struct HttpDownloadClient {
    http_client: Client,
    /// Semaphore to limit concurrent chunk downloads
    download_semaphore: std::sync::Arc<Semaphore>,
    /// Downloader's peer ID to send to provider for metrics tracking
    downloader_peer_id: Option<String>,
    /// Optional app handle for event bus integration
    app_handle: Option<AppHandle>,
}

impl HttpDownloadClient {
    pub fn new() -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            download_semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
            downloader_peer_id: None,
            app_handle: None,
        }
    }

    /// Create with downloader peer ID for provider-side metrics tracking
    pub fn new_with_peer_id(downloader_peer_id: Option<String>) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            download_semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
            downloader_peer_id,
            app_handle: None,
        }
    }

    /// Create with app handle for event bus integration
    pub fn new_with_event_bus(app_handle: AppHandle) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            download_semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
            downloader_peer_id: None,
            app_handle: Some(app_handle),
        }
    }

    /// Create with both peer ID and app handle for full integration
    pub fn new_with_peer_id_and_event_bus(
        downloader_peer_id: Option<String>,
        app_handle: AppHandle,
    ) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            download_semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
            downloader_peer_id,
            app_handle: Some(app_handle),
        }
    }

    /// Create with custom timeout
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .expect("Failed to create HTTP client"),
            download_semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
            downloader_peer_id: None,
            app_handle: None,
        }
    }

    /// Get the event bus if app handle is available
    fn get_event_bus(&self) -> Option<TransferEventBus> {
        self.app_handle.as_ref().map(|h| TransferEventBus::new(h.clone()))
    }

    /// Download a file from an HTTP seeder using Range requests
    ///
    /// Arguments:
    /// - `seeder_url`: Base URL of the seeder (e.g., "http://192.168.1.5:8080")
    /// - `file_hash`: File identifier (merkle_root)
    /// - `output_path`: Where to save the final file
    /// - `progress_tx`: Optional channel for progress updates
    ///
    /// Returns: Ok(()) on success
    ///
    /// This method:
    /// 1. Fetches file metadata to get size
    /// 2. Calculates byte ranges (256KB chunks)
    /// 3. Downloads chunks in parallel using Range headers
    /// 4. Reassembles chunks into final file
    ///
    /// Note: Files are downloaded as-is (encrypted if they were encrypted).
    /// Decryption happens at a higher level when needed.
    pub async fn download_file(
        &self,
        seeder_url: &str,
        file_hash: &str,
        output_path: &Path,
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
    ) -> Result<(), String> {
        tracing::info!(
            "Starting HTTP Range-based download: {} from {}",
            file_hash,
            seeder_url
        );

        // Step 1: Fetch file metadata
        self.send_progress(
            &progress_tx,
            file_hash,
            0,
            0,
            0,
            0,
            DownloadStatus::FetchingMetadata,
        )
        .await;

        let metadata = self.fetch_metadata(seeder_url, file_hash).await?;

        tracing::info!(
            "Fetched metadata: {} ({} bytes, encrypted: {})",
            metadata.name,
            metadata.size,
            metadata.encrypted
        );

        // Step 2: Calculate byte ranges
        let ranges = self.calculate_ranges(metadata.size);
        let total_chunks = ranges.len();

        tracing::info!(
            "Calculated {} chunks of {} bytes each",
            total_chunks,
            CHUNK_SIZE
        );

        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            0,
            0,
            metadata.size,
            DownloadStatus::Downloading,
        )
        .await;

        // Step 3: Download all chunks using Range requests
        let chunks = self
            .download_chunks_with_ranges(
                seeder_url,
                file_hash,
                &ranges,
                progress_tx.clone(),
                metadata.size,
            )
            .await?;

        tracing::info!("Downloaded {} chunks", chunks.len());

        // Step 4: Assemble final file
        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            total_chunks,
            metadata.size,
            metadata.size,
            DownloadStatus::Assembling,
        )
        .await;

        self.assemble_file(&chunks, output_path).await?;

        // Final status
        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            total_chunks,
            metadata.size,
            metadata.size,
            DownloadStatus::Completed,
        )
        .await;

        tracing::info!(
            "Download completed: {} ({})",
            output_path.display(),
            metadata.name
        );

        Ok(())
    }

    /// Download a file from an HTTP seeder with full event bus integration
    ///
    /// This method emits typed transfer events via the TransferEventBus for
    /// comprehensive download lifecycle tracking in the UI.
    ///
    /// Arguments:
    /// - `seeder_url`: Base URL of the seeder (e.g., "http://192.168.1.5:8080")
    /// - `file_hash`: File identifier (merkle_root)
    /// - `output_path`: Where to save the final file
    /// - `config`: Download configuration with transfer ID and file name
    /// - `progress_tx`: Optional channel for legacy progress updates
    ///
    /// Events emitted:
    /// - `TransferStarted`: When download begins
    /// - `SourceConnected`: When HTTP server connection is established
    /// - `ChunkCompleted`: For each successfully downloaded chunk
    /// - `TransferProgress`: Periodic progress updates
    /// - `TransferCompleted`/`TransferFailed`: On completion or failure
    pub async fn download_file_with_events(
        &self,
        seeder_url: &str,
        file_hash: &str,
        output_path: &Path,
        config: HttpDownloadConfig,
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
    ) -> Result<(), String> {
        let event_bus = self.get_event_bus();
        let start_time = Instant::now();
        let source_id = format!("http-{}", seeder_url);

        tracing::info!(
            "Starting HTTP Range-based download with events: {} from {} (transfer_id: {})",
            file_hash,
            seeder_url,
            config.transfer_id
        );

        // Step 1: Fetch file metadata
        self.send_progress(
            &progress_tx,
            file_hash,
            0,
            0,
            0,
            0,
            DownloadStatus::FetchingMetadata,
        )
        .await;

        let metadata = match self.fetch_metadata(seeder_url, file_hash).await {
            Ok(m) => m,
            Err(e) => {
                // Emit failed event
                if let Some(ref bus) = event_bus {
                    bus.emit_failed(TransferFailedEvent {
                        transfer_id: config.transfer_id.clone(),
                        file_hash: file_hash.to_string(),
                        failed_at: current_timestamp_ms(),
                        error: e.clone(),
                        error_category: ErrorCategory::Network,
                        downloaded_bytes: 0,
                        total_bytes: 0,
                        retry_possible: true,
                    });
                }
                return Err(e);
            }
        };

        tracing::info!(
            "Fetched metadata: {} ({} bytes, encrypted: {})",
            metadata.name,
            metadata.size,
            metadata.encrypted
        );

        // Step 2: Calculate byte ranges
        let ranges = self.calculate_ranges(metadata.size);
        let total_chunks = ranges.len() as u32;

        tracing::info!(
            "Calculated {} chunks of {} bytes each",
            total_chunks,
            CHUNK_SIZE
        );

        // Create source info for events
        let source_info = SourceInfo {
            id: source_id.clone(),
            source_type: SourceType::Http,
            address: seeder_url.to_string(),
            reputation: None,
            estimated_speed_bps: None,
            latency_ms: None,
            location: None,
        };

        // Emit started event
        if let Some(ref bus) = event_bus {
            bus.emit_started(TransferStartedEvent {
                transfer_id: config.transfer_id.clone(),
                file_hash: file_hash.to_string(),
                file_name: config.file_name.clone(),
                file_size: metadata.size,
                total_chunks,
                chunk_size: CHUNK_SIZE as usize,
                started_at: current_timestamp_ms(),
                available_sources: vec![source_info.clone()],
                selected_sources: vec![source_id.clone()],
            });

            // Emit source connected event
            bus.emit_source_connected(SourceConnectedEvent {
                transfer_id: config.transfer_id.clone(),
                source_id: source_id.clone(),
                source_type: SourceType::Http,
                source_info: source_info.clone(),
                connected_at: current_timestamp_ms(),
                assigned_chunks: (0..total_chunks).collect(),
            });
        }

        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks as usize,
            0,
            0,
            metadata.size,
            DownloadStatus::Downloading,
        )
        .await;

        // Step 3: Download all chunks using Range requests with event tracking
        let chunks_result = self
            .download_chunks_with_events(
                seeder_url,
                file_hash,
                &ranges,
                progress_tx.clone(),
                metadata.size,
                &config,
                &event_bus,
                &source_id,
                start_time,
            )
            .await;

        let chunks = match chunks_result {
            Ok(c) => c,
            Err(e) => {
                // Emit failed event
                if let Some(ref bus) = event_bus {
                    bus.emit_failed(TransferFailedEvent {
                        transfer_id: config.transfer_id.clone(),
                        file_hash: file_hash.to_string(),
                        failed_at: current_timestamp_ms(),
                        error: e.clone(),
                        error_category: ErrorCategory::Network,
                        downloaded_bytes: 0,
                        total_bytes: metadata.size,
                        retry_possible: true,
                    });

                    bus.emit_source_disconnected(SourceDisconnectedEvent {
                        transfer_id: config.transfer_id.clone(),
                        source_id: source_id.clone(),
                        source_type: SourceType::Http,
                        disconnected_at: current_timestamp_ms(),
                        reason: DisconnectReason::NetworkError,
                        chunks_completed: 0,
                        will_retry: false,
                    });
                }
                return Err(e);
            }
        };

        tracing::info!("Downloaded {} chunks", chunks.len());

        // Step 4: Assemble final file
        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks as usize,
            total_chunks as usize,
            metadata.size,
            metadata.size,
            DownloadStatus::Assembling,
        )
        .await;

        if let Err(e) = self.assemble_file(&chunks, output_path).await {
            // Emit failed event for assembly failure
            if let Some(ref bus) = event_bus {
                bus.emit_failed(TransferFailedEvent {
                    transfer_id: config.transfer_id.clone(),
                    file_hash: file_hash.to_string(),
                    failed_at: current_timestamp_ms(),
                    error: e.clone(),
                    error_category: ErrorCategory::Filesystem,
                    downloaded_bytes: metadata.size,
                    total_bytes: metadata.size,
                    retry_possible: false,
                });
            }
            return Err(e);
        }

        // Final status
        let duration_secs = start_time.elapsed().as_secs();
        let average_speed = if duration_secs > 0 {
            metadata.size as f64 / duration_secs as f64
        } else {
            metadata.size as f64
        };

        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks as usize,
            total_chunks as usize,
            metadata.size,
            metadata.size,
            DownloadStatus::Completed,
        )
        .await;

        // Emit completed event
        if let Some(ref bus) = event_bus {
            bus.emit_source_disconnected(SourceDisconnectedEvent {
                transfer_id: config.transfer_id.clone(),
                source_id: source_id.clone(),
                source_type: SourceType::Http,
                disconnected_at: current_timestamp_ms(),
                reason: DisconnectReason::Completed,
                chunks_completed: total_chunks,
                will_retry: false,
            });

            bus.emit_completed(TransferCompletedEvent {
                transfer_id: config.transfer_id.clone(),
                file_hash: file_hash.to_string(),
                file_name: config.file_name.clone(),
                file_size: metadata.size,
                output_path: output_path.to_string_lossy().to_string(),
                completed_at: current_timestamp_ms(),
                duration_seconds: duration_secs,
                average_speed_bps: average_speed,
                total_chunks,
                sources_used: vec![SourceSummary {
                    source_id: source_id.clone(),
                    source_type: SourceType::Http,
                    chunks_provided: total_chunks,
                    bytes_provided: metadata.size,
                    average_speed_bps: average_speed,
                    connection_duration_seconds: duration_secs,
                }],
            });
        }

        tracing::info!(
            "Download completed: {} ({}) in {} seconds",
            output_path.display(),
            metadata.name,
            duration_secs
        );

        Ok(())
    }

    /// Download chunks with event bus integration for per-chunk tracking
    async fn download_chunks_with_events(
        &self,
        seeder_url: &str,
        file_hash: &str,
        ranges: &[ByteRange],
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
        file_size: u64,
        config: &HttpDownloadConfig,
        event_bus: &Option<TransferEventBus>,
        source_id: &str,
        start_time: Instant,
    ) -> Result<Vec<Vec<u8>>, String> {
        let mut tasks = Vec::new();
        let completed_bytes = std::sync::Arc::new(AtomicU64::new(0));
        let completed_chunks = std::sync::Arc::new(AtomicU64::new(0));

        for range in ranges {
            let client = self.http_client.clone();
            let url = format!("{}/files/{}", seeder_url, file_hash);
            let start = range.start;
            let end = range.end;
            let index = range.index;
            let progress_tx = progress_tx.clone();
            let file_hash = file_hash.to_string();
            let total_chunks = ranges.len();
            let semaphore = self.download_semaphore.clone();
            let downloader_peer_id = self.downloader_peer_id.clone();
            let event_bus = (*event_bus).clone();
            let config = config.clone();
            let source_id = source_id.to_string();
            let completed_bytes = completed_bytes.clone();
            let completed_chunks = completed_chunks.clone();
            let start_time = start_time;

            let task = tokio::spawn(async move {
                let chunk_start_time = Instant::now();

                // Acquire permit (waits if MAX_CONCURRENT_CHUNKS already downloading)
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| format!("Failed to acquire semaphore: {}", e))?;

                tracing::debug!(
                    "Downloading chunk {} (bytes {}-{}) from {}",
                    index,
                    start,
                    end,
                    url
                );

                // Make request with Range header and optional peer ID
                let mut request = client
                    .get(&url)
                    .header("Range", format!("bytes={}-{}", start, end));

                if let Some(ref peer_id) = downloader_peer_id {
                    request = request.header("X-Downloader-Peer-ID", peer_id);
                }

                let response = request.send().await.map_err(|e| format!("Failed to download chunk {}: {}", index, e))?;

                // Verify 206 Partial Content response
                if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    return Err(format!(
                        "Chunk {} request failed: expected 206 Partial Content, got {}",
                        index,
                        response.status()
                    ));
                }

                // Read chunk data
                let data = response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read chunk {} data: {}", index, e))?
                    .to_vec();

                let expected_size = (end - start + 1) as usize;
                if data.len() != expected_size {
                    return Err(format!(
                        "Chunk {} size mismatch: expected {}, got {}",
                        index,
                        expected_size,
                        data.len()
                    ));
                }

                let chunk_duration_ms = chunk_start_time.elapsed().as_millis() as u64;
                let chunk_size = data.len() as u64;

                // Update atomic counters
                let new_bytes = completed_bytes.fetch_add(chunk_size, Ordering::SeqCst) + chunk_size;
                let new_chunks = completed_chunks.fetch_add(1, Ordering::SeqCst) + 1;

                tracing::debug!("Downloaded chunk {} ({} bytes in {} ms)", index, data.len(), chunk_duration_ms);

                // Emit chunk completed event
                if let Some(ref bus) = event_bus {
                    bus.emit_chunk_completed(ChunkCompletedEvent {
                        transfer_id: config.transfer_id.clone(),
                        chunk_id: index as u32,
                        chunk_size: data.len(),
                        source_id: source_id.clone(),
                        source_type: SourceType::Http,
                        completed_at: current_timestamp_ms(),
                        download_duration_ms: chunk_duration_ms,
                        verified: true,
                    });

                    // Emit progress event (every few chunks to avoid flooding)
                    if new_chunks % 5 == 0 || new_chunks as usize == total_chunks {
                        let elapsed_secs = start_time.elapsed().as_secs_f64();
                        let speed = if elapsed_secs > 0.0 {
                            new_bytes as f64 / elapsed_secs
                        } else {
                            0.0
                        };
                        let remaining = file_size.saturating_sub(new_bytes);
                        let eta = calculate_eta(remaining, speed);

                        bus.emit_progress(TransferProgressEvent {
                            transfer_id: config.transfer_id.clone(),
                            downloaded_bytes: new_bytes,
                            total_bytes: file_size,
                            completed_chunks: new_chunks as u32,
                            total_chunks: total_chunks as u32,
                            progress_percentage: calculate_progress(new_bytes, file_size),
                            download_speed_bps: speed,
                            upload_speed_bps: 0.0,
                            eta_seconds: eta,
                            active_sources: 1,
                            timestamp: current_timestamp_ms(),
                        });
                    }
                }

                // Send legacy progress update
                if let Some(tx) = progress_tx {
                    let _ = tx
                        .send(HttpDownloadProgress {
                            file_hash: file_hash.clone(),
                            chunks_total: total_chunks,
                            chunks_downloaded: new_chunks as usize,
                            bytes_downloaded: new_bytes,
                            bytes_total: file_size,
                            status: DownloadStatus::Downloading,
                        })
                        .await;
                }

                Ok::<(usize, Vec<u8>), String>((index, data))
            });

            tasks.push(task);
        }

        tracing::info!(
            "Downloading {} chunks with max {} concurrent downloads",
            ranges.len(),
            MAX_CONCURRENT_CHUNKS
        );

        // Wait for all chunks to download
        let mut results = Vec::new();
        for task in tasks {
            let result = task
                .await
                .map_err(|e| format!("Download task failed: {}", e))??;
            results.push(result);
        }

        // Sort by chunk index
        results.sort_by_key(|(index, _)| *index);

        // Extract just the data (drop indices)
        let chunks: Vec<Vec<u8>> = results.into_iter().map(|(_, data)| data).collect();

        Ok(chunks)
    }

    /// Fetch file metadata from seeder
    ///
    /// Calls: GET /files/{file_hash}/metadata
    async fn fetch_metadata(
        &self,
        seeder_url: &str,
        file_hash: &str,
    ) -> Result<HttpFileMetadata, String> {
        let url = format!("{}/files/{}/metadata", seeder_url, file_hash);

        tracing::info!("Fetching metadata from: {}", url);

        // Build request with optional peer ID header
        let mut request = self.http_client.get(&url);
        
        if let Some(ref peer_id) = self.downloader_peer_id {
            request = request.header("X-Downloader-Peer-ID", peer_id);
            tracing::debug!("📤 Adding downloader peer ID header: {}", peer_id);
        }

        let response = request.send().await.map_err(|e| {
            let err_msg = format!("Failed to fetch metadata from {}: {}", url, e);
            tracing::error!("{}", err_msg);
            err_msg
        })?;

        if !response.status().is_success() {
            let err_msg = format!(
                "Metadata request failed: {} ({})",
                response.status(),
                url
            );
            tracing::error!("{}", err_msg);
            return Err(err_msg);
        }

        let metadata: HttpFileMetadata = response.json().await.map_err(|e| {
            let err_msg = format!("Failed to parse metadata from {}: {}", url, e);
            tracing::error!("{}", err_msg);
            err_msg
        })?;

        tracing::info!(
            "Successfully fetched metadata: {} (size: {}, encrypted: {})",
            metadata.name,
            metadata.size,
            metadata.encrypted
        );

        Ok(metadata)
    }

    /// Calculate byte ranges for chunked download
    ///
    /// Splits file into CHUNK_SIZE (256KB) ranges
    /// Example for 1MB file:
    /// - Range 0: bytes 0-262143 (256KB)
    /// - Range 1: bytes 262144-524287 (256KB)
    /// - Range 2: bytes 524288-786431 (256KB)
    /// - Range 3: bytes 786432-1048575 (262KB, last chunk may be smaller)
    fn calculate_ranges(&self, file_size: u64) -> Vec<ByteRange> {
        let mut ranges = Vec::new();
        let mut offset = 0u64;
        let mut index = 0;

        while offset < file_size {
            let end = std::cmp::min(offset + CHUNK_SIZE - 1, file_size - 1);
            ranges.push(ByteRange {
                start: offset,
                end,
                index,
            });
            offset = end + 1;
            index += 1;
        }

        ranges
    }

    /// Download all chunks using Range requests with bounded parallelism
    ///
    /// Uses a semaphore to limit concurrent downloads to MAX_CONCURRENT_CHUNKS,
    /// preventing network/server overload while still achieving good parallelism.
    ///
    /// Flow:
    /// 1. Acquire semaphore permit (blocks if MAX_CONCURRENT_CHUNKS already downloading)
    /// 2. Send GET request with Range header
    /// 3. Verify 206 Partial Content response
    /// 4. Download chunk data
    /// 5. Release permit (allows next chunk to start)
    /// 6. Repeat until all chunks downloaded
    async fn download_chunks_with_ranges(
        &self,
        seeder_url: &str,
        file_hash: &str,
        ranges: &[ByteRange],
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
        file_size: u64,
    ) -> Result<Vec<Vec<u8>>, String> {
        let mut tasks = Vec::new();

        for range in ranges {
            let client = self.http_client.clone();
            let url = format!("{}/files/{}", seeder_url, file_hash);
            let start = range.start;
            let end = range.end;
            let index = range.index;
            let progress_tx = progress_tx.clone();
            let file_hash = file_hash.to_string();
            let total_chunks = ranges.len();
            let semaphore = self.download_semaphore.clone();
            let downloader_peer_id = self.downloader_peer_id.clone();

            // Spawn task for each chunk (but semaphore limits concurrency)
            let task = tokio::spawn(async move {
                // Acquire permit (waits if MAX_CONCURRENT_CHUNKS already downloading)
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| format!("Failed to acquire semaphore: {}", e))?;

                tracing::debug!(
                    "Downloading chunk {} (bytes {}-{}) from {}",
                    index,
                    start,
                    end,
                    url
                );

                // Make request with Range header and optional peer ID
                let mut request = client
                    .get(&url)
                    .header("Range", format!("bytes={}-{}", start, end));
                
                if let Some(ref peer_id) = downloader_peer_id {
                    request = request.header("X-Downloader-Peer-ID", peer_id);
                }
                
                let response = request.send().await.map_err(|e| format!("Failed to download chunk {}: {}", index, e))?;

                // Verify 206 Partial Content response
                if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    return Err(format!(
                        "Chunk {} request failed: expected 206 Partial Content, got {}",
                        index,
                        response.status()
                    ));
                }

                // Read chunk data
                let data = response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read chunk {} data: {}", index, e))?
                    .to_vec();

                let expected_size = (end - start + 1) as usize;
                if data.len() != expected_size {
                    return Err(format!(
                        "Chunk {} size mismatch: expected {}, got {}",
                        index,
                        expected_size,
                        data.len()
                    ));
                }

                tracing::debug!("Downloaded chunk {} ({} bytes)", index, data.len());

                // Send progress update
                if let Some(tx) = progress_tx {
                    let _ = tx
                        .send(HttpDownloadProgress {
                            file_hash: file_hash.clone(),
                            chunks_total: total_chunks,
                            chunks_downloaded: index + 1,
                            bytes_downloaded: data.len() as u64,
                            bytes_total: file_size,
                            status: DownloadStatus::Downloading,
                        })
                        .await;
                }

                Ok::<(usize, Vec<u8>), String>((index, data))
                // Permit automatically released when _permit is dropped
            });

            tasks.push(task);
        }

        tracing::info!(
            "Downloading {} chunks with max {} concurrent downloads",
            ranges.len(),
            MAX_CONCURRENT_CHUNKS
        );

        // Wait for all chunks to download
        let mut results = Vec::new();
        for task in tasks {
            let result = task
                .await
                .map_err(|e| format!("Download task failed: {}", e))??;
            results.push(result);
        }

        // Sort by chunk index
        results.sort_by_key(|(index, _)| *index);

        // Extract just the data (drop indices)
        let chunks: Vec<Vec<u8>> = results.into_iter().map(|(_, data)| data).collect();

        Ok(chunks)
    }

    /// Assemble chunks into final file
    ///
    /// Writes chunks sequentially to the output file
    async fn assemble_file(
        &self,
        chunks: &[Vec<u8>],
        output_path: &Path,
    ) -> Result<(), String> {
        // Ensure parent directory exists
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create parent directory: {}", e))?;
        }

        tracing::info!("Creating output file: {}", output_path.display());

        let mut file = File::create(output_path)
            .await
            .map_err(|e| format!("Failed to create output file at {}: {}", output_path.display(), e))?;

        tracing::info!("Writing {} chunks to file...", chunks.len());

        for (index, chunk) in chunks.iter().enumerate() {
            file.write_all(chunk)
                .await
                .map_err(|e| format!("Failed to write chunk {} to {}: {}", index, output_path.display(), e))?;
        }

        file.flush()
            .await
            .map_err(|e| format!("Failed to flush file {}: {}", output_path.display(), e))?;

        tracing::info!("Successfully assembled file: {} ({} chunks, {} bytes)", 
            output_path.display(), 
            chunks.len(),
            chunks.iter().map(|c| c.len()).sum::<usize>()
        );

        Ok(())
    }

    /// Helper to send progress updates
    async fn send_progress(
        &self,
        progress_tx: &Option<mpsc::Sender<HttpDownloadProgress>>,
        file_hash: &str,
        chunks_total: usize,
        chunks_downloaded: usize,
        bytes_downloaded: u64,
        bytes_total: u64,
        status: DownloadStatus,
    ) {
        if let Some(tx) = progress_tx {
            let _ = tx
                .send(HttpDownloadProgress {
                    file_hash: file_hash.to_string(),
                    chunks_total,
                    chunks_downloaded,
                    bytes_downloaded,
                    bytes_total,
                    status,
                })
                .await;
        }
    }


    /// Resume a download from a specific byte offset using Range requests
    ///
    /// This method downloads the remaining part of a file starting from `bytes_already_downloaded`
    /// and appends to an existing file.
    pub async fn resume_download_from_offset(
        &self,
        seeder_url: &str,
        file_hash: &str,
        output_path: &Path,
        bytes_already_downloaded: u64,
        total_size: u64,
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
    ) -> Result<(), String> {
        tracing::info!(
            "Resuming HTTP download: {} from {}, offset: {}/{}",
            file_hash,
            seeder_url,
            bytes_already_downloaded,
            total_size
        );

        // Calculate remaining bytes
        let remaining_bytes = total_size.saturating_sub(bytes_already_downloaded);
        if remaining_bytes == 0 {
            // Already complete
            if let Some(tx) = progress_tx {
                let _ = tx.send(HttpDownloadProgress {
                    file_hash: file_hash.to_string(),
                    chunks_total: (total_size / CHUNK_SIZE) as usize,
                    chunks_downloaded: (total_size / CHUNK_SIZE) as usize,
                    bytes_downloaded: total_size,
                    bytes_total: total_size,
                    status: DownloadStatus::Completed,
                }).await;
            }
            return Ok(());
        }

        // Open existing file for appending
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(output_path)
            .await
            .map_err(|e| format!("Failed to open file for resume: {}", e))?;

        // Calculate ranges starting from the resume offset
        let ranges = self.calculate_ranges_from_offset(bytes_already_downloaded, total_size);
        let total_chunks = ranges.len();

        // Send initial progress
        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            0, // chunks downloaded so far
            bytes_already_downloaded,
            total_size,
            DownloadStatus::Downloading,
        ).await;

        // Download remaining chunks
        let chunks = self
            .download_chunks_with_ranges(
                seeder_url,
                file_hash,
                &ranges,
                progress_tx.clone(),
                total_size,
            )
            .await?;

        // Write chunks to file in order
        for chunk in chunks {
            file.write_all(&chunk).await
                .map_err(|e| format!("Failed to write chunk to file: {}", e))?;
        }

        // Send final progress
        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            total_chunks,
            total_size,
            total_size,
            DownloadStatus::Completed,
        ).await;

        tracing::info!("Successfully resumed download: {}", file_hash);
        Ok(())
    }

    /// Calculate byte ranges starting from a specific offset
    fn calculate_ranges_from_offset(&self, start_offset: u64, file_size: u64) -> Vec<ByteRange> {
        let mut ranges = Vec::new();
        let mut offset = start_offset;
        let mut index = (start_offset / CHUNK_SIZE) as usize; // Continue chunk numbering

        while offset < file_size {
            let end = std::cmp::min(offset + CHUNK_SIZE - 1, file_size - 1);
            ranges.push(ByteRange {
                start: offset,
                end,
                index,
            });
            offset = end + 1;
            index += 1;
        }

        ranges
    }

}

#[cfg(test)]
mod tests {
    // Integration tests would go here
    // For now, these require a running HTTP server
}
