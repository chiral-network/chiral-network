use anyhow::{Context, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::fs;
use std::path::PathBuf;
use warp::hyper::StatusCode;
use warp::{Filter, Rejection, Reply};

#[derive(Serialize, Deserialize)]
pub struct ChunkUploadResponse {
    pub chunk_hash: String,
    pub size: usize,
    pub stored_at: u64,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

pub struct StorageNodeServer {
    storage_path: PathBuf,
    port: u16,
}

impl StorageNodeServer {
    pub fn new(storage_path: PathBuf, port: u16) -> Self {
        StorageNodeServer { storage_path, port }
    }

    /// Creates the warp filter chain for the storage API
    pub fn create_api(&self) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
        let storage_path = self.storage_path.clone();

        // CORS headers
        let cors = warp::cors()
            .allow_any_origin()
            .allow_headers(vec!["content-type", "x-chunk-hash"])
            .allow_methods(vec!["GET", "POST", "OPTIONS"]);

        let routes = self
            .store_chunk(storage_path.clone())
            .or(self.retrieve_chunk(storage_path.clone()))
            .or(self.list_chunks(storage_path.clone()))
            .or(self.health_check())
            .with(cors);

        routes
    }

    /// POST /chunks - Store a chunk
    fn store_chunk(
        &self,
        storage_path: PathBuf,
    ) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
        warp::path!("chunks")
            .and(warp::post())
            .and(warp::body::bytes())
            .and(warp::header::optional::<String>("x-chunk-hash"))
            .and(warp::any().map(move || storage_path.clone()))
            .and_then(handle_store_chunk)
    }

    /// GET /chunks/{hash} - Retrieve a chunk
    fn retrieve_chunk(
        &self,
        storage_path: PathBuf,
    ) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
        warp::path!("chunks" / String)
            .and(warp::get())
            .and(warp::any().map(move || storage_path.clone()))
            .and_then(handle_retrieve_chunk)
    }

    /// GET /chunks - List all chunks (for debugging)
    fn list_chunks(
        &self,
        storage_path: PathBuf,
    ) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
        warp::path!("chunks")
            .and(warp::get())
            .and(warp::any().map(move || storage_path.clone()))
            .and_then(handle_list_chunks)
    }

    /// GET /health - Health check endpoint
    fn health_check(&self) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
        warp::path!("health")
            .and(warp::get())
            .and_then(handle_health_check)
    }

    /// Start the storage node server
    pub async fn run(&self) -> Result<()> {
        // Ensure storage directory exists
        fs::create_dir_all(&self.storage_path).with_context(|| {
            format!(
                "Failed to create storage directory: {}",
                self.storage_path.display()
            )
        })?;

        let api = self.create_api();

        println!("Starting storage node server on port {}", self.port);
        println!("Storage path: {}", self.storage_path.display());

        warp::serve(api).run(([0, 0, 0, 0], self.port)).await;

        Ok(())
    }
}

/// Handles chunk storage requests
async fn handle_store_chunk(
    body: Bytes,
    provided_hash: Option<String>,
    storage_path: PathBuf,
) -> Result<impl Reply, Rejection> {
    if body.is_empty() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Empty chunk data".to_string(),
                code: 400,
            }),
            StatusCode::BAD_REQUEST,
        ));
    }

    // Calculate the hash of the received data
    let calculated_hash = calculate_chunk_hash(&body);

    // If a hash was provided, verify it matches
    if let Some(ref expected_hash) = provided_hash {
        if calculated_hash != *expected_hash {
            return Ok(warp::reply::with_status(
                warp::reply::json(&ErrorResponse {
                    error: "Chunk hash mismatch".to_string(),
                    code: 400,
                }),
                StatusCode::BAD_REQUEST,
            ));
        }
    }

    // Store the chunk
    match store_chunk_data(&storage_path, &calculated_hash, &body).await {
        Ok(()) => {
            let response = ChunkUploadResponse {
                chunk_hash: calculated_hash,
                size: body.len(),
                stored_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };

            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                StatusCode::CREATED,
            ))
        }
        Err(e) => {
            eprintln!("Failed to store chunk: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&ErrorResponse {
                    error: format!("Failed to store chunk: {}", e),
                    code: 500,
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handles chunk retrieval requests
async fn handle_retrieve_chunk(
    chunk_hash: String,
    storage_path: PathBuf,
) -> Result<Box<dyn Reply>, Rejection> {
    // Validate hash format (64 hex characters for SHA-256)
    if chunk_hash.len() != 64 || !chunk_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        let error_response = ErrorResponse {
            error: "Invalid chunk hash format".to_string(),
            code: 400,
        };

        return Ok(Box::new(warp::reply::with_status(
            warp::reply::json(&error_response),
            StatusCode::BAD_REQUEST,
        )));
    }

    match load_chunk_data(&storage_path, &chunk_hash).await {
        Ok(chunk_data) => {
            // Return the raw chunk data with appropriate headers
            Ok(Box::new(warp::reply::with_header(
                chunk_data,
                "content-type",
                "application/octet-stream",
            )))
        }
        Err(_) => {
            let error_response = ErrorResponse {
                error: "Chunk not found".to_string(),
                code: 404,
            };

            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&error_response),
                StatusCode::NOT_FOUND,
            )))
        }
    }
}

