use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use clap::Parser;
use serde::Deserialize;
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

    /// Path to a wallet-key file. The file's contents must be a hex
    /// secp256k1 private key (with or without leading `0x`). When set,
    /// the daemon loads it at startup and populates `state.wallet` so
    /// the CDN module can sign FileInfo / SeederInfo records (FM-A07,
    /// FM-A08, FM-A09). Without it, the CDN runs with empty signatures
    /// and clients reject every record it publishes — which broke
    /// every existing CDN upload after the FM-Agent enforcement
    /// landed.
    #[arg(long, env = "CHIRAL_WALLET_KEY_FILE")]
    wallet_key_file: Option<PathBuf>,
}

#[derive(Clone, serde::Serialize)]
struct WalletInfo {
    address: String,
    private_key: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRegistryEntry {
    peer_id: String,
    wallet_address: String,
    updated_at: u64,
}

fn host_registry_from_dht_value(
    registry_json: Option<String>,
) -> Result<Vec<HostRegistryEntry>, String> {
    match registry_json {
        Some(json) => serde_json::from_str(&json)
            .map_err(|e| format!("Malformed chiral_host_registry JSON: {}", e)),
        None => Ok(Vec::new()),
    }
}

fn host_registry_from_dht_lookup(
    lookup: Result<Option<String>, String>,
) -> Result<Vec<HostRegistryEntry>, String> {
    let registry_json =
        lookup.map_err(|e| format!("Failed to read chiral_host_registry: {}", e))?;
    host_registry_from_dht_value(registry_json)
}

fn host_registry_after_publish(
    mut registry: Vec<HostRegistryEntry>,
    peer_id: String,
    wallet_address: String,
    updated_at: u64,
) -> Vec<HostRegistryEntry> {
    registry.retain(|e| e.peer_id != peer_id);
    registry.push(HostRegistryEntry {
        peer_id,
        wallet_address,
        updated_at,
    });
    registry
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

fn is_hex_prefixed(value: &str, bytes: usize) -> bool {
    value.len() == 2 + bytes * 2
        && value.starts_with("0x")
        && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_headless_wallet_address(value: Option<&str>) -> Result<String, String> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        return Err("address required".to_string());
    }
    if !is_hex_prefixed(value, 20) {
        return Err("address must be a 0x-prefixed 20-byte hex string".to_string());
    }
    Ok(value.to_string())
}

const DEFAULT_GETH_LOG_LINES: usize = 100;
const MAX_GETH_LOG_LINES: usize = 1000;

fn validate_geth_log_lines(lines: Option<usize>) -> Result<usize, String> {
    let lines = lines.unwrap_or(DEFAULT_GETH_LOG_LINES);
    if lines == 0 {
        return Err("lines must be at least 1".to_string());
    }
    if lines > MAX_GETH_LOG_LINES {
        return Err(format!("lines must be at most {}", MAX_GETH_LOG_LINES));
    }
    Ok(lines)
}

fn tail_log_lines(contents: &str, max_lines: usize) -> String {
    let all_lines: Vec<&str> = contents.lines().collect();
    let start = if all_lines.len() > max_lines {
        all_lines.len() - max_lines
    } else {
        0
    };
    all_lines[start..].join("\n")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(unix)]
fn daemon_sigterm_handler_from_result<T>(handler: std::io::Result<T>) -> Result<T, String> {
    handler.map_err(|e| format!("failed to install SIGTERM handler: {}", e))
}

#[cfg(unix)]
fn install_daemon_sigterm_handler() -> Result<tokio::signal::unix::Signal, String> {
    daemon_sigterm_handler_from_result(tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::terminate(),
    ))
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
    let registry_key = "chiral_host_registry".to_string();
    let registry = host_registry_from_dht_lookup(dht.get_dht_value(registry_key.clone()).await)?;
    let registry =
        host_registry_after_publish(registry, peer_id.clone(), wallet_address.to_string(), now);
    let registry_json = serde_json::to_string(&registry)
        .map_err(|e| format!("Failed to serialize host registry: {}", e))?;

    let host_key = format!("chiral_host_{}", peer_id);
    dht.put_dht_value(host_key, ad_json).await?;
    dht.put_dht_value(registry_key, registry_json).await
}

fn read_geth_log(lines: Option<usize>) -> Result<String, String> {
    let max_lines = validate_geth_log_lines(lines)?;
    let data_dir = chiral_network::network::data_dir().join("geth");
    let log_path = data_dir.join("geth.log");
    if !log_path.exists() {
        return Ok("No geth.log found".to_string());
    }

    let contents = std::fs::read_to_string(&log_path)
        .map_err(|e| format!("Failed to read geth.log: {}", e))?;
    Ok(tail_log_lines(&contents, max_lines))
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
    folder_hash: Option<String>,
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
    protocol: Option<String>,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    wallet_address: String,
    /// Hex private key for signing FileInfo envelopes. Empty = unsigned
    /// (downloaders will reject; use for free-only or proxy seeding).
    #[serde(default)]
    private_key: String,
}

fn register_shared_file_protocol(protocol: Option<&str>) -> String {
    let protocol = protocol.unwrap_or("WebRTC").trim();
    if protocol.is_empty() {
        "WebRTC".to_string()
    } else {
        protocol.to_string()
    }
}

