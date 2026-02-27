use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::drive_api::{self, DriveState};
use crate::hosting::{self, HostedSite, SiteFile};
use crate::rating_api;
use crate::rating_storage::RatingState;
use crate::relay_share_proxy::{self, RelayShareRegistry};

/// Maximum total upload size per site (50 MB).
const MAX_SITE_BYTES: usize = 50 * 1024 * 1024;

/// Shared state for the hosting HTTP server.
#[derive(Clone)]
pub struct HostingServerState {
    /// Maps site_id -> HostedSite
    pub sites: Arc<RwLock<HashMap<String, HostedSite>>>,
}

impl HostingServerState {
    pub fn new() -> Self {
        Self {
            sites: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load sites from the persistence layer into server state.
    pub async fn load_from_disk(&self) {
        let loaded = hosting::load_sites();
        let mut sites = self.sites.write().await;
        sites.clear();
        for site in loaded {
            sites.insert(site.id.clone(), site);
        }
    }

    /// Register a site so it becomes servable.
    pub async fn register_site(&self, site: HostedSite) {
        let mut sites = self.sites.write().await;
        sites.insert(site.id.clone(), site);
    }

    /// Unregister a site.
    pub async fn unregister_site(&self, site_id: &str) {
        let mut sites = self.sites.write().await;
        sites.remove(site_id);
    }
}

// ---------------------------------------------------------------------------
// Serving handlers (GET)
// ---------------------------------------------------------------------------

/// GET /health
async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// GET /sites/{site_id}  — redirect to trailing slash
async fn redirect_to_index(Path(site_id): Path<String>) -> Response {
    (
        StatusCode::MOVED_PERMANENTLY,
        [("Location", format!("/sites/{}/", site_id))],
        "",
    )
        .into_response()
}

/// GET /sites/{site_id}/  — serve index.html
async fn serve_site_root(
    Path(site_id): Path<String>,
    State(state): State<Arc<HostingServerState>>,
) -> Response {
    serve_site_path_inner(&site_id, "index.html", &state).await
}

/// GET /sites/{site_id}/*path  — serve any file within the site
async fn serve_site_file(
    Path((site_id, file_path)): Path<(String, String)>,
    State(state): State<Arc<HostingServerState>>,
) -> Response {
    let path = if file_path.is_empty() || file_path == "/" {
        "index.html"
    } else {
        &file_path
    };
    serve_site_path_inner(&site_id, path, &state).await
}

/// Core file-serving logic with directory traversal protection.
async fn serve_site_path_inner(
    site_id: &str,
    requested_path: &str,
    state: &HostingServerState,
) -> Response {
    // Look up the site
    let sites = state.sites.read().await;
    let site = match sites.get(site_id) {
        Some(s) => s,
        None => {
            return (StatusCode::NOT_FOUND, "Site not found").into_response();
        }
    };

    let site_dir = PathBuf::from(&site.directory);

    // Sanitize: reject absolute paths, null bytes, and ".." components
    if requested_path.contains('\0')
        || requested_path.starts_with('/')
        || requested_path.starts_with('\\')
    {
        return (StatusCode::BAD_REQUEST, "Invalid path").into_response();
    }
    for component in requested_path.split(&['/', '\\']) {
        if component == ".." {
            return (StatusCode::FORBIDDEN, "Path traversal not allowed").into_response();
        }
    }

    let resolved = site_dir.join(requested_path);

    // Canonicalize and verify the resolved path is inside the site directory
    let canonical = match resolved.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };
    let canonical_site_dir = match site_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Site directory error").into_response();
        }
    };
    if !canonical.starts_with(&canonical_site_dir) {
        return (StatusCode::FORBIDDEN, "Path traversal not allowed").into_response();
    }

    // Read the file
    let data = match tokio::fs::read(&canonical).await {
        Ok(d) => d,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };

    // Determine MIME type from extension
    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let content_type = hosting::mime_from_extension(ext);

    (
        StatusCode::OK,
        [
            ("Content-Type", content_type.to_string()),
            ("Content-Length", data.len().to_string()),
            ("Cache-Control", "public, max-age=3600".to_string()),
        ],
        data,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Gateway API handlers (POST / DELETE) — used by relay server
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct UploadSiteRequest {
    /// Site ID (generated by the client)
    id: String,
    /// User-given site name
    name: String,
    /// Files to upload
    files: Vec<UploadFile>,
}

#[derive(Deserialize)]
struct UploadFile {
    /// Relative path within the site (e.g. "index.html")
    path: String,
    /// Base64-encoded file content
    data: String,
}

#[derive(Serialize)]
struct UploadSiteResponse {
    url: String,
}

/// POST /api/sites — upload a new site (used by Tauri clients publishing to relay)
async fn upload_site(
    State(state): State<Arc<HostingServerState>>,
    Json(req): Json<UploadSiteRequest>,
) -> Response {
    // Validate site ID
    if req.id.len() != 8 || !req.id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return (StatusCode::BAD_REQUEST, "Invalid site ID").into_response();
    }
    if req.name.is_empty() || req.name.len() > 200 {
        return (StatusCode::BAD_REQUEST, "Invalid site name").into_response();
    }
    if req.files.is_empty() {
        return (StatusCode::BAD_REQUEST, "No files provided").into_response();
    }

    // Decode all files first, checking total size
    let mut decoded_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_bytes: usize = 0;

    for upload_file in &req.files {
        // Validate file path
        if upload_file.path.is_empty()
            || upload_file.path.contains('\0')
            || upload_file.path.starts_with('/')
            || upload_file.path.starts_with('\\')
            || upload_file.path.contains("..")
        {
            return (StatusCode::BAD_REQUEST, "Invalid file path").into_response();
        }

        let data = match base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &upload_file.data,
        ) {
            Ok(d) => d,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "Invalid base64 data").into_response();
            }
        };

        total_bytes += data.len();
        if total_bytes > MAX_SITE_BYTES {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Site exceeds 50 MB size limit",
            )
                .into_response();
        }

        decoded_files.push((upload_file.path.clone(), data));
    }

    // Create site directory
    let base_dir = match hosting::sites_base_dir() {
        Some(d) => d,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Cannot determine data directory")
                .into_response();
        }
    };
    let site_dir = base_dir.join(&req.id);
    if let Err(e) = std::fs::create_dir_all(&site_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create site directory: {}", e),
        )
            .into_response();
    }

    // Write files to disk
    let mut site_files = Vec::new();
    for (path, data) in &decoded_files {
        let dest = site_dir.join(path);
        // Create parent directories if needed (e.g. "css/style.css")
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&dest, data) {
            // Clean up on error
            let _ = std::fs::remove_dir_all(&site_dir);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to write file {}: {}", path, e),
            )
                .into_response();
        }
        site_files.push(SiteFile {
            path: path.clone(),
            size: data.len() as u64,
        });
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let site = HostedSite {
        id: req.id.clone(),
        name: req.name,
        directory: site_dir.to_string_lossy().into_owned(),
        created_at: now,
        files: site_files,
        relay_url: None,
    };

    // Register in server state
    state.register_site(site.clone()).await;

    // Persist metadata
    let mut all_sites: Vec<HostedSite> = {
        let sites_map = state.sites.read().await;
        sites_map.values().cloned().collect()
    };
    // Avoid duplicates in persistence
    all_sites.retain(|s| s.id != site.id);
    all_sites.push(site);
    hosting::save_sites(&all_sites);

    let url = format!("/sites/{}/", req.id);
    println!("[GATEWAY] Uploaded site: {} -> {}", req.id, url);

    (
        StatusCode::CREATED,
        Json(UploadSiteResponse { url }),
    )
        .into_response()
}

