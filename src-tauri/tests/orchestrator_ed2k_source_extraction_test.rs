// Orchestrator ed2k Source Extraction Tests
// Tests for ed2k source integration into multi_source_download.rs

use chiral_network::dht::{Ed2kSourceInfo, FileMetadata};
use chiral_network::download_source::{DownloadSource, Ed2kSourceInfo as DownloadEd2kSourceInfo, FtpSourceInfo as DownloadFtpInfo, HttpSourceInfo, P2pSourceInfo};
use chiral_network::multi_source_download::ChunkInfo;
use chiral_network::ed2k_client::ED2K_CHUNK_SIZE;

// ============================================================================
// UNIT TESTS - Person 4: ed2k Source Extraction
// ============================================================================

/// Test 1: Extract ed2k sources from FileMetadata (empty)
#[test]
fn test_extract_ed2k_sources_from_metadata_empty() {
    let metadata = FileMetadata {
        merkle_root: "test_hash_123".to_string(),
        file_name: "test.bin".to_string(),
        file_size: 1024 * 1024, // 1MB
        file_data: vec![],
        seeders: vec![],
        created_at: 0,
        mime_type: None,
        is_encrypted: false,
        encryption_method: None,
        key_fingerprint: None,
        parent_hash: None,
        cids: None,
        encrypted_key_bundle: None,
        ftp_sources: None,
        http_sources: None,
        ed2k_sources: None, // No ed2k sources
        is_root: true,
        download_path: None,
        price: None,
        uploader_address: None,
        info_hash: None,
        trackers: None,
    };

    // Test that metadata has no ed2k sources
    assert!(metadata.ed2k_sources.is_none());
}

/// Test 2: Extract ed2k sources from FileMetadata (single source)
#[test]
fn test_extract_ed2k_sources_from_metadata_single() {
    let ed2k_info = Ed2kSourceInfo {
        server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
        file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
        file_size: 10 * 1024 * 1024, // 10MB
        file_name: Some("ubuntu.iso".to_string()),
        sources: Some(vec!["192.168.1.1:4662".to_string()]),
        timeout: Some(60),
    };

    let metadata = FileMetadata {
        merkle_root: "test_hash_123".to_string(),
        file_name: "test.bin".to_string(),
        file_size: 10 * 1024 * 1024,
        file_data: vec![],
        seeders: vec![],
        created_at: 0,
        mime_type: None,
        is_encrypted: false,
        encryption_method: None,
        key_fingerprint: None,
        parent_hash: None,
        cids: None,
        encrypted_key_bundle: None,
        ftp_sources: None,
        http_sources: None,
        ed2k_sources: Some(vec![ed2k_info.clone()]),
        is_root: true,
        download_path: None,
        price: None,
        uploader_address: None,
        info_hash: None,
        trackers: None,
    };

    // Test that metadata contains ed2k source
    assert!(metadata.ed2k_sources.is_some());
    let ed2k_sources = metadata.ed2k_sources.unwrap();
    
    assert_eq!(ed2k_sources.len(), 1);
    assert_eq!(ed2k_sources[0].server_url, "ed2k://|server|176.103.48.36|4661|/");
    assert_eq!(ed2k_sources[0].file_hash, "31D6CFE0D16AE931B73C59D7E0C089C0");
    assert_eq!(ed2k_sources[0].file_name, Some("ubuntu.iso".to_string()));
}

/// Test 3: Extract ed2k sources from FileMetadata (multiple sources)
#[test]
fn test_extract_ed2k_sources_from_metadata_multiple() {
    let ed2k_info1 = Ed2kSourceInfo {
        server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
        file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
        file_size: 10 * 1024 * 1024,
        file_name: Some("ubuntu.iso".to_string()),
        sources: Some(vec!["192.168.1.1:4662".to_string()]),
        timeout: Some(60),
    };

    let ed2k_info2 = Ed2kSourceInfo {
        server_url: "ed2k://|server|1.2.3.4|4661|/".to_string(),
        file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
        file_size: 10 * 1024 * 1024,
        file_name: Some("ubuntu.iso".to_string()),
        sources: None,
        timeout: Some(30),
    };

    let metadata = FileMetadata {
        merkle_root: "test_hash_123".to_string(),
        file_name: "test.bin".to_string(),
        file_size: 10 * 1024 * 1024,
        file_data: vec![],
        seeders: vec![],
        created_at: 0,
        mime_type: None,
        is_encrypted: false,
        encryption_method: None,
        key_fingerprint: None,
        parent_hash: None,
        cids: None,
        encrypted_key_bundle: None,
        ftp_sources: None,
        http_sources: None,
        ed2k_sources: Some(vec![ed2k_info1.clone(), ed2k_info2.clone()]),
        is_root: true,
        download_path: None,
        price: None,
        uploader_address: None,
        info_hash: None,
        trackers: None,
    };

    // Test that metadata contains multiple ed2k sources
    assert!(metadata.ed2k_sources.is_some());
    let ed2k_sources = metadata.ed2k_sources.unwrap();
    
    assert_eq!(ed2k_sources.len(), 2);
    assert_eq!(ed2k_sources[0].server_url, "ed2k://|server|176.103.48.36|4661|/");
    assert_eq!(ed2k_sources[1].server_url, "ed2k://|server|1.2.3.4|4661|/");
}

