mod dht;
pub mod drive_api;
pub mod drive_storage;
pub mod rating_api;
pub mod rating_storage;
pub mod relay_share_proxy;
mod encryption;
mod file_transfer;
mod geth;
mod geth_bootstrap;
pub mod hosting;
pub mod hosting_server;
mod speed_tiers;

use dht::DhtService;
use encryption::EncryptionKeypair;
use file_transfer::FileTransferService;
use geth::{GethDownloader, GethProcess, GethStatus, MinedBlock, MiningStatus};
use geth_bootstrap::BootstrapHealthReport;
use speed_tiers::SpeedTier;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::Emitter;
use secp256k1::{Secp256k1, SecretKey, Message};
use tiny_keccak::{Hasher, Keccak};
use rlp::RlpStream;

pub struct AppState {
    pub dht: Arc<Mutex<Option<Arc<DhtService>>>>,
    pub file_transfer: Arc<Mutex<FileTransferService>>,
    pub file_storage: Arc<Mutex<HashMap<String, Vec<u8>>>>, // hash -> file data (for local caching)
    pub geth: Arc<Mutex<GethProcess>>,
    pub encryption_keypair: Arc<Mutex<Option<EncryptionKeypair>>>,
    pub download_tiers: Arc<Mutex<HashMap<String, SpeedTier>>>, // request_id -> speed tier
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

#[tauri::command]
async fn start_dht(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let mut dht_guard = state.dht.lock().await;

    if dht_guard.is_some() {
        return Err("DHT already running".to_string());
    }

    let dht = Arc::new(DhtService::new(state.file_transfer.clone(), state.download_tiers.clone(), state.download_directory.clone(), state.download_credentials.clone()));
    let result = dht.start(app.clone()).await?;
    *dht_guard = Some(dht);

    Ok(result)
}

#[tauri::command]
async fn stop_dht(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.take() {
        dht.stop().await?;
    }

    Ok(())
}

#[tauri::command]
async fn get_dht_peers(state: tauri::State<'_, AppState>) -> Result<Vec<dht::PeerInfo>, String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        Ok(dht.get_peers().await)
    } else {
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
        ).await
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
    let file_name = path.file_name()
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
        ).await
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
    file_transfer.accept_transfer(app, transfer_id, custom_dir).await
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

        let wallet_address = ad["walletAddress"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Store individual advertisement
        let host_key = format!("chiral_host_{}", peer_id);
        dht.put_dht_value(host_key, ad_json).await?;

        // Update registry (read-modify-write)
        let registry_key = "chiral_host_registry".to_string();
        let mut registry: Vec<HostRegistryEntry> = match dht.get_dht_value(registry_key.clone()).await {
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
async fn unpublish_host_advertisement(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        let peer_id = dht.get_peer_id().await.ok_or("Peer ID not available")?;

        // Remove from registry
        let registry_key = "chiral_host_registry".to_string();
        let mut registry: Vec<HostRegistryEntry> = match dht.get_dht_value(registry_key.clone()).await {
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
async fn get_host_registry(
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
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

#[tauri::command]
async fn get_hosting_agreement(
    state: tauri::State<'_, AppState>,
    agreement_id: String,
) -> Result<Option<String>, String> {
    // Try local disk first
    let path = agreements_dir()?.join(format!("{}.json", agreement_id));
    if path.exists() {
        let json = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read agreement: {e}"))?;
        return Ok(Some(json));
    }

    // Fall back to DHT
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        let key = format!("chiral_agreement_{}", agreement_id);
        let result = dht.get_dht_value(key).await?;
        // If found in DHT, cache locally
        if let Some(ref json) = result {
            let _ = std::fs::write(&path, json);
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
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("Failed to read agreements dir: {e}"))?;
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
            Ok(paths.iter().map(|p| p.to_string_lossy().to_string()).collect())
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
async fn get_download_directory(
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
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
        println!("⚠️ Custom download directory '{}' is invalid, falling back to system default", dir);
    }
    dirs::download_dir().ok_or_else(|| "Could not find downloads directory".to_string())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishResult {
    merkle_root: String,
}

/// File metadata stored in DHT
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FileMetadata {
    hash: String,
    file_name: String,
    file_size: u64,
    protocol: String,
    created_at: u64,
    peer_id: String,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    wallet_address: String,
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
    use sha2::{Sha256, Digest};
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
                parse_chi_to_wei(price)?
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
        ).await;

        // Create file metadata
        let metadata = FileMetadata {
            hash: merkle_root.clone(),
            file_name: file_name.clone(),
            file_size,
            protocol: protocol.unwrap_or_else(|| "WebRTC".to_string()),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            peer_id: peer_id.clone(),
            price_wei: price_wei_val.to_string(),
            wallet_address: wallet_addr,
        };

        // Serialize metadata to JSON
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        // Store metadata in DHT using the hash as key
        let dht_key = format!("chiral_file_{}", merkle_root);
        dht.put_dht_value(dht_key, metadata_json).await?;

        println!("File metadata published to DHT: {}", merkle_root);
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
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let merkle_root = hex::encode(hash);

    println!("Publishing file from data: {} with hash: {}", file_name, merkle_root);

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
                parse_chi_to_wei(price)?
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
        ).await;

        // Create file metadata
        let metadata = FileMetadata {
            hash: merkle_root.clone(),
            file_name: file_name.clone(),
            file_size,
            protocol: "WebRTC".to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            peer_id: peer_id.clone(),
            price_wei: price_wei_val.to_string(),
            wallet_address: wallet_addr,
        };

        // Serialize and store in DHT
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
        let dht_key = format!("chiral_file_{}", merkle_root);
        dht.put_dht_value(dht_key, metadata_json).await?;

        println!("File data published to DHT: {}", merkle_root);
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
    seeders: Vec<String>,
    created_at: u64,
    price_wei: String,
    wallet_address: String,
}

#[tauri::command]
async fn search_file(
    state: tauri::State<'_, AppState>,
    file_hash: String,
) -> Result<Option<SearchResult>, String> {
    println!("Searching for file: {}", file_hash);

    // Get DHT service
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        // Search for file metadata in DHT
        let dht_key = format!("chiral_file_{}", file_hash);
        println!("Looking up DHT key: {}", dht_key);

        match dht.get_dht_value(dht_key.clone()).await {
            Ok(Some(metadata_json)) => {
                // Parse metadata from JSON
                let metadata: FileMetadata = serde_json::from_str(&metadata_json)
                    .map_err(|e| format!("Failed to parse file metadata: {}", e))?;

                println!("Found file in DHT: {} ({})", metadata.file_name, metadata.hash);

                // Build result with seeder info
                let mut seeders = Vec::new();
                if !metadata.peer_id.is_empty() {
                    seeders.push(metadata.peer_id);
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
                Ok(None)
            }
            Err(e) => {
                println!("DHT lookup error: {}", e);
                Err(e)
            }
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

/// Burn address for speed tier payments (deflationary)
const BURN_ADDRESS: &str = "0x000000000000000000000000000000000000dEaD";

#[tauri::command]
async fn start_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    file_hash: String,
    file_name: String,
    seeders: Vec<String>,
    speed_tier: String,
    file_size: u64,
    wallet_address: Option<String>,
    private_key: Option<String>,
    seeder_price_wei: Option<String>,
    _seeder_wallet_address: Option<String>,
) -> Result<DownloadStartResult, String> {
    // Parse speed tier
    let tier = SpeedTier::from_str(&speed_tier)?;
    println!("⚡ Starting download: {} (hash: {}) from {} seeders [tier: {:?}]",
             file_name, file_hash, seeders.len(), tier);

    // Handle payment for paid tiers
    let cost_wei = speed_tiers::calculate_cost(&tier, file_size);
    if cost_wei > 0 {
        let wallet_addr = wallet_address.as_deref()
            .ok_or("Wallet address required for paid speed tier")?;
        let priv_key = private_key.as_deref()
            .ok_or("Private key required for paid speed tier")?;

        // Convert wei to CHI string for send_transaction
        let cost_chi = speed_tiers::format_wei_as_chi(cost_wei);
        println!("💰 Speed tier payment: {} CHI ({} wei) to burn address", cost_chi, cost_wei);

        // Process payment to burn address
        let payment_result = send_transaction(
            wallet_addr.to_string(),
            BURN_ADDRESS.to_string(),
            cost_chi.clone(),
            priv_key.to_string(),
        ).await;

        match payment_result {
            Ok(result) => {
                println!("✅ Speed tier payment successful: tx {}", result.hash);
                // Record transaction metadata for enriched history
                let meta = TransactionMeta {
                    tx_hash: result.hash.clone(),
                    tx_type: "speed_tier_payment".to_string(),
                    description: format!("⚡ {} tier download: {}", speed_tier, file_name),
                    file_name: Some(file_name.clone()),
                    file_hash: Some(file_hash.clone()),
                    speed_tier: Some(speed_tier.clone()),
                    recipient_label: Some("Burn Address (Speed Tier)".to_string()),
                    balance_before: Some(result.balance_before.clone()),
                    balance_after: Some(result.balance_after.clone()),
                };
                let mut metadata = state.tx_metadata.lock().await;
                metadata.insert(result.hash.clone(), meta);

                // Emit event so Download page can show balance change
                let _ = app.emit("speed-tier-payment-complete", serde_json::json!({
                    "txHash": result.hash,
                    "fileHash": file_hash,
                    "fileName": file_name,
                    "speedTier": speed_tier,
                    "balanceBefore": result.balance_before,
                    "balanceAfter": result.balance_after,
                }));
            }
            Err(e) => {
                println!("❌ Speed tier payment failed: {}", e);
                return Err(format!("Payment failed: {}. Download not started.", e));
            }
        }
    }

    // First, check if we have the file in local cache
    {
        let storage = state.file_storage.lock().await;
        if let Some(file_data) = storage.get(&file_hash) {
            println!("📁 File found in local cache");

            // Save to downloads folder (rate-limited even for cached files)
            let custom_dir = state.download_directory.lock().await.clone();
            let downloads_dir = get_effective_download_dir(&custom_dir)?;
            let file_path = downloads_dir.join(&file_name);
            let file_hash_prefix = &file_hash[..std::cmp::min(8, file_hash.len())];
            let request_id = format!("local-{}-{}",
                file_hash_prefix,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis());

            let file_data_clone = file_data.clone();
            let app_clone = app.clone();
            let tier_clone = tier.clone();
            let hash_clone = file_hash.clone();
            let name_clone = file_name.clone();
            let rid_clone = request_id.clone();

            // Spawn rate-limited write
            tokio::spawn(async move {
                match speed_tiers::rate_limited_write(
                    &app_clone, &file_path, &file_data_clone, &tier_clone,
                    &rid_clone, &hash_clone, &name_clone,
                ).await {
                    Ok(_) => {
                        println!("📁 File saved to: {:?}", file_path);
                        let _ = app_clone.emit("file-download-complete", serde_json::json!({
                            "requestId": rid_clone,
                            "fileHash": hash_clone,
                            "fileName": name_clone,
                            "filePath": file_path.to_string_lossy(),
                            "fileSize": file_data_clone.len(),
                            "status": "completed"
                        }));
                    }
                    Err(e) => {
                        println!("❌ Failed to save cached file: {}", e);
                        let _ = app_clone.emit("file-download-failed", serde_json::json!({
                            "requestId": rid_clone,
                            "fileHash": hash_clone,
                            "error": format!("Failed to save file: {}", e)
                        }));
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
    if seeders.is_empty() {
        return Err("No seeders available for this file".to_string());
    }

    // Get DHT service
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        // Generate a unique request ID
        let file_hash_prefix = &file_hash[..std::cmp::min(8, file_hash.len())];
        let request_id = format!("download-{}-{}", file_hash_prefix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis());

        // Store the speed tier for this download so dht.rs can use it during write
        {
            let mut tiers = state.download_tiers.lock().await;
            tiers.insert(request_id.clone(), tier);
        }

        // Store download credentials if wallet is available (needed for file payment in event loop)
        let seeder_price: u128 = seeder_price_wei.as_deref().unwrap_or("0").parse().unwrap_or(0);
        if seeder_price > 0 || wallet_address.is_some() {
            if let (Some(ref addr), Some(ref key)) = (&wallet_address, &private_key) {
                let mut creds = state.download_credentials.lock().await;
                creds.insert(request_id.clone(), dht::DownloadCredentials {
                    wallet_address: addr.clone(),
                    private_key: key.clone(),
                });
            }
        }

        // Emit download started event
        let _ = app.emit("download-started", serde_json::json!({
            "requestId": request_id,
            "fileHash": file_hash,
            "fileName": file_name,
            "seeders": seeders.len(),
            "speedTier": speed_tier
        }));

        // Check which seeders are actually reachable before attempting download
        let mut reachable_seeders = Vec::new();
        let mut offline_count = 0;

        for seeder in &seeders {
            match dht.is_peer_connected(seeder).await {
                Ok(true) => {
                    println!("✅ Seeder {} is connected", seeder);
                    reachable_seeders.push(seeder.clone());
                }
                Ok(false) => {
                    println!("⚠️ Seeder {} is not currently connected", seeder);
                    // Still include them — the swarm RequestFile handler will attempt to dial
                    reachable_seeders.push(seeder.clone());
                    offline_count += 1;
                }
                Err(e) => {
                    println!("❌ Failed to check seeder {} connectivity: {}", seeder, e);
                    offline_count += 1;
                    reachable_seeders.push(seeder.clone());
                }
            }
        }

        if offline_count == seeders.len() {
            println!("⚠️ All {} seeders appear to be offline, will attempt dial anyway", seeders.len());
        }

        // Try each seeder until one succeeds
        let mut last_error = String::new();
        let mut request_sent = false;

        for (i, seeder) in reachable_seeders.iter().enumerate() {
            println!("Trying seeder {}/{}: {} for file {}", i + 1, reachable_seeders.len(), seeder, file_hash);

            match dht.request_file(seeder.clone(), file_hash.clone(), request_id.clone()).await {
                Ok(_) => {
                    println!("✅ File request sent successfully to seeder {}", seeder);
                    request_sent = true;
                    break;
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
            // Clean up tier entry on failure
            {
                let mut tiers = state.download_tiers.lock().await;
                tiers.remove(&request_id);
            }
            let error_msg = if offline_count > 0 {
                format!("All {} seeder(s) are offline or unreachable. The file owner may have disconnected.", seeders.len())
            } else {
                format!("No seeder could provide the file: {}", last_error)
            };
            let _ = app.emit("file-download-failed", serde_json::json!({
                "requestId": request_id,
                "fileHash": file_hash,
                "error": error_msg
            }));
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

/// Calculate the cost of downloading a file at a given speed tier
#[tauri::command]
async fn calculate_download_cost(
    speed_tier: String,
    file_size: u64,
) -> Result<DownloadCostResult, String> {
    let tier = SpeedTier::from_str(&speed_tier)?;
    let cost_wei = speed_tiers::calculate_cost(&tier, file_size);
    let cost_chi = speed_tiers::format_wei_as_chi(cost_wei);
    let speed_label = match tier.bytes_per_second() {
        Some(bps) if bps < 1024 * 1024 => format!("{} KB/s", bps / 1024),
        Some(bps) => format!("{} MB/s", bps / (1024 * 1024)),
        None => "Unlimited".to_string(),
    };

    Ok(DownloadCostResult {
        cost_wei: cost_wei.to_string(),
        cost_chi,
        tier: speed_tier,
        speed_label,
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
    println!("Re-registering shared file: {} (hash: {})", file_name, file_hash);

    // Verify file still exists
    if !std::path::Path::new(&file_path).exists() {
        return Err(format!("File no longer exists: {}", file_path));
    }

    // Parse price from CHI to wei
    let price_wei = if let Some(ref price) = price_chi {
        if price.is_empty() || price == "0" {
            0u128
        } else {
            parse_chi_to_wei(price)?
        }
    } else {
        0u128
    };
    let wallet_addr = wallet_address.unwrap_or_default();

    // Get DHT service
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        dht.register_shared_file(file_hash, file_path, file_name, file_size, price_wei, wallet_addr).await;
        Ok(())
    } else {
        // DHT not running yet - this is okay, will be registered when DHT starts
        println!("DHT not running, file will be registered when DHT starts");
        Ok(())
    }
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
    let torrent_data = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read torrent file: {}", e))?;

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
            println!("Extracted file hash from torrent pieces field: {}", info_hash);
        }
    }

    // If we couldn't extract the hash from pieces, this might be a standard BitTorrent torrent
    // In that case, we can't use it with our network
    if info_hash.is_empty() {
        return Err("Invalid torrent file: could not find Chiral Network file hash in pieces field".to_string());
    }

    if name.is_empty() {
        // Extract name from filename if not in torrent
        name = std::path::Path::new(&file_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
    }

    println!("Parsed torrent: name={}, size={}, hash={}", name, size, info_hash);

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

// Wallet balance tracking - queries blockchain via Geth RPC
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WalletBalanceResult {
    balance: String,
    balance_wei: String,
}

// Transaction metadata for enriching blockchain data with local context
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransactionMeta {
    tx_hash: String,
    tx_type: String,         // "send", "receive", "speed_tier_payment", "faucet"
    description: String,     // Human-readable description
    file_name: Option<String>,    // For download payments
    file_hash: Option<String>,    // For download payments
    speed_tier: Option<String>,   // For speed tier payments
    recipient_label: Option<String>, // User-provided label for recipient
    balance_before: Option<String>,  // Balance before tx (CHI)
    balance_after: Option<String>,   // Balance after tx (CHI)
}

// Transaction types
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Transaction {
    hash: String,
    from: String,
    to: String,
    value: String,
    value_wei: String,
    block_number: u64,
    timestamp: u64,
    status: String,          // "confirmed", "pending", "failed"
    gas_used: u64,
    // Enriched metadata fields
    tx_type: String,         // "send", "receive", "speed_tier_payment", "unknown"
    description: String,     // Human-readable description
    file_name: Option<String>,
    file_hash: Option<String>,
    speed_tier: Option<String>,
    recipient_label: Option<String>,
    balance_before: Option<String>,
    balance_after: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendTransactionResult {
    hash: String,
    status: String,
    balance_before: String,
    balance_after: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionHistoryResult {
    transactions: Vec<Transaction>,
}

// Use the shared RPC endpoint from geth module
fn default_rpc_endpoint() -> String {
    crate::geth::rpc_endpoint()
}

#[tauri::command]
async fn get_wallet_balance(address: String) -> Result<WalletBalanceResult, String> {
    let rpc_endpoint = default_rpc_endpoint();
    println!("[get_wallet_balance] Querying balance for {} from RPC: {}", address, rpc_endpoint);

    // Query balance from blockchain via JSON-RPC
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "pending"],
        "id": 1
    });

    let response = client
        .post(&rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            println!("[get_wallet_balance] Failed to connect to {}: {}", rpc_endpoint, e);
            format!("Failed to connect to blockchain node at {}: {}", rpc_endpoint, e)
        })?;

    let status = response.status();
    println!("[get_wallet_balance] RPC response status: {}", status);

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse blockchain response: {}", e))?;

    // Check for RPC errors
    if let Some(error) = json_response.get("error") {
        println!("[get_wallet_balance] RPC error: {}", error);
        return Err(format!("Blockchain RPC error: {}", error));
    }

    let balance_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid balance response from blockchain")?;

    // Convert hex to decimal (wei) - handle "0x" prefix
    let hex_str = balance_hex.trim_start_matches("0x");
    let balance_wei = if hex_str.is_empty() {
        0u128
    } else {
        u128::from_str_radix(hex_str, 16)
            .map_err(|e| format!("Failed to parse balance hex '{}': {}", balance_hex, e))?
    };

    // Convert wei to CHI (1 CHI = 10^18 wei)
    let balance_chi = balance_wei as f64 / 1e18;

    println!("[get_wallet_balance] Balance for {}: {} CHI (hex: {}, wei: {})", address, balance_chi, balance_hex, balance_wei);

    Ok(WalletBalanceResult {
        balance: format!("{:.6}", balance_chi),
        balance_wei: balance_wei.to_string(),
    })
}

/// Keccak256 hash helper
fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut output);
    output
}

/// Parse hex string to u64
fn parse_hex_u64(hex: &str) -> u64 {
    let hex = hex.trim_start_matches("0x");
    u64::from_str_radix(hex, 16).unwrap_or(0)
}

#[cfg(test)]
mod chi_to_wei_tests {
    use super::parse_chi_to_wei;

    #[test]
    fn test_whole_number() {
        assert_eq!(parse_chi_to_wei("1").unwrap(), 1_000_000_000_000_000_000);
    }

    #[test]
    fn test_zero() {
        assert_eq!(parse_chi_to_wei("0").unwrap(), 0);
    }

    #[test]
    fn test_standard_tier_cost() {
        // 0.001 CHI = 10^15 wei
        assert_eq!(parse_chi_to_wei("0.001").unwrap(), 1_000_000_000_000_000);
    }

    #[test]
    fn test_premium_tier_cost() {
        // 0.005 CHI = 5 * 10^15 wei
        assert_eq!(parse_chi_to_wei("0.005").unwrap(), 5_000_000_000_000_000);
    }

    #[test]
    fn test_leading_dot() {
        // .5 CHI = 0.5 CHI = 5 * 10^17 wei
        assert_eq!(parse_chi_to_wei(".5").unwrap(), 500_000_000_000_000_000);
    }

    #[test]
    fn test_exact_18_decimals() {
        assert_eq!(
            parse_chi_to_wei("1.123456789012345678").unwrap(),
            1_123_456_789_012_345_678
        );
    }

    #[test]
    fn test_more_than_18_decimals_truncates() {
        // 19 digits after dot: truncated to 18
        let result = parse_chi_to_wei("1.1234567890123456789").unwrap();
        assert_eq!(result, 1_123_456_789_012_345_678);
    }

    #[test]
    fn test_large_whole_number() {
        assert_eq!(
            parse_chi_to_wei("100").unwrap(),
            100_000_000_000_000_000_000
        );
    }

    #[test]
    fn test_trims_whitespace() {
        assert_eq!(
            parse_chi_to_wei(" 1.5 ").unwrap(),
            1_500_000_000_000_000_000
        );
    }

    #[test]
    fn test_empty_string_is_zero() {
        // Empty string trims to "", which parses whole part as 0
        assert_eq!(parse_chi_to_wei("").unwrap(), 0);
    }

    #[test]
    fn test_non_numeric_errors() {
        assert!(parse_chi_to_wei("abc").is_err());
    }

    #[test]
    fn test_multiple_dots_errors() {
        assert!(parse_chi_to_wei("1.2.3").is_err());
    }

    #[test]
    fn test_smallest_wei_unit() {
        // 0.000000000000000001 CHI = 1 wei
        assert_eq!(
            parse_chi_to_wei("0.000000000000000001").unwrap(),
            1
        );
    }

    #[test]
    fn test_fractional_only() {
        assert_eq!(
            parse_chi_to_wei("0.5").unwrap(),
            500_000_000_000_000_000
        );
    }

    #[test]
    fn test_very_large_overflows() {
        // u128 max is ~3.4 * 10^38, so 10^21 CHI = 10^39 wei would overflow
        assert!(parse_chi_to_wei("1000000000000000000000").is_err());
    }
}

/// Convert CHI amount string to wei using string math (avoids f64 precision loss)
fn parse_chi_to_wei(amount: &str) -> Result<u128, String> {
    let amount = amount.trim();
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 {
        return Err("Invalid amount format".to_string());
    }

    let whole: u128 = if parts[0].is_empty() { 0 } else {
        parts[0].parse().map_err(|_| "Invalid amount".to_string())?
    };

    let frac_wei = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 18 {
            // Truncate to 18 decimal places
            frac_str[..18].parse::<u128>().map_err(|_| "Invalid amount".to_string())?
        } else {
            let padded = format!("{:0<18}", frac_str);
            padded.parse::<u128>().map_err(|_| "Invalid amount".to_string())?
        }
    } else {
        0u128
    };

    let wei = whole
        .checked_mul(1_000_000_000_000_000_000u128)
        .and_then(|w| w.checked_add(frac_wei))
        .ok_or("Amount overflow".to_string())?;

    Ok(wei)
}

/// Encode unsigned transaction for signing (EIP-155)
fn encode_unsigned_tx(
    nonce: u64,
    gas_price: u128,
    gas_limit: u64,
    to: &[u8],
    value: u128,
    data: &[u8],
    chain_id: u64,
) -> Vec<u8> {
    let mut stream = RlpStream::new_list(9);
    stream.append(&nonce);
    stream.append(&gas_price);
    stream.append(&gas_limit);
    stream.append(&to.to_vec());
    stream.append(&value);
    stream.append(&data.to_vec());
    stream.append(&chain_id);
    stream.append(&0u8); // empty for EIP-155
    stream.append(&0u8); // empty for EIP-155
    stream.out().to_vec()
}

/// Strip leading zero bytes from a byte slice (for RLP integer encoding)
fn strip_leading_zeros(bytes: &[u8]) -> &[u8] {
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[first_nonzero..]
}

/// Append raw big-endian bytes as an RLP integer (stripping leading zeros)
fn rlp_append_bytes_as_uint(stream: &mut RlpStream, bytes: &[u8]) {
    let stripped = strip_leading_zeros(bytes);
    if stripped.is_empty() {
        stream.append(&0u8);
    } else {
        stream.append(&stripped.to_vec());
    }
}

/// Encode signed transaction
fn encode_signed_tx(
    nonce: u64,
    gas_price: u128,
    gas_limit: u64,
    to: &[u8],
    value: u128,
    data: &[u8],
    v: u64,
    r: &[u8],
    s: &[u8],
) -> Vec<u8> {
    let mut stream = RlpStream::new_list(9);
    stream.append(&nonce);
    stream.append(&gas_price);
    stream.append(&gas_limit);
    stream.append(&to.to_vec());
    stream.append(&value);
    stream.append(&data.to_vec());
    stream.append(&v);
    // r and s must be encoded as integers (leading zeros stripped)
    rlp_append_bytes_as_uint(&mut stream, r);
    rlp_append_bytes_as_uint(&mut stream, s);
    stream.out().to_vec()
}

/// Send a transaction from one address to another (signs locally)
#[tauri::command]
async fn send_transaction(
    from_address: String,
    to_address: String,
    amount: String,
    private_key: String,
) -> Result<SendTransactionResult, String> {
    let client = reqwest::Client::new();

    // Parse private key
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex)
        .map_err(|e| format!("Invalid private key hex: {}", e))?;

    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(&pk_bytes)
        .map_err(|e| format!("Invalid private key: {}", e))?;

    // Convert amount from CHI to wei (1 CHI = 10^18 wei)
    // Use string-based conversion to avoid f64 precision loss
    let amount_wei = parse_chi_to_wei(&amount)?;

    // Get the nonce for the sender address
    // Use "pending" to account for transactions still in the mempool
    let nonce_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [&from_address, "pending"],
        "id": 1
    });

    let nonce_response = client
        .post(&default_rpc_endpoint())
        .json(&nonce_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get nonce: {}", e))?;

    let nonce_json: serde_json::Value = nonce_response.json().await
        .map_err(|e| format!("Failed to parse nonce response: {}", e))?;

    if let Some(error) = nonce_json.get("error") {
        return Err(format!("RPC error getting nonce: {}", error));
    }

    let nonce = parse_hex_u64(nonce_json["result"].as_str().unwrap_or("0x0"));

    // Get sender balance to verify they have enough
    // Use "pending" to account for in-flight transactions consuming funds
    let balance_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [&from_address, "pending"],
        "id": 1
    });

    let balance_response = client
        .post(&default_rpc_endpoint())
        .json(&balance_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get balance: {}", e))?;

    let balance_json: serde_json::Value = balance_response.json().await
        .map_err(|e| format!("Failed to parse balance response: {}", e))?;

    let balance_hex = balance_json["result"].as_str().unwrap_or("0x0");
    let balance_wei = u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16).unwrap_or(0);

    println!("💰 Sender balance: {} wei ({} CHI)", balance_wei, balance_wei as f64 / 1e18);

    // Get gas price
    let gas_price_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });

    let gas_price_response = client
        .post(&default_rpc_endpoint())
        .json(&gas_price_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get gas price: {}", e))?;

    let gas_price_json: serde_json::Value = gas_price_response.json().await
        .map_err(|e| format!("Failed to parse gas price response: {}", e))?;

    let gas_price = parse_hex_u64(gas_price_json["result"].as_str().unwrap_or("0x0"));
    // Use at least 1 gwei if gas price is 0
    let gas_price = if gas_price == 0 { 1_000_000_000 } else { gas_price };

    let gas_limit: u64 = 21000; // Standard transfer
    let chain_id: u64 = geth::CHAIN_ID;
    let gas_price_u128 = gas_price as u128;

    // Check total cost (amount + gas)
    let gas_cost = gas_price_u128 * gas_limit as u128;
    let total_cost = amount_wei.checked_add(gas_cost).ok_or("Amount overflow".to_string())?;

    // Capture balance before/after for transaction history
    let balance_before_chi = format!("{:.6}", balance_wei as f64 / 1e18);
    let balance_after_wei = balance_wei.saturating_sub(total_cost);
    let balance_after_chi = format!("{:.6}", balance_after_wei as f64 / 1e18);

    if balance_wei < total_cost {
        return Err(format!(
            "Insufficient balance: have {:.6} CHI, need {:.6} CHI (amount) + {:.6} CHI (gas)",
            balance_wei as f64 / 1e18,
            amount_wei as f64 / 1e18,
            gas_cost as f64 / 1e18
        ));
    }

    // Parse to address
    let to_bytes = hex::decode(to_address.trim_start_matches("0x"))
        .map_err(|e| format!("Invalid to address: {}", e))?;

    // RLP encode for signing (EIP-155)
    let unsigned_tx = encode_unsigned_tx(
        nonce,
        gas_price_u128,
        gas_limit,
        &to_bytes,
        amount_wei,
        &[], // empty data for simple transfer
        chain_id,
    );

    // Hash the unsigned transaction
    let tx_hash = keccak256(&unsigned_tx);

    // Sign the hash
    let message = Message::from_digest_slice(&tx_hash)
        .map_err(|e| format!("Failed to create message: {}", e))?;

    let (recovery_id, signature) = secp
        .sign_ecdsa_recoverable(&message, &secret_key)
        .serialize_compact();

    // Calculate v value (EIP-155)
    let v = chain_id * 2 + 35 + recovery_id.to_i32() as u64;

    // Extract r and s from signature
    let r = &signature[0..32];
    let s = &signature[32..64];

    // RLP encode signed transaction
    let signed_tx = encode_signed_tx(
        nonce,
        gas_price_u128,
        gas_limit,
        &to_bytes,
        amount_wei,
        &[], // data
        v,
        r,
        s,
    );

    let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

    println!("📤 Sending transaction:");
    println!("   From: {}", from_address);
    println!("   To: {}", to_address);
    println!("   Amount: {} CHI ({} wei)", amount, amount_wei);
    println!("   Nonce: {}", nonce);
    println!("   Gas Price: {}", gas_price);
    println!("   Chain ID: {}", chain_id);
    println!("   V: {}", v);
    println!("   Signed TX: {}...", &signed_tx_hex[..66.min(signed_tx_hex.len())]);

    // Send the raw transaction
    let send_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [signed_tx_hex],
        "id": 1
    });

    let send_response = client
        .post(&default_rpc_endpoint())
        .json(&send_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send transaction: {}", e))?;

    let send_json: serde_json::Value = send_response.json().await
        .map_err(|e| format!("Failed to parse send response: {}", e))?;

    println!("📥 RPC Response: {}", send_json);

    // Handle RPC errors with retry for transient conditions
    if let Some(error) = send_json.get("error") {
        let error_msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("");

        if error_msg == "already known" {
            // Transaction is already in the mempool — treat as success
            println!("⚠️ Transaction already in mempool, proceeding");
        } else if error_msg.contains("overdraft") {
            // Pending transactions consuming balance — wait for them to clear and retry
            println!("⚠️ Overdraft due to pending txs, waiting for confirmation...");
            for attempt in 1..=15 {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                println!("⏳ Retry attempt {}/15...", attempt);

                // Re-send the same signed transaction
                let retry_resp = client
                    .post(&default_rpc_endpoint())
                    .json(&send_payload)
                    .send()
                    .await
                    .map_err(|e| format!("Retry failed: {}", e))?;

                let retry_json: serde_json::Value = retry_resp.json().await
                    .map_err(|e| format!("Failed to parse retry response: {}", e))?;

                if retry_json.get("result").is_some() && !retry_json["result"].is_null() {
                    println!("✅ Transaction accepted on retry {}", attempt);
                    let tx_hash = retry_json["result"].as_str().unwrap().to_string();
                    println!("✅ Transaction submitted: {}", tx_hash);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    return Ok(SendTransactionResult {
                        hash: tx_hash,
                        status: "pending".to_string(),
                        balance_before: balance_before_chi.clone(),
                        balance_after: balance_after_chi.clone(),
                    });
                }

                let retry_msg = retry_json.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("");

                if retry_msg == "already known" {
                    println!("⚠️ Transaction now in mempool on retry {}", attempt);
                    break;
                }
                if !retry_msg.contains("overdraft") {
                    return Err(format!("Transaction failed on retry: {}", retry_json.get("error").unwrap()));
                }
            }
        } else {
            return Err(format!("Transaction failed: {}", error));
        }
    }

    // Compute tx hash from the signed transaction bytes
    // If RPC returned it, use that; otherwise compute from our signed tx
    let tx_hash = if let Some(hash) = send_json["result"].as_str() {
        hash.to_string()
    } else {
        // Compute hash from signed transaction for "already known" / overdraft-retry case
        let tx_bytes = hex::decode(signed_tx_hex.trim_start_matches("0x"))
            .map_err(|e| format!("Failed to decode signed tx: {}", e))?;
        format!("0x{}", hex::encode(keccak256(&tx_bytes)))
    };

    println!("✅ Transaction submitted: {}", tx_hash);

    // Wait a moment and check if transaction is pending
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Check transaction status
    let receipt_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [&tx_hash],
        "id": 1
    });

    let receipt_response = client
        .post(&default_rpc_endpoint())
        .json(&receipt_payload)
        .send()
        .await;

    if let Ok(resp) = receipt_response {
        if let Ok(receipt_json) = resp.json::<serde_json::Value>().await {
            if receipt_json["result"].is_null() {
                println!("⏳ Transaction pending (not yet mined). Make sure mining is running!");
            } else {
                let status = receipt_json["result"]["status"].as_str().unwrap_or("unknown");
                let block = receipt_json["result"]["blockNumber"].as_str().unwrap_or("unknown");
                println!("📦 Transaction mined in block {} with status {}", block, status);
            }
        }
    }

    Ok(SendTransactionResult {
        hash: tx_hash,
        status: "pending".to_string(),
        balance_before: balance_before_chi,
        balance_after: balance_after_chi,
    })
}

/// Result of a payment transaction, including balance snapshots.
pub struct PaymentResult {
    pub tx_hash: String,
    pub balance_before: String,
    pub balance_after: String,
}

/// Public wrapper for sending payment transactions from dht.rs event loop.
/// Takes CHI amount string, returns tx hash and balance info on success.
pub async fn send_payment_transaction(
    from_address: &str,
    to_address: &str,
    amount_chi: &str,
    private_key: &str,
) -> Result<PaymentResult, String> {
    let result = send_transaction(
        from_address.to_string(),
        to_address.to_string(),
        amount_chi.to_string(),
        private_key.to_string(),
    ).await?;
    Ok(PaymentResult {
        tx_hash: result.hash,
        balance_before: result.balance_before,
        balance_after: result.balance_after,
    })
}

/// Get a transaction receipt to check if it has been mined
#[tauri::command]
async fn get_transaction_receipt(tx_hash: String) -> Result<Option<serde_json::Value>, String> {
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [&tx_hash],
        "id": 1
    });

    let response = client
        .post(&default_rpc_endpoint())
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get receipt: {}", e))?;

    let json: serde_json::Value = response.json().await
        .map_err(|e| format!("Failed to parse receipt: {}", e))?;

    if let Some(error) = json.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let result = json.get("result").cloned();
    if result.as_ref().map_or(true, |v| v.is_null()) {
        Ok(None)
    } else {
        Ok(result)
    }
}

/// Dev faucet - gives 1 CHI to an address for testing
/// Only works if there's CHI in the faucet address
#[tauri::command]
async fn request_faucet(address: String) -> Result<SendTransactionResult, String> {
    let client = reqwest::Client::new();

    // Faucet address - this is a known test address with pre-allocated balance
    let faucet_address = "0x0000000000000000000000000000000000001337";

    // Amount: 1 CHI = 1e18 wei = 0xde0b6b3a7640000
    let amount_hex = "0xde0b6b3a7640000";

    // Get the nonce for the faucet address
    let nonce_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [faucet_address, "latest"],
        "id": 1
    });

    let nonce_response = client
        .post(&default_rpc_endpoint())
        .json(&nonce_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get faucet nonce: {}", e))?;

    let nonce_json: serde_json::Value = nonce_response.json().await
        .map_err(|e| format!("Failed to parse nonce response: {}", e))?;

    let nonce = nonce_json["result"].as_str().unwrap_or("0x0");

    // Build transaction object
    let tx = serde_json::json!({
        "from": faucet_address,
        "to": address,
        "value": amount_hex,
        "gas": "0x5208", // 21000 gas for simple transfer
        "gasPrice": "0x0",
        "nonce": nonce
    });

    // The faucet address is a special address that doesn't need unlocking
    // in development mode (it's pre-unlocked or uses a null key)
    // Try to unlock it first (password is empty for dev)
    let unlock_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "personal_unlockAccount",
        "params": [faucet_address, "", 60],
        "id": 1
    });

    let _ = client
        .post(&default_rpc_endpoint())
        .json(&unlock_payload)
        .send()
        .await;

    // Send the transaction
    let send_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendTransaction",
        "params": [tx],
        "id": 1
    });

    let send_response = client
        .post(&default_rpc_endpoint())
        .json(&send_payload)
        .send()
        .await
        .map_err(|e| format!("Faucet request failed: {}", e))?;

    let send_json: serde_json::Value = send_response.json().await
        .map_err(|e| format!("Failed to parse faucet response: {}", e))?;

    if let Some(error) = send_json.get("error") {
        // If faucet fails, suggest mining instead
        return Err(format!("Faucet unavailable. Please mine some blocks to get CHI. Error: {}", error));
    }

    let tx_hash = send_json["result"]
        .as_str()
        .ok_or("No transaction hash in faucet response")?
        .to_string();

    Ok(SendTransactionResult {
        hash: tx_hash,
        status: "pending".to_string(),
        balance_before: String::new(),
        balance_after: String::new(),
    })
}

