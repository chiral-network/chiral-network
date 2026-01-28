use crate::analytics::AnalyticsService;
use crate::bittorrent_handler::BitTorrentHandler;
use crate::chunk_verification::{normalize_sha256_hex, verify_chunk_hash};
use crate::dht::{models::FileMetadata, DhtService, WebRTCOfferRequest};
use crate::download_source::{
    BitTorrentSourceInfo, DownloadSource, Ed2kSourceInfo as DownloadEd2kSourceInfo,
    FtpSourceInfo as DownloadFtpSourceInfo,
};
use crate::ed2k_client::{Ed2kClient, Ed2kConfig, ED2K_CHUNK_SIZE};
use crate::ftp_downloader::{FtpCredentials, FtpDownloader};
use crate::manager::{ChunkManager, FileManifest};
use crate::transfer_events::{
    calculate_progress, current_timestamp_ms, ChunkCompletedEvent, ChunkFailedEvent,
    DisconnectReason, ErrorCategory, SourceConnectedEvent, SourceDisconnectedEvent, SourceInfo,
    SourceSummary, SourceType, TransferCompletedEvent, TransferEventBus, TransferFailedEvent,
    TransferProgressEvent, TransferStartedEvent,
};
use crate::webrtc_service::{WebRTCFileRequest, WebRTCService};
use md4::Md4;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use suppaftp::FtpStream;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use url::Url;

const DEFAULT_CHUNK_SIZE: usize = 256 * 1024; // 256KB chunks
const MAX_CHUNKS_PER_PEER: usize = 10; // Maximum chunks to assign to a single peer
const MIN_CHUNKS_FOR_PARALLEL: usize = 4; // Minimum chunks to enable parallel download
const CONNECTION_TIMEOUT_SECS: u64 = 30;
#[allow(dead_code)]
const CHUNK_REQUEST_TIMEOUT_SECS: u64 = 60;
#[allow(dead_code)]
const MAX_RETRY_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct ChunkInfo {
    pub chunk_id: u32,
    pub offset: u64,
    pub size: usize,
    pub hash: String,
}

/// Assignment of chunks to a download source (P2P peer, HTTP, or FTP)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAssignment {
    /// Download source (P2P, HTTP, or FTP)
    pub source: DownloadSource,

    /// Chunk IDs assigned to this source
    pub chunks: Vec<u32>,

    /// Current status of this source
    pub status: SourceStatus,

    /// Timestamp when connection was established
    pub connected_at: Option<u64>,

    /// Timestamp of last activity from this source
    pub last_activity: Option<u64>,
}

/// Status of a download source
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SourceStatus {
    Connecting,
    Connected,
    Downloading,
    Failed,
    Completed,
}

/// Persisted download state for resuming across app restarts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    pub file_hash: String,
    pub file_metadata: crate::dht::models::FileMetadata,
    pub chunks: Vec<ChunkInfo>,
    pub source_assignments: Vec<SourceAssignment>,
    pub completed_chunk_ids: Vec<u32>,
    pub failed_chunks: Vec<u32>,
    pub start_time_unix: u64, // Unix timestamp instead of Instant
    pub output_path: String,
    pub ed2k_chunk_hashes: Option<Vec<String>>,
    pub saved_at: u64,
}

impl SourceAssignment {
    /// Create a new SourceAssignment from a DownloadSource
    pub fn new(source: DownloadSource, chunks: Vec<u32>) -> Self {
        Self {
            source,
            chunks,
            status: SourceStatus::Connecting,
            connected_at: None,
            last_activity: None,
        }
    }

    /// Get the source identifier (peer ID for P2P, URL for HTTP/FTP)
    pub fn source_id(&self) -> String {
        self.source.identifier()
    }
}

// Legacy type alias for backwards compatibility
#[deprecated(note = "Use SourceAssignment instead")]
pub type PeerAssignment = SourceAssignment;

#[deprecated(note = "Use SourceStatus instead")]
pub type PeerStatus = SourceStatus;

/// Normalize and validate SHA-256 hash format (64 hex characters, lowercase)
#[deprecated(note = "Use chunk_verification::normalize_sha256_hex instead")]
pub fn normalized_sha256_hex(hash: &str) -> Option<String> {
    normalize_sha256_hex(hash)
}

