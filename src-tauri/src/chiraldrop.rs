use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyUser {
    pub peer_id: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequest {
    pub request_id: String,
    pub from_peer_id: String,
    pub from_alias: String,
    pub file_name: String,
    pub file_size: u64,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChiralDropNotification {
    pub notification_type: String, // "transfer_request", "transfer_accepted", "transfer_declined"
    pub sender_peer_id: String,
    pub sender_alias: String,
    pub file_name: String,
    pub file_size: u64,
    pub price: f64,
    pub file_hash: Option<String>,
}

pub struct ChiralDropState {
    pub current_alias: Arc<Mutex<String>>,
    pub discovered_users: Arc<Mutex<Vec<NearbyUser>>>,
    pub pending_requests: Arc<Mutex<HashMap<String, TransferRequest>>>,
}

impl ChiralDropState {
    pub fn new() -> Self {
        Self {
            current_alias: Arc::new(Mutex::new(String::new())),
            discovered_users: Arc::new(Mutex::new(Vec::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Fun adjectives and nouns for generating random aliases
const ADJECTIVES: &[&str] = &[
    "Swift", "Bright", "Cosmic", "Digital", "Electric", "Frozen", "Golden", "Hidden",
    "Infinite", "Jovial", "Kinetic", "Lunar", "Mystic", "Neon", "Orbital", "Plasma",
    "Quantum", "Radiant", "Stellar", "Turbo", "Ultra", "Vivid", "Wild", "Xenon",
    "Yellow", "Zealous", "Azure", "Binary", "Crystal", "Dynamic",
];

const NOUNS: &[&str] = &[
    "Phoenix", "Dragon", "Tiger", "Wolf", "Eagle", "Falcon", "Hawk", "Lion",
    "Panther", "Raven", "Shark", "Viper", "Comet", "Nova", "Pulsar", "Quasar",
    "Nebula", "Galaxy", "Meteor", "Asteroid", "Proton", "Neutron", "Photon", "Electron",
    "Cipher", "Matrix", "Vector", "Tensor", "Nexus", "Vertex",
];

#[tauri::command]
pub async fn generate_alias(
    state: State<'_, ChiralDropState>,
    app_state: State<'_, AppState>,
) -> Result<String, String> {
    // Generate deterministic alias based on our own peer_id for consistency
    let dht_guard = app_state.dht.lock().await;
    
    let alias = if let Some(dht) = dht_guard.as_ref() {
        let local_peer_id = dht.get_peer_id().await;
        generate_alias_for_peer(&local_peer_id)
    } else {
        // Fallback to random if DHT not available
        let mut rng = rand::thread_rng();
        let adjective = ADJECTIVES[rng.gen_range(0..ADJECTIVES.len())];
        let noun = NOUNS[rng.gen_range(0..NOUNS.len())];
        let number = rng.gen_range(100..999);
        format!("{}{}{}", adjective, noun, number)
    };
    
    if let Ok(mut current) = state.current_alias.lock() {
        *current = alias.clone();
    }
    
    Ok(alias)
}

#[tauri::command]
pub async fn discover_nearby_users(
    state: State<'_, ChiralDropState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<NearbyUser>, String> {
    // Get real connected peers from DHT
    let dht_guard = app_state.dht.lock().await;
    
    if let Some(dht) = dht_guard.as_ref() {
        let peer_ids = dht.get_connected_peers().await;
        let local_peer_id = dht.get_peer_id().await;
        
        //Generate unique aliases for each peer, excluding self
        let mut users = Vec::new();
        
        for peer_id in &peer_ids {
            // Skip local peer
            if peer_id == &local_peer_id {
                continue;
            }
            
            // Generate a deterministic but unique alias based on peer_id
            let alias = generate_alias_for_peer(peer_id);
            users.push(NearbyUser {
                peer_id: peer_id.to_string(),
                alias,
            });
        }
        
        // Update discovered users cache
        if let Ok(mut discovered) = state.discovered_users.lock() {
            *discovered = users.clone();
        }
        
        Ok(users)
    } else {
        // DHT not running
        Ok(Vec::new())
    }
}

// Helper function to generate a consistent alias for a peer_id
fn generate_alias_for_peer(peer_id: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    peer_id.hash(&mut hasher);
    let hash = hasher.finish();
    
    let adjective_idx = (hash % ADJECTIVES.len() as u64) as usize;
    let noun_idx = ((hash / ADJECTIVES.len() as u64) % NOUNS.len() as u64) as usize;
    let number = (hash % 900) + 100; // 100-999
    
    format!("{}{}{}", ADJECTIVES[adjective_idx], NOUNS[noun_idx], number)
}

#[tauri::command]
pub async fn send_chiraldrop_file(
    recipient_peer_id: String,
    file_path: String,
    file_name: String,
    price: f64,
    state: State<'_, ChiralDropState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    use tokio::fs::File;
    use tokio::io::AsyncReadExt;
    
    // Calculate file hash
    let mut file = File::open(&file_path).await
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 8192];
    let mut file_size = 0u64;
    
    loop {
        let bytes_read = file.read(&mut buffer).await
            .map_err(|e| format!("Failed to read file: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        file_size += bytes_read as u64;
        hasher.update(&buffer[..bytes_read]);
    }
    
    let file_hash = format!("{:x}", hasher.finalize());
    
    // Get sender's alias
    let sender_alias = state.current_alias.lock().unwrap().clone();
    
    // Get sender's peer_id
    let dht_guard = app_state.dht.lock().await;
    let sender_peer_id = if let Some(dht) = dht_guard.as_ref() {
        dht.get_peer_id().await
    } else {
        return Err("DHT service not initialized".to_string());
    };
    drop(dht_guard);
    
    // Upload file to network
    let file_transfer_guard = app_state.file_transfer.lock().await;
    if let Some(file_transfer) = file_transfer_guard.as_ref() {
        let active_account = app_state.active_account.lock().await.clone();
        let active_private_key = app_state.active_account_private_key.lock().await.clone();
        
        file_transfer
            .upload_file_with_account(
                file_path.clone(),
                file_name.clone(),
                active_account,
                active_private_key,
            )
            .await?;
    } else {
        return Err("File transfer service not initialized".to_string());
    }
    drop(file_transfer_guard);
    
    // Create notification for recipient
    let notification = ChiralDropNotification {
        notification_type: "transfer_request".to_string(),
        sender_peer_id: sender_peer_id.clone(),
        sender_alias: sender_alias.clone(),
        file_name: file_name.clone(),
        file_size,
        price,
        file_hash: Some(file_hash.clone()),
    };
    
    // Send notification via DHT
    let notification_json = serde_json::to_vec(&notification)
        .map_err(|e| format!("Failed to serialize notification: {}", e))?;
    
    // Use base key for simple polling (will overwrite previous notification, which is acceptable)
    let notification_key = format!("chiraldrop:{}", recipient_peer_id);
    
    let dht_guard = app_state.dht.lock().await;
    if let Some(dht) = dht_guard.as_ref() {
        println!(
            "ChiralDrop: Sending notification for file '{}' (hash: {}) to peer {} with key '{}'",
            file_name, &file_hash[..8], &recipient_peer_id[..16], notification_key
        );
        
        match dht.put_dht_value(notification_key.clone(), notification_json).await {
            Ok(_) => println!("ChiralDrop: Notification successfully published to DHT"),
            Err(e) => {
                eprintln!("ChiralDrop: Failed to publish notification: {}", e);
                return Err(format!("Failed to send notification: {}", e));
            }
        }
    }
    
    println!(
        "ChiralDrop: File '{}' (hash: {}) uploaded and notification sent to {} (price: {} ETC)",
        file_name, &file_hash[..8], recipient_peer_id, price
    );
    
    Ok(())
}

#[tauri::command]
pub async fn check_chiraldrop_notifications(
    state: State<'_, ChiralDropState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<ChiralDropNotification>, String> {
    let dht_guard = app_state.dht.lock().await;
    
    if let Some(dht) = dht_guard.as_ref() {
        let local_peer_id = dht.get_peer_id().await;
        let notification_prefix = format!("chiraldrop:{}", local_peer_id);
        
        println!("ChiralDrop: Checking for notifications with prefix: {}", notification_prefix);
        
        // For now, check the base key (without file hash)
        // TODO: Implement prefix search to get all notifications for this peer
        let mut notifications = Vec::new();
        
        // Check base notification key pattern
        if let Some(notification_data) = dht.get_dht_value(notification_prefix.clone()).await? {
            println!("ChiralDrop: Found notification data ({} bytes)", notification_data.len());
            match serde_json::from_slice::<ChiralDropNotification>(&notification_data) {
                Ok(notification) => {
                    println!("ChiralDrop: Received transfer request for file '{}' from {}", 
                        notification.file_name, notification.sender_alias);
                    notifications.push(notification);
                },
                Err(e) => {
                    eprintln!("ChiralDrop: Failed to deserialize notification: {}", e);
                }
            }
        } else {
            // No notifications found - this is normal
        }
        
        Ok(notifications)
    } else {
        Err("DHT service not initialized".to_string())
    }
}

#[tauri::command]
pub async fn accept_chiraldrop_transfer(
    request_id: String,
    save_path: String,
    file_hash: String,
    state: State<'_, ChiralDropState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    // Get the file transfer service
    let file_transfer_guard = app_state.file_transfer.lock().await;
    
    if let Some(file_transfer) = file_transfer_guard.as_ref() {
        let active_account = app_state.active_account.lock().await.clone();
        let active_private_key = app_state.active_account_private_key.lock().await.clone();
        
        // Download the file
        file_transfer
            .download_file_with_account(
                file_hash.clone(),
                save_path.clone(),
                active_account,
                active_private_key,
            )
            .await?;
        
        // Remove from pending requests
        if let Ok(mut requests) = state.pending_requests.lock() {
            requests.remove(&request_id);
        }
        
        println!(
            "Accepted ChiralDrop transfer: downloading file {} to {}",
            &file_hash[..8], save_path
        );
        
        Ok(())
    } else {
        Err("File transfer service not initialized".to_string())
    }
}

#[tauri::command]
pub async fn decline_chiraldrop_transfer(
    request_id: String,
    state: State<'_, ChiralDropState>,
) -> Result<(), String> {
    if let Ok(mut requests) = state.pending_requests.lock() {
        if requests.remove(&request_id).is_some() {
            println!("Declined transfer request {}", request_id);
            return Ok(());
        }
    }
    
    Err("Transfer request not found".to_string())
}
