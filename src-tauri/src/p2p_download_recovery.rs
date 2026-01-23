// p2p_download_recovery.rs
// P2P Chunk-Based Download Recovery with DHT Integration
//
// This module implements content-addressed download recovery that integrates
// with the existing DHT and Bitswap infrastructure.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::{debug, info, warn};

/// Chunk size for P2P downloads (256KB, matching IPFS and manager.rs)
pub const P2P_CHUNK_SIZE: u64 = 256 * 1024;

/// State of an individual chunk during download/recovery
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChunkState {
    /// Not yet downloaded
    Pending,
    /// Downloaded but not yet verified
    Downloaded,
    /// Hash verified successfully
    Verified,
    /// Hash verification failed, needs re-download
    Failed,
}

impl Default for ChunkState {
    fn default() -> Self {
        ChunkState::Pending
    }
}

/// Metadata for a single chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Chunk index (0-based)
    pub index: u32,
    /// Byte offset in the file
    pub offset: u64,
    /// Size of this chunk in bytes
    pub size: u32,
    /// Expected SHA-256 hash of the chunk (hex encoded)
    pub expected_hash: Option<String>,
    /// Current state of this chunk
    pub state: ChunkState,
}

impl ChunkMetadata {
    pub fn new(index: u32, offset: u64, size: u32, expected_hash: Option<String>) -> Self {
        Self {
            index,
            offset,
            size,
            expected_hash,
            state: ChunkState::Pending,
        }
    }
}

/// Persistent state for a P2P download, stored as .chiral.p2p.meta.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2pDownloadState {
    /// Schema version for future compatibility
    pub version: u32,
    /// Content-addressed identifier (merkle_root from FileMetadata)
    pub merkle_root: String,
    /// Original file name
    pub file_name: String,
    /// Total file size in bytes
    pub file_size: u64,
    /// Path to the temporary download file
    pub temp_file_path: String,
    /// Path where the final file will be placed
    pub final_file_path: String,
    /// Chunk-level tracking
    pub chunks: Vec<ChunkMetadata>,
    /// Total number of verified bytes
    pub verified_bytes: u64,
    /// Peer IDs that have this content (for multi-source resume)
    pub known_peers: Vec<String>,
    /// Serialized FileManifest JSON (contains chunk hashes)
    pub manifest_json: Option<String>,
    /// Whether the file is encrypted
    pub is_encrypted: bool,
    /// Unix timestamp of last update
    pub last_updated: u64,
}

impl P2pDownloadState {
    pub const CURRENT_VERSION: u32 = 1;

