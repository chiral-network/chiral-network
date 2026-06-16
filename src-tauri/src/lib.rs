pub mod auth;
pub mod chain_rpc_api;
pub mod dht;
pub mod drive_api;
pub mod drive_storage;
mod encryption;
pub mod event_sink;
pub mod file_transfer;
pub mod cdn_server;
pub mod geth;
pub mod geth_gpu;
pub mod hosting;
pub mod hosting_server;
pub mod network;
pub mod rating_api;
pub mod rating_storage;
pub mod reputation;
pub mod relay_share_proxy;
pub mod rpc_client;
mod speed_tiers;
pub mod version;
pub mod wallet;
pub mod wallet_backup_api;

use dht::DhtService;
use encryption::EncryptionKeypair;
use file_transfer::FileTransferService;
use geth::{
    GethDownloader, GethProcess, GethStatus, GpuDevice, GpuMiningCapabilities, GpuMiningStatus,
    MiningStatus,
};
// Bootstrap health is reported via inline placeholder structs in this file
// — the legacy geth_bootstrap module was deleted with the geth rewrite.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    pub gpu_miner: Arc<Mutex<geth_gpu::GpuMiner>>,
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
    // The effective `VersionPolicy` lives in `version::EFFECTIVE_POLICY`
    // (a global RwLock) since Phase 5; that gives every caller —
    // including the sync libp2p Identify event handler — a non-blocking
    // read path. The Tauri commands and the fetch task all go through
    // `version::effective_policy()` / `version::update_effective_policy()`.
}

const DHT_RESEED_BOOTSTRAP_TIMEOUT_SECS: u64 = 180;

