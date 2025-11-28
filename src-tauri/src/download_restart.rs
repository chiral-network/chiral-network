// download_restart.rs
// Whole-File Pause & Resume Baseline Implementation
//
// This module implements the download restart system as specified in docs/download-restart.md
// Owner: Team Hawks (Nick)

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use hex;
use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Download ID type (UUID string)
pub type DownloadId = String;

/// Request to start a new download
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartDownloadRequest {
    /// Optional client-provided UUID; generates one if None
    pub download_id: Option<String>,
    /// HTTP or FTP URL
    pub source_url: String,
    /// Absolute path under the user's downloads directory
    pub destination_path: String,
    /// Optional final hash for verification
    pub expected_sha256: Option<String>,
}

/// Download state machine states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum DownloadState {
    Idle,
    Handshake,
    HandshakeRetry,
    LeaseRenewDue,
    PreparingHead,
    HeadBackoff,
    Restarting,
    PreflightStorage,
    ValidatingMetadata,
    Downloading,
    PersistingProgress,
    Paused,
    AwaitingResume,
    LeaseExpired,
    VerifyingSha,
    FinalizingIo,
    Completed,
    Failed,
}

impl DownloadState {
    /// Returns human-readable description for UI display
    pub fn to_human_readable(&self) -> &'static str {
        match self {
            DownloadState::Idle => "Idle",
            DownloadState::Handshake => "Requesting lease from seeder",
            DownloadState::HandshakeRetry => "Retrying lease request",
            DownloadState::LeaseRenewDue => "Renewing download lease",
            DownloadState::PreparingHead => "Fetching file metadata",
            DownloadState::HeadBackoff => "Retrying metadata fetch",
            DownloadState::Restarting => "Restarting download from beginning",
            DownloadState::PreflightStorage => "Checking disk space",
            DownloadState::ValidatingMetadata => "Validating resume data",
            DownloadState::Downloading => "Downloading",
            DownloadState::PersistingProgress => "Saving progress",
            DownloadState::Paused => "Paused",
            DownloadState::AwaitingResume => "Ready to resume",
            DownloadState::LeaseExpired => "Download lease expired",
            DownloadState::VerifyingSha => "Verifying file integrity",
            DownloadState::FinalizingIo => "Finalizing file",
            DownloadState::Completed => "Completed",
            DownloadState::Failed => "Failed",
        }
    }
}

/// Download status payload for events and queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStatus {
    pub download_id: DownloadId,
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub expected_size: Option<u64>,
    pub etag: Option<String>,
    pub lease_exp: Option<i64>, // Unix timestamp
    pub last_error: Option<String>,
}

/// Download error types
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum DownloadError {
    #[error("download not found")]
    NotFound,
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("source error: {0}")]
    Source(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("insufficient disk space")]
    DiskFull,
    #[error("already completed")]
    AlreadyCompleted,
    #[error("download cancelled")]
    Cancelled,
}

impl DownloadError {
    /// Maps error to standardized app code for telemetry
    pub fn to_error_code(&self) -> &'static str {
        match self {
            DownloadError::NotFound => "DOWNLOAD_NOT_FOUND",
            DownloadError::Invalid(_) => "DOWNLOAD_INVALID_REQUEST",
            DownloadError::Source(_) => "DOWNLOAD_SOURCE_ERROR",
            DownloadError::Io(_) => "IO_ERROR",
            DownloadError::DiskFull => "STORAGE_EXHAUSTED",
            DownloadError::AlreadyCompleted => "DOWNLOAD_ALREADY_COMPLETE",
            DownloadError::Cancelled => "DOWNLOAD_CANCELLED",
        }
    }

    /// Returns human-readable error message for UI display
    pub fn to_human_readable(&self) -> String {
        match self {
            DownloadError::NotFound => "Download not found. It may have been removed.".to_string(),
            DownloadError::Invalid(msg) => format!("Invalid request: {}", msg),
            DownloadError::Source(msg) => format!("Download source error: {}", msg),
            DownloadError::Io(msg) => format!("File system error: {}", msg),
            DownloadError::DiskFull => {
                "Insufficient disk space. Please free up space and try again.".to_string()
            }
            DownloadError::AlreadyCompleted => "This download is already completed.".to_string(),
            DownloadError::Cancelled => "Download cancelled".to_string(),
        }
    }
}

