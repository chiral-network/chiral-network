pub mod chain_rpc_api;
pub mod dht;
pub mod drive_api;
pub mod drive_storage;
mod encryption;
pub mod event_sink;
pub mod file_transfer;
pub mod geth;
pub mod geth_bootstrap;
pub mod hosting;
pub mod hosting_server;
pub mod rating_api;
pub mod rating_storage;
pub mod relay_share_proxy;
pub mod rpc_client;
mod speed_tiers;
pub mod wallet;
pub mod wallet_backup_api;

use dht::DhtService;
use encryption::EncryptionKeypair;
use file_transfer::FileTransferService;
use geth::{
    GethDownloader, GethProcess, GethStatus, GpuDevice, GpuMiningCapabilities, GpuMiningStatus,
    MiningStatus,
};
use geth_bootstrap::BootstrapHealthReport;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::Mutex;

pub struct AppState {
    pub dht: Arc<Mutex<Option<Arc<DhtService>>>>,
    pub file_transfer: Arc<Mutex<FileTransferService>>,
    pub file_storage: Arc<Mutex<HashMap<String, Vec<u8>>>>, // hash -> file data (for local caching)
    pub geth: Arc<Mutex<GethProcess>>,
    pub encryption_keypair: Arc<Mutex<Option<EncryptionKeypair>>>,
    pub tx_metadata: Arc<Mutex<HashMap<String, TransactionMeta>>>, // tx_hash -> metadata
    pub download_directory: Arc<Mutex<Option<String>>>, // custom download directory (None = system default)
    pub download_credentials: dht::DownloadCredentialsMap, // request_id -> wallet credentials for file payment
    // Hosting & Drive
    pub hosting_server_state: Arc<hosting_server::HostingServerState>,
    pub hosting_server_addr: Arc<Mutex<Option<std::net::SocketAddr>>>,
    pub hosting_server_shutdown: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    pub drive_state: Arc<drive_api::DriveState>,
    /// Active WebSocket tunnel tasks keyed by resource key (e.g. "site:abc123").
    /// Dropping the AbortHandle cancels the tunnel task.
    pub tunnel_handles: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>>,
}

/// Cleanup DHT seeder entries and host advertisement on shutdown.
/// Called from both the Tauri close handler and the SIGINT signal handler.
#[cfg(unix)]
async fn cleanup_dht_on_shutdown(dht_arc: &Arc<Mutex<Option<Arc<DhtService>>>>) {
    let dht_guard = dht_arc.lock().await;
    let dht = match dht_guard.as_ref() {
        Some(d) => d,
        None => return,
    };

    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    if peer_id.is_empty() {
        return;
    }

    // 1. Remove our peer_id from all shared file DHT records
    let shared = dht.get_shared_files();
    let file_hashes: Vec<String> = {
        let map = shared.lock().await;
        map.keys().cloned().collect()
    };
    println!(
        "🛑 Unpublishing {} shared files from DHT",
        file_hashes.len()
    );
    let mut count = 0u32;
    for file_hash in &file_hashes {
        let dht_key = format!("chiral_file_{}", file_hash);
        if let Ok(Some(json)) = dht.get_dht_value(dht_key.clone()).await {
            if let Ok(mut metadata) = serde_json::from_str::<FileMetadata>(&json) {
                metadata.seeders.retain(|s| s.peer_id != peer_id);
                if metadata.peer_id == peer_id {
                    metadata.peer_id = String::new();
                }
                if let Ok(updated_json) = serde_json::to_string(&metadata) {
                    let _ = dht.put_dht_value(dht_key, updated_json).await;
                    count += 1;
                }
            }
        }
    }
    println!("✅ Unpublished {} files from DHT", count);

    // 2. Remove our host advertisement from the registry
    let registry_key = "chiral_host_registry".to_string();
    if let Ok(Some(json)) = dht.get_dht_value(registry_key.clone()).await {
        let mut registry: Vec<HostRegistryEntry> = serde_json::from_str(&json).unwrap_or_default();
        registry.retain(|e| e.peer_id != peer_id);
        if let Ok(registry_json) = serde_json::to_string(&registry) {
            let _ = dht.put_dht_value(registry_key, registry_json).await;
        }
    }
    println!("✅ Unpublished host advertisement from DHT");
}

async fn start_dht_internal(
    app: tauri::AppHandle,
    state: &AppState,
    allow_already_running: bool,
) -> Result<String, String> {
    let mut dht_guard = state.dht.lock().await;

    if dht_guard.is_some() {
        if allow_already_running {
            return Ok("DHT already running".to_string());
        }
        return Err("DHT already running".to_string());
    }

    let dht = Arc::new(DhtService::new(
        state.file_transfer.clone(),
        state.download_directory.clone(),
        state.download_credentials.clone(),
    ));
    let app_for_delayed_reseed = app.clone();
    let result = dht.start(app).await?;
    *dht_guard = Some(dht);
    drop(dht_guard);

    // Ensure latest Drive manifest is in-memory before reseed to avoid startup races.
    state.drive_state.load_from_disk_async().await;

    // Restore persisted Drive seeding registrations (root + nested folders)
    // as soon as DHT comes online so "seeding=true" reflects actual availability.
    auto_reseed_drive_files(state).await;

    // Run follow-up reseed passes after startup to absorb timing races between
    // bootstrap, relay reservation, and Drive manifest load/refresh.
    tauri::async_runtime::spawn(async move {
        // Use staggered retries over an extended startup window so reseed
        // converges even when relay/bootstrap connectivity is delayed.
        let retry_delays_secs = [2u64, 5, 10, 20, 30, 45, 60, 90, 120, 180, 240, 300];
        for delay_secs in retry_delays_secs {
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;

            let app_state = app_for_delayed_reseed.state::<AppState>();
            let dht_running = {
                let dht_guard = app_state.dht.lock().await;
                dht_guard.is_some()
            };
            if !dht_running {
                break;
            }

            app_state.drive_state.load_from_disk_async().await;
            auto_reseed_drive_files(app_state.inner()).await;
        }
    });

    Ok(result)
}

#[tauri::command]
async fn start_dht(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    start_dht_internal(app, state.inner(), false).await
}

fn compute_sha256_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|e| {
        format!(
            "Failed to open file for hashing '{}': {}",
            path.display(),
            e
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| {
            format!(
                "Failed to read file for hashing '{}': {}",
                path.display(),
                e
            )
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn auto_reseed_drive_files(state: &AppState) {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };
    let Some(dht) = dht else {
        return;
    };

    let files_dir = match ds::drive_files_dir() {
        Some(dir) => dir,
        None => {
            println!("[DRIVE] Cannot determine drive files directory for auto-reseed");
            return;
        }
    };

    let candidates = {
        let manifest = state.drive_state.manifest.read().await;
        manifest
            .items
            .iter()
            .filter(|item| item.item_type == "file" && (item.seed_enabled || item.seeding))
            .filter_map(|item| {
                let storage_path = item.storage_path.clone()?;
                Some((
                    item.id.clone(),
                    item.owner.clone(),
                    item.name.clone(),
                    storage_path,
                    item.size,
                    item.merkle_root.clone(),
                    item.protocol.clone(),
                    item.price_chi.clone(),
                ))
            })
            .collect::<Vec<_>>()
    };

    if candidates.is_empty() {
        return;
    }

    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    if peer_id.is_empty() {
        println!(
            "[DRIVE] Auto-reseed: peer ID unavailable; registering local seeds and deferring DHT metadata publish"
        );
    }

    let our_multiaddrs = if peer_id.is_empty() {
        Vec::new()
    } else {
        dht.get_listening_addresses().await
    };
    let mut hash_updates: Vec<(String, String)> = Vec::new();
    let mut attempted_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut activated_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut disabled_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut reseeded_local = 0usize;
    let mut reseeded_dht_metadata = 0usize;

    for (
        item_id,
        owner,
        file_name,
        storage_path,
        file_size_hint,
        existing_merkle_root,
        protocol,
        price_chi,
    ) in candidates
    {
        attempted_ids.insert(item_id.clone());
        let full_path = files_dir.join(&storage_path);
        if !full_path.exists() {
            println!(
                "[DRIVE] Auto-reseed skipped missing file: {} ({})",
                file_name,
                full_path.display()
            );
            disabled_ids.insert(item_id.clone());
            continue;
        }

        let file_size = match file_size_hint {
            Some(sz) if sz > 0 => sz,
            _ => match std::fs::metadata(&full_path) {
                Ok(meta) => meta.len(),
                Err(e) => {
                    println!(
                        "[DRIVE] Auto-reseed failed to stat {}: {}",
                        full_path.display(),
                        e
                    );
                    continue;
                }
            },
        };

        let file_hash = match existing_merkle_root
            .clone()
            .filter(|h| !h.trim().is_empty())
        {
            Some(root) => root,
            None => match compute_sha256_file(&full_path) {
                Ok(hash) => {
                    hash_updates.push((item_id.clone(), hash.clone()));
                    hash
                }
                Err(e) => {
                    println!("[DRIVE] Auto-reseed failed to hash {}: {}", file_name, e);
                    continue;
                }
            },
        };

        let price_wei = match price_chi.as_deref() {
            Some(price) if !price.trim().is_empty() && price.trim() != "0" => {
                match wallet::parse_chi_to_wei(price) {
                    Ok(wei) => wei,
                    Err(e) => {
                        println!(
                            "[DRIVE] Auto-reseed invalid price for {} ({}): {}",
                            file_name, price, e
                        );
                        continue;
                    }
                }
            }
            _ => 0u128,
        };
        let wallet_addr = owner.trim().to_string();
        if price_wei > 0 && wallet_addr.is_empty() {
            println!(
                "[DRIVE] Auto-reseed skipped paid file with missing owner wallet: {}",
                file_name
            );
            continue;
        }

        dht.register_shared_file(
            file_hash.clone(),
            full_path.to_string_lossy().to_string(),
            file_name.clone(),
            file_size,
            price_wei,
            wallet_addr.clone(),
        )
        .await;

        activated_ids.insert(item_id.clone());
        reseeded_local += 1;

        // Keep local reseeding instant even when DHT identity/bootstrap is not
        // ready yet. Metadata will be refreshed on follow-up reseed passes.
        if peer_id.is_empty() {
            continue;
        }

        let dht_key = format!("chiral_file_{}", file_hash);
        let mut metadata = match dht.get_dht_value(dht_key.clone()).await {
            Ok(Some(json)) => {
                serde_json::from_str::<FileMetadata>(&json).unwrap_or_else(|_| FileMetadata {
                    hash: file_hash.clone(),
                    file_name: file_name.clone(),
                    file_size,
                    protocol: protocol.clone().unwrap_or_else(|| "WebRTC".to_string()),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    peer_id: String::new(),
                    price_wei: String::new(),
                    wallet_address: String::new(),
                    seeders: Vec::new(),
                publisher_signature: String::new(),
                })
            }
            _ => FileMetadata {
                hash: file_hash.clone(),
                file_name: file_name.clone(),
                file_size,
                protocol: protocol.clone().unwrap_or_else(|| "WebRTC".to_string()),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                peer_id: String::new(),
                price_wei: String::new(),
                wallet_address: String::new(),
                seeders: Vec::new(),
                publisher_signature: String::new(),
            },
        };

        let our_seeder = SeederInfo {
            peer_id: peer_id.clone(),
            price_wei: price_wei.to_string(),
            wallet_address: wallet_addr.clone(),
            multiaddrs: our_multiaddrs.clone(),
            signature: String::new(),
        };
        if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
            existing.price_wei = our_seeder.price_wei.clone();
            existing.wallet_address = our_seeder.wallet_address.clone();
            existing.multiaddrs = our_seeder.multiaddrs.clone();
        } else {
            metadata.seeders.push(our_seeder);
        }
        metadata.peer_id = peer_id.clone();
        metadata.price_wei = price_wei.to_string();
        metadata.wallet_address = wallet_addr;

        let metadata_json = match serde_json::to_string(&metadata) {
            Ok(json) => json,
            Err(e) => {
                println!(
                    "[DRIVE] Auto-reseed failed to serialize metadata for {}: {}",
                    file_name, e
                );
                continue;
            }
        };
        if let Err(e) = dht.put_dht_value(dht_key, metadata_json).await {
            println!(
                "[DRIVE] Auto-reseed failed to publish metadata for {}: {}",
                file_name, e
            );
            continue;
        }

        reseeded_dht_metadata += 1;
    }

    let mut manifest_changed = false;
    if !hash_updates.is_empty() || !attempted_ids.is_empty() {
        let mut manifest = state.drive_state.manifest.write().await;
        let now = ds::now_secs();
        for item in manifest
            .items
            .iter_mut()
            .filter(|i| i.item_type == "file" && attempted_ids.contains(&i.id))
        {
            if disabled_ids.contains(&item.id) {
                if item.seed_enabled || item.seeding {
                    item.seed_enabled = false;
                    item.seeding = false;
                    item.modified_at = now;
                    manifest_changed = true;
                }
                continue;
            }

            // Migrate legacy manifests where seeding intent lived only in `seeding`.
            if !item.seed_enabled {
                item.seed_enabled = true;
                item.modified_at = now;
                manifest_changed = true;
            }

            let active = activated_ids.contains(&item.id);
            if item.seeding != active {
                item.seeding = active;
                item.modified_at = now;
                manifest_changed = true;
            }
        }
        for (item_id, hash) in hash_updates {
            if let Some(item) = manifest.items.iter_mut().find(|i| i.id == item_id) {
                if item.merkle_root.as_deref() != Some(hash.as_str()) {
                    item.merkle_root = Some(hash);
                    item.modified_at = now;
                    manifest_changed = true;
                }
            }
        }
        drop(manifest);
        if manifest_changed {
            state.drive_state.persist().await;
        }
    }

    if reseeded_local > 0 {
        if peer_id.is_empty() {
            println!(
                "[DRIVE] Auto-reseeded {} Drive file(s) locally (metadata publish deferred)",
                reseeded_local
            );
        } else {
            println!(
                "[DRIVE] Auto-reseeded {} Drive file(s); refreshed metadata for {}",
                reseeded_local, reseeded_dht_metadata
            );
        }
    }
}

#[tauri::command]
async fn reseed_drive_files(state: tauri::State<'_, AppState>) -> Result<(), String> {
    // Reload disk state first to avoid stale in-memory manifests after app restart.
    state.drive_state.load_from_disk_async().await;
    auto_reseed_drive_files(state.inner()).await;
    Ok(())
}

#[tauri::command]
async fn stop_dht(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut dht_guard = state.dht.lock().await;
    let mut was_running = false;

    if let Some(dht) = dht_guard.take() {
        // Best-effort cleanup: remove our seeder entries so DHT reflects that
        // we're no longer actively seeding when users disconnect.
        // Timeout after 3 seconds so logout never hangs on slow/unreachable DHT.
        let cleanup_result = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let peer_id = dht.get_peer_id().await.unwrap_or_default();
            if !peer_id.is_empty() {
                let shared = dht.get_shared_files();
                let file_hashes: Vec<String> = {
                    let map = shared.lock().await;
                    map.keys().cloned().collect()
                };
                for file_hash in &file_hashes {
                    let dht_key = format!("chiral_file_{}", file_hash);
                    if let Ok(Some(json)) = dht.get_dht_value(dht_key.clone()).await {
                        if let Ok(mut metadata) = serde_json::from_str::<FileMetadata>(&json) {
                            metadata.seeders.retain(|s| s.peer_id != peer_id);
                            if metadata.peer_id == peer_id {
                                metadata.peer_id = String::new();
                            }
                            if metadata.wallet_address.is_empty() {
                                metadata.wallet_address = metadata
                                    .seeders
                                    .first()
                                    .map(|s| s.wallet_address.clone())
                                    .unwrap_or_default();
                            }
                            if let Ok(updated_json) = serde_json::to_string(&metadata) {
                                let _ = dht.put_dht_value(dht_key, updated_json).await;
                            }
                        }
                    }
                }
            }
        }).await;
        if cleanup_result.is_err() {
            eprintln!("[DHT] Seeder cleanup timed out during stop — skipping");
        }

        dht.stop().await?;
        was_running = true;
    }
    drop(dht_guard);

    // DHT is offline: clear active-seeding runtime state in Drive manifest.
    if was_running {
        let mut changed = false;
        let now = ds::now_secs();
        {
            let mut manifest = state.drive_state.manifest.write().await;
            for item in manifest
                .items
                .iter_mut()
                .filter(|i| i.item_type == "file" && i.seeding)
            {
                item.seeding = false;
                item.modified_at = now;
                changed = true;
            }
        }
        if changed {
            state.drive_state.persist().await;
        }
    }

    Ok(())
}

#[tauri::command]
async fn get_dht_peers(state: tauri::State<'_, AppState>) -> Result<Vec<dht::PeerInfo>, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        let peers = dht.get_peers().await;
        if peers.is_empty() {
            eprintln!("[get_dht_peers] WARNING: returning 0 peers (DHT is running but peer list is empty)");
        }
        Ok(peers)
    } else {
        eprintln!("[get_dht_peers] DHT not running, returning empty list");
        Ok(Vec::new())
    }
}

#[tauri::command]
async fn get_network_stats(state: tauri::State<'_, AppState>) -> Result<dht::NetworkStats, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        Ok(dht.get_stats().await)
    } else {
        Ok(dht::NetworkStats {
            connected_peers: 0,
            total_peers: 0,
        })
    }
}

#[tauri::command]
async fn get_dht_health(state: tauri::State<'_, AppState>) -> Result<dht::DhtHealthInfo, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        Ok(dht.get_health().await)
    } else {
        Ok(dht::DhtHealthInfo {
            running: false,
            peer_id: None,
            listening_addresses: vec![],
            connected_peer_count: 0,
            kademlia_peers: 0,
            bootstrap_nodes: vec![],
            shared_files: 0,
            protocols: vec![],
        })
    }
}

#[tauri::command]
fn get_bootstrap_peer_ids() -> Vec<String> {
    dht::get_bootstrap_peer_ids()
}

#[tauri::command]
async fn get_peer_id(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        Ok(dht.get_peer_id().await)
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn echo_peer(
    state: tauri::State<'_, AppState>,
    peer_id: String,
    payload: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let dht_guard = state.dht.lock().await;
    let dht = dht_guard
        .as_ref()
        .ok_or_else(|| "DHT not running".to_string())?;
    dht.echo(peer_id, payload).await
}

#[tauri::command]
async fn ping_peer(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    peer_id: String,
) -> Result<String, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        dht.ping_peer(peer_id, app).await
    } else {
        Err("DHT not running".to_string())
    }
}

#[tauri::command]
async fn send_file(
    state: tauri::State<'_, AppState>,
    peer_id: String,
    file_name: String,
    file_data: Vec<u8>,
    transfer_id: String,
    price_wei: Option<String>,
    sender_wallet: Option<String>,
    file_hash: Option<String>,
    file_size: Option<u64>,
) -> Result<(), String> {
    let actual_size = file_size.unwrap_or(file_data.len() as u64);
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        dht.send_file(
            peer_id,
            transfer_id,
            file_name,
            file_data,
            price_wei.unwrap_or_default(),
            sender_wallet.unwrap_or_default(),
            file_hash.unwrap_or_default(),
            actual_size,
        )
        .await
    } else {
        Err("DHT not running".to_string())
    }
}

/// Send a file to a peer by reading from a file path on disk (used by ChiralDrop)
#[tauri::command]
async fn send_file_by_path(
    state: tauri::State<'_, AppState>,
    peer_id: String,
    file_path: String,
    transfer_id: String,
    price_wei: Option<String>,
    sender_wallet: Option<String>,
    file_hash: Option<String>,
) -> Result<(), String> {
    let path = std::path::Path::new(&file_path);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.clone());
    let file_data = std::fs::read(&file_path).map_err(|e| format!("Failed to read file: {}", e))?;
    let file_size = file_data.len() as u64;

    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        dht.send_file(
            peer_id,
            transfer_id,
            file_name,
            file_data,
            price_wei.unwrap_or_default(),
            sender_wallet.unwrap_or_default(),
            file_hash.unwrap_or_default(),
            file_size,
        )
        .await
    } else {
        Err("DHT not running".to_string())
    }
}

