//! Relay-side share and site registry with HTTP reverse proxy.
//!
//! The relay never stores file data. It only keeps mappings from share tokens
//! and site IDs to the origin URL of the owner's local server. When a visitor
//! requests a share link or hosted site on the relay, the relay proxies the
//! request to the owner's local server in real time. If the owner is offline,
//! an error page is shown.

use axum::{
    body::Body,
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

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

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Registration API handlers
// ---------------------------------------------------------------------------

/// POST /api/drive/relay-register — register a share origin
async fn register_share(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Json(req): Json<RegisterRequest>,
) -> Response {
    if req.token.is_empty() || req.origin_url.is_empty() {
        return (StatusCode::BAD_REQUEST, "token and origin_url required").into_response();
    }
    println!(
        "[RELAY-SHARE] Registering share token={} origin={}",
        req.token, req.origin_url
    );
    state
        .register(ShareRegistration {
            token: req.token,
            origin_url: req.origin_url,
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
// Reverse proxy handlers
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

/// Proxy GET /drive/:token to the sharer's local server.
async fn proxy_share_root(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path(token): Path<String>,
    Query(q): Query<ProxyQuery>,
) -> Response {
    let reg = match state.lookup(&token).await {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, Html(offline_page("Share link not found"))).into_response(),
    };

    let qs = build_query_string(&q.params);
    let target = format!("{}/drive/{}{}", reg.origin_url, token, qs);
    proxy_request(&target).await
}

/// Proxy GET /drive/:token/*path to the sharer's local server.
async fn proxy_share_path(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path((token, subpath)): Path<(String, String)>,
    Query(q): Query<ProxyQuery>,
) -> Response {
    let reg = match state.lookup(&token).await {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, Html(offline_page("Share link not found"))).into_response(),
    };

    let qs = build_query_string(&q.params);
    let target = format!("{}/drive/{}/{}{}", reg.origin_url, token, subpath, qs);
    proxy_request(&target).await
}

/// Forward a GET request to the target URL and stream the response back.
async fn proxy_request(target: &str) -> Response {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default();

    let upstream = match client.get(target).send().await {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(offline_page("The file owner is currently offline. Please try again later.")),
            )
                .into_response();
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    // Forward relevant headers (convert reqwest HeaderValue -> axum HeaderValue)
    let mut headers = axum::http::HeaderMap::new();
    for key in &["content-type", "content-length", "content-disposition"] {
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
// Site registration API handlers
// ---------------------------------------------------------------------------

/// POST /api/sites/relay-register — register a site origin
async fn register_site(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Json(req): Json<SiteRegisterRequest>,
) -> Response {
    if req.site_id.is_empty() || req.origin_url.is_empty() {
        return (StatusCode::BAD_REQUEST, "site_id and origin_url required").into_response();
    }
    println!(
        "[RELAY-SITE] Registering site={} origin={}",
        req.site_id, req.origin_url
    );
    state
        .register_site(SiteRegistration {
            site_id: req.site_id,
            origin_url: req.origin_url,
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
    // Check if site is registered before redirecting
    if state.lookup_site(&site_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Html(offline_page("Site not found")),
        )
            .into_response();
    }
    // Redirect to trailing slash (same as local hosting server)
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
    let target = format!("{}/sites/{}/", reg.origin_url, site_id);
    proxy_request(&target).await
}

/// Proxy GET /sites/:site_id/*path to the owner's local server.
async fn proxy_site_path(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
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
    let target = format!("{}/sites/{}/{}", reg.origin_url, site_id, subpath);
    proxy_request(&target).await
}

// ---------------------------------------------------------------------------
// HTML template
// ---------------------------------------------------------------------------

fn offline_page(msg: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Drive</title>
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
pub fn relay_share_routes(state: Arc<RelayShareRegistry>) -> Router {
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
        .layer(Extension(state))
}
