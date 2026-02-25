use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::hosting::{self, HostedSite};

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
// Handlers
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
// Router & Server
// ---------------------------------------------------------------------------

/// Build the Axum router.
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

    println!("🌐 Hosting server started on http://{}", bound_addr);

    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            shutdown_rx.await.ok();
            println!("🌐 Hosting server received shutdown signal");
        });

        if let Err(e) = server.await {
            eprintln!("Hosting server error: {}", e);
        } else {
            println!("🌐 Hosting server shut down gracefully");
        }
    });

    Ok(bound_addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_health_check() {
        let state = Arc::new(HostingServerState::new());
        let app = create_router(state);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
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
                axum::http::Request::builder()
                    .uri("/sites/nonexistent/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_serves_html_file() {
        let tmp = std::env::temp_dir().join("chiral_test_site");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("index.html"), "<h1>Hello</h1>").unwrap();

        let state = Arc::new(HostingServerState::new());
        state.register_site(HostedSite {
            id: "testsite".into(),
            name: "Test".into(),
            directory: tmp.to_string_lossy().into(),
            created_at: 0,
            files: vec![hosting::SiteFile { path: "index.html".into(), size: 14 }],
        }).await;

        let app = create_router(state);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/sites/testsite/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let headers = response.headers();
        let ct = headers.get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/html"));

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_path_traversal_blocked() {
        let tmp = std::env::temp_dir().join("chiral_test_site_traversal");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("index.html"), "ok").unwrap();

        let state = Arc::new(HostingServerState::new());
        state.register_site(HostedSite {
            id: "travsite".into(),
            name: "Trav".into(),
            directory: tmp.to_string_lossy().into(),
            created_at: 0,
            files: vec![],
        }).await;

        let app = create_router(state);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/sites/travsite/../../etc/passwd")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should be 403 (traversal blocked) or 404, never 200
        assert_ne!(response.status(), StatusCode::OK);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_redirect_without_trailing_slash() {
        let state = Arc::new(HostingServerState::new());
        let app = create_router(state);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/sites/anysite")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        let loc = response.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(loc, "/sites/anysite/");
    }
}