#[tauri::command]
async fn accept_file_transfer(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    transfer_id: String,
) -> Result<String, String> {
    let custom_dir = state.download_directory.lock().await.clone();
    let file_transfer = state.file_transfer.lock().await;
    file_transfer
        .accept_transfer(
            crate::event_sink::EventSink::tauri(app),
            transfer_id,
            custom_dir,
        )
        .await
}

#[tauri::command]
async fn decline_file_transfer(
    state: tauri::State<'_, AppState>,
    transfer_id: String,
) -> Result<(), String> {
    let file_transfer = state.file_transfer.lock().await;
    file_transfer.decline_transfer(transfer_id).await
}

#[tauri::command]
async fn store_dht_value(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        dht.put_dht_value(key, value).await
    } else {
        Err("DHT not running".to_string())
    }
}

#[tauri::command]
async fn get_dht_value(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<Option<String>, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        dht.get_dht_value(key).await
    } else {
        Err("DHT not running".to_string())
    }
}

// ---------------------------------------------------------------------------
// Hosting marketplace commands
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HostRegistryEntry {
    #[serde(rename = "peerId")]
    peer_id: String,
    #[serde(rename = "walletAddress")]
    wallet_address: String,
    #[serde(rename = "updatedAt")]
    updated_at: u64,
}

#[tauri::command]
async fn publish_host_advertisement(
    state: tauri::State<'_, AppState>,
    advertisement_json: String,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        let peer_id = dht.get_peer_id().await.ok_or("Peer ID not available")?;

        // Parse advertisement to extract wallet address, inject peer_id
        let mut ad: serde_json::Value = serde_json::from_str(&advertisement_json)
            .map_err(|e| format!("Invalid advertisement JSON: {}", e))?;
        ad["peerId"] = serde_json::Value::String(peer_id.clone());
        let ad_json = serde_json::to_string(&ad)
            .map_err(|e| format!("Failed to serialize advertisement: {}", e))?;

        let wallet_address = ad["walletAddress"].as_str().unwrap_or("").to_string();

        // Store individual advertisement
        let host_key = format!("chiral_host_{}", peer_id);
        dht.put_dht_value(host_key, ad_json).await?;

        // Update registry (read-modify-write)
        let registry_key = "chiral_host_registry".to_string();
        let mut registry: Vec<HostRegistryEntry> =
            match dht.get_dht_value(registry_key.clone()).await {
                Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
                _ => Vec::new(),
            };

        // Remove existing entry for this peer, add fresh one
        registry.retain(|e| e.peer_id != peer_id);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        registry.push(HostRegistryEntry {
            peer_id,
            wallet_address,
            updated_at: now,
        });

        let registry_json = serde_json::to_string(&registry)
            .map_err(|e| format!("Failed to serialize registry: {}", e))?;
        dht.put_dht_value(registry_key, registry_json).await
    } else {
        Err("DHT not running".to_string())
    }
}

#[tauri::command]
async fn unpublish_host_advertisement(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        let peer_id = dht.get_peer_id().await.ok_or("Peer ID not available")?;

        // Remove from registry
        let registry_key = "chiral_host_registry".to_string();
        let mut registry: Vec<HostRegistryEntry> =
            match dht.get_dht_value(registry_key.clone()).await {
                Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
                _ => Vec::new(),
            };

        registry.retain(|e| e.peer_id != peer_id);

        let registry_json = serde_json::to_string(&registry)
            .map_err(|e| format!("Failed to serialize registry: {}", e))?;
        dht.put_dht_value(registry_key, registry_json).await
    } else {
        Err("DHT not running".to_string())
    }
}

#[tauri::command]
async fn get_host_registry(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        let registry_key = "chiral_host_registry".to_string();
        match dht.get_dht_value(registry_key).await {
            Ok(Some(json)) => Ok(json),
            Ok(None) => Ok("[]".to_string()),
            Err(e) => Err(e),
        }
    } else {
        Err("DHT not running".to_string())
    }
}

#[tauri::command]
async fn get_host_advertisement(
    state: tauri::State<'_, AppState>,
    peer_id: String,
) -> Result<Option<String>, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        let key = format!("chiral_host_{}", peer_id);
        dht.get_dht_value(key).await
    } else {
        Err("DHT not running".to_string())
    }
}

fn agreements_dir() -> Result<std::path::PathBuf, String> {
    let dir = dirs::data_dir()
        .ok_or("Could not find data directory")?
        .join("chiral-network")
        .join("agreements");
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create agreements dir: {e}"))?;
    }
    Ok(dir)
}

#[tauri::command]
async fn store_hosting_agreement(
    state: tauri::State<'_, AppState>,
    agreement_id: String,
    agreement_json: String,
) -> Result<(), String> {
    // Save locally on disk
    let path = agreements_dir()?.join(format!("{}.json", agreement_id));
    std::fs::write(&path, &agreement_json)
        .map_err(|e| format!("Failed to write agreement to disk: {e}"))?;

    // Also store in DHT for the other party
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let key = format!("chiral_agreement_{}", agreement_id);
        let _ = dht.put_dht_value(key, agreement_json).await;
    }

    Ok(())
}

/// Enforce agreement expiration: if active and past expiresAt, mark as expired.
fn enforce_agreement_expiration(json: &str, path: &std::path::Path) -> String {
    if let Ok(mut agreement) = serde_json::from_str::<serde_json::Value>(json) {
        let status = agreement.get("status").and_then(|s| s.as_str()).unwrap_or("");
        let expires_at = agreement.get("expiresAt").and_then(|e| e.as_u64()).unwrap_or(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if status == "active" && expires_at > 0 && now > expires_at {
            agreement["status"] = serde_json::Value::String("expired".to_string());
            let updated = serde_json::to_string(&agreement).unwrap_or_else(|_| json.to_string());
            let _ = std::fs::write(path, &updated);
            return updated;
        }
    }
    json.to_string()
}

#[tauri::command]
async fn get_hosting_agreement(
    state: tauri::State<'_, AppState>,
    agreement_id: String,
) -> Result<Option<String>, String> {
    let path = agreements_dir()?.join(format!("{}.json", agreement_id));
    if path.exists() {
        let json =
            std::fs::read_to_string(&path).map_err(|e| format!("Failed to read agreement: {e}"))?;
        return Ok(Some(enforce_agreement_expiration(&json, &path)));
    }

    // Fall back to DHT
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let key = format!("chiral_agreement_{}", agreement_id);
        let result = dht.get_dht_value(key).await?;
        if let Some(ref json) = result {
            let _ = std::fs::write(&path, json);
            return Ok(Some(enforce_agreement_expiration(json, &path)));
        }
        Ok(result)
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn list_hosting_agreements() -> Result<Vec<String>, String> {
    let dir = agreements_dir()?;
    let mut ids = Vec::new();
    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("Failed to read agreements dir: {e}"))?;
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if let Some(id) = name.strip_suffix(".json") {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
}

// File operations for Upload/Download pages

#[tauri::command]
async fn get_available_storage() -> Result<u64, String> {
    // Get available disk space in MB
    let home_dir = dirs::home_dir().ok_or("Could not find home directory")?;

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let output = Command::new("df")
            .arg("-BM")
            .arg(&home_dir)
            .output()
            .map_err(|e| e.to_string())?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        if lines.len() >= 2 {
            let parts: Vec<&str> = lines[1].split_whitespace().collect();
            if parts.len() >= 4 {
                let available = parts[3].trim_end_matches('M');
                return available.parse::<u64>().map_err(|e| e.to_string());
            }
        }
        Ok(0)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = home_dir; // Suppress warning
                          // Default fallback for other platforms
        Ok(10000) // Return 10GB as default
    }
}

#[tauri::command]
async fn get_file_size(file_path: String) -> Result<u64, String> {
    let metadata = std::fs::metadata(&file_path).map_err(|e| e.to_string())?;
    Ok(metadata.len())
}

#[tauri::command]
async fn open_file_dialog(multiple: bool) -> Result<Vec<String>, String> {
    use rfd::FileDialog;

    if multiple {
        let files = FileDialog::new().pick_files();
        if let Some(paths) = files {
            Ok(paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect())
        } else {
            Ok(vec![])
        }
    } else {
        let file = FileDialog::new().pick_file();
        if let Some(path) = file {
            Ok(vec![path.to_string_lossy().to_string()])
        } else {
            Ok(vec![])
        }
    }
}

#[tauri::command]
async fn pick_download_directory() -> Result<Option<String>, String> {
    use rfd::FileDialog;

    let dir = FileDialog::new()
        .set_title("Choose Download Directory")
        .pick_folder();

    Ok(dir.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
async fn set_download_directory(
    state: tauri::State<'_, AppState>,
    path: Option<String>,
) -> Result<(), String> {
    // Validate the path exists if provided
    if let Some(ref p) = path {
        if !p.is_empty() {
            let path_buf = std::path::Path::new(p);
            if !path_buf.exists() {
                return Err(format!("Directory does not exist: {}", p));
            }
            if !path_buf.is_dir() {
                return Err(format!("Path is not a directory: {}", p));
            }
        }
    }

    let mut dir = state.download_directory.lock().await;
    *dir = path.filter(|p| !p.is_empty());
    println!("📁 Download directory set to: {:?}", *dir);
    Ok(())
}

#[tauri::command]
async fn get_download_directory(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let dir = state.download_directory.lock().await;
    match dir.as_ref() {
        Some(path) => Ok(path.clone()),
        None => {
            // Return system default
            Ok(dirs::download_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default())
        }
    }
}

/// Helper to get the effective download directory from AppState
fn get_effective_download_dir(custom_dir: &Option<String>) -> Result<std::path::PathBuf, String> {
    if let Some(ref dir) = custom_dir {
        let path = std::path::PathBuf::from(dir);
        if path.exists() && path.is_dir() {
            return Ok(path);
        }
        println!(
            "⚠️ Custom download directory '{}' is invalid, falling back to system default",
            dir
        );
    }
    dirs::download_dir().ok_or_else(|| "Could not find downloads directory".to_string())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishResult {
    merkle_root: String,
}

/// Per-seeder info stored in DHT file metadata.
/// Each seeder signs their entry so downloaders can verify the wallet address is legitimate.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SeederInfo {
    peer_id: String,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    wallet_address: String,
    /// Multiaddresses where this seeder can be reached.
    #[serde(default)]
    multiaddrs: Vec<String>,
    /// ECDSA signature of "seeder:{peer_id}:{file_hash}:{wallet_address}" by wallet key.
    /// Proves this seeder controls the claimed wallet (prevents payment redirection).
    #[serde(default)]
    signature: String,
}

impl SeederInfo {
    /// Create the message bytes that are signed by the seeder's wallet.
    fn sign_payload(peer_id: &str, file_hash: &str, wallet_address: &str) -> Vec<u8> {
        format!("seeder:{}:{}:{}", peer_id, file_hash, wallet_address).into_bytes()
    }

    /// Verify that this seeder entry was signed by the claimed wallet address.
    fn verify(&self, file_hash: &str) -> bool {
        if self.signature.is_empty() {
            return false; // Unsigned entries are untrusted
        }
        let payload = Self::sign_payload(&self.peer_id, file_hash, &self.wallet_address);
        wallet::verify_signature(&payload, &self.signature, &self.wallet_address)
    }
}

/// File metadata stored in DHT.
/// The publisher signs the core metadata so readers can verify it hasn't been tampered with.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FileMetadata {
    hash: String,
    file_name: String,
    file_size: u64,
    protocol: String,
    created_at: u64,
    /// Legacy single-seeder field — kept for backward compat with old DHT records
    #[serde(default)]
    peer_id: String,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    wallet_address: String,
    /// Multi-seeder list — each seeder signs their own entry
    #[serde(default)]
    seeders: Vec<SeederInfo>,
    /// ECDSA signature of "file:{hash}:{file_name}:{file_size}" by the publisher's wallet.
    /// Proves the metadata was created by the wallet_address owner.
    #[serde(default)]
    publisher_signature: String,
}

impl FileMetadata {
    /// Create the message bytes that are signed by the publisher.
    fn sign_payload(hash: &str, file_name: &str, file_size: u64) -> Vec<u8> {
        format!("file:{}:{}:{}", hash, file_name, file_size).into_bytes()
    }

    /// Sign this file metadata with the publisher's wallet key.
    fn sign(&mut self, private_key: &str) {
        let payload = Self::sign_payload(&self.hash, &self.file_name, self.file_size);
        self.publisher_signature = wallet::sign_message(private_key, &payload).unwrap_or_default();
    }

    /// Verify that the publisher signature matches the claimed wallet_address.
    fn verify_publisher(&self) -> bool {
        if self.publisher_signature.is_empty() || self.wallet_address.is_empty() {
            return false;
        }
        let payload = Self::sign_payload(&self.hash, &self.file_name, self.file_size);
        wallet::verify_signature(&payload, &self.publisher_signature, &self.wallet_address)
    }
}

/// Build a signed SeederInfo entry.
fn make_signed_seeder(
    peer_id: &str,
    file_hash: &str,
    price_wei: &str,
    wallet_address: &str,
    multiaddrs: Vec<String>,
    private_key: Option<&str>,
) -> SeederInfo {
    let signature = if let Some(key) = private_key {
        let payload = SeederInfo::sign_payload(peer_id, file_hash, wallet_address);
        wallet::sign_message(key, &payload).unwrap_or_default()
    } else {
        String::new()
    };
    SeederInfo {
        peer_id: peer_id.to_string(),
        price_wei: price_wei.to_string(),
        wallet_address: wallet_address.to_string(),
        multiaddrs,
        signature,
    }
}

#[tauri::command]
async fn publish_file(
    state: tauri::State<'_, AppState>,
    file_path: String,
    file_name: String,
    protocol: Option<String>,
    price_chi: Option<String>,
    wallet_address: Option<String>,
) -> Result<PublishResult, String> {
    // Read file and compute hash
    let file_data = std::fs::read(&file_path).map_err(|e| e.to_string())?;
    let file_size = file_data.len() as u64;

    // Compute SHA-256 hash
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let merkle_root = hex::encode(hash);

    println!("Publishing file: {} with hash: {}", file_name, merkle_root);

    // Store file data in memory for serving to peers (local cache)
    {
        let mut storage = state.file_storage.lock().await;
        storage.insert(merkle_root.clone(), file_data);
    }

    // Get DHT service and peer ID
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let peer_id = dht.get_peer_id().await.unwrap_or_default();

        // Parse price from CHI to wei
        let price_wei_val = if let Some(ref price) = price_chi {
            if price.is_empty() || price == "0" {
                0u128
            } else {
                wallet::parse_chi_to_wei(price)?
            }
        } else {
            0u128
        };

        let wallet_addr = wallet_address.unwrap_or_default();

        // Validate: if price > 0, wallet must be non-empty
        if price_wei_val > 0 && wallet_addr.is_empty() {
            return Err("Wallet address is required when setting a file price".to_string());
        }

        // Register the file for sharing (so we can serve it to requesters)
        dht.register_shared_file(
            merkle_root.clone(),
            file_path.clone(),
            file_name.clone(),
            file_size,
            price_wei_val,
            wallet_addr.clone(),
        )
        .await;

        // Build our seeder entry with listening addresses so other peers can dial us
        let our_multiaddrs = dht.get_listening_addresses().await;
        let our_seeder = SeederInfo {
            peer_id: peer_id.clone(),
            price_wei: price_wei_val.to_string(),
            wallet_address: wallet_addr,
            multiaddrs: our_multiaddrs,
            signature: String::new(),
        };

        // Read-modify-write: preserve existing seeders from other peers
        let dht_key = format!("chiral_file_{}", merkle_root);
        let mut metadata = match dht.get_dht_value(dht_key.clone()).await {
            Ok(Some(json)) => {
                serde_json::from_str::<FileMetadata>(&json).unwrap_or_else(|_| FileMetadata {
                    hash: merkle_root.clone(),
                    file_name: file_name.clone(),
                    file_size,
                    protocol: protocol.clone().unwrap_or_else(|| "WebRTC".to_string()),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    peer_id: String::new(),
                    price_wei: String::new(),
                    wallet_address: String::new(),
                    seeders: Vec::new(),
                publisher_signature: String::new(),
                })
            }
            _ => FileMetadata {
                hash: merkle_root.clone(),
                file_name: file_name.clone(),
                file_size,
                protocol: protocol.unwrap_or_else(|| "WebRTC".to_string()),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                peer_id: String::new(),
                price_wei: String::new(),
                wallet_address: String::new(),
                seeders: Vec::new(),
                publisher_signature: String::new(),
            },
        };

        // Upsert our seeder entry (add or update by peer_id)
        if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
            existing.price_wei = our_seeder.price_wei.clone();
            existing.wallet_address = our_seeder.wallet_address.clone();
            existing.multiaddrs = our_seeder.multiaddrs.clone();
        } else {
            metadata.seeders.push(our_seeder);
        }
        // Update legacy fields for backward compat
        metadata.peer_id = peer_id.clone();
        metadata.price_wei = price_wei_val.to_string();
        metadata.wallet_address = metadata
            .seeders
            .iter()
            .find(|s| s.peer_id == peer_id)
            .map(|s| s.wallet_address.clone())
            .unwrap_or_default();

        // Serialize and store in DHT
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
        dht.put_dht_value(dht_key, metadata_json).await?;

        println!(
            "File metadata published to DHT: {} ({} seeders)",
            merkle_root,
            metadata.seeders.len()
        );
    } else {
        println!("DHT not running, file hash computed but not published to network");
    }

    Ok(PublishResult { merkle_root })
}

/// Publish file from raw bytes (for ChiralDrop paid transfers where file data comes from the browser)
#[tauri::command]
async fn publish_file_data(
    state: tauri::State<'_, AppState>,
    file_name: String,
    file_data: Vec<u8>,
    price_chi: Option<String>,
    wallet_address: Option<String>,
) -> Result<PublishResult, String> {
    let file_size = file_data.len() as u64;

    // Compute SHA-256 hash
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let merkle_root = hex::encode(hash);

    println!(
        "Publishing file from data: {} with hash: {}",
        file_name, merkle_root
    );

    // Store file data in memory for serving to peers
    {
        let mut storage = state.file_storage.lock().await;
        storage.insert(merkle_root.clone(), file_data);
    }

    // Get DHT service and peer ID
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let peer_id = dht.get_peer_id().await.unwrap_or_default();

        // Parse price from CHI to wei
        let price_wei_val = if let Some(ref price) = price_chi {
            if price.is_empty() || price == "0" {
                0u128
            } else {
                wallet::parse_chi_to_wei(price)?
            }
        } else {
            0u128
        };

        let wallet_addr = wallet_address.unwrap_or_default();

        if price_wei_val > 0 && wallet_addr.is_empty() {
            return Err("Wallet address is required when setting a file price".to_string());
        }

        // Register the file for sharing (use hash as path since data is in memory)
        dht.register_shared_file(
            merkle_root.clone(),
            format!("memory:{}", merkle_root),
            file_name.clone(),
            file_size,
            price_wei_val,
            wallet_addr.clone(),
        )
        .await;

        // Build our seeder entry with listening addresses so other peers can dial us
        let our_multiaddrs = dht.get_listening_addresses().await;
        let our_seeder = SeederInfo {
            peer_id: peer_id.clone(),
            price_wei: price_wei_val.to_string(),
            wallet_address: wallet_addr,
            multiaddrs: our_multiaddrs,
            signature: String::new(),
        };

        // Read-modify-write: preserve existing seeders from other peers
        let dht_key = format!("chiral_file_{}", merkle_root);
        let mut metadata = match dht.get_dht_value(dht_key.clone()).await {
            Ok(Some(json)) => {
                serde_json::from_str::<FileMetadata>(&json).unwrap_or_else(|_| FileMetadata {
                    hash: merkle_root.clone(),
                    file_name: file_name.clone(),
                    file_size,
                    protocol: "WebRTC".to_string(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    peer_id: String::new(),
                    price_wei: String::new(),
                    wallet_address: String::new(),
                    seeders: Vec::new(),
                publisher_signature: String::new(),
                })
            }
            _ => FileMetadata {
                hash: merkle_root.clone(),
                file_name: file_name.clone(),
                file_size,
                protocol: "WebRTC".to_string(),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                peer_id: String::new(),
                price_wei: String::new(),
                wallet_address: String::new(),
                seeders: Vec::new(),
                publisher_signature: String::new(),
            },
        };

        // Upsert our seeder entry
        if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
            existing.price_wei = our_seeder.price_wei.clone();
            existing.wallet_address = our_seeder.wallet_address.clone();
            existing.multiaddrs = our_seeder.multiaddrs.clone();
        } else {
            metadata.seeders.push(our_seeder);
        }
        metadata.peer_id = peer_id.clone();
        metadata.price_wei = price_wei_val.to_string();
        metadata.wallet_address = metadata
            .seeders
            .iter()
            .find(|s| s.peer_id == peer_id)
            .map(|s| s.wallet_address.clone())
            .unwrap_or_default();

        // Serialize and store in DHT
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
        dht.put_dht_value(dht_key, metadata_json).await?;

        println!(
            "File data published to DHT: {} ({} seeders)",
            merkle_root,
            metadata.seeders.len()
        );
    } else {
        println!("DHT not running, file hash computed but not published to network");
    }

    Ok(PublishResult { merkle_root })
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    hash: String,
    file_name: String,
    file_size: u64,
    seeders: Vec<SeederInfo>,
    created_at: u64,
    price_wei: String,
    wallet_address: String,
}