/// Metadata persisted to .meta.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMetadata {
    pub version: u32,
    pub download_id: DownloadId,
    pub url: String,
    pub etag: Option<String>,
    pub expected_size: Option<u64>,
    pub bytes_downloaded: u64,
    pub last_modified: Option<DateTime<Utc>>,
    pub sha256_final: Option<String>,
    pub lease_exp: Option<i64>,
    pub expected_sha256: Option<String>,
}

impl DownloadMetadata {
    /// Current metadata schema version
    pub const CURRENT_VERSION: u32 = 1;

    /// Create new metadata for a fresh download
    pub fn new(download_id: DownloadId, url: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            download_id,
            url,
            etag: None,
            expected_size: None,
            bytes_downloaded: 0,
            last_modified: None,
            sha256_final: None,
            lease_exp: None,
            expected_sha256: None,
        }
    }

    /// Validate metadata version
    pub fn validate_version(&self) -> Result<(), DownloadError> {
        if self.version > Self::CURRENT_VERSION {
            Err(DownloadError::Invalid(format!(
                "unsupported metadata version: {}",
                self.version
            )))
        } else {
            Ok(())
        }
    }
}

const METADATA_FLUSH_INTERVAL_BYTES: u64 = 512 * 1024;
const PROGRESS_EMIT_INTERVAL_MS: u64 = 750;

#[derive(Clone)]
struct RemoteHttpMetadata {
    size: u64,
    etag: Option<String>,
    last_modified: Option<DateTime<Utc>>,
}

/// Download manager internal state
struct DownloadTask {
    status: DownloadStatus,
    metadata: DownloadMetadata,
    destination_path: PathBuf,
    metadata_path: PathBuf,
    cancel_token: CancellationToken,
}

/// Download restart service singleton
pub struct DownloadRestartService {
    downloads: Arc<Mutex<HashMap<DownloadId, DownloadTask>>>,
    app_handle: Option<AppHandle>,
}

impl DownloadRestartService {
    /// Create new download restart service
    pub fn new(app_handle: Option<AppHandle>) -> Self {
        Self {
            downloads: Arc::new(Mutex::new(HashMap::new())),
            app_handle,
        }
    }

    fn metadata_path_for(destination_path: &Path) -> PathBuf {
        let file_name = destination_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download");
        destination_path.with_file_name(format!(".{}.chiral.meta.json", file_name))
    }

