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

use crate::manager::{ChunkManager, FileManifest};

/// HTTP Server for serving encrypted file chunks and decryption keys
///
/// Architecture:
/// - GET /chunks/{encrypted_hash} → Returns encrypted chunk data
/// - GET /files/{merkle_root}/manifest → Returns file manifest with key bundle
/// - GET /files/{merkle_root}/key → Returns just the encrypted key bundle
///
/// Security:
/// - All chunks are served encrypted (security at rest)
/// - Keys are encrypted with recipient's public key (ECIES)
/// - Simple HTTP for now (HTTPS can be added later)

#[derive(Clone)]
pub struct HttpServerState {
    /// Maps merkle_root → FileManifest (includes encrypted_key_bundle)
    pub manifests: Arc<RwLock<HashMap<String, FileManifest>>>,

    /// Path to chunk storage directory
    pub chunk_storage_path: PathBuf,

    /// ChunkManager for reading chunks from disk
    pub chunk_manager: Arc<ChunkManager>,
}

impl HttpServerState {
    pub fn new(chunk_storage_path: PathBuf) -> Self {
        Self {
            manifests: Arc::new(RwLock::new(HashMap::new())),
            chunk_storage_path: chunk_storage_path.clone(),
            chunk_manager: Arc::new(ChunkManager::new(chunk_storage_path)),
        }
    }

    /// Register a file manifest so it can be served via HTTP
    /// Should be called after successful upload
    pub async fn register_manifest(&self, merkle_root: String, manifest: FileManifest) {
        let mut manifests = self.manifests.write().await;
        manifests.insert(merkle_root.clone(), manifest);
        tracing::info!("Registered file manifest for HTTP serving: {}", merkle_root);
    }

    /// Unregister a file (e.g., when user stops seeding)
    pub async fn unregister_manifest(&self, merkle_root: &str) {
        let mut manifests = self.manifests.write().await;
        manifests.remove(merkle_root);
        tracing::info!("Unregistered file manifest: {}", merkle_root);
    }
}

/// Response for manifest endpoint
#[derive(Serialize, Deserialize)]
pub struct ManifestResponse {
    pub merkle_root: String,
    pub chunks: Vec<ChunkInfoResponse>,
    pub encrypted_key_bundle: Option<String>, // JSON serialized EncryptedAesKeyBundle
    pub total_size: usize,
}

#[derive(Serialize, Deserialize)]
pub struct ChunkInfoResponse {
    pub index: u32,
    pub encrypted_hash: String, // Hash to use for fetching chunk
    pub encrypted_size: usize,
}

/// Response for key-only endpoint
#[derive(Serialize, Deserialize)]
pub struct KeyResponse {
    pub merkle_root: String,
    pub encrypted_key_bundle: String, // JSON serialized
}

/// Error response
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ============================================================================
// HTTP Handlers
// ============================================================================

/// GET /chunks/{encrypted_hash}
///
/// Serves an encrypted chunk by its hash
///
/// Flow:
/// 1. Client requests chunk by encrypted_hash (from manifest)
/// 2. Server reads encrypted chunk from disk
/// 3. Server returns raw encrypted bytes
async fn serve_chunk(
    Path(encrypted_hash): Path<String>,
    State(state): State<Arc<HttpServerState>>,
) -> Response {
    tracing::debug!("Serving chunk: {}", encrypted_hash);

    // Read encrypted chunk from disk (includes [nonce][ciphertext])
    match state.chunk_manager.read_chunk(&encrypted_hash) {
        Ok(encrypted_data) => {
            tracing::info!("Served chunk {} ({} bytes)", encrypted_hash, encrypted_data.len());

            // Return raw bytes with appropriate content type
            (StatusCode::OK, encrypted_data).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to read chunk {}: {}", encrypted_hash, e);

            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("Chunk not found: {}", encrypted_hash),
                }),
            )
                .into_response()
        }
    }
}

