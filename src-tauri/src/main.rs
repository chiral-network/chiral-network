// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

// Modules unique to the binary
pub mod blockchain_listener;
pub mod commands {
    pub mod auth;
    pub mod bootstrap;
    pub mod network;
    pub mod proxy;
}
pub mod ethereum;
pub mod geth_bootstrap;
pub mod geth_downloader;
pub mod headless;
pub mod http_server;
pub mod net;
pub mod pool;
pub mod reassembly;
pub mod transaction_services;

// Re-export modules from the lib crate
use chiral_network::{
    analytics, bandwidth, bittorrent_handler, dht, download_restart,
    ed2k_client, encryption, file_transfer, http_download, keystore,
    logger, manager, multi_source_download, peer_selection, protocols, reputation,
    stream_auth, webrtc_service,
};

use protocols::{
    BitTorrentProtocolHandler, ProtocolHandler, ProtocolManager, SimpleProtocolHandler,
};

use crate::commands::auth::{
    cleanup_expired_proxy_auth_tokens, generate_proxy_auth_token, revoke_proxy_auth_token,
    validate_proxy_auth_token,
};

use crate::commands::bootstrap::get_bootstrap_nodes;
use crate::commands::bootstrap::get_bootstrap_nodes_command;
use crate::commands::network::get_full_network_stats;
use crate::commands::proxy::{
    disable_privacy_routing, enable_privacy_routing, list_proxies, proxy_connect, proxy_disconnect,
    proxy_echo, proxy_remove, ProxyNode,
};
use bandwidth::BandwidthController;
use chiral_network::transfer_events::{
    current_timestamp_ms, ErrorCategory, SourceInfo, SourceType, TransferCompletedEvent,
    TransferEventBus, TransferFailedEvent, TransferStartedEvent,
};
use dht::{models::DhtMetricsSnapshot, models::FileMetadata, DhtEvent, DhtService};
use directories::ProjectDirs;
use ethereum::{
    // Bootstrap peer management functions
    add_peer,
    create_new_account,
    get_account_from_private_key,
    get_balance,
    get_block_number,
    get_hashrate,
    get_mining_logs,
    get_mining_performance,
    get_mining_status, // Assuming you have a file_handler module
    get_network_difficulty,
    get_network_hashrate,
    get_node_info,
    get_peer_count,
    get_peer_info,
    get_peers,
    get_recent_mined_blocks,
    get_transaction_by_hash,
    get_txpool_status,
    get_txpool_content,
    debug_network_tx,
    reconnect_to_bootstrap_if_needed,
    start_mining,
    stop_mining,
    EthAccount,
    GethProcess,
    MinedBlock,
};
use file_transfer::{DownloadMetricsSnapshot, FileTransferEvent, FileTransferService};
use fs2::available_space;
use geth_downloader::GethDownloader;
use keystore::Keystore;
use lazy_static::lazy_static;
use multi_source_download::{MultiSourceDownloadService, MultiSourceEvent, MultiSourceProgress};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{
    io::{BufRead, BufReader},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use stream_auth::{
    AuthMessage, HmacKeyExchangeConfirmation, HmacKeyExchangeRequest, HmacKeyExchangeResponse,
    StreamAuthService,
};
use sysinfo::{Components, System};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};
use tokio::{io::AsyncReadExt, sync::Mutex, task::JoinHandle, time::sleep};
use totp_rs::{Algorithm, Secret, TOTP};
use tracing::{error, info, warn};
use webrtc_service::{set_webrtc_service, WebRTCFileRequest, WebRTCService};

use manager::ChunkManager; // Import the ChunkManager
                           // For key encoding
use blockstore::block::Block;
use dht::models::Ed2kDownloadStatus;
use dht::models::Ed2kSourceInfo;
use ed2k_client::{Ed2kClient, Ed2kSearchResult, Ed2kServerInfo};
use rand::Rng;
use std::io::Write;
use std::ops::Range;
use suppaftp::FtpStream;

use x25519_dalek::{PublicKey, StaticSecret}; // For key handling
                                             // Settings structure for backend use
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackendSettings {
    #[serde(rename = "storagePath")]
    storage_path: String,
    #[serde(rename = "enableFileLogging")]
    enable_file_logging: bool,
    #[serde(rename = "maxLogSizeMB")]
    max_log_size_mb: u64,
}

impl Default for BackendSettings {
    fn default() -> Self {
        Self {
            storage_path: "".to_string(), // No hardcoded default - get_download_directory handles this
            enable_file_logging: false,
            max_log_size_mb: 10,
        }
    }
}

/// Load settings from the Tauri app data directory
fn load_settings_from_file(app_handle: &tauri::AppHandle) -> BackendSettings {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .expect("Failed to get app data directory");

    let settings_file = app_data_dir.join("settings.json");

    if settings_file.exists() {
        match std::fs::read_to_string(&settings_file) {
            Ok(contents) => {
                match serde_json::from_str::<serde_json::Value>(&contents) {
                    Ok(json) => {
                        // Extract only the fields we need
                        let storage_path = json
                            .get("storagePath")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let enable_file_logging = json
                            .get("enableFileLogging")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let max_log_size_mb = json
                            .get("maxLogSizeMB")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(10);

                        return BackendSettings {
                            storage_path,
                            enable_file_logging,
                            max_log_size_mb,
                        };
                    }
                    Err(e) => {
                        eprintln!("Failed to parse settings file: {}. Using defaults.", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read settings file: {}. Using defaults.", e);
            }
        }
    }

    BackendSettings::default()
}

/// Expand tilde (~) in path to home directory
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        if let Some(base_dirs) = directories::BaseDirs::new() {
            return base_dirs
                .home_dir()
                .join(path.strip_prefix("~/").unwrap_or(""));
        }
    }
    PathBuf::from(path)
}

/// Get a unique file path by adding (1), (2), etc. if the file already exists
/// Example: "file.txt" -> "file (1).txt" if "file.txt" exists
fn get_unique_filepath(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    
    let parent = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let extension = path.extension().and_then(|e| e.to_str());
    
    let mut counter = 1;
    loop {
        let new_name = match extension {
            Some(ext) => format!("{} ({}).{}", stem, counter, ext),
            None => format!("{} ({})", stem, counter),
        };
        let new_path = parent.join(&new_name);
        if !new_path.exists() {
            return new_path;
        }
        counter += 1;
        // Safety limit to prevent infinite loop
        if counter > 1000 {
            return new_path;
        }
    }
}

/// Detect MIME type from file extension
fn detect_mime_type_from_filename(filename: &str) -> Option<String> {
    let extension = filename.rsplit('.').next()?.to_lowercase();

    match extension.as_str() {
        // Images
        "jpg" | "jpeg" => Some("image/jpeg".to_string()),
        "png" => Some("image/png".to_string()),
        "gif" => Some("image/gif".to_string()),
        "bmp" => Some("image/bmp".to_string()),
        "webp" => Some("image/webp".to_string()),
        "svg" => Some("image/svg+xml".to_string()),
        "ico" => Some("image/x-icon".to_string()),

        // Videos
        "mp4" => Some("video/mp4".to_string()),
        "avi" => Some("video/x-msvideo".to_string()),
        "mkv" => Some("video/x-matroska".to_string()),
        "mov" => Some("video/quicktime".to_string()),
        "wmv" => Some("video/x-ms-wmv".to_string()),
        "flv" => Some("video/x-flv".to_string()),
        "webm" => Some("video/webm".to_string()),

        // Audio
        "mp3" => Some("audio/mpeg".to_string()),
        "wav" => Some("audio/wav".to_string()),
        "flac" => Some("audio/flac".to_string()),
        "aac" => Some("audio/aac".to_string()),
        "ogg" => Some("audio/ogg".to_string()),
        "wma" => Some("audio/x-ms-wma".to_string()),

        // Documents
        "pdf" => Some("application/pdf".to_string()),
        "doc" => Some("application/msword".to_string()),
        "docx" => Some(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
        ),
        "xls" => Some("application/vnd.ms-excel".to_string()),
        "xlsx" => {
            Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string())
        }
        "ppt" => Some("application/vnd.ms-powerpoint".to_string()),
        "pptx" => Some(
            "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_string(),
        ),
        "txt" => Some("text/plain".to_string()),
        "rtf" => Some("application/rtf".to_string()),

        // Archives
        "zip" => Some("application/zip".to_string()),
        "rar" => Some("application/x-rar-compressed".to_string()),
        "7z" => Some("application/x-7z-compressed".to_string()),
        "tar" => Some("application/x-tar".to_string()),
        "gz" => Some("application/gzip".to_string()),

        // Code files
        "html" | "htm" => Some("text/html".to_string()),
        "css" => Some("text/css".to_string()),
        "js" => Some("application/javascript".to_string()),
        "json" => Some("application/json".to_string()),
        "xml" => Some("application/xml".to_string()),
        "py" => Some("text/x-python".to_string()),
        "rs" => Some("text/rust".to_string()),
        "java" => Some("text/x-java-source".to_string()),
        "cpp" | "cc" | "cxx" => Some("text/x-c++src".to_string()),
        "c" => Some("text/x-csrc".to_string()),
        "h" => Some("text/x-chdr".to_string()),
        "hpp" => Some("text/x-c++hdr".to_string()),

        // Other common types
        "exe" => Some("application/x-msdownload".to_string()),
        "dll" => Some("application/x-msdownload".to_string()),
        "iso" => Some("application/x-iso9660-image".to_string()),

        // Default fallback
        _ => Some("application/octet-stream".to_string()),
    }
}

#[derive(Clone)]
struct QueuedTransaction {
    id: String,
    to_address: String,
    amount: f64,
    timestamp: u64,
}

#[derive(Clone)]
struct ProxyAuthToken {
    token: String,
    proxy_address: String,
    expires_at: u64,
    created_at: u64,
}

#[derive(Clone, Debug)]
pub struct StreamingUploadSession {
    pub file_name: String,
    pub file_size: u64,
    pub received_chunks: u32,
    pub total_chunks: u32,
    pub hasher: sha2::Sha256,
    pub created_at: std::time::SystemTime,
    pub chunk_cids: Vec<String>,
    pub file_data: Vec<u8>,
    pub price: f64,
    pub is_complete: bool,
}

/// Session for streaming WebRTC downloads - writes chunks directly to disk
#[derive(Debug)]
pub struct StreamingDownloadSession {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub temp_path: std::path::PathBuf,
    pub output_path: String,
    pub received_chunks: std::collections::HashSet<u32>,
    pub total_chunks: u32,
    pub chunk_size: u32,
    pub created_at: std::time::SystemTime,
}

struct AppState {
    geth: Mutex<GethProcess>,
    downloader: Arc<GethDownloader>,
    miner_address: Mutex<Option<String>>,

    // Wrap in Arc so they can be cloned
    active_account: Arc<Mutex<Option<String>>>,
    active_account_private_key: Arc<Mutex<Option<String>>>,

    rpc_url: Mutex<String>,
    dht: Mutex<Option<Arc<DhtService>>>,
    file_transfer: Mutex<Option<Arc<FileTransferService>>>,
    webrtc: Mutex<Option<Arc<WebRTCService>>>,
    multi_source_download: Mutex<Option<Arc<MultiSourceDownloadService>>>,
    keystore: Arc<Mutex<Keystore>>,
    proxies: Arc<Mutex<Vec<ProxyNode>>>,
    privacy_proxies: Arc<Mutex<Vec<String>>>,
    file_transfer_pump: Mutex<Option<JoinHandle<()>>>,
    multi_source_pump: Mutex<Option<JoinHandle<()>>>,
    socks5_proxy_cli: Mutex<Option<String>>,
    analytics: Arc<analytics::AnalyticsService>,
    bandwidth: Arc<BandwidthController>,

    // New fields for transaction queue
    transaction_queue: Arc<Mutex<VecDeque<QueuedTransaction>>>,
    transaction_processor: Mutex<Option<JoinHandle<()>>>,
    processing_transaction: Arc<Mutex<bool>>,

    // New field for streaming upload sessions
    upload_sessions: Arc<Mutex<std::collections::HashMap<String, StreamingUploadSession>>>,

    // New field for streaming download sessions (WebRTC chunk streaming to disk)
    download_sessions: Arc<Mutex<std::collections::HashMap<String, StreamingDownloadSession>>>,

    // Proxy authentication tokens storage
    proxy_auth_tokens: Arc<Mutex<std::collections::HashMap<String, ProxyAuthToken>>>,

    // HTTP server for serving chunks and keys
    http_server_state: Arc<http_server::HttpServerState>,
    http_server_addr: Arc<Mutex<Option<std::net::SocketAddr>>>,
    http_server_shutdown: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,

    // Stream authentication service
    stream_auth: Arc<Mutex<StreamAuthService>>,

    // New field for storing canonical AES keys for files being seeded
    canonical_aes_keys: Arc<Mutex<std::collections::HashMap<String, [u8; 32]>>>,

    // Proof-of-Storage watcher background handle and contract address
    // make these clonable so we can .clone() and move into spawned tasks
    proof_watcher: Arc<Mutex<Option<JoinHandle<()>>>>,
    proof_contract_address: Arc<Mutex<Option<String>>>,

    // Relay reputation statistics storage
    relay_reputation: Arc<Mutex<std::collections::HashMap<String, RelayNodeStats>>>,

    // Relay node aliases (peer_id -> alias)
    relay_aliases: Arc<Mutex<std::collections::HashMap<String, String>>>,

    // Protocol manager for handling different download/upload protocols
    protocol_manager: Arc<ProtocolManager>,

    // AutoRelay timeline persistence across DHT restarts
    autorelay_last_enabled: Arc<Mutex<Option<SystemTime>>>,
    autorelay_last_disabled: Arc<Mutex<Option<SystemTime>>>,

    // File logger writer for dynamic log configuration updates
    file_logger: Arc<Mutex<Option<logger::ThreadSafeWriter>>>,
    // BitTorrent handler for creating and seeding torrents
    bittorrent_handler: Arc<bittorrent_handler::BitTorrentHandler>,

    // Chunk manager for file chunking operations
    chunk_manager: Mutex<Option<Arc<ChunkManager>>>,

    // Download restart service for pause/resume functionality
    download_restart: Mutex<Option<Arc<download_restart::DownloadRestartService>>>,

    // FTP server for serving uploaded files
    ftp_server: Arc<chiral_network::ftp_server::FtpServer>,
}

/// Tauri command to create a new Chiral account
#[tauri::command]
async fn create_chiral_account(state: State<'_, AppState>) -> Result<EthAccount, String> {
    let account = create_new_account()?;

    // Set as active account
    {
        let mut active_account = state.active_account.lock().await;
        *active_account = Some(account.address.clone());
    }

    // Store private key in session
    {
        let mut active_key = state.active_account_private_key.lock().await;
        *active_key = Some(account.private_key.clone());
    }

    Ok(account)
}

#[tauri::command]
async fn import_chiral_account(
    private_key: String,
    state: State<'_, AppState>,
) -> Result<EthAccount, String> {
    let account = get_account_from_private_key(&private_key)?;

    // Set as active account
    {
        let mut active_account = state.active_account.lock().await;
        *active_account = Some(account.address.clone());
    }

    // Store private key in session
    {
        let mut active_key = state.active_account_private_key.lock().await;
        *active_key = Some(account.private_key.clone());
    }

    Ok(account)
}

#[tauri::command]
async fn start_geth_node(
    state: State<'_, AppState>,
    data_dir: String,
    rpc_url: Option<String>,
) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    let miner_address = state.miner_address.lock().await;
    let rpc_url = rpc_url.unwrap_or_else(|| "http://127.0.0.1:8545".to_string());
    *state.rpc_url.lock().await = rpc_url.clone();

    geth.start(&data_dir, miner_address.as_deref())?;
    Ok(())
}

#[tauri::command]
async fn download(identifier: String, state: State<'_, AppState>) -> Result<(), String> {
    println!("Received download command for: {}", identifier);
    #[allow(deprecated)]
    state.protocol_manager.download_simple(&identifier).await
}

/// Tauri command to seed a file.
/// It takes a local file path, starts seeding, and returns a magnet link.
#[tauri::command]
async fn seed(file_path: String, state: State<'_, AppState>) -> Result<String, String> {
    println!("Received seed command for: {}", file_path);
    // Delegate the seed operation to the protocol manager.
    #[allow(deprecated)]
    state.protocol_manager.seed_simple(&file_path).await
}

/// Helper function to create and seed a BitTorrent file.
/// It takes a local file path and handler, creates a torrent, starts seeding, and returns a magnet link.
async fn create_and_seed_torrent_internal(
    file_path: String,
    handler: Arc<bittorrent_handler::BitTorrentHandler>,
) -> Result<String, String> {
    handler.seed(&file_path).await
}

/// Tauri command to create and seed a BitTorrent file.
/// It takes a local file path, creates a torrent, starts seeding, and returns a magnet link.
#[tauri::command]
async fn create_and_seed_torrent(
    file_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Use the BitTorrent handler directly to create and seed the torrent
    let handler = state.bittorrent_handler.clone();
    create_and_seed_torrent_internal(file_path, handler).await
}

#[tauri::command]
async fn stop_geth_node(state: State<'_, AppState>) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.stop()
}

#[tauri::command]
async fn save_account_to_keystore(
    address: String,
    private_key: String,
    password: String,
) -> Result<(), String> {
    let mut keystore = Keystore::load()?;
    keystore.add_account(address, &private_key, &password)?;
    Ok(())
}

#[tauri::command]
async fn load_account_from_keystore(
    address: String,
    password: String,
    state: State<'_, AppState>,
) -> Result<EthAccount, String> {
    let keystore = Keystore::load()?;

    // Get decrypted private key from keystore
    let private_key = keystore.get_account(&address, &password)?;

    // Set the active account in the app state
    {
        let mut active_account = state.active_account.lock().await;
        *active_account = Some(address.clone());
    }

    // Store the private key securely in memory for the session
    {
        let mut active_key = state.active_account_private_key.lock().await;
        *active_key = Some(private_key.clone());
    }

    // Update WebRTC service with the active private key for decryption
    if let Some(webrtc_service) = state.webrtc.lock().await.as_ref() {
        webrtc_service
            .set_active_private_key(Some(private_key.clone()))
            .await;
    }

    // Derive account details from private key
    get_account_from_private_key(&private_key)
}

#[tauri::command]
async fn list_keystore_accounts() -> Result<Vec<String>, String> {
    let keystore = Keystore::load()?;
    Ok(keystore.list_accounts())
}

#[tauri::command]
async fn get_disk_space(path: String) -> Result<u64, String> {
    match available_space(Path::new(&path)) {
        Ok(space) => Ok(space),
        Err(e) => Err(format!("Failed to get disk space: {}", e)),
    }
}

#[tauri::command]
async fn get_account_balance(address: String) -> Result<String, String> {
    get_balance(&address).await
}

#[tauri::command]
async fn get_user_balance(state: State<'_, AppState>) -> Result<String, String> {
    let account = get_active_account(&state).await?;
    get_balance(&account).await
}

#[tauri::command]
async fn get_transaction_receipt(
    tx_hash: String,
) -> Result<transaction_services::TransactionReceipt, String> {
    transaction_services::get_transaction_receipt(&tx_hash).await
}

#[tauri::command]
async fn get_gas_prices() -> Result<transaction_services::GasPrices, String> {
    transaction_services::get_recommended_gas_prices().await
}

#[tauri::command]
async fn estimate_transaction_gas(
    from: String,
    to: String,
    value: f64,
) -> Result<serde_json::Value, String> {
    // Convert value from Chiral to Wei (1 Chiral = 10^18 Wei)
    let value_wei = (value * 1_000_000_000_000_000_000.0) as u128;
    let value_hex = format!("0x{:x}", value_wei);
    
    // Estimate gas for the transaction (standard transfer is 21000)
    let gas_estimate = transaction_services::estimate_gas(&from, &to, &value_hex, None).await?;
    
    // Get current gas prices
    let gas_prices = transaction_services::get_recommended_gas_prices().await?;
    
    // Parse gas prices from hex to decimal (Wei)
    let slow_wei = u128::from_str_radix(&gas_prices.slow[2..], 16)
        .map_err(|e| format!("Failed to parse slow gas price: {}", e))?;
    let standard_wei = u128::from_str_radix(&gas_prices.standard[2..], 16)
        .map_err(|e| format!("Failed to parse standard gas price: {}", e))?;
    let fast_wei = u128::from_str_radix(&gas_prices.fast[2..], 16)
        .map_err(|e| format!("Failed to parse fast gas price: {}", e))?;
    
    // Calculate fees in Chiral (gas * gas_price / 10^18)
    let slow_fee = (gas_estimate as u128 * slow_wei) as f64 / 1_000_000_000_000_000_000.0;
    let standard_fee = (gas_estimate as u128 * standard_wei) as f64 / 1_000_000_000_000_000_000.0;
    let fast_fee = (gas_estimate as u128 * fast_wei) as f64 / 1_000_000_000_000_000_000.0;
    
    // Convert gas prices to Gwei for display (Wei / 10^9)
    let slow_gwei = slow_wei as f64 / 1_000_000_000.0;
    let standard_gwei = standard_wei as f64 / 1_000_000_000.0;
    let fast_gwei = fast_wei as f64 / 1_000_000_000.0;
    
    Ok(serde_json::json!({
        "gasLimit": gas_estimate,
        "gasPrices": {
            "slow": {
                "gwei": slow_gwei,
                "fee": slow_fee,
                "time": gas_prices.slow_time
            },
            "standard": {
                "gwei": standard_gwei,
                "fee": standard_fee,
                "time": gas_prices.standard_time
            },
            "fast": {
                "gwei": fast_gwei,
                "fee": fast_fee,
                "time": gas_prices.fast_time
            }
        },
        "networkCongestion": gas_prices.network_congestion
    }))
}

#[tauri::command]
async fn can_afford_download(state: State<'_, AppState>, price: f64) -> Result<bool, String> {
    let account = get_active_account(&state).await?;
    let balance_str = get_balance(&account).await?;
    let balance = balance_str
        .parse::<f64>()
        .map_err(|e| format!("Failed to parse balance: {}", e))?;
    Ok(balance >= price)
}

#[tauri::command]
async fn process_download_payment(
    state: State<'_, AppState>,
    uploader_address: String,
    price: f64,
) -> Result<String, String> {
    // Get the active account address
    let account = get_active_account(&state).await?;

    // Get the private key from state
    let private_key = {
        let key_guard = state.active_account_private_key.lock().await;
        key_guard
            .clone()
            .ok_or("No private key available. Please log in again.")?
    };

    // Send the payment transaction
    ethereum::send_transaction(&account, &uploader_address, price, &private_key).await
}

#[tauri::command]
async fn record_download_payment(
    app: tauri::AppHandle,
    file_hash: String,
    file_name: String,
    file_size: u64,
    seeder_wallet_address: String,
    seeder_peer_id: String,
    downloader_address: String,
    downloader_peer_id: String,
    amount: f64,
    transaction_id: u64,
    transaction_hash: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    println!(
        "📝 Download payment recorded: {} Chiral to wallet {} (peer: {}) from {} (peer: {}) tx: {}",
        amount,
        seeder_wallet_address,
        seeder_peer_id,
        downloader_address,
        downloader_peer_id,
        transaction_hash
    );
    println!(
        "🔍 IMPORTANT: downloader_peer_id value: '{}'",
        downloader_peer_id
    );
    println!("🔍 IMPORTANT: seeder_peer_id value: '{}'", seeder_peer_id);

    // Send P2P payment notification message to the seeder's peer
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct PaymentNotificationMessage {
        file_hash: String,
        file_name: String,
        file_size: u64,
        downloader_address: String,
        downloader_peer_id: String,
        seeder_wallet_address: String,
        amount: f64,
        transaction_id: u64,
        transaction_hash: String,
    }

    let payment_msg = PaymentNotificationMessage {
        file_hash,
        file_name,
        file_size,
        downloader_address,
        downloader_peer_id,
        seeder_wallet_address: seeder_wallet_address.clone(),
        amount,
        transaction_id,
        transaction_hash: transaction_hash.clone(),
    };

    // Serialize the payment message
    let payment_json = serde_json::to_string(&payment_msg)
        .map_err(|e| format!("Failed to serialize payment message: {}", e))?;

    // Emit local event for payment notification (works on same machine for testing)
    app.emit("seeder_payment_received", payment_msg.clone())
        .map_err(|e| format!("Failed to emit payment notification: {}", e))?;

    println!(
        "✅ Payment notification emitted locally for seeder: {}",
        seeder_wallet_address
    );

    // Update peer reputation: record successful payment transaction
    // This increments transfer_count for blockchain payments (separate from file transfers)
    {
        let dht_guard = state.dht.lock().await;
        if let Some(ref dht) = *dht_guard {
            // Record successful payment as a transfer success
            dht.record_transfer_success(&seeder_peer_id, file_size, 0).await;
            println!(
                "✅ Updated reputation for seeder peer {} after successful payment of {} Chiral",
                seeder_peer_id, amount
            );
        }
    }

    // Seeder will see the payment when they check the blockchain
    Ok(())
}

#[tauri::command]
async fn record_seeder_payment(
    _file_hash: String,
    _file_name: String,
    _file_size: u64,
    _downloader_address: String,
    _amount: f64,
    _transaction_id: u64,
) -> Result<(), String> {
    // Log the seeder payment receipt for analytics/audit purposes
    println!(
        "💰 Seeder payment received: {} Chiral from {}",
        _amount, _downloader_address
    );
    Ok(())
}

#[tauri::command]
async fn check_payment_notifications(
    _wallet_address: String,
    _state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    // NOTE: This command is kept for compatibility but not used anymore
    // Payment notifications are now handled via local events (seeder_payment_received)
    // For testing on same machine, the event system works fine
    // For cross-peer payments, this would need to be implemented with P2P messaging
    Ok(vec![])
}

#[tauri::command]
async fn get_network_peer_count() -> Result<u32, String> {
    get_peer_count().await
}

#[tauri::command]
async fn get_network_chain_id() -> Result<u64, String> {
    Ok(get_chain_id())
}

#[tauri::command]
async fn is_geth_running(state: State<'_, AppState>) -> Result<bool, String> {
    let geth = state.geth.lock().await;
    Ok(geth.is_running())
}

#[tauri::command]
async fn check_geth_binary(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.downloader.is_geth_installed())
}

#[tauri::command]
async fn download_geth_binary(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let downloader = state.downloader.clone();
    let app_handle = app.clone();

    downloader
        .download_geth(move |progress| {
            let _ = app_handle.emit("geth-download-progress", progress);
        })
        .await
}

#[tauri::command]
async fn set_miner_address(state: State<'_, AppState>, address: String) -> Result<(), String> {
    let mut miner_address = state.miner_address.lock().await;
    *miner_address = Some(address);
    Ok(())
}

#[tauri::command]
async fn test_backend_connection(state: State<'_, AppState>) -> Result<String, String> {
    info!("🧪 Testing backend connection...");

    let dht = { state.dht.lock().await.as_ref().cloned() };
    if let Some(dht) = dht {
        info!("✅ DHT service is available");
        Ok("DHT service is running".to_string())
    } else {
        info!("❌ DHT service is not available");
        Err("DHT not running".into())
    }
}

#[tauri::command]
async fn set_bandwidth_limits(
    upload_kbps: u64,
    download_kbps: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.bandwidth.set_limits(upload_kbps, download_kbps).await;
    Ok(())
}

#[tauri::command]
async fn establish_webrtc_connection(
    state: State<'_, AppState>,
    peer_id: String,
    offer: String,
) -> Result<(), String> {
    let webrtc = { state.webrtc.lock().await.as_ref().cloned() };
    if let Some(webrtc) = webrtc {
        webrtc
            .establish_connection_with_answer(peer_id, offer)
            .await
    } else {
        Err("WebRTC service not running".into())
    }
}

#[tauri::command]
async fn send_webrtc_file_request(
    state: State<'_, AppState>,
    peer_id: String,
    file_hash: String,
    file_name: String,
    file_size: u64,
) -> Result<(), String> {
    let webrtc = { state.webrtc.lock().await.as_ref().cloned() };
    if let Some(webrtc) = webrtc {
        let request = WebRTCFileRequest {
            file_hash,
            file_name,
            file_size,
            requester_peer_id: {
                let dht = state.dht.lock().await;
                if let Some(d) = dht.as_ref() {
                    d.get_peer_id().await
                } else {
                    "unknown".to_string()
                }
            },
            recipient_public_key: None, // No encryption for basic downloads
        };
        webrtc.send_file_request(peer_id, request).await
    } else {
        Err("WebRTC service not running".into())
    }
}

#[tauri::command]
async fn get_webrtc_connection_status(
    state: State<'_, AppState>,
    peer_id: String,
) -> Result<bool, String> {
    let webrtc = { state.webrtc.lock().await.as_ref().cloned() };
    if let Some(webrtc) = webrtc {
        Ok(webrtc.get_connection_status(&peer_id).await)
    } else {
        Ok(false)
    }
}

#[tauri::command]
async fn disconnect_from_peer(state: State<'_, AppState>, peer_id: String) -> Result<(), String> {
    let webrtc = { state.webrtc.lock().await.as_ref().cloned() };
    if let Some(webrtc) = webrtc {
        webrtc.close_connection(peer_id).await
    } else {
        Err("WebRTC service not running".into())
    }
}

/// Checks if the Geth RPC endpoint is ready to accept connections.
async fn is_geth_rpc_ready(state: &State<'_, AppState>) -> bool {
    let rpc_url = state.rpc_url.lock().await.clone();
    if let Ok(response) = reqwest::Client::new()
        .post(&rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "method": "net_version", "params": [], "id": 1
        }))
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(json) = response.json::<serde_json::Value>().await {
                return json.get("result").is_some();
            }
        }
    }
    false
}

