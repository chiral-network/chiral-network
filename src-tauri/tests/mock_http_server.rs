// Mock HTTP Server for Download Fault Injection Testing
//
// This module provides a configurable HTTP server that can simulate various
// fault conditions for testing download resume/restart logic:
// - ETag changes mid-download
// - Missing Accept-Ranges header
// - 200 OK instead of 206 Partial Content for range requests
// - 416 Range Not Satisfiable responses
// - Content-Length mismatches
//
// Aligned with docs/download-restart.md specification

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

/// Configuration for fault injection behavior
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// Whether to include Accept-Ranges header
    pub support_ranges: bool,
    /// Whether to return 200 instead of 206 for range requests
    pub ignore_range_requests: bool,
    /// ETag value to return (None = no ETag)
    pub etag: Option<String>,
    /// Whether ETag is weak (W/ prefix)
    pub weak_etag: bool,
    /// Whether to emit 416 Range Not Satisfiable
    pub emit_416: bool,
    /// Content-Length override (None = use actual file size)
    pub content_length_override: Option<u64>,
    /// Last-Modified header value
    pub last_modified: Option<String>,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            support_ranges: true,
            ignore_range_requests: false,
            etag: Some("\"abc123\"".to_string()),
            weak_etag: false,
            emit_416: false,
            content_length_override: None,
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
        }
    }
}

/// Mock file metadata matching HttpFileMetadata in http_download.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockFileMetadata {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub encrypted: bool,
}

/// Shared server state
#[derive(Clone)]
struct ServerState {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    metadata: Arc<Mutex<HashMap<String, MockFileMetadata>>>,
    fault_config: Arc<Mutex<FaultConfig>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            metadata: Arc::new(Mutex::new(HashMap::new())),
            fault_config: Arc::new(Mutex::new(FaultConfig::default())),
        }
    }

    fn add_file(&self, hash: String, name: String, data: Vec<u8>, encrypted: bool) {
        let size = data.len() as u64;
        let metadata = MockFileMetadata {
            hash: hash.clone(),
            name,
            size,
            encrypted,
        };

        self.files.lock().unwrap().insert(hash.clone(), data);
        self.metadata.lock().unwrap().insert(hash, metadata);
    }

    fn update_fault_config(&self, config: FaultConfig) {
        *self.fault_config.lock().unwrap() = config;
    }

    fn get_fault_config(&self) -> FaultConfig {
        self.fault_config.lock().unwrap().clone()
    }
}

/// Query parameters for fault injection
#[derive(Deserialize)]
struct FaultQuery {
    flip_etag: Option<bool>,
    drop_ranges: Option<bool>,
    return_200: Option<bool>,
    emit_416: Option<bool>,
}

/// Handle metadata requests: GET /files/{hash}/metadata
async fn handle_metadata(
    State(state): State<ServerState>,
    Path(hash): Path<String>,
) -> Response {
    let metadata_map = state.metadata.lock().unwrap();

    if let Some(metadata) = metadata_map.get(&hash) {
        let body = serde_json::to_string(&metadata).unwrap();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("File not found"))
            .unwrap()
    }
}

/// Handle file download requests: GET /files/{hash}
/// Supports Range requests and fault injection
async fn handle_file_download(
    State(state): State<ServerState>,
    Path(hash): Path<String>,
    headers: HeaderMap,
    Query(query): Query<FaultQuery>,
) -> Response {
    let files = state.files.lock().unwrap();
    let metadata_map = state.metadata.lock().unwrap();

    // Check if file exists
    let file_data = match files.get(&hash) {
        Some(data) => data,
        None => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("File not found"))
                .unwrap();
        }
    };

    let metadata = metadata_map.get(&hash).unwrap();
    let mut config = state.get_fault_config();

    // Apply query parameter overrides
    if query.drop_ranges.unwrap_or(false) {
        config.support_ranges = false;
    }
    if query.return_200.unwrap_or(false) {
        config.ignore_range_requests = true;
    }
    if query.emit_416.unwrap_or(false) {
        config.emit_416 = true;
    }
    if query.flip_etag.unwrap_or(false) {
        config.etag = Some("\"xyz789\"".to_string());
    }

    // Check for Range header
    let range_header = headers.get(header::RANGE);

    if let Some(range_value) = range_header {
        // Range request received
        let range_str = range_value.to_str().unwrap_or("");

        // Emit 416 if configured
        if config.emit_416 {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{}", metadata.size))
                .body(Body::empty())
                .unwrap();
        }

        // Return 200 instead of 206 if configured (invalid behavior)
        if config.ignore_range_requests {
            return build_full_response(&config, metadata, file_data);
        }

        // Parse range (format: "bytes=start-end")
        if let Some(byte_range) = range_str.strip_prefix("bytes=") {
            let parts: Vec<&str> = byte_range.split('-').collect();
            if parts.len() == 2 {
                let start: u64 = parts[0].parse().unwrap_or(0);
                let end: u64 = if parts[1].is_empty() {
                    metadata.size - 1
                } else {
                    parts[1].parse().unwrap_or(metadata.size - 1)
                };

                // Validate range
                if start >= metadata.size {
                    return Response::builder()
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .header(header::CONTENT_RANGE, format!("bytes */{}", metadata.size))
                        .body(Body::empty())
                        .unwrap();
                }

                let actual_end = std::cmp::min(end, metadata.size - 1);
                let range_data = file_data[start as usize..=actual_end as usize].to_vec();

                return build_partial_response(&config, metadata, start, actual_end, range_data);
            }
        }
    }

    // No range header - return full file
    build_full_response(&config, metadata, file_data)
}

