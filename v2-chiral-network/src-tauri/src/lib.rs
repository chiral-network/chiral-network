mod dht;
mod file_transfer;

use dht::DhtService;
use file_transfer::FileTransferService;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub struct AppState {
    pub dht: Arc<Mutex<Option<Arc<DhtService>>>>,
    pub file_transfer: Arc<Mutex<FileTransferService>>,
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
    
    let dht = Arc::new(DhtService::new(state.file_transfer.clone()));
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
) -> Result<(), String> {
    let dht_guard = state.dht.lock().await;

    if let Some(dht) = dht_guard.as_ref() {
        dht.send_file(peer_id, transfer_id, file_name, file_data).await
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
    let file_transfer = state.file_transfer.lock().await;
    file_transfer.accept_transfer(app, transfer_id).await
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
        // Default fallback for other platforms
        Ok(10000) // Return 10GB as default
    }
}

#[tauri::command]
async fn get_file_size(file_path: String) -> Result<u64, String> {
    let metadata = std::fs::metadata(&file_path).map_err(|e| e.to_string())?;
    Ok(metadata.len())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishResult {
    merkle_root: String,
}

#[tauri::command]
async fn publish_file(
    file_path: String,
    file_name: String,
) -> Result<PublishResult, String> {
    // Read file and compute hash
    let file_data = std::fs::read(&file_path).map_err(|e| e.to_string())?;

    // Compute SHA-256 hash
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let merkle_root = hex::encode(hash);

    // In a real implementation, we would publish to DHT here
    // For now, just return the hash
    println!("Publishing file: {} with hash: {}", file_name, merkle_root);

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
}

#[tauri::command]
async fn search_file(
    file_hash: String,
) -> Result<Option<SearchResult>, String> {
    // In a real implementation, we would search the DHT
    // For now, return None (file not found)
    println!("Searching for file: {}", file_hash);
    Ok(None)
}

#[tauri::command]
async fn start_download(
    file_hash: String,
    file_name: String,
    seeders: Vec<String>,
) -> Result<(), String> {
    // In a real implementation, we would start downloading from seeders
    println!("Starting download: {} from {} seeders", file_name, seeders.len());
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            dht: Arc::new(Mutex::new(None)),
            file_transfer: Arc::new(Mutex::new(FileTransferService::new())),
        })
        .invoke_handler(tauri::generate_handler![
            start_dht,
            stop_dht,
            get_dht_peers,
            get_network_stats,
            get_peer_id,
            ping_peer,
            send_file,
            accept_file_transfer,
            decline_file_transfer,
            store_dht_value,
            get_dht_value,
            get_available_storage,
            get_file_size,
            publish_file,
            search_file,
            start_download
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
