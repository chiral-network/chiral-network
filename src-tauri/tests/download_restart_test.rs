// Download Restart Snapshot Tests
// Tests event payloads across all main states

use chiral_network::download_restart::*;
use serde_json;

#[test]
fn test_download_status_serialization_idle() {
    let status = DownloadStatus {
        download_id: "test-123".to_string(),
        state: DownloadState::Idle,
        bytes_downloaded: 0,
        expected_size: None,
        etag: None,
        lease_exp: None,
        last_error: None,
    };

    let json = serde_json::to_string(&status).expect("Failed to serialize");
    assert!(json.contains("\"state\":\"Idle\""));
    assert!(json.contains("\"bytes_downloaded\":0"));

    // Verify round-trip
    let deserialized: DownloadStatus = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.download_id, "test-123");
    assert_eq!(deserialized.state, DownloadState::Idle);
}

#[test]
fn test_download_status_serialization_downloading() {
    let status = DownloadStatus {
        download_id: "test-456".to_string(),
        state: DownloadState::Downloading,
        bytes_downloaded: 5242880,
        expected_size: Some(10485760),
        etag: Some("\"abc123\"".to_string()),
        lease_exp: Some(1234567890),
        last_error: None,
    };

    let json = serde_json::to_string(&status).expect("Failed to serialize");
    assert!(json.contains("\"state\":\"Downloading\""));
    assert!(json.contains("\"bytes_downloaded\":5242880"));
    assert!(json.contains("\"expected_size\":10485760"));
    assert!(json.contains("\"etag\":\"\\\"abc123\\\"\""));

    // Verify round-trip
    let deserialized: DownloadStatus = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.state, DownloadState::Downloading);
    assert_eq!(deserialized.bytes_downloaded, 5242880);
}

#[test]
fn test_download_status_serialization_paused() {
    let status = DownloadStatus {
        download_id: "test-789".to_string(),
        state: DownloadState::Paused,
        bytes_downloaded: 2621440,
        expected_size: Some(10485760),
        etag: Some("\"xyz789\"".to_string()),
        lease_exp: Some(1234567890),
        last_error: None,
    };

    let json = serde_json::to_string(&status).expect("Failed to serialize");
    assert!(json.contains("\"state\":\"Paused\""));
    assert!(json.contains("\"bytes_downloaded\":2621440"));

    let deserialized: DownloadStatus = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.state, DownloadState::Paused);
}

#[test]
fn test_download_status_serialization_failed() {
    let status = DownloadStatus {
        download_id: "test-fail".to_string(),
        state: DownloadState::Failed,
        bytes_downloaded: 1048576,
        expected_size: Some(10485760),
        etag: Some("\"fail123\"".to_string()),
        lease_exp: None,
        last_error: Some("Network error: connection timeout".to_string()),
    };

    let json = serde_json::to_string(&status).expect("Failed to serialize");
    assert!(json.contains("\"state\":\"Failed\""));
    assert!(json.contains("\"last_error\":\"Network error: connection timeout\""));

    let deserialized: DownloadStatus = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.state, DownloadState::Failed);
    assert_eq!(
        deserialized.last_error,
        Some("Network error: connection timeout".to_string())
    );
}

#[test]
fn test_download_status_serialization_completed() {
    let status = DownloadStatus {
        download_id: "test-complete".to_string(),
        state: DownloadState::Completed,
        bytes_downloaded: 10485760,
        expected_size: Some(10485760),
        etag: Some("\"complete123\"".to_string()),
        lease_exp: None,
        last_error: None,
    };

    let json = serde_json::to_string(&status).expect("Failed to serialize");
    assert!(json.contains("\"state\":\"Completed\""));
    assert!(json.contains("\"bytes_downloaded\":10485760"));

    let deserialized: DownloadStatus = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.state, DownloadState::Completed);
    assert_eq!(deserialized.bytes_downloaded, 10485760);
}

#[test]
fn test_all_download_states() {
    // Test that all states can be serialized and deserialized
    let states = vec![
        DownloadState::Idle,
        DownloadState::Handshake,
        DownloadState::HandshakeRetry,
        DownloadState::LeaseRenewDue,
        DownloadState::PreparingHead,
        DownloadState::HeadBackoff,
        DownloadState::Restarting,
        DownloadState::PreflightStorage,
        DownloadState::ValidatingMetadata,
        DownloadState::Downloading,
        DownloadState::PersistingProgress,
        DownloadState::Paused,
        DownloadState::AwaitingResume,
        DownloadState::LeaseExpired,
        DownloadState::VerifyingSha,
        DownloadState::FinalizingIo,
        DownloadState::Completed,
        DownloadState::Failed,
    ];

    for state in states {
        let status = DownloadStatus {
            download_id: format!("test-{:?}", state),
            state: state.clone(),
            bytes_downloaded: 0,
            expected_size: None,
            etag: None,
            lease_exp: None,
            last_error: None,
        };

        let json = serde_json::to_string(&status).expect("Failed to serialize");
        let deserialized: DownloadStatus =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized.state, state);
    }
}

