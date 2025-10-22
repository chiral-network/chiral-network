#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

pub mod commands;

pub mod analytics;
mod blockchain_listener;
mod dht;
mod encryption;
mod ethereum;
mod file_transfer;
mod geth_downloader;
mod headless;
mod keystore;
mod manager;
mod multi_source_download;
pub mod net;
mod peer_selection;
mod pool;
mod proxy_latency;
mod stream_auth;
mod webrtc_service;

use crate::commands::auth::{
    cleanup_expired_proxy_auth_tokens, generate_proxy_auth_token, revoke_proxy_auth_token,
    validate_proxy_auth_token,
};
use crate::commands::bootstrap::get_bootstrap_nodes_command;
use crate::commands::proxy::{
    disable_privacy_routing, enable_privacy_routing, list_proxies, proxy_connect, proxy_disconnect,
    proxy_echo, proxy_remove, ProxyNode,
};
use crate::stream_auth::{
    AuthMessage, HmacKeyExchangeConfirmation, HmacKeyExchangeRequest, HmacKeyExchangeResponse,
    StreamAuthService,
};
use chrono;
use dht::{DhtEvent, DhtMetricsSnapshot, DhtService, FileMetadata};
use directories::ProjectDirs;
use ethereum::{
    create_new_account,
    get_account_from_private_key,
    get_balance,
    get_block_number,
    get_hashrate,
    get_mined_blocks_count,
    get_mining_logs,
    get_mining_performance,
    get_mining_status, // Assuming you have a file_handler module
    get_network_difficulty,
    get_network_hashrate,
    get_peer_count,
    get_recent_mined_blocks,
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
use std::collections::VecDeque;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex as StdMutex;
use std::{
    io::{BufRead, BufReader},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Components, System};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};
use tokio::time::Duration as TokioDuration;
use tokio::{sync::Mutex, task::JoinHandle, time::sleep};
use totp_rs::{Algorithm, Secret, TOTP};
use tracing::{debug, error, info, warn};
use webrtc_service::{WebRTCFileRequest, WebRTCService};

use crate::manager::ChunkManager; // Import the ChunkManager
                                  // For key encoding