/// Build 206 Partial Content response
fn build_partial_response(
    config: &FaultConfig,
    metadata: &MockFileMetadata,
    start: u64,
    end: u64,
    data: Vec<u8>,
) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_LENGTH, data.len())
        .header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, metadata.size),
        );

    // Add Accept-Ranges if supported
    if config.support_ranges {
        builder = builder.header(header::ACCEPT_RANGES, "bytes");
    }

    // Add ETag if configured
    if let Some(ref etag) = config.etag {
        let etag_value = if config.weak_etag {
            format!("W/{}", etag)
        } else {
            etag.clone()
        };
        builder = builder.header(header::ETAG, etag_value);
    }

    // Add Last-Modified if configured
    if let Some(ref last_modified) = config.last_modified {
        builder = builder.header(header::LAST_MODIFIED, last_modified.as_str());
    }

    builder.body(Body::from(data)).unwrap()
}

/// Build 200 OK response for full file
fn build_full_response(
    config: &FaultConfig,
    metadata: &MockFileMetadata,
    data: &[u8],
) -> Response {
    let content_length = config
        .content_length_override
        .unwrap_or(metadata.size);

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, content_length);

    // Add Accept-Ranges if supported
    if config.support_ranges {
        builder = builder.header(header::ACCEPT_RANGES, "bytes");
    }

    // Add ETag if configured
    if let Some(ref etag) = config.etag {
        let etag_value = if config.weak_etag {
            format!("W/{}", etag)
        } else {
            etag.clone()
        };
        builder = builder.header(header::ETAG, etag_value);
    }

    // Add Last-Modified if configured
    if let Some(ref last_modified) = config.last_modified {
        builder = builder.header(header::LAST_MODIFIED, last_modified.as_str());
    }

    builder.body(Body::from(data.to_vec())).unwrap()
}

/// Mock HTTP server for testing
pub struct MockHttpServer {
    state: ServerState,
    port: u16,
}

impl MockHttpServer {
    /// Create a new mock server (not yet started)
    pub fn new() -> Self {
        Self {
            state: ServerState::new(),
            port: 0, // Will be assigned when started
        }
    }

    /// Add a file to the mock server
    pub fn add_file(&mut self, hash: String, name: String, data: Vec<u8>, encrypted: bool) {
        self.state.add_file(hash, name, data, encrypted);
    }

    /// Update fault injection configuration
    pub fn set_fault_config(&mut self, config: FaultConfig) {
        self.state.update_fault_config(config);
    }

    /// Start the server and return the base URL
    pub async fn start(mut self) -> Result<(String, tokio::task::JoinHandle<()>), String> {
        let app = Router::new()
            .route("/files/:hash/metadata", get(handle_metadata))
            .route("/files/:hash", get(handle_file_download))
            .with_state(self.state.clone());

        // Bind to random available port
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind server: {}", e))?;

        let addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local address: {}", e))?;

        self.port = addr.port();
        let base_url = format!("http://127.0.0.1:{}", self.port);

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok((base_url, handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_server_creation() {
        let mut server = MockHttpServer::new();
        server.add_file(
            "test123".to_string(),
            "test.txt".to_string(),
            b"Hello, World!".to_vec(),
            false,
        );

        let (base_url, _handle) = server.start().await.unwrap();
        assert!(base_url.starts_with("http://127.0.0.1:"));
    }

    #[tokio::test]
    async fn test_fault_config_defaults() {
        let config = FaultConfig::default();
        assert!(config.support_ranges);
        assert!(!config.ignore_range_requests);
        assert_eq!(config.etag, Some("\"abc123\"".to_string()));
        assert!(!config.weak_etag);
    }
}