#[test]
fn test_download_metadata_version_validation() {
    let metadata = DownloadMetadata::new("test-id".to_string(), "http://example.com".to_string());
    assert!(metadata.validate_version().is_ok());
    assert_eq!(metadata.version, DownloadMetadata::CURRENT_VERSION);
}

#[test]
fn test_download_error_codes() {
    assert_eq!(DownloadError::NotFound.to_error_code(), "DOWNLOAD_NOT_FOUND");
    assert_eq!(
        DownloadError::Invalid("test".to_string()).to_error_code(),
        "DOWNLOAD_INVALID_REQUEST"
    );
    assert_eq!(
        DownloadError::Source("test".to_string()).to_error_code(),
        "DOWNLOAD_SOURCE_ERROR"
    );
    assert_eq!(
        DownloadError::Io("test".to_string()).to_error_code(),
        "IO_ERROR"
    );
    assert_eq!(DownloadError::DiskFull.to_error_code(), "STORAGE_EXHAUSTED");
    assert_eq!(
        DownloadError::AlreadyCompleted.to_error_code(),
        "DOWNLOAD_ALREADY_COMPLETE"
    );
}

#[test]
fn test_download_error_human_readable() {
    let error = DownloadError::NotFound;
    assert!(error
        .to_human_readable()
        .contains("Download not found"));

    let error = DownloadError::DiskFull;
    assert!(error
        .to_human_readable()
        .contains("Insufficient disk space"));

    let error = DownloadError::Invalid("path traversal".to_string());
    assert!(error
        .to_human_readable()
        .contains("Invalid request"));
}

#[test]
fn test_download_state_human_readable() {
    assert_eq!(DownloadState::Idle.to_human_readable(), "Idle");
    assert_eq!(
        DownloadState::Downloading.to_human_readable(),
        "Downloading"
    );
    assert_eq!(
        DownloadState::VerifyingSha.to_human_readable(),
        "Verifying file integrity"
    );
    assert_eq!(DownloadState::Failed.to_human_readable(), "Failed");
}

/// Snapshot test: verify event payload structure for Idle state
#[test]
fn snapshot_event_payload_idle() {
    let status = DownloadStatus {
        download_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        state: DownloadState::Idle,
        bytes_downloaded: 0,
        expected_size: None,
        etag: None,
        lease_exp: None,
        last_error: None,
    };

    let json = serde_json::to_value(&status).expect("Failed to serialize");
    insta::assert_json_snapshot!("idle_state", json);
}

/// Snapshot test: verify event payload structure for Downloading state
#[test]
fn snapshot_event_payload_downloading() {
    let status = DownloadStatus {
        download_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        state: DownloadState::Downloading,
        bytes_downloaded: 5242880,
        expected_size: Some(10485760),
        etag: Some("\"abc123def456\"".to_string()),
        lease_exp: Some(1704067200),
        last_error: None,
    };

    let json = serde_json::to_value(&status).expect("Failed to serialize");
    insta::assert_json_snapshot!("downloading_state", json);
}

/// Snapshot test: verify event payload structure for Paused state
#[test]
fn snapshot_event_payload_paused() {
    let status = DownloadStatus {
        download_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        state: DownloadState::Paused,
        bytes_downloaded: 7340032,
        expected_size: Some(10485760),
        etag: Some("\"abc123def456\"".to_string()),
        lease_exp: Some(1704067200),
        last_error: None,
    };

    let json = serde_json::to_value(&status).expect("Failed to serialize");
    insta::assert_json_snapshot!("paused_state", json);
}

/// Snapshot test: verify event payload structure for Failed state with error
#[test]
fn snapshot_event_payload_failed() {
    let status = DownloadStatus {
        download_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        state: DownloadState::Failed,
        bytes_downloaded: 2097152,
        expected_size: Some(10485760),
        etag: Some("\"abc123def456\"".to_string()),
        lease_exp: None,
        last_error: Some("Source error: weak ETag detected, cannot resume safely".to_string()),
    };

    let json = serde_json::to_value(&status).expect("Failed to serialize");
    insta::assert_json_snapshot!("failed_state", json);
}

/// Snapshot test: verify event payload structure for Completed state
#[test]
fn snapshot_event_payload_completed() {
    let status = DownloadStatus {
        download_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        state: DownloadState::Completed,
        bytes_downloaded: 10485760,
        expected_size: Some(10485760),
        etag: Some("\"abc123def456\"".to_string()),
        lease_exp: None,
        last_error: None,
    };

    let json = serde_json::to_value(&status).expect("Failed to serialize");
    insta::assert_json_snapshot!("completed_state", json);
}
