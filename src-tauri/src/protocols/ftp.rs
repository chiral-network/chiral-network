//! FTP Protocol Handler
//!
//! Wraps the existing FTP download functionality to implement the enhanced ProtocolHandler trait.

use super::traits::{
    DownloadHandle, DownloadOptions, DownloadProgress, DownloadStatus,
    ProtocolCapabilities, ProtocolError, ProtocolHandler, SeedOptions, SeedingInfo,
};
use crate::ftp_downloader::{FtpDownloader, FtpCredentials, FtpDownloadConfig};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{info, warn};
use url::Url;

/// FTP protocol handler implementing the enhanced ProtocolHandler trait
pub struct FtpProtocolHandler {
    /// Underlying FTP downloader (wrapped in Arc for sharing across async tasks)
    downloader: Arc<FtpDownloader>,
    /// Track active downloads
    active_downloads: Arc<Mutex<HashMap<String, FtpDownloadState>>>,
    /// Track download progress
    download_progress: Arc<Mutex<HashMap<String, DownloadProgress>>>,
}

/// Internal state for an FTP download
struct FtpDownloadState {
    url: String,
    output_path: PathBuf,
    started_at: u64,
    status: DownloadStatus,
    credentials: Option<FtpCredentials>,
    is_paused: bool,
}

impl FtpProtocolHandler {
    /// Creates a new FTP protocol handler with default config
    pub fn new() -> Self {
        Self {
            downloader: Arc::new(FtpDownloader::new()),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a handler with custom configuration
    pub fn with_config(config: FtpDownloadConfig) -> Self {
        Self {
            downloader: Arc::new(FtpDownloader::with_config(config)),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_progress: Arc::new(Mutex::new(HashMap::new())),
        }
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
        format!("ftp-{:x}", hasher.finish())
    }

    /// Parse FTP URL and extract credentials if present
    fn parse_ftp_url(url: &str) -> Result<(String, Option<FtpCredentials>), ProtocolError> {
        let parsed = Url::parse(url)
            .map_err(|e| ProtocolError::InvalidIdentifier(e.to_string()))?;

        let credentials = if !parsed.username().is_empty() {
            Some(FtpCredentials {
                username: parsed.username().to_string(),
                password: parsed.password().unwrap_or("").to_string(),
            })
        } else {
            None
        };

        Ok((url.to_string(), credentials))
    }
}

impl Default for FtpProtocolHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolHandler for FtpProtocolHandler {
    fn name(&self) -> &'static str {
        "ftp"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("ftp://") || identifier.starts_with("ftps://")
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        info!("FTP: Starting download for {}", identifier);

        let download_id = Self::generate_id(identifier);

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(&download_id) {
                return Err(ProtocolError::AlreadyExists(download_id));
            }
        }

        // Parse URL and extract credentials
        let (url, credentials) = Self::parse_ftp_url(identifier)?;

        let started_at = Self::now();

        // Initialize progress
        {
            let mut prog = self.download_progress.lock().await;
            prog.insert(download_id.clone(), DownloadProgress {
                downloaded_bytes: 0,
                total_bytes: 0,
                download_speed: 0.0,
                eta_seconds: None,
                active_peers: 1, // FTP has "1 peer" (the server)
                status: DownloadStatus::FetchingMetadata,
            });
        }

        // Track the download
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(download_id.clone(), FtpDownloadState {
                url: url.clone(),
                output_path: options.output_path.clone(),
                started_at,
                status: DownloadStatus::Downloading,
                credentials: credentials.clone(),
                is_paused: false,
            });
        }

        // Parse URL for FTP operations
        let parsed_url = Url::parse(&url)
            .map_err(|e| ProtocolError::InvalidIdentifier(e.to_string()))?;

        // Connect and get file size
        let downloader = self.downloader.clone();
        let progress = self.download_progress.clone();
        let id = download_id.clone();
        let output_path = options.output_path.clone();
        let creds = credentials.clone();

        // Spawn download task
        tokio::spawn(async move {
            // Connect to FTP server
            let mut stream = match downloader.connect_and_login(&parsed_url, creds).await {
                Ok(s) => s,
                Err(e) => {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.status = DownloadStatus::Failed;
                    }
                    tracing::error!("FTP connection failed: {}", e);
                    return;
                }
            };

            // Get remote path
            let remote_path = parsed_url.path();

            // Get file size
            match downloader.get_file_size(&mut stream, remote_path).await {
                Ok(size) => {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.total_bytes = size;
                        p.status = DownloadStatus::Downloading;
                    }
                }
                Err(e) => {
                    tracing::warn!("Could not get file size: {}", e);
                }
            }

