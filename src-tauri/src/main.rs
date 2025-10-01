#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

pub mod commands;

mod dht;
mod manager;
mod encryption;
mod ethereum;
mod file_transfer;
mod geth_downloader;
mod webrtc_service;
mod headless;
mod keystore;
mod pool;
pub mod net;
mod peer_selection;
mod transaction_services;
use crate::commands::proxy::{
    list_proxies, proxy_connect, proxy_disconnect, proxy_echo, ProxyNode,
};
use crate::commands::transaction_commands::{
    broadcast_transaction, estimate_transaction, get_address_nonce, get_network_gas_price,
    get_network_status, get_transaction_history, get_transaction_status,
};
use dht::{DhtEvent, DhtMetricsSnapshot, DhtService, FileMetadata};
use ethereum::{
    create_new_account, get_account_from_private_key, get_balance, get_block_number, get_hashrate,
    get_mined_blocks_count, get_mining_logs, get_mining_performance, get_mining_status,
    get_network_difficulty, get_network_hashrate, get_peer_count, get_recent_mined_blocks,
    start_mining, stop_mining, EthAccount, GethProcess, MinedBlock,
};
use file_transfer::{DownloadMetricsSnapshot, FileTransferEvent, FileTransferService};
use webrtc_service::{WebRTCService, WebRTCFileRequest};
use fs2::available_space;
use geth_downloader::GethDownloader;
use keystore::Keystore;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{
    io::{BufRead, BufReader},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Components, System, MINIMUM_CPU_UPDATE_INTERVAL};
use systemstat::{Platform, System as SystemStat};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};
use tokio::{sync::Mutex, task::JoinHandle, time::sleep};
use totp_rs::{Algorithm, Secret, TOTP};
use tracing::{info, warn};

#[derive(Clone)]
struct QueuedTransaction {
    id: String,
    to_address: String,
    amount: f64,
    timestamp: u64,
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
    proxies: Arc<Mutex<Vec<ProxyNode>>>,
    file_transfer_pump: Mutex<Option<JoinHandle<()>>>,
    socks5_proxy_cli: Mutex<Option<String>>,
    
