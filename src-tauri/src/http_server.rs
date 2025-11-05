use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

/// HTTP Server for serving files via Range requests
///
/// Simplified Architecture (no pre-chunking):
/// - GET /health → Health check
/// - GET /files/{file_hash} → Serve file (supports Range header for partial downloads)
/// - GET /files/{file_hash}/metadata → Returns file metadata (name, size, encrypted status)
///
/// This approach:
/// - Stores whole files (not pre-chunked)
/// - Uses HTTP Range requests for chunking on-demand
/// - Simpler than manifest-based chunking
/// - Aligns with professor feedback (PR #543)

/// File metadata for HTTP serving
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpFileMetadata {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub encrypted: bool,
}

#[derive(Clone)]
pub struct HttpServerState {
    /// Path to file storage directory (where whole files are stored)
    /// This is the same directory used by FileTransferService
    pub storage_dir: PathBuf,

    /// Maps file_hash → HttpFileMetadata
    /// Tracks which files are available for HTTP download
    pub files: Arc<RwLock<HashMap<String, HttpFileMetadata>>>,
}

impl HttpServerState {
    /// Create new HTTP server state
    ///
    /// The storage_dir should point to the FileTransferService storage directory
    /// (e.g., ~/.local/share/chiral-network/files/)
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage_dir,
            files: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a file for HTTP serving
    ///
    /// This should be called after a file is successfully uploaded and stored
    /// in the storage directory. The file should exist at: storage_dir/{metadata.hash}
    pub async fn register_file(&self, metadata: HttpFileMetadata) {
        let mut files = self.files.write().await;
        files.insert(metadata.hash.clone(), metadata.clone());
        tracing::info!(
            "Registered file for HTTP serving: {} ({})",
            metadata.name,
            metadata.hash
        );
    }

    /// Unregister a file (e.g., when user stops seeding)
    pub async fn unregister_file(&self, file_hash: &str) {
        let mut files = self.files.write().await;
        if let Some(metadata) = files.remove(file_hash) {
            tracing::info!("Unregistered file: {} ({})", metadata.name, file_hash);
        }
    }

    /// Get file metadata
    pub async fn get_file_metadata(&self, file_hash: &str) -> Option<HttpFileMetadata> {
        let files = self.files.read().await;
        files.get(file_hash).cloned()
    }

    /// Check if a file is registered
    pub async fn has_file(&self, file_hash: &str) -> bool {
        let files = self.files.read().await;
        files.contains_key(file_hash)
    }
}

/// Error response
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ============================================================================
// HTTP Handlers
// ============================================================================

/// GET /files/{file_hash}/metadata
///
/// Returns file metadata (name, size, encrypted status)
async fn serve_metadata(
    Path(file_hash): Path<String>,
    State(state): State<Arc<HttpServerState>>,
) -> Response {
    tracing::debug!("Serving metadata for: {}", file_hash);

    match state.get_file_metadata(&file_hash).await {
        Some(metadata) => {
            tracing::info!("Served metadata for {}: {}", file_hash, metadata.name);
            (StatusCode::OK, Json(metadata)).into_response()
        }
        None => {
            tracing::warn!("Metadata not found: {}", file_hash);
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("File not found: {}", file_hash),
                }),
            )
                .into_response()
        }
    }
}

/// GET /files/{file_hash}
///
/// Serves a file with support for HTTP Range requests
///
/// This allows clients to download specific byte ranges, enabling:
/// - Parallel chunk downloads from the same file
/// - Resume capability
/// - Bandwidth management
///
/// If no Range header is provided, returns the entire file.
async fn serve_file(
    Path(file_hash): Path<String>,
    State(state): State<Arc<HttpServerState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    tracing::debug!("Serving file: {}", file_hash);

    // Check if file is registered
    let metadata = match state.get_file_metadata(&file_hash).await {
        Some(m) => m,
        None => {
            tracing::warn!("File not registered: {}", file_hash);
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("File not found: {}", file_hash),
                }),
            )
                .into_response();
        }
    };

    // Build file path
    let file_path = state.storage_dir.join(&file_hash);

    // Check if file exists on disk
    if !file_path.exists() {
        tracing::error!(
            "File registered but not found on disk: {} at {:?}",
            file_hash,
            file_path
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "File not found on disk".to_string(),
            }),
        )
            .into_response();
    }

    // Check for Range header
    let range_header = headers
        .get("range")
        .and_then(|v| v.to_str().ok());

    if let Some(range_str) = range_header {
        // Serve partial content (Range request)
        serve_file_range(&file_path, range_str, metadata.size).await
    } else {
        // Serve entire file
        serve_entire_file(&file_path, metadata.size).await
    }
}

