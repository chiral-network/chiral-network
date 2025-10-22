use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Semaphore};
use x25519_dalek::StaticSecret;

use crate::encryption::{decrypt_aes_key, EncryptedAesKeyBundle};
use crate::manager::ChunkManager;

/// HTTP Download Client for fetching files via HTTP protocol
///
/// Flow:
/// 1. Fetch manifest from seeder's HTTP endpoint
/// 2. Download encrypted chunks in parallel
/// 3. Get decryption key from manifest
/// 4. Decrypt chunks
/// 5. Verify chunk hashes
/// 6. Assemble final file

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
    Decrypting,
    Verifying,
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
    /// - `output_path`: Where to save the final decrypted file
    /// - `recipient_secret_key`: User's private key for decrypting the AES key
    /// - `progress_tx`: Optional channel for progress updates
    ///
    /// Returns: Ok(()) on success
    pub async fn download_file(
        &self,
        seeder_url: &str,
        merkle_root: &str,
        output_path: &Path,
        recipient_secret_key: &StaticSecret,
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

        // Step 2: Download all encrypted chunks
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

        let encrypted_chunks = self
            .download_chunks(
                seeder_url,
                &manifest.chunks,
                merkle_root,
                progress_tx.clone(),
            )
            .await?;

        tracing::info!("Downloaded {} chunks", encrypted_chunks.len());

        // Step 3: Decrypt AES key from bundle
        self.send_progress(
            &progress_tx,
            merkle_root,
            manifest.chunks.len(),
            manifest.chunks.len(),
            0,
            manifest.total_size as u64,
            manifest.total_size as u64,
            DownloadStatus::Decrypting,
        )
        .await;

        let encrypted_key_bundle_json = manifest
            .encrypted_key_bundle
            .ok_or("No encryption key in manifest")?;

        let encrypted_key_bundle: EncryptedAesKeyBundle =
            serde_json::from_str(&encrypted_key_bundle_json)
                .map_err(|e| format!("Failed to parse key bundle: {}", e))?;

        let aes_key = decrypt_aes_key(&encrypted_key_bundle, recipient_secret_key)?;

        tracing::info!("Decrypted AES key");

        // Step 4: Decrypt and verify chunks
        self.send_progress(
            &progress_tx,
            merkle_root,
            manifest.chunks.len(),
            manifest.chunks.len(),
            0,
            manifest.total_size as u64,
            manifest.total_size as u64,
            DownloadStatus::Verifying,
        )
        .await;

        let decrypted_chunks = self
            .decrypt_and_verify_chunks(
                &encrypted_chunks,
                &aes_key,
                merkle_root,
                progress_tx.clone(),
            )
            .await?;

        tracing::info!("Decrypted and verified {} chunks", decrypted_chunks.len());

        // Step 5: Assemble final file
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

        self.assemble_file(&decrypted_chunks, output_path).await?;

        // Step 6: Final verification (compute merkle root from decrypted chunks)
        // TODO: Implement merkle tree verification here

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

    /// Decrypt and verify chunks
    async fn decrypt_and_verify_chunks(
        &self,
        encrypted_chunks: &[(u32, Vec<u8>)],
        aes_key: &[u8; 32],
        merkle_root: &str,
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
    ) -> Result<Vec<Vec<u8>>, String> {
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Key, Nonce};

        let key = Key::<Aes256Gcm>::from_slice(aes_key);
        let cipher = Aes256Gcm::new(key);

        let mut decrypted_chunks = Vec::new();

        for (index, encrypted_data) in encrypted_chunks {
            // Encrypted data format: [12-byte nonce][ciphertext]
            if encrypted_data.len() < 12 {
                return Err(format!("Chunk {} is too small to contain nonce", index));
            }

            let nonce_bytes = &encrypted_data[..12];
            let ciphertext = &encrypted_data[12..];

            let nonce = Nonce::from_slice(nonce_bytes);

            let decrypted = cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| format!("Failed to decrypt chunk {}: {}", index, e))?;

            tracing::debug!("Decrypted chunk {} ({} bytes)", index, decrypted.len());

            decrypted_chunks.push(decrypted);

            // Send progress update
            if let Some(tx) = &progress_tx {
                let _ = tx
                    .send(HttpDownloadProgress {
                        merkle_root: merkle_root.to_string(),
                        chunks_total: encrypted_chunks.len(),
                        chunks_downloaded: encrypted_chunks.len(),
                        chunks_verified: decrypted_chunks.len(),
                        bytes_downloaded: 0,
                        bytes_total: 0,
                        status: DownloadStatus::Verifying,
                    })
                    .await;
            }
        }

        // TODO: Verify merkle tree here
        // For now, we trust that decryption success means data is valid

        Ok(decrypted_chunks)
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
