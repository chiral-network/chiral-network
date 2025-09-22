use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::info;

use crate::AppState;

#[derive(Clone, serde::Serialize)]
pub struct ProxyNode {
  id: String,
  address: String,
  status: String,
  latency: u32,
  error: Option<String>,
}

#[tauri::command]
pub async fn proxy_connect(app: AppHandle, state: State<'_, AppState>, url: String, token: String) -> Result<(), String> {
    info!("Connecting to proxy: {}", url);

    // 1) Initial state addition + event (lock is held briefly and released)
    {
        let mut proxies = state.proxies.lock().await;
        let node = ProxyNode {
            id: url.clone(),
            address: url.clone(),
            status: "connecting".to_string(),
            latency: 999,
            error: None,
        };
        proxies.push(node.clone());
        let _ = app.emit("proxy_status_update", node);
    } // <- At this point, MutexGuard drop

    // 2) Clone only the handles to be passed to spawn (never capture State<'_> reference)
    let app_for_emit = app.clone();
    let proxies_arc: Arc<Mutex<Vec<ProxyNode>>> = state.proxies.clone();
    let url_for_task = url.clone();

    // 3) Async task (Send + 'static)
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let mut proxies = proxies_arc.lock().await;
        if let Some(proxy) = proxies.iter_mut().find(|p| p.id == url_for_task) {
            proxy.status = "online".to_string();
            proxy.latency = 120; // dummy
            let _ = app_for_emit.emit("proxy_status_update", proxy.clone());
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn proxy_disconnect(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    info!("Disconnecting from proxy: {}", url);
    {
        let mut proxies = state.proxies.lock().await;
        if let Some(proxy) = proxies.iter_mut().find(|p| p.id == url) {
            proxy.status = "offline".to_string();
            let _ = app.emit("proxy_status_update", proxy.clone());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_proxies(state: State<'_, AppState>) -> Result<Vec<ProxyNode>, String> {
    let proxies = state.proxies.lock().await;
    Ok(proxies.clone())
}
