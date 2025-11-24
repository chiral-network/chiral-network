// download_restart.rs
// Whole-File Pause & Resume Baseline Implementation
//
// This module implements the download restart system as specified in docs/download-restart.md
// Owner: Team Hawks (Nick)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tracing::info;
use crate::http_download::{HttpDownloadClient, HttpDownloadProgress, DownloadStatus as HttpDownloadStatus};

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
        }
    }

    /// Returns human-readable error message for UI display
    pub fn to_human_readable(&self) -> String {
        match self {
            DownloadError::NotFound => "Download not found. It may have been removed.".to_string(),
            DownloadError::Invalid(msg) => format!("Invalid request: {}", msg),
            DownloadError::Source(msg) => format!("Download source error: {}", msg),
            DownloadError::Io(msg) => format!("File system error: {}", msg),
            DownloadError::DiskFull => "Insufficient disk space. Please free up space and try again.".to_string(),
            DownloadError::AlreadyCompleted => "This download is already completed.".to_string(),
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

/// Download manager internal state
struct DownloadTask {
    status: DownloadStatus,
    metadata: DownloadMetadata,
    destination_path: PathBuf,
    /// Handle to the currently running download task for cancellation
    active_task: Option<tokio::task::JoinHandle<()>>,
}

/// Download restart service singleton
pub struct DownloadRestartService {
    downloads: Arc<Mutex<HashMap<DownloadId, DownloadTask>>>,
    app_handle: AppHandle,
}

impl DownloadRestartService {
    /// Create new download restart service
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            downloads: Arc::new(Mutex::new(HashMap::new())),
            app_handle,
        }
    }

    /// Emit download_status event to frontend
    async fn emit_status(&self, status: &DownloadStatus) -> Result<(), DownloadError> {
        self.app_handle
            .emit("download_status", status)
            .map_err(|e| DownloadError::Io(format!("Failed to emit event: {}", e)))?;
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
        dest_path: String,
        expected_sha256: Option<String>,
    ) -> Result<(), DownloadError> {
        use tokio::time::{sleep, Duration};

        // Initialize HTTP download client
        let http_client = HttpDownloadClient::new();
        let (progress_tx, mut progress_rx) = mpsc::channel::<HttpDownloadProgress>(32);

        // State: Handshake - Request lease from seeder
        self.update_state(&download_id, DownloadState::Handshake, None, None).await;

        // Simulate handshake delay
        sleep(Duration::from_millis(500)).await;

        // State: PreparingHead - Fetch file metadata
        self.update_state(&download_id, DownloadState::PreparingHead, None, Some("\"requesting-metadata\"".to_string())).await;

        // Simulate metadata fetch delay
        sleep(Duration::from_millis(800)).await;

        // For now, simulate metadata - in real implementation, this would come from HEAD request
        let total_size = 10 * 1024 * 1024u64; // 10 MB
        let etag = "\"simulated-etag-123\"".to_string();

        self.update_state(&download_id, DownloadState::PreparingHead, Some(total_size), Some(etag.clone())).await;

        // State: PreflightStorage - Check disk space
        self.update_state(&download_id, DownloadState::PreflightStorage, Some(total_size), Some(etag.clone())).await;

        // Simulate storage check
        sleep(Duration::from_millis(200)).await;

        // State: ValidatingMetadata - Validate resume data if any
        self.update_state(&download_id, DownloadState::ValidatingMetadata, Some(total_size), Some(etag.clone())).await;

        // Simulate validation
        sleep(Duration::from_millis(300)).await;

        // State: Downloading - Start actual download
        self.update_state(&download_id, DownloadState::Downloading, Some(total_size), Some(etag.clone())).await;

        // Start the actual HTTP download in a separate task
        let (completion_tx, mut completion_rx) = mpsc::channel::<Result<(), String>>(1);
        let progress_tx_clone = progress_tx.clone();

        let task_handle = tokio::spawn(async move {
            // Use real HTTP download with progress reporting
            let result = http_client.download_file(
                &source_url,
                &expected_sha256.unwrap_or_else(|| "unknown-hash".to_string()),
                std::path::Path::new(&dest_path),
                Some(progress_tx),
            ).await;

            // Send completion result
            let _ = completion_tx.send(result).await;
        });

        // Handle completion in a separate task
        tokio::spawn(async move {
            if let Some(result) = completion_rx.recv().await {
                match result {
                    Ok(_) => {
                        // Send completion progress
                        let _ = progress_tx_clone.send(HttpDownloadProgress {
                            file_hash: "placeholder-hash".to_string(),
                            chunks_total: (total_size / 256 * 1024) as usize,
                            chunks_downloaded: (total_size / 256 * 1024) as usize,
                            bytes_downloaded: total_size,
                            bytes_total: total_size,
                            status: HttpDownloadStatus::Completed,
                        }).await;
                    }
                    Err(e) => {
                        // Send failure progress
                        let _ = progress_tx_clone.send(HttpDownloadProgress {
                            file_hash: "placeholder-hash".to_string(),
                            chunks_total: 0,
                            chunks_downloaded: 0,
                            bytes_downloaded: 0,
                            bytes_total: total_size,
                            status: HttpDownloadStatus::Failed,
                        }).await;
                        tracing::error!("HTTP download failed: {}", e);
                    }
                }
            }
        });

        // Store the task handle for potential cancellation
        {
            let mut downloads = self.downloads.lock().await;
            if let Some(task) = downloads.get_mut(&download_id) {
                task.active_task = Some(task_handle);
            }
        }

        // Monitor progress and handle state updates
        let bytes_downloaded = 0u64;
        let mut download_active = true;

        while download_active {
            tokio::select! {
                // Handle progress updates from download task
                Some(progress) = progress_rx.recv() => {
                    match progress.status {
                        HttpDownloadStatus::Completed => {
                            download_active = false;
                            self.update_state(&download_id, DownloadState::VerifyingSha, Some(total_size), Some(etag.clone())).await;

                            // Simulate SHA verification
                            sleep(Duration::from_millis(500)).await;

                            self.update_state(&download_id, DownloadState::FinalizingIo, Some(total_size), Some(etag.clone())).await;

                            // Simulate finalization
                            sleep(Duration::from_millis(300)).await;

                            self.update_state(&download_id, DownloadState::Completed, Some(total_size), Some(etag.clone())).await;
                        }
                        HttpDownloadStatus::Failed => {
                            download_active = false;
                            self.update_state_with_error(&download_id, DownloadState::Failed, "Download failed".to_string()).await;
                        }
                        _ => {
                            // Continue downloading
                            self.update_state(&download_id, DownloadState::Downloading, Some(total_size), Some(etag.clone())).await;
                        }
                    }
                }

                // Check for pause/cancel commands every 100ms
                _ = sleep(Duration::from_millis(100)) => {
                    let mut downloads = self.downloads.lock().await;
                    if let Some(task) = downloads.get_mut(&download_id) {
                        match task.status.state {
                            DownloadState::Paused => {
                                // Wait for resume
                                drop(downloads);
                                sleep(Duration::from_millis(500)).await;
                                continue;
                            }
                            DownloadState::AwaitingResume => {
                                // Resume was requested
                                task.status.state = DownloadState::Downloading;
                                let status = task.status.clone();
                                drop(downloads);
                                self.emit_status(&status).await?;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Note: Task completion is handled by the AbortHandle mechanism
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
        self.update_state(&download_id, DownloadState::Handshake, None, None).await;

        // Transition to PreparingHead
        sleep(Duration::from_millis(800)).await;
        self.update_state(&download_id, DownloadState::PreparingHead, Some(total_size), Some("\"test-etag-123\"".to_string())).await;

        // Transition to Downloading
        sleep(Duration::from_millis(500)).await;
        self.update_state(&download_id, DownloadState::Downloading, Some(total_size), Some("\"test-etag-123\"".to_string())).await;

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
        self.update_state(&download_id, DownloadState::VerifyingSha, Some(total_size), Some("\"test-etag-123\"".to_string())).await;

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
    async fn update_state(&self, download_id: &str, state: DownloadState, expected_size: Option<u64>, etag: Option<String>) {
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
        let metadata = DownloadMetadata::new(download_id.clone(), request.source_url.clone());

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

        // Store download task
        downloads.insert(
            download_id.clone(),
            DownloadTask {
                status: status.clone(),
                metadata,
                destination_path: dest_path,
                active_task: None,
            },
        );

        // Emit initial status
        drop(downloads); // Release lock before emitting
        self.emit_status(&status).await?;

        // Start actual download state machine in background task
        let service = self.clone_service();
        let download_id_clone = download_id.clone();
        let source_url = request.source_url.clone();
        let dest_path = request.destination_path.clone();
        let expected_sha256 = request.expected_sha256.clone();

        tokio::spawn(async move {
            if let Err(e) = service.run_download_state_machine(
                download_id_clone,
                source_url,
                dest_path,
                expected_sha256,
            ).await {
                tracing::error!("Download state machine failed: {}", e);
            }
        });

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
        let task = downloads
            .get_mut(download_id)
            .unwrap(); // Safe because we checked existence above

        // Only pause if currently downloading
        if task.status.state != DownloadState::Downloading
            && task.status.state != DownloadState::PersistingProgress
        {
            return Err(DownloadError::Invalid(
                "cannot pause download in current state".to_string(),
            ));
        }

        // Update state to Paused and get task handle before dropping lock
        task.status.state = DownloadState::Paused;
        let status = task.status.clone();
        let task_handle = task.active_task.take();

        drop(downloads); // Release lock before emitting
        self.emit_status(&status).await?;

        // Cancel the active download task if it's running
        if let Some(task_handle) = task_handle {
            task_handle.abort();
            info!("Cancelled download task for {}", download_id);
        }

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
        let task = downloads
            .get_mut(download_id)
            .unwrap(); // Safe because we checked existence above

        // Only resume if paused or awaiting resume
        if task.status.state != DownloadState::Paused
            && task.status.state != DownloadState::AwaitingResume
        {
            return Err(DownloadError::Invalid(
                "cannot resume download in current state".to_string(),
            ));
        }

        // Extract data needed for resume before releasing lock
        let source_url = task.metadata.url.clone();
        let dest_path = task.destination_path.clone();
        let expected_sha256 = task.metadata.sha256_final.clone();
        let bytes_already_downloaded = task.status.bytes_downloaded;
        let total_size = task.status.expected_size.unwrap_or(0);
        let etag = task.status.etag.clone();

        // Update state to indicate resume is starting
        task.status.state = DownloadState::Downloading;
        let status = task.status.clone();

        drop(downloads); // Release lock before emitting
        self.emit_status(&status).await?;

        // Start the resume download task
        let http_client = HttpDownloadClient::new();
        let (progress_tx, progress_rx) = mpsc::channel::<HttpDownloadProgress>(32);

        let task_handle = tokio::spawn(async move {
            // Use HTTP Range requests to resume from current position
            let result = http_client.resume_download_from_offset(
                &source_url,
                &expected_sha256.unwrap_or_else(|| "unknown-hash".to_string()),
                std::path::Path::new(&dest_path),
                bytes_already_downloaded,
                total_size,
                Some(progress_tx),
            ).await;

            if let Err(e) = result {
                tracing::error!("HTTP resume failed: {}", e);
            }
        });

        // Store the task handle
        {
            let mut downloads = self.downloads.lock().await;
            if let Some(task) = downloads.get_mut(download_id) {
                task.active_task = Some(task_handle);
            }
        }

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
        assert_eq!(DownloadError::NotFound.to_error_code(), "DOWNLOAD_NOT_FOUND");
        assert_eq!(
            DownloadError::DiskFull.to_error_code(),
            "STORAGE_EXHAUSTED"
        );
        assert_eq!(
            DownloadError::Invalid("test".to_string()).to_error_code(),
            "DOWNLOAD_INVALID_REQUEST"
        );
    }

    #[test]
    fn test_metadata_version_validation() {
        let mut metadata = DownloadMetadata::new("test-id".to_string(), "http://example.com".to_string());
        assert!(metadata.validate_version().is_ok());

        metadata.version = 999;
        assert!(metadata.validate_version().is_err());
    }
}
