use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

/// File metadata stored in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HttpFileMetadata {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub uploader_address: String,
    pub upload_time: u64,
    pub download_url: String,
}

/// Shared state for the HTTP file server
#[derive(Clone)]
pub struct HttpFileServerState {
    /// File storage directory
    pub storage_dir: PathBuf,
    /// File registry: hash -> metadata
    pub file_registry: Arc<Mutex<HashMap<String, HttpFileMetadata>>>,
    /// Server address (for generating download URLs)
    pub server_address: String,
}

impl HttpFileServerState {
    pub fn new(storage_dir: PathBuf, server_address: String) -> Self {
        Self {
            storage_dir,
            file_registry: Arc::new(Mutex::new(HashMap::new())),
            server_address,
        }
    }
}

/// HTTP File Server for decentralized file sharing
pub struct HttpFileServer {
    state: HttpFileServerState,
    addr: SocketAddr,
}

impl HttpFileServer {
    pub fn new(port: u16, storage_dir: PathBuf) -> Self {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let server_address = format!("http://0.0.0.0:{}", port);

        Self {
            state: HttpFileServerState::new(storage_dir, server_address),
            addr,
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        // Ensure storage directory exists
        tokio::fs::create_dir_all(&self.state.storage_dir).await?;

        // Build router
        let app = Router::new()
            .route("/upload", post(upload_file))
            .route("/download/:file_hash", get(download_file))
            .route("/files", get(list_files))
            .route("/file/:file_hash", get(get_file_metadata))
            .route("/health", get(health_check))
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any),
            )
            .with_state(self.state);

        info!("🌐 HTTP File Server listening on {}", self.addr);

        // Start server
        let listener = tokio::net::TcpListener::bind(&self.addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "chiral-network-http-server"
    }))
}

/// Upload file endpoint
async fn upload_file(
    State(state): State<HttpFileServerState>,
    mut multipart: Multipart,
) -> Result<Json<HttpFileMetadata>, StatusCode> {
    let mut file_name = String::new();
    let mut file_data = Vec::new();

    // Parse multipart form data
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "file" => {
                file_name = field
                    .file_name()
                    .unwrap_or("unknown")
                    .to_string();
                file_data = field
                    .bytes()
                    .await
                    .map_err(|_| StatusCode::BAD_REQUEST)?
                    .to_vec();
            }
            _ => {}
        }
    }

    if file_name.is_empty() || file_data.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Calculate file hash (SHA-256)
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let file_hash = format!("{:x}", hasher.finalize());

    // Save file to storage
    let file_path = state.storage_dir.join(&file_hash);
    tokio::fs::write(&file_path, &file_data)
        .await
        .map_err(|e| {
            error!("Failed to save file: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Create metadata
    let metadata = HttpFileMetadata {
        file_hash: file_hash.clone(),
        file_name: file_name.clone(),
        file_size: file_data.len() as u64,
        uploader_address: "self".to_string(), // TODO: Add actual address
        upload_time: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        download_url: format!("{}/download/{}", state.server_address, file_hash),
    };

    // Store in registry
    {
        let mut registry = state.file_registry.lock().await;
        registry.insert(file_hash.clone(), metadata.clone());
    }

    info!("✅ File uploaded: {} (hash: {})", file_name, file_hash);

    Ok(Json(metadata))
}

/// Download file endpoint
async fn download_file(
    State(state): State<HttpFileServerState>,
    Path(file_hash): Path<String>,
) -> Result<Bytes, StatusCode> {
    let file_path = state.storage_dir.join(&file_hash);

    if !file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let file_data = tokio::fs::read(&file_path)
        .await
        .map_err(|e| {
            error!("Failed to read file: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("📥 File downloaded: {}", file_hash);

    Ok(Bytes::from(file_data))
}

/// List all files endpoint
async fn list_files(
    State(state): State<HttpFileServerState>,
) -> Json<Vec<HttpFileMetadata>> {
    let registry = state.file_registry.lock().await;
    let files: Vec<HttpFileMetadata> = registry.values().cloned().collect();
    Json(files)
}

/// Get file metadata endpoint
async fn get_file_metadata(
    State(state): State<HttpFileServerState>,
    Path(file_hash): Path<String>,
) -> Result<Json<HttpFileMetadata>, StatusCode> {
    let registry = state.file_registry.lock().await;

    registry
        .get(&file_hash)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