use blockstore::block::Block;
use x25519_dalek::{PublicKey, StaticSecret}; // For key handling

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

    // New fields for transaction queue
    transaction_queue: Arc<Mutex<VecDeque<QueuedTransaction>>>,
    transaction_processor: Mutex<Option<JoinHandle<()>>>,
    processing_transaction: Arc<Mutex<bool>>,

    // New field for streaming upload sessions
    upload_sessions: Arc<Mutex<std::collections::HashMap<String, StreamingUploadSession>>>,

    // Proxy authentication tokens storage
    proxy_auth_tokens: Arc<Mutex<std::collections::HashMap<String, ProxyAuthToken>>>,

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
}

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
    *state.rpc_url.lock().await = rpc_url;

    geth.start(&data_dir, miner_address.as_deref())
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
async fn can_afford_download(state: State<'_, AppState>, price: f64) -> Result<bool, String> {
    let account = get_active_account(&state).await?;
    let balance_str = get_balance(&account).await?;
    let balance = balance_str.parse::<f64>()
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
    amount: f64,
    transaction_id: u64,
    transaction_hash: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    println!(
        "📝 Download payment recorded: {} Chiral to wallet {} (peer: {}) tx: {}",
        amount, seeder_wallet_address, seeder_peer_id, transaction_hash
    );

    // Send P2P payment notification message to the seeder's peer
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct PaymentNotificationMessage {
        file_hash: String,
        file_name: String,
        file_size: u64,
        downloader_address: String,
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

    println!("✅ Payment notification emitted locally for seeder: {}", seeder_wallet_address);

    // Send P2P payment notification to the seeder's peer
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        // Convert payment message to JSON
        let notification_json = serde_json::to_value(&payment_msg)
            .map_err(|e| format!("Failed to serialize payment notification: {}", e))?;

        // Wrap in a payment notification envelope so receiver can identify it
        let wrapped_message = serde_json::json!({
            "type": "payment_notification",
            "payload": notification_json
        });

        // Send via DHT to the seeder's peer ID
        match dht.send_message_to_peer(&seeder_peer_id, wrapped_message).await {
            Ok(_) => {
                println!("✅ P2P payment notification sent to peer: {}", seeder_peer_id);
            }
            Err(e) => {
                // Don't fail the whole operation if P2P message fails
                println!("⚠️ Failed to send P2P payment notification: {}. Seeder will see payment when they check blockchain.", e);
            }
        }
    } else {
        println!("⚠️ DHT not available, payment notification only sent locally");
    }

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
    println!("💰 Seeder payment received: {} Chiral from {}", _amount, _downloader_address);
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
async fn get_file_versions_by_name(
    state: State<'_, AppState>,
    file_name: String,
) -> Result<Vec<FileMetadata>, String> {
    info!(
        "🚀 Tauri command: get_file_versions_by_name called with: {}",
        file_name
    );

    let dht = { state.dht.lock().await.as_ref().cloned() };
    if let Some(dht) = dht {
        info!("✅ DHT service found, calling get_versions_by_file_name");
        let result = (*dht).get_versions_by_file_name(file_name).await;
        match &result {
            Ok(versions) => info!(
                "🎉 Tauri command: Successfully returned {} versions",
                versions.len()
            ),
            Err(e) => info!("❌ Tauri command: Error occurred: {}", e),
        }
        result
    } else {
        info!("❌ Tauri command: DHT not running");
        Err("DHT not running".into())
    }
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

#[tauri::command]
async fn upload_versioned_file(
    state: State<'_, AppState>,
    file_name: String,
    file_path: String,
    _file_size: u64,
    mime_type: Option<String>,
    is_encrypted: bool,
    encryption_method: Option<String>,
    key_fingerprint: Option<String>,
    price: Option<f64>,
) -> Result<FileMetadata, String> {
    // Get the active account address
    let account = get_active_account(&state).await?;
    let dht_opt = { state.dht.lock().await.as_ref().cloned() };
    if let Some(dht) = dht_opt {
        // --- FIX: Calculate file_hash using file_transfer helper
        let file_data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| e.to_string())?;
        let file_hash = FileTransferService::calculate_file_hash(&file_data);

        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Use the DHT versioning helper to fill in parent_hash/version
        let metadata = dht
            .prepare_versioned_metadata(
                file_hash.clone(),
                file_name.clone(),
                file_data.len() as u64, // Use file size directly from data
                file_data.clone(),
                created_at,
                mime_type,
                None, // encrypted_key_bundle
                is_encrypted,
                encryption_method,
                key_fingerprint,
                price,
                Some(account.clone()),
            )
            .await?;

        // Store file data locally for seeding
        let ft = {
            let ft_guard = state.file_transfer.lock().await; // Store the file locally for seeding
            ft_guard.as_ref().cloned()
        };
        if let Some(ft) = ft {
            ft.store_file_data(file_hash.clone(), file_name, file_data)
                .await;
        }

        dht.publish_file(metadata.clone()).await?;
        Ok(metadata)
    } else {
        Err("DHT not running".into())
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
    stop_mining().await
}

#[tauri::command]
async fn get_miner_status() -> Result<bool, String> {
    get_mining_status().await
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
    Ok((difficulty, hashrate))
}

#[tauri::command]
async fn get_miner_logs(data_dir: String, lines: usize) -> Result<Vec<String>, String> {
    get_mining_logs(&data_dir, lines)
}

#[tauri::command]
async fn get_miner_performance(data_dir: String) -> Result<(u64, f64), String> {
    get_mining_performance(&data_dir)
}
lazy_static! {
    static ref BLOCKS_CACHE: StdMutex<Option<(String, u64, Instant)>> = StdMutex::new(None);
}
#[tauri::command]

async fn get_blocks_mined(address: String) -> Result<u64, String> {
    // Check cache (directly return within 500ms)
    {
        let cache = BLOCKS_CACHE.lock().unwrap();
        if let Some((cached_addr, cached_blocks, cached_time)) = cache.as_ref() {
            if cached_addr == &address && cached_time.elapsed() < Duration::from_millis(500) {
                return Ok(*cached_blocks);
            }
        }
    }

    // Invoke existing logic (slow query)
    let blocks = get_mined_blocks_count(&address).await?;

    // Update Cache
    {
        let mut cache = BLOCKS_CACHE.lock().unwrap();
        *cache = Some((address, blocks, Instant::now()));
    }

    Ok(blocks)
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
async fn start_dht_node(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    port: u16,
    bootstrap_nodes: Vec<String>,
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
) -> Result<String, String> {
    {
        let dht_guard = state.dht.lock().await;
        if dht_guard.is_some() {
            return Err("DHT node is already running".to_string());
        }
    }

    // Disable autonat by default to prevent warnings when no servers are available
    // Users can explicitly enable it when needed
    let auto_enabled = enable_autonat.unwrap_or(true);
    let probe_interval = autonat_probe_interval_secs.map(Duration::from_secs);
    let autonat_server_list = autonat_servers.unwrap_or_default();

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

    // --- Hotfix: Disable AutoRelay on bootstrap nodes (and via env var)
    let mut final_enable_autorelay = enable_autorelay.unwrap_or(true);
    if is_bootstrap.unwrap_or(false) {
        final_enable_autorelay = false;
        tracing::info!("AutoRelay disabled on bootstrap (hotfix).");
    }
    if std::env::var("CHIRAL_DISABLE_AUTORELAY").ok().as_deref() == Some("1") {
        final_enable_autorelay = false;
        tracing::info!("AutoRelay disabled via env CHIRAL_DISABLE_AUTORELAY=1");
    }

    let proj_dirs = ProjectDirs::from("com", "chiral-network", "chiral-network")
        .ok_or("Failed to get project directories")?;
    // Create the async_std::path::Path here so we can pass a reference to it.
    let blockstore_db_path = proj_dirs.data_dir().join("blockstore_db");
    let async_blockstore_path = async_std::path::Path::new(blockstore_db_path.as_os_str());

    let dht_service = DhtService::new(
        port,
        bootstrap_nodes,
        None,
        is_bootstrap.unwrap_or(false),
        /* enable AutoNAT by default for WAN */ auto_enabled,
        probe_interval,
        autonat_server_list,
        final_proxy_address,
        file_transfer_service,
        Some(chunk_manager), // Pass the chunk manager
        chunk_size_kb,
        cache_size_mb,
        /* enable AutoRelay (after hotfix) */ final_enable_autorelay,
        preferred_relays.unwrap_or_default(),
        is_bootstrap.unwrap_or(false), // enable_relay_server only on bootstrap
        Some(&async_blockstore_path),
    )
    .await
    .map_err(|e| format!("Failed to start DHT: {}", e))?;

    let peer_id = dht_service.get_peer_id().await;

    // DHT node is already running in a spawned background task
    let dht_arc = Arc::new(dht_service);

    // Spawn the event pump
    let app_handle = app.clone();
    let proxies_arc = state.proxies.clone();
    let relay_reputation_arc = state.relay_reputation.clone();
    let dht_clone_for_pump = dht_arc.clone();

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
                        let payload = serde_json::json!(metadata);
                        let _ = app_handle.emit("file_content", payload);
                    }
                    DhtEvent::PublishedFile(metadata) => {
                        let payload = serde_json::json!(metadata);
                        let _ = app_handle.emit("published_file", payload);
                    }
                    DhtEvent::FileDiscovered(metadata) => {
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
                                    .unwrap()
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
                    DhtEvent::BitswapChunkDownloaded { file_hash, chunk_index, total_chunks, chunk_size } => {
                        let payload = serde_json::json!({
                            "fileHash": file_hash,
                            "chunkIndex": chunk_index,
                            "totalChunks": total_chunks,
                            "chunkSize": chunk_size,
                        });
                        let _ = app_handle.emit("bitswap_chunk_downloaded", payload);
                    },
                    DhtEvent::PaymentNotificationReceived { from_peer, payload } => {
                        println!("💰 Payment notification received from peer {}: {:?}", from_peer, payload);
                        // Convert payload to match the expected format for seeder_payment_received
                        if let Ok(notification) = serde_json::from_value::<serde_json::Value>(payload.clone()) {
                            let formatted_payload = serde_json::json!({
                                "file_hash": notification.get("file_hash").and_then(|v| v.as_str()).unwrap_or(""),
                                "file_name": notification.get("file_name").and_then(|v| v.as_str()).unwrap_or(""),
                                "file_size": notification.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0),
                                "downloader_address": notification.get("downloader_address").and_then(|v| v.as_str()).unwrap_or(""),
                                "seeder_wallet_address": notification.get("seeder_wallet_address").and_then(|v| v.as_str()).unwrap_or(""),
                                "amount": notification.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                "transaction_id": notification.get("transaction_id").and_then(|v| v.as_u64()).unwrap_or(0),
                            });
                            // Emit the same event that local payments use
                            let _ = app_handle.emit("seeder_payment_received", formatted_payload);
                            println!("✅ Payment notification forwarded to frontend");
                        }
                    },
                    _ => {}
                }
            }
        }
    });

    {
        let mut dht_guard = state.dht.lock().await;
        *dht_guard = Some(dht_arc);
    }

    Ok(peer_id)
}