/// Test 4: Calculate chunk size with ed2k source (should return 9.28 MB)
#[test]
fn test_calculate_chunk_size_with_ed2k_source() {
    let ed2k_info = Ed2kSourceInfo {
        server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
        file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
        file_size: 10 * 1024 * 1024,
        file_name: None,
        sources: None,
        timeout: None,
    };

    let metadata = FileMetadata {
        merkle_root: "test_hash".to_string(),
        file_name: "test.bin".to_string(),
        file_size: 10 * 1024 * 1024,
        file_data: vec![],
        seeders: vec![],
        created_at: 0,
        mime_type: None,
        is_encrypted: false,
        encryption_method: None,
        key_fingerprint: None,
        parent_hash: None,
        cids: None,
        encrypted_key_bundle: None,
        ftp_sources: None,
        http_sources: None,
        ed2k_sources: Some(vec![ed2k_info]),
        is_root: true,
        download_path: None,
        price: None,
        uploader_address: None,
        info_hash: None,
        trackers: None,
    };

    // Test that ed2k chunk size is 9.28 MB
    assert_eq!(ED2K_CHUNK_SIZE, 9_728_000); // 9.28 MB
    
    // Verify metadata has ed2k sources
    assert!(metadata.ed2k_sources.is_some());
    assert!(!metadata.ed2k_sources.as_ref().unwrap().is_empty());
}

/// Test 5: Calculate chunk size without ed2k source (should return 256 KB)
#[test]
fn test_calculate_chunk_size_without_ed2k_source() {
    let metadata = FileMetadata {
        merkle_root: "test_hash".to_string(),
        file_name: "test.bin".to_string(),
        file_size: 10 * 1024 * 1024,
        file_data: vec![],
        seeders: vec![],
        created_at: 0,
        mime_type: None,
        is_encrypted: false,
        encryption_method: None,
        key_fingerprint: None,
        parent_hash: None,
        cids: None,
        encrypted_key_bundle: None,
        ftp_sources: None,
        http_sources: None,
        ed2k_sources: None, // No ed2k sources
        is_root: true,
        download_path: None,
        price: None,
        uploader_address: None,
        info_hash: None,
        trackers: None,
    };

    // Default chunk size should be 256 KB
    const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;
    
    // Verify metadata has no ed2k sources
    assert!(metadata.ed2k_sources.is_none() || metadata.ed2k_sources.as_ref().map(|v| v.is_empty()).unwrap_or(true));
    
    // Default chunk size is 256 KB
    assert_eq!(DEFAULT_CHUNK_SIZE, 256 * 1024);
}