/// GET /files/{merkle_root}/manifest
///
/// Returns file manifest including chunk list and encrypted key bundle
///
/// This is the primary endpoint clients use to start a download:
/// 1. Client queries DHT for merkle_root → gets seeder's HTTP URL
/// 2. Client fetches manifest from seeder
/// 3. Client gets list of chunks + encrypted key
/// 4. Client downloads chunks in parallel
/// 5. Client decrypts chunks with key
async fn serve_manifest(
    Path(merkle_root): Path<String>,
    State(state): State<Arc<HttpServerState>>,
) -> Response {
    tracing::debug!("Serving manifest for: {}", merkle_root);

    let manifests = state.manifests.read().await;

    match manifests.get(&merkle_root) {
        Some(manifest) => {
            let total_size: usize = manifest.chunks.iter().map(|c| c.encrypted_size).sum();

            let response = ManifestResponse {
                merkle_root: manifest.merkle_root.clone(),
                chunks: manifest
                    .chunks
                    .iter()
                    .map(|c| ChunkInfoResponse {
                        index: c.index,
                        encrypted_hash: c.encrypted_hash.clone(),
                        encrypted_size: c.encrypted_size,
                    })
                    .collect(),
                encrypted_key_bundle: manifest
                    .encrypted_key_bundle
                    .as_ref()
                    .map(|bundle| serde_json::to_string(bundle).unwrap_or_default()),
                total_size,
            };

            tracing::info!(
                "Served manifest for {} ({} chunks, {} bytes total)",
                merkle_root,
                response.chunks.len(),
                total_size
            );

            (StatusCode::OK, Json(response)).into_response()
        }
        None => {
            tracing::warn!("Manifest not found: {}", merkle_root);

            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("File not found: {}", merkle_root),
                }),
            )
                .into_response()
        }
    }
}

/// GET /files/{merkle_root}/key
///
/// Returns only the encrypted key bundle (without full manifest)
///
/// Useful for cases where client already has chunk list but needs key
async fn serve_key(
    Path(merkle_root): Path<String>,
    State(state): State<Arc<HttpServerState>>,
) -> Response {
    tracing::debug!("Serving key for: {}", merkle_root);

    let manifests = state.manifests.read().await;

    match manifests.get(&merkle_root) {
        Some(manifest) => {
            match &manifest.encrypted_key_bundle {
                Some(bundle) => {
                    let response = KeyResponse {
                        merkle_root: merkle_root.clone(),
                        encrypted_key_bundle: serde_json::to_string(bundle)
                            .unwrap_or_default(),
                    };

                    tracing::info!("Served key for {}", merkle_root);
                    (StatusCode::OK, Json(response)).into_response()
                }
                None => {
                    tracing::warn!("No key bundle for file: {}", merkle_root);

                    (
                        StatusCode::NOT_FOUND,
                        Json(ErrorResponse {
                            error: "No encryption key available for this file".to_string(),
                        }),
                    )
                        .into_response()
                }
            }
        }
        None => {
            tracing::warn!("File not found: {}", merkle_root);

            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("File not found: {}", merkle_root),
                }),
            )
                .into_response()
        }
    }
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
        .route("/chunks/:encrypted_hash", get(serve_chunk))
        .route("/files/:merkle_root/manifest", get(serve_manifest))
        .route("/files/:merkle_root/key", get(serve_key))
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
) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound_addr = listener.local_addr()?;

    tracing::info!("HTTP server listening on http://{}", bound_addr);
    tracing::info!("Endpoints:");
    tracing::info!("  GET /health");
    tracing::info!("  GET /chunks/:encrypted_hash");
    tracing::info!("  GET /files/:merkle_root/manifest");
    tracing::info!("  GET /files/:merkle_root/key");

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

    #[tokio::test]
    async fn test_health_check() {
        let state = Arc::new(HttpServerState::new(PathBuf::from("/tmp/test_chunks")));
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
}