#[tauri::command]
async fn stop_dht_node(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dht = {
        let mut dht_guard = state.dht.lock().await;
        dht_guard.take()
    };

    if let Some(dht) = dht {
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
async fn send_dht_message(
    state: State<'_, AppState>,
    peer_id: String,
    message: serde_json::Value,
) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        // Send message through DHT to target peer
        dht.send_message_to_peer(&peer_id, message)
            .await
            .map_err(|e| format!("Failed to send DHT message: {}", e))
    } else {
        Err("DHT not available".to_string())
    }
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
                DhtEvent::BitswapChunkDownloaded { file_hash, chunk_index, total_chunks, chunk_size } => {
                    format!("bitswap_chunk_downloaded:{}:{}:{}:{}", file_hash, chunk_index, total_chunks, chunk_size)
                },
                DhtEvent::PaymentNotificationReceived { from_peer, payload } => {
                    format!("payment_notification_received:{}:{:?}", from_peer, payload)
                },
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
            let mut last_update = last_update_mutex.lock().unwrap();
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
            let mut history = temp_history_mutex.lock().unwrap();

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
            let working_method = working_method_mutex.lock().unwrap();
            if let Some(ref method) = *working_method {
                if let Some(temp) = try_temperature_method(method) {
                    return Some(smooth_temperature(temp));
                }
                // Method stopped working, clear cache
                drop(working_method);
                let mut working_method = working_method_mutex.lock().unwrap();
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
                let mut working_method = working_method_mutex.lock().unwrap();
                *working_method = Some(method.clone());
                return Some(smooth_temperature(temp));
            }
        }

        // Try more Linux methods if the basic ones failed
        #[cfg(target_os = "linux")]
        {
            if let Some((temp, method)) = get_linux_temperature_advanced() {
                let mut working_method = working_method_mutex.lock().unwrap();
                *working_method = Some(method);
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
                        let mut logged = log_state.lock().unwrap();
                        if !*logged {
                            info!("✅ Temperature sensor detected via WMI HighPrecision: {:.1}°C", temp_celsius);
                            *logged = true;
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
                        let mut logged = log_state.lock().unwrap();
                        if !*logged {
                            info!("✅ Temperature sensor detected via WMI CurrentTemperature: {:.1}°C", temp_celsius);
                            *logged = true;
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
                        let mut logged = log_state.lock().unwrap();
                        if !*logged {
                            info!("✅ Temperature sensor detected via MSAcpi: {:.1}°C", temp_celsius);
                            *logged = true;
                        }
                        return Some(temp_celsius);
                    }
                }
            }
        }
    }

    // Log only once when no sensor is found
    let log_state = LAST_LOG_STATE.get_or_init(|| std::sync::Mutex::new(false));
    let mut logged = log_state.lock().unwrap();
    if !*logged {
        info!("⚠️ No WMI temperature sensors detected. Temperature monitoring disabled.");
        *logged = true;
    }

    None
}

