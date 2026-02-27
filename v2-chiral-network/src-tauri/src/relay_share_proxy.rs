//! Relay-side share registry and HTTP reverse proxy.
//!
//! The relay never stores file data. It only keeps a mapping from share tokens
//! to the origin URL of the sharer's local server. When a downloader visits a
//! share link on the relay, the relay proxies the request to the sharer's
//! local server in real time. If the sharer is offline, an error page is shown.

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

#[derive(Clone)]
pub struct RelayShareRegistry {
    pub shares: Arc<RwLock<HashMap<String, ShareRegistration>>>,
    persist_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedRegistry {
    shares: Vec<ShareRegistration>,
}

impl RelayShareRegistry {
    pub fn new(data_dir: PathBuf) -> Self {
        let persist_path = data_dir.join("chiral-relay-shares").join("registry.json");
        Self {
            shares: Arc::new(RwLock::new(HashMap::new())),
            persist_path,
        }
    }

    pub async fn load_from_disk(&self) {
        if let Ok(data) = std::fs::read_to_string(&self.persist_path) {
            if let Ok(reg) = serde_json::from_str::<PersistedRegistry>(&data) {
                let mut map = self.shares.write().await;
                for s in reg.shares {
                    map.insert(s.token.clone(), s);
                }
                println!(
                    "[RELAY-SHARE] Loaded {} share registrations from disk",
                    map.len()
                );
            }
        }
    }

    async fn persist(&self) {
        let map = self.shares.read().await;
        let reg = PersistedRegistry {
            shares: map.values().cloned().collect(),
        };
        if let Some(parent) = self.persist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&reg) {
            let _ = std::fs::write(&self.persist_path, json);
        }
    }

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
        // Registration API
        .route("/api/drive/relay-register", post(register_share))
        .route(
            "/api/drive/relay-register/:token",
            delete(unregister_share),
        )
        // Proxy routes — forward to sharer's local server
        .route("/drive/:token", get(proxy_share_root))
        .route("/drive/:token/*path", get(proxy_share_path))
        .layer(Extension(state))
}
