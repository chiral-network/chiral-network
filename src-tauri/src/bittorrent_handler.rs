use crate::protocols::SimpleProtocolHandler;
use crate::transfer_events::{
    TransferEventBus, TransferProgressEvent, TransferPausedEvent, TransferResumedEvent,
    PauseReason,
    current_timestamp_ms, calculate_progress, calculate_eta,
};
use async_trait::async_trait;
use librqbit::{AddTorrent, ManagedTorrent, Session, SessionOptions, create_torrent, CreateTorrentOptions, AddTorrentOptions};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio::time::{self, Duration}; // Added for timeout in tests
use tracing::{error, info, instrument, warn};
use crate::dht::DhtService;
use libp2p::Multiaddr;
use thiserror::Error;
use serde::{Deserialize, Serialize, ser::Serializer};

const MAX_ACTIVE_DOWNLOADS: usize = 3;
const PAYMENT_THRESHOLD_BYTES: u64 = 1024 * 1024; // 1 MB

/// Custom error type for BitTorrent operations
#[derive(Debug, Error, Clone)]
pub enum BitTorrentError {
    /// Session initialization failed
    #[error("Failed to initialize BitTorrent session: {message}")]
    SessionInit { message: String },

    /// Invalid magnet link format
    #[error("Invalid magnet link format: {url}")]
    InvalidMagnetLink { url: String },

    /// Torrent file not found or invalid
    #[error("Torrent file error: {message}")]
    TorrentFileError { message: String },

    /// File system error (file not found, permission denied, etc.)
    #[error("File system error: {message}")]
    FileSystemError { message: String },

    /// Network/connection error
    #[error("Network error during BitTorrent operation: {message}")]
    NetworkError { message: String },

    /// Torrent parsing error
    #[error("Failed to parse torrent: {message}")]
    TorrentParsingError { message: String },

    /// Download timeout
    #[error("Download timed out after {timeout_secs} seconds")]
    DownloadTimeout { timeout_secs: u64 },

    /// Seeding operation failed
    #[error("Seeding failed: {message}")]
    SeedingError { message: String },

    /// Torrent handle unavailable
    #[error("Torrent handle is not available")]
    HandleUnavailable,

    /// Generic I/O error
    #[error("I/O error: {message}")]
    IoError {
        message: String,
    },

    /// Configuration error
    #[error("Configuration error: {message}")]
    ConfigError { message: String },

    /// Torrent already exists
    #[error("Torrent already exists: {info_hash}")]
    TorrentExists { info_hash: String },

    /// Torrent not found
    #[error("Torrent not found: {info_hash}")]
    TorrentNotFound { info_hash: String },

    /// Protocol-specific error
    #[error("Protocol error: {message}")]
    ProtocolSpecific { message: String },

    /// Unknown error from librqbit
    #[error("Unknown BitTorrent error: {message}")]
    Unknown { message: String },
}

impl From<std::io::Error> for BitTorrentError {
    fn from(err: std::io::Error) -> Self {
        BitTorrentError::IoError { message: err.to_string() }
    }
}

impl BitTorrentError {
    /// Convert to user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            BitTorrentError::SessionInit { .. } => {
                "Failed to start BitTorrent engine. Please check your download directory permissions.".to_string()
            }
            BitTorrentError::InvalidMagnetLink { .. } => {
                "The magnet link format is invalid. Please check the link and try again.".to_string()
            }
            BitTorrentError::TorrentFileError { .. } => {
                "The torrent file is invalid or corrupted. Please try a different torrent file.".to_string()
            }
            BitTorrentError::FileSystemError { .. } => {
                "File system error occurred. Please check file permissions and available disk space.".to_string()
            }
            BitTorrentError::NetworkError { .. } => {
                "Network connection failed. Please check your internet connection and firewall settings.".to_string()
            }
            BitTorrentError::TorrentParsingError { .. } => {
                "Failed to parse the torrent. The torrent file may be corrupted or incompatible.".to_string()
            }
            BitTorrentError::DownloadTimeout { timeout_secs } => {
                format!("Download timed out after {} seconds. No peers may be available for this torrent.", timeout_secs)
            }
            BitTorrentError::SeedingError { .. } => {
                "Failed to start seeding. Please check that the file exists and is accessible.".to_string()
            }
            BitTorrentError::HandleUnavailable => {
                "Torrent is no longer available. It may have been removed or completed.".to_string()
            }
            BitTorrentError::IoError { .. } => {
                "File system operation failed. Please check permissions and available disk space.".to_string()
            }
            BitTorrentError::ConfigError { .. } => {
                "BitTorrent configuration error. Please check your settings.".to_string()
            }
            BitTorrentError::TorrentExists { .. } => {
                "This torrent is already being downloaded or seeded.".to_string()
            }
            BitTorrentError::TorrentNotFound { .. } => {
                "Torrent not found. It may have been removed or never added.".to_string()
            }
            BitTorrentError::ProtocolSpecific { message } => {
                format!("BitTorrent operation failed: {}", message)
            }
            BitTorrentError::Unknown { .. } => {
                "An unexpected error occurred. Please try again or contact support if the issue persists.".to_string()
            }
        }
    }

    /// Get error category for logging/analytics
    pub fn category(&self) -> &'static str {
        match self {
            BitTorrentError::SessionInit { .. } => "session",
            BitTorrentError::InvalidMagnetLink { .. } => "validation",
            BitTorrentError::TorrentFileError { .. } => "validation",
            BitTorrentError::FileSystemError { .. } => "filesystem",
            BitTorrentError::NetworkError { .. } => "network",
            BitTorrentError::TorrentParsingError { .. } => "parsing",
            BitTorrentError::DownloadTimeout { .. } => "timeout",
            BitTorrentError::SeedingError { .. } => "seeding",
            BitTorrentError::HandleUnavailable => "state",
            BitTorrentError::IoError { .. } => "filesystem",
            BitTorrentError::ConfigError { .. } => "config",
            BitTorrentError::TorrentExists { .. } => "state",
            BitTorrentError::TorrentNotFound { .. } => "state",
            BitTorrentError::ProtocolSpecific { .. } => "protocol",
            BitTorrentError::Unknown { .. } => "unknown",
        }
    }
}

/// Represents the source of a torrent, which can be a magnet link or a .torrent file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PersistentTorrentSource {
    Magnet(String),
    File(PathBuf),
}

/// Represents the status of a persistent torrent.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PersistentTorrentStatus {
    Downloading,
    Seeding,
}

/// A struct representing the state of a single torrent to be persisted to disk.
/// This allows the application to resume downloads and seeds across restarts.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistentTorrent {
    /// The unique info hash of the torrent, as a hex string. This will be our primary key.
    pub info_hash: String,

    /// The source of the torrent (magnet link or file path) needed to re-add it.
    pub source: PersistentTorrentSource,

    /// The directory where the torrent's content is stored.
    pub output_path: PathBuf,

    /// The last known status of the torrent (e.g., downloading or seeding).
    pub status: PersistentTorrentStatus,

    /// Timestamp (Unix epoch seconds) when the torrent was added.
    pub added_at: u64,

    /// Priority of the download. Lower numbers mean higher priority (e.g., 0 is highest).
    pub priority: u32,
}

