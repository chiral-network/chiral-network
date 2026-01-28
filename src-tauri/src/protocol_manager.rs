// protocol_manager.rs
// Protocol Manager for orchestrating file uploads/downloads across different protocols
//
// This module provides a unified interface for uploading and downloading files using
// various protocols (FTP, WebRTC, etc.). It cleanly separates concerns:
// 1. File hashing and validation
// 2. Protocol-specific upload/download operations
// 3. DHT minimal record publishing (discovery)
// 4. GossipSub protocol metadata publishing (detailed info)

use crate::app_state::CoreServices;
use crate::dht::{self, DhtService};
use crate::file_transfer::FileTransferService;
use crate::ftp_client::{decrypt_ftp_password, encrypt_ftp_password};
use crate::gossipsub_metadata::{
    FtpProtocolDetails, FtpSourceInfo, ProtocolDetails, WebRtcProtocolDetails,
};
use crate::transfer_events::{
    current_timestamp_ms, AppEventBus, ErrorCategory, SourceInfo, SourceType,
    TransferFailedEvent, TransferStartedEvent,
};
use crate::webrtc_service;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::time::Duration;
use tracing::{info, warn};

/// Result of FTP upload operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtpUploadResult {
    pub file_hash: String,
    pub ftp_url: String,
    pub ftp_source: FtpSourceInfo,
}

/// Result of WebRTC upload operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRTCUploadResult {
    pub file_hash: String,
    pub file_path: String,
}

/// Progress event for file hashing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHashingProgress {
    pub file_path: String,
    pub bytes_hashed: u64,
    pub total_bytes: u64,
    pub percent: f64,
}

/// File metadata for upload
#[derive(Debug, Clone)]
struct FileMetadata {
    name: String,
    size: u64,
    mime_type: Option<String>,
}

/// Protocol Manager for handling uploads and downloads
pub struct ProtocolManager {
    app_handle: tauri::AppHandle,
    event_bus: Option<AppEventBus>,
}