async fn build_local_search_result(dht: &Arc<DhtService>, file_hash: &str) -> Option<SearchResult> {
    let shared_files = dht.get_shared_files();
    let local_info = {
        let shared = shared_files.lock().await;
        shared.get(file_hash).cloned()
    }?;

    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    // Local-first search should never block on address discovery.
    // `multiaddrs` are optional for local-device downloads because start_download
    // already short-circuits against shared_files map.
    let seeders = if peer_id.is_empty() {
        Vec::new()
    } else {
        vec![SeederInfo {
            peer_id,
            price_wei: local_info.price_wei.to_string(),
            wallet_address: local_info.wallet_address.clone(),
            multiaddrs: Vec::new(),
            signature: String::new(),
        }]
    };

    Some(SearchResult {
        hash: file_hash.to_string(),
        file_name: local_info.file_name,
        file_size: local_info.file_size,
        seeders,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        price_wei: local_info.price_wei.to_string(),
        wallet_address: local_info.wallet_address,
    })
}

#[derive(Clone)]
struct LocalDriveSeedCandidate {
    item_id: String,
    owner: String,
    file_name: String,
    storage_path: String,
    file_size_hint: Option<u64>,
    protocol: Option<String>,
    price_chi: Option<String>,
}

async fn try_repair_local_drive_seed(
    state: &AppState,
    dht: &Arc<DhtService>,
    file_hash: &str,
) -> Option<SearchResult> {
    let candidate = {
        let manifest = state.drive_state.manifest.read().await;
        manifest
            .items
            .iter()
            .find(|item| {
                item.item_type == "file"
                    && item
                        .merkle_root
                        .as_deref()
                        .map(|h| h.eq_ignore_ascii_case(file_hash))
                        .unwrap_or(false)
                    && (item.seed_enabled || item.seeding)
            })
            .and_then(|item| {
                item.storage_path
                    .as_ref()
                    .map(|sp| LocalDriveSeedCandidate {
                        item_id: item.id.clone(),
                        owner: item.owner.clone(),
                        file_name: item.name.clone(),
                        storage_path: sp.clone(),
                        file_size_hint: item.size,
                        protocol: item.protocol.clone(),
                        price_chi: item.price_chi.clone(),
                    })
            })
    }?;

    let files_dir = ds::drive_files_dir()?;
    let full_path = files_dir.join(&candidate.storage_path);
    if !full_path.exists() {
        return None;
    }

    let file_size = match candidate.file_size_hint {
        Some(size) if size > 0 => size,
        _ => std::fs::metadata(&full_path).ok()?.len(),
    };

    let price_wei = match candidate.price_chi.as_deref() {
        Some(price) if !price.trim().is_empty() && price.trim() != "0" => {
            wallet::parse_chi_to_wei(price).unwrap_or(0)
        }
        _ => 0u128,
    };
    let wallet_addr = candidate.owner.trim().to_string();
    if price_wei > 0 && wallet_addr.is_empty() {
        return None;
    }

    dht.register_shared_file(
        file_hash.to_string(),
        full_path.to_string_lossy().to_string(),
        candidate.file_name.clone(),
        file_size,
        price_wei,
        wallet_addr.clone(),
    )
    .await;

    // Best-effort metadata republish for remote discoverability.
    // Keep search path fast: local repair should return immediately, while DHT
    // publish retries happen in the background.
    let dht_for_publish = dht.clone();
    let file_hash_for_publish = file_hash.to_string();
    let file_name_for_publish = candidate.file_name.clone();
    let protocol_for_publish = candidate
        .protocol
        .clone()
        .unwrap_or_else(|| "WebRTC".to_string());
    let wallet_for_publish = wallet_addr.clone();

    tauri::async_runtime::spawn(async move {
        let peer_id = dht_for_publish.get_peer_id().await.unwrap_or_default();
        if peer_id.is_empty() {
            return;
        }

        let our_multiaddrs = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            dht_for_publish.get_listening_addresses(),
        )
        .await
        .unwrap_or_default();

        let dht_key = format!("chiral_file_{}", file_hash_for_publish);
        let mut metadata = match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            dht_for_publish.get_dht_value(dht_key.clone()),
        )
        .await
        {
            Ok(Ok(Some(json))) => {
                serde_json::from_str::<FileMetadata>(&json).unwrap_or(FileMetadata {
                    hash: file_hash_for_publish.clone(),
                    file_name: file_name_for_publish.clone(),
                    file_size,
                    protocol: protocol_for_publish.clone(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    peer_id: String::new(),
                    price_wei: String::new(),
                    wallet_address: String::new(),
                    seeders: Vec::new(),
                publisher_signature: String::new(),
                })
            }
            _ => FileMetadata {
                hash: file_hash_for_publish.clone(),
                file_name: file_name_for_publish.clone(),
                file_size,
                protocol: protocol_for_publish.clone(),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                peer_id: String::new(),
                price_wei: String::new(),
                wallet_address: String::new(),
                seeders: Vec::new(),
                publisher_signature: String::new(),
            },
        };

        let our_seeder = SeederInfo {
            peer_id: peer_id.clone(),
            price_wei: price_wei.to_string(),
            wallet_address: wallet_for_publish.clone(),
            multiaddrs: our_multiaddrs,
            signature: String::new(),
        };
        if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
            existing.price_wei = our_seeder.price_wei.clone();
            existing.wallet_address = our_seeder.wallet_address.clone();
            existing.multiaddrs = our_seeder.multiaddrs.clone();
        } else {
            metadata.seeders.push(our_seeder);
        }
        metadata.peer_id = peer_id;
        metadata.price_wei = price_wei.to_string();
        metadata.wallet_address = wallet_for_publish;

        if let Ok(json) = serde_json::to_string(&metadata) {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                dht_for_publish.put_dht_value(dht_key, json),
            )
            .await;
        }
    });

    // Keep runtime Drive state consistent with repaired registration.
    {
        let mut manifest = state.drive_state.manifest.write().await;
        if let Some(item) = manifest
            .items
            .iter_mut()
            .find(|i| i.id == candidate.item_id)
        {
            if !item.seeding {
                item.seeding = true;
                item.modified_at = ds::now_secs();
            }
            if !item.seed_enabled {
                item.seed_enabled = true;
                item.modified_at = ds::now_secs();
            }
        }
    }

    state.drive_state.persist().await;
    build_local_search_result(dht, file_hash).await
}

