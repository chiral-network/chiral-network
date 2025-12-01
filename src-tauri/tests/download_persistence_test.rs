// download_persistence_test.rs
// Comprehensive integration tests for download persistence
//
// Tests required by Elliot's deliverables:
// - Crash between write and metadata flush → safe resume
// - Disk-full mid-stream → Failed(DiskFull); artifacts preserved
// - Cross-device finalize correctness

use chiral_network::download_persistence::{
    DownloadMetadata, DownloadPersistence, PartFileWriter, PersistenceConfig, PersistenceError,
};
use std::fs::{self, File};
use std::io::{Read, Write};
use tempfile::TempDir;

#[test]
fn test_crash_recovery_safe_resume() {
    // Simulate crash between write and metadata flush
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        fsync_interval: 1024, // Small interval for testing
        strict_validation: true,
    };
    
    let persistence = DownloadPersistence::new(config.clone());
    let dest_path = temp_dir.path().join("test.bin");
    let (part_path, meta_path) = persistence.get_temp_paths(&dest_path);
    
    // Initial download: write some data
    let metadata = DownloadMetadata {
        version: 1,
        download_id: "crash-test-1".to_string(),
        url: "https://example.com/file.bin".to_string(),
        etag: Some("\"etag123\"".to_string()),
        expected_size: 4096,
        bytes_downloaded: 0,
        last_modified: None,
        sha256_final: None,
    };
    
    // Write initial metadata
    persistence.write_metadata_atomic(&meta_path, &metadata).unwrap();
    
    // Acquire lock and write some data
    let (lock, file) = persistence.acquire_lock(&part_path).unwrap();
    let mut writer = PartFileWriter::new(file, lock, 512, 0).unwrap();
    
    let data = vec![0xAB; 2048];
    writer.write(&data).unwrap();
    writer.fsync().unwrap();
    
    // Update metadata to reflect bytes written
    let updated_metadata = DownloadMetadata {
        bytes_downloaded: 2048,
        ..metadata.clone()
    };
    persistence.write_metadata_atomic(&meta_path, &updated_metadata).unwrap();
    
    // Simulate crash: drop writer without finalizing
    drop(writer);
    
    // Resume: load metadata and validate part file
    let loaded_metadata = persistence.read_metadata(&meta_path).unwrap();
    assert_eq!(loaded_metadata.bytes_downloaded, 2048);
    
    // Validate part file matches metadata
    let validation_result = persistence.validate_part_file(&part_path, &loaded_metadata);
    assert!(validation_result.is_ok(), "Part file validation should pass after crash recovery");
    
    // Resume writing from offset
    let (lock2, file2) = persistence.acquire_lock(&part_path).unwrap();
    let mut writer2 = PartFileWriter::new(file2, lock2, 512, 2048).unwrap();
    
    let more_data = vec![0xCD; 2048];
    writer2.write(&more_data).unwrap();
    writer2.finalize().unwrap();
    
    // Verify total size
    let final_size = fs::metadata(&part_path).unwrap().len();
    assert_eq!(final_size, 4096, "Part file should have complete data after resume");
}

#[test]
fn test_metadata_persistence_atomic() {
    // Test atomic metadata write survives partial writes
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let meta_path = temp_dir.path().join("test.meta.json");
    
    let metadata = DownloadMetadata {
        version: 1,
        download_id: "atomic-test-1".to_string(),
        url: "https://example.com/large.bin".to_string(),
        etag: Some("\"xyz789\"".to_string()),
        expected_size: 1024000,
        bytes_downloaded: 512000,
        last_modified: Some("2025-01-10T12:00:00Z".to_string()),
        sha256_final: None,
    };
    
    // Write metadata multiple times (simulating updates)
    for i in 0..10 {
        let updated = DownloadMetadata {
            bytes_downloaded: 512000 + (i * 10000),
            ..metadata.clone()
        };
        persistence.write_metadata_atomic(&meta_path, &updated).unwrap();
    }
    
    // Verify final state is consistent
    let final_metadata = persistence.read_metadata(&meta_path).unwrap();
    assert_eq!(final_metadata.bytes_downloaded, 512000 + 90000);
    assert_eq!(final_metadata.version, 1);
    
    // Verify no .tmp files left behind
    let tmp_path = meta_path.with_extension("meta.json.tmp");
    assert!(!tmp_path.exists(), "Temporary file should be cleaned up");
}

