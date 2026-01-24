use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use rs_merkle::Hasher;
use rs_merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::cmp::min;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use chiral_network::ftp_server;

use crate::download_source::HttpSourceInfo;
use crate::http_download::HttpDownloadClient;
use crate::http_server;
use crate::manager::Sha256Hasher;
use crate::transaction_services;
use crate::{dht, ethereum};
use crate::{file_transfer::FileTransferService, manager::ChunkManager};
use crate::protocols::ProtocolHandler;

#[derive(Clone)]
pub struct HeadlessE2eState {
    pub dht: Arc<crate::dht::DhtService>,
    pub http_server_state: Arc<http_server::HttpServerState>,
    pub http_base_url: String,
    pub storage_dir: PathBuf,
    pub uploader_address: Option<String>,
    pub private_key: Option<String>,
    pub file_transfer_service: Option<Arc<FileTransferService>>,
    pub chunk_manager: Option<Arc<ChunkManager>>,
    /// Embedded FTP server (used for FTP upload E2E).
    pub ftp_server: Option<Arc<ftp_server::FtpServer>>,
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
    let dht_cmd_alive = state.dht.is_command_channel_alive().await;
    let rpc_endpoint = std::env::var("CHIRAL_RPC_ENDPOINT").ok();
    let status = if dht_cmd_alive {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(HealthResponse {
            ok: dht_cmd_alive,
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
    let protocol_upper = protocol.trim().to_uppercase();

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
    let mut seeder_url = state.http_base_url.clone();

    // Create temp file, stream-write deterministic bytes and compute sha256.
    // IMPORTANT: include file_name + protocol in the deterministic byte pattern so
    // different test cases don't collide on the same hash (e.g. 5MB WebRTC vs 5MB Bitswap).
    let tmp_dir = std::env::temp_dir().join("chiral-e2e");
    let _ = tokio::fs::create_dir_all(&tmp_dir).await;
    let tmp_path = tmp_dir.join(&file_name);

    let seed: u64 = {
        let mut h = sha2::Sha256::new();
        h.update(file_name.as_bytes());
        h.update(b"|");
        h.update(protocol.trim().as_bytes());
        let digest = h.finalize();
        u64::from_le_bytes(digest[0..8].try_into().unwrap_or([0u8; 8]))
    };

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
            let pos = written.wrapping_add(i as u64);
            // xorshift64* (deterministic, fast)
            let mut x = pos ^ seed;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let y = x.wrapping_mul(0x2545F4914F6CDD1D);
            *b = (y & 0xFF) as u8;
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

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let published_key = if protocol_upper == "HTTP" {
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
        file_hash.clone()
    } else if protocol_upper == "FTP" {
        let Some(ftp_server) = state.ftp_server.clone() else {
            return (
                StatusCode::BAD_REQUEST,
                Json(http_server::ErrorResponse {
                    error: "FTP upload requires an embedded FTP server in headless mode.".to_string(),
                }),
            )
                .into_response();
        };

        if !ftp_server.is_running().await {
            if let Err(e) = ftp_server.start().await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to start embedded FTP server: {}", e),
                    }),
                )
                    .into_response();
            }
        }