#[tauri::command]
async fn search_file(
    state: tauri::State<'_, AppState>,
    file_hash: String,
) -> Result<Option<SearchResult>, String> {
    println!("Searching for file: {}", file_hash);

    // Get DHT service
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };
    if let Some(dht) = dht.as_ref() {
        // Refresh manifest snapshot first so startup/local-repair checks see the
        // latest persisted Drive seeding intent.
        state.drive_state.load_from_disk_async().await;

        // Check if we are locally seeding this hash (used to merge with DHT results later)
        let local_result = build_local_search_result(dht, &file_hash).await;

        // Self-heal path: if manifest says this file should be seeded but runtime
        // shared-files map is missing it (restart race/crash), repair it.
        if local_result.is_none() {
            if let Some(_repaired) = try_repair_local_drive_seed(state.inner(), dht, &file_hash).await {
                println!(
                    "Repaired missing local seed registration for {} from Drive manifest",
                    file_hash
                );
            }
        }

        // Search for file metadata in DHT
        let dht_key = format!("chiral_file_{}", file_hash);
        println!("Looking up DHT key: {}", dht_key);

        let dht_lookup = tokio::time::timeout(
            tokio::time::Duration::from_millis(10000),
            dht.get_dht_value(dht_key.clone()),
        )
        .await;

        match dht_lookup {
            Err(_) => {
                println!("DHT lookup timed out for key: {}", dht_key);
                // Return local result if we're seeding, otherwise not found
                if let Some(local) = local_result {
                    println!("Returning local seeding result after DHT timeout for {}", file_hash);
                    Ok(Some(local))
                } else if let Some(repaired) =
                    try_repair_local_drive_seed(state.inner(), dht, &file_hash).await
                {
                    Ok(Some(repaired))
                } else {
                    Ok(None)
                }
            }
            Ok(lookup_result) => match lookup_result {
                Ok(Some(metadata_json)) => {
                    // Parse metadata from JSON
                    let metadata: FileMetadata = serde_json::from_str(&metadata_json)
                        .map_err(|e| format!("Failed to parse file metadata: {}", e))?;

                    println!(
                        "Found file in DHT: {} ({})",
                        metadata.file_name, metadata.hash
                    );

                    // Verify publisher signature if present
                    if metadata.verify_publisher() {
                        println!("✅ File metadata signature valid (publisher: {})", metadata.wallet_address);
                    } else if !metadata.publisher_signature.is_empty() {
                        println!("⚠️ File metadata signature INVALID — record may be tampered");
                    }

                    // Build seeder list: use seeders vec if present, fall back to legacy peer_id
                    let file_hash_for_verify = metadata.hash.clone();
                    let mut seeders = metadata.seeders;
                    if seeders.is_empty() && !metadata.peer_id.is_empty() {
                        // Legacy record: single peer_id with file-level price/wallet
                        seeders.push(SeederInfo {
                            peer_id: metadata.peer_id,
                            price_wei: metadata.price_wei.clone(),
                            wallet_address: metadata.wallet_address.clone(),
                            multiaddrs: vec![],
                            signature: String::new(),
                        });
                    }
                    seeders.retain(|s| !s.peer_id.trim().is_empty());

                    // Verify seeder signatures — log warnings for invalid ones
                    // but still include them (graceful degradation for unsigned legacy records)
                    for seeder in &seeders {
                        if seeder.verify(&file_hash_for_verify) {
                            println!("  ✅ Seeder {} signature valid", &seeder.peer_id[..20.min(seeder.peer_id.len())]);
                        } else if !seeder.signature.is_empty() {
                            println!("  ⚠️ Seeder {} has INVALID signature — may be impersonating wallet {}",
                                &seeder.peer_id[..20.min(seeder.peer_id.len())], seeder.wallet_address);
                        }
                    }

                    // Merge local seeder if we're seeding this file but not in the DHT list
                    if let Some(ref local) = local_result {
                        for local_seeder in &local.seeders {
                            if !seeders.iter().any(|s| s.peer_id == local_seeder.peer_id) {
                                seeders.push(local_seeder.clone());
                            }
                        }
                    }

                    Ok(Some(SearchResult {
                        hash: metadata.hash,
                        file_name: metadata.file_name,
                        file_size: metadata.file_size,
                        seeders,
                        created_at: metadata.created_at,
                        price_wei: metadata.price_wei,
                        wallet_address: metadata.wallet_address,
                    }))
                }
                Ok(None) => {
                    println!("File not found in DHT: {}", file_hash);
                    // DHT can lag behind local intent right after restart. Attempt
                    // local repair before returning not-found.
                    if let Some(repaired) =
                        try_repair_local_drive_seed(state.inner(), dht, &file_hash).await
                    {
                        println!(
                            "Recovered local seed registration for {} after DHT miss",
                            file_hash
                        );
                        Ok(Some(repaired))
                    } else {
                        Ok(None)
                    }
                }
                Err(e) => {
                    println!("DHT lookup error: {}. Trying local seeding fallback.", e);

                    // Even if DHT GET fails (e.g. bootstrap race), this node may still
                    // be actively seeding the file from local state.
                    if let Some(local) = build_local_search_result(dht, &file_hash).await {
                        println!(
                            "Returning local fallback for {} despite DHT lookup error",
                            file_hash
                        );
                        Ok(Some(local))
                    } else if let Some(repaired) =
                        try_repair_local_drive_seed(state.inner(), dht, &file_hash).await
                    {
                        println!(
                            "Returning repaired local fallback for {} despite DHT lookup error",
                            file_hash
                        );
                        Ok(Some(repaired))
                    } else {
                        Err(e)
                    }
                }
            },
        }
    } else {
        Err("DHT not running".to_string())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadStartResult {
    request_id: String,
    status: String,
}


#[tauri::command]
async fn start_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    file_hash: String,
    file_name: String,
    seeders: Vec<String>,
    file_size: u64,
    wallet_address: Option<String>,
    private_key: Option<String>,
    seeder_price_wei: Option<String>,
    _seeder_wallet_address: Option<String>,
) -> Result<DownloadStartResult, String> {
    println!(
        "⚡ Starting download: {} (hash: {}) from {} seeders",
        file_name,
        file_hash,
        seeders.len(),
    );

    // Handle payment
    let cost_wei = speed_tiers::calculate_cost(file_size);
    if cost_wei > 0 {
        let wallet_addr = wallet_address
            .as_deref()
            .ok_or("Wallet address required for paid download")?;
        let priv_key = private_key
            .as_deref()
            .ok_or("Private key required for paid download")?;

        // Split payment: 99.5% to burn address, 0.5% platform fee
        let (seller_wei, fee_wei) = speed_tiers::split_payment(cost_wei);
        let seller_chi = speed_tiers::format_wei_as_chi(seller_wei);
        let fee_chi = speed_tiers::format_wei_as_chi(fee_wei);
        let cost_chi = speed_tiers::format_wei_as_chi(cost_wei);
        println!(
            "💰 Download payment: {} CHI total ({} seller + {} platform fee)",
            cost_chi, seller_chi, fee_chi
        );

        let burn_addr = "0x000000000000000000000000000000000000dEaD";
        let endpoint = geth::effective_rpc_endpoint();

        // Send main payment to burn address
        let payment_result = wallet::send_transaction(
            &endpoint,
            wallet_addr,
            burn_addr,
            &seller_chi,
            priv_key,
        )
        .await;

        // Send platform fee (best-effort, don't block download on fee failure)
        if fee_wei > 0 {
            let fee_result = wallet::send_transaction(
                &endpoint,
                wallet_addr,
                speed_tiers::PLATFORM_WALLET,
                &fee_chi,
                priv_key,
            )
            .await;
            if let Err(e) = fee_result {
                eprintln!("[PLATFORM FEE] Failed to collect: {}", e);
            }
        }

        match payment_result {
            Ok(result) => {
                println!("✅ Download payment successful: tx {}", result.hash);
                // Record transaction metadata for enriched history
                let meta = TransactionMeta {
                    tx_hash: result.hash.clone(),
                    tx_type: "download_payment".to_string(),
                    description: format!("⚡ Download: {}", file_name),
                    file_name: Some(file_name.clone()),
                    file_hash: Some(file_hash.clone()),
                    speed_tier: Some("download".to_string()),
                    recipient_label: Some("Burn Address (Download)".to_string()),
                    balance_before: Some(result.balance_before.clone()),
                    balance_after: Some(result.balance_after.clone()),
                };
                let mut metadata = state.tx_metadata.lock().await;
                wallet::record_meta(&mut metadata, meta);

                // Emit event so Download page can show balance change
                let _ = app.emit(
                    "speed-tier-payment-complete",
                    serde_json::json!({
                        "txHash": result.hash,
                        "fileHash": file_hash,
                        "fileName": file_name,
                        "speedTier": "download",
                        "balanceBefore": result.balance_before,
                        "balanceAfter": result.balance_after,
                    }),
                );
            }
            Err(e) => {
                println!("❌ Download payment failed: {}", e);
                return Err(format!("Payment failed: {}. Download not started.", e));
            }
        }
    }

    // First, check if we have the file in local cache
    {
        let storage = state.file_storage.lock().await;
        if let Some(file_data) = storage.get(&file_hash) {
            println!("📁 File found in local cache");

            // Save to downloads folder
            let custom_dir = state.download_directory.lock().await.clone();
            let downloads_dir = get_effective_download_dir(&custom_dir)?;
            let file_path = downloads_dir.join(&file_name);
            let file_hash_prefix = &file_hash[..std::cmp::min(8, file_hash.len())];
            let request_id = format!(
                "local-{}-{}",
                file_hash_prefix,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            );

            let file_data_clone = file_data.clone();
            let app_clone = app.clone();
            let hash_clone = file_hash.clone();
            let name_clone = file_name.clone();
            let rid_clone = request_id.clone();

            // Spawn file write
            tokio::spawn(async move {
                match speed_tiers::write_file(
                    &app_clone,
                    &file_path,
                    &file_data_clone,
                    &rid_clone,
                    &hash_clone,
                    &name_clone,
                )
                .await
                {
                    Ok(_) => {
                        println!("📁 File saved to: {:?}", file_path);
                        let _ = app_clone.emit(
                            "file-download-complete",
                            serde_json::json!({
                                "requestId": rid_clone,
                                "fileHash": hash_clone,
                                "fileName": name_clone,
                                "filePath": file_path.to_string_lossy(),
                                "fileSize": file_data_clone.len(),
                                "status": "completed"
                            }),
                        );
                    }
                    Err(e) => {
                        println!("❌ Failed to save cached file: {}", e);
                        let _ = app_clone.emit(
                            "file-download-failed",
                            serde_json::json!({
                                "requestId": rid_clone,
                                "fileHash": hash_clone,
                                "error": format!("Failed to save file: {}", e)
                            }),
                        );
                    }
                }
            });

            return Ok(DownloadStartResult {
                request_id,
                status: "downloading".to_string(),
            });
        }
    }

    // Get DHT service
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    // Local short-circuit: if this node is currently seeding the hash, download
    // directly from local disk/memory instead of trying network/relay paths.
    if let Some(dht) = dht.as_ref() {
        let mut local_shared = {
            let shared = dht.get_shared_files();
            let map = shared.lock().await;
            map.get(&file_hash).cloned()
        };

        // Restart race guard: if shared-files map is missing this hash but Drive
        // manifest says it should be seeded, repair registration on-demand.
        if local_shared.is_none() {
            let _ = try_repair_local_drive_seed(state.inner(), dht, &file_hash).await;
            local_shared = {
                let shared = dht.get_shared_files();
                let map = shared.lock().await;
                map.get(&file_hash).cloned()
            };
        }

        if let Some(shared_file) = local_shared {
            println!("📁 File found in local shared-files map, bypassing network request");

            let file_data = if shared_file.file_path.starts_with("memory:") {
                let storage = state.file_storage.lock().await;
                storage
                    .get(&file_hash)
                    .cloned()
                    .ok_or_else(|| "Local shared file is missing from memory cache".to_string())?
            } else {
                std::fs::read(&shared_file.file_path).map_err(|e| {
                    format!(
                        "Failed to read locally shared file '{}': {}",
                        shared_file.file_path, e
                    )
                })?
            };

            let custom_dir = state.download_directory.lock().await.clone();
            let downloads_dir = get_effective_download_dir(&custom_dir)?;
            let file_path = downloads_dir.join(&file_name);
            let file_hash_prefix = &file_hash[..std::cmp::min(8, file_hash.len())];
            let request_id = format!(
                "local-{}-{}",
                file_hash_prefix,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            );

            let app_clone = app.clone();
            let hash_clone = file_hash.clone();
            let name_clone = file_name.clone();
            let rid_clone = request_id.clone();
            let file_data_clone = file_data.clone();

            tokio::spawn(async move {
                match speed_tiers::write_file(
                    &app_clone,
                    &file_path,
                    &file_data_clone,
                    &rid_clone,
                    &hash_clone,
                    &name_clone,
                )
                .await
                {
                    Ok(_) => {
                        println!("📁 Local shared file saved to: {:?}", file_path);
                        let _ = app_clone.emit(
                            "file-download-complete",
                            serde_json::json!({
                                "requestId": rid_clone,
                                "fileHash": hash_clone,
                                "fileName": name_clone,
                                "filePath": file_path.to_string_lossy(),
                                "fileSize": file_data_clone.len(),
                                "status": "completed"
                            }),
                        );
                    }
                    Err(e) => {
                        println!("❌ Failed to save local shared file: {}", e);
                        let _ = app_clone.emit(
                            "file-download-failed",
                            serde_json::json!({
                                "requestId": rid_clone,
                                "fileHash": hash_clone,
                                "error": format!("Failed to save file: {}", e)
                            }),
                        );
                    }
                }
            });

            return Ok(DownloadStartResult {
                request_id,
                status: "downloading".to_string(),
            });
        }
    }

    // If not local, request from remote seeders
    let candidate_seeders: Vec<String> = {
        let mut seen_seeders = std::collections::HashSet::new();
        seeders
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter(|s| seen_seeders.insert((*s).to_string()))
            .map(|s| s.to_string())
            .collect()
    };
    if candidate_seeders.is_empty() {
        return Err("No seeders available for this file".to_string());
    }

    if let Some(dht) = dht.as_ref() {
        // Generate a unique request ID
        let file_hash_prefix = &file_hash[..std::cmp::min(8, file_hash.len())];
        let request_id = format!(
            "download-{}-{}",
            file_hash_prefix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        // Store download credentials if wallet is available (needed for file payment in event loop)
        let seeder_price: u128 = seeder_price_wei
            .as_deref()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        if seeder_price > 0 || wallet_address.is_some() {
            if let (Some(ref addr), Some(ref key)) = (&wallet_address, &private_key) {
                let mut creds = state.download_credentials.lock().await;
                creds.insert(
                    request_id.clone(),
                    dht::DownloadCredentials {
                        wallet_address: addr.clone(),
                        private_key: key.clone(),
                    },
                );
            }
        }

        // Emit download started event
        let _ = app.emit(
            "download-started",
            serde_json::json!({
                "requestId": request_id,
                "fileHash": file_hash,
                "fileName": file_name,
                "seeders": seeders.len()
            }),
        );

        // Look up seeder multiaddresses from DHT metadata so we can dial peers directly.
        // Bound this lookup to keep download startup snappy even when DHT lookups are slow.
        let seeder_addrs: std::collections::HashMap<String, Vec<String>> = {
            let dht_key = format!("chiral_file_{}", file_hash);
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(900),
                dht.get_dht_value(dht_key),
            )
            .await
            {
                Ok(Ok(Some(json))) => serde_json::from_str::<FileMetadata>(&json)
                    .map(|meta| {
                        meta.seeders
                            .into_iter()
                            .map(|s| (s.peer_id, s.multiaddrs))
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => std::collections::HashMap::new(),
            }
        };

        // Start requests against all candidate seeders. The DHT layer will
        // keep trying alternatives and only fail when all attempts are exhausted.
        let mut last_error = String::new();
        let mut request_sent = false;

        let mut request_tasks = Vec::with_capacity(candidate_seeders.len());
        for (i, seeder) in candidate_seeders.iter().enumerate() {
            let dht = dht.clone();
            let seeder = seeder.clone();
            let fh = file_hash.clone();
            let rid = request_id.clone();
            let addrs = seeder_addrs.get(&seeder).cloned().unwrap_or_default();
            let total = candidate_seeders.len();
            request_tasks.push(async move {
                println!(
                    "Trying seeder {}/{}: {} for file {}",
                    i + 1,
                    total,
                    seeder,
                    fh
                );
                let result = dht.request_file(seeder.clone(), fh, rid, addrs).await;
                (seeder, result)
            });
        }

        for (seeder, result) in futures::future::join_all(request_tasks).await {
            match result {
                Ok(_) => {
                    println!("✅ File request started for seeder {}", seeder);
                    request_sent = true;
                }
                Err(e) => {
                    println!("❌ Failed to request file from seeder {}: {}", seeder, e);
                    last_error = e;
                }
            }
        }

        if request_sent {
            Ok(DownloadStartResult {
                request_id,
                status: "requesting".to_string(),
            })
        } else {
            let error_msg = if last_error.is_empty() {
                format!(
                    "No seeder could be contacted for this file ({} candidate(s)).",
                    candidate_seeders.len()
                )
            } else {
                format!("No seeder could provide the file: {}", last_error)
            };
            let _ = app.emit(
                "file-download-failed",
                serde_json::json!({
                    "requestId": request_id,
                    "fileHash": file_hash,
                    "error": error_msg
                }),
            );
            Err(error_msg)
        }
    } else {
        Err("DHT not running".to_string())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadCostResult {
    cost_wei: String,
    cost_chi: String,
    tier: String,
    speed_label: String,
}

/// Calculate the cost of downloading a file
#[tauri::command]
async fn calculate_download_cost(
    file_size: u64,
) -> Result<DownloadCostResult, String> {
    let cost_wei = speed_tiers::calculate_cost(file_size);
    let cost_chi = speed_tiers::format_wei_as_chi(cost_wei);

    Ok(DownloadCostResult {
        cost_wei: cost_wei.to_string(),
        cost_chi,
        tier: "standard".to_string(),
        speed_label: "Unlimited".to_string(),
    })
}

/// Re-register a previously shared file (called on app startup)
#[tauri::command]
async fn register_shared_file(
    state: tauri::State<'_, AppState>,
    file_hash: String,
    file_path: String,
    file_name: String,
    file_size: u64,
    price_chi: Option<String>,
    wallet_address: Option<String>,
) -> Result<(), String> {
    println!(
        "Re-registering shared file: {} (hash: {})",
        file_name, file_hash
    );

    // Verify file still exists
    if !std::path::Path::new(&file_path).exists() {
        return Err(format!("File no longer exists: {}", file_path));
    }

    // Parse price from CHI to wei
    let price_wei = if let Some(ref price) = price_chi {
        if price.is_empty() || price == "0" {
            0u128
        } else {
            wallet::parse_chi_to_wei(price)?
        }
    } else {
        0u128
    };
    let wallet_addr = wallet_address.unwrap_or_default();

    // Get DHT service
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        dht.register_shared_file(
            file_hash,
            file_path,
            file_name,
            file_size,
            price_wei,
            wallet_addr,
        )
        .await;
        Ok(())
    } else {
        // DHT not running yet - this is okay, will be registered when DHT starts
        println!("DHT not running, file will be registered when DHT starts");
        Ok(())
    }
}

/// Re-register a shared file AND update DHT metadata with our peer_id (called on app startup)
#[tauri::command]
async fn republish_shared_file(
    state: tauri::State<'_, AppState>,
    file_hash: String,
    file_path: String,
    file_name: String,
    file_size: u64,
    price_chi: Option<String>,
    wallet_address: Option<String>,
) -> Result<(), String> {
    println!(
        "Re-publishing shared file: {} (hash: {})",
        file_name, file_hash
    );

    // Verify file still exists
    if !std::path::Path::new(&file_path).exists() {
        return Err(format!("File no longer exists: {}", file_path));
    }

    let price_wei = if let Some(ref price) = price_chi {
        if price.is_empty() || price == "0" {
            0u128
        } else {
            wallet::parse_chi_to_wei(price)?
        }
    } else {
        0u128
    };
    let wallet_addr = wallet_address.unwrap_or_default();

    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        // Step 1: Register in shared_files map (same as register_shared_file)
        dht.register_shared_file(
            file_hash.clone(),
            file_path,
            file_name.clone(),
            file_size,
            price_wei,
            wallet_addr.clone(),
        )
        .await;

        // Step 2: Update DHT metadata — add ourselves to seeders list
        let peer_id = dht.get_peer_id().await.unwrap_or_default();
        if !peer_id.is_empty() {
            let dht_key = format!("chiral_file_{}", file_hash);

            let our_multiaddrs = dht.get_listening_addresses().await;
            let our_seeder = SeederInfo {
                peer_id: peer_id.clone(),
                price_wei: price_wei.to_string(),
                wallet_address: wallet_addr.clone(),
                multiaddrs: our_multiaddrs,
            signature: String::new(),
            };

            // Read existing metadata or create fresh
            let mut metadata = match dht.get_dht_value(dht_key.clone()).await {
                Ok(Some(json)) => {
                    serde_json::from_str::<FileMetadata>(&json).unwrap_or_else(|_| FileMetadata {
                        hash: file_hash.clone(),
                        file_name: file_name.clone(),
                        file_size,
                        protocol: "WebRTC".to_string(),
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        peer_id: String::new(),
                        price_wei: String::new(),
                        wallet_address: String::new(),
                        seeders: Vec::new(),
                publisher_signature: String::new(),
                    })
                }
                _ => FileMetadata {
                    hash: file_hash.clone(),
                    file_name: file_name.clone(),
                    file_size,
                    protocol: "WebRTC".to_string(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    peer_id: String::new(),
                    price_wei: String::new(),
                    wallet_address: String::new(),
                    seeders: Vec::new(),
                publisher_signature: String::new(),
                },
            };

            // Upsert our seeder entry
            if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
                existing.price_wei = our_seeder.price_wei.clone();
                existing.wallet_address = our_seeder.wallet_address.clone();
                existing.multiaddrs = our_seeder.multiaddrs.clone();
            } else {
                metadata.seeders.push(our_seeder);
            }
            metadata.peer_id = peer_id.clone();

            let metadata_json = serde_json::to_string(&metadata)
                .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
            let _ = dht.put_dht_value(dht_key, metadata_json).await;
            println!(
                "✅ Re-published {} to DHT with peer_id {} ({} seeders)",
                file_hash,
                peer_id,
                metadata.seeders.len()
            );
        }

        Ok(())
    } else {
        println!("DHT not running, file will be registered when DHT starts");
        Ok(())
    }
}

/// Remove our peer_id from all shared file DHT records (called on app shutdown)
#[tauri::command]
async fn unpublish_all_shared_files(state: tauri::State<'_, AppState>) -> Result<u32, String> {
    let dht_guard = state.dht.lock().await;
    let dht = match dht_guard.as_ref() {
        Some(d) => d,
        None => return Ok(0),
    };

    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    if peer_id.is_empty() {
        return Ok(0);
    }

    let shared = dht.get_shared_files();
    let file_hashes: Vec<String> = {
        let map = shared.lock().await;
        map.keys().cloned().collect()
    };

    println!(
        "🛑 Unpublishing {} shared files from DHT",
        file_hashes.len()
    );

    let mut count = 0u32;
    for file_hash in &file_hashes {
        let dht_key = format!("chiral_file_{}", file_hash);
        match dht.get_dht_value(dht_key.clone()).await {
            Ok(Some(json)) => {
                if let Ok(mut metadata) = serde_json::from_str::<FileMetadata>(&json) {
                    // Remove our peer_id from seeders list (preserve other seeders)
                    metadata.seeders.retain(|s| s.peer_id != peer_id);
                    // Clear legacy peer_id if it was us
                    if metadata.peer_id == peer_id {
                        metadata.peer_id = String::new();
                    }
                    if let Ok(updated_json) = serde_json::to_string(&metadata) {
                        let _ = dht.put_dht_value(dht_key, updated_json).await;
                        count += 1;
                    }
                }
            }
            _ => {}
        }
    }

    println!(
        "✅ Unpublished {} files from DHT (removed our seeder entry)",
        count
    );
    Ok(count)
}

/// Force-quit the application process (called after cleanup is done)
#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Get files we're hosting on behalf of other peers (from active agreements)
#[tauri::command]
async fn get_active_hosted_files(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let dht_guard = state.dht.lock().await;
    let my_peer_id = if let Some(dht) = dht_guard.as_ref() {
        dht.get_peer_id().await.unwrap_or_default()
    } else {
        return Ok(vec![]);
    };
    drop(dht_guard);

    if my_peer_id.is_empty() {
        return Ok(vec![]);
    }

    let dir = agreements_dir()?;
    let mut hosted_files = Vec::new();

    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("Failed to read agreements dir: {e}"))?;

    for entry in entries.flatten() {
        if let Ok(json) = std::fs::read_to_string(entry.path()) {
            if let Ok(agreement) = serde_json::from_str::<serde_json::Value>(&json) {
                let status = agreement
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let host_peer = agreement
                    .get("hostPeerId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let agreement_id = agreement
                    .get("agreementId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let client_peer = agreement
                    .get("clientPeerId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let expires_at = agreement.get("expiresAt").and_then(|v| v.as_u64());

                if (status == "active" || status == "accepted") && host_peer == my_peer_id {
                    if let Some(hashes) = agreement.get("fileHashes").and_then(|v| v.as_array()) {
                        for hash in hashes {
                            if let Some(h) = hash.as_str() {
                                hosted_files.push(serde_json::json!({
                                    "fileHash": h,
                                    "agreementId": agreement_id,
                                    "clientPeerId": client_peer,
                                    "expiresAt": expires_at,
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(hosted_files)
}

/// Clean up files for a cancelled hosting agreement: unregister from DHT, remove from cache, delete from disk.
#[tauri::command]
async fn cleanup_agreement_files(
    state: tauri::State<'_, AppState>,
    agreement_id: String,
) -> Result<(), String> {
    let dir = agreements_dir()?;
    let path = dir.join(format!("{}.json", agreement_id));
    let json =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read agreement: {e}"))?;
    let agreement: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("Failed to parse agreement: {e}"))?;

    let file_hashes: Vec<String> = agreement
        .get("fileHashes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if file_hashes.is_empty() {
        return Ok(());
    }

    // Get download directory for file deletion
    let download_dir = {
        let dir_lock = state.download_directory.lock().await;
        dir_lock.clone().unwrap_or_else(|| {
            dirs::download_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        })
    };

    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let peer_id = dht.get_peer_id().await.unwrap_or_default();
        let shared = dht.get_shared_files();
        for file_hash in &file_hashes {
            // Remove from SharedFilesMap (stops serving chunks)
            {
                let mut map = shared.lock().await;
                map.remove(file_hash);
            }

            // Remove ourselves from the seeder list in DHT
            let dht_key = format!("chiral_file_{}", file_hash);
            if let Ok(Some(meta_json)) = dht.get_dht_value(dht_key.clone()).await {
                if let Ok(mut metadata) = serde_json::from_str::<FileMetadata>(&meta_json) {
                    metadata.seeders.retain(|s| s.peer_id != peer_id);
                    if metadata.peer_id == peer_id {
                        metadata.peer_id = String::new();
                    }
                    if let Ok(updated) = serde_json::to_string(&metadata) {
                        let _ = dht.put_dht_value(dht_key, updated).await;
                    }
                }
            }

            // Also disable seeding in Drive manifest
            {
                let mut m = state.drive_state.manifest.write().await;
                if let Some(item) = m.items.iter_mut().find(|i| {
                    i.item_type == "file"
                        && i.merkle_root.as_ref().map(|h| h == file_hash).unwrap_or(false)
                }) {
                    item.seed_enabled = false;
                    item.seeding = false;
                }
            }

            println!("🗑️ Cleaned up hosted file: {}", file_hash);
        }
    }
    state.drive_state.persist().await;

    // Remove from in-memory file storage
    {
        let mut storage = state.file_storage.lock().await;
        for file_hash in &file_hashes {
            storage.remove(file_hash);
        }
    }

    // Delete files from download directory
    for file_hash in &file_hashes {
        let file_path = std::path::Path::new(&download_dir).join(file_hash);
        if file_path.exists() {
            let _ = std::fs::remove_file(&file_path);
            println!("🗑️ Deleted hosted file from disk: {}", file_hash);
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TorrentInfo {
    info_hash: String,
    name: String,
    size: u64,
}

#[tauri::command]
async fn parse_torrent_file(file_path: String) -> Result<TorrentInfo, String> {
    // Read the torrent file
    let torrent_data =
        std::fs::read(&file_path).map_err(|e| format!("Failed to read torrent file: {}", e))?;

    let mut name = String::new();
    let mut size: u64 = 0;
    let mut info_hash = String::new();

    // Simple bencode parsing for torrent files
    // Look for common patterns in torrent files

    // Find "name" field: look for pattern like "4:name" followed by length:string
    if let Some(name_pos) = find_bencode_key(&torrent_data, b"name") {
        if let Some(extracted) = extract_bencode_string(&torrent_data[name_pos..]) {
            name = extracted;
        }
    }

    // Find "length" field for single-file torrents
    if let Some(len_pos) = find_bencode_key(&torrent_data, b"length") {
        if let Some(len) = extract_bencode_integer(&torrent_data[len_pos..]) {
            size = len;
        }
    }

    // For Chiral Network torrents, the original file hash is stored in the "pieces" field
    // Extract it directly instead of computing a hash of the info dictionary
    if let Some(pieces_pos) = find_bencode_key(&torrent_data, b"pieces") {
        // The pieces field contains raw bytes (the hash), extract them
        if let Some(hash_bytes) = extract_bencode_bytes(&torrent_data[pieces_pos..]) {
            // Convert bytes to hex string
            info_hash = hex::encode(&hash_bytes);
            println!(
                "Extracted file hash from torrent pieces field: {}",
                info_hash
            );
        }
    }

    // If we couldn't extract the hash from pieces, this might be a standard BitTorrent torrent
    // In that case, we can't use it with our network
    if info_hash.is_empty() {
        return Err(
            "Invalid torrent file: could not find Chiral Network file hash in pieces field"
                .to_string(),
        );
    }

    if name.is_empty() {
        // Extract name from filename if not in torrent
        name = std::path::Path::new(&file_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
    }

    println!(
        "Parsed torrent: name={}, size={}, hash={}",
        name, size, info_hash
    );

    Ok(TorrentInfo {
        info_hash,
        name,
        size,
    })
}

// Helper function to find a bencode key in data
fn find_bencode_key(data: &[u8], key: &[u8]) -> Option<usize> {
    let key_len = key.len();
    let pattern = format!("{}:{}", key_len, String::from_utf8_lossy(key));
    let pattern_bytes = pattern.as_bytes();

    data.windows(pattern_bytes.len())
        .position(|w| w == pattern_bytes)
        .map(|p| p + pattern_bytes.len())
}

// Helper to extract a bencode string (format: length:string)
fn extract_bencode_string(data: &[u8]) -> Option<String> {
    // Find the length prefix
    let colon_pos = data.iter().position(|&b| b == b':')?;
    let len_str = std::str::from_utf8(&data[..colon_pos]).ok()?;
    let len: usize = len_str.parse().ok()?;

    if data.len() > colon_pos + 1 + len {
        let start = colon_pos + 1;
        let string_bytes = &data[start..start + len];
        Some(String::from_utf8_lossy(string_bytes).to_string())
    } else {
        None
    }
}

// Helper to extract a bencode integer (format: iNNNNe)
fn extract_bencode_integer(data: &[u8]) -> Option<u64> {
    if data.first() != Some(&b'i') {
        return None;
    }

    let end_pos = data.iter().position(|&b| b == b'e')?;
    let num_str = std::str::from_utf8(&data[1..end_pos]).ok()?;
    num_str.parse().ok()
}

// Helper to extract raw bytes from a bencode string (format: length:bytes)
fn extract_bencode_bytes(data: &[u8]) -> Option<Vec<u8>> {
    // Find the length prefix
    let colon_pos = data.iter().position(|&b| b == b':')?;
    let len_str = std::str::from_utf8(&data[..colon_pos]).ok()?;
    let len: usize = len_str.parse().ok()?;

    if data.len() >= colon_pos + 1 + len {
        let start = colon_pos + 1;
        Some(data[start..start + len].to_vec())
    } else {
        None
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportTorrentResult {
    path: String,
}

// Re-export wallet types used by AppState and Tauri commands
use wallet::TransactionMeta;

#[tauri::command]
async fn get_wallet_balance(
    address: String,
) -> Result<wallet::WalletBalanceResult, String> {
    let endpoint = geth::effective_rpc_endpoint();
    wallet::get_balance(&endpoint, &address).await
}



/// Send a transaction from one address to another (signs locally)
#[tauri::command]
async fn send_transaction(
    from_address: String,
    to_address: String,
    amount: String,
    private_key: String,
) -> Result<wallet::SendTransactionResult, String> {
    let endpoint = geth::effective_rpc_endpoint();
    wallet::send_transaction(&endpoint, &from_address, &to_address, &amount, &private_key).await
}

#[tauri::command]
async fn get_transaction_receipt(tx_hash: String) -> Result<Option<serde_json::Value>, String> {
    let endpoint = geth::effective_rpc_endpoint();
    wallet::get_receipt(&endpoint, &tx_hash).await
}

#[tauri::command]
async fn request_faucet(address: String) -> Result<wallet::SendTransactionResult, String> {
    wallet::request_faucet(&address).await
}


#[tauri::command]
async fn get_transaction_history(
    state: tauri::State<'_, AppState>,
    address: String,
) -> Result<wallet::TransactionHistoryResult, String> {
    let endpoint = geth::effective_rpc_endpoint();
    let metadata = { state.tx_metadata.lock().await.clone() };
    wallet::get_transaction_history(&endpoint, &address, &metadata).await
}

#[tauri::command]
async fn record_transaction_meta(
    state: tauri::State<'_, AppState>,
    tx_hash: String,
    tx_type: String,
    description: String,
    recipient_label: Option<String>,
    balance_before: Option<String>,
    balance_after: Option<String>,
) -> Result<(), String> {
    let meta = TransactionMeta {
        tx_hash: tx_hash.clone(), tx_type, description,
        file_name: None, file_hash: None, speed_tier: None,
        recipient_label, balance_before, balance_after,
    };
    let mut metadata = state.tx_metadata.lock().await;
    wallet::record_meta(&mut metadata, meta);
    Ok(())
}


#[tauri::command]
async fn export_torrent_file(
    file_hash: String,
    file_name: String,
    file_size: u64,
    file_path: String,
) -> Result<ExportTorrentResult, String> {
    // Create a simple bencode-formatted torrent file
    // This is a simplified torrent format for our network

    // Get the downloads directory for saving the torrent
    let downloads_dir =
        dirs::download_dir().ok_or_else(|| "Could not find downloads directory".to_string())?;

    // Create torrent filename
    let torrent_filename = format!("{}.torrent", file_name);
    let torrent_path = downloads_dir.join(&torrent_filename);

    // Build a simple bencode torrent structure
    // Format: d8:announce<url>4:infod6:length<size>4:name<name>12:piece length<piece_len>6:pieces<hash>ee

    // Our tracker URL (using DHT, but we include a placeholder)
    let announce = "udp://dht.chiral.network:6881/announce";

    // Piece length (256KB is common)
    let piece_length: u64 = 262144;

    // Build the torrent file content
    let mut torrent_content = Vec::new();

    // Start dictionary
    torrent_content.push(b'd');

    // Announce URL
    let announce_key = format!("8:announce{}:{}", announce.len(), announce);
    torrent_content.extend_from_slice(announce_key.as_bytes());

    // Created by
    let created_by = "chiral-network";
    let created_by_entry = format!("10:created by{}:{}", created_by.len(), created_by);
    torrent_content.extend_from_slice(created_by_entry.as_bytes());

    // Creation date (Unix timestamp)
    let creation_date = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let creation_date_entry = format!("13:creation datei{}e", creation_date);
    torrent_content.extend_from_slice(creation_date_entry.as_bytes());

    // Info dictionary
    torrent_content.extend_from_slice(b"4:infod");

    // File hash (our merkle root as the "pieces" field)
    // In real BitTorrent this would be SHA1 hashes of pieces
    let hash_bytes = hex::decode(&file_hash).map_err(|e| format!("Invalid hash: {}", e))?;
    let pieces_entry = format!("6:pieces{}:", hash_bytes.len());
    torrent_content.extend_from_slice(pieces_entry.as_bytes());
    torrent_content.extend_from_slice(&hash_bytes);

    // File length
    let length_entry = format!("6:lengthi{}e", file_size);
    torrent_content.extend_from_slice(length_entry.as_bytes());

    // File name
    let name_entry = format!("4:name{}:{}", file_name.len(), file_name);
    torrent_content.extend_from_slice(name_entry.as_bytes());

    // Piece length
    let piece_length_entry = format!("12:piece lengthi{}e", piece_length);
    torrent_content.extend_from_slice(piece_length_entry.as_bytes());

    // Source path (custom field for our network)
    let source_path_key = "11:source path";
    let source_entry = format!("{}{}:{}", source_path_key, file_path.len(), file_path);
    torrent_content.extend_from_slice(source_entry.as_bytes());

    // End info dictionary
    torrent_content.push(b'e');

    // End main dictionary
    torrent_content.push(b'e');

    // Write the torrent file
    std::fs::write(&torrent_path, &torrent_content)
        .map_err(|e| format!("Failed to write torrent file: {}", e))?;

    println!("Exported torrent file: {}", torrent_path.display());

    Ok(ExportTorrentResult {
        path: torrent_path.to_string_lossy().to_string(),
    })
}

/// Open a file with the system default application
#[tauri::command]
async fn open_file(path: String) -> Result<(), String> {
    let file_path = std::path::Path::new(&path);
    if !file_path.exists() {
        return Err(format!("File not found: {}", path));
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path])
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }

    Ok(())
}

/// Show a file in the system file manager
#[tauri::command]
async fn show_in_folder(path: String) -> Result<(), String> {
    let file_path = std::path::Path::new(&path);
    if !file_path.exists() {
        return Err(format!("File not found: {}", path));
    }

    #[cfg(target_os = "linux")]
    {
        // Try xdg-open on the parent directory; dbus method would be better but this is simpler
        if let Some(parent) = file_path.parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| format!("Failed to open folder: {}", e))?;
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("Failed to reveal in Finder: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("Failed to reveal in Explorer: {}", e))?;
    }

    Ok(())
}

/// Show a Drive item in the system file manager
#[tauri::command]
async fn show_drive_item_in_folder(owner: String, item_id: String) -> Result<(), String> {
    let manifest = ds::load_manifest();
    let item = manifest
        .items
        .iter()
        .find(|i| i.id == item_id && i.owner == owner)
        .ok_or("Item not found")?;

    let path = if item.item_type == "folder" {
        // For folders, open the Drive files directory
        ds::drive_files_dir().ok_or("Cannot determine storage directory")?
    } else {
        let storage = item
            .storage_path
            .as_ref()
            .ok_or("No storage path for this item")?;
        ds::drive_files_dir()
            .ok_or("Cannot determine storage directory")?
            .join(storage)
    };

    if !path.exists() {
        return Err(format!("File not found on disk: {}", path.display()));
    }

    let path_str = path.to_string_lossy().to_string();
    show_in_folder(path_str).await
}

#[tauri::command]
async fn get_drive_file_path(owner: String, item_id: String) -> Result<String, String> {
    let manifest = ds::load_manifest();
    let item = manifest
        .items
        .iter()
        .find(|i| i.id == item_id && i.owner == owner)
        .ok_or("Item not found")?;

    let storage = item
        .storage_path
        .as_ref()
        .ok_or("No storage path for this item")?;
    let path = ds::drive_files_dir()
        .ok_or("Cannot determine storage directory")?
        .join(storage);

    if !path.exists() {
        return Err(format!("File not found on disk: {}", path.display()));
    }

    Ok(path.to_string_lossy().to_string())
}

// ============================================================================
// Geth Commands
// ============================================================================

#[tauri::command]
async fn is_geth_installed() -> Result<bool, String> {
    let downloader = GethDownloader::new();
    Ok(downloader.is_geth_installed())
}

#[tauri::command]
async fn download_geth(app: tauri::AppHandle) -> Result<(), String> {
    let downloader = GethDownloader::new();

    downloader
        .download_geth(move |progress| {
            let _ = app.emit("geth-download-progress", &progress);
        })
        .await
}

#[tauri::command]
async fn start_geth(
    state: tauri::State<'_, AppState>,
    miner_address: Option<String>,
) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.start(miner_address.as_deref()).await
}

#[tauri::command]
async fn stop_geth(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.stop()
}

#[tauri::command]
async fn get_geth_status(state: tauri::State<'_, AppState>) -> Result<GethStatus, String> {
    let mut geth = state.geth.lock().await;
    geth.get_status().await
}

#[tauri::command]
async fn start_mining(
    state: tauri::State<'_, AppState>,
    threads: Option<u32>,
) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.start_mining(threads.unwrap_or(1)).await
}

#[tauri::command]
async fn stop_mining(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.stop_mining().await
}

#[tauri::command]
async fn get_mining_status(state: tauri::State<'_, AppState>) -> Result<MiningStatus, String> {
    let mut geth = state.geth.lock().await;
    geth.get_mining_status().await
}

#[tauri::command]
async fn get_gpu_mining_capabilities(
    state: tauri::State<'_, AppState>,
) -> Result<GpuMiningCapabilities, String> {
    let mut geth = state.geth.lock().await;
    geth.get_gpu_mining_capabilities().await
}

#[tauri::command]
async fn list_gpu_devices(state: tauri::State<'_, AppState>) -> Result<Vec<GpuDevice>, String> {
    let mut geth = state.geth.lock().await;
    geth.list_gpu_devices().await
}

#[tauri::command]
async fn start_gpu_mining(
    state: tauri::State<'_, AppState>,
    device_ids: Option<Vec<String>>,
    utilization_percent: Option<u8>,
) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.start_gpu_mining(device_ids, utilization_percent).await
}

#[tauri::command]
async fn stop_gpu_mining(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.stop_gpu_mining().await
}

#[tauri::command]
async fn get_gpu_mining_status(
    state: tauri::State<'_, AppState>,
) -> Result<GpuMiningStatus, String> {
    let mut geth = state.geth.lock().await;
    geth.get_gpu_mining_status().await
}



#[tauri::command]
async fn set_miner_address(
    state: tauri::State<'_, AppState>,
    address: String,
) -> Result<(), String> {
    let geth = state.geth.lock().await;
    geth.set_miner_address(&address).await
}

#[tauri::command]
fn get_chain_id() -> u64 {
    geth::CHAIN_ID
}

// ============================================================================
// Wallet Backup Email Command
// ============================================================================

/// Send wallet backup email via the relay server.
/// The relay has SMTP configured server-side — no user configuration needed.
#[tauri::command]
async fn send_wallet_backup_email(
    email: String,
    recovery_phrase: String,
    wallet_address: String,
    private_key: String,
) -> Result<(), String> {
    const RELAY_URL: &str = "http://130.245.173.73:8080/api/wallet/backup-email";

    let payload = serde_json::json!({
        "email": email.trim(),
        "recoveryPhrase": recovery_phrase,
        "walletAddress": wallet_address,
        "privateKey": private_key,
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .post(RELAY_URL)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to reach email server: {}", e))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(if body.is_empty() {
            format!("Email server returned error (HTTP {})", status.as_u16())
        } else {
            body
        })
    }
}

// ============================================================================
// Diagnostics Commands
// ============================================================================

/// Read the last N lines of the Geth log file
#[tauri::command]
fn read_geth_log(lines: Option<usize>) -> Result<String, String> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("chiral-network")
        .join("geth");
    let log_path = data_dir.join("geth.log");
    if !log_path.exists() {
        return Ok("No geth.log found".to_string());
    }
    let contents = std::fs::read_to_string(&log_path)
        .map_err(|e| format!("Failed to read geth.log: {}", e))?;
    let max_lines = lines.unwrap_or(100);
    let all_lines: Vec<&str> = contents.lines().collect();
    let start = if all_lines.len() > max_lines {
        all_lines.len() - max_lines
    } else {
        0
    };
    Ok(all_lines[start..].join("\n"))
}

// ============================================================================
// Bootstrap Health Commands
// ============================================================================

/// Check health of all bootstrap nodes
#[tauri::command]
async fn check_bootstrap_health() -> Result<BootstrapHealthReport, String> {
    Ok(geth_bootstrap::check_all_nodes().await)
}

/// Get cached bootstrap health report (faster, no network calls)
#[tauri::command]
async fn get_bootstrap_health() -> Result<Option<BootstrapHealthReport>, String> {
    Ok(geth_bootstrap::get_cached_report().await)
}

// ============================================================================
// Encryption Commands
// ============================================================================

/// Initialize encryption keypair (derived from wallet private key for consistency)
#[tauri::command]
async fn init_encryption_keypair(
    state: tauri::State<'_, AppState>,
    wallet_private_key: String,
) -> Result<String, String> {
    let pk_bytes =
        hex::decode(&wallet_private_key).map_err(|e| format!("Invalid private key hex: {}", e))?;

    let keypair = EncryptionKeypair::from_wallet_key(&pk_bytes);
    let public_key_hex = keypair.public_key_hex();

    let mut keypair_guard = state.encryption_keypair.lock().await;
    *keypair_guard = Some(keypair);

    Ok(public_key_hex)
}

/// Get our encryption public key (for sharing with others)
#[tauri::command]
async fn get_encryption_public_key(
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    let keypair_guard = state.encryption_keypair.lock().await;
    Ok(keypair_guard.as_ref().map(|k| k.public_key_hex()))
}

/// Encrypt file data for a recipient
#[tauri::command]
async fn encrypt_file_for_recipient(
    recipient_public_key: String,
    file_data: Vec<u8>,
) -> Result<encryption::EncryptedFileBundle, String> {
    encryption::encrypt_for_recipient_hex(&file_data, &recipient_public_key)
}

/// Decrypt file data using our keypair
#[tauri::command]
async fn decrypt_file_data(
    state: tauri::State<'_, AppState>,
    encrypted_bundle: encryption::EncryptedFileBundle,
) -> Result<Vec<u8>, String> {
    let keypair_guard = state.encryption_keypair.lock().await;
    let keypair = keypair_guard
        .as_ref()
        .ok_or("Encryption keypair not initialized")?;

    encryption::decrypt_with_keypair(&encrypted_bundle, keypair)
}

/// Send an encrypted file to a peer
#[tauri::command]
async fn send_encrypted_file(
    state: tauri::State<'_, AppState>,
    peer_id: String,
    file_name: String,
    file_data: Vec<u8>,
    recipient_public_key: String,
    transfer_id: String,
) -> Result<(), String> {
    // Encrypt the file for the recipient
    let encrypted_bundle =
        encryption::encrypt_for_recipient_hex(&file_data, &recipient_public_key)?;

    // Serialize the encrypted bundle to JSON for transmission
    let encrypted_json = serde_json::to_vec(&encrypted_bundle)
        .map_err(|e| format!("Failed to serialize encrypted bundle: {}", e))?;

    // Send via DHT
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        // Prefix file name with .encrypted to indicate it's encrypted
        let encrypted_file_name = format!("{}.encrypted", file_name);
        let size = encrypted_json.len() as u64;
        dht.send_file(
            peer_id,
            transfer_id,
            encrypted_file_name,
            encrypted_json,
            String::new(),
            String::new(),
            String::new(),
            size,
        )
        .await
    } else {
        Err("DHT not running".to_string())
    }
}

/// Publish a peer's encryption public key to the DHT (for discovery)
#[tauri::command]
async fn publish_encryption_key(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let keypair_guard = state.encryption_keypair.lock().await;
    let keypair = keypair_guard
        .as_ref()
        .ok_or("Encryption keypair not initialized")?;

    let public_key = keypair.public_key_hex();
    drop(keypair_guard);

    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let peer_id = dht.get_peer_id().await.ok_or("Peer ID not available")?;
        let key = format!("chiral_pubkey_{}", peer_id);
        dht.put_dht_value(key, public_key).await
    } else {
        Err("DHT not running".to_string())
    }
}

/// Lookup a peer's encryption public key from the DHT
#[tauri::command]
async fn lookup_encryption_key(
    state: tauri::State<'_, AppState>,
    peer_id: String,
) -> Result<Option<String>, String> {
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let key = format!("chiral_pubkey_{}", peer_id);
        dht.get_dht_value(key).await
    } else {
        Err("DHT not running".to_string())
    }
}

// ---------------------------------------------------------------------------
// Hosting commands
// ---------------------------------------------------------------------------

/// Compute the common path prefix between two paths.
fn common_prefix(a: &Path, b: &Path) -> PathBuf {
    let mut iter_a = a.components();
    let mut iter_b = b.components();
    let mut out = PathBuf::new();

    loop {
        match (iter_a.next(), iter_b.next()) {
            (Some(ca), Some(cb)) if ca == cb && ca != Component::CurDir => out.push(ca.as_os_str()),
            _ => break,
        }
    }

    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostingServerStatus {
    running: bool,
    address: Option<String>,
}

#[tauri::command]
async fn create_hosted_site(
    state: tauri::State<'_, AppState>,
    name: String,
    file_paths: Vec<String>,
) -> Result<hosting::HostedSite, String> {
    let site_id = hosting::generate_site_id();
    let base =
        hosting::sites_base_dir().ok_or_else(|| "Cannot determine data directory".to_string())?;
    let site_dir = base.join(&site_id);
    std::fs::create_dir_all(&site_dir).map_err(|e| format!("Failed to create site dir: {}", e))?;

    // Preserve folder structure so relative asset paths keep working (e.g., "images/photo.jpg").
    // We compute the common parent across the selected files and store paths relative to it.
    let src_paths: Vec<std::path::PathBuf> = file_paths.iter().map(std::path::PathBuf::from).collect();
    let common_root = src_paths
        .iter()
        .filter_map(|p| p.parent())
        .fold(None, |acc: Option<std::path::PathBuf>, next| match acc {
            None => Some(next.to_path_buf()),
            Some(curr) => Some(common_prefix(&curr, next)),
        })
        .unwrap_or_else(|| std::path::PathBuf::from(""));

    let mut site_files = Vec::new();

    for src in &src_paths {
        if !src.exists() {
            // Clean up on error
            let _ = std::fs::remove_dir_all(&site_dir);
            return Err(format!("File not found: {}", src.display()));
        }
        // Path inside the hosted site
        let rel_path = src
            .strip_prefix(&common_root)
            .unwrap_or_else(|_| Path::new(src.file_name().unwrap()))
            .to_path_buf();

        if rel_path.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&site_dir);
            return Err("Invalid relative path computed for uploaded file".to_string());
        }

        let dest = site_dir.join(&rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create folder {}: {}", parent.display(), e))?;
        }

        std::fs::copy(src, &dest)
            .map_err(|e| format!("Failed to copy {}: {}", rel_path.display(), e))?;

        let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        site_files.push(hosting::SiteFile {
            // Normalize separators for serving in URLs
            path: rel_path.to_string_lossy().replace('\\', "/"),
            size,
        });
    }

    // If no index.html was provided, generate a simple one that links to or embeds the first file.
    let has_index = site_files
        .iter()
        .any(|f| f.path.eq_ignore_ascii_case("index.html"));
    if !has_index {
        let first = site_files.get(0).map(|f| f.path.clone()).unwrap_or_default();
        let mut html = String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Hosted Site</title></head><body style=\"margin:0;padding:0;display:flex;align-items:center;justify-content:center;height:100vh;background:#111;color:#eee;font-family:sans-serif;\">\n");
        if !first.is_empty() {
            let lower = first.to_ascii_lowercase();
            if lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png") || lower.ends_with(".gif") || lower.ends_with(".webp") || lower.ends_with(".bmp") || lower.ends_with(".svg") {
                html.push_str(&format!("<img src=\"./{}\" alt=\"Hosted image\" style=\"max-width:100%;max-height:100%;object-fit:contain;\"/>", first));
            } else {
                html.push_str(&format!("<a href=\"./{0}\" style=\"color:#4ade80;font-size:1.1rem;\">Open {0}</a>", first));
            }
        } else {
            html.push_str("<p>No content uploaded.</p>");
        }
        html.push_str("</body></html>");

        let index_path = site_dir.join("index.html");
        std::fs::write(&index_path, html).map_err(|e| format!("Failed to write index.html: {}", e))?;
        let size = std::fs::metadata(&index_path).map(|m| m.len()).unwrap_or(0);
        site_files.push(hosting::SiteFile {
            path: "index.html".into(),
            size,
        });
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let site = hosting::HostedSite {
        id: site_id,
        name,
        directory: site_dir.to_string_lossy().into_owned(),
        created_at: now,
        files: site_files,
        relay_url: None,
    };

    // Persist to disk
    let mut all_sites = hosting::load_sites();
    all_sites.push(site.clone());
    hosting::save_sites(&all_sites);

    // Register with the running server
    state.hosting_server_state.register_site(site.clone()).await;

    println!("Created hosted site: {} ({})", site.name, site.id);
    Ok(site)
}

#[tauri::command]
async fn list_hosted_sites(
    _state: tauri::State<'_, AppState>,
) -> Result<Vec<hosting::HostedSite>, String> {
    Ok(hosting::load_sites())
}

#[tauri::command]
async fn delete_hosted_site(
    state: tauri::State<'_, AppState>,
    site_id: String,
) -> Result<(), String> {
    let mut all_sites = hosting::load_sites();
    let before_len = all_sites.len();
    all_sites.retain(|s| s.id != site_id);
    if all_sites.len() == before_len {
        return Err(format!("Site not found: {}", site_id));
    }
    hosting::save_sites(&all_sites);

    // Remove files from disk
    if let Some(base) = hosting::sites_base_dir() {
        let site_dir = base.join(&site_id);
        if site_dir.exists() {
            let _ = std::fs::remove_dir_all(&site_dir);
        }
    }

    // Unregister from server
    state.hosting_server_state.unregister_site(&site_id).await;

    println!("Deleted hosted site: {}", site_id);
    Ok(())
}

#[tauri::command]
async fn start_hosting_server(
    state: tauri::State<'_, AppState>,
    port: u16,
) -> Result<String, String> {
    // Check if already running
    {
        let addr = state.hosting_server_addr.lock().await;
        if addr.is_some() {
            return Err("Hosting server is already running".to_string());
        }
    }

    // Load sites into server state
    state.hosting_server_state.load_from_disk().await;

    let (tx, rx) = tokio::sync::oneshot::channel();

    let bound_addr =
        hosting_server::start_server(Arc::clone(&state.hosting_server_state), port, rx).await?;

    *state.hosting_server_addr.lock().await = Some(bound_addr);
    *state.hosting_server_shutdown.lock().await = Some(tx);

    Ok(bound_addr.to_string())
}

#[tauri::command]
async fn stop_hosting_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let shutdown_tx = state.hosting_server_shutdown.lock().await.take();
    if let Some(tx) = shutdown_tx {
        let _ = tx.send(());
    }
    *state.hosting_server_addr.lock().await = None;
    println!("Hosting server stopped");
    Ok(())
}

#[tauri::command]
async fn get_hosting_server_status(
    state: tauri::State<'_, AppState>,
) -> Result<HostingServerStatus, String> {
    let addr = state.hosting_server_addr.lock().await;
    Ok(HostingServerStatus {
        running: addr.is_some(),
        address: addr.map(|a| a.to_string()),
    })
}

// ---------------------------------------------------------------------------
// WebSocket tunnel to relay (NAT traversal for hosting/drive proxy)
// ---------------------------------------------------------------------------

/// Spawn a background task that maintains a WebSocket tunnel to the relay.
/// The relay forwards incoming visitor HTTP requests through this tunnel.
/// Returns the AbortHandle so the tunnel can be cancelled on unpublish.
fn spawn_relay_tunnel(
    relay_base: String,
    resource_type: String,
    resource_id: String,
    local_origin: String,
) -> tokio::task::AbortHandle {
    use futures_util::StreamExt;
    // Fix 0.0.0.0 — it's not a valid destination address for clients
    let local_origin = local_origin
        .replace("://0.0.0.0:", "://127.0.0.1:")
        .replace("://0.0.0.0/", "://127.0.0.1/");
    let handle = tokio::spawn(async move {
        loop {
            let ws_url = format!(
                "{}/api/tunnel/ws?type={}&id={}",
                relay_base
                    .replace("http://", "ws://")
                    .replace("https://", "wss://"),
                resource_type,
                resource_id
            );
            println!(
                "[TUNNEL] Connecting to {} for {}:{}",
                ws_url, resource_type, resource_id
            );

            match tokio_tungstenite::connect_async(&ws_url).await {
                Ok((ws_stream, _)) => {
                    println!("[TUNNEL] Connected for {}:{}", resource_type, resource_id);
                    let (mut ws_tx, mut ws_rx) = futures_util::StreamExt::split(ws_stream);

                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(30))
                        .build()
                        .unwrap_or_default();
                    let local = local_origin.clone();

                    while let Some(Ok(msg)) = ws_rx.next().await {
                        match msg {
                            tokio_tungstenite::tungstenite::Message::Text(text) => {
                                // Parse the tunnel request from the relay
                                #[derive(serde::Deserialize)]
                                struct TunnelReq {
                                    id: String,
                                    path: String,
                                }
                                let req: TunnelReq = match serde_json::from_str(&text) {
                                    Ok(r) => r,
                                    Err(_) => continue,
                                };

                                // Fetch from local server
                                let target = format!("{}{}", local, req.path);
                                let (status, headers, body_bytes): (
                                    u16,
                                    HashMap<String, String>,
                                    Vec<u8>,
                                ) = match client.get(&target).send().await {
                                    Ok(resp) => {
                                        let st = resp.status().as_u16();
                                        let mut hdr = HashMap::<String, String>::new();
                                        for (k, v) in resp.headers() {
                                            if let Ok(vs) = v.to_str() {
                                                hdr.insert(k.to_string(), vs.to_string());
                                            }
                                        }
                                        let bytes = resp.bytes().await.unwrap_or_default().to_vec();
                                        (st, hdr, bytes)
                                    }
                                    Err(_) => (502, HashMap::new(), b"Local server error".to_vec()),
                                };

                                // Send response back through the tunnel
                                use base64::Engine;
                                let resp_json = serde_json::json!({
                                    "id": req.id,
                                    "status": status,
                                    "headers": headers,
                                    "body": base64::engine::general_purpose::STANDARD.encode(&body_bytes),
                                });
                                let msg = tokio_tungstenite::tungstenite::Message::Text(
                                    resp_json.to_string(),
                                );
                                if futures_util::SinkExt::send(&mut ws_tx, msg).await.is_err() {
                                    break;
                                }
                            }
                            tokio_tungstenite::tungstenite::Message::Ping(data) => {
                                let _ = futures_util::SinkExt::send(
                                    &mut ws_tx,
                                    tokio_tungstenite::tungstenite::Message::Pong(data),
                                )
                                .await;
                            }
                            tokio_tungstenite::tungstenite::Message::Close(_) => break,
                            _ => {}
                        }
                    }
                    println!(
                        "[TUNNEL] Disconnected from relay for {}:{}",
                        resource_type, resource_id
                    );
                }
                Err(e) => {
                    println!(
                        "[TUNNEL] Failed to connect for {}:{}: {}",
                        resource_type, resource_id, e
                    );
                }
            }

            // Reconnect after a short delay
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
    handle.abort_handle()
}

#[tauri::command]
async fn publish_site_to_relay(
    state: tauri::State<'_, AppState>,
    site_id: String,
    relay_url: String,
) -> Result<String, String> {
    // Find the site in local metadata
    let mut all_sites = hosting::load_sites();
    let _site = all_sites
        .iter()
        .find(|s| s.id == site_id)
        .ok_or_else(|| format!("Site not found: {}", site_id))?
        .clone();

    // Get local server origin URL
    let origin = state
        .hosting_server_addr
        .lock()
        .await
        .map(|a| format!("http://{}", a))
        .ok_or("Local server not running")?;

    // Register site origin with relay (no file upload — relay will proxy)
    let relay_base = relay_url.trim_end_matches('/');
    let url = format!("{}/api/sites/relay-register", relay_base);

    let resp = rpc_client::client()
        .post(&url)
        .json(&serde_json::json!({
            "site_id": site_id,
            "origin_url": origin,
            "owner_wallet": "",
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to register site with relay: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Relay returned {}: {}", status, text));
    }

    // Start WebSocket tunnel BEFORE updating metadata — if tunnel fails, site stays unpublished
    let local_for_tunnel = state
        .hosting_server_addr
        .lock()
        .await
        .map(|a| format!("http://127.0.0.1:{}", a.port()))
        .ok_or("Local server not running")?;
    let tunnel_key = format!("site:{}", site_id);
    let abort_handle = spawn_relay_tunnel(
        relay_base.to_string(),
        "site".to_string(),
        site_id.clone(),
        local_for_tunnel,
    );
    state
        .tunnel_handles
        .lock()
        .await
        .insert(tunnel_key, abort_handle);

    // Only update metadata AFTER relay registration + tunnel are established
    let public_url = format!("{}/sites/{}/", relay_base, site_id);
    if let Some(s) = all_sites.iter_mut().find(|s| s.id == site_id) {
        s.relay_url = Some(public_url.clone());
    }
    hosting::save_sites(&all_sites);

    println!(
        "[HOSTING] Published site {} to relay: {}",
        site_id, public_url
    );
    Ok(public_url)
}

#[tauri::command]
async fn unpublish_site_from_relay(
    state: tauri::State<'_, AppState>,
    site_id: String,
) -> Result<(), String> {
    let mut all_sites = hosting::load_sites();
    let site = all_sites
        .iter()
        .find(|s| s.id == site_id)
        .ok_or_else(|| format!("Site not found: {}", site_id))?
        .clone();

    let relay_url = site
        .relay_url
        .as_ref()
        .ok_or_else(|| "Site is not published to a relay".to_string())?;

    // Extract the relay base URL from the full site URL
    // e.g. "http://130.245.173.73:8080/sites/abc12345/" -> "http://130.245.173.73:8080"
    let relay_base = relay_url
        .find("/sites/")
        .map(|pos| &relay_url[..pos])
        .ok_or_else(|| "Invalid relay URL format".to_string())?;

    let url = format!("{}/api/sites/relay-register/{}", relay_base, site_id);

    let resp = rpc_client::client()
        .delete(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to relay: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Relay returned {}: {}", status, text));
    }

    // Cancel the WebSocket tunnel
    let tunnel_key = format!("site:{}", site_id);
    if let Some(handle) = state.tunnel_handles.lock().await.remove(&tunnel_key) {
        handle.abort();
    }

    // Clear relay URL from local metadata
    if let Some(s) = all_sites.iter_mut().find(|s| s.id == site_id) {
        s.relay_url = None;
    }
    hosting::save_sites(&all_sites);

    println!("[HOSTING] Unpublished site {} from relay", site_id);
    Ok(())
}

// ---------------------------------------------------------------------------
// Drive CRUD commands (via Tauri invoke, bypasses browser HTTP restrictions)
// ---------------------------------------------------------------------------

use crate::drive_storage::{self as ds, DriveItem as DsItem, ShareLink as DsShareLink};

#[tauri::command]
async fn drive_list_items(
    state: tauri::State<'_, AppState>,
    owner: String,
    parent_id: Option<String>,
) -> Result<Vec<DsItem>, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    let m = state.drive_state.manifest.read().await;
    let parent = parent_id.as_deref();
    let mut items: Vec<DsItem> = m
        .items
        .iter()
        .filter(|i| i.parent_id.as_deref() == parent && i.owner == owner)
        .cloned()
        .collect();
    items.sort_by(|a, b| {
        if a.item_type != b.item_type {
            if a.item_type == "folder" {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });
    Ok(items)
}

/// List ALL Drive items for an owner (flat, ignoring folder hierarchy).
/// Used by hosted-file cleanup to find files regardless of which folder they're in.
#[tauri::command]
async fn drive_list_all_items(
    state: tauri::State<'_, AppState>,
    owner: String,
) -> Result<Vec<DsItem>, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    let m = state.drive_state.manifest.read().await;
    Ok(m.items
        .iter()
        .filter(|i| i.owner == owner)
        .cloned()
        .collect())
}

#[tauri::command]
async fn drive_create_folder(
    state: tauri::State<'_, AppState>,
    owner: String,
    name: String,
    parent_id: Option<String>,
) -> Result<DsItem, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    if name.is_empty() || name.len() > 255 {
        return Err("Invalid folder name".into());
    }
    let item = DsItem {
        id: ds::generate_id(),
        name,
        item_type: "folder".into(),
        parent_id,
        size: None,
        mime_type: None,
        created_at: ds::now_secs(),
        modified_at: ds::now_secs(),
        starred: false,
        storage_path: None,
        owner,
        is_public: true,
        merkle_root: None,
        protocol: None,
        price_chi: None,
        seed_enabled: false,
        seeding: false,
    };
    {
        let mut m = state.drive_state.manifest.write().await;
        m.items.push(item.clone());
    }
    state.drive_state.persist().await;
    Ok(item)
}

/// Read raw file bytes from Drive (for CDN upload).
/// Returns a binary IPC response (Tauri 2 transfers `tauri::ipc::Response` as
/// raw bytes, which is ~10× faster than the JSON `number[]` round-trip you'd
/// get from returning `Vec<u8>` directly).
#[tauri::command]
async fn drive_read_file_bytes(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
) -> Result<tauri::ipc::Response, String> {
    let storage_path = {
        let m = state.drive_state.manifest.read().await;
        let item = m.items.iter()
            .find(|i| i.id == item_id && i.owner == owner && i.item_type == "file")
            .ok_or("Drive file not found")?;
        item.storage_path.clone().ok_or("No storage path")?
    };
    let files_dir = ds::drive_files_dir().ok_or("Cannot determine drive files directory")?;
    let path = files_dir.join(&storage_path);
    let bytes = std::fs::read(&path).map_err(|e| format!("Failed to read file: {}", e))?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
async fn drive_upload_file(
    state: tauri::State<'_, AppState>,
    owner: String,
    file_path: String,
    parent_id: Option<String>,
    merkle_root: Option<String>,
) -> Result<DsItem, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    let src = std::path::Path::new(&file_path);
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid file path")?
        .to_string();
    let data = std::fs::read(src).map_err(|e| format!("Failed to read file: {}", e))?;

    if data.len() > 500 * 1024 * 1024 {
        return Err("File exceeds 500 MB limit".into());
    }

    let item_id = ds::generate_id();
    let storage_name = format!("{}_{}", item_id, file_name);
    let mime = ds::mime_from_name(&file_name);
    let files_dir = ds::drive_files_dir().ok_or("Cannot determine storage directory")?;
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("Failed to create storage dir: {}", e))?;
    let dest = files_dir.join(&storage_name);
    std::fs::write(&dest, &data).map_err(|e| format!("Failed to write file: {}", e))?;

    // Compute and persist file hash at upload time so later seeding can be instant.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let computed_merkle_root = hex::encode(hasher.finalize());

    let item = DsItem {
        id: item_id,
        name: file_name.clone(),
        item_type: "file".into(),
        parent_id,
        size: Some(data.len() as u64),
        mime_type: Some(mime),
        created_at: ds::now_secs(),
        modified_at: ds::now_secs(),
        starred: false,
        storage_path: Some(storage_name),
        owner,
        is_public: true,
        merkle_root: merkle_root.or(Some(computed_merkle_root)),
        protocol: None,
        price_chi: None,
        seed_enabled: false,
        seeding: false,
    };
    {
        let mut m = state.drive_state.manifest.write().await;
        m.items.push(item.clone());
    }
    state.drive_state.persist().await;
    println!(
        "[DRIVE] Uploaded file: {} ({} bytes)",
        file_name,
        data.len()
    );
    Ok(item)
}

#[tauri::command]
async fn drive_update_item(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
    name: Option<String>,
    parent_id: Option<String>,
    starred: Option<bool>,
    price_chi: Option<String>,
) -> Result<DsItem, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    let mut m = state.drive_state.manifest.write().await;
    let item = m
        .items
        .iter_mut()
        .find(|i| i.id == item_id && i.owner == owner)
        .ok_or("Item not found")?;
    if let Some(n) = name {
        if n.is_empty() || n.len() > 255 {
            return Err("Invalid name".into());
        }
        item.name = n;
    }
    if let Some(pid) = parent_id {
        item.parent_id = if pid.is_empty() { None } else { Some(pid) };
    }
    if let Some(s) = starred {
        item.starred = s;
    }
    if let Some(p) = price_chi {
        item.price_chi = if p.is_empty() { None } else { Some(p) };
    }
    item.modified_at = ds::now_secs();
    let updated = item.clone();
    drop(m);
    state.drive_state.persist().await;
    Ok(updated)
}

#[tauri::command]
async fn drive_delete_item(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
) -> Result<(), String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }

    // Snapshot owned items so we can do I/O without holding the manifest lock.
    let (to_delete, file_entries): (
        std::collections::HashSet<String>,
        Vec<(String, Option<String>, Option<String>)>,
    ) = {
        let m = state.drive_state.manifest.read().await;
        let owned_items: Vec<DsItem> = m
            .items
            .iter()
            .filter(|i| i.owner == owner)
            .cloned()
            .collect();

        if !owned_items.iter().any(|i| i.id == item_id) {
            return Err("Item not found".into());
        }

        let to_delete: std::collections::HashSet<String> =
            ds::collect_descendants(&item_id, &owned_items)
                .into_iter()
                .collect();

        let files = owned_items
            .iter()
            .filter(|i| to_delete.contains(&i.id) && i.item_type == "file")
            .map(|i| (i.id.clone(), i.storage_path.clone(), i.merkle_root.clone()))
            .collect::<Vec<_>>();

        (to_delete, files)
    };

    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };

    // Remove from active seeding + DHT seeder list so the file is no longer discoverable.
    for (_, _, merkle_root) in &file_entries {
        if let (Some(dht), Some(hash)) = (dht.as_ref(), merkle_root.as_ref()) {
            dht.unregister_shared_file(hash).await;

            // Remove ourselves from the DHT seeder list
            let peer_id = dht.get_peer_id().await.unwrap_or_default();
            if !peer_id.is_empty() {
                let dht_key = format!("chiral_file_{}", hash);
                if let Ok(Some(meta_json)) = dht.get_dht_value(dht_key.clone()).await {
                    if let Ok(mut metadata) = serde_json::from_str::<FileMetadata>(&meta_json) {
                        metadata.seeders.retain(|s| s.peer_id != peer_id);
                        if metadata.peer_id == peer_id {
                            metadata.peer_id = String::new();
                        }
                        if let Ok(updated) = serde_json::to_string(&metadata) {
                            let _ = dht.put_dht_value(dht_key, updated).await;
                        }
                    }
                }
            }
        }
    }
    if !file_entries.is_empty() {
        let mut storage = state.file_storage.lock().await;
        for (_, _, merkle_root) in &file_entries {
            if let Some(hash) = merkle_root {
                storage.remove(hash);
            }
        }
    }

    // Remove physical files from Drive storage.
    let mut delete_errors: Vec<String> = Vec::new();
    if let Some(files_dir) = ds::drive_files_dir() {
        for (id, storage_path, _) in &file_entries {
            let Some(sp) = storage_path.as_ref() else {
                continue;
            };
            let full_path = files_dir.join(sp);
            match std::fs::remove_file(&full_path) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    delete_errors.push(format!("{} (item {}): {}", full_path.display(), id, e))
                }
            }
        }
    }

    if !delete_errors.is_empty() {
        return Err(format!(
            "Failed to delete {} file(s): {}",
            delete_errors.len(),
            delete_errors.join(" | ")
        ));
    }

    let mut m = state.drive_state.manifest.write().await;
    m.items.retain(|i| !to_delete.contains(&i.id));
    m.shares.retain(|s| !to_delete.contains(&s.item_id));
    drop(m);
    state.drive_state.persist().await;
    Ok(())
}

#[tauri::command]
async fn drive_create_share(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
    price_chi: Option<String>,
    is_public: Option<bool>,
) -> Result<serde_json::Value, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    let mut m = state.drive_state.manifest.write().await;
    let item = m
        .items
        .iter()
        .find(|i| i.id == item_id && i.owner == owner)
        .cloned()
        .ok_or("Item not found")?;

    if item.owner.len() != 42
        || !item.owner.starts_with("0x")
        || !item.owner[2..].chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Item owner wallet must be a valid 0x address".into());
    }

    let requested_price = price_chi
        .or_else(|| item.price_chi.clone())
        .unwrap_or_default();
    let normalized_price = requested_price.trim().to_string();
    let price_wei = wallet::parse_chi_to_wei(&normalized_price)?;
    if price_wei == 0 {
        return Err("Share price must be greater than 0 CHI".into());
    }

    let token = ds::generate_share_token();
    let share = DsShareLink {
        id: token.clone(),
        item_id: item_id.clone(),
        created_at: ds::now_secs(),
        expires_at: None,
        price_chi: normalized_price,
        recipient_wallet: item.owner,
        is_public: is_public.unwrap_or(true),
        download_count: 0,
    };
    m.shares.push(share.clone());
    drop(m);
    state.drive_state.persist().await;
    Ok(serde_json::json!({
        "id": share.id,
        "itemId": item_id,
        "url": format!("/drive/{}", share.id),
        "isPublic": share.is_public,
        "priceChi": share.price_chi,
        "recipientWallet": share.recipient_wallet,
        "createdAt": share.created_at,
        "downloadCount": 0,
    }))
}

