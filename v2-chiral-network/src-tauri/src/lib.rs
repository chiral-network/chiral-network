mod dht;

use dht::DhtService;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AppState {
    pub dht: Arc<Mutex<Option<Arc<DhtService>>>>,
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
    
    let dht = Arc::new(DhtService::new());
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            dht: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            start_dht,
            stop_dht,
            get_dht_peers,
            get_network_stats,
            get_peer_id,
            ping_peer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