/// Events sent by the BitTorrent download monitor
#[derive(Debug)]
pub enum BitTorrentEvent {
    /// Download progress update
    Progress { downloaded: u64, total: u64 },
    /// Download has completed successfully
    Completed,
    /// Download has failed
    Failed(BitTorrentError),
}

/// Manages the persistence of torrent states to a JSON file.
#[derive(Debug)]
pub struct TorrentStateManager {
    state_file_path: PathBuf,
    torrents: BTreeMap<String, PersistentTorrent>, // Keyed by info_hash, sorted for consistent output
}

impl TorrentStateManager {
    /// Creates a new TorrentStateManager and loads the state from the given file path.
    pub fn new(state_file_path: PathBuf) -> Self {
        let mut manager = Self {
            state_file_path,
            torrents: BTreeMap::new(),
        };
        if let Err(e) = manager.load() {
            warn!(
                "Could not load torrent state file: {}. A new one will be created.",
                e
            );
        }
        manager
    }

    /// Loads the torrent state from the JSON file.
    fn load(&mut self) -> Result<(), std::io::Error> {
        if !self.state_file_path.exists() {
            return Ok(());
        }
        let file = std::fs::File::open(&self.state_file_path)?;
        let reader = std::io::BufReader::new(file);
        let loaded_torrents: Vec<PersistentTorrent> = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        self.torrents = loaded_torrents
            .into_iter()
            .map(|t| (t.info_hash.clone(), t))
            .collect();
        info!("Loaded {} torrents from state file.", self.torrents.len());
        Ok(())
    }

    /// Saves the current torrent state to the JSON file.
    pub fn save(&self) -> Result<(), std::io::Error> {
        // Ensure parent directory exists
        if let Some(parent) = self.state_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::File::create(&self.state_file_path)?;
        let writer = std::io::BufWriter::new(file);
        // Collect values to serialize them as a JSON array
        let values: Vec<&PersistentTorrent> = self.torrents.values().collect();
        serde_json::to_writer_pretty(writer, &values)?;
        Ok(())
    }

    /// Returns a vector of the torrents currently managed.
    pub fn get_all(&self) -> Vec<PersistentTorrent> {
        let mut torrents: Vec<PersistentTorrent> = self.torrents.values().cloned().collect();
        // Sort by priority (lower is higher), then by added_at timestamp as a tie-breaker.
        torrents.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.added_at.cmp(&b.added_at)));
        torrents
    }

    /// Updates the priorities of multiple torrents and saves the state.
    /// Accepts a list of (info_hash, new_priority) tuples.
    pub fn update_priorities(&mut self, updates: &[(String, u32)]) -> Result<(), std::io::Error> {
        let mut changed = false;
        for (info_hash, new_priority) in updates {
            if let Some(torrent) = self.torrents.get_mut(info_hash) {
                if torrent.priority != *new_priority {
                    torrent.priority = *new_priority;
                    changed = true;
                }
            }
        }
        if changed {
            self.save()
        } else {
            Ok(())
        }
    }
}

/// Tauri command to update the priority of downloads based on a new order.
/// The frontend sends a list of info_hashes in the desired order.
#[tauri::command]
pub async fn update_download_priorities(
    ordered_info_hashes: Vec<String>,
    // Assuming TorrentStateManager is managed in Tauri's state.
    // This is a common pattern and may need to be adjusted based on your main.rs setup.
    state_manager: tauri::State<'_, tokio::sync::Mutex<TorrentStateManager>>,
) -> Result<(), String> {
    info!("Updating download priorities for {} torrents.", ordered_info_hashes.len());

    // Convert the ordered list of hashes into a list of (hash, priority) tuples.
    let updates: Vec<(String, u32)> = ordered_info_hashes
        .into_iter()
        .enumerate()
        .map(|(index, hash)| (hash, index as u32))
        .collect();

    let mut manager = state_manager.lock().await;
    manager.update_priorities(&updates)
           .map_err(|e| format!("Failed to save updated priorities: {}", e))
}

/// Convert BitTorrentError to String for compatibility with ProtocolHandler trait
impl From<BitTorrentError> for String {
    fn from(error: BitTorrentError) -> Self {
        error.user_message()
    }
}

/// State for tracking per-peer data transfer deltas.
#[derive(Default, Debug, Clone)]
struct PeerTransferState {
    last_uploaded_bytes: u64,
    last_downloaded_bytes: u64,
}

/// Payload for the `payment_required` event.
#[derive(Debug, Clone, serde::Serialize)]
struct PaymentRequiredPayload {
    info_hash: String,
    peer_id: String,
    bytes_uploaded: u64,
    // In a real implementation, you'd also need the peer's wallet address.
    // This would be discovered during an initial handshake.
}
/// BitTorrent protocol handler implementing the ProtocolHandler trait.
/// This handler manages BitTorrent downloads and seeding operations using librqbit.
#[derive(Clone)]
pub struct BitTorrentHandler {
    rqbit_session: Arc<Session>,
    dht_service: Arc<DhtService>,
    download_directory: std::path::PathBuf,
    // NEW: Manage active torrents and their stats.
    active_torrents: Arc<tokio::sync::Mutex<HashMap<String, Arc<ManagedTorrent>>>>,
    peer_states: Arc<tokio::sync::Mutex<HashMap<String, HashMap<String, PeerTransferState>>>>,
    app_handle: Option<AppHandle>,
    event_bus: Option<Arc<TransferEventBus>>,
}

impl BitTorrentHandler {
    /// Creates a new BitTorrentHandler with the specified download directory.
    pub async fn new(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
    ) -> Result<Self, BitTorrentError> {
        Self::new_with_port_range(download_directory, dht_service, None).await
    }

    /// Creates a new BitTorrentHandler with a specific port range to avoid conflicts.
    pub async fn new_with_port_range(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        listen_port_range: Option<std::ops::Range<u16>>,
    ) -> Result<Self, BitTorrentError> {
        // Correctly call the main constructor, passing None for the app_handle.
        Self::new_with_port_range_and_app_handle(download_directory, dht_service, listen_port_range, None).await
    }

    /// Creates a new BitTorrentHandler with AppHandle for TransferEventBus integration.
    pub async fn new_with_app_handle(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        app_handle: AppHandle,
    ) -> Result<Self, BitTorrentError> {
        // Correctly call the main constructor, passing None for the port range and Some for the app_handle.
        Self::new_with_port_range_and_app_handle(download_directory, dht_service, None, Some(app_handle)).await
    }

