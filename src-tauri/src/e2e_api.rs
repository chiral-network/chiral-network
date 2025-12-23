use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

use sha2::Digest;
use tauri::Manager;

use crate::download_source::HttpSourceInfo;
use crate::http_download::HttpDownloadClient;
use crate::http_server;
use crate::transaction_services;

#[derive(Clone)]
pub struct E2eApiState {
    pub app: tauri::AppHandle,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    ok: bool,
    node_id: Option<String>,
    peer_id: Option<String>,
  file_server_url: Option<String>,
    rpc_endpoint: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PeersResponse {
    peers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadRequest {
    /// Size in MB for generated file
    size_mb: u64,
    /// Protocol string - for option1 we use HTTP for real network transfer
    protocol: Option<String>,
    price: Option<f64>,
    file_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadResponse {
    file_hash: String,
    file_name: String,
    file_size: u64,
    seeder_url: String,
    uploader_address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchRequest {
    file_hash: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadRequest {
    file_hash: String,
    /// Optional explicit seeder base URL (e.g., http://1.2.3.4:8080). If omitted, metadata.httpSources[0].url is used.
    seeder_url: Option<String>,
    /// Optional output file name; defaults to metadata.fileName.
    file_name: Option<String>,
    /// Optional protocol override. supported: HTTP, WebRTC, Bitswap
    protocol: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadResponse {
    download_path: String,
    verified: bool,
    bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayRequest {
    uploader_address: String,
    price: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PayResponse {
    tx_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptRequest {
    tx_hash: String,
}

pub async fn start_e2e_api_server(app: tauri::AppHandle, port: u16) -> Result<SocketAddr, String> {
    let state = E2eApiState { app };
    let router = create_router(state);

    let bind_addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| e.to_string())?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?;

    // No shutdown wiring for now; the process lifetime is the test lifetime.
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok(bound)
}

fn create_router(state: E2eApiState) -> Router {
    Router::new()
        .route("/api/health", get(api_health))
        .route("/api/dht/peers", get(api_dht_peers))
        .route("/api/upload", post(api_upload_generate))
        .route("/api/search", post(api_search))
        .route("/api/download", post(api_download))
        .route("/api/pay", post(api_pay))
        .route("/api/tx/receipt", post(api_tx_receipt))
        .with_state(Arc::new(state))
}

async fn api_health(State(state): State<Arc<E2eApiState>>) -> impl IntoResponse {
    // Best-effort info, avoid leaking secrets.
    let node_id = std::env::var("CHIRAL_NODE_ID").ok();
    let peer_id = {
        let app_state = state.app.state::<crate::AppState>();
        let dht = { app_state.dht.lock().await.as_ref().cloned() };
        match dht {
            Some(d) => Some(d.get_peer_id().await),
            None => None,
        }
    };
    let file_server_url = {
        // Prefer explicit public base URL (e.g., VM public IP + file server port).
        if let Ok(v) = std::env::var("CHIRAL_FILE_SERVER_URL") {
            if !v.trim().is_empty() {
                Some(v.trim().to_string())
            } else {
                None
            }
        } else {
            // Fall back to current bound HTTP file server port if available.
            let app_state = state.app.state::<crate::AppState>();
            let addr_opt = app_state.http_server_addr.lock().await.clone();
            addr_opt.map(|addr| {
                let host = std::env::var("CHIRAL_PUBLIC_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
                format!("http://{}:{}", host, addr.port())
            })
        }
    };
    let rpc_endpoint = std::env::var("CHIRAL_RPC_ENDPOINT").ok();

    (StatusCode::OK, Json(HealthResponse { ok: true, node_id, peer_id, file_server_url, rpc_endpoint }))
}

async fn api_dht_peers(State(state): State<Arc<E2eApiState>>) -> impl IntoResponse {
    let app_state = state.app.state::<crate::AppState>();
    let dht = { app_state.dht.lock().await.as_ref().cloned() };
    let peers = match dht {
        Some(d) => d.get_connected_peers().await,
        None => Vec::new(),
    };
    (StatusCode::OK, Json(PeersResponse { peers }))
}

async fn api_upload_generate(
    State(state): State<Arc<E2eApiState>>,
    Json(req): Json<UploadRequest>,
) -> impl IntoResponse {
    let protocol = req.protocol.unwrap_or_else(|| "HTTP".to_string());
    if protocol.to_uppercase() != "HTTP" {
        return (StatusCode::BAD_REQUEST, Json(crate::http_server::ErrorResponse {
            error: "Option1 currently supports protocol=HTTP only".to_string(),
        }))
        .into_response();
    }

    // Determine the seeder base URL (public IP if provided; otherwise localhost).
    let app_state = state.app.state::<crate::AppState>();
    let bound_addr = app_state.http_server_addr.lock().await.clone();
    let Some(bound_addr) = bound_addr else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
            error: "HTTP file server is not running (no bound address)".to_string(),
        }))
        .into_response();
    };
    let seeder_url = if let Ok(v) = std::env::var("CHIRAL_FILE_SERVER_URL") {
        if !v.trim().is_empty() {
            v.trim().to_string()
        } else {
            format!("http://127.0.0.1:{}", bound_addr.port())
        }
    } else {
        let host = std::env::var("CHIRAL_PUBLIC_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
        format!("http://{}:{}", host, bound_addr.port())
    };

    let file_name = req.file_name.unwrap_or_else(|| {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("e2e-{}.bin", ms)
    });
    let file_size = req.size_mb * 1024 * 1024;
    let price = req.price.unwrap_or(0.001);

    // Require active account for publishing/payment metadata consistency.
    let uploader_address = app_state.active_account.lock().await.clone();
    if uploader_address.is_none() {
        return (StatusCode::BAD_REQUEST, Json(crate::http_server::ErrorResponse {
            error: "No active account. Set CHIRAL_PRIVATE_KEY and restart node.".to_string(),
        }))
        .into_response();
    }

    // Create temp file, stream-write deterministic bytes and compute sha256.
    let tmp_dir = std::env::temp_dir().join("chiral-e2e");
    let _ = tokio::fs::create_dir_all(&tmp_dir).await;
    let tmp_path = tmp_dir.join(&file_name);

    let mut hasher = sha2::Sha256::new();
    let mut f = match tokio::fs::File::create(&tmp_path).await {
        Ok(f) => f,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
                error: format!("Failed to create temp file: {}", e),
            }))
            .into_response();
        }
    };
    let mut written: u64 = 0;
    let mut buf = vec![0u8; 64 * 1024];
    while written < file_size {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = ((written as usize + i) % 256) as u8;
        }
        let to_write = std::cmp::min(buf.len() as u64, file_size - written) as usize;
        if let Err(e) = f.write_all(&buf[..to_write]).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
                error: format!("Failed to write temp file: {}", e),
            }))
            .into_response();
        }
        hasher.update(&buf[..to_write]);
        written += to_write as u64;
    }
    let _ = f.flush().await;

    let file_hash = format!("{:x}", hasher.finalize());

    // Move into provider storage dir and register with HTTP file server state.
    let permanent_path = app_state.http_server_state.storage_dir.join(&file_hash);
    if let Err(e) = tokio::fs::rename(&tmp_path, &permanent_path).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
            error: format!("Failed to move file into storage: {}", e),
        }))
        .into_response();
    }

    app_state
        .http_server_state
        .register_file(http_server::HttpFileMetadata {
            hash: file_hash.clone(),      // merkle_root used as lookup key
            file_hash: file_hash.clone(), // storage filename (sha256)
            name: file_name.clone(),
            size: file_size,
            encrypted: false,
        })
        .await;

    // Publish metadata to DHT with HTTP source pointing at seeder base URL.
    let dht = { app_state.dht.lock().await.as_ref().cloned() };
    if let Some(dht) = dht {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let meta = crate::dht::models::FileMetadata {
            merkle_root: file_hash.clone(),
            file_name: file_name.clone(),
            file_size,
            file_data: vec![],
            seeders: vec![],
            created_at,
            mime_type: None,
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            parent_hash: None,
            cids: None,
            encrypted_key_bundle: None,
            ftp_sources: None,
            ed2k_sources: None,
            http_sources: Some(vec![HttpSourceInfo {
                url: seeder_url.clone(),
                auth_header: None,
                verify_ssl: true,
                headers: None,
                timeout_secs: None,
            }]),
            is_root: true,
            download_path: None,
            price,
            uploader_address: uploader_address.clone(),
            info_hash: None,
            trackers: None,
            manifest: None,
        };
        if let Err(e) = dht.publish_file(meta, None).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
                error: format!("Failed to publish metadata to DHT: {}", e),
            }))
            .into_response();
        }
    } else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
            error: "DHT is not running".to_string(),
        }))
        .into_response();
    }

    (
        StatusCode::OK,
        Json(UploadResponse {
            file_hash: file_hash.clone(),
            file_name,
            file_size,
            seeder_url,
            uploader_address,
        }),
    )
        .into_response()
}

