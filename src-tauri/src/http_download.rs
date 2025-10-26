use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Semaphore};

/// HTTP Download Client for fetching files via HTTP Range requests
///
/// Simplified Architecture (Range-based, no manifest):
/// 1. Fetch file metadata from seeder's HTTP endpoint
/// 2. Calculate byte ranges (e.g., 0-256KB, 256KB-512KB, etc.)
/// 3. Download chunks in parallel using Range headers
/// 4. Reassemble chunks into final file
///
/// This approach uses HTTP Range requests (RFC 7233) to download file chunks
/// dynamically, without requiring pre-chunking or manifest endpoints.
///
/// Example Range request:
/// GET /files/{hash}
/// Range: bytes=0-262143
///
/// Server responds with:
/// HTTP/1.1 206 Partial Content
/// Content-Range: bytes 0-262143/1048576
/// Content-Length: 262144

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpDownloadProgress {
    pub file_hash: String,
    pub chunks_total: usize,
    pub chunks_downloaded: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    FetchingMetadata,
    Downloading,
    Assembling,
    Completed,
    Failed,
}

/// File metadata from HTTP server (matches HttpFileMetadata in http_server.rs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpFileMetadata {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub encrypted: bool,
}

/// Represents a byte range to download
#[derive(Debug, Clone)]
struct ByteRange {
    start: u64,
    end: u64,
    index: usize,
}

/// Chunk size for Range requests (256 KB)
const CHUNK_SIZE: u64 = 256 * 1024;

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
    /// Semaphore to limit concurrent chunk downloads
    download_semaphore: std::sync::Arc<Semaphore>,
}