    async fn persist_metadata(
        metadata_path: &Path,
        metadata: &DownloadMetadata,
    ) -> Result<(), DownloadError> {
        if let Some(parent) = metadata_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| DownloadError::Io(format!("Failed to create metadata dir: {}", e)))?;
        }

        let data = serde_json::to_vec_pretty(metadata)
            .map_err(|e| DownloadError::Io(format!("Failed to serialize metadata: {}", e)))?;
        fs::write(metadata_path, data)
            .await
            .map_err(|e| DownloadError::Io(format!("Failed to persist metadata: {}", e)))
    }

    async fn remove_metadata_file(metadata_path: &Path) {
        if let Err(e) = fs::remove_file(metadata_path).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    "Failed to remove metadata file {}: {}",
                    metadata_path.display(),
                    e
                );
            }
        }
    }

    async fn set_state(
        &self,
        download_id: &str,
        state: DownloadState,
        bytes_downloaded: u64,
        expected_size: Option<u64>,
        etag: Option<String>,
        last_error: Option<String>,
        persist_metadata: bool,
    ) -> Result<(), DownloadError> {
        let (status, metadata, metadata_path) = {
            let mut downloads = self.downloads.lock().await;
            let task = downloads
                .get_mut(download_id)
                .ok_or(DownloadError::NotFound)?;
            task.status.state = state;
            task.status.bytes_downloaded = bytes_downloaded;
            task.status.last_error = last_error.clone();
            if let Some(expected) = expected_size {
                task.status.expected_size = Some(expected);
                task.metadata.expected_size = Some(expected);
            }
            if let Some(ref tag) = etag {
                task.status.etag = Some(tag.clone());
                task.metadata.etag = Some(tag.clone());
            }
            task.metadata.bytes_downloaded = bytes_downloaded;
            (
                task.status.clone(),
                if persist_metadata {
                    Some(task.metadata.clone())
                } else {
                    None
                },
                task.metadata_path.clone(),
            )
        };

        if let Some(metadata) = metadata {
            Self::persist_metadata(&metadata_path, &metadata).await?;
        }

        self.emit_status(&status).await?;
        Ok(())
    }

    async fn update_metadata_only<F>(
        &self,
        download_id: &str,
        mutator: F,
    ) -> Result<(), DownloadError>
    where
        F: FnOnce(&mut DownloadMetadata),
    {
        let (metadata, metadata_path) = {
            let mut downloads = self.downloads.lock().await;
            let task = downloads
                .get_mut(download_id)
                .ok_or(DownloadError::NotFound)?;
            mutator(&mut task.metadata);
            (task.metadata.clone(), task.metadata_path.clone())
        };

        Self::persist_metadata(&metadata_path, &metadata).await
    }

    async fn persist_current_metadata(&self, download_id: &str) -> Result<(), DownloadError> {
        let (metadata, metadata_path) = {
            let downloads = self.downloads.lock().await;
            let task = downloads.get(download_id).ok_or(DownloadError::NotFound)?;
            (task.metadata.clone(), task.metadata_path.clone())
        };
        Self::persist_metadata(&metadata_path, &metadata).await
    }

    async fn status_snapshot(&self, download_id: &str) -> Result<DownloadStatus, DownloadError> {
        let downloads = self.downloads.lock().await;
        downloads
            .get(download_id)
            .map(|task| task.status.clone())
            .ok_or(DownloadError::NotFound)
    }

    async fn fetch_remote_http_metadata(
        &self,
        client: &Client,
        url: &str,
    ) -> Result<RemoteHttpMetadata, DownloadError> {
        let response = client
            .head(url)
            .send()
            .await
            .map_err(|e| DownloadError::Source(format!("HEAD request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(DownloadError::Source(format!(
                "Metadata request failed with status {}",
                response.status()
            )));
        }

        let size = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .ok_or_else(|| DownloadError::Source("Missing Content-Length header".to_string()))?
            .to_str()
            .map_err(|e| DownloadError::Source(format!("Invalid Content-Length header: {}", e)))?
            .parse::<u64>()
            .map_err(|e| DownloadError::Source(format!("Invalid Content-Length value: {}", e)))?;

        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_matches('"').to_string());

        let last_modified = response
            .headers()
            .get(header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .and_then(|value| DateTime::parse_from_rfc2822(value).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(RemoteHttpMetadata {
            size,
            etag,
            last_modified,
        })
    }

    async fn prepare_destination_file(
        &self,
        download_id: &str,
        destination_path: &Path,
        remote_size: u64,
        remote_etag: Option<String>,
    ) -> Result<u64, DownloadError> {
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| DownloadError::Io(format!("Failed to create parent dir: {}", e)))?;
        }

        let mut resume_offset = match fs::metadata(destination_path).await {
            Ok(meta) => meta.len().min(remote_size),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => {
                return Err(DownloadError::Io(format!(
                    "Failed to inspect destination file: {}",
                    e
                )))
            }
        };

        let should_reset = {
            let downloads = self.downloads.lock().await;
            downloads
                .get(download_id)
                .and_then(|task| task.metadata.etag.clone())
                .zip(remote_etag.clone())
                .map(|(existing, remote)| existing != remote)
                .unwrap_or(false)
        };

        if should_reset && resume_offset > 0 {
            warn!(
                "Remote metadata changed for {}, deleting partial file",
                download_id
            );
            if let Err(e) = fs::remove_file(destination_path).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(DownloadError::Io(format!(
                        "Failed to delete old partial file: {}",
                        e
                    )));
                }
            }
            resume_offset = 0;
        }

        Ok(resume_offset)
    }

    async fn expected_sha(&self, download_id: &str) -> Result<Option<String>, DownloadError> {
        let downloads = self.downloads.lock().await;
        Ok(downloads
            .get(download_id)
            .and_then(|task| task.metadata.expected_sha256.clone()))
    }

    async fn verify_file_hash(&self, path: &Path, expected: &str) -> Result<String, DownloadError> {
        let mut file = File::open(path)
            .await
            .map_err(|e| DownloadError::Io(format!("Failed to open file for hashing: {}", e)))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];

        loop {
            let bytes_read = file.read(&mut buffer).await.map_err(|e| {
                DownloadError::Io(format!("Failed to read file for hashing: {}", e))
            })?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let actual = hex::encode(hasher.finalize());
        let normalized_expected = expected.trim().to_ascii_lowercase();

        if normalized_expected != actual {
            return Err(DownloadError::Invalid(format!(
                "SHA-256 mismatch: expected {}, got {}",
                normalized_expected, actual
            )));
        }

        Ok(actual)
    }

    async fn spawn_download_worker(&self, download_id: String) -> Result<(), DownloadError> {
        let (source_url, destination_path, metadata_path, cancel_token) = {
            let downloads = self.downloads.lock().await;
            let task = downloads.get(&download_id).ok_or(DownloadError::NotFound)?;
            (
                task.metadata.url.clone(),
                task.destination_path.clone(),
                task.metadata_path.clone(),
                task.cancel_token.clone(),
            )
        };

        let service = self.clone_service();
        tokio::spawn(async move {
            if let Err(err) = service
                .run_download_state_machine(
                    download_id.clone(),
                    source_url,
                    destination_path,
                    metadata_path,
                    cancel_token,
                )
                .await
            {
                if matches!(err, DownloadError::Cancelled) {
                    info!("Download {} paused/cancelled", download_id);
                } else {
                    service
                        .update_state_with_error(
                            &download_id,
                            DownloadState::Failed,
                            err.to_string(),
                        )
                        .await;
                }
            }
        });

        Ok(())
    }

    /// Emit download_status event to frontend
    async fn emit_status(&self, status: &DownloadStatus) -> Result<(), DownloadError> {
        if let Some(handle) = &self.app_handle {
            handle
                .emit("download_status", status)
                .map_err(|e| DownloadError::Io(format!("Failed to emit event: {}", e)))?;
        }
        Ok(())
    }

    /// Clone service for spawning tasks
    fn clone_service(&self) -> Self {
        Self {
            downloads: self.downloads.clone(),
            app_handle: self.app_handle.clone(),
        }
    }

    /// Run the actual download state machine
    async fn run_download_state_machine(
        &self,
        download_id: String,
        source_url: String,
        dest_path: PathBuf,
        metadata_path: PathBuf,
        cancel_token: CancellationToken,
    ) -> Result<(), DownloadError> {
        if !source_url.starts_with("http") {
            return Err(DownloadError::Invalid(
                "Restartable downloads currently support HTTP sources only".to_string(),
            ));
        }

        let destination_path = dest_path;
        let client = Client::new();

        self.set_state(
            &download_id,
            DownloadState::PreparingHead,
            0,
            None,
            None,
            None,
            false,
        )
        .await?;

        let remote_meta = self
            .fetch_remote_http_metadata(&client, &source_url)
            .await?;

        self.update_metadata_only(&download_id, |meta| {
            meta.expected_size = Some(remote_meta.size);
            meta.etag = remote_meta.etag.clone();
            meta.last_modified = remote_meta.last_modified;
        })
        .await?;

        self.set_state(
            &download_id,
            DownloadState::ValidatingMetadata,
            0,
            Some(remote_meta.size),
            remote_meta.etag.clone(),
            None,
            true,
        )
        .await?;

        let resume_offset = self
            .prepare_destination_file(
                &download_id,
                &destination_path,
                remote_meta.size,
                remote_meta.etag.clone(),
            )
            .await?;

        self.set_state(
            &download_id,
            DownloadState::PreflightStorage,
            resume_offset,
            Some(remote_meta.size),
            remote_meta.etag.clone(),
            None,
            true,
        )
        .await?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&destination_path)
            .await
            .map_err(|e| DownloadError::Io(format!("Failed to open destination: {}", e)))?;
        file.seek(SeekFrom::Start(resume_offset))
            .await
            .map_err(|e| DownloadError::Io(format!("Failed to seek file: {}", e)))?;
        if resume_offset > 0 {
            file.set_len(resume_offset)
                .await
                .map_err(|e| DownloadError::Io(format!("Failed to truncate file: {}", e)))?;
        }

        self.set_state(
            &download_id,
            DownloadState::Downloading,
            resume_offset,
            Some(remote_meta.size),
            remote_meta.etag.clone(),
            None,
            true,
        )
        .await?;

        let mut request = client.get(&source_url);
        if resume_offset > 0 {
            request = request.header(header::RANGE, format!("bytes={}-", resume_offset));
        }

        let mut response = request
            .send()
            .await
            .map_err(|e| DownloadError::Source(format!("HTTP request failed: {}", e)))?;

        if resume_offset > 0 && response.status() == StatusCode::OK {
            warn!(
                "Server at {} ignored Range header, restarting download from scratch",
                source_url
            );
            file.set_len(0)
                .await
                .map_err(|e| DownloadError::Io(format!("Failed to reset file: {}", e)))?;
            self.set_state(
                &download_id,
                DownloadState::Restarting,
                0,
                Some(remote_meta.size),
                remote_meta.etag.clone(),
                None,
                true,
            )
            .await?;
            response = client
                .get(&source_url)
                .send()
                .await
                .map_err(|e| DownloadError::Source(format!("HTTP request failed: {}", e)))?;
        }

        if resume_offset == 0 && response.status() != StatusCode::OK {
            return Err(DownloadError::Source(format!(
                "Unexpected status {} when starting download",
                response.status()
            )));
        }

        if resume_offset > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(DownloadError::Source(format!(
                "Server does not support ranged downloads (status: {})",
                response.status()
            )));
        }

        let mut stream = response.bytes_stream();
        let mut downloaded = resume_offset;
        let mut last_emit = Instant::now();
        let mut last_persist = resume_offset;

        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    self.persist_current_metadata(&download_id).await?;
                    return Err(DownloadError::Cancelled);
                }
                next = stream.next() => next
            };

            let Some(chunk) = chunk else {
                break;
            };

            let chunk = chunk.map_err(|e| DownloadError::Source(format!("Stream error: {}", e)))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| DownloadError::Io(format!("Failed to write chunk: {}", e)))?;

            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| DownloadError::Io("download size overflowed".to_string()))?;

            {
                let mut downloads = self.downloads.lock().await;
                if let Some(task) = downloads.get_mut(&download_id) {
                    task.metadata.bytes_downloaded = downloaded;
                    task.status.bytes_downloaded = downloaded;
                    task.status.state = DownloadState::Downloading;
                    task.status.last_error = None;
                }
            }

            if downloaded.saturating_sub(last_persist) >= METADATA_FLUSH_INTERVAL_BYTES {
                self.persist_current_metadata(&download_id).await?;
                last_persist = downloaded;
            }

            if last_emit.elapsed().as_millis() as u64 >= PROGRESS_EMIT_INTERVAL_MS {
                let status = self.status_snapshot(&download_id).await?;
                self.emit_status(&status).await?;
                last_emit = Instant::now();
            }
        }

        if downloaded < remote_meta.size {
            return Err(DownloadError::Source(format!(
                "Download ended early: expected {} bytes, got {}",
                remote_meta.size, downloaded
            )));
        }

        file.flush()
            .await
            .map_err(|e| DownloadError::Io(format!("Failed to flush file: {}", e)))?;

        self.set_state(
            &download_id,
            DownloadState::VerifyingSha,
            downloaded,
            Some(remote_meta.size),
            remote_meta.etag.clone(),
            None,
            true,
        )
        .await?;

        if let Some(expected) = self.expected_sha(&download_id).await? {
            let actual = self.verify_file_hash(&destination_path, &expected).await?;
            self.update_metadata_only(&download_id, |meta| {
                meta.sha256_final = Some(actual.clone());
            })
            .await?;
        }

        self.set_state(
            &download_id,
            DownloadState::FinalizingIo,
            downloaded,
            Some(remote_meta.size),
            remote_meta.etag.clone(),
            None,
            true,
        )
        .await?;

        Self::remove_metadata_file(&metadata_path).await;

        self.set_state(
            &download_id,
            DownloadState::Completed,
            downloaded,
            Some(remote_meta.size),
            remote_meta.etag.clone(),
            None,
            false,
        )
        .await?;

        Ok(())
    }

    /// Update download state with progress
    /// Update download state with error
    async fn update_state_with_error(
        &self,
        download_id: &str,
        state: DownloadState,
        error: String,
    ) {
        let mut downloads = self.downloads.lock().await;
        if let Some(task) = downloads.get_mut(download_id) {
            task.status.state = state;
            task.status.last_error = Some(error);
            let status = task.status.clone();
            drop(downloads);
            let _ = self.emit_status(&status).await;
        }
    }

    /// Mock download simulation for UI testing
    /// This will be replaced with real download logic by Josh/Matt/Elliot
    async fn simulate_download(&self, download_id: String) {
        use tokio::time::{sleep, Duration};

        // Simulate file size: 10 MB
        let total_size = 10 * 1024 * 1024u64;

        // Transition to Handshake
        sleep(Duration::from_millis(500)).await;
        self.update_state(&download_id, DownloadState::Handshake, None, None)
            .await;

        // Transition to PreparingHead
        sleep(Duration::from_millis(800)).await;
        self.update_state(
            &download_id,
            DownloadState::PreparingHead,
            Some(total_size),
            Some("\"test-etag-123\"".to_string()),
        )
        .await;

        // Transition to Downloading
        sleep(Duration::from_millis(500)).await;
        self.update_state(
            &download_id,
            DownloadState::Downloading,
            Some(total_size),
            Some("\"test-etag-123\"".to_string()),
        )
        .await;

        // Simulate download progress (10 updates)
        for i in 1..=10 {
            sleep(Duration::from_millis(800)).await;

            // Check if paused
            let mut downloads = self.downloads.lock().await;
            if let Some(task) = downloads.get_mut(&download_id) {
                if task.status.state == DownloadState::Paused {
                    drop(downloads);
                    return; // Stop simulation if paused
                }

                let bytes = (total_size * i) / 10;
                task.status.bytes_downloaded = bytes;
                task.status.state = if i % 2 == 0 {
                    DownloadState::PersistingProgress
                } else {
                    DownloadState::Downloading
                };

                let status = task.status.clone();
                drop(downloads);
                let _ = self.emit_status(&status).await;
            } else {
                return; // Download was deleted
            }
        }

        // Transition to VerifyingSha
        sleep(Duration::from_millis(1000)).await;
        self.update_state(
            &download_id,
            DownloadState::VerifyingSha,
            Some(total_size),
            Some("\"test-etag-123\"".to_string()),
        )
        .await;

        // Transition to Completed
        sleep(Duration::from_millis(1000)).await;
        let mut downloads = self.downloads.lock().await;
        if let Some(task) = downloads.get_mut(&download_id) {
            task.status.state = DownloadState::Completed;
            task.status.bytes_downloaded = total_size;
            task.metadata.sha256_final = Some("abc123def456...".to_string());

            let status = task.status.clone();
            drop(downloads);
            let _ = self.emit_status(&status).await;
        }
    }

    /// Update download state (helper for simulation)
    async fn update_state(
        &self,
        download_id: &str,
        state: DownloadState,
        expected_size: Option<u64>,
        etag: Option<String>,
    ) {
        let mut downloads = self.downloads.lock().await;
        if let Some(task) = downloads.get_mut(download_id) {
            task.status.state = state;
            if let Some(size) = expected_size {
                task.status.expected_size = Some(size);
                task.metadata.expected_size = Some(size);
            }
            if let Some(tag) = etag {
                task.status.etag = Some(tag.clone());
                task.metadata.etag = Some(tag);
            }

            let status = task.status.clone();
            drop(downloads);
            let _ = self.emit_status(&status).await;
        }
    }

    /// Get download status
    pub async fn get_status(&self, download_id: &str) -> Result<DownloadStatus, DownloadError> {
        let downloads = self.downloads.lock().await;
        downloads
            .get(download_id)
            .map(|task| task.status.clone())
            .ok_or(DownloadError::NotFound)
    }

    /// Start a new download
    pub async fn start_download(
        &self,
        request: StartDownloadRequest,
    ) -> Result<DownloadId, DownloadError> {
        // Generate or use provided download ID
        let download_id = request
            .download_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Validate destination path (security check)
        let dest_path = PathBuf::from(&request.destination_path);
        if !dest_path.is_absolute() {
            return Err(DownloadError::Invalid(
                "destination_path must be absolute".to_string(),
            ));
        }

        // Check if download already exists
        let mut downloads = self.downloads.lock().await;
        if downloads.contains_key(&download_id) {
            return Err(DownloadError::Invalid(
                "download_id already exists".to_string(),
            ));
        }

        // Create initial metadata
        let mut metadata = DownloadMetadata::new(download_id.clone(), request.source_url.clone());
        metadata.expected_sha256 = request.expected_sha256.clone();

        // Create initial status
        let status = DownloadStatus {
            download_id: download_id.clone(),
            state: DownloadState::Idle,
            bytes_downloaded: 0,
            expected_size: None,
            etag: None,
            lease_exp: None,
            last_error: None,
        };

        let metadata_path = Self::metadata_path_for(&dest_path);
        let cancel_token = CancellationToken::new();

        // Store download task
        downloads.insert(
            download_id.clone(),
            DownloadTask {
                status: status.clone(),
                metadata: metadata.clone(),
                destination_path: dest_path.clone(),
                metadata_path: metadata_path.clone(),
                cancel_token,
            },
        );

        drop(downloads); // Release lock before async work

        Self::persist_metadata(&metadata_path, &metadata).await?;
        self.emit_status(&status).await?;

        // Start actual download state machine in background task
        self.spawn_download_worker(download_id.clone()).await?;

        Ok(download_id)
    }

    /// Pause a download
    pub async fn pause_download(&self, download_id: &str) -> Result<(), DownloadError> {
        let task_exists = {
            let downloads = self.downloads.lock().await;
            downloads.contains_key(download_id)
        };

        if !task_exists {
            return Err(DownloadError::NotFound);
        }

        let mut downloads = self.downloads.lock().await;
        let task = downloads.get_mut(download_id).unwrap(); // Safe because we checked existence above

        // Only pause if currently downloading
        if task.status.state != DownloadState::Downloading
            && task.status.state != DownloadState::PersistingProgress
        {
            return Err(DownloadError::Invalid(
                "cannot pause download in current state".to_string(),
            ));
        }

        // Update state to Paused and get cancellation token before dropping lock
        task.status.state = DownloadState::Paused;
        let status = task.status.clone();
        let cancel_token = task.cancel_token.clone();
        task.cancel_token = CancellationToken::new();

        drop(downloads); // Release lock before emitting
        self.emit_status(&status).await?;

        cancel_token.cancel();
        self.persist_current_metadata(download_id).await?;
        info!("Paused download {}", download_id);

        Ok(())
    }

    /// Resume a paused download
    pub async fn resume_download(&self, download_id: &str) -> Result<(), DownloadError> {
        let task_exists = {
            let downloads = self.downloads.lock().await;
            downloads.contains_key(download_id)
        };

        if !task_exists {
            return Err(DownloadError::NotFound);
        }

        let mut downloads = self.downloads.lock().await;
        let task = downloads.get_mut(download_id).unwrap(); // Safe because we checked existence above

        // Only resume if paused or awaiting resume
        if task.status.state != DownloadState::Paused
            && task.status.state != DownloadState::AwaitingResume
        {
            return Err(DownloadError::Invalid(
                "cannot resume download in current state".to_string(),
            ));
        }

        // Update state to indicate resume is starting
        task.status.state = DownloadState::Downloading;
        task.status.last_error = None;
        task.cancel_token = CancellationToken::new();
        let status = task.status.clone();

        drop(downloads); // Release lock before emitting
        self.emit_status(&status).await?;

        self.spawn_download_worker(download_id.to_string()).await?;

        Ok(())
    }
}