async fn api_search(
    State(state): State<Arc<E2eApiState>>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    let app_state = state.app.state::<crate::AppState>();
    let dht = { app_state.dht.lock().await.as_ref().cloned() };
    let Some(dht) = dht else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
            error: "DHT is not running".to_string(),
        }))
        .into_response();
    };

    let timeout = req.timeout_ms.unwrap_or(10_000);
    match dht.synchronous_search_metadata(req.file_hash, timeout).await {
        Ok(m) => (StatusCode::OK, Json(m)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse { error: e })).into_response(),
    }
}

async fn api_download(
    State(state): State<Arc<E2eApiState>>,
    Json(req): Json<DownloadRequest>,
) -> impl IntoResponse {
    let app_state = state.app.state::<crate::AppState>();
    let dht = { app_state.dht.lock().await.as_ref().cloned() };
    let Some(dht) = dht else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
            error: "DHT is not running".to_string(),
        }))
        .into_response();
    };

    let meta_opt = match dht.synchronous_search_metadata(req.file_hash.clone(), 10_000).await {
        Ok(m) => m,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse { error: e })).into_response();
        }
    };
    let Some(meta) = meta_opt else {
        return (StatusCode::NOT_FOUND, Json(crate::http_server::ErrorResponse {
            error: "Metadata not found".to_string(),
        }))
        .into_response();
    };

    let out_name = req.file_name.unwrap_or_else(|| meta.file_name.clone());
    let output_path = app_state.http_server_state.storage_dir.join(&out_name);

    let protocol_upper = req
        .protocol
        .as_deref()
        .unwrap_or("HTTP")
        .trim()
        .to_uppercase();

    // Only HTTP downloads require an HTTP seeder URL / httpSources.
    let seeder_url = if protocol_upper == "HTTP" {
        let seeder_url = req
            .seeder_url
            .or_else(|| meta.http_sources.as_ref().and_then(|v| v.first()).map(|s| s.url.clone()))
            .ok_or_else(|| "No httpSources in metadata".to_string());
        match seeder_url {
            Ok(v) => Some(v),
            Err(e) => {
                return (StatusCode::BAD_REQUEST, Json(crate::http_server::ErrorResponse { error: e })).into_response();
            }
        }
    } else {
        None
    };

    if protocol_upper == "HTTP" {
        // Include downloader peer id for provider metrics if available.
        let peer_id = Some(dht.get_peer_id().await);
        let client = HttpDownloadClient::new_with_peer_id(peer_id);
        if let Err(e) = client
            .download_file(seeder_url.as_ref().unwrap(), &meta.merkle_root, &output_path, None)
            .await
        {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse { error: e })).into_response();
        }
    } else if protocol_upper == "WEBRTC" {
        let output_path_str = output_path.to_string_lossy().to_string();
        if let Err(e) =
            crate::download_file_from_network(app_state, meta.merkle_root.clone(), output_path_str)
                .await
        {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse { error: e })).into_response();
        }
    } else if protocol_upper == "BITSWAP" {
        let output_path_str = output_path.to_string_lossy().to_string();
        if let Err(e) = crate::download_blocks_from_network(app_state, meta.clone(), output_path_str).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse { error: e })).into_response();
        }
    } else {
        return (StatusCode::BAD_REQUEST, Json(crate::http_server::ErrorResponse {
            error: format!("Unsupported protocol '{}'. Use HTTP, WebRTC, or Bitswap.", protocol_upper),
        }))
        .into_response();
    }

    // Verify existence + size (protocol-independent light check).
    let bytes_len = match tokio::fs::metadata(&output_path).await {
        Ok(m) => m.len(),
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
                error: format!("Download finished but output file is missing: {}", e),
            }))
            .into_response();
        }
    };

    let verified = if protocol_upper == "HTTP" {
        // Option1: sha256(file) == merkle_root (used as fileHash).
        let bytes = match tokio::fs::read(&output_path).await {
            Ok(b) => b,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
                    error: format!("Failed to read downloaded file: {}", e),
                }))
                .into_response();
            }
        };
        let mut hasher = sha2::Sha256::new();
        hasher.update(&bytes);
        let computed = format!("{:x}", hasher.finalize());
        computed == meta.merkle_root
    } else {
        bytes_len == meta.file_size
    };

    (StatusCode::OK, Json(DownloadResponse {
        download_path: output_path.to_string_lossy().to_string(),
        verified,
        bytes: bytes_len,
    }))
    .into_response()
}