    /// Creates a new BitTorrentHandler with all options.
    pub async fn new_with_port_range_and_app_handle(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        listen_port_range: Option<std::ops::Range<u16>>,
        app_handle: Option<AppHandle>,
    ) -> Result<Self, BitTorrentError> {
        info!(
            "Creating BitTorrent session with download_directory: {:?}, port_range: {:?}",
            download_directory, listen_port_range
        );

        // Clean up any stale DHT or session state files that might be locked
        let state_files = ["session.json", "dht.json", "dht.db", "session.db", "dht.dat"];
        for file in &state_files {
            let state_path = download_directory.join(file);
            if state_path.exists() {
                if let Err(e) = std::fs::remove_file(&state_path) {
                    warn!("Failed to remove stale state file {:?}: {}", state_path, e);
                } else {
                    info!("Removed stale state file: {:?}", state_path);
                }
            }
        }

        let mut opts = SessionOptions::default();

        // Set port range if provided
        if let Some(range) = listen_port_range.clone() {
            opts.listen_port_range = Some(range);
        }

        // Enable persistence for session and DHT state
        // This allows torrents to resume after app restart
        opts.persistence = Some(librqbit::SessionPersistenceConfig::Json {
            folder: Some(download_directory.clone()),
        });

        let session = Session::new_with_opts(download_directory.clone(), opts).await.map_err(|e| {
            error!("Session initialization failed: {}", e);
            BitTorrentError::SessionInit {
                message: format!("Failed to create session: {}", e),
            }
        })?;

        // Create TransferEventBus if app_handle is provided
        let event_bus = app_handle.as_ref().map(|handle| Arc::new(TransferEventBus::new(handle.clone())));

        let handler = Self {
            rqbit_session: session.clone(),
            dht_service,
            download_directory: download_directory.clone(),
            active_torrents: Default::default(),
            peer_states: Default::default(),
            app_handle,
            event_bus,
        };
        
        // Spawn the background task for statistics polling.
        handler.spawn_stats_poller();

        info!(
            "Initializing BitTorrentHandler with download directory: {:?}",
            handler.download_directory
        );
        Ok(handler)
    }

