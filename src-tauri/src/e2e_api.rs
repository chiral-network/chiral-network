use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use base64::{engine::general_purpose, Engine as _};
use librqbit::torrent_from_bytes;
use sha2::Digest;
use tauri::Manager;

use crate::download_source::HttpSourceInfo;
use crate::file_transfer::FileTransferService;
use crate::http_download::HttpDownloadClient;
use crate::http_server;
use crate::manager::ChunkManager;
use crate::protocols::ProtocolHandler;
use crate::transaction_services;
use crate::webrtc_service::{set_webrtc_service, WebRTCService};

fn extract_btih_info_hash(identifier: &str) -> Option<String> {
    if let Some(start) = identifier.find("urn:btih:") {
        let start = start + 9;
        let end = identifier[start..]
            .find('&')
            .map(|i| start + i)
            .unwrap_or(identifier.len());
        return Some(identifier[start..end].to_lowercase());
    }
    None
}

fn bt_handshake_bytes(info_hash_hex: &str) -> Result<[u8; 68], String> {
    let ih = hex::decode(info_hash_hex)
        .map_err(|e| format!("Invalid info_hash hex for BT handshake: {}", e))?;
    if ih.len() != 20 {
        return Err(format!(
            "Invalid info_hash length for BT handshake: expected 20 bytes, got {}",
            ih.len()
        ));
    }
    let mut out = [0u8; 68];
    out[0] = 19;
    out[1..20].copy_from_slice(b"BitTorrent protocol");
    // reserved [20..28] left as 0
    out[28..48].copy_from_slice(&ih);
    // 20-byte peer id (dummy, deterministic)
    // MUST be exactly 20 bytes; otherwise this panics at runtime.
    const PEER_ID: [u8; 20] = *b"-CHIRAL-E2E-00000000";
    out[48..68].copy_from_slice(&PEER_ID);
    Ok(out)
}

async fn bt_handshake_diag_on_fail(
    addr: Option<std::net::SocketAddr>,
    expected_info_hash: &str,
) -> String {
    let Some(addr) = addr else {
        return "bt_fail_diag=<no-initial-peer>".to_string();
    };

    let hs = match bt_handshake_bytes(expected_info_hash) {
        Ok(h) => h,
        Err(e) => return format!("bt_fail_diag=<handshake-bytes-error:{}>", e),
    };

    let connect =
        tokio::time::timeout(std::time::Duration::from_secs(2), TcpStream::connect(addr)).await;
    let mut stream = match connect {
        Err(_) => return format!("bt_fail_diag=connect_timeout addr={}", addr),
        Ok(Err(e)) => return format!("bt_fail_diag=connect_error addr={} err={}", addr, e),
        Ok(Ok(s)) => s,
    };

    if tokio::time::timeout(std::time::Duration::from_secs(2), stream.write_all(&hs))
        .await
        .is_err()
    {
        return format!("bt_fail_diag=handshake_write_timeout addr={}", addr);
    }

    let mut resp = [0u8; 68];
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        stream.read_exact(&mut resp),
    )
    .await
    {
        Err(_) => format!("bt_fail_diag=handshake_read_timeout addr={}", addr),
        Ok(Err(e)) => format!("bt_fail_diag=handshake_read_error addr={} err={}", addr, e),
        Ok(Ok(_)) => {
            // Very lightweight validation: pstrlen + protocol string + info_hash match.
            if resp[0] != 19 || &resp[1..20] != b"BitTorrent protocol" {
                return format!(
                    "bt_fail_diag=handshake_invalid_prefix addr={} pstrlen={} proto={:?}",
                    addr,
                    resp[0],
                    truncate_diag(String::from_utf8_lossy(&resp[1..20]).to_string(), 64)
                );
            }
            let ih = match hex::decode(expected_info_hash) {
                Ok(v) => v,
                Err(_) => vec![],
            };
            if ih.len() == 20 && resp[28..48] != ih[..] {
                return format!(
                    "bt_fail_diag=handshake_info_hash_mismatch addr={} expected={} got={}",
                    addr,
                    expected_info_hash,
                    hex::encode(&resp[28..48])
                );
            }
            format!("bt_fail_diag=handshake_ok addr={}", addr)
        }
    }
}

fn truncate_diag(mut s: String, max_len: usize) -> String {
    if s.len() <= max_len {
        return s;
    }
    s.truncate(max_len);
    s.push_str("…<truncated>");
    s
}

fn build_magnet_link(
    info_hash: &str,
    display_name: Option<&str>,
    trackers: Option<&Vec<String>>,
) -> String {
    let mut s = format!("magnet:?xt=urn:btih:{}", info_hash);
    if let Some(name) = display_name {
        if !name.trim().is_empty() {
            s.push_str("&dn=");
            s.push_str(&urlencoding::encode(name));
        }
    }
    if let Some(trs) = trackers {
        for tr in trs {
            if tr.trim().is_empty() {
                continue;
            }
            s.push_str("&tr=");
            s.push_str(&urlencoding::encode(tr));
        }
    }
    s
}

async fn find_file_recursive(
    root: &std::path::Path,
    expected_name: &str,
    expected_size: u64,
) -> Result<std::path::PathBuf, String> {
    let mut queue: std::collections::VecDeque<std::path::PathBuf> =
        std::collections::VecDeque::new();
    queue.push_back(root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        while let Some(ent) = rd
            .next_entry()
            .await
            .map_err(|e| format!("Failed to iterate dir {:?}: {}", dir, e))?
        {
            let path = ent.path();
            let md = ent
                .metadata()
                .await
                .map_err(|e| format!("Failed to stat {:?}: {}", path, e))?;
            if md.is_dir() {
                queue.push_back(path);
                continue;
            }
            if md.is_file() {
                let name_ok = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s == expected_name)
                    .unwrap_or(false);
                if name_ok && md.len() == expected_size {
                    return Ok(path);
                }
            }
        }
    }

    Err(format!(
        "Downloaded BitTorrent file not found under {:?} (name={}, size={})",
        root, expected_name, expected_size
    ))
}