#[tauri::command]
async fn drive_revoke_share(
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<(), String> {
    let mut m = state.drive_state.manifest.write().await;
    let before = m.shares.len();
    m.shares.retain(|s| s.id != token);
    if m.shares.len() == before {
        return Err("Share not found".into());
    }
    drop(m);
    state.drive_state.persist().await;
    Ok(())
}

#[tauri::command]
async fn drive_list_shares(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let m = state.drive_state.manifest.read().await;
    let shares: Vec<serde_json::Value> = m
        .shares
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "itemId": s.item_id,
                "url": format!("/drive/{}", s.id),
                "isPublic": s.is_public,
                "priceChi": s.price_chi,
                "recipientWallet": s.recipient_wallet,
                "createdAt": s.created_at,
                "downloadCount": s.download_count,
            })
        })
        .collect();
    Ok(shares)
}

#[tauri::command]
async fn drive_toggle_visibility(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
    is_public: bool,
) -> Result<DsItem, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    let mut m = state.drive_state.manifest.write().await;
    let item = m
        .items
        .iter_mut()
        .find(|i| i.id == item_id && i.owner == owner)
        .ok_or("Item not found")?;
    item.is_public = is_public;
    item.modified_at = ds::now_secs();
    let updated = item.clone();
    drop(m);
    state.drive_state.persist().await;
    Ok(updated)
}