#[tauri::command]
fn detect_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".into())
}

#[tauri::command]
fn get_default_storage_path(app: tauri::AppHandle) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not get app data directory: {}", e))?;

    let storage_path = app_data_dir.join("Storage");

    storage_path
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to convert path to string".to_string())
}

#[tauri::command]
fn check_directory_exists(path: String) -> bool {
    Path::new(&path).is_dir()
}

#[tauri::command]
async fn start_file_transfer_service(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let ft_guard = state.file_transfer.lock().await;
        if ft_guard.is_some() {
            return Err("File transfer service is already running".to_string());
        }
    }

    let file_transfer_service = FileTransferService::new_with_encryption(true)
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
    )
    .await
    .map_err(|e| format!("Failed to start WebRTC service: {}", e))?;

    let webrtc_arc = Arc::new(webrtc_service);
    {
        let mut webrtc_guard = state.webrtc.lock().await;
        *webrtc_guard = Some(webrtc_arc.clone());
    }

    // Initialize multi-source download service
    let dht_arc = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht_service) = dht_arc {
        let multi_source_service = MultiSourceDownloadService::new(dht_service, webrtc_arc.clone());
        let multi_source_arc = Arc::new(multi_source_service);

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
    state: State<'_, AppState>,
    file_path: String,
    price: Option<f64>,
) -> Result<(), String> {
    println!("🔍 BACKEND: upload_file_to_network called with price: {:?}", price);

    // Get the active account address
    let account = get_active_account(&state).await?;

    // Get the private key from state
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

    if let Some(ft) = ft {
        // Upload the file
        let file_name = file_path.split('/').last().unwrap_or(&file_path);

        ft.upload_file_with_account(
            file_path.clone(),
            file_name.to_string(),
            Some(account.clone()),
            Some(private_key),
        )
        .await
        .map_err(|e| format!("Failed to upload file: {}", e))?;

        // Get the file hash by reading the file and calculating it
        let file_data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;
        let file_hash = file_transfer::FileTransferService::calculate_file_hash(&file_data);

        // Also publish to DHT if it's running
        let dht = {
            let dht_guard = state.dht.lock().await;
            dht_guard.as_ref().cloned()
        };

        if let Some(dht) = dht {
            let metadata = FileMetadata {
                merkle_root: file_hash.clone(),
                is_root: true,
                file_name: file_name.to_string(),
                file_size: file_data.len() as u64,
                file_data: file_data.clone(),
                seeders: vec![],
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                mime_type: None,
                is_encrypted: false,
                encryption_method: None,
                key_fingerprint: None,
                parent_hash: None,
                version: Some(1),
                cids: None,
                encrypted_key_bundle: None,
                price,
                uploader_address: Some(account.clone()),
                ..Default::default()
            };

            match dht.publish_file(metadata.clone()).await {
                Ok(_) => info!("Published file metadata to DHT: {}", file_hash),
                Err(e) => warn!("Failed to publish file metadata to DHT: {}", e),
            };

            Ok(())
        } else {
            Err("DHT Service not running.".to_string())
        }
    } else {
        Err("File transfer service is not running".to_string())
    }
}