#[derive(Clone)]
pub struct E2eApiState {
    pub app: tauri::AppHandle,
    downloads: Arc<Mutex<HashMap<String, DownloadJob>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadJob {
    status: String, // "running" | "success" | "failed"
    download_path: String,
    verified: bool,
    bytes: u64,
    error: Option<String>,
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
    /// Protocol string - supported: HTTP (range), WebRTC (P2P), Bitswap (blocks), FTP
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
    /// For BitTorrent E2E: base64-encoded .torrent bytes so downloaders can start without magnet metadata exchange.
    torrent_base64: Option<String>,
    /// For BitTorrent E2E: actual TCP listen port of the uploader's BitTorrent session.
    bittorrent_port: Option<u16>,
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
    /// Optional protocol override. supported: HTTP, WebRTC, Bitswap, FTP
    protocol: Option<String>,
    /// For BitTorrent E2E: base64-encoded .torrent bytes (optional).
    torrent_base64: Option<String>,
    /// For BitTorrent E2E: uploader public IP to use as initial peer.
    bittorrent_seeder_ip: Option<String>,
    /// For BitTorrent E2E: uploader BitTorrent TCP listen port to use as initial peer.
    bittorrent_seeder_port: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadResponse {
    download_path: String,
    verified: bool,
    bytes: u64,
    /// Present for async downloads (WebRTC/Bitswap). Use /api/download/status/:id to poll.
    download_id: Option<String>,
    /// "running" | "success" | "failed"
    status: Option<String>,
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

async fn ensure_p2p_services_started(app: &tauri::AppHandle) -> Result<(), String> {
    // E2E spawn mode starts `tauri dev` without any frontend interaction, so
    // FileTransfer/WebRTC may never be started. WebRTC downloads will then fail with
    // "File transfer service is not running". For E2E API usage, we auto-start them.
    let state = app.state::<crate::AppState>();

    // If the frontend never bootstrapped services, we may be missing:
    // - FileTransferService (required for WebRTC + MultiSource)
    // - WebRTCService (required for MultiSource)
    // - ChunkManager (required for FTP/MultiSource manifest-based verification)
    //
    // For E2E API usage we ensure all are present.

    // Ensure ChunkManager exists (used by FTP/MultiSource and some WebRTC paths).
    {
        let mut chunk_guard = state.chunk_manager.lock().await;
        if chunk_guard.is_none() {
            let chunk_storage_path = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Failed to get app data dir for chunk storage: {}", e))?
                .join("chunk_storage");
            let _ = std::fs::create_dir_all(&chunk_storage_path);
            *chunk_guard = Some(Arc::new(ChunkManager::new(chunk_storage_path)));
        }
    }

    // Ensure FileTransferService exists (storage under app data dir).
    let ft_arc = {
        let existing = {
            let ft_guard = state.file_transfer.lock().await;
            ft_guard.as_ref().cloned()
        };
        if let Some(ft) = existing {
            ft
        } else {
            let ft = FileTransferService::new_with_app_handle(app.clone())
                .await
                .map_err(|e| format!("Failed to start file transfer service: {}", e))?;
            let ft_arc = Arc::new(ft);
            let mut ft_guard = state.file_transfer.lock().await;
            *ft_guard = Some(ft_arc.clone());
            ft_arc
        }
    };

    // Ensure WebRTCService exists and set the global singleton (used by chunk processing).
    let webrtc_arc = {
        let existing = {
            let guard = state.webrtc.lock().await;
            guard.as_ref().cloned()
        };
        if let Some(w) = existing {
            w
        } else {
            let webrtc = WebRTCService::new(
                app.clone(),
                ft_arc.clone(),
                state.keystore.clone(),
                state.bandwidth.clone(),
            )
            .await
            .map_err(|e| format!("Failed to start WebRTC service: {}", e))?;

            let webrtc_arc = Arc::new(webrtc);
            let mut guard = state.webrtc.lock().await;
            *guard = Some(webrtc_arc.clone());
            webrtc_arc
        }
    };
    set_webrtc_service(webrtc_arc).await;

    Ok(())
}

// NOTE:
// `ensure_multi_source_services_started` was previously used for FTP E2E downloads.
// We now route FTP E2E downloads through the dedicated FTP protocol handler instead,
// because MultiSource deprioritizes FTP behind P2P sources and can get stuck on chunk/range behavior.

pub async fn start_e2e_api_server(app: tauri::AppHandle, port: u16) -> Result<SocketAddr, String> {
    let state = E2eApiState {
        app,
        downloads: Arc::new(Mutex::new(HashMap::new())),
    };
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
        .route("/api/download/status/:id", get(api_download_status))
        .route("/api/pay", post(api_pay))
        .route("/api/tx/receipt", post(api_tx_receipt))
        .with_state(Arc::new(state))
}

async fn api_download_status(
    State(state): State<Arc<E2eApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let jobs = state.downloads.lock().await;
    let Some(job) = jobs.get(&id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(crate::http_server::ErrorResponse {
                error: "Download not found".to_string(),
            }),
        )
            .into_response();
    };
    (StatusCode::OK, Json(job)).into_response()
}

async fn api_health(State(state): State<Arc<E2eApiState>>) -> impl IntoResponse {
    // IMPORTANT:
    // Real E2E spawn mode uses /api/health to decide "node is ready".
    // If we return 200 too early (before HTTP server / P2P services are initialized),
    // the subsequent /api/upload can fail with:
    // - "HTTP file server is not running (no bound address)"
    // - "File transfer service is not running" (WebRTC paths)
    //
    // So we treat /api/health as a readiness gate:
    // - ensure (best-effort) P2P services are started
    // - require DHT + HTTP file server bound before returning 200

    // Best-effort: auto-start P2P services needed for WebRTC uploads/downloads in E2E.
    // Don't fail health solely due to P2P init errors, but log readiness via HTTP status below.
    let _ = ensure_p2p_services_started(&state.app).await;

    let node_id = std::env::var("CHIRAL_NODE_ID").ok();
    let (peer_id, dht_cmd_alive) = {
        let app_state = state.app.state::<crate::AppState>();
        let dht = { app_state.dht.lock().await.as_ref().cloned() };
        match dht {
            Some(d) => {
                let alive = d.is_command_channel_alive().await;
                let id = Some(d.get_peer_id().await);
                (id, alive)
            }
            None => (None, false),
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
                let host =
                    std::env::var("CHIRAL_PUBLIC_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
                format!("http://{}:{}", host, addr.port())
            })
        }
    };
    let rpc_endpoint = std::env::var("CHIRAL_RPC_ENDPOINT").ok();

    // Readiness: require DHT peer_id and bound HTTP file server URL.
    // If not ready, return 503 so the test harness keeps polling.
    if peer_id.is_none() || file_server_url.is_none() || !dht_cmd_alive {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                ok: false,
                node_id,
                peer_id,
                file_server_url,
                rpc_endpoint,
            }),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(HealthResponse {
            ok: true,
            node_id,
            peer_id,
            file_server_url,
            rpc_endpoint,
        }),
    )
        .into_response()
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
    let protocol_norm = protocol.trim();
    let protocol_upper = protocol_norm.to_uppercase();

    // Determine the seeder base URL (public IP if provided; otherwise localhost).
    let app_state = state.app.state::<crate::AppState>();

    // For HTTP uploads, we need a running HTTP file server to serve range requests.
    let seeder_url = if protocol_upper == "HTTP" {
        let bound_addr = app_state.http_server_addr.lock().await.clone();
        let Some(bound_addr) = bound_addr else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: "HTTP file server is not running (no bound address)".to_string(),
                }),
            )
                .into_response();
        };
        if let Ok(v) = std::env::var("CHIRAL_FILE_SERVER_URL") {
            if !v.trim().is_empty() {
                v.trim().to_string()
            } else {
                format!("http://127.0.0.1:{}", bound_addr.port())
            }
        } else {
            let host =
                std::env::var("CHIRAL_PUBLIC_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
            format!("http://{}:{}", host, bound_addr.port())
        }
    } else {
        // For P2P protocols, this is not used.
        String::new()
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
        return (
            StatusCode::BAD_REQUEST,
            Json(crate::http_server::ErrorResponse {
                error: "No active account. Set CHIRAL_PRIVATE_KEY and restart node.".to_string(),
            }),
        )
            .into_response();
    }

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
        h.update(protocol_norm.as_bytes());
        let digest = h.finalize();
        u64::from_le_bytes(digest[0..8].try_into().unwrap_or([0u8; 8]))
    };

    let mut hasher = sha2::Sha256::new();
    let mut f = match tokio::fs::File::create(&tmp_path).await {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
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
                Json(crate::http_server::ErrorResponse {
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

    // Protocol-specific handling:
    // - HTTP: move into HTTP file server storage and publish metadata with http_sources
    // - WebRTC/Bitswap/FTP: invoke the app's upload command so protocol services publish correct metadata
    // - BitTorrent: seed via ProtocolManager to obtain a magnet/info_hash, then publish DHT metadata keyed by info_hash
    let published_key: String = if protocol_upper == "HTTP" {
        // Move into provider storage dir and register with HTTP file server state.
        let permanent_path = app_state.http_server_state.storage_dir.join(&file_hash);
        if let Err(e) = tokio::fs::rename(&tmp_path, &permanent_path).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: format!("Failed to move file into storage: {}", e),
                }),
            )
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(crate::http_server::ErrorResponse {
                        error: format!("Failed to publish metadata to DHT: {}", e),
                    }),
                )
                    .into_response();
            }
        } else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: "DHT is not running".to_string(),
                }),
            )
                .into_response();
        }
        file_hash.clone()
    } else if protocol_upper == "BITTORRENT" {
        // Seed the file via the protocol manager to obtain a magnet link (contains info_hash).
        let protocol_manager = { app_state.protocol_manager.lock().await.as_ref().cloned() };
        let Some(protocol_manager) = protocol_manager else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: "Protocol manager is not running".to_string(),
                }),
            )
                .into_response();
        };

        let seeding = match protocol_manager
            .seed(
                "bittorrent",
                std::path::PathBuf::from(&tmp_path),
                crate::protocols::traits::SeedOptions::default(),
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(crate::http_server::ErrorResponse {
                        error: format!("Failed to seed BitTorrent: {}", e),
                    }),
                )
                    .into_response();
            }
        };

        let info_hash = match extract_btih_info_hash(&seeding.identifier) {
            Some(h) => h,
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(crate::http_server::ErrorResponse {
                        error: format!(
                            "BitTorrent seeding returned an unsupported identifier (no btih): {}",
                            seeding.identifier
                        ),
                    }),
                )
                    .into_response();
            }
        };

        // Publish metadata to DHT keyed by info_hash so downloader can search by that key.
        let dht = { app_state.dht.lock().await.as_ref().cloned() };
        let Some(dht) = dht else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: "DHT is not running".to_string(),
                }),
            )
                .into_response();
        };

        // Include local peer id as a seeder so consumers can optionally correlate to libp2p identity.
        let local_peer_id = Some(dht.get_peer_id().await);
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let meta = crate::dht::models::FileMetadata {
            // For BitTorrent, use info_hash as the DHT key so search/download can use a stable identifier.
            merkle_root: info_hash.clone(),
            file_name: file_name.clone(),
            file_size,
            file_data: vec![],
            seeders: local_peer_id.map_or(vec![], |id| vec![id]),
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
            uploader_address: uploader_address.clone(),
            info_hash: Some(info_hash.clone()),
            // Keep consistent with the app-side BitTorrent publish default.
            trackers: Some(vec!["udp://tracker.openbittorrent.com:80".to_string()]),
            manifest: None,
        };

        if let Err(e) = dht.publish_file(meta, None).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: format!("Failed to publish BitTorrent metadata to DHT: {}", e),
                }),
            )
                .into_response();
        }

        // Wait until the metadata is visible on this node's DHT (best-effort).
        let mut found = None;
        for _ in 0..80 {
            match dht
                .synchronous_search_metadata(info_hash.clone(), 1_500)
                .await
            {
                Ok(Some(m)) => {
                    found = Some(m);
                    break;
                }
                _ => {}
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        if found.is_none() {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: format!(
                        "Upload completed but metadata not visible yet for {}",
                        info_hash
                    ),
                }),
            )
                .into_response();
        }

        info_hash
    } else if protocol_upper == "WEBRTC" || protocol_upper == "BITSWAP" || protocol_upper == "FTP" {
        // Pre-compute the DHT key for the published metadata so we can wait until it's discoverable.
        // WebRTC uses a manifest Merkle root; Bitswap uses a sha256-like content root (matching the file hash).
        let expected_merkle_root = if protocol_upper == "WEBRTC" {
            // Use the same ChunkManager logic as upload_file_to_network (but without secrets) to get the merkle root.
            let chunk_storage_path = match state.app.path().app_data_dir() {
                Ok(p) => p.join("chunks"),
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(crate::http_server::ErrorResponse {
                            error: format!("Failed to get app data dir for chunk storage: {}", e),
                        }),
                    )
                        .into_response();
                }
            };
            let manager = ChunkManager::new(chunk_storage_path);
            let tmp_path_clone = tmp_path.clone();
            let result = tokio::task::spawn_blocking(move || {
                manager.chunk_and_encrypt_file_canonical(std::path::Path::new(&tmp_path_clone))
            })
            .await
            .map_err(|e| format!("Failed to spawn blocking chunking task: {}", e));

            match result {
                Ok(Ok(canon)) => canon.manifest.merkle_root,
                Ok(Err(e)) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(crate::http_server::ErrorResponse {
                            error: format!("Failed to compute WebRTC merkle root: {}", e),
                        }),
                    )
                        .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(crate::http_server::ErrorResponse { error: e }),
                    )
                        .into_response();
                }
            }
        } else {
            file_hash.clone()
        };

        // Invoke the normal upload command (seeds + publishes protocol-correct metadata).
        // Note: upload_file_to_network returns immediately for some protocols; we'll wait on DHT visibility below.
        if let Err(e) = crate::upload_file_to_network(
            state.app.clone(),
            state.app.state::<crate::AppState>(),
            tmp_path.to_string_lossy().to_string(),
            Some(price),
            Some(protocol_norm.to_string()),
            Some(file_name.clone()),
        )
        .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse { error: e }),
            )
                .into_response();
        }

        // Wait until the metadata is visible on this node's DHT (best-effort, avoids race in tests).
        //
        // IMPORTANT:
        // For Bitswap, downloads require `metadata.cids` (root CID).
        //
        // NOTE: `synchronous_search_metadata` merges local cache, so it can return `cids` even when the actual
        // DHT record does not contain them yet. For Bitswap, we therefore validate against the raw DHT record bytes.
        let dht = { app_state.dht.lock().await.as_ref().cloned() };
        let Some(dht) = dht else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: "DHT is not running".to_string(),
                }),
            )
                .into_response();
        };
        // Trigger search and poll cache for metadata
        let _ = dht
            .search_metadata(expected_merkle_root.clone(), 1500)
            .await;

        let mut found = None;
        // Default: ~10s total. Bitswap may need longer until `cids` is observable.
        let max_attempts: u32 = if protocol_upper == "BITSWAP" { 240 } else { 40 }; // 60s vs 10s
        for _ in 0..max_attempts {
            if protocol_upper == "BITSWAP" {
                // Raw-record check (no cache merge): ensure the stored JSON contains a non-empty `cids` array.
                if let Ok(Some(bytes)) = dht.get_dht_value(expected_merkle_root.clone()).await {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        let cids_ok = json
                            .get("cids")
                            .and_then(|v| v.as_array())
                            .map(|arr| !arr.is_empty())
                            .unwrap_or(false);
                        if cids_ok {
                            // Once the raw record is good, we can return the parsed metadata.
                            if let Ok(Some(m)) = dht
                                .synchronous_search_metadata(expected_merkle_root.clone(), 1_500)
                                .await
                            {
                                found = Some(m);
                                break;
                            }
                        }
                    }
                }
            } else {
                match dht
                    .synchronous_search_metadata(expected_merkle_root.clone(), 1_500)
                    .await
                {
                    Ok(Some(m)) => {
                        found = Some(m);
                        break;
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        if found.is_none() {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(crate::http_server::ErrorResponse {
                error: if protocol_upper == "BITSWAP" {
                    format!(
                        "Upload completed but Bitswap DHT record not ready yet for {} (missing cids in raw record)",
                        expected_merkle_root
                    )
                } else {
                    format!(
                        "Upload completed but metadata not visible yet for {}",
                        expected_merkle_root
                    )
                },
            }))
            .into_response();
        }

        expected_merkle_root
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(crate::http_server::ErrorResponse {
                error: format!(
                    "Unsupported protocol '{}'. Use HTTP, WebRTC, Bitswap, FTP, or BitTorrent.",
                    protocol_norm
                ),
            }),
        )
            .into_response();
    };

    (
        StatusCode::OK,
        Json(UploadResponse {
            // For all protocols, return the DHT lookup key as fileHash (merkle_root / content root).
            file_hash: published_key.clone(),
            file_name,
            file_size,
            seeder_url,
            uploader_address,
            torrent_base64: if protocol_upper == "BITTORRENT" {
                let app_state = state.app.state::<crate::AppState>();
                let bt = { app_state.bittorrent_handler.lock().await.as_ref().cloned() };
                match bt {
                    Some(bt) => bt
                        .get_seeded_torrent_bytes(&published_key)
                        .await
                        .map(|bytes| general_purpose::STANDARD.encode(bytes)),
                    None => None,
                }
            } else {
                None
            },
            bittorrent_port: if protocol_upper == "BITTORRENT" {
                let app_state = state.app.state::<crate::AppState>();
                let bt = { app_state.bittorrent_handler.lock().await.as_ref().cloned() };
                bt.map(|bt| bt.rqbit_session().tcp_listen_port()).flatten()
            } else {
                None
            },
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
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::http_server::ErrorResponse {
                error: "DHT is not running".to_string(),
            }),
        )
            .into_response();
    };

    let timeout = req.timeout_ms.unwrap_or(10_000);

    // Trigger search
    if let Err(e) = dht.search_metadata(req.file_hash.clone(), timeout).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::http_server::ErrorResponse { error: e }),
        )
            .into_response();
    }

    // Poll cache for metadata
    let poll_iterations = (timeout / 100) as usize;
    let mut metadata = None;
    for _ in 0..poll_iterations {
        if let Some(m) = dht.get_cached_metadata(&req.file_hash).await {
            metadata = Some(m);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    (StatusCode::OK, Json(metadata)).into_response()
}

async fn api_download(
    State(state): State<Arc<E2eApiState>>,
    Json(req): Json<DownloadRequest>,
) -> impl IntoResponse {
    let app_state = state.app.state::<crate::AppState>();
    let dht = { app_state.dht.lock().await.as_ref().cloned() };
    let Some(dht) = dht else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::http_server::ErrorResponse {
                error: "DHT is not running".to_string(),
            }),
        )
            .into_response();
    };

    // Trigger search and poll cache
    if let Err(e) = dht.search_metadata(req.file_hash.clone(), 10_000).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::http_server::ErrorResponse { error: e }),
        )
            .into_response();
    }

    let mut meta_opt = None;
    for _ in 0..100 {
        // 100 * 100ms = 10s
        if let Some(m) = dht.get_cached_metadata(&req.file_hash).await {
            meta_opt = Some(m);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let Some(meta) = meta_opt else {
        return (
            StatusCode::NOT_FOUND,
            Json(crate::http_server::ErrorResponse {
                error: "Metadata not found".to_string(),
            }),
        )
            .into_response();
    };

    let out_name = req.file_name.unwrap_or_else(|| meta.file_name.clone());
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
            .or_else(|| {
                meta.http_sources
                    .as_ref()
                    .and_then(|v| v.first())
                    .map(|s| s.url.clone())
            })
            .ok_or_else(|| "No httpSources in metadata".to_string());
        match seeder_url {
            Ok(v) => Some(v),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(crate::http_server::ErrorResponse { error: e }),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    // Use a stable downloads dir under temp for E2E.
    let downloads_dir = std::env::temp_dir().join("chiral-e2e-downloads");
    let _ = tokio::fs::create_dir_all(&downloads_dir).await;
    // IMPORTANT:
    // - Our E2E endpoint verifies by reading `output_path` after the download finishes.
    // - If `output_path` already exists from a previous run, we can accidentally verify the wrong file
    //   (or trigger resume/partial behaviors depending on protocol/client).
    // So we always generate a unique output filename per request (all protocols).
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
        // Ensure we're not resuming into an unrelated stale file.
        let _ = tokio::fs::remove_file(&output_path).await;

        // Include downloader peer id for provider metrics if available.
        let peer_id = Some(dht.get_peer_id().await);
        let client = HttpDownloadClient::new_with_peer_id(peer_id);
        if let Err(e) = client
            .download_file(
                seeder_url.as_ref().unwrap(),
                &meta.merkle_root,
                &output_path,
                None,
            )
            .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse { error: e }),
            )
                .into_response();
        }
    } else if protocol_upper == "WEBRTC"
        || protocol_upper == "BITSWAP"
        || protocol_upper == "FTP"
        || protocol_upper == "BITTORRENT"
    {
        // Auto-start P2P services for E2E spawn mode (no frontend bootstrapping).
        if let Err(e) = ensure_p2p_services_started(&state.app).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse { error: e }),
            )
                .into_response();
        }

        // IMPORTANT: WebRTC/Bitswap downloads can take a long time and the node test runner (undici fetch)
        // can time out waiting for response headers. So we run the download asynchronously and return
        // a downloadId immediately; the client polls /api/download/status/:id.
        let download_id = format!(
            "dl-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            meta.merkle_root.chars().take(8).collect::<String>()
        );
        let out_path_str = output_path.to_string_lossy().to_string();

        {
            let mut jobs = state.downloads.lock().await;
            jobs.insert(
                download_id.clone(),
                DownloadJob {
                    status: "running".to_string(),
                    download_path: out_path_str.clone(),
                    verified: false,
                    bytes: 0,
                    error: None,
                },
            );
        }

        let downloads_map = state.downloads.clone();
        let download_id_for_task = download_id.clone();
        let out_path_for_task = out_path_str.clone();
        let app_handle_for_task = state.app.clone();
        let meta_for_task = meta.clone();
        let protocol_upper_for_task = protocol_upper.clone();
        let torrent_base64_for_task = req.torrent_base64.clone();
        let bt_seeder_ip_for_task = req.bittorrent_seeder_ip.clone();
        let bt_seeder_port_for_task = req.bittorrent_seeder_port;

        tauri::async_runtime::spawn(async move {
            // Protocol-specific overrides (milliseconds). Fallback order:
            // 1) E2E_{PROTOCOL}_DOWNLOAD_TIMEOUT_MS
            // 2) E2E_P2P_DOWNLOAD_TIMEOUT_MS
            // 3) E2E_DOWNLOAD_WAIT_TIMEOUT_MS (legacy)
            // 4) 600000 (10 minutes)
            let timeout_ms: u64 = {
                let protocol_key = match protocol_upper_for_task.as_str() {
                    "WEBRTC" => Some("E2E_WEBRTC_DOWNLOAD_TIMEOUT_MS"),
                    "BITSWAP" => Some("E2E_BITSWAP_DOWNLOAD_TIMEOUT_MS"),
                    "FTP" => Some("E2E_FTP_DOWNLOAD_TIMEOUT_MS"),
                    "BITTORRENT" => Some("E2E_BITTORRENT_DOWNLOAD_TIMEOUT_MS"),
                    _ => None,
                };

                let raw = protocol_key
                    .and_then(|k| std::env::var(k).ok())
                    .or_else(|| std::env::var("E2E_P2P_DOWNLOAD_TIMEOUT_MS").ok())
                    .or_else(|| std::env::var("E2E_DOWNLOAD_WAIT_TIMEOUT_MS").ok());

                raw.and_then(|s| s.parse().ok()).unwrap_or(600_000)
            };

            let result: Result<u64, String> = async {
                if protocol_upper_for_task == "WEBRTC" {
                        // In case the service was stopped after request but before task runs.
                        ensure_p2p_services_started(&app_handle_for_task).await?;
                    let peer_id = meta_for_task
                        .seeders
                        .get(0)
                        .cloned()
                        .ok_or_else(|| {
                            "No seeder peer_id available for WebRTC download".to_string()
                        })?;
                    tracing::info!(
                        "E2E WebRTC download using peer_id={} file_hash={} file_name={} file_size={}",
                        peer_id,
                        meta_for_task.merkle_root,
                        meta_for_task.file_name,
                        meta_for_task.file_size
                    );
                    if let Err(e) = crate::download_file_from_network(
                        app_handle_for_task.state::<crate::AppState>(),
                        peer_id,
                        meta_for_task.merkle_root.clone(),
                        meta_for_task.file_name.clone(),
                        meta_for_task.file_size,
                        out_path_for_task.clone(),
                    )
                    .await
                    {
                        return Err(e);
                    }
                } else if protocol_upper_for_task == "BITSWAP" {
                    if let Err(e) = crate::download_blocks_from_network(
                        app_handle_for_task.state::<crate::AppState>(),
                        meta_for_task.clone(),
                        out_path_for_task.clone(),
                    )
                    .await
                    {
                        return Err(e);
                    }
                } else if protocol_upper_for_task == "FTP" {
                    // FTP: use the dedicated protocol handler (single-source) so FTP isn't deprioritized
                    // behind P2P sources and we avoid chunked-range edge cases.
                    let ftp_url = meta_for_task
                        .ftp_sources
                        .as_ref()
                        .and_then(|v| v.first())
                        .map(|s| s.url.clone())
                        .ok_or_else(|| "No ftpSources in metadata".to_string())?;

                    let handler = crate::protocols::ftp::FtpProtocolHandler::new();
                    let opts = crate::protocols::traits::DownloadOptions {
                        output_path: std::path::PathBuf::from(&out_path_for_task),
                        max_peers: None,
                        chunk_size: None,
                        encryption: false,
                        bandwidth_limit: None,
                    };
                    handler
                        .download(&ftp_url, opts)
                        .await
                        .map_err(|e| format!("FTP download failed: {}", e))?;
                    } else if protocol_upper_for_task == "BITTORRENT" {
                        // BitTorrent: download via bittorrent handler (magnet), then copy the completed file into
                        // the E2E output path so verification/polling is consistent across protocols.
                        let bt = {
                            let app_state = app_handle_for_task.state::<crate::AppState>();
                            let bt_opt = app_state
                                .bittorrent_handler
                                .lock()
                                .await
                                .as_ref()
                                .cloned();
                            bt_opt
                        };
                        let Some(bt) = bt else {
                            return Err("BitTorrent handler is not running".to_string());
                        };

                        let expected_info_hash = meta_for_task
                            .info_hash
                            .clone()
                            .unwrap_or_else(|| meta_for_task.merkle_root.clone())
                            .to_lowercase();

                        let start_timeout_ms: u64 = std::env::var("E2E_BITTORRENT_START_TIMEOUT_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(30_000);

                        let initial_peer = bt_seeder_ip_for_task
                            .as_deref()
                            .and_then(|s| s.parse::<std::net::IpAddr>().ok())
                            .zip(bt_seeder_port_for_task)
                            .map(|(ip, port)| std::net::SocketAddr::new(ip, port));
                        let initial_peer_dbg = initial_peer
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "<none>".to_string());
                        let bt_source = if torrent_base64_for_task.is_some() {
                            "torrent_bytes"
                        } else {
                            "magnet"
                        };

                        // Prefer .torrent bytes in real-network E2E to avoid magnet metadata exchange hangs.
                        let managed = if let Some(tb64) = torrent_base64_for_task.as_ref() {
                            let bytes = general_purpose::STANDARD
                                .decode(tb64)
                                .map_err(|e| format!("Invalid torrentBase64: {}", e))?;
                            // Sanity: torrent bytes must match the expected info_hash from metadata.
                            // If not, the peer will immediately reject the handshake and we'll see 0 progress forever.
                            if let Ok(ti) = torrent_from_bytes::<Vec<u8>>(&bytes) {
                                let parsed = hex::encode(ti.info_hash.0).to_lowercase();
                                if parsed != expected_info_hash {
                                    return Err(format!(
                                        "torrentBase64 info_hash mismatch: expected={} parsed={}",
                                        expected_info_hash, parsed
                                    ));
                                }
                            }
                            let peer = initial_peer;
                            // Optional preflight (OFF by default):
                            // Real-world seeders can behave defensively (close/ratelimit) when they see
                            // unexpected preflight traffic. Keep it opt-in to avoid increasing flakiness.
                            let do_tcp_preflight = std::env::var("E2E_BITTORRENT_PREFLIGHT_TCP")
                                .ok()
                                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                                .unwrap_or(false);
                            let do_handshake_preflight = std::env::var("E2E_BITTORRENT_PREFLIGHT_HANDSHAKE")
                                .ok()
                                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                                .unwrap_or(false);

                            if do_tcp_preflight {
                                if let Some(addr) = peer {
                                    match tokio::time::timeout(
                                        std::time::Duration::from_secs(2),
                                        TcpStream::connect(addr),
                                    )
                                    .await
                                    {
                                        Err(_) => {
                                            tracing::warn!(
                                                "BitTorrent preflight TCP connect timed out to {} (continuing anyway)",
                                                addr
                                            );
                                        }
                                        Ok(Err(e)) => {
                                            tracing::warn!(
                                                "BitTorrent preflight TCP connect failed to {}: {} (continuing anyway)",
                                                addr,
                                                e
                                            );
                                        }
                                        Ok(Ok(mut stream)) => {
                                            if do_handshake_preflight {
                                                let hs = bt_handshake_bytes(&expected_info_hash)?;
                                                if tokio::time::timeout(
                                                    std::time::Duration::from_secs(2),
                                                    stream.write_all(&hs),
                                                )
                                                .await
                                                .is_err()
                                                {
                                                    tracing::warn!(
                                                        "BitTorrent preflight handshake write timed out to {} (continuing anyway)",
                                                        addr
                                                    );
                                                } else {
                                                    let mut resp = [0u8; 68];
                                                    match tokio::time::timeout(
                                                        std::time::Duration::from_secs(2),
                                                        stream.read_exact(&mut resp),
                                                    )
                                                    .await
                                                    {
                                                        Err(_) => tracing::warn!(
                                                            "BitTorrent preflight handshake read timed out from {} (continuing anyway)",
                                                            addr
                                                        ),
                                                        Ok(Err(e)) => tracing::warn!(
                                                            "BitTorrent preflight handshake read failed from {}: {} (continuing anyway)",
                                                            addr,
                                                            e
                                                        ),
                                                        Ok(Ok(_)) => {}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(p) = peer {
                                tokio::time::timeout(
                                    std::time::Duration::from_millis(start_timeout_ms),
                                    bt.start_download_from_bytes_with_initial_peer(bytes, p),
                                )
                                .await
                                .map_err(|_| {
                                    format!(
                                        "BitTorrent start_download_from_bytes timed out after {}ms.",
                                        start_timeout_ms
                                    )
                                })?
                                .map_err(|e| format!("BitTorrent download failed to start: {}", e))?
                            } else {
                                tokio::time::timeout(
                                    std::time::Duration::from_millis(start_timeout_ms),
                                    bt.start_download_from_bytes(bytes),
                                )
                                .await
                                .map_err(|_| {
                                    format!(
                                        "BitTorrent start_download_from_bytes timed out after {}ms.",
                                        start_timeout_ms
                                    )
                                })?
                                .map_err(|e| format!("BitTorrent download failed to start: {}", e))?
                            }
                        } else {
                            let magnet = build_magnet_link(
                                &expected_info_hash,
                                Some(&meta_for_task.file_name),
                                meta_for_task.trackers.as_ref(),
                            );
                            // NOTE: bt.start_download can block while resolving the magnet / peers.
                            // Put an explicit cap so the test doesn't hit the global vitest 10min timeout.
                            let bt2 = bt.clone();
                            let fut = async move {
                                if let Some(p) = initial_peer {
                                    bt2.start_download_with_initial_peer(&magnet, p).await
                                } else {
                                    bt2.start_download(&magnet).await
                                }
                            };
                            tokio::time::timeout(
                                std::time::Duration::from_millis(start_timeout_ms),
                                fut,
                            )
                            .await
                            .map_err(|_| {
                                format!(
                                    "BitTorrent start_download timed out after {}ms (magnet resolve/peer connect).",
                                    start_timeout_ms
                                )
                            })?
                            .map_err(|e| format!("BitTorrent download failed to start: {}", e))?
                        };

                        let actual_info_hash = hex::encode(managed.info_hash().0);
                        let download_dir = bt
                            .get_torrent_folder(&actual_info_hash)
                            .await
                            .map_err(|e| format!("BitTorrent download folder unavailable: {}", e))?;

                        // Wait until the torrent is finished (fail-fast on explicit error / no-progress).
                        let bt_start = std::time::Instant::now();
                        // Avoid overly-aggressive "no progress" timers causing false negatives on real networks.
                        // If you really want < 60s, set E2E_BITTORRENT_ALLOW_SHORT_NO_PROGRESS=1.
                        let no_progress_grace_ms_raw: u64 = std::env::var("E2E_BITTORRENT_NO_PROGRESS_FAIL_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(60_000);
                        let allow_short = std::env::var("E2E_BITTORRENT_ALLOW_SHORT_NO_PROGRESS")
                            .ok()
                            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                            .unwrap_or(false);
                        let no_progress_grace_ms: u64 = if allow_short {
                            no_progress_grace_ms_raw
                        } else {
                            no_progress_grace_ms_raw.max(60_000)
                        };
                        let mut last_progress_bytes: u64 = 0;
                        let mut last_progress_at = std::time::Instant::now();
                        let mut peak = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
                        loop {
                            // Read stats directly from the managed torrent so we can access
                            // librqbit's aggregate peer state counters (queued/connecting/live/etc).
                            let s = managed.stats();
                            let state_str = s.state.to_string();
                            if state_str == "error" {
                                return Err(format!(
                                    "BitTorrent torrent entered error state (info_hash={}): error={:?}",
                                    actual_info_hash, s.error
                                ));
                            }
                            if s.finished {
                                break;
                            }

                            // Track peak peer counters so we don't miss short-lived dial attempts.
                            if let Some(l) = s.live.as_ref() {
                                let ps = &l.snapshot.peer_stats;
                                peak.0 = peak.0.max(ps.queued);
                                peak.1 = peak.1.max(ps.connecting);
                                peak.2 = peak.2.max(ps.live);
                                peak.3 = peak.3.max(ps.seen);
                                peak.4 = peak.4.max(ps.dead);
                                peak.5 = peak.5.max(ps.not_needed);
                            }

                            if s.progress_bytes > last_progress_bytes {
                                last_progress_bytes = s.progress_bytes;
                                last_progress_at = std::time::Instant::now();
                            } else if last_progress_bytes == 0
                                && last_progress_at.elapsed().as_millis() as u64 >= no_progress_grace_ms
                            {
                                let peer_diag = s
                                    .live
                                    .as_ref()
                                    .map(|l| {
                                        let ps = &l.snapshot.peer_stats;
                                        format!(
                                            "peer_stats={{queued={},connecting={},live={},seen={},dead={},not_needed={}}}",
                                            ps.queued, ps.connecting, ps.live, ps.seen, ps.dead, ps.not_needed
                                        )
                                    })
                                    .unwrap_or_else(|| "peer_stats=<none>".to_string());
                                let peak_diag = format!(
                                    "peer_peak={{queued={},connecting={},live={},seen={},dead={},not_needed={}}}",
                                    peak.0, peak.1, peak.2, peak.3, peak.4, peak.5
                                );
                                let live_snapshot_diag = s
                                    .live
                                    .as_ref()
                                    .map(|l| {
                                        format!(
                                            "live_snapshot={}",
                                            truncate_diag(format!("{:?}", l.snapshot), 2000)
                                        )
                                    })
                                    .unwrap_or_else(|| "live_snapshot=<none>".to_string());
                                let bt_fail_diag =
                                    bt_handshake_diag_on_fail(initial_peer, &expected_info_hash)
                                        .await;
                                return Err(format!(
                                    "BitTorrent made no download progress for {}ms (info_hash={}, source={}, state={}, finished={}, total_bytes={}, initial_peer={}, {}, {}, {}, {}).",
                                    no_progress_grace_ms, actual_info_hash, bt_source, state_str, s.finished, s.total_bytes, initial_peer_dbg, peer_diag, peak_diag, live_snapshot_diag, bt_fail_diag
                                ));
                            }
                            if bt_start.elapsed().as_millis() as u64 >= timeout_ms {
                                return Err(format!(
                                    "BitTorrent download did not complete within {}ms (info_hash={})",
                                    timeout_ms, actual_info_hash
                                ));
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }

                        // Find the downloaded file and copy it into the expected E2E output path.
                        let downloaded_path = find_file_recursive(
                            &download_dir,
                            &meta_for_task.file_name,
                            meta_for_task.file_size,
                        )
                        .await?;
                        if let Some(parent) = std::path::Path::new(&out_path_for_task).parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }
                        tokio::fs::copy(&downloaded_path, &out_path_for_task)
                            .await
                            .map_err(|e| format!("BitTorrent failed to copy output file: {}", e))?;
                } else {
                    return Err(format!(
                        "Unsupported protocol '{}' for async download task",
                        protocol_upper_for_task
                    ));
                }

                let start = std::time::Instant::now();
                let ftp_finalize_grace_ms: u64 = std::env::var("E2E_FTP_FINALIZE_GRACE_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30_000);
                let mut first_seen_incomplete_full_len_at: Option<std::time::Instant> = None;
                loop {
                    match tokio::fs::metadata(&out_path_for_task).await {
                        Ok(m) => {
                            let len = m.len();
                            if len == meta_for_task.file_size {
                                if protocol_upper_for_task == "FTP" {
                                    // For FTP, don't treat file length as completion because MultiSource finalization
                                    // pre-allocates the file length before chunk writes finish.
                                    match tokio::fs::read(&out_path_for_task).await {
                                        Ok(b) => {
                                            let mut h = sha2::Sha256::new();
                                            h.update(&b);
                                            let computed = format!("{:x}", h.finalize());
                                            if computed == meta_for_task.merkle_root {
                                                return Ok(len);
                                            }
                                            // The file may be mid-finalization; allow a short grace window.
                                            let now = std::time::Instant::now();
                                            if first_seen_incomplete_full_len_at.is_none() {
                                                first_seen_incomplete_full_len_at = Some(now);
                                            }
                                            if first_seen_incomplete_full_len_at
                                                .as_ref()
                                                .is_some_and(|t| t.elapsed().as_millis() as u64 >= ftp_finalize_grace_ms)
                                            {
                                                return Err(format!(
                                                    "FTP output file reached expected length but sha256 mismatch persisted for {}ms (expected merkle_root={}): {}",
                                                    ftp_finalize_grace_ms,
                                                    meta_for_task.merkle_root,
                                                    out_path_for_task
                                                ));
                                            }
                                        }
                                        Err(e) => {
                                            return Err(format!(
                                                "FTP output file exists but failed to read for verification: {}",
                                                e
                                            ));
                                        }
                                    }
                                } else {
                                    return Ok(len);
                                }
                            }
                        }
                        Err(_) => {}
                    }
                    if start.elapsed().as_millis() as u64 >= timeout_ms {
                        return Err(format!(
                            "Download output file missing or incomplete after {}ms (expected {} bytes): {}",
                            timeout_ms, meta_for_task.file_size, out_path_for_task
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
            .await;

            let mut jobs = downloads_map.lock().await;
            if let Some(job) = jobs.get_mut(&download_id_for_task) {
                match result {
                    Ok(bytes) => {
                        job.status = "success".to_string();
                        job.bytes = bytes;
                        if protocol_upper_for_task == "FTP" {
                            // Stronger verification for FTP: sha256(file) must match merkle_root (file hash).
                            match tokio::fs::read(&out_path_for_task).await {
                                Ok(b) => {
                                    let mut h = sha2::Sha256::new();
                                    h.update(&b);
                                    let computed = format!("{:x}", h.finalize());
                                    job.verified = computed == meta_for_task.merkle_root
                                        && bytes == meta_for_task.file_size;
                                }
                                Err(_) => {
                                    job.verified = false;
                                }
                            }
                        } else {
                            job.verified = bytes == meta_for_task.file_size;
                        }
                        job.error = None;
                    }
                    Err(e) => {
                        job.status = "failed".to_string();
                        job.bytes = 0;
                        job.verified = false;
                        job.error = Some(e);
                    }
                }
            }
        });

        return (
            StatusCode::ACCEPTED,
            Json(DownloadResponse {
                download_path: out_path_str,
                verified: false,
                bytes: 0,
                download_id: Some(download_id),
                status: Some("running".to_string()),
            }),
        )
            .into_response();
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(crate::http_server::ErrorResponse {
                error: format!(
                    "Unsupported protocol '{}'. Use HTTP, WebRTC, Bitswap, FTP, or BitTorrent.",
                    protocol_upper
                ),
            }),
        )
            .into_response();
    }

    // HTTP path returns synchronously.
    let bytes_len = match tokio::fs::metadata(&output_path).await {
        Ok(m) => m.len(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
                    error: format!("Download finished but output file is missing: {}", e),
                }),
            )
                .into_response();
        }
    };

    // Option1: sha256(file) == merkle_root (used as fileHash).
    let bytes = match tokio::fs::read(&output_path).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::http_server::ErrorResponse {
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
            bytes: bytes_len,
            download_id: None,
            status: Some("success".to_string()),
        }),
    )
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
            return (
                StatusCode::BAD_REQUEST,
                Json(crate::http_server::ErrorResponse {
                    error: "No active account. Set CHIRAL_PRIVATE_KEY and restart node."
                        .to_string(),
                }),
            )
                .into_response();
        }
    };
    let private_key = match app_state.active_account_private_key.lock().await.clone() {
        Some(k) => k,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(crate::http_server::ErrorResponse {
                    error: "No private key loaded. Set CHIRAL_PRIVATE_KEY and restart node."
                        .to_string(),
                }),
            )
                .into_response();
        }
    };

    match crate::ethereum::send_transaction(
        &account,
        &req.uploader_address,
        req.price,
        &private_key,
    )
    .await
    {
        Ok(tx_hash) => (StatusCode::OK, Json(PayResponse { tx_hash })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::http_server::ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

async fn api_tx_receipt(
    State(_state): State<Arc<E2eApiState>>,
    Json(req): Json<ReceiptRequest>,
) -> impl IntoResponse {
    match transaction_services::get_transaction_receipt(&req.tx_hash).await {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::http_server::ErrorResponse { error: e }),
        )
            .into_response(),
    }
}
