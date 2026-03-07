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
    #[arg(long, default_value_t = 9419)]
    port: u16,

    /// Optional PID file path
    #[arg(long)]
    pid_file: Option<PathBuf>,
}

#[derive(Clone)]
struct HeadlessRuntimeState {
    dht: Arc<Mutex<Option<Arc<dht::DhtService>>>>,
    file_transfer: Arc<Mutex<FileTransferService>>,
    download_tiers: dht::DownloadTiersMap,
    download_directory: dht::DownloadDirectoryRef,
    download_credentials: dht::DownloadCredentialsMap,
    geth: Arc<Mutex<GethProcess>>,
}

impl HeadlessRuntimeState {
    fn new() -> Self {
        Self {
            dht: Arc::new(Mutex::new(None)),
            file_transfer: Arc::new(Mutex::new(FileTransferService::new())),
            download_tiers: Arc::new(Mutex::new(HashMap::new())),
            download_directory: Arc::new(Mutex::new(None)),
            download_credentials: Arc::new(Mutex::new(HashMap::new())),
            geth: Arc::new(Mutex::new(GethProcess::new())),
        }
    }

    async fn dht_service(&self) -> Option<Arc<dht::DhtService>> {
        self.dht.lock().await.clone()
    }
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
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

fn read_geth_log(lines: Option<usize>) -> Result<String, String> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
        .join("geth");
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
        Arc::clone(&state.download_tiers),
        Arc::clone(&state.download_directory),
        Arc::clone(&state.download_credentials),
    ));

    match svc.start_headless().await {
        Ok(message) => {
            *guard = Some(svc);
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

fn headless_routes(state: Arc<HeadlessRuntimeState>) -> Router {
    Router::new()
        .route("/api/headless/runtime", get(runtime_status))
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
        .route("/api/headless/drop/inbox", get(drop_inbox))
        .route("/api/headless/drop/outgoing", get(drop_outgoing))
        .route("/api/headless/drop/accept", post(drop_accept))
        .route("/api/headless/drop/decline", post(drop_decline))
        .route("/api/headless/geth/install", post(geth_install))
        .route("/api/headless/geth/start", post(geth_start))
        .route("/api/headless/geth/stop", post(geth_stop))
        .route("/api/headless/geth/status", get(geth_status))
        .route("/api/headless/geth/logs", get(geth_logs))
        .route("/api/headless/mining/start", post(mining_start))
        .route("/api/headless/mining/stop", post(mining_stop))
        .route("/api/headless/mining/status", get(mining_status))
        .route("/api/headless/mining/blocks", get(mining_blocks))
        .route(
            "/api/headless/mining/miner-address",
            post(set_miner_address),
        )
        .with_state(state)
}

#[tokio::main]
async fn main() {
    let args = DaemonArgs::parse();
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

    if let Err(e) = tokio::signal::ctrl_c().await {
        eprintln!("Signal handler error: {}", e);
    }

    // Best-effort runtime cleanup.
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
}