    // New fields for transaction queue
    transaction_queue: Arc<Mutex<VecDeque<QueuedTransaction>>>,
    transaction_processor: Mutex<Option<JoinHandle<()>>>,
    processing_transaction: Arc<Mutex<bool>>,
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
    state: State<'_, AppState>
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
async fn start_geth_node(state: State<'_, AppState>, data_dir: String) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    let miner_address = state.miner_address.lock().await;
    // TODO: The port and address should be configurable from the frontend.
    // For now, we'll update the rpc_url in the state when starting.
    let rpc_url = "http://127.0.0.1:8545".to_string();
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
    file_name: String
) -> Result<Vec<FileMetadata>, String> {
    let dht = { state.dht.lock().await.as_ref().cloned() };
    if let Some(dht) = dht {
        (*dht).get_versions_by_file_name(file_name).await
    } else {
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
        webrtc.establish_connection_with_answer(peer_id, offer).await
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
async fn upload_versioned_file(
    state: State<'_, AppState>,
    file_name: String,
    file_path: String,
    file_size: u64,
    mime_type: Option<String>,
    is_encrypted: bool,
    encryption_method: Option<String>,
    key_fingerprint: Option<String>,
) -> Result<FileMetadata, String> {
    let dht_opt = { state.dht.lock().await.as_ref().cloned() };
    if let Some(dht) = dht_opt {
        // --- FIX: Calculate file_hash using file_transfer helper
        let file_data = tokio::fs::read(&file_path).await.map_err(|e| e.to_string())?;
        let file_hash = FileTransferService::calculate_file_hash(&file_data);

        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Use the DHT versioning helper to fill in parent_hash/version
        let metadata = dht
            .prepare_versioned_metadata(
                file_hash.clone(),
                file_name,
                file_data.len() as u64,  // Use file size directly from data
                created_at,
                mime_type,
                is_encrypted,
                encryption_method,
                key_fingerprint,
            )
            .await?;

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

#[tauri::command]
async fn get_blocks_mined(address: String) -> Result<u64, String> {
    get_mined_blocks_count(&address).await
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
) -> Result<String, String> {
    {
        let dht_guard = state.dht.lock().await;
        if dht_guard.is_some() {
            return Err("DHT node is already running".to_string());
        }
    }

    // Disable autonat by default to prevent warnings when no servers are available
    // Users can explicitly enable it when needed
    let auto_enabled = enable_autonat.unwrap_or(false);
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

    let dht_service = DhtService::new(
        port,
        bootstrap_nodes,
        None,
        false,
        auto_enabled,
        probe_interval,
        autonat_server_list,
        final_proxy_address,
        file_transfer_service,
    )
    .await
    .map_err(|e| format!("Failed to start DHT: {}", e))?;

    let peer_id = dht_service.get_peer_id().await;

    // Start the DHT node running in background
    dht_service.run().await;
    let dht_arc = Arc::new(dht_service);

    // Spawn the event pump
    let app_handle = app.clone();
    let proxies_arc = state.proxies.clone();
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
                    DhtEvent::ProxyStatus {
                        id,
                        address,
                        status,
                        latency_ms,
                        error,
                    } => {
                        let mut proxies = proxies_arc.lock().await;

                        // 1) Find existing proxy by id, then by address, then by address==id
                        let mut idx = proxies.iter().position(|p| p.id == id);
                        if idx.is_none() && !address.is_empty() {
                            idx = proxies.iter().position(|p| p.address == address);
                        }
                        if idx.is_none() {
                            idx = proxies.iter().position(|p| p.address == id);
                        }

                        if let Some(i) = idx {
                            // 2) Safe merge: only overwrite with incoming values
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
                            let _ = app_handle.emit("proxy_status_update", p.clone());
                        } else {
                            // 3) If not found, add new (if address is empty, use id instead)
                            let new_node = ProxyNode {
                                id: id.clone(),
                                address: if address.is_empty() {
                                    id.clone()
                                } else {
                                    address.clone()
                                },
                                status,
                                latency: latency_ms.unwrap_or(0) as u32,
                                error,
                            };
                            proxies.push(new_node.clone());
                            let _ = app_handle.emit("proxy_status_update", new_node);
                        }
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

                        // If recieved an echo, mark the node as online in the proxy list
                        {
                            let mut proxies = proxies_arc.lock().await;
                            if let Some(p) = proxies.iter_mut().find(|p| p.id == from) {
                                p.status = "online".to_string();
                                let _ = app_handle.emit("proxy_status_update", p.clone());
                            } else {
                                let new_node = ProxyNode {
                                    id: from.clone(),
                                    address: String::new(),
                                    status: "online".to_string(),
                                    latency: 0,
                                    error: None,
                                };
                                proxies.push(new_node.clone());
                                let _ = app_handle.emit("proxy_status_update", new_node);
                            }
                        }
                    }
                    DhtEvent::PeerRtt { peer, rtt_ms } => {
                        let mut proxies = proxies_arc.lock().await;
                        if let Some(p) = proxies.iter_mut().find(|p| p.id == peer) {
                            p.latency = rtt_ms as u32;
                            let _ = app_handle.emit("proxy_status_update", p.clone());
                        }
                    }
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
async fn stop_dht_node(state: State<'_, AppState>) -> Result<(), String> {
    let dht = {
        let mut dht_guard = state.dht.lock().await;
        dht_guard.take()
    };

    if let Some(dht) = dht {
        dht.shutdown()
            .await
            .map_err(|e| format!("Failed to stop DHT: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
async fn publish_file_metadata(
    state: State<'_, AppState>,
    file_hash: String,
    file_name: String,
    file_size: u64,
    mime_type: Option<String>,
) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let metadata = FileMetadata {
            file_hash,
            file_name,
            file_size,
            seeders: vec![],
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            mime_type,
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            parent_hash: None,
            version: Some(1),
        };

        dht.publish_file(metadata).await
    } else {
        Err("DHT node is not running".to_string())
    }
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
async fn send_dht_message(
    state: State<'_, AppState>,
    peer_id: String,
    message: serde_json::Value
) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        // Send message through DHT to target peer
        dht.send_message_to_peer(&peer_id, message).await
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
                DhtEvent::PeerDiscovered(p) => format!("peer_discovered:{}", p),
                DhtEvent::PeerConnected(p) => format!("peer_connected:{}", p),
                DhtEvent::PeerDisconnected(p) => format!("peer_disconnected:{}", p),
                DhtEvent::FileDiscovered(meta) => format!(
                    "file_discovered:{}:{}:{}",
                    meta.file_hash, meta.file_name, meta.file_size
                ),
                DhtEvent::FileNotFound(hash) => format!("file_not_found:{}", hash),
                DhtEvent::Error(err) => format!("error:{}", err),
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
            })
            .collect();
        Ok(mapped)
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
fn get_cpu_temperature() -> Option<f32> {
    static mut LAST_UPDATE: Option<Instant> = None;
    unsafe {
        if let Some(last) = LAST_UPDATE {
            if last.elapsed() < MINIMUM_CPU_UPDATE_INTERVAL {
                return None;
            }
        }
        LAST_UPDATE = Some(Instant::now());
    }

    // Try sysinfo first (works on some platforms including M1 macs and some Windows)
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
        return Some(sum / core_count as f32);
    }

    // Windows-specific temperature detection methods
    #[cfg(target_os = "windows")]
    {
        if let Some(temp) = get_windows_temperature() {
            return Some(temp);
        }
    }

    // Linux-specific temperature detection methods
    #[cfg(target_os = "linux")]
    {
        if let Some(temp) = get_linux_temperature() {
            return Some(temp);
        }
    }

    // Fallback for other platforms
    let stat_sys = SystemStat::new();
    if let Ok(temp) = stat_sys.cpu_temp() {
        return Some(temp);
    }

    None
}

#[cfg(target_os = "windows")]
fn get_windows_temperature() -> Option<f32> {
    use std::process::Command;

    // Method 1: Try the fastest method first - HighPrecisionTemperature from WMI
    if let Ok(output) = Command::new("powershell")
        .args([
            "-Command",
            "Get-WmiObject -Query \"SELECT HighPrecisionTemperature FROM Win32_PerfRawData_Counters_ThermalZoneInformation\" | Select-Object -First 1 -ExpandProperty HighPrecisionTemperature"
        ])
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            if let Ok(temp_tenths_kelvin) = output_str.trim().parse::<f32>() {
                let temp_celsius = (temp_tenths_kelvin / 10.0) - 273.15;
                if temp_celsius > 0.0 && temp_celsius < 150.0 {
                    return Some(temp_celsius);
                }
            }
        }
    }

    // Method 2: Fallback to regular Temperature field
    if let Ok(output) = Command::new("powershell")
        .args([
            "-Command",
            "Get-WmiObject -Query \"SELECT Temperature FROM Win32_PerfRawData_Counters_ThermalZoneInformation\" | Select-Object -First 1 -ExpandProperty Temperature"
        ])
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            if let Ok(temp_tenths_kelvin) = output_str.trim().parse::<f32>() {
                let temp_celsius = (temp_tenths_kelvin / 10.0) - 273.15;
                if temp_celsius > 0.0 && temp_celsius < 150.0 {
                    return Some(temp_celsius);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn get_linux_temperature() -> Option<f32> {
    use std::fs;

    // Method 1: Try sensors command first (most reliable and matches user expectations)
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

    // Method 2: Try thermal zones (fallback)
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
                            return Some(temp_celsius);
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
                            return Some(temp_celsius);
                        }
                    }
                }
            }
        }
    }

    // Method 3: Try hwmon (hardware monitoring) interfaces
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
                                return Some(temp_celsius);
                            }
                        }
                    }
                }
            }
        }
    }

    // Method 4: Try reading from specific CPU temperature files
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
                                return Some(temp_celsius);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