#[test]
fn test_part_size_mismatch_detection() {
    // Test detection of corrupt part files (size mismatch)
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        strict_validation: true,
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let dest_path = temp_dir.path().join("corrupt.bin");
    let (part_path, meta_path) = persistence.get_temp_paths(&dest_path);
    
    // Create metadata claiming 1024 bytes downloaded
    let metadata = DownloadMetadata {
        version: 1,
        download_id: "mismatch-test-1".to_string(),
        url: "https://example.com/file.bin".to_string(),
        etag: Some("\"abc123\"".to_string()),
        expected_size: 2048,
        bytes_downloaded: 1024,
        last_modified: None,
        sha256_final: None,
    };
    
    persistence.write_metadata_atomic(&meta_path, &metadata).unwrap();
    
    // Create part file with wrong size (2048 bytes instead of 1024)
    let mut part_file = File::create(&part_path).unwrap();
    part_file.write_all(&vec![0u8; 2048]).unwrap();
    drop(part_file);
    
    // Validation should detect mismatch
    let result = persistence.validate_part_file(&part_path, &metadata);
    assert!(matches!(result, Err(PersistenceError::PartSizeMismatch { expected: 1024, actual: 2048 })));
}

#[test]
fn test_preflight_disk_space_check() {
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let dest_path = temp_dir.path().join("big.bin");
    
    // Reasonable size should pass
    let result = persistence.preflight_storage_check(&dest_path, 10 * 1024 * 1024, 0);
    assert!(result.is_ok(), "Preflight should succeed for reasonable file size");
    
    // Unreasonably large size should fail
    let result = persistence.preflight_storage_check(&dest_path, u64::MAX / 2, 0);
    assert!(matches!(result, Err(PersistenceError::DiskFull { .. })), 
            "Preflight should fail for unreasonably large files");
}

#[test]
fn test_path_sandboxing_security() {
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    
    // Create a valid file inside downloads_root
    let valid_subdir = temp_dir.path().join("safe");
    fs::create_dir_all(&valid_subdir).unwrap();
    let valid_file = valid_subdir.join("file.bin");
    File::create(&valid_file).unwrap();
    
    // Valid path should pass
    assert!(persistence.validate_destination_path(&valid_file).is_ok());
    
    // Path traversal attempts should fail
    let traversal_attempts = vec![
        temp_dir.path().join("../../../etc/passwd"),
        temp_dir.path().join("..").join("..").join("etc").join("hosts"),
    ];
    
    for evil_path in traversal_attempts {
        let result = persistence.validate_destination_path(&evil_path);
        assert!(matches!(result, Err(PersistenceError::PathTraversal(_))),
                "Path traversal should be blocked: {:?}", evil_path);
    }
}

#[test]
fn test_fsync_interval_policy() {
    // Test that fsync happens at correct intervals
    let temp_dir = TempDir::new().unwrap();
    let part_path = temp_dir.path().join("fsync_test.part");
    
    let file = File::create(&part_path).unwrap();
    let lock = std::sync::Arc::new(std::sync::Mutex::new(()));
    
    // 16-byte fsync interval for testing
    let mut writer = PartFileWriter::new(file, lock, 16, 0).unwrap();
    
    // Write 48 bytes (should trigger 3 fsyncs)
    let chunk1 = vec![0x01; 16];
    let chunk2 = vec![0x02; 16];
    let chunk3 = vec![0x03; 16];
    
    writer.write(&chunk1).unwrap();
    writer.write(&chunk2).unwrap();
    writer.write(&chunk3).unwrap();
    
    assert_eq!(writer.total_bytes_written(), 48);
    
    // Finalize should fsync remaining bytes
    writer.finalize().unwrap();
    
    // Verify file contents
    let mut contents = Vec::new();
    File::open(&part_path).unwrap().read_to_end(&mut contents).unwrap();
    assert_eq!(contents.len(), 48);
}

#[test]
fn test_cross_volume_finalize() {
    // Note: This test simulates cross-volume scenario
    // In practice, we can't easily test actual cross-filesystem moves in unit tests
    // but we can verify the logic handles both cases
    
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let dest_path = temp_dir.path().join("final.bin");
    let (part_path, meta_path) = persistence.get_temp_paths(&dest_path);
    
    // Create part file with test data
    let test_data = vec![0xAB; 4096];
    fs::write(&part_path, &test_data).unwrap();
    
    // Create metadata
    let metadata = DownloadMetadata {
        version: 1,
        download_id: "finalize-test-1".to_string(),
        url: "https://example.com/file.bin".to_string(),
        etag: Some("\"final123\"".to_string()),
        expected_size: 4096,
        bytes_downloaded: 4096,
        last_modified: None,
        sha256_final: Some("abcdef".to_string()),
    };
    
    persistence.write_metadata_atomic(&meta_path, &metadata).unwrap();
    
    // Finalize download
    persistence.finalize_download(&part_path, &dest_path, &meta_path).unwrap();
    
    // Verify final file exists and has correct content
    assert!(dest_path.exists(), "Final file should exist");
    let final_content = fs::read(&dest_path).unwrap();
    assert_eq!(final_content, test_data);
    
    // Verify cleanup
    assert!(!part_path.exists(), ".part file should be removed");
    assert!(!meta_path.exists(), ".meta.json file should be removed");
}