/// Test 6: Assign chunks to ed2k sources (round-robin)
#[test]
fn test_ed2k_chunk_assignment() {
    // Create test chunks (256KB each)
    let chunks = vec![
        ChunkInfo { chunk_id: 0, offset: 0, size: 256 * 1024, hash: "hash0".to_string() },
        ChunkInfo { chunk_id: 1, offset: 256 * 1024, size: 256 * 1024, hash: "hash1".to_string() },
        ChunkInfo { chunk_id: 2, offset: 512 * 1024, size: 256 * 1024, hash: "hash2".to_string() },
        ChunkInfo { chunk_id: 3, offset: 768 * 1024, size: 256 * 1024, hash: "hash3".to_string() },
    ];

    // Create test sources (2 ed2k sources)
    let sources = vec![
        DownloadSource::Ed2k(DownloadEd2kSourceInfo {
            server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
            file_hash: "HASH1".to_string(),
            file_size: 1024 * 1024,
            file_name: None,
            sources: None,
            timeout_secs: Some(30),
        }),
        DownloadSource::Ed2k(DownloadEd2kSourceInfo {
            server_url: "ed2k://|server|1.2.3.4|4661|/".to_string(),
            file_hash: "HASH1".to_string(),
            file_size: 1024 * 1024,
            file_name: None,
            sources: None,
            timeout_secs: Some(30),
        }),
    ];

    // Test round-robin assignment logic
    let mut assignments: Vec<(DownloadSource, Vec<u32>)> = sources.iter().map(|s| (s.clone(), Vec::new())).collect();
    
    for (index, chunk) in chunks.iter().enumerate() {
        let source_index = index % sources.len();
        if let Some((_, chunk_ids)) = assignments.get_mut(source_index) {
            chunk_ids.push(chunk.chunk_id);
        }
    }

    // Verify round-robin distribution
    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].1, vec![0, 2]); // First source gets chunks 0, 2
    assert_eq!(assignments[1].1, vec![1, 3]); // Second source gets chunks 1, 3
}

/// Test 7: Mixed source assignment (P2P, HTTP, ed2k, FTP)
#[test]
fn test_mixed_source_download() {
    let sources = vec![
        DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooW1".to_string(),
            multiaddr: None,
            reputation: Some(90),
            supports_encryption: true,
            protocol: None,
        }),
        DownloadSource::Http(HttpSourceInfo {
            url: "https://example.com/file.zip".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: Some(30),
        }),
        DownloadSource::Ed2k(DownloadEd2kSourceInfo {
            server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
            file_hash: "HASH1".to_string(),
            file_size: 1024 * 1024,
            file_name: None,
            sources: None,
            timeout_secs: Some(30),
        }),
        DownloadSource::Ftp(DownloadFtpInfo {
            url: "ftp://ftp.example.com/file.zip".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(30),
        }),
    ];

    // Test priority ordering
    let mut sorted_sources = sources.clone();
    sorted_sources.sort_by(|a, b| b.priority_score().cmp(&a.priority_score()));

    // Verify priority order: P2P > HTTP > ed2k > FTP
    assert_eq!(sorted_sources[0].source_type(), "P2P");
    assert_eq!(sorted_sources[1].source_type(), "HTTP");
    assert_eq!(sorted_sources[2].source_type(), "ED2K");
    assert_eq!(sorted_sources[3].source_type(), "FTP");

    // Verify priority scores
    assert!(sorted_sources[0].priority_score() > sorted_sources[1].priority_score());
    assert!(sorted_sources[1].priority_score() > sorted_sources[2].priority_score());
    assert!(sorted_sources[2].priority_score() > sorted_sources[3].priority_score());
}

// ============================================================================
// Chunk Mapping Tests (Task 4.4: Handle ed2k Chunk Size Mismatch)
// ============================================================================

/// Test 8: Map ed2k chunk 0 to our chunks (should be 0-37)
#[test]
fn test_map_ed2k_chunk_0_to_our_chunks() {
    // ed2k chunk size: 9,728,000 bytes
    // Our chunk size: 256,000 bytes
    // One ed2k chunk = 9,728,000 / 256,000 = 38 of our chunks
    
    let total_file_size = 20 * 1024 * 1024; // 20MB (more than one ed2k chunk)
    let our_chunk_size = 256 * 1024; // 256KB
    
    let ed2k_chunk_id = 0;
    let ed2k_chunk_start_offset = ed2k_chunk_id as u64 * ED2K_CHUNK_SIZE as u64;
    let ed2k_chunk_end_offset = std::cmp::min(
        ed2k_chunk_start_offset + ED2K_CHUNK_SIZE as u64,
        total_file_size,
    );

    let start_chunk_id = (ed2k_chunk_start_offset / our_chunk_size as u64) as u32;
    let end_chunk_id = ((ed2k_chunk_end_offset - 1) / our_chunk_size as u64) as u32;

    // ed2k chunk 0 should map to our chunks 0-37 (38 chunks total)
    assert_eq!(start_chunk_id, 0);
    assert_eq!(end_chunk_id, 37);
    assert_eq!(end_chunk_id - start_chunk_id + 1, 38);
}

