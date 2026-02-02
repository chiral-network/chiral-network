mod dht;
mod file_transfer;

use dht::DhtService;
use file_transfer::FileTransferService;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::Emitter;

pub struct AppState {
    pub dht: Arc<Mutex<Option<Arc<DhtService>>>>,
    pub file_transfer: Arc<Mutex<FileTransferService>>,
    pub file_storage: Arc<Mutex<HashMap<String, Vec<u8>>>>, // hash -> file data (for local caching)
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
}

#[tauri::command]
async fn publish_file(
    state: tauri::State<'_, AppState>,
    file_path: String,
    file_name: String,
    protocol: Option<String>,
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

        // Register the file for sharing (so we can serve it to requesters)
        dht.register_shared_file(
            merkle_root.clone(),
            file_path.clone(),
            file_name.clone(),
            file_size,
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
    state: tauri::State<'_, AppState>,
    file_hash: String,
) -> Result<Option<SearchResult>, String> {
    println!("Searching for file: {}", file_hash);

    // Get DHT service
    let dht_guard = state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        // Search for file metadata in DHT
        let dht_key = format!("chiral_file_{}", file_hash);
        match dht.get_dht_value(dht_key).await? {
            Some(metadata_json) => {
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
                }))
            }
            None => {
                println!("File not found in DHT: {}", file_hash);
                Ok(None)
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

#[tauri::command]
async fn start_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    file_hash: String,
    file_name: String,
    seeders: Vec<String>,
) -> Result<DownloadStartResult, String> {
    // Log the download initiation
    println!("Starting download: {} (hash: {}) from {} seeders", file_name, file_hash, seeders.len());

    // First, check if we have the file in local cache
    {
        let storage = state.file_storage.lock().await;
        if let Some(file_data) = storage.get(&file_hash) {
            println!("File found in local cache");

            // Save to downloads folder
            let downloads_dir = dirs::download_dir()
                .ok_or("Could not find downloads directory")?;
            let file_path = downloads_dir.join(&file_name);

            std::fs::write(&file_path, file_data)
                .map_err(|e| format!("Failed to write file: {}", e))?;

            println!("File downloaded to: {:?}", file_path);

            // Emit completion event
            let _ = app.emit("file-download-complete", serde_json::json!({
                "requestId": format!("local-{}", file_hash[..8].to_string()),
                "fileHash": file_hash,
                "fileName": file_name,
                "filePath": file_path.to_string_lossy().to_string(),
                "fileSize": file_data.len(),
                "status": "completed"
            }));

            return Ok(DownloadStartResult {
                request_id: format!("local-{}", file_hash[..8].to_string()),
                status: "completed".to_string(),
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
        let request_id = format!("download-{}-{}", &file_hash[..8],
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis());

        // Emit download started event
        let _ = app.emit("download-started", serde_json::json!({
            "requestId": request_id,
            "fileHash": file_hash,
            "fileName": file_name,
            "seeders": seeders.len()
        }));

        // Request file from the first available seeder
        // In a production system, we'd try multiple seeders in parallel or with fallback
        let seeder = &seeders[0];
        println!("Requesting file {} from seeder {}", file_hash, seeder);

        match dht.request_file(seeder.clone(), file_hash.clone(), request_id.clone()).await {
            Ok(_) => {
                println!("File request sent successfully");
                Ok(DownloadStartResult {
                    request_id,
                    status: "requesting".to_string(),
                })
            }
            Err(e) => {
                println!("Failed to request file: {}", e);
                let _ = app.emit("file-download-failed", serde_json::json!({
                    "requestId": request_id,
                    "fileHash": file_hash,
                    "error": e
                }));
                Err(format!("Failed to request file from seeder: {}", e))
            }
        }
    } else {
        Err("DHT not running".to_string())
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
    use sha2::{Sha256, Digest};

    // Read the torrent file
    let torrent_data = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read torrent file: {}", e))?;

    let mut name = String::new();
    let mut size: u64 = 0;

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

    // For multi-file torrents, we'd need to sum up all file lengths
    // This is a simplified implementation

    // Compute info hash - find the info dictionary and hash it
    // BitTorrent uses SHA-1, but we'll use SHA-256 for our purposes
    let info_hash = if let Some(info_start) = find_info_dict(&torrent_data) {
        // Find the end of the info dictionary
        if let Some(info_end) = find_dict_end(&torrent_data[info_start..]) {
            let info_bytes = &torrent_data[info_start..info_start + info_end];
            let mut hasher = Sha256::new();
            hasher.update(info_bytes);
            hex::encode(hasher.finalize())
        } else {
            // Fallback: hash entire file
            let mut hasher = Sha256::new();
            hasher.update(&torrent_data);
            hex::encode(hasher.finalize())
        }
    } else {
        // Fallback: hash entire file
        let mut hasher = Sha256::new();
        hasher.update(&torrent_data);
        hex::encode(hasher.finalize())
    };

    if name.is_empty() {
        // Extract name from filename if not in torrent
        name = std::path::Path::new(&file_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
    }

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

// Find the start of the info dictionary
fn find_info_dict(data: &[u8]) -> Option<usize> {
    // Look for "4:infod" pattern (info key followed by dictionary start)
    let pattern = b"4:infod";
    data.windows(pattern.len())
        .position(|w| w == pattern)
        .map(|p| p + 6) // Skip "4:info" to get to the 'd'
}

// Find the end of a dictionary starting at position 0
fn find_dict_end(data: &[u8]) -> Option<usize> {
    if data.first() != Some(&b'd') {
        return None;
    }

    let mut depth = 0;
    let mut i = 0;

    while i < data.len() {
        match data[i] {
            b'd' | b'l' => {
                depth += 1;
                i += 1;
            }
            b'e' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'i' => {
                // Integer: skip to 'e'
                while i < data.len() && data[i] != b'e' {
                    i += 1;
                }
                i += 1; // Skip 'e'
            }
            b'0'..=b'9' => {
                // String: find length, skip string
                let start = i;
                while i < data.len() && data[i] != b':' {
                    i += 1;
                }
                if let Ok(len_str) = std::str::from_utf8(&data[start..i]) {
                    if let Ok(len) = len_str.parse::<usize>() {
                        i += 1 + len; // Skip ':' and string content
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    None
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportTorrentResult {
    path: String,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            dht: Arc::new(Mutex::new(None)),
            file_transfer: Arc::new(Mutex::new(FileTransferService::new())),
            file_storage: Arc::new(Mutex::new(HashMap::new())),
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
            open_file_dialog,
            publish_file,
            search_file,
            start_download,
            parse_torrent_file,
            export_torrent_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