/// Classify a transaction based on known addresses and local metadata
fn classify_transaction(
    tx_hash: &str,
    from: &str,
    to: &str,
    address: &str,
    metadata: &HashMap<String, TransactionMeta>,
) -> (String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) {
    let address_lower = address.to_lowercase();
    let to_lower = to.to_lowercase();
    let from_lower = from.to_lowercase();
    let burn_lower = BURN_ADDRESS.to_lowercase();

    // Check local metadata first (most accurate)
    if let Some(meta) = metadata.get(tx_hash) {
        return (
            meta.tx_type.clone(),
            meta.description.clone(),
            meta.file_name.clone(),
            meta.file_hash.clone(),
            meta.speed_tier.clone(),
            meta.recipient_label.clone(),
            meta.balance_before.clone(),
            meta.balance_after.clone(),
        );
    }

    // Auto-detect based on addresses
    if to_lower == burn_lower && from_lower == address_lower {
        // Payment to burn address — likely a speed tier payment
        return (
            "speed_tier_payment".to_string(),
            "⚡ Speed tier download payment".to_string(),
            None, None, None,
            Some("Burn Address (Speed Tier)".to_string()),
            None, None,
        );
    }

    if from_lower == address_lower && to_lower != burn_lower {
        return (
            "send".to_string(),
            format!("💸 Sent to {}", &to[..std::cmp::min(10, to.len())]),
            None, None, None, None, None, None,
        );
    }

    if to_lower == address_lower {
        return (
            "receive".to_string(),
            format!("📥 Received from {}", &from[..10]),
            None, None, None, None, None, None,
        );
    }

    ("unknown".to_string(), "Transaction".to_string(), None, None, None, None, None, None)
}