/// Serve a byte range from a file (206 Partial Content)
async fn serve_file_range(
    file_path: &PathBuf,
    range_str: &str,
    file_size: u64,
) -> Response {
    use tokio::fs::File;
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    // Parse Range header: "bytes=start-end"
    let (start, end) = match parse_range_header(range_str, file_size) {
        Some(range) => range,
        None => {
            tracing::warn!("Invalid Range header: {}", range_str);
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                Json(ErrorResponse {
                    error: "Invalid Range header".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Open file
    let mut file = match File::open(file_path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Failed to open file {:?}: {}", file_path, e);
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    // Seek to start position
    if let Err(e) = file.seek(tokio::io::SeekFrom::Start(start)).await {
        tracing::error!("Failed to seek in file: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
    }

    // Read chunk
    let chunk_size = (end - start + 1) as usize;
    let mut buffer = vec![0u8; chunk_size];

    match file.read_exact(&mut buffer).await {
        Ok(_) => {
            tracing::debug!(
                "Serving range {}-{} of {:?} ({} bytes)",
                start,
                end,
                file_path,
                chunk_size
            );

            // Return 206 Partial Content
            (
                StatusCode::PARTIAL_CONTENT,
                [
                    (
                        "Content-Range",
                        format!("bytes {}-{}/{}", start, end, file_size),
                    ),
                    ("Content-Length", chunk_size.to_string()),
                    ("Accept-Ranges", "bytes".to_string()),
                ],
                buffer,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to read file: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
    }
}

/// Serve the entire file (200 OK)
async fn serve_entire_file(file_path: &PathBuf, file_size: u64) -> Response {
    match tokio::fs::read(file_path).await {
        Ok(data) => {
            tracing::debug!("Serving entire file {:?} ({} bytes)", file_path, data.len());

            (
                StatusCode::OK,
                [
                    ("Content-Length", data.len().to_string()),
                    ("Accept-Ranges", "bytes".to_string()),
                ],
                data,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to read file {:?}: {}", file_path, e);
            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
    }
}

/// Parse HTTP Range header
///
/// Supports formats:
/// - "bytes=0-999" → Some((0, 999))
/// - "bytes=1000-" → Some((1000, file_size-1))
/// - "bytes=-500" → Not supported (returns None)
fn parse_range_header(range_str: &str, file_size: u64) -> Option<(u64, u64)> {
    let bytes_str = range_str.strip_prefix("bytes=")?;
    let mut parts = bytes_str.split('-');

    let start_str = parts.next()?;
    let end_str = parts.next()?;

    // Parse start
    let start: u64 = if start_str.is_empty() {
        // Suffix range (e.g., "bytes=-500") - not supported
        return None;
    } else {
        start_str.parse().ok()?
    };

    // Parse end
    let end: u64 = if end_str.is_empty() {
        // Open-ended range (e.g., "bytes=1000-")
        file_size - 1
    } else {
        end_str.parse().ok()?
    };

    // Validate range
    if start > end || start >= file_size {
        return None;
    }

    // Clamp end to file size
    let end = std::cmp::min(end, file_size - 1);

    Some((start, end))
}

/// GET /health
///
/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

// ============================================================================
// Server Setup
// ============================================================================

/// Creates the HTTP server router with all endpoints
pub fn create_router(state: Arc<HttpServerState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/files/:file_hash", get(serve_file))
        .route("/files/:file_hash/metadata", get(serve_metadata))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

/// Starts the HTTP server on the specified address
///
/// Returns the server's actual bound address (useful if port 0 was used)
pub async fn start_server(
    state: Arc<HttpServerState>,
    addr: SocketAddr,
) -> Result<SocketAddr, String> {
    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

    tracing::info!("HTTP server listening on http://{}", bound_addr);
    tracing::info!("Endpoints:");
    tracing::info!("  GET /health");
    tracing::info!("  GET /files/:file_hash (supports Range header)");
    tracing::info!("  GET /files/:file_hash/metadata");

    // Spawn server in background
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("HTTP server error: {}", e);
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
        let state = Arc::new(HttpServerState::new(PathBuf::from("/tmp/test_files")));
        let app = create_router(state);

        let response = app
            .clone()
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

    #[test]
    fn test_parse_range_header() {
        // Standard range
        assert_eq!(
            parse_range_header("bytes=0-262143", 1048576),
            Some((0, 262143))
        );

        // Open-ended range
        assert_eq!(
            parse_range_header("bytes=1000-", 2000),
            Some((1000, 1999))
        );

        // Range beyond file size (clamped)
        assert_eq!(
            parse_range_header("bytes=0-999999", 1000),
            Some((0, 999))
        );

        // Invalid ranges
        assert_eq!(parse_range_header("bytes=-500", 1000), None);
        assert_eq!(parse_range_header("bytes=2000-", 1000), None);
    }
}