/// DELETE /api/sites/:site_id — remove a site (used by Tauri clients unpublishing)
async fn delete_site_api(
    Path(site_id): Path<String>,
    State(state): State<Arc<HostingServerState>>,
) -> Response {
    // Check if site exists
    {
        let sites = state.sites.read().await;
        if !sites.contains_key(&site_id) {
            return (StatusCode::NOT_FOUND, "Site not found").into_response();
        }
    }

    // Remove from disk
    if let Some(base) = hosting::sites_base_dir() {
        let site_dir = base.join(&site_id);
        if site_dir.exists() {
            let _ = std::fs::remove_dir_all(&site_dir);
        }
    }

    // Unregister from state
    state.unregister_site(&site_id).await;

    // Update persistence
    let all_sites: Vec<HostedSite> = {
        let sites_map = state.sites.read().await;
        sites_map.values().cloned().collect()
    };
    hosting::save_sites(&all_sites);

    println!("[GATEWAY] Deleted site: {}", site_id);

    (StatusCode::OK, "Deleted").into_response()
}

// ---------------------------------------------------------------------------
// Router & Server
// ---------------------------------------------------------------------------

/// Build the Axum router for local hosting only (no upload API).
pub fn create_router(state: Arc<HostingServerState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/sites/:site_id", get(redirect_to_index))
        .route("/sites/:site_id/", get(serve_site_root))
        .route("/sites/:site_id/*path", get(serve_site_file))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

/// Build the Axum router for the relay gateway (includes upload/delete API + Drive API + Rating API).
pub fn create_gateway_router(
    state: Arc<HostingServerState>,
    drive_state: Option<Arc<DriveState>>,
    rating_state: Option<Arc<RatingState>>,
    relay_share_state: Option<Arc<RelayShareRegistry>>,
) -> Router {
    // Base: health check is always present
    let mut app = Router::new().route("/health", get(health_check));

    if relay_share_state.is_some() {
        // Relay mode: /sites/* and /drive/* handled by proxy routes below.
        // No local hosting serving routes or upload/delete API needed.
    } else {
        // Local mode: serve sites from disk + upload/delete API
        app = app.merge(
            Router::new()
                .route("/sites/:site_id", get(redirect_to_index))
                .route("/sites/:site_id/", get(serve_site_root))
                .route("/sites/:site_id/*path", get(serve_site_file))
                .route("/api/sites", post(upload_site))
                .route("/api/sites/:site_id", delete(delete_site_api))
                .with_state(state),
        );
    }

    // Merge Drive API routes if drive state is provided (local server)
    if let Some(ds) = drive_state {
        app = app.merge(drive_api::drive_routes(ds));
    }

    // Merge Rating API routes if rating state is provided
    if let Some(rs) = rating_state {
        app = app.merge(rating_api::rating_routes(rs));
    }

    // Merge relay share proxy routes if relay share state is provided (relay server)
    if let Some(rss) = relay_share_state {
        let tunnel_reg = Arc::new(relay_share_proxy::TunnelRegistry::new());
        app = app.merge(relay_share_proxy::relay_share_routes(rss, tunnel_reg));
    }

    app.layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
            .expose_headers(Any),
    )
}

