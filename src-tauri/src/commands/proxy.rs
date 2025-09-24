use crate::dht::DhtService; // Assuming DhtService is accessible here
use crate::AppState;
use tauri::{AppHandle, Manager, State};
use tauri::Emitter;
use tracing::info;

#[derive(Clone, serde::Serialize)]
pub struct ProxyNode {
    pub id: String,
    pub address: String,
    pub status: String,
    pub latency: u32,
    pub error: Option<String>,
}

fn normalize_to_multiaddr(input: &str) -> Result<String, String> {
    if input.trim().starts_with("/") {
        return Ok(input.to_string());
    }
    if let Some(stripped) = input.strip_prefix("ws://") {
        let (host, port) = stripped.split_once(':').ok_or("invalid ws addr")?;
        Ok(format!("/dns4/{host}/tcp/{port}/ws"))
    } else if let Some(stripped) = input.strip_prefix("tcp://") {
        let (host, port) = stripped.split_once(':').ok_or("invalid tcp addr")?;
        Ok(format!("/dns4/{host}/tcp/{port}"))
    } else if input.contains(':') {
        let (host, port) = input.split_once(':').ok_or("invalid host:port")?;
        if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
            Ok(format!("/ip4/{host}/tcp/{port}"))
        } else {
            Ok(format!("/dns4/{host}/tcp/{port}"))
        }
    } else {
        Err(format!("unsupported address format: {}", input))
    }
}

#[tauri::command]
pub(crate) async fn proxy_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    _token: String,
) -> Result<(), String> {
    info!("Connecting to proxy: {}", url);

    {
        let mut proxies = state.proxies.lock().await;
        if let Some(p) = proxies.iter_mut().find(|p| p.address == url) {
            p.status = "connecting".into();
            p.error = None;
            p.latency = 999;
            let _ = app.emit("proxy_status_update", p.clone());
        } else {
            let node = ProxyNode {
                id: url.clone(),
                address: url.clone(),
                status: "connecting".into(),
                latency: 999,
                error: None,
            };
            proxies.push(node.clone());
            let _ = app.emit("proxy_status_update", node);
        }
    }

    if let Some(dht) = state.dht.lock().await.as_ref() {
        let multi = normalize_to_multiaddr(&url)?;
        dht.connect_peer(multi).await?;
        Ok(())
    } else {
        Err("DHT not initialized".into())
    }
}

#[tauri::command]
pub(crate) async fn proxy_disconnect(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    info!("Disconnecting from proxy: {}", url);
    let mut proxies = state.proxies.lock().await;
    if let Some(p) = proxies.iter_mut().find(|p| p.address == url || p.id == url) {
        p.status = "offline".into();
        let _ = app.emit("proxy_status_update", p.clone());
    }
    Ok(())
}

#[tauri::command]
pub async fn list_proxies(state: State<'_, AppState>) -> Result<Vec<ProxyNode>, String> {
    let proxies = state.proxies.lock().await;
    Ok(proxies.clone())
}

#[tauri::command]
pub async fn proxy_echo(
    state: State<'_, AppState>,
    peer_id: String,
    payload: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let dht_guard = state.dht.lock().await;
    let dht: &DhtService = dht_guard.as_ref().ok_or_else(|| "DHT not running".to_string())?;
    dht.echo(peer_id, payload).await
}