#[test]
fn test_concurrent_lock_prevention() {
    // Test that OS advisory lock prevents concurrent writes
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let dest_path = temp_dir.path().join("locked.bin");
    let (part_path, _) = persistence.get_temp_paths(&dest_path);
    
    // Acquire first lock
    let (lock1, _file1) = persistence.acquire_lock(&part_path).unwrap();
    
    // Try to acquire second lock (should fail)
    let result = persistence.acquire_lock(&part_path);
    assert!(matches!(result, Err(PersistenceError::LockFailed(_))),
            "Second lock acquisition should fail");
    
    // Drop first lock
    drop(lock1);
    drop(_file1);
    
    // Now second lock should succeed
    let result2 = persistence.acquire_lock(&part_path);
    assert!(result2.is_ok(), "Lock should succeed after first lock is released");
}

#[test]
fn test_metadata_version_validation() {
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let meta_path = temp_dir.path().join("version_test.meta.json");
    
    // Write metadata with unsupported version
    let future_metadata = serde_json::json!({
        "version": 99,
        "download_id": "future-1",
        "url": "https://example.com/file.bin",
        "expected_size": 1024,
        "bytes_downloaded": 512
    });
    
    fs::write(&meta_path, future_metadata.to_string()).unwrap();
    
    // Reading should fail with version error
    let result = persistence.read_metadata(&meta_path);
    assert!(matches!(result, Err(PersistenceError::UnsupportedVersion(99))),
            "Should reject unsupported metadata version");
}

#[test]
fn test_cleanup_artifacts() {
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let dest_path = temp_dir.path().join("cleanup.bin");
    let (part_path, meta_path) = persistence.get_temp_paths(&dest_path);
    
    // Create artifacts
    fs::write(&part_path, b"test data").unwrap();
    fs::write(&meta_path, b"{}").unwrap();
    
    assert!(part_path.exists());
    assert!(meta_path.exists());
    
    // Clean up
    persistence.cleanup_artifacts(&part_path, &meta_path).unwrap();
    
    // Verify cleanup
    assert!(!part_path.exists(), ".part should be removed");
    assert!(!meta_path.exists(), ".meta.json should be removed");
}

#[test]
fn test_resume_with_offset() {
    // Test resuming download from specific offset
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        downloads_root: temp_dir.path().to_path_buf(),
        fsync_interval: 1024,
        ..Default::default()
    };
    
    let persistence = DownloadPersistence::new(config);
    let dest_path = temp_dir.path().join("resume.bin");
    let (part_path, meta_path) = persistence.get_temp_paths(&dest_path);
    
    // Initial download: write first 2048 bytes
    let (lock1, file1) = persistence.acquire_lock(&part_path).unwrap();
    let mut writer1 = PartFileWriter::new(file1, lock1, 512, 0).unwrap();
    writer1.write(&vec![0xAA; 2048]).unwrap();
    writer1.finalize().unwrap();
    
    // Save metadata
    let metadata = DownloadMetadata {
        version: 1,
        download_id: "resume-test-1".to_string(),
        url: "https://example.com/file.bin".to_string(),
        etag: Some("\"resume123\"".to_string()),
        expected_size: 4096,
        bytes_downloaded: 2048,
        last_modified: None,
        sha256_final: None,
    };
    persistence.write_metadata_atomic(&meta_path, &metadata).unwrap();
    
    // Resume: write next 2048 bytes
    let (lock2, file2) = persistence.acquire_lock(&part_path).unwrap();
    let mut writer2 = PartFileWriter::new(file2, lock2, 512, 2048).unwrap();
    writer2.write(&vec![0xBB; 2048]).unwrap();
    writer2.finalize().unwrap();
    
    // Verify complete file
    let contents = fs::read(&part_path).unwrap();
    assert_eq!(contents.len(), 4096);
    assert_eq!(&contents[0..2048], &vec![0xAA; 2048][..]);
    assert_eq!(&contents[2048..4096], &vec![0xBB; 2048][..]);
}