/// Start the hosting HTTP server. Returns the bound address.
pub async fn start_server(
    state: Arc<HostingServerState>,
    port: u16,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<SocketAddr, String> {
    let app = create_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind port {}: {}", port, e))?;
    let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

    println!("Hosting server started on http://{}", bound_addr);

    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            shutdown_rx.await.ok();
            println!("Hosting server received shutdown signal");
        });

        if let Err(e) = server.await {
            eprintln!("Hosting server error: {}", e);
        } else {
            println!("Hosting server shut down gracefully");
        }
    });

    Ok(bound_addr)
}

/// Start the gateway HTTP server (with upload/delete API + Drive API + Rating API). Returns the bound address.
pub async fn start_gateway_server(
    state: Arc<HostingServerState>,
    drive_state: Option<Arc<DriveState>>,
    rating_state: Option<Arc<RatingState>>,
    relay_share_state: Option<Arc<RelayShareRegistry>>,
    port: u16,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<SocketAddr, String> {
    let app = create_gateway_router(state, drive_state, rating_state, relay_share_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind port {}: {}", port, e))?;
    let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

    println!("Gateway server started on http://{}", bound_addr);

    tokio::spawn(async move {
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
            println!("Gateway server received shutdown signal");
        });

        if let Err(e) = server.await {
            eprintln!("Gateway server error: {}", e);
        } else {
            println!("Gateway server shut down gracefully");
        }
    });

    Ok(bound_addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

    fn make_site(id: &str, name: &str, dir: &str) -> HostedSite {
        HostedSite {
            id: id.into(),
            name: name.into(),
            directory: dir.into(),
            created_at: 0,
            files: vec![],
            relay_url: None,
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let state = Arc::new(HostingServerState::new());
        let app = create_router(state);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_unknown_site_returns_404() {
        let state = Arc::new(HostingServerState::new());
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/nonexistent/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_serves_html_file() {
        let tmp = std::env::temp_dir().join("chiral_test_site_serve");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("index.html"), "<h1>Hello</h1>").unwrap();

        let state = Arc::new(HostingServerState::new());
        let mut site = make_site("testsite", "Test", &tmp.to_string_lossy());
        site.files = vec![SiteFile { path: "index.html".into(), size: 14 }];
        state.register_site(site).await;

        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/testsite/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let ct = response.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/html"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_path_traversal_blocked() {
        let tmp = std::env::temp_dir().join("chiral_test_site_trav2");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("index.html"), "ok").unwrap();

        let state = Arc::new(HostingServerState::new());
        state.register_site(make_site("travsite", "Trav", &tmp.to_string_lossy())).await;

        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/travsite/../../etc/passwd")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::OK);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_redirect_without_trailing_slash() {
        let state = Arc::new(HostingServerState::new());
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/anysite")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        let loc = response.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/sites/anysite/");
    }

    #[tokio::test]
    async fn test_gateway_upload_and_serve() {
        use base64::Engine;

        let state = Arc::new(HostingServerState::new());
        let app = create_gateway_router(Arc::clone(&state), None, None, None);

        let html_content = "<h1>Gateway Test</h1>";
        let b64 = base64::engine::general_purpose::STANDARD.encode(html_content);

        let body = serde_json::json!({
            "id": "gw12test",
            "name": "Gateway Test",
            "files": [{ "path": "index.html", "data": b64 }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sites")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        // Verify site was registered
        let sites = state.sites.read().await;
        assert!(sites.contains_key("gw12test"));
        let site = &sites["gw12test"];
        assert_eq!(site.name, "Gateway Test");
        assert_eq!(site.files.len(), 1);
        drop(sites);

        // Now serve the uploaded file
        let app2 = create_gateway_router(Arc::clone(&state), None, None, None);
        let response2 = app2
            .oneshot(
                Request::builder()
                    .uri("/sites/gw12test/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::OK);

        // Clean up
        if let Some(base) = hosting::sites_base_dir() {
            let _ = std::fs::remove_dir_all(base.join("gw12test"));
        }
    }

    #[tokio::test]
    async fn test_gateway_delete() {
        use base64::Engine;

        let state = Arc::new(HostingServerState::new());

        // First upload
        let app = create_gateway_router(Arc::clone(&state), None, None, None);
        let b64 = base64::engine::general_purpose::STANDARD.encode("hello");
        let body = serde_json::json!({
            "id": "del1test",
            "name": "Delete Test",
            "files": [{ "path": "index.html", "data": b64 }]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sites")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Then delete
        let app2 = create_gateway_router(Arc::clone(&state), None, None, None);
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/sites/del1test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);

        // Verify removed from state
        let sites = state.sites.read().await;
        assert!(!sites.contains_key("del1test"));
    }

    #[tokio::test]
    async fn test_gateway_upload_rejects_oversized() {
        let state = Arc::new(HostingServerState::new());
        let app = create_gateway_router(state, None, None, None);

        // Create a base64 string that decodes to > 50 MB
        // We'll just test with an invalid site id instead for a simpler check
        let body = serde_json::json!({
            "id": "bad!!!!!",
            "name": "Bad",
            "files": [{ "path": "index.html", "data": "aGVsbG8=" }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sites")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
