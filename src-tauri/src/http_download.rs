use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Semaphore};

use crate::manager::ChunkManager;

/// HTTP Download Client for fetching files via HTTP protocol
///
/// Simplified Flow (similar to WebRTC):
/// 1. Fetch manifest from seeder's HTTP endpoint
/// 2. Download chunks in parallel (encrypted or unencrypted)
/// 3. Assemble final file
///
/// Note: Decryption and verification are deferred for future implementation
/// when the RequestFileAccess protocol handler is completed.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpDownloadProgress {
    pub merkle_root: String,
    pub chunks_total: usize,
    pub chunks_downloaded: usize,
    pub chunks_verified: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    FetchingManifest,
    Downloading,
    Assembling,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestResponse {
    pub merkle_root: String,
    pub chunks: Vec<ChunkInfo>,
    pub encrypted_key_bundle: Option<String>,
    pub total_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub index: u32,
    pub encrypted_hash: String,
    pub encrypted_size: usize,
}

/// Maximum concurrent chunk downloads per HTTP source
///
/// Set conservatively to 5 because:
/// - Multi-source downloads will have multiple HTTP sources running in parallel
/// - Total concurrency = MAX_CONCURRENT_CHUNKS × number_of_sources
/// - Example: 3 sources × 5 chunks each = 15 concurrent downloads total
/// - Prevents network/bandwidth saturation
/// - Leaves headroom for WebRTC/Bitswap downloads happening simultaneously
const MAX_CONCURRENT_CHUNKS: usize = 5;

pub struct HttpDownloadClient {
    http_client: Client,
    chunk_manager: Arc<ChunkManager>,
    /// Semaphore to limit concurrent chunk downloads
    download_semaphore: Arc<Semaphore>,
}