/// Get transaction history for an address.
/// Scans only a recent block window using JSON-RPC batch requests for efficiency.
#[tauri::command]
async fn get_transaction_history(
    state: tauri::State<'_, AppState>,
    address: String,
) -> Result<TransactionHistoryResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let rpc = default_rpc_endpoint();

    // Get the latest block number
    let block_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let block_response = client
        .post(&rpc)
        .json(&block_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get block number: {}", e))?;

    let block_json: serde_json::Value = block_response.json().await
        .map_err(|e| format!("Failed to parse block response: {}", e))?;

    let latest_block_hex = block_json["result"].as_str().unwrap_or("0x0");
    let latest_block = u64::from_str_radix(latest_block_hex.trim_start_matches("0x"), 16).unwrap_or(0);

    // Load local metadata for enrichment
    let metadata = state.tx_metadata.lock().await;

    let mut transactions = Vec::new();
    let address_lower = address.to_lowercase();

    // Scan only a recent block window from the chain tip.
    const MAX_BLOCKS_TO_SCAN: u64 = 20000;
    const BATCH_SIZE: u64 = 100;
    let first_block_to_scan = latest_block.saturating_sub(MAX_BLOCKS_TO_SCAN.saturating_sub(1));
    let mut cursor = latest_block;

    'outer: loop {
        let batch_start = cursor
            .saturating_sub(BATCH_SIZE - 1)
            .max(first_block_to_scan);
        // Build a JSON-RPC batch request
        let batch: Vec<serde_json::Value> = (batch_start..=cursor)
            .rev()
            .enumerate()
            .map(|(i, block_num)| {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "eth_getBlockByNumber",
                    "params": [format!("0x{:x}", block_num), true],
                    "id": i + 1
                })
            })
            .collect();

        let batch_response = client
            .post(&rpc)
            .json(&batch)
            .send()
            .await;

        if let Ok(response) = batch_response {
            if let Ok(results) = response.json::<Vec<serde_json::Value>>().await {
                for item in &results {
                    if let Some(result) = item.get("result") {
                        if let Some(txs) = result.get("transactions").and_then(|t| t.as_array()) {
                            if txs.is_empty() {
                                continue;
                            }
                            let block_timestamp = result.get("timestamp")
                                .and_then(|t| t.as_str())
                                .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0))
                                .unwrap_or(0);
                            let block_number_hex = result.get("number")
                                .and_then(|n| n.as_str())
                                .unwrap_or("0x0");
                            let block_num = u64::from_str_radix(block_number_hex.trim_start_matches("0x"), 16).unwrap_or(0);

                            for tx in txs {
                                let from = tx.get("from").and_then(|f| f.as_str()).unwrap_or("").to_lowercase();
                                let to = tx.get("to").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();

                                if from == address_lower || to == address_lower {
                                    let value_hex = tx.get("value").and_then(|v| v.as_str()).unwrap_or("0x0");
                                    let value_wei = u128::from_str_radix(value_hex.trim_start_matches("0x"), 16).unwrap_or(0);
                                    let value_chi = value_wei as f64 / 1e18;

                                    let gas_hex = tx.get("gas").and_then(|g| g.as_str()).unwrap_or("0x0");
                                    let gas_used = u64::from_str_radix(gas_hex.trim_start_matches("0x"), 16).unwrap_or(0);

                                    let tx_hash = tx.get("hash").and_then(|h| h.as_str()).unwrap_or("");
                                    let tx_from = tx.get("from").and_then(|f| f.as_str()).unwrap_or("");
                                    let tx_to = tx.get("to").and_then(|t| t.as_str()).unwrap_or("");

                                    let (tx_type, description, file_name, file_hash, speed_tier, recipient_label, balance_before, balance_after) =
                                        classify_transaction(tx_hash, tx_from, tx_to, &address, &metadata);

                                    transactions.push(Transaction {
                                        hash: tx_hash.to_string(),
                                        from: tx_from.to_string(),
                                        to: tx_to.to_string(),
                                        value: format!("{:.6}", value_chi),
                                        value_wei: value_wei.to_string(),
                                        block_number: block_num,
                                        timestamp: block_timestamp,
                                        status: "confirmed".to_string(),
                                        gas_used,
                                        tx_type,
                                        description,
                                        file_name,
                                        file_hash,
                                        speed_tier,
                                        recipient_label,
                                        balance_before,
                                        balance_after,
                                    });

                                    if transactions.len() >= 50 {
                                        break 'outer;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // If batch request fails, stop scanning
            break;
        }

        if batch_start <= first_block_to_scan {
            break;
        }
        cursor = batch_start - 1;
    }

    // Sort newest first
    transactions.sort_by(|a, b| b.block_number.cmp(&a.block_number));

    Ok(TransactionHistoryResult { transactions })
}

/// Record transaction metadata for enriching transaction history
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
        tx_hash: tx_hash.clone(),
        tx_type,
        description,
        file_name: None,
        file_hash: None,
        speed_tier: None,
        recipient_label,
        balance_before,
        balance_after,
    };
    let mut metadata = state.tx_metadata.lock().await;
    metadata.insert(tx_hash, meta);
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
    let downloads_dir = dirs::download_dir()
        .ok_or_else(|| "Could not find downloads directory".to_string())?;

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
    let hash_bytes = hex::decode(&file_hash)
        .map_err(|e| format!("Invalid hash: {}", e))?;
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
    let mut geth = state.geth.lock().await;
    geth.get_mining_status().await
}

#[tauri::command]
async fn get_mined_blocks(
    state: tauri::State<'_, AppState>,
    max_blocks: Option<u64>,
) -> Result<Vec<MinedBlock>, String> {
    let geth = state.geth.lock().await;
    geth.get_mined_blocks(max_blocks.unwrap_or(500)).await
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
    let start = if all_lines.len() > max_lines { all_lines.len() - max_lines } else { 0 };
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
    let pk_bytes = hex::decode(&wallet_private_key)
        .map_err(|e| format!("Invalid private key hex: {}", e))?;

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
    let keypair = keypair_guard.as_ref()
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
    let encrypted_bundle = encryption::encrypt_for_recipient_hex(&file_data, &recipient_public_key)?;

    // Serialize the encrypted bundle to JSON for transmission
    let encrypted_json = serde_json::to_vec(&encrypted_bundle)
        .map_err(|e| format!("Failed to serialize encrypted bundle: {}", e))?;

    // Send via DHT
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        // Prefix file name with .encrypted to indicate it's encrypted
        let encrypted_file_name = format!("{}.encrypted", file_name);
        let size = encrypted_json.len() as u64;
        dht.send_file(peer_id, transfer_id, encrypted_file_name, encrypted_json, String::new(), String::new(), String::new(), size).await
    } else {
        Err("DHT not running".to_string())
    }
}

