//! HTTP Protocol Handler
//!
//! Wraps the existing HTTP download functionality to implement the enhanced ProtocolHandler trait.

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus,
    ProtocolCapabilities, ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
};
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{info, warn, error};

/// HTTP protocol handler implementing the enhanced ProtocolHandler trait
pub struct HttpProtocolHandler {
    /// HTTP client for making requests
    client: Client,
    /// Track active downloads
    active_downloads: Arc<Mutex<HashMap<String, HttpDownloadState>>>,
    /// Track download progress
    download_progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
}

/// Internal state for an HTTP download
struct HttpDownloadState {
    url: String,
    output_path: PathBuf,
    started_at: u64,
    status: DownloadStatus,
    cancel_token: tokio::sync::watch::Sender<bool>,
}

impl HttpProtocolHandler {
    /// Creates a new HTTP protocol handler
    pub fn new() -> Result<Self, ProtocolError> {
        let client = Client::builder()
            .user_agent("Chiral-Network/1.0")
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self {
            client,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Creates a handler with custom timeout
    pub fn with_timeout(timeout_secs: u64) -> Result<Self, ProtocolError> {
        let client = Client::builder()
            .user_agent("Chiral-Network/1.0")
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        Ok(Self {
            client,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Get current timestamp
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Generate a unique ID for tracking downloads
    fn generate_id(url: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        format!("http-{:x}", hasher.finish())
    }

    /// Download file with progress tracking
    async fn download_with_progress(
        client: Client,
        url: String,
        output_path: PathBuf,
        progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
        download_id: String,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), ProtocolError> {
        // Initial HEAD request to get content length
        let head_response = client
            .head(&url)
            .send()
            .await
            .map_err(|e| ProtocolError::NetworkError(e.to_string()))?;

        let total_bytes = head_response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        // Update progress with total size
        {
            let mut prog = progress.lock().await;
            if let Some(p) = prog.get_mut(&download_id) {
                p.total_bytes = total_bytes;
                p.status = DownloadStatus::Downloading;
            }
        }

        // Start download
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProtocolError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ProtocolError::NetworkError(
                format!("HTTP {} for {}", response.status(), url)
            ));
        }

        // Create output file
        let mut file = File::create(&output_path)
            .await
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        let mut downloaded_bytes: u64 = 0;
        let mut stream = response.bytes_stream();
        let start_time = std::time::Instant::now();

        use futures::StreamExt;

        loop {
            tokio::select! {
                // Check for cancellation
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        // Cancelled
                        let mut prog = progress.lock().await;
                        if let Some(p) = prog.get_mut(&download_id) {
                            p.status = DownloadStatus::Cancelled;
                        }
                        return Err(ProtocolError::Internal("Download cancelled".to_string()));
                    }
                }
                // Process next chunk
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            file.write_all(&bytes)
                                .await
                                .map_err(|e| ProtocolError::Internal(e.to_string()))?;

                            downloaded_bytes += bytes.len() as u64;

                            // Update progress
                            let elapsed = start_time.elapsed().as_secs_f64();
                            let speed = if elapsed > 0.0 {
                                downloaded_bytes as f64 / elapsed
                            } else {
                                0.0
                            };

                            let eta = if speed > 0.0 && total_bytes > downloaded_bytes {
                                Some(((total_bytes - downloaded_bytes) as f64 / speed) as u64)
                            } else {
                                None
                            };

                            let mut prog = progress.lock().await;
                            if let Some(p) = prog.get_mut(&download_id) {
                                p.downloaded_bytes = downloaded_bytes;
                                p.download_speed = speed;
                                p.eta_seconds = eta;
                            }
                        }
                        Some(Err(e)) => {
                            let mut prog = progress.lock().await;
                            if let Some(p) = prog.get_mut(&download_id) {
                                p.status = DownloadStatus::Failed;
                            }
                            return Err(ProtocolError::NetworkError(e.to_string()));
                        }
                        None => {
                            // Download complete
                            break;
                        }
                    }
                }
            }
        }

        file.flush()
            .await
            .map_err(|e| ProtocolError::Internal(e.to_string()))?;

        // Mark as completed
        let mut prog = progress.lock().await;
        if let Some(p) = prog.get_mut(&download_id) {
            p.status = DownloadStatus::Completed;
            p.downloaded_bytes = downloaded_bytes;
        }

        info!("HTTP: Download completed: {} bytes", downloaded_bytes);
        Ok(())
    }
}