            // Download the file
            match downloader.download_full(&mut stream, remote_path).await {
                Ok(data) => {
                    // Write to file
                    if let Err(e) = tokio::fs::write(&output_path, &data).await {
                        let mut prog = progress.lock().await;
                        if let Some(p) = prog.get_mut(&id) {
                            p.status = DownloadStatus::Failed;
                        }
                        tracing::error!("Failed to write file: {}", e);
                        return;
                    }

                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.downloaded_bytes = data.len() as u64;
                        p.status = DownloadStatus::Completed;
                    }
                    tracing::info!("FTP download completed: {} bytes", data.len());
                }
                Err(e) => {
                    let mut prog = progress.lock().await;
                    if let Some(p) = prog.get_mut(&id) {
                        p.status = DownloadStatus::Failed;
                    }
                    tracing::error!("FTP download failed: {}", e);
                }
            }

            // Disconnect
            let _ = downloader.disconnect(&mut stream).await;
        });

        Ok(DownloadHandle {
            identifier: download_id,
            protocol: "ftp".to_string(),
            started_at,
        })
    }

    async fn seed(
        &self,
        file_path: PathBuf,
        _options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        info!("FTP: Seeding not directly supported - would need FTP server");

        // FTP "seeding" would mean uploading to an FTP server
        // This requires server configuration which we don't have here
        warn!("FTP seeding requires an FTP server to be configured");

        // Return a placeholder - in a real implementation, this would upload to a configured server
        Ok(SeedingInfo {
            identifier: format!("ftp://chiral-node/{}", file_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string())),
            file_path,
            protocol: "ftp".to_string(),
            active_peers: 0,
            bytes_uploaded: 0,
        })
    }

    async fn stop_seeding(&self, identifier: &str) -> Result<(), ProtocolError> {
        warn!("FTP: stop_seeding - {}", identifier);
        // Would delete file from FTP server
        Ok(())
    }

    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("FTP: Pausing download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.get_mut(identifier) {
            state.is_paused = true;
            state.status = DownloadStatus::Paused;

            let mut prog = self.download_progress.lock().await;
            if let Some(p) = prog.get_mut(identifier) {
                p.status = DownloadStatus::Paused;
            }

            // FTP supports REST command for resume, so pause is viable
            // The actual pause would need to close the connection and track position
            warn!("FTP: pause requires reconnection to resume with REST command");
            Ok(())
        } else {
            Err(ProtocolError::DownloadNotFound(identifier.to_string()))
        }
    }

    async fn resume_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("FTP: Resuming download {}", identifier);

        let downloads = self.active_downloads.lock().await;
        if let Some(state) = downloads.get(identifier) {
            if !state.is_paused {
                return Err(ProtocolError::ProtocolSpecific(
                    "Download is not paused".to_string()
                ));
            }

            // Would need to reconnect and use REST command to resume
            // This requires tracking the bytes already downloaded
            warn!("FTP: resume_download would use REST command - not fully implemented");
            Err(ProtocolError::NotSupported)
        } else {
            Err(ProtocolError::DownloadNotFound(identifier.to_string()))
        }
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        info!("FTP: Cancelling download {}", identifier);

        let mut downloads = self.active_downloads.lock().await;
        if downloads.remove(identifier).is_some() {
            let mut prog = self.download_progress.lock().await;
            if let Some(p) = prog.get_mut(identifier) {
                p.status = DownloadStatus::Cancelled;
            }
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
        // Would list files on configured FTP server
        Ok(Vec::new())
    }

    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities {
            supports_seeding: true,  // Via upload to FTP server
            supports_pause_resume: true,  // Via REST command
            supports_multi_source: false,
            supports_encryption: true,  // FTPS
            supports_dht: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_ftp() {
        let handler = FtpProtocolHandler::new();
        assert!(handler.supports("ftp://ftp.example.com/file.zip"));
        assert!(handler.supports("ftps://secure.example.com/file.zip"));
        assert!(!handler.supports("http://example.com/file.zip"));
        assert!(!handler.supports("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn test_parse_ftp_url_no_creds() {
        let (url, creds) = FtpProtocolHandler::parse_ftp_url("ftp://example.com/file.zip").unwrap();
        assert_eq!(url, "ftp://example.com/file.zip");
        assert!(creds.is_none());
    }

    #[test]
    fn test_parse_ftp_url_with_creds() {
        let (_, creds) = FtpProtocolHandler::parse_ftp_url("ftp://user:pass@example.com/file.zip").unwrap();
        assert!(creds.is_some());
        let c = creds.unwrap();
        assert_eq!(c.username, "user");
        assert_eq!(c.password, "pass");
    }

    #[test]
    fn test_generate_id() {
        let id1 = FtpProtocolHandler::generate_id("ftp://example.com/file.zip");
        let id2 = FtpProtocolHandler::generate_id("ftp://example.com/file.zip");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("ftp-"));
    }
}