impl ProtocolManager {
    /// Create a new ProtocolManager
    /// Services (DHT, FileTransfer) are queried from CoreServices state when needed
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle: app_handle.clone(),
            event_bus: Some(AppEventBus::new(app_handle)),
        }
    }

    /// Get DHT service from CoreServices state
    async fn dht(&self) -> Option<Arc<DhtService>> {
        let state = self.app_handle.state::<CoreServices>();
        let guard = state.dht.lock().await;
        guard.clone()
    }

    /// Get FileTransferService from CoreServices state
    async fn file_transfer(&self) -> Option<Arc<FileTransferService>> {
        let state = self.app_handle.state::<CoreServices>();
        let guard = state.file_transfer.lock().await;
        guard.clone()
    }

    /// Get WebRTCService from CoreServices state
    async fn webrtc(&self) -> Option<Arc<crate::webrtc_service::WebRTCService>> {
        let state = self.app_handle.state::<CoreServices>();
        let guard = state.webrtc.lock().await;
        guard.clone()
    }

    /// Upload file to external FTP server and publish metadata
    ///
    /// This method orchestrates the complete FTP upload flow:
    /// 1. Hash file locally BEFORE upload (with progress tracking)
    /// 2. Upload to external FTP server
    /// 3. Publish minimal DHT record (discovery only)
    /// 4. Encrypt FTP credentials
    /// 5. Publish protocol metadata to GossipSub
    ///
    /// # Arguments
    /// * `file_path` - Local path to file to upload
    /// * `ftp_url` - FTP server URL (e.g., "ftp://example.com/uploads/")
    /// * `username` - Optional FTP username
    /// * `password` - Optional FTP password (will be encrypted)
    /// * `use_ftps` - Whether to use FTPS (FTP over TLS)
    /// * `passive_mode` - Whether to use passive mode
    /// * `price_per_mb` - Price per MB for downloading this file
    pub async fn upload_via_ftp(
        &self,
        file_path: String,
        ftp_url: String,
        username: Option<String>,
        password: Option<String>,
        use_ftps: bool,
        passive_mode: bool,
        price_per_mb: f64,
    ) -> Result<FtpUploadResult> {
        info!("🚀 Starting FTP upload: file={}", file_path);

        // 1. Hash file locally BEFORE upload
        let file_hash = self.hash_file(&file_path).await?;
        let file_metadata = self.get_file_metadata(&file_path).await?;
        info!("✅ File hashed: {}", file_hash);

        // 2. Upload to external FTP server
        let ftp_upload_url = self
            .upload_to_ftp(
                &file_path,
                &ftp_url,
                &file_hash,
                username.as_deref(),
                password.as_deref(),
                use_ftps,
                passive_mode,
            )
            .await?;
        info!("✅ Uploaded to FTP: {}", ftp_upload_url);

        // 3. Publish minimal DHT record
        if let Some(dht) = self.dht().await {
            dht.publish_minimal_dht(
                file_hash.clone(),
                file_metadata.name.clone(),
                file_metadata.size,
                file_metadata.mime_type.clone(),
            )
            .await
            .map_err(|e| anyhow!("{}", e))?;
        }
        info!("✅ Published minimal DHT record");

        // 4. Encrypt credentials for sharing
        let encrypted_password = if let Some(pwd) = password.as_ref() {
            Some(encrypt_ftp_password(pwd, &file_hash)?)
        } else {
            None
        };

        // 5. Create FTP source info
        let ftp_source = FtpSourceInfo {
            url: ftp_upload_url.clone(),
            username: username.clone(),
            encrypted_password,
            passive_mode,
            use_ftps,
            timeout_secs: Some(30),
            supports_resume: true,
            file_size: file_metadata.size,
            last_checked: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
            is_available: true,
        };

        // 6. Create protocol details for GossipSub
        let protocol_details = ProtocolDetails {
            ftp: Some(FtpProtocolDetails {
                sources: vec![ftp_source.clone()],
            }),
            ..Default::default()
        };

        // 7. Publish protocol metadata to GossipSub (non-fatal - will be republished periodically)
        if let Some(dht) = self.dht().await {
            match dht
                .publish_protocol_metadata(file_hash.clone(), protocol_details, price_per_mb)
                .await
            {
                Ok(_) => info!("✅ Published protocol metadata to GossipSub"),
                Err(e) => warn!(
                    "⚠️ Failed to publish to GossipSub (will retry periodically): {}",
                    e
                ),
            }
        }

        Ok(FtpUploadResult {
            file_hash,
            ftp_url: ftp_upload_url,
            ftp_source,
        })
    }

    /// Seed file via WebRTC (internal seeding, no external upload)
    ///
    /// This method orchestrates the WebRTC seeding flow:
    /// 1. Hash file locally (with progress tracking)
    /// 2. Publish minimal DHT record (discovery only)
    /// 3. Publish protocol metadata to GossipSub (WebRTC enabled)
    /// 4. File stays local and is served via WebRTC transfers
    ///
    /// # Arguments
    /// * `file_path` - Local path to file to seed
    /// * `price_per_mb` - Price per MB for downloading this file
    pub async fn upload_via_webrtc(
        &self,
        file_path: String,
        price_per_mb: f64,
    ) -> Result<WebRTCUploadResult> {
        info!("🚀 Starting WebRTC seeding: file={}", file_path);

        // 1. Hash file
        let file_hash = self.hash_file(&file_path).await?;
        let file_metadata = self.get_file_metadata(&file_path).await?;
        info!("✅ File hashed: {}", file_hash);

        // 2. Publish minimal DHT record
        if let Some(dht) = self.dht().await {
            dht.publish_minimal_dht(
                file_hash.clone(),
                file_metadata.name.clone(),
                file_metadata.size,
                file_metadata.mime_type.clone(),
            )
            .await
            .map_err(|e| anyhow!("{}", e))?;
        }
        info!("✅ Published minimal DHT record");

        // 3. WebRTC doesn't need external upload - file stays local
        // Just mark it as available for WebRTC transfer

        // 4. Create protocol details (WebRTC enabled)
        let protocol_details = ProtocolDetails {
            webrtc: Some(WebRtcProtocolDetails { enabled: true }),
            ..Default::default()
        };

        // 5. Publish protocol metadata to GossipSub (non-fatal - will be republished periodically)
        if let Some(dht) = self.dht().await {
            match dht
                .publish_protocol_metadata(file_hash.clone(), protocol_details, price_per_mb)
                .await
            {
                Ok(_) => info!("✅ Published protocol metadata to GossipSub"),
                Err(e) => warn!(
                    "⚠️ Failed to publish to GossipSub (will retry periodically): {}",
                    e
                ),
            }
        }

        // 6. Store file data so peers can request it over WebRTC
        let ft = self
            .file_transfer()
            .await
            .ok_or_else(|| anyhow!("FileTransferService not initialized"))?;
        let file_data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| anyhow!("Failed to read file for storage: {}", e))?;
        ft.store_file_data(file_hash.clone(), file_metadata.name.clone(), file_data)
            .await;

        Ok(WebRTCUploadResult {
            file_hash,
            file_path,
        })
    }

    /// Download file from FTP source
    ///
    /// # Arguments
    /// * `ftp_source` - FTP source information (may contain encrypted password)
    /// * `file_hash` - File hash for password decryption
    /// * `destination` - Local destination path
    pub async fn download_via_ftp(
        &self,
        ftp_source: FtpSourceInfo,
        file_hash: String,
        destination: String,
    ) -> Result<()> {
        use suppaftp::types::FileType;
        use suppaftp::{FtpStream, NativeTlsFtpStream};

        // Decrypt password if present
        let password = if let Some(encrypted) = &ftp_source.encrypted_password {
            Some(decrypt_ftp_password(encrypted, &file_hash)?)
        } else {
            None
        };

        // Parse URL
        let url = url::Url::parse(&ftp_source.url).context("Failed to parse FTP URL")?;

        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("No host in FTP URL"))?;
        let port = url.port().unwrap_or(21);
        let remote_path = url.path();

        let host = host.to_string();
        let username = ftp_source.username.clone();
        let use_ftps = ftp_source.use_ftps;
        let passive_mode = ftp_source.passive_mode;
        let remote_path = remote_path.to_string();
        let destination_clone = destination.clone();

        // Download in blocking task
        tokio::task::spawn_blocking(move || -> Result<()> {
            if use_ftps {
                let mut ftp = NativeTlsFtpStream::connect((host.as_str(), port))
                    .context("Failed to connect to FTPS server")?;

                ftp.login(
                    username.as_deref().unwrap_or("anonymous"),
                    password.as_deref().unwrap_or("anonymous@"),
                )
                .context("FTPS login failed")?;

                if passive_mode {
                    ftp.set_mode(suppaftp::Mode::Passive);
                }

                ftp.transfer_type(FileType::Binary)
                    .context("Failed to set binary mode")?;

                let mut file = std::fs::File::create(&destination_clone)
                    .context("Failed to create output file")?;

                ftp.retr(&remote_path, |stream| {
                    let _ = std::io::copy(stream, &mut file);
                    Ok(())
                })
                .context("FTPS download failed")?;

                ftp.quit().ok();
                Ok(())
            } else {
                let mut ftp = FtpStream::connect((host.as_str(), port))
                    .context("Failed to connect to FTP server")?;

                ftp.login(
                    username.as_deref().unwrap_or("anonymous"),
                    password.as_deref().unwrap_or("anonymous@"),
                )
                .context("FTP login failed")?;

                if passive_mode {
                    ftp.set_mode(suppaftp::Mode::Passive);
                }

                ftp.transfer_type(FileType::Binary)
                    .context("Failed to set binary mode")?;

                let mut file = std::fs::File::create(&destination_clone)
                    .context("Failed to create output file")?;

                ftp.retr(&remote_path, |stream| {
                    let _ = std::io::copy(stream, &mut file);
                    Ok(())
                })
                .context("FTP download failed")?;

                ftp.quit().ok();
                Ok(())
            }
        })
        .await
        .map_err(|e| anyhow!("Join error: {}", e))?
    }

    /// Download file via WebRTC
    ///
    /// # Arguments
    /// * `peer_id` - Peer ID to download from
    /// * `file_hash` - Hash of file to download
    /// * `file_name` - Name of the file
    /// * `file_size` - Size of the file in bytes
    /// * `destination` - Local destination path
    ///
    /// Returns the tracking transfer ID for progress
    pub async fn download_via_webrtc(
        &self,
        peer_id: String,
        file_hash: String,
        file_name: String,
        file_size: u64,
        destination: String,
    ) -> Result<String> {
        let transfer_id = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            peer_id.hash(&mut hasher);
            file_hash.hash(&mut hasher);
            file_name.hash(&mut hasher);
            file_size.hash(&mut hasher);
            destination.hash(&mut hasher);
            format!("{:x}", hasher.finish())
        };

        let emit_failed = |error: &str, category: ErrorCategory| {
            if let Some(ref event_bus) = self.event_bus {
                event_bus.emit_failed(TransferFailedEvent {
                    transfer_id: transfer_id.clone(),
                    file_hash: file_hash.clone(),
                    protocol: "WEBRTC".to_string(),
                    failed_at: current_timestamp_ms(),
                    error: error.to_string(),
                    error_category: category,
                    downloaded_bytes: 0,
                    total_bytes: file_size,
                    retry_possible: true,
                });
            }
        };

        if peer_id.trim().is_empty() {
            let message = "peer_id is required".to_string();
            emit_failed(&message, ErrorCategory::Protocol);
            return Err(anyhow!(message));
        }

        let resolved_output_path = {
            let p = PathBuf::from(&destination);
            if p.exists() && p.is_dir() {
                info!(
                    "Output path is a directory; resolving filename to {}",
                    file_name
                );
                p.join(&file_name)
            } else {
                p
            }
        };

        let resolved_output_path_str = resolved_output_path.to_string_lossy().to_string();

        if let Some(parent) = resolved_output_path.parent() {
            if !parent.exists() {
                let message = format!("Directory does not exist: {}", parent.display());
                emit_failed(&message, ErrorCategory::Filesystem);
                return Err(anyhow!(message));
            }
            if !parent.is_dir() {
                let message = format!("Path is not a directory: {}", parent.display());
                emit_failed(&message, ErrorCategory::Filesystem);
                return Err(anyhow!(message));
            }
        } else {
            let message = "Invalid file path".to_string();
            emit_failed(&message, ErrorCategory::Filesystem);
            return Err(anyhow!(message));
        }

        let total_chunks = (file_size as f64 / (32.0 * 1024.0)).ceil() as u32;

        if let Some(ref event_bus) = self.event_bus {
            event_bus.emit_started(TransferStartedEvent {
                transfer_id: transfer_id.clone(),
                file_hash: file_hash.clone(),
                protocol: "WEBRTC".to_string(),
                file_name: file_name.clone(),
                file_size,
                total_chunks,
                chunk_size: 32 * 1024,
                started_at: current_timestamp_ms(),
                available_sources: vec![SourceInfo {
                    id: peer_id.clone(),
                    source_type: SourceType::WebRtc,
                    address: peer_id.clone(),
                    reputation: None,
                    estimated_speed_bps: None,
                    latency_ms: None,
                    location: None,
                }],
                selected_sources: vec![peer_id.clone()],
            });
        }

        info!(
            "Starting WebRTC download: transfer_id={} file_hash={} peer_id={} file_name={} file_size={} output_path={}",
            transfer_id, file_hash, peer_id, file_name, file_size, resolved_output_path_str
        );

        webrtc_service::set_requested_download_output_path(
            file_hash.clone(),
            resolved_output_path_str.clone(),
        )
        .await;
        webrtc_service::set_requested_download_transfer_id(
            file_hash.clone(),
            transfer_id.clone(),
        )
        .await;
        webrtc_service::set_requested_download_start_time(file_hash.clone()).await;
        info!(
            "Recorded requested output path for WebRTC download: {}",
            resolved_output_path_str
        );

        let webrtc_service = self
            .webrtc()
            .await
            .ok_or_else(|| {
                let message = "WebRTC service not available".to_string();
                emit_failed(&message, ErrorCategory::Protocol);
                anyhow!(message)
            })?;
        let dht_service = self
            .dht()
            .await
            .ok_or_else(|| {
                let message = "DHT service not available".to_string();
                emit_failed(&message, ErrorCategory::Protocol);
                anyhow!(message)
            })?;

        let requester_peer_id = dht_service.get_peer_id().await;
        info!("Requester peer id resolved: {}", requester_peer_id);

        let connected_peers = dht_service.get_connected_peers().await;
        if !connected_peers.contains(&peer_id) {
            info!(
                "Peer {} not connected, attempting to connect before signaling",
                peer_id
            );
            dht_service
                .connect_to_peer_by_id(peer_id.clone())
                .await
                .map_err(|e| {
                    let message = format!("Failed to connect to peer: {}", e);
                    emit_failed(&message, ErrorCategory::Network);
                    anyhow!(message)
                })?;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if webrtc_service.has_open_connection(&peer_id).await {
            info!("Reusing existing WebRTC connection to peer {}", peer_id);
            let file_request = webrtc_service::WebRTCFileRequest {
                file_hash: file_hash.clone(),
                file_name: file_name.clone(),
                file_size,
                requester_peer_id,
                recipient_public_key: None,
            };

            webrtc_service
                .send_file_request(peer_id.clone(), file_request)
                .await
                .map_err(|e| {
                    let message = format!("Failed to send file request: {}", e);
                    emit_failed(&message, ErrorCategory::Network);
                    anyhow!(message)
                })?;

            info!(
                "Sent file request for {} to peer {} (reused connection)",
                file_name, peer_id
            );
            return Ok(transfer_id);
        }

        info!(
            "Creating WebRTC offer for peer {} via DHT signaling",
            peer_id
        );
        let offer = webrtc_service
            .create_offer(peer_id.clone())
            .await
            .map_err(|e| {
                let message = format!("WebRTC offer creation failed: {}", e);
                emit_failed(&message, ErrorCategory::Protocol);
                anyhow!(message)
            })?;

        let offer_request = dht::WebRTCOfferRequest {
            offer_sdp: offer,
            file_hash: file_hash.clone(),
            requester_peer_id: requester_peer_id.clone(),
        };

        info!("Sending WebRTC offer to peer {}", peer_id);
        let answer_receiver = dht_service
            .send_webrtc_offer(peer_id.clone(), offer_request)
            .await
            .map_err(|e| {
                let message = format!("Failed to send WebRTC offer: {}", e);
                emit_failed(&message, ErrorCategory::Network);
                anyhow!(message)
            })?;

        info!("Waiting for WebRTC answer from peer {}", peer_id);
        let answer_response = match tokio::time::timeout(Duration::from_secs(30), answer_receiver)
            .await
        {
            Ok(Ok(Ok(answer))) => answer,
            Ok(Ok(Err(e))) => {
                let message = format!("WebRTC signaling failed: {}", e);
                emit_failed(&message, ErrorCategory::Network);
                return Err(anyhow!(message));
            }
            Ok(Err(_)) => {
                let message = "WebRTC answer receiver was canceled".to_string();
                emit_failed(&message, ErrorCategory::Network);
                return Err(anyhow!(message));
            }
            Err(_) => {
                let message = format!("WebRTC answer timeout from peer {}", peer_id);
                emit_failed(&message, ErrorCategory::Network);
                return Err(anyhow!(message));
            }
        };

        info!("Received WebRTC answer from peer {}", peer_id);
        webrtc_service
            .establish_connection_with_answer(peer_id.clone(), answer_response.answer_sdp)
            .await
            .map_err(|e| {
                let message = format!("Failed to establish WebRTC connection: {}", e);
                emit_failed(&message, ErrorCategory::Network);
                anyhow!(message)
            })?;
        info!("WebRTC connection established with peer {}", peer_id);

        let file_request = webrtc_service::WebRTCFileRequest {
            file_hash: file_hash.clone(),
            file_name: file_name.clone(),
            file_size,
            requester_peer_id,
            recipient_public_key: None,
        };

        webrtc_service
            .send_file_request(peer_id.clone(), file_request)
            .await
            .map_err(|e| {
                let message = format!("Failed to send file request: {}", e);
                emit_failed(&message, ErrorCategory::Network);
                anyhow!(message)
            })?;

        info!("Sent file request for {} to peer {}", file_name, peer_id);

        Ok(transfer_id)
    }

    // ------ Private helper methods ------

    /// Hash file efficiently using SHA-256 with progress tracking
    async fn hash_file(&self, file_path: &str) -> Result<String> {
        let mut file = File::open(file_path)
            .await
            .context("Failed to open file for hashing")?;

        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024]; // 64KB chunks
        let file_size = file.metadata().await?.len();
        let mut bytes_read_total = 0u64;

        loop {
            let n = file
                .read(&mut buffer)
                .await
                .context("Failed to read file during hashing")?;
            if n == 0 {
                break;
            }

            hasher.update(&buffer[..n]);
            bytes_read_total += n as u64;

            // Emit progress event every 100ms (throttled by frontend)
            let _ = self.app_handle.emit(
                "file_hashing_progress",
                FileHashingProgress {
                    file_path: file_path.to_string(),
                    bytes_hashed: bytes_read_total,
                    total_bytes: file_size,
                    percent: if file_size > 0 {
                        (bytes_read_total as f64 / file_size as f64) * 100.0
                    } else {
                        100.0
                    },
                },
            );
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Get file metadata (name, size, MIME type)
    async fn get_file_metadata(&self, file_path: &str) -> Result<FileMetadata> {
        let path = Path::new(file_path);
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let metadata = tokio::fs::metadata(file_path)
            .await
            .context("Failed to read file metadata")?;

        // Simple MIME type guessing based on extension
        let mime_type = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| match ext.to_lowercase().as_str() {
                "txt" => Some("text/plain"),
                "pdf" => Some("application/pdf"),
                "jpg" | "jpeg" => Some("image/jpeg"),
                "png" => Some("image/png"),
                "gif" => Some("image/gif"),
                "mp4" => Some("video/mp4"),
                "mp3" => Some("audio/mpeg"),
                "zip" => Some("application/zip"),
                "json" => Some("application/json"),
                _ => None,
            })
            .map(String::from);

        Ok(FileMetadata {
            name: file_name,
            size: metadata.len(),
            mime_type,
        })
    }

    /// Upload to external FTP (implementation using suppaftp)
    async fn upload_to_ftp(
        &self,
        file_path: &str,
        ftp_url: &str,
        remote_file_name: &str,
        username: Option<&str>,
        password: Option<&str>,
        use_ftps: bool,
        passive_mode: bool,
    ) -> Result<String> {
        use suppaftp::types::FileType;
        use suppaftp::{FtpStream, NativeTlsFtpStream};

        // Parse FTP URL
        let url = url::Url::parse(ftp_url).context("Failed to parse FTP URL")?;

        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("No host in FTP URL"))?;
        let port = url.port().unwrap_or(21);

        // Read file
        let file_data = tokio::fs::read(file_path)
            .await
            .context("Failed to read file")?;

        // Upload in blocking task
        let host = host.to_string();
        let username = username.map(String::from);
        let password = password.map(String::from);
        let file_name = remote_file_name.to_string();

        tokio::task::spawn_blocking(move || -> Result<String> {
            if use_ftps {
                // FTPS (FTP over TLS)
                let mut ftp = NativeTlsFtpStream::connect((host.as_str(), port))
                    .context("Failed to connect to FTPS server")?;

                ftp.login(
                    username.as_deref().unwrap_or("anonymous"),
                    password.as_deref().unwrap_or("anonymous@"),
                )
                .context("FTPS login failed")?;

                if passive_mode {
                    ftp.set_mode(suppaftp::Mode::Passive);
                }

                ftp.transfer_type(FileType::Binary)
                    .context("Failed to set binary mode")?;

                let mut cursor = std::io::Cursor::new(file_data);
                ftp.put_file(&file_name, &mut cursor)
                    .context("FTPS upload failed")?;

                ftp.quit().ok();
                Ok(format!("ftps://{}:{}/{}", host, port, file_name))
            } else {
                // Regular FTP
                let mut ftp = FtpStream::connect((host.as_str(), port))
                    .context("Failed to connect to FTP server")?;

                ftp.login(
                    username.as_deref().unwrap_or("anonymous"),
                    password.as_deref().unwrap_or("anonymous@"),
                )
                .context("FTP login failed")?;

                if passive_mode {
                    ftp.set_mode(suppaftp::Mode::Passive);
                }

                ftp.transfer_type(FileType::Binary)
                    .context("Failed to set binary mode")?;

                let mut cursor = std::io::Cursor::new(file_data);
                ftp.put_file(&file_name, &mut cursor)
                    .context("FTP upload failed")?;

                ftp.quit().ok();
                Ok(format!("ftp://{}:{}/{}", host, port, file_name))
            }
        })
        .await
        .map_err(|e| anyhow!("Join error: {}", e))?
    }
}