impl HttpDownloadClient {
    pub fn new(chunk_storage_path: std::path::PathBuf) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            chunk_manager: Arc::new(ChunkManager::new(chunk_storage_path)),
            download_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
        }
    }

    /// Download a file from an HTTP seeder
    ///
    /// Arguments:
    /// - `seeder_url`: Base URL of the seeder (e.g., "http://192.168.1.5:8080")
    /// - `merkle_root`: File identifier
    /// - `output_path`: Where to save the final file
    /// - `progress_tx`: Optional channel for progress updates
    ///
    /// Returns: Ok(()) on success
    ///
    /// Note: This simplified version downloads and assembles chunks without decryption.
    /// Files are saved as-is (encrypted if they were encrypted, unencrypted otherwise).
    pub async fn download_file(
        &self,
        seeder_url: &str,
        merkle_root: &str,
        output_path: &Path,
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
    ) -> Result<(), String> {
        tracing::info!(
            "Starting HTTP download: {} from {}",
            merkle_root,
            seeder_url
        );

        // Step 1: Fetch manifest
        self.send_progress(
            &progress_tx,
            merkle_root,
            0,
            0,
            0,
            0,
            0,
            DownloadStatus::FetchingManifest,
        )
        .await;

        let manifest = self.fetch_manifest(seeder_url, merkle_root).await?;

        tracing::info!(
            "Fetched manifest: {} chunks, {} bytes total",
            manifest.chunks.len(),
            manifest.total_size
        );

        // Step 2: Download all chunks
        self.send_progress(
            &progress_tx,
            merkle_root,
            manifest.chunks.len(),
            0,
            0,
            0,
            manifest.total_size as u64,
            DownloadStatus::Downloading,
        )
        .await;

        let chunks = self
            .download_chunks(
                seeder_url,
                &manifest.chunks,
                merkle_root,
                progress_tx.clone(),
            )
            .await?;

        tracing::info!("Downloaded {} chunks", chunks.len());

        // Step 3: Assemble final file
        // Note: Chunks are saved as-is (encrypted if they were encrypted)
        // Decryption will be implemented later when RequestFileAccess handler is complete
        self.send_progress(
            &progress_tx,
            merkle_root,
            manifest.chunks.len(),
            manifest.chunks.len(),
            manifest.chunks.len(),
            manifest.total_size as u64,
            manifest.total_size as u64,
            DownloadStatus::Assembling,
        )
        .await;

        // Extract just the data from chunks (chunks is Vec<(u32, Vec<u8>)>)
        let chunk_data: Vec<Vec<u8>> = chunks.iter().map(|(_, data)| data.clone()).collect();

        self.assemble_file(&chunk_data, output_path).await?;

        // Final status
        self.send_progress(
            &progress_tx,
            merkle_root,
            manifest.chunks.len(),
            manifest.chunks.len(),
            manifest.chunks.len(),
            manifest.total_size as u64,
            manifest.total_size as u64,
            DownloadStatus::Completed,
        )
        .await;

        tracing::info!("Download completed: {}", output_path.display());

        Ok(())
    }

    /// Fetch file manifest from HTTP seeder
    async fn fetch_manifest(
        &self,
        seeder_url: &str,
        merkle_root: &str,
    ) -> Result<ManifestResponse, String> {
        let url = format!("{}/files/{}/manifest", seeder_url, merkle_root);

        tracing::debug!("Fetching manifest from: {}", url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch manifest: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Manifest request failed: {}",
                response.status()
            ));
        }

        let manifest: ManifestResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse manifest: {}", e))?;

        Ok(manifest)
    }

    /// Download all chunks with bounded parallelism
    ///
    /// Uses a semaphore to limit concurrent downloads to MAX_CONCURRENT_CHUNKS,
    /// preventing network/server overload while still achieving good parallelism.
    ///
    /// Flow:
    /// 1. Acquire semaphore permit (blocks if MAX_CONCURRENT_CHUNKS already downloading)
    /// 2. Download chunk
    /// 3. Release permit (allows next chunk to start)
    /// 4. Repeat until all chunks downloaded
    async fn download_chunks(
        &self,
        seeder_url: &str,
        chunks: &[ChunkInfo],
        merkle_root: &str,
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
    ) -> Result<Vec<(u32, Vec<u8>)>, String> {
        let mut tasks = Vec::new();

        for chunk_info in chunks {
            let client = self.http_client.clone();
            let url = format!("{}/chunks/{}", seeder_url, chunk_info.encrypted_hash);
            let index = chunk_info.index;
            let expected_size = chunk_info.encrypted_size;
            let progress_tx = progress_tx.clone();
            let merkle_root = merkle_root.to_string();
            let total_chunks = chunks.len();
            let total_size = chunks.iter().map(|c| c.encrypted_size).sum::<usize>();
            let semaphore = self.download_semaphore.clone();

            // Spawn task for each chunk (but semaphore limits concurrency)
            let task = tokio::spawn(async move {
                // Acquire permit (waits if MAX_CONCURRENT_CHUNKS already downloading)
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| format!("Failed to acquire semaphore: {}", e))?;
                tracing::debug!("Downloading chunk {} from {}", index, url);

                let response = client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download chunk {}: {}", index, e))?;

                if !response.status().is_success() {
                    return Err(format!(
                        "Chunk {} request failed: {}",
                        index,
                        response.status()
                    ));
                }

                let data = response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read chunk {} data: {}", index, e))?
                    .to_vec();

                if data.len() != expected_size {
                    return Err(format!(
                        "Chunk {} size mismatch: expected {}, got {}",
                        index,
                        expected_size,
                        data.len()
                    ));
                }

                tracing::debug!("Downloaded chunk {} ({} bytes)", index, data.len());

                // Send progress update
                if let Some(tx) = progress_tx {
                    let _ = tx
                        .send(HttpDownloadProgress {
                            merkle_root: merkle_root.clone(),
                            chunks_total: total_chunks,
                            chunks_downloaded: index as usize + 1,
                            chunks_verified: 0,
                            bytes_downloaded: data.len() as u64,
                            bytes_total: total_size as u64,
                            status: DownloadStatus::Downloading,
                        })
                        .await;
                }

                Ok::<(u32, Vec<u8>), String>((index, data))
                // Permit automatically released when _permit is dropped
            });

            tasks.push(task);
        }

        tracing::info!(
            "Downloading {} chunks with max {} concurrent downloads",
            chunks.len(),
            MAX_CONCURRENT_CHUNKS
        );

        // Wait for all chunks to download
        let mut results = Vec::new();
        for task in tasks {
            let result = task
                .await
                .map_err(|e| format!("Download task failed: {}", e))??;
            results.push(result);
        }

        // Sort by chunk index
        results.sort_by_key(|(index, _)| *index);

        Ok(results)
    }

    /// Assemble chunks into final file
    async fn assemble_file(
        &self,
        decrypted_chunks: &[Vec<u8>],
        output_path: &Path,
    ) -> Result<(), String> {
        let mut file = File::create(output_path)
            .await
            .map_err(|e| format!("Failed to create output file: {}", e))?;

        for (index, chunk) in decrypted_chunks.iter().enumerate() {
            file.write_all(chunk)
                .await
                .map_err(|e| format!("Failed to write chunk {}: {}", index, e))?;
        }

        file.flush()
            .await
            .map_err(|e| format!("Failed to flush file: {}", e))?;

        tracing::info!("Assembled file: {}", output_path.display());

        Ok(())
    }

    /// Helper to send progress updates
    async fn send_progress(
        &self,
        progress_tx: &Option<mpsc::Sender<HttpDownloadProgress>>,
        merkle_root: &str,
        chunks_total: usize,
        chunks_downloaded: usize,
        chunks_verified: usize,
        bytes_downloaded: u64,
        bytes_total: u64,
        status: DownloadStatus,
    ) {
        if let Some(tx) = progress_tx {
            let _ = tx
                .send(HttpDownloadProgress {
                    merkle_root: merkle_root.to_string(),
                    chunks_total,
                    chunks_downloaded,
                    chunks_verified,
                    bytes_downloaded,
                    bytes_total,
                    status,
                })
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests would go here
    // For now, these require a running HTTP server
}