#[tauri::command]
fn detect_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".into())
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

    let file_transfer_service = FileTransferService::new()
        .await
        .map_err(|e| format!("Failed to start file transfer service: {}", e))?;

    let ft_arc = Arc::new(file_transfer_service);
    {
        let mut ft_guard = state.file_transfer.lock().await;
        *ft_guard = Some(ft_arc.clone());
    }

    // Initialize WebRTC service with file transfer service
    let webrtc_service = WebRTCService::new(ft_arc.clone())
        .await
        .map_err(|e| format!("Failed to start WebRTC service: {}", e))?;

    let webrtc_arc = Arc::new(webrtc_service);
    {
        let mut webrtc_guard = state.webrtc.lock().await;
        *webrtc_guard = Some(webrtc_arc.clone());
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
) -> Result<FileMetadata, String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().await;
        ft_guard.as_ref().cloned()
    };

    if let Some(ft) = ft {
        // Upload the file
        let file_name = file_path.split('/').last().unwrap_or(&file_path);

        ft.upload_file(file_path.clone(), file_name.to_string())
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
                file_hash: file_hash.clone(),
                file_name: file_name.to_string(),
                file_size: file_data.len() as u64,
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
            };

            if let Err(e) = dht.publish_file(metadata.clone()).await {
                warn!("Failed to publish file metadata to DHT: {}", e);
            }

            return Ok(metadata);
        }

        Err("DHT Service not running".to_string())
    } else {
        Err("File transfer service is not running".to_string())
    }
}

