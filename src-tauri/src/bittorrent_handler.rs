use crate::chiral_bittorrent_extension::{ChiralBitTorrentExtension, ChiralExtensionEvent};
use crate::dht::DhtService;
use crate::manager::ChunkManager;
use crate::protocols::SimpleProtocolHandler;
use crate::transfer_events::{
    calculate_eta, calculate_progress, current_timestamp_ms, PauseReason, TransferEventBus,
    TransferPausedEvent, TransferProgressEvent, TransferResumedEvent,
};
use async_trait::async_trait;
use libp2p::Multiaddr;
use librqbit::{
    create_torrent, torrent_from_bytes, AddTorrent, AddTorrentOptions, CreateTorrentOptions,
    ManagedTorrent, Session, SessionOptions,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::{error, info, instrument, warn};

fn is_private_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let oct = v4.octets();
            matches!(oct[0], 10 | 127)
                || (oct[0] == 192 && oct[1] == 168)
                || (oct[0] == 172 && (16..=31).contains(&oct[1]))
        }
        // For E2E we prefer IPv4 anyway; treat IPv6 as non-private here.
        std::net::IpAddr::V6(_) => false,
    }
}

fn get_e2e_bittorrent_seeder_port() -> u16 {
    std::env::var("E2E_BITTORRENT_SEED_PORT")
        .or_else(|_| std::env::var("CHIRAL_BITTORRENT_SEED_PORT"))
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(30000)
}

fn append_magnet_x_pe(identifier: &str, ip: std::net::IpAddr, port: u16) -> String {
    // Common magnet extension used by some clients: x.pe=IP:PORT
    // We only add it if it's not already present.
    if identifier.contains("x.pe=") {
        return identifier.to_string();
    }
    let sep = if identifier.contains('?') { "&" } else { "?" };
    format!("{}{}x.pe={}:{}", identifier, sep, ip, port)
}

fn ensure_initial_peer(add_opts: &mut AddTorrentOptions, ip: std::net::IpAddr, port: u16) {
    // librqbit supports deterministic peer injection via AddTorrentOptions.initial_peers.
    // This is more reliable than relying on magnet extensions like x.pe.
    if add_opts.initial_peers.as_ref().is_some_and(|v| !v.is_empty()) {
        return;
    }
    add_opts.initial_peers = Some(vec![SocketAddr::new(ip, port)]);
}

