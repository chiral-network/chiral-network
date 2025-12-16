//! Hash Verification Tests for All Download Types
//! 
//! Tests hash verification functionality across all download source types:
//! - WebRTC/P2P downloads
//! - ED2K downloads
//! - HTTP downloads (already tested in multi_source_download.rs)
//! - FTP downloads (already tested in multi_source_download.rs)

use chiral_network::multi_source_download::{verify_chunk_integrity, ChunkInfo};
use sha2::{Digest, Sha256};
use hex;

#[test]
fn test_verify_chunk_integrity_webrtc_scenario() {
    // Simulate WebRTC chunk verification
    let chunk_data = b"WebRTC chunk data for testing";
    let mut hasher = Sha256::new();
    hasher.update(chunk_data);
    let expected_hash = hex::encode(hasher.finalize());

    let chunk = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: chunk_data.len(),
        hash: expected_hash.clone(),
    };

    // Test successful verification
    assert!(verify_chunk_integrity(&chunk, chunk_data).is_ok());

    // Test hash mismatch detection
    let corrupted_data = b"Corrupted WebRTC chunk data";
    let result = verify_chunk_integrity(&chunk, corrupted_data);
    assert!(result.is_err());
    
    if let Err((expected, actual)) = result {
        assert_eq!(expected, expected_hash);
        // Actual hash should be different
        let mut hasher = Sha256::new();
        hasher.update(corrupted_data);
        let actual_expected = hex::encode(hasher.finalize());
        assert_eq!(actual, actual_expected);
    }
}

#[test]
fn test_verify_chunk_integrity_ed2k_scenario() {
    // Simulate ED2K chunk verification (extracted from 9.28MB ed2k chunk)
    let chunk_data = b"ED2K extracted chunk data";
    let mut hasher = Sha256::new();
    hasher.update(chunk_data);
    let expected_hash = hex::encode(hasher.finalize());

    let chunk = ChunkInfo {
        chunk_id: 5,
        offset: 256_000 * 5, // 5th chunk at 1.28MB offset
        size: chunk_data.len(),
        hash: expected_hash.clone(),
    };

    // Test successful verification
    assert!(verify_chunk_integrity(&chunk, chunk_data).is_ok());

    // Test hash mismatch detection
    let corrupted_data = b"Corrupted ED2K chunk data";
    let result = verify_chunk_integrity(&chunk, corrupted_data);
    assert!(result.is_err());
}

#[test]
fn test_verify_chunk_integrity_graceful_degradation() {
    // Test that invalid hash formats are gracefully skipped
    let chunk_data = b"test data";
    
    // Test with non-hex hash (should skip verification)
    let chunk_invalid = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: chunk_data.len(),
        hash: "not_a_valid_hash".to_string(),
    };
    assert!(verify_chunk_integrity(&chunk_invalid, chunk_data).is_ok());

    // Test with empty hash (should skip verification)
    let chunk_empty = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: chunk_data.len(),
        hash: "".to_string(),
    };
    assert!(verify_chunk_integrity(&chunk_empty, chunk_data).is_ok());

    // Test with short hash (should skip verification)
    let chunk_short = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: chunk_data.len(),
        hash: "abc123".to_string(),
    };
    assert!(verify_chunk_integrity(&chunk_short, chunk_data).is_ok());
}

#[test]
fn test_verify_chunk_integrity_case_insensitive() {
    // Test that hash comparison is case-insensitive (normalized)
    let chunk_data = b"test data for case insensitive hash";
    let mut hasher = Sha256::new();
    hasher.update(chunk_data);
    let expected_hash = hex::encode(hasher.finalize());

    // Test with lowercase hash
    let chunk_lower = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: chunk_data.len(),
        hash: expected_hash.clone(),
    };
    assert!(verify_chunk_integrity(&chunk_lower, chunk_data).is_ok());

    // Test with uppercase hash (should be normalized)
    let chunk_upper = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: chunk_data.len(),
        hash: expected_hash.to_uppercase(),
    };
    assert!(verify_chunk_integrity(&chunk_upper, chunk_data).is_ok());
}

#[test]
fn test_verify_chunk_integrity_large_chunk() {
    // Test with large chunk (simulating real file transfer)
    let chunk_data: Vec<u8> = (0..256_000).map(|i| (i % 256) as u8).collect(); // 256KB chunk
    let mut hasher = Sha256::new();
    hasher.update(&chunk_data);
    let expected_hash = hex::encode(hasher.finalize());

    let chunk = ChunkInfo {
        chunk_id: 10,
        offset: 256_000 * 10,
        size: chunk_data.len(),
        hash: expected_hash,
    };

    // Test successful verification with large chunk
    assert!(verify_chunk_integrity(&chunk, &chunk_data).is_ok());

    // Test with corrupted large chunk
    let mut corrupted_data = chunk_data.clone();
    corrupted_data[1000] = !corrupted_data[1000]; // Flip one byte
    assert!(verify_chunk_integrity(&chunk, &corrupted_data).is_err());
}

#[test]
fn test_verify_chunk_integrity_edge_cases() {
    // Test with empty data
    let empty_data = b"";
    let mut hasher = Sha256::new();
    hasher.update(empty_data);
    let expected_hash = hex::encode(hasher.finalize());

    let chunk = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: 0,
        hash: expected_hash,
    };

    assert!(verify_chunk_integrity(&chunk, empty_data).is_ok());

    // Test with single byte
    let single_byte = b"a";
    let mut hasher = Sha256::new();
    hasher.update(single_byte);
    let expected_hash = hex::encode(hasher.finalize());

    let chunk = ChunkInfo {
        chunk_id: 0,
        offset: 0,
        size: 1,
        hash: expected_hash,
    };

    assert!(verify_chunk_integrity(&chunk, single_byte).is_ok());
}

#[test]
fn test_verify_chunk_integrity_multiple_chunks() {
    // Test verification of multiple chunks (simulating multi-chunk download)
    let chunks_data = vec![
        b"chunk 0 data".to_vec(),
        b"chunk 1 data".to_vec(),
        b"chunk 2 data".to_vec(),
    ];

    let chunks: Vec<ChunkInfo> = chunks_data
        .iter()
        .enumerate()
        .map(|(idx, data)| {
            let mut hasher = Sha256::new();
            hasher.update(data);
            let hash = hex::encode(hasher.finalize());
            ChunkInfo {
                chunk_id: idx as u32,
                offset: (idx * data.len()) as u64,
                size: data.len(),
                hash,
            }
        })
        .collect();

    // Verify all chunks succeed
    for (chunk, data) in chunks.iter().zip(chunks_data.iter()) {
        assert!(verify_chunk_integrity(chunk, data).is_ok());
    }

    // Verify that mixing chunks fails
    assert!(verify_chunk_integrity(&chunks[0], &chunks_data[1]).is_err());
    assert!(verify_chunk_integrity(&chunks[1], &chunks_data[2]).is_err());
}

