//! Integration tests for FileManifest generation and DHT storage
//! 
//! Tests the complete flow:
//! 1. File upload with manifest generation
//! 2. Manifest storage in FileMetadata
//! 3. DHT publish and retrieve with manifest
//! 4. Chunk hash extraction for download verification

use chiral_network::dht::models::FileMetadata;
use chiral_network::manager::{ChunkManager, FileManifest, ChunkInfo};
use std::path::Path;
use tempfile::TempDir;
use tokio;

/// Test that ChunkManager generates a valid FileManifest
#[tokio::test]
async fn test_chunk_manager_generates_manifest() {
    let temp_dir = TempDir::new().unwrap();
    let chunk_dir = temp_dir.path().join("chunks");
    std::fs::create_dir_all(&chunk_dir).unwrap();

    // Create a test file
    let test_file = temp_dir.path().join("test.txt");
    let test_data = b"Hello, this is test data for chunking and hashing!".repeat(1000);
    std::fs::write(&test_file, &test_data).unwrap();

    // Create ChunkManager and generate manifest
    let chunk_manager = ChunkManager::new(chunk_dir);
    let result = tokio::task::spawn_blocking({
        let test_file_clone = test_file.clone();
        move || chunk_manager.chunk_and_encrypt_file_canonical(Path::new(&test_file_clone))
    })
    .await
    .unwrap();

    assert!(result.is_ok());
    let encryption_result = result.unwrap();
    let manifest = encryption_result.manifest;

    // Verify manifest structure
    assert!(!manifest.merkle_root.is_empty());
    assert!(!manifest.chunks.is_empty());

    // Verify each chunk has a hash
    for chunk in &manifest.chunks {
        assert!(!chunk.hash.is_empty());
        assert_eq!(chunk.hash.len(), 64); // SHA-256 hex = 64 chars
        assert!(chunk.hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(chunk.size > 0);
    }

    println!("[OK] ChunkManager generated manifest with {} chunks", manifest.chunks.len());
}

/// Test FileMetadata serialization with manifest
#[test]
fn test_file_metadata_with_manifest_serialization() {
    let manifest = FileManifest {
        merkle_root: "test_merkle_root_123".to_string(),
        chunks: vec![
            ChunkInfo {
                index: 0,
                hash: "a".repeat(64),
                size: 1024,
                encrypted_hash: String::new(),
                encrypted_size: 1024,
            },
            ChunkInfo {
                index: 1,
                hash: "b".repeat(64),
                size: 1024,
                encrypted_hash: String::new(),
                encrypted_size: 1024,
            },
        ],
        encrypted_key_bundle: None,
    };

    let manifest_json = serde_json::to_string(&manifest).unwrap();

    let metadata = FileMetadata {
        merkle_root: "test_file_hash".to_string(),
        file_name: "test.txt".to_string(),
        file_size: 2048,
        file_data: vec![],
        seeders: vec!["peer1".to_string()],
        created_at: 1234567890,
        mime_type: Some("text/plain".to_string()),
        is_encrypted: false,
        encryption_method: None,
        key_fingerprint: None,
        parent_hash: None,
        cids: None,
        encrypted_key_bundle: None,
        ftp_sources: None,
        ed2k_sources: None,
        http_sources: None,
        is_root: true,
        download_path: None,
        price: 0.0,
        uploader_address: Some("0x123".to_string()),
        info_hash: None,
        trackers: None,
        manifest: Some(manifest_json.clone()),
    };

    // Serialize to JSON (simulating DHT storage)
    let metadata_json = serde_json::to_string(&metadata).unwrap();
    // Note: manifest_json will be escaped when serialized inside metadata_json
    assert!(metadata_json.contains("test_merkle_root_123"));

    // Deserialize (simulating DHT retrieval)
    let deserialized: FileMetadata = serde_json::from_str(&metadata_json).unwrap();
    assert!(deserialized.manifest.is_some());

    // Verify manifest can be extracted
    let extracted_manifest: FileManifest = 
        serde_json::from_str(&deserialized.manifest.unwrap()).unwrap();
    assert_eq!(extracted_manifest.chunks.len(), 2);
    assert_eq!(extracted_manifest.merkle_root, "test_merkle_root_123");

    println!("[OK] FileMetadata with manifest can be serialized and deserialized");
}

/// Test chunk hash extraction workflow
#[test]
fn test_chunk_hash_extraction_workflow() {
    // Simulate upload: create manifest
    let upload_manifest = FileManifest {
        merkle_root: "root_hash".to_string(),
        chunks: vec![
            ChunkInfo {
                index: 0,
                hash: "chunk0_hash_".to_string() + &"a".repeat(52),
                size: 256 * 1024,
                encrypted_hash: String::new(),
                encrypted_size: 256 * 1024,
            },
            ChunkInfo {
                index: 1,
                hash: "chunk1_hash_".to_string() + &"b".repeat(52),
                size: 256 * 1024,
                encrypted_hash: String::new(),
                encrypted_size: 256 * 1024,
            },
            ChunkInfo {
                index: 2,
                hash: "chunk2_hash_".to_string() + &"c".repeat(52),
                size: 128 * 1024,
                encrypted_hash: String::new(),
                encrypted_size: 128 * 1024,
            },
        ],
        encrypted_key_bundle: None,
    };

    // Store in metadata (upload to DHT)
    let manifest_json = serde_json::to_string(&upload_manifest).unwrap();
    let metadata = FileMetadata {
        merkle_root: "file_hash".to_string(),
        file_name: "large_file.bin".to_string(),
        file_size: 640 * 1024,
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
        ed2k_sources: None,
        http_sources: None,
        is_root: false,
        download_path: None,
        price: 0.0,
        uploader_address: None,
        info_hash: None,
        trackers: None,
        manifest: Some(manifest_json),
    };

    // Simulate download: extract manifest
    assert!(metadata.manifest.is_some());
    let download_manifest: FileManifest = 
        serde_json::from_str(&metadata.manifest.unwrap()).unwrap();

    // Extract chunk hashes for verification
    let mut chunk_hashes = Vec::new();
    for chunk in &download_manifest.chunks {
        while chunk_hashes.len() <= chunk.index as usize {
            chunk_hashes.push(String::new());
        }
        chunk_hashes[chunk.index as usize] = chunk.hash.clone();
    }

    // Verify all chunks have hashes
    assert_eq!(chunk_hashes.len(), 3);
    assert!(chunk_hashes[0].starts_with("chunk0_hash_"));
    assert!(chunk_hashes[1].starts_with("chunk1_hash_"));
    assert!(chunk_hashes[2].starts_with("chunk2_hash_"));

    println!("[OK] Chunk hash extraction workflow successful");
    println!("   Extracted {} chunk hashes from manifest", chunk_hashes.len());
}

/// Test backward compatibility: metadata without manifest
#[test]
fn test_backward_compatibility_download() {
    // Old-style metadata without manifest field
    let metadata = FileMetadata {
        merkle_root: "old_file_hash".to_string(),
        file_name: "old_file.txt".to_string(),
        file_size: 1024,
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
        ed2k_sources: None,
        http_sources: None,
        is_root: false,
        download_path: None,
        price: 0.0,
        uploader_address: None,
        info_hash: None,
        trackers: None,
        manifest: None, // No manifest - old metadata
    };

    // Should handle gracefully
    assert!(metadata.manifest.is_none());

    // Simulate download code handling None manifest
    if let Some(manifest_json) = &metadata.manifest {
        let _manifest: FileManifest = serde_json::from_str(manifest_json).unwrap();
        panic!("Should not have manifest");
    } else {
        // Use fallback: generate placeholder hashes
        let total_chunks = ((metadata.file_size as f64) / (256.0 * 1024.0)).ceil() as u32;
        let mut placeholder_hashes = Vec::new();
        for chunk_id in 0..total_chunks {
            placeholder_hashes.push(format!("{}_{}", metadata.merkle_root, chunk_id));
        }
        assert_eq!(placeholder_hashes.len(), total_chunks as usize);
    }

    println!("[OK] Backward compatibility maintained for old metadata");
}

/// Test manifest with different chunk sizes
#[test]
fn test_manifest_with_variable_chunk_sizes() {
    let manifest = FileManifest {
        merkle_root: "root".to_string(),
        chunks: vec![
            ChunkInfo {
                index: 0,
                hash: "hash0".to_string(),
                size: 256 * 1024, // Full chunk
                encrypted_hash: String::new(),
                encrypted_size: 256 * 1024,
            },
            ChunkInfo {
                index: 1,
                hash: "hash1".to_string(),
                size: 256 * 1024, // Full chunk
                encrypted_hash: String::new(),
                encrypted_size: 256 * 1024,
            },
            ChunkInfo {
                index: 2,
                hash: "hash2".to_string(),
                size: 50 * 1024, // Partial last chunk
                encrypted_hash: String::new(),
                encrypted_size: 50 * 1024,
            },
        ],
        encrypted_key_bundle: None,
    };

    let manifest_json = serde_json::to_string(&manifest).unwrap();
    let parsed: FileManifest = serde_json::from_str(&manifest_json).unwrap();

    // Verify sizes are preserved
    assert_eq!(parsed.chunks[0].size, 256 * 1024);
    assert_eq!(parsed.chunks[1].size, 256 * 1024);
    assert_eq!(parsed.chunks[2].size, 50 * 1024);

    // Calculate total file size
    let total_size: usize = parsed.chunks.iter().map(|c| c.size).sum();
    assert_eq!(total_size, 562 * 1024);

    println!("[OK] Variable chunk sizes handled correctly");
}

/// Test manifest integrity after JSON round-trip
#[test]
fn test_manifest_json_round_trip_integrity() {
    use sha2::{Digest, Sha256};

    // Create manifest with real SHA-256 hashes
    let mut chunks = Vec::new();
    let test_data = vec![
        b"Chunk 0: The quick brown fox".to_vec(),
        b"Chunk 1: jumps over the lazy dog".to_vec(),
        b"Chunk 2: Lorem ipsum dolor sit amet".to_vec(),
    ];

    for (index, data) in test_data.iter().enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hex::encode(hasher.finalize());

        chunks.push(ChunkInfo {
            index: index as u32,
            hash: hash.clone(),
            size: data.len(),
            encrypted_hash: String::new(),
            encrypted_size: data.len(),
        });
    }

    let original_manifest = FileManifest {
        merkle_root: "integrity_test_root".to_string(),
        chunks,
        encrypted_key_bundle: None,
    };

    // JSON round-trip
    let json = serde_json::to_string(&original_manifest).unwrap();
    let restored_manifest: FileManifest = serde_json::from_str(&json).unwrap();

    // Verify integrity
    assert_eq!(restored_manifest.merkle_root, original_manifest.merkle_root);
    assert_eq!(restored_manifest.chunks.len(), original_manifest.chunks.len());

    for (original, restored) in original_manifest.chunks.iter().zip(restored_manifest.chunks.iter()) {
        assert_eq!(original.index, restored.index);
        assert_eq!(original.hash, restored.hash);
        assert_eq!(original.size, restored.size);
    }

    println!("[OK] Manifest integrity maintained through JSON round-trip");
}