/// Handles listing all chunks (for debugging)
async fn handle_list_chunks(storage_path: PathBuf) -> Result<Box<dyn Reply>, Rejection> {
    match list_stored_chunks(&storage_path).await {
        Ok(chunks) => {
            #[derive(Serialize)]
            struct ChunkListResponse {
                chunks: Vec<String>,
                count: usize,
            }

            let response = ChunkListResponse {
                count: chunks.len(),
                chunks,
            };

            Ok(Box::new(warp::reply::json(&response)))
        }
        Err(e) => {
            eprintln!("Failed to list chunks: {}", e);

            let error_response = ErrorResponse {
                error: format!("Failed to list chunks: {}", e),
                code: 500,
            };

            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&error_response),
                StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }
}

/// Handles health check requests
async fn handle_health_check() -> Result<impl Reply, Rejection> {
    #[derive(Serialize)]
    struct HealthResponse {
        status: String,
        timestamp: u64,
        version: String,
    }

    let response = HealthResponse {
        status: "healthy".to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    Ok(warp::reply::json(&response))
}

/// Calculate SHA-256 hash of chunk data
fn calculate_chunk_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Store chunk data to filesystem
async fn store_chunk_data(storage_path: &PathBuf, chunk_hash: &str, data: &[u8]) -> Result<()> {
    let file_path = storage_path.join(chunk_hash);

    // Use tokio for async file operations
    tokio::fs::write(&file_path, data)
        .await
        .with_context(|| format!("Failed to write chunk to {}", file_path.display()))?;

    Ok(())
}

/// Load chunk data from filesystem
async fn load_chunk_data(storage_path: &PathBuf, chunk_hash: &str) -> Result<Vec<u8>> {
    let file_path = storage_path.join(chunk_hash);

    tokio::fs::read(&file_path)
        .await
        .with_context(|| format!("Failed to read chunk from {}", file_path.display()))
}

/// List all stored chunks
async fn list_stored_chunks(storage_path: &PathBuf) -> Result<Vec<String>> {
    let mut entries = tokio::fs::read_dir(storage_path)
        .await
        .context("Failed to read storage directory")?;

    let mut chunks = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .context("Failed to read directory entry")?
    {
        if entry
            .file_type()
            .await
            .context("Failed to get file type")?
            .is_file()
        {
            if let Some(file_name) = entry.file_name().to_str() {
                // Validate that it looks like a chunk hash
                if file_name.len() == 64 && file_name.chars().all(|c| c.is_ascii_hexdigit()) {
                    chunks.push(file_name.to_string());
                }
            }
        }
    }

    chunks.sort();
    Ok(chunks)
}

/// Handle warp rejections and convert them to proper error responses
async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "Not found";
    } else if let Some(_) = err.find::<warp::filters::body::BodyDeserializeError>() {
        code = StatusCode::BAD_REQUEST;
        message = "Invalid request body";
    } else if let Some(_) = err.find::<warp::reject::MethodNotAllowed>() {
        code = StatusCode::METHOD_NOT_ALLOWED;
        message = "Method not allowed";
    } else {
        eprintln!("Unhandled rejection: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "Internal server error";
    }

    let json = warp::reply::json(&ErrorResponse {
        error: message.to_string(),
        code: code.as_u16(),
    });

    Ok(warp::reply::with_status(json, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_store_and_retrieve_chunk() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().to_path_buf();

        let test_data = b"Hello, world!";
        let expected_hash = calculate_chunk_hash(test_data);

        // Store chunk
        store_chunk_data(&storage_path, &expected_hash, test_data)
            .await
            .unwrap();

        // Retrieve chunk
        let retrieved_data = load_chunk_data(&storage_path, &expected_hash)
            .await
            .unwrap();

        assert_eq!(test_data.to_vec(), retrieved_data);
    }

    #[tokio::test]
    async fn test_calculate_chunk_hash() {
        let data = b"test data";
        let hash = calculate_chunk_hash(data);

        // Hash should be 64 hex characters (SHA-256)
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_list_chunks() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().to_path_buf();

        // Store some test chunks
        let test_data1 = b"chunk1";
        let test_data2 = b"chunk2";
        let hash1 = calculate_chunk_hash(test_data1);
        let hash2 = calculate_chunk_hash(test_data2);

        store_chunk_data(&storage_path, &hash1, test_data1)
            .await
            .unwrap();
        store_chunk_data(&storage_path, &hash2, test_data2)
            .await
            .unwrap();

        // List chunks
        let chunks = list_stored_chunks(&storage_path).await.unwrap();

        assert_eq!(chunks.len(), 2);
        assert!(chunks.contains(&hash1));
        assert!(chunks.contains(&hash2));
    }
}

/// Binary entry point for the storage node server
#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    use clap::Parser;

    #[derive(Parser)]
    #[clap(name = "storage-node")]
    #[clap(about = "Chiral Network Storage Node")]
    struct Args {
        #[clap(short, long, default_value = "8080")]
        port: u16,

        #[clap(short, long, default_value = "./storage")]
        storage_path: PathBuf,

        #[clap(short, long)]
        verbose: bool,
    }

    let args = Args::parse();

    if args.verbose {
        tracing_subscriber::fmt::init();
    }

    let server = StorageNodeServer::new(args.storage_path, args.port);
    server.run().await
}