#[tauri::command]
async fn download_blocks_from_network(
    state: State<'_, AppState>,
    file_metadata: FileMetadata,
    download_path: String,
) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        info!("calling dht download_file");
        dht.download_file(file_metadata, download_path).await
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn download_file_from_network(
    state: State<'_, AppState>,
    file_hash: String,
    _output_path: String,
) -> Result<String, String> {
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
            // Search for file metadata in DHT with 5 second timeout
            match dht_service
                .synchronous_search_metadata(file_hash.clone(), 5000)
                .await
            {
                Ok(Some(metadata)) => {
                    info!(
                        "Found file metadata in DHT: {} (size: {} bytes)",
                        metadata.file_name, metadata.file_size
                    );

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
                        info!("File found but no seeders currently available");
                        // Return metadata as JSON instead of error so frontend can display file info
                        let metadata_json = serde_json::to_string(&metadata)
                            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
                        return Ok(metadata_json);
                    }

                    // Implement chunk requesting protocol with real WebRTC
                    // Create WebRTC offer for the first available peer
                    let webrtc = {
                        let webrtc_guard = state.webrtc.lock().await;
                        webrtc_guard.as_ref().cloned()
                    };

                    if let Some(webrtc_service) = webrtc {
                        // Select the best peer for download
                        let selected_peer = if available_peers.len() == 1 {
                            available_peers[0].clone()
                        } else {
                            // Use peer selection strategy to pick the best peer
                            let recommended = dht_service
                                .select_peers_with_strategy(
                                    &available_peers,
                                    1,
                                    crate::peer_selection::SelectionStrategy::FastestFirst,
                                    false,
                                )
                                .await;
                            recommended
                                .into_iter()
                                .next()
                                .unwrap_or_else(|| available_peers[0].clone())
                        };

                        info!("Selected peer {} for WebRTC download", selected_peer);

                        // Create WebRTC offer
                        match webrtc_service.create_offer(selected_peer.clone()).await {
                            Ok(offer) => {
                                info!("Created WebRTC offer for peer {}", selected_peer);

                                // Send WebRTC offer via DHT signaling
                                let offer_request = dht::WebRTCOfferRequest {
                                    offer_sdp: offer, // The Merkle root is now the primary file hash
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
                                                        let file_request = crate::webrtc_service::WebRTCFileRequest {
                                                            file_hash: metadata.merkle_root.clone(),
                                                            file_name: metadata.file_name.clone(),
                                                            file_size: metadata.file_size,
                                                            requester_peer_id: dht_service.get_peer_id().await,
                                                            recipient_public_key: None, // No encryption for basic downloads
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

                                                                // The peer will now start sending chunks automatically
                                                                // We don't need to request individual chunks - the WebRTC service handles this
                                                                Ok(format!(
                                                                    "WebRTC download initiated: {} ({} bytes) from peer {}",
                                                                    metadata.file_name, metadata.file_size, selected_peer
                                                                ))
                                                            }
                                                            Err(e) => {
                                                                warn!("Failed to send file request: {}", e);
                                                                Err(format!("Failed to send file request: {}", e))
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        warn!("Failed to establish WebRTC connection: {}", e);
                                                        Err(format!(
                                                            "WebRTC connection failed: {}",
                                                            e
                                                        ))
                                                    }
                                                }
                                            }
                                            Ok(Ok(Err(e))) => {
                                                warn!("WebRTC signaling failed: {}", e);
                                                Err(format!("WebRTC signaling failed: {}", e))
                                            }
                                            Ok(Err(_)) => {
                                                warn!("WebRTC answer receiver was canceled");
                                                Err("WebRTC answer receiver was canceled"
                                                    .to_string())
                                            }
                                            Err(_) => {
                                                warn!(
                                                    "WebRTC answer timeout from peer {}",
                                                    selected_peer
                                                );
                                                Err(format!(
                                                    "WebRTC answer timeout from peer {}",
                                                    selected_peer
                                                ))
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to send WebRTC offer: {}", e);
                                        Err(format!("Failed to send WebRTC offer: {}", e))
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to create WebRTC offer: {}", e);
                                Err(format!("WebRTC setup failed: {}", e))
                            }
                        }
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
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
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
        .unwrap()
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
    let metadata = fs::metadata(&file_path)
        .map_err(|e| format!("Failed to get file metadata: {}", e))?;
    Ok(metadata.len())
}

#[tauri::command]
async fn start_streaming_upload(
    file_name: String,
    file_size: u64,
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
            .unwrap()
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
) -> Result<Option<String>, String> {
    let mut upload_sessions = state.upload_sessions.lock().await;
    let session = upload_sessions
        .get_mut(&upload_id)
        .ok_or_else(|| format!("Upload session {} not found", upload_id))?;

    // Update hasher with chunk data and accumulate file data
    session.hasher.update(&chunk_data);
    session.file_data.extend_from_slice(&chunk_data);
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

    if is_last_chunk {
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
            .unwrap()
            .as_secs();

        let metadata = dht::FileMetadata {
            merkle_root: merkle_root, // Store Merkle root for verification
            file_name: session.file_name.clone(),
            file_size: session.file_size,
            file_data: vec![], // Empty - data is stored in Bitswap blocks
            seeders: vec![],
            created_at,
            mime_type: None,
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            version: Some(1),
            cids: Some(vec![root_cid.clone()]), // The root CID for retrieval
            encrypted_key_bundle: None,
            parent_hash: None,
            is_root: true,
            download_path: None,
            price: None,
            uploader_address: None,
        };

        // Store complete file data locally for seeding
        let complete_file_data = session.file_data.clone();
        let file_name_for_storage = session.file_name.clone();

        // Clean up session before storing file data
        let file_hash = root_cid.to_string();
        upload_sessions.remove(&upload_id);

        // Release the upload_sessions lock before the async operation
        drop(upload_sessions);

        // Store file data in FileTransferService
        let ft = {
            let ft_guard = state.file_transfer.lock().await;
            ft_guard.as_ref().cloned()
        };
        if let Some(ft) = ft {
            ft.store_file_data(file_hash.clone(), file_name_for_storage, complete_file_data)
                .await;
        }

        // Publish to DHT
        if let Some(dht) = dht_opt {
            dht.publish_file(metadata.clone()).await?;
        } else {
            return Err("DHT not running".into());
        }

        Ok(Some(file_hash))
    } else {
        Ok(None)
    }
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

#[tauri::command]
async fn search_file_metadata(
    state: State<'_, AppState>,
    file_hash: String,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let timeout = timeout_ms.unwrap_or(10_000);
        dht.search_metadata(file_hash, timeout).await
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
        Ok(dht_service.get_seeders_for_file(&file_hash).await)
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
        data_dir: data_dir_value,
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
) -> Result<Vec<crate::peer_selection::PeerMetrics>, String> {
    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        Ok(dht.get_peer_metrics().await)
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
    use crate::peer_selection::SelectionStrategy;

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
            .unwrap()
            .as_millis()
    );

    // Create queued transaction
    let queued_tx = QueuedTransaction {
        id: tx_id.clone(),
        to_address,
        amount,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
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

#[cfg(not(test))]
fn main() {
    // Initialize logging for debug builds
    #[cfg(debug_assertions)]
    {
        use tracing_subscriber::{fmt, prelude::*, EnvFilter};
        tracing_subscriber::registry()
            .with(fmt::layer())
            .with(
                EnvFilter::from_default_env()
                    .add_directive("chiral_network=info".parse().unwrap())
                    .add_directive("libp2p=warn".parse().unwrap())
                    .add_directive("libp2p_kad=warn".parse().unwrap())
                    .add_directive("libp2p_swarm=warn".parse().unwrap())
                    .add_directive("libp2p_mdns=warn".parse().unwrap()),
            )
            .init();
    }

    // Parse command line arguments
    use clap::Parser;
    let args = headless::CliArgs::parse();

    // If running in headless mode, don't start the GUI
    if args.headless {
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

    println!("Starting Chiral Network...");

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            geth: Mutex::new(GethProcess::new()),
            downloader: Arc::new(GethDownloader::new()),
            miner_address: Mutex::new(None),
            active_account: Arc::new(Mutex::new(None)),
            active_account_private_key: Arc::new(Mutex::new(None)),
            rpc_url: Mutex::new("http://127.0.0.1:8545".to_string()),
            dht: Mutex::new(None),
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

            // Initialize transaction queue
            transaction_queue: Arc::new(Mutex::new(VecDeque::new())),
            transaction_processor: Mutex::new(None),
            processing_transaction: Arc::new(Mutex::new(false)),

            // Initialize upload sessions
            upload_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Initialize proxy authentication tokens
            proxy_auth_tokens: Arc::new(Mutex::new(std::collections::HashMap::new())),

            // Initialize stream authentication
            stream_auth: Arc::new(Mutex::new(crate::stream_auth::StreamAuthService::new())),

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
        })
        .invoke_handler(tauri::generate_handler![
            create_chiral_account,
            import_chiral_account,
            has_active_account,
            get_user_balance,
            can_afford_download,
            process_download_payment,
            record_download_payment,
            record_seeder_payment,
            check_payment_notifications,
            get_network_peer_count,
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
            get_cpu_temperature,
            is_geth_running,
            check_geth_binary,
            get_geth_status,
            download_geth_binary,
            set_miner_address,
            start_miner,
            stop_miner,
            get_miner_status,
            get_miner_hashrate,
            get_current_block,
            get_network_stats,
            get_miner_logs,
            get_miner_performance,
            get_blocks_mined,
            get_recent_mined_blocks_pub,
            get_cpu_temperature,
            start_dht_node,
            stop_dht_node,
            stop_publishing_file,
            search_file_metadata,
            get_file_seeders,
            connect_to_peer,
            get_dht_events,
            detect_locale,
            get_default_storage_path,
            check_directory_exists,
            get_dht_health,
            get_dht_peer_count,
            get_dht_peer_id,
            get_dht_connected_peers,
            send_dht_message,
            start_file_transfer_service,
            download_file_from_network,
            upload_file_to_network,
            download_blocks_from_network,
            start_multi_source_download,
            cancel_multi_source_download,
            get_multi_source_progress,
            update_proxy_latency,
            get_proxy_optimization_status,
            download_file_multi_source,
            get_file_transfer_events,
            write_file,
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
            report_malicious_peer,
            select_peers_with_strategy,
            set_peer_encryption_support,
            cleanup_inactive_peers,
            upload_versioned_file,
            get_file_versions_by_name,
            test_backend_connection,
            establish_webrtc_connection,
            send_webrtc_file_request,
            get_webrtc_connection_status,
            disconnect_from_peer,
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
            save_temp_file_for_upload,
            get_file_size,
            encrypt_file_for_self_upload,
            encrypt_file_for_recipient,
            //request_file_access,
            upload_and_publish_file,
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
            send_chat_message,
            store_file_data,
            start_proof_of_storage_watcher,
            stop_proof_of_storage_watcher,
            get_relay_reputation_stats,
            set_relay_alias,
            get_relay_alias,
            get_multiaddresses
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

            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
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
                window.show().unwrap();
                window.set_focus().unwrap();

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

// #[tauri::command]
// async fn request_file_access(
//     state: State<'_, AppState>,
//     seeder_peer_id: String,
//     merkle_root: String,
// ) -> Result<String, String> {
//     let dht = state.dht.lock().await.as_ref().cloned().ok_or("DHT not running")?;
//
//     // 1. Get own public key
//     let private_key_hex = state
//         .active_account_private_key
//         .lock()
//         .await
//         .clone()
//         .ok_or("No active account to derive public key from.")?;
//     let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x")).map_err(|_| "Invalid private key format")?;
//     let secret_key = StaticSecret::from(<[u8; 32]>::try_from(pk_bytes).map_err(|_| "Private key is not 32 bytes")?);
//     let public_key = PublicKey::from(&secret_key);
//
//     // 2. Parse seeder peer id
//     let seeder = seeder_peer_id.parse().map_err(|_| "Invalid seeder peer ID")?;
//
//     // 3. Call the new DHT service method
//     let bundle = dht.request_aes_key(seeder, merkle_root, public_key).await?;
//
//     // 4. Serialize the bundle to send to the frontend
//     serde_json::to_string(&bundle).map_err(|e| e.to_string())
// }

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
    version: u32,
}

#[tauri::command]
async fn upload_and_publish_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
    file_name: Option<String>,
    recipient_public_key: Option<String>,
) -> Result<UploadResult, String> {
    // 1. Process file with ChunkManager (encrypt, chunk, build Merkle tree)
    let manifest = encrypt_file_for_recipient(
        app.clone(),
        state.clone(),
        file_path.clone(),
        recipient_public_key.clone(),
    )
    .await?;

    // 2. Get file name and size
    let file_name = file_name.unwrap_or_else(|| {
        std::path::Path::new(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let file_size: u64 = manifest.chunks.iter().map(|c| c.size as u64).sum();

    // 3. Get peer ID from DHT
    let peer_id = {
        let dht_guard = state.dht.lock().await;
        if let Some(dht) = dht_guard.as_ref() {
            dht.get_peer_id().await
        } else {
            "unknown".to_string()
        }
    };

    // 4. Publish to DHT with versioning support
    let dht = {
        let dht_guard = state.dht.lock().await; // Use the Merkle root as the file hash
        dht_guard.as_ref().cloned()
    };

    let version = if let Some(dht) = dht {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Use prepare_versioned_metadata to handle version incrementing and parent_hash
        let mime_type = detect_mime_type_from_filename(&file_name)
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let metadata = dht
            .prepare_versioned_metadata(
                manifest.merkle_root.clone(), // This is the Merkle root
                file_name.clone(),
                file_size,
                vec![], // Empty - chunks already stored
                created_at,
                Some(mime_type),
                None,                            // encrypted_key_bundle
                true,                            // is_encrypted
                Some("AES-256-GCM".to_string()), // Encryption method
                None,                            // key_fingerprint (deprecated)
                None,                            // price
                None,                            // uploader_address
            )
            .await?;

        println!("📦 BACKEND: Created versioned metadata");

        let version = metadata.version.unwrap_or(1);

        // Store file data locally for seeding (CRITICAL FIX)
        let ft = {
            let ft_guard = state.file_transfer.lock().await;
            ft_guard.as_ref().cloned()
        };
        if let Some(ft) = ft {
            // Read the original file data to store locally
            let file_data = tokio::fs::read(&file_path)
                .await
                .map_err(|e| format!("Failed to read file for local storage: {}", e))?;

            ft.store_file_data(manifest.merkle_root.clone(), file_name.clone(), file_data)
                .await; // Store with Merkle root as key
        }

        dht.publish_file(metadata).await?;
        version
    } else {
        1 // Default to v1 if DHT not running
    };

    // 5. Return metadata to frontend
    Ok(UploadResult {
        merkle_root: manifest.merkle_root,
        file_name,
        file_size,
        is_encrypted: true,
        peer_id,
        version,
    })
}

// async fn break_into_chunks(
//     app: tauri::AppHandle,
//     state: State<'_, AppState>,
//     file_path: String,
// ) -> Result<FileManifestForJs, String> {
//     // Get the app data directory for chunk storage
//     let app_data_dir = app
//         .path()
//         .app_data_dir()
//         .map_err(|e| format!("Could not get app data directory: {}", e))?;
//     let chunk_storage_path = app_data_dir.join("chunk_storage");

//     // Use the active user's own public key
//     let private_key_hex = state
//         .active_account_private_key
//         .lock()
//         .await
//         .clone()
//         .ok_or("No account is currently active. Please log in.")?;
//     let pk_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
//         .map_err(|_| "Invalid private key format".to_string())?;
//     let secret_key = StaticSecret::from(
//         <[u8; 32]>::try_from(pk_bytes).map_err(|_| "Private key is not 32 bytes")?,
//     );
//     PublicKey::from(&secret_key);

//     // Run the encryption in a blocking task to avoid blocking the async runtime
//     tokio::task::spawn_blocking(move || {
//         // Initialize ChunkManager with proper app data directory
//         let manager = ChunkManager::new(chunk_storage_path);

//         // Call the existing backend function to perform the encryption with recipient's public key
//         let manifest = manager.chunk_and_encrypt_file(Path::new(&file_path), &recipient_pk)?;

//         // Serialize the key bundle to a JSON string so it can be sent to the frontend easily.
//         let bundle_json =
//             serde_json::to_string(&manifest.encrypted_key_bundle).map_err(|e| e.to_string())?;

//         Ok(FileManifestForJs {
//             merkle_root: manifest.merkle_root,
//             chunks: manifest.chunks,
//             encrypted_key_bundle: bundle_json,
//         })
//     })
//     .await
//     .map_err(|e| format!("Encryption task failed: {}", e))?
// }

#[tauri::command]
async fn has_active_account(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.active_account.lock().await.is_some())
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

#[derive(serde::Serialize, Clone)]
struct ChatMessageForFrontend {
    from_peer_id: String,
    message_id: String,
    encrypted_payload: Vec<u8>,
    timestamp: u64,
    signature: Vec<u8>,
}

/// Sends an E2EE chat message to a peer.
#[tauri::command]
async fn send_chat_message(
    state: State<'_, AppState>,
    recipient_peer_id: String,
    encrypted_payload: Vec<u8>,
    signature: Vec<u8>,
) -> Result<(), String> {
    debug!("send_chat_message called for peer: {}", recipient_peer_id);
    let webrtc = state
        .webrtc
        .lock()
        .await
        .as_ref()
        .cloned()
        .ok_or("WebRTC service not running")?;

    // 1. Check if a WebRTC connection already exists.
    if !webrtc.get_connection_status(&recipient_peer_id).await {
        debug!(
            "No existing WebRTC connection to {}. Attempting to establish one.",
            recipient_peer_id
        );
        let dht = state
            .dht
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or("DHT service not running")?;

        dht.connect_to_peer_by_id(recipient_peer_id.clone()).await?;

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        if !webrtc.get_connection_status(&recipient_peer_id).await {
            error!(
                "Failed to establish WebRTC connection with peer {} after 5s.",
                recipient_peer_id
            );
            return Err("Failed to establish WebRTC connection with peer.".to_string());
        }
        debug!(
            "WebRTC connection to {} established successfully.",
            recipient_peer_id
        );
    }

    // 3. Construct the message payload.
    let chat_message = webrtc_service::WebRTCChatMessage {
        message_id: format!(
            "msg_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ),
        encrypted_payload,
        timestamp: chrono::Utc::now().timestamp() as u64,
        signature,
    };
    let message = webrtc_service::WebRTCMessage::ChatMessage(chat_message);
    let message_bytes = serde_json::to_vec(&message)
        .map_err(|e| format!("Failed to serialize chat message: {}", e))?;
    debug!("Sending chat message to {}", recipient_peer_id);
    // Correctly serialize the message to a string before sending via send_text
    let message_str = serde_json::to_string(&message)
        .map_err(|e| format!("Failed to serialize chat message to string: {}", e))?;

    // Assuming send_data is a method that sends text. If it sends bytes, use message_bytes.
    webrtc.send_data(&recipient_peer_id, message_bytes).await
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

// --- New: Proof-of-Storage watcher commands & task ----------------------------------
//
// Summary of additions:
// - start_proof_of_storage_watcher(contract_address, poll_interval_secs, response_timeout_secs)
//      stores contract address in AppState and spawns a background task to watch for challenges
// - stop_proof_of_storage_watcher() stops the background task if running
//
// The background task is a skeleton showing:
//  - how to fetch challenges (TODO: integrate with your ethereum module / event subscription)
//  - how to locate requested chunk (TODO: use your ChunkManager/FileTransferService)
//  - how to generate Merkle proof (TODO: call your Merkle helper)
//  - how to submit proof to contract (TODO: call ethereum::verify_proof or similar)
//  - timeout handling for missed responses
//
// The TODO markers indicate where to plug in concrete project functions.

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
        match tokio::time::timeout(TokioDuration::from_secs(2), handle).await {
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
