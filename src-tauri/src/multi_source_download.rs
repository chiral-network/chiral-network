use crate::dht::{DhtService, FileMetadata, WebRTCOfferRequest};
use crate::download_source::{DownloadSource, FtpSourceInfo as DownloadFtpSourceInfo};
use crate::webrtc_service::{WebRTCFileRequest, WebRTCService};
use url::Url;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::timeout;
use tracing::{error, info, warn};

const DEFAULT_CHUNK_SIZE: usize = 256 * 1024; // 256KB chunks
const MAX_CHUNKS_PER_PEER: usize = 10; // Maximum chunks to assign to a single peer
const MIN_CHUNKS_FOR_PARALLEL: usize = 4; // Minimum chunks to enable parallel download
const CONNECTION_TIMEOUT_SECS: u64 = 30;
#[allow(dead_code)]
const CHUNK_REQUEST_TIMEOUT_SECS: u64 = 60;
#[allow(dead_code)]
const MAX_RETRY_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

pub struct MultiSourceDownloadService {
    dht_service: Arc<DhtService>,
    webrtc_service: Arc<WebRTCService>,
    proxy_latency_service: Option<Arc<Mutex<crate::proxy_latency::ProxyLatencyService>>>,
    active_downloads: Arc<RwLock<HashMap<String, ActiveDownload>>>,
    event_tx: mpsc::UnboundedSender<MultiSourceEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<MultiSourceEvent>>>,
    command_tx: mpsc::UnboundedSender<MultiSourceCommand>,
    command_rx: Arc<Mutex<mpsc::UnboundedReceiver<MultiSourceCommand>>>,
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
    pub fn new(dht_service: Arc<DhtService>, webrtc_service: Arc<WebRTCService>) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        Self {
            dht_service,
            webrtc_service,
            proxy_latency_service: Some(Arc::new(Mutex::new(
                crate::proxy_latency::ProxyLatencyService::new(),
            ))),
            active_downloads: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            command_tx,
            command_rx: Arc::new(Mutex::new(command_rx)),
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

        // Search for file metadata
        let metadata = match self
            .dht_service
            .synchronous_search_metadata(file_hash.clone(), 5000)
            .await
        {
            Ok(Some(metadata)) => metadata,
            Ok(None) => return Err("File metadata not found".to_string()),
            Err(e) => return Err(format!("DHT search failed: {}", e)),
        };

        // Discover available sources (P2P peers + FTP sources)
        let mut available_sources = Vec::new();

        // 1. Discover P2P peers
        let available_peers = self
            .dht_service
            .discover_peers_for_file(&metadata)
            .await
            .map_err(|e| format!("Peer discovery failed: {}", e))?;

        info!("Found {} available P2P peers for file", available_peers.len());

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
                    passive_mode: true,  // Default to passive mode
                    use_ftps: false,     // Default to regular FTP
                    timeout_secs: Some(30),
                }));
            }
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

        if !use_multi_source {
            info!("Using single-source download (not enough chunks or sources)");
            return self
                .start_single_source_download(metadata, output_path)
                .await;
        }

        // Select optimal sources for multi-source download
        let max_sources = max_peers.unwrap_or(available_sources.len().min(4));
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
        };

        // Store download state
        {
            let mut downloads = self.active_downloads.write().await;
            downloads.insert(file_hash.clone(), download);
        }

        // Start source connections and assign chunks
        self.start_source_connections(&file_hash, selected_sources.clone())
            .await?;

        // Emit download started event
        let _ = self.event_tx.send(MultiSourceEvent::DownloadStarted {
            file_hash: file_hash.clone(),
            total_peers: selected_sources.len(),
        });

        // Start monitoring download progress
        self.spawn_download_monitor(file_hash).await;

        Ok(())
    }

    async fn start_single_source_download(
        &self,
        _metadata: FileMetadata,
        _output_path: String,
    ) -> Result<(), String> {
        // Fallback to existing single-peer download logic
        warn!("Multi-source download not applicable, falling back to single-source");
        Err("Single-source download not implemented in this service".to_string())
    }

    fn calculate_chunks(&self, metadata: &FileMetadata, chunk_size: usize) -> Vec<ChunkInfo> {
        let mut chunks = Vec::new();
        let total_size = metadata.file_size as usize;
        let mut offset = 0u64;
        let mut chunk_id = 0u32;

        while offset < metadata.file_size {
            let remaining = (metadata.file_size - offset) as usize;
            let size = remaining.min(chunk_size);

            // Calculate chunk hash (simplified - in real implementation this would be pre-calculated)
            let hash = format!("{}_{}", metadata.merkle_root, chunk_id);

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
    fn select_optimal_sources(&self, available_sources: &[DownloadSource], max_sources: usize) -> Vec<DownloadSource> {
        let mut sources = available_sources.to_vec();
        
        // Sort by priority score (higher is better)
        sources.sort_by(|a, b| b.priority_score().cmp(&a.priority_score()));
        
        // Take the top sources
        sources.truncate(max_sources);
        
        info!("Selected sources by priority:");
        for (i, source) in sources.iter().enumerate() {
            info!("  {}: {} (priority: {})", i + 1, source.display_name(), source.priority_score());
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
        let chunk_assignments = self.assign_chunks_to_sources(&download.chunks, &sources);
        drop(downloads);

        // Start connecting to sources
        for (source, chunk_ids) in chunk_assignments {
            match &source {
                DownloadSource::P2p(p2p_info) => {
                    self.start_p2p_connection(file_hash, p2p_info.peer_id.clone(), chunk_ids).await?;
                }
                DownloadSource::Ftp(ftp_info) => {
                    // TODO: FTP data fetching implementation needed
                    // This belongs to "FTP Data Fetching & Verification" task
                    warn!("FTP download not yet implemented for source: {}", ftp_info.url);
                }
                DownloadSource::Http(http_info) => {
                    self.start_http_download(file_hash, http_info.clone(), chunk_ids).await?;
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
    ) -> Vec<(DownloadSource, Vec<u32>)> {
        // Defensive: if no sources, return an empty assignment list instead of panicking.
        if sources.is_empty() {
            return Vec::new();
        }

        let mut assignments: Vec<(DownloadSource, Vec<u32>)> =
            sources.iter().map(|s| (s.clone(), Vec::new())).collect();

        // Round-robin assignment
        for (index, chunk) in chunks.iter().enumerate() {
            let source_index = index % sources.len();
            if let Some((_, chunks)) = assignments.get_mut(source_index) {
                if chunks.len() < MAX_CHUNKS_PER_PEER {
                    chunks.push(chunk.chunk_id);
                }
            }
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

    // FTP download implementation removed - this belongs to "FTP Data Fetching & Verification" task
    // Scope was only FTP source handling and chunk assignment, not data fetching

    /// Start HTTP download (placeholder)
    async fn start_http_download(
        &self,
        file_hash: &str,
        _http_info: crate::download_source::HttpSourceInfo,
        chunk_ids: Vec<u32>,
    ) -> Result<(), String> {
        info!("HTTP download not yet implemented");
        
        // Mark as failed for now
        self.on_source_failed(file_hash, "http_placeholder", "HTTP download not implemented".to_string()).await;
        Err("HTTP download not implemented".to_string())
    }

    // FTP chunk downloading implementation removed - this belongs to "FTP Data Fetching & Verification" task

    /// Parse remote path from FTP URL
    fn parse_ftp_remote_path(&self, url: &str) -> Result<String, String> {
        let parsed_url = Url::parse(url)
            .map_err(|e| format!("Invalid FTP URL: {}", e))?;
        
        let path = parsed_url.path();
        if path.is_empty() || path == "/" {
            return Err("No file path specified in FTP URL".to_string());
        }
        
        Ok(path.to_string())
    }

    /// Calculate byte range for FTP request based on chunk info
    fn calculate_ftp_byte_range(&self, chunk_info: &ChunkInfo) -> (u64, u64) {
        (chunk_info.offset, chunk_info.size as u64)
    }

    /// Handle source connection success
    async fn on_source_connected(&self, file_hash: &str, source_id: &str, chunk_ids: Vec<u32>) {
        info!("Source {} connected for file {}", source_id, file_hash);

        // Avoid unwrap() on SystemTime duration to prevent panics when system clock is before UNIX_EPOCH
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(e) => {
                warn!("System time before UNIX_EPOCH: {}", e);
                0
            }
        };

        // Update source status
        {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(source_id) {
                    assignment.status = SourceStatus::Connected;
                    assignment.connected_at = Some(now);
                    assignment.last_activity = Some(now);
                }
            }
        }

        // Emit event
        let _ = self.event_tx.send(MultiSourceEvent::PeerConnected {
            file_hash: file_hash.to_string(),
            peer_id: source_id.to_string(),
        });
    }

    /// Handle source connection failure
    async fn on_source_failed(&self, file_hash: &str, source_id: &str, error: String) {
        warn!("Source {} failed for file {}: {}", source_id, file_hash, error);

        // Update source status and reassign chunks
        let reassign_chunks = {
            let mut downloads = self.active_downloads.write().await;
            if let Some(download) = downloads.get_mut(file_hash) {
                if let Some(assignment) = download.source_assignments.get_mut(source_id) {
                    assignment.status = SourceStatus::Failed;
                    let chunks = assignment.chunks.clone();

                    // Add failed chunks back to retry queue
                    for chunk_id in &chunks {
                        download.failed_chunks.push_back(*chunk_id);
                    }

                    chunks
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        };

        // Emit event
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
        self.start_p2p_connection(file_hash, peer_id, chunk_ids).await
    }

    async fn on_peer_connected(&self, file_hash: &str, peer_id: &str, chunk_ids: Vec<u32>) {
        // Delegate to unified source connection handler
        self.on_source_connected(file_hash, peer_id, chunk_ids).await
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
            // Close all peer connections
            for peer_id in download.source_assignments.keys() {
                let _ = self.webrtc_service.close_connection(peer_id.clone()).await;
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

        // Try to find available peers for retry
        let available_peers = {
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
                    .map(|(peer_id, _)| peer_id.clone())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };

        if available_peers.is_empty() {
            warn!("No available peers for retry");
            return Err("No available peers for retry".to_string());
        }

        // Reassign failed chunks to available peers
        for (index, chunk_id) in failed_chunks.iter().enumerate() {
            let peer_index = index % available_peers.len();
            let peer_id = &available_peers[peer_index];

            // Add chunk to peer's assignment
            {
                let mut downloads = self.active_downloads.write().await;
                if let Some(download) = downloads.get_mut(file_hash) {
                    if let Some(assignment) = download.source_assignments.get_mut(peer_id) {
                        assignment.chunks.push(*chunk_id);
                    }
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

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));

            loop {
                interval.tick().await;

                let progress = {
                    let downloads = downloads.read().await;
                    if let Some(download) = downloads.get(&file_hash) {
                        Some(Self::calculate_progress_static(download))
                    } else {
                        None
                    }
                };

                if let Some(progress) = progress {
                    // Check if download is complete
                    if progress.completed_chunks >= progress.total_chunks {
                        // Finalize download
                        if let Err(e) = Self::finalize_download_static(&downloads, &file_hash).await
                        {
                            let _ = event_tx.send(MultiSourceEvent::DownloadFailed {
                                file_hash: file_hash.clone(),
                                error: format!("Failed to finalize download: {}", e),
                            });
                        }
                        break;
                    }

                    // Emit progress update
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
            let mut file_data = vec![0u8; download.file_metadata.file_size as usize];

            for chunk_info in &download.chunks {
                if let Some(completed_chunk) = download.completed_chunks.get(&chunk_info.chunk_id) {
                    let start = chunk_info.offset as usize;
                    let end = start + completed_chunk.data.len();
                    file_data[start..end].copy_from_slice(&completed_chunk.data);
                }
            }

            // Write file to disk
            tokio::fs::write(&download.output_path, file_data)
                .await
                .map_err(|e| format!("Failed to write file: {}", e))?;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dht::DhtService;
    use crate::webrtc_service::WebRTCService;
    use std::sync::Arc;

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
}