/// Publish a peer's encryption public key to the DHT (for discovery)
#[tauri::command]
async fn publish_encryption_key(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let keypair_guard = state.encryption_keypair.lock().await;
    let keypair = keypair_guard.as_ref()
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
    let base = hosting::sites_base_dir()
        .ok_or_else(|| "Cannot determine data directory".to_string())?;
    let site_dir = base.join(&site_id);
    std::fs::create_dir_all(&site_dir).map_err(|e| format!("Failed to create site dir: {}", e))?;

    let mut site_files = Vec::new();

    for src_path_str in &file_paths {
        let src = std::path::Path::new(src_path_str);
        if !src.exists() {
            // Clean up on error
            let _ = std::fs::remove_dir_all(&site_dir);
            return Err(format!("File not found: {}", src_path_str));
        }
        let file_name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid file name: {}", src_path_str))?;

        let dest = site_dir.join(file_name);
        std::fs::copy(src, &dest)
            .map_err(|e| format!("Failed to copy {}: {}", file_name, e))?;

        let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        site_files.push(hosting::SiteFile {
            path: file_name.to_string(),
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

    let bound_addr = hosting_server::start_server(
        Arc::clone(&state.hosting_server_state),
        port,
        rx,
    )
    .await?;

    *state.hosting_server_addr.lock().await = Some(bound_addr);
    *state.hosting_server_shutdown.lock().await = Some(tx);

    Ok(bound_addr.to_string())
}

#[tauri::command]
async fn stop_hosting_server(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
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
                relay_base.replace("http://", "ws://").replace("https://", "wss://"),
                resource_type,
                resource_id
            );
            println!(
                "[TUNNEL] Connecting to {} for {}:{}",
                ws_url, resource_type, resource_id
            );

            match tokio_tungstenite::connect_async(&ws_url).await {
                Ok((ws_stream, _)) => {
                    println!(
                        "[TUNNEL] Connected for {}:{}",
                        resource_type, resource_id
                    );
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
                                let (status, headers, body_bytes): (u16, HashMap<String, String>, Vec<u8>) =
                                    match client.get(&target).send().await {
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
                                        Err(_) => {
                                            (502, HashMap::new(), b"Local server error".to_vec())
                                        }
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

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "site_id": site_id,
            "origin_url": origin,
            "owner_wallet": "",
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

    // Build the public URL
    let public_url = format!("{}/sites/{}/", relay_base, site_id);

    // Update local metadata with the relay URL
    if let Some(s) = all_sites.iter_mut().find(|s| s.id == site_id) {
        s.relay_url = Some(public_url.clone());
    }
    hosting::save_sites(&all_sites);

    // Start WebSocket tunnel to relay for NAT traversal
    // Use localhost for the tunnel's local fetch (origin may be 0.0.0.0 which isn't routable)
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

    let client = reqwest::Client::new();
    let resp = client
        .delete(&url)
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
// Drive CRUD commands (via Tauri invoke, bypasses browser HTTP restrictions)
// ---------------------------------------------------------------------------

use crate::drive_storage::{
    self as ds, DriveItem as DsItem, ShareLink as DsShareLink,
};

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
    };
    {
        let mut m = state.drive_state.manifest.write().await;
        m.items.push(item.clone());
    }
    state.drive_state.persist().await;
    Ok(item)
}

#[tauri::command]
async fn drive_upload_file(
    state: tauri::State<'_, AppState>,
    owner: String,
    file_path: String,
    parent_id: Option<String>,
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
    };
    {
        let mut m = state.drive_state.manifest.write().await;
        m.items.push(item.clone());
    }
    state.drive_state.persist().await;
    println!("[DRIVE] Uploaded file: {} ({} bytes)", file_name, data.len());
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
    let mut m = state.drive_state.manifest.write().await;
    if !m.items.iter().any(|i| i.id == item_id && i.owner == owner) {
        return Err("Item not found".into());
    }
    let to_delete: std::collections::HashSet<String> =
        ds::collect_descendants(&item_id, &m.items)
            .into_iter()
            .collect();
    if let Some(files_dir) = ds::drive_files_dir() {
        for id in &to_delete {
            if let Some(item) = m.items.iter().find(|i| &i.id == id) {
                if let Some(sp) = &item.storage_path {
                    let _ = std::fs::remove_file(files_dir.join(sp));
                }
            }
        }
    }
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
    password: Option<String>,
    is_public: Option<bool>,
) -> Result<serde_json::Value, String> {
    if owner.is_empty() {
        return Err("owner required".into());
    }
    let mut m = state.drive_state.manifest.write().await;
    if !m.items.iter().any(|i| i.id == item_id && i.owner == owner) {
        return Err("Item not found".into());
    }
    let token = ds::generate_share_token();
    let share = DsShareLink {
        id: token.clone(),
        item_id: item_id.clone(),
        created_at: ds::now_secs(),
        expires_at: None,
        password_hash: password.as_deref().map(ds::hash_password),
        is_public: is_public.unwrap_or(false),
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
        "hasPassword": share.password_hash.is_some(),
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
                "hasPassword": s.password_hash.is_some(),
                "createdAt": s.created_at,
                "downloadCount": s.download_count,
            })
        })
        .collect();
    Ok(shares)
}