    /// Create new state for a fresh P2P download
    pub fn new(
        merkle_root: String,
        file_name: String,
        file_size: u64,
        temp_file_path: String,
        final_file_path: String,
    ) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            merkle_root,
            file_name,
            file_size,
            temp_file_path,
            final_file_path,
            chunks: Vec::new(),
            verified_bytes: 0,
            known_peers: Vec::new(),
            manifest_json: None,
            is_encrypted: false,
            last_updated: now_unix(),
        }
    }

    /// Initialize chunks based on file size
    pub fn init_chunks(&mut self, chunk_hashes: Option<&[String]>) {
        let total_chunks = ((self.file_size + P2P_CHUNK_SIZE - 1) / P2P_CHUNK_SIZE) as u32;
        self.chunks.clear();

        for i in 0..total_chunks {
            let offset = i as u64 * P2P_CHUNK_SIZE;
            let remaining = self.file_size.saturating_sub(offset);
            let size = remaining.min(P2P_CHUNK_SIZE) as u32;

            let hash = chunk_hashes.and_then(|h| h.get(i as usize).cloned());

            self.chunks.push(ChunkMetadata::new(i, offset, size, hash));
        }
    }

    /// Mark a chunk as downloaded (but not yet verified)
    pub fn mark_chunk_downloaded(&mut self, index: u32) {
        if let Some(chunk) = self.chunks.get_mut(index as usize) {
            if chunk.state == ChunkState::Pending {
                chunk.state = ChunkState::Downloaded;
                self.last_updated = now_unix();
            }
        }
    }

    /// Mark a chunk as verified
    pub fn mark_chunk_verified(&mut self, index: u32) {
        if let Some(chunk) = self.chunks.get_mut(index as usize) {
            if chunk.state != ChunkState::Verified {
                chunk.state = ChunkState::Verified;
                self.verified_bytes += chunk.size as u64;
                self.last_updated = now_unix();
            }
        }
    }

    /// Mark a chunk as failed (hash mismatch)
    pub fn mark_chunk_failed(&mut self, index: u32) {
        if let Some(chunk) = self.chunks.get_mut(index as usize) {
            if chunk.state == ChunkState::Verified {
                self.verified_bytes = self.verified_bytes.saturating_sub(chunk.size as u64);
            }
            chunk.state = ChunkState::Failed;
            self.last_updated = now_unix();
        }
    }

    /// Get indices of chunks that need to be downloaded
    pub fn get_pending_chunks(&self) -> Vec<u32> {
        self.chunks
            .iter()
            .filter(|c| c.state == ChunkState::Pending || c.state == ChunkState::Failed)
            .map(|c| c.index)
            .collect()
    }

    /// Get indices of chunks that need verification
    pub fn get_unverified_chunks(&self) -> Vec<u32> {
        self.chunks
            .iter()
            .filter(|c| c.state == ChunkState::Downloaded)
            .map(|c| c.index)
            .collect()
    }

    /// Check if download is complete (all chunks verified)
    pub fn is_complete(&self) -> bool {
        !self.chunks.is_empty() && self.chunks.iter().all(|c| c.state == ChunkState::Verified)
    }

    /// Calculate download progress (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        if self.file_size == 0 {
            return 1.0;
        }
        self.verified_bytes as f32 / self.file_size as f32
    }

    /// Add a known peer
    pub fn add_peer(&mut self, peer_id: String) {
        if !self.known_peers.contains(&peer_id) {
            self.known_peers.push(peer_id);
            self.last_updated = now_unix();
        }
    }
}

/// Get the metadata file path for a download
pub fn meta_path_for(temp_file_path: &Path) -> PathBuf {
    let file_name = temp_file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    temp_file_path.with_file_name(format!(".{}.chiral.p2p.meta.json", file_name))
}

/// Persist download state atomically (write to temp, then rename)
pub async fn persist_state(state: &P2pDownloadState) -> Result<(), String> {
    let temp_path = PathBuf::from(&state.temp_file_path);
    let meta_path = meta_path_for(&temp_path);

    // Ensure parent directory exists
    if let Some(parent) = meta_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create metadata directory: {}", e))?;
    }

    // Serialize state
    let data = serde_json::to_vec_pretty(state)
        .map_err(|e| format!("Failed to serialize state: {}", e))?;

    // Write to temp file first
    let tmp_meta_path = meta_path.with_extension("tmp");
    fs::write(&tmp_meta_path, &data)
        .await
        .map_err(|e| format!("Failed to write temp metadata: {}", e))?;

    // Atomic rename
    fs::rename(&tmp_meta_path, &meta_path)
        .await
        .map_err(|e| format!("Failed to rename metadata file: {}", e))?;

    debug!(
        "Persisted P2P download state for {} ({} chunks, {:.1}% complete)",
        state.merkle_root,
        state.chunks.len(),
        state.progress() * 100.0
    );

    Ok(())
}

/// Load download state from metadata file
pub async fn load_state(meta_path: &Path) -> Result<P2pDownloadState, String> {
    let data = fs::read(meta_path)
        .await
        .map_err(|e| format!("Failed to read metadata file: {}", e))?;

    let state: P2pDownloadState =
        serde_json::from_slice(&data).map_err(|e| format!("Failed to parse metadata: {}", e))?;

    // Version check
    if state.version > P2pDownloadState::CURRENT_VERSION {
        return Err(format!(
            "Unsupported metadata version: {} (max supported: {})",
            state.version,
            P2pDownloadState::CURRENT_VERSION
        ));
    }

    Ok(state)
}