/// Publish a Drive file to the P2P network (compute hash, register as shared, publish to DHT).
/// Returns the SHA-256 file hash so it can be used in hosting proposals.
#[tauri::command]
async fn publish_drive_file(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
    protocol: Option<String>,
    price_chi: Option<String>,
    wallet_address: Option<String>,
    private_key: Option<String>,
) -> Result<ds::DriveItem, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }

    // Look up the Drive item from the manifest.
    let (file_name, storage_path, file_size_hint, existing_merkle_root) = {
        let m = state.drive_state.manifest.read().await;
        let item = m
            .items
            .iter()
            .find(|i| i.id == item_id && i.owner == owner && i.item_type == "file")
            .ok_or("Drive file not found")?;
        let sp = item
            .storage_path
            .as_ref()
            .ok_or("Drive file has no storage path")?;
        (
            item.name.clone(),
            sp.clone(),
            item.size,
            item.merkle_root.clone(),
        )
    };

    // Resolve the full filesystem path
    let files_dir = ds::drive_files_dir().ok_or("Cannot determine drive files directory")?;
    let full_path = files_dir.join(&storage_path);
    if !full_path.exists() {
        return Err("Drive file not found on disk".into());
    }
    let full_path_str = full_path.to_string_lossy().to_string();

    let actual_size = match file_size_hint {
        Some(sz) if sz > 0 => sz,
        _ => std::fs::metadata(&full_path)
            .map_err(|e| format!("Failed to stat file: {}", e))?
            .len(),
    };

    // Reuse persisted hash when available so publishing from Drive is instant.
    let file_hash = if let Some(root) = existing_merkle_root
        .clone()
        .filter(|h| !h.trim().is_empty())
    {
        root
    } else {
        use sha2::{Digest, Sha256};
        use std::io::Read;

        let mut file =
            std::fs::File::open(&full_path).map_err(|e| format!("Failed to open file: {}", e))?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 1024 * 1024];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| format!("Failed to read file while hashing: {}", e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        hex::encode(hasher.finalize())
    };

    let proto = protocol.unwrap_or_else(|| "WebRTC".to_string());

    println!(
        "[DRIVE] Publishing drive file to network: {} (hash: {}, protocol: {})",
        file_name, file_hash, proto
    );

    // Parse price from CHI to wei
    let price_wei_val = if let Some(ref price) = price_chi {
        if price.is_empty() || price == "0" {
            0u128
        } else {
            wallet::parse_chi_to_wei(price)?
        }
    } else {
        0u128
    };
    let wallet_addr = wallet_address
        .filter(|addr| !addr.trim().is_empty())
        .unwrap_or_else(|| owner.clone())
        .trim()
        .to_string();
    if price_wei_val > 0 && wallet_addr.is_empty() {
        return Err("Wallet address is required when setting a file price".to_string());
    }

    // Register with DHT/shared-files map by filesystem path (no file upload/copy).
    // If DHT is not running, fail fast so the UI doesn't show a false "seeding" state.
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard
            .as_ref()
            .cloned()
            .ok_or("DHT not running. Connect to the network before seeding.")?
    };

    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    if peer_id.is_empty() {
        return Err("DHT peer ID unavailable. Try reconnecting to the network.".to_string());
    }

    dht.register_shared_file(
        file_hash.clone(),
        full_path_str,
        file_name.clone(),
        actual_size,
        price_wei_val,
        wallet_addr.clone(),
    )
    .await;

    let our_multiaddrs = dht.get_listening_addresses().await;
    let our_seeder = make_signed_seeder(
        &peer_id, &file_hash, &price_wei_val.to_string(),
        &wallet_addr, our_multiaddrs, private_key.as_deref(),
    );

    let dht_key = format!("chiral_file_{}", file_hash);
    let mut metadata = match dht.get_dht_value(dht_key.clone()).await {
        Ok(Some(json)) => {
            serde_json::from_str::<FileMetadata>(&json).unwrap_or_else(|_| FileMetadata {
                hash: file_hash.clone(),
                file_name: file_name.clone(),
                file_size: actual_size,
                protocol: proto.clone(),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                peer_id: String::new(),
                price_wei: String::new(),
                wallet_address: String::new(),
                seeders: Vec::new(),
                publisher_signature: String::new(),
            })
        }
        _ => FileMetadata {
            hash: file_hash.clone(),
            file_name: file_name.clone(),
            file_size: actual_size,
            protocol: proto.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            peer_id: String::new(),
            price_wei: String::new(),
            wallet_address: String::new(),
            seeders: Vec::new(),
                publisher_signature: String::new(),
        },
    };

    if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
        *existing = our_seeder;
    } else {
        metadata.seeders.push(our_seeder);
    }
    metadata.peer_id = peer_id;
    metadata.price_wei = price_wei_val.to_string();
    metadata.wallet_address = wallet_addr;

    // Sign the metadata if private key is available
    if let Some(ref key) = private_key {
        metadata.sign(key);
    }

    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
    dht.put_dht_value(dht_key, metadata_json).await?;

    // Update the Drive manifest with seeding metadata
    let updated_item = {
        let mut m = state.drive_state.manifest.write().await;
        let item = m
            .items
            .iter_mut()
            .find(|i| i.id == item_id && i.owner == owner && i.item_type == "file")
            .ok_or("Drive item not found in manifest")?;
        item.merkle_root = Some(file_hash);
        item.protocol = Some(proto);
        item.price_chi = price_chi;
        item.seed_enabled = true;
        item.seeding = true;
        item.modified_at = ds::now_secs();
        let cloned = item.clone();
        cloned
    };
    state.drive_state.persist().await;

    Ok(updated_item)
}

