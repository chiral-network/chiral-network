use crate::dht::DhtService; // Assuming DhtService is accessible here
use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri::Emitter;
use tokio::sync::Mutex;
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
    // Very simple mapper; improve as needed
    if let Some(stripped) = input.strip_prefix("ws://") {
        // ws://host:port -> /dns4/host/tcp/port/ws
        let (host, port) = stripped.split_once(':').ok_or("invalid ws addr")?;
        Ok(format!("/dns4/{host}/tcp/{port}/ws"))
    } else if let Some(stripped) = input.strip_prefix("tcp://") {
        // tcp://host:port -> /dns4/host/tcp/port
        let (host, port) = stripped.split_once(':').ok_or("invalid tcp addr")?;
        Ok(format!("/dns4/{host}/tcp/{port}"))
    } else if input.contains(':') {
        // host:port or 127.0.0.1:4001
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
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
    _token: String,
) -> Result<(), String> {
    info!("Connecting to proxy: {}", url);

    // 1) optimistic UI
    {
        let mut proxies = state.proxies.lock().await;
        if let Some(p) = proxies.iter_mut().find(|p| p.address == url) {
            p.status = "connecting".into();
            p.error = None;
            p.latency = 999;
            let _ = app.emit("proxy_status_update", p.clone());
        } else {
            // The ID should be the normalized multiaddr, but we don't have it yet.
            // We'll use the URL as a temporary ID and the event pump will fix it.
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

    // 2) dial via DHT
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
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    info!("Disconnecting from proxy: {}", url);
    // For proto v1: mark offline locally (no full teardown API yet)
    let mut proxies = state.proxies.lock().await;
    if let Some(p) = proxies.iter_mut().find(|p| p.address == url || p.id == url) {
        p.status = "offline".into();
        let _ = app.emit("proxy_status_update", p.clone());
    }
    // Note: This doesn't actually disconnect the peer in libp2p in this version.
    Ok(())
}

#[tauri::command]
pub async fn list_proxies(state: State<'_, AppState>) -> Result<Vec<ProxyNode>, String> {
    let proxies = state.proxies.lock().await;
    Ok(proxies.clone())
}