// ---------------------------------------------------------------------------
// Drive server & relay commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_drive_server_url(
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
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
    let url = format!(
        "{}/api/drive/relay-register/{}",
        relay_base, share_token
    );

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

    println!(
        "[DRIVE] Unpublished share token={} from relay",
        share_token
    );
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let geth = Arc::new(Mutex::new(GethProcess::new()));
    let geth_for_signal = geth.clone();
    let geth_for_exit = geth.clone();

    // Spawn a background task to stop Geth on SIGINT (Ctrl+C) or SIGTERM
    // This prevents orphaned Geth processes when the app is killed
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
                        println!("🛑 SIGINT received — stopping Geth before exit");
                    }
                    _ = sigterm.recv() => {
                        println!("🛑 SIGTERM received — stopping Geth before exit");
                    }
                }
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
                                let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
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
            dht: Arc::new(Mutex::new(None)),
            file_transfer: Arc::new(Mutex::new(FileTransferService::new())),
            file_storage: Arc::new(Mutex::new(HashMap::new())),
            geth,
            encryption_keypair: Arc::new(Mutex::new(None)),
            download_tiers: Arc::new(Mutex::new(HashMap::new())),
            tx_metadata: Arc::new(Mutex::new(HashMap::new())),
            download_directory: Arc::new(Mutex::new(None)),
            download_credentials: Arc::new(Mutex::new(HashMap::new())),
            // Hosting & Drive
            hosting_server_state: Arc::new(hosting_server::HostingServerState::new()),
            hosting_server_addr: Arc::new(Mutex::new(None)),
            hosting_server_shutdown: Arc::new(Mutex::new(None)),
            drive_state: Arc::new(drive_api::DriveState::new()),
            tunnel_handles: Arc::new(Mutex::new(HashMap::new())),
        })
        .setup(|app| {
            use tauri::Manager;
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
            tauri::async_runtime::spawn(async move {
                hosting.load_from_disk().await;
                drive.load_from_disk_async().await;
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
            get_mined_blocks,
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
            drive_create_folder,
            drive_upload_file,
            drive_update_item,
            drive_delete_item,
            drive_create_share,
            drive_revoke_share,
            drive_list_shares,
            // Hosting marketplace commands
            publish_host_advertisement,
            unpublish_host_advertisement,
            get_host_registry,
            get_host_advertisement,
            store_hosting_agreement,
            get_hosting_agreement,
            list_hosting_agreements,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app, event| {
            if let tauri::RunEvent::Exit = event {
                // Stop Geth cleanly when the app exits (window close, quit, etc.)
                // Use try_lock to avoid deadlock — if the mutex is held by another
                // task (e.g. mining status poll), force-kill via PID file instead.
                println!("🛑 App exiting — stopping Geth and mining");
                match geth_for_exit.try_lock() {
                    Ok(mut geth) => {
                        let _ = geth.stop();
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
                                let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
                                std::thread::sleep(std::time::Duration::from_millis(500));
                                let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
                            }
                        }
                        let _ = std::fs::remove_file(&pid_path);
                    }
                }
            }
        });
}