/// Auto-seed a file after a hosting agreement download completes.
/// Finds the Drive item by merkle_root, enables seeding, and publishes to DHT.
#[tauri::command]
async fn seed_hosted_file(
    state: tauri::State<'_, AppState>,
    file_hash: String,
    price_chi: Option<String>,
    wallet_address: String,
) -> Result<(), String> {
    if file_hash.is_empty() {
        return Err("file_hash required".into());
    }

    // Find Drive item by merkle_root matching file_hash
    let (item_id, file_name, storage_path, file_size) = {
        let m = state.drive_state.manifest.read().await;
        let item = m
            .items
            .iter()
            .find(|i| {
                i.item_type == "file"
                    && i.merkle_root
                        .as_ref()
                        .map(|h| h == &file_hash)
                        .unwrap_or(false)
            })
            .ok_or_else(|| format!("No Drive file with hash {}", file_hash))?;
        (
            item.id.clone(),
            item.name.clone(),
            item.storage_path.clone().ok_or("No storage path")?,
            item.size.unwrap_or(0),
        )
    };

    let files_dir = ds::drive_files_dir().ok_or("Cannot determine drive files directory")?;
    let full_path = files_dir.join(&storage_path);
    if !full_path.exists() {
        return Err("File not found on disk".into());
    }
    let full_path_str = full_path.to_string_lossy().to_string();

    let price_wei_val = if let Some(ref p) = price_chi {
        if p.is_empty() || p == "0" { 0u128 } else { wallet::parse_chi_to_wei(p)? }
    } else {
        0u128
    };

    let dht = {
        let guard = state.dht.lock().await;
        guard.as_ref().cloned().ok_or("DHT not running")?
    };

    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    if peer_id.is_empty() {
        return Err("Peer ID unavailable".into());
    }

    // Register locally so we can serve chunks
    dht.register_shared_file(
        file_hash.clone(), full_path_str, file_name.clone(),
        file_size, price_wei_val, wallet_address.clone(),
    ).await;

    // Update DHT record to add ourselves as a seeder
    let our_addrs = dht.get_listening_addresses().await;
    let our_seeder = SeederInfo {
        peer_id: peer_id.clone(),
        price_wei: price_wei_val.to_string(),
        wallet_address: wallet_address.clone(),
        multiaddrs: our_addrs,
            signature: String::new(),
    };

    let dht_key = format!("chiral_file_{}", file_hash);
    let mut metadata = match dht.get_dht_value(dht_key.clone()).await {
        Ok(Some(json)) => serde_json::from_str::<FileMetadata>(&json).unwrap_or_else(|_| FileMetadata {
            hash: file_hash.clone(), file_name: file_name.clone(), file_size,
            protocol: "WebRTC".into(),
            created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            peer_id: String::new(), price_wei: String::new(), wallet_address: String::new(),
            seeders: Vec::new(),
                publisher_signature: String::new(),
        }),
        _ => FileMetadata {
            hash: file_hash.clone(), file_name: file_name.clone(), file_size,
            protocol: "WebRTC".into(),
            created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            peer_id: String::new(), price_wei: String::new(), wallet_address: String::new(),
            seeders: Vec::new(),
                publisher_signature: String::new(),
        },
    };

    // Add or update our seeder entry
    if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
        existing.price_wei = our_seeder.price_wei.clone();
        existing.wallet_address = our_seeder.wallet_address.clone();
        existing.multiaddrs = our_seeder.multiaddrs.clone();
    } else {
        metadata.seeders.push(our_seeder);
    }

    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    dht.put_dht_value(dht_key, metadata_json).await?;

    // Enable seeding in Drive manifest
    {
        let mut m = state.drive_state.manifest.write().await;
        if let Some(item) = m.items.iter_mut().find(|i| i.id == item_id) {
            item.seed_enabled = true;
            item.seeding = true;
            item.price_chi = price_chi;
            item.modified_at = ds::now_secs();
        }
    }
    state.drive_state.persist().await;

    println!("[HOSTING] Now seeding hosted file: {} (hash: {})", file_name, file_hash);
    Ok(())
}

#[tauri::command]
async fn drive_stop_seeding(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
) -> Result<ds::DriveItem, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }

    // Look up the Drive item
    let merkle_root = {
        let m = state.drive_state.manifest.read().await;
        let item = m
            .items
            .iter()
            .find(|i| i.id == item_id && i.owner == owner && i.item_type == "file")
            .ok_or("Drive item not found")?;
        item.merkle_root.clone()
    };

    // Unregister from DHT shared files and remove from seeder list
    if let Some(ref hash) = merkle_root {
        let dht_guard = state.dht.lock().await;
        if let Some(dht) = dht_guard.as_ref() {
            // Stop serving chunks
            dht.unregister_shared_file(hash).await;

            // Remove ourselves from the DHT seeder list so downloaders stop seeing us
            let peer_id = dht.get_peer_id().await.unwrap_or_default();
            if !peer_id.is_empty() {
                let dht_key = format!("chiral_file_{}", hash);
                if let Ok(Some(meta_json)) = dht.get_dht_value(dht_key.clone()).await {
                    if let Ok(mut metadata) = serde_json::from_str::<FileMetadata>(&meta_json) {
                        metadata.seeders.retain(|s| s.peer_id != peer_id);
                        if metadata.peer_id == peer_id {
                            metadata.peer_id = String::new();
                        }
                        if let Ok(updated) = serde_json::to_string(&metadata) {
                            let _ = dht.put_dht_value(dht_key, updated).await;
                        }
                    }
                }
            }
        }
        // Remove from in-memory file storage
        let mut storage = state.file_storage.lock().await;
        storage.remove(hash);
    }

    // Update manifest
    let updated_item = {
        let mut m = state.drive_state.manifest.write().await;
        let item = m
            .items
            .iter_mut()
            .find(|i| i.id == item_id && i.owner == owner && i.item_type == "file")
            .ok_or("Drive item not found in manifest")?;
        item.seed_enabled = false;
        item.seeding = false;
        item.modified_at = ds::now_secs();
        let cloned = item.clone();
        cloned
    };
    state.drive_state.persist().await;

    Ok(updated_item)
}

#[tauri::command]
async fn drive_export_torrent(
    state: tauri::State<'_, AppState>,
    owner: String,
    item_id: String,
) -> Result<String, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }

    let (file_name, file_size, merkle_root, storage_path) = {
        let m = state.drive_state.manifest.read().await;
        let item = m
            .items
            .iter()
            .find(|i| i.id == item_id && i.owner == owner && i.item_type == "file")
            .ok_or("Drive file not found")?;
        let hash = item
            .merkle_root
            .as_ref()
            .ok_or("File has not been published to network")?;
        let sp = item
            .storage_path
            .as_ref()
            .ok_or("File has no storage path")?;
        (
            item.name.clone(),
            item.size.unwrap_or(0),
            hash.clone(),
            sp.clone(),
        )
    };

    let files_dir = ds::drive_files_dir().ok_or("Cannot determine drive files directory")?;
    let full_path = files_dir.join(&storage_path).to_string_lossy().to_string();

    // Delegate to existing export_torrent_file logic
    let result = export_torrent_file(merkle_root, file_name, file_size, full_path).await?;

    Ok(result.path)
}

// ---------------------------------------------------------------------------
// Drive server & relay commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_drive_server_url(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    // The local server starts asynchronously in .setup(). Wait up to 10s for it
    // to become ready, polling every 100ms, so the frontend doesn't get a null
    // URL on first mount.
    for _ in 0..100 {
        let addr = state.hosting_server_addr.lock().await;
        if let Some(a) = *addr {
            // The server binds to 0.0.0.0 which isn't fetchable from a
            // browser/WebView. Return localhost instead.
            return Ok(Some(format!("http://localhost:{}", a.port())));
        }
        drop(addr);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(None)
}

#[tauri::command]
async fn publish_drive_share(
    state: tauri::State<'_, AppState>,
    share_token: String,
    relay_url: String,
    owner_wallet: String,
) -> Result<(), String> {
    let origin = state
        .hosting_server_addr
        .lock()
        .await
        .map(|a| format!("http://{}", a))
        .ok_or("Local server not running")?;

    let relay_base = relay_url.trim_end_matches('/');
    let url = format!("{}/api/drive/relay-register", relay_base);

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "token": share_token,
            "origin_url": origin,
            "owner_wallet": owner_wallet,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Failed to register share with relay: {}", e))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Relay error: {}", text));
    }

    // Start WebSocket tunnel to relay for NAT traversal
    let tunnel_key = format!("share:{}", share_token);
    let abort_handle = spawn_relay_tunnel(
        relay_base.to_string(),
        "share".to_string(),
        share_token.clone(),
        origin,
    );
    state
        .tunnel_handles
        .lock()
        .await
        .insert(tunnel_key, abort_handle);

    println!(
        "[DRIVE] Published share token={} to relay {}",
        share_token, relay_base
    );
    Ok(())
}

