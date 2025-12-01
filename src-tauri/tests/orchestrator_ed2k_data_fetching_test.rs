//! ed2k Data Fetching & Verification Tests
//!
//! This module tests the ed2k data fetching implementation, including:
//! - Downloading chunks from ed2k sources
//! - MD4 hash verification
//! - Chunk mapping between ed2k (9.28 MB) and our chunks (256 KB)
//! - Error handling and retries
//! - Progress tracking and source exchange

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

use chiral_network::download_source::{DownloadSource, Ed2kSourceInfo};
use chiral_network::ed2k_client::{Ed2kClient, Ed2kConfig, ED2K_CHUNK_SIZE};
use chiral_network::multi_source_download::{
    ChunkInfo, CompletedChunk, ActiveDownload, MultiSourceDownloadService, SourceAssignment, SourceStatus,
    MultiSourceCommand, MultiSourceEvent,
};

/// Helper function to create test ed2k source info
fn create_test_ed2k_source() -> Ed2kSourceInfo {
    Ed2kSourceInfo {
        server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
        file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
        file_size: 10485760, // 10 MB
        file_name: Some("test_file.txt".to_string()),
        sources: Some(vec!["192.168.1.100:4662".to_string()]),
        timeout_secs: Some(30),
    }
}

/// Helper function to create test chunks
fn create_test_chunks(count: u32, chunk_size: usize) -> Vec<ChunkInfo> {
    (0..count)
        .map(|i| ChunkInfo {
            chunk_id: i,
            offset: i as u64 * chunk_size as u64,
            size: chunk_size,
        })
        .collect()
}

// TODO: This test helper needs to be redesigned to work with the current MultiSourceDownloadService API.
// The current constructor requires:
//   - Arc<DhtService>
//   - Arc<WebRTCService>
//   - Arc<BitTorrentHandler>
//   - Arc<TransferEventBus> (requires Tauri AppHandle)
//
// For unit testing, consider:
//   1. Creating mock implementations of the services
//   2. Using a trait-based approach for dependency injection
//   3. Testing the internal helper methods directly
//
// For now, these tests are skipped because the mock service creation needs redesign.

/// Placeholder for mock download service - needs redesign for current API
async fn create_mock_download_service() -> ! {
    panic!(
        "create_mock_download_service() needs to be redesigned for the current MultiSourceDownloadService API. \
         The constructor now requires Arc<DhtService>, Arc<WebRTCService>, Arc<BitTorrentHandler>, and \
         Arc<TransferEventBus> (which requires a Tauri AppHandle)."
    )
}

#[tokio::test]
async fn test_group_chunks_by_ed2k_chunk_single_ed2k_chunk() {
    let service = create_mock_download_service().await;
    
    // Create chunks that all belong to ed2k chunk 0 (0-37)
    let our_chunks = create_test_chunks(38, 256_000); // 38 chunks of 256KB = 9.728 MB
    
    let grouped = service.group_chunks_by_ed2k_chunk(&our_chunks);
    
    assert_eq!(grouped.len(), 1);
    assert!(grouped.contains_key(&0));
    assert_eq!(grouped[&0].len(), 38);
}

#[tokio::test]
async fn test_group_chunks_by_ed2k_chunk_multiple_ed2k_chunks() {
    let service = create_mock_download_service().await;
    
    // Create chunks spanning 2 ed2k chunks (0-75 = chunks 0-37 in ed2k chunk 0, 38-75 in ed2k chunk 1)
    let our_chunks = create_test_chunks(76, 256_000);
    
    let grouped = service.group_chunks_by_ed2k_chunk(&our_chunks);
    
    assert_eq!(grouped.len(), 2);
    assert!(grouped.contains_key(&0));
    assert!(grouped.contains_key(&1));
    assert_eq!(grouped[&0].len(), 38);
    assert_eq!(grouped[&1].len(), 38);
}

#[tokio::test]
async fn test_group_chunks_by_ed2k_chunk_mixed_chunks() {
    let service = create_mock_download_service().await;
    
    // Create chunks from different ed2k chunks (0-5, 38-40)
    let mut our_chunks = create_test_chunks(6, 256_000); // Chunks 0-5 (ed2k chunk 0)
    let mut additional_chunks = create_test_chunks(3, 256_000); // Chunks 0-2
    
    // Adjust chunk IDs and offsets for chunks 38-40
    for (i, chunk) in additional_chunks.iter_mut().enumerate() {
        chunk.chunk_id = 38 + i as u32;
        chunk.offset = (38 + i) as u64 * 256_000;
    }
    
    our_chunks.extend(additional_chunks);
    
    let grouped = service.group_chunks_by_ed2k_chunk(&our_chunks);
    
    assert_eq!(grouped.len(), 2);
    assert!(grouped.contains_key(&0));
    assert!(grouped.contains_key(&1));
    assert_eq!(grouped[&0].len(), 6); // Chunks 0-5
    assert_eq!(grouped[&1].len(), 3); // Chunks 38-40 (mapped to ed2k chunk 1)
}

