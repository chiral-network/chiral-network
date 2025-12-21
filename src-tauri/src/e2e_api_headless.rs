use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use crate::download_source::HttpSourceInfo;
use crate::http_download::HttpDownloadClient;
use crate::http_server;
use crate::transaction_services;
use crate::{dht, ethereum};

#[derive(Clone)]
pub struct HeadlessE2eState {
    pub dht: Arc<crate::dht::DhtService>,
    pub http_server_state: Arc<http_server::HttpServerState>,
    pub http_base_url: String,
    pub storage_dir: PathBuf,
    pub uploader_address: Option<String>,
    pub private_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    ok: bool,
    peer_id: String,
    http_base_url: String,
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
    size_mb: u64,
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
    seeder_url: Option<String>,
    file_name: Option<String>,
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

pub async fn start_headless_e2e_api_server(
    state: HeadlessE2eState,
    port: u16,
) -> Result<(SocketAddr, oneshot::Sender<()>), String> {
    let router = create_router(state);

    let bind_addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| e.to_string())?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        let _ = server.await;
    });

    Ok((bound, shutdown_tx))
}

fn create_router(state: HeadlessE2eState) -> Router {
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

async fn api_health(State(state): State<Arc<HeadlessE2eState>>) -> impl IntoResponse {
    let peer_id = state.dht.get_peer_id().await;
    let rpc_endpoint = std::env::var("CHIRAL_RPC_ENDPOINT").ok();
    (
        StatusCode::OK,
        Json(HealthResponse {
            ok: true,
            peer_id,
            http_base_url: state.http_base_url.clone(),
            rpc_endpoint,
        }),
    )
}

async fn api_dht_peers(State(state): State<Arc<HeadlessE2eState>>) -> impl IntoResponse {
    let peers = state.dht.get_connected_peers().await;
    (StatusCode::OK, Json(PeersResponse { peers }))
}

async fn api_upload_generate(
    State(state): State<Arc<HeadlessE2eState>>,
    Json(req): Json<UploadRequest>,
) -> impl IntoResponse {
    let protocol = req.protocol.unwrap_or_else(|| "HTTP".to_string());
    if protocol.to_uppercase() != "HTTP" {
        return (
            StatusCode::BAD_REQUEST,
            Json(http_server::ErrorResponse {
                error: "Headless option1 supports protocol=HTTP only".to_string(),
            }),
        )
            .into_response();
    }

    if state.uploader_address.is_none() || state.private_key.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(http_server::ErrorResponse {
                error: "No account loaded. Set CHIRAL_PRIVATE_KEY and restart node.".to_string(),
            }),
        )
            .into_response();
    }

    let file_name = req.file_name.unwrap_or_else(|| {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("e2e-{}.bin", ms)
    });

    let file_size = req.size_mb * 1024 * 1024;
    let price = req.price.unwrap_or(0.001);
    let seeder_url = state.http_base_url.clone();

    // Create temp file, stream-write deterministic bytes and compute sha256.
    let tmp_dir = std::env::temp_dir().join("chiral-e2e");
    let _ = tokio::fs::create_dir_all(&tmp_dir).await;
    let tmp_path = tmp_dir.join(&file_name);

    let mut hasher = sha2::Sha256::new();
    let mut f = match tokio::fs::File::create(&tmp_path).await {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse {
                    error: format!("Failed to create temp file: {}", e),
                }),
            )
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
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse {
                    error: format!("Failed to write temp file: {}", e),
                }),
            )
                .into_response();
        }
        hasher.update(&buf[..to_write]);
        written += to_write as u64;
    }
    let _ = f.flush().await;

    let file_hash = format!("{:x}", hasher.finalize());

    // Move into provider storage dir and register with HTTP file server state.
    let permanent_path = state.http_server_state.storage_dir.join(&file_hash);
    if let Err(e) = tokio::fs::rename(&tmp_path, &permanent_path).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(http_server::ErrorResponse {
                error: format!("Failed to move file into storage: {}", e),
            }),
        )
            .into_response();
    }

    state
        .http_server_state
        .register_file(http_server::HttpFileMetadata {
            hash: file_hash.clone(),
            file_hash: file_hash.clone(),
            name: file_name.clone(),
            size: file_size,
            encrypted: false,
        })
        .await;

    // Publish metadata to DHT with HTTP source.
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let meta = dht::models::FileMetadata {
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
        uploader_address: state.uploader_address.clone(),
        info_hash: None,
        trackers: None,
        manifest: None,
    };

    if let Err(e) = state.dht.publish_file(meta, None).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(http_server::ErrorResponse {
                error: format!("Failed to publish metadata to DHT: {}", e),
            }),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(UploadResponse {
            file_hash,
            file_name,
            file_size,
            seeder_url,
            uploader_address: state.uploader_address.clone(),
        }),
    )
        .into_response()
}