/// Stops, restarts, and waits for the Geth node to be ready.
/// This is used when `miner_setEtherbase` is not available and a restart is required.
async fn restart_geth_and_wait(state: &State<'_, AppState>, data_dir: &str) -> Result<(), String> {
    info!("Restarting Geth with new configuration...");

    // Stop Geth
    state.geth.lock().await.stop()?;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await; // Brief pause for shutdown

    // Restart with the stored miner address
    {
        let mut geth = state.geth.lock().await;
        let miner_address = state.miner_address.lock().await;
        info!("Restarting Geth with miner address: {:?}", miner_address);
        geth.start(data_dir, miner_address.as_deref())?;
    }

    // Wait for Geth to become responsive
    let max_attempts = 30;
    for attempt in 1..=max_attempts {
        if is_geth_rpc_ready(state).await {
            info!("Geth is ready for RPC calls after restart.");
            return Ok(());
        }
        info!(
            "Waiting for Geth to start... (attempt {}/{})",
            attempt, max_attempts
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    Err("Geth failed to start up within 30 seconds after restart.".to_string())
}

#[tauri::command]
async fn get_miner_diagnostics(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    use crate::ethereum::NETWORK_CONFIG;
    use reqwest::Client;

    let client = Client::new();

    // Get current miner address from state
    let miner_addr = state.miner_address.lock().await;
    let current_miner = miner_addr
        .as_ref()
        .map(|s| s.clone())
        .unwrap_or_else(|| "Not set".to_string());

    // Get current block number
    let block_num_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let mut recent_miners = serde_json::Map::new();

    if let Ok(response) = client
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&block_num_payload)
        .send()
        .await
    {
        if let Ok(json) = response.json::<serde_json::Value>().await {
            if let Some(result) = json.get("result").and_then(|r| r.as_str()) {
                if let Ok(current_block) = u64::from_str_radix(&result[2..], 16) {
                    // Check last 5 blocks
                    for block_num in (current_block.saturating_sub(4)..=current_block).rev() {
                        let block_payload = serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": "eth_getBlockByNumber",
                            "params": [format!("0x{:x}", block_num), false],
                            "id": 1
                        });

                        if let Ok(block_response) = client
                            .post(&NETWORK_CONFIG.rpc_endpoint)
                            .json(&block_payload)
                            .send()
                            .await
                        {
                            if let Ok(block_json) = block_response.json::<serde_json::Value>().await
                            {
                                if let Some(block) = block_json.get("result") {
                                    if let Some(miner) = block.get("miner").and_then(|m| m.as_str())
                                    {
                                        recent_miners.insert(
                                            format!("{}", block_num),
                                            serde_json::Value::String(miner.to_string()),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let result = serde_json::json!({
        "current_miner_address": current_miner,
        "recent_block_miners": recent_miners
    });

    Ok(result)
}

#[tauri::command]
async fn start_miner(
    state: State<'_, AppState>,
    address: String,
    threads: u32,
    data_dir: String,
) -> Result<(), String> {
    // Store the miner address for future geth restarts
    {
        let mut miner_address = state.miner_address.lock().await;
        *miner_address = Some(address.clone());
    } // MutexGuard is dropped here

    // Also store in static variable for mining monitor
    {
        let mut current_address = CURRENT_MINER_ADDRESS.lock().await;
        *current_address = Some(address.clone());
    }

    // Try to start mining
    match start_mining(&address, threads).await {
        Ok(_) => Ok(()),
        Err(e) if e.contains("-32601") || e.to_lowercase().contains("does not exist") => {
            // miner_setEtherbase method doesn't exist, need to restart with etherbase
            warn!("miner_setEtherbase not supported, restarting geth with miner address...");
            restart_geth_and_wait(&state, &data_dir).await?;

            // Try mining again without setting etherbase (it's set via command line now)
            let rpc_url = state.rpc_url.lock().await.clone();
            let client = reqwest::Client::new();
            let start_mining_direct = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "miner_start",
                "params": [threads],
                "id": 1
            });

            let response = client
                .post(&rpc_url)
                .json(&start_mining_direct)
                .send()
                .await
                .map_err(|e| format!("Failed to start mining after restart: {}", e))?;

            let json_response: serde_json::Value = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            if let Some(error) = json_response.get("error") {
                Err(format!("Failed to start mining after restart: {}", error))
            } else {
                Ok(())
            }
        }
        Err(e) => Err(format!("Failed to start mining: {}", e)),
    }
}

#[tauri::command]
async fn stop_miner() -> Result<(), String> {
    // Try to stop mining, but if Geth is not running, just clear the state
    match stop_mining().await {
        Ok(_) => {
            // Successfully stopped mining
        }
        Err(e) if e.contains("Connection refused") || e.contains("connect error") => {
            // Geth is not running - this is okay, just clear the state
            eprintln!("Geth not running (connection refused) - clearing mining state");
        }
        Err(e) => {
            // Some other error - return it
            return Err(e);
        }
    }

    // Clear the current mining address
    {
        let mut current_address = CURRENT_MINER_ADDRESS.lock().await;
        *current_address = None;
    }
    Ok(())
}

#[tauri::command]
async fn get_miner_status() -> Result<bool, String> {
    get_mining_status().await
}

#[tauri::command]
async fn get_blockchain_sync_status(
    state: State<'_, AppState>,
) -> Result<ethereum::SyncStatus, String> {
    // Only query sync status if Geth is actually running
    let geth = state.geth.lock().await;
    if !geth.is_running() {
        return Err("Geth node is not running".to_string());
    }

    ethereum::get_sync_status().await
}

#[tauri::command]
async fn get_miner_hashrate() -> Result<String, String> {
    get_hashrate().await
}

#[tauri::command]
async fn get_current_block() -> Result<u64, String> {
    get_block_number().await
}

#[tauri::command]
async fn get_network_stats() -> Result<(String, String), String> {
    let difficulty = get_network_difficulty().await?;
    let hashrate = get_network_hashrate().await?;
    Ok((difficulty, hashrate.to_string()))
}

#[tauri::command]
fn get_chain_id() -> u64 {
    ethereum::NETWORK_CONFIG.chain_id
}

#[tauri::command]
async fn get_block_details_by_number(
    block_number: u64,
) -> Result<Option<serde_json::Value>, String> {
    ethereum::get_block_details_by_number(block_number).await
}

#[tauri::command]
async fn get_miner_logs(data_dir: String, lines: usize) -> Result<Vec<String>, String> {
    get_mining_logs(&data_dir, lines)
}

#[tauri::command]
async fn get_miner_performance(data_dir: String) -> Result<(u64, f64), String> {
    get_mining_performance(&data_dir).await
}

#[tauri::command]
async fn start_mining_monitor(app: tauri::AppHandle, data_dir: String) -> Result<(), String> {
    use std::fs::File;
    use std::io::{BufReader, Seek, SeekFrom};
    use tokio::time::{sleep, Duration};

    // Store the last position we read from
    static LAST_POSITION: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);

    // Resolve data directory
    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("Failed to get exe path: {}", e))?
        .parent()
        .ok_or("Failed to get exe dir")?
        .to_path_buf();
    let data_path = if Path::new(&data_dir).is_absolute() {
        PathBuf::from(&data_dir)
    } else {
        exe_dir.join(&data_dir)
    };
    let log_path = data_path.join("geth.log");

    tokio::spawn(async move {
        // Wait a moment for Geth to potentially create/update the log file
        tokio::time::sleep(Duration::from_secs(2)).await;

        loop {
            if let Ok(file) = File::open(&log_path) {
                let mut reader = BufReader::new(&file);

                // Get the last known position (don't hold the lock across await)
                let last_pos_value = {
                    let last_pos = LAST_POSITION.lock().unwrap();
                    *last_pos
                };

                // If this is the first run, start from the end of the file
                if last_pos_value.is_none() {
                    if let Ok(metadata) = file.metadata() {
                        let file_size = metadata.len();
                        let _ = reader.seek(SeekFrom::Start(file_size));
                    }
                } else if let Some(pos) = last_pos_value {
                    let _ = reader.seek(SeekFrom::Start(pos));
                }

                let mut new_lines = Vec::new();

                // Read new lines since last position
                if let Ok(metadata) = file.metadata() {
                    let file_size = metadata.len();

                    use std::io::BufRead;
                    for line_result in reader.lines() {
                        if let Ok(line) = line_result {
                            // Check if this line indicates a block was mined
                            // Only trigger on "Successfully sealed new block" to avoid duplicate events
                            if line.contains("Successfully sealed new block") {
                                // 🎉 WE MINED A BLOCK! 🎉
                                // Get the current mining address and increment the counter for that address
                                if let Some(miner_address) =
                                    CURRENT_MINER_ADDRESS.lock().await.clone()
                                {
                                    increment_mined_blocks(miner_address).await;
                                } else {
                                    println!("⚠️  Block mined but no current miner address set!");
                                }

                                // Emit event to frontend - that's it!
                                let result = app.emit(
                                    "block_mined",
                                    serde_json::json!({
                                        "log_line": line,
                                        "timestamp": chrono::Utc::now().timestamp()
                                    }),
                                );
                                match result {
                                    Ok(_) => {}
                                    Err(e) => {}
                                }
                            }
                            new_lines.push(line);
                        }
                    }

                    // Update the last position (acquire lock only for the update)
                    {
                        let mut last_pos = LAST_POSITION.lock().unwrap();
                        *last_pos = Some(file_size);
                    }
                }
            }

            // Check every 1 second
            sleep(Duration::from_secs(1)).await;
        }
    });

    Ok(())
}

lazy_static! {
    static ref BLOCKS_CACHE: Mutex<Option<(String, u64, Instant)>> = Mutex::new(None);
    // Running count of blocks mined per address
    static ref TOTAL_MINED_BLOCKS: Mutex<HashMap<String, u64>> = Mutex::new(HashMap::new());
    // Current mining address
    static ref CURRENT_MINER_ADDRESS: Mutex<Option<String>> = Mutex::new(None);
}

async fn increment_mined_blocks(miner_address: String) {
    // Normalize address to lowercase for consistent HashMap keys
    let normalized_address = miner_address.to_lowercase();
    let mut counts = TOTAL_MINED_BLOCKS.lock().await;
    let count = counts.entry(normalized_address.clone()).or_insert(0);
    *count += 1;
    println!(
        "🎉 Block mined by {}! Total blocks mined by this address: {}",
        normalized_address, *count
    );
}

async fn get_total_mined_blocks(miner_address: &str) -> u64 {
    // Normalize address to lowercase for consistent HashMap keys
    let normalized_address = miner_address.to_lowercase();
    let counts = TOTAL_MINED_BLOCKS.lock().await;
    *counts.get(&normalized_address).unwrap_or(&0)
}

/// Set the mined blocks count for an address (used to initialize from blockchain data)
pub async fn set_mined_blocks_count(miner_address: &str, count: u64) {
    // Normalize address to lowercase for consistent HashMap keys
    let normalized_address = miner_address.to_lowercase();
    let mut counts = TOTAL_MINED_BLOCKS.lock().await;
    counts.insert(normalized_address.clone(), count);
    println!(
        "📊 Initialized mined blocks count for {}: {}",
        normalized_address, count
    );
}

#[tauri::command]
async fn clear_blocks_cache() {
    let mut cache = BLOCKS_CACHE.lock().await;
    *cache = None;

    // Don't reset incremental scanning - let it continue from where it left off
    // This ensures we maintain our scanning progress and don't lose discovered blocks
}

/// Initialize the mined blocks count for an address from blockchain data
/// This should be called when an account is loaded to sync session counter with blockchain
#[tauri::command]
async fn initialize_mined_blocks_count(address: String, count: u64) -> Result<(), String> {
    set_mined_blocks_count(&address, count).await;
    Ok(())
}

#[tauri::command]
async fn get_blocks_mined(_app: tauri::AppHandle, address: String) -> Result<u64, String> {
    // Return the running count for this address
    let count = get_total_mined_blocks(&address).await;
    Ok(count)
}
#[tauri::command]
async fn get_recent_mined_blocks_pub(
    address: String,
    lookback: u64,
    limit: usize,
) -> Result<Vec<MinedBlock>, String> {
    get_recent_mined_blocks(&address, lookback, limit).await
}

#[tauri::command]
async fn get_mined_blocks_range(
    address: String,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<MinedBlock>, String> {
    ethereum::get_mined_blocks_range(&address, from_block, to_block).await
}

#[tauri::command]
async fn get_total_mining_rewards(address: String) -> Result<f64, String> {
    ethereum::get_total_mining_rewards(&address).await
}

#[tauri::command]
fn get_block_reward() -> f64 {
    ethereum::BLOCK_REWARD
}

#[tauri::command]
async fn calculate_accurate_totals(
    address: String,
    app: tauri::AppHandle,
) -> Result<ethereum::AccurateTotals, String> {
    ethereum::calculate_accurate_totals(&address, app).await
}

#[tauri::command]
async fn get_transaction_history(
    address: String,
    lookback: u64,
) -> Result<Vec<ethereum::TransactionHistoryItem>, String> {
    // Get current block number
    let current_block = ethereum::get_block_number().await?;

    // Calculate from_block (current - lookback, but not less than 0)
    let from_block = current_block.saturating_sub(lookback);

    // Scan transactions
    ethereum::get_transaction_history(&address, from_block, current_block).await
}

#[tauri::command]
async fn get_transaction_history_range(
    address: String,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<ethereum::TransactionHistoryItem>, String> {
    ethereum::get_transaction_history(&address, from_block, to_block).await
}

#[tauri::command]
async fn start_dht_node(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    port: u16,
    mut bootstrap_nodes: Vec<String>,
    enable_autonat: Option<bool>,
    autonat_probe_interval_secs: Option<u64>,
    autonat_servers: Option<Vec<String>>,
    proxy_address: Option<String>,
    is_bootstrap: Option<bool>,
    chunk_size_kb: Option<usize>,
    cache_size_mb: Option<usize>,
    // New optional relay controls
    enable_autorelay: Option<bool>,
    preferred_relays: Option<Vec<String>>,
    enable_relay_server: Option<bool>,
    enable_upnp: Option<bool>,
) -> Result<String, String> {
    {
        let dht_guard = state.dht.lock().await;
        if dht_guard.is_some() {
            return Err("DHT node is already running".to_string());
        }
    }

    // AutoNAT disabled by default - users can enable in settings if needed for NAT detection
    let auto_enabled = enable_autonat.unwrap_or(false);
    info!("AUTONAT {}", auto_enabled);
    let probe_interval = autonat_probe_interval_secs.map(Duration::from_secs);
    let autonat_server_list = autonat_servers.unwrap_or(bootstrap_nodes.clone());

    // Get the proxy from the command line, if it was provided at launch
    let cli_proxy = state.socks5_proxy_cli.lock().await.clone();
    // Prioritize the command-line argument. Fall back to the one from the UI.
    let final_proxy_address = cli_proxy.or(proxy_address.clone());

    // Get the file transfer service for DHT integration
    let file_transfer_service = {
        let ft_guard = state.file_transfer.lock().await;
        ft_guard.as_ref().cloned()
    };

    // Create a ChunkManager instance
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not get app data directory: {}", e))?;
    let chunk_storage_path = app_data_dir.join("chunk_storage");
    let chunk_manager = Arc::new(ChunkManager::new(chunk_storage_path));

    // --- AutoRelay is now disabled by default (can be enabled via config or env var)
    // Disable AutoRelay on bootstrap nodes (and via env var)
    let mut final_enable_autorelay = enable_autorelay.unwrap_or(true);
    if is_bootstrap.unwrap_or(false) {
        final_enable_autorelay = false;
        tracing::info!("AutoRelay disabled on bootstrap (hotfix).");
    }
    if std::env::var("CHIRAL_DISABLE_AUTORELAY").ok().as_deref() == Some("1") {
        final_enable_autorelay = false;
        tracing::info!("AutoRelay disabled via env CHIRAL_DISABLE_AUTORELAY=1");
    }

    // PHASE 2 FIX: Merge preferred relays into bootstrap nodes
    // This ensures relay nodes serve dual purpose:
    // 1. Circuit Relay v2 for NAT traversal
    // 2. DHT bootstrap for file discovery/publishing
    if let Some(relays) = &preferred_relays {
        for relay in relays {
            if !bootstrap_nodes.contains(relay) {
                info!(
                    "🔗 Adding relay {} to bootstrap nodes for DHT operations",
                    relay
                );
                bootstrap_nodes.push(relay.clone());
            }
        }
    }

    let proj_dirs = ProjectDirs::from("com", "chiral-network", "chiral-network")
        .ok_or("Failed to get project directories")?;
    let blockstore_db_path = proj_dirs.data_dir().join("blockstore_db");
    let async_blockstore_path = async_std::path::Path::new(blockstore_db_path.as_os_str());

    let previous_autorelay_enabled = {
        let guard = state.autorelay_last_enabled.lock().await;
        guard.clone()
    };
    let previous_autorelay_disabled = {
        let guard = state.autorelay_last_disabled.lock().await;
        guard.clone()
    };

    // Clone bootstrap nodes for health monitor before moving to DhtService::new
    let bootstrap_nodes_for_monitor = bootstrap_nodes.clone();

    let dht_service = DhtService::new(
        port,
        bootstrap_nodes,
        None,
        is_bootstrap.unwrap_or(false),
        auto_enabled,
        probe_interval,
        autonat_server_list,
        final_proxy_address,
        file_transfer_service,
        Some(chunk_manager.clone()), // Pass the chunk manager
        chunk_size_kb,
        cache_size_mb,
        /* enable AutoRelay (disabled by default) */ final_enable_autorelay,
        preferred_relays.unwrap_or_default(),
        is_bootstrap.unwrap_or(false), // enable_relay_server only on bootstrap
        enable_upnp.unwrap_or(true),   // enable UPnP by default
        Some(&async_blockstore_path),
        previous_autorelay_enabled,
        previous_autorelay_disabled,
    )
    .await
    .map_err(|e| format!("Failed to start DHT: {}", e))?;

    let (last_enabled, last_disabled) = dht_service.autorelay_history().await;
    {
        let mut guard = state.autorelay_last_enabled.lock().await;
        *guard = last_enabled;
    }
    {
        let mut guard = state.autorelay_last_disabled.lock().await;
        *guard = last_disabled;
    }

    let peer_id = dht_service.get_peer_id().await;

    // DHT node is already running in a spawned background task
    let dht_arc = Arc::new(dht_service);

    // Spawn the event pump
    let app_handle = app.clone();
    let proxies_arc = state.proxies.clone();
    let relay_reputation_arc = state.relay_reputation.clone();
    let dht_clone_for_pump = dht_arc.clone();
    let analytics_arc = state.analytics.clone();

    tokio::spawn(async move {
        use std::time::Duration;
        loop {
            // If the DHT service has been shut down, the weak reference will be None
            let events = dht_clone_for_pump.drain_events(64).await;
            if events.is_empty() {
                // Avoid busy-waiting
                tokio::time::sleep(Duration::from_millis(200)).await;
                // Check if the DHT is still alive before continuing
                if Arc::strong_count(&dht_clone_for_pump) <= 1 {
                    // 1 is the pump itself
                    info!("DHT service appears to be shut down. Exiting event pump.");
                    break;
                }
                continue;
            }

            for ev in events {
                match ev {
                    DhtEvent::PeerDiscovered { peer_id, addresses } => {
                        let payload = serde_json::json!({
                            "peerId": peer_id,
                            "addresses": addresses,
                        });
                        let _ = app_handle.emit("dht_peer_discovered", payload);
                    }
                    DhtEvent::PeerConnected { peer_id, address } => {
                        let payload = serde_json::json!({
                            "peerId": peer_id,
                            "address": address,
                        });
                        let _ = app_handle.emit("dht_peer_connected", payload);
                    }
                    DhtEvent::PeerDisconnected { peer_id } => {
                        let payload = serde_json::json!({ "peerId": peer_id });
                        let _ = app_handle.emit("dht_peer_disconnected", payload);
                    }
                    DhtEvent::ProxyStatus {
                        id,
                        address,
                        status,
                        latency_ms,
                        error,
                    } => {
                        let to_emit: ProxyNode = {
                            let mut proxies = proxies_arc.lock().await;

                            if let Some(i) = proxies.iter().position(|p| p.id == id) {
                                let p = &mut proxies[i];
                                if p.id != id {
                                    p.id = id.clone();
                                }
                                if !address.is_empty() {
                                    p.address = address.clone();
                                }
                                p.status = status.clone();
                                if let Some(ms) = latency_ms {
                                    p.latency = ms as u32;
                                }
                                p.error = error.clone();
                                p.clone()
                            } else {
                                let node = ProxyNode {
                                    id: id.clone(),
                                    address: address.clone(),
                                    status,
                                    latency: latency_ms.unwrap_or(0) as u32,
                                    error,
                                };
                                proxies.push(node.clone());
                                node
                            }
                        };

                        let _ = app_handle.emit("proxy_status_update", to_emit);
                    }
                    DhtEvent::NatStatus {
                        state,
                        confidence,
                        last_error,
                        summary,
                    } => {
                        let payload = serde_json::json!({
                            "state": state,
                            "confidence": confidence,
                            "lastError": last_error,
                            "summary": summary,
                        });
                        let _ = app_handle.emit("nat_status_update", payload);
                    }
                    DhtEvent::EchoReceived { from, utf8, bytes } => {
                        // Sending inbox event to frontend
                        let payload =
                            serde_json::json!({ "from": from, "text": utf8, "bytes": bytes });
                        let _ = app_handle.emit("proxy_echo_rx", payload);
                    }
                    DhtEvent::PeerRtt { peer, rtt_ms } => {
                        // NOTE: if from dht.rs only sends rtt for known proxies, then this is fine.
                        // If it can send rtt for any peer, we need to first check if it's generated from ProxyStatus
                        let mut proxies = proxies_arc.lock().await;
                        if let Some(p) = proxies.iter_mut().find(|p| p.id == peer) {
                            p.latency = rtt_ms as u32;
                            let _ = app_handle.emit("proxy_status_update", p.clone());
                        }
                    }
                    DhtEvent::DownloadedFile(metadata) => {
                        info!("Emitting file_content event for completed download: {} ({})", metadata.file_name, metadata.merkle_root);
                        let payload = serde_json::json!(metadata);
                        let _ = app_handle.emit("file_content", payload);

                        let file_size = metadata.file_size;

                        // Immediately re-publish the downloaded file so this node becomes a seeder.
                        let promote_metadata = metadata.clone();
                        let dht_for_promotion = dht_clone_for_pump.clone();
                        tokio::spawn(async move {
                            if let Err(err) = dht_for_promotion
                                .promote_downloaded_file(promote_metadata)
                                .await
                            {
                                warn!("Failed to promote downloaded file to seeder: {}", err);
                            }
                        });

                        // Update analytics: record download completion and bandwidth
                        analytics_arc.record_download_completed().await;
                        analytics_arc.record_download(file_size).await;
                        analytics_arc.decrement_active_downloads().await;
                    }
                    DhtEvent::PublishedFile(metadata) => {
                        println!("🔍 DEBUG MAIN: PublishedFile event received");
                        println!("🔍 DEBUG MAIN: metadata.seeders = {:?}", metadata.seeders);
                        let payload = serde_json::json!(metadata);
                        println!("🔍 DEBUG MAIN: Emitting published_file event to frontend");
                        let _ = app_handle.emit("published_file", payload);
                        // Update analytics: record upload completion
                        analytics_arc.record_upload_completed().await;
                        analytics_arc.decrement_active_uploads().await;
                    }
                    DhtEvent::FileDiscovered(metadata) => {
                        info!("📡 Emitting found_file event to frontend for: {}", metadata.file_name);
                        let payload = serde_json::json!(metadata);
                        let _ = app_handle.emit("found_file", payload);
                    }
                    DhtEvent::ReputationEvent {
                        peer_id,
                        event_type,
                        impact,
                        data,
                    } => {
                        // Update relay reputation statistics
                        let mut stats = relay_reputation_arc.lock().await;
                        let entry = stats.entry(peer_id.clone()).or_insert(RelayNodeStats {
                            peer_id: peer_id.clone(),
                            alias: None,
                            reputation_score: 0.0,
                            reservations_accepted: 0,
                            circuits_established: 0,
                            circuits_successful: 0,
                            total_events: 0,
                            last_seen: 0,
                        });

                        // Update statistics based on event type
                        entry.reputation_score += impact;
                        entry.total_events += 1;
                        entry.last_seen = data
                            .get("timestamp")
                            .and_then(|v| v.as_u64())
                            .unwrap_or_else(|| {
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or(std::time::Duration::from_secs(0))
                                    .as_secs()
                            });

                        match event_type.as_str() {
                            "RelayReservationAccepted" => entry.reservations_accepted += 1,
                            "RelayCircuitEstablished" => entry.circuits_established += 1,
                            "RelayCircuitSuccessful" => entry.circuits_successful += 1,
                            _ => {}
                        }

                        // Emit event to frontend
                        let payload = serde_json::json!({
                            "peerId": peer_id,
                            "eventType": event_type,
                            "impact": impact,
                            "data": data,
                        });
                        let _ = app_handle.emit("relay_reputation_event", payload);
                    }
                    DhtEvent::BitswapChunkDownloaded {
                        file_hash,
                        chunk_index,
                        total_chunks,
                        chunk_size,
                    } => {
                        let payload = serde_json::json!({
                            "fileHash": file_hash,
                            "chunkIndex": chunk_index,
                            "totalChunks": total_chunks,
                            "chunkSize": chunk_size,
                        });
                        let _ = app_handle.emit("bitswap_chunk_downloaded", payload);
                    }
                    DhtEvent::PaymentNotificationReceived { from_peer, payload } => {
                        println!(
                            "💰 Payment notification received from peer {}: {:?}",
                            from_peer, payload
                        );
                        // Convert payload to match the expected format for seeder_payment_received
                        if let Ok(notification) =
                            serde_json::from_value::<serde_json::Value>(payload.clone())
                        {
                            let formatted_payload = serde_json::json!({
                                "file_hash": notification.get("file_hash").and_then(|v| v.as_str()).unwrap_or(""),
                                "file_name": notification.get("file_name").and_then(|v| v.as_str()).unwrap_or(""),
                                "file_size": notification.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0),
                                "downloader_address": notification.get("downloader_address").and_then(|v| v.as_str()).unwrap_or(""),
                                "downloader_peer_id": notification.get("downloader_peer_id").and_then(|v| v.as_str()).unwrap_or(""),
                                "seeder_wallet_address": notification.get("seeder_wallet_address").and_then(|v| v.as_str()).unwrap_or(""),
                                "amount": notification.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                "transaction_id": notification.get("transaction_id").and_then(|v| v.as_u64()).unwrap_or(0),
                                "transaction_hash": notification.get("transaction_hash").and_then(|v| v.as_str()).unwrap_or(""),
                            });
                            // Emit the same event that local payments use
                            let _ = app_handle.emit("seeder_payment_received", formatted_payload);
                            println!("✅ Payment notification forwarded to frontend with transaction_hash and downloader_peer_id");
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    {
        let mut dht_guard = state.dht.lock().await;
        *dht_guard = Some(dht_arc.clone());
    }

    // Store chunk manager in AppState
    {
        let mut chunk_guard = state.chunk_manager.lock().await;
        *chunk_guard = Some(chunk_manager.clone());
    }

    // Also attach DHT to HTTP server state for provider-side metrics
    state.http_server_state.set_dht(dht_arc.clone()).await;

    // Monitor peer health and auto-reconnect to bootstrap when needed
    let dht_for_monitor = dht_arc.clone();
    let app_for_monitor = app.clone();
    
    tokio::spawn(async move {
        use std::time::Duration;
        let mut last_check = std::time::Instant::now();
        let check_interval = Duration::from_secs(30); // Check every 30 seconds
        const MINIMUM_PEERS: usize = 5; // Auto-reconnect if below this
        
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            
            // Check if DHT is still alive
            if Arc::strong_count(&dht_for_monitor) <= 1 {
                tracing::info!("DHT health monitor: DHT service shut down, exiting");
                break;
            }
            
            if last_check.elapsed() < check_interval {
                continue;
            }
            
            last_check = std::time::Instant::now();
            let peer_count = dht_for_monitor.get_peer_count().await;
            
            if peer_count < MINIMUM_PEERS {
                tracing::warn!(
                    "⚠️ Low peer count: {} (minimum: {}). Attempting to reconnect to bootstrap nodes...",
                    peer_count,
                    MINIMUM_PEERS
                );
                
                // Reconnect to bootstrap nodes
                for bootstrap_node in &bootstrap_nodes_for_monitor {
                    match dht_for_monitor.connect_peer(bootstrap_node.clone()).await {
                        Ok(_) => {
                            tracing::info!("📡 Reconnected to bootstrap node: {}", bootstrap_node);
                        }
                        Err(e) => {
                            tracing::debug!("Failed to reconnect to {}: {}", bootstrap_node, e);
                        }
                    }
                }
                
                // Emit warning to UI
                let _ = app_for_monitor.emit("dht_low_peer_count", serde_json::json!({
                    "peer_count": peer_count,
                    "minimum": MINIMUM_PEERS,
                    "message": format!("DHT has only {} peers. Reconnecting to bootstrap nodes...", peer_count)
                }));
            } else {
                tracing::debug!("✅ DHT peer count healthy: {}", peer_count);
            }
        }
    });

    Ok(peer_id)
}

#[tauri::command]
async fn stop_dht_node(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dht = {
        let mut dht_guard = state.dht.lock().await;
        dht_guard.take()
    };

    if let Some(dht) = dht {
        let (last_enabled, last_disabled) = dht.autorelay_history().await;
        {
            let mut guard = state.autorelay_last_enabled.lock().await;
            *guard = last_enabled;
        }
        {
            let mut guard = state.autorelay_last_disabled.lock().await;
            *guard = last_disabled;
        }

        (*dht)
            .shutdown()
            .await
            .map_err(|e| format!("Failed to stop DHT: {}", e))?;
    }

    // Proxy reset
    {
        let mut proxies = state.proxies.lock().await;
        proxies.clear();
    }
    let _ = app.emit("proxy_reset", ());

    Ok(())
}

#[tauri::command]
async fn stop_publishing_file(state: State<'_, AppState>, file_hash: String) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };
    if let Some(dht) = dht {
        dht.stop_publishing_file(file_hash).await
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn connect_to_peer(state: State<'_, AppState>, peer_address: String) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        dht.connect_peer(peer_address).await
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn is_dht_running(state: State<'_, AppState>) -> Result<bool, String> {
    let dht_guard = state.dht.lock().await;
    Ok(dht_guard.is_some())
}

#[tauri::command]
async fn get_dht_peer_count(state: State<'_, AppState>) -> Result<usize, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        Ok(dht.get_peer_count().await)
    } else {
        Ok(0) // Return 0 if DHT is not running
    }
}

#[tauri::command]
async fn get_dht_peer_id(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        Ok(Some(dht.get_peer_id().await))
    } else {
        Ok(None) // Return None if DHT is not running
    }
}

/// Get the peer ID (required for reputation system)
/// Returns error if DHT is not running
#[tauri::command]
async fn get_peer_id(state: State<'_, AppState>) -> Result<String, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let peer_id = dht.get_peer_id().await;
        println!("🔍 get_peer_id() called -> returning: {}", peer_id);
        Ok(peer_id)
    } else {
        println!("❌ get_peer_id() called but DHT is not running");
        Err("DHT is not running. Cannot get peer ID.".to_string())
    }
}

#[tauri::command]
async fn get_dht_connected_peers(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        // Get connected peers from DHT
        let connected_peers = dht.get_connected_peers().await;
        Ok(connected_peers)
    } else {
        Ok(Vec::new()) // Return empty vector if DHT is not running
    }
}

#[tauri::command]
async fn create_auth_session(
    state: State<'_, AppState>,
    session_id: String,
    hmac_key: Vec<u8>,
) -> Result<(), String> {
    let mut auth_service = state.stream_auth.lock().await;
    auth_service.create_session(session_id, hmac_key)
}

#[tauri::command]
async fn verify_stream_auth(
    state: State<'_, AppState>,
    session_id: String,
    auth_message: AuthMessage,
) -> Result<bool, String> {
    let mut auth_service = state.stream_auth.lock().await;
    auth_service.verify_data(&session_id, &auth_message)
}

#[tauri::command]
async fn generate_hmac_key() -> Vec<u8> {
    StreamAuthService::generate_hmac_key()
}

#[tauri::command]
async fn cleanup_auth_sessions(state: State<'_, AppState>) -> Result<(), String> {
    let mut auth_service = state.stream_auth.lock().await;
    auth_service.cleanup_expired_sessions();
    auth_service.cleanup_expired_exchanges();
    Ok(())
}

#[tauri::command]
async fn initiate_hmac_key_exchange(
    state: State<'_, AppState>,
    initiator_peer_id: String,
    target_peer_id: String,
    session_id: String,
) -> Result<HmacKeyExchangeRequest, String> {
    let mut auth_service = state.stream_auth.lock().await;
    auth_service.initiate_key_exchange(initiator_peer_id, target_peer_id, session_id)
}

#[tauri::command]
async fn respond_to_hmac_key_exchange(
    state: State<'_, AppState>,
    request: HmacKeyExchangeRequest,
    responder_peer_id: String,
) -> Result<HmacKeyExchangeResponse, String> {
    let mut auth_service = state.stream_auth.lock().await;
    auth_service.respond_to_key_exchange(request, responder_peer_id)
}

#[tauri::command]
async fn confirm_hmac_key_exchange(
    state: State<'_, AppState>,
    response: HmacKeyExchangeResponse,
    initiator_peer_id: String,
) -> Result<HmacKeyExchangeConfirmation, String> {
    let mut auth_service = state.stream_auth.lock().await;
    auth_service.confirm_key_exchange(response, initiator_peer_id)
}

#[tauri::command]
async fn finalize_hmac_key_exchange(
    state: State<'_, AppState>,
    confirmation: HmacKeyExchangeConfirmation,
    responder_peer_id: String,
) -> Result<(), String> {
    let mut auth_service = state.stream_auth.lock().await;
    auth_service.finalize_key_exchange(confirmation, responder_peer_id)
}

#[tauri::command]
async fn get_hmac_exchange_status(
    state: State<'_, AppState>,
    exchange_id: String,
) -> Result<Option<String>, String> {
    let auth_service = state.stream_auth.lock().await;
    Ok(auth_service
        .get_exchange_status(&exchange_id)
        .map(|s| format!("{:?}", s)))
}

#[tauri::command]
async fn get_active_hmac_exchanges(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let auth_service = state.stream_auth.lock().await;
    Ok(auth_service.get_active_exchanges())
}

#[tauri::command]
async fn get_dht_health(state: State<'_, AppState>) -> Result<Option<DhtMetricsSnapshot>, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        Ok(Some(dht.metrics_snapshot().await))
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn get_dht_events(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let events = dht.drain_events(100).await;
        // Convert events to concise human-readable strings for the UI
        let mapped: Vec<String> = events
            .into_iter()
            .map(|e| match e {
                // DhtEvent::PeerDiscovered(p) => format!("peer_discovered:{}", p),
                // DhtEvent::PeerConnected(p) => format!("peer_connected:{}", p),
                // DhtEvent::PeerDisconnected(p) => format!("peer_disconnected:{}", p),
                DhtEvent::PeerDiscovered { peer_id, addresses } => {
                    let joined = if addresses.is_empty() {
                        "-".to_string()
                    } else {
                        addresses.join("|")
                    };
                    format!("peer_discovered:{}:{}", peer_id, joined)
                }
                DhtEvent::PeerConnected { peer_id, address } => {
                    format!("peer_connected:{}:{}", peer_id, address.unwrap_or_default())
                }
                DhtEvent::PeerDisconnected { peer_id } => {
                    format!("peer_disconnected:{}", peer_id)
                }
                DhtEvent::FileDiscovered(meta) => {
                    // Serialize the full metadata object to JSON for the frontend
                    let payload = serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string());
                    format!("file_discovered:{}", payload)
                }
                DhtEvent::PublishedFile(meta) => format!(
                    "file_published:{}:{}:{}", // Use merkle_root as the primary identifier
                    meta.merkle_root, meta.file_name, meta.file_size
                ),
                DhtEvent::DownloadedFile(file_metadata) => {
                    format!("Downloaded File {}", file_metadata.file_name)
                }
                DhtEvent::FileNotFound(hash) => format!("file_not_found:{}", hash),
                DhtEvent::Error(err) => format!("error:{}", err),
                DhtEvent::Info(msg) => format!("info:{}", msg),
                DhtEvent::Warning(msg) => format!("warning:{}", msg),
                DhtEvent::ProxyStatus {
                    id,
                    address,
                    status,
                    latency_ms,
                    error,
                } => {
                    let lat = latency_ms
                        .map(|ms| format!("{ms}"))
                        .unwrap_or_else(|| "-".into());
                    let err = error.unwrap_or_default();
                    format!(
                        "proxy_status:{id}:{address}:{status}:{lat}{}",
                        if err.is_empty() {
                            "".into()
                        } else {
                            format!(":{err}")
                        }
                    )
                }
                DhtEvent::NatStatus {
                    state,
                    confidence,
                    last_error,
                    summary,
                } => match serde_json::to_string(&serde_json::json!({
                    "state": state,
                    "confidence": confidence,
                    "lastError": last_error,
                    "summary": summary,
                })) {
                    Ok(json) => format!("nat_status:{json}"),
                    Err(_) => "nat_status:{}".to_string(),
                },
                DhtEvent::PeerRtt { peer, rtt_ms } => format!("peer_rtt:{peer}:{rtt_ms}"),
                DhtEvent::EchoReceived { from, utf8, bytes } => format!(
                    "echo_received:{}:{}:{}",
                    from,
                    utf8.unwrap_or_default(),
                    bytes
                ),
                DhtEvent::BitswapDataReceived { query_id, data } => {
                    format!("bitswap_data_received:{}:{}", query_id, data.len())
                }
                DhtEvent::BitswapError { query_id, error } => {
                    format!("bitswap_error:{}:{}", query_id, error)
                }
                DhtEvent::FileDownloaded { file_hash } => {
                    format!("file_downloaded:{}", file_hash)
                }
                DhtEvent::BitswapChunkDownloaded {
                    file_hash,
                    chunk_index,
                    total_chunks,
                    chunk_size,
                } => {
                    format!(
                        "bitswap_chunk_downloaded:{}:{}:{}:{}",
                        file_hash, chunk_index, total_chunks, chunk_size
                    )
                }
                DhtEvent::PaymentNotificationReceived { from_peer, payload } => {
                    format!("payment_notification_received:{}:{:?}", from_peer, payload)
                }
                DhtEvent::ReputationEvent {
                    peer_id,
                    event_type,
                    impact,
                    data,
                } => {
                    let json = serde_json::to_string(&serde_json::json!({
                        "peer_id": peer_id,
                        "event_type": event_type,
                        "impact": impact,
                        "data": data,
                    }))
                    .unwrap_or_else(|_| "{}".to_string());
                    format!("reputation_event:{}", json)
                }
            })
            .collect();
        Ok(mapped)
    } else {
        Ok(vec![])
    }
}

#[derive(Debug, Clone)]
enum TemperatureMethod {
    Sysinfo,
    #[cfg(target_os = "windows")]
    WindowsWmi,
    #[cfg(target_os = "linux")]
    LinuxSensors,
    #[cfg(target_os = "linux")]
    LinuxThermalZone(String),
    #[cfg(target_os = "linux")]
    LinuxHwmon(String),
}
#[tauri::command]
async fn get_power_consumption() -> Option<f32> {
    tokio::task::spawn_blocking(move || {
        use std::sync::OnceLock;
        use std::time::Instant;

        static LAST_UPDATE: OnceLock<std::sync::Mutex<Option<Instant>>> = OnceLock::new();
        static POWER_HISTORY: OnceLock<std::sync::Mutex<Vec<(Instant, f32)>>> = OnceLock::new();
        static WORKING_METHOD: OnceLock<std::sync::Mutex<Option<PowerMethod>>> = OnceLock::new();

        let last_update_mutex = LAST_UPDATE.get_or_init(|| std::sync::Mutex::new(None));
        let power_history_mutex = POWER_HISTORY.get_or_init(|| std::sync::Mutex::new(Vec::new()));
        let working_method_mutex = WORKING_METHOD.get_or_init(|| std::sync::Mutex::new(None));

        // Check if we have a cached working method and if it's still working
        if let Ok(mut working_method) = working_method_mutex.lock() {
            if let Some(ref method) = *working_method {
                if let Some(power) = try_power_method(method) {
                    return Some(smooth_power(power));
                }
                // Method stopped working, clear cache
                *working_method = None;
            }
        }

        // Try all methods to find one that works and cache it
        let methods_to_try = vec![PowerMethod::Systemstat, PowerMethod::Sysinfo];

        for method in methods_to_try {
            if let Some(power) = try_power_method(&method) {
                // Cache the working method
                let mut working_method = working_method_mutex.lock().ok()?;
                *working_method = Some(method.clone());
                return Some(smooth_power(power));
            }
        }

        // Try Windows-specific methods if basic ones failed
        #[cfg(target_os = "windows")]
        {
            if let Some((power, method)) = get_windows_power() {
                if let Ok(mut working_method) = working_method_mutex.lock() {
                    *working_method = Some(method);
                }
                return Some(smooth_power(power));
            }
        }

        // Try Linux-specific methods if basic ones failed
        #[cfg(target_os = "linux")]
        {
            if let Some((power, method)) = get_linux_power() {
                if let Ok(mut working_method) = working_method_mutex.lock() {
                    *working_method = Some(method);
                }
                return Some(smooth_power(power));
            }
        }

        // Try Mac-specific methods if basic ones failed
        #[cfg(target_os = "macos")]
        {
            if let Some((power, method)) = get_mac_power() {
                if let Ok(mut working_method) = working_method_mutex.lock() {
                    *working_method = Some(method);
                }
                return Some(smooth_power(power));
            }
        }

        // Final fallback: return None when power monitoring is unavailable

        None
    })
    .await // Await the result of the blocking task
    .unwrap_or(None)
}

#[derive(Clone, Debug)]
enum PowerMethod {
    Sysinfo,
    Systemstat,
}

fn smooth_power(raw_power: f32) -> f32 {
    use std::sync::OnceLock;
    use std::time::Instant;

    static POWER_HISTORY: OnceLock<std::sync::Mutex<Vec<(Instant, f32)>>> = OnceLock::new();

    let power_history_mutex = POWER_HISTORY.get_or_init(|| std::sync::Mutex::new(Vec::new()));

    // Helper function to add power reading to history and return smoothed value
    let now = Instant::now();
    let mut history = match power_history_mutex.lock() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to acquire power history lock: {}", e);
            return raw_power; // Return raw power if lock fails
        }
    };

    // Add current reading
    history.push((now, raw_power));

    // Keep only last 10 readings within 60 seconds
    history.retain(|(time, _)| now.duration_since(*time).as_secs() < 60);
    if history.len() > 10 {
        history.remove(0);
    }

    // Return smoothed power (weighted average, recent readings have more weight)
    if history.len() == 1 {
        raw_power
    } else {
        let total_weight: f32 = (1..=history.len()).map(|i| i as f32).sum();
        let weighted_sum: f32 = history
            .iter()
            .enumerate()
            .map(|(i, (_, power))| power * (i + 1) as f32)
            .sum();
        weighted_sum / total_weight
    }
}

fn try_power_method(method: &PowerMethod) -> Option<f32> {
    match method {
        PowerMethod::Sysinfo => {
            // Note: sysinfo doesn't currently support power consumption monitoring
            // This is a placeholder for future sysinfo versions
            None
        }
        PowerMethod::Systemstat => {
            // systemstat also doesn't support power consumption directly
            // This could be extended with platform-specific implementations
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn get_windows_power() -> Option<(f32, PowerMethod)> {
    // Try multiple methods to get Windows power consumption

    // Method 1: Use PowerShell to query performance counters
    if let Some(power) = get_windows_power_via_powershell() {
        return Some((power, PowerMethod::Systemstat));
    }

    // Method 2: Try WMI queries for power information
    if let Some(power) = get_windows_power_via_wmi() {
        return Some((power, PowerMethod::Systemstat));
    }

    // Method 3: Try Windows Performance Counters directly
    if let Some(power) = get_windows_power_via_perf_counters() {
        return Some((power, PowerMethod::Systemstat));
    }

    // Method 4: Try direct Windows API calls or system information
    if let Some(power) = get_windows_power_via_system_info() {
        return Some((power, PowerMethod::Systemstat));
    }

    None
}

#[cfg(target_os = "windows")]
fn get_windows_power_via_powershell() -> Option<f32> {
    use std::process::Command;

    // Try to get CPU power consumption via Windows Performance Counters using PowerShell
    let ps_script = r#"
    try {
        # Get CPU power consumption from performance counters
        $counter = Get-Counter -Counter "\Processor Information(_Total)\% Processor Performance" -ErrorAction Stop
        $cpuUsage = $counter.CounterSamples.CookedValue

        # Estimate power based on CPU usage (this is an approximation, but better than nothing)
        # TDP for typical CPUs: assume 65W base + usage-based scaling
        $basePower = 65.0
        $usagePower = ($cpuUsage / 100.0) * 35.0  # Additional power based on usage
        $totalPower = $basePower + $usagePower

        # Output the power value
        [math]::Round($totalPower, 2)
    } catch {
        $null
    }
    "#;

    if let Ok(output) = Command::new("powershell")
        .args(["-Command", ps_script])
        .output()
    {
        if output.status.success() {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                let power_str = output_str.trim();
                if let Ok(power) = power_str.parse::<f32>() {
                    if power > 0.0 && power < 500.0 {
                        // Reasonable power range for CPU
                        return Some(power);
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn get_windows_power_via_wmi() -> Option<f32> {
    use std::process::Command;

    // Try WMI to get battery information (for laptops) or power scheme info
    let wmi_script = r#"
    try {
        # Try to get battery discharge rate (for laptops)
        $battery = Get-WmiObject -Class Win32_Battery -ErrorAction Stop | Select-Object -First 1
        if ($battery -and $battery.EstimatedChargeRemaining -ne $null -and $battery.EstimatedRunTime -ne $null) {
            # Calculate current power draw from battery
            $remainingTimeHours = $battery.EstimatedRunTime / 60.0
            if ($remainingTimeHours -gt 0) {
                # Estimate power consumption based on battery capacity and remaining time
                # This is approximate but gives real power usage for battery-powered systems
                $power = 0.0
                if ($battery.DesignCapacity -gt 0) {
                    $power = $battery.DesignCapacity / $remainingTimeHours
                    if ($power -gt 0 -and $power -lt 200) {
                        [math]::Round($power, 2)
                        exit 0
                    }
                }
            }
        }

        # Fallback: Try to get active power scheme information
        $scheme = Get-WmiObject -Class Win32_PowerPlan -Filter "IsActive=True" -ErrorAction Stop
        if ($scheme) {
            # This doesn't give actual power consumption, but we can use it as a fallback indicator
            # Return a default power consumption based on power scheme
            if ($scheme.ElementName -like "*High Performance*") {
                85.0
            } elseif ($scheme.ElementName -like "*Balanced*") {
                65.0
            } elseif ($scheme.ElementName -like "*Power Saver*") {
                45.0
            } else {
                65.0
            }
        } else {
            $null
        }
    } catch {
        $null
    }
    "#;

    if let Ok(output) = Command::new("powershell")
        .args(["-Command", wmi_script])
        .output()
    {
        if output.status.success() {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                let power_str = output_str.trim();
                if !power_str.is_empty() && power_str != "null" {
                    if let Ok(power) = power_str.parse::<f32>() {
                        if power > 0.0 && power < 200.0 {
                            // Reasonable power range
                            return Some(power);
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn get_windows_power_via_perf_counters() -> Option<f32> {
    use std::process::Command;

    // Try to get more detailed performance counter data
    let perf_script = r#"
    try {
        # Get multiple CPU-related performance counters
        $counters = @(
            "\Processor(_Total)\% Processor Time",
            "\Processor Information(_Total)\% Processor Performance"
        )

        $results = Get-Counter -Counter $counters -SampleInterval 1 -MaxSamples 1 -ErrorAction Stop

        $cpuTime = 0.0
        $cpuPerformance = 0.0

        foreach ($sample in $results.CounterSamples) {
            if ($sample.Path -like "*% Processor Time*") {
                $cpuTime = $sample.CookedValue
            } elseif ($sample.Path -like "*% Processor Performance*") {
                $cpuPerformance = $sample.CookedValue
            }
        }

        # Calculate power based on CPU activity
        # Base TDP assumption: 65W for typical desktop CPU
        $baseTDP = 65.0

        # Scale based on CPU performance percentage
        $power = $baseTDP * ($cpuPerformance / 100.0)

        # Ensure minimum power draw even when idle
        $power = [math]::Max($power, 35.0)

        [math]::Round($power, 2)
    } catch {
        $null
    }
    "#;

    if let Ok(output) = Command::new("powershell")
        .args(["-Command", perf_script])
        .output()
    {
        if output.status.success() {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                let power_str = output_str.trim();
                if let Ok(power) = power_str.parse::<f32>() {
                    if power > 0.0 && power < 500.0 {
                        // Reasonable power range
                        return Some(power);
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn get_windows_power_via_system_info() -> Option<f32> {
    use std::process::Command;

    // Try to get system information and estimate power based on actual hardware
    let system_info_script = r#"
    try {
        # Get CPU information
        $cpu = Get-WmiObject -Class Win32_Processor -ErrorAction Stop | Select-Object -First 1
        $cpuName = $cpu.Name
        $cpuCores = $cpu.NumberOfCores
        $cpuThreads = $cpu.NumberOfLogicalProcessors

        # Get current CPU usage
        $cpuUsage = (Get-WmiObject -Class Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'").PercentProcessorTime

        # Estimate TDP based on CPU model (this is more accurate than generic assumptions)
        $estimatedTDP = 65.0  # Default

        if ($cpuName -match "Intel.*Core.*i[3579]-1[0-9][0-9][0-9][0-9]") {
            # Intel 12th/13th/14th gen high-end CPUs
            $estimatedTDP = 125.0
        } elseif ($cpuName -match "Intel.*Core.*i[3579]-[0-9][0-9][0-9][0-9]") {
            # Intel 12th/13th/14th gen mainstream CPUs
            $estimatedTDP = 65.0
        } elseif ($cpuName -match "Intel.*Core.*i[3579]-[0-9][0-9][0-9]") {
            # Intel 10th/11th gen CPUs
            $estimatedTDP = 65.0
        } elseif ($cpuName -match "AMD.*Ryzen.*[79][0-9][0-9][0-9]") {
            # AMD Ryzen 7000/9000 series
            $estimatedTDP = 65.0
        } elseif ($cpuName -match "AMD.*Ryzen.*[3579][0-9][0-9][0-9]") {
            # AMD Ryzen 3000/5000/7000 series
            $estimatedTDP = 65.0
        } elseif ($cpuName -match "Threadripper") {
            # AMD Threadripper
            $estimatedTDP = 280.0
        }

        # Get RAM information and add its power consumption
        $ramModules = Get-WmiObject -Class Win32_PhysicalMemory -ErrorAction Stop
        $totalRamGB = 0
        foreach ($module in $ramModules) {
            $totalRamGB += [math]::Round($module.Capacity / 1GB, 0)
        }

        # Estimate RAM power consumption (about 2-3W per 8GB DDR4)
        $ramPower = [math]::Ceiling($totalRamGB / 8) * 2.5

        # Get GPU information (if dedicated GPU)
        $gpuPower = 0.0
        $gpus = Get-WmiObject -Class Win32_VideoController -ErrorAction Stop | Where-Object { $_.AdapterRAM -gt 0 }
        foreach ($gpu in $gpus) {
            if ($gpu.Name -notmatch "Microsoft Basic Display") {
                # Estimate GPU power based on VRAM
                $vramGB = [math]::Round($gpu.AdapterRAM / 1GB, 0)
                if ($gpu.Name -match "RTX|GeForce") {
                    $gpuPower += [math]::Max(50.0, $vramGB * 10.0)  # NVIDIA GPUs
                } elseif ($gpu.Name -match "Radeon|RX") {
                    $gpuPower += [math]::Max(40.0, $vramGB * 8.0)   # AMD GPUs
                } else {
                    $gpuPower += 30.0  # Generic dedicated GPU
                }
            }
        }

        # Calculate current power based on CPU usage
        $cpuPower = $estimatedTDP * ($cpuUsage / 100.0)
        $cpuPower = [math]::Max($cpuPower, $estimatedTDP * 0.3)  # Minimum 30% of TDP even when idle

        # Total system power
        $totalPower = $cpuPower + $ramPower + $gpuPower

        # Add a small base system power for motherboard, drives, etc.
        $totalPower += 25.0

        [math]::Round($totalPower, 2)
    } catch {
        $null
    }
    "#;

    if let Ok(output) = Command::new("powershell")
        .args(["-Command", system_info_script])
        .output()
    {
        if output.status.success() {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                let power_str = output_str.trim();
                if let Ok(power) = power_str.parse::<f32>() {
                    if power > 20.0 && power < 1000.0 {
                        // Reasonable power range for full system
                        return Some(power);
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn get_linux_power() -> Option<(f32, PowerMethod)> {
    use std::fs;

    // Try RAPL (Running Average Power Limit) interface on Intel systems
    // Read from all available RAPL domains (core, dram, etc.) and sum them

    static mut LAST_TOTAL_ENERGY: Option<(u64, Instant)> = None;
    static mut LAST_TOTAL_POWER: f32 = 0.0;

    // Find all RAPL energy files
    let mut rapl_paths = Vec::new();

    // Main package
    rapl_paths.push("/sys/class/powercap/intel-rapl:0/energy_uj".to_string());

    // Alternative main package path
    rapl_paths.push("/sys/class/powercap/intel-rapl/energy_uj".to_string());

    // Sub-domains (core, dram, etc.)
    for i in 0..10 {
        for j in 0..10 {
            let path = format!(
                "/sys/class/powercap/intel-rapl:{}/intel-rapl:{}:{}/energy_uj",
                i, i, j
            );
            if std::path::Path::new(&path).exists() {
                rapl_paths.push(path);
            }
        }
    }

    let mut total_energy_uj: u64 = 0;
    let mut valid_readings = 0;

    // Sum energy from all domains
    for path in &rapl_paths {
        if let Ok(energy_str) = fs::read_to_string(path) {
            if let Ok(energy_uj) = energy_str.trim().parse::<u64>() {
                total_energy_uj += energy_uj;
                valid_readings += 1;
            }
        }
    }

    if valid_readings == 0 {
        return None; // No RAPL sensors available
    }

    let now = Instant::now();

    unsafe {
        if let Some((last_total_energy, last_time)) = LAST_TOTAL_ENERGY {
            let time_diff = now.duration_since(last_time).as_secs_f64();
            if time_diff > 0.0 {
                let energy_diff = if total_energy_uj >= last_total_energy {
                    total_energy_uj - last_total_energy
                } else {
                    // Counter wrapped around (highly unlikely for total)
                    u64::MAX - last_total_energy + total_energy_uj
                };

                let power_watts = (energy_diff as f64 / 1_000_000.0) / time_diff; // Convert µJ to J, then to W

                if power_watts > 0.0 && power_watts < 2000.0 {
                    // Reasonable power range (allow higher for multi-domain sum)
                    LAST_TOTAL_POWER = power_watts as f32;
                    LAST_TOTAL_ENERGY = Some((total_energy_uj, now));
                    return Some((power_watts as f32, PowerMethod::Systemstat));
                }
            }
        } else {
            // First reading, store and wait for next
            LAST_TOTAL_ENERGY = Some((total_energy_uj, now));
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn get_mac_power() -> Option<(f32, PowerMethod)> {
    use std::time::Instant;

    // Static variables to track power readings over time
    static mut LAST_CPU_USAGE: Option<(f32, Instant)> = None;
    static mut LAST_POWER: f32 = 0.0;

    // Try to get power from SMC (System Management Controller)
    // SMC provides direct hardware power readings on Mac systems
    if let Some(power) = get_mac_power_from_smc() {
        return Some((power, PowerMethod::Systemstat));
    }

    // Fallback: Estimate power based on CPU usage
    // This is less accurate but works when SMC access is unavailable
    if let Some(power) = get_mac_power_from_cpu_usage() {
        return Some((power, PowerMethod::Sysinfo));
    }

    None
}

#[cfg(target_os = "macos")]
fn get_mac_power_from_smc() -> Option<f32> {
    // Try to read power consumption from SMC
    // The SMC provides real-time power metrics on Mac hardware

    // Note: SMC access on macOS requires specific hardware keys
    // This implementation may need adjustment based on actual Mac hardware
    // For now, we'll skip SMC implementation and rely on CPU estimation

    // The smc crate has complex API requirements and version conflicts
    // that make it difficult to use reliably across different Mac models

    None
}

#[cfg(target_os = "macos")]
fn get_mac_power_from_cpu_usage() -> Option<f32> {
    use std::process::Command;

    // Use system commands to estimate power consumption
    // Method 1: Get CPU usage and estimate from TDP

    // Get CPU usage percentage
    let cpu_usage_output = Command::new("sh")
        .arg("-c")
        .arg("ps -A -o %cpu | awk '{s+=$1} END {print s}'")
        .output();

    if let Ok(output) = cpu_usage_output {
        if output.status.success() {
            if let Ok(usage_str) = String::from_utf8(output.stdout) {
                if let Ok(cpu_usage) = usage_str.trim().parse::<f32>() {
                    // Get system info to estimate TDP
                    let sysctl_output = Command::new("sysctl")
                        .arg("-n")
                        .arg("machdep.cpu.brand_string")
                        .output();

                    let mut estimated_tdp = 15.0; // Default for MacBook

                    if let Ok(sysctl_out) = sysctl_output {
                        if let Ok(cpu_brand) = String::from_utf8(sysctl_out.stdout) {
                            // Estimate TDP based on CPU model
                            if cpu_brand.contains("M1")
                                || cpu_brand.contains("M2")
                                || cpu_brand.contains("M3")
                            {
                                // Apple Silicon - very efficient
                                estimated_tdp = 20.0; // M-series chips are low power
                            } else if cpu_brand.contains("Intel") {
                                // Intel Mac
                                if cpu_brand.contains("i9") {
                                    estimated_tdp = 45.0;
                                } else if cpu_brand.contains("i7") {
                                    estimated_tdp = 28.0;
                                } else if cpu_brand.contains("i5") {
                                    estimated_tdp = 20.0;
                                } else {
                                    estimated_tdp = 15.0;
                                }
                            }
                        }
                    }

                    // Get number of cores to adjust estimation
                    let core_count_output =
                        Command::new("sysctl").arg("-n").arg("hw.ncpu").output();

                    let mut num_cores = 4.0;
                    if let Ok(core_out) = core_count_output {
                        if let Ok(cores_str) = String::from_utf8(core_out.stdout) {
                            if let Ok(cores) = cores_str.trim().parse::<f32>() {
                                num_cores = cores;
                            }
                        }
                    }

                    // Calculate power consumption
                    // cpu_usage is total across all cores, so normalize by core count
                    let normalized_usage = cpu_usage / num_cores;
                    let cpu_power = estimated_tdp * (normalized_usage / 100.0);

                    // Add base system power (display, memory, etc.)
                    let base_power = 5.0; // Base system power for Mac
                    let total_power = cpu_power + base_power;

                    // Ensure minimum power draw
                    let final_power = total_power.max(estimated_tdp * 0.2); // At least 20% of TDP

                    if final_power > 0.0 && final_power < 200.0 {
                        return Some(final_power);
                    }
                }
            }
        }
    }

    None
}

#[tauri::command]
async fn get_cpu_temperature() -> Option<f32> {
    tokio::task::spawn_blocking(move || {
        use std::sync::OnceLock;
        use std::time::Instant;
        use sysinfo::MINIMUM_CPU_UPDATE_INTERVAL;
        use tracing::info;

        static LAST_UPDATE: OnceLock<std::sync::Mutex<Option<Instant>>> = OnceLock::new();
        static WORKING_METHOD: OnceLock<std::sync::Mutex<Option<TemperatureMethod>>> = OnceLock::new();
        static TEMP_HISTORY: OnceLock<std::sync::Mutex<Vec<(Instant, f32)>>> = OnceLock::new();

        let last_update_mutex = LAST_UPDATE.get_or_init(|| std::sync::Mutex::new(None));
        let working_method_mutex = WORKING_METHOD.get_or_init(|| std::sync::Mutex::new(None));
        let temp_history_mutex = TEMP_HISTORY.get_or_init(|| std::sync::Mutex::new(Vec::new()));

        {
            let mut last_update = last_update_mutex.lock().ok()?;
            if let Some(last) = *last_update {
                if last.elapsed() < MINIMUM_CPU_UPDATE_INTERVAL {
                    return None;
                }
            }
            *last_update = Some(Instant::now());
        }

        // Helper function to add temperature to history and return smoothed value
        let smooth_temperature = |raw_temp: f32| -> f32 {
            let now = Instant::now();
            let mut history = match temp_history_mutex.lock() {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!("Failed to acquire temperature history lock: {}", e);
                    return raw_temp; // Return raw temp if lock fails
                }
            };

            // Add current reading
            history.push((now, raw_temp));

            // Keep only last 5 readings within 30 seconds
            history.retain(|(time, _)| now.duration_since(*time).as_secs() < 30);
            if history.len() > 5 {
                let excess = history.len() - 5;
                history.drain(0..excess);
            }

            // Return smoothed temperature (weighted average, recent readings have more weight)
            if history.len() == 1 {
                raw_temp
            } else {
                let total_weight: f32 = (1..=history.len()).map(|i| i as f32).sum();
                let weighted_sum: f32 = history.iter().enumerate()
                    .map(|(i, (_, temp))| temp * (i + 1) as f32)
                    .sum();
                weighted_sum / total_weight
            }
        };

        // Try cached working method first
        {
            let working_method = working_method_mutex.lock().ok()?;
            if let Some(ref method) = *working_method {
                if let Some(temp) = try_temperature_method(method) {
                    return Some(smooth_temperature(temp));
                }
                // Method stopped working, clear cache
                drop(working_method);
                let mut working_method = working_method_mutex.lock().ok()?;
                *working_method = None;
            }
        }

        // Try all methods to find one that works and cache it
        let methods_to_try = vec![
            TemperatureMethod::Sysinfo,
            #[cfg(target_os = "windows")]
            TemperatureMethod::WindowsWmi,
            #[cfg(target_os = "linux")]
            TemperatureMethod::LinuxSensors,
        ];

        for method in methods_to_try {
            if let Some(temp) = try_temperature_method(&method) {
                // Cache the working method
                let mut working_method = working_method_mutex.lock().ok()?;
                *working_method = Some(method.clone());
                return Some(smooth_temperature(temp));
            }
        }

        // Try more Linux methods if the basic ones failed
        #[cfg(target_os = "linux")]
        {
            if let Some((temp, method)) = get_linux_temperature_advanced() {
                if let Ok(mut working_method) = working_method_mutex.lock() {
                    *working_method = Some(method);
                }
                return Some(smooth_temperature(temp));
            }
        }

        // Final fallback: return None when sensors are unavailable
        // Only log the info message once to avoid spamming logs
        static SENSOR_WARNING_LOGGED: OnceLock<()> = OnceLock::new();

        SENSOR_WARNING_LOGGED.get_or_init(|| {
            info!("Hardware temperature sensors not accessible on this system. Temperature monitoring disabled.");
        });

        None
    })
    .await // 2. Await the result of the blocking task
    .unwrap_or(None)
}

fn try_temperature_method(method: &TemperatureMethod) -> Option<f32> {
    match method {
        TemperatureMethod::Sysinfo => {
            let mut sys = System::new_all();
            sys.refresh_cpu_all();
            let components = Components::new_with_refreshed_list();

            let mut core_count = 0;
            let sum: f32 = components
                .iter()
                .filter(|c| {
                    let label = c.label().to_lowercase();
                    label.contains("cpu")
                        || label.contains("package")
                        || label.contains("tdie")
                        || label.contains("core")
                        || label.contains("thermal")
                })
                .map(|c| {
                    core_count += 1;
                    c.temperature()
                })
                .sum();

            if core_count > 0 {
                let avg_temp = sum / core_count as f32;
                if avg_temp > 0.0 && avg_temp < 150.0 {
                    return Some(avg_temp);
                }
            }
            None
        }
        #[cfg(target_os = "windows")]
        TemperatureMethod::WindowsWmi => get_windows_temperature(),
        #[cfg(target_os = "linux")]
        TemperatureMethod::LinuxSensors => get_linux_sensors_temperature(),
        #[cfg(target_os = "linux")]
        TemperatureMethod::LinuxThermalZone(path) => {
            if let Ok(temp_str) = std::fs::read_to_string(path) {
                if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                    let temp_celsius = temp_millidegrees as f32 / 1000.0;
                    if temp_celsius > 0.0 && temp_celsius < 150.0 {
                        return Some(temp_celsius);
                    }
                }
            }
            None
        }
        #[cfg(target_os = "linux")]
        TemperatureMethod::LinuxHwmon(path) => {
            if let Ok(temp_str) = std::fs::read_to_string(path) {
                if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                    let temp_celsius = temp_millidegrees as f32 / 1000.0;
                    if temp_celsius > 0.0 && temp_celsius < 150.0 {
                        return Some(temp_celsius);
                    }
                }
            }
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn get_linux_sensors_temperature() -> Option<f32> {
    // Try sensors command (most reliable and matches user expectations)
    if let Ok(output) = std::process::Command::new("sensors")
        .arg("-u") // Raw output
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            let lines: Vec<&str> = output_str.lines().collect();
            let mut i = 0;

            while i < lines.len() {
                let line = lines[i].trim();

                // Look for CPU package temperature section
                if line.contains("Package id 0:") {
                    // Look for temp1_input in the following lines
                    for j in (i + 1)..(i + 10).min(lines.len()) {
                        let temp_line = lines[j].trim();
                        if temp_line.starts_with("temp1_input:") {
                            if let Some(temp_str) = temp_line.split(':').nth(1) {
                                if let Ok(temp) = temp_str.trim().parse::<f32>() {
                                    if temp > 0.0 && temp < 150.0 {
                                        return Some(temp);
                                    }
                                }
                            }
                            break;
                        }
                    }
                }
                // Look for first core temperature as fallback
                else if line.contains("Core 0:") {
                    // Look for temp2_input (Core 0 uses temp2_input)
                    for j in (i + 1)..(i + 10).min(lines.len()) {
                        let temp_line = lines[j].trim();
                        if temp_line.starts_with("temp2_input:") {
                            if let Some(temp_str) = temp_line.split(':').nth(1) {
                                if let Ok(temp) = temp_str.trim().parse::<f32>() {
                                    if temp > 0.0 && temp < 150.0 {
                                        return Some(temp);
                                    }
                                }
                            }
                            break;
                        }
                    }
                }
                i += 1;
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn get_linux_temperature_advanced() -> Option<(f32, TemperatureMethod)> {
    use std::fs;

    // Method 1: Try thermal zones (prioritize x86_pkg_temp)
    // Look for CPU thermal zones in /sys/class/thermal/
    // Prioritize x86_pkg_temp as it's usually the most accurate for CPU package temperature
    for i in 0..20 {
        let type_path = format!("/sys/class/thermal/thermal_zone{}/type", i);
        if let Ok(zone_type) = fs::read_to_string(&type_path) {
            let zone_type = zone_type.trim().to_lowercase();
            if zone_type == "x86_pkg_temp" {
                let thermal_path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
                if let Ok(temp_str) = fs::read_to_string(&thermal_path) {
                    if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                        let temp_celsius = temp_millidegrees as f32 / 1000.0;
                        if temp_celsius > 0.0 && temp_celsius < 150.0 {
                            return Some((
                                temp_celsius,
                                TemperatureMethod::LinuxThermalZone(thermal_path),
                            ));
                        }
                    }
                }
            }
        }
    }

    // Fallback to other CPU thermal zones
    for i in 0..20 {
        let type_path = format!("/sys/class/thermal/thermal_zone{}/type", i);
        if let Ok(zone_type) = fs::read_to_string(&type_path) {
            let zone_type = zone_type.trim().to_lowercase();
            if zone_type.contains("cpu")
                || zone_type.contains("coretemp")
                || zone_type.contains("k10temp")
            {
                let thermal_path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
                if let Ok(temp_str) = fs::read_to_string(&thermal_path) {
                    if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                        let temp_celsius = temp_millidegrees as f32 / 1000.0;
                        if temp_celsius > 0.0 && temp_celsius < 150.0 {
                            return Some((
                                temp_celsius,
                                TemperatureMethod::LinuxThermalZone(thermal_path),
                            ));
                        }
                    }
                }
            }
        }
    }

    // Method 2: Try hwmon (hardware monitoring) interfaces
    // Look for CPU temperature sensors in /sys/class/hwmon/
    for i in 0..10 {
        let hwmon_dir = format!("/sys/class/hwmon/hwmon{}", i);

        // Check if this hwmon device is for CPU temperature
        let name_path = format!("{}/name", hwmon_dir);
        if let Ok(name) = fs::read_to_string(&name_path) {
            let name = name.trim().to_lowercase();
            if name.contains("coretemp")
                || name.contains("k10temp")
                || name.contains("cpu")
                || name.contains("acpi")
            {
                // Try different temperature input files
                for temp_input in 1..=8 {
                    let temp_path = format!("{}/temp{}_input", hwmon_dir, temp_input);
                    if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                        if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                            let temp_celsius = temp_millidegrees as f32 / 1000.0;
                            if temp_celsius > 0.0 && temp_celsius < 150.0 {
                                return Some((
                                    temp_celsius,
                                    TemperatureMethod::LinuxHwmon(temp_path),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // Method 3: Try reading from specific CPU temperature files using glob patterns
    let cpu_temp_paths = [
        "/sys/devices/platform/coretemp.0/hwmon/hwmon*/temp1_input",
        "/sys/devices/platform/coretemp.0/temp1_input",
        "/sys/bus/platform/devices/coretemp.0/hwmon/hwmon*/temp*_input",
        "/sys/devices/pci0000:00/0000:00:18.3/hwmon/hwmon*/temp1_input", // AMD
    ];

    for pattern in &cpu_temp_paths {
        if let Ok(paths) = glob::glob(pattern) {
            for path_result in paths {
                if let Ok(path) = path_result {
                    if let Ok(temp_str) = fs::read_to_string(&path) {
                        if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                            let temp_celsius = temp_millidegrees as f32 / 1000.0;
                            if temp_celsius > 0.0 && temp_celsius < 150.0 {
                                let path_str = path.to_string_lossy().to_string();
                                return Some((
                                    temp_celsius,
                                    TemperatureMethod::LinuxHwmon(path_str),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn get_windows_temperature() -> Option<f32> {
    use std::sync::OnceLock;

    static LAST_LOG_STATE: OnceLock<std::sync::Mutex<bool>> = OnceLock::new();

    // Try multiple WMI methods for better compatibility

    // Method 1: Try HighPrecisionTemperature (newer Windows versions)
    if let Ok(output) = Command::new("powershell")
        .args([
            "-Command",
            "try { Get-WmiObject -Query \"SELECT HighPrecisionTemperature FROM Win32_PerfRawData_Counters_ThermalZoneInformation\" -ErrorAction Stop | Select-Object -First 1 -ExpandProperty HighPrecisionTemperature } catch { $null }"
        ])
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            let trimmed = output_str.trim();
            if !trimmed.is_empty() && trimmed != "null" {
                if let Ok(temp_tenths_kelvin) = trimmed.parse::<f32>() {
                    let temp_celsius = (temp_tenths_kelvin / 10.0) - 273.15;
                    if temp_celsius > 0.0 && temp_celsius < 150.0 {
                        // Log success only once
                        let log_state = LAST_LOG_STATE.get_or_init(|| std::sync::Mutex::new(false));
                        if let Ok(mut logged) = log_state.lock() {
                            if !*logged {
                                *logged = true;
                            }
                        }
                        return Some(temp_celsius);
                    }
                }
            }
        }
    }

    // Method 2: Try CurrentTemperature (older Windows versions)
    if let Ok(output) = Command::new("powershell")
        .args([
            "-Command",
            "try { Get-WmiObject -Query \"SELECT CurrentTemperature FROM Win32_TemperatureProbe\" -ErrorAction Stop | Select-Object -First 1 -ExpandProperty CurrentTemperature } catch { $null }"
        ])
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            let trimmed = output_str.trim();
            if !trimmed.is_empty() && trimmed != "null" {
                if let Ok(temp_tenths_kelvin) = trimmed.parse::<f32>() {
                    let temp_celsius = (temp_tenths_kelvin / 10.0) - 273.15;
                    if temp_celsius > 0.0 && temp_celsius < 150.0 {
                        let log_state = LAST_LOG_STATE.get_or_init(|| std::sync::Mutex::new(false));
                        if let Ok(mut logged) = log_state.lock() {
                            if !*logged {
                                *logged = true;
                            }
                        }
                        return Some(temp_celsius);
                    }
                }
            }
        }
    }

    // Method 3: Try MSAcpi_ThermalZoneTemperature (alternative approach)
    if let Ok(output) = Command::new("powershell")
        .args([
            "-Command",
            "try { Get-WmiObject -Namespace \"root\\wmi\" -Query \"SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature\" -ErrorAction Stop | Select-Object -First 1 -ExpandProperty CurrentTemperature } catch { $null }"
        ])
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            let trimmed = output_str.trim();
            if !trimmed.is_empty() && trimmed != "null" {
                if let Ok(temp_tenths_kelvin) = trimmed.parse::<f32>() {
                    let temp_celsius = (temp_tenths_kelvin / 10.0) - 273.15;
                    if temp_celsius > 0.0 && temp_celsius < 150.0 {
                        let log_state = LAST_LOG_STATE.get_or_init(|| std::sync::Mutex::new(false));
                        if let Ok(mut logged) = log_state.lock() {
                            if !*logged {
                                *logged = true;
                            }
                        }
                        return Some(temp_celsius);
                    }
                }
            }
        }
    }

    // Log only once when no sensor is found
    let log_state = LAST_LOG_STATE.get_or_init(|| std::sync::Mutex::new(false));
    if let Ok(mut logged) = log_state.lock() {
        if !*logged {
            info!("⚠️ No WMI temperature sensors detected. Temperature monitoring disabled.");
            *logged = true;
        }
    }

    None
}

#[tauri::command]
fn detect_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".into())
}

/// Get the resolved download directory.
#[tauri::command]
fn get_download_directory(app: tauri::AppHandle) -> Result<String, String> {
    // Load backend settings from file
    let backend_settings = load_settings_from_file(&app);

    // If backend has a configured path, use it
    if !backend_settings.storage_path.is_empty() {
        let expanded_path = expand_tilde(&backend_settings.storage_path);
        return expanded_path
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Failed to convert path to string".to_string());
    }

    // Cross-platform default
    let default_path = "~/Downloads/Chiral-Network-Storage";

    let expanded_path = expand_tilde(default_path);
    expanded_path
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to convert path to string".to_string())
}

/// Validates a storage path to ensure it's a valid absolute path
/// This prevents issues where relative paths or tilde expansion
/// could create directories in unexpected locations.
/// 
/// Returns Ok(()) if path is valid, or Err with validation message.
/// The error message may be a warning (starting with "WARNING:") if the path
/// is valid but the directory doesn't exist yet.
#[tauri::command]
fn validate_storage_path(path: String) -> Result<(), String> {
    let trimmed = path.trim();
    
    if trimmed.is_empty() {
        return Err("Storage path cannot be empty".to_string());
    }
    
    // Platform-specific validation BEFORE general absolute check
    #[cfg(target_os = "windows")]
    {
        // On Windows, reject tilde since it's not supported
        if trimmed.starts_with('~') {
            return Err("The ~ character is not a valid Windows directory. Please enter a full Windows path (e.g., C:\\Users\\...) or use the folder picker.".to_string());
        }
        
        // On Windows, reject Unix-style paths (starting with /)
        if trimmed.starts_with('/') {
            return Err("Unix-style paths (e.g., /home/) are not valid on Windows. Please use a Windows path (e.g., C:\\Users\\...)".to_string());
        }
        
        // Extract drive letter and check if it exists
        if let Some(drive_letter) = trimmed.chars().next() {
            if drive_letter.is_ascii_alphabetic() {
                let drive_root = format!("{}:\\", drive_letter.to_ascii_uppercase());
                let drive_path = Path::new(&drive_root);
                
                // Check if the drive exists by checking if we can read the root directory
                if !drive_path.exists() {
                    return Err(format!("Drive {}:\\ does not exist on this system", drive_letter.to_ascii_uppercase()));
                }
            }
        }
    }
    
    let path_obj = Path::new(trimmed);
    
    // Path must be absolute (check after platform-specific validation)
    if !path_obj.is_absolute() {
        return Err("Storage path must be an absolute path (e.g., C:\\Users\\... on Windows or /home/... on Unix)".to_string());
    }
    
    // Check if directory exists - if not, it will be created
    if !path_obj.exists() {
        return Err(format!("WARNING: Directory does not exist and will be created: {}", trimmed));
    }
    
    Ok(())
}

#[tauri::command]
async fn ensure_directory_exists(path: String) -> Result<(), String> {
    let path_obj = Path::new(&path);

    // Get parent directory (in case path is a file path)
    let dir_to_create = if path_obj.extension().is_some() {
        // This looks like a file path, get the parent directory
        path_obj
            .parent()
            .ok_or_else(|| "Invalid path".to_string())?
    } else {
        // This is a directory path
        path_obj
    };

    tokio::fs::create_dir_all(dir_to_create)
        .await
        .map_err(|e| format!("Failed to create directory: {}", e))
}

#[tauri::command]
async fn start_file_transfer_service(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let ft_guard = state.file_transfer.lock().await;
        if ft_guard.is_some() {
            warn!("File transfer service is already running");
            return Err("File transfer service is already running".to_string());
        }
    }

    // Use the internal app data directory for file storage (hash-named files + metadata)
    // NOT the user's download directory - that's only for final downloaded files
    let file_transfer_service = FileTransferService::new_with_app_handle(app.clone())
        .await
        .map_err(|e| format!("Failed to start file transfer service: {}", e))?;

    let ft_arc = Arc::new(file_transfer_service);
    {
        let mut ft_guard = state.file_transfer.lock().await;
        *ft_guard = Some(ft_arc.clone());
    }

    // Initialize WebRTC service with file transfer service
    let webrtc_service = WebRTCService::new(
        app.app_handle().clone(),
        ft_arc.clone(),
        state.keystore.clone(),
        state.bandwidth.clone(),
    )
    .await
    .map_err(|e| format!("Failed to start WebRTC service: {}", e))?;

    let webrtc_arc = Arc::new(webrtc_service);
    {
        let mut webrtc_guard = state.webrtc.lock().await;
        *webrtc_guard = Some(webrtc_arc.clone());
    }

    // Set the global singleton to the SAME instance (not a duplicate!)
    // This is critical: process_incoming_chunk uses get_webrtc_service() to send HMAC
    // key exchange requests, and it needs to use the same connections map.
    set_webrtc_service(webrtc_arc.clone()).await;

    // Initialize multi-source download service
    let dht_arc = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht_service) = dht_arc.clone() {
        // Create transfer event bus for unified event emission
        let transfer_event_bus = Arc::new(TransferEventBus::new(app.app_handle().clone()));
        // Get chunk manager from AppState
        let chunk_manager_arc = {
            let chunk_guard = state.chunk_manager.lock().await;
            chunk_guard.as_ref().cloned()
        };

        let chunk_manager = chunk_manager_arc.ok_or_else(|| "Chunk manager not initialized".to_string())?;

        let multi_source_service = MultiSourceDownloadService::new(
            dht_service,
            webrtc_arc.clone(),
            state.bittorrent_handler.clone(),
            transfer_event_bus,
            state.analytics.clone(),
            chunk_manager,
        );
        let multi_source_arc                                                                                                                                                                                                             = Arc::new(multi_source_service);

        {
            let mut multi_source_guard = state.multi_source_download.lock().await;
            *multi_source_guard = Some(multi_source_arc.clone());
        }

        // Start multi-source download service
        {
            let mut pump_guard = state.multi_source_pump.lock().await;
            if pump_guard.is_none() {
                let app_handle = app.clone();
                let ms_clone = multi_source_arc.clone();
                let handle = tokio::spawn(async move {
                    pump_multi_source_events(app_handle, ms_clone).await;
                });
                *pump_guard = Some(handle);
            }
        }

        // Start the service background task
        let ms_clone = multi_source_arc.clone();
        tokio::spawn(async move {
            ms_clone.run().await;
        });
    }

    {
        let mut pump_guard = state.file_transfer_pump.lock().await;
        if pump_guard.is_none() {
            let app_handle = app.clone();
            let ft_clone = ft_arc.clone();
            let handle = tokio::spawn(async move {
                pump_file_transfer_events(app_handle, ft_clone).await;
            });
            *pump_guard = Some(handle);
        }
    }

    Ok(())
}

#[tauri::command]
async fn upload_file_to_network(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
    price: Option<f64>,
    protocol: Option<String>,
    original_file_name: Option<String>,
) -> Result<(), String> {
    // Use provided original filename, or extract from path if not provided
    let original_file_name = original_file_name.unwrap_or_else(|| {
        Path::new(&file_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    // Ensure price is never null - default to 0
    let price = price.unwrap_or(0.0);

    // Get the active account for uploader_address
    let account = get_active_account(&state).await?;

    // Calculate file hash without loading entire file into memory
    let mut hasher = sha2::Sha256::new();
    let mut file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|e| format!("Failed to open file for hashing: {}", e))?;
    let mut buffer = vec![0u8; 64 * 1024]; // 64KB chunks for hashing

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Failed to read chunk for hashing: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let file_hash = format!("{:x}", hasher.finalize());
    let file_size = tokio::fs::metadata(&file_path)
        .await
        .map_err(|e| format!("Failed to get file size: {}", e))?
        .len();

    let dont_need_to_copy_protocols = vec!["BitSwap", "WebRTC"];
    let mut file_path = file_path.clone();

    // Handle protocol-specific uploads
    if let Some(protocol_name) = &protocol {
        if !dont_need_to_copy_protocols.contains(&protocol_name.as_str()) {
            // handle if error
            file_path = copy_file_to_temp(file_path.clone())
                .await
                .map_err(|e| format!("Failed to copy file to temp: {}", e))?;
        }

        match protocol_name.as_str() {
            "HTTP" => {
                let permanent_path = state.http_server_state.storage_dir.join(&file_hash);
                // Move/rename temp file to permanent storage instead of copying
                tokio::fs::rename(&file_path, &permanent_path)
                    .await
                    .map_err(|e| format!("Failed to move file to permanent storage: {}", e))?;

                // Update file_path to point to the permanent location
                let file_path = permanent_path.to_string_lossy().to_string();

                state
                    .http_server_state
                    .register_file(http_server::HttpFileMetadata {
                        hash: file_hash.clone(),
                        file_hash: file_hash.clone(),
                        name: original_file_name.clone(),
                        size: file_size,
                        encrypted: false,
                    })
                    .await;
            }
            "BitTorrent" => {
                // Check if file exists before attempting to seed
                if !std::path::Path::new(&file_path).exists() {
                    error!(
                        "BitTorrent seeding failed: File does not exist: {}",
                        file_path
                    );
                    return Err(format!("File does not exist: {}", file_path));
                }

                if !std::path::Path::new(&file_path).is_file() {
                    error!(
                        "BitTorrent seeding failed: Path is not a file: {}",
                        file_path
                    );
                    return Err(format!("Path is not a file: {}", file_path));
                }

                // Use torrent seeding
                let handler = state.bittorrent_handler.clone();
                match create_and_seed_torrent_internal(file_path.clone(), handler).await {
                    Ok(magnet_link) => {
                        let info_hash = {
                            // Extract info hash from magnet link
                            if let Some(start) = magnet_link.find("urn:btih:") {
                                let start = start + 9;
                                let end = magnet_link[start..]
                                    .find('&')
                                    .map(|i| start + i)
                                    .unwrap_or(magnet_link.len());
                                Some(magnet_link[start..end].to_lowercase())
                            } else {
                                None
                            }
                        };

                        // Get the local peer ID to add as a seeder
                        let local_peer_id = {
                            let dht_guard = state.dht.lock().await;
                            if let Some(dht) = dht_guard.as_ref() {
                                Some(dht.get_peer_id().await)
                            } else {
                                None
                            }
                        };

                        let metadata = FileMetadata {
                            merkle_root: info_hash.clone().unwrap_or_else(|| file_hash.clone()), // Use info_hash as key for magnet link searches
                            is_root: true,
                            file_name: original_file_name.clone(),
                            file_size,
                            file_data: vec![], // Not stored for torrents
                            seeders: local_peer_id.clone().map_or(vec![], |id| vec![id]),
                            created_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            mime_type: None,
                            is_encrypted: false,
                            encryption_method: None,
                            key_fingerprint: None,
                            parent_hash: None,
                            cids: None,
                            encrypted_key_bundle: None,
                            price,
                            uploader_address: Some(account),
                            ftp_sources: None,
                            http_sources: None,
                            info_hash: info_hash.clone(),
                            trackers: Some(vec!["udp://tracker.openbittorrent.com:80".to_string()]),
                            ed2k_sources: None,
                            download_path: None,
                        };

                        // Publish merged metadata to DHT for discoverability
                        let dht = {
                            let dht_guard = state.dht.lock().await;
                            dht_guard.as_ref().cloned()
                        };

                        if let Some(dht) = dht {
                            if let Err(e) = dht.publish_file(metadata.clone(), None).await {
                                warn!("Failed to publish BitTorrent file metadata to DHT: {}", e);
                                // Don't fail the upload, just log the warning
                            }
                        }

                        return Ok(());
                    }
                    Err(e) => {
                        return Err(format!("Failed to create torrent: {}", e));
                    }
                }
            }
            "ED2K" => {
                // Actually use the ED2K protocol handler to generate real ed2k links

                let file_path_buf = PathBuf::from(&file_path);

                // Create ED2K protocol handler with an active server URL
                // ED2K now works in decentralized P2P mode with DHT support
                let ed2k_handler = protocols::ed2k::Ed2kProtocolHandler::new(
                    "ed2k://|server|45.82.80.155|5687|/".to_string(),
                );

                // Seed the file using the protocol handler
                let seed_options = protocols::traits::SeedOptions {
                    announce_dht: false, // ED2K has its own DHT
                    enable_encryption: false,
                    upload_slots: None,
                };

                match ed2k_handler.seed(file_path_buf.clone(), seed_options).await {
                    Ok(seeding_info) => {
                        let file_size = match tokio::fs::metadata(&file_path).await {
                            Ok(metadata) => metadata.len(),
                            Err(_) => 0,
                        };

                        let ed2k_hash = {
                            // Extract hash from ed2k link: ed2k://|file|name|size|hash|/
                            let parts: Vec<&str> = seeding_info.identifier.split('|').collect();
                            if parts.len() >= 5 {
                                Some(parts[4].to_string())
                            } else {
                                None
                            }
                        };

                        // Get the local peer ID to add as a seeder
                        let local_peer_id = {
                            let dht_guard = state.dht.lock().await;
                            if let Some(dht) = dht_guard.as_ref() {
                                Some(dht.get_peer_id().await)
                            } else {
                                None
                            }
                        };

                        let metadata = FileMetadata {
                            merkle_root: ed2k_hash.clone().unwrap_or_else(|| file_hash.clone()), // Use ED2K hash as key for ED2K link searches
                            is_root: true,
                            file_name: original_file_name.clone(),
                            file_size,
                            file_data: vec![],
                            seeders: local_peer_id.clone().map_or(vec![], |id| vec![id]),
                            created_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            mime_type: None,
                            is_encrypted: false,
                            encryption_method: None,
                            key_fingerprint: None,
                            parent_hash: None,
                            cids: None,
                            encrypted_key_bundle: None,
                            price,
                            uploader_address: Some(account),
                            ftp_sources: None,
                            http_sources: None,
                            info_hash: None,
                            trackers: None,
                            ed2k_sources: Some(vec![dht::models::Ed2kSourceInfo {
                                server_url: "ed2k://|server|45.82.80.155|5687|/".to_string(),
                                file_hash: ed2k_hash
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                file_size,
                                file_name: Some(original_file_name.clone()),
                                sources: None,
                                timeout: None,
                                chunk_hashes: None,
                            }]),
                            download_path: None,
                        };

                        // Publish merged metadata to DHT for discoverability
                        let dht = {
                            let dht_guard = state.dht.lock().await;
                            dht_guard.as_ref().cloned()
                        };

                        if let Some(dht) = dht {
                            if let Err(e) = dht.publish_file(metadata.clone(), None).await {
                                warn!("Failed to publish ED2K file metadata to DHT: {}", e);
                                // Don't fail the upload, just log the warning
                            }
                        }

                        return Ok(());
                    }
                    Err(e) => {
                        println!("❌ ED2K seeding failed: {}", e);
                        return Err(format!("ED2K seeding failed: {}", e));
                    }
                }
            }
            "FTP" => {
                // FTP upload uses the built-in FTP server
                println!("📡 FTP upload: Using built-in FTP server");

                // Ensure FTP server is running
                if !state.ftp_server.is_running().await {
                    state.ftp_server.start().await
                        .map_err(|e| format!("Failed to start FTP server: {}", e))?;
                }

                // Read the file data
                let file_data = tokio::fs::read(&file_path)
                    .await
                    .map_err(|e| format!("Failed to read file: {}", e))?;
                let file_size = file_data.len() as u64;

                // Use file hash as the filename to ensure uniqueness
                let ftp_file_name = format!("{}_{}", file_hash, original_file_name);

                // Add file to FTP server
                let ftp_url = state.ftp_server.add_file_data(&file_data, &ftp_file_name).await
                    .map_err(|e| format!("Failed to add file to FTP server: {}", e))?;

                println!("✅ File added to FTP server: {}", ftp_url);

                let metadata = FileMetadata {
                    merkle_root: file_hash.clone(),
                    is_root: true,
                    file_name: original_file_name.clone(),
                    file_size,
                    file_data: vec![],
                    seeders: vec![],
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    mime_type: None,
                    is_encrypted: false,
                    encryption_method: None,
                    key_fingerprint: None,
                    parent_hash: None,
                    cids: None,
                    encrypted_key_bundle: None,
                    price,
                    uploader_address: Some(account),
                    http_sources: None,
                    ftp_sources: Some(vec![dht::models::FtpSourceInfo {
                        url: ftp_url.clone(),
                        username: None,
                        password: None,
                        supports_resume: true,
                        file_size,
                        last_checked: Some(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                        ),
                        is_available: true,
                    }]),
                    info_hash: None,
                    trackers: None,
                    ed2k_sources: None,
                    download_path: None,
                };

                let dht = {
                    let dht_guard = state.dht.lock().await;
                    dht_guard.as_ref().cloned()
                };

                if let Some(dht) = dht {
                    if let Err(e) = dht.publish_file(metadata.clone(), None).await {
                        warn!("Failed to publish FTP file metadata to DHT: {}", e);
                    }
                }

                println!("✅ FTP upload complete - file available at: {}", ftp_url);
                return Ok(());
            }
            "Bitswap" => {
                // Use streaming upload for Bitswap to handle large files
                println!(
                    "📡 Using streaming Bitswap upload for protocol: {}",
                    protocol_name
                );

                // For very large files, use a smaller chunk size to reduce memory pressure
                let chunk_size = if file_size > 1024 * 1024 * 1024 {
                    // > 1GB
                    256 * 1024 // 256KB chunks
                } else {
                    1024 * 1024 // 1MB chunks
                };

                let total_chunks = ((file_size + chunk_size - 1) / chunk_size) as usize;

                println!(
                    "📡 Starting Bitswap streaming upload: {} chunks of {} bytes each",
                    total_chunks, chunk_size
                );

                // Start streaming upload session
                let upload_id = start_streaming_upload(
                    original_file_name.clone(),
                    file_size,
                    price,
                    state.clone(),
                )
                .await?;

                // Stream file in chunks
                let mut file = tokio::fs::File::open(&file_path)
                    .await
                    .map_err(|e| format!("Failed to open file for streaming: {}", e))?;

                let mut chunk_index = 0;
                let mut buffer = vec![0u8; chunk_size as usize];

                loop {
                    let bytes_read = file
                        .read(&mut buffer)
                        .await
                        .map_err(|e| format!("Failed to read chunk: {}", e))?;

                    if bytes_read == 0 {
                        break; // EOF
                    }

                    // Create chunk data (truncate if partial read)
                    let chunk_data = if bytes_read < buffer.len() {
                        buffer[..bytes_read].to_vec()
                    } else {
                        buffer.clone()
                    };

                    let is_last_chunk = chunk_index >= total_chunks - 1; // Use >= to handle edge cases

                    // Send chunk
                    upload_file_chunk(
                        upload_id.clone(),
                        chunk_data,
                        chunk_index as u32,
                        is_last_chunk,
                        state.clone(),
                    )
                    .await?;

                    // Progress logging for large files
                    if chunk_index % 100 == 0 || is_last_chunk {
                        println!(
                            "📊 Upload progress: {}/{} chunks ({:.1}%)",
                            chunk_index + 1,
                            total_chunks,
                            (chunk_index + 1) as f64 / total_chunks as f64 * 100.0
                        );
                    }

                    chunk_index += 1;

                    // Prevent too many concurrent operations
                    if chunk_index % 50 == 0 {
                        tokio::task::yield_now().await;
                    }
                }

                // After all chunks are uploaded, finalize the metadata
                let mut upload_sessions = state.upload_sessions.lock().await;
                if let Some(session) = upload_sessions.get_mut(&upload_id) {
                    if session.is_complete {
                        // Calculate Merkle root for integrity verification
                        let hasher = std::mem::replace(&mut session.hasher, sha2::Sha256::new());
                        let merkle_root = format!("{:x}", hasher.finalize());

                        // Create root block containing the list of chunk CIDs
                        let chunk_cids = std::mem::take(&mut session.chunk_cids);
                        let root_block_data = match serde_json::to_vec(&chunk_cids) {
                            Ok(data) => data,
                            Err(e) => {
                                return Err(format!("Failed to serialize chunk CIDs: {}", e));
                            }
                        };

                        // Generate CID for the root block
                        use dht::{Cid, Code, MultihashDigest, RAW_CODEC};
                        let root_cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(&root_block_data));

                        // Store root block in Bitswap
                        let dht_opt = { state.dht.lock().await.as_ref().cloned() };
                        if let Some(dht) = &dht_opt {
                            if let Err(e) = dht.store_block(root_cid.clone(), root_block_data).await {
                                error!("failed to store root block: {}", e);
                                return Err(format!("failed to store root block: {}", e));
                            }
                        } else {
                            return Err("DHT not running".into());
                        }

                        // Create minimal metadata (without file_data to avoid DHT size limits)
                        let created_at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or(std::time::Duration::from_secs(0))
                            .as_secs();

                        // Get the account address for the uploader
                        let account = get_active_account(&state).await?;

                        let metadata = dht::models::FileMetadata {
                            merkle_root: merkle_root.clone(), // Store Merkle root for verification
                            file_name: session.file_name.clone(),
                            file_size: session.file_size,
                            file_data: vec![], // Empty - data is stored in Bitswap blocks
                            seeders: vec![],
                            created_at,
                            mime_type: None,
                            is_encrypted: false,
                            encryption_method: None,
                            key_fingerprint: None,
                            cids: Some(vec![root_cid.clone()]), // The root CID for retrieval
                            encrypted_key_bundle: None,
                            parent_hash: None,
                            is_root: true,
                            download_path: None,
                            price: session.price,
                            uploader_address: Some(account),
                            ftp_sources: None,
                            http_sources: None,
                            info_hash: None,
                            trackers: None,
                            ed2k_sources: None,
                        };

                        // Publish merged metadata to DHT
                        if let Some(dht) = dht_opt {
                            dht.publish_file(metadata.clone(), None).await?;
                        } else {
                            return Err("DHT not running".into());
                        }

                        let file_hash = root_cid.to_string();
                        println!("✅ Bitswap streaming upload completed: {}", file_hash);

                        // Clean up session
                        upload_sessions.remove(&upload_id);
                    }
                }
                drop(upload_sessions);

                return Ok(());
            }
            _ => {
                // WebRTC and other protocols use the default Chiral flow
                // Spawn in background task to avoid callback timeout issues
                println!(
                    "📡 Using Chiral network upload for protocol: {}",
                    protocol_name
                );

                // Get required state before spawning
                let account = get_active_account(&state).await?;
                let private_key = {
                    let key_guard = state.active_account_private_key.lock().await;
                    key_guard
                        .clone()
                        .ok_or("No private key available. Please log in again.")?
                };
                let ft = {
                    let ft_guard = state.file_transfer.lock().await;
                    ft_guard.as_ref().cloned()
                };
                let dht = {
                    let dht_guard = state.dht.lock().await;
                    dht_guard.as_ref().cloned()
                };

                let ft = ft.ok_or("File transfer service is not running")?;
                let dht = dht.ok_or("DHT Service not running.")?;
                
                // Get local peer ID to add as seeder
                let local_peer_id = dht.get_peer_id().await;

                // Spawn background task - return immediately to avoid callback timeout
                tokio::spawn(async move {
                    let result: Result<(), String> = async {
                        let c = file_path.clone();
                        let file_name = Path::new(&c)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&file_path);

                        ft.upload_file_with_account(
                            file_path.clone(),
                            file_name.to_string(),
                            Some(account.clone()),
                            Some(private_key),
                        )
                        .await
                        .map_err(|e| format!("Failed to upload file: {}", e))?;

                        let file_data = tokio::fs::read(&file_path)
                            .await
                            .map_err(|e| format!("Failed to read file: {}", e))?;
                        let file_hash = file_transfer::FileTransferService::calculate_file_hash(&file_data);

                        let created_at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or(std::time::Duration::from_secs(0))
                            .as_secs();

                        let metadata = FileMetadata {
                            merkle_root: file_hash.clone(),
                            is_root: true,
                            file_name: original_file_name.clone(),
                            file_size: file_data.len() as u64,
                            file_data: vec![],
                            seeders: vec![local_peer_id.clone()],
                            created_at,
                            mime_type: None,
                            is_encrypted: false,
                            encryption_method: None,
                            key_fingerprint: None,
                            parent_hash: None,
                            cids: None,
                            encrypted_key_bundle: None,
                            price,
                            uploader_address: Some(account.clone()),
                            ftp_sources: None,
                            http_sources: None,
                            info_hash: None,
                            trackers: None,
                            ed2k_sources: None,
                            download_path: None,
                        };

                        dht.publish_file(metadata.clone(), None).await?;

                        ft.store_file_data(file_hash.clone(), file_name.to_string(), file_data.clone())
                            .await;

                        info!(
                            "WebRTC upload complete: {} (merkle_root: {})",
                            file_name, metadata.merkle_root
                        );

                        Ok(())
                    }.await;

                    if let Err(e) = result {
                        error!("WebRTC upload failed: {}", e);
                    }
                });

                // Return immediately - frontend will receive published_file event when done
                return Ok(());
            }
        }
    }

    // This code path should no longer be reached for WebRTC uploads
    Err("Unexpected code path in upload_file_to_network".to_string())
}

#[tauri::command]
async fn start_ftp_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
    output_path: String,
    username: Option<String>,
    password: Option<String>,
) -> Result<String, String> {
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().ok_or("Invalid FTP URL")?;
    let path = parsed.path();
    let file_name = path.split('/').last().unwrap_or("ftp_download").to_string();

    // Generate a unique transfer ID
    let transfer_id = format!("ftp-{}", uuid::Uuid::new_v4());

    // Create transfer event bus for emitting events
    let transfer_event_bus = TransferEventBus::new(app.clone());
    let analytics_service = state.analytics.clone();
    let start_time = std::time::Instant::now();

    // Connect to FTP
    let mut ftp = FtpStream::connect((host, 21))
        .map_err(|e| format!("Failed to connect to FTP server: {}", e))?;

    // Login
    if let (Some(user), Some(pass)) = (username.clone(), password.clone()) {
        ftp.login(&user, &pass)
            .map_err(|e| format!("FTP login failed: {}", e))?;
    } else {
        ftp.login("anonymous", "anonymous")
            .map_err(|e| format!("Anonymous login failed: {}", e))?;
    }

    // Get file size if possible
    let file_size = ftp.size(path).unwrap_or(0) as u64;

    // Emit started event
    transfer_event_bus
        .emit_started_with_analytics(
            TransferStartedEvent {
                transfer_id: transfer_id.clone(),
                file_hash: transfer_id.clone(),
                file_name: file_name.clone(),
                file_size,
                total_chunks: 1,
                chunk_size: file_size as usize,
                started_at: current_timestamp_ms(),
                available_sources: vec![SourceInfo {
                    id: host.to_string(),
                    source_type: SourceType::Ftp,
                    address: url.clone(),
                    reputation: None,
                    estimated_speed_bps: None,
                    latency_ms: None,
                    location: None,
                }],
                selected_sources: vec![host.to_string()],
            },
            &analytics_service,
        )
        .await;

    // Get unique output path to avoid overwriting existing files
    let unique_output_path = get_unique_filepath(Path::new(&output_path));
    let output_path = unique_output_path.to_string_lossy().to_string();

    // Create output file
    let mut file = std::fs::File::create(&output_path).map_err(|e| {
        // Capture error message before moving
        let error_msg = format!("Failed to create output file: {}", e);

        // Emit failed event on file creation error
        let event_bus = TransferEventBus::new(app.clone());
        let analytics = analytics_service.clone();
        let tid = transfer_id.clone();
        let error_for_event = error_msg.clone();
        tokio::spawn(async move {
            event_bus
                .emit_failed_with_analytics(
                    TransferFailedEvent {
                        transfer_id: tid.clone(),
                        file_hash: tid,
                        failed_at: current_timestamp_ms(),
                        error: error_for_event,
                        error_category: ErrorCategory::Filesystem,
                        downloaded_bytes: 0,
                        total_bytes: file_size,
                        retry_possible: true,
                    },
                    &analytics,
                )
                .await;
        });
        error_msg
    })?;

    // Track downloaded bytes for progress updates
    let downloaded_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let downloaded_bytes_clone = downloaded_bytes.clone();

    // Retrieve file and stream in chunks
    let result = ftp.retr(path, |reader| {
        let mut buffer = [0u8; 65536]; // 64 KB
        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .map_err(|e| suppaftp::FtpError::ConnectionError(e))?;
            if bytes_read == 0 {
                break; // End of file
            }
            file.write_all(&buffer[..bytes_read])
                .map_err(|e| suppaftp::FtpError::ConnectionError(e))?;
            downloaded_bytes_clone
                .fetch_add(bytes_read as u64, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    });

    ftp.quit().ok();

    let total_downloaded = downloaded_bytes.load(std::sync::atomic::Ordering::Relaxed);
    let duration = start_time.elapsed();
    let avg_speed = if duration.as_secs_f64() > 0.0 {
        total_downloaded as f64 / duration.as_secs_f64()
    } else {
        0.0
    };

    match result {
        Ok(()) => {
            // Emit completed event
            transfer_event_bus
                .emit_completed_with_analytics(
                    TransferCompletedEvent {
                        transfer_id: transfer_id.clone(),
                        file_hash: transfer_id,
                        file_name,
                        file_size: total_downloaded,
                        output_path: output_path.clone(),
                        completed_at: current_timestamp_ms(),
                        duration_seconds: duration.as_secs(),
                        average_speed_bps: avg_speed,
                        total_chunks: 1,
                        sources_used: Vec::new(),
                    },
                    &analytics_service,
                )
                .await;

            Ok(format!("Downloaded successfully to {}", output_path))
        }
        Err(e) => {
            // Emit failed event
            transfer_event_bus
                .emit_failed_with_analytics(
                    TransferFailedEvent {
                        transfer_id: transfer_id.clone(),
                        file_hash: transfer_id,
                        failed_at: current_timestamp_ms(),
                        error: format!("FTP RETR failed: {}", e),
                        error_category: ErrorCategory::Network,
                        downloaded_bytes: total_downloaded,
                        total_bytes: file_size,
                        retry_possible: true,
                    },
                    &analytics_service,
                )
                .await;

            Err(format!("FTP RETR failed: {}", e))
        }
    }
}

#[tauri::command]
async fn add_ed2k_source(
    file_hash: String,
    ed2k_link: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let ed2k_info = Ed2kSourceInfo::from_ed2k_link(&ed2k_link)
        .map_err(|e| format!("Invalid ed2k link: {}", e))?;

    let dht_guard = state.dht.lock().await;
    let dht = dht_guard.as_ref().ok_or("DHT not initialized")?;

    let metadata_opt = dht
        .synchronous_search_metadata(file_hash.clone(), 3000)
        .await?;

    let mut metadata = metadata_opt.ok_or("Metadata not found")?;

    let mut list = metadata.ed2k_sources.take().unwrap_or_default();
    list.push(ed2k_info);
    metadata.ed2k_sources = Some(list);

    dht.publish_file(metadata, None).await?;

    Ok(())
}

#[tauri::command]
async fn list_ed2k_sources(
    file_hash: String,
    state: State<'_, AppState>,
) -> Result<Vec<Ed2kSourceInfo>, String> {
    let dht_guard = state.dht.lock().await;
    let dht = dht_guard.as_ref().ok_or("DHT not initialized")?;

    let metadata_opt = dht
        .synchronous_search_metadata(file_hash.clone(), 3000)
        .await?;

    let metadata = metadata_opt.ok_or(format!("Metadata not found for {}", file_hash))?;

    Ok(metadata.ed2k_sources.unwrap_or_default())
}

#[tauri::command]
async fn remove_ed2k_source(
    file_hash: String,
    server_url: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;
    let dht = dht_guard.as_ref().ok_or("DHT not initialized")?;

    let metadata_opt = dht
        .synchronous_search_metadata(file_hash.clone(), 3000)
        .await?;

    let mut metadata = metadata_opt.ok_or("Metadata not found")?;

    if let Some(list) = &mut metadata.ed2k_sources {
        list.retain(|s| s.server_url != server_url);
    }

    dht.publish_file(metadata, None).await?;

    Ok(())
}

#[tauri::command]
async fn test_ed2k_connection(server_url: String) -> Result<Ed2kServerInfo, String> {
    use ed2k_client::Ed2kClient;

    let mut client = Ed2kClient::new(server_url.clone());

    // Try to connect
    client
        .connect()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?;

    // Get server info
    let server_info = client.get_server_info().await.map_err(|e| e.to_string())?;

    Ok(server_info)
}

#[tauri::command]
async fn search_ed2k_file(
    query: String,
    server_url: Option<String>,
) -> Result<Vec<Ed2kSearchResult>, String> {
    let server = server_url.unwrap_or_else(|| "ed2k://|server|45.82.80.155|5687|/".to_string());
    let mut client = Ed2kClient::new(server);

    client.connect().await.map_err(|e| e.to_string())?;
    let results = client.search(&query).await.map_err(|e| e.to_string())?;

    Ok(results)
}

#[tauri::command]
async fn get_ed2k_download_status(
    file_hash: String,
    state: State<'_, AppState>,
) -> Result<Ed2kDownloadStatus, String> {
    // Since ED2K downloading is not implemented yet,
    // return a placeholder status
    Ok(Ed2kDownloadStatus {
        progress: 0.0,
        downloaded_bytes: 0,
        total_bytes: 0,
        state: format!("No ED2K download active for {}", file_hash),
    })
}

#[tauri::command]
fn parse_ed2k_link(ed2k_link: String) -> Result<Ed2kSourceInfo, String> {
    Ed2kSourceInfo::from_ed2k_link(&ed2k_link).map_err(|e| e.to_string())
}

#[tauri::command]
async fn download_blocks_from_network(
    state: State<'_, AppState>,
    file_metadata: FileMetadata,
    download_path: String,
) -> Result<(), String> {
    info!("🔽 download_blocks_from_network called for file: {} to path: {}", file_metadata.file_name, download_path);
    info!("🔽 file has {} seeders, cids: {:?}", file_metadata.seeders.len(), file_metadata.cids);

    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        info!("🔽 DHT node is running, calling dht.download_file");
        dht.download_file(file_metadata, download_path).await
    } else {
        error!("🔽 DHT node is not running!");
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn download_file_from_network(
    state: State<'_, AppState>,
    file_hash: String,
    output_path: String, // Remove the underscore - we'll use this now
) -> Result<String, String> {
    use std::path::Path;

    // ✅ VALIDATE OUTPUT PATH BEFORE STARTING DOWNLOAD
    let path = Path::new(&output_path);

    // Check if parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(format!(
                "Download failed: Directory does not exist: {}",
                parent.display()
            ));
        }
        if !parent.is_dir() {
            return Err(format!(
                "Download failed: Path is not a directory: {}",
                parent.display()
            ));
        }
    } else {
        return Err("Download failed: Invalid file path".to_string());
    }

    let ft = {
        let ft_guard = state.file_transfer.lock().await;
        ft_guard.as_ref().cloned()
    };

    if let Some(_ft) = ft {
        info!("Starting P2P download for: {}", file_hash);

        // Search DHT for file metadata
        let dht = {
            let dht_guard = state.dht.lock().await;
            dht_guard.as_ref().cloned()
        };

        if let Some(dht_service) = dht {
            // Search for file metadata in DHT with 35 second timeout
            // This is longer than the Kademlia query timeout (30s) to account for:
            // - Provider queries that run in parallel (can take 3-5s)
            // - Network latency and retries
            // - Multiple query rounds for distant peers
            match dht_service
                .synchronous_search_metadata(file_hash.clone(), 35000)
                .await
            {
                Ok(Some(metadata)) => {
                    info!(
                        "Found file metadata in DHT: {} (size: {} bytes)",
                        metadata.file_name, metadata.file_size
                    );

                    // Check if we are the seeder - if so, use local copy instead of WebRTC
                    let local_peer_id = dht_service.get_peer_id().await;
                    let is_local_seeder = metadata.seeders.contains(&local_peer_id);
                    
                    if is_local_seeder {
                        info!("We are a seeder for this file - using local copy instead of WebRTC");
                        
                        // Get file data from local storage
                        let ft = {
                            let ft_guard = state.file_transfer.lock().await;
                            ft_guard.as_ref().cloned()
                        };
                        
                        if let Some(file_transfer) = ft {
                            match file_transfer.get_file_data(&metadata.merkle_root).await {
                                Some(file_data) => {
                                    // Get unique output path to avoid overwriting existing files
                                    let unique_output_path = get_unique_filepath(Path::new(&output_path));
                                    let final_output_path = unique_output_path.to_string_lossy().to_string();
                                    
                                    // Write to output path
                                    use std::io::Write;
                                    let mut file = std::fs::File::create(&final_output_path)
                                        .map_err(|e| format!("Failed to create output file: {}", e))?;
                                    file.write_all(&file_data)
                                        .map_err(|e| format!("Failed to write file data: {}", e))?;
                                    
                                    info!("Local copy completed: {} -> {}", metadata.file_name, final_output_path);
                                    return Ok(format!(
                                        "Download completed (local copy): {} ({} bytes)",
                                        metadata.file_name, file_data.len()
                                    ));
                                }
                                None => {
                                    warn!("Failed to get local file data - file not found in local storage");
                                    // Fall through to try WebRTC with other peers
                                }
                            }
                        }
                    }

                    // Implement peer discovery for file chunks
                    info!(
                        "Discovering peers for file: {} with {} known seeders",
                        metadata.file_name,
                        metadata.seeders.len()
                    );

                    if metadata.seeders.is_empty() {
                        return Err(format!(
                            "No seeders available for file: {} ({})",
                            metadata.file_name, metadata.merkle_root
                        ));
                    }

                    // Discover and verify available peers for this file
                    let available_peers = dht_service
                        .discover_peers_for_file(&metadata)
                        .await
                        .map_err(|e| format!("Peer discovery failed: {}", e))?;

                    if available_peers.is_empty() {
                        warn!("File found but no seeders currently available or reachable");
                        return Err(format!(
                            "No seeders available for '{}'. The file exists but no peers are currently online or reachable.",
                            metadata.file_name
                        ));
                    }

                    // Implement chunk requesting protocol with real WebRTC
                    // Create WebRTC offer for the first available peer
                    let webrtc = {
                        let webrtc_guard = state.webrtc.lock().await;
                        webrtc_guard.as_ref().cloned()
                    };

                    if let Some(webrtc_service) = webrtc {
                        // Try multiple peers with retry logic
                        let max_retries = 3;
                        let mut last_error = String::new();
                        let mut tried_peers: Vec<String> = Vec::new();

                        for attempt in 0..max_retries {
                            // Select the best peer that hasn't been tried yet
                            let selected_peer = if available_peers.len() == 1 {
                                available_peers[0].clone()
                            } else {
                                // Filter out already tried peers
                                let untried_peers: Vec<_> = available_peers
                                    .iter()
                                    .filter(|p| !tried_peers.contains(p))
                                    .cloned()
                                    .collect();

                                if untried_peers.is_empty() {
                                    // All peers tried, start over with delay
                                    if attempt < max_retries - 1 {
                                        info!("All peers tried, waiting before retry...");
                                        tokio::time::sleep(Duration::from_secs(2)).await;
                                        tried_peers.clear();
                                    }
                                    available_peers[0].clone()
                                } else {
                                    // Use peer selection strategy on untried peers
                                    let recommended = dht_service
                                        .select_peers_with_strategy(
                                            &untried_peers,
                                            1,
                                            peer_selection::SelectionStrategy::FastestFirst,
                                            false,
                                        )
                                        .await;
                                    recommended
                                        .into_iter()
                                        .next()
                                        .unwrap_or_else(|| untried_peers[0].clone())
                                }
                            };

                            tried_peers.push(selected_peer.clone());
                            info!(
                                "Selected peer {} for WebRTC download (attempt {}/{})",
                                selected_peer,
                                attempt + 1,
                                max_retries
                            );

                            // Check if peer is connected, try to reconnect if not
                            let connected_peers = dht_service.get_connected_peers().await;
                            if !connected_peers.contains(&selected_peer) {
                                info!(
                                    "Peer {} not connected, attempting to reconnect...",
                                    selected_peer
                                );
                                // Try to reconnect
                                let _ = dht_service
                                    .connect_to_peer_by_id(selected_peer.clone())
                                    .await;
                                // Give it a moment to establish connection
                                tokio::time::sleep(Duration::from_millis(500)).await;

                                // Re-check connection
                                let connected_peers = dht_service.get_connected_peers().await;
                                if !connected_peers.contains(&selected_peer) {
                                    warn!(
                                        "Failed to reconnect to peer {}, trying next peer",
                                        selected_peer
                                    );
                                    last_error =
                                        format!("Peer {} not reachable", selected_peer);
                                    continue;
                                }
                            }

                            // Check if we already have an open WebRTC connection to this peer
                            if webrtc_service.has_open_connection(&selected_peer).await {
                                info!("♻️ Reusing existing WebRTC connection to peer {}", selected_peer);
                                
                                // Just send the file request directly
                                let file_request = webrtc_service::WebRTCFileRequest {
                                    file_hash: metadata.merkle_root.clone(),
                                    file_name: metadata.file_name.clone(),
                                    file_size: metadata.file_size,
                                    requester_peer_id: dht_service.get_peer_id().await,
                                    recipient_public_key: None,
                                };

                                match webrtc_service.send_file_request(selected_peer.clone(), file_request).await {
                                    Ok(_) => {
                                        info!("Sent file request for {} to peer {} (reused connection)", metadata.file_name, selected_peer);
                                        state.analytics.increment_active_downloads().await;
                                        return Ok(format!(
                                            "WebRTC download initiated: {} ({} bytes) from peer {} (reused connection)",
                                            metadata.file_name, metadata.file_size, selected_peer
                                        ));
                                    }
                                    Err(e) => {
                                        warn!("Failed to send file request on existing connection: {}, will create new connection", e);
                                        // Fall through to create new connection
                                    }
                                }
                            }

                            // Create WebRTC offer
                            match webrtc_service.create_offer(selected_peer.clone()).await {
                                Ok(offer) => {
                                    info!("Created WebRTC offer for peer {}", selected_peer);

                                    // Send WebRTC offer via DHT signaling
                                    let offer_request = dht::WebRTCOfferRequest {
                                        offer_sdp: offer,
                                        file_hash: metadata.merkle_root.clone(),
                                        requester_peer_id: dht_service.get_peer_id().await,
                                    };

                                    match dht_service
                                        .send_webrtc_offer(selected_peer.clone(), offer_request)
                                        .await
                                    {
                                        Ok(answer_receiver) => {
                                            info!(
                                                "Sent WebRTC offer to peer {}, waiting for answer",
                                                selected_peer
                                            );

                                            // Wait for WebRTC answer with timeout
                                            match tokio::time::timeout(
                                                Duration::from_secs(30),
                                                answer_receiver,
                                            )
                                            .await
                                            {
                                                Ok(Ok(Ok(answer_response))) => {
                                                    info!(
                                                        "Received WebRTC answer from peer {}",
                                                        selected_peer
                                                    );

                                                    // Establish WebRTC connection with the answer
                                                    match webrtc_service
                                                        .establish_connection_with_answer(
                                                            selected_peer.clone(),
                                                            answer_response.answer_sdp,
                                                        )
                                                        .await
                                                    {
                                                        Ok(_) => {
                                                            info!("WebRTC connection established with peer {}", selected_peer);

                                                            // Send file request over WebRTC data channel
                                                            let file_request =
                                                                webrtc_service::WebRTCFileRequest {
                                                                    file_hash: metadata
                                                                        .merkle_root
                                                                        .clone(),
                                                                    file_name: metadata
                                                                        .file_name
                                                                        .clone(),
                                                                    file_size: metadata.file_size,
                                                                    requester_peer_id: dht_service
                                                                        .get_peer_id()
                                                                        .await,
                                                                    recipient_public_key: None,
                                                                };

                                                            match webrtc_service
                                                                .send_file_request(
                                                                    selected_peer.clone(),
                                                                    file_request,
                                                                )
                                                                .await
                                                            {
                                                                Ok(_) => {
                                                                    info!("Sent file request for {} to peer {}", metadata.file_name, selected_peer);
                                                                    state
                                                                        .analytics
                                                                        .increment_active_downloads()
                                                                        .await;
                                                                    return Ok(format!(
                                                                        "WebRTC download initiated: {} ({} bytes) from peer {}",
                                                                        metadata.file_name, metadata.file_size, selected_peer
                                                                    ));
                                                                }
                                                                Err(e) => {
                                                                    warn!("Failed to send file request: {}", e);
                                                                    last_error = format!(
                                                                        "Failed to send file request: {}",
                                                                        e
                                                                    );
                                                                }
                                                            }
                                                        }
                                                        Err(e) => {
                                                            warn!("Failed to establish WebRTC connection: {}", e);
                                                            last_error = format!(
                                                                "WebRTC connection failed: {}",
                                                                e
                                                            );
                                                        }
                                                    }
                                                }
                                                Ok(Ok(Err(e))) => {
                                                    warn!("WebRTC signaling failed: {}", e);
                                                    last_error =
                                                        format!("WebRTC signaling failed: {}", e);
                                                }
                                                Ok(Err(_)) => {
                                                    warn!("WebRTC answer receiver was canceled");
                                                    last_error =
                                                        "WebRTC answer receiver was canceled"
                                                            .to_string();
                                                }
                                                Err(_) => {
                                                    warn!(
                                                        "WebRTC answer timeout from peer {}",
                                                        selected_peer
                                                    );
                                                    last_error = format!(
                                                        "WebRTC answer timeout from peer {}",
                                                        selected_peer
                                                    );
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to send WebRTC offer: {}", e);
                                            last_error =
                                                format!("Failed to send WebRTC offer: {}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to create WebRTC offer: {}", e);
                                    last_error = format!("WebRTC setup failed: {}", e);
                                }
                            }

                            // If we get here, this attempt failed - try again with delay
                            if attempt < max_retries - 1 {
                                info!(
                                    "WebRTC attempt {} failed, retrying in 1s...",
                                    attempt + 1
                                );
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }

                        // All retries exhausted
                        Err(format!(
                            "WebRTC download failed after {} attempts: {}",
                            max_retries, last_error
                        ))
                    } else {
                        Err("WebRTC service not available".to_string())
                    }
                }
                Ok(None) => {
                    return Err("DHT search timed out - file metadata not found".to_string());
                }
                Err(e) => {
                    warn!("DHT search failed: {}", e);

                    return Err(format!("DHT search failed: {}", e));
                }
            }
        } else {
            return Err("DHT service not available".to_string());
        }
    } else {
        Err("File transfer service is not running".to_string())
    }
}

#[tauri::command]
async fn show_in_folder(path: String) -> Result<(), String> {
    let path_obj = Path::new(&path);

    #[cfg(target_os = "windows")]
    {
        // If it's a directory, just open it. If it's a file, select it.
        if path_obj.is_dir() {
            std::process::Command::new("explorer")
                .arg(&path)
                .spawn()
                .map_err(|e| format!("Failed to open folder: {}", e))?;
        } else {
            std::process::Command::new("explorer")
                .args(["/select,", &path])
                .spawn()
                .map_err(|e| format!("Failed to open folder: {}", e))?;
        }
    }

    #[cfg(target_os = "macos")]
    {
        // If it's a directory, just open it. If it's a file, reveal it.
        if path_obj.is_dir() {
            std::process::Command::new("open")
                .arg(&path)
                .spawn()
                .map_err(|e| format!("Failed to open folder: {}", e))?;
        } else {
            std::process::Command::new("open")
                .args(["-R", &path])
                .spawn()
                .map_err(|e| format!("Failed to open folder: {}", e))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, xdg-open works for both files and directories
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {}", e))?;
    }
    Ok(())
}

/// Save a file blob to a temporary file (for drag-and-drop uploads)
/// Returns the path to the temp file
#[tauri::command]
async fn save_temp_file_for_upload(
    file_name: String,
    file_data: Vec<u8>,
) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("chiral_uploads");
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp directory: {}", e))?;

    // Create unique temp file path
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();
    let temp_file_path = temp_dir.join(format!("{}_{}", timestamp, file_name));

    // Write file data
    fs::write(&temp_file_path, file_data)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    Ok(temp_file_path.to_string_lossy().to_string())
}

/// Get file size in bytes
#[tauri::command]
async fn get_file_size(file_path: String) -> Result<u64, String> {
    let metadata =
        fs::metadata(&file_path).map_err(|e| format!("Failed to get file metadata: {}", e))?;
    Ok(metadata.len())
}

#[tauri::command]
async fn create_temp_file_for_streaming(file_name: String) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("chiral_uploads");
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp directory: {}", e))?;

    // Create unique temp file path
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();
    let temp_file_path = temp_dir.join(format!("{}_{}", timestamp, file_name));

    // Create empty file
    fs::write(&temp_file_path, &[]).map_err(|e| format!("Failed to create temp file: {}", e))?;

    Ok(temp_file_path.to_string_lossy().to_string())
}

#[tauri::command]
async fn append_chunk_to_temp_file(
    temp_file_path: String,
    chunk_data: Vec<u8>,
) -> Result<(), String> {
    use tokio::fs::OpenOptions;
    use tokio::io::AsyncWriteExt;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&temp_file_path)
        .await
        .map_err(|e| format!("Failed to open temp file for appending: {}", e))?;

    file.write_all(&chunk_data)
        .await
        .map_err(|e| format!("Failed to append chunk to temp file: {}", e))?;

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn copy_file_to_temp(file_path: String) -> Result<String, String> {
    use std::path::Path;
    use tokio::fs;

    let temp_dir = std::env::temp_dir().join("chiral_uploads");
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    // Get original file name
    let file_name = Path::new(&file_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Create unique temp file path
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();
    let temp_file_path = temp_dir.join(format!("{}_{}", timestamp, file_name));

    // Copy the original file to temp location
    fs::copy(&file_path, &temp_file_path)
        .await
        .map_err(|e| format!("Failed to copy file to temp location: {}", e))?;

    Ok(temp_file_path.to_string_lossy().to_string())
}

#[tauri::command]
async fn start_streaming_upload(
    file_name: String,
    file_size: u64,
    price: f64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Check for active account - require login for all uploads
    let account = get_active_account(&state).await?;

    let dht_opt = { state.dht.lock().await.as_ref().cloned() };
    if dht_opt.is_none() {
        return Err("DHT not running".into());
    }

    // Generate a unique upload session ID
    let upload_id = format!(
        "upload_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_nanos()
    );

    // Store upload session in app state
    let mut upload_sessions = state.upload_sessions.lock().await;
    upload_sessions.insert(
        upload_id.clone(),
        StreamingUploadSession {
            file_name,
            file_size,
            received_chunks: 0,
            total_chunks: 0, // Will be set when we know chunk count
            hasher: sha2::Sha256::new(),
            created_at: std::time::SystemTime::now(),
            chunk_cids: Vec::new(),
            file_data: Vec::new(),
            price,
            is_complete: false,
        },
    );

    Ok(upload_id)
}

#[tauri::command]
async fn upload_file_chunk(
    upload_id: String,
    chunk_data: Vec<u8>,
    _chunk_index: u32,
    is_last_chunk: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut upload_sessions = state.upload_sessions.lock().await;
    let session = upload_sessions
        .get_mut(&upload_id)
        .ok_or_else(|| format!("Upload session {} not found", upload_id))?;

    // Update hasher with chunk data
    session.hasher.update(&chunk_data);
    session.received_chunks += 1;

    // Store chunk directly in Bitswap (if DHT is available)
    if let Some(dht) = state.dht.lock().await.as_ref() {
        // Create a block from the chunk data
        use dht::split_into_blocks;
        let blocks = split_into_blocks(&chunk_data, dht.chunk_size());

        for block in blocks.iter() {
            let cid = match block.cid() {
                Ok(c) => c,
                Err(e) => {
                    error!("failed to get cid for chunk block: {}", e);
                    return Err(format!("failed to get cid for chunk block: {}", e));
                }
            };

            // Collect CID for root block creation
            session.chunk_cids.push(cid.to_string());

            // Store block in Bitswap via DHT command
            if let Err(e) = dht.store_block(cid.clone(), block.data().to_vec()).await {
                error!("failed to store chunk block {}: {}", cid, e);
                return Err(format!("failed to store chunk block {}: {}", cid, e));
            }
        }
    }

    // Mark session as complete when last chunk is received
    if is_last_chunk {
        session.is_complete = true;
    }

    Ok(())
}

#[tauri::command]
async fn cancel_streaming_upload(
    upload_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut upload_sessions = state.upload_sessions.lock().await;
    upload_sessions.remove(&upload_id);
    Ok(())
}

#[tauri::command]
async fn write_file(path: String, contents: Vec<u8>) -> Result<(), String> {
    tokio::fs::write(&path, contents)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;
    Ok(())
}

/// Initialize a streaming download session - creates temp file and returns session ID
#[tauri::command]
async fn init_streaming_download(
    state: State<'_, AppState>,
    file_hash: String,
    file_name: String,
    file_size: u64,
    output_path: String,
    total_chunks: u32,
    chunk_size: u32,
) -> Result<String, String> {
    use std::time::SystemTime;

    // Generate unique session ID
    let session_id = format!(
        "dl-{}-{}",
        file_hash.chars().take(8).collect::<String>(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    // Create temp file path
    let temp_path = std::path::PathBuf::from(&output_path).with_extension("chiral_partial");

    // Pre-allocate file with zeros for efficient random writes
    let file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    file.set_len(file_size)
        .await
        .map_err(|e| format!("Failed to pre-allocate file: {}", e))?;
    drop(file);

    let session = StreamingDownloadSession {
        file_hash: file_hash.clone(),
        file_name,
        file_size,
        temp_path,
        output_path,
        received_chunks: std::collections::HashSet::new(),
        total_chunks,
        chunk_size,
        created_at: SystemTime::now(),
    };

    let mut sessions = state.download_sessions.lock().await;
    sessions.insert(session_id.clone(), session);

    info!(
        "Initialized streaming download session: {} for file {}",
        session_id, file_hash
    );
    Ok(session_id)
}

/// Write a chunk directly to the temp file at the correct offset
#[tauri::command]
async fn write_download_chunk(
    state: State<'_, AppState>,
    session_id: String,
    chunk_index: u32,
    chunk_data: Vec<u8>,
) -> Result<bool, String> {
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};

    let mut sessions = state.download_sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Download session not found: {}", session_id))?;

    // Check if chunk already received
    if session.received_chunks.contains(&chunk_index) {
        return Ok(false); // Already have this chunk
    }

    // Calculate offset
    let offset = (chunk_index as u64) * (session.chunk_size as u64);

    // Write chunk to file at correct offset
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&session.temp_path)
        .await
        .map_err(|e| format!("Failed to open temp file: {}", e))?;

    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|e| format!("Failed to seek in file: {}", e))?;

    file.write_all(&chunk_data)
        .await
        .map_err(|e| format!("Failed to write chunk: {}", e))?;

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush chunk: {}", e))?;

    session.received_chunks.insert(chunk_index);

    // Return true if all chunks received
    Ok(session.received_chunks.len() as u32 >= session.total_chunks)
}

/// Get download session progress
#[tauri::command]
async fn get_streaming_download_progress(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(u32, u32), String> {
    let sessions = state.download_sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("Download session not found: {}", session_id))?;

    Ok((session.received_chunks.len() as u32, session.total_chunks))
}

/// Finalize the download - rename temp file to final destination
#[tauri::command]
async fn finalize_streaming_download(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let mut sessions = state.download_sessions.lock().await;
    let session = sessions
        .remove(&session_id)
        .ok_or_else(|| format!("Download session not found: {}", session_id))?;

    // Verify all chunks received
    if session.received_chunks.len() as u32 != session.total_chunks {
        return Err(format!(
            "Download incomplete: received {}/{} chunks",
            session.received_chunks.len(),
            session.total_chunks
        ));
    }

    // Get unique output path to avoid overwriting existing files
    let unique_output_path = get_unique_filepath(std::path::Path::new(&session.output_path));
    let final_output_path = unique_output_path.to_string_lossy().to_string();

    // Rename temp file to final destination
    tokio::fs::rename(&session.temp_path, &final_output_path)
        .await
        .map_err(|e| format!("Failed to finalize download: {}", e))?;

    info!(
        "Finalized streaming download: {} -> {}",
        session_id, final_output_path
    );
    Ok(final_output_path)
}

/// Cancel and cleanup a streaming download
#[tauri::command]
async fn cancel_streaming_download(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut sessions = state.download_sessions.lock().await;
    if let Some(session) = sessions.remove(&session_id) {
        // Delete temp file if it exists
        let _ = tokio::fs::remove_file(&session.temp_path).await;
        // Delete checkpoint file if exists
        let checkpoint_path = session.temp_path.with_extension("checkpoint");
        let _ = tokio::fs::remove_file(&checkpoint_path).await;
        info!("Cancelled streaming download: {}", session_id);
    }
    Ok(())
}

/// Save checkpoint for resume support
#[tauri::command]
async fn save_download_checkpoint(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let sessions = state.download_sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("Download session not found: {}", session_id))?;

    let checkpoint = serde_json::json!({
        "file_hash": session.file_hash,
        "file_name": session.file_name,
        "file_size": session.file_size,
        "output_path": session.output_path,
        "total_chunks": session.total_chunks,
        "chunk_size": session.chunk_size,
        "received_chunks": session.received_chunks.iter().collect::<Vec<_>>(),
        "temp_path": session.temp_path.to_string_lossy(),
    });

    let checkpoint_path = session.temp_path.with_extension("checkpoint");
    tokio::fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&checkpoint).unwrap(),
    )
    .await
    .map_err(|e| format!("Failed to save checkpoint: {}", e))?;

    info!(
        "Saved checkpoint: {} chunks received",
        session.received_chunks.len()
    );
    Ok(())
}

/// Load checkpoint and resume download
#[tauri::command]
async fn resume_download_from_checkpoint(
    state: State<'_, AppState>,
    checkpoint_path: String,
) -> Result<(String, Vec<u32>), String> {
    let checkpoint_data = tokio::fs::read_to_string(&checkpoint_path)
        .await
        .map_err(|e| format!("Failed to read checkpoint: {}", e))?;

    let checkpoint: serde_json::Value = serde_json::from_str(&checkpoint_data)
        .map_err(|e| format!("Failed to parse checkpoint: {}", e))?;

    let file_hash = checkpoint["file_hash"].as_str().unwrap_or("").to_string();
    let file_name = checkpoint["file_name"].as_str().unwrap_or("").to_string();
    let file_size = checkpoint["file_size"].as_u64().unwrap_or(0);
    let output_path = checkpoint["output_path"].as_str().unwrap_or("").to_string();
    let total_chunks = checkpoint["total_chunks"].as_u64().unwrap_or(0) as u32;
    let chunk_size = checkpoint["chunk_size"].as_u64().unwrap_or(16384) as u32;
    let temp_path_str = checkpoint["temp_path"].as_str().unwrap_or("");
    let received_chunks: Vec<u32> = checkpoint["received_chunks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default();

    let session_id = format!(
        "dl-resume-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let temp_path = std::path::PathBuf::from(temp_path_str);

    if !temp_path.exists() {
        return Err("Temp file not found, cannot resume".to_string());
    }

    let session = StreamingDownloadSession {
        file_hash: file_hash.clone(),
        file_name,
        file_size,
        temp_path,
        output_path,
        received_chunks: received_chunks.iter().cloned().collect(),
        total_chunks,
        chunk_size,
        created_at: std::time::SystemTime::now(),
    };

    let mut sessions = state.download_sessions.lock().await;
    sessions.insert(session_id.clone(), session);

    let missing_chunks: Vec<u32> = (0..total_chunks)
        .filter(|i| !received_chunks.contains(i))
        .collect();

    info!(
        "Resumed download: {}/{} chunks missing",
        missing_chunks.len(),
        total_chunks
    );
    Ok((session_id, missing_chunks))
}

#[tauri::command]
async fn get_file_transfer_events(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().await;
        ft_guard.as_ref().cloned()
    };

    if let Some(ft) = ft {
        let events = ft.drain_events(100).await;
        let mapped: Vec<String> = events
            .into_iter()
            .map(|e| match e {
                FileTransferEvent::FileUploaded {
                    file_hash,
                    file_name,
                } => {
                    format!("file_uploaded:{}:{}", file_hash, file_name)
                }
                FileTransferEvent::FileDownloaded { file_path } => {
                    format!("file_downloaded:{}", file_path)
                }
                FileTransferEvent::FileNotFound { file_hash } => {
                    format!("file_not_found:{}", file_hash)
                }
                FileTransferEvent::Error { message } => {
                    format!("error:{}", message)
                }
                FileTransferEvent::DownloadAttempt(snapshot) => {
                    match serde_json::to_string(&snapshot) {
                        Ok(json) => format!("download_attempt:{}", json),
                        Err(_) => "download_attempt:{}".to_string(),
                    }
                }
            })
            .collect();
        Ok(mapped)
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
async fn get_download_metrics(
    state: State<'_, AppState>,
) -> Result<DownloadMetricsSnapshot, String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().await;
        ft_guard.as_ref().cloned()
    };

    if let Some(ft) = ft {
        Ok(ft.download_metrics_snapshot().await)
    } else {
        Ok(DownloadMetricsSnapshot::default())
    }
}

async fn pump_file_transfer_events(app: tauri::AppHandle, ft: Arc<FileTransferService>) {
    loop {
        let events = ft.drain_events(64).await;
        if events.is_empty() {
            if Arc::strong_count(&ft) <= 1 {
                break;
            }
            sleep(Duration::from_millis(250)).await;
            continue;
        }

        for event in events {
            match event {
                FileTransferEvent::DownloadAttempt(snapshot) => {
                    if let Err(err) = app.emit("download_attempt", &snapshot) {
                        warn!("Failed to emit download_attempt event: {}", err);
                    }
                }
                other => {
                    if let Err(err) = app.emit("file_transfer_event", format!("{:?}", other)) {
                        warn!("Failed to emit file_transfer_event: {}", err);
                    }
                }
            }
        }
    }
}

async fn pump_multi_source_events(app: tauri::AppHandle, ms: Arc<MultiSourceDownloadService>) {
    loop {
        let events = ms.drain_events(64).await;
        if events.is_empty() {
            if Arc::strong_count(&ms) <= 1 {
                break;
            }
            sleep(Duration::from_millis(250)).await;
            continue;
        }

        for event in events {
            match &event {
                MultiSourceEvent::DownloadStarted {
                    file_hash: _,
                    total_peers: _,
                } => {
                    if let Err(err) = app.emit("multi_source_download_started", &event) {
                        warn!(
                            "Failed to emit multi_source_download_started event: {}",
                            err
                        );
                    }
                }
                MultiSourceEvent::ProgressUpdate {
                    file_hash: _,
                    progress,
                } => {
                    if let Err(err) = app.emit("multi_source_progress_update", progress) {
                        warn!("Failed to emit multi_source_progress_update event: {}", err);
                    }
                }
                MultiSourceEvent::DownloadCompleted {
                    file_hash: _,
                    output_path: _,
                    duration_secs: _,
                    average_speed_bps: _,
                } => {
                    if let Err(err) = app.emit("multi_source_download_completed", &event) {
                        warn!(
                            "Failed to emit multi_source_download_completed event: {}",
                            err
                        );
                    }
                }
                _ => {
                    if let Err(err) = app.emit("multi_source_event", &event) {
                        warn!("Failed to emit multi_source_event: {}", err);
                    }
                }
            }
        }
    }
}

#[tauri::command]
async fn start_multi_source_download(
    state: State<'_, AppState>,
    file_hash: String,
    output_path: String,
    max_peers: Option<usize>,
    chunk_size: Option<usize>,
) -> Result<String, String> {
    let ms = {
        let ms_guard = state.multi_source_download.lock().await;
        ms_guard.as_ref().cloned()
    };

    if let Some(multi_source_service) = ms {
        multi_source_service
            .start_download(file_hash.clone(), output_path, max_peers, chunk_size)
            .await?;

        Ok(format!("Multi-source download started for: {}", file_hash))
    } else {
        Err("Multi-source download service not available".to_string())
    }
}

#[tauri::command]
async fn cancel_multi_source_download(
    state: State<'_, AppState>,
    file_hash: String,
) -> Result<(), String> {
    let ms = {
        let ms_guard = state.multi_source_download.lock().await;
        ms_guard.as_ref().cloned()
    };

    if let Some(multi_source_service) = ms {
        multi_source_service.cancel_download(file_hash).await
    } else {
        Err("Multi-source download service not available".to_string())
    }
}

#[tauri::command]
async fn get_multi_source_progress(
    state: State<'_, AppState>,
    file_hash: String,
) -> Result<Option<MultiSourceProgress>, String> {
    let ms = {
        let ms_guard = state.multi_source_download.lock().await;
        ms_guard.as_ref().cloned()
    };

    if let Some(multi_source_service) = ms {
        Ok(multi_source_service.get_download_progress(&file_hash).await)
    } else {
        Err("Multi-source download service not available".to_string())
    }
}

#[tauri::command]
async fn update_proxy_latency(
    state: State<'_, AppState>,
    proxy_id: String,
    latency_ms: Option<u64>,
) -> Result<(), String> {
    let ms = {
        let ms_guard = state.multi_source_download.lock().await;
        ms_guard.as_ref().cloned()
    };

    if let Some(multi_source_service) = ms {
        multi_source_service
            .update_proxy_latency(proxy_id, latency_ms)
            .await;
        Ok(())
    } else {
        Err("Multi-source download service not available for proxy latency update".to_string())
    }
}

#[tauri::command]
async fn get_proxy_optimization_status(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let ms = {
        let ms_guard = state.multi_source_download.lock().await;
        ms_guard.as_ref().cloned()
    };

    if let Some(multi_source_service) = ms {
        Ok(multi_source_service.get_proxy_optimization_status().await)
    } else {
        Err("Multi-source download service not available for proxy optimization status".to_string())
    }
}

#[tauri::command]
async fn download_file_multi_source(
    state: State<'_, AppState>,
    file_hash: String,
    output_path: String,
    prefer_multi_source: Option<bool>,
    max_peers: Option<usize>,
) -> Result<String, String> {
    let prefer_multi_source = prefer_multi_source.unwrap_or(true);

    // If multi-source is preferred and available, use it
    if prefer_multi_source {
        let ms = {
            let ms_guard = state.multi_source_download.lock().await;
            ms_guard.as_ref().cloned()
        };

        if let Some(multi_source_service) = ms {
            info!("Using multi-source download for file: {}", file_hash);
            return multi_source_service
                .start_download(file_hash.clone(), output_path, max_peers, None)
                .await
                .map(|_| format!("Multi-source download initiated for: {}", file_hash));
        }
    }

    // Fallback to original single-source download
    info!(
        "Falling back to single-source download for file: {}",
        file_hash
    );
    download_file_from_network(state, file_hash, output_path).await
}

#[tauri::command]
async fn encrypt_file_with_password(
    input_path: String,
    output_path: String,
    password: String,
) -> Result<encryption::EncryptionInfo, String> {
    use std::path::Path;

    let input = Path::new(&input_path);
    let output = Path::new(&output_path);

    if !input.exists() {
        return Err("Input file does not exist".to_string());
    }

    let result =
        encryption::FileEncryption::encrypt_file_with_password(input, output, &password).await?;

    Ok(result.encryption_info)
}

#[tauri::command]
async fn decrypt_file_with_password(
    input_path: String,
    output_path: String,
    password: String,
    encryption_info: encryption::EncryptionInfo,
) -> Result<u64, String> {
    use std::path::Path;

    let input = Path::new(&input_path);
    let output = Path::new(&output_path);

    if !input.exists() {
        return Err("Encrypted file does not exist".to_string());
    }

    encryption::FileEncryption::decrypt_file_with_password(
        input,
        output,
        &password,
        &encryption_info,
    )
    .await
}

#[tauri::command]
async fn encrypt_file_for_upload(
    input_path: String,
    password: Option<String>,
) -> Result<(String, encryption::EncryptionInfo), String> {
    use std::path::Path;

    let input = Path::new(&input_path);
    if !input.exists() {
        return Err("Input file does not exist".to_string());
    }

    // Create encrypted file in same directory with .enc extension
    let encrypted_path = input.with_extension("enc");

    let result = if let Some(pwd) = password {
        encryption::FileEncryption::encrypt_file_with_password(input, &encrypted_path, &pwd).await?
    } else {
        // Generate random key for no-password encryption
        let key = encryption::FileEncryption::generate_random_key();
        encryption::FileEncryption::encrypt_file(input, &encrypted_path, &key).await?
    };

    Ok((
        encrypted_path.to_string_lossy().to_string(),
        result.encryption_info,
    ))
}

// Update the search_file_metadata Tauri command around line 5392:
#[tauri::command]
async fn search_file_metadata(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_hash: String,
    timeout_ms: Option<u64>,
) -> Result<Option<FileMetadata>, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let timeout = timeout_ms.unwrap_or(10_000);
        let result = dht.synchronous_search_metadata(file_hash, timeout).await?;

        // If we found metadata (including from cache), emit the found_file event
        // This ensures the frontend gets notified even for cache hits
        if let Some(ref metadata) = result {
            let _ = app.emit("found_file", metadata);
        }

        Ok(result)
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn get_file_seeders(
    state: State<'_, AppState>,
    file_hash: String,
) -> Result<Vec<String>, String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht_service) = dht {
        let seeders = dht_service.get_seeders_for_file(&file_hash).await;
        Ok(seeders)
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn get_available_storage() -> f64 {
    use std::time::Duration;
    use tokio::time::timeout;

    // On Windows, use the current directory's drive, on Unix use "/"
    let path = if cfg!(windows) {
        Path::new(".")
    } else {
        Path::new("/")
    };

    // Add timeout to prevent hanging - run in a blocking task with timeout
    let result = timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            available_space(path).map(|space| space as f64 / 1024.0 / 1024.0 / 1024.0)
            // Convert to GB
        }),
    )
    .await;

    match result {
        Ok(Ok(storage_result)) => match storage_result {
            Ok(storage_gb) => {
                if storage_gb > 0.0 && storage_gb.is_finite() {
                    storage_gb.floor()
                } else {
                    warn!("Invalid storage value: {:.2}, using fallback", storage_gb);
                    100.0
                }
            }
            Err(e) => {
                warn!("Disk space check failed: {}, using fallback", e);
                100.0
            }
        },
        Ok(Err(e)) => {
            warn!("Task failed: {}, using fallback", e);
            100.0
        }
        Err(_) => {
            warn!("Failed to get available storage (timeout or error), using fallback");
            100.0
        }
    }
}

const DEFAULT_GETH_DATA_DIR: &str = "./bin/geth-data";

/// Robust disk space checking that tries multiple methods to avoid hanging
fn get_disk_space_robust(path: &std::path::Path) -> Result<f64, String> {
    use std::fs;
    use std::process::Command;

    // Method 1: Try fs2::available_space (can hang on Windows)
    match available_space(path) {
        Ok(space) => return Ok(space as f64 / 1024.0 / 1024.0 / 1024.0),
        Err(_) => {
            // Continue to other methods
        }
    }

    // Method 2: Try using system commands (Windows: wmic, Unix: df)
    #[cfg(windows)]
    {
        match Command::new("wmic")
            .args(&["logicaldisk", "where", "name='C:'", "get", "freespace"])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        let line = line.trim();
                        if let Ok(bytes) = line.parse::<u64>() {
                            return Ok(bytes as f64 / 1024.0 / 1024.0);
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }

    #[cfg(unix)]
    {
        match Command::new("df").arg(path).arg("-k").output() {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines().skip(1) {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 {
                            if let Ok(kilobytes) = parts[3].parse::<u64>() {
                                return Ok(kilobytes as f64 / 1024.0 / 1024.0);
                            }
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }

    // Method 3: Try filesystem metadata (less accurate but won't hang)
    match fs::metadata(path) {
        Ok(_) => {
            // If we can read metadata, assume we have at least some space
            // This is a fallback that won't hang
            return Ok(50.0); // Assume 50GB as safe fallback
        }
        Err(_) => {}
    }

    // Final fallback
    Err("Unable to determine available disk space".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GethStatusPayload {
    installed: bool,
    running: bool,
    binary_path: Option<String>,
    data_dir: String,
    data_dir_exists: bool,
    log_path: Option<String>,
    log_available: bool,
    log_lines: usize,
    version: Option<String>,
    last_logs: Vec<String>,
    last_updated: u64,
}

fn resolve_geth_data_dir(data_dir: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(data_dir);
    if dir.is_absolute() {
        return Ok(dir);
    }

    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("Failed to get executable path: {}", e))?
        .parent()
        .ok_or_else(|| "Failed to determine executable directory".to_string())?
        .to_path_buf();

    Ok(exe_dir.join(dir))
}

fn read_last_lines(path: &Path, max_lines: usize) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open log file: {}", e))?;
    let reader = BufReader::new(file);
    let mut buffer = VecDeque::with_capacity(max_lines);

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Failed to read log file: {}", e))?;
        if buffer.len() == max_lines {
            buffer.pop_front();
        }
        buffer.push_back(line);
    }

    Ok(buffer.into_iter().collect())
}

#[tauri::command]
async fn check_bootstrap_health() -> Result<geth_bootstrap::BootstrapHealthReport, String> {
    Ok(geth_bootstrap::check_all_bootstrap_nodes().await)
}

/// Get cached bootstrap health report without performing new checks
#[tauri::command]
async fn get_cached_bootstrap_health(
) -> Result<Option<geth_bootstrap::BootstrapHealthReport>, String> {
    Ok(geth_bootstrap::get_cached_health_report().await)
}

/// Clear the bootstrap cache to force fresh health checks
#[tauri::command]
async fn clear_bootstrap_cache() -> Result<(), String> {
    geth_bootstrap::clear_bootstrap_cache().await;
    Ok(())
}

/// Reconnect to bootstrap nodes if peer count is low
#[tauri::command]
async fn reconnect_geth_bootstrap(min_peers: Option<u32>) -> Result<u32, String> {
    let threshold = min_peers.unwrap_or(3);
    reconnect_to_bootstrap_if_needed(threshold).await
}

/// Add a specific peer to Geth
#[tauri::command]
async fn add_geth_peer(enode: String) -> Result<bool, String> {
    add_peer(&enode).await
}

/// Get current Geth peers
#[tauri::command]
async fn get_geth_peers() -> Result<Vec<serde_json::Value>, String> {
    get_peers().await
}

/// Get Geth node info
#[tauri::command]
async fn get_geth_node_info() -> Result<serde_json::Value, String> {
    get_node_info().await
}

#[tauri::command]
async fn get_geth_status(
    state: State<'_, AppState>,
    data_dir: Option<String>,
    log_lines: Option<usize>,
) -> Result<GethStatusPayload, String> {
    let requested_lines = log_lines.unwrap_or(40).clamp(1, 200);
    let data_dir_value = data_dir.unwrap_or_else(|| DEFAULT_GETH_DATA_DIR.to_string());

    let running = {
        let geth = state.geth.lock().await;
        geth.is_running()
    };

    let downloader = state.downloader.clone();
    let geth_path = downloader.geth_path();
    let installed = geth_path.exists();
    let binary_path = installed.then(|| geth_path.to_string_lossy().into_owned());

    let data_path = resolve_geth_data_dir(&data_dir_value)?;
    let data_dir_exists = data_path.exists();
    let log_path = data_path.join("geth.log");
    let log_available = log_path.exists();

    let last_logs = if log_available {
        match read_last_lines(&log_path, requested_lines) {
            Ok(lines) => lines,
            Err(err) => {
                warn!("Failed to read geth logs: {}", err);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let version = if installed {
        match Command::new(&geth_path).arg("version").output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                }
            }
            Ok(output) => {
                warn!(
                    "geth version command exited with status {:?}",
                    output.status.code()
                );
                None
            }
            Err(err) => {
                warn!("Failed to execute geth version: {}", err);
                None
            }
        }
    } else {
        None
    };

    let last_updated = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let log_path_string = if log_available {
        Some(log_path.to_string_lossy().into_owned())
    } else {
        None
    };

    Ok(GethStatusPayload {
        installed,
        running,
        binary_path,
        data_dir: data_path.to_string_lossy().into_owned(),
        data_dir_exists,
        log_path: log_path_string,
        log_available,
        log_lines: requested_lines,
        version,
        last_logs,
        last_updated,
    })
}

#[tauri::command]
async fn logout(state: State<'_, AppState>) -> Result<(), ()> {
    let mut active_account = state.active_account.lock().await;
    *active_account = None;

    // Clear private key from memory
    let mut active_key = state.active_account_private_key.lock().await;
    *active_key = None;

    // Clear private key from WebRTC service
    if let Some(webrtc_service) = state.webrtc.lock().await.as_ref() {
        webrtc_service.set_active_private_key(None).await;
    }

    Ok(())
}

async fn get_active_account(state: &State<'_, AppState>) -> Result<String, String> {
    state
        .active_account
        .lock()
        .await
        .clone()
        .ok_or_else(|| "No account is currently active. Please log in.".to_string())
}

// --- 2FA Commands ---

#[derive(serde::Serialize)]
struct TotpSetup {
    secret: String,
    otpauth_url: String,
}

#[tauri::command]
fn generate_totp_secret() -> Result<TotpSetup, String> {
    // Customize the issuer and account name.
    // The account name should ideally be the user's identifier (e.g., email or username).
    let issuer = "Chiral Network".to_string();
    let account_name = "Chiral User".to_string(); // Generic name, as it's not tied to a specific account yet

    // Generate a new secret using random bytes
    use rand::RngCore;
    let mut rng = rand::thread_rng();
    let mut secret_bytes = [0u8; 20]; // 160-bit secret (recommended for SHA1)
    rng.fill_bytes(&mut secret_bytes);
    let secret = Secret::Raw(secret_bytes.to_vec());

    // Create a TOTP object.
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,  // 6 digits
        1,  // 1 second tolerance
        30, // 30 second step
        secret.to_bytes().map_err(|e| e.to_string())?,
        Some(issuer),
        account_name,
    )
    .map_err(|e| e.to_string())?;

    let otpauth_url = totp.get_url();
    // For totp-rs v5+, use to_encoded() to get the base32 string
    let secret_string = secret.to_encoded().to_string();

    Ok(TotpSetup {
        secret: secret_string,
        otpauth_url,
    })
}

#[tauri::command]
async fn is_2fa_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    let address = get_active_account(&state).await?;
    let keystore = Keystore::load()?;
    Ok(keystore.is_2fa_enabled(&address)?)
}

#[tauri::command]
async fn verify_and_enable_totp(
    secret: String,
    code: String,
    password: String, // Password needed to encrypt the secret
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let address = get_active_account(&state).await?;

    // 1. Verify the code against the provided secret first.
    // Create a Secret enum from the base32 string, then get its raw bytes.
    let secret_bytes = Secret::Encoded(secret.clone());
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes.to_bytes().map_err(|e| e.to_string())?,
        Some("Chiral Network".to_string()),
        address.clone(),
    )
    .map_err(|e| e.to_string())?;

    if !totp.check_current(&code).unwrap_or(false) {
        return Ok(false); // Code is invalid, don't enable.
    }

    // 2. Code is valid, so save the secret to the keystore.
    let mut keystore = Keystore::load()?;
    keystore.set_2fa_secret(&address, &secret, &password)?;

    Ok(true)
}

#[tauri::command]
async fn verify_totp_code(
    code: String,
    password: String, // Password needed to decrypt the secret
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let address = get_active_account(&state).await?;
    let keystore = Keystore::load()?;

    // 1. Retrieve the secret from the keystore.
    let secret_b32 = keystore
        .get_2fa_secret(&address, &password)?
        .ok_or_else(|| "2FA is not enabled for this account.".to_string())?;

    // 2. Verify the provided code against the stored secret.
    // Create a Secret enum from the base32 string, then get its raw bytes.
    let secret_bytes = Secret::Encoded(secret_b32);
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes.to_bytes().map_err(|e| e.to_string())?,
        Some("Chiral Network".to_string()),
        address.clone(),
    )
    .map_err(|e| e.to_string())?;

    Ok(totp.check_current(&code).unwrap_or(false))
}

#[tauri::command]
async fn disable_2fa(password: String, state: State<'_, AppState>) -> Result<(), String> {
    let address = get_active_account(&state).await?;
    let mut keystore = Keystore::load()?;
    keystore.remove_2fa_secret(&address, &password)?;
    Ok(())
}

// Peer Selection Commands

#[tauri::command]
async fn get_recommended_peers_for_file(
    state: State<'_, AppState>,
    file_hash: String,
    file_size: u64,
    require_encryption: bool,
) -> Result<Vec<String>, String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        Ok(dht
            .get_recommended_peers_for_download(&file_hash, file_size, require_encryption)
            .await)
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn record_transfer_success(
    state: State<'_, AppState>,
    peer_id: String,
    bytes: u64,
    duration_ms: u64,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        dht.record_transfer_success(&peer_id, bytes, duration_ms)
            .await;
        Ok(())
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn record_transfer_failure(
    state: State<'_, AppState>,
    peer_id: String,
    error: String,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        dht.record_transfer_failure(&peer_id, &error).await;
        Ok(())
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn get_peer_metrics(
    state: State<'_, AppState>,
) -> Result<Vec<peer_selection::PeerMetrics>, String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        Ok(dht.get_peer_metrics().await)
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn get_connected_peer_metrics(
    state: State<'_, AppState>,
) -> Result<Vec<peer_selection::PeerMetrics>, String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        Ok(dht.get_connected_peer_metrics().await)
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn report_malicious_peer(
    peer_id: String,
    severity: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        dht.report_malicious_peer(&peer_id, &severity).await;
        Ok(())
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn select_peers_with_strategy(
    state: State<'_, AppState>,
    available_peers: Vec<String>,
    count: usize,
    strategy: String,
    require_encryption: bool,
    blacklisted_peers: Vec<String>,
) -> Result<Vec<String>, String> {
    use peer_selection::SelectionStrategy;

    let selection_strategy = match strategy.as_str() {
        "fastest" => SelectionStrategy::FastestFirst,
        "reliable" => SelectionStrategy::MostReliable,
        "bandwidth" => SelectionStrategy::HighestBandwidth,
        "balanced" => SelectionStrategy::Balanced,
        "encryption" => SelectionStrategy::EncryptionPreferred,
        "load_balanced" => SelectionStrategy::LoadBalanced,
        _ => SelectionStrategy::Balanced,
    };

    let filtered_peers: Vec<String> = available_peers
        .into_iter()
        .filter(|peer| !blacklisted_peers.contains(peer))
        .collect();

    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        Ok(dht
            .select_peers_with_strategy(
                &filtered_peers,
                count,
                selection_strategy,
                require_encryption,
            )
            .await)
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn set_peer_encryption_support(
    state: State<'_, AppState>,
    peer_id: String,
    supported: bool,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        dht.set_peer_encryption_support(&peer_id, supported).await;
        Ok(())
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn cleanup_inactive_peers(
    state: State<'_, AppState>,
    max_age_seconds: u64,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        dht.cleanup_inactive_peers(max_age_seconds).await;
        Ok(())
    } else {
        Err("DHT service not available".to_string())
    }
}

#[tauri::command]
async fn send_chiral_transaction(
    state: State<'_, AppState>,
    to_address: String,
    amount: f64,
) -> Result<String, String> {
    // Get the active account address
    let account = get_active_account(&state).await?;

    // Get the private key from state
    let private_key = {
        let key_guard = state.active_account_private_key.lock().await;
        key_guard
            .clone()
            .ok_or("No private key available. Please log in again.")?
    };

    let tx_hash = ethereum::send_transaction(&account, &to_address, amount, &private_key).await?;

    Ok(tx_hash)
}

#[tauri::command]
async fn queue_transaction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    to_address: String,
    amount: f64,
) -> Result<String, String> {
    // Validate account is logged in
    let account = get_active_account(&state).await?;

    // Generate unique transaction ID
    let tx_id = format!(
        "tx_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis()
    );

    // Create queued transaction
    let queued_tx = QueuedTransaction {
        id: tx_id.clone(),
        to_address,
        amount,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs(),
    };

    // Add to queue
    {
        let mut queue = state.transaction_queue.lock().await;
        queue.push_back(queued_tx);
    }

    // Start processor if not running
    {
        let mut processor_guard = state.transaction_processor.lock().await;
        if processor_guard.is_none() {
            let app_handle = app.clone();
            let queue_arc = state.transaction_queue.clone();
            let processing_arc = state.processing_transaction.clone();

            // Clone the Arc references we need instead of borrowing state
            let active_account_arc = state.active_account.clone();
            let active_key_arc = state.active_account_private_key.clone();

            let handle = tokio::spawn(async move {
                process_transaction_queue(
                    app_handle,
                    queue_arc,
                    processing_arc,
                    active_account_arc,
                    active_key_arc,
                )
                .await;
            });

            *processor_guard = Some(handle);
        }
    }

    Ok(tx_id)
}

async fn process_transaction_queue(
    app: tauri::AppHandle,
    queue: Arc<Mutex<VecDeque<QueuedTransaction>>>,
    processing: Arc<Mutex<bool>>,
    active_account: Arc<Mutex<Option<String>>>,
    active_private_key: Arc<Mutex<Option<String>>>,
) {
    loop {
        // Check if already processing
        {
            let is_processing = processing.lock().await;
            if *is_processing {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        }

        // Get next transaction from queue
        let next_tx = {
            let mut queue_guard = queue.lock().await;
            queue_guard.pop_front()
        };

        if let Some(tx) = next_tx {
            // Mark as processing
            {
                let mut is_processing = processing.lock().await;
                *is_processing = true;
            }

            // Emit queue status
            let _ = app.emit("transaction_queue_processing", &tx.id);

            // Get account and private key from the Arc references
            let account_opt = {
                let account_guard = active_account.lock().await;
                account_guard.clone()
            };

            let private_key_opt = {
                let key_guard = active_private_key.lock().await;
                key_guard.clone()
            };

            match (account_opt, private_key_opt) {
                (Some(account), Some(private_key)) => {
                    // Process transaction
                    match ethereum::send_transaction(
                        &account,
                        &tx.to_address,
                        tx.amount,
                        &private_key,
                    )
                    .await
                    {
                        Ok(tx_hash) => {
                            // Success - emit event
                            let _ = app.emit(
                                "transaction_sent",
                                serde_json::json!({
                                    "id": tx.id,
                                    "txHash": tx_hash,
                                    "to": tx.to_address,
                                    "amount": tx.amount,
                                }),
                            );

                            // Wait a bit before processing next (to ensure nonce increments)
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                        Err(e) => {
                            // Error - emit event
                            warn!("Transaction failed: {}", e);
                            let _ = app.emit(
                                "transaction_failed",
                                serde_json::json!({
                                    "id": tx.id,
                                    "error": e,
                                    "to": tx.to_address,
                                    "amount": tx.amount,
                                }),
                            );
                        }
                    }
                }
                _ => {
                    // No account or private key - user logged out
                    warn!("Cannot process transaction - user logged out");
                    let _ = app.emit(
                        "transaction_failed",
                        serde_json::json!({
                            "id": tx.id,
                            "error": "User logged out",
                            "to": tx.to_address,
                            "amount": tx.amount,
                        }),
                    );
                }
            }

            // Mark as not processing
            {
                let mut is_processing = processing.lock().await;
                *is_processing = false;
            }
        } else {
            // Queue is empty, sleep
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

#[tauri::command]
async fn get_transaction_queue_status(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let queue = state.transaction_queue.lock().await;
    let processing = state.processing_transaction.lock().await;

    Ok(serde_json::json!({
        "queueLength": queue.len(),
        "isProcessing": *processing,
        "transactions": queue.iter().map(|tx| serde_json::json!({
            "id": tx.id,
            "to": tx.to_address,
            "amount": tx.amount,
            "timestamp": tx.timestamp,
        })).collect::<Vec<_>>(),
    }))
}

// Analytics commands
#[tauri::command]
async fn get_bandwidth_stats(
    state: State<'_, AppState>,
) -> Result<analytics::BandwidthStats, String> {
    Ok(state.analytics.get_bandwidth_stats().await)
}

#[tauri::command]
async fn get_bandwidth_history(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<analytics::BandwidthDataPoint>, String> {
    Ok(state.analytics.get_bandwidth_history(limit).await)
}

#[tauri::command]
async fn get_performance_metrics(
    state: State<'_, AppState>,
) -> Result<analytics::PerformanceMetrics, String> {
    Ok(state.analytics.get_performance_metrics().await)
}

#[tauri::command]
async fn get_network_activity(
    state: State<'_, AppState>,
) -> Result<analytics::NetworkActivity, String> {
    Ok(state.analytics.get_network_activity().await)
}

#[tauri::command]
async fn get_resource_contribution(
    state: State<'_, AppState>,
) -> Result<analytics::ResourceContribution, String> {
    Ok(state.analytics.get_resource_contribution().await)
}

#[tauri::command]
async fn get_contribution_history(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<analytics::ContributionDataPoint>, String> {
    Ok(state.analytics.get_contribution_history(limit).await)
}

#[tauri::command]
async fn reset_analytics(state: State<'_, AppState>) -> Result<(), String> {
    state.analytics.reset_stats().await;
    Ok(())
}

// Logger configuration commands
/// Saves application settings to a JSON file in the app data directory
#[tauri::command]
async fn save_app_settings(app: tauri::AppHandle, settings_json: String) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    // Ensure the directory exists
    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("Failed to create app data directory: {}", e))?;

    let settings_file = app_data_dir.join("settings.json");

    std::fs::write(&settings_file, settings_json)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;

    info!("Settings saved to: {}", settings_file.display());
    Ok(())
}

/// Updates the file logger configuration at runtime.
/// This allows enabling/disabling file logging and changing log rotation settings
/// without restarting the application.
///
/// All existing `info!()`, `debug!()`, `error!()` etc. calls throughout the codebase
/// will automatically be captured and written to the log files when enabled.
///
/// Logs are always written to the AppData directory, not the user's storage directory.
///
/// Note: The tracing subscriber is initialized at startup, so changes to enable/disable
/// logging will only affect whether logs are written to disk. Console logging remains active.
#[tauri::command]
async fn update_log_config(
    app: tauri::AppHandle,
    max_log_size_mb: u64,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Get the app data directory (not the user's storage directory)
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let logs_dir = app_data_dir.join("logs");
    let config = logger::LogConfig::new(&logs_dir, max_log_size_mb, enabled);

    let logger_lock = state.file_logger.lock().await;
    if let Some(ref writer) = *logger_lock {
        writer.update_config(config).map_err(|e| e.to_string())?;

        if enabled {
            info!(
                "File logging enabled: {} (max size: {} MB)",
                logs_dir.display(),
                max_log_size_mb
            );
            // Force a write to create the log file if it doesn't exist
            info!("Logger configuration updated");
        } else {
            info!("File logging disabled");
        }
    } else {
        return Err("File logger not initialized. Please restart the application.".to_string());
    }

    Ok(())
}

/// Get the directory where logs are stored
#[tauri::command]
fn get_logs_directory(app: tauri::AppHandle) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let logs_dir = app_data_dir.join("logs");
    Ok(logs_dir.to_string_lossy().to_string())
}
#[tauri::command]
async fn reset_network_services(state: State<'_, AppState>) -> Result<(), String> {
    // Stop DHT if running
    if let Some(dht) = state.dht.lock().await.as_ref() {
        let _ = dht.shutdown().await;
    }
    *state.dht.lock().await = None;

    // Stop WebRTC if running (just clear the reference)
    *state.webrtc.lock().await = None;

    // Stop file transfer service (just clear the reference)
    *state.file_transfer.lock().await = None;

    // Stop multi-source download service (just clear the reference)
    *state.multi_source_download.lock().await = None;

    // Stop any running pumps
    *state.file_transfer_pump.lock().await = None;
    *state.multi_source_pump.lock().await = None;
    Ok(())
}

// ============================================================================
// HTTP Server Commands - Serve files via HTTP protocol
// ============================================================================

/// Start HTTP server for serving encrypted chunks and file manifests
///
/// The server will listen on the specified port and serve files that have been
/// registered via `register_file()`.
///
/// Returns the actual bound address (useful if port 0 was used for auto-assignment)
#[tauri::command]
async fn start_http_server(state: State<'_, AppState>, port: u16) -> Result<String, String> {
    // Check if server is already running
    {
        let addr_lock = state.http_server_addr.lock().await;
        if addr_lock.is_some() {
            return Err("HTTP server is already running".to_string());
        }
    }

    let bind_addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();

    tracing::info!("Starting HTTP server on {}", bind_addr);

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    // Start the server with shutdown signal
    let server_state = state.http_server_state.clone();
    let bound_addr = http_server::start_server(server_state, bind_addr, shutdown_rx)
        .await
        .map_err(|e| format!("Failed to start HTTP server: {}", e))?;

    // Store the bound address and shutdown sender
    {
        let mut addr_lock = state.http_server_addr.lock().await;
        *addr_lock = Some(bound_addr);
    }
    {
        let mut shutdown_lock = state.http_server_shutdown.lock().await;
        *shutdown_lock = Some(shutdown_tx);
    }

    Ok(format!("http://{}", bound_addr))
}

/// Stop HTTP server
#[tauri::command]
async fn stop_http_server(state: State<'_, AppState>) -> Result<(), String> {
    let addr = {
        let mut addr_lock = state.http_server_addr.lock().await;
        if addr_lock.is_none() {
            return Err("HTTP server is not running".to_string());
        }
        addr_lock.take()
    };

    tracing::info!("Stopping HTTP server at {:?}", addr);

    // Send shutdown signal
    let shutdown_tx = {
        let mut shutdown_lock = state.http_server_shutdown.lock().await;
        shutdown_lock.take()
    };

    if let Some(tx) = shutdown_tx {
        // Send shutdown signal (ignore error if receiver already dropped)
        let _ = tx.send(());
        tracing::info!("Sent graceful shutdown signal to HTTP server");
    } else {
        tracing::warn!("No shutdown channel found, server may not shut down gracefully");
    }

    Ok(())
}

/// Get HTTP server status
#[tauri::command]
async fn get_http_server_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let addr_lock = state.http_server_addr.lock().await;

    match &*addr_lock {
        Some(addr) => Ok(serde_json::json!({
            "running": true,
            "address": format!("http://{}", addr)
        })),
        None => Ok(serde_json::json!({
            "running": false,
            "address": null
        })),
    }
}

/// Download a file via HTTP protocol using Range requests
///
/// This uses HTTP Range headers (RFC 7233) to download file chunks in parallel,
/// without requiring pre-chunking or manifest endpoints.
///
/// Flow:
/// 1. Fetch file metadata from HTTP server
/// 2. Calculate byte ranges (256KB chunks)
/// 3. Download chunks in parallel using Range headers
/// 4. Reassemble chunks into final file
///
/// Files are downloaded as-is (encrypted if they were encrypted).
/// Decryption happens at a higher level when needed.
///
/// Emits `http_download_progress` events with progress updates.
#[tauri::command]
async fn download_file_http(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    seeder_url: String,
    merkle_root: String,
    output_path: String,
    peer_id: Option<String>,
) -> Result<(), String> {
    tracing::info!(
        "Starting HTTP Range-based download: {} from {}",
        merkle_root,
        seeder_url
    );

    tracing::info!("Output path: {}", output_path);

    // Get our local peer ID to send to provider
    let downloader_peer_id = if let Some(dht) = state.dht.lock().await.as_ref() {
        Some(dht.get_peer_id().await)
    } else {
        None
    };

    if let Some(ref local_id) = downloader_peer_id {
        tracing::info!("📤 Downloader peer ID: {}", local_id);
    }

    // Create progress channel
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::channel::<http_download::HttpDownloadProgress>(100);

    // Spawn progress event emitter
    let app_handle = app.clone();
    let emit_task = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            tracing::info!(
                "HTTP download progress: {}/{} chunks, {}/{} bytes, status: {:?}",
                progress.chunks_downloaded,
                progress.chunks_total,
                progress.bytes_downloaded,
                progress.bytes_total,
                progress.status
            );
            let _ = app_handle.emit("http_download_progress", &progress);
        }
    });

    // Create HTTP download client with downloader peer ID
    let client = http_download::HttpDownloadClient::new_with_peer_id(downloader_peer_id);

    let start_time = std::time::Instant::now();

    // Start download using Range requests
    let result = client
        .download_file(
            &seeder_url,
            &merkle_root,
            std::path::Path::new(&output_path),
            Some(progress_tx),
        )
        .await;

    // Wait for progress emitter to finish
    drop(emit_task);

    match result {
        Ok(()) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            // Get file size
            let file_size = tokio::fs::metadata(&output_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);

            tracing::info!(
                "HTTP download completed successfully: {} ({} bytes in {} ms)",
                output_path,
                file_size,
                duration_ms
            );

            // Record successful transfer metrics if peer_id provided
            if let Some(ref peer_id_str) = peer_id {
                if let Some(dht) = state.dht.lock().await.as_ref() {
                    dht.record_transfer_success(peer_id_str, file_size, duration_ms)
                        .await;
                    tracing::info!("📊 Recorded successful transfer for peer: {}", peer_id_str);
                }
            }

            Ok(())
        }
        Err(e) => {
            tracing::error!("HTTP download failed: {}", e);

            // Record failed transfer metrics if peer_id provided
            if let Some(ref peer_id_str) = peer_id {
                if let Some(dht) = state.dht.lock().await.as_ref() {
                    dht.record_transfer_failure(peer_id_str, "http_download_error")
                        .await;
                    tracing::info!("📊 Recorded failed transfer for peer: {}", peer_id_str);
                }
            }

            Err(e)
        }
    }
}

// Protocol-specific download commands

#[tauri::command]
async fn download_ed2k(link: String, state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("Starting ED2K download: {}", link);

    // Use the protocol manager for ED2K downloads
    use crate::protocols::traits::DownloadOptions;
    let options = DownloadOptions {
        output_path: std::path::PathBuf::from("./downloads"),
        max_peers: Some(5),
        chunk_size: Some(1024 * 1024), // 1MB chunks
        ..Default::default()
    };

    state
        .protocol_manager
        .download(&link, options)
        .await
        .map_err(|e| format!("ED2K download failed: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn download_ftp(
    url: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use chiral_network::transfer_events::{
        current_timestamp_ms, SourceInfo, SourceType, TransferPriority,
        TransferQueuedEvent, TransferStartedEvent, TransferEvent,
        TransferFailedEvent, TransferCompletedEvent, SourceConnectedEvent,
        SourceSummary, ErrorCategory,
    };
    use chiral_network::ftp_downloader::FtpDownloader;
    use tauri::Emitter;
    
    tracing::info!("Starting FTP download: {}", url);

    // Validate FTP URL
    if !url.starts_with("ftp://") {
        return Err(format!("Invalid FTP URL scheme: {}", url));
    }

    // Parse URL to extract file info
    let parsed_url = url::Url::parse(&url)
        .map_err(|e| format!("Invalid FTP URL: {}", e))?;
    
    // Extract filename from URL and strip hash prefix if present
    // FTP uploads store files as "{hash}_{originalname}" for uniqueness
    let raw_file_name = parsed_url
        .path_segments()
        .and_then(|segments| segments.last())
        .map(|s| urlencoding::decode(s).unwrap_or_else(|_| s.into()).to_string())
        .unwrap_or_else(|| "unknown_file".to_string());
    
    // Strip the hash prefix (format: {64-char-hash}_{original_filename})
    let file_name = if raw_file_name.len() > 65 && raw_file_name.chars().nth(64) == Some('_') {
        // Check if first 64 chars look like a hex hash
        let potential_hash = &raw_file_name[..64];
        if potential_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            raw_file_name[65..].to_string() // Skip hash and underscore
        } else {
            raw_file_name
        }
    } else {
        raw_file_name
    };
    
    let host = parsed_url.host_str().unwrap_or("unknown").to_string();
    
    // Generate transfer ID
    let transfer_id = format!("ftp-{:x}", {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        hasher.finish()
    });
    
    let started_at = current_timestamp_ms();
    let source_id = format!("ftp-{}", host);
    
    // Use the same download directory as specified in settings
    let download_dir = get_download_directory(app_handle.clone())
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Fallback to default if settings can't be loaded
            directories::ProjectDirs::from("com", "chiral-network", "chiral-network")
                .map(|dirs| dirs.data_dir().join("downloads"))
                .unwrap_or_else(|| std::env::current_dir().unwrap().join("downloads"))
        });
    
    // Ensure download directory exists
    if let Err(e) = std::fs::create_dir_all(&download_dir) {
        return Err(format!("Failed to create download directory: {}", e));
    }
    
    let output_path = download_dir.join(&file_name);
    
    // Emit queued event via transfer:event channel
    let queued_event = TransferQueuedEvent {
        transfer_id: transfer_id.clone(),
        file_hash: transfer_id.clone(),
        file_name: file_name.clone(),
        file_size: 0, // Unknown until connected
        output_path: output_path.to_string_lossy().to_string(),
        priority: TransferPriority::Normal,
        queued_at: started_at,
        queue_position: 0,
        estimated_sources: 1,
    };
    let _ = app_handle.emit("transfer:event", &TransferEvent::Queued(queued_event));
    
    // Create source info for events
    let source_info = SourceInfo {
        id: source_id.clone(),
        source_type: SourceType::Ftp,
        address: host.clone(),
        reputation: None,
        estimated_speed_bps: None,
        latency_ms: None,
        location: None,
    };
    
    // Emit started event
    let started_event = TransferStartedEvent {
        transfer_id: transfer_id.clone(),
        file_hash: transfer_id.clone(),
        file_name: file_name.clone(),
        file_size: 0,
        total_chunks: 1,
        chunk_size: 0,
        started_at,
        available_sources: vec![source_info.clone()],
        selected_sources: vec![source_id.clone()],
    };
    let _ = app_handle.emit("transfer:event", &TransferEvent::Started(started_event));

    // Clone values for the spawned task
    let transfer_id_clone = transfer_id.clone();
    let file_name_clone = file_name.clone();
    let source_id_clone = source_id.clone();
    let source_info_clone = source_info.clone();
    let parsed_url_clone = parsed_url.clone();
    // URL-decode the path to handle spaces and special characters
    let remote_path = urlencoding::decode(parsed_url.path())
        .unwrap_or_else(|_| parsed_url.path().into())
        .to_string();
    
    // Spawn download in background task so we return immediately
    tokio::spawn(async move {
        let downloader = FtpDownloader::new();
        let download_start = std::time::Instant::now();
        
        // Connect to FTP server
        let mut stream = match downloader.connect_and_login(&parsed_url_clone, None).await {
            Ok(s) => {
                // Emit source connected event
                let connected_event = SourceConnectedEvent {
                    transfer_id: transfer_id_clone.clone(),
                    source_id: source_id_clone.clone(),
                    source_type: SourceType::Ftp,
                    source_info: source_info_clone.clone(),
                    connected_at: current_timestamp_ms(),
                    assigned_chunks: vec![0],
                };
                let _ = app_handle.emit("transfer:event", &TransferEvent::SourceConnected(connected_event));
                s
            }
            Err(e) => {
                let failed_event = TransferFailedEvent {
                    transfer_id: transfer_id_clone.clone(),
                    file_hash: transfer_id_clone.clone(),
                    failed_at: current_timestamp_ms(),
                    error: format!("FTP connection failed: {}", e),
                    error_category: ErrorCategory::Network,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    retry_possible: true,
                };
                let _ = app_handle.emit("transfer:event", &TransferEvent::Failed(failed_event));
                tracing::error!("FTP connection failed: {}", e);
                return;
            }
        };
        
        // Get file size
        let file_size = match downloader.get_file_size(&mut stream, &remote_path).await {
            Ok(size) => size,
            Err(e) => {
                tracing::warn!("Could not get file size: {}", e);
                0
            }
        };
        
        // Download the file
        match downloader.download_full(&mut stream, &remote_path).await {
            Ok(data) => {
                let download_duration = download_start.elapsed();
                let duration_secs = download_duration.as_secs_f64();
                let speed_bps = if duration_secs > 0.0 {
                    data.len() as f64 / duration_secs
                } else {
                    0.0
                };
                
                // Save to disk
                if let Err(e) = std::fs::write(&output_path, &data) {
                    let failed_event = TransferFailedEvent {
                        transfer_id: transfer_id_clone.clone(),
                        file_hash: transfer_id_clone.clone(),
                        failed_at: current_timestamp_ms(),
                        error: format!("Failed to save file: {}", e),
                        error_category: ErrorCategory::Filesystem,
                        downloaded_bytes: data.len() as u64,
                        total_bytes: file_size,
                        retry_possible: true,
                    };
                    let _ = app_handle.emit("transfer:event", &TransferEvent::Failed(failed_event));
                    tracing::error!("Failed to save FTP download: {}", e);
                    return;
                }
                
                // Emit completed event
                let completed_event = TransferCompletedEvent {
                    transfer_id: transfer_id_clone.clone(),
                    file_hash: transfer_id_clone.clone(),
                    file_name: file_name_clone.clone(),
                    file_size: data.len() as u64,
                    output_path: output_path.to_string_lossy().to_string(),
                    completed_at: current_timestamp_ms(),
                    duration_seconds: duration_secs as u64,
                    average_speed_bps: speed_bps,
                    total_chunks: 1,
                    sources_used: vec![SourceSummary {
                        source_id: source_id_clone.clone(),
                        source_type: SourceType::Ftp,
                        chunks_provided: 1,
                        bytes_provided: data.len() as u64,
                        average_speed_bps: speed_bps,
                        connection_duration_seconds: duration_secs as u64,
                    }],
                };
                let _ = app_handle.emit("transfer:event", &TransferEvent::Completed(completed_event));
                tracing::info!("FTP download completed: {} ({} bytes in {:.2}s)", 
                    file_name_clone, data.len(), duration_secs);
            }
            Err(e) => {
                let failed_event = TransferFailedEvent {
                    transfer_id: transfer_id_clone.clone(),
                    file_hash: transfer_id_clone.clone(),
                    failed_at: current_timestamp_ms(),
                    error: format!("FTP download failed: {}", e),
                    error_category: ErrorCategory::Network,
                    downloaded_bytes: 0,
                    total_bytes: file_size,
                    retry_possible: true,
                };
                let _ = app_handle.emit("transfer:event", &TransferEvent::Failed(failed_event));
                tracing::error!("FTP download failed: {}", e);
            }
        }
    });

    Ok(())
}

// Download restart Tauri commands

#[tauri::command]
async fn start_download_restart(
    request: download_restart::StartDownloadRequest,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let dr_guard = state.download_restart.lock().await;
    if let Some(ref service) = *dr_guard {
        service
            .start_download(request)
            .await
            .map_err(|e| e.to_string())
    } else {
        Err("Download restart service not initialized".to_string())
    }
}

#[tauri::command]
async fn pause_download_restart(
    download_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let dr_guard = state.download_restart.lock().await;
    if let Some(ref service) = *dr_guard {
        service
            .pause_download(&download_id)
            .await
            .map_err(|e| e.to_string())
    } else {
        Err("Download restart service not initialized".to_string())
    }
}

#[tauri::command]
async fn resume_download_restart(
    download_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let dr_guard = state.download_restart.lock().await;
    if let Some(ref service) = *dr_guard {
        service
            .resume_download(&download_id)
            .await
            .map_err(|e| e.to_string())
    } else {
        Err("Download restart service not initialized".to_string())
    }
}

#[tauri::command]
async fn get_download_status_restart(
    download_id: String,
    state: State<'_, AppState>,
) -> Result<download_restart::DownloadStatus, String> {
    let dr_guard = state.download_restart.lock().await;
    if let Some(ref service) = *dr_guard {
        service
            .get_status(&download_id)
            .await
            .map_err(|e| e.to_string())
    } else {
        Err("Download restart service not initialized".to_string())
    }
}

// #[cfg(not(test))]
fn main() {
    // Don't initialize tracing subscriber here - we'll do it in setup() after loading settings
    // so we can configure file logging properly

    // Parse command line arguments
    use clap::Parser;
    let args = headless::CliArgs::parse();

    // For headless mode, initialize basic console logging
    if args.headless {
        use tracing_subscriber::{fmt, prelude::*, EnvFilter};
        let mut filter = EnvFilter::from_default_env();

        // Add directives with safe fallback
        if let Ok(directive) = "chiral_network=info".parse() {
            filter = filter.add_directive(directive);
        }
        if let Ok(directive) = "libp2p=warn".parse() {
            filter = filter.add_directive(directive);
        }
        if let Ok(directive) = "libp2p_kad=warn".parse() {
            filter = filter.add_directive(directive);
        }
        if let Ok(directive) = "libp2p_swarm=warn".parse() {
            filter = filter.add_directive(directive);
        }
        if let Ok(directive) = "libp2p_mdns=warn".parse() {
            filter = filter.add_directive(directive);
        }

        tracing_subscriber::registry()
            .with(fmt::layer())
            .with(filter)
            .init();

        println!("Running in headless mode...");

        // Create a tokio runtime for async operations
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        // Run the headless mode
        if let Err(e) = runtime.block_on(headless::run_headless(args)) {
            eprintln!("Error in headless mode: {}", e);
            std::process::exit(1);
        }
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    // --- Single Instance Check ---
    // Prevent multiple instances from running by checking if the DHT port is already in use
    let single_instance_port = 4001;
    if std::net::TcpListener::bind(format!("127.0.0.1:{}", single_instance_port)).is_err() {
        eprintln!("Error: Another instance of Chiral Network is already running.");
        eprintln!("Please close the existing instance before starting a new one.");
        std::process::exit(1);
    }

    // --- Initialize DHT Service at startup ---
    let instance_suffix = String::new();
    let instance_suffix_clone = instance_suffix.clone();
    let dht_service_arc = runtime.block_on(async move {
        // These settings can be moved to a config file later
        let bootstrap_nodes = get_bootstrap_nodes();
        let port = 4001; // Default DHT port
        let is_bootstrap = false;
        let enable_autonat = true;
        let enable_autorelay = true;

        let proj_dirs = ProjectDirs::from("com", "chiral-network", "chiral-network")
            .ok_or("Failed to get project directories")
            .unwrap();
        let blockstore_db_path = proj_dirs.data_dir().join("blockstore_db");
        let async_blockstore_path = async_std::path::Path::new(blockstore_db_path.as_os_str());

        let dht_service = DhtService::new(
            port,
            bootstrap_nodes,
            None, // secret
            is_bootstrap,
            enable_autonat,
            Some(Duration::from_secs(30)), // autonat_probe_interval
            Vec::new(),                    // autonat_servers
            None,                          // proxy_address
            None,                          // file_transfer_service
            None,                          // chunk_manager
            None,                          // chunk_size_kb
            None,                          // cache_size_mb
            enable_autorelay,
            Vec::new(),   // preferred_relays
            is_bootstrap, // enable_relay_server
            true,         // enable_upnp
            Some(&async_blockstore_path),
            None,
            None,
        )
        .await
        .expect("Failed to create DHT service at startup");

        Arc::new(dht_service)
    });

    // Store DHT service and related data for later use in setup()
    let dht_service_for_bt = dht_service_arc.clone();

    let (bittorrent_handler_arc, protocol_manager_arc) = runtime.block_on(async move {
        // Use the instance_id and instance_suffix from above for BitTorrent paths
        let download_dir =
            directories::ProjectDirs::from("com", "chiral-network", "chiral-network")
                .map(|dirs| {
                    dirs.data_dir()
                        .join(format!("downloads{}", instance_suffix))
                })
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .unwrap()
                        .join(format!("downloads{}", instance_suffix))
                });

        if let Err(e) = std::fs::create_dir_all(&download_dir) {
            eprintln!("Failed to create download directory: {}", e);
        }

        // Use default BitTorrent port range
        let port_range = 6881..6891;

        println!(
            "Using BitTorrent port range: {}-{}",
            port_range.start, port_range.end
        );

        // Pass the initialized DHT service to the BitTorrent handler
        let bittorrent_handler = bittorrent_handler::BitTorrentHandler::new_with_port_range(
            download_dir.clone(),
            dht_service_for_bt,
            Some(port_range),
        )
        .await
        .expect("Failed to create BitTorrent handler");
        let bittorrent_handler_arc = Arc::new(bittorrent_handler);

        let mut manager = ProtocolManager::new();

        // Wrap the simple handler in the enhanced protocol handler
        let bittorrent_protocol_handler =
            BitTorrentProtocolHandler::new(bittorrent_handler_arc.clone());
        manager.register(Box::new(bittorrent_protocol_handler));

        // Register ED2K and FTP handlers
        let ed2k_handler = protocols::ed2k::Ed2kProtocolHandler::new("ed2k://|server|45.82.80.155|5687|/".to_string());
        manager.register(Box::new(ed2k_handler));

        let ftp_handler = protocols::ftp::FtpProtocolHandler::new();
        manager.register(Box::new(ftp_handler));

        (bittorrent_handler_arc, Arc::new(manager))
    });

    // Reputation system Tauri commands
    #[tauri::command]
    async fn publish_reputation_verdict(
        verdict: reputation::TransactionVerdict,
        state: State<'_, AppState>,
    ) -> Result<(), String> {
        println!("📊 RUST: publish_reputation_verdict called");
        tracing::info!(
            "📊 publish_reputation_verdict: {} -> {} ({:?})",
            verdict.issuer_id,
            verdict.target_id,
            verdict.outcome
        );

        // Get DHT service from AppState
        let dht_guard = state.dht.lock().await;
        let dht = dht_guard
            .as_ref()
            .ok_or_else(|| "DHT service not initialized".to_string())?;

        // Create ReputationDhtService and store verdict
        let mut reputation_dht = reputation::ReputationDhtService::new();
        reputation_dht.set_dht_service(Arc::clone(dht));
        println!("📊 RUST: About to store verdict");
        reputation_dht.store_transaction_verdict(&verdict).await?;

        println!("✅ RUST: Verdict stored successfully");
        tracing::info!(
            "✅ Published verdict to DHT for peer: {}",
            verdict.target_id
        );
        Ok(())
    }

    #[tauri::command]
    async fn get_reputation_verdicts(
        peer_id: String,
        state: State<'_, AppState>,
    ) -> Result<Vec<reputation::TransactionVerdict>, String> {
        println!("🔍 RUST: get_reputation_verdicts called for: {}", peer_id);
        tracing::info!("📊 get_reputation_verdicts for peer: {}", peer_id);

        // Get DHT service from AppState
        let dht_guard = state.dht.lock().await;
        let dht = dht_guard
            .as_ref()
            .ok_or_else(|| "DHT service not initialized".to_string())?;

        // Create ReputationDhtService and retrieve verdicts
        let mut reputation_dht = reputation::ReputationDhtService::new();
        reputation_dht.set_dht_service(Arc::clone(dht));
        println!("🔍 RUST: About to retrieve verdicts");
        let verdicts = reputation_dht
            .retrieve_transaction_verdicts(&peer_id)
            .await?;

        println!("✅ RUST: Retrieved {} verdicts", verdicts.len());
        tracing::info!(
            "✅ Retrieved {} verdicts for peer: {}",
            verdicts.len(),
            peer_id
        );
        Ok(verdicts)
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            geth: Mutex::new(GethProcess::new()),
            downloader: Arc::new(GethDownloader::new()),
            miner_address: Mutex::new(None),
            active_account: Arc::new(Mutex::new(None)),
            active_account_private_key: Arc::new(Mutex::new(None)),
            rpc_url: Mutex::new("http://127.0.0.1:8545".to_string()),
            dht: Mutex::new(Some(dht_service_arc.clone())),
            file_transfer: Mutex::new(None),
            webrtc: Mutex::new(None),
            multi_source_download: Mutex::new(None),
            keystore: Arc::new(Mutex::new(
                Keystore::load().unwrap_or_else(|_| Keystore::new()),
            )),
            proxies: Arc::new(Mutex::new(Vec::new())),
            privacy_proxies: Arc::new(Mutex::new(Vec::new())),
            file_transfer_pump: Mutex::new(None),
            multi_source_pump: Mutex::new(None),
            socks5_proxy_cli: Mutex::new(args.socks5_proxy),
            analytics: Arc::new(analytics::AnalyticsService::new()),
            bandwidth: Arc::new(BandwidthController::new()),

            // Initialize transaction queue
            transaction_queue: Arc::new(Mutex::new(VecDeque::new())),
            transaction_processor: Mutex::new(None),
            processing_transaction: Arc::new(Mutex::new(false)),

            // Initialize upload sessions
            upload_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Initialize download sessions (for streaming WebRTC downloads)
            download_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Initialize proxy authentication tokens
            proxy_auth_tokens: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Initialize HTTP server state (uses same storage as FileTransferService)
            http_server_state: Arc::new(http_server::HttpServerState::new({
                // Use same storage directory as FileTransferService (files/, not chunks/)
                use directories::ProjectDirs;
                ProjectDirs::from("com", "chiral-network", "chiral-network")
                    .map(|dirs| dirs.data_dir().join("files"))
                    .unwrap_or_else(|| std::env::current_dir().unwrap().join("files"))
            })),
            http_server_addr: Arc::new(Mutex::new(None)),
            http_server_shutdown: Arc::new(Mutex::new(None)),

            // Initialize stream authentication
            stream_auth: Arc::new(Mutex::new(stream_auth::StreamAuthService::new())),

            // Initialize the new map for AES keys
            canonical_aes_keys: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Proof-of-Storage watcher background handle and contract address
            // make these clonable so we can .clone() and move into spawned tasks
            proof_watcher: Arc::new(Mutex::new(None)),
            proof_contract_address: Arc::new(Mutex::new(None)),

            // Relay reputation statistics
            relay_reputation: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Relay aliases
            relay_aliases: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Protocol Manager with BitTorrent support
            protocol_manager: protocol_manager_arc,

            // AutoRelay timeline persistence across DHT restarts
            autorelay_last_enabled: Arc::new(Mutex::new(None)),
            autorelay_last_disabled: Arc::new(Mutex::new(None)),

            // File logger - will be initialized in setup phase after loading settings
            file_logger: Arc::new(Mutex::new(None)),

            // BitTorrent handler for creating and seeding torrents
            bittorrent_handler: bittorrent_handler_arc,

            // Chunk manager (will be initialized when DHT starts)
            chunk_manager: Mutex::new(None),

            // Download restart service (will be initialized in setup)
            download_restart: Mutex::new(None),

            // FTP server for serving uploaded files
            ftp_server: Arc::new(chiral_network::ftp_server::FtpServer::new(
                directories::ProjectDirs::from("com", "chiral-network", "chiral-network")
                    .map(|dirs| dirs.data_dir().join("ftp_files"))
                    .unwrap_or_else(|| std::env::current_dir().unwrap().join("ftp_files")),
                2121, // FTP port
            )),
        })
        .invoke_handler(tauri::generate_handler![
            create_chiral_account,
            import_chiral_account,
            has_active_account,
            get_active_account_address,
            get_active_account_private_key,
            get_account_balance,
            get_user_balance,
            get_transaction_receipt,
            get_gas_prices,
            estimate_transaction_gas,
            can_afford_download,
            process_download_payment,
            record_download_payment,
            record_seeder_payment,
            check_payment_notifications,
            get_network_peer_count,
            get_network_chain_id,
            start_geth_node,
            stop_geth_node,
            save_account_to_keystore,
            load_account_from_keystore,
            list_keystore_accounts,
            pool::discover_mining_pools,
            pool::create_mining_pool,
            pool::join_mining_pool,
            pool::leave_mining_pool,
            pool::get_current_pool_info,
            pool::get_pool_stats,
            pool::update_pool_discovery,
            get_disk_space,
            send_chiral_transaction,
            queue_transaction,
            get_transaction_queue_status,
            get_transaction_by_hash,
            get_txpool_status,
            get_txpool_content,
            get_peer_info,
            debug_network_tx,
            get_cpu_temperature,
            get_power_consumption,
            download,
            seed,
            create_and_seed_torrent,
            is_geth_running,
            check_geth_binary,
            get_geth_status,
            download_geth_binary,
            check_bootstrap_health,
            get_cached_bootstrap_health,
            clear_bootstrap_cache,
            reconnect_geth_bootstrap,
            add_geth_peer,
            get_geth_peers,
            get_geth_node_info,
            set_miner_address,
            start_miner,
            stop_miner,
            get_miner_status,
            get_blockchain_sync_status,
            get_miner_hashrate,
            get_current_block,
            get_network_stats,
            get_chain_id,
            get_block_details_by_number,
            get_transaction_history,
            get_transaction_history_range,
            get_miner_logs,
            get_miner_performance,
            get_miner_diagnostics,
            start_mining_monitor,
            clear_blocks_cache,
            get_blocks_mined,
            initialize_mined_blocks_count,
            get_recent_mined_blocks_pub,
            get_mined_blocks_range,
            get_total_mining_rewards,
            get_block_reward,
            calculate_accurate_totals,
            get_cpu_temperature,
            start_dht_node,
            stop_dht_node,
            stop_publishing_file,
            search_file_metadata,
            get_file_seeders,
            connect_to_peer,
            get_dht_events,
            detect_locale,
            get_download_directory,
            check_directory_exists,
            get_default_storage_directory,
            validate_storage_path,
            ensure_directory_exists,
            get_dht_health,
            get_dht_peer_count,
            get_dht_peer_id,
            get_peer_id,
            is_dht_running,
            get_dht_connected_peers,
            start_file_transfer_service,
            download_file_from_network,
            upload_file_to_network,
            start_ftp_download,
            download_blocks_from_network,
            start_multi_source_download,
            cancel_multi_source_download,
            get_multi_source_progress,
            update_proxy_latency,
            get_proxy_optimization_status,
            download_file_multi_source,
            get_file_transfer_events,
            write_file,
            init_streaming_download,
            write_download_chunk,
            get_streaming_download_progress,
            finalize_streaming_download,
            cancel_streaming_download,
            save_download_checkpoint,
            resume_download_from_checkpoint,
            get_download_metrics,
            encrypt_file_with_password,
            decrypt_file_with_password,
            encrypt_file_for_upload,
            show_in_folder,
            get_available_storage,
            proxy_connect,
            proxy_disconnect,
            proxy_remove,
            proxy_echo,
            list_proxies,
            enable_privacy_routing,
            disable_privacy_routing,
            get_bootstrap_nodes_command,
            generate_totp_secret,
            is_2fa_enabled,
            verify_and_enable_totp,
            verify_totp_code,
            logout,
            disable_2fa,
            get_recommended_peers_for_file,
            record_transfer_success,
            record_transfer_failure,
            get_peer_metrics,
            get_connected_peer_metrics,
            report_malicious_peer,
            select_peers_with_strategy,
            set_peer_encryption_support,
            cleanup_inactive_peers,
            test_backend_connection,
            set_bandwidth_limits,
            establish_webrtc_connection,
            send_webrtc_file_request,
            get_webrtc_connection_status,
            disconnect_from_peer,
            create_temp_file_for_streaming,
            append_chunk_to_temp_file,
            copy_file_to_temp,
            start_streaming_upload,
            upload_file_chunk,
            cancel_streaming_upload,
            get_bandwidth_stats,
            get_bandwidth_history,
            get_performance_metrics,
            get_network_activity,
            get_resource_contribution,
            get_contribution_history,
            reset_analytics,
            reset_network_services,
            // ed2k server commands
            add_ed2k_source,
            list_ed2k_sources,
            remove_ed2k_source,
            test_ed2k_connection,
            search_ed2k_file,
            get_ed2k_download_status,
            parse_ed2k_link,
            // HTTP server commands
            start_http_server,
            stop_http_server,
            get_http_server_status,
            // Reputation system commands
            publish_reputation_verdict,
            get_reputation_verdicts,
            download_file_http,
            download_ed2k,
            download_ftp,
            save_temp_file_for_upload,
            get_file_size,
            // Reassembly system commands
            reassembly::write_chunk_temp,
            reassembly::verify_and_finalize,
            reassembly::save_chunk_bitmap,
            reassembly::load_chunk_bitmap,
            reassembly::cleanup_transfer_temp,
            encrypt_file_for_self_upload,
            encrypt_file_for_recipient,
            //request_file_access,
            decrypt_and_reassemble_file,
            create_auth_session,
            verify_stream_auth,
            generate_hmac_key,
            cleanup_auth_sessions,
            initiate_hmac_key_exchange,
            respond_to_hmac_key_exchange,
            confirm_hmac_key_exchange,
            finalize_hmac_key_exchange,
            get_hmac_exchange_status,
            get_active_hmac_exchanges,
            generate_proxy_auth_token,
            validate_proxy_auth_token,
            revoke_proxy_auth_token,
            cleanup_expired_proxy_auth_tokens,
            get_file_data,
            store_file_data,
            start_proof_of_storage_watcher,
            stop_proof_of_storage_watcher,
            get_relay_reputation_stats,
            set_relay_alias,
            get_relay_alias,
            save_app_settings,
            update_log_config,
            get_logs_directory,
            check_directory_exists,
            get_multiaddresses,
            clear_seed_list,
            get_full_network_stats,
            // Download restart commands
            start_download_restart,
            pause_download_restart,
            resume_download_restart,
            get_download_status_restart
        ])
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // When window is destroyed, stop geth
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    if let Ok(mut geth) = state.geth.try_lock() {
                        let _ = geth.stop();
                        println!("Geth node stopped on window destroy");
                    }
                }
            }
        })
        .setup(|app| {
            // Load settings from disk
            let settings = load_settings_from_file(&app.handle());

            // Initialize tracing subscriber with console output and optionally file output
            use tracing_subscriber::{fmt, prelude::*, EnvFilter};

            let env_filter = {
                #[cfg(debug_assertions)]
                {
                    EnvFilter::from_default_env()
                        .add_directive("chiral_network=info".parse().unwrap())
                        .add_directive("libp2p=warn".parse().unwrap())
                        .add_directive("libp2p_kad=warn".parse().unwrap())
                        .add_directive("libp2p_swarm=warn".parse().unwrap())
                        .add_directive("libp2p_mdns=warn".parse().unwrap())
                }
                #[cfg(not(debug_assertions))]
                {
                    EnvFilter::from_default_env()
                        .add_directive("chiral_network=warn".parse().unwrap())
                        .add_directive("libp2p=error".parse().unwrap())
                }
            };

            // Always create file logger (even if disabled) so it can be enabled/disabled later
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");
            let logs_dir = app_data_dir.join("logs");

            let log_config = logger::LogConfig::new(
                &logs_dir,
                settings.max_log_size_mb,
                settings.enable_file_logging,
            );

            let file_logger_writer = match logger::RotatingFileWriter::new(log_config) {
                Ok(writer) => {
                    let thread_safe_writer = logger::ThreadSafeWriter::new(writer);
                    Some(thread_safe_writer)
                }
                Err(e) => {
                    eprintln!("Failed to initialize file logger: {}", e);
                    None
                }
            };

            // Initialize tracing subscriber with both console and file output
            // File output will only write if enabled in config
            if let Some(ref file_writer) = file_logger_writer {
                tracing_subscriber::registry()
                    .with(fmt::layer()) // Console output
                    .with(fmt::layer().with_writer(file_writer.clone())) // File output (respects enabled flag)
                    .with(env_filter)
                    .init();
            } else {
                tracing_subscriber::registry()
                    .with(fmt::layer()) // Console output only
                    .with(env_filter)
                    .init();
            }

            // Store the file logger in app state so it can be updated later
            if let Some(file_writer) = file_logger_writer {
                if let Some(state) = app.try_state::<AppState>() {
                    let mut file_logger = state.file_logger.blocking_lock();
                    *file_logger = Some(file_writer.clone());

                    // Log the current log file path if logging is enabled
                    if settings.enable_file_logging {
                        if let Some(path) = file_writer.current_log_file_path() {
                            info!("Logs are being written to: {}", path.display());
                        }
                    }
                }
            }

            // Clean up any orphaned geth processes on startup
            #[cfg(unix)]
            {
                use std::process::Command;
                // Kill any geth processes that might be running from previous sessions
                let _ = Command::new("pkill")
                    .arg("-9")
                    .arg("-f")
                    .arg("geth.*--datadir.*geth-data")
                    .output();
            }

            #[cfg(windows)]
            {
                use std::process::Command;
                // On Windows, use taskkill to terminate geth processes
                let _ = Command::new("taskkill")
                    .args(["/F", "/IM", "geth.exe"])
                    .output();
            }

            // Also remove the lock file if it exists
            let lock_file = std::path::Path::new(DEFAULT_GETH_DATA_DIR).join("LOCK");
            if lock_file.exists() {
                println!("Removing stale LOCK file: {:?}", lock_file);
                let _ = std::fs::remove_file(&lock_file);
            }

            // Remove geth.ipc file if it exists (another common lock point)
            let ipc_file = std::path::Path::new(DEFAULT_GETH_DATA_DIR).join("geth.ipc");
            if ipc_file.exists() {
                println!("Removing stale IPC file: {:?}", ipc_file);
                let _ = std::fs::remove_file(&ipc_file);
            }

            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

            let icon = app
                .default_window_icon()
                .ok_or("Failed to get default window icon")?
                .clone();

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("Chiral Network")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } => {
                        println!("Tray icon left-clicked");
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        println!("Show menu item clicked");
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "hide" => {
                        println!("Hide menu item clicked");
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    "quit" => {
                        println!("Quit menu item clicked");
                        // Stop geth before exiting
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Ok(mut geth) = state.geth.try_lock() {
                                let _ = geth.stop();
                                println!("Geth node stopped");
                            }
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Get the main window and ensure it's visible
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();

                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        // Prevent the window from closing and hide it instead
                        api.prevent_close();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                });
            } else {
                println!("Could not find main window!");
            }

            // NOTE: You must add `start_proof_of_storage_watcher` to the invoke_handler call in the
            // real code where you register other commands. For brevity the snippet above shows where to add it.

            // Auto-start HTTP server
            // Spawn directly in setup() - no need to wait for window events
            {
                let app_handle = app.handle().clone();

                tauri::async_runtime::spawn(async move {
                    // Small delay to ensure state is fully initialized
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    if let Some(state) = app_handle.try_state::<AppState>() {
                        // Try ports 8080-8090 to support multiple instances
                        let mut server_started = false;
                        for port in 8080..=8090 {
                            let bind_addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();

                            // Create shutdown channel
                            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

                            match http_server::start_server(
                                state.http_server_state.clone(),
                                bind_addr,
                                shutdown_rx,
                            )
                            .await
                            {
                                Ok(bound_addr) => {
                                    let mut addr_lock = state.http_server_addr.lock().await;
                                    *addr_lock = Some(bound_addr);

                                    let mut shutdown_lock = state.http_server_shutdown.lock().await;
                                    *shutdown_lock = Some(shutdown_tx);

                                    server_started = true;
                                    break;
                                }
                                Err(e)
                                    if e.to_string().contains("address already in use")
                                        || e.to_string().contains("Address already in use")
                                        || e.to_string().contains("os error 48") =>
                                {
                                    tracing::debug!(
                                        "Port {} already in use, trying next port...",
                                        port
                                    );
                                    continue;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to start HTTP server on port {}: {}",
                                        port,
                                        e
                                    );
                                    eprintln!(
                                        "⚠️  HTTP server failed to start on port {}: {}",
                                        port, e
                                    );
                                    break;
                                }
                            }
                        }

                        if !server_started {
                            tracing::warn!("Could not start HTTP server on any port (8080-8090)");
                            eprintln!(
                                "⚠️  HTTP server could not start - all ports 8080-8090 are in use"
                            );
                        }
                    }
                });
            }

            // Initialize download restart service
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        let download_restart_service = Arc::new(
                            download_restart::DownloadRestartService::new(Some(app_handle.clone())),
                        );
                        if let Ok(mut dr_guard) = state.download_restart.try_lock() {
                            *dr_guard = Some(download_restart_service);
                        }
                    }
                });
            }

            // Start DHT event pump with the real app handle
            {
                let app_handle = app.handle().clone();
                let dht_clone_for_pump = {
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        if let Ok(dht_guard) = state.dht.try_lock() {
                            dht_guard.clone()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(dht_service) = dht_clone_for_pump {
                    let proxies_arc_for_pump = Arc::new(Mutex::new(Vec::new()));
                    let relay_reputation_arc_for_pump =
                        Arc::new(Mutex::new(std::collections::HashMap::new()));

                    tauri::async_runtime::spawn(async move {
                        pump_dht_events(
                            app_handle,
                            dht_service,
                            proxies_arc_for_pump,
                            relay_reputation_arc_for_pump,
                        )
                        .await;
                    });
                }
            }

            // Set app handle on bandwidth controller for event emission
            {
                let app_handle = app.handle().clone();
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let bandwidth_controller = state.bandwidth.clone();
                    let app_handle_for_bandwidth = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        bandwidth_controller
                            .set_app_handle(app_handle_for_bandwidth)
                            .await;
                    });
                }
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            tauri::RunEvent::ExitRequested { .. } => {
                println!("Exit requested event received");
                // Don't prevent exit, let it proceed naturally
            }
            tauri::RunEvent::Exit => {
                println!("App exiting, cleaning up geth...");
                // Stop geth before exiting
                if let Some(state) = app_handle.try_state::<AppState>() {
                    if let Ok(mut geth) = state.geth.try_lock() {
                        let _ = geth.stop();
                        println!("Geth node stopped on exit");
                    }
                }
            }
            _ => {}
        });
}

async fn create_bt_handler_with_fallback(
    download_dir: PathBuf,
    dht_service: Arc<DhtService>,
    port_range: Range<u16>,
) -> bittorrent_handler::BitTorrentHandler {
    // Try the requested range first
    if let Ok(h) = bittorrent_handler::BitTorrentHandler::new_with_port_range(
        download_dir.clone(),
        dht_service.clone(),
        Some(port_range.clone()),
    )
    .await
    {
        return h;
    }

    // Fallback: random range
    let mut rng = rand::thread_rng();

    loop {
        let start = rng.gen_range(30000..60000);
        let fallback = start..(start + 10);

        if let Ok(h) = bittorrent_handler::BitTorrentHandler::new_with_port_range(
            download_dir.clone(),
            dht_service.clone(),
            Some(fallback),
        )
        .await
        {
            return h;
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileManifestForJs {
    merkle_root: String,
    chunks: Vec<manager::ChunkInfo>,
    encrypted_key_bundle: String, // Serialized JSON of the bundle
}

#[tauri::command]
async fn encrypt_file_for_self_upload(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> Result<FileManifestForJs, String> {
    // 1. Get the active user's private key from state to derive the public key.
    let private_key_hex = state
        .active_account_private_key
        .lock()
        .await
        .clone()
        .ok_or("No account is currently active. Please log in.")?;

    // Get the app data directory for chunk storage
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not get app data directory: {}", e))?;
    let chunk_storage_path = app_data_dir.join("chunk_storage");

    // Run the encryption in a blocking task to avoid blocking the async runtime
    tokio::task::spawn_blocking(move || {
        let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
            .map_err(|_| "Invalid private key format".to_string())?;
        let secret_key = StaticSecret::from(
            <[u8; 32]>::try_from(pk_bytes).map_err(|_| "Private key is not 32 bytes")?,
        );
        let public_key = PublicKey::from(&secret_key);

        // 2. Initialize ChunkManager with proper app data directory
        let manager = ChunkManager::new(chunk_storage_path);

        // 3. Call the existing backend function to perform the encryption.
        let manifest = manager.chunk_and_encrypt_file(Path::new(&file_path), &public_key)?;

        // 4. Serialize the key bundle to a JSON string so it can be sent to the frontend easily.
        let bundle_json =
            serde_json::to_string(&manifest.encrypted_key_bundle).map_err(|e| e.to_string())?;

        Ok(FileManifestForJs {
            merkle_root: manifest.merkle_root,
            chunks: manifest.chunks,
            encrypted_key_bundle: bundle_json,
        })
    })
    .await
    .map_err(|e| format!("Encryption task failed: {}", e))?
}

/// Encrypt a file for upload with optional recipient public key
#[tauri::command]
async fn encrypt_file_for_recipient(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
    recipient_public_key: Option<String>,
) -> Result<FileManifestForJs, String> {
    // Get the app data directory for chunk storage
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not get app data directory: {}", e))?;
    let chunk_storage_path = app_data_dir.join("chunk_storage");

    // Determine the public key to use for encryption
    let recipient_pk = if let Some(pk_hex) = recipient_public_key {
        // Use the provided recipient public key
        let pk_bytes = hex::decode(pk_hex.trim_start_matches("0x"))
            .map_err(|_| "Invalid recipient public key format".to_string())?;
        PublicKey::from(
            <[u8; 32]>::try_from(pk_bytes).map_err(|_| "Recipient public key is not 32 bytes")?,
        )
    } else {
        // Use the active user's own public key
        let private_key_hex = state
            .active_account_private_key
            .lock()
            .await
            .clone()
            .ok_or("No account is currently active. Please log in.")?;
        let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
            .map_err(|_| "Invalid private key format".to_string())?;
        let secret_key = StaticSecret::from(
            <[u8; 32]>::try_from(pk_bytes).map_err(|_| "Private key is not 32 bytes")?,
        );
        PublicKey::from(&secret_key)
    };

    let private_key_hex = state
        .active_account_private_key
        .lock()
        .await
        .clone()
        .ok_or("No account is currently active. Please log in.")?;

    // Run the encryption in a blocking task to avoid blocking the async runtime
    tokio::task::spawn_blocking(move || {
        let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
            .map_err(|_| "Invalid private key format".to_string())?;
        let secret_key = StaticSecret::from(
            <[u8; 32]>::try_from(pk_bytes).map_err(|_| "Private key is not 32 bytes")?,
        );

        // Initialize ChunkManager with proper app data directory
        let manager = ChunkManager::new(chunk_storage_path);

        // Call the existing backend function to perform the encryption with recipient's public key
        let manifest = manager.chunk_and_encrypt_file(Path::new(&file_path), &recipient_pk)?;

        // Serialize the key bundle to a JSON string so it can be sent to the frontend easily.
        let bundle_json = match manifest.encrypted_key_bundle {
            Some(bundle) => serde_json::to_string(&bundle).map_err(|e| e.to_string())?,
            None => return Err("No encryption key bundle generated".to_string()),
        };

        Ok(FileManifestForJs {
            merkle_root: manifest.merkle_root,
            chunks: manifest.chunks,
            encrypted_key_bundle: bundle_json,
        })
    })
    .await
    .map_err(|e| format!("Encryption task failed: {}", e))?
}

/// Unified upload command: processes file with ChunkManager and auto-publishes to DHT
/// Returns file metadata for frontend use
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadResult {
    merkle_root: String,
    file_name: String,
    file_size: u64,
    is_encrypted: bool,
    peer_id: String,
    cid: Option<String>, // Add CID field for Bitswap uploads
}

#[tauri::command]
async fn has_active_account(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.active_account.lock().await.is_some())
}

#[tauri::command]
async fn get_active_account_address(state: State<'_, AppState>) -> Result<String, String> {
    state
        .active_account
        .lock()
        .await
        .clone()
        .ok_or_else(|| "No account is currently active. Please log in.".to_string())
}

#[tauri::command]
async fn get_active_account_private_key(state: State<'_, AppState>) -> Result<String, String> {
    state
        .active_account_private_key
        .lock()
        .await
        .clone()
        .ok_or_else(|| "No account is currently active. Please log in.".to_string())
}

#[tauri::command]
async fn decrypt_and_reassemble_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    manifest_js: FileManifestForJs,
    output_path: String,
) -> Result<(), String> {
    // 1. Get the active user's private key for decryption.
    let private_key_hex = state
        .active_account_private_key
        .lock()
        .await
        .clone()
        .ok_or("No account is currently active. Please log in.")?;

    let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
        .map_err(|_| "Invalid private key format".to_string())?;
    let secret_key = StaticSecret::from(
        <[u8; 32]>::try_from(pk_bytes).map_err(|_| "Private key is not 32 bytes")?,
    );

    // 2. Deserialize the key bundle from the string.
    let encrypted_key_bundle: encryption::EncryptedAesKeyBundle =
        serde_json::from_str(&manifest_js.encrypted_key_bundle).map_err(|e| e.to_string())?;

    // Get the app data directory for chunk storage
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not get app data directory: {}", e))?;
    let chunk_storage_path = app_data_dir.join("chunk_storage");

    // 3. Clone the data we need for the blocking task
    let chunks = manifest_js.chunks.clone();
    let output_path_clone = output_path.clone();

    // Run the decryption in a blocking task to avoid blocking the async runtime
    tokio::task::spawn_blocking(move || {
        // 4. Initialize ChunkManager with proper app data directory
        let manager = ChunkManager::new(chunk_storage_path);

        // 5. Call the existing backend function to decrypt and save the file.
        manager.reassemble_and_decrypt_file(
            &chunks,
            Path::new(&output_path_clone),
            &Some(encrypted_key_bundle),
            &secret_key, // Pass the secret key
        )
    })
    .await
    .map_err(|e| format!("Decryption task failed: {}", e))?
}

#[tauri::command]
async fn get_file_data(state: State<'_, AppState>, file_hash: String) -> Result<String, String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().await;
        ft_guard.as_ref().cloned()
    };
    if let Some(ft) = ft {
        let data = ft
            .get_file_data(&file_hash)
            .await
            .ok_or("File not found".to_string())?;
        use base64::{engine::general_purpose, Engine as _};
        Ok(general_purpose::STANDARD.encode(&data))
    } else {
        Err("File transfer service not running".to_string())
    }
}

#[tauri::command]
async fn store_file_data(
    state: State<'_, AppState>,
    file_hash: String,
    file_name: String,
    file_data: Vec<u8>,
) -> Result<(), String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().await;
        ft_guard.as_ref().cloned()
    };
    if let Some(ft) = ft {
        ft.store_file_data(file_hash, file_name, file_data).await;
        Ok(())
    } else {
        Err("File transfer service not running".to_string())
    }
}

// Proof-of-Storage blockchain watcher commands
// Monitors smart contract for storage challenges and submits proofs
#[tauri::command]
async fn start_proof_of_storage_watcher(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    contract_address: String,
    ws_url: String,
) -> Result<(), String> {
    // Basic validation
    if contract_address.trim().is_empty() {
        return Err("contract_address cannot be empty".into());
    }
    if ws_url.trim().is_empty() {
        return Err("ws_url cannot be empty".into());
    }

    // Store contract address in app state
    {
        let mut addr = state.proof_contract_address.lock().await;
        *addr = Some(contract_address.clone());
    }

    // Ensure any previous watcher is stopped
    stop_proof_of_storage_watcher(state.clone()).await.ok();

    // The DHT service is required for the listener to locate file chunks.
    let dht_service = {
        state
            .dht
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or("DHT service is not running. Cannot start proof watcher.")?
    };

    let handle = tokio::spawn(async move {
        tracing::info!("Starting proof-of-storage watcher...");
        // The listener will run until the contract address is cleared or an error occurs.
        if let Err(e) =
            blockchain_listener::run_blockchain_listener(ws_url, contract_address, dht_service)
                .await
        {
            tracing::error!("Proof-of-storage watcher failed: {}", e);
            // Emit an event to the frontend to notify the user of the failure.
            let _ = app.emit(
                "proof_watcher_error",
                format!("Watcher failed: {}", e.to_string()),
            );
        }
        tracing::info!("Proof watcher task exiting");
    });

    // Store the handle in AppState to manage its lifecycle
    {
        let mut guard = state.proof_watcher.lock().await;
        *guard = Some(handle);
    }

    Ok(())
}

// MerkleProof placeholder type - replace with your actual proof representation.
#[derive(Debug, Clone)]
struct MerkleProof {
    pub leaf_hash: Vec<u8>,
    pub proof_nodes: Vec<Vec<u8>>, // sequence of sibling hashes
    pub index: u32,
    pub total_leaves: u32,
}

#[tauri::command]
async fn stop_proof_of_storage_watcher(state: State<'_, AppState>) -> Result<(), String> {
    // Clear the configured contract address, which signals the listener loop to exit.
    {
        let mut addr = state.proof_contract_address.lock().await;
        *addr = None;
    }

    // Stop the background task if present
    let maybe_handle = {
        let mut guard = state.proof_watcher.lock().await;
        guard.take()
    };

    if let Some(handle) = maybe_handle {
        tracing::info!("Stopping Proof-of-Storage watcher...");
        // Abort the task to ensure it stops immediately.
        handle.abort();
        // Awaiting the aborted handle can confirm it's terminated.
        match tokio::time::timeout(tokio::time::Duration::from_secs(2), handle).await {
            Ok(_) => tracing::info!("Proof watcher task successfully joined."),
            Err(_) => tracing::warn!("Proof watcher abort timed out"),
        }
    } else {
        tracing::info!("No proof watcher to stop");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_mime_type_from_filename() {
        let cases = vec![
            ("image.jpg", "image/jpeg"),
            ("image.jpeg", "image/jpeg"),
            ("image.png", "image/png"),
            ("video.mp4", "video/mp4"),
            ("audio.mp3", "audio/mpeg"),
            ("document.pdf", "application/pdf"),
            ("archive.zip", "application/zip"),
            ("script.js", "application/javascript"),
            ("style.css", "text/css"),
            ("index.html", "text/html"),
            ("data.json", "application/json"),
            ("unknown.ext", "application/octet-stream"),
        ];

        for (input, expected_mime) in cases {
            let mime = detect_mime_type_from_filename(input);
            assert_eq!(mime, Some(expected_mime.to_string()));
        }
    }

    // Add more tests for other functions/modules as needed
}

#[derive(Debug, Serialize, Deserialize)]
struct RelayReputationStats {
    total_relays: usize,
    top_relays: Vec<RelayNodeStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayNodeStats {
    peer_id: String,
    alias: Option<String>,
    reputation_score: f64,
    reservations_accepted: u64,
    circuits_established: u64,
    circuits_successful: u64,
    total_events: u64,
    last_seen: u64,
}

#[tauri::command]
async fn get_relay_reputation_stats(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<RelayReputationStats, String> {
    // Read from relay reputation storage
    let stats_map = state.relay_reputation.lock().await;
    let aliases_map = state.relay_aliases.lock().await;

    let max_relays = limit.unwrap_or(100);

    // Convert HashMap to Vec, populate aliases, and sort by reputation score (descending)
    let mut all_relays: Vec<RelayNodeStats> = stats_map
        .values()
        .map(|stats| {
            let mut stats_with_alias = stats.clone();
            stats_with_alias.alias = aliases_map.get(&stats.peer_id).cloned();
            stats_with_alias
        })
        .collect();

    all_relays.sort_by(|a, b| {
        b.reputation_score
            .partial_cmp(&a.reputation_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Take top N relays
    let top_relays = all_relays.into_iter().take(max_relays).collect();
    let total_relays = stats_map.len();

    Ok(RelayReputationStats {
        total_relays,
        top_relays,
    })
}

#[tauri::command]
async fn set_relay_alias(
    state: State<'_, AppState>,
    peer_id: String,
    alias: String,
) -> Result<(), String> {
    let mut aliases = state.relay_aliases.lock().await;

    if alias.trim().is_empty() {
        aliases.remove(&peer_id);
    } else {
        aliases.insert(peer_id, alias.trim().to_string());
    }

    Ok(())
}

#[tauri::command]
async fn get_relay_alias(
    state: State<'_, AppState>,
    peer_id: String,
) -> Result<Option<String>, String> {
    let aliases = state.relay_aliases.lock().await;
    Ok(aliases.get(&peer_id).cloned())
}

#[tauri::command]
async fn get_multiaddresses(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        Ok(dht.get_multiaddresses().await)
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
async fn clear_seed_list() -> Result<(), String> {
    // Since you're using localStorage fallback, this command just needs to exist
    // The actual clearing happens in the frontend via localStorage.removeItem()
    // This command is here for consistency if you add file-based storage later
    Ok(())
}

#[tauri::command]
fn check_directory_exists(path: String) -> Result<bool, String> {
    use std::path::Path;
    let p = Path::new(&path);
    Ok(p.exists() && p.is_dir())
}

/// Returns the platform-specific default storage directory path as a string
#[tauri::command]
fn get_default_storage_directory() -> String {
    #[cfg(target_os = "windows")]
    {
        // Get the user's home directory from environment variable
        let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\<user>".to_string());
        return format!("{}\\Downloads\\Chiral-Network-Storage", user_profile);
    }
    #[cfg(target_os = "macos")]
    {
        // Use home directory with tilde expansion
        return "~/Downloads/Chiral-Network-Storage".to_string();
    }
    #[cfg(target_os = "linux")]
    {
        // Use home directory with tilde expansion
        return "~/Downloads/Chiral-Network-Storage".to_string();
    }
    // Fallback for other platforms
    "~/Downloads/Chiral-Network-Storage".to_string()
}

/// Event pump for DHT events, moved out of start_dht_node
async fn pump_dht_events(
    app_handle: tauri::AppHandle,
    dht_service: Arc<DhtService>,
    proxies_arc: Arc<Mutex<Vec<ProxyNode>>>,
    relay_reputation_arc: Arc<Mutex<std::collections::HashMap<String, RelayNodeStats>>>,
) {
    loop {
        let events = dht_service.drain_events(64).await;
        if events.is_empty() {
            if Arc::strong_count(&dht_service) <= 1 {
                info!("DHT service appears to be shut down. Exiting event pump.");
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }

        for ev in events {
            match ev {
                DhtEvent::PeerDiscovered { peer_id, addresses } => {
                    let payload = serde_json::json!({ "peerId": peer_id, "addresses": addresses });
                    let _ = app_handle.emit("dht_peer_discovered", payload);
                }
                DhtEvent::PeerConnected { peer_id, address } => {
                    let payload = serde_json::json!({ "peerId": peer_id, "address": address });
                    let _ = app_handle.emit("dht_peer_connected", payload);
                }
                DhtEvent::PeerDisconnected { peer_id } => {
                    let payload = serde_json::json!({ "peerId": peer_id });
                    let _ = app_handle.emit("dht_peer_disconnected", payload);
                }
                DhtEvent::ProxyStatus {
                    id,
                    address,
                    status,
                    latency_ms,
                    error,
                } => {
                    let to_emit: ProxyNode = {
                        let mut proxies = proxies_arc.lock().await;
                        if let Some(i) = proxies.iter().position(|p| p.id == id) {
                            let p = &mut proxies[i];
                            if !address.is_empty() {
                                p.address = address.clone();
                            }
                            p.status = status.clone();
                            if let Some(ms) = latency_ms {
                                p.latency = ms as u32;
                            }
                            p.error = error.clone();
                            p.clone()
                        } else {
                            let node = ProxyNode {
                                id: id.clone(),
                                address,
                                status,
                                latency: latency_ms.unwrap_or(0) as u32,
                                error,
                            };
                            proxies.push(node.clone());
                            node
                        }
                    };
                    let _ = app_handle.emit("proxy_status_update", to_emit);
                }
                DhtEvent::NatStatus {
                    state,
                    confidence,
                    last_error,
                    summary,
                } => {
                    let payload = serde_json::json!({ "state": state, "confidence": confidence, "lastError": last_error, "summary": summary });
                    let _ = app_handle.emit("nat_status_update", payload);
                }
                DhtEvent::FileDiscovered(metadata) => {
                    let _ = app_handle.emit("found_file", &metadata);
                }
                DhtEvent::PublishedFile(metadata) => {
                    let _ = app_handle.emit("published_file", &metadata);
                }
                DhtEvent::ReputationEvent {
                    peer_id,
                    event_type,
                    impact,
                    data,
                } => {
                    let mut stats = relay_reputation_arc.lock().await;
                    let entry = stats.entry(peer_id.clone()).or_insert(RelayNodeStats {
                        peer_id: peer_id.clone(),
                        alias: None,
                        reputation_score: 0.0,
                        reservations_accepted: 0,
                        circuits_established: 0,
                        circuits_successful: 0,
                        total_events: 0,
                        last_seen: 0,
                    });

                    entry.reputation_score += impact;
                    entry.total_events += 1;
                    entry.last_seen = data
                        .get("timestamp")
                        .and_then(|v| v.as_u64())
                        .unwrap_or_else(|| {
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        });

                    match event_type.as_str() {
                        "RelayReservationAccepted" => entry.reservations_accepted += 1,
                        "RelayCircuitEstablished" => entry.circuits_established += 1,
                        "RelayCircuitSuccessful" => entry.circuits_successful += 1,
                        _ => {}
                    }

                    let payload = serde_json::json!({ "peerId": peer_id, "eventType": event_type, "impact": impact, "data": data });
                    let _ = app_handle.emit("relay_reputation_event", payload);
                }
                DhtEvent::BitswapChunkDownloaded {
                    file_hash,
                    chunk_index,
                    total_chunks,
                    chunk_size,
                } => {
                    let payload = serde_json::json!({ "fileHash": file_hash, "chunkIndex": chunk_index, "totalChunks": total_chunks, "chunkSize": chunk_size });
                    let _ = app_handle.emit("bitswap_chunk_downloaded", payload);
                }
                DhtEvent::PaymentNotificationReceived { from_peer, payload } => {
                    if let Ok(notification) =
                        serde_json::from_value::<serde_json::Value>(payload.clone())
                    {
                        let _ = app_handle.emit("seeder_payment_received", &notification);
                    }
                }
                _ => {}
            }
        }
    }
}