        // Read file bytes (small in E2E) and seed via embedded FTP server.
        let bytes = match tokio::fs::read(&tmp_path).await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to read temp file for FTP publish: {}", e),
                    }),
                )
                    .into_response();
            }
        };

        let safe_name = file_name.replace(['\\', '/', ':'], "_");
        let ftp_file_name = format!("{}_{}", file_hash, safe_name);
        let mut ftp_url: String = match ftp_server.add_file_data(&bytes, &ftp_file_name).await {
            Ok(u) => u,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to add file to embedded FTP server: {}", e),
                    }),
                )
                    .into_response();
            }
        };

        // If CHIRAL_PUBLIC_IP is set, rewrite the URL host to make it reachable cross-machine.
        if let Ok(host) = std::env::var("CHIRAL_PUBLIC_IP") {
            let host = host.trim();
            if !host.is_empty() {
                if let Ok(mut url) = url::Url::parse(&ftp_url) {
                    let _ = url.set_host(Some(host));
                    ftp_url = url.to_string();
                }
            }
        }
        // Surface the seeder URL in the upload response for debugging.
        seeder_url = ftp_url.clone();

        // Build manifest (SHA-256 per 256KB chunk) so multi-source validators can verify chunks if used.
        let chunk_size: usize = 256 * 1024;
        let mut manifest_chunks: Vec<crate::manager::ChunkInfo> = Vec::new();
        {
            use sha2::{Digest as _, Sha256};
            let mut offset: usize = 0;
            let mut index: u32 = 0;
            while offset < bytes.len() {
                let end = min(bytes.len(), offset + chunk_size);
                let slice = &bytes[offset..end];
                let mut h = Sha256::new();
                h.update(slice);
                let hash = format!("{:x}", h.finalize());
                let size = slice.len();
                manifest_chunks.push(crate::manager::ChunkInfo {
                    index,
                    hash: hash.clone(),
                    size,
                    encrypted_hash: hash,
                    encrypted_size: size,
                });
                offset = end;
                index += 1;
            }
        }
        let file_manifest = crate::manager::FileManifest {
            merkle_root: file_hash.clone(),
            chunks: manifest_chunks,
            encrypted_key_bundle: None,
        };
        let manifest_json = match serde_json::to_string(&file_manifest) {
            Ok(s) => s,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to serialize FileManifest: {}", e),
                    }),
                )
                    .into_response();
            }
        };

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
            ftp_sources: Some(vec![dht::models::FtpSourceInfo {
                url: ftp_url.clone(),
                username: None,
                password: None,
                supports_resume: true,
                file_size,
                last_checked: Some(created_at),
                is_available: true,
            }]),
            ed2k_sources: None,
            http_sources: None,
            is_root: true,
            download_path: None,
            price,
            uploader_address: state.uploader_address.clone(),
            info_hash: None,
            trackers: None,
            manifest: Some(manifest_json),
        };

        if let Err(e) = state.dht.publish_file(meta, None).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse {
                    error: format!("Failed to publish FTP metadata to DHT: {}", e),
                }),
            )
                .into_response();
        }

        // DHT key for FTP is the file hash.
        file_hash.clone()
    } else if protocol_upper == "WEBRTC" {
        let Some(ft) = state.file_transfer_service.clone() else {
            return (
                StatusCode::BAD_REQUEST,
                Json(http_server::ErrorResponse {
                    error: "WebRTC upload requires P2P services in headless mode. Set CHIRAL_ENABLE_P2P=1 and restart node.".to_string(),
                }),
            )
                .into_response();
        };
        let Some(chunk_manager) = state.chunk_manager.clone() else {
            return (
                StatusCode::BAD_REQUEST,
                Json(http_server::ErrorResponse {
                    error: "WebRTC upload requires ChunkManager in headless mode. Set CHIRAL_ENABLE_P2P=1 and restart node.".to_string(),
                }),
            )
                .into_response();
        };

        // Compute manifest merkle root (used as DHT key for WebRTC)
        let tmp_path_clone = tmp_path.clone();
        let cm = chunk_manager.clone();
        let canon = match tokio::task::spawn_blocking(move || {
            cm.chunk_and_encrypt_file_canonical(std::path::Path::new(&tmp_path_clone))
        })
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to create WebRTC manifest: {}", e),
                    }),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to run WebRTC manifest task: {}", e),
                    }),
                )
                    .into_response();
            }
        };
        let merkle_root = canon.manifest.merkle_root.clone();
        let manifest_json = match serde_json::to_string(&canon.manifest) {
            Ok(s) => s,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to serialize WebRTC manifest: {}", e),
                    }),
                )
                    .into_response();
            }
        };

        // Store file data under merkle_root so WebRTC requests can be served by FileTransferService.
        let bytes = match tokio::fs::read(&tmp_path).await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to read temp file for WebRTC storage: {}", e),
                    }),
                )
                    .into_response();
            }
        };
        ft.store_file_data(merkle_root.clone(), file_name.clone(), bytes).await;

        let meta = dht::models::FileMetadata {
            merkle_root: merkle_root.clone(),
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
            http_sources: None,
            is_root: true,
            download_path: None,
            price,
            uploader_address: state.uploader_address.clone(),
            info_hash: None,
            trackers: None,
            manifest: Some(manifest_json),
        };
        if let Err(e) = state.dht.publish_file(meta, None).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse {
                    error: format!("Failed to publish WebRTC metadata to DHT: {}", e),
                }),
            )
                .into_response();
        }
        merkle_root
    } else if protocol_upper == "BITSWAP" {
        // Compute merkle root using the same scheme as DHT publish (chunk hashes -> merkle root).
        let bytes = match tokio::fs::read(&tmp_path).await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(http_server::ErrorResponse {
                        error: format!("Failed to read temp file for Bitswap publish: {}", e),
                    }),
                )
                    .into_response();
            }
        };
        let chunk_size = 256 * 1024;
        let mut hashes: Vec<[u8; 32]> = Vec::new();
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = min(bytes.len(), offset + chunk_size);
            hashes.push(Sha256Hasher::hash(&bytes[offset..end]));
            offset = end;
        }
        let tree = MerkleTree::<Sha256Hasher>::from_leaves(&hashes);
        let root = match tree.root() {
            Some(r) => r,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(http_server::ErrorResponse {
                        error: "Failed to compute merkle root (empty file?)".to_string(),
                    }),
                )
                    .into_response();
            }
        };
        let merkle_root = hex::encode(root);

        // Provide file_data so DHT publish can insert blocks into Bitswap and set root CID.
        let meta = dht::models::FileMetadata {
            merkle_root: merkle_root.clone(),
            file_name: file_name.clone(),
            file_size,
            file_data: bytes,
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
            http_sources: None,
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
                    error: format!("Failed to publish Bitswap metadata to DHT: {}", e),
                }),
            )
                .into_response();
        }

        // IMPORTANT:
        // Bitswap downloads require `metadata.cids` (root CID). In real networks the record may become visible
        // before all fields are populated/preserved. Also, `synchronous_search_metadata` merges local cache,
        // so we validate against the raw DHT record bytes here (no cache merge).
        //
        // Default: wait up to ~60s.
        let max_attempts: u32 = 240;
        let mut ready = false;
        for _ in 0..max_attempts {
            match state.dht.get_dht_value(merkle_root.clone()).await {
                Ok(Some(bytes)) => {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        let cids_ok = json
                            .get("cids")
                            .and_then(|v| v.as_array())
                            .map(|arr| !arr.is_empty())
                            .unwrap_or(false);
                        if cids_ok {
                            ready = true;
                            break;
                        }
                    }
                }
                _ => {}
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        if !ready {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse {
                    error: format!(
                        "Upload completed but Bitswap DHT record not ready yet for {} (missing cids in raw record)",
                        merkle_root
                    ),
                }),
            )
                .into_response();
        }
        merkle_root
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(http_server::ErrorResponse {
                error: format!("Unsupported protocol '{}'. Use HTTP, WebRTC, Bitswap, or FTP.", protocol),
            }),
        )
            .into_response();
    };

    (
        StatusCode::OK,
        Json(UploadResponse {
            file_hash: published_key,
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

    let protocol_upper = req.protocol.as_deref().unwrap_or("HTTP").trim().to_uppercase();
    let seeder_url = if protocol_upper == "HTTP" {
        let seeder_url = req
            .seeder_url
            .or_else(|| meta.http_sources.as_ref().and_then(|v| v.first()).map(|s| s.url.clone()))
            .ok_or_else(|| "No httpSources in metadata".to_string());
        match seeder_url {
            Ok(v) => Some(v),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(http_server::ErrorResponse { error: e }),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    let out_name = req.file_name.unwrap_or_else(|| meta.file_name.clone());
    let downloads_dir = state.storage_dir.join("downloads");
    let _ = tokio::fs::create_dir_all(&downloads_dir).await;
    // Avoid collisions across runs by generating a unique output name.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let hash_prefix: String = meta.merkle_root.chars().take(8).collect();
    let safe_name = out_name.replace(['\\', '/', ':'], "_");
    let output_path = downloads_dir.join(format!(
        "{}-{}-{}-{}",
        protocol_upper.to_lowercase(),
        hash_prefix,
        now_ms,
        safe_name
    ));

    if protocol_upper == "HTTP" {
        let peer_id = Some(state.dht.get_peer_id().await);
        let client = HttpDownloadClient::new_with_peer_id(peer_id);
        if let Err(e) = client
            .download_file(seeder_url.as_ref().unwrap(), &meta.merkle_root, &output_path, None)
            .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse { error: e }),
            )
                .into_response();
        }
    } else if protocol_upper == "FTP" {
        let ftp_url = meta
            .ftp_sources
            .as_ref()
            .and_then(|v| v.first())
            .map(|s| s.url.clone())
            .ok_or_else(|| "No ftpSources in metadata".to_string());
        let ftp_url = match ftp_url {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(http_server::ErrorResponse { error: e }),
                )
                    .into_response();
            }
        };

        // Use the protocol handler to download to the target output path.
        let handler = crate::protocols::ftp::FtpProtocolHandler::new();
        let opts = crate::protocols::traits::DownloadOptions {
            output_path: output_path.clone(),
            max_peers: None,
            chunk_size: None,
            encryption: false,
            bandwidth_limit: None,
        };
        if let Err(e) = handler.download(&ftp_url, opts).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse { error: e.to_string() }),
            )
                .into_response();
        }

        // Best-effort wait for file to appear and reach expected size.
        for _ in 0..240 {
            if let Ok(m) = tokio::fs::metadata(&output_path).await {
                if m.len() == meta.file_size {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    } else if protocol_upper == "WEBRTC" || protocol_upper == "BITSWAP" {
        // Note: current DHT DownloadFile command uses WebRTC if available (Bitswap path is not wired).
        // Still allow requesting downloads in headless for these protocols.
        let mut meta_for_dl = meta.clone();
        meta_for_dl.download_path = Some(output_path.to_string_lossy().to_string());
        if let Err(e) = state
            .dht
            .download_file(meta_for_dl, output_path.to_string_lossy().to_string())
            .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse { error: e }),
            )
                .into_response();
        }

        // Best-effort wait for file to appear.
        for _ in 0..240 {
            if tokio::fs::metadata(&output_path).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(http_server::ErrorResponse {
                error: format!("Unsupported protocol '{}'. Use HTTP, WebRTC, Bitswap, or FTP.", protocol_upper),
            }),
        )
            .into_response();
    }

    let bytes_len = match tokio::fs::metadata(&output_path).await {
        Ok(m) => m.len(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(http_server::ErrorResponse {
                    error: format!("Failed to stat downloaded file: {}", e),
                }),
            )
                .into_response();
        }
    };
    // Stronger verify for FTP: sha256(file) must match merkle_root (file hash).
    let verified = if protocol_upper == "FTP" {
        match tokio::fs::read(&output_path).await {
            Ok(b) => {
                let mut h = sha2::Sha256::new();
                h.update(&b);
                let computed = format!("{:x}", h.finalize());
                computed == meta.merkle_root && bytes_len == meta.file_size
            }
            Err(_) => false,
        }
    } else {
        bytes_len == meta.file_size
    };

    (
        StatusCode::OK,
        Json(DownloadResponse {
            download_path: output_path.to_string_lossy().to_string(),
            verified,
            bytes: bytes_len,
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