/// Verify chunk integrity by comparing SHA-256 hash of data with expected hash from ChunkInfo
/// Returns Ok(()) if hash matches, Err((expected, actual)) if mismatch
pub fn verify_chunk_integrity(chunk: &ChunkInfo, data: &[u8]) -> Result<(), (String, String)> {
    verify_chunk_hash(&chunk.hash, data).map_err(|e| (e.expected, e.actual))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiSourceProgress {
    pub file_hash: String,
    pub file_name: String,
    pub total_size: u64,
    pub downloaded_size: u64,
    pub total_chunks: u32,
    pub completed_chunks: u32,
    pub active_sources: usize,
    pub download_speed_bps: f64,
    pub eta_seconds: Option<u32>,
    pub source_assignments: Vec<SourceAssignment>,
}

#[derive(Debug, Clone)]
pub struct ChunkRequest {
    #[allow(dead_code)]
    pub chunk_id: u32,
    #[allow(dead_code)]
    pub source_id: String, // Changed from peer_id - can be peer ID, URL, etc.
    #[allow(dead_code)]
    pub requested_at: Instant,
    #[allow(dead_code)]
    pub retry_count: u32,
}

#[derive(Debug, Clone)]
pub struct CompletedChunk {
    #[allow(dead_code)]
    pub chunk_id: u32,
    pub data: Vec<u8>,
    #[allow(dead_code)]
    pub source_id: String, // Changed from peer_id - can be peer ID, URL, etc.
    #[allow(dead_code)]
    pub completed_at: Instant,
}

#[derive(Debug)]
pub struct ActiveDownload {
    pub file_metadata: FileMetadata,
    pub chunks: Vec<ChunkInfo>,
    pub source_assignments: HashMap<String, SourceAssignment>, // Changed from source_assignments
    pub completed_chunks: HashMap<u32, CompletedChunk>,
    pub pending_requests: HashMap<u32, ChunkRequest>,
    pub failed_chunks: VecDeque<u32>,
    pub start_time: Instant,
    pub last_progress_update: Instant,
    pub output_path: String,
    /// ED2K chunk hashes (MD4 hashes for each 9.28MB chunk)
    pub ed2k_chunk_hashes: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct MultiSourceDownloadService {
    dht_service: Arc<DhtService>,
    webrtc_service: Arc<WebRTCService>,
    ftp_downloader: Arc<FtpDownloader>,
    bittorrent_handler: Arc<BitTorrentHandler>,
    proxy_latency_service: Option<Arc<Mutex<crate::proxy_latency::ProxyLatencyService>>>,
    active_downloads: Arc<RwLock<HashMap<String, ActiveDownload>>>,
    event_tx: mpsc::UnboundedSender<MultiSourceEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<MultiSourceEvent>>>,
    command_tx: mpsc::UnboundedSender<MultiSourceCommand>,
    command_rx: Arc<Mutex<mpsc::UnboundedReceiver<MultiSourceCommand>>>,
    // FTP connection pool: maps server URL to list of connections for concurrent downloads
    ftp_connections: Arc<Mutex<HashMap<String, Vec<FtpStream>>>>,
    // Ed2k connection pool - maps server URL to Ed2k client for reuse
    ed2k_connections: Arc<Mutex<HashMap<String, Ed2kClient>>>,
    // Transfer event bus for unified event emission to frontend
    transfer_event_bus: Arc<TransferEventBus>,
    // Analytics service for backend metrics tracking
    analytics_service: Arc<AnalyticsService>,
    // Unified chunk storage manager for persistence and caching
    chunk_manager: Arc<ChunkManager>,
}

#[derive(Debug, Serialize)]
pub enum MultiSourceCommand {
    StartDownload {
        file_hash: String,
        output_path: String,
        max_peers: Option<usize>,
        chunk_size: Option<usize>,
    },
    CancelDownload {
        file_hash: String,
    },
    RetryFailedChunks {
        file_hash: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub enum MultiSourceEvent {
    DownloadStarted {
        file_hash: String,
        total_peers: usize,
    },
    PeerConnected {
        file_hash: String,
        peer_id: String,
    },
    PeerFailed {
        file_hash: String,
        peer_id: String,
        error: String,
    },
    ChunkCompleted {
        file_hash: String,
        chunk_id: u32,
        peer_id: String,
    },
    ChunkFailed {
        file_hash: String,
        chunk_id: u32,
        peer_id: String,
        error: String,
    },
    ProgressUpdate {
        file_hash: String,
        progress: MultiSourceProgress,
    },
    DownloadCompleted {
        file_hash: String,
        output_path: String,
        duration_secs: u64,
        average_speed_bps: f64,
    },
    DownloadFailed {
        file_hash: String,
        error: String,
    },
}

impl MultiSourceDownloadService {
    pub fn new(
        dht_service: Arc<DhtService>,
        webrtc_service: Arc<WebRTCService>,
        bittorrent_handler: Arc<BitTorrentHandler>,
        transfer_event_bus: Arc<TransferEventBus>,
        analytics_service: Arc<AnalyticsService>,
        chunk_manager: Arc<ChunkManager>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        Self {
            dht_service,
            webrtc_service,
            ftp_downloader: Arc::new(FtpDownloader::new()),
            bittorrent_handler,
            proxy_latency_service: Some(Arc::new(Mutex::new(
                crate::proxy_latency::ProxyLatencyService::new(),
            ))),
            active_downloads: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            command_tx,
            command_rx: Arc::new(Mutex::new(command_rx)),
            ftp_connections: Arc::new(Mutex::new(HashMap::new())),
            ed2k_connections: Arc::new(Mutex::new(HashMap::new())),
            transfer_event_bus,
            analytics_service,
            chunk_manager,
        }
    }

    pub async fn start_download(
        &self,
        file_hash: String,
        output_path: String,
        max_peers: Option<usize>,
        chunk_size: Option<usize>,
    ) -> Result<(), String> {
        self.command_tx
            .send(MultiSourceCommand::StartDownload {
                file_hash,
                output_path,
                max_peers,
                chunk_size,
            })
            .map_err(|e| format!("Failed to send download command: {}", e))
    }

    pub async fn cancel_download(&self, file_hash: String) -> Result<(), String> {
        self.command_tx
            .send(MultiSourceCommand::CancelDownload { file_hash })
            .map_err(|e| format!("Failed to send cancel command: {}", e))
    }

    pub async fn get_download_progress(&self, file_hash: &str) -> Option<MultiSourceProgress> {
        let downloads = self.active_downloads.read().await;
        if let Some(download) = downloads.get(file_hash) {
            Some(self.calculate_progress(download))
        } else {
            None
        }
    }

    /// Verify chunk integrity and handle failure if hash mismatch
    /// Returns Ok(()) if verification passes, Err(()) if it fails
    pub async fn verify_chunk_for_download(
        &self,
        file_hash: &str,
        chunk_id: u32,
        data: &[u8],
        source_id: &str,
    ) -> Result<(), ()> {
        let downloads = self.active_downloads.read().await;
        if let Some(download) = downloads.get(file_hash) {
            if let Some(chunk_info) = download.chunks.iter().find(|c| c.chunk_id == chunk_id) {
                if let Err((expected, actual)) = verify_chunk_integrity(chunk_info, data) {
                    drop(downloads);

                    // Mark chunk as failed
                    {
                        let mut downloads = self.active_downloads.write().await;
                        if let Some(download) = downloads.get_mut(file_hash) {
                            download.failed_chunks.push_back(chunk_id);
                        }
                    }

                    // Emit ChunkFailed event
                    let error_msg =
                        format!("Chunk hash mismatch: expected {}, got {}", expected, actual);
                    let current_timestamp = current_timestamp_ms();

                    self.transfer_event_bus.emit_chunk_failed(ChunkFailedEvent {
                        transfer_id: file_hash.to_string(),
                        chunk_id,
                        source_id: source_id.to_string(),
                        source_type: SourceType::P2p,
                        failed_at: current_timestamp,
                        error: error_msg,
                        retry_count: 0,
                        will_retry: true,
                        next_retry_at: None,
                    });

                    return Err(());
                }
                Ok(())
            } else {
                // No ChunkInfo found, skip verification
                Ok(())
            }
        } else {
            // No active download found, skip verification
            Ok(())
        }
    }

    pub async fn run(&self) {
        info!("Starting MultiSourceDownloadService");

        let mut command_rx = self.command_rx.lock().await;

        while let Some(command) = command_rx.recv().await {
            match command {
                MultiSourceCommand::StartDownload {
                    file_hash,
                    output_path,
                    max_peers,
                    chunk_size,
                } => {
                    if let Err(e) = self
                        .handle_start_download(file_hash, output_path, max_peers, chunk_size)
                        .await
                    {
                        error!("Failed to start download: {}", e);
                    }
                }
                MultiSourceCommand::CancelDownload { file_hash } => {
                    self.handle_cancel_download(&file_hash).await;
                }
                MultiSourceCommand::RetryFailedChunks { file_hash } => {
                    if let Err(e) = self.handle_retry_failed_chunks(&file_hash).await {
                        error!("Failed to retry chunks for {}: {}", file_hash, e);
                    }
                }
            }
        }
    }

    async fn handle_start_download(
        &self,
        file_hash: String,
        output_path: String,
        max_peers: Option<usize>,
        chunk_size: Option<usize>,
    ) -> Result<(), String> {
        info!("Starting multi-source download for file: {}", file_hash);

        // Check if download is already active
        {
            let downloads = self.active_downloads.read().await;
            if downloads.contains_key(&file_hash) {
                return Err("Download already in progress".to_string());
            }
        }

        // Search for file metadata with sufficient timeout for DHT queries
        // Trigger progressive search and poll cache until metadata arrives
        self.dht_service
            .search_metadata(file_hash.clone(), 35000)
            .await
            .map_err(|e| format!("Failed to trigger DHT search: {}", e))?;

        // Poll cache for up to 35 seconds until metadata arrives via events
        let mut metadata = None;
        for _ in 0..350 {
            // 350 * 100ms = 35s
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if let Some(meta) = self.dht_service.get_cached_metadata(&file_hash).await {
                metadata = Some(meta);
                break;
            }
        }

        let metadata = metadata.ok_or_else(|| "File metadata not found after 35s".to_string())?;

        // Discover available sources (P2P peers + FTP sources)
        let mut available_sources = Vec::new();

        // 1. Discover P2P peers
        let available_peers = self
            .dht_service
            .discover_peers_for_file(&metadata)
            .await
            .map_err(|e| format!("Peer discovery failed: {}", e))?;

        info!(
            "Found {} available P2P peers for file",
            available_peers.len()
        );

        // Convert P2P peers to DownloadSource instances
        for peer_id in available_peers {
            available_sources.push(DownloadSource::P2p(crate::download_source::P2pSourceInfo {
                peer_id: peer_id.clone(),
                multiaddr: None,
                reputation: None,
                supports_encryption: false,
                protocol: Some("webrtc".to_string()),
            }));
        }

        // 2. Discover FTP sources from metadata
        if let Some(ftp_sources) = &metadata.ftp_sources {
            info!("Found {} FTP sources for file", ftp_sources.len());

            for ftp_info in ftp_sources {
                // Convert DHT FtpSourceInfo to DownloadSource FtpSourceInfo
                available_sources.push(DownloadSource::Ftp(DownloadFtpSourceInfo {
                    url: ftp_info.url.clone(),
                    username: ftp_info.username.clone(),
                    encrypted_password: ftp_info.password.clone(),
                    passive_mode: true, // Default to passive mode
                    use_ftps: false,    // Default to regular FTP
                    timeout_secs: Some(30),
                }));
            }
        }

        // 3. Discover ed2k sources from metadata
        let mut ed2k_chunk_hashes: Option<Vec<String>> = None;
        if let Some(ed2k_sources) = &metadata.ed2k_sources {
            info!("Found {} ed2k sources for file", ed2k_sources.len());

            for ed2k_info in ed2k_sources {
                // Extract chunk hashes from the first ED2K source that has them
                if ed2k_chunk_hashes.is_none() {
                    ed2k_chunk_hashes = ed2k_info.chunk_hashes.clone();
                }

                // Convert DHT Ed2kSourceInfo to DownloadSource Ed2kSourceInfo
                available_sources.push(DownloadSource::Ed2k(DownloadEd2kSourceInfo {
                    server_url: ed2k_info.server_url.clone(),
                    file_hash: ed2k_info.file_hash.clone(),
                    file_size: ed2k_info.file_size,
                    file_name: ed2k_info.file_name.clone(),
                    sources: ed2k_info.sources.clone(),
                    timeout_secs: ed2k_info.timeout,
                    chunk_hashes: ed2k_info.chunk_hashes.clone(),
                }));
            }
        }

        // 4. Discover BitTorrent source from metadata
        if let Some(info_hash) = &metadata.info_hash {
            info!(
                "Found BitTorrent source for file with info_hash: {}",
                info_hash
            );
            let mut magnet_uri = format!("magnet:?xt=urn:btih:{}", info_hash);
            if let Some(trackers) = &metadata.trackers {
                for tracker in trackers {
                    magnet_uri.push_str("&tr=");
                    magnet_uri.push_str(tracker);
                }
            }
            available_sources.push(DownloadSource::BitTorrent(BitTorrentSourceInfo {
                magnet_uri,
            }));
        }

        if available_sources.is_empty() {
            return Err("No sources available for download".to_string());
        }

        // Calculate chunk information
        let chunk_size = chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);
        let total_chunks = ((metadata.file_size as usize + chunk_size - 1) / chunk_size) as u32;
        let chunks = self.calculate_chunks(&metadata, chunk_size);

        // Determine if we should use multi-source download
        let use_multi_source =
            total_chunks >= MIN_CHUNKS_FOR_PARALLEL as u32 && available_sources.len() > 1;

        // Select optimal sources (cap at 1 when multi-source is not beneficial)
        let max_sources = if use_multi_source {
            max_peers.unwrap_or(available_sources.len().min(4))
        } else {
            1
        };
        let max_sources = max_sources.max(1);
        let selected_sources = self.select_optimal_sources(&available_sources, max_sources);

        info!(
            "Selected {} sources for multi-source download",
            selected_sources.len()
        );

        // Create download state
        let download = ActiveDownload {
            file_metadata: metadata.clone(),
            chunks,
            source_assignments: HashMap::new(),
            completed_chunks: HashMap::new(),
            pending_requests: HashMap::new(),
            failed_chunks: VecDeque::new(),
            start_time: Instant::now(),
            last_progress_update: Instant::now(),
            output_path,
            ed2k_chunk_hashes,
        };

        // Store download state
        {
            let mut downloads = self.active_downloads.write().await;
            downloads.insert(file_hash.clone(), download);
        }

        // Load any existing chunks from disk before starting downloads
        match self.load_existing_chunks_into_download(&file_hash).await {
            Ok(loaded_count) => {
                if loaded_count > 0 {
                    info!(
                        "Resumed download with {} existing chunks loaded from disk",
                        loaded_count
                    );

                    // Emit progress update for loaded chunks
                    let downloads = self.active_downloads.read().await;
                    if let Some(download) = downloads.get(&file_hash) {
                        let completed_chunks = download.completed_chunks.len() as u32;
                        let total_chunks = download.chunks.len() as u32;
                        let progress = (completed_chunks as f64 / total_chunks as f64) * 100.0;

                        // Emit progress event
                        self.transfer_event_bus
                            .emit_progress(TransferProgressEvent {
                                transfer_id: file_hash.clone(),
                                protocol: "WebRTC".to_string(),
                                downloaded_bytes: (completed_chunks as u64) * 256 * 1024, // Approximate based on chunk size
                                total_bytes: (total_chunks as u64) * 256 * 1024,
                                completed_chunks,
                                total_chunks,
                                progress_percentage: progress,
                                download_speed_bps: 0.0, // No speed for resumed chunks
                                upload_speed_bps: 0.0,
                                eta_seconds: None,
                                active_sources: 0,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                            });

                        // Check if download is already complete
                        if completed_chunks >= total_chunks {
                            info!("Download {} is already complete from disk", file_hash);
                            self.finalize_download(&file_hash).await?;
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to load existing chunks for download {}: {}",
                    file_hash, e
                );
                // Continue with download anyway
            }
        }

        // Start source connections and assign chunks
        self.start_source_connections(&file_hash, selected_sources.clone())
            .await?;

        // Emit download started event via TransferEventBus
        let available_source_infos: Vec<SourceInfo> = selected_sources
            .iter()
            .map(|s| {
                let (source_type, address) = match s {
                    DownloadSource::P2p(info) => (SourceType::P2p, info.peer_id.clone()),
                    DownloadSource::Http(info) => (SourceType::Http, info.url.clone()),
                    DownloadSource::Ftp(info) => (SourceType::Ftp, info.url.clone()),
                    DownloadSource::BitTorrent(info) => {
                        (SourceType::BitTorrent, info.magnet_uri.clone())
                    }
                    DownloadSource::Ed2k(info) => (SourceType::P2p, info.server_url.clone()),
                };
                SourceInfo {
                    id: s.identifier(),
                    source_type,
                    address,
                    reputation: None,
                    estimated_speed_bps: None,
                    latency_ms: None,
                    location: None,
                }
            })
            .collect();

        let selected_source_ids: Vec<String> =
            selected_sources.iter().map(|s| s.identifier()).collect();

        self.transfer_event_bus
            .emit_started_with_analytics(
                TransferStartedEvent {
                    transfer_id: file_hash.clone(),
                    file_hash: file_hash.clone(),
                    protocol: "WebRTC".to_string(),
                    file_name: metadata.file_name.clone(),
                    file_size: metadata.file_size,
                    total_chunks,
                    chunk_size,
                    started_at: current_timestamp_ms(),
                    available_sources: available_source_infos,
                    selected_sources: selected_source_ids,
                },
                &self.analytics_service,
            )
            .await;

        // Also emit legacy internal event for backwards compatibility
        let _ = self.event_tx.send(MultiSourceEvent::DownloadStarted {
            file_hash: file_hash.clone(),
            total_peers: selected_sources.len(),
        });

        // Start monitoring download progress
        self.spawn_download_monitor(file_hash).await;

        Ok(())
    }

    /// Extract chunk hashes from a serialized FileManifest JSON
    /// Returns a vector of chunk hashes indexed by chunk index
    fn extract_chunk_hashes_from_manifest(manifest_json: &str) -> Result<Vec<String>, String> {
        let manifest: FileManifest = serde_json::from_str(manifest_json)
            .map_err(|e| format!("Failed to parse manifest JSON: {}", e))?;

        // Create a vector with enough capacity for all chunks
        let mut hashes = Vec::new();
        for chunk in &manifest.chunks {
            // Ensure we have enough capacity
            while hashes.len() <= chunk.index as usize {
                hashes.push(String::new());
            }
            hashes[chunk.index as usize] = chunk.hash.clone();
        }

        Ok(hashes)
    }

    fn calculate_chunks(&self, metadata: &FileMetadata, chunk_size: usize) -> Vec<ChunkInfo> {
        let mut chunks = Vec::new();
        let total_size = metadata.file_size as usize;
        let mut offset = 0u64;
        let mut chunk_id = 0u32;

        // Try to extract chunk hashes from manifest if available
        let chunk_hashes = if let Some(manifest_json) = &metadata.manifest {
            match Self::extract_chunk_hashes_from_manifest(manifest_json) {
                Ok(hashes) => {
                    debug!(
                        "Successfully extracted {} chunk hashes from manifest",
                        hashes.len()
                    );
                    hashes
                }
                Err(e) => {
                    warn!(
                        "Failed to parse manifest for file {}: {}. Using placeholder hashes.",
                        metadata.merkle_root, e
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        while offset < metadata.file_size {
            let remaining = (metadata.file_size - offset) as usize;
            let size = remaining.min(chunk_size);

            // Use actual hash from manifest if available, otherwise fallback to placeholder
            let hash = if chunk_id < chunk_hashes.len() as u32
                && !chunk_hashes[chunk_id as usize].is_empty()
            {
                chunk_hashes[chunk_id as usize].clone()
            } else {
                // Fallback to placeholder hash for backward compatibility
                format!("{}_{}", metadata.merkle_root, chunk_id)
            };

            chunks.push(ChunkInfo {
                chunk_id,
                offset,
                size,
                hash,
            });

            offset += size as u64;
            chunk_id += 1;
        }

        chunks
    }

    /// Select optimal sources based on priority scoring
    fn select_optimal_sources(
        &self,
        available_sources: &[DownloadSource],
        max_sources: usize,
    ) -> Vec<DownloadSource> {
        let mut sources = available_sources.to_vec();

        // Sort by priority score (higher is better)
        sources.sort_by(|a, b| b.priority_score().cmp(&a.priority_score()));

        // Take the top sources
        sources.truncate(max_sources);

        info!("Selected sources by priority:");
        for (i, source) in sources.iter().enumerate() {
            info!(
                "  {}: {} (priority: {})",
                i + 1,
                source.display_name(),
                source.priority_score()
            );
        }

        sources
    }

    /// Start connections to all selected sources and assign chunks
    async fn start_source_connections(
        &self,
        file_hash: &str,
        sources: Vec<DownloadSource>,
    ) -> Result<(), String> {
        // Validate inputs early to avoid panics (empty sources would cause division/mod by zero)
        if sources.is_empty() {
            return Err("No sources provided for download".to_string());
        }

        let downloads = self.active_downloads.read().await;
        let download = downloads.get(file_hash).ok_or("Download not found")?;

        // Assign chunks to sources using round-robin strategy
        let chunk_assignments =
            self.assign_chunks_to_sources(&download.chunks, &sources, &download.completed_chunks);
        drop(downloads);

        // Start connecting to sources
        for (source, chunk_ids) in chunk_assignments {
            match &source {
                DownloadSource::P2p(p2p_info) => {
                    self.start_p2p_connection(file_hash, p2p_info.peer_id.clone(), chunk_ids)
                        .await?;
                }
                DownloadSource::Ftp(ftp_info) => {
                    self.start_ftp_connection(file_hash, ftp_info.clone(), chunk_ids)
                        .await?;
                }
                DownloadSource::Http(http_info) => {
                    self.start_http_download(file_hash, http_info.clone(), chunk_ids)
                        .await?;
                }
                DownloadSource::Ed2k(ed2k_info) => {
                    self.start_ed2k_connection(file_hash, ed2k_info.clone(), chunk_ids)
                        .await?;
                }
                DownloadSource::BitTorrent(bt_info) => {
                    self.start_bittorrent_download(file_hash, bt_info.clone(), chunk_ids)
                        .await?;
                }
            }
        }

        Ok(())
    }

    /// Assign chunks to sources using round-robin strategy
    fn assign_chunks_to_sources(
        &self,
        chunks: &[ChunkInfo],
        sources: &[DownloadSource],
        completed_chunks: &HashMap<u32, CompletedChunk>,
    ) -> Vec<(DownloadSource, Vec<u32>)> {
        // Defensive: if no sources, return an empty assignment list instead of panicking.
        if sources.is_empty() {
            return Vec::new();
        }

        let mut assignments: Vec<(DownloadSource, Vec<u32>)> =
            sources.iter().map(|s| (s.clone(), Vec::new())).collect();

        // Round-robin assignment - skip already completed chunks
        let mut source_index = 0;
        for chunk in chunks.iter() {
            // Skip chunks that are already completed
            if completed_chunks.contains_key(&chunk.chunk_id) {
                continue;
            }

            // Find next available source
            let mut assigned = false;
            for _ in 0..sources.len() {
                if let Some((_, chunks)) = assignments.get_mut(source_index) {
                    if chunks.len() < MAX_CHUNKS_PER_PEER {
                        chunks.push(chunk.chunk_id);
                        assigned = true;
                        break;
                    }
                }
                source_index = (source_index + 1) % sources.len();
            }

            // If no source has capacity, we'll skip this chunk
            // (it will be picked up by failed chunk retry logic later)
            if !assigned {
                debug!("No available source capacity for chunk {}", chunk.chunk_id);
            }

            source_index = (source_index + 1) % sources.len();
        }

        // Redistribute chunks if some sources have too few
        self.balance_source_assignments(assignments, chunks.len())
    }

    /// Balance chunk assignments across sources
    fn balance_source_assignments(
        &self,
        mut assignments: Vec<(DownloadSource, Vec<u32>)>,
        total_chunks: usize,
    ) -> Vec<(DownloadSource, Vec<u32>)> {
        let source_count = assignments.len();
        let target_chunks_per_source = (total_chunks + source_count - 1) / source_count;

        // Find sources with too many chunks and redistribute
        let mut excess_chunks = Vec::new();
        for (_, chunks) in assignments.iter_mut() {
            while chunks.len() > target_chunks_per_source {
                if let Some(chunk_id) = chunks.pop() {
                    excess_chunks.push(chunk_id);
                }
            }
        }

        // Redistribute excess chunks to sources with capacity
        for chunk_id in excess_chunks {
            for (_, chunks) in assignments.iter_mut() {
                if chunks.len() < target_chunks_per_source {
                    chunks.push(chunk_id);
                    break;
                }
            }
        }

        assignments
    }

    /// Start P2P connection (existing logic)
    async fn start_p2p_connection(
        &self,
        file_hash: &str,
        peer_id: String,
        chunk_ids: Vec<u32>,
    ) -> Result<(), String> {
        info!(
            "Connecting to P2P peer {} for {} chunks",
            peer_id,
            chunk_ids.len()
        );

        // Update source assignment status
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                let p2p_source = DownloadSource::P2p(crate::download_source::P2pSourceInfo {
                    peer_id: peer_id.clone(),
                    multiaddr: None,
                    reputation: None,
                    supports_encryption: false,
                    protocol: Some("webrtc".to_string()),
                });

                download.source_assignments.insert(
                    peer_id.clone(),
                    SourceAssignment::new(p2p_source, chunk_ids.clone()),
                );
            }
        }

        // Create WebRTC offer (existing WebRTC logic)
        match self.webrtc_service.create_offer(peer_id.clone()).await {
            Ok(offer) => {
                let offer_request = WebRTCOfferRequest {
                    offer_sdp: offer,
                    file_hash: file_hash.to_string(),
                    requester_peer_id: self.dht_service.get_peer_id().await,
                };

                match timeout(
                    Duration::from_secs(CONNECTION_TIMEOUT_SECS),
                    self.dht_service
                        .send_webrtc_offer(peer_id.clone(), offer_request),
                )
                .await
                {
                    Ok(Ok(answer_receiver)) => {
                        match timeout(
                            Duration::from_secs(CONNECTION_TIMEOUT_SECS),
                            answer_receiver,
                        )
                        .await
                        {
                            Ok(Ok(Ok(answer_response))) => {
                                match self
                                    .webrtc_service
                                    .establish_connection_with_answer(
                                        peer_id.clone(),
                                        answer_response.answer_sdp,
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        self.on_source_connected(file_hash, &peer_id, chunk_ids)
                                            .await;
                                        Ok(())
                                    }
                                    Err(e) => {
                                        self.on_source_failed(
                                            file_hash,
                                            &peer_id,
                                            format!("Connection failed: {}", e),
                                        )
                                        .await;
                                        Err(e)
                                    }
                                }
                            }
                            _ => {
                                let error = "Answer timeout".to_string();
                                self.on_source_failed(file_hash, &peer_id, error.clone())
                                    .await;
                                Err(error)
                            }
                        }
                    }
                    _ => {
                        let error = "Offer timeout".to_string();
                        self.on_source_failed(file_hash, &peer_id, error.clone())
                            .await;
                        Err(error)
                    }
                }
            }
            Err(e) => {
                let error = format!("Failed to create offer: {}", e);
                self.on_source_failed(file_hash, &peer_id, error.clone())
                    .await;
                Err(error)
            }
        }
    }

    /// Start FTP connection and chunk downloading
    async fn start_ftp_connection(
        &self,
        file_hash: &str,
        ftp_info: DownloadFtpSourceInfo,
        chunk_ids: Vec<u32>,
    ) -> Result<(), String> {
        info!(
            "Connecting to FTP server {} for {} chunks",
            ftp_info.url,
            chunk_ids.len()
        );

        let ftp_url_id = ftp_info.url.clone();

        // Update source assignment status
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                let ftp_source = DownloadSource::Ftp(ftp_info.clone());

                download.source_assignments.insert(
                    ftp_url_id.clone(),
                    SourceAssignment::new(ftp_source, chunk_ids.clone()),
                );
            }
        }

        // Parse FTP URL to get connection info
        let url = Url::parse(&ftp_info.url).map_err(|e| format!("Invalid FTP URL: {}", e))?;

        // Create credentials from FTP source info
        let credentials = if let Some(username) = &ftp_info.username {
            let password = ftp_info
                .encrypted_password
                .as_deref()
                .unwrap_or("anonymous@chiral.network");
            Some(FtpCredentials::new(username.clone(), password.to_string()))
        } else {
            None // Use anonymous credentials
        };

        // Attempt to establish FTP connection
        match self
            .ftp_downloader
            .connect_and_login(&url, credentials)
            .await
        {
            Ok(ftp_stream) => {
                info!("Successfully connected to FTP server: {}", ftp_info.url);

                // Store connection in pool for reuse
                {
                    let mut connections = self.ftp_connections.lock().await;
                    connections
                        .entry(ftp_url_id.clone())
                        .or_insert_with(Vec::new)
                        .push(ftp_stream);
                }

                // Mark source as connected and start chunk downloads
                self.on_source_connected(file_hash, &ftp_url_id, chunk_ids.clone())
                    .await;
                self.start_ftp_chunk_downloads(file_hash, ftp_info, chunk_ids)
                    .await;

                Ok(())
            }
            Err(e) => {
                // Provide more specific error messages based on common FTP errors
                let error_msg = if e.contains("Connection refused") {
                    format!(
                        "FTP server refused connection: {} (server may be down)",
                        ftp_info.url
                    )
                } else if e.contains("timeout") || e.contains("Timeout") {
                    format!(
                        "FTP connection timeout: {} (server may be slow or unreachable)",
                        ftp_info.url
                    )
                } else if e.contains("login") || e.contains("authentication") || e.contains("530") {
                    format!(
                        "FTP authentication failed: {} (invalid credentials)",
                        ftp_info.url
                    )
                } else if e.contains("550") {
                    format!("FTP file not found or permission denied: {}", ftp_info.url)
                } else {
                    format!("FTP connection failed: {} - {}", ftp_info.url, e)
                };

                warn!("{}", error_msg);
                self.on_source_failed(file_hash, &ftp_url_id, error_msg.clone())
                    .await;
                Err(error_msg)
            }
        }
    }

    /// Start downloading chunks from FTP server
    async fn start_ftp_chunk_downloads(
        &self,
        file_hash: &str,
        ftp_info: DownloadFtpSourceInfo,
        chunk_ids: Vec<u32>,
    ) {
        let ftp_url_id = ftp_info.url.clone();

        // Get chunk information for the assigned chunks
        let chunks_to_download = {
            let downloads = self.active_downloads.read().await;
            if let Some(download) = downloads.get(file_hash) {
                chunk_ids
                    .iter()
                    .filter_map(|&chunk_id| {
                        download
                            .chunks
                            .iter()
                            .find(|chunk| chunk.chunk_id == chunk_id)
                            .cloned()
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };

        if chunks_to_download.is_empty() {
            warn!("No chunks found for FTP download");
            return;
        }

        // Update source status to downloading
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(&ftp_url_id) {
                    assignment.status = SourceStatus::Downloading;
                }
            }
        }

        // Parse remote file path from FTP URL
        let remote_path = match self.parse_ftp_remote_path(&ftp_info.url) {
            Ok(path) => path,
            Err(e) => {
                self.on_source_failed(file_hash, &ftp_url_id, format!("Invalid FTP path: {}", e))
                    .await;
                return;
            }
        };

        // Download chunks concurrently (but limit concurrency to avoid overwhelming FTP server)
        let downloader = self.ftp_downloader.clone();
        let connections = self.ftp_connections.clone();
        let file_hash_clone = file_hash.to_string();
        let ftp_url_clone = ftp_url_id.clone();
        let event_tx = self.event_tx.clone();
        let downloads = self.active_downloads.clone();
        let transfer_event_bus = self.transfer_event_bus.clone();
        let chunk_manager = self.chunk_manager.clone();
        let ftp_info_clone = ftp_info.clone();
        let command_tx = self.command_tx.clone();

        tokio::spawn(async move {
            let semaphore = Arc::new(tokio::sync::Semaphore::new(2)); // Max 2 concurrent FTP downloads per server

            let mut tasks = Vec::new();

            for chunk_info in chunks_to_download {
                let permit = semaphore.clone().acquire_owned().await;
                if permit.is_err() {
                    continue;
                }

                let downloader = downloader.clone();
                let connections = connections.clone();
                let remote_path = remote_path.clone();
                let file_hash = file_hash_clone.clone();
                let ftp_url = ftp_url_clone.clone();
                let event_tx = event_tx.clone();
                let downloads = downloads.clone();
                let chunk = chunk_info.clone();
                let transfer_event_bus = transfer_event_bus.clone();
                let chunk_manager = chunk_manager.clone();
                let ftp_info_for_task = ftp_info_clone.clone();
                let command_tx = command_tx.clone();

                let task = tokio::spawn(async move {
                    let _permit = permit.unwrap();

                    // Calculate byte range for this chunk
                    let (start_byte, size) = (chunk.offset, chunk.size as u64);

                    info!(
                        "Downloading FTP chunk {} ({}:{}) from {}",
                        chunk.chunk_id, start_byte, size, remote_path
                    );

                    // Capture start time for duration tracking
                    let download_start_ms = current_timestamp_ms();

                    // Get FTP connection from pool or create new one
                    let download_result = {
                        let ftp_stream = {
                            let mut connections_guard = connections.lock().await;
                            let pool = connections_guard
                                .entry(ftp_url.clone())
                                .or_insert_with(Vec::new);

                            if let Some(stream) = pool.pop() {
                                drop(connections_guard);
                                stream
                            } else {
                                drop(connections_guard);

                                // Create new connection
                                let url = Url::parse(&ftp_url)
                                    .map_err(|e| format!("Invalid FTP URL: {}", e))?;
                                let credentials =
                                    if let Some(username) = &ftp_info_for_task.username {
                                        let password = ftp_info_for_task
                                            .encrypted_password
                                            .as_deref()
                                            .unwrap_or("anonymous@chiral.network");
                                        Some(FtpCredentials::new(
                                            username.clone(),
                                            password.to_string(),
                                        ))
                                    } else {
                                        None
                                    };

                                match downloader.connect_and_login(&url, credentials).await {
                                    Ok(stream) => stream,
                                    Err(e) => {
                                        return Err(format!(
                                            "Failed to create FTP connection: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        };

                        // Hard timeout + blocking isolation:
                        // move the stream into the downloader so we can enforce a timeout even if the data socket hangs.
                        match downloader
                            .download_range_with_timeout(
                                ftp_stream,
                                remote_path.clone(),
                                start_byte,
                                size,
                            )
                            .await
                        {
                            Ok((returned_stream, data)) => {
                                // Return connection to pool for reuse
                                let mut connections_guard = connections.lock().await;
                                let pool = connections_guard
                                    .entry(ftp_url.clone())
                                    .or_insert_with(Vec::new);
                                pool.push(returned_stream);
                                Ok(data)
                            }
                            Err((maybe_stream, e)) => {
                                // Return the connection only if we still have it.
                                if let Some(returned_stream) = maybe_stream {
                                    let mut connections_guard = connections.lock().await;
                                    let pool = connections_guard
                                        .entry(ftp_url.clone())
                                        .or_insert_with(Vec::new);
                                    pool.push(returned_stream);
                                }
                                Err(e)
                            }
                        }
                    };

                    match download_result {
                        Ok(data) => {
                            // Verify chunk data (basic size check)
                            if data.len() != chunk.size {
                                warn!(
                                    "FTP chunk {} size mismatch: expected {}, got {}",
                                    chunk.chunk_id,
                                    chunk.size,
                                    data.len()
                                );

                                // Reject partial data
                                let error_msg = format!(
                                    "Chunk size mismatch: expected {}, got {} (partial data rejected)",
                                    chunk.size,
                                    data.len()
                                );
                                warn!(
                                    "FTP chunk {} rejected due to size mismatch - marking for retry",
                                    chunk.chunk_id
                                );

                                {
                                    let mut downloads_guard = downloads.write().await;
                                    if let Some(download) = downloads_guard.get_mut(&file_hash) {
                                        download.failed_chunks.push_back(chunk.chunk_id);
                                    }
                                }
                                // Emit chunk failed event via TransferEventBus
                                transfer_event_bus.emit_chunk_failed(ChunkFailedEvent {
                                    transfer_id: file_hash.clone(),
                                    chunk_id: chunk.chunk_id,
                                    source_id: ftp_url.clone(),
                                    source_type: SourceType::Ftp,
                                    failed_at: current_timestamp_ms(),
                                    error: error_msg.clone(),
                                    retry_count: 0,
                                    will_retry: true,
                                    next_retry_at: None,
                                });
                                // Also emit legacy internal event
                                let _ = event_tx.send(MultiSourceEvent::ChunkFailed {
                                    file_hash: file_hash.clone(),
                                    chunk_id: chunk.chunk_id,
                                    peer_id: ftp_url.clone(),
                                    error: error_msg.clone(),
                                });

                                // Trigger retry
                                let _ = command_tx.send(MultiSourceCommand::RetryFailedChunks {
                                    file_hash: file_hash.clone(),
                                });

                                return Ok(());
                            }

                            if let Err((expected, actual)) = verify_chunk_integrity(&chunk, &data) {
                                let error_msg = format!(
                                    "Chunk hash mismatch: expected {}, got {}",
                                    expected, actual
                                );
                                warn!(
                                    "FTP chunk {} hash verification failed: {}",
                                    chunk.chunk_id, error_msg
                                );
                                {
                                    let mut downloads_guard = downloads.write().await;
                                    if let Some(download) = downloads_guard.get_mut(&file_hash) {
                                        download.failed_chunks.push_back(chunk.chunk_id);
                                    }
                                }
                                // Emit chunk failed event via TransferEventBus
                                transfer_event_bus.emit_chunk_failed(ChunkFailedEvent {
                                    transfer_id: file_hash.clone(),
                                    chunk_id: chunk.chunk_id,
                                    source_id: ftp_url.clone(),
                                    source_type: SourceType::Ftp,
                                    failed_at: current_timestamp_ms(),
                                    error: error_msg.clone(),
                                    retry_count: 0,
                                    will_retry: true,
                                    next_retry_at: None,
                                });
                                // Also emit legacy internal event
                                let _ = event_tx.send(MultiSourceEvent::ChunkFailed {
                                    file_hash: file_hash.clone(),
                                    chunk_id: chunk.chunk_id,
                                    peer_id: ftp_url.clone(),
                                    error: error_msg,
                                });

                                // Trigger retry
                                let _ = command_tx.send(MultiSourceCommand::RetryFailedChunks {
                                    file_hash: file_hash.clone(),
                                });

                                return Ok(());
                            }

                            // Store completed chunk and check for completion
                            let is_complete = {
                                let mut downloads_guard = downloads.write().await;
                                if let Some(download) = downloads_guard.get_mut(&file_hash) {
                                    let completed_chunk = CompletedChunk {
                                        chunk_id: chunk.chunk_id,
                                        data: data.clone(), // Clone for memory storage
                                        source_id: ftp_url.clone(),
                                        completed_at: Instant::now(),
                                    };
                                    download
                                        .completed_chunks
                                        .insert(chunk.chunk_id, completed_chunk);

                                    // Update last activity
                                    if let Some(assignment) =
                                        download.source_assignments.get_mut(&ftp_url)
                                    {
                                        let now = match SystemTime::now().duration_since(UNIX_EPOCH)
                                        {
                                            Ok(d) => Some(d.as_secs()),
                                            Err(_) => None,
                                        };
                                        assignment.last_activity = now;
                                    }

                                    // Check if download is complete
                                    download.completed_chunks.len() == download.chunks.len()
                                } else {
                                    false
                                }
                            };

                            info!(
                                "Successfully downloaded FTP chunk {} ({} bytes)",
                                chunk.chunk_id, chunk.size
                            );

                            // Store chunk data to disk for persistence (clone before moving into CompletedChunk)
                            let data_for_disk = data.clone();
                            let file_hash_for_disk = file_hash.clone();
                            let chunk_id_for_disk = chunk.chunk_id;

                            // Store chunk to disk asynchronously
                            let chunk_manager_clone = chunk_manager.clone();
                            tokio::spawn(async move {
                                let chunks_dir = std::path::Path::new("./chunks");
                                if !chunks_dir.exists() {
                                    let _ = std::fs::create_dir_all(chunks_dir);
                                }

                                let file_dir = chunks_dir.join(&file_hash_for_disk);
                                if !file_dir.exists() {
                                    let _ = std::fs::create_dir_all(&file_dir);
                                }

                                let chunk_path =
                                    file_dir.join(format!("chunk_{}.dat", chunk_id_for_disk));
                                if let Err(e) = tokio::fs::write(&chunk_path, &data_for_disk).await
                                {
                                    warn!(
                                        "Failed to write chunk {} to disk: {}",
                                        chunk_id_for_disk, e
                                    );
                                } else {
                                    let metadata_path =
                                        file_dir.join(format!("chunk_{}.meta", chunk_id_for_disk));
                                    let metadata = serde_json::json!({
                                        "chunk_id": chunk_id_for_disk,
                                        "size": data_for_disk.len(),
                                        "stored_at": std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        "file_hash": file_hash_for_disk
                                    });
                                    let _ = tokio::fs::write(
                                        &metadata_path,
                                        serde_json::to_string_pretty(&metadata).unwrap(),
                                    )
                                    .await;

                                    // Also store in ChunkManager for deduplication (generate content hash)
                                    let mut hasher = Sha256::new();
                                    hasher.update(&data_for_disk);
                                    let content_hash = format!("{:x}", hasher.finalize());
                                    let _ = chunk_manager_clone
                                        .save_chunk(&content_hash, &data_for_disk);
                                }
                            });

                            // Calculate actual download duration
                            let completed_at = current_timestamp_ms();
                            let download_duration_ms =
                                completed_at.saturating_sub(download_start_ms);

                            // Emit chunk completed event via TransferEventBus
                            transfer_event_bus.emit_chunk_completed(ChunkCompletedEvent {
                                transfer_id: file_hash.clone(),
                                chunk_id: chunk.chunk_id,
                                chunk_size: chunk.size,
                                source_id: ftp_url.clone(),
                                source_type: SourceType::Ftp,
                                completed_at,
                                download_duration_ms,
                                verified: true,
                            });

                            // Also emit legacy internal event for backwards compatibility
                            let _ = event_tx.send(MultiSourceEvent::ChunkCompleted {
                                file_hash: file_hash.clone(),
                                chunk_id: chunk.chunk_id,
                                peer_id: ftp_url.clone(),
                            });

                            // Check if download is complete and finalize
                            if is_complete {
                                if let Err(e) =
                                    Self::finalize_download_static(&downloads, &file_hash).await
                                {
                                    error!("Failed to finalize FTP download: {}", e);
                                }
                            }
                            Ok(())
                        }
                        Err(e) => {
                            warn!("Failed to download FTP chunk {}: {}", chunk.chunk_id, e);

                            // Add chunk back to failed queue
                            {
                                let mut downloads_guard = downloads.write().await;
                                if let Some(download) = downloads_guard.get_mut(&file_hash) {
                                    download.failed_chunks.push_back(chunk.chunk_id);
                                }
                            }

                            // Emit chunk failed event via TransferEventBus
                            transfer_event_bus.emit_chunk_failed(ChunkFailedEvent {
                                transfer_id: file_hash.clone(),
                                chunk_id: chunk.chunk_id,
                                source_id: ftp_url.clone(),
                                source_type: SourceType::Ftp,
                                failed_at: current_timestamp_ms(),
                                error: e.clone(),
                                retry_count: 0,
                                will_retry: true,
                                next_retry_at: None,
                            });
                            // Also emit legacy internal event
                            let _ = event_tx.send(MultiSourceEvent::ChunkFailed {
                                file_hash: file_hash.clone(),
                                chunk_id: chunk.chunk_id,
                                peer_id: ftp_url.clone(),
                                error: e,
                            });
                            Ok(())
                        }
                    }
                });

                tasks.push(task);
            }

            // Wait for all chunk downloads to complete
            for task in tasks {
                let _ = task.await;
            }

            // Check if all chunks for this FTP source are completed
            let all_chunks_completed = {
                let downloads_guard = downloads.read().await;
                if let Some(download) = downloads_guard.get(&file_hash_clone) {
                    if let Some(assignment) = download.source_assignments.get(&ftp_url_clone) {
                        assignment
                            .chunks
                            .iter()
                            .all(|&chunk_id| download.completed_chunks.contains_key(&chunk_id))
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if all_chunks_completed {
                // Mark FTP source as completed
                {
                    let mut downloads_guard = downloads.write().await;
                    if let Some(download) = downloads_guard.get_mut(&file_hash_clone) {
                        if let Some(assignment) =
                            download.source_assignments.get_mut(&ftp_url_clone)
                        {
                            assignment.status = SourceStatus::Completed;
                        }
                    }
                }

                info!("FTP source {} completed all assigned chunks", ftp_url_clone);
            }
        });
    }

    /// Start HTTP download (placeholder implementation)
    async fn start_http_download(
        &self,
        file_hash: &str,
        http_info: crate::download_source::HttpSourceInfo,
        chunk_ids: Vec<u32>,
    ) -> Result<(), String> {
        info!(
            "Starting HTTP download for {} chunks from {}",
            chunk_ids.len(),
            http_info.url
        );

        // For now, implement basic HTTP download support
        // In a full implementation, this would use the http_download.rs module
        // to download chunks with Range requests and verify hashes

        // Get file metadata to access chunk information
        let downloads = self.active_downloads.read().await;
        let download = match downloads.get(file_hash) {
            Some(download) => download,
            None => {
                let error = format!("No active download found for file {}", file_hash);
                error!("{}", error);
                self.on_source_failed(file_hash, &http_info.url, error.clone())
                    .await;
                return Err(error);
            }
        };

        // For each requested chunk, attempt HTTP download with hash verification
        for chunk_id in chunk_ids {
            // Capture start time for duration tracking
            let download_start_ms = current_timestamp_ms();

            // Find chunk info
            let chunk_info = match download.chunks.iter().find(|c| c.chunk_id == chunk_id) {
                Some(chunk) => chunk,
                None => {
                    warn!(
                        "Chunk {} not found in metadata for file {}",
                        chunk_id, file_hash
                    );
                    continue;
                }
            };

            // Calculate byte range for this chunk
            let start_byte = chunk_info.offset;
            let end_byte = start_byte + chunk_info.size as u64 - 1;

            // Create HTTP client for range request
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            // Make range request
            let response = match client
                .get(&http_info.url)
                .header("Range", format!("bytes={}-{}", start_byte, end_byte))
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    let error = format!("HTTP request failed for chunk {}: {}", chunk_id, e);
                    warn!("{}", error);
                    self.on_source_failed(file_hash, &http_info.url, error)
                        .await;
                    continue;
                }
            };

            // Check for partial content response
            if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                let error = format!(
                    "HTTP server doesn't support range requests for chunk {} (status: {})",
                    chunk_id,
                    response.status()
                );
                warn!("{}", error);
                self.on_source_failed(file_hash, &http_info.url, error)
                    .await;
                continue;
            }

            // Read response data
            let chunk_data = match response.bytes().await {
                Ok(data) => data.to_vec(),
                Err(e) => {
                    let error =
                        format!("Failed to read HTTP response for chunk {}: {}", chunk_id, e);
                    warn!("{}", error);
                    self.on_source_failed(file_hash, &http_info.url, error)
                        .await;
                    continue;
                }
            };

            // Verify chunk size
            if chunk_data.len() != chunk_info.size {
                let error = format!(
                    "HTTP chunk {} size mismatch: expected {}, got {}",
                    chunk_id,
                    chunk_info.size,
                    chunk_data.len()
                );
                warn!("{}", error);
                self.on_source_failed(file_hash, &http_info.url, error)
                    .await;
                continue;
            }

            // Verify chunk hash
            if let Err((expected, actual)) = verify_chunk_integrity(chunk_info, &chunk_data) {
                let error = format!(
                    "HTTP chunk {} hash verification failed: expected {}, got {}",
                    chunk_id, expected, actual
                );
                warn!("{}", error);
                self.on_source_failed(file_hash, &http_info.url, error)
                    .await;
                continue;
            }

            // Chunk passed verification - store it
            info!(
                "HTTP chunk {} downloaded and verified successfully",
                chunk_id
            );
            if let Err(e) = self
                .store_verified_chunk(
                    file_hash,
                    chunk_info,
                    chunk_data,
                    download_start_ms,
                    &http_info.url,
                    SourceType::Http,
                )
                .await
            {
                let error = format!("Failed to store HTTP chunk {}: {}", chunk_id, e);
                error!("{}", error);
                self.on_source_failed(file_hash, &http_info.url, error)
                    .await;
            }
        }

        Ok(())
    }

    /// Store a verified chunk in the active download
    async fn store_verified_chunk(
        &self,
        file_hash: &str,
        chunk_info: &ChunkInfo,
        data: Vec<u8>,
        download_start_ms: u64,
        source_id: &str,
        source_type: SourceType,
    ) -> Result<(), String> {
        let mut downloads = self.active_downloads.write().await;
        let download = downloads
            .get_mut(file_hash)
            .ok_or_else(|| format!("Active download not found for file {}", file_hash))?;

        // Prepare data for disk storage (clone before moving into CompletedChunk)
        let data_for_disk = data.clone();
        let file_hash_for_disk = file_hash.to_string();
        let chunk_id_for_disk = chunk_info.chunk_id;

        // Store the chunk data in memory
        let completed_chunk = CompletedChunk {
            chunk_id: chunk_info.chunk_id,
            data,
            source_id: source_id.to_string(),
            completed_at: std::time::Instant::now(),
        };
        download
            .completed_chunks
            .insert(chunk_info.chunk_id, completed_chunk);

        // Get completion info before releasing the lock
        let is_complete = download.completed_chunks.len() == download.chunks.len();

        // Release the lock before disk I/O and finalization
        drop(downloads);

        // Store chunk to disk asynchronously (keep existing approach for chunk_id mapping)
        // Also store in ChunkManager for potential deduplication
        let chunk_manager = self.chunk_manager.clone();
        let chunk_manager_clone = chunk_manager.clone();
        tokio::spawn(async move {
            // Inline the disk storage logic to avoid lifetime issues
            let chunks_dir = std::path::Path::new("./chunks");
            if !chunks_dir.exists() {
                let _ = std::fs::create_dir_all(chunks_dir);
            }

            let file_dir = chunks_dir.join(&file_hash_for_disk);
            if !file_dir.exists() {
                let _ = std::fs::create_dir_all(&file_dir);
            }

            let chunk_path = file_dir.join(format!("chunk_{}.dat", chunk_id_for_disk));
            if let Err(e) = tokio::fs::write(&chunk_path, &data_for_disk).await {
                warn!(
                    "Failed to write HTTP chunk {} to disk: {}",
                    chunk_id_for_disk, e
                );
            } else {
                let metadata_path = file_dir.join(format!("chunk_{}.meta", chunk_id_for_disk));
                let metadata = serde_json::json!({
                    "chunk_id": chunk_id_for_disk,
                    "size": data_for_disk.len(),
                    "stored_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    "file_hash": file_hash_for_disk
                });
                let _ = tokio::fs::write(
                    &metadata_path,
                    serde_json::to_string_pretty(&metadata).unwrap(),
                )
                .await;

                // Also store in ChunkManager for deduplication (generate content hash)
                let mut hasher = Sha256::new();
                hasher.update(&data_for_disk);
                let content_hash = format!("{:x}", hasher.finalize());
                let _ = chunk_manager_clone.save_chunk(&content_hash, &data_for_disk);
            }
        });

        // Calculate actual download duration
        let completed_at = current_timestamp_ms();
        let download_duration_ms = completed_at.saturating_sub(download_start_ms);

        // Emit chunk completed event via TransferEventBus
        self.transfer_event_bus
            .emit_chunk_completed(ChunkCompletedEvent {
                transfer_id: file_hash.to_string(),
                chunk_id: chunk_info.chunk_id,
                chunk_size: chunk_info.size,
                source_id: source_id.to_string(),
                source_type,
                completed_at,
                download_duration_ms,
                verified: true,
            });

        // Also emit legacy internal event for backwards compatibility
        if let Err(e) = self.event_tx.send(MultiSourceEvent::ChunkCompleted {
            file_hash: file_hash.to_string(),
            chunk_id: chunk_info.chunk_id,
            peer_id: source_id.to_string(),
        }) {
            warn!("Failed to emit chunk completed event: {}", e);
        }

        // Check if download is complete
        if is_complete {
            Self::finalize_download_static(&self.active_downloads, file_hash).await?;
        }

        Ok(())
    }

    /// Ingest a fully downloaded file (e.g., from BitTorrent) into the chunk pipeline
    async fn ingest_file_chunks(
        downloads: &Arc<RwLock<HashMap<String, ActiveDownload>>>,
        transfer_event_bus: &Arc<TransferEventBus>,
        event_tx: &mpsc::UnboundedSender<MultiSourceEvent>,
        chunk_manager: &Arc<ChunkManager>,
        file_hash: &str,
        source_id: &str,
        file_bytes: Vec<u8>,
    ) -> Result<(), String> {
        // Snapshot chunks to avoid holding the lock for the entire ingestion
        let (chunks, output_path) = {
            let downloads_read = downloads.read().await;
            let download = downloads_read
                .get(file_hash)
                .ok_or_else(|| "Download not found while ingesting completed file".to_string())?;
            (download.chunks.clone(), download.output_path.clone())
        };

        let total_chunks = chunks.len();

        for chunk_info in chunks {
            let start = chunk_info.offset as usize;
            let end = start.saturating_add(chunk_info.size).min(file_bytes.len());

            let slice = file_bytes
                .get(start..end)
                .ok_or_else(|| format!("Chunk {} range out of bounds", chunk_info.chunk_id))?
                .to_vec();

            {
                let mut downloads_write = downloads.write().await;
                if let Some(download) = downloads_write.get_mut(file_hash) {
                    download.completed_chunks.insert(
                        chunk_info.chunk_id,
                        CompletedChunk {
                            chunk_id: chunk_info.chunk_id,
                            data: slice.clone(),
                            source_id: source_id.to_string(),
                            completed_at: std::time::Instant::now(),
                        },
                    );

                    if let Some(assignment) = download.source_assignments.get_mut(source_id) {
                        assignment.last_activity = Some(current_timestamp_ms());
                    }
                }
            }

            // Persist the chunk to disk and chunk manager (mirrors store_verified_chunk)
            let data_for_disk = slice.clone();
            let file_hash_for_disk = file_hash.to_string();
            let chunk_id_for_disk = chunk_info.chunk_id;
            let chunk_manager_clone = chunk_manager.clone();
            tokio::spawn(async move {
                let chunks_dir = std::path::Path::new("./chunks");
                if !chunks_dir.exists() {
                    let _ = std::fs::create_dir_all(chunks_dir);
                }

                let file_dir = chunks_dir.join(&file_hash_for_disk);
                if !file_dir.exists() {
                    let _ = std::fs::create_dir_all(&file_dir);
                }

                let chunk_path = file_dir.join(format!("chunk_{}.dat", chunk_id_for_disk));
                if let Err(e) = tokio::fs::write(&chunk_path, &data_for_disk).await {
                    warn!(
                        "Failed to write BitTorrent chunk {} to disk: {}",
                        chunk_id_for_disk, e
                    );
                } else {
                    let metadata_path = file_dir.join(format!("chunk_{}.meta", chunk_id_for_disk));
                    let metadata = serde_json::json!({
                        "chunk_id": chunk_id_for_disk,
                        "size": data_for_disk.len(),
                        "stored_at": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        "file_hash": file_hash_for_disk
                    });
                    let _ = tokio::fs::write(
                        &metadata_path,
                        serde_json::to_string_pretty(&metadata).unwrap(),
                    )
                    .await;

                    // Also store in ChunkManager for deduplication (generate content hash)
                    let mut hasher = Sha256::new();
                    hasher.update(&data_for_disk);
                    let content_hash = format!("{:x}", hasher.finalize());
                    let _ = chunk_manager_clone.save_chunk(&content_hash, &data_for_disk);
                }
            });

            // Emit chunk completion events
            let completed_at = current_timestamp_ms();
            transfer_event_bus.emit_chunk_completed(ChunkCompletedEvent {
                transfer_id: file_hash.to_string(),
                chunk_id: chunk_info.chunk_id,
                chunk_size: chunk_info.size,
                source_id: source_id.to_string(),
                source_type: SourceType::BitTorrent,
                completed_at,
                download_duration_ms: 0,
                verified: true,
            });

            if let Err(e) = event_tx.send(MultiSourceEvent::ChunkCompleted {
                file_hash: file_hash.to_string(),
                chunk_id: chunk_info.chunk_id,
                peer_id: source_id.to_string(),
            }) {
                warn!("Failed to emit chunk completed event: {}", e);
            }
        }

        {
            let mut downloads_write = downloads.write().await;
            if let Some(download) = downloads_write.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(source_id) {
                    assignment.status = SourceStatus::Completed;
                }
            }
        }

        // Finalize assembled file
        Self::finalize_download_static(downloads, file_hash).await?;

        // Clean up persisted download state if present
        let downloads_dir = std::path::Path::new("./downloads");
        let state_path = downloads_dir.join(format!("{}.state", file_hash));
        if state_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&state_path).await {
                warn!("Failed to remove persisted state for {}: {}", file_hash, e);
            }
        }

        info!(
            "BitTorrent download {} finalized to {} ({} chunks)",
            file_hash, output_path, total_chunks
        );

        Ok(())
    }
    /// Start BitTorrent download
    async fn start_bittorrent_download(
        &self,
        file_hash: &str,
        bt_info: BitTorrentSourceInfo,
        chunk_ids: Vec<u32>,
    ) -> Result<(), String> {
        info!(
            "Starting BitTorrent download for {} chunks using magnet: {}",
            chunk_ids.len(),
            bt_info.magnet_uri
        );

        // Track the source assignment
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                let bt_source = DownloadSource::BitTorrent(bt_info.clone());
                download.source_assignments.insert(
                    bt_info.magnet_uri.clone(),
                    SourceAssignment::new(bt_source, chunk_ids.clone()),
                );
            } else {
                return Err(format!(
                    "Download {} not found for BitTorrent source",
                    file_hash
                ));
            }
        }

        // Determine output folder for the torrent (parent of requested output path)
        let (output_folder, expected_name) = {
            let downloads = self.active_downloads.read().await;
            let download = downloads
                .get(file_hash)
                .ok_or_else(|| "Download state missing during BitTorrent start".to_string())?;

            let target_path = std::path::PathBuf::from(&download.output_path);
            let parent = target_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));

            (parent, download.file_metadata.file_name.clone())
        };

        if let Err(e) = tokio::fs::create_dir_all(&output_folder).await {
            let err = format!(
                "Failed to create BitTorrent output dir {:?}: {}",
                output_folder, e
            );
            self.on_source_failed(file_hash, &bt_info.magnet_uri, err.clone())
                .await;
            return Err(err);
        }

        // Kick off the torrent download with the specified output folder
        let handle = match self
            .bittorrent_handler
            .start_download_to(&bt_info.magnet_uri, output_folder.clone())
            .await
        {
            Ok(handle) => handle,
            Err(e) => {
                let err = format!("BitTorrent start failed: {}", e);
                self.on_source_failed(file_hash, &bt_info.magnet_uri, err.clone())
                    .await;
                return Err(err);
            }
        };

        // Update status to Downloading
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(&bt_info.magnet_uri) {
                    assignment.status = SourceStatus::Downloading;
                    assignment.connected_at = Some(current_timestamp_ms());
                }
            }
        }

        // Monitor torrent in the background and ingest completed data into our chunk pipeline
        let downloads_arc = self.active_downloads.clone();
        let event_tx = self.event_tx.clone();
        let transfer_bus = self.transfer_event_bus.clone();
        let chunk_manager = self.chunk_manager.clone();
        let bittorrent_handler = self.bittorrent_handler.clone();
        let file_hash_string = file_hash.to_string();
        let magnet = bt_info.magnet_uri.clone();
        let target_path = std::path::PathBuf::from(&output_folder).join(expected_name.clone());

        tokio::spawn(async move {
            let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(8);
            let handler_clone = bittorrent_handler.clone();
            let handle_clone = handle.clone();

            // Spawn monitor loop
            tokio::spawn(async move {
                handler_clone
                    .monitor_download(handle_clone, progress_tx)
                    .await;
            });

            while let Some(event) = progress_rx.recv().await {
                match event {
                    crate::bittorrent_handler::BitTorrentEvent::Progress { .. } => {
                        // Update last activity timestamp
                        let mut downloads = downloads_arc.write().await;
                        if let Some(download) = downloads.get_mut(&file_hash_string) {
                            if let Some(assignment) = download.source_assignments.get_mut(&magnet) {
                                assignment.last_activity = Some(current_timestamp_ms());
                            }
                        }
                    }
                    crate::bittorrent_handler::BitTorrentEvent::Completed => {
                        info!("BitTorrent download completed for {}", &file_hash_string);

                        match tokio::fs::read(&target_path).await {
                            Ok(file_bytes) => {
                                // Ingest the file into chunk pipeline so finalize_download works
                                if let Err(e) = Self::ingest_file_chunks(
                                    &downloads_arc,
                                    &transfer_bus,
                                    &event_tx,
                                    &chunk_manager,
                                    &file_hash_string,
                                    &magnet,
                                    file_bytes,
                                )
                                .await
                                {
                                    warn!(
                                        "Failed to ingest BitTorrent download {}: {}",
                                        file_hash_string, e
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "BitTorrent file read failed for {} at {:?}: {}",
                                    file_hash_string, target_path, e
                                );
                                let mut downloads = downloads_arc.write().await;
                                if let Some(download) = downloads.get_mut(&file_hash_string) {
                                    if let Some(assignment) =
                                        download.source_assignments.get_mut(&magnet)
                                    {
                                        assignment.status = SourceStatus::Failed;
                                    }
                                }
                            }
                        }
                        break;
                    }
                    crate::bittorrent_handler::BitTorrentEvent::Failed(err) => {
                        warn!(
                            "BitTorrent download failed for {}: {}",
                            file_hash_string, err
                        );
                        // Mark as failed
                        let mut downloads = downloads_arc.write().await;
                        if let Some(download) = downloads.get_mut(&file_hash_string) {
                            if let Some(assignment) = download.source_assignments.get_mut(&magnet) {
                                assignment.status = SourceStatus::Failed;
                            }
                        }
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Parse remote path from FTP URL (placeholder implementation)
    fn parse_ftp_remote_path(&self, url: &str) -> Result<String, String> {
        use url::Url;

        let parsed_url = Url::parse(url).map_err(|e| format!("Invalid FTP URL: {}", e))?;

        let path = parsed_url.path();
        if path.is_empty() || path == "/" {
            return Err("No file path specified in FTP URL".to_string());
        }

        Ok(path.to_string())
    }

    /// Start Ed2k connection and begin downloading chunks
    async fn start_ed2k_connection(
        &self,
        file_hash: &str,
        ed2k_info: DownloadEd2kSourceInfo,
        chunk_ids: Vec<u32>,
    ) -> Result<(), String> {
        info!(
            "Connecting to Ed2k server {} for {} chunks",
            ed2k_info.server_url,
            chunk_ids.len()
        );

        let server_url_id = ed2k_info.server_url.clone();

        // Update source assignment status
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                let ed2k_source = DownloadSource::Ed2k(ed2k_info.clone());

                download.source_assignments.insert(
                    server_url_id.clone(),
                    SourceAssignment::new(ed2k_source, chunk_ids.clone()),
                );
            }
        }

        // Create Ed2k client with configuration
        let config = Ed2kConfig {
            server_url: ed2k_info.server_url.clone(),
            timeout: std::time::Duration::from_secs(ed2k_info.timeout_secs.unwrap_or(30)),
            client_id: None, // Will be assigned by server
        };

        let mut ed2k_client = Ed2kClient::with_config(config);

        // Attempt to establish Ed2k connection
        match ed2k_client.connect().await {
            Ok(()) => {
                info!(
                    "Successfully connected to Ed2k server: {}",
                    ed2k_info.server_url
                );

                // Store connection for reuse
                {
                    let mut connections = self.ed2k_connections.lock().await;
                    connections.insert(server_url_id.clone(), ed2k_client);
                }

                // Mark source as connected and start chunk downloads
                self.on_source_connected(file_hash, &server_url_id, chunk_ids.clone())
                    .await;
                self.start_ed2k_chunk_downloads(file_hash, ed2k_info, chunk_ids)
                    .await;

                Ok(())
            }
            Err(e) => {
                let error_msg =
                    format!("Ed2k connection failed: {} - {:?}", ed2k_info.server_url, e);
                warn!("{}", error_msg);
                self.on_source_failed(file_hash, &server_url_id, error_msg.clone())
                    .await;
                Err(error_msg)
            }
        }
    }

    /// Start downloading chunks from Ed2k network
    ///
    /// Groups 256KB chunks by their parent 9.28MB ed2k chunk, downloads each ed2k chunk once,
    /// then extracts all needed 256KB chunks from it.
    async fn start_ed2k_chunk_downloads(
        &self,
        file_hash: &str,
        ed2k_info: DownloadEd2kSourceInfo,
        chunk_ids: Vec<u32>,
    ) {
        let server_url_id = ed2k_info.server_url.clone();

        // Get chunk information for the assigned chunks
        let (chunks_info, chunks_map) = {
            let downloads = self.active_downloads.read().await;
            if let Some(download) = downloads.get(file_hash) {
                let chunks_info: Vec<ChunkInfo> = chunk_ids
                    .iter()
                    .filter_map(|&chunk_id| {
                        download
                            .chunks
                            .iter()
                            .find(|chunk| chunk.chunk_id == chunk_id)
                            .cloned()
                    })
                    .collect();

                let chunks_map: HashMap<u32, ChunkInfo> = chunks_info
                    .iter()
                    .map(|chunk| (chunk.chunk_id, chunk.clone()))
                    .collect();

                (chunks_info, chunks_map)
            } else {
                (Vec::new(), HashMap::new())
            }
        };

        if chunks_info.is_empty() {
            warn!("No chunks to download for Ed2k source");
            return;
        }

        // Group chunks by ed2k chunk to avoid duplicate downloads
        let grouped_by_ed2k = self.group_chunks_by_ed2k_chunk(&chunks_info);

        let file_hash_clone = file_hash.to_string();
        let ed2k_connections = Arc::clone(&self.ed2k_connections);
        let active_downloads = Arc::clone(&self.active_downloads);
        let chunks_map_clone = Arc::new(chunks_map);
        let transfer_event_bus = Arc::clone(&self.transfer_event_bus);
        let event_tx = self.event_tx.clone();
        let chunk_manager = self.chunk_manager.clone();

        // Spawn task to download chunks
        tokio::spawn(async move {
            // Limit concurrent ed2k chunk downloads (ed2k chunks are 9.28 MB each)
            let semaphore = Arc::new(tokio::sync::Semaphore::new(2));
            let mut handles = Vec::new();

            // Download each ed2k chunk once, then extract all needed chunks
            let mut sorted_ed2k_chunks: Vec<_> = grouped_by_ed2k.into_iter().collect();
            sorted_ed2k_chunks.sort_by_key(|(ed2k_id, _)| *ed2k_id);

            for (ed2k_chunk_id, mut our_chunk_infos) in sorted_ed2k_chunks {
                // Sort chunks by ID for ordered extraction
                our_chunk_infos.sort_by_key(|chunk| chunk.chunk_id);
                let permit = semaphore.clone().acquire_owned().await;
                let ed2k_connections_clone = Arc::clone(&ed2k_connections);
                let active_downloads_clone = Arc::clone(&active_downloads);
                let file_hash_inner = file_hash_clone.clone();
                let server_url_clone = server_url_id.clone();
                let ed2k_file_hash = ed2k_info.file_hash.clone();
                let chunks_map_clone = chunks_map_clone.clone();
                let transfer_event_bus_clone = Arc::clone(&transfer_event_bus);
                let event_tx_clone = event_tx.clone();
                let chunk_manager_clone = chunk_manager.clone();

                let handle = tokio::spawn(async move {
                    let _permit = permit; // Hold permit until task completes

                    // Get ed2k client from pool
                    let ed2k_client = {
                        let mut connections = ed2k_connections_clone.lock().await;
                        connections.remove(&server_url_clone)
                    };

                    if let Some(mut client) = ed2k_client {
                        // Calculate expected MD4 hash for the ed2k chunk
                        let expected_chunk_hash = {
                            let downloads_guard = active_downloads_clone.read().await;
                            if let Some(download) = downloads_guard.get(&file_hash_inner) {
                                if let Some(ed2k_hashes) = &download.ed2k_chunk_hashes {
                                    if let Some(hash) = ed2k_hashes.get(ed2k_chunk_id as usize) {
                                        hash.clone()
                                    } else {
                                        // Calculate MD4 hash from file hash and chunk ID
                                        let mut hasher = Md4::new();
                                        hasher.update(ed2k_file_hash.as_bytes());
                                        hasher.update(&ed2k_chunk_id.to_le_bytes());
                                        hex::encode(hasher.finalize())
                                    }
                                } else {
                                    // Calculate MD4 hash from file hash and chunk ID
                                    let mut hasher = Md4::new();
                                    hasher.update(ed2k_file_hash.as_bytes());
                                    hasher.update(&ed2k_chunk_id.to_le_bytes());
                                    hex::encode(hasher.finalize())
                                }
                            } else {
                                // Calculate MD4 hash from file hash and chunk ID
                                let mut hasher = Md4::new();
                                hasher.update(ed2k_file_hash.as_bytes());
                                hasher.update(&ed2k_chunk_id.to_le_bytes());
                                hex::encode(hasher.finalize())
                            }
                        };

                        match client
                            .download_chunk(&ed2k_file_hash, ed2k_chunk_id, &expected_chunk_hash)
                            .await
                        {
                            Ok(ed2k_chunk_data) => {
                                // Verify ed2k chunk size
                                if ed2k_chunk_data.len() != ED2K_CHUNK_SIZE
                                    && ed2k_chunk_data.len() < ED2K_CHUNK_SIZE
                                {
                                    error!(
                                        "Ed2k chunk {} size mismatch: expected at least {}, got {}",
                                        ed2k_chunk_id,
                                        ED2K_CHUNK_SIZE,
                                        ed2k_chunk_data.len()
                                    );

                                    // Mark chunks as failed
                                    let mut downloads = active_downloads_clone.write().await;
                                    if let Some(download) = downloads.get_mut(&file_hash_inner) {
                                        for chunk_info in &our_chunk_infos {
                                            download.failed_chunks.push_back(chunk_info.chunk_id);
                                        }
                                    }

                                    let mut connections = ed2k_connections_clone.lock().await;
                                    connections.insert(server_url_clone.clone(), client);
                                    return;
                                }

                                // Verify MD4 hash
                                let mut hasher = Md4::new();
                                hasher.update(&ed2k_chunk_data);
                                let computed_hash = hex::encode(hasher.finalize());

                                if !computed_hash.eq_ignore_ascii_case(&expected_chunk_hash) {
                                    warn!(
                                        "Ed2k chunk {} hash verification failed: expected {}, got {}",
                                        ed2k_chunk_id, expected_chunk_hash, computed_hash
                                    );
                                    // Mark chunks as failed
                                    let mut downloads = active_downloads_clone.write().await;
                                    if let Some(download) = downloads.get_mut(&file_hash_inner) {
                                        for chunk_info in &our_chunk_infos {
                                            download.failed_chunks.push_back(chunk_info.chunk_id);
                                        }
                                    }
                                    let mut connections = ed2k_connections_clone.lock().await;
                                    connections.insert(server_url_clone.clone(), client);
                                    return;
                                }

                                // Extract all needed chunks from the downloaded ed2k chunk
                                let download_start_ms = current_timestamp_ms();
                                let mut extracted_chunks = Vec::new();
                                let is_complete = {
                                    let mut downloads = active_downloads_clone.write().await;
                                    if let Some(download) = downloads.get_mut(&file_hash_inner) {
                                        for chunk_info in &our_chunk_infos {
                                            let offset_within_ed2k =
                                                chunk_info.offset % ED2K_CHUNK_SIZE as u64;
                                            let start = offset_within_ed2k as usize;
                                            let end = std::cmp::min(
                                                start + chunk_info.size,
                                                ed2k_chunk_data.len(),
                                            );

                                            if end <= ed2k_chunk_data.len() {
                                                let chunk_data =
                                                    ed2k_chunk_data[start..end].to_vec();

                                                // Verify SHA-256 hash for the extracted chunk
                                                if let Err((expected, actual)) =
                                                    verify_chunk_integrity(chunk_info, &chunk_data)
                                                {
                                                    warn!(
                                                        "ED2K chunk {} hash verification failed: expected {}, got {}",
                                                        chunk_info.chunk_id, expected, actual
                                                    );
                                                    download
                                                        .failed_chunks
                                                        .push_back(chunk_info.chunk_id);

                                                    // Emit ChunkFailed event
                                                    let error_msg = format!(
                                                        "Chunk hash mismatch: expected {}, got {}",
                                                        expected, actual
                                                    );
                                                    let current_timestamp = current_timestamp_ms();

                                                    transfer_event_bus_clone.emit_chunk_failed(
                                                        ChunkFailedEvent {
                                                            transfer_id: file_hash_inner.clone(),
                                                            chunk_id: chunk_info.chunk_id,
                                                            source_id: server_url_clone.clone(),
                                                            source_type: SourceType::P2p,
                                                            failed_at: current_timestamp,
                                                            error: error_msg,
                                                            retry_count: 0,
                                                            will_retry: true,
                                                            next_retry_at: None,
                                                        },
                                                    );

                                                    continue; // Skip this chunk
                                                }

                                                let completed_chunk = CompletedChunk {
                                                    chunk_id: chunk_info.chunk_id,
                                                    data: chunk_data.clone(),
                                                    source_id: server_url_clone.clone(),
                                                    completed_at: Instant::now(),
                                                };

                                                download
                                                    .completed_chunks
                                                    .insert(chunk_info.chunk_id, completed_chunk);

                                                extracted_chunks
                                                    .push((chunk_info.clone(), chunk_data));

                                                info!(
                                                    "Ed2k chunk {} extracted and verified from ed2k chunk {} (offset {})",
                                                    chunk_info.chunk_id, ed2k_chunk_id, offset_within_ed2k
                                                );
                                            } else {
                                                error!(
                                                    "Cannot extract chunk {} from ed2k chunk {}: offset {} + size {} exceeds ed2k chunk size {}",
                                                    chunk_info.chunk_id, ed2k_chunk_id, start, chunk_info.size, ed2k_chunk_data.len()
                                                );
                                                download
                                                    .failed_chunks
                                                    .push_back(chunk_info.chunk_id);
                                            }
                                        }
                                        download.completed_chunks.len() == download.chunks.len()
                                    } else {
                                        false
                                    }
                                };

                                // Emit events and store chunks to disk
                                for (chunk_info, chunk_data) in extracted_chunks {
                                    let completed_at = current_timestamp_ms();
                                    let download_duration_ms =
                                        completed_at.saturating_sub(download_start_ms);

                                    transfer_event_bus_clone.emit_chunk_completed(
                                        ChunkCompletedEvent {
                                            transfer_id: file_hash_inner.clone(),
                                            chunk_id: chunk_info.chunk_id,
                                            chunk_size: chunk_info.size,
                                            source_id: server_url_clone.clone(),
                                            source_type: SourceType::P2p,
                                            completed_at,
                                            download_duration_ms,
                                            verified: true,
                                        },
                                    );

                                    let _ = event_tx_clone.send(MultiSourceEvent::ChunkCompleted {
                                        file_hash: file_hash_inner.clone(),
                                        chunk_id: chunk_info.chunk_id,
                                        peer_id: server_url_clone.clone(),
                                    });

                                    // Store chunk to disk
                                    let data_for_disk = chunk_data;
                                    let file_hash_for_disk = file_hash_inner.clone();
                                    let chunk_id_for_disk = chunk_info.chunk_id;
                                    let chunk_manager_for_disk = chunk_manager_clone.clone();

                                    tokio::spawn(async move {
                                        let chunks_dir = std::path::Path::new("./chunks");
                                        if !chunks_dir.exists() {
                                            let _ = std::fs::create_dir_all(chunks_dir);
                                        }

                                        let file_dir = chunks_dir.join(&file_hash_for_disk);
                                        if !file_dir.exists() {
                                            let _ = std::fs::create_dir_all(&file_dir);
                                        }

                                        let chunk_path = file_dir
                                            .join(format!("chunk_{}.dat", chunk_id_for_disk));
                                        if let Err(e) =
                                            tokio::fs::write(&chunk_path, &data_for_disk).await
                                        {
                                            warn!(
                                                "Failed to write ED2K chunk {} to disk: {}",
                                                chunk_id_for_disk, e
                                            );
                                        } else {
                                            let metadata_path = file_dir
                                                .join(format!("chunk_{}.meta", chunk_id_for_disk));
                                            let metadata = serde_json::json!({
                                                "chunk_id": chunk_id_for_disk,
                                                "size": data_for_disk.len(),
                                                "stored_at": std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_secs(),
                                                "file_hash": file_hash_for_disk,
                                                "source_type": "ed2k"
                                            });
                                            let _ = tokio::fs::write(
                                                &metadata_path,
                                                serde_json::to_string_pretty(&metadata).unwrap(),
                                            )
                                            .await;

                                            // Store in ChunkManager for deduplication
                                            let mut hasher = Sha256::new();
                                            hasher.update(&data_for_disk);
                                            let content_hash = format!("{:x}", hasher.finalize());
                                            let _ = chunk_manager_for_disk
                                                .save_chunk(&content_hash, &data_for_disk);
                                        }
                                    });
                                }

                                if is_complete {
                                    if let Err(e) = Self::finalize_download_static(
                                        &active_downloads_clone,
                                        &file_hash_inner,
                                    )
                                    .await
                                    {
                                        error!("Failed to finalize ED2K download: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to download Ed2k chunk {}: {:?}", ed2k_chunk_id, e);

                                // Mark all chunks in this ed2k chunk as failed
                                let mut downloads = active_downloads_clone.write().await;
                                if let Some(download) = downloads.get_mut(&file_hash_inner) {
                                    for chunk_info in &our_chunk_infos {
                                        download.failed_chunks.push_back(chunk_info.chunk_id);
                                    }
                                }
                            }
                        }

                        // Return client to pool
                        let mut connections = ed2k_connections_clone.lock().await;
                        connections.insert(server_url_clone, client);
                    } else {
                        warn!("Ed2k client not found in connection pool");

                        // Mark all chunks as failed if client not available
                        let mut downloads = active_downloads_clone.write().await;
                        if let Some(download) = downloads.get_mut(&file_hash_inner) {
                            for chunk_info in &our_chunk_infos {
                                download.failed_chunks.push_back(chunk_info.chunk_id);
                            }
                        }
                    }
                });
                handles.push(handle);
            }

            // Wait for all downloads to complete
            for handle in handles {
                let _ = handle.await;
            }

            info!(
                "Ed2k source {} completed all assigned chunks",
                server_url_id
            );
        });
    }

    /// Group our chunks by ed2k chunk
    /// Returns: HashMap<ed2k_chunk_id, Vec<our_chunk_info>>
    fn group_chunks_by_ed2k_chunk(
        &self,
        our_chunks: &[ChunkInfo],
    ) -> std::collections::HashMap<u32, Vec<ChunkInfo>> {
        let mut grouped = std::collections::HashMap::new();

        for chunk in our_chunks {
            let (ed2k_chunk_id, _) = self.map_our_chunk_to_ed2k_chunk(chunk);
            grouped
                .entry(ed2k_chunk_id)
                .or_insert_with(Vec::new)
                .push(chunk.clone());
        }

        grouped
    }

    /// Static version of group_chunks_by_ed2k_chunk for use in spawned tasks
    fn group_chunks_by_ed2k_chunk_static(
        our_chunks: &[ChunkInfo],
    ) -> std::collections::HashMap<u32, Vec<ChunkInfo>> {
        let mut grouped = std::collections::HashMap::new();

        for chunk in our_chunks {
            let ed2k_chunk_id = (chunk.offset / ED2K_CHUNK_SIZE as u64) as u32;
            grouped
                .entry(ed2k_chunk_id)
                .or_insert_with(Vec::new)
                .push(chunk.clone());
        }

        grouped
    }

    /// Download entire ed2k chunk (9.28 MB) with MD4 verification
    async fn download_ed2k_chunk(
        &self,
        ed2k_connections: &Arc<Mutex<HashMap<String, Ed2kClient>>>,
        server_url: &str,
        file_hash: &str,
        ed2k_chunk_id: u32,
    ) -> Result<Vec<u8>, String> {
        // Get ed2k client from connection pool
        let mut ed2k_client = {
            let mut connections = ed2k_connections.lock().await;
            connections.remove(server_url).ok_or_else(|| {
                format!(
                    "Ed2k client not found in connection pool for {}",
                    server_url
                )
            })?
        };

        // Calculate expected chunk hash for verification
        let expected_chunk_hash = self.get_ed2k_chunk_hash(file_hash, ed2k_chunk_id).await?;

        // Download the ed2k chunk
        let result = ed2k_client
            .download_chunk(file_hash, ed2k_chunk_id, &expected_chunk_hash)
            .await;

        // Return client to pool regardless of outcome
        {
            let mut connections = ed2k_connections.lock().await;
            connections.insert(server_url.to_string(), ed2k_client);
        }

        // Process download result
        match result {
            Ok(data) => {
                // Verify MD4 hash
                if self
                    .verify_ed2k_chunk_hash(&data, &expected_chunk_hash)
                    .await?
                {
                    info!(
                        "Ed2k chunk {} downloaded and verified successfully",
                        ed2k_chunk_id
                    );
                    Ok(data)
                } else {
                    Err(format!(
                        "Ed2k chunk {} hash verification failed",
                        ed2k_chunk_id
                    ))
                }
            }
            Err(e) => Err(format!("Ed2k chunk download failed: {:?}", e)),
        }
    }

    /// Split ed2k chunk into our chunks and store them
    async fn split_and_store_ed2k_chunk(
        &self,
        active_downloads: &Arc<RwLock<HashMap<String, ActiveDownload>>>,
        file_hash: &str,
        server_url: &str,
        ed2k_chunk_id: u32,
        ed2k_chunk_data: &[u8],
        our_chunks: &[ChunkInfo],
    ) {
        for chunk in our_chunks {
            let (chunk_ed2k_id, offset_within_ed2k) = self.map_our_chunk_to_ed2k_chunk(chunk);

            // Ensure this chunk belongs to this ed2k chunk
            if chunk_ed2k_id != ed2k_chunk_id {
                continue;
            }

            // Extract our chunk data from ed2k chunk
            let start_offset = offset_within_ed2k as usize;
            let end_offset = std::cmp::min(start_offset + chunk.size, ed2k_chunk_data.len());

            if start_offset >= ed2k_chunk_data.len() {
                warn!(
                    "Chunk {} offset {} beyond ed2k chunk size {}",
                    chunk.chunk_id,
                    start_offset,
                    ed2k_chunk_data.len()
                );
                continue;
            }

            let our_chunk_data = ed2k_chunk_data[start_offset..end_offset].to_vec();

            // Verify our chunk size
            if our_chunk_data.len() != chunk.size {
                // Allow size mismatch for last chunk
                let is_last_chunk = self
                    .is_last_chunk(active_downloads, file_hash, chunk.chunk_id)
                    .await;
                if !is_last_chunk {
                    warn!(
                        "Chunk {} size mismatch: expected {}, got {}",
                        chunk.chunk_id,
                        chunk.size,
                        our_chunk_data.len()
                    );
                    continue;
                }
            }

            // Store completed chunk
            let mut downloads = active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                let completed_chunk = CompletedChunk {
                    chunk_id: chunk.chunk_id,
                    data: our_chunk_data,
                    source_id: server_url.to_string(),
                    completed_at: Instant::now(),
                };

                download
                    .completed_chunks
                    .insert(chunk.chunk_id, completed_chunk);
                info!(
                    "Ed2k chunk {} split and stored successfully (chunk_id: {})",
                    ed2k_chunk_id, chunk.chunk_id
                );
            }
        }
    }

    /// Static version of split_and_store_ed2k_chunk for use in spawned tasks
    async fn split_and_store_ed2k_chunk_static(
        active_downloads: &Arc<RwLock<HashMap<String, ActiveDownload>>>,
        file_hash: &str,
        server_url: &str,
        ed2k_chunk_id: u32,
        ed2k_chunk_data: &[u8],
        our_chunks: &[ChunkInfo],
    ) {
        for chunk in our_chunks {
            let chunk_ed2k_id = (chunk.offset / ED2K_CHUNK_SIZE as u64) as u32;
            let offset_within_ed2k = chunk.offset % ED2K_CHUNK_SIZE as u64;

            // Ensure this chunk belongs to this ed2k chunk
            if chunk_ed2k_id != ed2k_chunk_id {
                continue;
            }

            // Extract our chunk data from ed2k chunk
            let start_offset = offset_within_ed2k as usize;
            let end_offset = std::cmp::min(start_offset + chunk.size, ed2k_chunk_data.len());

            if start_offset >= ed2k_chunk_data.len() {
                warn!(
                    "Chunk {} offset {} beyond ed2k chunk size {}",
                    chunk.chunk_id,
                    start_offset,
                    ed2k_chunk_data.len()
                );
                continue;
            }

            let our_chunk_data = ed2k_chunk_data[start_offset..end_offset].to_vec();

            // Verify our chunk size
            if our_chunk_data.len() != chunk.size {
                // Allow size mismatch for last chunk
                let is_last_chunk =
                    Self::is_last_chunk_static(active_downloads, file_hash, chunk.chunk_id).await;
                if !is_last_chunk {
                    warn!(
                        "Chunk {} size mismatch: expected {}, got {}",
                        chunk.chunk_id,
                        chunk.size,
                        our_chunk_data.len()
                    );
                    continue;
                }
            }

            // Store completed chunk
            let mut downloads = active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                let completed_chunk = CompletedChunk {
                    chunk_id: chunk.chunk_id,
                    data: our_chunk_data,
                    source_id: server_url.to_string(),
                    completed_at: Instant::now(),
                };

                download
                    .completed_chunks
                    .insert(chunk.chunk_id, completed_chunk);
                info!(
                    "Ed2k chunk {} split and stored successfully (chunk_id: {})",
                    ed2k_chunk_id, chunk.chunk_id
                );
            }
        }
    }

    /// Get expected MD4 hash for ed2k chunk
    async fn get_ed2k_chunk_hash(
        &self,
        file_hash: &str,
        ed2k_chunk_id: u32,
    ) -> Result<String, String> {
        // First check if we have stored ED2K chunk hashes from metadata
        let downloads_guard = self.active_downloads.read().await;
        if let Some(download) = downloads_guard.get(file_hash) {
            if let Some(ed2k_hashes) = &download.ed2k_chunk_hashes {
                if let Some(chunk_hash) = ed2k_hashes.get(ed2k_chunk_id as usize) {
                    return Ok(chunk_hash.clone());
                }
            }

            // Check if we have the actual chunk data and can calculate the real MD4 hash
            if let Some(completed_chunk) = download.completed_chunks.get(&ed2k_chunk_id) {
                // Calculate MD4 hash of the actual chunk data
                let mut hasher = Md4::new();
                hasher.update(&completed_chunk.data);
                let result = hasher.finalize();
                return Ok(hex::encode(result));
            }
        }
        drop(downloads_guard);

        // Fallback: derive a hash from file hash and chunk ID
        // This should ideally never be reached if ED2K metadata is properly provided
        warn!(
            "Using fallback ED2K hash derivation for chunk {} of file {}",
            ed2k_chunk_id, file_hash
        );
        let mut hasher = Md4::new();
        hasher.update(file_hash.as_bytes());
        hasher.update(&ed2k_chunk_id.to_le_bytes());
        let result = hasher.finalize();
        Ok(hex::encode(result))
    }

    /// Verify MD4 hash of ed2k chunk
    async fn verify_ed2k_chunk_hash(
        &self,
        data: &[u8],
        expected_hash: &str,
    ) -> Result<bool, String> {
        let mut hasher = Md4::new();
        hasher.update(data);
        let computed = hex::encode(hasher.finalize());

        Ok(computed.eq_ignore_ascii_case(expected_hash))
    }

    /// Static version of verify_ed2k_chunk_hash
    async fn verify_ed2k_chunk_hash_static(
        data: &[u8],
        expected_hash: &str,
    ) -> Result<bool, String> {
        let mut hasher = Md4::new();
        hasher.update(data);
        let computed = hex::encode(hasher.finalize());

        Ok(computed.eq_ignore_ascii_case(expected_hash))
    }

    /// Check if a chunk is the last chunk in the file
    async fn is_last_chunk(
        &self,
        active_downloads: &Arc<RwLock<HashMap<String, ActiveDownload>>>,
        file_hash: &str,
        chunk_id: u32,
    ) -> bool {
        let downloads = active_downloads.read().await;
        if let Some(download) = downloads.get(file_hash) {
            let max_chunk_id = download
                .chunks
                .iter()
                .map(|c| c.chunk_id)
                .max()
                .unwrap_or(0);
            chunk_id == max_chunk_id
        } else {
            false
        }
    }

    /// Static version of is_last_chunk
    async fn is_last_chunk_static(
        active_downloads: &Arc<RwLock<HashMap<String, ActiveDownload>>>,
        file_hash: &str,
        chunk_id: u32,
    ) -> bool {
        let downloads = active_downloads.read().await;
        if let Some(download) = downloads.get(file_hash) {
            let max_chunk_id = download
                .chunks
                .iter()
                .map(|c| c.chunk_id)
                .max()
                .unwrap_or(0);
            chunk_id == max_chunk_id
        } else {
            false
        }
    }

    /// Verify chunk hash using internal hash system
    async fn verify_chunk_hash(
        &self,
        data: &[u8],
        file_hash: &str,
        chunk_id: u32,
    ) -> Result<bool, String> {
        // Simple hash verification using SHA-256 of data + file_hash + chunk_id
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.update(file_hash.as_bytes());
        hasher.update(&chunk_id.to_le_bytes());
        let computed = hex::encode(hasher.finalize());

        // Get the expected hash from stored chunk metadata
        let downloads_guard = self.active_downloads.read().await;
        if let Some(download) = downloads_guard.get(file_hash) {
            // Find the chunk info for this chunk_id
            if let Some(chunk_info) = download.chunks.iter().find(|c| c.chunk_id == chunk_id) {
                let expected_hash = &chunk_info.hash;
                let matches = computed == *expected_hash;

                if !matches {
                    debug!(
                        "Chunk hash verification failed: computed={}, expected={}",
                        computed, expected_hash
                    );
                }

                Ok(matches)
            } else {
                Err(format!(
                    "No chunk info found for chunk_id {} in file {}",
                    chunk_id, file_hash
                ))
            }
        } else {
            Err(format!("No active download found for file {}", file_hash))
        }
    }

    /// Store chunk data to disk for persistence and memory efficiency
    async fn store_chunk(
        &self,
        file_hash: &str,
        chunk_id: u32,
        data: Vec<u8>,
    ) -> Result<(), String> {
        // Create chunks directory if it doesn't exist
        let chunks_dir = std::path::Path::new("./chunks");
        if !chunks_dir.exists() {
            std::fs::create_dir_all(chunks_dir)
                .map_err(|e| format!("Failed to create chunks directory: {}", e))?;
        }

        // Create file-specific subdirectory
        let file_dir = chunks_dir.join(file_hash);
        if !file_dir.exists() {
            std::fs::create_dir_all(&file_dir)
                .map_err(|e| format!("Failed to create file directory: {}", e))?;
        }

        // Write chunk to file
        let chunk_path = file_dir.join(format!("chunk_{}.dat", chunk_id));
        tokio::fs::write(&chunk_path, &data)
            .await
            .map_err(|e| format!("Failed to write chunk {}: {}", chunk_id, e))?;

        info!(
            "Stored chunk {} for file {} ({} bytes) to disk",
            chunk_id,
            file_hash,
            data.len()
        );

        // Also store metadata about the chunk for later retrieval
        let metadata_path = file_dir.join(format!("chunk_{}.meta", chunk_id));
        let metadata = serde_json::json!({
            "chunk_id": chunk_id,
            "size": data.len(),
            "stored_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "file_hash": file_hash
        });

        tokio::fs::write(
            &metadata_path,
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .await
        .map_err(|e| format!("Failed to write chunk metadata {}: {}", chunk_id, e))?;

        Ok(())
    }

    /// Report chunk completion progress
    async fn report_chunk_complete(&self, file_hash: &str, chunk_id: u32) -> Result<(), String> {
        // Emit chunk completed event via TransferEventBus
        self.transfer_event_bus
            .emit_chunk_completed(ChunkCompletedEvent {
                transfer_id: file_hash.to_string(),
                chunk_id,
                chunk_size: 0, // Size unknown at this point
                source_id: "ed2k".to_string(),
                source_type: SourceType::P2p,
                completed_at: current_timestamp_ms(),
                download_duration_ms: 0,
                verified: true,
            });

        // Also emit legacy internal event for backwards compatibility
        let _ = self.event_tx.send(MultiSourceEvent::ChunkCompleted {
            file_hash: file_hash.to_string(),
            chunk_id,
            peer_id: "ed2k".to_string(),
        });
        Ok(())
    }

    /// Handle source connection success
    async fn on_source_connected(&self, file_hash: &str, source_id: &str, chunk_ids: Vec<u32>) {
        info!("Source {} connected for file {}", source_id, file_hash);

        let now_ms = current_timestamp_ms();
        let now_secs = now_ms / 1000;

        // Determine source type from source_id pattern
        let source_type = if source_id.starts_with("http://") || source_id.starts_with("https://") {
            SourceType::Http
        } else if source_id.starts_with("ftp://") {
            SourceType::Ftp
        } else if source_id.starts_with("magnet:") {
            SourceType::BitTorrent
        } else {
            SourceType::P2p
        };

        // Update source status
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(source_id) {
                    assignment.status = SourceStatus::Connected;
                    assignment.connected_at = Some(now_secs);
                    assignment.last_activity = Some(now_secs);
                }
            }
        }

        // Emit event via TransferEventBus
        self.transfer_event_bus
            .emit_source_connected(SourceConnectedEvent {
                transfer_id: file_hash.to_string(),
                source_id: source_id.to_string(),
                source_type: source_type.clone(),
                source_info: SourceInfo {
                    id: source_id.to_string(),
                    source_type,
                    address: source_id.to_string(),
                    reputation: None,
                    estimated_speed_bps: None,
                    latency_ms: None,
                    location: None,
                },
                connected_at: now_ms,
                assigned_chunks: chunk_ids.iter().map(|&id| id).collect(),
            });

        // Also emit legacy internal event for backwards compatibility
        let _ = self.event_tx.send(MultiSourceEvent::PeerConnected {
            file_hash: file_hash.to_string(),
            peer_id: source_id.to_string(),
        });
    }

    /// Handle source connection failure
    async fn on_source_failed(&self, file_hash: &str, source_id: &str, error: String) {
        warn!(
            "Source {} failed for file {}: {}",
            source_id, file_hash, error
        );

        let now_ms = current_timestamp_ms();

        // Determine source type from source_id pattern
        let source_type = if source_id.starts_with("http://") || source_id.starts_with("https://") {
            SourceType::Http
        } else if source_id.starts_with("ftp://") {
            SourceType::Ftp
        } else if source_id.starts_with("magnet:") {
            SourceType::BitTorrent
        } else {
            SourceType::P2p
        };

        // Update source status and reassign chunks
        let (reassign_chunks, chunks_completed) = {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(source_id) {
                    assignment.status = SourceStatus::Failed;
                    let chunks = assignment.chunks.clone();
                    let completed = download.completed_chunks.len() as u32;

                    // Add failed chunks back to retry queue
                    for chunk_id in &chunks {
                        download.failed_chunks.push_back(*chunk_id);
                    }

                    (chunks, completed)
                } else {
                    (Vec::new(), 0)
                }
            } else {
                (Vec::new(), 0)
            }
        };

        // Determine disconnect reason from error message
        let disconnect_reason = if error.contains("timeout") || error.contains("Timeout") {
            DisconnectReason::Timeout
        } else if error.contains("network")
            || error.contains("Network")
            || error.contains("connection")
        {
            DisconnectReason::NetworkError
        } else if error.contains("unavailable") || error.contains("not found") {
            DisconnectReason::SourceUnavailable
        } else if error.contains("protocol") || error.contains("Protocol") {
            DisconnectReason::ProtocolError
        } else {
            DisconnectReason::Other(error.clone())
        };

        // Emit event via TransferEventBus
        self.transfer_event_bus
            .emit_source_disconnected(SourceDisconnectedEvent {
                transfer_id: file_hash.to_string(),
                source_id: source_id.to_string(),
                source_type,
                disconnected_at: now_ms,
                reason: disconnect_reason,
                chunks_completed,
                will_retry: !reassign_chunks.is_empty(),
            });

        // Also emit legacy internal event for backwards compatibility
        let _ = self.event_tx.send(MultiSourceEvent::PeerFailed {
            file_hash: file_hash.to_string(),
            peer_id: source_id.to_string(),
            error,
        });

        // Try to reassign chunks to other sources or retry later
        if !reassign_chunks.is_empty() {
            let _ = self.command_tx.send(MultiSourceCommand::RetryFailedChunks {
                file_hash: file_hash.to_string(),
            });
        }
    }

    async fn connect_to_peer(
        &self,
        file_hash: &str,
        peer_id: String,
        chunk_ids: Vec<u32>,
    ) -> Result<(), String> {
        // This method is now replaced by start_p2p_connection
        // Keeping for backwards compatibility but delegating to new method
        self.start_p2p_connection(file_hash, peer_id, chunk_ids)
            .await
    }

    async fn on_peer_connected(&self, file_hash: &str, peer_id: &str, chunk_ids: Vec<u32>) {
        // Delegate to unified source connection handler
        self.on_source_connected(file_hash, peer_id, chunk_ids)
            .await
    }

    async fn on_peer_failed(&self, file_hash: &str, peer_id: &str, error: String) {
        // Delegate to unified source failure handler
        self.on_source_failed(file_hash, peer_id, error).await
    }

    async fn start_chunk_requests(&self, file_hash: &str, peer_id: &str, chunk_ids: Vec<u32>) {
        info!(
            "Starting chunk requests from peer {} for {} chunks",
            peer_id,
            chunk_ids.len()
        );

        // Send file request first
        let metadata = {
            let downloads = self.active_downloads.read().await;
            downloads.get(file_hash).map(|d| d.file_metadata.clone())
        };

        if let Some(metadata) = metadata {
            let file_request = WebRTCFileRequest {
                file_hash: metadata.merkle_root.clone(),
                file_name: metadata.file_name.clone(),
                file_size: metadata.file_size,
                requester_peer_id: self.dht_service.get_peer_id().await,
                recipient_public_key: None, // No encryption for basic multi-source downloads
            };

            if let Err(e) = self
                .webrtc_service
                .send_file_request(peer_id.to_string(), file_request)
                .await
            {
                warn!("Failed to send file request to peer {}: {}", peer_id, e);
                self.on_peer_failed(file_hash, peer_id, format!("File request failed: {}", e))
                    .await;
                return;
            }

            // Update peer status to downloading
            {
                let mut downloads = self.active_downloads.write().await;
                if let Some(download) = downloads.get_mut(file_hash) {
                    if let Some(assignment) = download.source_assignments.get_mut(peer_id) {
                        assignment.status = SourceStatus::Downloading;
                    }
                }
            }

            // The WebRTC service will handle the actual chunk requests and responses
            // We just need to track the progress
        }
    }

    async fn handle_cancel_download(&self, file_hash: &str) {
        info!("Cancelling download for file: {}", file_hash);

        let download = {
            let mut downloads = self.active_downloads.write().await;
            downloads.remove(file_hash)
        };

        if let Some(download) = download {
            // Close connections based on source type
            for (source_id, assignment) in download.source_assignments.iter() {
                match &assignment.source {
                    DownloadSource::P2p(_) => {
                        // Close P2P/WebRTC connections
                        let _ = self
                            .webrtc_service
                            .close_connection(source_id.clone())
                            .await;
                    }
                    DownloadSource::Ftp(_) => {
                        // Close all FTP connections for this server
                        let mut connections = self.ftp_connections.lock().await;
                        if let Some(streams) = connections.remove(source_id) {
                            for mut ftp_stream in streams {
                                let _ = self.ftp_downloader.disconnect(&mut ftp_stream).await;
                            }
                        }
                    }
                    DownloadSource::Http(_) => {
                        // HTTP connections are typically closed automatically
                        // No explicit cleanup needed for HTTP
                    }
                    DownloadSource::Ed2k(_) => {
                        // Close Ed2k connections
                        let mut connections = self.ed2k_connections.lock().await;
                        if let Some(mut ed2k_client) = connections.remove(source_id) {
                            let _ = ed2k_client.disconnect().await;
                        }
                    }
                    DownloadSource::BitTorrent(bt_info) => {
                        if let Some(info_hash) =
                            Self::extract_info_hash_from_magnet(&bt_info.magnet_uri)
                        {
                            if let Err(e) = self
                                .bittorrent_handler
                                .cancel_torrent(&info_hash, false)
                                .await
                            {
                                warn!("Failed to cancel BitTorrent download {}: {}", info_hash, e);
                            }
                        }
                    }
                }
            }
        }
    }

    async fn handle_retry_failed_chunks(&self, file_hash: &str) -> Result<(), String> {
        info!("Retrying failed chunks for file: {}", file_hash);

        let failed_chunks = {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                let mut chunks = Vec::new();
                while let Some(chunk_id) = download.failed_chunks.pop_front() {
                    chunks.push(chunk_id);
                    if chunks.len() >= 10 {
                        break; // Limit retry batch size
                    }
                }
                chunks
            } else {
                return Err("Download not found".to_string());
            }
        };

        if failed_chunks.is_empty() {
            return Ok(());
        }

        // Try to find available sources for retry.
        //
        // IMPORTANT:
        // We must actively re-trigger downloads for the failed chunks.
        // Merely pushing chunk IDs back into an assignment list is not sufficient for
        // some protocols (FTP in particular), since chunk downloads are spawned as tasks
        // and do not poll assignment queues continuously.
        let available_sources = {
            let downloads = self.active_downloads.read().await;
            if let Some(download) = downloads.get(file_hash) {
                download
                    .source_assignments
                    .iter()
                    .filter(|(_, assignment)| {
                        matches!(
                            assignment.status,
                            SourceStatus::Connected | SourceStatus::Downloading
                        )
                    })
                    .map(|(source_id, assignment)| (source_id.clone(), assignment.source.clone()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };

        if available_sources.is_empty() {
            warn!("No available sources for retry");
            return Err("No available sources for retry".to_string());
        }

        // Prefer retrying via a connected FTP source if one exists (FTP-only transfers depend on this).
        for (_source_id, source) in &available_sources {
            if let DownloadSource::Ftp(ftp_info) = source {
                // Kick off a new FTP chunk download wave for these failed chunks.
                self.start_ftp_chunk_downloads(file_hash, ftp_info.clone(), failed_chunks.clone())
                    .await;
                return Ok(());
            }
        }

        // Fallback: if no FTP source exists, keep the previous behavior of reassigning chunks
        // to connected sources (best-effort). This supports P2P/HTTP flows that may poll queues elsewhere.
        let available_peer_ids: Vec<String> =
            available_sources.iter().map(|(id, _)| id.clone()).collect();
        for (index, chunk_id) in failed_chunks.iter().enumerate() {
            let peer_index = index % available_peer_ids.len();
            let peer_id = &available_peer_ids[peer_index];
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(peer_id) {
                    assignment.chunks.push(*chunk_id);
                }
            }
        }

        Ok(())
    }

    fn calculate_progress(&self, download: &ActiveDownload) -> MultiSourceProgress {
        let total_chunks = download.chunks.len() as u32;
        let completed_chunks = download.completed_chunks.len() as u32;
        let downloaded_size = download
            .completed_chunks
            .values()
            .map(|chunk| chunk.data.len() as u64)
            .sum();

        let active_sources = download
            .source_assignments
            .values()
            .filter(|assignment| {
                matches!(
                    assignment.status,
                    SourceStatus::Connected | SourceStatus::Downloading
                )
            })
            .count();

        let duration = download.start_time.elapsed();
        // Use secs_f64 to capture sub-second durations instead of integer secs which can be 0 for <1s
        let download_speed_bps = if duration.as_secs_f64() > 0.0 {
            downloaded_size as f64 / duration.as_secs_f64()
        } else {
            0.0
        };

        let eta_seconds = if download_speed_bps > 0.0 {
            let remaining_bytes = download.file_metadata.file_size - downloaded_size;
            Some((remaining_bytes as f64 / download_speed_bps) as u32)
        } else {
            None
        };

        MultiSourceProgress {
            file_hash: download.file_metadata.merkle_root.clone(),
            file_name: download.file_metadata.file_name.clone(),
            total_size: download.file_metadata.file_size,
            downloaded_size,
            total_chunks,
            completed_chunks,
            active_sources,
            download_speed_bps,
            eta_seconds,
            source_assignments: download.source_assignments.values().cloned().collect(),
        }
    }

    async fn spawn_download_monitor(&self, file_hash: String) {
        let downloads = self.active_downloads.clone();
        let event_tx = self.event_tx.clone();
        let transfer_event_bus = self.transfer_event_bus.clone();
        let analytics_service = self.analytics_service.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            let start_time = std::time::Instant::now();

            loop {
                interval.tick().await;

                let (progress, download_info, sources_used) = {
                    let downloads = downloads.read().await;
                    if let Some(download) = downloads.get(&file_hash) {
                        let progress = Self::calculate_progress_static(download);
                        let info = (
                            download.file_metadata.file_name.clone(),
                            download.file_metadata.file_size,
                            download.output_path.clone(),
                        );

                        // Calculate source statistics from completed chunks
                        let now_secs = current_timestamp_ms() / 1000;
                        let sources: Vec<SourceSummary> = download
                            .source_assignments
                            .iter()
                            .map(|(source_id, assignment)| {
                                let source_type = match &assignment.source {
                                    DownloadSource::P2p(_) => SourceType::P2p,
                                    DownloadSource::Http(_) => SourceType::Http,
                                    DownloadSource::Ftp(_) => SourceType::Ftp,
                                    DownloadSource::BitTorrent(_) => SourceType::BitTorrent,
                                    DownloadSource::Ed2k(_) => SourceType::P2p,
                                };

                                // Count chunks and bytes provided by this source
                                let mut chunks_provided = 0u32;
                                let mut bytes_provided = 0u64;
                                for completed_chunk in download.completed_chunks.values() {
                                    if completed_chunk.source_id == *source_id {
                                        chunks_provided += 1;
                                        // Find chunk size from chunks metadata
                                        if let Some(chunk_info) = download
                                            .chunks
                                            .iter()
                                            .find(|c| c.chunk_id == completed_chunk.chunk_id)
                                        {
                                            bytes_provided += chunk_info.size as u64;
                                        }
                                    }
                                }

                                // Calculate connection duration
                                let connection_duration_seconds =
                                    if let Some(connected_at_ms) = assignment.connected_at {
                                        let connected_at_secs = connected_at_ms / 1000;
                                        now_secs.saturating_sub(connected_at_secs)
                                    } else {
                                        0
                                    };

                                // Calculate average speed
                                let average_speed_bps = if connection_duration_seconds > 0 {
                                    bytes_provided as f64 / connection_duration_seconds as f64
                                } else {
                                    0.0
                                };

                                SourceSummary {
                                    source_id: source_id.clone(),
                                    source_type,
                                    chunks_provided,
                                    bytes_provided,
                                    average_speed_bps,
                                    connection_duration_seconds,
                                }
                            })
                            .collect();

                        (Some(progress), Some(info), sources)
                    } else {
                        (None, None, Vec::new())
                    }
                };

                if let Some(progress) = progress {
                    // Check if download is complete
                    if progress.completed_chunks >= progress.total_chunks {
                        let (file_name, file_size, output_path) = download_info.unwrap_or_default();
                        let duration = start_time.elapsed();
                        let avg_speed = if duration.as_secs_f64() > 0.0 {
                            file_size as f64 / duration.as_secs_f64()
                        } else {
                            0.0
                        };

                        // Finalize download
                        if let Err(e) = Self::finalize_download_static(&downloads, &file_hash).await
                        {
                            // Emit failed event via TransferEventBus with analytics
                            transfer_event_bus
                                .emit_failed_with_analytics(
                                    TransferFailedEvent {
                                        transfer_id: file_hash.clone(),
                                        file_hash: file_hash.clone(),
                                        protocol: "WebRTC".to_string(),
                                        failed_at: current_timestamp_ms(),
                                        error: format!("Failed to finalize download: {}", e),
                                        error_category: ErrorCategory::Filesystem,
                                        downloaded_bytes: progress.downloaded_size,
                                        total_bytes: progress.total_size,
                                        retry_possible: false,
                                    },
                                    &analytics_service,
                                )
                                .await;
                            // Also emit legacy internal event
                            let _ = event_tx.send(MultiSourceEvent::DownloadFailed {
                                file_hash: file_hash.clone(),
                                error: format!("Failed to finalize download: {}", e),
                            });
                        } else {
                            // Emit completed event via TransferEventBus with analytics
                            transfer_event_bus
                                .emit_completed_with_analytics(
                                    TransferCompletedEvent {
                                        transfer_id: file_hash.clone(),
                                        file_hash: file_hash.clone(),
                                        protocol: "WebRTC".to_string(),
                                        file_name,
                                        file_size,
                                        output_path,
                                        completed_at: current_timestamp_ms(),
                                        duration_seconds: duration.as_secs(),
                                        average_speed_bps: avg_speed,
                                        total_chunks: progress.total_chunks,
                                        sources_used,
                                    },
                                    &analytics_service,
                                )
                                .await;
                            // Also emit legacy internal event
                            let _ = event_tx.send(MultiSourceEvent::DownloadCompleted {
                                file_hash: file_hash.clone(),
                                output_path: String::new(),
                                duration_secs: duration.as_secs(),
                                average_speed_bps: avg_speed,
                            });
                        }
                        break;
                    }

                    // Emit progress update via TransferEventBus with analytics
                    transfer_event_bus
                        .emit_progress_with_analytics(
                            TransferProgressEvent {
                                transfer_id: file_hash.clone(),
                                protocol: "WebRTC".to_string(),
                                downloaded_bytes: progress.downloaded_size,
                                total_bytes: progress.total_size,
                                completed_chunks: progress.completed_chunks,
                                total_chunks: progress.total_chunks,
                                progress_percentage: calculate_progress(
                                    progress.downloaded_size,
                                    progress.total_size,
                                ),
                                download_speed_bps: progress.download_speed_bps,
                                upload_speed_bps: 0.0,
                                eta_seconds: progress.eta_seconds,
                                active_sources: progress.active_sources,
                                timestamp: current_timestamp_ms(),
                            },
                            &analytics_service,
                        )
                        .await;

                    // Also emit legacy internal event
                    let _ = event_tx.send(MultiSourceEvent::ProgressUpdate {
                        file_hash: file_hash.clone(),
                        progress,
                    });
                } else {
                    // Download was cancelled or removed
                    break;
                }
            }
        });
    }

    fn calculate_progress_static(download: &ActiveDownload) -> MultiSourceProgress {
        let total_chunks = download.chunks.len() as u32;
        let completed_chunks = download.completed_chunks.len() as u32;
        let downloaded_size = download
            .completed_chunks
            .values()
            .map(|chunk| chunk.data.len() as u64)
            .sum();

        let active_sources = download
            .source_assignments
            .values()
            .filter(|assignment| {
                matches!(
                    assignment.status,
                    SourceStatus::Connected | SourceStatus::Downloading
                )
            })
            .count();

        let duration = download.start_time.elapsed();
        // Use secs_f64 to capture sub-second durations instead of integer secs which can be 0 for <1s
        let download_speed_bps = if duration.as_secs_f64() > 0.0 {
            downloaded_size as f64 / duration.as_secs_f64()
        } else {
            0.0
        };

        let eta_seconds = if download_speed_bps > 0.0 {
            let remaining_bytes = download.file_metadata.file_size - downloaded_size;
            Some((remaining_bytes as f64 / download_speed_bps) as u32)
        } else {
            None
        };

        MultiSourceProgress {
            file_hash: download.file_metadata.merkle_root.clone(),
            file_name: download.file_metadata.file_name.clone(),
            total_size: download.file_metadata.file_size,
            downloaded_size,
            total_chunks,
            completed_chunks,
            active_sources,
            download_speed_bps,
            eta_seconds,
            source_assignments: download.source_assignments.values().cloned().collect(),
        }
    }

    /// Extract info hash from a magnet URI
    fn extract_info_hash_from_magnet(magnet: &str) -> Option<String> {
        magnet.split('&').find_map(|part| {
            if let Some(rest) = part.strip_prefix("magnet:?xt=urn:btih:") {
                Some(rest.to_string())
            } else if let Some(rest) = part.strip_prefix("xt=urn:btih:") {
                Some(rest.to_string())
            } else {
                None
            }
        })
    }

    /// Finalize a completed download
    async fn finalize_download(&self, file_hash: &str) -> Result<(), String> {
        Self::finalize_download_static(&self.active_downloads, file_hash).await?;
        // Remove persisted download state since download is complete
        if let Err(e) = self.remove_download_state(file_hash).await {
            warn!("Failed to remove download state for {}: {}", file_hash, e);
        }
        Ok(())
    }

    async fn finalize_download_static(
        downloads: &Arc<RwLock<HashMap<String, ActiveDownload>>>,
        file_hash: &str,
    ) -> Result<(), String> {
        let download = {
            let mut downloads = downloads.write().await;
            downloads.remove(file_hash)
        };

        if let Some(download) = download {
            // Assemble file from chunks
            // Stream assembly directly to disk (avoid allocating a full-file Vec<u8>).
            let output_path = std::path::Path::new(&download.output_path);
            if let Some(parent) = output_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("Failed to create output directory: {}", e))?;
            }

            use std::io::SeekFrom;
            use tokio::io::{AsyncSeekExt, AsyncWriteExt};

            let mut file = tokio::fs::File::create(output_path)
                .await
                .map_err(|e| format!("Failed to create output file: {}", e))?;

            // Pre-allocate file size to reduce fragmentation and improve write performance.
            file.set_len(download.file_metadata.file_size)
                .await
                .map_err(|e| format!("Failed to set output file size: {}", e))?;

            for chunk_info in &download.chunks {
                let completed_chunk = download
                    .completed_chunks
                    .get(&chunk_info.chunk_id)
                    .ok_or_else(|| {
                        format!("Missing chunk {} during finalization", chunk_info.chunk_id)
                    })?;

                file.seek(SeekFrom::Start(chunk_info.offset))
                    .await
                    .map_err(|e| format!("Failed to seek output file: {}", e))?;

                file.write_all(&completed_chunk.data)
                    .await
                    .map_err(|e| format!("Failed to write chunk {}: {}", chunk_info.chunk_id, e))?;
            }

            file.flush()
                .await
                .map_err(|e| format!("Failed to flush output file: {}", e))?;

            let duration = download.start_time.elapsed();
            let average_speed = download.file_metadata.file_size as f64 / duration.as_secs_f64();

            info!(
                "Download completed: {} ({} bytes) in {:.2}s at {:.2} KB/s",
                download.file_metadata.file_name,
                download.file_metadata.file_size,
                duration.as_secs_f64(),
                average_speed / 1024.0
            );

            Ok(())
        } else {
            Err("Download not found".to_string())
        }
    }

    pub async fn drain_events(&self, max_events: usize) -> Vec<MultiSourceEvent> {
        let mut events = Vec::new();
        let mut event_rx = self.event_rx.lock().await;

        for _ in 0..max_events {
            match event_rx.try_recv() {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }

        events
    }

    /// Update proxy latency information for optimization
    pub async fn update_proxy_latency(&self, proxy_id: String, latency_ms: Option<u64>) {
        if let Some(proxy_service) = &self.proxy_latency_service {
            let mut service = proxy_service.lock().await;
            service.update_proxy_latency(
                proxy_id.clone(),
                latency_ms,
                crate::proxy_latency::ProxyStatus::Online,
            );
            info!("Updated proxy latency for proxy: {}", proxy_id);
        } else {
            warn!("Proxy latency service not available for update");
        }
    }

    /// Get current proxy optimization status
    pub async fn get_proxy_optimization_status(&self) -> serde_json::Value {
        if let Some(proxy_service) = &self.proxy_latency_service {
            let service = proxy_service.lock().await;
            let enabled = service.should_use_proxy_routing();
            let best_proxy = service.get_best_proxy();

            serde_json::json!({
                "enabled": enabled,
                "best_proxy": best_proxy,
                "status": "Proxy latency tracking active"
            })
        } else {
            serde_json::json!({
                "enabled": false,
                "status": "Proxy latency service not available"
            })
        }
    }

    /// Get statistics about FTP connections and performance
    pub async fn get_ftp_statistics(&self) -> serde_json::Value {
        let connection_count = {
            let connections = self.ftp_connections.lock().await;
            connections.len()
        };

        let active_ftp_downloads = {
            let downloads = self.active_downloads.read().await;
            downloads
                .values()
                .map(|download| {
                    download
                        .source_assignments
                        .values()
                        .filter(|assignment| matches!(assignment.source, DownloadSource::Ftp(_)))
                        .count()
                })
                .sum::<usize>()
        };

        serde_json::json!({
            "active_connections": connection_count,
            "active_ftp_downloads": active_ftp_downloads,
            "ftp_enabled": true
        })
    }

    /// Cleanup all resources (FTP connections, etc.) when service shuts down
    pub async fn cleanup(&self) {
        info!("Cleaning up MultiSourceDownloadService resources");

        // Close all active FTP connections
        let mut connections = self.ftp_connections.lock().await;
        let connection_urls: Vec<String> = connections.keys().cloned().collect();

        for url in connection_urls {
            if let Some(streams) = connections.remove(&url) {
                for mut ftp_stream in streams {
                    if let Err(e) = self.ftp_downloader.disconnect(&mut ftp_stream).await {
                        warn!("Failed to disconnect FTP connection {}: {}", url, e);
                    } else {
                        info!("Closed FTP connection: {}", url);
                    }
                }
            }
        }

        // Cancel all active downloads
        let active_hashes: Vec<String> = {
            let downloads = self.active_downloads.read().await;
            downloads.keys().cloned().collect()
        };

        for file_hash in active_hashes {
            self.handle_cancel_download(&file_hash).await;
        }

        info!("MultiSourceDownloadService cleanup completed");
    }

    /// Map our chunk ID to ed2k chunk ID and offset within that ed2k chunk (Person 4 function)
    fn map_our_chunk_to_ed2k_chunk(&self, our_chunk: &ChunkInfo) -> (u32, u64) {
        let ed2k_chunk_id = (our_chunk.offset / ED2K_CHUNK_SIZE as u64) as u32;
        let offset_within_ed2k = our_chunk.offset % ED2K_CHUNK_SIZE as u64;
        (ed2k_chunk_id, offset_within_ed2k)
    }

    /// Map ed2k chunk ID to range of our chunk IDs (Person 4 function)  
    fn map_ed2k_chunk_to_our_chunks(&self, ed2k_chunk_id: u32, total_file_size: u64) -> Vec<u32> {
        let ed2k_chunk_start_offset = ed2k_chunk_id as u64 * ED2K_CHUNK_SIZE as u64;
        let ed2k_chunk_end_offset = std::cmp::min(
            ed2k_chunk_start_offset + ED2K_CHUNK_SIZE as u64,
            total_file_size,
        );

        let start_chunk_id = (ed2k_chunk_start_offset / 256_000) as u32;
        let end_chunk_id = ((ed2k_chunk_end_offset + 256_000 - 1) / 256_000) as u32;

        (start_chunk_id..end_chunk_id).collect()
    }

    /// Check if a chunk exists on disk for the given file hash and chunk ID
    pub async fn chunk_exists_on_disk(&self, file_hash: &str, chunk_id: u32) -> bool {
        let chunks_dir = std::path::Path::new("./chunks");
        let file_dir = chunks_dir.join(file_hash);
        let chunk_path = file_dir.join(format!("chunk_{}.dat", chunk_id));
        let metadata_path = file_dir.join(format!("chunk_{}.meta", chunk_id));

        chunk_path.exists() && metadata_path.exists()
    }

    /// Load a chunk from disk storage with validation
    pub async fn load_chunk_from_disk(
        &self,
        file_hash: &str,
        chunk_id: u32,
    ) -> Result<Vec<u8>, String> {
        let chunks_dir = std::path::Path::new("./chunks");
        let file_dir = chunks_dir.join(file_hash);
        let chunk_path = file_dir.join(format!("chunk_{}.dat", chunk_id));
        let metadata_path = file_dir.join(format!("chunk_{}.meta", chunk_id));

        // Check if files exist
        if !chunk_path.exists() || !metadata_path.exists() {
            return Err(format!(
                "Chunk {} not found on disk for file {}",
                chunk_id, file_hash
            ));
        }

        // Read metadata first
        let metadata_content = tokio::fs::read_to_string(&metadata_path)
            .await
            .map_err(|e| format!("Failed to read chunk metadata: {}", e))?;

        let metadata: serde_json::Value = serde_json::from_str(&metadata_content)
            .map_err(|e| format!("Failed to parse chunk metadata: {}", e))?;

        // Validate metadata
        let expected_file_hash = metadata["file_hash"]
            .as_str()
            .ok_or("Missing file_hash in metadata")?;
        let expected_chunk_id = metadata["chunk_id"]
            .as_u64()
            .ok_or("Missing chunk_id in metadata")? as u32;
        let expected_size = metadata["size"]
            .as_u64()
            .ok_or("Missing size in metadata")? as usize;

        if expected_file_hash != file_hash {
            return Err(format!(
                "File hash mismatch in metadata: expected {}, got {}",
                file_hash, expected_file_hash
            ));
        }

        if expected_chunk_id != chunk_id {
            return Err(format!(
                "Chunk ID mismatch in metadata: expected {}, got {}",
                chunk_id, expected_chunk_id
            ));
        }

        // Read chunk data
        let chunk_data = tokio::fs::read(&chunk_path)
            .await
            .map_err(|e| format!("Failed to read chunk data: {}", e))?;

        // Validate size
        if chunk_data.len() != expected_size {
            return Err(format!(
                "Chunk size mismatch: expected {}, got {}",
                expected_size,
                chunk_data.len()
            ));
        }

        // Get the expected chunk info to validate hash
        let downloads = self.active_downloads.read().await;
        if let Some(download) = downloads.get(file_hash) {
            if let Some(chunk_info) = download.chunks.iter().find(|c| c.chunk_id == chunk_id) {
                // Verify chunk hash if available
                let mut hasher = Sha256::new();
                hasher.update(&chunk_data);
                let actual_hash = format!("{:x}", hasher.finalize());
                if actual_hash != chunk_info.hash {
                    return Err(format!(
                        "Chunk hash mismatch: expected {}, got {}",
                        chunk_info.hash, actual_hash
                    ));
                }
            }
        }

        Ok(chunk_data)
    }

    /// Scan existing chunks on disk and return list of available chunk IDs for a file
    pub async fn scan_existing_chunks(&self, file_hash: &str) -> Result<Vec<u32>, String> {
        let chunks_dir = std::path::Path::new("./chunks");
        let file_dir = chunks_dir.join(file_hash);

        if !file_dir.exists() {
            return Ok(Vec::new());
        }

        let mut existing_chunks = Vec::new();
        let mut dir_entries = tokio::fs::read_dir(&file_dir)
            .await
            .map_err(|e| format!("Failed to read chunks directory: {}", e))?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read directory entry: {}", e))?
        {
            let file_name_owned = entry.file_name().to_string_lossy().to_string();

            // Look for metadata files
            if file_name_owned.ends_with(".meta") && file_name_owned.starts_with("chunk_") {
                if let Some(chunk_id_str) = file_name_owned
                    .strip_prefix("chunk_")
                    .and_then(|s| s.strip_suffix(".meta"))
                {
                    if let Ok(chunk_id) = chunk_id_str.parse::<u32>() {
                        // Verify the corresponding .dat file exists
                        let dat_path = file_dir.join(format!("chunk_{}.dat", chunk_id));
                        if dat_path.exists() {
                            existing_chunks.push(chunk_id);
                        }
                    }
                }
            }
        }

        // Sort chunks by ID for consistent ordering
        existing_chunks.sort_unstable();
        Ok(existing_chunks)
    }

    /// Load all existing chunks for a file and add them to the active download
    pub async fn load_existing_chunks_into_download(
        &self,
        file_hash: &str,
    ) -> Result<usize, String> {
        let existing_chunks = self.scan_existing_chunks(file_hash).await?;

        if existing_chunks.is_empty() {
            return Ok(0);
        }

        let mut downloads = self.active_downloads.write().await;
        let download = downloads
            .get_mut(file_hash)
            .ok_or_else(|| format!("Active download not found for file {}", file_hash))?;

        let mut loaded_count = 0;
        for chunk_id in existing_chunks {
            // Check if chunk is already in memory
            if download.completed_chunks.contains_key(&chunk_id) {
                continue;
            }

            // Try to load from disk
            match self.load_chunk_from_disk(file_hash, chunk_id).await {
                Ok(chunk_data) => {
                    let completed_chunk = CompletedChunk {
                        chunk_id,
                        data: chunk_data,
                        source_id: "disk".to_string(), // Mark as loaded from disk
                        completed_at: std::time::Instant::now(),
                    };
                    download.completed_chunks.insert(chunk_id, completed_chunk);
                    loaded_count += 1;
                    info!("Loaded chunk {} from disk for file {}", chunk_id, file_hash);
                }
                Err(e) => {
                    warn!("Failed to load chunk {} from disk: {}", chunk_id, e);
                    // Continue with other chunks
                }
            }
        }

        Ok(loaded_count)
    }

    /// Clean up old or orphaned chunks to free disk space
    pub async fn cleanup_chunks(&self, max_age_days: Option<u64>) -> Result<usize, String> {
        let chunks_dir = std::path::Path::new("./chunks");
        if !chunks_dir.exists() {
            return Ok(0);
        }

        let mut cleaned_count = 0;
        let mut dir_entries = tokio::fs::read_dir(&chunks_dir)
            .await
            .map_err(|e| format!("Failed to read chunks directory: {}", e))?;

        let max_age_seconds = max_age_days.map(|days| days * 24 * 60 * 60);

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read directory entry: {}", e))?
        {
            let file_dir = entry.path();
            if !file_dir.is_dir() {
                continue;
            }

            // Check if this file hash is still being downloaded
            let file_name = file_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");

            let downloads = self.active_downloads.read().await;
            let is_active_download = downloads.contains_key(file_name);
            drop(downloads);

            if is_active_download {
                // Don't clean up active downloads
                continue;
            }

            // Clean up this file's chunks
            let file_cleanup_count = self.cleanup_file_chunks(&file_dir, max_age_seconds).await?;
            cleaned_count += file_cleanup_count;

            // If all chunks are cleaned up, remove the directory
            if let Ok(mut file_dir_entries) = tokio::fs::read_dir(&file_dir).await {
                let mut has_files = false;
                while let Some(entry) = file_dir_entries
                    .next_entry()
                    .await
                    .map_err(|e| format!("Failed to read file dir entry: {}", e))?
                {
                    has_files = true;
                    break;
                }

                if !has_files {
                    let _ = tokio::fs::remove_dir(&file_dir).await;
                }
            }
        }

        Ok(cleaned_count)
    }

    /// Clean up chunks for a specific file
    async fn cleanup_file_chunks(
        &self,
        file_dir: &std::path::Path,
        max_age_seconds: Option<u64>,
    ) -> Result<usize, String> {
        let mut cleaned_count = 0;
        let mut dir_entries = tokio::fs::read_dir(file_dir)
            .await
            .map_err(|e| format!("Failed to read file chunks directory: {}", e))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read file dir entry: {}", e))?
        {
            let file_name_owned = entry.file_name().to_string_lossy().to_string();

            // Only process metadata files for cleanup decisions
            if file_name_owned.ends_with(".meta") {
                let metadata_path = entry.path();
                let dat_path = file_dir.join(file_name_owned.replace(".meta", ".dat"));

                // Check if corresponding .dat file exists
                if !dat_path.exists() {
                    // Remove orphaned metadata file
                    let _ = tokio::fs::remove_file(&metadata_path).await;
                    cleaned_count += 1;
                    continue;
                }

                // Check age if max_age specified
                if let Some(max_age) = max_age_seconds {
                    match tokio::fs::metadata(&metadata_path).await {
                        Ok(metadata) => {
                            if let Ok(modified) = metadata.modified() {
                                if let Ok(age) = modified.elapsed() {
                                    if age.as_secs() > max_age {
                                        // Remove old chunk files
                                        let _ = tokio::fs::remove_file(&metadata_path).await;
                                        let _ = tokio::fs::remove_file(&dat_path).await;
                                        cleaned_count += 1;
                                        continue;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            // If we can't read metadata, remove the files (corrupted)
                            let _ = tokio::fs::remove_file(&metadata_path).await;
                            let _ = tokio::fs::remove_file(&dat_path).await;
                            cleaned_count += 1;
                            continue;
                        }
                    }
                }

                // Validate chunk integrity
                if let Err(_) = self.validate_chunk_metadata(&metadata_path).await {
                    // Remove corrupted chunks
                    let _ = tokio::fs::remove_file(&metadata_path).await;
                    let _ = tokio::fs::remove_file(&dat_path).await;
                    cleaned_count += 1;
                }
            }
        }

        Ok(cleaned_count)
    }

    /// Validate chunk metadata file
    async fn validate_chunk_metadata(&self, metadata_path: &std::path::Path) -> Result<(), String> {
        let content = tokio::fs::read_to_string(metadata_path)
            .await
            .map_err(|e| format!("Failed to read metadata: {}", e))?;

        let _: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("Invalid metadata JSON: {}", e))?;

        Ok(())
    }

    /// Remove duplicate chunks across different files (if they have the same content hash)
    pub async fn deduplicate_chunks(&self) -> Result<usize, String> {
        let chunks_dir = std::path::Path::new("./chunks");
        if !chunks_dir.exists() {
            return Ok(0);
        }

        let mut content_hashes: std::collections::HashMap<String, std::path::PathBuf> =
            std::collections::HashMap::new();
        let mut duplicates = Vec::new();

        // Scan all chunk files and collect content hashes
        let mut dir_entries = tokio::fs::read_dir(&chunks_dir)
            .await
            .map_err(|e| format!("Failed to read chunks directory: {}", e))?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read directory entry: {}", e))?
        {
            let file_dir = entry.path();
            if !file_dir.is_dir() {
                continue;
            }

            let mut file_dir_entries = tokio::fs::read_dir(&file_dir)
                .await
                .map_err(|e| format!("Failed to read file directory: {}", e))?;

            while let Some(chunk_entry) = file_dir_entries
                .next_entry()
                .await
                .map_err(|e| format!("Failed to read chunk entry: {}", e))?
            {
                let file_name = chunk_entry.file_name().to_string_lossy().to_string();

                if file_name.ends_with(".dat") {
                    let chunk_path = chunk_entry.path();

                    // Read chunk content and hash it
                    match tokio::fs::read(&chunk_path).await {
                        Ok(data) => {
                            let mut hasher = Sha256::new();
                            hasher.update(&data);
                            let content_hash = format!("{:x}", hasher.finalize());

                            // Check if we've seen this content hash before
                            if let Some(existing_path) = content_hashes.get(&content_hash) {
                                // This is a duplicate
                                duplicates.push((chunk_path.clone(), existing_path.clone()));
                            } else {
                                content_hashes.insert(content_hash, chunk_path);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to read chunk file {}: {}", chunk_path.display(), e);
                        }
                    }
                }
            }
        }

        // Remove duplicate files (keep the first occurrence)
        let mut removed_count = 0;
        for (duplicate_path, _original_path) in duplicates {
            // Remove the duplicate .dat file
            if let Err(e) = tokio::fs::remove_file(&duplicate_path).await {
                warn!(
                    "Failed to remove duplicate chunk {}: {}",
                    duplicate_path.display(),
                    e
                );
            } else {
                removed_count += 1;
            }

            // Also remove the corresponding .meta file
            let meta_path = duplicate_path.with_extension("meta");
            let _ = tokio::fs::remove_file(&meta_path).await;
        }

        Ok(removed_count)
    }

    /// Save download state to disk for persistence across app restarts
    pub async fn save_download_state(&self) -> Result<(), String> {
        let downloads_dir = std::path::Path::new("./downloads");
        if !downloads_dir.exists() {
            std::fs::create_dir_all(downloads_dir)
                .map_err(|e| format!("Failed to create downloads directory: {}", e))?;
        }

        let downloads = self.active_downloads.read().await;

        for (file_hash, download) in downloads.iter() {
            let state_path = downloads_dir.join(format!("{}.state", file_hash));

            let state = DownloadState {
                file_hash: file_hash.clone(),
                file_metadata: download.file_metadata.clone(),
                chunks: download.chunks.clone(),
                source_assignments: download.source_assignments.values().cloned().collect(),
                completed_chunk_ids: download.completed_chunks.keys().cloned().collect(),
                failed_chunks: download.failed_chunks.iter().cloned().collect(),
                start_time_unix: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .saturating_sub(download.start_time.elapsed().as_secs()),
                output_path: download.output_path.clone(),
                ed2k_chunk_hashes: download.ed2k_chunk_hashes.clone(),
                saved_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };

            let state_json = serde_json::to_string_pretty(&state)
                .map_err(|e| format!("Failed to serialize download state: {}", e))?;

            tokio::fs::write(&state_path, state_json)
                .await
                .map_err(|e| format!("Failed to write download state file: {}", e))?;

            debug!("Saved download state for file {}", file_hash);
        }

        Ok(())
    }

    /// Load persisted download states from disk
    pub async fn load_download_states(&self) -> Result<Vec<String>, String> {
        let downloads_dir = std::path::Path::new("./downloads");
        if !downloads_dir.exists() {
            return Ok(Vec::new());
        }

        let mut loaded_files = Vec::new();
        let mut dir_entries = tokio::fs::read_dir(downloads_dir)
            .await
            .map_err(|e| format!("Failed to read downloads directory: {}", e))?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read directory entry: {}", e))?
        {
            let file_name_owned = entry.file_name().to_string_lossy().to_string();

            if file_name_owned.ends_with(".state") {
                let state_path = entry.path();
                let file_hash = file_name_owned
                    .strip_suffix(".state")
                    .unwrap_or(&file_name_owned);

                match self.load_download_state(&state_path, file_hash).await {
                    Ok(_) => {
                        loaded_files.push(file_hash.to_string());
                        info!("Loaded persisted download state for file {}", file_hash);
                    }
                    Err(e) => {
                        warn!("Failed to load download state for {}: {}", file_hash, e);
                        // Remove corrupted state file
                        let _ = tokio::fs::remove_file(&state_path).await;
                    }
                }
            }
        }

        Ok(loaded_files)
    }

    /// Load a specific download state from file
    async fn load_download_state(
        &self,
        state_path: &std::path::Path,
        file_hash: &str,
    ) -> Result<(), String> {
        let state_content = tokio::fs::read_to_string(state_path)
            .await
            .map_err(|e| format!("Failed to read state file: {}", e))?;

        let state: DownloadState = serde_json::from_str(&state_content)
            .map_err(|e| format!("Failed to parse state file: {}", e))?;

        // Validate state
        if state.file_hash != file_hash {
            return Err("File hash mismatch in state file".to_string());
        }

        // Check if download is already active
        {
            let downloads = self.active_downloads.read().await;
            if downloads.contains_key(file_hash) {
                return Err("Download already active".to_string());
            }
        }

        // Reconstruct source assignments map
        let mut source_assignments = HashMap::new();
        for assignment in state.source_assignments {
            source_assignments.insert(assignment.source.identifier(), assignment);
        }

        // Reconstruct completed chunks (load from disk)
        let mut completed_chunks = HashMap::new();
        for chunk_id in state.completed_chunk_ids {
            match self.load_chunk_from_disk(file_hash, chunk_id).await {
                Ok(data) => {
                    let completed_chunk = CompletedChunk {
                        chunk_id,
                        data,
                        source_id: "persisted".to_string(), // Mark as loaded from persisted state
                        completed_at: std::time::Instant::now(),
                    };
                    completed_chunks.insert(chunk_id, completed_chunk);
                }
                Err(e) => {
                    warn!(
                        "Failed to load persisted chunk {} for {}: {}",
                        chunk_id, file_hash, e
                    );
                    // Continue without this chunk
                }
            }
        }

        // Create the download state
        let download = ActiveDownload {
            file_metadata: state.file_metadata,
            chunks: state.chunks,
            source_assignments,
            completed_chunks,
            pending_requests: HashMap::new(), // Will be reconstructed when sources reconnect
            failed_chunks: state.failed_chunks.into(),
            start_time: std::time::Instant::now(), // We'll use current time as approximation
            last_progress_update: std::time::Instant::now(),
            output_path: state.output_path,
            ed2k_chunk_hashes: state.ed2k_chunk_hashes,
        };

        // Store the download
        {
            let mut downloads = self.active_downloads.write().await;
            downloads.insert(file_hash.to_string(), download);
        }

        Ok(())
    }

    /// Remove persisted download state (called when download completes)
    pub async fn remove_download_state(&self, file_hash: &str) -> Result<(), String> {
        let downloads_dir = std::path::Path::new("./downloads");
        let state_path = downloads_dir.join(format!("{}.state", file_hash));

        if state_path.exists() {
            tokio::fs::remove_file(&state_path)
                .await
                .map_err(|e| format!("Failed to remove download state file: {}", e))?;
        }

        Ok(())
    }

    /// Clean up old persisted download states (for completed downloads)
    pub async fn cleanup_old_download_states(&self) -> Result<usize, String> {
        let downloads_dir = std::path::Path::new("./downloads");
        if !downloads_dir.exists() {
            return Ok(0);
        }

        let downloads = self.active_downloads.read().await;
        let active_file_hashes: std::collections::HashSet<String> =
            downloads.keys().cloned().collect();
        drop(downloads);

        let mut cleaned_count = 0;
        let mut dir_entries = tokio::fs::read_dir(downloads_dir)
            .await
            .map_err(|e| format!("Failed to read downloads directory: {}", e))?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read directory entry: {}", e))?
        {
            let file_name_owned = entry.file_name().to_string_lossy().to_string();

            if file_name_owned.ends_with(".state") {
                if let Some(file_hash) = file_name_owned.strip_suffix(".state") {
                    if !active_file_hashes.contains(file_hash) {
                        // This state file is for a download that's no longer active
                        let state_path = entry.path();
                        match tokio::fs::remove_file(&state_path).await {
                            Ok(_) => {
                                cleaned_count += 1;
                                debug!("Removed old download state file for {}", file_hash);
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to remove old download state file {}: {}",
                                    state_path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(cleaned_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dht::DhtService;
    use crate::webrtc_service::WebRTCService;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    #[test]
    fn verify_chunk_integrity_accepts_matching_hash() {
        let data = b"hello world";
        let expected = hex::encode(Sha256::digest(data));
        let chunk = ChunkInfo {
            chunk_id: 0,
            offset: 0,
            size: data.len(),
            hash: expected,
        };

        assert!(verify_chunk_integrity(&chunk, data).is_ok());
    }

    #[test]
    fn verify_chunk_integrity_detects_mismatch() {
        let data = b"hello world";
        let expected = hex::encode(Sha256::digest(data));
        let chunk = ChunkInfo {
            chunk_id: 0,
            offset: 0,
            size: data.len(),
            hash: expected,
        };

        let other_data = b"goodbye world";
        assert!(verify_chunk_integrity(&chunk, other_data).is_err());
    }

    #[test]
    fn test_file_size_thresholds() {
        // Test the constants used for multi-source decisions
        let small_file = 500 * 1024; // 500KB
        let large_file = 2 * 1024 * 1024; // 2MB

        assert!(small_file < 1024 * 1024); // Less than 1MB
        assert!(large_file > 1024 * 1024); // Greater than 1MB
    }

    #[test]
    fn test_multi_source_event_serialization() {
        let event = MultiSourceEvent::DownloadStarted {
            file_hash: "test_hash".to_string(),
            total_peers: 3,
        };

        // Test that event can be serialized (required for Tauri events)
        let serialized = serde_json::to_string(&event);
        assert!(serialized.is_ok());
    }

    #[test]
    fn test_ftp_source_assignment() {
        use crate::download_source::{DownloadSource, FtpSourceInfo as DownloadFtpSourceInfo};

        let ftp_info = DownloadFtpSourceInfo {
            url: "ftp://ftp.example.com/file.bin".to_string(),
            username: Some("testuser".to_string()),
            encrypted_password: Some("testpass".to_string()),
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(30),
        };

        let ftp_source = DownloadSource::Ftp(ftp_info);
        let chunk_ids = vec![1, 2, 3];
        let assignment = SourceAssignment::new(ftp_source.clone(), chunk_ids.clone());

        assert_eq!(assignment.source_id(), "ftp://ftp.example.com/file.bin");
        assert_eq!(assignment.chunks, chunk_ids);
        assert_eq!(assignment.status, SourceStatus::Connecting);
        assert!(matches!(assignment.source, DownloadSource::Ftp(_)));
    }

    #[test]
    fn verify_chunk_integrity_skips_non_hex_hash() {
        let data = b"hello world";
        let chunk = ChunkInfo {
            chunk_id: 0,
            offset: 0,
            size: data.len(),
            hash: "hash0".to_string(),
        };

        assert!(verify_chunk_integrity(&chunk, data).is_ok());
    }

    // Helper function to create mock services
    fn create_mock_services() -> (Arc<DhtService>, Arc<WebRTCService>) {
        // For testing, we'll skip actual service initialization
        // These would need proper mocking in a real test environment
        panic!("Mock services not implemented - this is a placeholder for integration tests")
    }

    #[test]
    fn test_chunk_info_creation() {
        let chunk = ChunkInfo {
            chunk_id: 0,
            offset: 0,
            size: 256 * 1024,
            hash: "test_hash".to_string(),
        };

        assert_eq!(chunk.chunk_id, 0);
        assert_eq!(chunk.offset, 0);

        assert_eq!(chunk.size, 256 * 1024);
        assert_eq!(chunk.hash, "test_hash");
    }

    #[test]
    fn test_multi_source_constants() {
        assert_eq!(DEFAULT_CHUNK_SIZE, 256 * 1024);
        assert_eq!(MAX_CHUNKS_PER_PEER, 10);
        assert_eq!(MIN_CHUNKS_FOR_PARALLEL, 4);
        assert_eq!(CONNECTION_TIMEOUT_SECS, 30);
    }

    #[test]
    fn test_chunk_request_creation() {
        let request = ChunkRequest {
            chunk_id: 1,
            source_id: "peer123".to_string(),
            requested_at: Instant::now(),
            retry_count: 0,
        };

        assert_eq!(request.chunk_id, 1);
        assert_eq!(request.source_id, "peer123");
        assert_eq!(request.retry_count, 0);
    }

    #[test]
    fn test_completed_chunk_creation() {
        let data = vec![1, 2, 3, 4, 5];
        let chunk = CompletedChunk {
            chunk_id: 2,
            data: data.clone(),
            source_id: "peer456".to_string(),
            completed_at: Instant::now(),
        };

        assert_eq!(chunk.chunk_id, 2);
        assert_eq!(chunk.data, data);
        assert_eq!(chunk.source_id, "peer456");
    }
}
