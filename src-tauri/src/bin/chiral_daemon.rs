use axum::{
    extract::{DefaultBodyLimit, Multipart, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use base64::Engine;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use chiral_network::dht;
use chiral_network::drive_api::DriveState;
use chiral_network::event_sink::EventSink;
use chiral_network::file_transfer::{FileTransferService, PendingTransfer};
use chiral_network::geth::{GethDownloader, GethProcess};
use chiral_network::hosting_server::{self, HostingServerState};
use chiral_network::rating_storage::RatingState;

#[derive(Parser, Debug)]
#[command(name = "chiral_daemon")]
#[command(about = "Chiral Network headless daemon")]
struct DaemonArgs {
    /// Local gateway port (Drive + Rating + Hosting + Headless routes)
    #[arg(long, default_value_t = 9419, env = "CHIRAL_DAEMON_PORT")]
    port: u16,

    /// Optional PID file path
    #[arg(long, env = "CHIRAL_DAEMON_PID_FILE")]
    pid_file: Option<PathBuf>,

    /// Auto-start DHT on startup (required for P2P networking)
    #[arg(long, env = "CHIRAL_AUTO_START_DHT", default_value_t = false)]
    auto_start_dht: bool,

    /// Auto-start Geth node on startup
    #[arg(long, env = "CHIRAL_AUTO_START_GETH", default_value_t = false)]
    auto_start_geth: bool,

    /// Auto-start mining on startup (implies --auto-start-geth and --auto-start-dht)
    #[arg(long, env = "CHIRAL_AUTO_MINE", default_value_t = false)]
    auto_mine: bool,

    /// Miner address (wallet) for mining rewards
    #[arg(long, env = "CHIRAL_MINER_ADDRESS")]
    miner_address: Option<String>,

    /// Number of CPU mining threads (default: 1)
    #[arg(long, env = "CHIRAL_MINING_THREADS", default_value_t = 1)]
    mining_threads: u32,

    /// Pin the libp2p TCP port. Default: OS-assigned (0). Set this on k3s/Docker
    /// nodes to match the externally exposed NodePort, otherwise the random port
    /// the daemon picks won't be reachable and stale multiaddrs end up in the DHT.
    #[arg(long, env = "CHIRAL_P2P_PORT")]
    p2p_port: Option<u16>,
}

#[derive(Clone, serde::Serialize)]
struct WalletInfo {
    address: String,
    private_key: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRegistryEntry {
    peer_id: String,
    wallet_address: String,
    updated_at: u64,
}

#[derive(Clone)]
struct HeadlessRuntimeState {
    dht: Arc<Mutex<Option<Arc<dht::DhtService>>>>,
    file_transfer: Arc<Mutex<FileTransferService>>,
    download_directory: dht::DownloadDirectoryRef,
    download_credentials: dht::DownloadCredentialsMap,
    geth: Arc<Mutex<GethProcess>>,
    wallet: Arc<Mutex<Option<WalletInfo>>>,
}

impl HeadlessRuntimeState {
    fn new() -> Self {
        Self {
            dht: Arc::new(Mutex::new(None)),
            file_transfer: Arc::new(Mutex::new(FileTransferService::new())),
            download_directory: Arc::new(Mutex::new(None)),
            download_credentials: Arc::new(Mutex::new(HashMap::new())),
            geth: Arc::new(Mutex::new(GethProcess::new())),
            wallet: Arc::new(Mutex::new(None)),
        }
    }

    async fn dht_service(&self) -> Option<Arc<dht::DhtService>> {
        self.dht.lock().await.clone()
    }
}

fn default_data_dir() -> PathBuf {
    chiral_network::network::data_dir()
}

fn default_pid_file() -> PathBuf {
    default_data_dir()
        .join("headless")
        .join("chiral-daemon.pid")
}

fn write_pid_file(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create PID directory {}: {}", parent.display(), e))?;
    }
    let pid = std::process::id();
    std::fs::write(path, pid.to_string())
        .map_err(|e| format!("Failed to write PID file {}: {}", path.display(), e))?;
    Ok(())
}

fn remove_pid_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Publish a lightweight host advertisement so wallet identity is discoverable by peers.
async fn auto_publish_wallet_advertisement(
    state: &HeadlessRuntimeState,
    wallet_address: &str,
) -> Result<(), String> {
    let Some(dht) = state.dht_service().await else {
        return Err("DHT not running".to_string());
    };

    // Give the swarm a short moment to expose peer ID after start.
    let mut peer_id: Option<String> = None;
    for _ in 0..10 {
        peer_id = dht.get_peer_id().await;
        if peer_id.as_deref().is_some_and(|v| !v.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let peer_id = peer_id.ok_or("Peer ID not available".to_string())?;

    let now = now_secs();
    // Match marketplace advertisement shape so existing browse/discovery code can consume it.
    let ad = json!({
        "peerId": peer_id,
        "walletAddress": wallet_address,
        "maxStorageBytes": 10_u64 * 1024 * 1024 * 1024, // 10 GB default
        "usedStorageBytes": 0_u64,
        "pricePerMbPerDayWei": "1000000000000000", // 0.001 CHI/MB/day
        "minDepositWei": "100000000000000000", // 0.1 CHI
        "uptimePercent": 100_u64,
        "publishedAt": now,
        "lastHeartbeatAt": now,
        "autoAdvertisedWallet": true
    });

    let ad_json = serde_json::to_string(&ad)
        .map_err(|e| format!("Failed to serialize wallet advertisement: {}", e))?;
    let host_key = format!("chiral_host_{}", peer_id);
    dht.put_dht_value(host_key, ad_json).await?;

    // Update host registry so other peers discover this wallet->peer mapping.
    let registry_key = "chiral_host_registry".to_string();
    let mut registry: Vec<HostRegistryEntry> = match dht.get_dht_value(registry_key.clone()).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    registry.retain(|e| e.peer_id != peer_id);
    registry.push(HostRegistryEntry {
        peer_id,
        wallet_address: wallet_address.to_string(),
        updated_at: now,
    });

    let registry_json = serde_json::to_string(&registry)
        .map_err(|e| format!("Failed to serialize host registry: {}", e))?;
    dht.put_dht_value(registry_key, registry_json).await
}

fn read_geth_log(lines: Option<usize>) -> Result<String, String> {
    let data_dir = chiral_network::network::data_dir().join("geth");
    let log_path = data_dir.join("geth.log");
    if !log_path.exists() {
        return Ok("No geth.log found".to_string());
    }

    let contents = std::fs::read_to_string(&log_path)
        .map_err(|e| format!("Failed to read geth.log: {}", e))?;
    let max_lines = lines.unwrap_or(100);
    let all_lines: Vec<&str> = contents.lines().collect();
    let start = if all_lines.len() > max_lines {
        all_lines.len() - max_lines
    } else {
        0
    };
    Ok(all_lines[start..].join("\n"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyValueRequest {
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyRequest {
    key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerRequest {
    peer_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EchoRequest {
    peer_id: String,
    payload_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestFileRequest {
    peer_id: String,
    file_hash: String,
    request_id: String,
    #[serde(default)]
    multiaddrs: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendFileRequest {
    peer_id: String,
    transfer_id: String,
    file_name: String,
    file_path: String,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    sender_wallet: String,
    #[serde(default)]
    file_hash: String,
    file_size: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterSharedFileRequest {
    file_hash: String,
    file_path: String,
    file_name: String,
    file_size: u64,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    wallet_address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnregisterSharedFileRequest {
    file_hash: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptTransferRequest {
    transfer_id: String,
    download_dir: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartGethRequest {
    miner_address: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartMiningRequest {
    threads: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MinerAddressRequest {
    address: String,
}

#[derive(Deserialize)]
struct LogsQuery {
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct BlocksQuery {
    max: Option<u64>,
}

/// GET /api/health — liveness probe (always returns 200 if server is up)
async fn health_check() -> Response {
    Json(json!({ "status": "ok" })).into_response()
}

/// GET /api/ready — readiness probe (checks if DHT and Geth are running)
async fn readiness_check(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let dht_running = state.dht_service().await.is_some();
    let geth_running = {
        let mut geth = state.geth.lock().await;
        geth.get_status().await.map(|s| s.running).unwrap_or(false)
    };
    let ready = dht_running; // DHT is the minimum requirement for readiness

    let status_code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(json!({
            "ready": ready,
            "dht": dht_running,
            "geth": geth_running,
        })),
    )
        .into_response()
}

async fn runtime_status(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let dht_service = state.dht_service().await;
    let dht_running = dht_service.is_some();
    let peer_id = if let Some(dht) = dht_service {
        dht.get_peer_id().await
    } else {
        None
    };

    let mut geth = state.geth.lock().await;
    let geth_status = geth.get_status().await.ok();

    Json(json!({
        "dhtRunning": dht_running,
        "peerId": peer_id,
        "gethStatus": geth_status,
    }))
    .into_response()
}

async fn dht_start(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let mut guard = state.dht.lock().await;
    if guard.is_some() {
        return Json(json!({ "status": "already_running" })).into_response();
    }

    let svc = Arc::new(dht::DhtService::new(
        Arc::clone(&state.file_transfer),
        Arc::clone(&state.download_directory),
        Arc::clone(&state.download_credentials),
    ));

    match svc.start_headless().await {
        Ok(message) => {
            *guard = Some(svc);
            drop(guard);

            let wallet_address = {
                let wallet = state.wallet.lock().await;
                wallet.as_ref().map(|w| w.address.clone())
            };
            if let Some(addr) = wallet_address {
                if let Err(err) = auto_publish_wallet_advertisement(state.as_ref(), &addr).await {
                    eprintln!(
                        "[AUTO] Wallet advertisement after DHT start failed: {}",
                        err
                    );
                }
            }

            Json(json!({ "status": "started", "message": message })).into_response()
        }
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_stop(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let mut guard = state.dht.lock().await;
    let Some(svc) = guard.take() else {
        return Json(json!({ "status": "stopped" })).into_response();
    };

    match svc.stop().await {
        Ok(()) => Json(json!({ "status": "stopped" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_health(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let Some(svc) = state.dht_service().await else {
        return Json(json!({
            "running": false,
            "peerId": null,
            "listeningAddresses": [],
            "connectedPeerCount": 0,
            "kademliaPeers": 0,
            "bootstrapNodes": [],
            "sharedFiles": 0,
            "protocols": []
        }))
        .into_response();
    };

    Json(svc.get_health().await).into_response()
}

async fn dht_peers(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let Some(svc) = state.dht_service().await else {
        return Json(Vec::<dht::PeerInfo>::new()).into_response();
    };
    Json(svc.get_peers().await).into_response()
}

async fn dht_peer_id(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let Some(svc) = state.dht_service().await else {
        return Json(json!({ "peerId": null })).into_response();
    };
    Json(json!({ "peerId": svc.get_peer_id().await })).into_response()
}

async fn dht_put(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<KeyValueRequest>,
) -> Response {
    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    match svc.put_dht_value(req.key, req.value).await {
        Ok(()) => Json(json!({ "status": "ok" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_get(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<KeyRequest>,
) -> Response {
    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    match svc.get_dht_value(req.key).await {
        Ok(value) => Json(json!({ "value": value })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_ping(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<PeerRequest>,
) -> Response {
    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    match svc.ping_peer_headless(req.peer_id).await {
        Ok(message) => Json(json!({ "status": "ok", "message": message })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_echo(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<EchoRequest>,
) -> Response {
    let payload = match base64::engine::general_purpose::STANDARD.decode(req.payload_base64) {
        Ok(v) => v,
        Err(e) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid base64 payload: {}", e),
            )
        }
    };

    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    match svc.echo(req.peer_id, payload).await {
        Ok(resp) => Json(json!({
            "payloadBase64": base64::engine::general_purpose::STANDARD.encode(resp)
        }))
        .into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_request_file(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<RequestFileRequest>,
) -> Response {
    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    match svc
        .request_file(req.peer_id, req.file_hash, req.request_id, req.multiaddrs)
        .await
    {
        Ok(()) => Json(json!({ "status": "ok" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_send_file(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<SendFileRequest>,
) -> Response {
    let data = match std::fs::read(&req.file_path) {
        Ok(v) => v,
        Err(e) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                format!("Failed to read file {}: {}", req.file_path, e),
            )
        }
    };

    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    let file_size = req.file_size.unwrap_or(data.len() as u64);
    match svc
        .send_file(
            req.peer_id,
            req.transfer_id,
            req.file_name,
            data,
            req.price_wei,
            req.sender_wallet,
            req.file_hash,
            file_size,
        )
        .await
    {
        Ok(()) => Json(json!({ "status": "ok" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn dht_listening_addresses(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    Json(json!({ "addresses": svc.get_listening_addresses().await })).into_response()
}

async fn dht_register_shared_file(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<RegisterSharedFileRequest>,
) -> Response {
    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    let price_wei = req.price_wei.parse::<u128>().unwrap_or(0);
    svc.register_shared_file(
        req.file_hash,
        req.file_path,
        req.file_name,
        req.file_size,
        price_wei,
        req.wallet_address,
    )
    .await;

    Json(json!({ "status": "ok" })).into_response()
}

async fn dht_unregister_shared_file(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<UnregisterSharedFileRequest>,
) -> Response {
    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    svc.unregister_shared_file(&req.file_hash).await;
    Json(json!({ "status": "ok" })).into_response()
}

async fn drop_inbox(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let svc = state.file_transfer.lock().await;
    let pending: Vec<PendingTransfer> = svc.get_pending_incoming().await;
    Json(pending).into_response()
}

async fn drop_outgoing(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let svc = state.file_transfer.lock().await;
    let pending: Vec<PendingTransfer> = svc.get_pending_outgoing().await;
    Json(pending).into_response()
}

async fn drop_accept(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<AcceptTransferRequest>,
) -> Response {
    let svc = state.file_transfer.lock().await;
    match svc
        .accept_transfer(EventSink::noop(), req.transfer_id, req.download_dir)
        .await
    {
        Ok(path) => Json(json!({ "status": "accepted", "path": path })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn drop_decline(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<KeyRequest>,
) -> Response {
    let svc = state.file_transfer.lock().await;
    match svc.decline_transfer(req.key).await {
        Ok(()) => Json(json!({ "status": "declined" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn geth_install() -> Response {
    let downloader = GethDownloader::new();
    match downloader.download_geth(|_progress| {}).await {
        Ok(()) => Json(json!({ "status": "installed" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn geth_start(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<StartGethRequest>,
) -> Response {
    let mut geth = state.geth.lock().await;
    match geth.start(req.miner_address.as_deref()).await {
        Ok(()) => Json(json!({ "status": "started" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn geth_stop(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let mut geth = state.geth.lock().await;
    match geth.stop() {
        Ok(()) => Json(json!({ "status": "stopped" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn geth_status(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let mut geth = state.geth.lock().await;
    match geth.get_status().await {
        Ok(status) => Json(status).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn geth_logs(Query(q): Query<LogsQuery>) -> Response {
    match read_geth_log(q.lines) {
        Ok(logs) => Json(json!({ "logs": logs })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn mining_start(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<StartMiningRequest>,
) -> Response {
    let mut geth = state.geth.lock().await;
    match geth.start_mining(req.threads.unwrap_or(1)).await {
        Ok(()) => Json(json!({ "status": "started" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn mining_stop(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let mut geth = state.geth.lock().await;
    match geth.stop_mining().await {
        Ok(()) => Json(json!({ "status": "stopped" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn mining_status(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let mut geth = state.geth.lock().await;
    match geth.get_mining_status().await {
        Ok(status) => Json(status).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn mining_blocks(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Query(q): Query<BlocksQuery>,
) -> Response {
    let geth = state.geth.lock().await;
    match geth.get_mined_blocks(q.max.unwrap_or(500)).await {
        Ok(blocks) => Json(blocks).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn set_miner_address(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<MinerAddressRequest>,
) -> Response {
    let geth = state.geth.lock().await;
    match geth.set_miner_address(&req.address).await {
        Ok(()) => Json(json!({ "status": "ok" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

// ---------------------------------------------------------------------------
// Wallet endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportWalletRequest {
    private_key: String,
}

/// POST /api/headless/wallet/create — generate a new wallet (random private key)
async fn wallet_create(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    // Generate a random 32-byte private key
    let mut rng_bytes = [0u8; 32];
    use std::io::Read;
    match std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut rng_bytes).map(|_| ()))
    {
        Ok(()) => {}
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("RNG failed: {}", e),
            )
        }
    }
    let private_key_hex = hex::encode(rng_bytes);

    // Derive address from private key using secp256k1
    let secp = secp256k1::Secp256k1::new();
    let secret_key = match secp256k1::SecretKey::from_slice(&rng_bytes) {
        Ok(k) => k,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Invalid key: {}", e),
            )
        }
    };
    let public_key = secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
    let pub_bytes = public_key.serialize_uncompressed();
    // Address = last 20 bytes of keccak256(pub_key_bytes[1..])
    use tiny_keccak::{Hasher, Keccak};
    let mut keccak = Keccak::v256();
    keccak.update(&pub_bytes[1..]);
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);
    let address = format!("0x{}", hex::encode(&hash[12..]));

    let wallet = WalletInfo {
        address: address.clone(),
        private_key: format!("0x{}", private_key_hex),
    };
    *state.wallet.lock().await = Some(wallet.clone());

    let (wallet_advertised, wallet_advertise_error) =
        match auto_publish_wallet_advertisement(state.as_ref(), &wallet.address).await {
            Ok(()) => (true, None),
            Err(err) => {
                eprintln!(
                    "[AUTO] Wallet advertisement after wallet create failed: {}",
                    err
                );
                (false, Some(err))
            }
        };

    Json(json!({
        "address": wallet.address,
        "privateKey": wallet.private_key,
        "walletAdvertised": wallet_advertised,
        "walletAdvertiseError": wallet_advertise_error,
    }))
    .into_response()
}

/// POST /api/headless/wallet/import — import wallet from private key
async fn wallet_import(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<ImportWalletRequest>,
) -> Response {
    let pk_hex = req.private_key.trim().trim_start_matches("0x");
    let pk_bytes = match hex::decode(pk_hex) {
        Ok(b) if b.len() == 32 => b,
        _ => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "Invalid private key (must be 32 bytes hex)",
            )
        }
    };

    let secp = secp256k1::Secp256k1::new();
    let secret_key = match secp256k1::SecretKey::from_slice(&pk_bytes) {
        Ok(k) => k,
        Err(e) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid private key: {}", e),
            )
        }
    };
    let public_key = secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
    let pub_bytes = public_key.serialize_uncompressed();
    use tiny_keccak::{Hasher, Keccak};
    let mut keccak = Keccak::v256();
    keccak.update(&pub_bytes[1..]);
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);
    let address = format!("0x{}", hex::encode(&hash[12..]));

    let wallet = WalletInfo {
        address: address.clone(),
        private_key: format!("0x{}", pk_hex),
    };
    *state.wallet.lock().await = Some(wallet.clone());

    let (wallet_advertised, wallet_advertise_error) =
        match auto_publish_wallet_advertisement(state.as_ref(), &wallet.address).await {
            Ok(()) => (true, None),
            Err(err) => {
                eprintln!(
                    "[AUTO] Wallet advertisement after wallet import failed: {}",
                    err
                );
                (false, Some(err))
            }
        };

    Json(json!({
        "address": wallet.address,
        "privateKey": wallet.private_key,
        "walletAdvertised": wallet_advertised,
        "walletAdvertiseError": wallet_advertise_error,
    }))
    .into_response()
}

/// GET /api/headless/wallet — get current wallet info
async fn wallet_show(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let guard = state.wallet.lock().await;
    match &*guard {
        Some(w) => Json(json!({
            "address": w.address,
            "privateKey": w.private_key,
        }))
        .into_response(),
        None => json_error(StatusCode::NOT_FOUND, "No wallet loaded"),
    }
}

// ---- Wallet balance/transaction endpoints ----

async fn wallet_balance(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let address = body["address"].as_str().unwrap_or("").to_string();
    if address.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "address required");
    }
    let endpoint = chiral_network::geth::effective_rpc_endpoint();
    match chiral_network::wallet::get_balance(&endpoint, &address).await {
        Ok(result) => Json(json!(result)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_send(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let from = body["from"].as_str().unwrap_or("").to_string();
    let to = body["to"].as_str().unwrap_or("").to_string();
    let amount = body["amount"].as_str().unwrap_or("").to_string();
    let private_key = body["privateKey"].as_str().unwrap_or("").to_string();

    if from.is_empty() || to.is_empty() || amount.is_empty() || private_key.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "from, to, amount, privateKey required");
    }

    let endpoint = chiral_network::geth::effective_rpc_endpoint();
    match chiral_network::wallet::send_transaction(&endpoint, &from, &to, &amount, &private_key).await {
        Ok(result) => Json(json!(result)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_receipt(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let tx_hash = body["txHash"].as_str().unwrap_or("").to_string();
    if tx_hash.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "txHash required");
    }
    let endpoint = chiral_network::geth::effective_rpc_endpoint();
    match chiral_network::wallet::get_receipt(&endpoint, &tx_hash).await {
        Ok(Some(receipt)) => Json(json!({"receipt": receipt})).into_response(),
        Ok(None) => Json(json!({"receipt": null})).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_history(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let address = body["address"].as_str().unwrap_or("").to_string();
    if address.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "address required");
    }
    let endpoint = chiral_network::geth::effective_rpc_endpoint();
    let metadata = chiral_network::wallet::load_tx_metadata();
    match chiral_network::wallet::get_transaction_history(&endpoint, &address, &metadata).await {
        Ok(result) => Json(json!(result)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_faucet(
    Json(body): Json<serde_json::Value>,
) -> Response {
    let address = body["address"].as_str().unwrap_or("").to_string();
    if address.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "address required");
    }
    match chiral_network::wallet::request_faucet(&address).await {
        Ok(result) => Json(json!(result)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_chain_id() -> Response {
    Json(json!({"chainId": chiral_network::geth::chain_id()})).into_response()
}

// ---- File search endpoint ----

async fn file_search(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let file_hash = body["fileHash"].as_str().unwrap_or("").to_string();
    if file_hash.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "fileHash required");
    }
    let dht = match state.dht_service().await {
        Some(d) => d,
        None => return json_error(StatusCode::SERVICE_UNAVAILABLE, "DHT not running"),
    };
    let dht_key = format!("chiral_file_{}", file_hash);
    match dht.get_dht_value(dht_key).await {
        Ok(Some(json_str)) => {
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(metadata) => Json(json!({"found": true, "metadata": metadata})).into_response(),
                Err(_) => Json(json!({"found": true, "raw": json_str})).into_response(),
            }
        }
        Ok(None) => Json(json!({"found": false})).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

// ---- Hosting advertisement endpoints ----

async fn hosting_publish_ad(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let dht = match state.dht_service().await {
        Some(d) => d,
        None => return json_error(StatusCode::SERVICE_UNAVAILABLE, "DHT not running"),
    };
    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    if peer_id.is_empty() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Peer ID not available");
    }
    let mut ad = body.clone();
    ad["peerId"] = serde_json::Value::String(peer_id.clone());
    let ad_json = serde_json::to_string(&ad).unwrap_or_default();
    let wallet_address = ad["walletAddress"].as_str().unwrap_or("").to_string();

    // Store individual ad
    let host_key = format!("chiral_host_{}", peer_id);
    if let Err(e) = dht.put_dht_value(host_key, ad_json).await {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    // Update registry
    let registry_key = "chiral_host_registry".to_string();
    let mut registry: Vec<serde_json::Value> = match dht.get_dht_value(registry_key.clone()).await {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
        _ => Vec::new(),
    };
    registry.retain(|e| e["peerId"].as_str() != Some(&peer_id));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    registry.push(json!({"peerId": peer_id, "walletAddress": wallet_address, "updatedAt": now}));
    let registry_json = serde_json::to_string(&registry).unwrap_or_default();
    match dht.put_dht_value(registry_key, registry_json).await {
        Ok(_) => Json(json!({"status": "published"})).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn hosting_get_registry(
    State(state): State<Arc<HeadlessRuntimeState>>,
) -> Response {
    let dht = match state.dht_service().await {
        Some(d) => d,
        None => return json_error(StatusCode::SERVICE_UNAVAILABLE, "DHT not running"),
    };
    match dht.get_dht_value("chiral_host_registry".to_string()).await {
        Ok(Some(json)) => Json(json!({"registry": serde_json::from_str::<serde_json::Value>(&json).unwrap_or(json!([]))})).into_response(),
        Ok(None) => Json(json!({"registry": []})).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn bootstrap_health(
    State(state): State<Arc<HeadlessRuntimeState>>,
) -> Response {
    let report = chiral_network::geth_bootstrap::check_all_nodes().await;
    Json(json!(report)).into_response()
}

// ============================================================================
// CDN endpoints — always-on file hosting service
// ============================================================================

/// CDN storage directory.
fn cdn_storage_dir() -> PathBuf {
    default_data_dir().join("cdn")
}

/// CDN metadata file (tracks all hosted files and their owners).
fn cdn_metadata_path() -> PathBuf {
    default_data_dir().join("cdn_registry.json")
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CdnFileEntry {
    file_hash: String,
    file_name: String,
    file_size: u64,
    owner_wallet: String,
    price_chi_per_month: String,
    #[serde(default)]
    download_price_chi: String,
    payment_tx: String,
    uploaded_at: u64,
    expires_at: u64,
}

// ---- CDN dynamic pricing engine ----

/// Minimum floor price: 0.001 CHI per MB per month.
const CDN_FLOOR_PRICE_WEI_PER_MB_MONTH: u128 = 1_000_000_000_000_000; // 0.001 CHI

/// CDN charges a 1.2x premium over median peer price for guaranteed uptime.
const CDN_UPTIME_PREMIUM: f64 = 1.2;

/// Calculate the current CDN storage price based on the peer hosting marketplace.
/// Returns price in wei per MB per month.
async fn calculate_cdn_price(state: &HeadlessRuntimeState) -> u128 {
    let dht = match state.dht_service().await {
        Some(d) => d,
        None => return CDN_FLOOR_PRICE_WEI_PER_MB_MONTH,
    };

    // Fetch the host registry from DHT
    let registry_json = match dht.get_dht_value("chiral_host_registry".to_string()).await {
        Ok(Some(json)) => json,
        _ => return CDN_FLOOR_PRICE_WEI_PER_MB_MONTH,
    };

    let registry: Vec<serde_json::Value> = serde_json::from_str(&registry_json).unwrap_or_default();
    if registry.is_empty() {
        return CDN_FLOOR_PRICE_WEI_PER_MB_MONTH;
    }

    // Fetch each host's advertisement to get their price
    let mut peer_prices: Vec<u128> = Vec::new();
    for entry in &registry {
        let peer_id = entry["peerId"].as_str().unwrap_or("");
        if peer_id.is_empty() { continue; }

        let host_key = format!("chiral_host_{}", peer_id);
        if let Ok(Some(ad_json)) = dht.get_dht_value(host_key).await {
            if let Ok(ad) = serde_json::from_str::<serde_json::Value>(&ad_json) {
                let price_str = ad["pricePerMbPerDayWei"].as_str().unwrap_or("0");
                if let Ok(price_per_day) = price_str.parse::<u128>() {
                    if price_per_day > 0 {
                        // Convert daily price to monthly (× 30)
                        peer_prices.push(price_per_day * 30);
                    }
                }
            }
        }
    }

    if peer_prices.is_empty() {
        return CDN_FLOOR_PRICE_WEI_PER_MB_MONTH;
    }

    // Calculate median peer price
    peer_prices.sort();
    let median = peer_prices[peer_prices.len() / 2];

    // CDN price = max(floor, median × premium)
    let cdn_price = (median as f64 * CDN_UPTIME_PREMIUM) as u128;
    std::cmp::max(CDN_FLOOR_PRICE_WEI_PER_MB_MONTH, cdn_price)
}

/// Format wei to CHI string for display.
fn wei_to_chi_display(wei: u128) -> String {
    let chi = wei as f64 / 1e18;
    if chi < 0.000001 {
        format!("{:.18}", chi).trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        format!("{:.6}", chi)
    }
}

fn load_cdn_registry() -> Vec<CdnFileEntry> {
    let path = cdn_metadata_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn save_cdn_registry(entries: &[CdnFileEntry]) {
    let path = cdn_metadata_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(path, json);
    }
}


/// POST /api/cdn/upload — Upload a file to the CDN.
/// `multipart/form-data` with a single "file" field (raw bytes + filename).
/// Metadata is passed via headers:
///   X-Payment-Tx        — payment tx hash (required)
///   X-Owner-Wallet      — owner's wallet address (required)
///   X-Duration-Days     — hosting duration in days (default: 30)
///   X-Download-Price-Chi — price downloaders pay (CHI, default: "0")
///
/// Payment verification (wait for tx to mine, ~15s+ block time) runs in
/// parallel with the multipart body upload, so the client doesn't pay for
/// both serially.
async fn cdn_upload(
    State(state): State<Arc<HeadlessRuntimeState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    fn header_str(h: &HeaderMap, key: &str) -> String {
        h.get(key).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
    }

    let payment_tx = header_str(&headers, "X-Payment-Tx");
    let owner_wallet = header_str(&headers, "X-Owner-Wallet");
    let duration_days: u64 = header_str(&headers, "X-Duration-Days").parse().unwrap_or(30);
    let download_price_chi = {
        let s = header_str(&headers, "X-Download-Price-Chi");
        if s.is_empty() { "0".to_string() } else { s }
    };

    if payment_tx.is_empty() || owner_wallet.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "X-Payment-Tx and X-Owner-Wallet headers required");
    }

    let cdn_address = {
        let cdn_wallet = state.wallet.lock().await;
        cdn_wallet.as_ref().map(|w| w.address.clone()).unwrap_or_default()
    };
    if cdn_address.is_empty() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "CDN wallet not configured");
    }

    // Kick off block-wait in parallel with body upload. The tx hash is known
    // from the header, so we don't need to wait for the body to start polling.
    let mined_task = {
        let tx = payment_tx.clone();
        tokio::spawn(async move { chiral_network::wallet::wait_for_tx_mined(&tx).await })
    };

    // Read the file field from the multipart body
    let mut file_name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return json_error(StatusCode::BAD_REQUEST, &format!("Multipart error: {}", e)),
        };
        if field.name() == Some("file") {
            file_name = field.file_name().map(|s| s.to_string());
            match field.bytes().await {
                Ok(b) => file_data = Some(b.to_vec()),
                Err(e) => return json_error(StatusCode::BAD_REQUEST, &format!("Failed to read file: {}", e)),
            }
        }
    }

    let file_name = match file_name.filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => return json_error(StatusCode::BAD_REQUEST, "Multipart file field missing or unnamed"),
    };
    let file_data = match file_data {
        Some(d) if !d.is_empty() => d,
        _ => return json_error(StatusCode::BAD_REQUEST, "Empty file"),
    };

    if file_data.len() > 500 * 1024 * 1024 {
        return json_error(StatusCode::BAD_REQUEST, "File exceeds 500MB limit");
    }

    // Now that we know file size, compute required cost.
    let price_per_mb_month_wei = calculate_cdn_price(&state).await;
    let file_mb = (file_data.len() as f64) / (1024.0 * 1024.0);
    let months = (duration_days as f64) / 30.0;
    let required_cost_wei = (price_per_mb_month_wei as f64 * file_mb * months) as u128;
    // 5% tolerance for CHI→wei rounding during send_transaction
    let min_accepted_wei = required_cost_wei * 95 / 100;

    println!("[CDN] Verifying payment: tx={} from={} to={} required_wei={} min_accepted={}",
        payment_tx, owner_wallet, cdn_address, required_cost_wei, min_accepted_wei);

    // Join the parallel mining wait
    let mined = match mined_task.await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            println!("[CDN] Payment verification ERROR: tx={} err={}", payment_tx, e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Payment verification failed: {}", e));
        }
        Err(e) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Payment verification task failed: {}", e));
        }
    };
    if !mined {
        println!("[CDN] Payment not mined in time: tx={}", payment_tx);
        return json_error(StatusCode::PAYMENT_REQUIRED,
            &format!("Payment not confirmed in time. Tx: {}", payment_tx));
    }

    // Tx is mined; now check from/to/amount with the actual required cost.
    match chiral_network::wallet::verify_tx_details(&payment_tx, &owner_wallet, &cdn_address, min_accepted_wei).await {
        Ok(true) => println!("[CDN] Payment verified OK: tx={}", payment_tx),
        Ok(false) => {
            println!("[CDN] Payment details mismatch: tx={} (wrong recipient or insufficient amount)", payment_tx);
            return json_error(StatusCode::PAYMENT_REQUIRED,
                &format!("Payment details mismatch. Tx: {}. Expected: from={}, to={}, amount>={} wei",
                    payment_tx, owner_wallet, cdn_address, min_accepted_wei));
        }
        Err(e) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Payment detail check failed: {}", e));
        }
    }

    // Compute SHA-256 hash
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let file_hash = hex::encode(hasher.finalize());

    // Store file on disk
    let storage_dir = cdn_storage_dir();
    let _ = std::fs::create_dir_all(&storage_dir);
    let file_path = storage_dir.join(&file_hash);
    if let Err(e) = std::fs::write(&file_path, &file_data) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to store file: {}", e));
    }

    let file_size = file_data.len() as u64;
    let now = now_secs();
    let expires = now + duration_days * 86400;

    // Calculate dynamic CDN price based on peer marketplace
    let price_per_mb_month_wei = calculate_cdn_price(&state).await;
    let file_mb = (file_size as f64) / (1024.0 * 1024.0);
    let months = (duration_days as f64) / 30.0;
    let total_cost_wei = (price_per_mb_month_wei as f64 * file_mb * months) as u128;
    let total_cost_chi = wei_to_chi_display(total_cost_wei);
    let price_per_mb_chi = wei_to_chi_display(price_per_mb_month_wei);

    println!("[CDN] Price: {} CHI/MB/month, total: {} CHI for {:.2} MB × {:.1} months",
        price_per_mb_chi, total_cost_chi, file_mb, months);

    // Register as seeder in DHT
    let dht = match state.dht_service().await {
        Some(d) => d,
        None => return json_error(StatusCode::SERVICE_UNAVAILABLE, "DHT not running"),
    };

    // Parse download price (CHI → wei)
    let download_price_wei = if download_price_chi.is_empty() || download_price_chi == "0" {
        0u128
    } else {
        chiral_network::wallet::parse_chi_to_wei(&download_price_chi).unwrap_or(0)
    };
    let download_price_wei_str = download_price_wei.to_string();

    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    dht.register_shared_file(
        file_hash.clone(),
        file_path.to_string_lossy().to_string(),
        file_name.clone(),
        file_size,
        download_price_wei,
        owner_wallet.clone(),
    ).await;

    // Publish file metadata to DHT with the download price
    let our_addrs = dht.get_listening_addresses().await;
    let metadata = json!({
        "hash": file_hash,
        "fileName": file_name,
        "fileSize": file_size,
        "protocol": "WebRTC",
        "createdAt": now,
        "peerId": peer_id,
        "priceWei": download_price_wei_str,
        "walletAddress": owner_wallet,
        "seeders": [{
            "peerId": peer_id,
            "priceWei": download_price_wei_str,
            "walletAddress": owner_wallet,
            "multiaddrs": our_addrs,
            "signature": ""
        }],
        "publisherSignature": ""
    });

    let dht_key = format!("chiral_file_{}", file_hash);
    if let Err(e) = dht.put_dht_value(dht_key, serde_json::to_string(&metadata).unwrap_or_default()).await {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("DHT publish failed: {}", e));
    }

    // Save to CDN registry
    let mut registry = load_cdn_registry();
    registry.retain(|e| e.file_hash != file_hash);
    registry.push(CdnFileEntry {
        file_hash: file_hash.clone(),
        file_name: file_name.clone(),
        file_size,
        owner_wallet: owner_wallet.clone(),
        price_chi_per_month: price_per_mb_chi.clone(),
        download_price_chi: download_price_chi.clone(),
        payment_tx: payment_tx.clone(),
        uploaded_at: now,
        expires_at: expires,
    });
    save_cdn_registry(&registry);

    println!("[CDN] Uploaded: {} ({} bytes, hash: {}, owner: {}, cost: {} CHI, expires: {})",
        file_name, file_size, file_hash, owner_wallet, total_cost_chi, expires);

    Json(json!({
        "status": "uploaded",
        "fileHash": file_hash,
        "fileName": file_name,
        "fileSize": file_size,
        "expiresAt": expires,
        "cdnPeerId": peer_id,
        "pricing": {
            "pricePerMbMonthChi": price_per_mb_chi,
            "pricePerMbMonthWei": price_per_mb_month_wei.to_string(),
            "totalCostChi": total_cost_chi,
            "totalCostWei": total_cost_wei.to_string(),
            "durationDays": duration_days,
            "source": "market_median_1.2x_premium",
        }
    })).into_response()
}

/// GET /api/cdn/files?owner=0xABC — List CDN files, optionally filtered by owner.
async fn cdn_list(
    State(_state): State<Arc<HeadlessRuntimeState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let owner_filter = params.get("owner").cloned().unwrap_or_default().to_lowercase();
    let now = now_secs();
    let registry = load_cdn_registry();

    let files: Vec<&CdnFileEntry> = registry.iter()
        .filter(|e| {
            let not_expired = e.expires_at > now;
            let owner_match = owner_filter.is_empty() || e.owner_wallet.to_lowercase() == owner_filter;
            not_expired && owner_match
        })
        .collect();

    Json(json!({
        "files": files,
        "totalFiles": files.len(),
        "storageUsedBytes": files.iter().map(|f| f.file_size).sum::<u64>(),
    })).into_response()
}

/// DELETE /api/cdn/files/:fileHash?owner=0xABC — Remove a file from CDN.
async fn cdn_delete(
    State(state): State<Arc<HeadlessRuntimeState>>,
    axum::extract::Path(file_hash): axum::extract::Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let owner = params.get("owner").cloned().unwrap_or_default().to_lowercase();
    if owner.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "owner query param required");
    }

    let mut registry = load_cdn_registry();
    let before = registry.len();
    registry.retain(|e| !(e.file_hash == file_hash && e.owner_wallet.to_lowercase() == owner));

    if registry.len() == before {
        return json_error(StatusCode::NOT_FOUND, "File not found or not owned by this wallet");
    }

    save_cdn_registry(&registry);

    // Remove from disk
    let file_path = cdn_storage_dir().join(&file_hash);
    let _ = std::fs::remove_file(&file_path);

    // Unregister from DHT — remove CDN's seeder entry from the metadata
    if let Some(dht) = state.dht_service().await {
        dht.unregister_shared_file(&file_hash).await;

        let peer_id = dht.get_peer_id().await.unwrap_or_default();
        let dht_key = format!("chiral_file_{}", file_hash);
        if let Ok(Some(meta_json)) = dht.get_dht_value(dht_key.clone()).await {
            if let Ok(mut metadata) = serde_json::from_str::<serde_json::Value>(&meta_json) {
                // Remove CDN's peer from seeders array
                if let Some(seeders) = metadata["seeders"].as_array_mut() {
                    seeders.retain(|s| s["peerId"].as_str() != Some(&peer_id));
                }
                if metadata["peerId"].as_str() == Some(&peer_id) {
                    metadata["peerId"] = serde_json::json!("");
                }
                let _ = dht.put_dht_value(dht_key, serde_json::to_string(&metadata).unwrap_or_default()).await;
            }
        }
    }

    println!("[CDN] Deleted: {} (owner: {})", file_hash, owner);
    Json(json!({"status": "deleted", "fileHash": file_hash})).into_response()
}

/// PUT /api/cdn/files/:fileHash — Update download price for a CDN file.
async fn cdn_update_price(
    State(state): State<Arc<HeadlessRuntimeState>>,
    axum::extract::Path(file_hash): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let owner = body["owner"].as_str().unwrap_or("").to_lowercase();
    let new_price = match &body["downloadPriceChi"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => "0".to_string(),
    };

    if owner.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "owner required");
    }

    let mut registry = load_cdn_registry();
    let entry = registry.iter_mut().find(|e| e.file_hash == file_hash && e.owner_wallet.to_lowercase() == owner);

    let entry = match entry {
        Some(e) => e,
        None => return json_error(StatusCode::NOT_FOUND, "File not found or not owned by this wallet"),
    };

    entry.download_price_chi = new_price.clone();
    save_cdn_registry(&registry);

    // Update DHT record with new price
    let new_price_wei = if new_price.is_empty() || new_price == "0" {
        0u128
    } else {
        chiral_network::wallet::parse_chi_to_wei(&new_price).unwrap_or(0)
    };

    if let Some(dht) = state.dht_service().await {
        let peer_id = dht.get_peer_id().await.unwrap_or_default();
        let dht_key = format!("chiral_file_{}", file_hash);
        if let Ok(Some(meta_json)) = dht.get_dht_value(dht_key.clone()).await {
            if let Ok(mut metadata) = serde_json::from_str::<serde_json::Value>(&meta_json) {
                metadata["priceWei"] = serde_json::json!(new_price_wei.to_string());
                if let Some(seeders) = metadata["seeders"].as_array_mut() {
                    for s in seeders.iter_mut() {
                        if s["peerId"].as_str() == Some(&peer_id) {
                            s["priceWei"] = serde_json::json!(new_price_wei.to_string());
                        }
                    }
                }
                let _ = dht.put_dht_value(dht_key, serde_json::to_string(&metadata).unwrap_or_default()).await;
            }
        }

        // Also update shared file registration
        let file_path = cdn_storage_dir().join(&file_hash);
        if file_path.exists() {
            let entry = registry.iter().find(|e| e.file_hash == file_hash).unwrap();
            dht.register_shared_file(
                file_hash.clone(),
                file_path.to_string_lossy().to_string(),
                entry.file_name.clone(),
                entry.file_size,
                new_price_wei,
                entry.owner_wallet.clone(),
            ).await;
        }
    }

    println!("[CDN] Updated price for {}: {} CHI", file_hash, new_price);
    Json(json!({"status": "updated", "fileHash": file_hash, "downloadPriceChi": new_price})).into_response()
}

/// GET /api/cdn/pricing?sizeMb=10&durationDays=30 — Calculate storage cost.
async fn cdn_pricing(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let size_mb: f64 = params.get("sizeMb").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let duration_days: u64 = params.get("durationDays").and_then(|s| s.parse().ok()).unwrap_or(30);

    let price_per_mb_month_wei = calculate_cdn_price(&state).await;
    let months = (duration_days as f64) / 30.0;
    let total_cost_wei = (price_per_mb_month_wei as f64 * size_mb * months) as u128;

    Json(json!({
        "pricePerMbMonthChi": wei_to_chi_display(price_per_mb_month_wei),
        "pricePerMbMonthWei": price_per_mb_month_wei.to_string(),
        "totalCostChi": wei_to_chi_display(total_cost_wei),
        "totalCostWei": total_cost_wei.to_string(),
        "sizeMb": size_mb,
        "durationDays": duration_days,
        "source": "market_median_1.2x_premium",
        "floorPriceWei": CDN_FLOOR_PRICE_WEI_PER_MB_MONTH.to_string(),
        "uptimePremium": CDN_UPTIME_PREMIUM,
    })).into_response()
}

/// GET /api/cdn/status — CDN service status.
async fn cdn_status(
    State(state): State<Arc<HeadlessRuntimeState>>,
) -> Response {
    let registry = load_cdn_registry();
    let now = now_secs();
    let active: Vec<&CdnFileEntry> = registry.iter().filter(|e| e.expires_at > now).collect();
    let peer_id = if let Some(dht) = state.dht_service().await {
        dht.get_peer_id().await.unwrap_or_default()
    } else {
        String::new()
    };
    let cdn_wallet = state.wallet.lock().await;
    let wallet_address = cdn_wallet.as_ref().map(|w| w.address.clone()).unwrap_or_default();

    // Use floor price for fast response — full calculation is on /api/cdn/pricing
    let floor_price = CDN_FLOOR_PRICE_WEI_PER_MB_MONTH;

    Json(json!({
        "status": "online",
        "peerId": peer_id,
        "walletAddress": wallet_address,
        "activeFiles": active.len(),
        "totalStorageBytes": active.iter().map(|f| f.file_size).sum::<u64>(),
        "uniqueOwners": active.iter().map(|f| f.owner_wallet.to_lowercase()).collect::<std::collections::HashSet<_>>().len(),
        "pricing": {
            "pricePerMbMonthChi": wei_to_chi_display(floor_price),
            "pricePerMbMonthWei": floor_price.to_string(),
            "source": "market_median_1.2x_premium",
            "floorPriceChi": wei_to_chi_display(CDN_FLOOR_PRICE_WEI_PER_MB_MONTH),
        }
    })).into_response()
}

fn headless_routes(state: Arc<HeadlessRuntimeState>) -> Router {
    Router::new()
        // Health/readiness probes
        .route("/api/health", get(health_check))
        .route("/api/ready", get(readiness_check))
        // Wallet management
        .route("/api/headless/wallet", get(wallet_show))
        .route("/api/headless/wallet/create", post(wallet_create))
        .route("/api/headless/wallet/import", post(wallet_import))
        // Wallet transactions
        .route("/api/headless/wallet/balance", post(wallet_balance))
        .route("/api/headless/wallet/send", post(wallet_send))
        .route("/api/headless/wallet/receipt", post(wallet_receipt))
        .route("/api/headless/wallet/history", post(wallet_history))
        .route("/api/headless/wallet/faucet", post(wallet_faucet))
        .route("/api/headless/wallet/chain-id", get(wallet_chain_id))
        // Runtime
        .route("/api/headless/runtime", get(runtime_status))
        // DHT
        .route("/api/headless/dht/start", post(dht_start))
        .route("/api/headless/dht/stop", post(dht_stop))
        .route("/api/headless/dht/health", get(dht_health))
        .route("/api/headless/dht/peers", get(dht_peers))
        .route("/api/headless/dht/peer-id", get(dht_peer_id))
        .route("/api/headless/dht/put", post(dht_put))
        .route("/api/headless/dht/get", post(dht_get))
        .route("/api/headless/dht/ping", post(dht_ping))
        .route("/api/headless/dht/echo", post(dht_echo))
        .route("/api/headless/dht/request-file", post(dht_request_file))
        .route("/api/headless/dht/send-file", post(dht_send_file))
        .route(
            "/api/headless/dht/listening-addresses",
            get(dht_listening_addresses),
        )
        .route(
            "/api/headless/dht/register-shared-file",
            post(dht_register_shared_file),
        )
        .route(
            "/api/headless/dht/unregister-shared-file",
            post(dht_unregister_shared_file),
        )
        // File search
        .route("/api/headless/file/search", post(file_search))
        // ChiralDrop
        .route("/api/headless/drop/inbox", get(drop_inbox))
        .route("/api/headless/drop/outgoing", get(drop_outgoing))
        .route("/api/headless/drop/accept", post(drop_accept))
        .route("/api/headless/drop/decline", post(drop_decline))
        // Geth
        .route("/api/headless/geth/install", post(geth_install))
        .route("/api/headless/geth/start", post(geth_start))
        .route("/api/headless/geth/stop", post(geth_stop))
        .route("/api/headless/geth/status", get(geth_status))
        .route("/api/headless/geth/logs", get(geth_logs))
        // Mining
        .route("/api/headless/mining/start", post(mining_start))
        .route("/api/headless/mining/stop", post(mining_stop))
        .route("/api/headless/mining/status", get(mining_status))
        .route("/api/headless/mining/blocks", get(mining_blocks))
        .route(
            "/api/headless/mining/miner-address",
            post(set_miner_address),
        )
        // Hosting
        .route("/api/headless/hosting/publish-ad", post(hosting_publish_ad))
        .route("/api/headless/hosting/registry", get(hosting_get_registry))
        // CDN — always-on file hosting service
        // 500MB body limit matches the per-file cap enforced inside cdn_upload.
        .route(
            "/api/cdn/upload",
            post(cdn_upload).layer(DefaultBodyLimit::max(500 * 1024 * 1024)),
        )
        .route("/api/cdn/files", get(cdn_list))
        .route("/api/cdn/files/:file_hash", delete(cdn_delete).put(cdn_update_price))
        .route("/api/cdn/pricing", get(cdn_pricing))
        .route("/api/cdn/status", get(cdn_status))
        // Diagnostics
        .route("/api/headless/bootstrap-health", get(bootstrap_health))
        .with_state(state)
}

#[tokio::main]
async fn main() {
    let args = DaemonArgs::parse();
    if let Some(port) = args.p2p_port {
        std::env::set_var("CHIRAL_P2P_PORT", port.to_string());
    }
    let pid_file = args.pid_file.unwrap_or_else(default_pid_file);

    if let Err(e) = write_pid_file(&pid_file) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    let hosting_state = Arc::new(HostingServerState::new());
    hosting_state.load_from_disk().await;

    let drive_state = Arc::new(DriveState::new());
    drive_state.load_from_disk_async().await;

    let rating_state = Arc::new(RatingState::new(default_data_dir()));
    let runtime_state = Arc::new(HeadlessRuntimeState::new());

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    let base_router = hosting_server::create_gateway_router(
        Arc::clone(&hosting_state),
        Some(Arc::clone(&drive_state)),
        Some(Arc::clone(&rating_state)),
        None,
    );

    let app = base_router
        .merge(headless_routes(Arc::clone(&runtime_state)))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
                .expose_headers(Any),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(v) => v,
        Err(e) => {
            remove_pid_file(&pid_file);
            eprintln!("Failed to bind headless gateway: {}", e);
            std::process::exit(1);
        }
    };

    let bound = match listener.local_addr() {
        Ok(v) => v,
        Err(e) => {
            remove_pid_file(&pid_file);
            eprintln!("Failed to read bound address: {}", e);
            std::process::exit(1);
        }
    };

    println!("chiral-daemon running on http://{}", bound);
    println!("PID file: {}", pid_file.display());

    // Auto-start services if requested
    let auto_mine = args.auto_mine;
    let auto_start_dht = args.auto_start_dht || auto_mine;
    let auto_start_geth = args.auto_start_geth || auto_mine;
    let miner_address = args.miner_address.clone();
    let mining_threads = args.mining_threads;

    if auto_start_dht || auto_start_geth || auto_mine {
        let rt = Arc::clone(&runtime_state);
        tokio::spawn(async move {
            // Start DHT
            if auto_start_dht {
                println!("[AUTO] Starting DHT...");
                let mut guard = rt.dht.lock().await;
                if guard.is_none() {
                    let ft = Arc::clone(&rt.file_transfer);
                    let dd = Arc::clone(&rt.download_directory);
                    let dc = Arc::clone(&rt.download_credentials);
                    let svc = Arc::new(dht::DhtService::new(ft, dd, dc));
                    match svc.start_headless().await {
                        Ok(_) => {
                            *guard = Some(svc);
                            println!("[AUTO] DHT started successfully");
                        }
                        Err(e) => eprintln!("[AUTO] DHT start failed: {}", e),
                    }
                }
                drop(guard);

                let wallet_address = {
                    let wallet = rt.wallet.lock().await;
                    wallet.as_ref().map(|w| w.address.clone())
                };
                if let Some(addr) = wallet_address {
                    match auto_publish_wallet_advertisement(rt.as_ref(), &addr).await {
                        Ok(()) => println!("[AUTO] Wallet advertisement published for {}", addr),
                        Err(e) => eprintln!("[AUTO] Wallet advertisement publish failed: {}", e),
                    }
                }

                // Wait for DHT bootstrap
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }

            // Start Geth
            if auto_start_geth {
                println!("[AUTO] Starting Geth...");
                let mut geth = rt.geth.lock().await;
                match geth.start(miner_address.as_deref()).await {
                    Ok(()) => println!("[AUTO] Geth started successfully"),
                    Err(e) => eprintln!("[AUTO] Geth start failed: {}", e),
                }
                drop(geth);
                // Wait for Geth RPC to be ready
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }

            // Start mining
            if auto_mine {
                if let Some(ref addr) = miner_address {
                    println!("[AUTO] Setting miner address: {}", addr);
                    let geth = rt.geth.lock().await;
                    let _ = geth.set_miner_address(addr).await;
                    drop(geth);
                }

                println!(
                    "[AUTO] Starting mining with {} thread(s)...",
                    mining_threads
                );
                let mut geth = rt.geth.lock().await;
                match geth.start_mining(mining_threads).await {
                    Ok(()) => println!("[AUTO] Mining started successfully"),
                    Err(e) => eprintln!("[AUTO] Mining start failed: {}", e),
                }
            }
        });
    }

    // CDN re-seed: re-register all non-expired CDN files in the DHT on startup
    {
        let cdn_reseed_rt = Arc::clone(&runtime_state);
        tokio::spawn(async move {
            // Wait for DHT to be fully bootstrapped
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;

            let now = now_secs();
            let registry = load_cdn_registry();
            let active: Vec<&CdnFileEntry> = registry.iter().filter(|e| e.expires_at > now).collect();

            if active.is_empty() { return; }

            let dht = match cdn_reseed_rt.dht_service().await {
                Some(d) => d,
                None => { eprintln!("[CDN] Cannot re-seed: DHT not running"); return; }
            };

            let peer_id = dht.get_peer_id().await.unwrap_or_default();
            let our_addrs = dht.get_listening_addresses().await;

            for entry in &active {
                let file_path = cdn_storage_dir().join(&entry.file_hash);
                if !file_path.exists() { continue; }

                // Parse download price
                let price_wei = if entry.download_price_chi.is_empty() || entry.download_price_chi == "0" {
                    0u128
                } else {
                    chiral_network::wallet::parse_chi_to_wei(&entry.download_price_chi).unwrap_or(0)
                };

                dht.register_shared_file(
                    entry.file_hash.clone(),
                    file_path.to_string_lossy().to_string(),
                    entry.file_name.clone(),
                    entry.file_size,
                    price_wei,
                    entry.owner_wallet.clone(),
                ).await;

                // Re-publish to DHT
                let metadata = serde_json::json!({
                    "hash": entry.file_hash,
                    "fileName": entry.file_name,
                    "fileSize": entry.file_size,
                    "protocol": "WebRTC",
                    "createdAt": entry.uploaded_at,
                    "peerId": peer_id,
                    "priceWei": price_wei.to_string(),
                    "walletAddress": entry.owner_wallet,
                    "seeders": [{
                        "peerId": peer_id,
                        "priceWei": price_wei.to_string(),
                        "walletAddress": entry.owner_wallet,
                        "multiaddrs": our_addrs,
                        "signature": ""
                    }],
                    "publisherSignature": ""
                });
                let dht_key = format!("chiral_file_{}", entry.file_hash);
                let _ = dht.put_dht_value(dht_key, serde_json::to_string(&metadata).unwrap_or_default()).await;
            }
            println!("[CDN] Re-seeded {} files on startup", active.len());
        });
    }

    // CDN expiration cleanup — runs every 60 seconds, removes expired files
    {
        let cdn_rt = Arc::clone(&runtime_state);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let now = now_secs();
                let mut registry = load_cdn_registry();
                let before = registry.len();
                let expired: Vec<CdnFileEntry> = registry.iter()
                    .filter(|e| e.expires_at <= now)
                    .cloned()
                    .collect();

                if expired.is_empty() { continue; }

                registry.retain(|e| e.expires_at > now);
                save_cdn_registry(&registry);

                // Unregister expired files from DHT and delete from disk
                for entry in &expired {
                    let file_path = cdn_storage_dir().join(&entry.file_hash);
                    let _ = std::fs::remove_file(&file_path);

                    if let Some(dht) = cdn_rt.dht_service().await {
                        dht.unregister_shared_file(&entry.file_hash).await;
                    }

                    println!("[CDN] Expired and removed: {} (owner: {}, hash: {})",
                        entry.file_name, entry.owner_wallet, entry.file_hash);
                }
                println!("[CDN] Cleanup: removed {} expired files ({} remaining)",
                    expired.len(), registry.len());
            }
        });
    }

    tokio::spawn(async move {
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
            println!("Headless daemon received shutdown signal");
        });

        if let Err(e) = server.await {
            eprintln!("Headless daemon server error: {}", e);
        }
    });

    // Wait for SIGINT (Ctrl+C) or SIGTERM (Docker stop)
    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();

        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => println!("Received SIGINT"),
                _ = sigterm.recv() => println!("Received SIGTERM"),
            }
        }

        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
            println!("Received SIGINT");
        }
    };
    shutdown.await;

    // Best-effort runtime cleanup.
    println!("Shutting down...");
    if let Some(dht) = runtime_state.dht_service().await {
        let _ = dht.stop().await;
    }
    {
        let mut geth = runtime_state.geth.lock().await;
        let _ = geth.stop();
    }

    let _ = shutdown_tx.send(());
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    remove_pid_file(&pid_file);
    println!("Daemon stopped.");
}