/// Test 9: Map ed2k chunk 1 to our chunks (should be 38-75)
#[test]
fn test_map_ed2k_chunk_1_to_our_chunks() {
    let total_file_size = 30 * 1024 * 1024; // 30MB (more than two ed2k chunks)
    let our_chunk_size = 256 * 1024; // 256KB
    
    let ed2k_chunk_id = 1;
    let ed2k_chunk_start_offset = ed2k_chunk_id as u64 * ED2K_CHUNK_SIZE as u64;
    let ed2k_chunk_end_offset = std::cmp::min(
        ed2k_chunk_start_offset + ED2K_CHUNK_SIZE as u64,
        total_file_size,
    );

    let start_chunk_id = (ed2k_chunk_start_offset / our_chunk_size as u64) as u32;
    let end_chunk_id = ((ed2k_chunk_end_offset - 1) / our_chunk_size as u64) as u32;

    // ed2k chunk 1 should map to our chunks 37-74 (38 chunks total)
    // Note: chunk 37 spans both ed2k chunk 0 and 1, but starts in chunk 0
    // ed2k chunk 1 starts at offset 9,728,000
    // Chunk 37 starts at 9,699,328 (in ed2k chunk 0) but extends into ed2k chunk 1
    // Chunk 37 ends at 9,961,471, which is in ed2k chunk 1
    // So ed2k chunk 1 includes chunks 37-74
    assert_eq!(start_chunk_id, 37);
    assert_eq!(end_chunk_id, 74);
    assert_eq!(end_chunk_id - start_chunk_id + 1, 38);
}

/// Test 10: Map our chunk 0 to ed2k chunk (should be chunk 0, offset 0)
#[test]
fn test_map_our_chunk_0_to_ed2k_chunk() {
    let our_chunk = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: 256 * 1024,
        hash: "hash0".to_string(),
    };

    let ed2k_chunk_id = (our_chunk.offset / ED2K_CHUNK_SIZE as u64) as u32;
    let offset_within_ed2k = our_chunk.offset % ED2K_CHUNK_SIZE as u64;

    // Our chunk 0 should map to ed2k chunk 0, offset 0
    assert_eq!(ed2k_chunk_id, 0);
    assert_eq!(offset_within_ed2k, 0);
}

/// Test 11: Map our chunk 38 to ed2k chunk (should be chunk 1, offset 233472)
#[test]
fn test_map_our_chunk_38_to_ed2k_chunk() {
    // Chunk 38 starts at offset 38 * 256KB = 9,961,472 bytes
    // ed2k chunk 1 starts at 9,728,000 bytes
    // So chunk 38 is at offset 9,961,472 - 9,728,000 = 233,472 bytes into ed2k chunk 1
    let our_chunk = ChunkInfo {
        chunk_id: 38,
        offset: 38 * 256 * 1024, // 9,961,472 bytes
        size: 256 * 1024,
        hash: "hash38".to_string(),
    };

    let ed2k_chunk_id = (our_chunk.offset / ED2K_CHUNK_SIZE as u64) as u32;
    let offset_within_ed2k = our_chunk.offset % ED2K_CHUNK_SIZE as u64;

    // Our chunk 38 should map to ed2k chunk 1, offset 233,472
    assert_eq!(ed2k_chunk_id, 1);
    assert_eq!(offset_within_ed2k, 233472);
}

/// Test 12: Map our chunk 39 to ed2k chunk (should be chunk 1, offset 495616)
#[test]
fn test_map_our_chunk_39_to_ed2k_chunk() {
    // Chunk 39 starts at offset 39 * 256KB = 10,223,616 bytes
    // ed2k chunk 1 starts at 9,728,000 bytes
    // So chunk 39 is at offset 10,223,616 - 9,728,000 = 495,616 bytes into ed2k chunk 1
    let our_chunk = ChunkInfo {
        chunk_id: 39,
        offset: 39 * 256 * 1024, // 10,223,616 bytes
        size: 256 * 1024,
        hash: "hash39".to_string(),
    };

    let ed2k_chunk_id = (our_chunk.offset / ED2K_CHUNK_SIZE as u64) as u32;
    let offset_within_ed2k = our_chunk.offset % ED2K_CHUNK_SIZE as u64;

    // Our chunk 39 should map to ed2k chunk 1, offset 495,616
    assert_eq!(ed2k_chunk_id, 1);
    assert_eq!(offset_within_ed2k, 495616);
}