#[tauri::command]
async fn download_file_from_network(
    state: State<'_, AppState>,
    file_hash: String,
    output_path: String,
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
            match dht_service.search_metadata(file_hash.clone(), 5000).await {
                Ok(Some(metadata)) => {
                    info!("Found file metadata in DHT: {} (size: {} bytes)",
                          metadata.file_name, metadata.file_size);

                    // Implement peer discovery for file chunks
                    info!("Discovering peers for file: {} with {} known seeders",
                          metadata.file_name, metadata.seeders.len());

                    if metadata.seeders.is_empty() {
                        return Err(format!(
                            "No seeders available for file: {} ({})",
                            metadata.file_name, metadata.file_hash
                        ));
                    }

                    // Discover and verify available peers for this file
                    let available_peers = dht_service.discover_peers_for_file(&metadata).await
                        .map_err(|e| format!("Peer discovery failed: {}", e))?;

                    if available_peers.is_empty() {
                        info!("File found but no seeders currently available");
                        // TODO: Return metadata to frontend with 0 seeders instead of error
                        return Err(format!(
                            "File found but no seeders available: {} ({} bytes) - 0 seeders online",
                            metadata.file_name, metadata.file_size
                        ));
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
                            recommended.into_iter().next().unwrap_or_else(|| available_peers[0].clone())
                        };

                        info!("Selected peer {} for WebRTC download", selected_peer);

                        // Create WebRTC offer
                        match webrtc_service.create_offer(selected_peer.clone()).await {
                            Ok(offer) => {
                                info!("Created WebRTC offer for peer {}", selected_peer);

                                // Send WebRTC offer via DHT signaling
                                let offer_request = dht::WebRTCOfferRequest {
                                    offer_sdp: offer,
                                    file_hash: metadata.file_hash.clone(),
                                    requester_peer_id: dht_service.get_peer_id().await,
                                };

                                match dht_service.send_webrtc_offer(selected_peer.clone(), offer_request).await {
                                    Ok(answer_receiver) => {
                                        info!("Sent WebRTC offer to peer {}, waiting for answer", selected_peer);

                                        // Wait for WebRTC answer with timeout
                                        match tokio::time::timeout(Duration::from_secs(30), answer_receiver).await {
                                            Ok(Ok(Ok(answer_response))) => {
                                                info!("Received WebRTC answer from peer {}", selected_peer);

                                                // Establish WebRTC connection with the answer
                                                match webrtc_service.establish_connection_with_answer(
                                                    selected_peer.clone(),
                                                    answer_response.answer_sdp,
                                                ).await {
                                                    Ok(_) => {
                                                        info!("WebRTC connection established with peer {}", selected_peer);

                                                        // Send file request over WebRTC data channel
                                                        let file_request = crate::webrtc_service::WebRTCFileRequest {
                                                            file_hash: metadata.file_hash.clone(),
                                                            file_name: metadata.file_name.clone(),
                                                            file_size: metadata.file_size,
                                                            requester_peer_id: dht_service.get_peer_id().await,
                                                        };

                                                        match webrtc_service.send_file_request(selected_peer.clone(), file_request).await {
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
                                                        Err(format!("WebRTC connection failed: {}", e))
                                                    }
                                                }
                                            }
                                            Ok(Ok(Err(e))) => {
                                                warn!("WebRTC signaling failed: {}", e);
                                                Err(format!("WebRTC signaling failed: {}", e))
                                            }
                                            Ok(Err(_)) => {
                                                warn!("WebRTC answer receiver was canceled");
                                                Err("WebRTC answer receiver was canceled".to_string())
                                            }
                                            Err(_) => {
                                                warn!("WebRTC answer timeout from peer {}", selected_peer);
                                                Err(format!("WebRTC answer timeout from peer {}", selected_peer))
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
async fn upload_file_data_to_network(
    state: State<'_, AppState>,
    file_name: String,
    file_data: Vec<u8>,
    mime_type: Option<String>,
    is_encrypted: bool,
    encryption_method: Option<String>,
    key_fingerprint: Option<String>,
) -> Result<FileMetadata, String> {
    let dht_opt = { state.dht.lock().await.as_ref().cloned() };
    if let Some(dht) = dht_opt {
        // Calculate file hash from the data
        let file_hash = file_transfer::FileTransferService::calculate_file_hash(&file_data);

        // Store the file data directly in memory
        let file_size = file_data.len() as u64;
        let ft = {
            let ft_guard = state.file_transfer.lock().await;
            ft_guard.as_ref().cloned()
        };

        if let Some(ft) = ft {
            ft.store_file_data(file_hash.clone(), file_name.clone(), file_data)
                .await;
        }

        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Use the DHT versioning helper to fill in parent_hash/version
        let metadata = dht
            .prepare_versioned_metadata(
                file_hash.clone(),
                file_name,
                file_size,
                created_at,
                mime_type,
                is_encrypted,
                encryption_method,
                key_fingerprint,
            )
            .await?;

        dht.publish_file(metadata.clone()).await?;
        Ok(metadata)
    } else {
        Err("DHT not running".into())
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
) -> Result<Option<FileMetadata>, String> {
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
fn get_available_storage() -> f64 {
    let storage = available_space(Path::new("/")).unwrap_or(0);
    (storage as f64 / 1024.0 / 1024.0 / 1024.0).floor()
}

const DEFAULT_GETH_DATA_DIR: &str = "./bin/geth-data";

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
    // This action is protected by `with2FA` on the frontend, so we can assume
    // the user has already been verified via `verify_totp_code`.
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
async fn select_peers_with_strategy(
    state: State<'_, AppState>,
    available_peers: Vec<String>,
    count: usize,
    strategy: String,
    require_encryption: bool,
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

    let dht_guard = state.dht.lock().await;
    if let Some(ref dht) = *dht_guard {
        Ok(dht
            .select_peers_with_strategy(
                &available_peers,
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
        key_guard.clone().ok_or("No private key available. Please log in again.")?
    };
    
    let tx_hash = ethereum::send_transaction(
        &account,
        &to_address,
        amount,
        &private_key
    ).await?;
    
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
    let _account = get_active_account(&state).await?;
    
    // Generate unique transaction ID
    let tx_id = format!("tx_{}", SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis());
    
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
                    active_key_arc
                ).await;
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
                    ).await {
                        Ok(tx_hash) => {
                            // Success - emit event
                            let _ = app.emit("transaction_sent", serde_json::json!({
                                "id": tx.id,
                                "txHash": tx_hash,
                                "to": tx.to_address,
                                "amount": tx.amount,
                            }));
                            
                            // Wait a bit before processing next (to ensure nonce increments)
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                        Err(e) => {
                            // Error - emit event
                            warn!("Transaction failed: {}", e);
                            let _ = app.emit("transaction_failed", serde_json::json!({
                                "id": tx.id,
                                "error": e,
                                "to": tx.to_address,
                                "amount": tx.amount,
                            }));
                        }
                    }
                }
                _ => {
                    // No account or private key - user logged out
                    warn!("Cannot process transaction - user logged out");
                    let _ = app.emit("transaction_failed", serde_json::json!({
                        "id": tx.id,
                        "error": "User logged out",
                        "to": tx.to_address,
                        "amount": tx.amount,
                    }));
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
                    .add_directive("libp2p=debug".parse().unwrap())
                    .add_directive("libp2p_kad=debug".parse().unwrap())
                    .add_directive("libp2p_swarm=debug".parse().unwrap()),
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
    tracing::info!("🚀 Registering pool mining commands...");

    tauri::Builder::default()
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
            proxies: Arc::new(Mutex::new(Vec::new())),
            file_transfer_pump: Mutex::new(None),
            socks5_proxy_cli: Mutex::new(args.socks5_proxy),

            // Initialize transaction queue
            transaction_queue: Arc::new(Mutex::new(VecDeque::new())),
            transaction_processor: Mutex::new(None),
            processing_transaction: Arc::new(Mutex::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            create_chiral_account,
            import_chiral_account,
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
            publish_file_metadata,
            stop_publishing_file,
            search_file_metadata,
            connect_to_peer,
            get_dht_events,
            detect_locale,
            get_dht_health,
            get_dht_peer_count,
            get_dht_peer_id,
            get_dht_connected_peers,
            send_dht_message,
            start_file_transfer_service,
            upload_file_to_network,
            upload_file_data_to_network,
            download_file_from_network,
            get_file_transfer_events,
            get_download_metrics,
            encrypt_file_with_password,
            decrypt_file_with_password,
            encrypt_file_for_upload,
            show_in_folder,
            write_file,
            get_available_storage,
            proxy_connect,
            proxy_disconnect,
            proxy_echo,
            list_proxies,
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
            select_peers_with_strategy,
            set_peer_encryption_support,
            cleanup_inactive_peers,
            // Transaction commands
            broadcast_transaction,
            get_transaction_status,
            get_transaction_history,
            get_address_nonce,
            estimate_transaction,
            get_network_gas_price,
            get_network_status,
            upload_versioned_file,
            get_file_versions_by_name,
            establish_webrtc_connection,
            send_webrtc_file_request,
            get_webrtc_connection_status,
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
            println!("Cleaning up any orphaned geth processes from previous sessions...");
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

            println!("App setup complete");
            println!("Window should be visible now!");

            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
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
                println!("Window shown and focused");

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