// Note: Tauri commands are defined in main.rs to access AppState

/// Metrics for telemetry (Nick's task: metrics/logs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMetrics {
    pub retry_count: u32,
    pub restart_count: u32,
    pub lease_renewal_count: u32,
    pub last_failure_reason: Option<String>,
}

impl Default for DownloadMetrics {
    fn default() -> Self {
        Self {
            retry_count: 0,
            restart_count: 0,
            lease_renewal_count: 0,
            last_failure_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_state_human_readable() {
        assert_eq!(DownloadState::Idle.to_human_readable(), "Idle");
        assert_eq!(
            DownloadState::Downloading.to_human_readable(),
            "Downloading"
        );
        assert_eq!(
            DownloadState::VerifyingSha.to_human_readable(),
            "Verifying file integrity"
        );
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(
            DownloadError::NotFound.to_error_code(),
            "DOWNLOAD_NOT_FOUND"
        );
        assert_eq!(DownloadError::DiskFull.to_error_code(), "STORAGE_EXHAUSTED");
        assert_eq!(
            DownloadError::Invalid("test".to_string()).to_error_code(),
            "DOWNLOAD_INVALID_REQUEST"
        );
    }

    #[test]
    fn test_metadata_version_validation() {
        let mut metadata =
            DownloadMetadata::new("test-id".to_string(), "http://example.com".to_string());
        assert!(metadata.validate_version().is_ok());

        metadata.version = 999;
        assert!(metadata.validate_version().is_err());
    }
}