async fn api_pay(
    State(state): State<Arc<E2eApiState>>,
    Json(req): Json<PayRequest>,
) -> impl IntoResponse {
    let app_state = state.app.state::<crate::AppState>();
    // Reuse the same logic as the tauri command (no UI needed).
    let account = match app_state.active_account.lock().await.clone() {
        Some(a) => a,
        None => {
            return (StatusCode::BAD_REQUEST, Json(crate::http_server::ErrorResponse {
                error: "No active account. Set CHIRAL_PRIVATE_KEY and restart node.".to_string(),
            }))
            .into_response();
        }
    };
    let private_key = match app_state.active_account_private_key.lock().await.clone() {
        Some(k) => k,
        None => {
            return (StatusCode::BAD_REQUEST, Json(crate::http_server::ErrorResponse {
                error: "No private key loaded. Set CHIRAL_PRIVATE_KEY and restart node.".to_string(),
            }))
            .into_response();
        }
    };

    match crate::ethereum::send_transaction(&account, &req.uploader_address, req.price, &private_key).await {
        Ok(tx_hash) => (StatusCode::OK, Json(PayResponse { tx_hash })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse { error: e })).into_response(),
    }
}

async fn api_tx_receipt(
    State(_state): State<Arc<E2eApiState>>,
    Json(req): Json<ReceiptRequest>,
) -> impl IntoResponse {
    match transaction_services::get_transaction_receipt(&req.tx_hash).await {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse { error: e })).into_response(),
    }
}