impl Default for HttpProtocolHandler {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP handler")
    }
}

#[async_trait]
impl ProtocolHandler for HttpProtocolHandler {
    fn name(&self) -> &'static str {
        "http"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("http://") || identifier.starts_with("https://")
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("HTTP: Starting download for {}", identifier);

        let download_id = Self::generate_id(identifier);

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(&download_id) {
                return Err(ProtocolError::AlreadyExists(download_id));
            }
        }

        let started_at = Self::now();

        // Create cancellation channel
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        // Initialize progress
        {
            let mut prog = self.download_progress.lock().await;
            prog.insert(download_id.clone(), DownloadProgress {
                downloaded_bytes: 0,
                total_bytes: 0,
                download_speed: 0.0,
                eta_seconds: None,
                active_peers: 1, // HTTP has "1 peer" (the server)
                status: DownloadStatus::FetchingMetadata,
            });
        }

        // Track the download
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(download_id.clone(), HttpDownloadState {
                url: identifier.to_string(),
                output_path: options.output_path.clone(),
                started_at,
                status: DownloadStatus::Downloading,
                cancel_token: cancel_tx,
            });
        }

        // Spawn download task
        let client = self.client.clone();
        let url = identifier.to_string();
        let output_path = options.output_path;
        let progress = self.download_progress.clone();
        let id = download_id.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::download_with_progress(
                client,
                url,
                output_path,
                progress,
                id,
                cancel_rx,
            ).await {
                error!("HTTP download failed: {}", e);
            }
        });

        Ok(DownloadHandle {
            identifier: download_id,
            protocol: "http".to_string(),
            started_at,
        })
    }

    async fn seed(
        &self,
        _file_path: PathBuf,
        _options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        // HTTP doesn't support traditional seeding
        // Would need to run an HTTP server
        warn!("HTTP: Seeding not supported");
        Err(ProtocolError::NotSupported)
    }

    async fn stop_seeding(&self, _identifier: &str) -> Result<(), ProtocolError> {
        Err(ProtocolError::NotSupported)
    }

    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        // HTTP doesn't easily support pause without range requests
        // For now, we cancel and would need to resume with range request
        warn!("HTTP: pause_download - cancelling download (resume requires range request support)");
        self.cancel_download(identifier).await
    }

    async fn resume_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
        // Would need to track partial file and use Range header
        warn!("HTTP: resume_download not yet implemented");
        Err(ProtocolError::NotSupported)
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("HTTP: Cancelling download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.remove(identifier) {
            // Signal cancellation
            let _ = state.cancel_token.send(true);
            Ok(())
        } else {
            Err(ProtocolError::DownloadNotFound(identifier.to_string()))
        }
    }

    async fn get_download_progress(
        &self,
        identifier: &str,
    ) -> Result<DownloadProgress, ProtocolError> {
        let progress = self.download_progress.lock().await;
        progress
            .get(identifier)
            .cloned()
            .ok_or_else(|| ProtocolError::DownloadNotFound(identifier.to_string()))
    }

    async fn list_seeding(&self) -> Result<Vec<SeedingInfo>, ProtocolError> {
        // HTTP doesn't support seeding
        Ok(Vec::new())
    }

    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities {
            supports_seeding: false,
            supports_pause_resume: false, // Could be true with range request implementation
            supports_multi_source: true,  // Can download same file from multiple URLs
            supports_encryption: true,    // HTTPS
            supports_dht: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_http() {
        let handler = HttpProtocolHandler::new().unwrap();
        assert!(handler.supports("http://example.com/file.zip"));
        assert!(handler.supports("https://example.com/file.zip"));
        assert!(!handler.supports("ftp://example.com/file.zip"));
        assert!(!handler.supports("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn test_generate_id() {
        let id1 = HttpProtocolHandler::generate_id("http://example.com/file.zip");
        let id2 = HttpProtocolHandler::generate_id("http://example.com/file.zip");
        let id3 = HttpProtocolHandler::generate_id("http://other.com/file.zip");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert!(id1.starts_with("http-"));
    }
}