    /// Spawns a background task to periodically poll for and process per-peer statistics.
    fn spawn_stats_poller(&self) {
        let active_torrents = self.active_torrents.clone();
        let peer_states = self.peer_states.clone();
        let app_handle = self.app_handle.clone();
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                let torrents = active_torrents.lock().await;
                let mut states = peer_states.lock().await;

                for (info_hash_str, handle) in torrents.iter() {
                    // Use aggregate torrent stats instead of per-peer API (API surface varies between librqbit versions).
                    let stats = handle.stats();
                    let torrent_peer_states = states.entry(info_hash_str.clone()).or_default();
                    // Use a synthetic key for session-level accumulation when per-peer IDs are not available.
                    let session_key = "__session__".to_string();
                    let state = torrent_peer_states.entry(session_key.clone()).or_default();

                    let uploaded_total = stats.uploaded_bytes;
                    let downloaded_total = stats.progress_bytes;
                    let total_bytes = stats.total_bytes;

                    // Emit progress event via TransferEventBus
                    if let Some(ref bus) = event_bus {
                        let progress_pct = calculate_progress(downloaded_total, total_bytes);
                        let (download_speed, upload_speed) = if let Some(live) = &stats.live {
                            (
                                live.download_speed.mbps as f64 * 125_000.0,
                                live.upload_speed.mbps as f64 * 125_000.0,
                            )
                        } else {
                            (0.0, 0.0)
                        };
                        let eta = calculate_eta(
                            total_bytes.saturating_sub(downloaded_total),
                            download_speed,
                        );

                        bus.emit_progress(TransferProgressEvent {
                            transfer_id: info_hash_str.clone(),
                            downloaded_bytes: downloaded_total,
                            total_bytes,
                            completed_chunks: 0,
                            total_chunks: 0,
                            progress_percentage: progress_pct,
                            download_speed_bps: download_speed,
                            upload_speed_bps: upload_speed,
                            eta_seconds: eta,
                            active_sources: 1,
                            timestamp: current_timestamp_ms(),
                        });
                    }

                    let uploaded_delta = uploaded_total.saturating_sub(state.last_uploaded_bytes);
                    if uploaded_delta >= PAYMENT_THRESHOLD_BYTES {
                        info!(
                            info_hash = %info_hash_str,
                            bytes = uploaded_delta,
                            "Payment threshold reached for session"
                        );

                        let payload = PaymentRequiredPayload {
                            info_hash: info_hash_str.clone(),
                            peer_id: session_key.clone(),
                            bytes_uploaded: uploaded_delta,
                        };

                        if let Some(handle) = app_handle.as_ref() {
                            if let Err(e) = handle.emit("payment_required", payload) {
                                error!("Failed to emit payment_required event: {}", e);
                            }
                        } else {
                            warn!("No AppHandle available; skipping emit of payment_required event");
                        }

                        state.last_uploaded_bytes = uploaded_total;
                    }
                    state.last_downloaded_bytes = downloaded_total;
                }
            }
        });
    }

    /// Starts a download and returns a handle to the torrent.
    /// This method is non-blocking.
    pub async fn start_download(
        &self,
        identifier: &str,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        self.start_download_with_options(identifier, AddTorrentOptions::default())
            .await
    }

    /// Start a download with a custom output folder.
    pub async fn start_download_to(
        &self,
        identifier: &str,
        output_folder: PathBuf,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        let mut opts = AddTorrentOptions::default();
        opts.output_folder = Some(output_folder.to_string_lossy().to_string());
        self.start_download_with_options(identifier, opts).await
    }

    /// Re-evaluates the download queue, pausing or resuming torrents based on priority
    /// and the MAX_ACTIVE_DOWNLOADS limit.
    async fn re_evaluate_queue(&self) -> Result<(), BitTorrentError> {
        info!("Re-evaluating download queue...");
        let torrent_handles = self.active_torrents.lock().await;

        // Create a sorted list of torrents based on priority.
        // We need to collect stats for sorting.
        let mut all_torrents: Vec<_> = torrent_handles.values().map(|h| (h.clone(), h.stats())).collect();
        // A simple sort by state: downloading torrents first, then by other states.
        // A more robust implementation would use the priority from TorrentStateManager.
        // For now, we just find paused torrents to resume.
        all_torrents.sort_by_key(|(_, stats)| stats.state.to_string() != "Downloading");

        let mut active_downloads = all_torrents
            .iter()
            .filter(|(_, stats)| !stats.finished && stats.state.to_string() == "Downloading")
            .count();

        info!("Currently {} active downloads (limit is {}).", active_downloads, MAX_ACTIVE_DOWNLOADS);

        // If we have open slots, try to resume paused torrents.
        for (handle, stats) in all_torrents {
            if active_downloads >= MAX_ACTIVE_DOWNLOADS {
                break; // No more slots available.
            }

            // Find a paused torrent that is not finished and resume it.
            if !stats.finished && stats.state.to_string() == "Paused" {
                info!("Found paused torrent, resuming it to fill queue slot.");
                if self.rqbit_session.unpause(&handle).await.is_ok() {
                    active_downloads += 1;
                } else {
                    warn!("Failed to resume a paused torrent during queue re-evaluation.");
                }
            }
        }
        Ok(())
    }

    async fn start_download_with_options(
        &self,
        identifier: &str,
        add_opts: AddTorrentOptions,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        info!("Starting BitTorrent download for: {}", identifier);

        // Queueing Logic: Check if we should pause this new torrent.
        let active_downloads = {
            let torrents = self.active_torrents.lock().await;
            torrents.values().filter(|h| {
                let stats = h.stats();
                !stats.finished && stats.state.to_string() == "Downloading"
            }).count()
        };

        if active_downloads >= MAX_ACTIVE_DOWNLOADS {
            info!("Max active downloads ({}) reached. Adding new torrent in paused state.", MAX_ACTIVE_DOWNLOADS);
            add_opts.paused = true;
        }


        let add_torrent = if identifier.starts_with("magnet:") {
            Self::validate_magnet_link(identifier).map_err(|e| { //
                error!("Magnet link validation failed: {}", e);
                e
            })?;
            AddTorrent::from_url(identifier)
        } else {
            Self::validate_torrent_file(identifier).map_err(|e| {
                error!("Torrent file validation failed: {}", e);
                e
            })?;
            AddTorrent::from_local_filename(identifier).map_err(|e| {
                error!("Failed to load torrent file: {}", e);
                BitTorrentError::TorrentFileError {
                    message: format!("Cannot read torrent file {}: {}", identifier, e),
                }
            })?
        };

        // Temporarily get the info_hash to check for Chiral peers *before* adding the torrent.
        // This is a bit of a workaround as librqbit doesn't let us easily get the hash before adding.
        // We'll parse it from the magnet or torrent file.
        let temp_info_hash = if identifier.starts_with("magnet:") {
            crate::dht::parse_magnet_uri(identifier).map(|m| m.info_hash).ok()
        } else {
            // For .torrent files, we'll need to parse the info hash differently
            // We can read the file and parse it manually, or use the info_hash after adding
            // For now, we'll set to None and handle Chiral peer discovery after adding
            None
        };


        // Check for Chiral peers but don't require exclusive mode
        if let Some(hash) = &temp_info_hash {
            match self.dht_service.search_peers_by_infohash(hash.clone()).await {
                Ok(chiral_peer_ids) if !chiral_peer_ids.is_empty() => {
                    info!("Found {} Chiral peers for {}. They will be discovered via DHT.", 
                          chiral_peer_ids.len(), hash);
                }
                Ok(_) => info!("No Chiral peers found for {}.", hash),
                Err(e) => warn!("Chiral peer search failed: {}", e),
            }
        }

        // Add the torrent to the session
        let add_torrent_response = self
            .rqbit_session
            .add_torrent(add_torrent, Some(add_opts))
            .await
            .map_err(|e| {
                error!("Failed to add torrent to session: {}", e);
                Self::map_generic_error(e)
            })?;

        let handle = add_torrent_response
            .into_handle()
            .ok_or(BitTorrentError::HandleUnavailable)?;

        // Get the info_hash from the handle (works for both magnets and .torrent files)
        let torrent_info_hash = handle.info_hash();
        let hash_hex = hex::encode(torrent_info_hash.0);

        // Store the torrent handle for tracking
        {
            let mut torrents = self.active_torrents.lock().await;
            torrents.insert(hash_hex.clone(), handle.clone());
        }

        Ok(handle)
    }

    /// Monitors a torrent download and sends progress events.
    pub async fn monitor_download(
        &self,
        handle: Arc<ManagedTorrent>,
        event_tx: mpsc::Sender<BitTorrentEvent>,
    ) {
        let mut interval = time::interval(Duration::from_secs(1));
        let mut no_progress_count = 0;
        const MAX_NO_PROGRESS_ITERATIONS: u32 = 300; // 5 minutes with 1-second intervals

        loop {
            interval.tick().await;
            let stats = handle.stats();
            let downloaded = stats.progress_bytes;
            let total = stats.total_bytes;

            if event_tx.is_closed() {
                error!("Failed to send progress event, receiver dropped.");
                return;
            }

            if let Err(_) = event_tx
                .send(BitTorrentEvent::Progress { downloaded, total })
                .await
            {
                error!("Failed to send progress event, receiver dropped.");
                return;
            }

            // Check for completion
            if total > 0 && downloaded >= total {
                info!("Download completed for torrent");
                let _ = event_tx.send(BitTorrentEvent::Completed).await;
                // Re-evaluate the queue to start the next download.
                let _ = self.re_evaluate_queue().await;
                return;
            }

            // Check for timeout (no progress for extended period)
            if downloaded == 0 {
                no_progress_count += 1;
                if no_progress_count >= MAX_NO_PROGRESS_ITERATIONS {
                    error!(
                        "Download timeout: no progress after {} seconds",
                        MAX_NO_PROGRESS_ITERATIONS
                    );
                    let _ = event_tx
                        .send(BitTorrentEvent::Failed(BitTorrentError::DownloadTimeout {
                            timeout_secs: MAX_NO_PROGRESS_ITERATIONS as u64,
                        }))
                        .await;
                    return;
                }
            } else {
                no_progress_count = 0; // Reset counter when progress is made
            }
        }
    }

    /// Validate magnet link format
    fn validate_magnet_link(url: &str) -> Result<(), BitTorrentError> {
        if !url.starts_with("magnet:?xt=urn:btih:") {
            return Err(BitTorrentError::InvalidMagnetLink {
                url: url.to_string(),
            });
        }

        // Extract info hash to validate length
        if let Some(hash_start) = url.find("urn:btih:") {
            let hash_start = hash_start + 9; // Length of "urn:btih:"
            let hash_end = url[hash_start..]
                .find('&')
                .unwrap_or(url.len() - hash_start)
                + hash_start;
            let hash = &url[hash_start..hash_end];

            // Check hash length (40 chars for SHA-1, 64 for SHA-256)
            if hash.len() != 40 && hash.len() != 64 {
                return Err(BitTorrentError::InvalidMagnetLink {
                    url: url.to_string(),
                });
            }

            // Check if hash contains only hex characters
            if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(BitTorrentError::InvalidMagnetLink {
                    url: url.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Validate torrent file path
    fn validate_torrent_file(path: &str) -> Result<(), BitTorrentError> {
        let file_path = Path::new(path);

        if !file_path.exists() {
            return Err(BitTorrentError::TorrentFileError {
                message: format!("Torrent file not found: {}", path),
            });
        }

        if !file_path.is_file() {
            return Err(BitTorrentError::TorrentFileError {
                message: format!("Path is not a file: {}", path),
            });
        }

        if !path.ends_with(".torrent") {
            return Err(BitTorrentError::TorrentFileError {
                message: format!("File does not have .torrent extension: {}", path),
            });
        }

        Ok(())
    }

    /// Map generic errors to our custom error type
    fn map_generic_error(error: impl std::fmt::Display) -> BitTorrentError {
        let error_msg = error.to_string();
        if error_msg.contains("network") || error_msg.contains("connection") {
            BitTorrentError::NetworkError { message: error_msg }
        } else if error_msg.contains("timeout") {
            BitTorrentError::DownloadTimeout { timeout_secs: 30 }
        } else if error_msg.contains("parse") || error_msg.contains("invalid") {
            BitTorrentError::TorrentParsingError { message: error_msg }
        } else {
            BitTorrentError::Unknown { message: error_msg }
        }
    }

    
}

/// Helper to convert a libp2p Multiaddr to a standard SocketAddr.
/// This is a simplified conversion that only handles TCP/IP.
fn multiaddr_to_socket_addr(multiaddr: &Multiaddr) -> Result<std::net::SocketAddr, &'static str> {
    use libp2p::multiaddr::Protocol;

    let mut iter = multiaddr.iter();
    let proto1 = iter.next().ok_or("Empty Multiaddr")?;
    let proto2 = iter.next().ok_or("Multiaddr needs at least two protocols")?;
    
    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(std::net::SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(std::net::SocketAddr::new(ip.into(), port)),
        _ => Err("Multiaddr format not supported (expected IP/TCP)"),
    }
}

#[async_trait]
impl SimpleProtocolHandler for BitTorrentHandler {
    fn name(&self) -> &'static str {
        "bittorrent"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("magnet:") || identifier.ends_with(".torrent")
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn download(&self, identifier: &str) -> Result<(), String> {
        let handle = self.start_download(identifier).await?;
        let self_arc = Arc::new(self.clone());
        let (tx, mut rx) = mpsc::channel(10);
        tokio::spawn(async move {
            self_arc.monitor_download(handle, tx).await;
        });

        while let Some(event) = rx.recv().await {
            match event {
                BitTorrentEvent::Completed => return Ok(()),
                BitTorrentEvent::Failed(e) => return Err(e.into()),
                _ => {}
            }
        }
        // If the loop exits, it means the channel was closed without a final event.
        Err("Monitoring channel closed unexpectedly.".to_string())
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn seed(&self, file_path: &str) -> Result<String, String> {
        let path = Path::new(file_path);
        if !path.exists() {
            let error = BitTorrentError::FileSystemError {
                message: format!("File does not exist: {}", file_path),
            };
            error!("Seeding failed: {}", error);
            return Err(error.into());
        }

        if !path.is_file() {
            let error = BitTorrentError::FileSystemError {
                message: format!("Path is not a file: {}", file_path),
            };
            error!("Seeding failed: {}", error);
            return Err(error.into());
        }

        // Create a torrent from the file
        let torrent = create_torrent(path, CreateTorrentOptions::default()).await.map_err(|e| {
            let error = BitTorrentError::SeedingError {
                message: format!("Failed to create torrent from file {}: {}", file_path, e),
            };
            error!("Torrent creation failed: {}", e);
            error!("Seeding failed: {}", error);
            String::from(error)
        })?;

        // Convert the torrent to bytes and create AddTorrent
        let torrent_bytes = torrent.as_bytes().map_err(|e| {
            let error = BitTorrentError::SeedingError {
                message: format!("Failed to serialize torrent for {}: {}", file_path, e),
            };
            error!("Torrent serialization failed: {}", e);
            error!("Seeding failed: {}", error);
            String::from(error)
        })?;

        let add_torrent = AddTorrent::from_bytes(torrent_bytes.clone());

        // For seeding, we need to allow overwriting existing files
        let options = AddTorrentOptions {
            overwrite: true,
            ..Default::default()
        };

        let handle = self
            .rqbit_session
            .add_torrent(add_torrent, Some(options))
            .await
            .map_err(|e| {
                let error = BitTorrentError::SeedingError {
                    message: format!("Failed to add torrent for seeding: {}", e),
                };
                error!("Failed to add torrent to session: {}", e);
                error!("Seeding failed: {}", error);
                String::from(error)
            })?
            .into_handle()
            .ok_or_else(|| String::from(BitTorrentError::HandleUnavailable))?;

        // Get the info hash and construct a magnet link
        let info_hash = handle.info_hash();
        let magnet_link = format!("magnet:?xt=urn:btih:{}", hex::encode(info_hash.0));

        Ok(magnet_link)
    }
}

// Pause/Resume/Cancel methods for torrent control
impl BitTorrentHandler {
    /// Pause a torrent by info hash
    pub async fn pause_torrent(&self, info_hash: &str) -> Result<(), BitTorrentError> {
        info!("Pausing torrent: {}", info_hash);

        let torrents = self.active_torrents.lock().await;
        if let Some(handle) = torrents.get(info_hash) {
            let stats = handle.stats();
            self.rqbit_session
                .pause(handle)
                .await
                .map_err(|e| BitTorrentError::ProtocolSpecific {
                    message: format!("Failed to pause torrent: {}", e),
                })?;

            // Emit paused event via TransferEventBus
            if let Some(ref bus) = self.event_bus {
                bus.emit_paused(TransferPausedEvent {
                    transfer_id: info_hash.to_string(),
                    paused_at: current_timestamp_ms(),
                    reason: PauseReason::UserRequested,
                    can_resume: true,
                    downloaded_bytes: stats.progress_bytes,
                    total_bytes: stats.total_bytes,
                });
            }

            info!("Successfully paused torrent: {}", info_hash);
            Ok(())
        } else {
            Err(BitTorrentError::TorrentNotFound {
                info_hash: info_hash.to_string(),
            })
        }
    }

    /// Resume a paused torrent by info hash
    pub async fn resume_torrent(&self, info_hash: &str) -> Result<(), BitTorrentError> {
        info!("Resuming torrent: {}", info_hash);

        let torrents = self.active_torrents.lock().await;
        if let Some(handle) = torrents.get(info_hash) {
            let stats = handle.stats();
            self.rqbit_session
                .unpause(handle)
                .await
                .map_err(|e| BitTorrentError::ProtocolSpecific {
                    message: format!("Failed to resume torrent: {}", e),
                })?;

            // Emit resumed event via TransferEventBus
            if let Some(ref bus) = self.event_bus {
                bus.emit_resumed(TransferResumedEvent {
                    transfer_id: info_hash.to_string(),
                    resumed_at: current_timestamp_ms(),
                    downloaded_bytes: stats.progress_bytes,
                    remaining_bytes: stats.total_bytes.saturating_sub(stats.progress_bytes),
                    active_sources: 1,
                });
            }

            info!("Successfully resumed torrent: {}", info_hash);
            Ok(())
        } else {
            Err(BitTorrentError::TorrentNotFound {
                info_hash: info_hash.to_string(),
            })
        }
    }

    /// Cancel/remove a torrent by info hash
    pub async fn cancel_torrent(&self, info_hash: &str, delete_files: bool) -> Result<(), BitTorrentError> {
        info!("Cancelling torrent: {} (delete_files: {})", info_hash, delete_files);
        
        // Remove from our tracking first
        let handle = {
            let mut torrents = self.active_torrents.lock().await;
            torrents.remove(info_hash)
        };
        
        if let Some(handle) = handle {
            // Use the torrent's ID for deletion
            let torrent_id = handle.id();
            self.rqbit_session
                .delete(torrent_id.into(), delete_files)
                .await
                .map_err(|e| BitTorrentError::ProtocolSpecific {
                    message: format!("Failed to cancel torrent: {}", e),
                })?;

            // Re-evaluate the queue as a slot may have opened up.
            self.re_evaluate_queue().await?;

            info!("Successfully cancelled torrent: {}", info_hash);
            Ok(())
        } else {
            Err(BitTorrentError::TorrentNotFound {
                info_hash: info_hash.to_string(),
            })
        }
    }

    /// Stop seeding a torrent (same as cancel but specifically for seeding)
    pub async fn stop_seeding_torrent(&self, info_hash: &str) -> Result<(), BitTorrentError> {
        info!("Stopping seeding for torrent: {}", info_hash);
        // For seeding, we just cancel without deleting files
        self.cancel_torrent(info_hash, false).await
    }

    /// Get progress information for a torrent
    pub async fn get_torrent_progress(&self, info_hash: &str) -> Result<TorrentProgress, BitTorrentError> {
        let torrents = self.active_torrents.lock().await;
        
        if let Some(handle) = torrents.get(info_hash) {
            let stats = handle.stats();
            
            // Extract download/upload speed from live stats if available
            // Speed is in Mbps, convert to bytes/sec (Mbps * 1_000_000 / 8)
            let (download_speed, upload_speed, eta_seconds) = if let Some(live) = &stats.live {
                let download_speed = live.download_speed.mbps as f64 * 125_000.0; // Mbps to bytes/sec
                let upload_speed = live.upload_speed.mbps as f64 * 125_000.0;
                // time_remaining is a DurationWithHumanReadable, extract seconds if available
                let eta = live.average_piece_download_time.map(|d| {
                    if stats.total_bytes > stats.progress_bytes {
                        let remaining = stats.total_bytes - stats.progress_bytes;
                        let speed_bps = download_speed.max(1.0);
                        (remaining as f64 / speed_bps) as u64
                    } else {
                        0
                    }
                });
                (download_speed, upload_speed, eta)
            } else {
                (0.0, 0.0, None)
            };
            
            Ok(TorrentProgress {
                downloaded_bytes: stats.progress_bytes,
                uploaded_bytes: stats.uploaded_bytes,
                total_bytes: stats.total_bytes,
                download_speed,
                upload_speed,
                eta_seconds,
                is_finished: stats.finished,
                state: format!("{}", stats.state),
            })
        } else {
            Err(BitTorrentError::TorrentNotFound {
                info_hash: info_hash.to_string(),
            })
        }
    }
}

/// Progress information for a torrent
#[derive(Debug, Clone)]
pub struct TorrentProgress {
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub total_bytes: u64,
    pub download_speed: f64,
    pub upload_speed: f64,
    pub eta_seconds: Option<u64>,
    pub is_finished: bool,
    pub state: String,
}

// Helper functions for error mapping and validation
impl BitTorrentHandler {
    /// Check if string is a valid magnet link
    pub fn is_magnet_link(url: &str) -> bool {
        Self::validate_magnet_link(url).is_ok()
    }

    /// Check if path points to a valid torrent file
    pub fn is_torrent_file(path: &str) -> bool {
        Self::validate_torrent_file(path).is_ok()
    }

    /// Extract info hash from magnet link
    pub fn extract_info_hash(magnet: &str) -> Option<String> {
        if let Ok(_) = Self::validate_magnet_link(magnet) {
            if let Some(hash_start) = magnet.to_lowercase().find("urn:btih:") {
                let hash_start = hash_start + 9;
                let hash_end = magnet[hash_start..]
                    .find('&')
                    .unwrap_or(magnet.len() - hash_start)
                    + hash_start;
                Some(magnet[hash_start..hash_end].to_string())
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;
    use std::io::Write;

    fn create_test_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
        let file_path = dir.join(name);
        let mut file = File::create(&file_path).unwrap();
        write!(file, "{}", content).unwrap();
        file_path
    }

    #[test]
    fn test_validate_magnet_link_valid() {
        assert!(BitTorrentHandler::validate_magnet_link(
            "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678"
        )
        .is_ok());
        assert!(BitTorrentHandler::validate_magnet_link(
            "magnet:?xt=urn:btih:ABCDEF1234567890ABCDEF1234567890ABCDEF12&dn=test"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_magnet_link_invalid() {
        assert!(BitTorrentHandler::validate_magnet_link("http://example.com").is_err());
        assert!(BitTorrentHandler::validate_magnet_link("magnet:?xt=urn:btih:invalid").is_err());
        assert!(BitTorrentHandler::validate_magnet_link("magnet:?xt=urn:btih:123").is_err());
        // Too short
    }

    #[test]
    fn test_validate_torrent_file() {
        let temp_dir = tempdir().unwrap();
        let torrent_path = create_test_file(temp_dir.path(), "test.torrent", "content");

        assert!(
            BitTorrentHandler::validate_torrent_file(torrent_path.to_str().unwrap()).is_ok()
        );
        assert!(BitTorrentHandler::validate_torrent_file("/nonexistent/file.torrent").is_err());

        let txt_path = create_test_file(temp_dir.path(), "test.txt", "content");
        assert!(BitTorrentHandler::validate_torrent_file(txt_path.to_str().unwrap()).is_err());
    }

    #[test]
    fn test_persistent_torrent_serialization_round_trip() {
        // Test with a Magnet link source
        let original_magnet = PersistentTorrent {
            info_hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
            source: PersistentTorrentSource::Magnet("magnet:?xt=urn:btih:a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string()),
            output_path: PathBuf::from("/downloads/test_magnet"),
            status: PersistentTorrentStatus::Downloading,
            added_at: 1678886400,
            priority: 0, // Add priority field
        };

        let serialized_magnet = serde_json::to_string_pretty(&original_magnet).unwrap();
        println!("Serialized Magnet Torrent:\n{}", serialized_magnet);

        let deserialized_magnet: PersistentTorrent = serde_json::from_str(&serialized_magnet).unwrap();

        assert_eq!(original_magnet.info_hash, deserialized_magnet.info_hash);
        assert_eq!(original_magnet.source, deserialized_magnet.source);
        assert_eq!(original_magnet.output_path, deserialized_magnet.output_path);
        assert_eq!(original_magnet.status, deserialized_magnet.status);
        assert_eq!(original_magnet.added_at, deserialized_magnet.added_at);
        // Full struct comparison
        assert_eq!(
            serde_json::to_string(&original_magnet).unwrap(),
            serde_json::to_string(&deserialized_magnet).unwrap()
        );

        // Test with a File source
        let original_file = PersistentTorrent {
            info_hash: "f1e2d3c4b5a6f1e2d3c4b5a6f1e2d3c4b5a6f1e2".to_string(),
            source: PersistentTorrentSource::File(PathBuf::from("/torrents/test.torrent")),
            output_path: PathBuf::from("/downloads/test_file"),
            status: PersistentTorrentStatus::Seeding,
            added_at: 1678887400,
            priority: 1, // Add priority field
        };

        let serialized_file = serde_json::to_string_pretty(&original_file).unwrap();
        println!("Serialized File Torrent:\n{}", serialized_file);

        let deserialized_file: PersistentTorrent = serde_json::from_str(&serialized_file).unwrap();

        assert_eq!(original_file.info_hash, deserialized_file.info_hash);
        assert_eq!(original_file.source, deserialized_file.source);
        assert_eq!(original_file.output_path, deserialized_file.output_path);
        assert_eq!(original_file.status, deserialized_file.status);
        assert_eq!(original_file.added_at, deserialized_file.added_at);
        // Full struct comparison
        assert_eq!(
            serde_json::to_string(&original_file).unwrap(),
            serde_json::to_string(&deserialized_file).unwrap()
        );
    }


    #[tokio::test]
    #[ignore] // Ignored by default as it performs a real network download
    async fn test_integration_download_public_torrent() {
        let temp_dir = tempdir().expect("Failed to create temp directory for download");
        let download_path = temp_dir.path().to_path_buf();

        // Create a DHT service for the test
        let dht_service = Arc::new(
            DhtService::new(
                0,                            // Random port
                vec![],                       // No bootstrap nodes for this test
                None,                         // No identity secret
                false,                        // Not bootstrap node
                false,                        // Disable AutoNAT for test
                None,                         // No autonat probe interval
                vec![],                       // No custom AutoNAT servers
                None,                         // No proxy
                None,                         // No file transfer service
                None,                         // No chunk manager
                Some(256),                    // chunk_size_kb
                Some(1024),                   // cache_size_mb
                false,                        // enable_autorelay
                Vec::new(),                   // preferred_relays
                false,                        // enable_relay_server
                false,                        // enable_upnp
                None,                         // blockstore_db_path
                None,
                None,
            )
            .await
            .expect("Failed to create DHT service for test"),
        );

        // Use a specific port range to avoid conflicts if other tests run in parallel
        let handler = BitTorrentHandler::new_with_port_range(download_path.clone(), dht_service, Some(31000..32000))
            .await
            .expect("Failed to create BitTorrentHandler");

        // A small, well-seeded, and legal torrent for testing (e.g., a public domain text file)
        let magnet_link = "magnet:?xt=urn:btih:a8a823138a32856187539439325938e3f2a1e2e3&dn=The.WIRED.Book-sample.pdf";

        let handle = handler
            .start_download(magnet_link)
            .await
            .expect("Failed to start download");

        let (tx, mut rx) = mpsc::channel(100);

        // Spawn the monitor in the background
        tokio::spawn(async move {
            handler.monitor_download(handle, tx).await;
        });

        // Wait for completion or failure
        let mut final_event: Option<BitTorrentEvent> = None;
        let timeout_duration = Duration::from_secs(300); // 5-minute timeout

        match time::timeout(timeout_duration, async {
            while let Some(event) = rx.recv().await {
                if matches!(event, BitTorrentEvent::Completed | BitTorrentEvent::Failed(_)) {
                    final_event = Some(event);
                    break;
                }
            }
        }).await {
            Ok(_) => assert!(matches!(final_event, Some(BitTorrentEvent::Completed)), "Download did not complete successfully. Last event: {:?}", final_event),
            Err(_) => panic!("Download timed out after {} seconds", timeout_duration.as_secs()),
        }
    }

    #[tokio::test]
    #[ignore] // Ignored by default: real network download of a ~50MB file.
    async fn test_integration_protocol_handler_download_linux_distro() {
        let temp_dir = tempdir().expect("Failed to create temp directory for download");
        let download_path = temp_dir.path().to_path_buf();

        // Create a DHT service for the test
        let dht_service = Arc::new(
            DhtService::new(
                0,                            // Random port
                vec![],                       // No bootstrap nodes for this test
                None,                         // No identity secret
                false,                        // Not bootstrap node
                false,                        // Disable AutoNAT for test
                None,                         // No autonat probe interval
                vec![],                       // No custom AutoNAT servers
                None,                         // No proxy
                None,                         // No file transfer service
                None,                         // No chunk manager
                Some(256),                    // chunk_size_kb
                Some(1024),                   // cache_size_mb
                false,                        // enable_autorelay
                Vec::new(),                   // preferred_relays
                false,                        // enable_relay_server
                false,                        // enable_upnp
                None,                         // blockstore_db_path
                None,
                None,
            )
            .await
            .expect("Failed to create DHT service for test"),
        );

        // Use a specific port range to avoid conflicts
        let handler = BitTorrentHandler::new_with_port_range(download_path.clone(), dht_service, Some(33000..34000))
            .await
            .expect("Failed to create BitTorrentHandler");

        // A small, well-seeded, and legal torrent for a Linux distro (~50MB)
        let magnet_link = "magnet:?xt=urn:btih:a24f6cb6c62b23c235a2889c0c8e65f4350100d0&dn=slitaz-rolling.iso";

        // The download() method from the trait handles the full lifecycle.
        // We'll wrap it in a timeout to prevent the test from running indefinitely.
        let timeout_duration = Duration::from_secs(600); // 10-minute timeout

        let result = time::timeout(timeout_duration, handler.download(magnet_link)).await;

        // Check for timeout first
        assert!(result.is_ok(), "Download timed out after {} seconds", timeout_duration.as_secs());

        // Check if the download method itself returned Ok
        let download_result = result.unwrap();
        assert!(download_result.is_ok(), "Download failed with error: {:?}", download_result.err());

        // Verify that the file was actually created
        assert!(download_path.join("slitaz-rolling.iso").exists(), "Downloaded file does not exist");
    }

    #[tokio::test]
    #[ignore] // Ignored by default as it involves file I/O and a real session
    async fn test_integration_seed_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path = create_test_file(temp_dir.path(), "seed_me.txt", "hello world seeding test");

        // Create a DHT service for the test
        let dht_service = Arc::new(
            DhtService::new(
                0,                            // Random port
                vec![],                       // No bootstrap nodes for this test
                None,                         // No identity secret
                false,                        // Not bootstrap node
                false,                        // Disable AutoNAT for test
                None,                         // No autonat probe interval
                vec![],                       // No custom AutoNAT servers
                None,                         // No proxy
                None,                         // No file transfer service
                None,                         // No chunk manager
                Some(256),                    // chunk_size_kb
                Some(1024),                   // cache_size_mb
                false,                        // enable_autorelay
                Vec::new(),                   // preferred_relays
                false,                        // enable_relay_server
                false,                        // enable_upnp
                None,                         // blockstore_db_path
                None,
                None,
            )
            .await
            .expect("Failed to create DHT service for test"),
        );

        // Use a specific port range to avoid conflicts
        let handler = BitTorrentHandler::new_with_port_range(temp_dir.path().to_path_buf(), dht_service, Some(32000..33000))
            .await
            .expect("Failed to create BitTorrentHandler");

        let magnet_link = handler
            .seed(file_path.to_str().unwrap())
            .await
            .expect("Seeding failed");

        // Validate the magnet link
        assert!(
            magnet_link.starts_with("magnet:?xt=urn:btih:"),
            "Invalid magnet link generated: {}",
            magnet_link
        );

        // Check that the torrent is now managed by the session
        let torrent_count = handler.rqbit_session.with_torrents(|torrents| torrents.count());
        assert_eq!(torrent_count, 1, "Torrent was not added to the session");
    }

    #[test]
    fn test_error_user_messages() {
        let error = BitTorrentError::InvalidMagnetLink {
            url: "invalid".to_string(),
        };
        assert!(error.user_message().contains("magnet link format"));

        let error = BitTorrentError::NetworkError {
            message: "connection failed".to_string(),
        };
        assert!(error.user_message().contains("Network connection failed"));
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(
            BitTorrentError::InvalidMagnetLink {
                url: "test".to_string()
            }
            .category(),
            "validation"
        );
        assert_eq!(
            BitTorrentError::NetworkError {
                message: "test".to_string()
            }
            .category(),
            "network"
        );
        assert_eq!(
            BitTorrentError::FileSystemError {
                message: "test".to_string()
            }
            .category(),
            "filesystem"
        );
    }

    #[test]
    fn test_multiaddr_to_socket_addr() {
        // IPv4 test
        let multiaddr_ipv4: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        let socket_addr_ipv4 = multiaddr_to_socket_addr(&multiaddr_ipv4).unwrap();
        assert_eq!(
            socket_addr_ipv4,
            "127.0.0.1:8080".parse::<std::net::SocketAddr>().unwrap()
        );

        // IPv6 test
        let multiaddr_ipv6: Multiaddr = "/ip6/::1/tcp/8080".parse().unwrap();
        let socket_addr_ipv6 = multiaddr_to_socket_addr(&multiaddr_ipv6).unwrap();
        assert_eq!(
            socket_addr_ipv6,
            "[::1]:8080".parse::<std::net::SocketAddr>().unwrap()
        );

        // Invalid format (DNS)
        let multiaddr_dns: Multiaddr = "/dns/localhost/tcp/8080".parse().unwrap();
        assert!(multiaddr_to_socket_addr(&multiaddr_dns).is_err());

        // Invalid format (UDP)
        let multiaddr_udp: Multiaddr = "/ip4/127.0.0.1/udp/8080".parse().unwrap();
        assert!(multiaddr_to_socket_addr(&multiaddr_udp).is_err());

        // Too short
        let multiaddr_short: Multiaddr = "/ip4/127.0.0.1".parse().unwrap();
        assert!(multiaddr_to_socket_addr(&multiaddr_short).is_err());

        // Empty
        let multiaddr_empty: Multiaddr = "".parse().unwrap();
        assert!(multiaddr_to_socket_addr(&multiaddr_empty).is_err());
    }
}

#[cfg(test)]
mod torrent_state_manager_tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_torrent_state_manager_new_and_load_empty_file() {
        let temp_dir = tempdir().unwrap();
        let state_file_path = temp_dir.path().join("torrent_state.json");

        // Manager should initialize with an empty list if file doesn't exist
        let manager = TorrentStateManager::new(state_file_path.clone());
        assert!(manager.torrents.is_empty());
        assert!(!state_file_path.exists()); // File should not be created on new if empty
    }

    #[test]
    fn test_torrent_state_manager_save_and_load() {
        let temp_dir = tempdir().unwrap();
        let state_file_path = temp_dir.path().join("torrent_state.json");

        let mut manager = TorrentStateManager::new(state_file_path.clone());

        let torrent1 = PersistentTorrent {
            info_hash: "hash1".to_string(),
            source: PersistentTorrentSource::Magnet("magnet1".to_string()),
            output_path: PathBuf::from("/downloads/torrent1"),
            status: PersistentTorrentStatus::Downloading,
            added_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            priority: 0,
        };
        let torrent2 = PersistentTorrent {
            info_hash: "hash2".to_string(),
            source: PersistentTorrentSource::File(PathBuf::from("/path/to/file2.torrent")),
            output_path: PathBuf::from("/downloads/torrent2"),
            status: PersistentTorrentStatus::Seeding,
            added_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 100,
            priority: 1,
        };

        manager.torrents.insert(torrent1.info_hash.clone(), torrent1.clone());
        manager.torrents.insert(torrent2.info_hash.clone(), torrent2.clone());

        // Save the state
        manager.save().unwrap();
        assert!(state_file_path.exists());

        // Verify content of the saved file
        let saved_content = fs::read_to_string(&state_file_path).unwrap();
        let loaded_from_file: Vec<PersistentTorrent> = serde_json::from_str(&saved_content).unwrap();
        assert_eq!(loaded_from_file.len(), 2);
        assert!(loaded_from_file.contains(&torrent1));
        assert!(loaded_from_file.contains(&torrent2));

        // Create a new manager and load the state
        let loaded_manager = TorrentStateManager::new(state_file_path.clone());
        assert_eq!(loaded_manager.torrents.len(), 2);
        assert_eq!(loaded_manager.torrents.get("hash1").unwrap(), &torrent1);
        assert_eq!(loaded_manager.torrents.get("hash2").unwrap(), &torrent2);
    }

    #[test]
    fn test_torrent_state_manager_get_all() {
        let temp_dir = tempdir().unwrap();
        let state_file_path = temp_dir.path().join("torrent_state.json");

        let mut manager = TorrentStateManager::new(state_file_path.clone());

        let torrent1 = PersistentTorrent {
            info_hash: "hash1".to_string(),
            source: PersistentTorrentSource::Magnet("magnet1".to_string()),
            output_path: PathBuf::from("/downloads/torrent1"),
            status: PersistentTorrentStatus::Downloading,
            added_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            priority: 0,
        };
        let torrent2 = PersistentTorrent {
            info_hash: "hash2".to_string(),
            source: PersistentTorrentSource::File(PathBuf::from("/path/to/file2.torrent")),
            output_path: PathBuf::from("/downloads/torrent2"),
            status: PersistentTorrentStatus::Seeding,
            added_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 100,
            priority: 1,
        };

        manager.torrents.insert(torrent1.info_hash.clone(), torrent1.clone());
        manager.torrents.insert(torrent2.info_hash.clone(), torrent2.clone());

        let all_torrents = manager.get_all();
        assert_eq!(all_torrents.len(), 2);
        assert!(all_torrents.contains(&torrent1));
        assert!(all_torrents.contains(&torrent2));
    }

    // Test for malformed JSON file (should load empty or return error, depending on desired behavior)
    // Current implementation logs a warning and returns empty, which is good.
}