fn signed_file_metadata_json_for_register(
    req: &RegisterSharedFileRequest,
    protocol: &str,
) -> Result<String, String> {
    let metadata = chiral_network::try_make_signed_file_metadata(
        &req.file_hash,
        &req.file_name,
        req.file_size,
        protocol,
        &req.wallet_address,
        Some(&req.private_key),
    )
    .ok_or_else(|| "Failed to sign file metadata".to_string())?;
    serde_json::to_string(&metadata)
        .map_err(|e| format!("Failed to serialize file metadata: {}", e))
}

fn register_shared_file_unpublished_payload(warning: impl Into<String>) -> serde_json::Value {
    json!({
        "status": "ok",
        "dhtPublished": false,
        "warning": warning.into(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnregisterSharedFileRequest {
    #[serde(default)]
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
        let geth = state.geth.lock().await;
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

    let geth = state.geth.lock().await;
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
    // Reserved namespaces are interpreted by every other peer as
    // authoritative metadata (file metadata, seeder records, folder
    // manifests, site directory, etc). Each namespace has its own
    // dedicated publication command that signs the record; allowing a
    // raw `put` here lets any HTTP client forge those records, the
    // same defect class that originally enabled BUG-001.
    const RESERVED_PREFIXES: &[&str] = &[
        "chiral_file_",
        "chiral_seeder_",
        "chiral_folder_",
        "chiral_drive_share_",
        "chiral_host_ad_",
    ];
    if RESERVED_PREFIXES.iter().any(|p| req.key.starts_with(p)) {
        return json_error(
            StatusCode::FORBIDDEN,
            format!(
                "Key '{}' is in a reserved namespace; use the dedicated signed-publication command",
                req.key
            ),
        );
    }
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
        .request_file(
            req.peer_id,
            req.file_hash,
            req.request_id,
            req.multiaddrs,
            req.folder_hash,
        )
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
    let data = match tokio::fs::read(&req.file_path).await {
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

    // Local seed registration first (fast, no I/O on the network).
    svc.register_shared_file(
        req.file_hash.clone(),
        req.file_path.clone(),
        req.file_name.clone(),
        req.file_size,
        price_wei,
        req.wallet_address.clone(),
        req.private_key.clone(),
    )
    .await;

    // Then publish to the DHT so other peers can discover us. Without this
    // step the file is only locally seeded and `file/search` from any
    // remote node returns "not found".
    if req.private_key.is_empty() || req.wallet_address.is_empty() {
        return Json(register_shared_file_unpublished_payload(
            "No wallet/private key provided — file registered locally only; remote peers cannot discover it",
        ))
        .into_response();
    }

    let peer_id = svc.get_peer_id().await.unwrap_or_default();
    if peer_id.is_empty() {
        return Json(register_shared_file_unpublished_payload(
            "DHT peer ID unavailable",
        ))
        .into_response();
    }

    let multiaddrs = svc.get_listening_addresses().await;
    let Some(seeder) = chiral_network::try_make_signed_seeder(
        &peer_id,
        &req.file_hash,
        &price_wei.to_string(),
        &req.wallet_address,
        multiaddrs,
        Some(&req.private_key),
    ) else {
        return Json(register_shared_file_unpublished_payload(
            "Failed to sign seeder entry",
        ))
        .into_response();
    };

    // Always publish (don't gate on existing-blob check). With first-hit
    // Kademlia, a stale local copy would short-circuit the put, then
    // expire from the store, and the file becomes unreachable. Multiple
    // signed blobs at the same chiral_file_<hash> key are harmless —
    // verify_publisher accepts whichever the reader sees.
    let blob_key = format!("chiral_file_{}", req.file_hash);
    let protocol = register_shared_file_protocol(req.protocol.as_deref());
    let blob = match signed_file_metadata_json_for_register(&req, &protocol) {
        Ok(blob) => blob,
        Err(e) => {
            return Json(register_shared_file_unpublished_payload(e)).into_response();
        }
    };
    if let Err(e) = svc.put_dht_value(blob_key, blob).await {
        return Json(register_shared_file_unpublished_payload(format!(
            "File metadata publish failed: {}",
            e
        )))
        .into_response();
    }

    if let Err(e) = chiral_network::publish_seeder_entry(&svc, &req.file_hash, &seeder).await {
        return Json(register_shared_file_unpublished_payload(format!(
            "Seeder entry publish failed: {}",
            e
        )))
        .into_response();
    }

    Json(json!({ "status": "ok", "dhtPublished": true })).into_response()
}

fn validate_headless_file_hash(file_hash: &str) -> Result<String, String> {
    let trimmed = file_hash.trim();
    if trimmed.is_empty() {
        return Err("fileHash required".to_string());
    }
    if trimmed.len() != 64 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("fileHash must be a 64-character hex content hash".to_string());
    }
    Ok(trimmed.to_ascii_lowercase())
}

async fn dht_unregister_shared_file(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(req): Json<UnregisterSharedFileRequest>,
) -> Response {
    let file_hash = match validate_headless_file_hash(&req.file_hash) {
        Ok(file_hash) => file_hash,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, err),
    };

    let Some(svc) = state.dht_service().await else {
        return json_error(StatusCode::BAD_REQUEST, "DHT not running");
    };

    // Local map first so subsequent FileInfo requests are immediately
    // refused. Then mirror the desktop `drive_stop_seeding` flow: drop
    // our per-seeder DHT record and stop being a Kademlia provider for
    // this file. Without these calls a peer who unregisters headlessly
    // stays in everyone else's seeder lists until the records age out
    // naturally (~3 min republish + remote TTL), and provider lookups
    // keep returning a peer that no longer serves the file.
    svc.unregister_shared_file(&file_hash).await;
    if let Err(e) = chiral_network::remove_seeder_entry(&svc, &file_hash).await {
        eprintln!(
            "[DAEMON] remove_seeder_entry failed for {}: {} (local seeding stopped, but DHT cleanup incomplete)",
            file_hash, e
        );
    }
    Json(json!({ "status": "ok" })).into_response()
}

#[cfg(test)]
mod unregister_hash_tests {
    use super::*;

    #[test]
    fn validate_headless_file_hash_rejects_missing_hash() {
        let err =
            validate_headless_file_hash("  ").expect_err("missing file hash should be rejected");

        assert!(err.contains("fileHash"));
        assert!(err.contains("required"));
    }

    #[test]
    fn validate_headless_file_hash_rejects_malformed_hash() {
        let err = validate_headless_file_hash("not-a-hash")
            .expect_err("malformed file hash should be rejected");

        assert!(err.contains("64-character hex"));
    }

    #[test]
    fn validate_headless_file_hash_rejects_non_hex_hash() {
        let err = validate_headless_file_hash(&"g".repeat(64))
            .expect_err("non-hex file hash should be rejected");

        assert!(err.contains("64-character hex"));
    }

    #[test]
    fn validate_headless_file_hash_accepts_and_normalizes_hash() {
        let hash = validate_headless_file_hash(&"A".repeat(64))
            .expect("valid file hash should be accepted");

        assert_eq!(hash, "a".repeat(64));
    }
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
    let geth = state.geth.lock().await;
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
    let geth = state.geth.lock().await;
    match geth.start_mining(req.threads.unwrap_or(1)).await {
        Ok(()) => Json(json!({ "status": "started" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn mining_stop(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let geth = state.geth.lock().await;
    match geth.stop_mining().await {
        Ok(()) => Json(json!({ "status": "stopped" })).into_response(),
        Err(err) => json_error(StatusCode::BAD_REQUEST, err),
    }
}

async fn mining_status(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let geth = state.geth.lock().await;
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
    let mut geth = state.geth.lock().await;
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

/// Derive `0x`-prefixed lowercase Ethereum address from a 32-byte
/// secp256k1 private key.
fn address_from_private_key(priv_bytes: &[u8; 32]) -> Result<String, String> {
    let secp = secp256k1::Secp256k1::new();
    let secret_key = secp256k1::SecretKey::from_slice(priv_bytes)
        .map_err(|e| format!("invalid private key: {}", e))?;
    let public_key = secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
    let pub_bytes = public_key.serialize_uncompressed();
    use tiny_keccak::{Hasher, Keccak};
    let mut keccak = Keccak::v256();
    keccak.update(&pub_bytes[1..]);
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

/// Read a wallet-key file: trim whitespace, strip optional `0x`,
/// require 32-byte hex. Returns `WalletInfo` populated with both
/// address (derived) and the canonical `0x`-prefixed private key.
fn load_wallet_from_file(path: &PathBuf) -> Result<WalletInfo, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let cleaned = raw.trim().trim_start_matches("0x");
    let bytes = hex::decode(cleaned).map_err(|e| format!("wallet-key file is not hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!(
            "wallet-key file decoded to {} bytes, expected 32",
            bytes.len()
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let address = address_from_private_key(&arr)?;
    Ok(WalletInfo {
        address,
        private_key: format!("0x{}", hex::encode(arr)),
    })
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
    State(_state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let address = body["address"].as_str().unwrap_or("").to_string();
    if address.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "address required");
    }
    // Use the canonical-RPC fallback list so a firewall-blocked or
    // momentarily-down direct RPC port falls through to the relay's
    // /api/chain/rpc proxy on 8080 instead of returning a misleading
    // 0 / 500.
    let endpoints = chiral_network::geth::wallet_rpc_endpoints();
    let result = match chiral_network::rpc_client::call_with_fallbacks(
        &endpoints,
        "eth_getBalance",
        serde_json::json!([address, "latest"]),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };
    let hex = match result.as_str() {
        Some(hex) => hex,
        None => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("eth_getBalance returned a non-string hex value: {result}"),
            )
        }
    };
    let wei = match chiral_network::rpc_client::hex_to_u128(hex) {
        Ok(wei) => wei,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("eth_getBalance: {e}"),
            )
        }
    };
    Json(json!({
        "balance": chiral_network::rpc_client::wei_to_chi_string(wei),
        "balanceWei": wei.to_string(),
    }))
    .into_response()
}

async fn wallet_send(
    State(_state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let from = body["from"].as_str().unwrap_or("").to_string();
    let to = body["to"].as_str().unwrap_or("").to_string();
    let amount = body["amount"].as_str().unwrap_or("").to_string();
    let private_key = body["privateKey"].as_str().unwrap_or("").to_string();

    if from.is_empty() || to.is_empty() || amount.is_empty() || private_key.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "from, to, amount, privateKey required",
        );
    }

    // Headless daemon uses its own local geth when available
    // (effective_rpc_endpoint returns 127.0.0.1:8545 in that case) and
    // falls back to the configured remote otherwise. Single-element
    // list — no second endpoint to fall back to from the daemon's
    // perspective.
    let endpoints = [chiral_network::geth::effective_rpc_endpoint()];
    match chiral_network::wallet::send_transaction(&endpoints, &from, &to, &amount, &private_key)
        .await
    {
        Ok(result) => Json(json!(result)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_receipt(
    State(_state): State<Arc<HeadlessRuntimeState>>,
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
    State(_state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let address = match validate_headless_wallet_address(body["address"].as_str()) {
        Ok(address) => address,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, err),
    };
    let endpoint = chiral_network::geth::effective_rpc_endpoint();
    let metadata = chiral_network::wallet::load_tx_metadata();
    match chiral_network::wallet::get_transaction_history(&endpoint, &address, &metadata).await {
        Ok(result) => Json(json!(result)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_faucet(Json(body): Json<serde_json::Value>) -> Response {
    let address = match validate_headless_wallet_address(body["address"].as_str()) {
        Ok(address) => address,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, err),
    };
    match chiral_network::wallet::request_faucet(&address).await {
        Ok(result) => Json(json!(result)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn wallet_chain_id() -> Response {
    Json(json!({"chainId": chiral_network::geth::chain_id()})).into_response()
}

// ---- File search endpoint ----

fn parse_headless_seeder_record(
    peer_id: &str,
    key: &str,
    json_str: &str,
) -> Result<serde_json::Value, serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(json_str).map_err(|e| {
        json!({
            "type": "malformedSeederRecord",
            "peerId": peer_id,
            "key": key,
            "error": format!("Malformed seeder record JSON: {}", e),
        })
    })
}

fn append_file_search_warnings(response: &mut serde_json::Value, warnings: Vec<serde_json::Value>) {
    if !warnings.is_empty() {
        response["warnings"] = serde_json::Value::Array(warnings);
    }
}

#[cfg(test)]
mod file_search_seeder_tests {
    use super::*;

    #[test]
    fn parse_headless_seeder_record_accepts_valid_json() {
        let entry = parse_headless_seeder_record(
            "peer-a",
            "chiral_seeder_hash_peer-a",
            r#"{"peerId":"peer-a","multiaddrs":["/ip4/127.0.0.1/tcp/1"]}"#,
        )
        .expect("valid seeder JSON should parse");

        assert_eq!(entry["peerId"], "peer-a");
        assert_eq!(entry["multiaddrs"][0], "/ip4/127.0.0.1/tcp/1");
    }

    #[test]
    fn parse_headless_seeder_record_returns_warning_for_malformed_json() {
        let warning =
            parse_headless_seeder_record("peer-a", "chiral_seeder_hash_peer-a", "{not json")
                .expect_err("malformed seeder JSON should return a warning");

        assert_eq!(warning["type"], "malformedSeederRecord");
        assert_eq!(warning["peerId"], "peer-a");
        assert_eq!(warning["key"], "chiral_seeder_hash_peer-a");
        assert!(warning["error"]
            .as_str()
            .expect("warning should include an error")
            .contains("Malformed seeder record JSON"));
    }

    #[test]
    fn mixed_headless_seeder_records_preserve_valid_entries_and_warn() {
        let records = [
            (
                "peer-a",
                "chiral_seeder_hash_peer-a",
                r#"{"peerId":"peer-a"}"#,
            ),
            ("peer-b", "chiral_seeder_hash_peer-b", "{not json"),
        ];
        let mut seeders = Vec::new();
        let mut warnings = Vec::new();

        for (peer_id, key, json_str) in records {
            match parse_headless_seeder_record(peer_id, key, json_str) {
                Ok(entry) => seeders.push(entry),
                Err(warning) => warnings.push(warning),
            }
        }

        assert_eq!(seeders.len(), 1);
        assert_eq!(seeders[0]["peerId"], "peer-a");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["peerId"], "peer-b");

        let mut response = json!({
            "found": true,
            "metadata": null,
            "providers": ["peer-a", "peer-b"],
            "seeders": seeders,
        });
        append_file_search_warnings(&mut response, warnings);

        assert_eq!(response["warnings"][0]["type"], "malformedSeederRecord");
        assert_eq!(response["warnings"][0]["peerId"], "peer-b");
    }
}

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
    // Run the blob + providers lookups in parallel and bound them; libp2p
    // Kademlia otherwise waits for the full query convergence (often
    // 10-20s) even when the record is in the local store. Without
    // bounds the desktop's 5s HTTP timeout always fires before the body
    // arrives, so CDN search results never reach the user.
    let blob_fut = tokio::time::timeout(
        std::time::Duration::from_millis(4000),
        dht.get_dht_value(dht_key),
    );
    let providers_fut = tokio::time::timeout(
        std::time::Duration::from_millis(3000),
        dht.get_file_providers(file_hash.clone()),
    );
    let (blob_res, providers_res) = tokio::join!(blob_fut, providers_fut);
    let blob = match blob_res {
        Ok(r) => r,
        Err(_) => Ok(None),
    };
    let providers: Vec<String> = match providers_res {
        Ok(Ok(p)) => p,
        _ => Vec::new(),
    };
    // Per-seeder records: parallel fetch with a short shared deadline so
    // one slow provider doesn't extend the search budget.
    let fetches = providers.iter().cloned().map(|peer_id| {
        let key = format!("chiral_seeder_{}_{}", file_hash, peer_id);
        let dht = dht.clone();
        async move {
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(3000),
                dht.get_dht_value(key.clone()),
            )
            .await;
            (peer_id, key, result)
        }
    });
    let fetched = futures::future::join_all(fetches).await;
    let mut seeders: Vec<serde_json::Value> = Vec::new();
    let mut seeder_warnings: Vec<serde_json::Value> = Vec::new();
    for (peer_id, key, r) in fetched {
        if let Ok(Ok(Some(json_str))) = r {
            match parse_headless_seeder_record(&peer_id, &key, &json_str) {
                Ok(entry) => seeders.push(entry),
                Err(warning) => {
                    let error = warning
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Malformed seeder record JSON");
                    eprintln!(
                        "[DAEMON] malformed seeder record for file {} peer {} key {}: {}",
                        file_hash, peer_id, key, error
                    );
                    seeder_warnings.push(warning);
                }
            }
        }
    }
    match blob {
        Ok(Some(json_str)) => {
            let metadata = serde_json::from_str::<serde_json::Value>(&json_str)
                .unwrap_or_else(|_| json!({"raw": json_str}));
            let mut response = json!({
                "found": true,
                "metadata": metadata,
                "providers": providers,
                "seeders": seeders,
            });
            append_file_search_warnings(&mut response, seeder_warnings);
            Json(response).into_response()
        }
        Ok(None) if !seeders.is_empty() => {
            let mut response = json!({
                "found": true,
                "metadata": null,
                "providers": providers,
                "seeders": seeders,
            });
            append_file_search_warnings(&mut response, seeder_warnings);
            Json(response).into_response()
        }
        Ok(None) => {
            let mut response = json!({"found": false, "providers": providers});
            append_file_search_warnings(&mut response, seeder_warnings);
            Json(response).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

// ---- Folder bundle search endpoint ----

async fn folder_search(
    State(state): State<Arc<HeadlessRuntimeState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let folder_hash = body["folderHash"].as_str().unwrap_or("").to_string();
    if folder_hash.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "folderHash required");
    }
    let dht = match state.dht_service().await {
        Some(d) => d,
        None => return json_error(StatusCode::SERVICE_UNAVAILABLE, "DHT not running"),
    };
    let key = format!("chiral_folder_{}", folder_hash);
    let blob = match tokio::time::timeout(
        std::time::Duration::from_millis(4000),
        dht.get_dht_value(key),
    )
    .await
    {
        Ok(Ok(Some(json_str))) => json_str,
        Ok(Ok(None)) => return Json(json!({"found": false})).into_response(),
        Ok(Err(e)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
        Err(_) => return Json(json!({"found": false, "error": "timeout"})).into_response(),
    };
    // Trust contract: folder manifests are ECDSA-signed by their owner.
    // Without verification any peer could re-publish chiral_folder_<H>
    // with a forged priceWei / walletAddress and divert payment. Drop
    // unsigned / signature-invalid manifests instead of returning them
    // — buyers MUST be able to trust the headless response shape.
    let manifest_typed: chiral_network::FolderManifest = match serde_json::from_str(&blob) {
        Ok(m) => m,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Invalid folder manifest: {}", e),
            )
        }
    };
    if !manifest_typed.verify() {
        let reason = if manifest_typed.publisher_signature.is_empty() {
            "unsigned"
        } else {
            "INVALID signature"
        };
        return Json(json!({
            "found": false,
            "error": format!("Folder manifest {} — dropped", reason),
        }))
        .into_response();
    }
    let manifest = match serde_json::from_str::<serde_json::Value>(&blob) {
        Ok(m) => m,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Invalid folder manifest: {}", e),
            )
        }
    };
    Json(json!({
        "found": true,
        "manifest": manifest,
    }))
    .into_response()
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
    let ad_json = match serde_json::to_string(&ad) {
        Ok(json) => json,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize advertisement: {}", e),
            )
        }
    };
    let wallet_address = ad["walletAddress"].as_str().unwrap_or("").to_string();

    // Update registry
    let registry_key = "chiral_host_registry".to_string();
    let registry =
        match host_registry_from_dht_lookup(dht.get_dht_value(registry_key.clone()).await) {
            Ok(registry) => registry,
            Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        };
    let now = now_secs();
    let registry = host_registry_after_publish(registry, peer_id.clone(), wallet_address, now);
    let registry_json = match serde_json::to_string(&registry) {
        Ok(json) => json,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize host registry: {}", e),
            )
        }
    };

    // Store individual ad only after the shared registry parses cleanly.
    let host_key = format!("chiral_host_{}", peer_id);
    if let Err(e) = dht.put_dht_value(host_key, ad_json).await {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &e);
    }

    match dht.put_dht_value(registry_key, registry_json).await {
        Ok(_) => Json(json!({"status": "published"})).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn hosting_get_registry(State(state): State<Arc<HeadlessRuntimeState>>) -> Response {
    let dht = match state.dht_service().await {
        Some(d) => d,
        None => return json_error(StatusCode::SERVICE_UNAVAILABLE, "DHT not running"),
    };
    match dht.get_dht_value("chiral_host_registry".to_string()).await {
        Ok(Some(json)) => match host_registry_from_dht_value(Some(json)) {
            Ok(registry) => Json(json!({"registry": registry})).into_response(),
            Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        Ok(None) => Json(json!({"registry": []})).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn bootstrap_health(State(_state): State<Arc<HeadlessRuntimeState>>) -> Response {
    // Bootstrap discovery was removed with the geth rewrite. Report the
    // active network's configured enode (if any) as a single static entry.
    let cfg = chiral_network::network::active();
    let has_enode = !cfg.geth_bootstrap_enode.is_empty();
    Json(json!({
        "totalNodes": if has_enode { 1 } else { 0 },
        "healthyNodes": if has_enode { 1 } else { 0 },
        "nodes": [],
        "isHealthy": true,
        "healthyEnodeString": cfg.geth_bootstrap_enode,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    }))
    .into_response()
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
        .route("/api/headless/folder/search", post(folder_search))
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
        // CDN routes live in crate::cdn_server and are merged into the
        // top-level router via main() — kept separate from this handler
        // state so the CDN module can own its own registry + price config.
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
    chiral_network::version::log_policy_key_status();
    let pid_file = args.pid_file.unwrap_or_else(default_pid_file);

    if let Err(e) = write_pid_file(&pid_file) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    let hosting_state = Arc::new(HostingServerState::new());
    hosting_state.load_from_disk().await;

    let drive_state = Arc::new(DriveState::new());
    drive_state.load_from_disk_async().await;

    let runtime_state = Arc::new(HeadlessRuntimeState::new());
    let rating_state = Arc::new(RatingState::new_with_issuer_dht(
        default_data_dir(),
        Some(Arc::clone(&runtime_state.dht)),
    ));

    // Load wallet key at startup if --wallet-key-file (or
    // CHIRAL_WALLET_KEY_FILE) was set. This populates state.wallet
    // before CdnState::new runs so the CDN module can pull the
    // private_key for FileInfo / SeederInfo signing.
    if let Some(ref key_path) = args.wallet_key_file {
        match load_wallet_from_file(key_path) {
            Ok(wallet) => {
                println!(
                    "[WALLET] Loaded key from {}: address={}",
                    key_path.display(),
                    wallet.address
                );
                *runtime_state.wallet.lock().await = Some(wallet);
            }
            Err(e) => {
                eprintln!(
                    "[WALLET] Failed to load wallet from {}: {} — daemon will run without a signing key (CDN can serve only free files; signed records won't be published).",
                    key_path.display(),
                    e
                );
            }
        }
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    let base_router = hosting_server::create_gateway_router(
        Arc::clone(&hosting_state),
        Some(Arc::clone(&drive_state)),
        Some(Arc::clone(&rating_state)),
        None,
    );

    // If the operator passed --miner-address at launch, pre-populate the
    // wallet slot with that address (no private key). This has to run
    // before CdnState is constructed — CdnState captures the wallet
    // address at build time, and an empty one means CDN clients have no
    // address to pay.
    if let Some(ref addr) = args.miner_address {
        let mut wallet_guard = runtime_state.wallet.lock().await;
        if wallet_guard.is_none() {
            *wallet_guard = Some(WalletInfo {
                address: addr.clone(),
                private_key: String::new(),
            });
            println!("Pre-populated CDN wallet with miner address: {}", addr);
        }
    }

    // CDN server owns its own registry + price config. We build it here so
    // both the router and the reseed/expiration tasks share the same state.
    let cdn_state = {
        let (wallet_address, wallet_private_key) = {
            let guard = runtime_state.wallet.lock().await;
            guard
                .as_ref()
                .map(|w| (w.address.clone(), w.private_key.clone()))
                .unwrap_or_default()
        };
        Arc::new(
            chiral_network::cdn_server::CdnState::new(
                wallet_address,
                wallet_private_key,
                Arc::clone(&runtime_state.dht),
            )
            .await,
        )
    };

    let app = base_router
        .merge(headless_routes(Arc::clone(&runtime_state)))
        .merge(chiral_network::cdn_server::router(Arc::clone(&cdn_state)))
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
    {
        // Loud network log so an operator restarting a CDN server can see
        // immediately which chain it's bound to. Override at launch with
        // `CHIRAL_NETWORK=testnet ./chiral_daemon ...`.
        let net = chiral_network::network::active();
        println!(
            "Network: {} (chain id {}, datadir {})",
            net.display_name,
            net.chain_id,
            chiral_network::network::data_dir().display(),
        );
    }

    // Auto-start services if requested
    let auto_mine = args.auto_mine;
    let auto_start_dht = args.auto_start_dht || auto_mine;
    let auto_start_geth = args.auto_start_geth || auto_mine;
    let miner_address = args.miner_address.clone();
    let mining_threads = args.mining_threads;

    // (Wallet pre-populate moved earlier — see above.)

    if auto_start_dht || auto_start_geth || auto_mine {
        let rt = Arc::clone(&runtime_state);
        tokio::spawn(async move {
            // Start DHT
            if auto_start_dht {
                println!("[AUTO] Starting DHT...");
                let dht_for_bootstrap = {
                    let mut guard = rt.dht.lock().await;
                    if guard.is_none() {
                        let ft = Arc::clone(&rt.file_transfer);
                        let dd = Arc::clone(&rt.download_directory);
                        let dc = Arc::clone(&rt.download_credentials);
                        let svc = Arc::new(dht::DhtService::new(ft, dd, dc));
                        match svc.start_headless().await {
                            Ok(_) => {
                                *guard = Some(svc.clone());
                                println!("[AUTO] DHT started successfully");
                                Some(svc)
                            }
                            Err(e) => {
                                eprintln!("[AUTO] DHT start failed: {}", e);
                                None
                            }
                        }
                    } else {
                        guard.as_ref().cloned()
                    }
                };

                if let Some(dht) = dht_for_bootstrap.as_ref() {
                    if !dht
                        .wait_for_bootstrap_ready(std::time::Duration::from_secs(180))
                        .await
                    {
                        eprintln!(
                            "[AUTO] DHT bootstrap did not complete within 180s; continuing startup"
                        );
                    }
                }

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
                    let mut geth = rt.geth.lock().await;
                    let _ = geth.set_miner_address(addr).await;
                    drop(geth);
                }

                println!(
                    "[AUTO] Starting mining with {} thread(s)...",
                    mining_threads
                );
                let geth = rt.geth.lock().await;
                match geth.start_mining(mining_threads).await {
                    Ok(()) => println!("[AUTO] Mining started successfully"),
                    Err(e) => eprintln!("[AUTO] Mining start failed: {}", e),
                }
            }
        });
    }

    // CDN background tasks (startup reseed + 60s expiration sweep) — logic
    // lives in crate::cdn_server. See also the router merge above.
    {
        let cdn_state = Arc::clone(&cdn_state);
        tokio::spawn(chiral_network::cdn_server::reseed_on_startup(cdn_state));
    }
    {
        let cdn_state = Arc::clone(&cdn_state);
        tokio::spawn(chiral_network::cdn_server::expiration_loop(cdn_state));
    }
    // Kademlia handles provider-record republishing on its configured
    // interval, so the manual CDN republish loop from the legacy blob
    // schema is no longer needed.

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
            let mut sigterm = match install_daemon_sigterm_handler() {
                Ok(handler) => handler,
                Err(e) => {
                    eprintln!("SIGTERM shutdown handling disabled: {}", e);
                    ctrl_c.await.ok();
                    println!("Received SIGINT");
                    return;
                }
            };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_wallet_address_validation_rejects_missing_value() {
        let err =
            validate_headless_wallet_address(None).expect_err("missing address should be rejected");

        assert_eq!(err, "address required");
    }

    #[test]
    fn headless_wallet_address_validation_rejects_malformed_value() {
        let err = validate_headless_wallet_address(Some("0x1234"))
            .expect_err("short address should be rejected");

        assert!(err.contains("20-byte"));
    }

    #[test]
    fn headless_wallet_address_validation_accepts_valid_value() {
        let address = "0x1111111111111111111111111111111111111111";

        assert_eq!(
            validate_headless_wallet_address(Some(address)).unwrap(),
            address
        );
    }

    #[tokio::test]
    async fn wallet_history_rejects_missing_address_before_rpc() {
        let response = wallet_history(
            State(Arc::new(HeadlessRuntimeState::new())),
            Json(json!({})),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn wallet_faucet_rejects_malformed_address_before_request() {
        let response = wallet_faucet(Json(json!({
            "address": "0x1234",
        })))
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    fn host_registry_entry(
        peer_id: &str,
        wallet_address: &str,
        updated_at: u64,
    ) -> HostRegistryEntry {
        HostRegistryEntry {
            peer_id: peer_id.to_string(),
            wallet_address: wallet_address.to_string(),
            updated_at,
        }
    }

    fn wallet_address_from_private_key(private_key: &str) -> String {
        let probe = b"daemon-register-shared-file";
        let signature = chiral_network::wallet::sign_message(private_key, probe).unwrap();
        chiral_network::wallet::recover_signer(probe, &signature).unwrap()
    }

    fn register_shared_file_request(
        protocol: Option<String>,
        private_key: &str,
    ) -> RegisterSharedFileRequest {
        RegisterSharedFileRequest {
            file_hash: "abcdef0123456789".to_string(),
            file_path: "/tmp/example.txt".to_string(),
            file_name: "example.txt".to_string(),
            file_size: 123,
            protocol,
            price_wei: "1000".to_string(),
            wallet_address: wallet_address_from_private_key(private_key),
            private_key: private_key.to_string(),
        }
    }

    #[test]
    fn headless_host_registry_missing_value_starts_empty() {
        let registry =
            host_registry_from_dht_value(None).expect("missing registry should be empty");

        assert!(registry.is_empty());
    }

    #[test]
    fn headless_host_registry_reads_valid_json() {
        let json = serde_json::to_string(&vec![host_registry_entry("peer-a", "0xwallet", 10)])
            .expect("test registry should serialize");
        let registry =
            host_registry_from_dht_value(Some(json)).expect("valid registry should parse");

        assert_eq!(registry.len(), 1);
        assert_eq!(registry[0].peer_id, "peer-a");
        assert_eq!(registry[0].wallet_address, "0xwallet");
        assert_eq!(registry[0].updated_at, 10);
    }

    #[test]
    fn headless_host_registry_rejects_malformed_json() {
        let err = host_registry_from_dht_value(Some("{not json".to_string()))
            .expect_err("malformed registry must not be reset to empty");

        assert!(err.contains("Malformed chiral_host_registry JSON"));
    }

    #[test]
    fn headless_host_registry_publish_replaces_existing_peer() {
        let registry = host_registry_after_publish(
            vec![
                host_registry_entry("peer-a", "0xold", 10),
                host_registry_entry("peer-b", "0xother", 20),
            ],
            "peer-a".to_string(),
            "0xnew".to_string(),
            30,
        );

        assert_eq!(registry.len(), 2);
        assert!(registry
            .iter()
            .any(|entry| entry.peer_id == "peer-b" && entry.wallet_address == "0xother"));
        let updated = registry
            .iter()
            .find(|entry| entry.peer_id == "peer-a")
            .expect("peer-a should be replaced");
        assert_eq!(updated.wallet_address, "0xnew");
        assert_eq!(updated.updated_at, 30);
    }

    #[test]
    fn register_shared_file_protocol_defaults_to_webrtc() {
        assert_eq!(register_shared_file_protocol(None), "WebRTC");
        assert_eq!(register_shared_file_protocol(Some("   ")), "WebRTC");
        assert_eq!(register_shared_file_protocol(Some("iroh")), "iroh");
    }

    #[test]
    fn register_shared_file_unpublished_payload_reports_false() {
        let payload = register_shared_file_unpublished_payload("metadata publish failed");

        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["dhtPublished"], false);
        assert_eq!(payload["warning"], "metadata publish failed");
    }

    #[test]
    fn register_shared_file_metadata_preserves_protocol() {
        let private_key = "0x4c0883a69102937d6231471b5dbb6204fe512961708279cea2c89f1f7a0f2c4f";
        let req = register_shared_file_request(Some("iroh".to_string()), private_key);
        let protocol = register_shared_file_protocol(req.protocol.as_deref());

        let metadata_json = signed_file_metadata_json_for_register(&req, &protocol)
            .expect("valid signed metadata should serialize");
        let metadata: serde_json::Value = serde_json::from_str(&metadata_json).unwrap();

        assert_eq!(metadata["protocol"], "iroh");
        assert_eq!(metadata["walletAddress"], req.wallet_address);
        assert!(metadata["publisherSignature"]
            .as_str()
            .map(|signature| !signature.is_empty())
            .unwrap_or(false));
    }

    #[test]
    fn register_shared_file_metadata_rejects_signing_failure() {
        let private_key = "0x4c0883a69102937d6231471b5dbb6204fe512961708279cea2c89f1f7a0f2c4f";
        let mut req = register_shared_file_request(Some("WebRTC".to_string()), private_key);
        req.private_key = "not-a-private-key".to_string();

        let err = signed_file_metadata_json_for_register(&req, "WebRTC")
            .expect_err("invalid private key should not produce metadata");

        assert_eq!(err, "Failed to sign file metadata");
    }

    #[cfg(unix)]
    #[test]
    fn daemon_sigterm_handler_accepts_registered_handler() {
        let handler = daemon_sigterm_handler_from_result(Ok("sigterm"))
            .expect("successful handler result should pass through");

        assert_eq!(handler, "sigterm");
    }

    #[cfg(unix)]
    #[test]
    fn daemon_sigterm_handler_reports_install_failure() {
        let err = daemon_sigterm_handler_from_result::<&str>(Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "sigterm unavailable",
        )))
        .expect_err("SIGTERM handler failure should be reported");

        assert!(err.contains("SIGTERM"));
        assert!(err.contains("sigterm unavailable"));
    }

    #[test]
    fn geth_log_lines_validation_defaults_absent_to_100() {
        assert_eq!(
            validate_geth_log_lines(None).unwrap(),
            DEFAULT_GETH_LOG_LINES
        );
    }

    #[test]
    fn geth_log_lines_validation_accepts_valid_count() {
        assert_eq!(validate_geth_log_lines(Some(25)).unwrap(), 25);
    }

    #[test]
    fn geth_log_lines_validation_rejects_zero() {
        let err = validate_geth_log_lines(Some(0)).expect_err("zero lines should be rejected");

        assert!(err.contains("at least 1"));
    }

    #[test]
    fn geth_log_lines_validation_rejects_over_limit_count() {
        let err = validate_geth_log_lines(Some(MAX_GETH_LOG_LINES + 1))
            .expect_err("over-limit lines should be rejected");

        assert!(err.contains(&MAX_GETH_LOG_LINES.to_string()));
    }

    #[test]
    fn tail_log_lines_returns_requested_tail() {
        assert_eq!(tail_log_lines("one\ntwo\nthree\nfour", 2), "three\nfour");
    }

    #[test]
    fn tail_log_lines_returns_all_lines_when_under_limit() {
        assert_eq!(tail_log_lines("one\ntwo", 100), "one\ntwo");
    }

    #[tokio::test]
    async fn geth_logs_rejects_zero_lines_before_file_read() {
        let response = geth_logs(Query(LogsQuery { lines: Some(0) })).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn geth_logs_rejects_over_limit_lines_before_file_read() {
        let response = geth_logs(Query(LogsQuery {
            lines: Some(MAX_GETH_LOG_LINES + 1),
        }))
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