#[tokio::test]
async fn test_verify_ed2k_chunk_hash_success() {
    let service = create_mock_download_service().await;
    
    let test_data = b"Test ed2k chunk data for MD4 verification";
    
    // Calculate expected hash
    use md4::{Digest, Md4};
    let mut hasher = Md4::new();
    hasher.update(test_data);
    let expected_hash = hex::encode(hasher.finalize());
    
    let result = service.verify_ed2k_chunk_hash(test_data, &expected_hash).await;
    
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_verify_ed2k_chunk_hash_failure() {
    let service = create_mock_download_service().await;
    
    let test_data = b"Test ed2k chunk data for MD4 verification";
    let wrong_hash = "0123456789ABCDEF0123456789ABCDEF";
    
    let result = service.verify_ed2k_chunk_hash(test_data, wrong_hash).await;
    
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_get_ed2k_chunk_hash() {
    let service = create_mock_download_service().await;
    
    let file_hash = "31D6CFE0D16AE931B73C59D7E0C089C0";
    let ed2k_chunk_id = 0;
    
    let result = service.get_ed2k_chunk_hash(file_hash, ed2k_chunk_id).await;
    
    assert!(result.is_ok());
    let hash = result.unwrap();
    assert_eq!(hash.len(), 32); // MD4 hash is 32 hex characters
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn test_verify_chunk_hash() {
    let service = create_mock_download_service().await;
    
    let test_data = b"Test chunk data";
    let file_hash = "31D6CFE0D16AE931B73C59D7E0C089C0";
    let chunk_id = 42;
    
    let result = service.verify_chunk_hash(test_data, file_hash, chunk_id).await;
    
    assert!(result.is_ok());
    // Currently returns true always - would verify against stored hashes in real implementation
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_is_last_chunk() {
    let service = create_mock_download_service().await;
    
    let file_hash = "test_file_hash";
    let chunks = create_test_chunks(10, 256_000);
    
    // Create download with chunks
    let download = Download {
        file_hash: file_hash.to_string(),
        chunks: chunks.clone(),
        total_size: 10 * 256_000,
        completed_chunks: HashMap::new(),
        failed_chunks: std::collections::VecDeque::new(),
        source_assignments: HashMap::new(),
        started_at: Instant::now(),
        last_activity: Instant::now(),
    };
    
    // Insert into active downloads
    service.active_downloads.write().await.insert(file_hash.to_string(), download);
    
    // Test last chunk
    let is_last = service.is_last_chunk(&service.active_downloads, file_hash, 9).await;
    assert!(is_last);
    
    // Test non-last chunk
    let is_not_last = service.is_last_chunk(&service.active_downloads, file_hash, 5).await;
    assert!(!is_not_last);
}

#[tokio::test]
async fn test_split_and_store_ed2k_chunk_single_chunk() {
    let service = create_mock_download_service().await;
    
    let file_hash = "test_file_hash";
    let server_url = "ed2k://|server|176.103.48.36|4661|/";
    let ed2k_chunk_id = 0;
    
    // Create test data for ed2k chunk (9.28 MB)
    let ed2k_chunk_data = vec![0xAB; ED2K_CHUNK_SIZE];
    
    // Create one chunk that should be extracted from this ed2k chunk
    let our_chunk = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: 256_000,
    };
    
    // Create download
    let download = Download {
        file_hash: file_hash.to_string(),
        chunks: vec![our_chunk.clone()],
        total_size: 256_000,
        completed_chunks: HashMap::new(),
        failed_chunks: std::collections::VecDeque::new(),
        source_assignments: HashMap::new(),
        started_at: Instant::now(),
        last_activity: Instant::now(),
    };
    
    service.active_downloads.write().await.insert(file_hash.to_string(), download);
    
    // Split and store
    service.split_and_store_ed2k_chunk(
        &service.active_downloads,
        file_hash,
        server_url,
        ed2k_chunk_id,
        &ed2k_chunk_data,
        &[our_chunk],
    ).await;
    
    // Verify chunk was stored
    let downloads = service.active_downloads.read().await;
    let download = &downloads[file_hash];
    assert!(download.completed_chunks.contains_key(&0));
    
    let completed = &download.completed_chunks[&0];
    assert_eq!(completed.chunk_id, 0);
    assert_eq!(completed.data.len(), 256_000);
    assert_eq!(completed.source_id, server_url);
}

#[tokio::test]
async fn test_split_and_store_ed2k_chunk_multiple_chunks() {
    let service = create_mock_download_service().await;
    
    let file_hash = "test_file_hash";
    let server_url = "ed2k://|server|176.103.48.36|4661|/";
    let ed2k_chunk_id = 0;
    
    // Create test data for ed2k chunk
    let ed2k_chunk_data = vec![0xCD; ED2K_CHUNK_SIZE];
    
    // Create multiple chunks from same ed2k chunk (0, 1, 2)
    let our_chunks = create_test_chunks(3, 256_000);
    
    // Create download
    let download = Download {
        file_hash: file_hash.to_string(),
        chunks: our_chunks.clone(),
        total_size: 3 * 256_000,
        completed_chunks: HashMap::new(),
        failed_chunks: std::collections::VecDeque::new(),
        source_assignments: HashMap::new(),
        started_at: Instant::now(),
        last_activity: Instant::now(),
    };
    
    service.active_downloads.write().await.insert(file_hash.to_string(), download);
    
    // Split and store
    service.split_and_store_ed2k_chunk(
        &service.active_downloads,
        file_hash,
        server_url,
        ed2k_chunk_id,
        &ed2k_chunk_data,
        &our_chunks,
    ).await;
    
    // Verify all chunks were stored
    let downloads = service.active_downloads.read().await;
    let download = &downloads[file_hash];
    
    for i in 0..3 {
        assert!(download.completed_chunks.contains_key(&i));
        let completed = &download.completed_chunks[&i];
        assert_eq!(completed.chunk_id, i);
        assert_eq!(completed.data.len(), 256_000);
    }
}

#[tokio::test]
async fn test_split_and_store_ed2k_chunk_wrong_ed2k_chunk() {
    let service = create_mock_download_service().await;
    
    let file_hash = "test_file_hash";
    let server_url = "ed2k://|server|176.103.48.36|4661|/";
    let ed2k_chunk_id = 0;
    
    let ed2k_chunk_data = vec![0xEF; ED2K_CHUNK_SIZE];
    
    // Create chunk that belongs to ed2k chunk 1, not 0
    let our_chunk = ChunkInfo {
        chunk_id: 38, // This maps to ed2k chunk 1
        offset: 38 * 256_000,
        size: 256_000,
    };
    
    let download = Download {
        file_hash: file_hash.to_string(),
        chunks: vec![our_chunk.clone()],
        total_size: 256_000,
        completed_chunks: HashMap::new(),
        failed_chunks: std::collections::VecDeque::new(),
        source_assignments: HashMap::new(),
        started_at: Instant::now(),
        last_activity: Instant::now(),
    };
    
    service.active_downloads.write().await.insert(file_hash.to_string(), download);
    
    // Split and store - should skip chunk 38 since it doesn't belong to ed2k chunk 0
    service.split_and_store_ed2k_chunk(
        &service.active_downloads,
        file_hash,
        server_url,
        ed2k_chunk_id,
        &ed2k_chunk_data,
        &[our_chunk],
    ).await;
    
    // Verify chunk was NOT stored
    let downloads = service.active_downloads.read().await;
    let download = &downloads[file_hash];
    assert!(!download.completed_chunks.contains_key(&38));
}

// Integration tests require more complex setup and would test:
// - Complete ed2k download workflow
// - Multi-source downloads with ed2k + P2P + HTTP + FTP
// - Error handling and retries
// - Connection pooling
// - Source exchange

#[tokio::test]
async fn test_ed2k_chunk_size_constants() {
    // Verify ed2k chunk size constant
    assert_eq!(ED2K_CHUNK_SIZE, 9_728_000); // 9.28 MB
    
    // Verify that our default chunk size (256KB) divides evenly into ed2k chunk size
    const OUR_CHUNK_SIZE: usize = 256_000;
    assert_eq!(ED2K_CHUNK_SIZE % OUR_CHUNK_SIZE, 0);
    assert_eq!(ED2K_CHUNK_SIZE / OUR_CHUNK_SIZE, 38); // 38 of our chunks per ed2k chunk
}

#[tokio::test]
async fn test_chunk_mapping_ed2k_chunk_0() {
    let service = create_mock_download_service().await;
    
    // Test mapping for chunks in ed2k chunk 0 (chunks 0-37)
    for i in 0..38 {
        let chunk = ChunkInfo {
            chunk_id: i,
            offset: i as u64 * 256_000,
            size: 256_000,
        };
        
        let (ed2k_chunk_id, offset_within_ed2k) = service.map_our_chunk_to_ed2k_chunk(&chunk);
        
        assert_eq!(ed2k_chunk_id, 0);
        assert_eq!(offset_within_ed2k, i as u64 * 256_000);
    }
}

#[tokio::test]
async fn test_chunk_mapping_ed2k_chunk_1() {
    let service = create_mock_download_service().await;
    
    // Test mapping for chunks in ed2k chunk 1 (chunks 38-75)
    for i in 38..76 {
        let chunk = ChunkInfo {
            chunk_id: i,
            offset: i as u64 * 256_000,
            size: 256_000,
        };
        
        let (ed2k_chunk_id, offset_within_ed2k) = service.map_our_chunk_to_ed2k_chunk(&chunk);
        
        assert_eq!(ed2k_chunk_id, 1);
        assert_eq!(offset_within_ed2k, (i - 38) as u64 * 256_000);
    }
}