/// Remove metadata file
pub async fn remove_state(temp_file_path: &Path) {
    let meta_path = meta_path_for(temp_file_path);
    if let Err(e) = fs::remove_file(&meta_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!("Failed to remove metadata file {}: {}", meta_path.display(), e);
        }
    }
}

/// Scan a directory for incomplete P2P downloads
pub async fn scan_for_incomplete_downloads(dir: &Path) -> Vec<P2pDownloadState> {
    let mut results = Vec::new();

    let mut entries = match fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to read directory {}: {}", dir.display(), e);
            return results;
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Look for .chiral.p2p.meta.json files
        if file_name.ends_with(".chiral.p2p.meta.json") {
            match load_state(&path).await {
                Ok(state) => {
                    if !state.is_complete() {
                        info!(
                            "Found incomplete P2P download: {} ({:.1}% complete)",
                            state.merkle_root,
                            state.progress() * 100.0
                        );
                        results.push(state);
                    } else {
                        // Complete download with leftover metadata - clean up
                        debug!("Removing stale metadata for completed download: {}", state.merkle_root);
                        let _ = fs::remove_file(&path).await;
                    }
                }
                Err(e) => {
                    warn!("Failed to load metadata from {}: {}", path.display(), e);
                }
            }
        }
    }

    results
}

/// Verify a single chunk against its expected hash
pub async fn verify_chunk(
    file_path: &Path,
    chunk: &ChunkMetadata,
) -> Result<bool, String> {
    let expected_hash = match &chunk.expected_hash {
        Some(h) => h,
        None => {
            // No hash to verify against - assume valid
            return Ok(true);
        }
    };

    let mut file = tokio::fs::File::open(file_path)
        .await
        .map_err(|e| format!("Failed to open file for verification: {}", e))?;

    // Seek to chunk offset
    file.seek(std::io::SeekFrom::Start(chunk.offset))
        .await
        .map_err(|e| format!("Failed to seek to chunk {}: {}", chunk.index, e))?;

    // Read chunk data
    let mut buffer = vec![0u8; chunk.size as usize];
    let bytes_read = file
        .read_exact(&mut buffer)
        .await
        .map_err(|e| format!("Failed to read chunk {}: {}", chunk.index, e));

    // Handle EOF gracefully (file might be smaller than expected)
    if bytes_read.is_err() {
        return Ok(false);
    }

    // Compute SHA-256 hash
    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    let actual_hash = hex::encode(hasher.finalize());

    let matches = actual_hash.eq_ignore_ascii_case(expected_hash);

    if !matches {
        debug!(
            "Chunk {} hash mismatch: expected {}, got {}",
            chunk.index, expected_hash, actual_hash
        );
    }

    Ok(matches)
}

/// Verify all downloaded chunks in a state and update their status
pub async fn verify_all_chunks(state: &mut P2pDownloadState) -> Result<VerificationResult, String> {
    let temp_path = PathBuf::from(&state.temp_file_path);

    if !temp_path.exists() {
        return Ok(VerificationResult {
            total: state.chunks.len(),
            verified: 0,
            failed: 0,
            skipped: state.chunks.len(),
        });
    }

    let mut result = VerificationResult::default();
    result.total = state.chunks.len();

    // Get chunks that need verification
    let chunks_to_verify: Vec<_> = state
        .chunks
        .iter()
        .filter(|c| c.state == ChunkState::Downloaded || c.state == ChunkState::Verified)
        .cloned()
        .collect();

    for chunk in chunks_to_verify {
        if chunk.expected_hash.is_none() {
            result.skipped += 1;
            continue;
        }

        match verify_chunk(&temp_path, &chunk).await {
            Ok(true) => {
                state.mark_chunk_verified(chunk.index);
                result.verified += 1;
            }
            Ok(false) => {
                state.mark_chunk_failed(chunk.index);
                result.failed += 1;
            }
            Err(e) => {
                warn!("Verification error for chunk {}: {}", chunk.index, e);
                state.mark_chunk_failed(chunk.index);
                result.failed += 1;
            }
        }
    }

    Ok(result)
}