impl HttpDownloadClient {
    pub fn new() -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            download_semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
        }
    }

    /// Create with custom timeout
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .expect("Failed to create HTTP client"),
            download_semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS)),
        }
    }

    /// Download a file from an HTTP seeder using Range requests
    ///
    /// Arguments:
    /// - `seeder_url`: Base URL of the seeder (e.g., "http://192.168.1.5:8080")
    /// - `file_hash`: File identifier (merkle_root)
    /// - `output_path`: Where to save the final file
    /// - `progress_tx`: Optional channel for progress updates
    ///
    /// Returns: Ok(()) on success
    ///
    /// This method:
    /// 1. Fetches file metadata to get size
    /// 2. Calculates byte ranges (256KB chunks)
    /// 3. Downloads chunks in parallel using Range headers
    /// 4. Reassembles chunks into final file
    ///
    /// Note: Files are downloaded as-is (encrypted if they were encrypted).
    /// Decryption happens at a higher level when needed.
    pub async fn download_file(
        &self,
        seeder_url: &str,
        file_hash: &str,
        output_path: &Path,
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
    ) -> Result<(), String> {
        tracing::info!(
            "Starting HTTP Range-based download: {} from {}",
            file_hash,
            seeder_url
        );

        // Step 1: Fetch file metadata
        self.send_progress(
            &progress_tx,
            file_hash,
            0,
            0,
            0,
            0,
            DownloadStatus::FetchingMetadata,
        )
        .await;

        let metadata = self.fetch_metadata(seeder_url, file_hash).await?;

        tracing::info!(
            "Fetched metadata: {} ({} bytes, encrypted: {})",
            metadata.name,
            metadata.size,
            metadata.encrypted
        );

        // Step 2: Calculate byte ranges
        let ranges = self.calculate_ranges(metadata.size);
        let total_chunks = ranges.len();

        tracing::info!(
            "Calculated {} chunks of {} bytes each",
            total_chunks,
            CHUNK_SIZE
        );

        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            0,
            0,
            metadata.size,
            DownloadStatus::Downloading,
        )
        .await;

        // Step 3: Download all chunks using Range requests
        let chunks = self
            .download_chunks_with_ranges(
                seeder_url,
                file_hash,
                &ranges,
                progress_tx.clone(),
                metadata.size,
            )
            .await?;

        tracing::info!("Downloaded {} chunks", chunks.len());

        // Step 4: Assemble final file
        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            total_chunks,
            metadata.size,
            metadata.size,
            DownloadStatus::Assembling,
        )
        .await;

        self.assemble_file(&chunks, output_path).await?;

        // Final status
        self.send_progress(
            &progress_tx,
            file_hash,
            total_chunks,
            total_chunks,
            metadata.size,
            metadata.size,
            DownloadStatus::Completed,
        )
        .await;

        tracing::info!(
            "Download completed: {} ({})",
            output_path.display(),
            metadata.name
        );

        Ok(())
    }

    /// Fetch file metadata from HTTP seeder
    ///
    /// Calls: GET /files/{file_hash}/metadata
    async fn fetch_metadata(
        &self,
        seeder_url: &str,
        file_hash: &str,
    ) -> Result<HttpFileMetadata, String> {
        let url = format!("{}/files/{}/metadata", seeder_url, file_hash);

        tracing::debug!("Fetching metadata from: {}", url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch metadata: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Metadata request failed: {} ({})",
                response.status(),
                url
            ));
        }

        let metadata: HttpFileMetadata = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse metadata: {}", e))?;

        Ok(metadata)
    }

    /// Calculate byte ranges for chunked download
    ///
    /// Splits file into CHUNK_SIZE (256KB) ranges
    /// Example for 1MB file:
    /// - Range 0: bytes 0-262143 (256KB)
    /// - Range 1: bytes 262144-524287 (256KB)
    /// - Range 2: bytes 524288-786431 (256KB)
    /// - Range 3: bytes 786432-1048575 (262KB, last chunk may be smaller)
    fn calculate_ranges(&self, file_size: u64) -> Vec<ByteRange> {
        let mut ranges = Vec::new();
        let mut offset = 0u64;
        let mut index = 0;

        while offset < file_size {
            let end = std::cmp::min(offset + CHUNK_SIZE - 1, file_size - 1);
            ranges.push(ByteRange {
                start: offset,
                end,
                index,
            });
            offset = end + 1;
            index += 1;
        }

        ranges
    }

    /// Download all chunks using Range requests with bounded parallelism
    ///
    /// Uses a semaphore to limit concurrent downloads to MAX_CONCURRENT_CHUNKS,
    /// preventing network/server overload while still achieving good parallelism.
    ///
    /// Flow:
    /// 1. Acquire semaphore permit (blocks if MAX_CONCURRENT_CHUNKS already downloading)
    /// 2. Send GET request with Range header
    /// 3. Verify 206 Partial Content response
    /// 4. Download chunk data
    /// 5. Release permit (allows next chunk to start)
    /// 6. Repeat until all chunks downloaded
    async fn download_chunks_with_ranges(
        &self,
        seeder_url: &str,
        file_hash: &str,
        ranges: &[ByteRange],
        progress_tx: Option<mpsc::Sender<HttpDownloadProgress>>,
        file_size: u64,
    ) -> Result<Vec<Vec<u8>>, String> {
        let mut tasks = Vec::new();

        for range in ranges {
            let client = self.http_client.clone();
            let url = format!("{}/files/{}", seeder_url, file_hash);
            let start = range.start;
            let end = range.end;
            let index = range.index;
            let progress_tx = progress_tx.clone();
            let file_hash = file_hash.to_string();
            let total_chunks = ranges.len();
            let semaphore = self.download_semaphore.clone();

            // Spawn task for each chunk (but semaphore limits concurrency)
            let task = tokio::spawn(async move {
                // Acquire permit (waits if MAX_CONCURRENT_CHUNKS already downloading)
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| format!("Failed to acquire semaphore: {}", e))?;

                tracing::debug!(
                    "Downloading chunk {} (bytes {}-{}) from {}",
                    index,
                    start,
                    end,
                    url
                );

                // Make request with Range header
                let response = client
                    .get(&url)
                    .header("Range", format!("bytes={}-{}", start, end))
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download chunk {}: {}", index, e))?;

                // Verify 206 Partial Content response
                if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    return Err(format!(
                        "Chunk {} request failed: expected 206 Partial Content, got {}",
                        index,
                        response.status()
                    ));
                }

                // Read chunk data
                let data = response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read chunk {} data: {}", index, e))?
                    .to_vec();

                let expected_size = (end - start + 1) as usize;
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
                            file_hash: file_hash.clone(),
                            chunks_total: total_chunks,
                            chunks_downloaded: index + 1,
                            bytes_downloaded: data.len() as u64,
                            bytes_total: file_size,
                            status: DownloadStatus::Downloading,
                        })
                        .await;
                }

                Ok::<(usize, Vec<u8>), String>((index, data))
                // Permit automatically released when _permit is dropped
            });

            tasks.push(task);
        }

        tracing::info!(
            "Downloading {} chunks with max {} concurrent downloads",
            ranges.len(),
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

        // Extract just the data (drop indices)
        let chunks: Vec<Vec<u8>> = results.into_iter().map(|(_, data)| data).collect();

        Ok(chunks)
    }

    /// Assemble chunks into final file
    ///
    /// Writes chunks sequentially to the output file
    async fn assemble_file(
        &self,
        chunks: &[Vec<u8>],
        output_path: &Path,
    ) -> Result<(), String> {
        let mut file = File::create(output_path)
            .await
            .map_err(|e| format!("Failed to create output file: {}", e))?;

        for (index, chunk) in chunks.iter().enumerate() {
            file.write_all(chunk)
                .await
                .map_err(|e| format!("Failed to write chunk {}: {}", index, e))?;
        }

        file.flush()
            .await
            .map_err(|e| format!("Failed to flush file: {}", e))?;

        tracing::info!("Assembled file: {} ({} chunks)", output_path.display(), chunks.len());

        Ok(())
    }

    /// Helper to send progress updates
    async fn send_progress(
        &self,
        progress_tx: &Option<mpsc::Sender<HttpDownloadProgress>>,
        file_hash: &str,
        chunks_total: usize,
        chunks_downloaded: usize,
        bytes_downloaded: u64,
        bytes_total: u64,
        status: DownloadStatus,
    ) {
        if let Some(tx) = progress_tx {
            let _ = tx
                .send(HttpDownloadProgress {
                    file_hash: file_hash.to_string(),
                    chunks_total,
                    chunks_downloaded,
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
