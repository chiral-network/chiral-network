//! Relay-side share and site registry with HTTP reverse proxy + WebSocket tunnel.
//!
//! The relay never stores file data. It keeps mappings from share tokens and
//! site IDs to the owner's local server. When a visitor requests content:
//!
//! 1. If the owner has an active WebSocket tunnel, the request is forwarded
//!    through the tunnel (works behind NAT without port forwarding).
//! 2. Otherwise, the relay tries a direct HTTP proxy to the origin URL.
//! 3. If both fail, an offline error page is shown.

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket},
        ConnectInfo, Extension, Path, Query, WebSocketUpgrade,
    },
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct ShareRegistration {
    pub token: String,
    pub origin_url: String,
    pub owner_wallet: String,
    pub registered_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SiteRegistration {
    pub site_id: String,
    pub origin_url: String,
    pub owner_wallet: String,
    pub registered_at: u64,
}

#[derive(Clone)]
pub struct RelayShareRegistry {
    pub shares: Arc<RwLock<HashMap<String, ShareRegistration>>>,
    pub sites: Arc<RwLock<HashMap<String, SiteRegistration>>>,
    persist_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedRegistry {
    shares: Vec<ShareRegistration>,
    #[serde(default)]
    sites: Vec<SiteRegistration>,
}

impl RelayShareRegistry {
    pub fn new(data_dir: PathBuf) -> Self {
        let persist_path = data_dir.join("chiral-relay-shares").join("registry.json");
        Self {
            shares: Arc::new(RwLock::new(HashMap::new())),
            sites: Arc::new(RwLock::new(HashMap::new())),
            persist_path,
        }
    }

    pub async fn load_from_disk(&self) {
        if let Ok(data) = std::fs::read_to_string(&self.persist_path) {
            if let Ok(reg) = serde_json::from_str::<PersistedRegistry>(&data) {
                let mut share_map = self.shares.write().await;
                for s in reg.shares {
                    share_map.insert(s.token.clone(), s);
                }
                let share_count = share_map.len();
                drop(share_map);

                let mut site_map = self.sites.write().await;
                for s in reg.sites {
                    site_map.insert(s.site_id.clone(), s);
                }
                let site_count = site_map.len();
                drop(site_map);

                println!(
                    "[RELAY-SHARE] Loaded {} share + {} site registrations from disk",
                    share_count, site_count
                );
            }
        }
    }

    async fn persist(&self) {
        let share_map = self.shares.read().await;
        let site_map = self.sites.read().await;
        let reg = PersistedRegistry {
            shares: share_map.values().cloned().collect(),
            sites: site_map.values().cloned().collect(),
        };
        drop(share_map);
        drop(site_map);
        if let Some(parent) = self.persist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&reg) {
            let _ = std::fs::write(&self.persist_path, json);
        }
    }

    // --- Share methods ---

    pub async fn register(&self, reg: ShareRegistration) {
        let mut map = self.shares.write().await;
        map.insert(reg.token.clone(), reg);
        drop(map);
        self.persist().await;
    }

    pub async fn unregister(&self, token: &str) -> bool {
        let mut map = self.shares.write().await;
        let removed = map.remove(token).is_some();
        drop(map);
        if removed {
            self.persist().await;
        }
        removed
    }

    pub async fn lookup(&self, token: &str) -> Option<ShareRegistration> {
        let map = self.shares.read().await;
        map.get(token).cloned()
    }

    // --- Site methods ---

    pub async fn register_site(&self, reg: SiteRegistration) {
        let mut map = self.sites.write().await;
        map.insert(reg.site_id.clone(), reg);
        drop(map);
        self.persist().await;
    }

    pub async fn unregister_site(&self, site_id: &str) -> bool {
        let mut map = self.sites.write().await;
        let removed = map.remove(site_id).is_some();
        drop(map);
        if removed {
            self.persist().await;
        }
        removed
    }

    pub async fn lookup_site(&self, site_id: &str) -> Option<SiteRegistration> {
        let map = self.sites.read().await;
        map.get(site_id).cloned()
    }
}

// ---------------------------------------------------------------------------
// WebSocket tunnel registry
// ---------------------------------------------------------------------------

/// A pending tunnel request: the relay sends a TunnelRequest over the WS and
/// waits on the oneshot for the client's TunnelResponse.
type TunnelResponder = oneshot::Sender<TunnelResponse>;

/// Messages sent relay → client over the WebSocket.
#[derive(Serialize, Deserialize)]
struct TunnelRequest {
    id: String,
    path: String,
}

/// Messages sent client → relay over the WebSocket.
#[derive(Serialize, Deserialize)]
struct TunnelResponse {
    id: String,
    status: u16,
    #[serde(default)]
    headers: HashMap<String, String>,
    /// Base64-encoded body
    body: String,
}

/// Active tunnel: a sender half of an mpsc channel to push requests into the
/// WebSocket writer task, which forwards them to the connected client.
type TunnelSender = tokio::sync::mpsc::Sender<(TunnelRequest, TunnelResponder)>;

/// Global registry of active tunnels keyed by resource key (e.g. "site:abc" or
/// "share:xyz").
pub struct TunnelRegistry {
    tunnels: RwLock<HashMap<String, TunnelSender>>,
}

impl TunnelRegistry {
    pub fn new() -> Self {
        Self {
            tunnels: RwLock::new(HashMap::new()),
        }
    }

    async fn register(&self, key: String, sender: TunnelSender) {
        self.tunnels.write().await.insert(key, sender);
    }

    async fn unregister(&self, key: &str) {
        self.tunnels.write().await.remove(key);
    }

    /// Send a request through the tunnel and wait for the response.
    async fn request(&self, key: &str, path: String) -> Option<TunnelResponse> {
        let sender = {
            let map = self.tunnels.read().await;
            map.get(key).cloned()
        };
        let sender = sender?;

        let id = uuid::Uuid::new_v4().to_string();
        let (resp_tx, resp_rx) = oneshot::channel();

        let req = TunnelRequest {
            id: id.clone(),
            path,
        };

        if sender.send((req, resp_tx)).await.is_err() {
            return None;
        }

        // Wait up to 30s for the client to respond
        match tokio::time::timeout(std::time::Duration::from_secs(30), resp_rx).await {
            Ok(Ok(resp)) => Some(resp),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Request/response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RegisterRequest {
    token: String,
    origin_url: String,
    owner_wallet: String,
}

#[derive(Deserialize)]
struct SiteRegisterRequest {
    site_id: String,
    origin_url: String,
    owner_wallet: String,
}

#[derive(Deserialize)]
struct ProxyQuery {
    #[serde(flatten)]
    params: HashMap<String, String>,
}

#[derive(Deserialize)]
struct TunnelQuery {
    /// "site" or "share"
    #[serde(rename = "type")]
    resource_type: String,
    /// The site_id or share token
    id: String,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Registration API handlers
// ---------------------------------------------------------------------------

/// Replace 0.0.0.0 or 127.0.0.1 in origin URL with the client's real IP.
/// e.g. "http://0.0.0.0:9419" + client_ip 1.2.3.4 → "http://1.2.3.4:9419"
fn fix_origin_url(origin_url: &str, client_ip: std::net::IpAddr) -> String {
    for placeholder in &["0.0.0.0", "127.0.0.1", "localhost"] {
        if origin_url.contains(placeholder) {
            return origin_url.replace(placeholder, &client_ip.to_string());
        }
    }
    origin_url.to_string()
}

/// POST /api/drive/relay-register — register a share origin
async fn register_share(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<RegisterRequest>,
) -> Response {
    if req.token.is_empty() || req.origin_url.is_empty() {
        return (StatusCode::BAD_REQUEST, "token and origin_url required").into_response();
    }
    let origin = fix_origin_url(&req.origin_url, addr.ip());
    println!(
        "[RELAY-SHARE] Registering share token={} origin={} (raw={})",
        req.token, origin, req.origin_url
    );
    state
        .register(ShareRegistration {
            token: req.token,
            origin_url: origin,
            owner_wallet: req.owner_wallet,
            registered_at: now_secs(),
        })
        .await;
    (StatusCode::OK, "Registered").into_response()
}

/// DELETE /api/drive/relay-register/:token — unregister a share
async fn unregister_share(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path(token): Path<String>,
) -> Response {
    if state.unregister(&token).await {
        println!("[RELAY-SHARE] Unregistered share token={}", token);
        (StatusCode::OK, "Unregistered").into_response()
    } else {
        (StatusCode::NOT_FOUND, "Share not found").into_response()
    }
}

// ---------------------------------------------------------------------------
// WebSocket tunnel endpoint
// ---------------------------------------------------------------------------

/// GET /api/tunnel/ws?type=site&id=xxx — WebSocket tunnel for NAT traversal.
///
/// The client (site/share owner) connects here after publishing. The relay
/// forwards incoming visitor requests through this WebSocket.
async fn tunnel_ws_handler(
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Query(q): Query<TunnelQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let key = format!("{}:{}", q.resource_type, q.id);
    println!("[TUNNEL] WebSocket upgrade for key={}", key);
    ws.on_upgrade(move |socket| handle_tunnel_ws(socket, key, tunnel_reg))
}

async fn handle_tunnel_ws(
    socket: WebSocket,
    key: String,
    tunnel_reg: Arc<TunnelRegistry>,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Channel for the proxy handlers to send requests into this tunnel
    let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<(TunnelRequest, TunnelResponder)>(32);

    tunnel_reg.register(key.clone(), req_tx).await;
    println!("[TUNNEL] Connected: {}", key);

    // Map of pending request IDs → responders
    let pending: Arc<RwLock<HashMap<String, TunnelResponder>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let pending_for_read = Arc::clone(&pending);

    // Task: read responses from the WebSocket client
    let read_key = key.clone();
    let read_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(resp) = serde_json::from_str::<TunnelResponse>(&text) {
                        let mut map = pending_for_read.write().await;
                        if let Some(tx) = map.remove(&resp.id) {
                            let _ = tx.send(resp);
                        }
                    }
                }
                Message::Close(_) => {
                    println!("[TUNNEL] Client closed: {}", read_key);
                    break;
                }
                _ => {}
            }
        }
    });

    // Task: forward requests from proxy handlers to the WebSocket client
    let write_task = tokio::spawn(async move {
        // Also send periodic pings to keep the connection alive
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                req = req_rx.recv() => {
                    match req {
                        Some((tunnel_req, responder)) => {
                            let id = tunnel_req.id.clone();
                            pending.write().await.insert(id, responder);
                            let json = serde_json::to_string(&tunnel_req).unwrap_or_default();
                            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = ping_interval.tick() => {
                    if ws_tx.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Wait for either task to finish (connection dropped)
    tokio::select! {
        _ = read_task => {}
        _ = write_task => {}
    }

    tunnel_reg.unregister(&key).await;
    println!("[TUNNEL] Disconnected: {}", key);
}

// ---------------------------------------------------------------------------
// Reverse proxy helpers
// ---------------------------------------------------------------------------

/// Build the query string from the flattened params map.
fn build_query_string(params: &HashMap<String, String>) -> String {
    if params.is_empty() {
        return String::new();
    }
    let qs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    format!("?{}", qs.join("&"))
}

/// Try the WebSocket tunnel first; if unavailable fall back to direct HTTP proxy.
async fn proxy_via_tunnel_or_http(
    tunnel_reg: &Arc<TunnelRegistry>,
    tunnel_key: &str,
    path: &str,
    direct_url: &str,
) -> Response {
    // Try tunnel first
    if let Some(resp) = tunnel_reg.request(tunnel_key, path.to_string()).await {
        return tunnel_response_to_axum(resp);
    }

    // Fall back to direct HTTP proxy (works if port is forwarded)
    proxy_request_direct(direct_url).await
}

/// Convert a TunnelResponse into an Axum HTTP response.
fn tunnel_response_to_axum(resp: TunnelResponse) -> Response {
    use base64::Engine;
    let status = StatusCode::from_u16(resp.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let body_bytes = base64::engine::general_purpose::STANDARD
        .decode(&resp.body)
        .unwrap_or_default();

    let mut headers = axum::http::HeaderMap::new();
    for (k, v) in &resp.headers {
        if let Ok(name) = axum::http::header::HeaderName::from_bytes(k.as_bytes()) {
            if let Ok(hv) = axum::http::HeaderValue::from_str(v) {
                headers.insert(name, hv);
            }
        }
    }

    (status, headers, body_bytes).into_response()
}

/// Forward a GET request to the target URL directly and stream the response back.
async fn proxy_request_direct(target: &str) -> Response {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let upstream = match client.get(target).send().await {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(offline_page(
                    "The owner is currently offline. Please try again later.",
                )),
            )
                .into_response();
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    // Forward relevant headers (convert reqwest HeaderValue -> axum HeaderValue)
    let mut headers = axum::http::HeaderMap::new();
    for key in &[
        "content-type",
        "content-length",
        "content-disposition",
        "cache-control",
        "etag",
    ] {
        if let Some(val) = upstream.headers().get(*key) {
            if let Ok(name) = axum::http::header::HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(hv) = axum::http::HeaderValue::from_bytes(val.as_bytes()) {
                    headers.insert(name, hv);
                }
            }
        }
    }

    // Stream the response body
    let stream = upstream.bytes_stream();
    let body = Body::from_stream(stream);

    (status, headers, body).into_response()
}

// ---------------------------------------------------------------------------
// Drive share proxy handlers
// ---------------------------------------------------------------------------

/// Proxy GET /drive/:token to the sharer's local server.
async fn proxy_share_root(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path(token): Path<String>,
    Query(q): Query<ProxyQuery>,
) -> Response {
    let reg = match state.lookup(&token).await {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Html(offline_page("Share link not found")),
            )
                .into_response()
        }
    };

    let qs = build_query_string(&q.params);
    let path = format!("/drive/{}{}", token, qs);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("share:{}", token);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

/// Proxy GET /drive/:token/*path to the sharer's local server.
async fn proxy_share_path(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path((token, subpath)): Path<(String, String)>,
    Query(q): Query<ProxyQuery>,
) -> Response {
    let reg = match state.lookup(&token).await {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Html(offline_page("Share link not found")),
            )
                .into_response()
        }
    };

    let qs = build_query_string(&q.params);
    let path = format!("/drive/{}/{}{}", token, subpath, qs);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("share:{}", token);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

// ---------------------------------------------------------------------------
// Site registration API handlers
// ---------------------------------------------------------------------------

/// POST /api/sites/relay-register — register a site origin
async fn register_site(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<SiteRegisterRequest>,
) -> Response {
    if req.site_id.is_empty() || req.origin_url.is_empty() {
        return (StatusCode::BAD_REQUEST, "site_id and origin_url required").into_response();
    }
    let origin = fix_origin_url(&req.origin_url, addr.ip());
    println!(
        "[RELAY-SITE] Registering site={} origin={} (raw={})",
        req.site_id, origin, req.origin_url
    );
    state
        .register_site(SiteRegistration {
            site_id: req.site_id,
            origin_url: origin,
            owner_wallet: req.owner_wallet,
            registered_at: now_secs(),
        })
        .await;
    (StatusCode::OK, "Registered").into_response()
}

/// DELETE /api/sites/relay-register/:site_id — unregister a site
async fn unregister_site(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path(site_id): Path<String>,
) -> Response {
    if state.unregister_site(&site_id).await {
        println!("[RELAY-SITE] Unregistered site={}", site_id);
        (StatusCode::OK, "Unregistered").into_response()
    } else {
        (StatusCode::NOT_FOUND, "Site not found").into_response()
    }
}

// ---------------------------------------------------------------------------
// Site reverse proxy handlers
// ---------------------------------------------------------------------------

/// Proxy GET /sites/:site_id to redirect (matching local server behavior).
async fn proxy_site_redirect(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path(site_id): Path<String>,
) -> Response {
    if state.lookup_site(&site_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Html(offline_page("Site not found")),
        )
            .into_response();
    }
    (
        StatusCode::MOVED_PERMANENTLY,
        [("Location", format!("/sites/{}/", site_id))],
        "",
    )
        .into_response()
}

/// Proxy GET /sites/:site_id/ to the owner's local server.
async fn proxy_site_root(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path(site_id): Path<String>,
) -> Response {
    let reg = match state.lookup_site(&site_id).await {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Html(offline_page("Site not found")),
            )
                .into_response()
        }
    };
    let path = format!("/sites/{}/", site_id);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("site:{}", site_id);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

/// Proxy GET /sites/:site_id/*path to the owner's local server.
async fn proxy_site_path(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path((site_id, subpath)): Path<(String, String)>,
) -> Response {
    let reg = match state.lookup_site(&site_id).await {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Html(offline_page("Site not found")),
            )
                .into_response()
        }
    };
    let path = format!("/sites/{}/{}", site_id, subpath);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("site:{}", site_id);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

// ---------------------------------------------------------------------------
// HTML template
// ---------------------------------------------------------------------------

fn offline_page(msg: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Network</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white flex items-center justify-center min-h-screen">
<div class="bg-gray-800 rounded-xl p-8 max-w-md w-full mx-4 shadow-2xl text-center">
<div class="w-16 h-16 bg-gray-700 rounded-full flex items-center justify-center mx-auto mb-4">
<svg class="w-8 h-8 text-red-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18.364 5.636a9 9 0 010 12.728M5.636 18.364a9 9 0 010-12.728M12 9v4m0 4h.01"/></svg>
</div>
<h1 class="text-xl font-bold mb-2">Unavailable</h1>
<p class="text-gray-400 text-sm mb-4">{}</p>
<p class="text-xs text-gray-500">Shared via Chiral Network</p>
</div></body></html>"#,
        msg
    )
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the relay share proxy router. Uses Extension for state injection.
pub fn relay_share_routes(
    state: Arc<RelayShareRegistry>,
    tunnel_reg: Arc<TunnelRegistry>,
) -> Router {
    Router::new()
        // Drive share registration API
        .route("/api/drive/relay-register", post(register_share))
        .route(
            "/api/drive/relay-register/:token",
            delete(unregister_share),
        )
        // Drive share proxy routes
        .route("/drive/:token", get(proxy_share_root))
        .route("/drive/:token/*path", get(proxy_share_path))
        // Site registration API
        .route("/api/sites/relay-register", post(register_site))
        .route(
            "/api/sites/relay-register/:site_id",
            delete(unregister_site),
        )
        // Site proxy routes
        .route("/sites/:site_id", get(proxy_site_redirect))
        .route("/sites/:site_id/", get(proxy_site_root))
        .route("/sites/:site_id/*path", get(proxy_site_path))
        // WebSocket tunnel
        .route("/api/tunnel/ws", get(tunnel_ws_handler))
        .layer(Extension(state))
        .layer(Extension(tunnel_reg))
}