async fn api_search(
    State(state): State<Arc<HeadlessE2eState>>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    let timeout = req.timeout_ms.unwrap_or(10_000);
    match state
        .dht
        .synchronous_search_metadata(req.file_hash, timeout)
        .await
    {
        Ok(m) => (StatusCode::OK, Json(m)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(http_server::ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

async fn api_download(
    State(state): State<Arc<HeadlessE2eState>>,
    Json(req): Json<DownloadRequest>,
) -> impl IntoResponse {
    let meta_opt = match state
        .dht
        .synchronous_search_metadata(req.file_hash.clone(), 10_000)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse { error: e }),
            )
                .into_response();
        }
    };
    let Some(meta) = meta_opt else {
        return (
            StatusCode::NOT_FOUND,
            Json(http_server::ErrorResponse {
                error: "Metadata not found".to_string(),
            }),
        )
            .into_response();
    };

    let seeder_url = req
        .seeder_url
        .or_else(|| meta.http_sources.as_ref().and_then(|v| v.first()).map(|s| s.url.clone()))
        .ok_or_else(|| "No httpSources in metadata".to_string());
    let seeder_url = match seeder_url {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(http_server::ErrorResponse { error: e }),
            )
                .into_response();
        }
    };

    let out_name = req.file_name.unwrap_or_else(|| meta.file_name.clone());
    let downloads_dir = state.storage_dir.join("downloads");
    let _ = tokio::fs::create_dir_all(&downloads_dir).await;
    let output_path = downloads_dir.join(&out_name);

    let peer_id = Some(state.dht.get_peer_id().await);
    let client = HttpDownloadClient::new_with_peer_id(peer_id);
    if let Err(e) = client
        .download_file(&seeder_url, &meta.merkle_root, &output_path, None)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(http_server::ErrorResponse { error: e }),
        )
            .into_response();
    }

    let bytes = match tokio::fs::read(&output_path).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse {
                    error: format!("Failed to read downloaded file: {}", e),
                }),
            )
                .into_response();
        }
    };
    let mut hasher = sha2::Sha256::new();
    hasher.update(&bytes);
    let computed = format!("{:x}", hasher.finalize());
    let verified = computed == meta.merkle_root;

    (
        StatusCode::OK,
        Json(DownloadResponse {
            download_path: output_path.to_string_lossy().to_string(),
            verified,
            bytes: bytes.len() as u64,
        }),
    )
        .into_response()
}

async fn api_pay(
    State(state): State<Arc<HeadlessE2eState>>,
    Json(req): Json<PayRequest>,
) -> impl IntoResponse {
    let Some(account) = state.uploader_address.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(http_server::ErrorResponse {
                error: "No active account. Set CHIRAL_PRIVATE_KEY and restart node.".to_string(),
            }),
        )
            .into_response();
    };
    let Some(private_key) = state.private_key.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(http_server::ErrorResponse {
                error: "No private key loaded. Set CHIRAL_PRIVATE_KEY and restart node.".to_string(),
            }),
        )
            .into_response();
    };

    match ethereum::send_transaction(&account, &req.uploader_address, req.price, &private_key).await {
        Ok(tx_hash) => (StatusCode::OK, Json(PayResponse { tx_hash })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(http_server::ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

async fn api_tx_receipt(
    State(_state): State<Arc<HeadlessE2eState>>,
    Json(req): Json<ReceiptRequest>,
) -> impl IntoResponse {
    match transaction_services::get_transaction_receipt(&req.tx_hash).await {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(http_server::ErrorResponse { error: e }),
        )
            .into_response(),
    }
}