async fn wait_for_dht_bootstrap_for_reseed(dht: &Arc<DhtService>, label: &str) -> bool {
    if dht
        .wait_for_bootstrap_ready(std::time::Duration::from_secs(
            DHT_RESEED_BOOTSTRAP_TIMEOUT_SECS,
        ))
        .await
    {
        return true;
    }

    println!(
        "[DHT] {} skipped: bootstrap did not complete within {}s",
        label, DHT_RESEED_BOOTSTRAP_TIMEOUT_SECS
    );
    false
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
    // Provider-records teardown: `remove_seeder_entry` calls
    // `stop_providing_file` so downloaders' search queries see us drop out
    // of the seeder set. The file metadata blob is immutable per-file in
    // the new schema, so we no longer mutate it on shutdown.
    let mut count = 0u32;
    for file_hash in &file_hashes {
        let _ = remove_seeder_entry(dht, file_hash).await;
        count += 1;
    }
    let _ = peer_id; // retained for the host-registry cleanup below
    println!("✅ Stopped providing {} files to DHT", count);

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

#[cfg(unix)]
fn shutdown_signal_pair_from_results<T>(
    sigint: std::io::Result<T>,
    sigterm: std::io::Result<T>,
) -> Result<(T, T), String> {
    let sigint = sigint.map_err(|e| format!("failed to install SIGINT handler: {}", e))?;
    let sigterm = sigterm.map_err(|e| format!("failed to install SIGTERM handler: {}", e))?;
    Ok((sigint, sigterm))
}

#[cfg(unix)]
fn install_shutdown_signal_handlers() -> Result<
    (tokio::signal::unix::Signal, tokio::signal::unix::Signal),
    String,
> {
    use tokio::signal::unix::{signal, SignalKind};
    shutdown_signal_pair_from_results(
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
    )
}

async fn start_dht_internal(
    app: tauri::AppHandle,
    state: &AppState,
    allow_already_running: bool,
) -> Result<String, String> {
    // Phase 2 gate: refuse to enter the network if our build is below
    // the policy's `min_required`. The frontend's blocking modal already
    // prevents this in the UI; this catches direct Tauri-invoke bypasses.
    ensure_version_supported(state).await?;

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
    let app_for_bootstrap_reseed = app.clone();
    let dht_for_bootstrap_reseed = dht.clone();
    let result = dht.start(app).await?;
    *dht_guard = Some(dht);
    drop(dht_guard);

    // Ensure latest Drive manifest is in-memory before reseed to avoid startup races.
    state.drive_state.load_from_disk_async().await;

    // Restore persisted Drive seeding registrations (root + nested folders)
    // as soon as DHT comes online so "seeding=true" reflects actual availability.
    // No wallet credentials available here (DHT can start before login), so
    // this pass only re-registers files locally. The frontend will call
    // `reseed_drive_files` again with the unlocked wallet after login to
    // publish the signed DHT records.
    auto_reseed_drive_files(state, None, None).await;

    // Run one follow-up reseed pass once Kademlia bootstrap has actually
    // completed, instead of guessing with fixed startup sleeps.
    tauri::async_runtime::spawn(async move {
        if wait_for_dht_bootstrap_for_reseed(&dht_for_bootstrap_reseed, "Drive startup reseed")
            .await
        {
            let app_state = app_for_bootstrap_reseed.state::<AppState>();
            app_state.drive_state.load_from_disk_async().await;
            auto_reseed_drive_files(app_state.inner(), None, None).await;
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

/// Re-register Drive files that should be seeding.
///
/// Always runs the local-side re-registration (so this node can serve
/// chunks if a peer happens to ask). When `wallet_address` AND
/// `private_key` are both provided, ALSO publishes the signed DHT
/// records (`chiral_file_<hash>` metadata blob + `chiral_seeder_*`
/// entry + Kademlia provider) so other peers can discover us via
/// `search_file(hash)`. Without those records, this node is invisible
/// to remote search even though `seeding=true` locally — that was the
/// "auto-seeding on login doesn't work" symptom.
async fn auto_reseed_drive_files(
    state: &AppState,
    wallet_address: Option<&str>,
    private_key: Option<&str>,
) {
    let dht = {
        let dht_guard = state.dht.lock().await;
        dht_guard.as_ref().cloned()
    };
    let Some(dht) = dht else {
        return;
    };
    let can_publish_signed = wallet_address
        .map(|w| !w.trim().is_empty())
        .unwrap_or(false)
        && private_key.map(|k| !k.trim().is_empty()).unwrap_or(false);

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
                let folder_access = paid_folder_policies_for_drive_item(&manifest, item);
                Some((
                    item.id.clone(),
                    item.owner.clone(),
                    item.name.clone(),
                    storage_path,
                    item.size,
                    item.merkle_root.clone(),
                    item.protocol.clone(),
                    item.price_chi.clone(),
                    folder_access,
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
    // Counts files for which we actually published signed DHT records
    // (file metadata + seeder entry + provider). Only non-zero when
    // wallet creds are present; matches the variable expected later.
    let published_dht_records_count = std::sync::atomic::AtomicUsize::new(0);

    for (
        item_id,
        owner,
        file_name,
        storage_path,
        file_size_hint,
        existing_merkle_root,
        _protocol,
        price_chi,
        folder_access,
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
            None => {
                // Hash off the async runtime — auto-reseed iterates every
                // Drive file at startup, so a single multi-GB file would
                // otherwise block libp2p / wallet RPC / UI events for the
                // duration of the read.
                let path_for_hash = full_path.clone();
                match tokio::task::spawn_blocking(move || compute_sha256_file(&path_for_hash)).await
                {
                    Ok(Ok(hash)) => {
                        hash_updates.push((item_id.clone(), hash.clone()));
                        hash
                    }
                    Ok(Err(e)) => {
                        println!("[DRIVE] Auto-reseed failed to hash {}: {}", file_name, e);
                        continue;
                    }
                    Err(e) => {
                        println!("[DRIVE] Auto-reseed hash task panicked for {}: {}", file_name, e);
                        continue;
                    }
                }
            }
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

        // Register locally so we can serve chunks if a peer requests
        // them. Pass the unlocked private key when available so the
        // seeder responder can sign FileInfo envelopes (FM-A09); when
        // we have no wallet (DHT auto-started before login), pass empty
        // string and the responder will refuse to serve until the next
        // reseed pass populates it.
        dht.register_shared_file_with_folder_access(
            file_hash.clone(),
            full_path.to_string_lossy().to_string(),
            file_name.clone(),
            file_size,
            price_wei,
            wallet_addr.clone(),
            private_key.map(|k| k.to_string()).unwrap_or_default(),
            folder_access,
        )
        .await;

        activated_ids.insert(item_id.clone());
        reseeded_local += 1;

        // Keep local reseeding instant even when DHT identity/bootstrap is not
        // ready yet. Metadata will be refreshed on follow-up reseed passes.
        if peer_id.is_empty() {
            continue;
        }

        // Publish signed DHT records ONLY when the wallet is unlocked.
        // Readers drop unsigned records (FM-A07/A08), so writing them
        // without a private key would just spam the DHT with useless
        // data. The post-login reseed pass (frontend calls
        // `reseed_drive_files` with the wallet creds) re-runs this loop
        // and publishes the signed records — that's what restores
        // search-by-hash discoverability after a fresh login.
        if !can_publish_signed {
            continue;
        }
        let owner_wallet = wallet_address.unwrap();
        let pk = private_key.unwrap();
        let seeder_price_str = price_wei.to_string();
        // Sign + publish chiral_file_<hash> metadata blob.
        let Some(metadata) = try_make_signed_file_metadata(
            &file_hash,
            &file_name,
            file_size,
            "WebRTC",
            owner_wallet,
            Some(pk),
        ) else {
            println!(
                "[DRIVE] Auto-reseed for {} skipped signed metadata publish — sign_message failed",
                file_hash
            );
            continue;
        };
        let metadata_json = match serde_json::to_string(&metadata) {
            Ok(s) => s,
            Err(e) => {
                println!(
                    "[DRIVE] Auto-reseed serialize metadata failed for {}: {}",
                    file_hash, e
                );
                continue;
            }
        };
        let dht_key = format!("chiral_file_{}", file_hash);
        if let Err(e) = dht.put_dht_value(dht_key, metadata_json).await {
            println!(
                "[DRIVE] Auto-reseed put_dht_value for {} failed: {}",
                file_hash, e
            );
            continue;
        }
        // Sign + publish per-seeder entry + Kademlia provider record.
        let Some(seeder) = try_make_signed_seeder(
            &peer_id,
            &file_hash,
            &seeder_price_str,
            owner_wallet,
            our_multiaddrs.clone(),
            Some(pk),
        ) else {
            println!(
                "[DRIVE] Auto-reseed for {} skipped signed seeder publish — sign_message failed",
                file_hash
            );
            continue;
        };
        if let Err(e) = publish_seeder_entry(&dht, &file_hash, &seeder).await {
            println!(
                "[DRIVE] Auto-reseed publish_seeder_entry for {} failed: {}",
                file_hash, e
            );
            continue;
        }
        published_dht_records_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let reseeded_dht_metadata =
        published_dht_records_count.load(std::sync::atomic::Ordering::Relaxed);
    let _ = (reseeded_dht_metadata, &peer_id, &our_multiaddrs);

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
async fn reseed_drive_files(
    state: tauri::State<'_, AppState>,
    wallet_address: Option<String>,
    private_key: Option<String>,
) -> Result<(), String> {
    let will_publish_signed = wallet_address
        .as_deref()
        .map(|w| !w.trim().is_empty())
        .unwrap_or(false)
        && private_key
            .as_deref()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false);
    if will_publish_signed {
        let dht = {
            let guard = state.dht.lock().await;
            guard.as_ref().cloned().ok_or("DHT not running")?
        };
        if !wait_for_dht_bootstrap_for_reseed(&dht, "Drive signed reseed").await {
            return Err("DHT bootstrap did not complete; reseed deferred".to_string());
        }
    }

    // Reload disk state first to avoid stale in-memory manifests after app restart.
    state.drive_state.load_from_disk_async().await;
    auto_reseed_drive_files(
        state.inner(),
        wallet_address.as_deref(),
        private_key.as_deref(),
    )
    .await;
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
                    // File metadata blob is immutable per-file; seeder
                    // liveness lives in Kademlia provider records. Logout
                    // just drops us from the provider set.
                    let _ = remove_seeder_entry(&dht, file_hash).await;
                }
                let _ = peer_id;
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
    let dir = network::data_dir().join("agreements");
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
pub struct SeederInfo {
    pub(crate) peer_id: String,
    #[serde(default)]
    pub(crate) price_wei: String,
    #[serde(default)]
    pub(crate) wallet_address: String,
    /// Multiaddresses where this seeder can be reached.
    #[serde(default)]
    pub(crate) multiaddrs: Vec<String>,
    /// ECDSA signature of "seeder:{peer_id}:{file_hash}:{wallet_address}" by wallet key.
    /// Proves this seeder controls the claimed wallet (prevents payment redirection).
    #[serde(default)]
    pub(crate) signature: String,
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

/// File metadata stored in DHT under `chiral_file_{hash}`.
///
/// In the provider-records model this blob is **immutable** per-file: the
/// publisher writes it once at share time and never touches it again.
/// Seeders live in Kademlia provider records plus `chiral_seeder_{hash}_{peerId}`
/// entries — not in this struct. The `wallet_address` field here is the
/// publisher's wallet, used only to verify `publisher_signature`.
///
/// Serde defaults on every field preserve forward-compat with records
/// written by older clients that had extra fields (peer_id, price_wei,
/// seeders, etc). Those get silently dropped on read.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    hash: String,
    file_name: String,
    file_size: u64,
    protocol: String,
    created_at: u64,
    /// Publisher's wallet address (for publisher_signature verification).
    #[serde(default)]
    wallet_address: String,
    /// ECDSA signature of "file:{hash}:{file_name}:{file_size}" by the
    /// publisher's wallet. Proves the metadata was created by wallet_address.
    #[serde(default)]
    publisher_signature: String,
}

impl FileMetadata {
    /// Create the message bytes that are signed by the publisher.
    /// Length-prefixed canonical encoding so an attacker-controlled
    /// `file_name` containing a colon cannot shift content across field
    /// boundaries (same defect class as the historical
    /// `canonical_signing_payload` and `SiteDirectoryEntry::sign_payload`
    /// bugs — see `fm_agent/bug_validation/`).
    fn sign_payload(hash: &str, file_name: &str, file_size: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + hash.len() + file_name.len());
        out.extend_from_slice(b"file");
        out.push(0);
        for part in [hash.as_bytes(), file_name.as_bytes()] {
            out.extend_from_slice(&(part.len() as u32).to_le_bytes());
            out.extend_from_slice(part);
        }
        out.extend_from_slice(&file_size.to_le_bytes());
        out
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

// ============================================================================
// Per-seeder DHT key schema (Stage 1 of provider-based refactor).
//
// New model:
//   - `chiral_file_{hash}`            → immutable file metadata (publisher-signed).
//   - `chiral_seeder_{hash}_{peerId}` → one seeder's own entry (price, wallet,
//                                       multiaddrs, signature). Each seeder
//                                       writes ONLY their own key — no
//                                       read-modify-write races.
//   - Kademlia providers on the file hash → live peer-ID set of currently-
//                                           online seeders. Auto-expires.
//
// `publish_seeder_entry` + `start_providing_file` together make this node a
// discoverable seeder. `fetch_seeders` is the reverse operation: discover
// providers via Kademlia, then fetch each one's per-seeder record.
// ============================================================================

/// DHT key namespacing a seeder's per-file metadata record.
fn seeder_entry_key(file_hash: &str, peer_id: &str) -> String {
    format!("chiral_seeder_{}_{}", file_hash, peer_id)
}

/// Publish this node's seeder entry for a file and register it as a Kademlia
/// provider. Safe to call repeatedly — each call refreshes the entry and
/// republishes the provider record.
pub async fn publish_seeder_entry(
    dht: &dht::DhtService,
    file_hash: &str,
    seeder: &SeederInfo,
) -> Result<(), String> {
    let json = serde_json::to_string(seeder).map_err(|e| e.to_string())?;
    let key = seeder_entry_key(file_hash, &seeder.peer_id);
    dht.put_dht_value(key, json).await?;
    dht.start_providing_file(file_hash.to_string()).await?;
    Ok(())
}

/// Remove this node from a file's provider set AND purge its per-seeder
/// record from the local Kademlia store. Stopping providing alone isn't
/// enough — libp2p-kad auto-republishes locally-stored records every
/// ~3 min, which would keep a stale per-seeder entry alive on the network
/// long after we've deleted the file. Removing the local record kills the
/// republish cycle; remote replicas age out naturally over the record TTL.
pub async fn remove_seeder_entry(
    dht: &dht::DhtService,
    file_hash: &str,
) -> Result<(), String> {
    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    if !peer_id.is_empty() {
        let key = seeder_entry_key(file_hash, &peer_id);
        let _ = dht.remove_dht_record(key).await;
    }
    dht.stop_providing_file(file_hash.to_string()).await
}

/// Discover all currently-online seeders for a file. Queries Kademlia for
/// the provider set, then fetches each provider's per-seeder record in
/// parallel. Signature-verified entries are returned; unsigned or
/// signature-invalid entries are logged and dropped.
async fn fetch_seeders(
    dht: &dht::DhtService,
    file_hash: &str,
) -> Result<Vec<SeederInfo>, String> {
    let providers = dht.get_file_providers(file_hash.to_string()).await?;
    if providers.is_empty() {
        return Ok(Vec::new());
    }
    let fetches = providers.into_iter().map(|peer_id| {
        let key = seeder_entry_key(file_hash, &peer_id);
        async move {
            let fetched = match dht.get_dht_value(key).await {
                Ok(Some(json)) => serde_json::from_str::<SeederInfo>(&json).ok(),
                _ => None,
            };
            (peer_id, fetched)
        }
    });
    let results = futures::future::join_all(fetches).await;
    let mut seeders = Vec::new();
    for (peer_id, entry) in results {
        match entry {
            Some(entry) if entry.verify(file_hash) => {
                // Signed + valid.
                seeders.push(entry);
            }
            Some(entry) => {
                // Unsigned or invalid signature. CLAUDE.md's trust contract
                // ("ECDSA-signed seeder entries prevent payment redirection")
                // requires us to drop these — accepting them would let any
                // peer redirect downloads to an attacker wallet. Empty-sig
                // entries fall in here too.
                let reason = if entry.signature.is_empty() { "unsigned" } else { "INVALID signature" };
                println!(
                    "  ⚠️ Seeder entry for {} {} — dropping",
                    &peer_id[..20.min(peer_id.len())],
                    reason
                );
            }
            None => {
                // Provider advertises the hash but their per-seeder record
                // isn't retrievable yet (put replication lagging, or peer
                // never got a chance to persist it). Emit a stub with empty
                // wallet/price so the UI still surfaces the seeder; the
                // download path treats stubs as "not yet trustworthy" —
                // the chunked-transfer FileInfo response is what the
                // downloader will actually use, and that path needs its
                // own signature wiring (FM-A09).
                seeders.push(SeederInfo {
                    peer_id,
                    price_wei: String::new(),
                    wallet_address: String::new(),
                    multiaddrs: Vec::new(),
                    signature: String::new(),
                });
            }
        }
    }
    Ok(seeders)
}

/// Build a signed SeederInfo entry. Returns `None` if the wallet
/// address or private key are missing — readers reject unsigned
/// SeederInfo entries, so publishing one would just poison the DHT
/// with a record nobody trusts.
pub fn try_make_signed_seeder(
    peer_id: &str,
    file_hash: &str,
    price_wei: &str,
    wallet_address: &str,
    multiaddrs: Vec<String>,
    private_key: Option<&str>,
) -> Option<SeederInfo> {
    let key = match private_key {
        Some(k) if !k.is_empty() => k,
        _ => return None,
    };
    if wallet_address.is_empty() {
        return None;
    }
    let payload = SeederInfo::sign_payload(peer_id, file_hash, wallet_address);
    let signature = wallet::sign_message(key, &payload).unwrap_or_default();
    if signature.is_empty() {
        return None;
    }
    let entry = SeederInfo {
        peer_id: peer_id.to_string(),
        price_wei: price_wei.to_string(),
        wallet_address: wallet_address.to_string(),
        multiaddrs,
        signature,
    };
    if !entry.verify(file_hash) {
        return None;
    }
    Some(entry)
}

/// Build a signed FileMetadata blob. Returns `None` if the wallet
/// address or private key are missing — readers reject unsigned
/// FileMetadata, so publishing one is worse than not publishing at
/// all (the reader's not-found path lets the user retry from a
/// signed publisher).
pub fn try_make_signed_file_metadata(
    hash: &str,
    file_name: &str,
    file_size: u64,
    protocol: &str,
    wallet_address: &str,
    private_key: Option<&str>,
) -> Option<FileMetadata> {
    let key = match private_key {
        Some(k) if !k.is_empty() => k,
        _ => return None,
    };
    if wallet_address.is_empty() {
        return None;
    }
    let mut metadata = FileMetadata {
        hash: hash.to_string(),
        file_name: file_name.to_string(),
        file_size,
        protocol: protocol.to_string(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
        wallet_address: wallet_address.to_string(),
        publisher_signature: String::new(),
    };
    metadata.sign(key);
    if !metadata.verify_publisher() {
        return None;
    }
    Some(metadata)
}

#[tauri::command]
async fn publish_file(
    state: tauri::State<'_, AppState>,
    file_path: String,
    file_name: String,
    protocol: Option<String>,
    price_chi: Option<String>,
    wallet_address: Option<String>,
    private_key: Option<String>,
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
            private_key.clone().unwrap_or_default(),
        )
        .await;

        let our_multiaddrs = dht.get_listening_addresses().await;
        let proto = protocol.unwrap_or_else(|| "WebRTC".to_string());
        let our_seeder = try_make_signed_seeder(
            &peer_id,
            &merkle_root,
            &price_wei_val.to_string(),
            &wallet_addr,
            our_multiaddrs,
            private_key.as_deref(),
        )
        .ok_or_else(|| {
            "Wallet must be unlocked (private key + address required) to publish a file".to_string()
        })?;

        // Always publish the metadata blob. An earlier optimization
        // gated this on "blob already present" but interacted badly
        // with first-hit Kademlia (a stale local copy would skip the
        // put, then expire, and the file would become unreachable).
        // Re-publishing the publisher's own signed blob is safe and
        // refreshes the Kademlia record TTL.
        let dht_key = format!("chiral_file_{}", merkle_root);
        let metadata = try_make_signed_file_metadata(
            &merkle_root,
            &file_name,
            file_size,
            &proto,
            &wallet_addr,
            private_key.as_deref(),
        )
        .ok_or_else(|| {
            "Wallet must be unlocked (private key + address required) to publish a file"
                .to_string()
        })?;
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
        dht.put_dht_value(dht_key, metadata_json).await?;

        if let Err(e) = publish_seeder_entry(dht, &merkle_root, &our_seeder).await {
            println!("Provider publish failed for {}: {}", merkle_root, e);
        }

        println!("File published: {}", merkle_root);
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
    private_key: Option<String>,
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
            private_key.clone().unwrap_or_default(),
        )
        .await;

        let our_multiaddrs = dht.get_listening_addresses().await;
        let our_seeder = try_make_signed_seeder(
            &peer_id,
            &merkle_root,
            &price_wei_val.to_string(),
            &wallet_addr,
            our_multiaddrs,
            private_key.as_deref(),
        )
        .ok_or_else(|| {
            "Wallet must be unlocked (private key + address required) to publish a file".to_string()
        })?;

        // Always publish; see publish_file for rationale.
        let dht_key = format!("chiral_file_{}", merkle_root);
        let metadata = try_make_signed_file_metadata(
            &merkle_root,
            &file_name,
            file_size,
            "WebRTC",
            &wallet_addr,
            private_key.as_deref(),
        )
        .ok_or_else(|| {
            "Wallet must be unlocked (private key + address required) to publish a file"
                .to_string()
        })?;
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
        dht.put_dht_value(dht_key, metadata_json).await?;

        if let Err(e) = publish_seeder_entry(dht, &merkle_root, &our_seeder).await {
            println!("Provider publish failed for {}: {}", merkle_root, e);
        }

        println!(
            "File data published: {}",
            merkle_root
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
    folder_access: Vec<dht::FolderAccessPolicy>,
}

fn drive_reseed_price_wei(price_chi: Option<&str>) -> Result<u128, String> {
    let Some(price) = price_chi else {
        return Ok(0);
    };
    let trimmed = price.trim();
    if trimmed.is_empty() || trimmed == "0" {
        return Ok(0);
    }
    wallet::parse_chi_to_wei(trimmed).map_err(|err| {
        format!(
            "invalid Drive item price {:?}; refusing to auto-reseed as free: {}",
            trimmed, err
        )
    })
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
                item.storage_path.as_ref().map(|sp| {
                    let folder_access = paid_folder_policies_for_drive_item(&manifest, item);
                    LocalDriveSeedCandidate {
                        item_id: item.id.clone(),
                        owner: item.owner.clone(),
                        file_name: item.name.clone(),
                        storage_path: sp.clone(),
                        file_size_hint: item.size,
                        protocol: item.protocol.clone(),
                        price_chi: item.price_chi.clone(),
                        folder_access,
                    }
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

    let price_wei = match drive_reseed_price_wei(candidate.price_chi.as_deref()) {
        Ok(price_wei) => price_wei,
        Err(err) => {
            eprintln!(
                "[DRIVE] Skipping auto-reseed repair for item {}: {}",
                candidate.item_id, err
            );
            return None;
        }
    };
    let wallet_addr = candidate.owner.trim().to_string();
    if price_wei > 0 && wallet_addr.is_empty() {
        return None;
    }

    dht.register_shared_file_with_folder_access(
        file_hash.to_string(),
        full_path.to_string_lossy().to_string(),
        candidate.file_name.clone(),
        file_size,
        price_wei,
        wallet_addr.clone(),
        // try_repair has no unlocked wallet — empty private_key means
        // the seeder responder won't sign FileInfo; downloader will
        // fail this seeder and try another. User can re-seed
        // explicitly via the signed publishers.
        String::new(),
        candidate.folder_access.clone(),
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

        // Repair runs without an unlocked wallet, so it has no signing
        // key. Readers reject unsigned FileMetadata / SeederInfo
        // (FM-A07/A08), so writing them here would be useless. Don't
        // publish — the user can re-seed manually once they unlock the
        // wallet, which goes through the signed publishers.
        let _ = (
            peer_id,
            our_multiaddrs,
            file_hash_for_publish,
            file_name_for_publish,
            file_size,
            protocol_for_publish,
            wallet_for_publish,
            price_wei,
        );
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

        // Search for file metadata in DHT. Stage 2: fire the new-schema
        // provider lookup in parallel with the legacy blob lookup so
        // provider-sourced seeders augment the search result without
        // adding latency.
        let dht_key = format!("chiral_file_{}", file_hash);
        println!("Looking up DHT key: {}", dht_key);

        let blob_fut = tokio::time::timeout(
            tokio::time::Duration::from_millis(10000),
            dht.get_dht_value(dht_key.clone()),
        );
        let providers_fut = tokio::time::timeout(
            tokio::time::Duration::from_millis(5000),
            fetch_seeders(dht, &file_hash),
        );
        let (dht_lookup, provider_seeders_res) = tokio::join!(blob_fut, providers_fut);
        let provider_seeders: Vec<SeederInfo> = match provider_seeders_res {
            Ok(Ok(list)) => list,
            _ => Vec::new(),
        };

        match dht_lookup {
            Err(_) => {
                println!("DHT lookup timed out for key: {}", dht_key);
                // If the legacy blob timed out but providers returned seeders,
                // still surface a result from the new-schema path.
                if !provider_seeders.is_empty() {
                    println!(
                        "Returning {} provider-sourced seeders after blob timeout for {}",
                        provider_seeders.len(),
                        file_hash
                    );
                    let first = provider_seeders.first().cloned();
                    return Ok(Some(SearchResult {
                        hash: file_hash.clone(),
                        file_name: local_result
                            .as_ref()
                            .map(|r| r.file_name.clone())
                            .unwrap_or_default(),
                        file_size: local_result.as_ref().map(|r| r.file_size).unwrap_or(0),
                        seeders: provider_seeders,
                        created_at: 0,
                        price_wei: first
                            .as_ref()
                            .map(|s| s.price_wei.clone())
                            .unwrap_or_default(),
                        wallet_address: first
                            .map(|s| s.wallet_address)
                            .unwrap_or_default(),
                    }));
                }
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

                    // Trust contract: file metadata MUST be signed by its
                    // publisher's wallet. An unsigned or invalid record may
                    // have been forged by any peer (the DHT itself accepts
                    // anything at any key), and downstream code consumes
                    // `metadata.file_name` / `metadata.wallet_address` —
                    // both attacker-controllable in an unsigned record.
                    // Drop the metadata entirely; let the local-seed and
                    // CDN fallbacks below handle the search.
                    if !metadata.verify_publisher() {
                        let reason = if metadata.publisher_signature.is_empty() {
                            "unsigned"
                        } else {
                            "INVALID signature"
                        };
                        println!(
                            "⚠️ File metadata for {} {} — dropping blob. Falling back to provider-sourced seeders.",
                            metadata.hash, reason
                        );
                        // The blob is attacker-forgeable, so we drop its
                        // file_name / wallet_address. But provider records
                        // (`chiral_seeder_<hash>_<peer>`) carry their own
                        // ECDSA signatures verified inside fetch_seeders, so
                        // they're independently trustworthy. Surface them
                        // even when the legacy blob is bad — otherwise a
                        // single forged blob hides every honest seeder.
                        if !provider_seeders.is_empty() {
                            let first = provider_seeders.first().cloned();
                            return Ok(Some(SearchResult {
                                hash: file_hash.clone(),
                                file_name: local_result
                                    .as_ref()
                                    .map(|r| r.file_name.clone())
                                    .unwrap_or_default(),
                                file_size: local_result.as_ref().map(|r| r.file_size).unwrap_or(0),
                                seeders: provider_seeders,
                                created_at: 0,
                                price_wei: first
                                    .as_ref()
                                    .map(|s| s.price_wei.clone())
                                    .unwrap_or_default(),
                                wallet_address: first
                                    .map(|s| s.wallet_address)
                                    .unwrap_or_default(),
                            }));
                        }
                        return if let Some(local) = local_result {
                            Ok(Some(local))
                        } else {
                            Ok(None)
                        };
                    }
                    println!("✅ File metadata signature valid (publisher: {})", metadata.wallet_address);

                    // Seeder list comes exclusively from Kademlia providers +
                    // per-seeder records now. The blob is immutable metadata
                    // (name/size/publisher) — no seeder info in it.
                    let mut seeders = provider_seeders;

                    // Merge local seeder if we're seeding this file but haven't
                    // yet propagated our provider record to other nodes.
                    if let Some(ref local) = local_result {
                        for local_seeder in &local.seeders {
                            if !seeders.iter().any(|s| s.peer_id == local_seeder.peer_id) {
                                seeders.push(local_seeder.clone());
                            }
                        }
                    }

                    let price_wei = seeders
                        .first()
                        .map(|s| s.price_wei.clone())
                        .unwrap_or_default();
                    Ok(Some(SearchResult {
                        hash: metadata.hash,
                        file_name: metadata.file_name,
                        file_size: metadata.file_size,
                        seeders,
                        created_at: metadata.created_at,
                        price_wei,
                        wallet_address: metadata.wallet_address,
                    }))
                }
                Ok(None) => {
                    println!("File not found in DHT: {}", file_hash);
                    // Stage 2: if the legacy blob is absent but providers
                    // surfaced seeders, surface a result from the new schema
                    // alone — this is the path that lets pure-new-schema
                    // publishers still be discovered.
                    if !provider_seeders.is_empty() {
                        println!(
                            "Returning {} provider-sourced seeders despite missing blob for {}",
                            provider_seeders.len(),
                            file_hash
                        );
                        let first = provider_seeders.first().cloned();
                        return Ok(Some(SearchResult {
                            hash: file_hash.clone(),
                            file_name: local_result
                                .as_ref()
                                .map(|r| r.file_name.clone())
                                .unwrap_or_default(),
                            file_size: local_result.as_ref().map(|r| r.file_size).unwrap_or(0),
                            seeders: provider_seeders,
                            created_at: 0,
                            price_wei: first
                                .as_ref()
                                .map(|s| s.price_wei.clone())
                                .unwrap_or_default(),
                            wallet_address: first
                                .map(|s| s.wallet_address)
                                .unwrap_or_default(),
                        }));
                    }
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

fn download_request_timestamp_millis_at(now: std::time::SystemTime) -> Result<u128, String> {
    now.duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|err| {
            format!(
                "Cannot start download because the system clock is before UNIX_EPOCH: {}",
                err
            )
        })
}

fn download_request_id_at(
    kind: &str,
    file_hash: &str,
    now: std::time::SystemTime,
) -> Result<String, String> {
    let file_hash_prefix = &file_hash[..std::cmp::min(8, file_hash.len())];
    let timestamp = download_request_timestamp_millis_at(now)?;
    Ok(format!("{kind}-{file_hash_prefix}-{timestamp}"))
}

fn current_download_request_id(kind: &str, file_hash: &str) -> Result<String, String> {
    download_request_id_at(kind, file_hash, std::time::SystemTime::now())
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
    folder_hash: Option<String>,
    folder_payment_tx: Option<String>,
) -> Result<DownloadStartResult, String> {
    // Phase 2 version gate: refuse paid downloads from out-of-date
    // clients (the frontend modal already prevents this for honest UIs;
    // this handles direct invoke bypasses).
    ensure_version_supported(state.inner()).await?;

    // Stage 3: single chosen seeder — the frontend passes the user's selection
    // as the first (and ideally only) entry. We no longer dispatch to multiple
    // seeders in parallel: the user chose who to pay, so only that seeder gets
    // the request and the payment.
    if seeders.len() > 1 {
        println!(
            "⚠️ start_download received {} seeders; Stage 3 dispatches to the first only. \
             Update the UI to pass a single chosen seeder.",
            seeders.len()
        );
    }
    let _ = file_size; // file_size is used by the frontend for UX; backend no longer charges by size.
    println!(
        "⚡ Starting download: {} (hash: {}) — chosen seeder: {}",
        file_name,
        file_hash,
        seeders.first().cloned().unwrap_or_else(|| "(none)".into())
    );

    // Stage 3: the burn-address download-fee payment has been removed. The
    // per-seeder payment happens inside the dht event loop when the seeder's
    // FileInfo response arrives — that's the payment the user actually
    // authorised (to the seeder's wallet, at the seeder's price). Keeping a
    // separate flat burn fee alongside would double-charge users and muddy
    // the pay-the-chosen-seeder contract.
    // `seeder_price_wei` is consumed below when deciding whether to stash
    // wallet credentials for the event-loop payment path.
    let folder_hash_for_request = folder_hash
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string);
    let folder_payment_tx_for_request = folder_payment_tx
        .as_deref()
        .map(str::trim)
        .filter(|tx| !tx.is_empty())
        .map(str::to_string);

    // First, check if we have the file in local cache
    {
        let storage = state.file_storage.lock().await;
        if let Some(file_data) = storage.get(&file_hash) {
            println!("📁 File found in local cache");

            // Save to downloads folder
            let custom_dir = state.download_directory.lock().await.clone();
            let downloads_dir = get_effective_download_dir(&custom_dir)?;
            let file_path = downloads_dir.join(&file_name);
            let request_id = current_download_request_id("local", &file_hash)?;

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
            let request_id = current_download_request_id("local", &file_hash)?;

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
        let request_id = current_download_request_id("download", &file_hash)?;

        // Store download credentials if wallet is available (needed for file payment in event loop)
        let seeder_price: u128 = seeder_price_wei
            .as_deref()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        if seeder_price > 0 || wallet_address.is_some() || folder_payment_tx_for_request.is_some() {
            if let (Some(ref addr), Some(ref key)) = (&wallet_address, &private_key) {
                let mut creds = state.download_credentials.lock().await;
                creds.insert(
                    request_id.clone(),
                    dht::DownloadCredentials {
                        wallet_address: addr.clone(),
                        private_key: key.clone(),
                        folder_hash: folder_hash_for_request.clone(),
                        folder_payment_tx: folder_payment_tx_for_request.clone(),
                    },
                );
            } else if folder_payment_tx_for_request.is_some() {
                return Err(
                    "Wallet address is required when presenting a folder payment transaction"
                        .to_string(),
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

        // Look up seeder multiaddresses from per-seeder records so we can
        // dial peers directly. Bound the lookup so download startup stays
        // snappy even when DHT lookups are slow.
        let seeder_addrs: std::collections::HashMap<String, Vec<String>> = {
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(1200),
                fetch_seeders(dht, &file_hash),
            )
            .await
            {
                Ok(Ok(list)) => list
                    .into_iter()
                    .map(|s| (s.peer_id, s.multiaddrs))
                    .collect(),
                _ => std::collections::HashMap::new(),
            }
        };

        // Stage 3: dispatch to the chosen seeder only. If this one fails, the
        // user is informed explicitly and can re-select (a new pick = a new
        // payment). No silent fall-through to a seeder the user did not
        // authorise.
        let chosen_seeder = candidate_seeders.first().cloned().ok_or_else(|| {
            "No chosen seeder provided — the Download UI must select one first.".to_string()
        })?;
        let addrs = seeder_addrs.get(&chosen_seeder).cloned().unwrap_or_default();
        println!(
            "Dispatching file request to chosen seeder: {} for file {}",
            chosen_seeder, file_hash
        );
        let result = dht
            .request_file(
                chosen_seeder.clone(),
                file_hash.clone(),
                request_id.clone(),
                addrs,
                folder_hash_for_request,
            )
            .await;

        let request_sent = result.is_ok();
        let last_error = result.err().unwrap_or_default();

        if request_sent {
            Ok(DownloadStartResult {
                request_id,
                status: "requesting".to_string(),
            })
        } else {
            let error_msg = if last_error.is_empty() {
                format!(
                    "Chosen seeder ({}) could not be contacted.",
                    chosen_seeder
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

fn shared_file_credentials_for_price(
    price_wei: u128,
    wallet_address: Option<String>,
    private_key: Option<String>,
) -> Result<(String, String), String> {
    let wallet_addr = wallet_address.unwrap_or_default().trim().to_string();
    let private_key = private_key.unwrap_or_default().trim().to_string();
    if price_wei > 0 {
        if wallet_addr.is_empty() {
            return Err("Wallet address is required when republishing a paid file".to_string());
        }
        if private_key.is_empty() {
            return Err("Private key is required when republishing a paid file".to_string());
        }
    }
    Ok((wallet_addr, private_key))
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
    private_key: Option<String>,
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
    let (wallet_addr, private_key_value) =
        shared_file_credentials_for_price(price_wei, wallet_address, private_key)?;

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
            private_key_value,
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
    private_key: Option<String>,
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
    let (wallet_addr, private_key_value) =
        shared_file_credentials_for_price(price_wei, wallet_address, private_key)?;

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
            private_key_value.clone(),
        )
        .await;

        // Step 2: publish our per-seeder record + register as a Kademlia
        // provider. The file metadata blob is immutable per-file in the
        // provider-records model; if it doesn't already exist we write it
        // once, otherwise we only refresh our seeder entry.
        let peer_id = dht.get_peer_id().await.unwrap_or_default();
        if !peer_id.is_empty() {
            let dht_key = format!("chiral_file_{}", file_hash);
            let our_multiaddrs = dht.get_listening_addresses().await;
            let signing_key = if private_key_value.is_empty() {
                None
            } else {
                Some(private_key_value.as_str())
            };
            let Some(our_seeder) = try_make_signed_seeder(
                &peer_id,
                &file_hash,
                &price_wei.to_string(),
                &wallet_addr,
                our_multiaddrs,
                signing_key,
            ) else {
                println!(
                    "Re-publish for {} skipped — wallet must be unlocked to sign records",
                    file_hash
                );
                return Ok(());
            };

            // Always publish; see publish_file for rationale.
            if let Some(metadata) = try_make_signed_file_metadata(
                &file_hash,
                &file_name,
                file_size,
                "WebRTC",
                &wallet_addr,
                signing_key,
            ) {
                if let Ok(metadata_json) = serde_json::to_string(&metadata) {
                    let _ = dht.put_dht_value(dht_key, metadata_json).await;
                }
            }

            if let Err(e) = publish_seeder_entry(dht, &file_hash, &our_seeder).await {
                println!("Provider publish failed for {}: {}", file_hash, e);
            }

            println!(
                "✅ Re-published {} to DHT as seeder {}",
                file_hash, peer_id
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
        // File metadata blob is immutable per-file; seeder liveness lives
        // in Kademlia provider records, so teardown is just stop_providing.
        let _ = remove_seeder_entry(dht, file_hash).await;
        count += 1;
    }
    let _ = peer_id;

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

            // Stop being a Kademlia provider so the new-schema search
            // drops us from seeder results immediately. The immutable file
            // metadata blob is left alone.
            let _ = remove_seeder_entry(dht, file_hash).await;
            let _ = peer_id;

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

fn torrent_creation_timestamp_secs_at(now: std::time::SystemTime) -> Result<u64, String> {
    now.duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|err| {
            format!(
                "Cannot export torrent because the system clock is before UNIX_EPOCH: {}",
                err
            )
        })
}

fn torrent_creation_date_entry_at(now: std::time::SystemTime) -> Result<String, String> {
    let creation_date = torrent_creation_timestamp_secs_at(now)?;
    Ok(format!("13:creation datei{}e", creation_date))
}

fn current_torrent_creation_date_entry() -> Result<String, String> {
    torrent_creation_date_entry_at(std::time::SystemTime::now())
}

// Re-export wallet types used by AppState and Tauri commands
use wallet::TransactionMeta;

#[tauri::command]
async fn get_wallet_balance(
    address: String,
) -> Result<wallet::WalletBalanceResult, String> {
    // Try the canonical RPC first; if it's firewall-blocked or down,
    // fall back to the relay's `/api/chain/rpc` proxy on port 8080
    // (which is the same chain, just same-origin-proxied through the
    // gateway). The wallet still never touches local Geth — that's
    // the rule that prevents private-fork balance leaks.
    let endpoints = geth::wallet_rpc_endpoints();
    let result = rpc_client::call_with_fallbacks(
        &endpoints,
        "eth_getBalance",
        serde_json::json!([address, "latest"]),
    )
    .await?;
    let hex = result
        .as_str()
        .ok_or_else(|| format!("eth_getBalance returned a non-string hex value: {result}"))?;
    let wei = rpc_client::hex_to_u128(hex)
        .map_err(|e| format!("eth_getBalance: {e}"))?;
    Ok(wallet::WalletBalanceResult {
        balance: rpc_client::wei_to_chi_string(wei),
        balance_wei: wei.to_string(),
    })
}



/// Send a transaction from one address to another (signs locally).
/// Routes through the canonical RPC fallback list — a user running
/// local geth that's isolated from the network would otherwise submit
/// the tx to their own private chain, where nobody (including the CDN)
/// can see it. The fallback list lets the relay's :8080 proxy stand in
/// when the direct :8545 is firewalled off (e.g. on the canonical
/// relay box after the 2026-05 lockdown).
#[tauri::command]
async fn send_transaction(
    from_address: String,
    to_address: String,
    amount: String,
    private_key: String,
) -> Result<wallet::SendTransactionResult, String> {
    let endpoints = geth::wallet_rpc_endpoints();
    wallet::send_transaction(&endpoints, &from_address, &to_address, &amount, &private_key).await
}

#[tauri::command]
async fn get_transaction_receipt(tx_hash: String) -> Result<Option<serde_json::Value>, String> {
    let endpoint = geth::wallet_rpc_endpoint();
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
    let endpoint = geth::wallet_rpc_endpoint();
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
    let creation_date_entry = current_torrent_creation_date_entry()?;
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

/// Stop Geth and wipe the local chaindata so the next start re-inits
/// from genesis and re-syncs against the canonical bootnode. Used to
/// recover from a private-fork situation (the mining diagnostic flags
/// `diverged: true` when the local Geth's balance for the miner address
/// differs from the canonical RPC's balance by more than 0.001 CHI).
/// Mining rewards on the pre-reset fork are unrecoverable — they were
/// never on the canonical chain.
#[tauri::command]
async fn reset_local_chain(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    // Stop first; reset_chain refuses to wipe chaindata while a Geth
    // process is still pinned to it (open file handles + the active
    // process would either fight the rm or miss the wipe entirely).
    geth.stop()?;
    geth.reset_chain()
}

#[tauri::command]
async fn get_geth_status(state: tauri::State<'_, AppState>) -> Result<GethStatus, String> {
    let geth = state.geth.lock().await;
    geth.get_status().await
}

#[tauri::command]
async fn start_mining(
    state: tauri::State<'_, AppState>,
    threads: Option<u32>,
) -> Result<(), String> {
    let geth = state.geth.lock().await;
    geth.start_mining(threads.unwrap_or(1)).await
}

#[tauri::command]
async fn stop_mining(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let geth = state.geth.lock().await;
    geth.stop_mining().await
}

#[tauri::command]
async fn get_mining_status(state: tauri::State<'_, AppState>) -> Result<MiningStatus, String> {
    let geth = state.geth.lock().await;
    geth.get_mining_status().await
}

/// Diagnose the "mining page shows N CHI but wallet page shows 0" class
/// of bug. Queries the same wallet address on both the local Geth node
/// (what the mining page reports) and the canonical RPC (what the
/// wallet page reports) and returns both balances plus a derived
/// `diverged` flag. The Mining page surfaces a banner when these
/// disagree, which means either:
///   - the local node is mining on a private fork that hasn't synced
///     to the canonical chain, or
///   - the canonical RPC is unreachable / down.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MiningBalanceDiagnostic {
    address: String,
    local_balance_wei: String,
    local_balance_chi: f64,
    local_error: Option<String>,
    canonical_balance_wei: String,
    canonical_balance_chi: f64,
    canonical_error: Option<String>,
    diverged: bool,
}

#[tauri::command]
async fn get_mining_balance_diagnostic(address: String) -> Result<MiningBalanceDiagnostic, String> {
    if address.is_empty() {
        return Err("address required".to_string());
    }
    async fn fetch_wei_single(endpoint: &str, addr: &str) -> Result<u128, String> {
        let v = rpc_client::call(
            endpoint,
            "eth_getBalance",
            serde_json::json!([addr, "latest"]),
        )
        .await?;
        let hex = v
            .as_str()
            .ok_or_else(|| format!("eth_getBalance returned a non-string hex value: {v}"))?;
        rpc_client::hex_to_u128(hex).map_err(|e| format!("eth_getBalance: {e}"))
    }
    async fn fetch_wei_fallback(endpoints: &[String], addr: &str) -> Result<u128, String> {
        let v = rpc_client::call_with_fallbacks(
            endpoints,
            "eth_getBalance",
            serde_json::json!([addr, "latest"]),
        )
        .await?;
        let hex = v
            .as_str()
            .ok_or_else(|| format!("eth_getBalance returned a non-string hex value: {v}"))?;
        rpc_client::hex_to_u128(hex).map_err(|e| format!("eth_getBalance: {e}"))
    }
    let local_endpoint = "http://127.0.0.1:8545";
    let canonical_endpoints = geth::wallet_rpc_endpoints();
    let (local_res, canonical_res) = tokio::join!(
        fetch_wei_single(local_endpoint, &address),
        fetch_wei_fallback(&canonical_endpoints, &address),
    );
    let (local_wei, local_error) = match local_res {
        Ok(w) => (w, None),
        Err(e) => (0u128, Some(e)),
    };
    let (canonical_wei, canonical_error) = match canonical_res {
        Ok(w) => (w, None),
        Err(e) => (0u128, Some(e)),
    };
    // "Diverged" when both sides answered AND the gap is meaningful
    // (more than ~0.001 CHI to avoid noise from a single fee). If
    // either side errored, divergence is unknowable; the UI shows an
    // RPC-error indicator instead of a divergence banner.
    let diverged = local_error.is_none()
        && canonical_error.is_none()
        && local_wei.abs_diff(canonical_wei) > 1_000_000_000_000_000u128;
    Ok(MiningBalanceDiagnostic {
        address: address.to_lowercase(),
        local_balance_wei: local_wei.to_string(),
        local_balance_chi: local_wei as f64 / 1e18,
        local_error,
        canonical_balance_wei: canonical_wei.to_string(),
        canonical_balance_chi: canonical_wei as f64 / 1e18,
        canonical_error,
        diverged,
    })
}

#[tauri::command]
async fn get_gpu_mining_capabilities(
    state: tauri::State<'_, AppState>,
) -> Result<GpuMiningCapabilities, String> {
    let miner = state.gpu_miner.lock().await;
    Ok(miner.capabilities())
}

#[tauri::command]
async fn list_gpu_devices(state: tauri::State<'_, AppState>) -> Result<Vec<GpuDevice>, String> {
    let miner = state.gpu_miner.lock().await;
    miner.list_devices()
}

#[tauri::command]
async fn start_gpu_mining(
    state: tauri::State<'_, AppState>,
    device_ids: Option<Vec<String>>,
    utilization_percent: Option<u8>,
) -> Result<(), String> {
    // The miner address comes from whatever geth was started with —
    // ethminer talks getwork to geth, and geth's --miner.etherbase is
    // what actually determines the reward destination.
    let miner_address = {
        let geth = state.geth.lock().await;
        geth.get_mining_status()
            .await
            .ok()
            .and_then(|s| s.miner_address)
            .unwrap_or_default()
    };
    if miner_address.is_empty() {
        return Err("Start geth with a miner address before GPU mining".to_string());
    }
    let mut miner = state.gpu_miner.lock().await;
    miner.start(&miner_address, device_ids, utilization_percent)
}

#[tauri::command]
async fn stop_gpu_mining(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut miner = state.gpu_miner.lock().await;
    miner.stop()
}

#[tauri::command]
async fn get_gpu_mining_status(
    state: tauri::State<'_, AppState>,
) -> Result<GpuMiningStatus, String> {
    let miner = state.gpu_miner.lock().await;
    Ok(miner.status())
}



#[tauri::command]
async fn set_miner_address(
    state: tauri::State<'_, AppState>,
    address: String,
) -> Result<(), String> {
    let mut geth = state.geth.lock().await;
    geth.set_miner_address(&address).await
}

#[tauri::command]
fn get_chain_id() -> u64 {
    geth::chain_id()
}

// ============================================================================
// Network selection commands
// ============================================================================

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkInfo {
    name: String,
    display_name: String,
    chain_id: u64,
}

fn network_info(cfg: &network::NetworkConfig) -> NetworkInfo {
    NetworkInfo {
        name: cfg.name.to_string(),
        display_name: cfg.display_name.to_string(),
        chain_id: cfg.chain_id,
    }
}

/// Return the active network config (resolved at process start).
#[tauri::command]
fn get_active_network() -> NetworkInfo {
    network_info(network::active())
}

/// Return every configured network.
#[tauri::command]
fn list_networks() -> Vec<NetworkInfo> {
    network::ALL.iter().map(|c| network_info(c)).collect()
}

/// Persist the active-network choice to disk. The change takes effect on the
/// next app launch — geth chain state, DHT identity, and wallet tx history
/// must all swap atomically, and doing that hot is too fragile.
#[tauri::command]
fn set_active_network(name: String) -> Result<(), String> {
    network::set_active(&name)
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
    let data_dir = network::data_dir().join("geth");
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
// Bootstrap Health Commands (frontend stubs — real bootstrap discovery was
// dropped with the geth rewrite, since freshnet is solo-mining only).
// ============================================================================

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BootstrapHealthReport {
    total_nodes: usize,
    healthy_nodes: usize,
    nodes: Vec<serde_json::Value>,
    timestamp: u64,
    is_healthy: bool,
    healthy_enode_string: String,
}

fn current_bootstrap_health() -> BootstrapHealthReport {
    let cfg = network::active();
    let has_enode = !cfg.geth_bootstrap_enode.is_empty();
    BootstrapHealthReport {
        total_nodes: if has_enode { 1 } else { 0 },
        healthy_nodes: if has_enode { 1 } else { 0 },
        nodes: Vec::new(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        is_healthy: true,
        healthy_enode_string: cfg.geth_bootstrap_enode.to_string(),
    }
}

#[tauri::command]
async fn check_bootstrap_health() -> Result<BootstrapHealthReport, String> {
    Ok(current_bootstrap_health())
}

#[tauri::command]
async fn get_bootstrap_health() -> Result<Option<BootstrapHealthReport>, String> {
    Ok(Some(current_bootstrap_health()))
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

/// Decrypt file data using our keypair.
/// Returns raw bytes over binary IPC — JSON `number[]` would ~10× inflate
/// the wire payload for anything larger than a few KB.
#[tauri::command]
async fn decrypt_file_data(
    state: tauri::State<'_, AppState>,
    encrypted_bundle: encryption::EncryptedFileBundle,
) -> Result<tauri::ipc::Response, String> {
    let keypair_guard = state.encryption_keypair.lock().await;
    let keypair = keypair_guard
        .as_ref()
        .ok_or("Encryption keypair not initialized")?;

    let bytes = encryption::decrypt_with_keypair(&encrypted_bundle, keypair)?;
    Ok(tauri::ipc::Response::new(bytes))
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
        cdn_url: None,
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
    owner_wallet: String,
    private_key: String,
) -> Result<String, String> {
    if owner_wallet.is_empty() || private_key.is_empty() {
        return Err("Wallet must be unlocked to publish a site to a relay".into());
    }
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

    let owner_lower = owner_wallet.to_lowercase();
    let payload = relay_share_proxy::register_payload("site", &site_id, &owner_lower, &origin);
    let signature = wallet::sign_message(&private_key, &payload)
        .map_err(|e| format!("Failed to sign register payload: {}", e))?;

    // The shared client's 5s default is tuned for JSON-RPC; relay
    // register can take longer on a congested or distant relay. Bring
    // this in line with the share-register path's 30s budget.
    let resp = rpc_client::client()?
        .post(&url)
        .json(&serde_json::json!({
            "site_id": site_id,
            "origin_url": origin,
            "owner_wallet": owner_lower,
            "signature": signature,
        }))
        .timeout(std::time::Duration::from_secs(30))
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
    owner_wallet: String,
    private_key: String,
) -> Result<(), String> {
    if owner_wallet.is_empty() || private_key.is_empty() {
        return Err("Wallet must be unlocked to unpublish a site from a relay".into());
    }
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

    let path = format!("/api/sites/relay-register/{}", site_id);
    let url = format!("{}{}", relay_base, path);

    // Owner-proof header: server checks the recovered wallet matches
    // the site's stored `owner_wallet` (FM-A04/A05).
    let owner_lower = owner_wallet.to_lowercase();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let proof_payload = auth::owner_proof_payload(&owner_lower, ts, "DELETE", &path);
    let signature = wallet::sign_message(&private_key, &proof_payload)
        .map_err(|e| format!("Failed to sign unregister proof: {}", e))?;

    let resp = rpc_client::client()?
        .delete(&url)
        .header("X-Owner", &owner_lower)
        .header("X-Owner-Sig", format!("{}:{}", ts, signature))
        .timeout(std::time::Duration::from_secs(30))
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
// CDN site hosting — always-on counterpart to relay tunnels.
//
// Relay-published sites stop responding when the local daemon goes offline
// (the relay is just a reverse proxy through a WebSocket tunnel that the
// owner's process holds open). Uploading to a CDN copies every file to the
// CDN's disk, so visitors keep getting served even when the owner closes
// their app.
//
// The CDN charges price-per-MB-month × site_size_mb × duration; the client
// pays the CDN's wallet and POSTs a multipart/form-data of every file with
// each form-field's filename set to the file's path relative to the site
// root. The CDN serves the result at <cdn_base>/cdn/sites/<site_id>/.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CdnSitePublishResult {
    site_id: String,
    cdn_url: String,
    file_count: u32,
    total_size_bytes: u64,
    expires_at: u64,
    payment_tx: String,
}

/// Stage progress event for CDN site upload. Emitted on the
/// `cdn-upload-progress` Tauri channel so the upload modal can show what
/// the long blocking call is actually doing instead of just spinning.
/// Stages, in order: `preparing` → `quoting` → `paying` → `paid` →
/// `reading` → `uploading` → `done` (or `error` from any prior stage).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CdnUploadProgress {
    site_id: String,
    stage: &'static str,
    message: Option<String>,
    file_count: Option<u64>,
    total_bytes: Option<u64>,
    total_cost_chi: Option<String>,
    tx_hash: Option<String>,
}

impl CdnUploadProgress {
    fn new(site_id: &str, stage: &'static str) -> Self {
        Self {
            site_id: site_id.to_string(),
            stage,
            message: None,
            file_count: None,
            total_bytes: None,
            total_cost_chi: None,
            tx_hash: None,
        }
    }
}

#[tauri::command]
async fn publish_site_to_cdn(
    app: tauri::AppHandle,
    site_id: String,
    cdn_url: String,
    duration_days: Option<u64>,
    owner_wallet: String,
    private_key: String,
) -> Result<CdnSitePublishResult, String> {
    // Helper: emit a stage event. The button is locked for ~30-180s on
    // freshnet (multipart upload + on-chain payment verify); without
    // these, the modal spinner looks stuck.
    let emit_stage = |progress: CdnUploadProgress| {
        let _ = app.emit("cdn-upload-progress", progress);
    };
    let emit_error = |stage: &'static str, msg: &str| {
        let mut p = CdnUploadProgress::new(&site_id, "error");
        p.message = Some(format!("{}: {}", stage, msg));
        let _ = app.emit("cdn-upload-progress", p);
    };

    if owner_wallet.is_empty() || private_key.is_empty() {
        emit_error("validate", "Wallet must be unlocked");
        return Err("Wallet must be unlocked before publishing to a CDN".into());
    }
    let duration_days = duration_days.unwrap_or(30);
    if duration_days == 0 {
        emit_error("validate", "Duration must be at least 1 day");
        return Err("Duration must be at least 1 day".into());
    }
    emit_stage(CdnUploadProgress::new(&site_id, "preparing"));

    // Look up the site + walk every file on disk so we can submit the full
    // tree as a multipart upload.
    let mut all_sites = hosting::load_sites();
    let site = all_sites
        .iter()
        .find(|s| s.id == site_id)
        .ok_or_else(|| {
            emit_error("lookup", "Site not found");
            format!("Site not found: {}", site_id)
        })?
        .clone();
    let site_dir = std::path::PathBuf::from(&site.directory);
    if !site_dir.is_dir() {
        let m = format!("Site directory missing on disk: {}", site.directory);
        emit_error("lookup", &m);
        return Err(m);
    }

    // First walk the tree to enumerate (rel_path, abs_path) pairs only —
    // no I/O of contents yet. We need the total size for the quote, and
    // we want to defer the actual byte reads off the async runtime.
    fn enumerate_files(
        root: &std::path::Path,
        cur: &std::path::Path,
        out: &mut Vec<(String, std::path::PathBuf, u64)>,
    ) -> Result<(), String> {
        let entries = std::fs::read_dir(cur)
            .map_err(|e| format!("Read dir {}: {}", cur.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("Read dir entry: {}", e))?;
            let path = entry.path();
            let ft = entry.file_type().map_err(|e| format!("File type: {}", e))?;
            if ft.is_dir() {
                enumerate_files(root, &path, out)?;
            } else if ft.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|e| format!("Strip prefix: {}", e))?
                    .to_string_lossy()
                    .replace('\\', "/");
                let size = entry
                    .metadata()
                    .map_err(|e| format!("File metadata: {}", e))?
                    .len();
                out.push((rel, path, size));
            }
        }
        Ok(())
    }
    let mut file_index: Vec<(String, std::path::PathBuf, u64)> = Vec::new();
    if let Err(e) = enumerate_files(&site_dir, &site_dir, &mut file_index) {
        emit_error("enumerate", &e);
        return Err(e);
    }
    if file_index.is_empty() {
        emit_error("enumerate", "Site directory is empty");
        return Err("Site directory is empty".into());
    }
    let total_size: u64 = file_index.iter().map(|(_, _, sz)| *sz).sum();
    {
        let mut p = CdnUploadProgress::new(&site_id, "quoting");
        p.file_count = Some(file_index.len() as u64);
        p.total_bytes = Some(total_size);
        emit_stage(p);
    }

    // Get pricing + CDN wallet so we know who to pay and how much. Use
    // exact `bytes=` (not the legacy `sizeMb=` f64 path) so the client
    // quote and the server's upload-time `required_upload_wei` math agree
    // bit-for-bit — under-quoting here used to make the upload reject the
    // payment as "amount mismatch".
    let cdn_base = cdn_url.trim_end_matches('/').to_string();
    let pricing_url = format!(
        "{}/api/cdn/pricing?bytes={}&durationDays={}",
        cdn_base, total_size, duration_days
    );
    let http_client = rpc_client::client()?;
    let pricing: serde_json::Value = match http_client.get(&pricing_url).send().await {
        Ok(r) => match r.json().await {
            Ok(j) => j,
            Err(e) => {
                let m = format!("Parse pricing response: {}", e);
                emit_error("quoting", &m);
                return Err(m);
            }
        },
        Err(e) => {
            let m = format!("CDN pricing request failed: {}", e);
            emit_error("quoting", &m);
            return Err(m);
        }
    };
    let total_cost_chi = match pricing.get("totalCostChi").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            let m = "CDN pricing response missing totalCostChi".to_string();
            emit_error("quoting", &m);
            return Err(m);
        }
    };

    let status: serde_json::Value = match http_client
        .get(&format!("{}/api/cdn/status", cdn_base))
        .send()
        .await
    {
        Ok(r) => match r.json().await {
            Ok(j) => j,
            Err(e) => {
                let m = format!("Parse status response: {}", e);
                emit_error("quoting", &m);
                return Err(m);
            }
        },
        Err(e) => {
            let m = format!("CDN status request failed: {}", e);
            emit_error("quoting", &m);
            return Err(m);
        }
    };
    let cdn_wallet = match status
        .get("walletAddress")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => {
            let m = "CDN wallet address not configured".to_string();
            emit_error("quoting", &m);
            return Err(m);
        }
    };

    // Pay the CDN.
    let payment_tx = if total_cost_chi == "0" {
        String::new()
    } else {
        {
            let mut p = CdnUploadProgress::new(&site_id, "paying");
            p.total_cost_chi = Some(total_cost_chi.clone());
            emit_stage(p);
        }
        let endpoints = geth::wallet_rpc_endpoints();
        let result = match wallet::send_transaction(
            &endpoints,
            &owner_wallet,
            &cdn_wallet,
            &total_cost_chi,
            &private_key,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                let m = format!("CDN payment failed: {}", e);
                emit_error("paying", &m);
                return Err(m);
            }
        };
        let mut p = CdnUploadProgress::new(&site_id, "paid");
        p.tx_hash = Some(result.hash.clone());
        emit_stage(p);
        result.hash
    };

    // Read every file off the async runtime in parallel. tokio::fs::read
    // dispatches each open+read to the blocking thread pool, so a 100-file
    // site is bounded by the slowest single read instead of the sum.
    // Sequential std::fs::read on the async runtime would block libp2p,
    // wallet RPC, and UI events for the duration of the walk.
    {
        let mut p = CdnUploadProgress::new(&site_id, "reading");
        p.file_count = Some(file_index.len() as u64);
        p.total_bytes = Some(total_size);
        emit_stage(p);
    }
    let read_futures = file_index.into_iter().map(|(rel_path, abs_path, _sz)| async move {
        let bytes = tokio::fs::read(&abs_path)
            .await
            .map_err(|e| format!("Read file {}: {}", abs_path.display(), e))?;
        Ok::<(String, Vec<u8>), String>((rel_path, bytes))
    });
    let read_results = futures::future::join_all(read_futures).await;
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(read_results.len());
    for r in read_results {
        match r {
            Ok(item) => files.push(item),
            Err(e) => {
                emit_error("reading", &e);
                return Err(e);
            }
        }
    }

    // Multipart upload — each file gets a separate form-data part with the
    // relative path stuck into the filename slot. Move bytes (don't clone)
    // so the peak memory cost is 1× site size, not 2×.
    let local_file_count = files.len();
    let mut form = reqwest::multipart::Form::new();
    for (rel_path, bytes) in files {
        let part = match reqwest::multipart::Part::bytes(bytes)
            .file_name(rel_path)
            .mime_str("application/octet-stream")
        {
            Ok(p) => p,
            Err(e) => {
                let m = format!("Build multipart part: {}", e);
                emit_error("uploading", &m);
                return Err(m);
            }
        };
        form = form.part("file", part);
    }
    let upload_url = format!("{}/api/cdn/sites/upload", cdn_base);
    {
        let mut p = CdnUploadProgress::new(&site_id, "uploading");
        p.file_count = Some(local_file_count as u64);
        p.total_bytes = Some(total_size);
        if !payment_tx.is_empty() {
            p.tx_hash = Some(payment_tx.clone());
            // The CDN runs `wait_for_tx_mined` server-side after it
            // receives the multipart, so this stage covers the network
            // upload AND the on-chain confirmation wait. Surface that
            // to the user so a 30-60s pause looks like progress, not a
            // hang.
            p.message = Some(
                "Uploading files and verifying payment on-chain (this can take 30–60s)..."
                    .to_string(),
            );
        }
        emit_stage(p);
    }
    // The shared rpc_client has a 5s timeout tuned for JSON-RPC calls. A
    // CDN site upload streams multipart bytes AND waits for on-chain
    // payment verification (often 30-60s on freshnet), which the 5s
    // bound kills with `error sending request: operation timed out`.
    // Override with a per-request 180s timeout — long enough for big
    // sites + slow chain confirms, short enough to surface real hangs.
    let resp = match http_client
        .post(&upload_url)
        .timeout(std::time::Duration::from_secs(180))
        .header("X-Site-Id", &site_id)
        .header("X-Site-Name", &site.name)
        .header("X-Owner-Wallet", &owner_wallet)
        .header("X-Payment-Tx", &payment_tx)
        .header("X-Duration-Days", duration_days.to_string())
        .multipart(form)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let m = format!("CDN upload failed: {}", e);
            emit_error("uploading", &m);
            return Err(m);
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let m = format!("CDN returned {}: {}", status, text);
        emit_error("uploading", &m);
        return Err(m);
    }
    let resp_body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            let m = format!("Parse CDN response: {}", e);
            emit_error("uploading", &m);
            return Err(m);
        }
    };
    let expires_at = resp_body
        .get("expiresAt")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let file_count = resp_body
        .get("fileCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(local_file_count as u64) as u32;

    let public_url = format!("{}/cdn/sites/{}/", cdn_base, site_id);

    // Persist the CDN URL on the local site so the UI can show it.
    if let Some(s) = all_sites.iter_mut().find(|s| s.id == site_id) {
        s.cdn_url = Some(public_url.clone());
    }
    hosting::save_sites(&all_sites);

    println!(
        "[HOSTING] Site {} uploaded to CDN: {} ({} files, {} bytes)",
        site_id, public_url, file_count, total_size
    );
    {
        let mut p = CdnUploadProgress::new(&site_id, "done");
        p.file_count = Some(file_count as u64);
        p.total_bytes = Some(total_size);
        if !payment_tx.is_empty() {
            p.tx_hash = Some(payment_tx.clone());
        }
        emit_stage(p);
    }
    Ok(CdnSitePublishResult {
        site_id,
        cdn_url: public_url,
        file_count,
        total_size_bytes: total_size,
        expires_at,
        payment_tx,
    })
}

#[tauri::command]
async fn unpublish_site_from_cdn(
    site_id: String,
    cdn_url: Option<String>,
    owner_wallet: String,
    private_key: String,
) -> Result<(), String> {
    if owner_wallet.is_empty() || private_key.is_empty() {
        return Err("Wallet must be unlocked to unpublish from a CDN".into());
    }
    let mut all_sites = hosting::load_sites();
    let site = all_sites
        .iter()
        .find(|s| s.id == site_id)
        .ok_or_else(|| format!("Site not found: {}", site_id))?
        .clone();

    // Pull the CDN base from either the explicit arg or the persisted URL.
    let cdn_base = cdn_url
        .or_else(|| site.cdn_url.clone())
        .ok_or_else(|| "Site is not published to any CDN".to_string())?;
    let cdn_base = match cdn_base.find("/cdn/sites/") {
        Some(pos) => cdn_base[..pos].to_string(),
        None => cdn_base.trim_end_matches('/').to_string(),
    };

    // Owner-proof: the CDN's `/api/cdn/sites/:id` DELETE route is now
    // gated by the owner-proof middleware. Sign the (wallet, ts, method,
    // path-with-query) tuple so the server can recover the wallet from
    // the signature and verify it matches the registry's stored owner.
    let owner_lower = owner_wallet.to_lowercase();
    let path = format!("/api/cdn/sites/{}?owner={}", site_id, owner_lower);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let proof_payload = auth::owner_proof_payload(&owner_lower, ts, "DELETE", &path);
    let signature = wallet::sign_message(&private_key, &proof_payload)
        .map_err(|e| format!("Failed to sign unpublish proof: {}", e))?;

    let url = format!("{}{}", cdn_base, path);
    let resp = rpc_client::client()?
        .delete(&url)
        .header("X-Owner", &owner_lower)
        .header("X-Owner-Sig", format!("{}:{}", ts, signature))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to CDN: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("CDN returned {}: {}", status, text));
    }

    if let Some(s) = all_sites.iter_mut().find(|s| s.id == site_id) {
        s.cdn_url = None;
    }
    hosting::save_sites(&all_sites);

    println!("[HOSTING] Site {} removed from CDN", site_id);
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
        payment_wallet: None,
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
    let src = std::path::PathBuf::from(&file_path);
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid file path")?
        .to_string();

    // Stat first so we can reject oversize uploads without copying anything.
    let file_size = tokio::fs::metadata(&src)
        .await
        .map_err(|e| format!("Failed to stat file: {}", e))?
        .len();
    if file_size > 500 * 1024 * 1024 {
        return Err("File exceeds 500 MB limit".into());
    }

    let item_id = ds::generate_id();
    let storage_name = format!("{}_{}", item_id, file_name);
    let mime = ds::mime_from_name(&file_name);
    let files_dir = ds::drive_files_dir().ok_or("Cannot determine storage directory")?;
    tokio::fs::create_dir_all(&files_dir)
        .await
        .map_err(|e| format!("Failed to create storage dir: {}", e))?;
    let dest = files_dir.join(&storage_name);

    // Copy + hash off the async runtime. Previously this used
    // `std::fs::read` → `std::fs::write` → SHA-256 over the in-memory
    // buffer, all synchronously on the tokio runtime — so a multi-GB
    // upload froze libp2p, wallet RPC, and the UI event loop until the
    // copy finished. Now we tokio::fs::copy (delegates to the blocking
    // pool) AND spawn_blocking the hash, both running on threads
    // separate from the async reactor.
    let dest_for_copy = dest.clone();
    let src_for_copy = src.clone();
    let copied_bytes = tokio::fs::copy(&src_for_copy, &dest_for_copy)
        .await
        .map_err(|e| format!("Failed to copy file: {}", e))?;

    // Reuse caller-supplied merkle_root if present (e.g. import flows
    // that already know the hash). Otherwise hash the just-written
    // destination on the blocking pool so the runtime stays free.
    let computed_merkle_root = if let Some(h) = merkle_root.clone().filter(|h| !h.trim().is_empty())
    {
        h
    } else {
        let dest_for_hash = dest.clone();
        tokio::task::spawn_blocking(move || -> Result<String, String> {
            use sha2::{Digest, Sha256};
            use std::io::Read;
            let mut file = std::fs::File::open(&dest_for_hash)
                .map_err(|e| format!("Failed to open uploaded file for hashing: {}", e))?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 1024 * 1024];
            loop {
                let n = file
                    .read(&mut buf)
                    .map_err(|e| format!("Failed to read uploaded file while hashing: {}", e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        })
        .await
        .map_err(|e| format!("Hash task panicked: {}", e))??
    };

    let item = DsItem {
        id: item_id,
        name: file_name.clone(),
        item_type: "file".into(),
        parent_id,
        size: Some(copied_bytes),
        mime_type: Some(mime),
        created_at: ds::now_secs(),
        modified_at: ds::now_secs(),
        starred: false,
        storage_path: Some(storage_name),
        owner,
        is_public: true,
        merkle_root: Some(computed_merkle_root),
        protocol: None,
        price_chi: None,
        payment_wallet: None,
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
        copied_bytes
    );
    Ok(item)
}

fn validate_drive_item_price_update(price_chi: String) -> Result<Option<String>, String> {
    if price_chi.is_empty() {
        return Ok(None);
    }
    wallet::parse_chi_to_wei(&price_chi)
        .map_err(|err| format!("Invalid Drive item price: {}", err))?;
    Ok(Some(price_chi))
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
        item.price_chi = validate_drive_item_price_update(p)?;
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
            // Stop being a Kademlia provider for this file; the immutable
            // file metadata blob is left alone.
            let _ = remove_seeder_entry(dht, hash).await;
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
    publish_drive_file_inner(
        state.inner(),
        &owner,
        &item_id,
        protocol,
        price_chi,
        wallet_address,
        private_key,
    )
    .await
}

/// Shared body for "publish a single Drive file as a paid seeder". Reused by
/// the per-file Tauri command and the folder-level loop in
/// `publish_drive_folder`.
async fn publish_drive_file_inner(
    state: &AppState,
    owner: &str,
    item_id: &str,
    protocol: Option<String>,
    price_chi: Option<String>,
    wallet_address: Option<String>,
    private_key: Option<String>,
) -> Result<ds::DriveItem, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }

    // Look up the Drive item from the manifest.
    let (file_name, storage_path, file_size_hint, existing_merkle_root, folder_access) = {
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
            paid_folder_policies_for_drive_item(&m, item),
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

    // Reuse persisted hash when available so re-publishing from Drive is
    // instant. First-publish hashes the file off the async runtime via
    // spawn_blocking — without this, hashing a multi-GB file blocks
    // every other tokio task (network I/O included), and the
    // publish_drive_folder loop runs the hashes serially across files,
    // turning a folder publish into a multi-minute hang.
    let file_hash = if let Some(root) = existing_merkle_root
        .clone()
        .filter(|h| !h.trim().is_empty())
    {
        root
    } else {
        let path_for_hash = full_path.clone();
        tokio::task::spawn_blocking(move || -> Result<String, String> {
            use sha2::{Digest, Sha256};
            use std::io::Read;
            let mut file = std::fs::File::open(&path_for_hash)
                .map_err(|e| format!("Failed to open file: {}", e))?;
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
            Ok(hex::encode(hasher.finalize()))
        })
        .await
        .map_err(|e| format!("Hash task panicked: {}", e))??
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
        .unwrap_or_else(|| owner.to_string())
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

    dht.register_shared_file_with_folder_access(
        file_hash.clone(),
        full_path_str,
        file_name.clone(),
        actual_size,
        price_wei_val,
        wallet_addr.clone(),
        private_key.clone().unwrap_or_default(),
        folder_access,
    )
    .await;

    let our_multiaddrs = dht.get_listening_addresses().await;
    let our_seeder = try_make_signed_seeder(
        &peer_id, &file_hash, &price_wei_val.to_string(),
        &wallet_addr, our_multiaddrs, private_key.as_deref(),
    )
    .ok_or_else(|| {
        "Cannot publish seeder entry: wallet must be unlocked (private key + address required)"
            .to_string()
    })?;

    // Always publish the metadata blob (refreshes Kademlia record TTL
    // and avoids the first-hit-vs-stale-local interaction described in
    // publish_file). Per-seeder data lives in chiral_seeder_<hash>_<peer>.
    let dht_key = format!("chiral_file_{}", file_hash);
    let metadata = try_make_signed_file_metadata(
        &file_hash, &file_name, actual_size, &proto, &wallet_addr, private_key.as_deref(),
    )
    .ok_or_else(|| {
        "Cannot publish file metadata: wallet must be unlocked (private key + address required)"
            .to_string()
    })?;
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
    dht.put_dht_value(dht_key, metadata_json).await?;
    let _ = peer_id;
    let _ = price_wei_val;
    let _ = wallet_addr;

    if let Err(e) = publish_seeder_entry(&dht, &file_hash, &our_seeder).await {
        println!("Provider publish failed for {}: {}", file_hash, e);
    }

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
    private_key: Option<String>,
) -> Result<(), String> {
    if file_hash.is_empty() {
        return Err("file_hash required".into());
    }

    // Find Drive item by merkle_root matching file_hash
    let (item_id, file_name, storage_path, file_size, folder_access) = {
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
            paid_folder_policies_for_drive_item(&m, item),
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
    dht.register_shared_file_with_folder_access(
        file_hash.clone(), full_path_str, file_name.clone(),
        file_size, price_wei_val, wallet_address.clone(),
        private_key.clone().unwrap_or_default(),
        folder_access,
    ).await;

    // Update DHT record to add ourselves as a seeder
    let our_addrs = dht.get_listening_addresses().await;
    let our_seeder = try_make_signed_seeder(
        &peer_id,
        &file_hash,
        &price_wei_val.to_string(),
        &wallet_address,
        our_addrs,
        private_key.as_deref(),
    )
    .ok_or_else(|| {
        "Wallet must be unlocked (private key + address required) to seed a hosted file"
            .to_string()
    })?;

    // Always publish; see publish_file for rationale.
    let dht_key = format!("chiral_file_{}", file_hash);
    let metadata = try_make_signed_file_metadata(
        &file_hash,
        &file_name,
        file_size,
        "WebRTC",
        &wallet_address,
        private_key.as_deref(),
    )
    .ok_or_else(|| {
        "Wallet must be unlocked (private key + address required) to publish file metadata"
            .to_string()
    })?;
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    dht.put_dht_value(dht_key, metadata_json).await?;
    let _ = peer_id;

    if let Err(e) = publish_seeder_entry(&dht, &file_hash, &our_seeder).await {
        println!("Provider publish failed for {}: {}", file_hash, e);
    }

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
    drive_stop_seeding_inner(state.inner(), &owner, &item_id).await
}

/// Shared body for "stop seeding a single Drive file". Reused by the
/// per-file Tauri command and the folder-level loop in
/// `unpublish_drive_folder`.
async fn drive_stop_seeding_inner(
    state: &AppState,
    owner: &str,
    item_id: &str,
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
            // Stop serving chunks + stop advertising as a Kademlia provider.
            dht.unregister_shared_file(hash).await;
            let _ = remove_seeder_entry(dht, hash).await;
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

/// Result of a folder seed/unseed operation.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveFolderSeedResult {
    folder: ds::DriveItem,
    files_total: usize,
    files_succeeded: usize,
    files_failed: usize,
    /// Per-file failure messages, keyed by file id.
    failures: Vec<DriveFolderFileError>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveFolderFileError {
    item_id: String,
    name: String,
    error: String,
}

/// One file as it appears in a folder bundle. Buyers download each entry
/// individually using the per-file price the seeder advertises in their
/// per-seeder DHT record — this struct just enumerates what's *in* the
/// folder.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderManifestFile {
    /// Path relative to the folder root, e.g. "lecture-1.pdf" or
    /// "src/main.rs". Slash-separated, no leading slash.
    pub rel_path: String,
    pub file_hash: String,
    pub file_size: u64,
}

/// Folder bundle published at `chiral_folder_{folder_hash}`. The folder hash
/// is content-addressed (deterministic from owner + sorted file list) so
/// republishing the same folder produces the same hash, and a buyer with a
/// folder hash can reach the same content from any seeder of those files.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderManifest {
    /// Same as the DHT key suffix.
    pub hash: String,
    /// Display name copied from the seller's local Drive item.
    pub name: String,
    /// Wallet that published this folder bundle. Authoritative — verified
    /// via `publisher_signature` below before this manifest is trusted.
    pub owner_wallet: String,
    pub created_at: u64,
    pub files: Vec<FolderManifestFile>,
    /// Folder-level sale price in wei (u128 as decimal string). Buyers pay
    /// this once to `owner_wallet` and unlock every file in the bundle —
    /// individual file prices are ignored when downloading "as part of
    /// folder F". Empty/"0" = free folder.
    #[serde(default)]
    pub price_wei: String,
    /// Wallet that receives the folder-level payment. Defaults to
    /// `owner_wallet` when missing — kept as a separate field so the
    /// publisher can route payments to a different wallet later without
    /// breaking the owner-signed verification model.
    #[serde(default)]
    pub wallet_address: String,
    /// ECDSA signature by `owner_wallet` over the canonical bytes of every
    /// other field. Readers reject manifests that fail to verify so a
    /// peer can't forge `name` / `owner_wallet` / `files` for a hash they
    /// don't own.
    #[serde(default)]
    pub publisher_signature: String,
}

impl FolderManifest {
    /// Length-prefixed canonical bytes for the v2 signing payload (with
    /// folder-level pricing). Order: hash, name, owner_wallet,
    /// created_at, price_wei, wallet_address, file count, then each
    /// file as (rel_path, file_hash, file_size).
    fn sign_payload(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        for part in [
            self.hash.as_bytes(),
            self.name.as_bytes(),
            self.owner_wallet.as_bytes(),
        ] {
            out.extend_from_slice(&(part.len() as u32).to_le_bytes());
            out.extend_from_slice(part);
        }
        out.extend_from_slice(&self.created_at.to_le_bytes());
        // Folder-level price + wallet must be inside the signed payload —
        // otherwise a hostile peer could substitute a different price /
        // recipient for the same hash.
        for part in [
            self.price_wei.as_bytes(),
            self.wallet_address.as_bytes(),
        ] {
            out.extend_from_slice(&(part.len() as u32).to_le_bytes());
            out.extend_from_slice(part);
        }
        out.extend_from_slice(&(self.files.len() as u32).to_le_bytes());
        for f in &self.files {
            for part in [f.rel_path.as_bytes(), f.file_hash.as_bytes()] {
                out.extend_from_slice(&(part.len() as u32).to_le_bytes());
                out.extend_from_slice(part);
            }
            out.extend_from_slice(&f.file_size.to_le_bytes());
        }
        out
    }

    /// v1 (pre-folder-pricing) signing payload. Same as `sign_payload`
    /// except the `price_wei` + `wallet_address` fields aren't included.
    /// Used as a backward-compat fallback in `verify` so manifests
    /// signed before folder-level pricing existed still validate. Never
    /// used for *new* signatures — `sign` always uses the v2 payload.
    fn sign_payload_legacy_v1(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        for part in [
            self.hash.as_bytes(),
            self.name.as_bytes(),
            self.owner_wallet.as_bytes(),
        ] {
            out.extend_from_slice(&(part.len() as u32).to_le_bytes());
            out.extend_from_slice(part);
        }
        out.extend_from_slice(&self.created_at.to_le_bytes());
        out.extend_from_slice(&(self.files.len() as u32).to_le_bytes());
        for f in &self.files {
            for part in [f.rel_path.as_bytes(), f.file_hash.as_bytes()] {
                out.extend_from_slice(&(part.len() as u32).to_le_bytes());
                out.extend_from_slice(part);
            }
            out.extend_from_slice(&f.file_size.to_le_bytes());
        }
        out
    }

    pub fn sign(&mut self, private_key: &str) {
        let payload = self.sign_payload();
        self.publisher_signature = wallet::sign_message(private_key, &payload).unwrap_or_default();
    }

    pub fn verify(&self) -> bool {
        if self.publisher_signature.is_empty() || self.owner_wallet.is_empty() {
            return false;
        }
        // Try v2 (current) first.
        let payload = self.sign_payload();
        if wallet::verify_signature(&payload, &self.publisher_signature, &self.owner_wallet) {
            return true;
        }
        // Fallback: v1 payload (no price_wei / wallet_address fields).
        // Only accept the legacy verification when those fields are
        // genuinely empty — a manifest that *claims* a non-empty price
        // but verifies under v1 is suspicious (someone tacked price
        // fields onto a legacy-signed manifest), so we reject it.
        if self.price_wei.is_empty() && self.wallet_address.is_empty() {
            let legacy = self.sign_payload_legacy_v1();
            return wallet::verify_signature(&legacy, &self.publisher_signature, &self.owner_wallet);
        }
        false
    }
}

/// Deterministic content hash for a folder bundle. Combines the owner
/// wallet (so two sellers with the same files don't collide on the same
/// folder hash) with the SHA-256 over a canonical ordering of
/// `rel_path \0 file_hash \0` lines.
///
/// `files` does not need to be pre-sorted — we sort here.
fn compute_folder_hash(owner_wallet: &str, files: &[FolderManifestFile]) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&FolderManifestFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    let mut hasher = Sha256::new();
    hasher.update(owner_wallet.to_lowercase().as_bytes());
    hasher.update([0u8]);
    for f in sorted {
        hasher.update(f.rel_path.as_bytes());
        hasher.update([0u8]);
        hasher.update(f.file_hash.as_bytes());
        hasher.update([0u8]);
    }
    hex::encode(hasher.finalize())
}

fn folder_manifest_key(folder_hash: &str) -> String {
    format!("chiral_folder_{}", folder_hash)
}

/// Walk the manifest from `folder_id` and return the IDs (in arbitrary order)
/// of every descendant file owned by `owner`.
fn collect_descendant_files(
    manifest: &ds::DriveManifest,
    owner: &str,
    folder_id: &str,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<String> = vec![folder_id.to_string()];
    while let Some(parent) = stack.pop() {
        for item in &manifest.items {
            if item.owner != owner {
                continue;
            }
            if item.parent_id.as_deref() != Some(parent.as_str()) {
                continue;
            }
            match item.item_type.as_str() {
                "folder" => stack.push(item.id.clone()),
                "file" => out.push((item.id.clone(), item.name.clone())),
                _ => {}
            }
        }
    }
    out
}

fn signing_key_matches_wallet(private_key: &str, wallet_address: &str) -> bool {
    let key = private_key.trim();
    let wallet_address = wallet_address.trim();
    if key.is_empty() || wallet_address.is_empty() {
        return false;
    }
    let payload = b"folder-manifest-owner-preflight-v1";
    wallet::sign_message(key, payload)
        .map(|sig| wallet::verify_signature(payload, &sig, wallet_address))
        .unwrap_or(false)
}

fn positive_price_wei(price_chi: Option<&str>) -> Option<u128> {
    let price = price_chi?.trim();
    if price.is_empty() || price == "0" {
        return None;
    }
    wallet::parse_chi_to_wei(price).ok().filter(|wei| *wei > 0)
}

fn paid_folder_policies_for_drive_item(
    manifest: &ds::DriveManifest,
    item: &ds::DriveItem,
) -> Vec<dht::FolderAccessPolicy> {
    let by_id: HashMap<&str, &ds::DriveItem> = manifest
        .items
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect();
    let mut policies = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();
    let mut current_parent = item.parent_id.as_deref();

    while let Some(parent_id) = current_parent {
        if !visited.insert(parent_id) {
            break;
        }
        let Some(folder) = by_id.get(parent_id).copied() else {
            break;
        };
        if folder.item_type == "folder"
            && folder.owner == item.owner
            && (folder.seed_enabled || folder.seeding)
        {
            if let (Some(folder_hash), Some(price_wei)) = (
                folder
                    .merkle_root
                    .as_deref()
                    .map(str::trim)
                    .filter(|h| !h.is_empty()),
                positive_price_wei(folder.price_chi.as_deref()),
            ) {
                let wallet_address = folder
                    .payment_wallet
                    .as_deref()
                    .map(str::trim)
                    .filter(|wallet| !wallet.is_empty())
                    .unwrap_or(folder.owner.trim());
                if !wallet_address.is_empty() {
                    policies.push(dht::FolderAccessPolicy {
                        folder_hash: folder_hash.to_string(),
                        price_wei,
                        wallet_address: wallet_address.to_string(),
                    });
                }
            }
        }
        current_parent = folder.parent_id.as_deref();
    }

    policies
}

async fn rollback_drive_folder_child_seeds(state: &AppState, owner: &str, file_ids: &[String]) {
    for file_id in file_ids {
        if let Err(e) = drive_stop_seeding_inner(state, owner, file_id).await {
            println!(
                "[DRIVE] Folder publish rollback failed for child {}: {}",
                file_id, e
            );
        }
    }
}

/// Publish a folder bundle for sale at a single folder-level price. Each
/// child file is registered as a free seed (price 0) — payment is
/// collected once at the folder level when the buyer presents a
/// `chiral_folder_<hash>` PaymentProof. The folder hash is
/// content-addressed (same files + same owner → same hash) so re-publishes
/// are stable. `price_chi` here is the FOLDER price, not the per-file
/// price.
#[tauri::command]
async fn publish_drive_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    owner: String,
    folder_id: String,
    protocol: Option<String>,
    price_chi: Option<String>,
    wallet_address: Option<String>,
    private_key: Option<String>,
) -> Result<DriveFolderSeedResult, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }

    // Snapshot the file list under this folder.
    let files: Vec<(String, String)> = {
        let m = state.drive_state.manifest.read().await;
        let folder = m
            .items
            .iter()
            .find(|i| i.id == folder_id && i.owner == owner && i.item_type == "folder")
            .ok_or("Drive folder not found")?;
        let _ = folder; // existence check
        collect_descendant_files(&m, &owner, &folder_id)
    };
    if files.is_empty() {
        return Err("Folder is empty — nothing to sell".into());
    }
    // Live progress events. The button can be locked for a while on
    // first-time-seeded folders (parallel SHA-256 of every child file
    // bounded by the slowest single hash); without these the UI looks
    // frozen even though work is happening.
    let total_files = files.len();
    let _ = app.emit(
        "drive-folder-publish-progress",
        serde_json::json!({
            "folderId": folder_id,
            "stage": "starting",
            "total": total_files,
            "completed": 0,
        }),
    );

    // Compute folder-level price in wei + select payment recipient. Both
    // are baked into the signed FolderManifest, so a hostile peer can't
    // republish the same `chiral_folder_<H>` key with a swapped wallet
    // or price.
    let folder_price_wei: u128 = if let Some(ref p) = price_chi {
        if p.trim().is_empty() || p == "0" {
            0
        } else {
            wallet::parse_chi_to_wei(p)?
        }
    } else {
        0
    };
    let folder_payment_wallet = wallet_address
        .clone()
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| owner.clone())
        .trim()
        .to_string();
    if folder_price_wei > 0 && folder_payment_wallet.is_empty() {
        return Err("Wallet address is required when setting a folder price".to_string());
    }
    if folder_price_wei > 0
        && !private_key
            .as_deref()
            .map(|key| signing_key_matches_wallet(key, &owner))
            .unwrap_or(false)
    {
        return Err(
            "Wallet must be unlocked with the folder owner key to publish a paid folder"
                .to_string(),
        );
    }

    // First pass: per-file publishes happen in parallel so the per-file
    // SHA-256 hashing (now off the async runtime) overlaps across files
    // instead of running serially. For a folder of N first-time-seeded
    // files this is the difference between N×hash_time and 1×hash_time.
    // Each file's merkle_root is persisted to the Drive manifest by the
    // inner function so the folder hash can be computed afterward.
    let completed_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let publish_futures = files.iter().map(|(file_id, file_name)| {
        let state = state.inner();
        let owner = owner.clone();
        let file_id = file_id.clone();
        let file_name = file_name.clone();
        let protocol = protocol.clone();
        let wallet_address = wallet_address.clone();
        let private_key = private_key.clone();
        let app = app.clone();
        let folder_id = folder_id.clone();
        let counter = completed_counter.clone();
        async move {
            let res = publish_drive_file_inner(
                state,
                &owner,
                &file_id,
                protocol,
                Some("0".to_string()),
                wallet_address,
                private_key,
            )
            .await;
            // Emit progress as each child file finishes so the UI can
            // tick a counter / surface failures inline instead of
            // waiting for the whole bulk to resolve.
            let done = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let (ok, err_msg) = match &res {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e.clone())),
            };
            let _ = app.emit(
                "drive-folder-publish-progress",
                serde_json::json!({
                    "folderId": folder_id,
                    "stage": "file",
                    "total": total_files,
                    "completed": done,
                    "fileId": file_id,
                    "fileName": file_name,
                    "ok": ok,
                    "error": err_msg,
                }),
            );
            (file_id, file_name, res)
        }
    });
    let results = futures::future::join_all(publish_futures).await;
    let mut succeeded = 0usize;
    let mut failures: Vec<DriveFolderFileError> = Vec::new();
    for (file_id, file_name, res) in results {
        match res {
            Ok(_) => succeeded += 1,
            Err(e) => failures.push(DriveFolderFileError {
                item_id: file_id,
                name: file_name,
                error: e,
            }),
        }
    }
    let _ = app.emit(
        "drive-folder-publish-progress",
        serde_json::json!({
            "folderId": folder_id,
            "stage": "files_done",
            "total": total_files,
            "completed": total_files,
            "succeeded": succeeded,
            "failed": failures.len(),
        }),
    );

    // Build a folder manifest from every successfully-seeded child file so
    // a buyer can search by *one* hash and download the whole bundle. The
    // folder hash is content-addressed (same files → same hash) so it's
    // stable across re-publishes. Snapshot the manifest once instead of
    // re-acquiring the read lock 3× — the per-file publishes have all
    // committed their merkle_root by now and we don't need a fresh view
    // for each lookup.
    let manifest_snapshot = state.drive_state.manifest.read().await.clone();
    let folder_name = manifest_snapshot
        .items
        .iter()
        .find(|i| i.id == folder_id && i.owner == owner && i.item_type == "folder")
        .map(|i| i.name.clone())
        .unwrap_or_default();
    let folder_root_path = std::path::PathBuf::from(
        manifest_snapshot
            .items
            .iter()
            .find(|i| i.id == folder_id && i.owner == owner)
            .and_then(|i| i.storage_path.clone())
            .unwrap_or_default(),
    );
    // (file_id, FolderManifestFile) — we need the file_id so we can
    // re-register each child with folder context after the folder hash
    // is computed.
    let manifest_entries: Vec<(String, FolderManifestFile)> = {
        let m = &manifest_snapshot;
        files
            .iter()
            .filter_map(|(file_id, _name)| {
                let item = m.items.iter().find(|i| i.id == *file_id)?;
                let hash = item.merkle_root.clone()?;
                if hash.trim().is_empty() {
                    return None;
                }
                let rel = item
                    .storage_path
                    .as_ref()
                    .and_then(|sp| {
                        let sp = std::path::PathBuf::from(sp);
                        if folder_root_path.as_os_str().is_empty() {
                            None
                        } else {
                            sp.strip_prefix(&folder_root_path)
                                .ok()
                                .map(|p| p.to_string_lossy().replace('\\', "/"))
                        }
                    })
                    .unwrap_or_else(|| item.name.clone());
                Some((
                    file_id.clone(),
                    FolderManifestFile {
                        rel_path: rel,
                        file_hash: hash,
                        file_size: item.size.unwrap_or(0),
                    },
                ))
            })
            .collect()
    };
    let manifest_file_ids: Vec<String> = manifest_entries
        .iter()
        .map(|(file_id, _)| file_id.clone())
        .collect();
    let manifest_files: Vec<FolderManifestFile> =
        manifest_entries.iter().map(|(_, f)| f.clone()).collect();

    let folder_hash_opt = if manifest_files.is_empty() {
        None
    } else {
        let hash = compute_folder_hash(&owner, &manifest_files);
        let mut manifest = FolderManifest {
            hash: hash.clone(),
            name: folder_name,
            owner_wallet: owner.clone(),
            created_at: ds::now_secs(),
            files: manifest_files,
            price_wei: folder_price_wei.to_string(),
            wallet_address: folder_payment_wallet.clone(),
            publisher_signature: String::new(),
        };
        let signed = match private_key.as_deref() {
            Some(key) if !key.is_empty() => {
                manifest.sign(key);
                manifest.verify()
            }
            _ => false,
        };
        if !signed {
            println!(
                "[DRIVE] Folder manifest publish for {} skipped — wallet must be unlocked to sign the bundle",
                hash
            );
            if folder_price_wei > 0 {
                rollback_drive_folder_child_seeds(state.inner(), &owner, &manifest_file_ids).await;
                return Err(
                    "Paid folder publish failed because the folder manifest could not be signed"
                        .to_string(),
                );
            }
            None
        } else {
            let dht = {
                let dht_guard = state.dht.lock().await;
                dht_guard.as_ref().cloned()
            };
            if let Some(dht) = dht {
                match serde_json::to_string(&manifest) {
                    Ok(json) => {
                        if let Err(e) = dht.put_dht_value(folder_manifest_key(&hash), json).await {
                            println!("[DRIVE] Folder manifest publish failed for {}: {}", hash, e);
                            if folder_price_wei > 0 {
                                rollback_drive_folder_child_seeds(
                                    state.inner(),
                                    &owner,
                                    &manifest_file_ids,
                                )
                                .await;
                                return Err(format!("Paid folder manifest publish failed: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        if folder_price_wei > 0 {
                            rollback_drive_folder_child_seeds(
                                state.inner(),
                                &owner,
                                &manifest_file_ids,
                            )
                            .await;
                            return Err(format!("Failed to serialize folder manifest: {}", e));
                        }
                    }
                }
                // Register as a Kademlia provider for the folder hash so other
                // peers' get_providers calls find this seller.
                if let Err(e) = dht.start_providing_file(hash.clone()).await {
                    println!("[DRIVE] Folder provider publish failed for {}: {}", hash, e);
                    if folder_price_wei > 0 {
                        rollback_drive_folder_child_seeds(
                            state.inner(),
                            &owner,
                            &manifest_file_ids,
                        )
                        .await;
                        return Err(format!("Paid folder provider publish failed: {}", e));
                    }
                }
                let member_hashes: Vec<String> = manifest_entries
                    .iter()
                    .map(|(_, f)| f.file_hash.clone())
                    .collect();
                let registered = dht
                    .register_folder_bundle_access(
                        hash.clone(),
                        member_hashes.clone(),
                        folder_price_wei,
                        folder_payment_wallet.clone(),
                    )
                    .await;
                if folder_price_wei > 0 && registered != member_hashes.len() {
                    rollback_drive_folder_child_seeds(state.inner(), &owner, &manifest_file_ids)
                        .await;
                    return Err(format!(
                        "Paid folder policy registration failed for {}/{} child files",
                        member_hashes.len().saturating_sub(registered),
                        member_hashes.len()
                    ));
                }
            } else if folder_price_wei > 0 {
                rollback_drive_folder_child_seeds(state.inner(), &owner, &manifest_file_ids).await;
                return Err("DHT not running while publishing paid folder manifest".to_string());
            }
            Some(hash)
        }
    };

    // Mark the folder itself as seeding + record the price so the UI can
    // display the sale state on the folder card. seed_enabled is purely a
    // UI hint here — auto-reseed iterates files, not folders, and each
    // child file already has its own seed_enabled flag set. We also stash
    // the folder hash on `merkle_root` so the existing "Copy Merkle Hash"
    // context-menu item works for sold folders too.
    let folder = {
        let mut m = state.drive_state.manifest.write().await;
        let item = m
            .items
            .iter_mut()
            .find(|i| i.id == folder_id && i.owner == owner && i.item_type == "folder")
            .ok_or("Drive folder not found in manifest")?;
        item.price_chi = price_chi;
        item.payment_wallet = if folder_payment_wallet.is_empty() {
            None
        } else {
            Some(folder_payment_wallet.clone())
        };
        item.protocol = protocol;
        item.seed_enabled = true;
        item.seeding = succeeded > 0;
        if folder_hash_opt.is_some() {
            item.merkle_root = folder_hash_opt.clone();
        }
        item.modified_at = ds::now_secs();
        item.clone()
    };
    state.drive_state.persist().await;

    let _ = app.emit(
        "drive-folder-publish-progress",
        serde_json::json!({
            "folderId": folder_id,
            "stage": "done",
            "total": total_files,
            "completed": total_files,
            "succeeded": succeeded,
            "failed": failures.len(),
            "folderHash": folder_hash_opt,
        }),
    );

    Ok(DriveFolderSeedResult {
        folder,
        files_total: files.len(),
        files_succeeded: succeeded,
        files_failed: failures.len(),
        failures,
    })
}

/// Stop seeding every file inside `folder_id` (recursively) and clear the
/// folder's own seeding state. Mirror of `publish_drive_folder`.
#[tauri::command]
async fn unpublish_drive_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    owner: String,
    folder_id: String,
) -> Result<DriveFolderSeedResult, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }

    let files: Vec<(String, String)> = {
        let m = state.drive_state.manifest.read().await;
        let folder = m
            .items
            .iter()
            .find(|i| i.id == folder_id && i.owner == owner && i.item_type == "folder")
            .ok_or("Drive folder not found")?;
        let _ = folder;
        collect_descendant_files(&m, &owner, &folder_id)
    };
    let total_files = files.len();
    let _ = app.emit(
        "drive-folder-unpublish-progress",
        serde_json::json!({
            "folderId": folder_id,
            "stage": "starting",
            "total": total_files,
            "completed": 0,
        }),
    );

    // Run the per-file teardowns in parallel instead of one-after-another.
    // Each call hits the DHT command channel + acquires the manifest write
    // lock briefly + persists the manifest; under a serial loop, a folder
    // with N seeded files locked the "Stop selling folder" button for
    // N × (DHT round-trip + disk fsync). Parallelizing with join_all caps
    // the wall time at ~max(file_i) instead of sum(file_i). Persistence
    // contention is bounded by tokio's I/O queue; each persist writes the
    // full manifest, but the saved time on DHT round-trips dominates.
    let completed_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stop_futures = files.iter().map(|(file_id, file_name)| {
        let state = state.inner();
        let owner = owner.clone();
        let file_id = file_id.clone();
        let file_name = file_name.clone();
        let app = app.clone();
        let folder_id_evt = folder_id.clone();
        let counter = completed_counter.clone();
        async move {
            let res = drive_stop_seeding_inner(state, &owner, &file_id).await;
            let done = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let (ok, err_msg) = match &res {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e.clone())),
            };
            let _ = app.emit(
                "drive-folder-unpublish-progress",
                serde_json::json!({
                    "folderId": folder_id_evt,
                    "stage": "file",
                    "total": total_files,
                    "completed": done,
                    "fileId": file_id,
                    "fileName": file_name,
                    "ok": ok,
                    "error": err_msg,
                }),
            );
            (file_id, file_name, res)
        }
    });
    let results = futures::future::join_all(stop_futures).await;
    let mut succeeded = 0usize;
    let mut failures: Vec<DriveFolderFileError> = Vec::new();
    for (file_id, file_name, res) in results {
        match res {
            Ok(_) => succeeded += 1,
            Err(e) => failures.push(DriveFolderFileError {
                item_id: file_id,
                name: file_name,
                error: e,
            }),
        }
    }

    // Tear down the folder manifest: stop providing the folder hash and
    // purge the local KV record so we no longer republish it. The folder
    // hash lives in `merkle_root` (set by publish_drive_folder).
    let folder_hash = {
        let m = state.drive_state.manifest.read().await;
        m.items
            .iter()
            .find(|i| i.id == folder_id && i.owner == owner && i.item_type == "folder")
            .and_then(|i| i.merkle_root.clone())
            .filter(|h| !h.trim().is_empty())
    };
    if let Some(hash) = folder_hash.clone() {
        let dht = {
            let dht_guard = state.dht.lock().await;
            dht_guard.as_ref().cloned()
        };
        if let Some(dht) = dht {
            let _ = dht.remove_dht_record(folder_manifest_key(&hash)).await;
            let _ = dht.stop_providing_file(hash).await;
        }
    }

    let folder = {
        let mut m = state.drive_state.manifest.write().await;
        let item = m
            .items
            .iter_mut()
            .find(|i| i.id == folder_id && i.owner == owner && i.item_type == "folder")
            .ok_or("Drive folder not found in manifest")?;
        item.seed_enabled = false;
        item.seeding = false;
        item.payment_wallet = None;
        if folder_hash.is_some() {
            item.merkle_root = None;
        }
        item.modified_at = ds::now_secs();
        item.clone()
    };
    state.drive_state.persist().await;

    let _ = app.emit(
        "drive-folder-unpublish-progress",
        serde_json::json!({
            "folderId": folder_id,
            "stage": "done",
            "total": total_files,
            "completed": total_files,
            "succeeded": succeeded,
            "failed": failures.len(),
        }),
    );

    Ok(DriveFolderSeedResult {
        folder,
        files_total: files.len(),
        files_succeeded: succeeded,
        files_failed: failures.len(),
        failures,
    })
}

// ---------------------------------------------------------------------------
// Folder bundle search — buyer pastes a folder hash, we resolve the
// manifest + per-file seeders and report which seeders provide *every* file
// in the bundle (so the buyer's choice of seeder works for the whole
// folder, not just some files).
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderSearchFile {
    rel_path: String,
    file_hash: String,
    file_size: u64,
    /// All seeders for this file (subset of `common_seeders` shows in the
    /// UI for "everyone covered" rows).
    seeders: Vec<SeederInfo>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderSearchResult {
    hash: String,
    name: String,
    owner_wallet: String,
    created_at: u64,
    files: Vec<FolderSearchFile>,
    /// Folder-level sale price in wei (u128 as decimal string). Empty/"0"
    /// means the folder is free. Buyers pay this *once* to
    /// `payment_wallet` and unlock every file in the bundle.
    price_wei: String,
    /// Wallet that receives the folder-level payment. Falls back to
    /// `owner_wallet` when the manifest didn't carry an explicit
    /// payment_wallet (legacy / pre-folder-pricing manifests).
    payment_wallet: String,
    /// Peer IDs that appear in `seeders` for *every* file. These are the
    /// seeders the buyer should pick — anyone in this set will reliably
    /// serve every file in the folder.
    common_seeders: Vec<SeederInfo>,
}

#[tauri::command]
async fn search_folder(
    state: tauri::State<'_, AppState>,
    folder_hash: String,
) -> Result<Option<FolderSearchResult>, String> {
    if folder_hash.trim().is_empty() {
        return Ok(None);
    }
    let dht = match state.dht.lock().await.as_ref().cloned() {
        Some(d) => d,
        None => return Err("DHT not running".into()),
    };

    let key = folder_manifest_key(&folder_hash);
    let manifest_json = match dht.get_dht_value(key).await {
        Ok(Some(j)) => j,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e),
    };
    let manifest: FolderManifest = match serde_json::from_str(&manifest_json) {
        Ok(m) => m,
        Err(e) => return Err(format!("Invalid folder manifest: {}", e)),
    };
    // Trust contract: folder manifests are ECDSA-signed by `owner_wallet`.
    // Without verification, any peer could re-publish `chiral_folder_<H>`
    // with a forged `name` / `owner_wallet` / file list (FM-A20).
    if !manifest.verify() {
        let reason = if manifest.publisher_signature.is_empty() {
            "unsigned"
        } else {
            "INVALID signature"
        };
        println!(
            "⚠️ Folder manifest for {} {} — dropping",
            manifest.hash, reason
        );
        return Ok(None);
    }

    // For every file in the bundle, fetch its seeders in parallel.
    let fetches = manifest.files.iter().cloned().map(|f| {
        let dht = dht.clone();
        async move {
            let seeders = fetch_seeders(&dht, &f.file_hash).await.unwrap_or_default();
            FolderSearchFile {
                rel_path: f.rel_path,
                file_hash: f.file_hash,
                file_size: f.file_size,
                seeders,
            }
        }
    });
    let files: Vec<FolderSearchFile> = futures::future::join_all(fetches).await;

    // Common seeders = intersection of per-file seeder peer-ID sets, with
    // the price/wallet/multiaddrs from the FIRST file's record (any seeder
    // who provides every file should advertise consistent metadata across
    // them, but we keep the first-record values to give the UI something
    // concrete to render).
    let mut common: Vec<SeederInfo> = Vec::new();
    if let Some(first) = files.first() {
        for s in &first.seeders {
            let in_all = files
                .iter()
                .skip(1)
                .all(|f| f.seeders.iter().any(|x| x.peer_id == s.peer_id));
            if in_all {
                common.push(s.clone());
            }
        }
    }

    let payment_wallet = if manifest.wallet_address.trim().is_empty() {
        manifest.owner_wallet.clone()
    } else {
        manifest.wallet_address.clone()
    };
    Ok(Some(FolderSearchResult {
        hash: manifest.hash,
        name: manifest.name,
        owner_wallet: manifest.owner_wallet,
        created_at: manifest.created_at,
        files,
        price_wei: manifest.price_wei,
        payment_wallet,
        common_seeders: common,
    }))
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
    private_key: String,
) -> Result<(), String> {
    if owner_wallet.is_empty() || private_key.is_empty() {
        return Err("Wallet must be unlocked to publish a share to the relay".into());
    }
    let origin = state
        .hosting_server_addr
        .lock()
        .await
        .map(|a| format!("http://{}", a))
        .ok_or("Local server not running")?;

    let relay_base = relay_url.trim_end_matches('/');
    let url = format!("{}/api/drive/relay-register", relay_base);

    // Sign the (operation, token, owner_wallet, origin_url) tuple so
    // the relay can verify we own `owner_wallet` and bind this
    // signature to this exact registration (FM-A04/A05).
    let owner_lower = owner_wallet.to_lowercase();
    let payload = relay_share_proxy::register_payload("share", &share_token, &owner_lower, &origin);
    let signature = wallet::sign_message(&private_key, &payload)
        .map_err(|e| format!("Failed to sign register payload: {}", e))?;

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "token": share_token,
            "origin_url": origin,
            "owner_wallet": owner_lower,
            "signature": signature,
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
    owner_wallet: String,
    private_key: String,
) -> Result<(), String> {
    if owner_wallet.is_empty() || private_key.is_empty() {
        return Err("Wallet must be unlocked to unpublish a share".into());
    }
    let relay_base = relay_url.trim_end_matches('/');
    let path = format!("/api/drive/relay-register/{}", share_token);
    let url = format!("{}{}", relay_base, path);

    // Owner-proof: server checks `X-Owner-Sig` and verifies the
    // recovered wallet matches the share's stored `owner_wallet`.
    let owner_lower = owner_wallet.to_lowercase();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let proof_payload = auth::owner_proof_payload(&owner_lower, ts, "DELETE", &path);
    let signature = wallet::sign_message(&private_key, &proof_payload)
        .map_err(|e| format!("Failed to sign unregister proof: {}", e))?;

    let client = reqwest::Client::new();
    let resp = client
        .delete(&url)
        .header("X-Owner", &owner_lower)
        .header("X-Owner-Sig", format!("{}:{}", ts, signature))
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

// ---------------------------------------------------------------------------
// Version policy plumbing (Phase 1 + Phase 2)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionStatus {
    /// Compile-time version of this client.
    current_version: String,
    /// "ok" — at or above `recommended`.
    /// "recommended" — between `min_required` and `recommended` (soft nudge).
    /// "required" — strictly below `min_required` (hard block).
    status: String,
    /// The policy that produced this status — relay-fetched if we have one,
    /// otherwise the bundled compile-time snapshot.
    policy: version::VersionPolicy,
}

/// Sign a relay register-payload (FM-A04/A05). Frontend calls this
/// before POST /api/{drive,sites}/relay-register so the relay can
/// verify the registrant owns `owner_wallet`.
#[tauri::command]
fn compute_relay_register_signature(
    operation: String,
    id: String,
    owner_wallet: String,
    origin_url: String,
    private_key: String,
) -> Result<String, String> {
    if owner_wallet.is_empty() || private_key.is_empty() {
        return Err("owner_wallet and private_key required".to_string());
    }
    if operation != "share" && operation != "site" {
        return Err("operation must be 'share' or 'site'".to_string());
    }
    let payload =
        relay_share_proxy::register_payload(&operation, &id, &owner_wallet.to_lowercase(), &origin_url);
    wallet::sign_message(&private_key, &payload)
        .map_err(|e| format!("sign_message failed: {}", e))
}

/// Compute an owner-proof signature so the frontend can attach the
/// `X-Owner-Sig: <ts>:<sig>` header to HTTP requests against the
/// daemon's authenticated routes. Stateless — `ts` is taken from the
/// system clock; the resulting proof is good for ±5 min.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnerProof {
    timestamp: i64,
    signature: String,
    /// Convenience: the value the caller should put into `X-Owner-Sig`
    /// directly — `<ts>:<signature>`.
    header: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReputationVerdictProof {
    issuer_wallet: String,
    verifying_key: String,
    owner_signature: String,
    updated_at: u64,
    verdict_signature: String,
}

#[tauri::command]
fn compute_owner_proof(
    method: String,
    path: String,
    wallet_address: String,
    private_key: String,
) -> Result<OwnerProof, String> {
    if wallet_address.is_empty() || private_key.is_empty() {
        return Err("wallet_address and private_key required".to_string());
    }
    let owner = wallet_address.to_lowercase();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let payload = auth::owner_proof_payload(&owner, ts, &method, &path);
    let signature = wallet::sign_message(&private_key, &payload)
        .map_err(|e| format!("sign_message failed: {}", e))?;
    Ok(OwnerProof {
        timestamp: ts,
        header: format!("{}:{}", ts, signature),
        signature,
    })
}

fn verify_private_key_matches_wallet(
    private_key: &str,
    wallet_address: &str,
) -> Result<(), String> {
    let probe = b"chiral-reputation-wallet-check-v1";
    let signature = wallet::sign_message(private_key, probe)
        .map_err(|e| format!("sign_message failed: {}", e))?;
    let recovered = wallet::recover_signer(probe, &signature)
        .map_err(|e| format!("recover_signer failed: {}", e))?;
    if !recovered.eq_ignore_ascii_case(wallet_address) {
        return Err("private_key does not match wallet_address".to_string());
    }
    Ok(())
}

fn derive_reputation_issuer_key(private_key: &str) -> Result<ed25519_dalek::SigningKey, String> {
    use sha2::{Digest, Sha256};

    let key_hex = private_key.trim().trim_start_matches("0x");
    let key_bytes =
        hex::decode(key_hex).map_err(|e| format!("private_key is not valid hex: {}", e))?;
    if key_bytes.len() != 32 {
        return Err("private_key must be 32 bytes".to_string());
    }

    let mut hasher = Sha256::new();
    hasher.update(b"chiral-reputation-issuer-key-v1");
    hasher.update(&key_bytes);
    let digest = hasher.finalize();
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&digest);
    Ok(ed25519_dalek::SigningKey::from_bytes(&secret))
}

#[tauri::command]
fn compute_reputation_verdict_proof(
    transfer_id: String,
    seeder_wallet: String,
    downloader_wallet: String,
    file_hash: String,
    amount_wei: String,
    outcome: String,
    tx_hash: Option<String>,
    wallet_address: String,
    private_key: String,
) -> Result<ReputationVerdictProof, String> {
    use ed25519_dalek::Signer;

    if private_key.trim().is_empty() {
        return Err("private_key required".to_string());
    }

    let issuer_wallet = reputation::normalize_wallet(&wallet_address)?;
    let normalized_downloader = reputation::normalize_wallet(&downloader_wallet)?;
    if issuer_wallet != normalized_downloader {
        return Err("wallet_address must match downloader_wallet".to_string());
    }
    verify_private_key_matches_wallet(&private_key, &issuer_wallet)?;

    let amount_wei = if amount_wei.trim().is_empty() {
        "0".to_string()
    } else {
        amount_wei.trim().to_string()
    };
    amount_wei
        .parse::<u128>()
        .map_err(|_| "amountWei must be an integer wei string".to_string())?;

    let outcome = match outcome.trim().to_lowercase().as_str() {
        "completed" => "completed".to_string(),
        "failed" => "failed".to_string(),
        _ => return Err("outcome must be completed or failed".to_string()),
    };

    let tx_hash = tx_hash
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let signing_key = derive_reputation_issuer_key(&private_key)?;
    let verifying_key = hex::encode(signing_key.verifying_key().to_bytes());
    let owner_payload = reputation::issuer_key_binding_payload(&issuer_wallet, &verifying_key);
    let owner_signature = wallet::sign_message(&private_key, &owner_payload)
        .map_err(|e| format!("sign_message failed: {}", e))?;
    let verdict = reputation::ReputationVerdictPayload {
        transfer_id: transfer_id.trim().to_string(),
        seeder_wallet: seeder_wallet.trim().to_string(),
        downloader_wallet: issuer_wallet.clone(),
        file_hash: file_hash.trim().to_string(),
        amount_wei,
        outcome,
        tx_hash,
    };
    let verdict_signature = signing_key.sign(&reputation::verdict_signing_payload(&verdict));

    Ok(ReputationVerdictProof {
        issuer_wallet,
        verifying_key,
        owner_signature,
        updated_at: rating_storage::now_secs(),
        verdict_signature: hex::encode(verdict_signature.to_bytes()),
    })
}

/// Returns the active version policy (network-fetched if available, else
/// the bundled snapshot).
#[tauri::command]
fn get_version_policy() -> version::VersionPolicy {
    version::effective_policy()
}

/// Returns `{currentVersion, status, policy}` so the frontend can drive
/// the soft "update available" banner and the hard blocking modal off
/// one Tauri call.
#[tauri::command]
fn get_version_status() -> VersionStatus {
    let policy = version::effective_policy();
    let status = version::compare_to_policy(version::CURRENT_VERSION, &policy);
    VersionStatus {
        current_version: version::CURRENT_VERSION.to_string(),
        status: status.to_string(),
        policy,
    }
}

/// Phase 2 backend gate: refuse to enter ops protected by the version
/// floor (DHT start, paid downloads, …) if this client is below the
/// policy's `min_required`. The frontend's blocking modal already
/// prevents this in honest UIs; the backend gate stops anyone bypassing
/// it via direct Tauri invokes.
async fn ensure_version_supported(_state: &AppState) -> Result<(), String> {
    let policy = version::effective_policy();
    if version::compare_to_policy(version::CURRENT_VERSION, &policy) == "required" {
        return Err(format!(
            "This client (v{}) is below the network's required version (v{}). \
             Update from {} to continue.",
            version::CURRENT_VERSION,
            policy.min_required,
            policy.download_url
        ));
    }
    Ok(())
}

/// Startup probe: pull the relay's `/api/version-policy`, log it, and
/// store it in `AppState` if it's newer than what we already trust.
/// `issuedAt` is the rollback-protection axis — refuse to replace a
/// policy with one whose `issuedAt` is older than the stored one. The
/// bundled policy starts at `issuedAt: 0`, so the first relay-supplied
/// non-zero issuance always wins.
async fn fetch_and_log_remote_version_policy() {
    let url = "http://130.245.173.73:8080/api/version-policy";
    let client = match rpc_client::client() {
        Ok(client) => client,
        Err(e) => {
            println!(
                "[VERSION] Local build {} — shared HTTP client unavailable: {}",
                version::CURRENT_VERSION,
                e
            );
            return;
        }
    };
    let resp = match client
        .get(url)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            println!(
                "[VERSION] Local build {} — relay policy fetch failed: {}",
                version::CURRENT_VERSION,
                e
            );
            return;
        }
    };
    if !resp.status().is_success() {
        println!(
            "[VERSION] Local build {} — relay policy returned HTTP {}",
            version::CURRENT_VERSION,
            resp.status()
        );
        return;
    }
    let remote: version::VersionPolicy = match resp.json().await {
        Ok(p) => p,
        Err(e) => {
            println!(
                "[VERSION] Local build {} — could not parse relay policy: {}",
                version::CURRENT_VERSION,
                e
            );
            return;
        }
    };

    // Hand the candidate to the global slot; it applies the
    // signed-or-permissive + rollback rules and tells us whether it
    // accepted.
    let outcome = version::compare_to_policy(version::CURRENT_VERSION, &remote);
    let accepted = version::update_effective_policy(remote.clone());
    if accepted {
        println!(
            "[VERSION] Local build {} — relay says min={} recommended={} → {} (accepted)",
            version::CURRENT_VERSION,
            remote.min_required,
            remote.recommended,
            outcome
        );
    } else {
        println!(
            "[VERSION] Local build {} — relay policy rejected (signed?{} issuedAt={}, min={})",
            version::CURRENT_VERSION,
            !remote.signature.is_empty(),
            remote.issued_at,
            remote.min_required,
        );
    }
}

fn tauri_builder_startup_error(error: impl std::fmt::Display) -> String {
    format!(
        "Failed to build the Tauri application during startup: {}. \
         Check tauri.conf.json, plugin initialization, and bundled resources.",
        error
    )
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
                let (mut sigint, mut sigterm) = match install_shutdown_signal_handlers() {
                    Ok(signals) => signals,
                    Err(e) => {
                        eprintln!("⚠️  Shutdown signal cleanup disabled: {}", e);
                        return;
                    }
                };
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
                        let data_dir = network::data_dir().join("geth");
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

    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            dht: dht_arc,
            file_transfer: Arc::new(Mutex::new(FileTransferService::new())),
            file_storage: Arc::new(Mutex::new(HashMap::new())),
            geth,
            gpu_miner: Arc::new(Mutex::new(geth_gpu::GpuMiner::new())),
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

            // Log policy-key status on startup so operators see whether
            // signed policies are enabled (FM-A26).
            version::log_policy_key_status();
            // Background probe of the relay's /api/version-policy. The
            // result is funnelled into the global EFFECTIVE_POLICY slot
            // in version.rs, which backs get_version_status() (Tauri),
            // the libp2p Identify rejection in dht.rs, and the frontend
            // UpdateGate.
            tauri::async_runtime::spawn(fetch_and_log_remote_version_policy());
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
            // Network selection
            get_active_network,
            list_networks,
            set_active_network,
            // Geth commands
            is_geth_installed,
            download_geth,
            start_geth,
            stop_geth,
            reset_local_chain,
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
            // Version policy
            get_version_policy,
            get_version_status,
            compute_owner_proof,
            compute_reputation_verdict_proof,
            compute_relay_register_signature,
            get_mining_balance_diagnostic,
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
            publish_site_to_cdn,
            unpublish_site_from_cdn,
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
            publish_drive_folder,
            unpublish_drive_folder,
            search_folder,
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
        .build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("{}", tauri_builder_startup_error(error));
            return;
        }
    };

    app.run(move |_app, event| {
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
                    let data_dir = network::data_dir().join("geth");
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
    fn download_request_id_at_preserves_existing_id_shape() {
        let request_id = download_request_id_at(
            "download",
            "abcdef1234567890",
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(1234),
        )
        .expect("post-epoch timestamp should be valid");

        assert_eq!(request_id, "download-abcdef12-1234");
    }

    #[test]
    fn download_request_id_at_handles_short_hashes() {
        let request_id = download_request_id_at(
            "local",
            "abc",
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(7),
        )
        .expect("post-epoch timestamp should be valid");

        assert_eq!(request_id, "local-abc-7");
    }

    #[test]
    fn download_request_id_at_changes_with_timestamp() {
        let first = download_request_id_at(
            "download",
            "abcdef1234567890",
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(1),
        )
        .expect("post-epoch timestamp should be valid");
        let second = download_request_id_at(
            "download",
            "abcdef1234567890",
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(2),
        )
        .expect("post-epoch timestamp should be valid");

        assert_ne!(first, second);
    }

    #[test]
    fn download_request_id_at_rejects_pre_epoch_clock() {
        let err = download_request_id_at(
            "download",
            "abcdef1234567890",
            std::time::UNIX_EPOCH - std::time::Duration::from_secs(1),
        )
        .expect_err("pre-epoch timestamp should be rejected");

        assert!(err.contains("system clock is before UNIX_EPOCH"));
    }

    #[test]
    fn torrent_creation_date_entry_at_preserves_bencode_output() {
        let entry = torrent_creation_date_entry_at(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        )
        .expect("post-epoch timestamp should be valid");

        assert_eq!(entry, "13:creation datei1700000000e");
    }

    #[test]
    fn torrent_creation_timestamp_rejects_pre_epoch_clock() {
        let err = torrent_creation_date_entry_at(
            std::time::UNIX_EPOCH - std::time::Duration::from_secs(1),
        )
        .expect_err("pre-epoch timestamp should be rejected");

        assert!(err.contains("system clock is before UNIX_EPOCH"));
    }

    #[test]
    fn tauri_builder_startup_error_is_actionable() {
        let error = tauri_builder_startup_error("missing resource");

        assert!(error.contains("Tauri application"));
        assert!(error.contains("startup"));
        assert!(error.contains("missing resource"));
        assert!(error.contains("tauri.conf.json"));
        assert!(error.contains("plugin initialization"));
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_signal_pair_accepts_registered_handlers() {
        let pair = shutdown_signal_pair_from_results(Ok("sigint"), Ok("sigterm"))
            .expect("both signal handlers should be accepted");

        assert_eq!(pair, ("sigint", "sigterm"));
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_signal_pair_reports_sigint_failure() {
        let err = shutdown_signal_pair_from_results::<&str>(
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "interrupt unavailable",
            )),
            Ok("sigterm"),
        )
        .expect_err("SIGINT setup failure should be reported");

        assert!(err.contains("SIGINT"));
        assert!(err.contains("interrupt unavailable"));
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_signal_pair_reports_sigterm_failure() {
        let err = shutdown_signal_pair_from_results::<&str>(
            Ok("sigint"),
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "terminate unavailable",
            )),
        )
        .expect_err("SIGTERM setup failure should be reported");

        assert!(err.contains("SIGTERM"));
        assert!(err.contains("terminate unavailable"));
    }

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

    // In the provider-records model, FileMetadata is the immutable
    // publisher-signed header for a file. Seeder identity lives in Kademlia
    // provider records + `chiral_seeder_{hash}_{peerId}` entries, so these
    // tests cover only the header round-trip and the seeder-key schema.

    #[test]
    fn file_metadata_immutable_header_roundtrip() {
        let metadata = FileMetadata {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            protocol: "WebRTC".to_string(),
            created_at: 1700000000,
            wallet_address: "0xpublisher".to_string(),
            publisher_signature: String::new(),
        };
        let json = serde_json::to_string(&metadata).unwrap();
        let restored: FileMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.hash, "abc123");
        assert_eq!(restored.file_name, "test.txt");
        assert_eq!(restored.file_size, 1024);
        assert_eq!(restored.wallet_address, "0xpublisher");
    }

    #[test]
    fn file_metadata_ignores_stale_seeder_fields_from_old_clients() {
        // Records written by clients on the legacy schema carry extra
        // fields (peerId, priceWei, seeders). New clients should ignore
        // them silently — provider records are the source of truth.
        let legacy_json = r#"{
            "hash": "abc123",
            "fileName": "old_file.txt",
            "fileSize": 512,
            "protocol": "WebRTC",
            "createdAt": 1700000000,
            "peerId": "12D3KooWLegacy",
            "priceWei": "1000",
            "walletAddress": "0xlegacy",
            "seeders": [{"peerId": "12D3KooWLegacy"}],
            "publisherSignature": ""
        }"#;
        let metadata: FileMetadata = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(metadata.hash, "abc123");
        assert_eq!(metadata.wallet_address, "0xlegacy");
    }

    #[test]
    fn seeder_entry_key_namespace_is_unique_per_peer() {
        let key_a = seeder_entry_key("file1", "peerA");
        let key_b = seeder_entry_key("file1", "peerB");
        let key_c = seeder_entry_key("file2", "peerA");
        assert_ne!(key_a, key_b);
        assert_ne!(key_a, key_c);
        assert!(key_a.starts_with("chiral_seeder_"));
    }

    #[test]
    fn search_result_serialization_camel_case() {
        let result = SearchResult {
            hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            seeders: vec![SeederInfo {
                peer_id: "PeerA".to_string(),
                price_wei: "0".to_string(),
                wallet_address: String::new(),
                multiaddrs: vec![],
                signature: String::new(),
            }],
            created_at: 1700000000,
            price_wei: "0".to_string(),
            wallet_address: String::new(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["seeders"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["seeders"][0]["peerId"], "PeerA");
        assert_eq!(parsed["fileName"], "test.txt");
    }

    #[test]
    fn drive_reseed_price_wei_preserves_valid_paid_price() {
        assert_eq!(
            drive_reseed_price_wei(Some("0.25")).expect("valid Drive price should parse"),
            wallet::parse_chi_to_wei("0.25").unwrap()
        );
    }

    #[test]
    fn drive_reseed_price_wei_keeps_free_values_free() {
        assert_eq!(drive_reseed_price_wei(None).unwrap(), 0);
        assert_eq!(drive_reseed_price_wei(Some("")).unwrap(), 0);
        assert_eq!(drive_reseed_price_wei(Some(" 0 ")).unwrap(), 0);
    }

    #[test]
    fn drive_reseed_price_wei_rejects_malformed_price() {
        let err = drive_reseed_price_wei(Some("not-a-price"))
            .expect_err("malformed Drive price should fail closed");

        assert!(err.contains("invalid Drive item price"));
        assert!(err.contains("refusing to auto-reseed as free"));
    }

    #[test]
    fn drive_item_price_update_accepts_valid_price() {
        assert_eq!(
            validate_drive_item_price_update("0.25".to_string())
                .expect("valid Drive item price should be accepted"),
            Some("0.25".to_string())
        );
    }

    #[test]
    fn drive_item_price_update_clears_empty_price() {
        assert_eq!(validate_drive_item_price_update(String::new()).unwrap(), None);
    }

    #[test]
    fn drive_item_price_update_rejects_malformed_price() {
        let err = validate_drive_item_price_update("not-a-price".to_string())
            .expect_err("malformed Drive item price should be rejected");

        assert!(err.contains("Invalid Drive item price"));
        assert!(err.contains("Invalid amount"));
    }

    #[test]
    fn shared_file_credentials_allow_free_republish_without_wallet() {
        assert_eq!(
            shared_file_credentials_for_price(0, None, None).unwrap(),
            (String::new(), String::new())
        );
    }

    #[test]
    fn shared_file_credentials_allow_paid_republish_with_wallet_and_key() {
        assert_eq!(
            shared_file_credentials_for_price(
                1,
                Some(" 0xwallet ".to_string()),
                Some(" private-key ".to_string()),
            )
            .unwrap(),
            ("0xwallet".to_string(), "private-key".to_string())
        );
    }

    #[test]
    fn shared_file_credentials_reject_paid_republish_without_wallet() {
        let err = shared_file_credentials_for_price(1, None, Some("private-key".to_string()))
            .expect_err("paid republish should require a wallet address");

        assert!(err.contains("Wallet address is required"));
    }

    #[test]
    fn shared_file_credentials_reject_paid_republish_without_private_key() {
        let err = shared_file_credentials_for_price(1, Some("0xwallet".to_string()), None)
            .expect_err("paid republish should require a private key");

        assert!(err.contains("Private key is required"));
    }

    /// Manifests signed before folder-level pricing existed must still
    /// verify after the v2 payload was introduced — otherwise every
    /// pre-pricing folder bundle goes dark on search_folder.
    ///
    /// We construct a manifest, sign it under the v1 (legacy) payload,
    /// then verify with the v2-aware verify(). Acceptance is required.
    #[test]
    fn folder_manifest_verify_accepts_legacy_v1_signatures() {
        // Use a fixed secp256k1 private key. Recover the matching
        // wallet address by signing a probe message and asking
        // recover_signer who signed it — avoids depending on a
        // dedicated address_from_private_key helper.
        let private_key = "0x4c0883a69102937d6231471b5dbb6204fe512961708279cea2c89f1f7a0f2c4f";
        let probe_sig = wallet::sign_message(private_key, b"probe").unwrap();
        let wallet = wallet::recover_signer(b"probe", &probe_sig).expect("recover address");

        let mut m = FolderManifest {
            hash: "abcdef0123456789".to_string(),
            name: "legacy folder".to_string(),
            owner_wallet: wallet.clone(),
            created_at: 1_700_000_000,
            files: vec![FolderManifestFile {
                rel_path: "x.bin".to_string(),
                file_hash: "ff".repeat(32),
                file_size: 4096,
            }],
            // Legacy manifest: no folder-level pricing.
            price_wei: String::new(),
            wallet_address: String::new(),
            publisher_signature: String::new(),
        };
        // Sign over the v1 payload (what pre-pricing publishers wrote).
        let legacy_payload = m.sign_payload_legacy_v1();
        m.publisher_signature = wallet::sign_message(private_key, &legacy_payload).unwrap();

        // Sanity: the v2 payload should NOT match the legacy signature
        // (they're different byte strings) — proves we're actually
        // exercising the fallback path rather than accidentally getting
        // an identical encoding.
        assert!(!wallet::verify_signature(
            &m.sign_payload(),
            &m.publisher_signature,
            &m.owner_wallet,
        ));

        // verify() must accept the legacy signature.
        assert!(m.verify(), "legacy v1 manifest must verify under v2 verifier");

        // And tampering with the price fields after the fact must be
        // rejected — the v1 fallback is gated on price_wei +
        // wallet_address being empty, so a buyer can't pay 0 to a
        // hostile recipient.
        m.price_wei = "1000000000000000000".to_string();
        m.wallet_address = "0xdeadbeef".repeat(5).chars().take(42).collect();
        assert!(!m.verify(), "legacy fallback must not accept tacked-on pricing");
    }

    fn mff(rel_path: &str, file_hash: &str, file_size: u64) -> FolderManifestFile {
        FolderManifestFile {
            rel_path: rel_path.to_string(),
            file_hash: file_hash.to_string(),
            file_size,
        }
    }

    fn test_drive_item(
        id: &str,
        item_type: &str,
        parent_id: Option<&str>,
        owner: &str,
    ) -> ds::DriveItem {
        ds::DriveItem {
            id: id.to_string(),
            name: id.to_string(),
            item_type: item_type.to_string(),
            parent_id: parent_id.map(str::to_string),
            size: Some(1),
            mime_type: None,
            created_at: 1,
            modified_at: 1,
            starred: false,
            storage_path: Some(format!("{}.bin", id)),
            owner: owner.to_string(),
            is_public: true,
            merkle_root: None,
            protocol: None,
            price_chi: None,
            payment_wallet: None,
            seed_enabled: false,
            seeding: false,
        }
    }

    #[test]
    fn paid_folder_policy_rehydrates_from_manifest_for_reseed() {
        let owner = "0xowner";
        let payment_wallet = "0xpayment";
        let mut folder = test_drive_item("folder", "folder", None, owner);
        folder.storage_path = None;
        folder.size = None;
        folder.merkle_root = Some("folder-hash".to_string());
        folder.price_chi = Some("0.25".to_string());
        folder.payment_wallet = Some(payment_wallet.to_string());
        folder.seed_enabled = true;

        let mut child = test_drive_item("child", "file", Some("folder"), owner);
        child.merkle_root = Some("child-hash".to_string());
        child.seed_enabled = true;

        let manifest = ds::DriveManifest {
            items: vec![folder, child.clone()],
            shares: Vec::new(),
        };
        let policies = paid_folder_policies_for_drive_item(&manifest, &child);

        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].folder_hash, "folder-hash");
        assert_eq!(
            policies[0].price_wei,
            wallet::parse_chi_to_wei("0.25").unwrap()
        );
        assert_eq!(policies[0].wallet_address, payment_wallet);
    }

    #[test]
    fn folder_manifest_signing_preflight_requires_owner_key() {
        let private_key = "0x4c0883a69102937d6231471b5dbb6204fe512961708279cea2c89f1f7a0f2c4f";
        let probe_sig = wallet::sign_message(private_key, b"probe").unwrap();
        let owner_wallet = wallet::recover_signer(b"probe", &probe_sig).unwrap();

        assert!(signing_key_matches_wallet(private_key, &owner_wallet));
        assert!(!signing_key_matches_wallet("", &owner_wallet));
        assert!(!signing_key_matches_wallet(
            private_key,
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        ));
    }

    /// Folder hash is stable: same owner + same file set produces the
    /// same hash regardless of the order in which files were enumerated
    /// upstream. Critical for republish stability — if the hash drifted
    /// when filesystem walk order changed, every republish would create
    /// a new folder hash and break buyers' bookmarks.
    #[test]
    fn compute_folder_hash_is_order_independent() {
        let owner = "0xABCDEF1234567890";
        let a = vec![
            mff("a.bin", "11".repeat(32).as_str(), 100),
            mff("dir/b.bin", "22".repeat(32).as_str(), 200),
            mff("c.bin", "33".repeat(32).as_str(), 300),
        ];
        let mut b = a.clone();
        b.reverse();
        let mut c = a.clone();
        c.swap(0, 1);
        assert_eq!(compute_folder_hash(owner, &a), compute_folder_hash(owner, &b));
        assert_eq!(compute_folder_hash(owner, &a), compute_folder_hash(owner, &c));
    }

    #[test]
    fn compute_folder_hash_owner_is_lowercased() {
        let files = vec![mff("x.bin", "ff".repeat(32).as_str(), 1)];
        let upper = compute_folder_hash("0xABCDEF1234567890", &files);
        let lower = compute_folder_hash("0xabcdef1234567890", &files);
        assert_eq!(upper, lower, "owner casing must not change the folder hash");
    }

    #[test]
    fn compute_folder_hash_changes_on_owner() {
        let files = vec![mff("x.bin", "ff".repeat(32).as_str(), 1)];
        let h1 = compute_folder_hash("0x0000000000000000000000000000000000000001", &files);
        let h2 = compute_folder_hash("0x0000000000000000000000000000000000000002", &files);
        assert_ne!(h1, h2);
    }

    #[test]
    fn compute_folder_hash_changes_on_file_set() {
        let owner = "0x0000000000000000000000000000000000000001";
        let f1 = vec![mff("x.bin", "ff".repeat(32).as_str(), 1)];
        let f2 = vec![
            mff("x.bin", "ff".repeat(32).as_str(), 1),
            mff("y.bin", "ee".repeat(32).as_str(), 1),
        ];
        let f3 = vec![mff("x.bin", "fe".repeat(32).as_str(), 1)]; // different content hash
        assert_ne!(compute_folder_hash(owner, &f1), compute_folder_hash(owner, &f2));
        assert_ne!(compute_folder_hash(owner, &f1), compute_folder_hash(owner, &f3));
    }

    /// rel_path is hashed verbatim (with owner-lowercasing only). Two
    /// distinct casings of the same path are different folders. This is
    /// intentional: filesystems differ on case sensitivity, so we don't
    /// silently merge.
    #[test]
    fn compute_folder_hash_is_path_case_sensitive() {
        let owner = "0x0000000000000000000000000000000000000001";
        let a = vec![mff("Readme.md", "aa".repeat(32).as_str(), 1)];
        let b = vec![mff("readme.md", "aa".repeat(32).as_str(), 1)];
        assert_ne!(compute_folder_hash(owner, &a), compute_folder_hash(owner, &b));
    }

    /// file_size is NOT included in the folder hash (only rel_path +
    /// file_hash are). This is by design: file_size is a hint that the
    /// content_hash already binds, so a same-content file with a
    /// metadata-tracked size of 0 still produces the same folder hash.
    /// Locking it down so a future change doesn't accidentally include
    /// file_size and break stable republishes.
    #[test]
    fn compute_folder_hash_ignores_file_size() {
        let owner = "0x0000000000000000000000000000000000000001";
        let a = vec![mff("x.bin", "ff".repeat(32).as_str(), 100)];
        let b = vec![mff("x.bin", "ff".repeat(32).as_str(), 999_999)];
        assert_eq!(compute_folder_hash(owner, &a), compute_folder_hash(owner, &b));
    }

    /// Folder pricing v2 round-trip: sign, verify, mutate price, expect
    /// rejection. Complements the legacy-v1 acceptance test above by
    /// exercising the new payload's tamper detection.
    #[test]
    fn folder_manifest_v2_pricing_is_signed() {
        let private_key = "0x4c0883a69102937d6231471b5dbb6204fe512961708279cea2c89f1f7a0f2c4f";
        let probe_sig = wallet::sign_message(private_key, b"probe").unwrap();
        let wallet = wallet::recover_signer(b"probe", &probe_sig).unwrap();

        let mut m = FolderManifest {
            hash: "feedface".to_string(),
            name: "priced folder".to_string(),
            owner_wallet: wallet.clone(),
            created_at: 1_700_000_000,
            files: vec![mff("a.bin", "ff".repeat(32).as_str(), 4096)],
            price_wei: "1000000000000000000".to_string(), // 1 CHI
            wallet_address: wallet.clone(),
            publisher_signature: String::new(),
        };
        m.sign(private_key);
        assert!(m.verify(), "freshly v2-signed manifest must verify");

        // Tamper price → reject.
        let saved = m.price_wei.clone();
        m.price_wei = "1".to_string();
        assert!(!m.verify(), "modified price_wei must invalidate signature");
        m.price_wei = saved;
        assert!(m.verify());

        // Tamper recipient → reject.
        m.wallet_address = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string();
        assert!(!m.verify(), "modified wallet_address must invalidate signature");
    }
}