#[tauri::command]
async fn unpublish_drive_share(
    state: tauri::State<'_, AppState>,
    share_token: String,
    relay_url: String,
) -> Result<(), String> {
    let relay_base = relay_url.trim_end_matches('/');
    let url = format!("{}/api/drive/relay-register/{}", relay_base, share_token);

    let client = reqwest::Client::new();
    let resp = client
        .delete(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Failed to unregister share from relay: {}", e))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Relay error: {}", text));
    }

    // Cancel the WebSocket tunnel
    let tunnel_key = format!("share:{}", share_token);
    if let Some(handle) = state.tunnel_handles.lock().await.remove(&tunnel_key) {
        handle.abort();
    }

    println!("[DRIVE] Unpublished share token={} from relay", share_token);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let geth = Arc::new(Mutex::new(GethProcess::new()));
    #[cfg(unix)]
    let geth_for_signal = geth.clone();
    let geth_for_exit = geth.clone();
    let hosting_shutdown_for_exit: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
        Arc::new(Mutex::new(None));
    let tunnel_handles_for_exit: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Create DHT Arc before signal handler so it can be shared
    let dht_arc: Arc<Mutex<Option<Arc<DhtService>>>> = Arc::new(Mutex::new(None));
    #[cfg(unix)]
    let dht_for_signal = dht_arc.clone();

    // Spawn a background task to stop Geth + cleanup DHT on SIGINT (Ctrl+C) or SIGTERM
    // This prevents orphaned Geth processes and stale DHT entries when the app is killed
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigint = signal(SignalKind::interrupt()).unwrap();
                let mut sigterm = signal(SignalKind::terminate()).unwrap();
                tokio::select! {
                    _ = sigint.recv() => {
                        println!("🛑 SIGINT received — cleaning up before exit");
                    }
                    _ = sigterm.recv() => {
                        println!("🛑 SIGTERM received — cleaning up before exit");
                    }
                }

                // Cleanup DHT: unpublish shared files + host advertisement
                // Use tokio::time::timeout to avoid hanging if DHT is unresponsive
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    cleanup_dht_on_shutdown(&dht_for_signal),
                )
                .await;

                // Use try_lock to avoid deadlock with the main thread
                match geth_for_signal.try_lock() {
                    Ok(mut geth) => {
                        let _ = geth.stop();
                    }
                    Err(_) => {
                        // Fallback: kill via PID file
                        let data_dir = dirs::data_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("chiral-network")
                            .join("geth");
                        let pid_path = data_dir.join("geth.pid");
                        if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                let _ = std::process::Command::new("kill")
                                    .arg(pid.to_string())
                                    .output();
                            }
                        }
                    }
                }
                std::process::exit(0);
            }
        });
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            dht: dht_arc,
            file_transfer: Arc::new(Mutex::new(FileTransferService::new())),
            file_storage: Arc::new(Mutex::new(HashMap::new())),
            geth,
            encryption_keypair: Arc::new(Mutex::new(None)),
            tx_metadata: Arc::new(Mutex::new(wallet::load_tx_metadata())),
            download_directory: Arc::new(Mutex::new(None)),
            download_credentials: Arc::new(Mutex::new(HashMap::new())),
            // Hosting & Drive
            hosting_server_state: Arc::new(hosting_server::HostingServerState::new()),
            hosting_server_addr: Arc::new(Mutex::new(None)),
            hosting_server_shutdown: Arc::clone(&hosting_shutdown_for_exit),
            drive_state: Arc::new(drive_api::DriveState::new()),
            tunnel_handles: Arc::clone(&tunnel_handles_for_exit),
        })
        .setup(|app| {
            use tauri::Manager;
            let app_handle = app.handle().clone();
            // Auto-start local server with Drive routes on port 9419
            let state = app.state::<AppState>();
            let hosting: Arc<hosting_server::HostingServerState> =
                Arc::clone(&state.hosting_server_state);
            let drive: Arc<drive_api::DriveState> = Arc::clone(&state.drive_state);
            let addr_store: Arc<Mutex<Option<std::net::SocketAddr>>> =
                Arc::clone(&state.hosting_server_addr);
            let shutdown_store: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
                Arc::clone(&state.hosting_server_shutdown);
            let tunnel_handles: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>> =
                Arc::clone(&state.tunnel_handles);
            let app_for_boot = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                hosting.load_from_disk().await;
                drive.load_from_disk_async().await;

                // Always start DHT on app launch so seeding resumes immediately after restart.
                let app_state = app_for_boot.state::<AppState>();
                match start_dht_internal(app_for_boot.clone(), app_state.inner(), true).await {
                    Ok(msg) => println!("[DHT] Auto-start on launch: {}", msg),
                    Err(err) => eprintln!("[DHT] Auto-start on launch failed: {}", err),
                }

                let (tx, rx) = tokio::sync::oneshot::channel();
                // Local server: has drive routes, no relay share proxy
                match hosting_server::start_gateway_server(
                    hosting,
                    Some(drive),
                    None,
                    None,
                    9419,
                    rx,
                )
                .await
                {
                    Ok(addr) => {
                        *addr_store.lock().await = Some(addr);
                        *shutdown_store.lock().await = Some(tx);
                        println!("[DRIVE] Local server started on http://{}", addr);

                        // Re-establish tunnels for already-published sites
                        let local_origin = format!("http://localhost:{}", addr.port());
                        let relay_base = "http://130.245.173.73:8080";
                        let sites = hosting::load_sites();
                        for site in &sites {
                            if site.relay_url.is_some() {
                                println!("[TUNNEL] Re-establishing tunnel for site {}", site.id);
                                let abort = spawn_relay_tunnel(
                                    relay_base.to_string(),
                                    "site".to_string(),
                                    site.id.clone(),
                                    local_origin.clone(),
                                );
                                tunnel_handles
                                    .lock()
                                    .await
                                    .insert(format!("site:{}", site.id), abort);
                            }
                        }
                    }
                    Err(e) => eprintln!("[DRIVE] Failed to start local server: {}", e),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // DHT commands
            start_dht,
            reseed_drive_files,
            stop_dht,
            get_dht_peers,
            get_network_stats,
            get_peer_id,
            get_dht_health,
            get_bootstrap_peer_ids,
            ping_peer,
            send_file,
            send_file_by_path,
            accept_file_transfer,
            decline_file_transfer,
            store_dht_value,
            get_dht_value,
            // File commands
            get_available_storage,
            get_file_size,
            open_file_dialog,
            pick_download_directory,
            set_download_directory,
            get_download_directory,
            publish_file,
            publish_file_data,
            search_file,
            start_download,
            calculate_download_cost,
            register_shared_file,
            parse_torrent_file,
            export_torrent_file,
            open_file,
            show_in_folder,
            show_drive_item_in_folder,
            get_drive_file_path,
            // Wallet commands
            get_wallet_balance,
            send_transaction,
            get_transaction_receipt,
            get_transaction_history,
            record_transaction_meta,
            request_faucet,
            get_chain_id,
            // Geth commands
            is_geth_installed,
            download_geth,
            start_geth,
            stop_geth,
            get_geth_status,
            start_mining,
            stop_mining,
            get_mining_status,
            get_gpu_mining_capabilities,
            list_gpu_devices,
            start_gpu_mining,
            stop_gpu_mining,
            get_gpu_mining_status,
            set_miner_address,
            // Diagnostics commands
            read_geth_log,
            // Bootstrap health commands
            check_bootstrap_health,
            get_bootstrap_health,
            // Encryption commands
            init_encryption_keypair,
            get_encryption_public_key,
            encrypt_file_for_recipient,
            decrypt_file_data,
            send_encrypted_file,
            publish_encryption_key,
            lookup_encryption_key,
            // Hosting commands
            create_hosted_site,
            list_hosted_sites,
            delete_hosted_site,
            start_hosting_server,
            stop_hosting_server,
            get_hosting_server_status,
            publish_site_to_relay,
            unpublish_site_from_relay,
            // Drive commands
            get_drive_server_url,
            publish_drive_share,
            unpublish_drive_share,
            drive_list_items,
            drive_list_all_items,
            drive_create_folder,
            drive_read_file_bytes,
            drive_upload_file,
            drive_update_item,
            drive_delete_item,
            drive_create_share,
            drive_revoke_share,
            drive_list_shares,
            drive_toggle_visibility,
            publish_drive_file,
            seed_hosted_file,
            drive_stop_seeding,
            drive_export_torrent,
            // Hosting marketplace commands
            publish_host_advertisement,
            unpublish_host_advertisement,
            get_host_registry,
            get_host_advertisement,
            echo_peer,
            store_hosting_agreement,
            get_hosting_agreement,
            list_hosting_agreements,
            // File lifecycle commands
            republish_shared_file,
            unpublish_all_shared_files,
            get_active_hosted_files,
            cleanup_agreement_files,
            // Wallet backup email
            send_wallet_backup_email,
            // App lifecycle
            exit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app, event| {
            if let tauri::RunEvent::Exit = event {
                println!("🛑 App exiting — stopping gateway server and relay tunnels");
                if let Ok(mut shutdown_store) = hosting_shutdown_for_exit.try_lock() {
                    if let Some(tx) = shutdown_store.take() {
                        let _ = tx.send(());
                    }
                } else {
                    println!("⚠️  Could not acquire hosting shutdown lock on exit");
                }

                if let Ok(mut tunnel_store) = tunnel_handles_for_exit.try_lock() {
                    for (_, handle) in tunnel_store.drain() {
                        handle.abort();
                    }
                } else {
                    println!("⚠️  Could not acquire tunnel handle lock on exit");
                }

                // Stop Geth cleanly when the app exits (window close, quit, etc.)
                // Use try_lock to avoid deadlock — if the mutex is held by another
                // task (e.g. mining status poll), force-kill via PID file instead.
                println!("🛑 App exiting — stopping Geth and mining");
                match geth_for_exit.try_lock() {
                    Ok(mut geth) => {
                        let _ = geth.stop_fast();
                    }
                    Err(_) => {
                        // Mutex is held — use synchronous force-kill as fallback
                        println!("⚠️  Could not acquire Geth lock on exit, force-killing via PID");
                        let data_dir = dirs::data_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("chiral-network")
                            .join("geth");
                        let pid_path = data_dir.join("geth.pid");
                        if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                println!("🛑 Force-killing Geth PID {} on exit", pid);
                                let _ = std::process::Command::new("kill")
                                    .arg(pid.to_string())
                                    .output();
                                let _ = std::process::Command::new("kill")
                                    .args(["-9", &pid.to_string()])
                                    .output();
                            }
                        }
                        let _ = std::fs::remove_file(&pid_path);
                    }
                }
            }
        });
}

#[cfg(test)]
mod multi_seeder_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // --- Serialization / Deserialization ---

    #[test]
    fn seeder_info_roundtrip() {
        let seeder = SeederInfo {
            peer_id: "12D3KooWTest1".to_string(),
            price_wei: "1000000000000000".to_string(),
            wallet_address: "0xabc123".to_string(),
            multiaddrs: vec![],
            signature: String::new(),
        };
        let json = serde_json::to_string(&seeder).unwrap();
        assert!(json.contains("peerId")); // camelCase
        assert!(json.contains("priceWei"));
        assert!(json.contains("walletAddress"));

        let deserialized: SeederInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.peer_id, "12D3KooWTest1");
        assert_eq!(deserialized.price_wei, "1000000000000000");
        assert_eq!(deserialized.wallet_address, "0xabc123");
    }

    #[tokio::test]
    async fn build_local_search_result_reads_local_shared_file() {
        let dht = Arc::new(DhtService::new(
            Arc::new(Mutex::new(FileTransferService::new())),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(HashMap::new())),
        ));

        let tmp_path = std::env::temp_dir().join("chiral-local-search-test.bin");
        std::fs::write(&tmp_path, b"hello").unwrap();

        let file_hash = "localhash123".to_string();
        dht.register_shared_file(
            file_hash.clone(),
            tmp_path.to_string_lossy().to_string(),
            "hello.bin".to_string(),
            5,
            0,
            String::new(),
        )
        .await;

        let result = build_local_search_result(&dht, &file_hash).await;
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.hash, file_hash);
        assert_eq!(result.file_name, "hello.bin");
        assert_eq!(result.file_size, 5);
    }

    #[tokio::test]
    async fn build_local_search_result_returns_none_for_missing_hash() {
        let dht = Arc::new(DhtService::new(
            Arc::new(Mutex::new(FileTransferService::new())),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(HashMap::new())),
        ));

        let result = build_local_search_result(&dht, "does-not-exist").await;
        assert!(result.is_none());
    }

    #[test]
    fn file_metadata_with_seeders_roundtrip() {
        let metadata = FileMetadata {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            peer_id: "12D3KooWPeerA".to_string(),
            price_wei: "0".to_string(),
            wallet_address: String::new(),
            seeders: vec![
                SeederInfo {
                    peer_id: "12D3KooWPeerA".to_string(),
                    price_wei: "0".to_string(),
                    wallet_address: String::new(),
                    multiaddrs: vec![],
            signature: String::new(),
                },
                SeederInfo {
                    peer_id: "12D3KooWPeerB".to_string(),
                    price_wei: "5000000000000000".to_string(),
                    wallet_address: "0xdef456".to_string(),
                    multiaddrs: vec![],
            signature: String::new(),
                },
            ],
            publisher_signature: String::new(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let restored: FileMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.seeders.len(), 2);
        assert_eq!(restored.seeders[0].peer_id, "12D3KooWPeerA");
        assert_eq!(restored.seeders[1].peer_id, "12D3KooWPeerB");
        assert_eq!(restored.seeders[1].price_wei, "5000000000000000");
        assert_eq!(restored.seeders[1].wallet_address, "0xdef456");
    }

    // --- Backward Compatibility ---

    #[test]
    fn legacy_metadata_without_seeders_field_deserializes() {
        // Old records stored in DHT won't have a "seeders" field
        let legacy_json = r#"{
            "hash": "abc123",
            "fileName": "old_file.txt",
            "fileSize": 512,
            "protocol": "WebRTC",
            "createdAt": 1700000000,
            "peerId": "12D3KooWLegacy",
            "priceWei": "1000",
            "walletAddress": "0xlegacy"
        }"#;

        let metadata: FileMetadata = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(metadata.peer_id, "12D3KooWLegacy");
        assert_eq!(metadata.price_wei, "1000");
        assert_eq!(metadata.wallet_address, "0xlegacy");
        // seeders should default to empty vec
        assert!(metadata.seeders.is_empty());
    }

    #[test]
    fn legacy_metadata_search_creates_seeder_from_peer_id() {
        // Simulate what search_file does with legacy records
        let metadata = FileMetadata {
            hash: "abc123".to_string(),
            file_name: "legacy.txt".to_string(),
            file_size: 256,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            peer_id: "12D3KooWLegacy".to_string(),
            price_wei: "2000".to_string(),
            wallet_address: "0xlegacy".to_string(),
            seeders: Vec::new(), // empty — old record
                publisher_signature: String::new(),
        };

        // This is the logic from search_file:
        let mut seeders = metadata.seeders;
        if seeders.is_empty() && !metadata.peer_id.is_empty() {
            seeders.push(SeederInfo {
                peer_id: metadata.peer_id,
                price_wei: metadata.price_wei.clone(),
                wallet_address: metadata.wallet_address.clone(),
                multiaddrs: vec![],
            signature: String::new(),
            });
        }

        assert_eq!(seeders.len(), 1);
        assert_eq!(seeders[0].peer_id, "12D3KooWLegacy");
        assert_eq!(seeders[0].price_wei, "2000");
        assert_eq!(seeders[0].wallet_address, "0xlegacy");
    }

    // --- Upsert Logic ---

    #[test]
    fn upsert_adds_new_seeder() {
        let mut metadata = FileMetadata {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            peer_id: "12D3KooWPeerA".to_string(),
            price_wei: "0".to_string(),
            wallet_address: String::new(),
            seeders: vec![SeederInfo {
                peer_id: "12D3KooWPeerA".to_string(),
                price_wei: "0".to_string(),
                wallet_address: String::new(),
                multiaddrs: vec![],
            signature: String::new(),
            }],
            publisher_signature: String::new(),
        };

        let new_peer = "12D3KooWPeerB";
        let our_seeder = SeederInfo {
            peer_id: new_peer.to_string(),
            price_wei: "5000".to_string(),
            wallet_address: "0xB".to_string(),
            multiaddrs: vec![],
            signature: String::new(),
        };

        // Upsert logic from publish_file
        if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == new_peer) {
            existing.price_wei = our_seeder.price_wei;
            existing.wallet_address = our_seeder.wallet_address;
        } else {
            metadata.seeders.push(our_seeder);
        }

        assert_eq!(metadata.seeders.len(), 2);
        assert_eq!(metadata.seeders[1].peer_id, "12D3KooWPeerB");
        assert_eq!(metadata.seeders[1].price_wei, "5000");
    }

    #[test]
    fn upsert_updates_existing_seeder_price() {
        let mut metadata = FileMetadata {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            peer_id: "12D3KooWPeerA".to_string(),
            price_wei: "1000".to_string(),
            wallet_address: "0xA".to_string(),
            seeders: vec![SeederInfo {
                peer_id: "12D3KooWPeerA".to_string(),
                price_wei: "1000".to_string(),
                wallet_address: "0xA".to_string(),
                multiaddrs: vec![],
            signature: String::new(),
            }],
            publisher_signature: String::new(),
        };

        let peer_id = "12D3KooWPeerA";
        let new_price = "9999".to_string();
        let new_wallet = "0xA_new".to_string();

        if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
            existing.price_wei = new_price;
            existing.wallet_address = new_wallet;
        } else {
            unreachable!("should have found existing seeder");
        }

        assert_eq!(metadata.seeders.len(), 1); // no duplicates
        assert_eq!(metadata.seeders[0].price_wei, "9999");
        assert_eq!(metadata.seeders[0].wallet_address, "0xA_new");
    }

    // --- Unpublish (removal) ---

    #[test]
    fn unpublish_removes_only_our_peer() {
        let our_peer = "12D3KooWUs";
        let other_peer = "12D3KooWOther";

        let mut metadata = FileMetadata {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            peer_id: our_peer.to_string(),
            price_wei: "0".to_string(),
            wallet_address: String::new(),
            seeders: vec![
                SeederInfo {
                    peer_id: our_peer.to_string(),
                    price_wei: "0".to_string(),
                    wallet_address: String::new(),
                    multiaddrs: vec![],
            signature: String::new(),
                },
                SeederInfo {
                    peer_id: other_peer.to_string(),
                    price_wei: "3000".to_string(),
                    wallet_address: "0xOther".to_string(),
                    multiaddrs: vec![],
            signature: String::new(),
                },
            ],
            publisher_signature: String::new(),
        };

        // Unpublish logic from unpublish_all_shared_files
        metadata.seeders.retain(|s| s.peer_id != our_peer);
        if metadata.peer_id == our_peer {
            metadata.peer_id = String::new();
        }

        assert_eq!(metadata.seeders.len(), 1);
        assert_eq!(metadata.seeders[0].peer_id, other_peer);
        assert_eq!(metadata.seeders[0].price_wei, "3000");
        assert!(metadata.peer_id.is_empty());
    }

    #[test]
    fn unpublish_from_single_seeder_leaves_empty() {
        let our_peer = "12D3KooWSolo";

        let mut metadata = FileMetadata {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            peer_id: our_peer.to_string(),
            price_wei: "0".to_string(),
            wallet_address: String::new(),
            seeders: vec![SeederInfo {
                peer_id: our_peer.to_string(),
                price_wei: "0".to_string(),
                wallet_address: String::new(),
                multiaddrs: vec![],
            signature: String::new(),
            }],
            publisher_signature: String::new(),
        };

        metadata.seeders.retain(|s| s.peer_id != our_peer);
        if metadata.peer_id == our_peer {
            metadata.peer_id = String::new();
        }

        assert!(metadata.seeders.is_empty());
        assert!(metadata.peer_id.is_empty());
    }

    // --- Multiple seeders scenario ---

    #[test]
    fn three_seeders_with_different_prices() {
        let mut metadata = FileMetadata {
            hash: "multiseed".to_string(),
            file_name: "popular.zip".to_string(),
            file_size: 10_000_000,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            peer_id: String::new(),
            price_wei: String::new(),
            wallet_address: String::new(),
            seeders: Vec::new(),
                publisher_signature: String::new(),
        };

        // Three peers publish in sequence
        let peers = vec![
            ("PeerA", "0", ""),
            ("PeerB", "1000000000000000", "0xB_wallet"),
            ("PeerC", "5000000000000000", "0xC_wallet"),
        ];

        for (peer_id, price, wallet) in &peers {
            let seeder = SeederInfo {
                peer_id: peer_id.to_string(),
                price_wei: price.to_string(),
                wallet_address: wallet.to_string(),
                multiaddrs: vec![],
            signature: String::new(),
            };
            if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == *peer_id) {
                existing.price_wei = seeder.price_wei;
                existing.wallet_address = seeder.wallet_address;
            } else {
                metadata.seeders.push(seeder);
            }
            metadata.peer_id = peer_id.to_string();
        }

        assert_eq!(metadata.seeders.len(), 3);
        // Free seeder
        assert_eq!(metadata.seeders[0].price_wei, "0");
        // Cheapest paid seeder
        assert_eq!(metadata.seeders[1].price_wei, "1000000000000000");
        // Most expensive seeder
        assert_eq!(metadata.seeders[2].price_wei, "5000000000000000");
    }

    #[test]
    fn search_result_serialization_with_seeder_info() {
        let result = SearchResult {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            seeders: vec![
                SeederInfo {
                    peer_id: "PeerA".to_string(),
                    price_wei: "0".to_string(),
                    wallet_address: String::new(),
                    multiaddrs: vec![],
            signature: String::new(),
                },
                SeederInfo {
                    peer_id: "PeerB".to_string(),
                    price_wei: "5000".to_string(),
                    wallet_address: "0xB".to_string(),
                    multiaddrs: vec![],
            signature: String::new(),
                },
            ],
            created_at: 1700000000,
            price_wei: "0".to_string(),
            wallet_address: String::new(),
        };

        let json = serde_json::to_string(&result).unwrap();
        // Verify it serializes as array of objects (not strings)
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let seeders = parsed["seeders"].as_array().unwrap();
        assert_eq!(seeders.len(), 2);
        assert_eq!(seeders[0]["peerId"], "PeerA");
        assert_eq!(seeders[1]["peerId"], "PeerB");
        assert_eq!(seeders[1]["priceWei"], "5000");
        assert_eq!(seeders[1]["walletAddress"], "0xB");
    }
}