async fn maybe_inject_e2e_initial_peer(
    dht_service: &DhtService,
    info_hash_hex: &str,
    add_opts: &mut AddTorrentOptions,
) {
    // Only in E2E attach/headless contexts.
    if !(std::env::var("CHIRAL_E2E_API_PORT").ok().is_some()
        || std::env::var("E2E_ATTACH").ok().as_deref() == Some("true"))
    {
        return;
    }

    let port = get_e2e_bittorrent_seeder_port();

    // 1) Prefer explicit public IP hint from env (VM uploader public IP).
    if let Ok(ip_str) =
        std::env::var("CHIRAL_PUBLIC_IP").or_else(|_| std::env::var("E2E_UPLOADER_PUBLIC_IP"))
    {
        if let Ok(ip) = ip_str.trim().parse::<std::net::IpAddr>() {
            if !is_private_ip(&ip) {
                ensure_initial_peer(add_opts, ip, port);
                info!(
                    "E2E: injecting initial_peers {}:{} (from env) for torrent {}",
                    ip, port, info_hash_hex
                );
                return;
            }
        }
    }

    // 2) Fallback: derive a usable IP from Chiral DHT peer multiaddrs.
    if let Ok(peer_ids) = dht_service.search_peers_by_infohash(info_hash_hex.to_string()).await {
        if peer_ids.is_empty() {
            return;
        }
        if let Ok(addr_map) = dht_service.get_peer_addresses(peer_ids).await {
            let mut selected_ip: Option<std::net::IpAddr> = None;
            'outer: for (_pid, addrs) in addr_map {
                for a in addrs {
                    if let Ok(ma) = a.parse::<Multiaddr>() {
                        if let Ok(sock) = multiaddr_to_socket_addr(&ma) {
                            let ip = sock.ip();
                            if !is_private_ip(&ip) {
                                selected_ip = Some(ip);
                                break 'outer;
                            }
                            if selected_ip.is_none() {
                                selected_ip = Some(ip);
                            }
                        }
                    }
                }
            }
            if let Some(ip) = selected_ip {
                ensure_initial_peer(add_opts, ip, port);
                info!(
                    "E2E: injecting initial_peers {}:{} (from DHT peer addresses) for torrent {}",
                    ip, port, info_hash_hex
                );
            }
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
    IoError { message: String },

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
        BitTorrentError::IoError {
            message: err.to_string(),
        }
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
#[serde(rename_all = "lowercase")]
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

    /// The name of the torrent, usually derived from the torrent file or magnet link.
    pub name: Option<String>,

    /// The total size of the torrent's content in bytes.
    pub size: Option<u64>,

    /// The priority of the download.
    pub priority: u32,
}

impl PersistentTorrent {
    /// Helper to get the current Unix timestamp in seconds.
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// The top-level struct that is serialized to the state file.
#[derive(Serialize, Deserialize, Debug, Default)]
struct TorrentState {
    version: u32,
    torrents: BTreeMap<String, PersistentTorrent>,
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
    state: TorrentState,
}

impl TorrentStateManager {
    const CURRENT_VERSION: u32 = 1;

    /// Creates a new TorrentStateManager and loads the state from the given file path.
    pub async fn new(state_file_path: PathBuf) -> Self {
        let mut manager = Self {
            state_file_path,
            state: TorrentState {
                version: Self::CURRENT_VERSION,
                ..Default::default()
            },
        };
        if let Err(e) = manager.load().await {
            warn!(
                "Could not load torrent state file: {}. A new one will be created.",
                e
            );
        }
        manager
    }

    /// Loads the torrent state from the JSON file.
    async fn load(&mut self) -> Result<(), std::io::Error> {
        if !self.state_file_path.exists() {
            return Ok(());
        }
        let file = std::fs::File::open(&self.state_file_path)?;
        let reader = std::io::BufReader::new(file);
        let loaded_torrents: Vec<PersistentTorrent> = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let loaded_state: BTreeMap<String, PersistentTorrent> = loaded_torrents
            .into_iter()
            .map(|t| (t.info_hash.clone(), t))
            .collect();
        self.state.torrents = loaded_state;
        info!(
            "Loaded {} torrents from state file.",
            self.state.torrents.len()
        );
        Ok(())
    }

    /// Saves the current torrent state to the JSON file.
    pub async fn save(&self) -> Result<(), std::io::Error> {
        // Ensure parent directory exists
        if let Some(parent) = self.state_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::File::create(&self.state_file_path)?;
        let writer = std::io::BufWriter::new(file);
        // Collect values to serialize them as a JSON array
        let values: Vec<&PersistentTorrent> = self.state.torrents.values().collect();
        serde_json::to_writer_pretty(writer, &values)?;
        Ok(())
    }

    pub async fn add_torrent(&mut self, torrent: PersistentTorrent) -> Result<(), std::io::Error> {
        self.state
            .torrents
            .insert(torrent.info_hash.clone(), torrent);
        self.save().await
    }

    pub async fn remove_torrent(
        &mut self,
        info_hash: &str,
    ) -> Result<Option<PersistentTorrent>, std::io::Error> {
        let removed = self.state.torrents.remove(info_hash);
        self.save().await?;
        Ok(removed)
    }

    pub async fn update_torrent(
        &mut self,
        info_hash: &str,
        torrent: PersistentTorrent,
    ) -> Result<(), std::io::Error> {
        self.state.torrents.insert(info_hash.to_string(), torrent);
        self.save().await
    }

    pub fn get_torrent(&self, info_hash: &str) -> Option<&PersistentTorrent> {
        self.state.torrents.get(info_hash)
    }

    pub fn get_all_torrents(&self) -> &BTreeMap<String, PersistentTorrent> {
        &self.state.torrents
    }

    /// Returns a vector of the torrents currently managed.
    pub fn get_all_torrents_vec(&self) -> Vec<PersistentTorrent> {
        self.state.torrents.values().cloned().collect()
    }
    pub fn get_all(&self) -> Vec<PersistentTorrent> {
        let mut torrents: Vec<PersistentTorrent> = self.state.torrents.values().cloned().collect();
        // Sort by priority (lower is higher), then by added_at timestamp as a tie-breaker.
        torrents.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(a.added_at.cmp(&b.added_at))
        });
        torrents
    }

    /// Updates the priorities of multiple torrents and saves the state.
    /// Accepts a list of (info_hash, new_priority) tuples.
    pub async fn update_priorities(
        &mut self,
        updates: &[(String, u32)],
    ) -> Result<(), std::io::Error> {
        let mut changed = false;
        for (info_hash, new_priority) in updates {
            if let Some(torrent) = self.state.torrents.get_mut(info_hash) {
                if torrent.priority != *new_priority {
                    torrent.priority = *new_priority;
                    changed = true;
                }
            }
        }
        if changed {
            self.save().await
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
    info!(
        "Updating download priorities for {} torrents.",
        ordered_info_hashes.len()
    );

    // Convert the ordered list of hashes into a list of (hash, priority) tuples.
    let updates: Vec<(String, u32)> = ordered_info_hashes
        .into_iter()
        .enumerate()
        .map(|(index, hash)| (hash, index as u32))
        .collect();

    let mut manager = state_manager.lock().await;
    manager
        .update_priorities(&updates)
        .await
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
}

/// BitTorrent protocol handler implementing the ProtocolHandler trait.
/// This handler manages BitTorrent downloads and seeding operations using librqbit.
#[derive(Clone)]
pub struct BitTorrentHandler {
    rqbit_session: Arc<Session>,
    dht_service: Arc<DhtService>,
    download_directory: std::path::PathBuf,
    // NEW: Manage active torrents and their stats.
    chiral_extension: Option<Arc<ChiralBitTorrentExtension>>,
    active_torrents: Arc<tokio::sync::Mutex<HashMap<String, Arc<ManagedTorrent>>>>,
    // Cache the .torrent bytes for seeded torrents so real-network E2E can download deterministically
    // without relying on magnet metadata exchange (which can hang in restricted environments).
    seeded_torrent_bytes: Arc<tokio::sync::Mutex<HashMap<String, Vec<u8>>>>,
    peer_states: Arc<tokio::sync::Mutex<HashMap<String, HashMap<String, PeerTransferState>>>>,
    app_handle: Arc<tokio::sync::Mutex<Option<AppHandle>>>,
    event_bus: Arc<tokio::sync::Mutex<Option<Arc<TransferEventBus>>>>,
    state_manager: Option<Arc<tokio::sync::Mutex<TorrentStateManager>>>,
    state_file_path: std::path::PathBuf,
}

impl BitTorrentHandler {
    /// Set the AppHandle after construction.
    /// This allows the stats_poller to emit events to the frontend.
    pub async fn set_app_handle(&self, app_handle: AppHandle) {
        *self.app_handle.lock().await = Some(app_handle.clone());

        // Also update the event_bus
        *self.event_bus.lock().await = Some(Arc::new(TransferEventBus::new(app_handle)));

        info!("AppHandle set on BitTorrentHandler - stats polling will now emit events");
    }

    /// Get a reference to the rqbit session.
    /// This is useful for accessing torrent metadata.
    pub fn rqbit_session(&self) -> &Arc<Session> {
        &self.rqbit_session
    }

    pub async fn start_download_from_bytes_with_initial_peer(
        &self,
        bytes: Vec<u8>,
        peer: SocketAddr,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        let mut add_opts = AddTorrentOptions::default();
        add_opts.initial_peers = Some(vec![peer]);
        // Delegate to the standard bytes path (will also inject E2E peer hints as needed).
        // We keep the explicit peer as the highest priority.
        self.start_download_from_bytes_with_options(bytes, add_opts).await
    }

    pub async fn start_download_from_bytes_with_options(
        &self,
        bytes: Vec<u8>,
        mut add_opts: AddTorrentOptions,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        // Best-effort: parse info_hash first so we can inject initial peers in E2E before adding.
        let parsed_info_hash_hex = torrent_from_bytes::<Vec<u8>>(&bytes)
            .ok()
            .map(|t| hex::encode(t.info_hash.0));

        // In E2E, inject a deterministic initial peer if the caller didn't specify one.
        if add_opts
            .initial_peers
            .as_ref()
            .map(|v| v.is_empty())
            .unwrap_or(true)
        {
            if let Some(info_hash_hex) = parsed_info_hash_hex.as_deref() {
                maybe_inject_e2e_initial_peer(&self.dht_service, info_hash_hex, &mut add_opts).await;
            }
        }

        info!(
            "Starting BitTorrent download from torrent bytes with initial_peers={:?}",
            add_opts.initial_peers
        );

        let add_torrent = AddTorrent::from_bytes(bytes);
        let add_torrent_response = self
            .rqbit_session
            .add_torrent(add_torrent, Some(add_opts))
            .await
            .map_err(|e| {
                error!("Failed to add torrent from bytes: {}", e);
                Self::map_generic_error(e)
            })?;

        let handle = add_torrent_response
            .into_handle()
            .ok_or(BitTorrentError::HandleUnavailable)?;

        let torrent_info_hash = handle.info_hash();
        let hash_hex = hex::encode(torrent_info_hash.0);
        info!(
            "Torrent from bytes added successfully, info_hash: {}",
            hash_hex
        );

        {
            let mut torrents = self.active_torrents.lock().await;
            torrents.insert(hash_hex.clone(), handle.clone());
        }

        Ok(handle)
    }

    pub async fn get_seeded_torrent_bytes(&self, info_hash_hex: &str) -> Option<Vec<u8>> {
        self.seeded_torrent_bytes
            .lock()
            .await
            .get(info_hash_hex)
            .cloned()
    }

    /// Creates a new BitTorrentHandler with the specified download directory.
    pub async fn new(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
    ) -> Result<Self, BitTorrentError> {
        let state_file_path = download_directory.join("torrents_state.json");
        Self::new_with_port_range_app_handle_and_state_path(
            download_directory,
            dht_service,
            None,
            None,
            state_file_path,
        )
        .await //
    }

    /// Creates a new BitTorrentHandler with a specific port range to avoid conflicts.
    pub async fn new_with_port_range(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        listen_port_range: Option<std::ops::Range<u16>>,
    ) -> Result<Self, BitTorrentError> {
        // Correctly call the main constructor, passing None for the app_handle.
        Self::new_with_port_range_and_app_handle(
            download_directory,
            dht_service,
            listen_port_range,
            None,
        )
        .await
    }

    /// Creates a new BitTorrentHandler with AppHandle for TransferEventBus integration.
    pub async fn new_with_app_handle(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        app_handle: AppHandle,
    ) -> Result<Self, BitTorrentError> {
        let state_file_path = download_directory.join("torrents_state.json");
        // Correctly call the main constructor, passing None for the port range and Some for the app_handle. //
        Self::new_with_port_range_and_app_handle(
            download_directory,
            dht_service,
            None,
            Some(app_handle),
        )
        .await
    }

    /// Creates a new BitTorrentHandler with all options.
    ///
    /// If a state_manager is provided, this constructor will automatically restore
    /// all previously saved torrents on initialization.
    pub async fn new_with_port_range_and_app_handle(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        listen_port_range: Option<std::ops::Range<u16>>,
        app_handle: Option<AppHandle>,
    ) -> Result<Self, BitTorrentError> {
        let state_file_path = download_directory.join("torrents_state.json");
        Self::new_with_port_range_app_handle_and_state_path(
            download_directory,
            dht_service,
            listen_port_range,
            app_handle,
            state_file_path,
        )
        .await
    }

    /// Creates a new BitTorrentHandler with custom state file path.
    pub async fn new_with_state_path(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        state_file_path: std::path::PathBuf,
    ) -> Result<Self, BitTorrentError> {
        Self::new_with_port_range_and_state_path(
            download_directory,
            dht_service,
            None,
            state_file_path,
        )
        .await
    }

    /// Creates a new BitTorrentHandler with port range and custom state file path.
    pub async fn new_with_port_range_and_state_path(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        listen_port_range: Option<std::ops::Range<u16>>,
        state_file_path: std::path::PathBuf,
    ) -> Result<Self, BitTorrentError> {
        Self::new_with_port_range_app_handle_and_state_path(
            download_directory,
            dht_service,
            listen_port_range,
            None,
            state_file_path,
        )
        .await
    }

    /// Creates a new BitTorrentHandler with all configuration options including state file path.
    pub async fn new_with_port_range_app_handle_and_state_path(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        listen_port_range: Option<std::ops::Range<u16>>,
        app_handle: Option<AppHandle>,
        state_file_path: std::path::PathBuf,
    ) -> Result<Self, BitTorrentError> {
        info!(
            "Creating BitTorrent session with download_directory: {:?}, port_range: {:?}, state_file: {:?}",
            download_directory, listen_port_range, state_file_path
        );
        // Call new_with_state with None for state_manager
        Self::new_with_state(
            download_directory,
            dht_service,
            listen_port_range,
            app_handle,
            None,
        )
        .await
    }

    /// Creates a new BitTorrentHandler with all options including state restoration.
    ///
    /// If a state_manager is provided, this constructor will automatically restore
    /// all previously saved torrents on initialization.
    pub async fn new_with_state(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        listen_port_range: Option<std::ops::Range<u16>>,
        app_handle: Option<AppHandle>,
        state_manager: Option<TorrentStateManager>,
    ) -> Result<Self, BitTorrentError> {
        info!(
        "Creating BitTorrent session with download_directory: {:?}, port_range: {:?}, state_manager: {}",
        download_directory, listen_port_range, state_manager.is_some()
    );

        // Create a separate directory for BitTorrent session state
        let bt_state_dir = download_directory.join(".bittorrent_state");
        if let Err(e) = std::fs::create_dir_all(&bt_state_dir) {
            error!(
                "Failed to create BitTorrent state directory {:?}: {}",
                bt_state_dir, e
            );
            return Err(BitTorrentError::SessionInit {
                message: format!("Failed to create state directory: {}", e),
            });
        }

        // Clean up any stale DHT or session state files that might be corrupted
        let state_files = [
            "session.json",
            "dht.json",
            "dht.db",
            "session.db",
            "dht.dat",
        ];
        for file in &state_files {
            let state_path = bt_state_dir.join(file);
            if state_path.exists() {
                if let Err(e) = std::fs::remove_file(&state_path) {
                    warn!(
                        "Failed to remove stale state file {:?}: {}. This may cause DHT issues.",
                        state_path, e
                    );
                    // Don't fail here, just warn - the file might be locked temporarily
                } else {
                    info!("Cleaned stale state file: {:?}", state_path);
                }
            }
        }

        let mut opts = SessionOptions::default();

        // Set port range if provided
        if let Some(range) = listen_port_range.clone() {
            opts.listen_port_range = Some(range);
        }

        // Enable session persistence for torrents in dedicated directory
        // but disable DHT persistence as it's causing initialization failures
        opts.persistence = Some(librqbit::SessionPersistenceConfig::Json {
            folder: Some(bt_state_dir),
        });
        opts.disable_dht_persistence = true;

        let session = Session::new_with_opts(download_directory.clone(), opts)
            .await
            .map_err(|e| {
                error!("Session initialization failed: {}", e);
                BitTorrentError::SessionInit {
                    message: format!("Failed to create session: {}", e),
                }
            })?;

        if let Some(p) = session.tcp_listen_port() {
            info!("BitTorrent session TCP listen port: {}", p);
        } else {
            warn!("BitTorrent session TCP listen port is not set (unexpected)");
        }

        // Create TransferEventBus if app_handle is provided
        let event_bus = app_handle
            .as_ref()
            .map(|handle| Arc::new(TransferEventBus::new(handle.clone())));

        // Wrap state_manager in Arc<Mutex> if provided
        let state_manager_arc = state_manager.map(|sm| Arc::new(tokio::sync::Mutex::new(sm)));

        let handler = Self {
            rqbit_session: session.clone(),
            dht_service,
            download_directory: download_directory.clone(),
            active_torrents: Default::default(),
            chiral_extension: None,
            seeded_torrent_bytes: Default::default(),
            peer_states: Default::default(),
            app_handle: Arc::new(tokio::sync::Mutex::new(app_handle)),
            event_bus: Arc::new(tokio::sync::Mutex::new(event_bus)),
            state_manager: state_manager_arc.clone(),
            state_file_path: download_directory.join("torrents_state.json"),
        };

        // Spawn the background task for statistics polling.
        handler.spawn_stats_poller();

        info!(
            "Initializing BitTorrentHandler with download directory: {:?}",
            handler.download_directory
        );

        // Restore torrents from state if state_manager was provided
        if let Some(sm) = state_manager_arc {
            let sm_guard = sm.lock().await;
            let persistent_torrents = sm_guard.get_all();
            drop(sm_guard); // Release lock before async operations

            if !persistent_torrents.is_empty() {
                info!(
                    "Restoring {} saved torrent(s) on initialization",
                    persistent_torrents.len()
                );

                for torrent in persistent_torrents {
                    info!(
                        "Restoring torrent: {} (status: {:?})",
                        torrent.info_hash, torrent.status
                    );

                    // Determine the identifier based on the source
                    let identifier = match &torrent.source {
                        PersistentTorrentSource::Magnet(url) => {
                            info!("  Source: magnet link");
                            url.clone()
                        }
                        PersistentTorrentSource::File(path) => {
                            info!("  Source: torrent file at {:?}", path);
                            path.to_string_lossy().to_string()
                        }
                    };

                    // Re-add the torrent with the original output path
                    match handler
                        .start_download_to(&identifier, torrent.output_path.clone())
                        .await
                    {
                        Ok(_) => {
                            info!(
                                "✓ Successfully restored torrent: {} to {:?}",
                                torrent.info_hash, torrent.output_path
                            );
                        }
                        Err(e) => {
                            error!("✗ Failed to restore torrent {}: {}", torrent.info_hash, e);
                        }
                    }
                }

                info!("Torrent restoration complete");
            }
        }

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
                    let stats = handle.stats();
                    let torrent_peer_states = states.entry(info_hash_str.clone()).or_default();
                    let session_key = "__session__".to_string();
                    let state = torrent_peer_states.entry(session_key.clone()).or_default();

                    let uploaded_total = stats.uploaded_bytes;
                    let downloaded_total = stats.progress_bytes;
                    let total_bytes = stats.total_bytes;

                    // Emit progress event via TransferEventBus
                    if let Some(ref bus) = *event_bus.lock().await {
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
                            protocol: "BitTorrent".to_string(),
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

                    // Also emit torrent_event Progress for the frontend UI
                    if let Some(ref app) = *app_handle.lock().await {
                        let (download_speed, _upload_speed) = if let Some(live) = &stats.live {
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
                        // Note: librqbit doesn't expose peer count directly, so we use 0 for now
                        // This is just for display purposes
                        let peers = 0;

                        // Always emit Progress event with current stats
                        let progress_event = serde_json::json!({
                            "Progress": {
                                "info_hash": info_hash_str,
                                "downloaded": downloaded_total,
                                "total": total_bytes,
                                "speed": download_speed as u64,
                                "peers": peers,
                                "eta_seconds": eta.unwrap_or(0) as u64
                            }
                        });
                        let _ = app.emit("torrent_event", progress_event);

                        // Check if download just completed (emit Complete event only once)
                        if stats.finished || (total_bytes > 0 && downloaded_total >= total_bytes) {
                            // Check if we've already notified about completion by tracking last state
                            let was_complete =
                                state.last_downloaded_bytes >= total_bytes && total_bytes > 0;
                            if !was_complete {
                                // Emit Complete event only on transition to complete
                                // Use info_hash as name since we don't have easy access to the actual name
                                let torrent_name = format!("Torrent {}", &info_hash_str[..8]);
                                let complete_event = serde_json::json!({
                                    "Complete": {
                                        "info_hash": info_hash_str,
                                        "name": torrent_name
                                    }
                                });
                                let _ = app.emit("torrent_event", complete_event);
                            }
                        }
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

                        if let Some(ref handle) = *app_handle.lock().await {
                            if let Err(e) = handle.emit("payment_required", payload) {
                                error!("Failed to emit payment_required event: {}", e);
                            }
                        } else {
                            warn!(
                                "No AppHandle available; skipping emit of payment_required event"
                            );
                        }

                        state.last_uploaded_bytes = uploaded_total;
                    }
                    state.last_downloaded_bytes = downloaded_total;
                }
            }
        });
    }

    /// Creates a new BitTorrentHandler with Chiral extension support
    pub async fn new_with_chiral_extension(
        download_directory: std::path::PathBuf,
        dht_service: Arc<DhtService>,
        wallet_address: Option<String>,
    ) -> Result<Self, BitTorrentError> {
        let mut handler = Self::new(download_directory, dht_service).await?;

        // Initialize Chiral extension
        let chiral_extension =
            ChiralBitTorrentExtension::new(handler.rqbit_session.clone(), wallet_address);

        handler.chiral_extension = Some(Arc::new(chiral_extension));

        info!("BitTorrentHandler initialized with Chiral extension support");
        Ok(handler)
    }

    /// Subscribe to Chiral extension events
    pub fn subscribe_chiral_events(
        &self,
    ) -> Option<tokio::sync::broadcast::Receiver<ChiralExtensionEvent>> {
        self.chiral_extension
            .as_ref()
            .map(|ext| ext.subscribe_events())
    }

    /// Get Chiral peers for prioritized connections
    pub async fn get_chiral_peers(&self, info_hash: &str) -> Vec<String> {
        if let Some(extension) = &self.chiral_extension {
            extension.get_prioritized_peers(info_hash).await
        } else {
            Vec::new()
        }
    }

    /// Register a torrent with the Chiral extension
    async fn register_torrent_with_chiral_extension(
        &self,
        info_hash: &str,
    ) -> Result<(), BitTorrentError> {
        if let Some(extension) = &self.chiral_extension {
            extension
                .register_with_torrent(info_hash)
                .await
                .map_err(|e| BitTorrentError::Unknown {
                    message: format!("Failed to register torrent with Chiral extension: {}", e),
                })?;
        }
        Ok(())
    }

    /// Starts a download and returns a handle to the torrent.
    pub async fn start_download(
        &self,
        identifier: &str,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        self.start_download_with_options(identifier, AddTorrentOptions::default())
            .await
    }

    /// Starts a download with an explicit initial peer hint.
    /// Useful for real-network E2E where magnet metadata exchange / DHT discovery can be flaky.
    pub async fn start_download_with_initial_peer(
        &self,
        identifier: &str,
        peer: SocketAddr,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        let mut opts = AddTorrentOptions::default();
        opts.initial_peers = Some(vec![peer]);
        self.start_download_with_options(identifier, opts).await
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

    /// Start a download from torrent file bytes.
    /// This method accepts the raw bytes of a .torrent file and starts downloading.
    pub async fn start_download_from_bytes(
        &self,
        bytes: Vec<u8>,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        // Keep backward-compat: bytes download with default options.
        self.start_download_from_bytes_with_options(bytes, AddTorrentOptions::default())
            .await
    }
    /// Re-evaluates the download queue, pausing or resuming torrents based on priority
    /// and the MAX_ACTIVE_DOWNLOADS limit.
    async fn re_evaluate_queue(&self) -> Result<(), BitTorrentError> {
        info!("Re-evaluating download queue...");
        let torrent_handles = self.active_torrents.lock().await;

        // Create a sorted list of torrents based on priority.
        // We need to collect stats for sorting.
        let mut all_torrents: Vec<_> = torrent_handles
            .values()
            .map(|h| (h.clone(), h.stats()))
            .collect();
        // A simple sort by state: downloading torrents first, then by other states.
        // A more robust implementation would use the priority from TorrentStateManager.
        // For now, we just find paused torrents to resume.
        all_torrents.sort_by_key(|(_, stats)| stats.state.to_string() != "Downloading");

        let mut active_downloads = all_torrents
            .iter()
            .filter(|(_, stats)| !stats.finished && stats.state.to_string() == "Downloading")
            .count();

        info!(
            "Currently {} active downloads (limit is {}).",
            active_downloads, MAX_ACTIVE_DOWNLOADS
        );

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
        mut add_opts: AddTorrentOptions,
    ) -> Result<Arc<ManagedTorrent>, BitTorrentError> {
        info!("Starting BitTorrent download for: {}", identifier);

        // Phase 3: Get info_hash BEFORE adding the torrent to check for duplicates.
        let info_hash_hex = if identifier.starts_with("magnet:") {
            Self::extract_info_hash(identifier).ok_or_else(|| {
                BitTorrentError::InvalidMagnetLink {
                    url: identifier.to_string(),
                }
            })?
        } else {
            // For .torrent files, we must parse the file to get the info_hash.
            let torrent_bytes =
                std::fs::read(identifier).map_err(|e| BitTorrentError::TorrentFileError {
                    message: format!("Could not read torrent file {}: {}", identifier, e),
                })?;
            let torrent_info = torrent_from_bytes::<Vec<u8>>(&torrent_bytes).map_err(|e| {
                BitTorrentError::TorrentParsingError {
                    message: format!("Could not parse torrent file {}: {}", identifier, e),
                }
            })?;
            hex::encode(torrent_info.info_hash.0)
        };

        // Phase 3: Check if the torrent already exists.
        if self.has_torrent(&info_hash_hex).await {
            return Err(BitTorrentError::TorrentExists {
                info_hash: info_hash_hex,
            });
        }

        // Queueing Logic: Check if we should pause this new torrent.
        let active_downloads = {
            let torrents = self.active_torrents.lock().await;
            torrents
                .values()
                .filter(|h| {
                    let stats = h.stats();
                    !stats.finished && stats.state.to_string() == "Downloading"
                })
                .count()
        };

        if active_downloads >= MAX_ACTIVE_DOWNLOADS {
            info!(
                "Max active downloads ({}) reached. Adding new torrent in paused state.",
                MAX_ACTIVE_DOWNLOADS
            );
            add_opts.paused = true;
        }

        // In E2E, we can optionally force a direct peer hint using Chiral DHT provider addresses.
        // This avoids reliance on public BitTorrent trackers / BT-DHT in restricted environments.
        let mut identifier_for_add = identifier.to_string();

        if identifier.starts_with("magnet:") {
            maybe_inject_e2e_initial_peer(&self.dht_service, &info_hash_hex, &mut add_opts).await;

            // Best-effort: also append x.pe for clients that support it (rqbit may ignore).
            if let Some(peers) = add_opts.initial_peers.as_ref() {
                if let Some(p) = peers.first() {
                    let with_hint = append_magnet_x_pe(&identifier_for_add, p.ip(), p.port());
                    if with_hint != identifier_for_add {
                        info!(
                            "E2E: injecting magnet peer hint x.pe={}:{} for torrent {}",
                            p.ip(),
                            p.port(),
                            info_hash_hex
                        );
                    }
                    identifier_for_add = with_hint;
                }
            }
        }

        let add_torrent = if identifier_for_add.starts_with("magnet:") {
            Self::validate_magnet_link(identifier).map_err(|e| {
                //
                error!("Magnet link validation failed: {}", e);
                e
            })?;
            AddTorrent::from_url(&identifier_for_add)
        } else {
            Self::validate_torrent_file(identifier).map_err(|e| {
                error!("Torrent file validation failed: {}", e);
                e
            })?;
            AddTorrent::from_local_filename(identifier).map_err(|e| {
                BitTorrentError::TorrentFileError {
                    message: format!("Cannot read torrent file {}: {}", identifier, e),
                }
            })?
        };

        // Keep caller-provided add_opts (paused/output_folder/etc).

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

        // Register with Chiral extension if available
        if let Some(info_hash) = Self::extract_info_hash(&identifier_for_add) {
            if let Err(e) = self
                .register_torrent_with_chiral_extension(&info_hash)
                .await
            {
                warn!("Failed to register torrent with Chiral extension: {}", e);
            }
        }
        // Continue without Chiral extension rather than failing the download
        // Now get the info_hash from the handle (works for both magnets and .torrent files)
        // Get the info_hash from the handle (works for both magnets and .torrent files)
        let torrent_info_hash = handle.info_hash();
        let hash_hex = hex::encode(torrent_info_hash.0);

        // Store the torrent handle for tracking
        {
            let mut torrents = self.active_torrents.lock().await;
            torrents.insert(info_hash_hex.clone(), handle.clone());
        }

        // Add to active torrents
        let mut active_torrents = self.active_torrents.lock().await;
        active_torrents.insert(info_hash_hex.clone(), handle.clone());
        drop(active_torrents);

        // Create persistent torrent state
        let persistent_torrent = if identifier.starts_with("magnet:") {
            PersistentTorrent {
                info_hash: info_hash_hex.clone(),
                source: PersistentTorrentSource::Magnet(identifier.to_string()),
                output_path: self.download_directory.clone(),
                status: PersistentTorrentStatus::Downloading,
                added_at: PersistentTorrent::current_timestamp(),
                name: None,
                priority: 0, // Default priority
                size: None,
            }
        } else {
            PersistentTorrent {
                info_hash: info_hash_hex.clone(),
                source: PersistentTorrentSource::File(PathBuf::from(identifier)),
                output_path: self.download_directory.clone(),
                status: PersistentTorrentStatus::Downloading,
                added_at: PersistentTorrent::current_timestamp(),
                name: None,
                priority: 0, // Default priority
                size: None,
            }
        };

        self.save_torrent_to_state(&info_hash_hex, persistent_torrent)
            .await?;

        Ok(handle)
    }

    async fn save_torrent_to_state(
        &self,
        info_hash: &str,
        torrent: PersistentTorrent,
    ) -> Result<(), BitTorrentError> {
        if let Some(state_manager) = &self.state_manager {
            let mut sm = state_manager.lock().await;
            sm.add_torrent(torrent).await.map_err(|e| {
                error!("Failed to save torrent {} to state: {}", info_hash, e);
                BitTorrentError::ConfigError {
                    message: format!("Failed to save torrent state: {}", e),
                }
            })?;
            info!("Saved torrent {} to persistent state", info_hash);
            Ok(())
        } else {
            warn!(
                "No state manager available, torrent {} will not be persisted",
                info_hash
            );
            Ok(())
        }
    }

    /// Check if a torrent exists in the active session or persistent state.
    pub async fn has_torrent(&self, info_hash: &str) -> bool {
        // Check active torrents first
        if self.active_torrents.lock().await.contains_key(info_hash) {
            return true;
        }
        // Then check persistent state
        self.has_persistent_torrent(info_hash).await
    }

    /// Get all persistent torrents from state
    pub async fn get_persistent_torrents(&self) -> BTreeMap<String, PersistentTorrent> {
        if let Some(state_manager) = &self.state_manager {
            let state_manager = state_manager.lock().await;
            state_manager.get_all_torrents().clone()
        } else {
            BTreeMap::new()
        }
    }

    /// Get persistent torrents by mode (Download or Seed)
    pub async fn get_persistent_torrents_by_mode(
        &self,
        mode: PersistentTorrentStatus,
    ) -> Vec<PersistentTorrent> {
        if let Some(state_manager) = &self.state_manager {
            let state_manager = state_manager.lock().await;
            state_manager
                .get_all_torrents()
                .values()
                .filter(|t| t.status == mode)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Check if a torrent exists in persistent state
    pub async fn has_persistent_torrent(&self, info_hash: &str) -> bool {
        if let Some(state_manager) = &self.state_manager {
            let state_manager = state_manager.lock().await;
            state_manager.get_torrent(info_hash).is_some()
        } else {
            false
        }
    }

    /// Get count of persistent torrents
    pub async fn get_persistent_torrent_count(&self) -> usize {
        if let Some(state_manager) = &self.state_manager {
            let state_manager = state_manager.lock().await;
            state_manager.state.torrents.len()
        } else {
            0
        }
    }

    /// Update torrent metadata in persistent state (e.g., name, size when available)
    pub async fn update_torrent_metadata(
        &self,
        info_hash: &str,
        name: Option<String>,
        size: Option<u64>,
    ) -> Result<(), BitTorrentError> {
        if let Some(state_manager_arc) = &self.state_manager {
            let mut state_manager = state_manager_arc.lock().await;

            if let Some(mut persistent_torrent) = state_manager.get_torrent(info_hash).cloned() {
                // Update metadata
                if name.is_some() {
                    persistent_torrent.name = name;
                }
                if size.is_some() {
                    persistent_torrent.size = size;
                }

                state_manager
                    .update_torrent(info_hash, persistent_torrent)
                    .await
                    .map_err(|e| {
                        error!("Failed to update torrent metadata for {}: {}", info_hash, e);
                        BitTorrentError::ConfigError {
                            message: format!("Failed to update torrent metadata: {}", e),
                        }
                    })?;

                info!("Updated metadata for torrent: {}", info_hash);
                Ok(())
            } else {
                Err(BitTorrentError::TorrentNotFound {
                    info_hash: info_hash.to_string(),
                })
            }
        } else {
            Err(BitTorrentError::ConfigError {
                message: "State manager not available".to_string(),
            })
        }
    }

    /// Clear all torrents from both active session and persistent state
    pub async fn clear_all_torrents(&self, delete_files: bool) -> Result<(), BitTorrentError> {
        info!("Clearing all torrents (delete_files: {})", delete_files);

        // Get all active torrent info hashes
        let info_hashes: Vec<String> = {
            let active_torrents = self.active_torrents.lock().await;
            active_torrents.keys().cloned().collect()
        };

        // Cancel each torrent
        for info_hash in info_hashes {
            if let Err(e) = self.cancel_torrent(&info_hash, delete_files).await {
                warn!("Failed to cancel torrent {}: {}", info_hash, e);
                // Continue with other torrents
            }
        }

        // Clear any remaining state (in case some torrents weren't active)
        if let Some(state_manager_arc) = &self.state_manager {
            let mut state_manager = state_manager_arc.lock().await;
            state_manager.state.torrents.clear();
            state_manager.save().await.map_err(|e| {
                error!("Failed to clear persistent state: {}", e);
                BitTorrentError::ConfigError {
                    message: format!("Failed to clear persistent state: {}", e),
                }
            })?;
        }
        info!("Cleared all torrents from session and persistent state");
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

/// Pause/Resume/Cancel methods for torrent control
impl BitTorrentHandler {
    /// Pause a torrent by info hash
    pub async fn pause_torrent(&self, info_hash: &str) -> Result<(), BitTorrentError> {
        info!("Pausing torrent: {}", info_hash);

        let torrents = self.active_torrents.lock().await;
        if let Some(handle) = torrents.get(info_hash) {
            let stats = handle.stats();
            self.rqbit_session.pause(handle).await.map_err(|e| {
                BitTorrentError::ProtocolSpecific {
                    message: format!("Failed to pause torrent: {}", e),
                }
            })?;

            // Emit paused event via TransferEventBus
            if let Some(ref bus) = *self.event_bus.lock().await {
                bus.emit_paused(TransferPausedEvent {
                    transfer_id: info_hash.to_string(),
                    protocol: "BitTorrent".to_string(),
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
            let stats = handle.stats(); // ADD THIS LINE
            self.rqbit_session.unpause(handle).await.map_err(|e| {
                BitTorrentError::ProtocolSpecific {
                    message: format!("Failed to resume torrent: {}", e),
                }
            })?;

            // Emit resumed event via TransferEventBus
            if let Some(ref bus) = *self.event_bus.lock().await {
                bus.emit_resumed(TransferResumedEvent {
                    transfer_id: info_hash.to_string(),
                    protocol: "BitTorrent".to_string(),
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
    pub async fn cancel_torrent(
        &self,
        info_hash: &str,
        delete_files: bool,
    ) -> Result<(), BitTorrentError> {
        info!(
            "Cancelling torrent: {} (delete_files: {})",
            info_hash, delete_files
        );

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

    /// Get the download folder path for a torrent
    pub async fn get_torrent_folder(&self, info_hash: &str) -> Result<PathBuf, BitTorrentError> {
        let torrents = self.active_torrents.lock().await;

        if torrents.contains_key(info_hash) {
            // The download_directory is where all torrents are stored
            // Each torrent typically creates its own subfolder based on the torrent name
            Ok(self.download_directory.clone())
        } else {
            Err(BitTorrentError::TorrentNotFound {
                info_hash: info_hash.to_string(),
            })
        }
    }

    /// Get progress information for a torrent
    pub async fn get_torrent_progress(
        &self,
        info_hash: &str,
    ) -> Result<TorrentProgress, BitTorrentError> {
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

    /// After a BitTorrent download completes, automatically:
    /// 1. Continue seeding the file
    /// 2. Compute the Chiral Network hash (SHA-256)
    /// 3. Publish metadata to the DHT so it's discoverable on the Chiral Network
    pub async fn post_download_seed_and_publish(
        &self,
        info_hash: &str,
    ) -> Result<PostDownloadResult, String> {
        // Normalize info_hash to lowercase for consistent DHT key storage
        let info_hash_lower = info_hash.to_lowercase();

        info!(
            "Starting post-download seed and publish for info_hash: {} (normalized to: {})",
            info_hash, info_hash_lower
        );

        // Get the torrent handle (use original case for lookup)
        let torrents = self.active_torrents.lock().await;
        let handle = torrents
            .get(info_hash)
            .ok_or_else(|| format!("Torrent not found: {}", info_hash))?;

        // Get torrent stats to get the file path
        let stats = handle.stats();

        // Get the download folder
        let folder_path = self.download_directory.clone();

        // Find the completed file - for now, use first file in directory
        // In production, you'd want to iterate through the torrent's files
        let mut file_path = folder_path.clone();
        let mut file_name = String::from("unknown");

        // Try to find the file by reading directory
        if let Ok(entries) = tokio::fs::read_dir(&folder_path).await {
            let mut entries = entries;
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    if metadata.is_file() {
                        file_path = entry.path();
                        file_name = entry.file_name().to_string_lossy().to_string();
                        break;
                    }
                }
            }
        }

        // Verify file exists
        if !file_path.exists() || file_path == folder_path {
            return Err(format!(
                "Downloaded file not found in: {}",
                folder_path.display()
            ));
        }

        // Read the file and compute Chiral Network hash (SHA-256)
        let file_data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let chiral_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&file_data);
            format!("{:x}", hasher.finalize())
        };

        info!("Computed Chiral hash for {}: {}", file_name, chiral_hash);

        // Get file size
        let file_size = stats.total_bytes;

        // Get our local peer ID from DHT for seeders list
        let local_peer_id = self.dht_service.get_peer_id().await.to_string();

        // Create FileMetadata for DHT publishing
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let magnet_link = format!("magnet:?xt=urn:btih:{}", info_hash);

        // Create FileManifest using ChunkManager
        let chunk_storage_path = self.download_directory.join("chunks");
        let manager = ChunkManager::new(chunk_storage_path);

        // Use chunk_and_encrypt_file_canonical to generate FileManifest
        // This will calculate chunk hashes even without encryption
        let file_manifest_result = tokio::task::spawn_blocking({
            let file_path_clone = file_path.clone();
            move || manager.chunk_and_encrypt_file_canonical(Path::new(&file_path_clone))
        })
        .await
        .map_err(|e| format!("Failed to spawn blocking task: {}", e))?;

        let file_manifest =
            file_manifest_result.map_err(|e| format!("Failed to create FileManifest: {}", e))?;

        // Serialize manifest to JSON
        let manifest_json = serde_json::to_string(&file_manifest.manifest)
            .map_err(|e| format!("Failed to serialize FileManifest: {}", e))?;

        let metadata = crate::dht::models::FileMetadata {
            merkle_root: file_manifest.manifest.merkle_root.clone(),
            file_name: file_name.clone(),
            file_size,
            file_data: Vec::new(), // Don't include file data in DHT
            seeders: vec![local_peer_id.clone()],
            created_at,
            mime_type: None,
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            encrypted_key_bundle: None,
            uploader_address: None,
            cids: None,
            // BitTorrent-specific fields (use lowercase for consistent DHT indexing)
            info_hash: Some(info_hash_lower.clone()),
            trackers: Some(vec![]), // rqbit handles trackers internally
            // Other protocol fields
            ftp_sources: None,
            ed2k_sources: None,
            http_sources: None,
            price: 0.0,
            is_root: false,
            parent_hash: None,
            download_path: None,
            manifest: Some(manifest_json),
        };

        // Publish to DHT
        info!("Publishing file metadata to Chiral DHT...");
        self.dht_service
            .publish_file(metadata.clone(), None)
            .await
            .map_err(|e| format!("Failed to publish to DHT: {}", e))?;

        info!(
            "Successfully published to DHT with merkle_root: {}",
            chiral_hash
        );

        // Note: The torrent is already seeding through rqbit, so no need to call seed() again
        // rqbit automatically continues seeding after download completes

        Ok(PostDownloadResult {
            chiral_hash,
            magnet_link,
            file_name,
            file_size,
            info_hash: info_hash_lower,
            published_to_dht: true,
        })
    }
}

/// Result of post-download seeding and DHT publishing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostDownloadResult {
    pub chiral_hash: String,
    pub magnet_link: String,
    pub file_name: String,
    pub file_size: u64,
    pub info_hash: String,
    pub published_to_dht: bool,
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

    /// Validate magnet link format
    fn validate_magnet_link(url: &str) -> Result<(), BitTorrentError> {
        if !url.starts_with("magnet:?xt=urn:btih:") {
            return Err(BitTorrentError::InvalidMagnetLink {
                url: url.to_string(),
            });
        }

        if let Some(hash_start) = url.find("urn:btih:") {
            let hash_start = hash_start + 9;
            let hash_end = url[hash_start..]
                .find('&')
                .unwrap_or(url.len() - hash_start)
                + hash_start;
            let hash = &url[hash_start..hash_end];

            if hash.len() != 40 && hash.len() != 64 {
                return Err(BitTorrentError::InvalidMagnetLink {
                    url: url.to_string(),
                });
            }

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

    /// Monitor a running torrent and emit BitTorrentEvent messages via the provided sender.
    /// This is used by the MultiSourceDownloadService to bridge librqbit progress into the
    /// multi-source pipeline.
    pub async fn monitor_download(
        &self,
        handle: Arc<ManagedTorrent>,
        tx: mpsc::Sender<BitTorrentEvent>,
    ) {
        let mut interval = time::interval(Duration::from_secs(1));
        let mut no_progress_count: u32 = 0;
        const MAX_NO_PROGRESS_ITERATIONS: u32 = 300;

        loop {
            interval.tick().await;

            let stats = handle.stats();
            let downloaded = stats.progress_bytes;
            let total = stats.total_bytes;

            // If receiver dropped, stop monitoring
            if tx.is_closed() {
                return;
            }

            // Send progress update; if send fails, stop
            if tx
                .send(BitTorrentEvent::Progress { downloaded, total })
                .await
                .is_err()
            {
                return;
            }

            // Completed
            if total > 0 && downloaded >= total {
                let _ = tx.send(BitTorrentEvent::Completed).await;
                return;
            }

            // Simple stalled-download detection
            if downloaded == 0 {
                no_progress_count = no_progress_count.saturating_add(1);
                if no_progress_count >= MAX_NO_PROGRESS_ITERATIONS {
                    let _ = tx
                        .send(BitTorrentEvent::Failed(BitTorrentError::DownloadTimeout {
                            timeout_secs: MAX_NO_PROGRESS_ITERATIONS as u64,
                        }))
                        .await;
                    return;
                }
            } else {
                no_progress_count = 0;
            }
        }
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
        let (tx, mut rx) = mpsc::channel(10);

        let handle_clone = handle.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1));
            let mut no_progress_count = 0;
            const MAX_NO_PROGRESS_ITERATIONS: u32 = 300;

            loop {
                interval.tick().await;
                let stats = handle_clone.stats();
                let downloaded = stats.progress_bytes;
                let total = stats.total_bytes;

                if tx.is_closed() {
                    return;
                }

                if let Err(_) = tx
                    .send(BitTorrentEvent::Progress { downloaded, total })
                    .await
                {
                    return;
                }

                if total > 0 && downloaded >= total {
                    let _ = tx.send(BitTorrentEvent::Completed).await;
                    return;
                }

                if downloaded == 0 {
                    no_progress_count += 1;
                    if no_progress_count >= MAX_NO_PROGRESS_ITERATIONS {
                        let _ = tx
                            .send(BitTorrentEvent::Failed(BitTorrentError::DownloadTimeout {
                                timeout_secs: MAX_NO_PROGRESS_ITERATIONS as u64,
                            }))
                            .await;
                        return;
                    }
                } else {
                    no_progress_count = 0;
                }
            }
        });

        while let Some(event) = rx.recv().await {
            match event {
                BitTorrentEvent::Completed => return Ok(()),
                BitTorrentEvent::Failed(e) => return Err(e.into()),
                _ => {}
            }
        }
        Err("Monitoring channel closed unexpectedly.".to_string())
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn seed(&self, file_path: &str) -> Result<String, String> {
        let path = Path::new(file_path);
        if !path.exists() {
            return Err(BitTorrentError::FileSystemError {
                message: format!("File does not exist: {}", file_path),
            }
            .into());
        }

        if !path.is_file() {
            return Err(BitTorrentError::FileSystemError {
                message: format!("Path is not a file: {}", file_path),
            }
            .into());
        }

        let torrent = create_torrent(path, CreateTorrentOptions::default())
            .await
            .map_err(|e| BitTorrentError::SeedingError {
                message: format!("Failed to create torrent from file {}: {}", file_path, e),
            })?;

        // Phase 3: Get info_hash from created torrent and check for duplicates.
        let info_hash_str = hex::encode(torrent.info_hash().0);
        if self.has_torrent(&info_hash_str).await {
            // Convert BitTorrentError to String for the trait's return type.
            return Err(BitTorrentError::TorrentExists {
                info_hash: info_hash_str,
            }
            .into());
        }

        let torrent_bytes = torrent
            .as_bytes()
            .map_err(|e| BitTorrentError::SeedingError {
                message: format!("Failed to serialize torrent for {}: {}", file_path, e),
            })?;

        // Diagnostics: verify the on-disk file layout matches what the torrent expects.
        // If this is wrong, the seeder can end up with 0 pieces and downloads will make no progress.
        // Always log this (no env gating, no torrent parsing gating).
        let output_folder_dbg = path.parent().unwrap_or_else(|| Path::new("."));
        // Best-effort: single-file torrents are the common case for Chiral seeding.
        let expected = output_folder_dbg.join(path.file_name().unwrap_or_default());
        let (exists_ok, size) = match std::fs::metadata(&expected) {
            Ok(m) if m.is_file() => (true, m.len()),
            _ => (false, 0),
        };
        info!(
            "BitTorrent seed file check: output_folder={:?} expected_path={:?} exists_ok={} size={}",
            output_folder_dbg, expected, exists_ok, size
        );

        let add_torrent = AddTorrent::from_bytes(torrent_bytes.clone());

        // IMPORTANT:
        // For seeding, rqbit must know where the underlying file already exists.
        // Otherwise it may create storage under the session's default folder and have 0 pieces,
        // leading to "connected but no download progress" on clients.
        let output_folder = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_string_lossy()
            .into_owned();
        let options = AddTorrentOptions {
            overwrite: true,
            output_folder: Some(output_folder.clone()),
            ..Default::default()
        };

        let handle = self
            .rqbit_session
            .add_torrent(add_torrent, Some(options))
            .await
            .map_err(|e| BitTorrentError::SeedingError {
                message: format!("Failed to add torrent for seeding: {}", e),
            })?
            .into_handle()
            .ok_or(BitTorrentError::HandleUnavailable)?;

        let magnet_link = format!("magnet:?xt=urn:btih:{}", info_hash_str);

        // Best-effort: wait a short time for initial hashcheck to complete so the torrent becomes seedable.
        // Without this, a downloader can connect immediately but observe 0 available pieces for a bit.
        let seed_ready_wait_ms: u64 = std::env::var("E2E_BITTORRENT_SEED_READY_WAIT_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            // Real networks can be slower to finish the initial hashcheck; default higher to avoid "0 pieces" windows.
            .unwrap_or(30_000);
        let start = std::time::Instant::now();
        loop {
            let stats = handle.stats();
            if stats.finished {
                info!(
                    "BitTorrent seed ready: info_hash={} output_folder={} state={} finished={}",
                    info_hash_str,
                    output_folder,
                    stats.state.to_string(),
                    stats.finished
                );
                break;
            }
            if start.elapsed().as_millis() as u64 >= seed_ready_wait_ms {
                warn!(
                    "BitTorrent seed not marked finished after {}ms (info_hash={}, state={}). Proceeding anyway.",
                    seed_ready_wait_ms,
                    info_hash_str,
                    stats.state.to_string()
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        {
            self.active_torrents
                .lock()
                .await
                .insert(info_hash_str.clone(), handle);
        }

        // Cache the .torrent bytes for E2E attach-mode downloaders (so they can skip magnet metadata exchange).
        {
            self.seeded_torrent_bytes
                .lock()
                .await
                .insert(info_hash_str.clone(), torrent_bytes.to_vec());
        }

        // Construct the persistent state for the seeded torrent.
        let persistent_torrent = PersistentTorrent {
            info_hash: info_hash_str.clone(),
            // We use the magnet link as the source for simplicity in re-adding.
            source: PersistentTorrentSource::Magnet(magnet_link.clone()),
            output_path: path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
            status: PersistentTorrentStatus::Seeding,
            added_at: PersistentTorrent::current_timestamp(),
            name: path.file_name().and_then(|n| n.to_str()).map(String::from),
            size: std::fs::metadata(path).ok().map(|m| m.len()),
            priority: 0, // Default priority for new seeds
        };

        // Save the state to torrent_state.json
        if let Err(e) = self
            .save_torrent_to_state(&info_hash_str, persistent_torrent)
            .await
        {
            warn!("Failed to save seeding torrent to state: {}", e);
        }
        info!("Started seeding {} and saved to state.", file_path);
        Ok(magnet_link)
    }
}

// Helper function
fn multiaddr_to_socket_addr(multiaddr: &Multiaddr) -> Result<std::net::SocketAddr, &'static str> {
    use libp2p::multiaddr::Protocol;

    let mut iter = multiaddr.iter();
    let proto1 = iter.next().ok_or("Empty Multiaddr")?;
    let proto2 = iter
        .next()
        .ok_or("Multiaddr needs at least two protocols")?;

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(std::net::SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(std::net::SocketAddr::new(ip.into(), port)),
        _ => Err("Multiaddr format not supported (expected IP/TCP)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dht::DhtConfig;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

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

        assert!(BitTorrentHandler::validate_torrent_file(torrent_path.to_str().unwrap()).is_ok());
        assert!(BitTorrentHandler::validate_torrent_file("/nonexistent/file.torrent").is_err());

        let txt_path = create_test_file(temp_dir.path(), "test.txt", "content");
        assert!(BitTorrentHandler::validate_torrent_file(txt_path.to_str().unwrap()).is_err());
    }

    #[test]
    fn test_persistent_torrent_serialization_round_trip() {
        // Test with a Magnet link source
        let original_magnet = PersistentTorrent {
            info_hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
            source: PersistentTorrentSource::Magnet(
                "magnet:?xt=urn:btih:a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
            ),
            output_path: PathBuf::from("/downloads/test_magnet"),
            status: PersistentTorrentStatus::Downloading,
            added_at: 1678886400,
            priority: 0, // Add priority field
            name: None,
            size: None,
        };

        let serialized_magnet = serde_json::to_string_pretty(&original_magnet).unwrap();
        println!("Serialized Magnet Torrent:\n{}", serialized_magnet);

        let deserialized_magnet: PersistentTorrent =
            serde_json::from_str(&serialized_magnet).unwrap();

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
            name: None,
            size: None,
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

        let config = DhtConfig::builder().build();
        let dht_service = Arc::new(DhtService::new(config, None, None, None).await.unwrap());

        // Use a specific port range to avoid conflicts if other tests run in parallel
        let handler = BitTorrentHandler::new_with_port_range(
            download_path.clone(),
            dht_service,
            Some(31000..32000),
        )
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
                if matches!(
                    event,
                    BitTorrentEvent::Completed | BitTorrentEvent::Failed(_)
                ) {
                    final_event = Some(event);
                    break;
                }
            }
        })
        .await
        {
            Ok(_) => assert!(
                matches!(final_event, Some(BitTorrentEvent::Completed)),
                "Download did not complete successfully. Last event: {:?}",
                final_event
            ),
            Err(_) => panic!(
                "Download timed out after {} seconds",
                timeout_duration.as_secs()
            ),
        }
    }

    #[tokio::test]
    #[ignore] // Ignored by default: real network download of a ~50MB file.
    async fn test_integration_protocol_handler_download_linux_distro() {
        let temp_dir = tempdir().expect("Failed to create temp directory for download");
        let download_path = temp_dir.path().to_path_buf();

        let config = DhtConfig::builder().build();
        let dht_service = Arc::new(DhtService::new(config, None, None, None).await.unwrap());

        // Use a specific port range to avoid conflicts
        let handler = BitTorrentHandler::new_with_port_range(
            download_path.clone(),
            dht_service,
            Some(33000..34000),
        )
        .await
        .expect("Failed to create BitTorrentHandler");

        // A small, well-seeded, and legal torrent for a Linux distro (~50MB)
        let magnet_link =
            "magnet:?xt=urn:btih:a24f6cb6c62b23c235a2889c0c8e65f4350100d0&dn=slitaz-rolling.iso";

        // The download() method from the trait handles the full lifecycle.
        // We'll wrap it in a timeout to prevent the test from running indefinitely.
        let timeout_duration = Duration::from_secs(600); // 10-minute timeout

        let result = time::timeout(timeout_duration, handler.download(magnet_link)).await;

        // Check for timeout first
        assert!(
            result.is_ok(),
            "Download timed out after {} seconds",
            timeout_duration.as_secs()
        );

        // Check if the download method itself returned Ok
        let download_result = result.unwrap();
        assert!(
            download_result.is_ok(),
            "Download failed with error: {:?}",
            download_result.err()
        );

        // Verify that the file was actually created
        assert!(
            download_path.join("slitaz-rolling.iso").exists(),
            "Downloaded file does not exist"
        );
    }

    #[tokio::test]
    #[ignore] // Ignored by default as it involves file I/O and a real session
    async fn test_integration_seed_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let file_path =
            create_test_file(temp_dir.path(), "seed_me.txt", "hello world seeding test");

        let config = DhtConfig::builder().build();
        let dht_service = Arc::new(DhtService::new(config, None, None, None).await.unwrap());

        // Use a specific port range to avoid conflicts
        let handler = BitTorrentHandler::new_with_port_range(
            temp_dir.path().to_path_buf(),
            dht_service,
            Some(32000..33000),
        )
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
        let torrent_count = handler
            .rqbit_session
            .with_torrents(|torrents| torrents.count());
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
    fn test_extract_info_hash() {
        let magnet = "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn=test";
        let hash = BitTorrentHandler::extract_info_hash(magnet);
        assert_eq!(
            hash,
            Some("1234567890abcdef1234567890abcdef12345678".to_string())
        );

        let invalid_magnet = "not_a_magnet_link";
        let hash = BitTorrentHandler::extract_info_hash(invalid_magnet);
        assert_eq!(hash, None);
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
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_torrent_state_manager_new_and_load_empty_file() {
        let temp_dir = tempdir().unwrap();
        let state_file_path = temp_dir.path().join("torrent_state.json");

        // Manager should initialize with an empty list if file doesn't exist
        let manager = TorrentStateManager::new(state_file_path.clone()).await;
        assert!(manager.state.torrents.is_empty());
        assert!(!state_file_path.exists()); // File should not be created on new if empty
    }

    #[tokio::test]
    async fn test_torrent_state_manager_save_and_load() {
        let temp_dir = tempdir().unwrap();
        let state_file_path = temp_dir.path().join("torrent_state.json");

        let mut manager = TorrentStateManager::new(state_file_path.clone()).await;

        let torrent1 = PersistentTorrent {
            info_hash: "hash1".to_string(),
            source: PersistentTorrentSource::Magnet("magnet1".to_string()),
            output_path: PathBuf::from("/downloads/torrent1"),
            status: PersistentTorrentStatus::Downloading,
            added_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            name: None,
            size: None,
            priority: 0,
        };
        let torrent2 = PersistentTorrent {
            info_hash: "hash2".to_string(),
            source: PersistentTorrentSource::File(PathBuf::from("/path/to/file2.torrent")),
            output_path: PathBuf::from("/downloads/torrent2"),
            status: PersistentTorrentStatus::Seeding,
            added_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 100,
            name: None,
            size: None,
            priority: 1,
        };

        manager
            .state
            .torrents
            .insert(torrent1.info_hash.clone(), torrent1.clone());
        manager
            .state
            .torrents
            .insert(torrent2.info_hash.clone(), torrent2.clone());

        // Save the state
        manager.save().await.unwrap();
        assert!(state_file_path.exists());

        // Verify content of the saved file
        let saved_content = std::fs::read_to_string(&state_file_path).unwrap();
        let loaded_from_file: Vec<PersistentTorrent> =
            serde_json::from_str(&saved_content).unwrap();
        assert_eq!(loaded_from_file.len(), 2);
        assert!(loaded_from_file.contains(&torrent1));
        assert!(loaded_from_file.contains(&torrent2));

        // Create a new manager and load the state (note: new() now loads automatically)
        let loaded_manager = TorrentStateManager::new(state_file_path.clone()).await;
        assert_eq!(loaded_manager.state.torrents.len(), 2);
        assert_eq!(
            loaded_manager.state.torrents.get("hash1").unwrap(),
            &torrent1
        );
        assert_eq!(
            loaded_manager.state.torrents.get("hash2").unwrap(),
            &torrent2
        );
    }

    #[tokio::test]
    async fn test_torrent_state_manager_get_all() {
        let temp_dir = tempdir().unwrap();
        let state_file_path = temp_dir.path().join("torrent_state.json");

        let mut manager = TorrentStateManager::new(state_file_path.clone()).await;

        let torrent1 = PersistentTorrent {
            info_hash: "hash1".to_string(),
            source: PersistentTorrentSource::Magnet("magnet1".to_string()),
            output_path: PathBuf::from("/downloads/torrent1"),
            status: PersistentTorrentStatus::Downloading,
            added_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            name: None,
            size: None,
            priority: 0,
        };
        let torrent2 = PersistentTorrent {
            info_hash: "hash2".to_string(),
            source: PersistentTorrentSource::File(PathBuf::from("/path/to/file2.torrent")),
            output_path: PathBuf::from("/downloads/torrent2"),
            status: PersistentTorrentStatus::Seeding,
            added_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 100,
            name: None,
            size: None,
            priority: 1,
        };

        manager
            .state
            .torrents
            .insert(torrent1.info_hash.clone(), torrent1.clone());
        manager
            .state
            .torrents
            .insert(torrent2.info_hash.clone(), torrent2.clone());

        let all_torrents = manager.get_all_torrents_vec();
        assert_eq!(all_torrents.len(), 2);
        assert!(all_torrents.contains(&torrent1));
        assert!(all_torrents.contains(&torrent2));
    }

    // Test for malformed JSON file (should load empty or return error, depending on desired behavior)
    // Current implementation logs a warning and returns empty, which is good.
}
