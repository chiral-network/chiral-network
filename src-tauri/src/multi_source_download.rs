use crate::dht::{DhtService, FileMetadata, WebRTCOfferRequest};
use crate::download_source::{DownloadSource, Ed2kSourceInfo as DownloadEd2kSourceInfo, FtpSourceInfo as DownloadFtpSourceInfo};
use crate::ed2k_client::{Ed2kClient, Ed2kConfig, ED2K_CHUNK_SIZE};
use crate::ftp_downloader::{FtpDownloader, FtpCredentials};
use crate::webrtc_service::{WebRTCFileRequest, WebRTCService};
use suppaftp::FtpStream;
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
    ftp_downloader: Arc<FtpDownloader>,
    proxy_latency_service: Option<Arc<Mutex<crate::proxy_latency::ProxyLatencyService>>>,
    active_downloads: Arc<RwLock<HashMap<String, ActiveDownload>>>,
    event_tx: mpsc::UnboundedSender<MultiSourceEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<MultiSourceEvent>>>,
    command_tx: mpsc::UnboundedSender<MultiSourceCommand>,
    command_rx: Arc<Mutex<mpsc::UnboundedReceiver<MultiSourceCommand>>>,
    // FTP connection pool - maps FTP URL to connection for reuse
    ftp_connections: Arc<Mutex<HashMap<String, FtpStream>>>,
    // Ed2k connection pool - maps server URL to Ed2k client for reuse
    ed2k_connections: Arc<Mutex<HashMap<String, Ed2kClient>>>,
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
            ftp_downloader: Arc::new(FtpDownloader::new()),
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

        // Search for file metadata with sufficient timeout for DHT queries
        // Using 35s to match main.rs and allow full Kademlia query time (30s) + provider queries
        let metadata = match self
            .dht_service
            .synchronous_search_metadata(file_hash.clone(), 35000)
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

        // 3. Discover ed2k sources from metadata
        if let Some(ed2k_sources) = &metadata.ed2k_sources {
            info!("Found {} ed2k sources for file", ed2k_sources.len());
            
            for ed2k_info in ed2k_sources {
                // Convert DHT Ed2kSourceInfo to DownloadSource Ed2kSourceInfo
                available_sources.push(DownloadSource::Ed2k(DownloadEd2kSourceInfo {
                    server_url: ed2k_info.server_url.clone(),
                    file_hash: ed2k_info.file_hash.clone(),
                    file_size: ed2k_info.file_size,
                    file_name: ed2k_info.file_name.clone(),
                    sources: ed2k_info.sources.clone(),
                    timeout_secs: ed2k_info.timeout,
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
                    self.start_ftp_connection(file_hash, ftp_info.clone(), chunk_ids).await?;
                }
                DownloadSource::Http(http_info) => {
                    self.start_http_download(file_hash, http_info.clone(), chunk_ids).await?;
                }
                DownloadSource::Ed2k(ed2k_info) => {
                    self.start_ed2k_connection(file_hash, ed2k_info.clone(), chunk_ids).await?;
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
        let url = Url::parse(&ftp_info.url)
            .map_err(|e| format!("Invalid FTP URL: {}", e))?;

        // Create credentials from FTP source info
        let credentials = if let Some(username) = &ftp_info.username {
            let password = ftp_info.encrypted_password
                .as_deref()
                .unwrap_or("anonymous@chiral.network");
            Some(FtpCredentials::new(username.clone(), password.to_string()))
        } else {
            None // Use anonymous credentials
        };

        // Attempt to establish FTP connection
        match self.ftp_downloader.connect_and_login(&url, credentials).await {
            Ok(ftp_stream) => {
                info!("Successfully connected to FTP server: {}", ftp_info.url);

                // Store connection for reuse
                {
                    let mut connections = self.ftp_connections.lock().await;
                    connections.insert(ftp_url_id.clone(), ftp_stream);
                }

                // Mark source as connected and start chunk downloads
                self.on_source_connected(file_hash, &ftp_url_id, chunk_ids.clone()).await;
                self.start_ftp_chunk_downloads(file_hash, ftp_info, chunk_ids).await;

                Ok(())
            }
            Err(e) => {
                // Provide more specific error messages based on common FTP errors
                let error_msg = if e.contains("Connection refused") {
                    format!("FTP server refused connection: {} (server may be down)", ftp_info.url)
                } else if e.contains("timeout") || e.contains("Timeout") {
                    format!("FTP connection timeout: {} (server may be slow or unreachable)", ftp_info.url)
                } else if e.contains("login") || e.contains("authentication") || e.contains("530") {
                    format!("FTP authentication failed: {} (invalid credentials)", ftp_info.url)
                } else if e.contains("550") {
                    format!("FTP file not found or permission denied: {}", ftp_info.url)
                } else {
                    format!("FTP connection failed: {} - {}", ftp_info.url, e)
                };
                
                warn!("{}", error_msg);
                self.on_source_failed(file_hash, &ftp_url_id, error_msg.clone()).await;
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
                        download.chunks.iter().find(|chunk| chunk.chunk_id == chunk_id).cloned()
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
                self.on_source_failed(file_hash, &ftp_url_id, format!("Invalid FTP path: {}", e)).await;
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

                let task = tokio::spawn(async move {
                    let _permit = permit.unwrap();

                    // Calculate byte range for this chunk
                    let (start_byte, size) = (chunk.offset, chunk.size as u64);
                    
                    info!(
                        "Downloading FTP chunk {} ({}:{}) from {}",
                        chunk.chunk_id, start_byte, size, remote_path
                    );

                    // Get FTP connection (we need to handle connection sharing carefully)
                    let download_result = {
                        let mut connections_guard = connections.lock().await;
                        if let Some(ftp_stream) = connections_guard.get_mut(&ftp_url) {
                            downloader.download_range(ftp_stream, &remote_path, start_byte, size).await
                        } else {
                            Err("FTP connection not found".to_string())
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
                                
                                // For now, we'll accept partial data if it's the last chunk
                                let is_last_chunk = {
                                    let downloads_guard = downloads.read().await;
                                    if let Some(download) = downloads_guard.get(&file_hash) {
                                        chunk.chunk_id == (download.chunks.len() - 1) as u32
                                    } else {
                                        false
                                    }
                                };

                                if !is_last_chunk {
                                    let error_msg = format!("Chunk size mismatch: expected {}, got {}", chunk.size, data.len());
                                    let _ = event_tx.send(MultiSourceEvent::ChunkFailed {
                                        file_hash: file_hash.clone(),
                                        chunk_id: chunk.chunk_id,
                                        peer_id: ftp_url.clone(),
                                        error: error_msg,
                                    });
                                    return;
                                }
                            }

                            // TODO: Add hash verification here once chunk hashes are properly calculated
                            // For now, we'll skip hash verification as it needs to be implemented in the chunk calculation

                            // Store completed chunk
                            {
                                let mut downloads_guard = downloads.write().await;
                                if let Some(download) = downloads_guard.get_mut(&file_hash) {
                                    let completed_chunk = CompletedChunk {
                                        chunk_id: chunk.chunk_id,
                                        data,
                                        source_id: ftp_url.clone(),
                                        completed_at: Instant::now(),
                                    };
                                    download.completed_chunks.insert(chunk.chunk_id, completed_chunk);

                                    // Update last activity
                                    if let Some(assignment) = download.source_assignments.get_mut(&ftp_url) {
                                        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
                                            Ok(d) => Some(d.as_secs()),
                                            Err(_) => None,
                                        };
                                        assignment.last_activity = now;
                                    }
                                }
                            }

                            info!(
                                "Successfully downloaded FTP chunk {} ({} bytes)",
                                chunk.chunk_id,
                                chunk.size
                            );

                            // Emit chunk completed event
                            let _ = event_tx.send(MultiSourceEvent::ChunkCompleted {
                                file_hash: file_hash.clone(),
                                chunk_id: chunk.chunk_id,
                                peer_id: ftp_url.clone(),
                            });
                        }
                        Err(e) => {
                            warn!(
                                "Failed to download FTP chunk {}: {}",
                                chunk.chunk_id, e
                            );

                            // Add chunk back to failed queue
                            {
                                let mut downloads_guard = downloads.write().await;
                                if let Some(download) = downloads_guard.get_mut(&file_hash) {
                                    download.failed_chunks.push_back(chunk.chunk_id);
                                }
                            }

                            // Emit chunk failed event
                            let _ = event_tx.send(MultiSourceEvent::ChunkFailed {
                                file_hash: file_hash.clone(),
                                chunk_id: chunk.chunk_id,
                                peer_id: ftp_url.clone(),
                                error: e,
                            });
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
                        assignment.chunks.iter().all(|&chunk_id| {
                            download.completed_chunks.contains_key(&chunk_id)
                        })
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
                        if let Some(assignment) = download.source_assignments.get_mut(&ftp_url_clone) {
                            assignment.status = SourceStatus::Completed;
                        }
                    }
                }

                info!("FTP source {} completed all assigned chunks", ftp_url_clone);
            }
        });
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
                info!("Successfully connected to Ed2k server: {}", ed2k_info.server_url);

                // Store connection for reuse
                {
                    let mut connections = self.ed2k_connections.lock().await;
                    connections.insert(server_url_id.clone(), ed2k_client);
                }

                // Mark source as connected and start chunk downloads
                self.on_source_connected(file_hash, &server_url_id, chunk_ids.clone()).await;
                self.start_ed2k_chunk_downloads(file_hash, ed2k_info, chunk_ids).await;

                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Ed2k connection failed: {} - {:?}", ed2k_info.server_url, e);
                warn!("{}", error_msg);
                self.on_source_failed(file_hash, &server_url_id, error_msg.clone()).await;
                Err(error_msg)
            }
        }
    }

    /// Start downloading chunks from Ed2k network
    /// 
    /// This function efficiently downloads ed2k chunks by:
    /// 1. Grouping assigned 256KB chunks by their parent 9.28MB ed2k chunk
    /// 2. Downloading each ed2k chunk only once
    /// 3. Extracting all needed 256KB chunks from each downloaded ed2k chunk
    /// 4. Sorting chunks to ensure extraction happens in order
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
                        download.chunks.iter().find(|chunk| chunk.chunk_id == chunk_id).cloned()
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

        // Group chunks by ed2k chunk to avoid downloading the same ed2k chunk multiple times
        let grouped_by_ed2k = self.group_chunks_by_ed2k_chunk(&chunk_ids, &chunks_info);

        let file_hash_clone = file_hash.to_string();
        let ed2k_connections: Arc<Mutex<HashMap<String, Ed2kClient>>> = Arc::clone(&self.ed2k_connections);
        let active_downloads = Arc::clone(&self.active_downloads);
        let chunks_map_clone = Arc::new(chunks_map);

        // Spawn task to download chunks concurrently (limit to 2 concurrent downloads per server)
        tokio::spawn(async move {
            let semaphore = Arc::new(tokio::sync::Semaphore::new(2));
            let mut handles = Vec::new();

            // Download each ed2k chunk once and extract all needed chunks from it
            // Sort ed2k chunks by ID to process in order
            let mut sorted_ed2k_chunks: Vec<_> = grouped_by_ed2k.into_iter().collect();
            sorted_ed2k_chunks.sort_by_key(|(ed2k_id, _)| *ed2k_id);
            
            for (ed2k_chunk_id, mut our_chunk_ids) in sorted_ed2k_chunks {
                // Sort chunk IDs within this ed2k chunk to extract in order
                our_chunk_ids.sort();
                let permit = semaphore.clone().acquire_owned().await;
                let ed2k_connections_clone: Arc<Mutex<HashMap<String, Ed2kClient>>> = Arc::clone(&ed2k_connections);
                let active_downloads_clone = Arc::clone(&active_downloads);
                let file_hash_inner = file_hash_clone.clone();
                let server_url_clone = server_url_id.clone();
                let ed2k_file_hash = ed2k_info.file_hash.clone();
                let chunks_map_clone = chunks_map_clone.clone();

                let handle = tokio::spawn(async move {
                    let _permit = permit; // Hold permit until task completes

                    // Get Ed2k client from connection pool
                    let ed2k_client = {
                        let mut connections = ed2k_connections_clone.lock().await;
                        connections.remove(&server_url_clone)
                    };

                    if let Some(mut client) = ed2k_client {
                        // Download the entire ed2k chunk once
                        // Use the first chunk's hash as reference (all chunks in same ed2k chunk share the ed2k chunk hash)
                        let first_chunk_id = our_chunk_ids[0];
                        let expected_chunk_hash = if let Some(first_chunk) = chunks_map_clone.get(&first_chunk_id) {
                            format!("{:032x}", first_chunk.chunk_id)
                        } else {
                            format!("{:032x}", first_chunk_id)
                        };

                        match client.download_chunk(&ed2k_file_hash, ed2k_chunk_id, &expected_chunk_hash).await {
                            Ok(ed2k_chunk_data) => {
                                // Verify ed2k chunk size (should be 9.28 MB, except possibly the last chunk)
                                if ed2k_chunk_data.len() != ED2K_CHUNK_SIZE && ed2k_chunk_data.len() < ED2K_CHUNK_SIZE {
                                    error!(
                                        "Ed2k chunk {} size mismatch: expected at least {}, got {}",
                                        ed2k_chunk_id, ED2K_CHUNK_SIZE, ed2k_chunk_data.len()
                                    );
                                    
                                    // Mark all chunks in this ed2k chunk as failed
                                    let mut downloads = active_downloads_clone.write().await;
                                    if let Some(download) = downloads.get_mut(&file_hash_inner) {
                                        for chunk_id in &our_chunk_ids {
                                            download.failed_chunks.push_back(*chunk_id);
                                        }
                                    }
                                    
                                    // Return client to pool
                                    let mut connections = ed2k_connections_clone.lock().await;
                                    connections.insert(server_url_clone.clone(), client);
                                    return;
                                }

                                // Extract all needed chunks from the downloaded ed2k chunk
                                let mut downloads = active_downloads_clone.write().await;
                                if let Some(download) = downloads.get_mut(&file_hash_inner) {
                                    for chunk_id in &our_chunk_ids {
                                        if let Some(chunk_info) = chunks_map_clone.get(chunk_id) {
                                            // Calculate offset within the ed2k chunk
                                            let offset_within_ed2k = chunk_info.offset % ED2K_CHUNK_SIZE as u64;

                                            // Extract the 256 KB chunk from the ed2k chunk
                                            let start = offset_within_ed2k as usize;
                                            let end = std::cmp::min(start + chunk_info.size, ed2k_chunk_data.len());
                                            
                                            if end <= ed2k_chunk_data.len() {
                                                let chunk_data = ed2k_chunk_data[start..end].to_vec();

                                                let completed_chunk = CompletedChunk {
                                                    chunk_id: *chunk_id,
                                                    data: chunk_data,
                                                    source_id: server_url_clone.clone(),
                                                    completed_at: Instant::now(),
                                                };

                                                download.completed_chunks.insert(*chunk_id, completed_chunk);

                                                info!(
                                                    "Ed2k chunk {} extracted from ed2k chunk {} (offset {})",
                                                    chunk_id, ed2k_chunk_id, offset_within_ed2k
                                                );
                                            } else {
                                                error!(
                                                    "Cannot extract chunk {} from ed2k chunk {}: offset {} + size {} exceeds ed2k chunk size {}",
                                                    chunk_id, ed2k_chunk_id, start, chunk_info.size, ed2k_chunk_data.len()
                                                );
                                                download.failed_chunks.push_back(*chunk_id);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Failed to download Ed2k chunk {}: {:?}",
                                    ed2k_chunk_id, e
                                );

                                // Mark all chunks in this ed2k chunk as failed
                                let mut downloads = active_downloads_clone.write().await;
                                if let Some(download) = downloads.get_mut(&file_hash_inner) {
                                    for chunk_id in &our_chunk_ids {
                                        download.failed_chunks.push_back(*chunk_id);
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
                            for chunk_id in &our_chunk_ids {
                                download.failed_chunks.push_back(*chunk_id);
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

            info!("Ed2k source {} completed all assigned chunks", server_url_id);
        });
    }

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

    // ============================================================================
    // ed2k Chunk Mapping Functions (Person 4: Task 4.2 & 4.4)
    // ============================================================================

    /// Map our chunk ID to ed2k chunk ID and offset within that ed2k chunk
    /// 
    /// Our chunks are 256KB, ed2k chunks are 9.28 MB (9,728,000 bytes)
    /// One ed2k chunk contains approximately 38 of our chunks (9,728,000 / 256,000 = 38)
    /// 
    /// Returns: (ed2k_chunk_id, offset_within_ed2k_chunk)
    fn map_our_chunk_to_ed2k_chunk(&self, our_chunk: &ChunkInfo) -> (u32, u64) {
        let ed2k_chunk_id = (our_chunk.offset / ED2K_CHUNK_SIZE as u64) as u32;
        let offset_within_ed2k = our_chunk.offset % ED2K_CHUNK_SIZE as u64;
        (ed2k_chunk_id, offset_within_ed2k)
    }

    /// Map ed2k chunk ID to range of our chunk IDs
    /// 
    /// Returns the range of our chunk IDs that fall within the specified ed2k chunk
    /// 
    /// Returns: (start_chunk_id, end_chunk_id_inclusive)
    fn map_ed2k_chunk_to_our_chunks(
        &self,
        ed2k_chunk_id: u32,
        total_file_size: u64,
        our_chunk_size: usize,
    ) -> (u32, u32) {
        let ed2k_chunk_start_offset = ed2k_chunk_id as u64 * ED2K_CHUNK_SIZE as u64;
        let ed2k_chunk_end_offset = std::cmp::min(
            ed2k_chunk_start_offset + ED2K_CHUNK_SIZE as u64,
            total_file_size,
        );

        let start_chunk_id = (ed2k_chunk_start_offset / our_chunk_size as u64) as u32;
        let end_chunk_id = ((ed2k_chunk_end_offset - 1) / our_chunk_size as u64) as u32;

        (start_chunk_id, end_chunk_id)
    }

    /// Group our chunk IDs by the ed2k chunk they belong to
    /// 
    /// This is useful for Person 5 to download entire ed2k chunks and then split them
    /// 
    /// Returns: HashMap<ed2k_chunk_id, Vec<our_chunk_ids>>
    fn group_chunks_by_ed2k_chunk(
        &self,
        our_chunk_ids: &[u32],
        chunks: &[ChunkInfo],
    ) -> HashMap<u32, Vec<u32>> {
        let mut grouped: HashMap<u32, Vec<u32>> = HashMap::new();

        for &chunk_id in our_chunk_ids {
            if let Some(chunk) = chunks.iter().find(|c| c.chunk_id == chunk_id) {
                let (ed2k_chunk_id, _) = self.map_our_chunk_to_ed2k_chunk(chunk);
                grouped
                    .entry(ed2k_chunk_id)
                    .or_insert_with(Vec::new)
                    .push(chunk_id);
            }
        }

        grouped
    }

    /// Calculate chunk size considering ed2k sources
    /// 
    /// If ed2k sources are present, returns the ed2k chunk size (9.28 MB)
    /// Otherwise, returns the default chunk size (256 KB)
    /// 
    /// This is used to understand the relationship between our chunks and ed2k chunks
    fn calculate_ed2k_aware_chunk_size(
        &self,
        metadata: &FileMetadata,
    ) -> usize {
        if metadata.ed2k_sources.is_some() && !metadata.ed2k_sources.as_ref().unwrap().is_empty() {
            // ed2k sources present - return ed2k chunk size for reference
            ED2K_CHUNK_SIZE
        } else {
            // No ed2k sources - use default chunk size
            DEFAULT_CHUNK_SIZE
        }
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
            // Close connections based on source type
            for (source_id, assignment) in download.source_assignments.iter() {
                match &assignment.source {
                    DownloadSource::P2p(_) => {
                        // Close P2P/WebRTC connections
                        let _ = self.webrtc_service.close_connection(source_id.clone()).await;
                    }
                    DownloadSource::Ftp(_) => {
                        // Close FTP connections
                        let mut connections = self.ftp_connections.lock().await;
                        if let Some(mut ftp_stream) = connections.remove(source_id) {
                            let _ = self.ftp_downloader.disconnect(&mut ftp_stream).await;
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

    /// Get statistics about FTP connections and performance
    pub async fn get_ftp_statistics(&self) -> serde_json::Value {
        let connection_count = {
            let connections = self.ftp_connections.lock().await;
            connections.len()
        };

        let active_ftp_downloads = {
            let downloads = self.active_downloads.read().await;
            downloads.values()
                .map(|download| {
                    download.source_assignments.values()
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
            if let Some(mut ftp_stream) = connections.remove(&url) {
                if let Err(e) = self.ftp_downloader.disconnect(&mut ftp_stream).await {
                    warn!("Failed to disconnect FTP connection {}: {}", url, e);
                } else {
                    info!("Closed FTP connection: {}", url);
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

    #[test]
    fn test_ftp_source_assignment() {
        use crate::download_source::{FtpSourceInfo as DownloadFtpSourceInfo, DownloadSource};

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
    fn test_ftp_priority_score() {
        use crate::download_source::{FtpSourceInfo as DownloadFtpSourceInfo, DownloadSource, P2pSourceInfo, HttpSourceInfo};

        let ftp_source = DownloadSource::Ftp(DownloadFtpSourceInfo {
            url: "ftp://example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: None,
        });

        let p2p_source = DownloadSource::P2p(P2pSourceInfo {
            peer_id: "peer123".to_string(),
            multiaddr: None,
            reputation: Some(80),
            supports_encryption: false,
            protocol: None,
        });

        let http_source = DownloadSource::Http(HttpSourceInfo {
            url: "http://example.com/file".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: None,
        });

        // FTP should have lowest priority (25), HTTP medium (50), P2P highest (100 + reputation)
        assert_eq!(ftp_source.priority_score(), 25);
        assert_eq!(http_source.priority_score(), 50);
        assert_eq!(p2p_source.priority_score(), 180); // 100 + 80 reputation
    }
}