/// Result of chunk verification
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationResult {
    pub total: usize,
    pub verified: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Information about a recoverable download
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverableDownload {
    pub merkle_root: String,
    pub file_name: String,
    pub file_size: u64,
    pub progress: f32,
    pub verified_bytes: u64,
    pub pending_chunks: usize,
    pub known_peers: Vec<String>,
    pub temp_file_path: String,
    pub final_file_path: String,
}

impl From<&P2pDownloadState> for RecoverableDownload {
    fn from(state: &P2pDownloadState) -> Self {
        Self {
            merkle_root: state.merkle_root.clone(),
            file_name: state.file_name.clone(),
            file_size: state.file_size,
            progress: state.progress(),
            verified_bytes: state.verified_bytes,
            pending_chunks: state.get_pending_chunks().len(),
            known_peers: state.known_peers.clone(),
            temp_file_path: state.temp_file_path.clone(),
            final_file_path: state.final_file_path.clone(),
        }
    }
}

/// Get current unix timestamp
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_state_default() {
        let state: ChunkState = Default::default();
        assert_eq!(state, ChunkState::Pending);
    }

    #[test]
    fn test_p2p_download_state_init_chunks() {
        let mut state = P2pDownloadState::new(
            "abc123".to_string(),
            "test.bin".to_string(),
            1024 * 1024, // 1MB
            "/tmp/test.tmp".to_string(),
            "/downloads/test.bin".to_string(),
        );

        state.init_chunks(None);

        // 1MB / 256KB = 4 chunks
        assert_eq!(state.chunks.len(), 4);
        assert_eq!(state.chunks[0].offset, 0);
        assert_eq!(state.chunks[0].size, 256 * 1024);
        assert_eq!(state.chunks[3].offset, 3 * 256 * 1024);
    }

    #[test]
    fn test_p2p_download_state_progress() {
        let mut state = P2pDownloadState::new(
            "abc123".to_string(),
            "test.bin".to_string(),
            1024 * 1024,
            "/tmp/test.tmp".to_string(),
            "/downloads/test.bin".to_string(),
        );

        state.init_chunks(None);
        assert_eq!(state.progress(), 0.0);

        state.mark_chunk_downloaded(0);
        state.mark_chunk_verified(0);

        // 256KB / 1MB = 0.25
        assert!((state.progress() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_p2p_download_state_pending_chunks() {
        let mut state = P2pDownloadState::new(
            "abc123".to_string(),
            "test.bin".to_string(),
            512 * 1024, // 512KB = 2 chunks
            "/tmp/test.tmp".to_string(),
            "/downloads/test.bin".to_string(),
        );

        state.init_chunks(None);

        let pending = state.get_pending_chunks();
        assert_eq!(pending.len(), 2);

        state.mark_chunk_downloaded(0);
        state.mark_chunk_verified(0);

        let pending = state.get_pending_chunks();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], 1);
    }

    #[test]
    fn test_p2p_download_state_is_complete() {
        let mut state = P2pDownloadState::new(
            "abc123".to_string(),
            "test.bin".to_string(),
            256 * 1024, // Single chunk
            "/tmp/test.tmp".to_string(),
            "/downloads/test.bin".to_string(),
        );

        state.init_chunks(None);
        assert!(!state.is_complete());

        state.mark_chunk_downloaded(0);
        assert!(!state.is_complete());

        state.mark_chunk_verified(0);
        assert!(state.is_complete());
    }

    #[test]
    fn test_add_peer() {
        let mut state = P2pDownloadState::new(
            "abc123".to_string(),
            "test.bin".to_string(),
            1024,
            "/tmp/test.tmp".to_string(),
            "/downloads/test.bin".to_string(),
        );

        state.add_peer("peer1".to_string());
        state.add_peer("peer2".to_string());
        state.add_peer("peer1".to_string()); // Duplicate

        assert_eq!(state.known_peers.len(), 2);
    }

    #[test]
    fn test_meta_path_for() {
        let temp_path = PathBuf::from("/downloads/test.tmp");
        let meta_path = meta_path_for(&temp_path);
        assert_eq!(
            meta_path.to_str().unwrap(),
            "/downloads/.test.tmp.chiral.p2p.meta.json"
        );
    }
}
