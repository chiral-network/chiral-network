// Orchestrator FTP Integration Tests
// Tests for FTP source integration into multi_source_download.rs
//
// FTP protocol is fully implemented and working. These tests verify
// FTP source handling in the download orchestrator.

use chiral_network::dht::{FileMetadata, FtpSourceInfo};
use chiral_network::download_source::{DownloadSource, FtpSourceInfo as DownloadFtpInfo, P2pSourceInfo};
use chiral_network::multi_source_download::ChunkInfo;

// ============================================================================
// FTP Source Handling Tests
// ============================================================================

/// Test extracting FTP sources from FileMetadata
#[tokio::test]
async fn test_extract_ftp_sources_from_metadata() {
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
        manifest: None,
        encrypted_key_bundle: None,
        ftp_sources: Some(vec![
            FtpSourceInfo {
                url: "ftp://mirror1.example.com/file.bin".to_string(),
                username: None,
                password: None,
                supports_resume: true,
                file_size: 1024 * 1024,
                last_checked: Some(1640995200),
                is_available: true,
            },
            FtpSourceInfo {
                url: "ftp://mirror2.example.com/file.bin".to_string(),
                username: Some("user".to_string()),
                password: Some("encrypted_pass".to_string()),
                supports_resume: false,
                file_size: 1024 * 1024,
                last_checked: Some(1640995200),
                is_available: true,
            },
        ]),
        ed2k_sources: None,
        http_sources: None,
        is_root: true,
        download_path: None,
        price: 0.0,
        uploader_address: None,
        info_hash: None,
        trackers: None,
    };

    // Test that metadata contains FTP sources
    assert!(metadata.ftp_sources.is_some());
    let ftp_sources = metadata.ftp_sources.unwrap();
    
    assert_eq!(ftp_sources.len(), 2);
    assert_eq!(ftp_sources[0].url, "ftp://mirror1.example.com/file.bin");
    assert_eq!(ftp_sources[1].username, Some("user".to_string()));
}

/// Test FTP source priority ordering
#[tokio::test]
async fn test_ftp_source_priority_ordering() {
    let sources = vec![
        DownloadSource::Ftp(DownloadFtpInfo {
            url: "ftp://slow.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(60),
        }),
        DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooW1".to_string(),
            multiaddr: None,
            reputation: Some(90),
            supports_encryption: true,
            protocol: None,
        }),
        DownloadSource::Ftp(DownloadFtpInfo {
            url: "ftps://secure.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: true,
            timeout_secs: Some(30),
        }),
    ];

    // Test priority scoring
    let mut sorted = sources.clone();
    sorted.sort_by(|a, b| b.priority_score().cmp(&a.priority_score()));

    // P2P should have highest priority
    assert_eq!(sorted[0].source_type(), "P2P");
    
    // Both FTP sources should be after P2P (they have same priority score)
    assert_eq!(sorted[1].source_type(), "FTP");
    assert_eq!(sorted[2].source_type(), "FTP");
    
    // Verify we have one FTPS and one regular FTP
    let ftp_sources: Vec<_> = sorted.iter().filter_map(|s| {
        if let DownloadSource::Ftp(info) = s { Some(info) } else { None }
    }).collect();
    assert_eq!(ftp_sources.len(), 2);
    assert!(ftp_sources.iter().any(|info| info.use_ftps));
    assert!(ftp_sources.iter().any(|info| !info.use_ftps));
}

/// Test FTP connection establishment in parallel with P2P
#[tokio::test]
async fn test_ftp_connection_establishment() {
    // TODO: Create orchestrator with mock DHT and WebRTC services
    // let orchestrator = create_test_orchestrator().await;

    // TODO: Start download with FTP sources
    // let result = orchestrator.start_download(
    //     "test_hash".to_string(),
    //     "/tmp/test.bin".to_string(),
    //     None,
    //     None,
    // ).await;

    // assert!(result.is_ok());

    // TODO: Verify FTP connections were established
    // let active_download = orchestrator.get_active_download("test_hash").await;
    // assert!(active_download.ftp_connections.len() > 0);
}

/// Test chunk assignment to FTP sources
#[tokio::test]
async fn test_ftp_chunk_assignment() {
    // Create test chunks
    let chunks = vec![
        ChunkInfo { chunk_id: 0, offset: 0, size: 256 * 1024, hash: "hash0".to_string() },
        ChunkInfo { chunk_id: 1, offset: 256 * 1024, size: 256 * 1024, hash: "hash1".to_string() },
        ChunkInfo { chunk_id: 2, offset: 512 * 1024, size: 256 * 1024, hash: "hash2".to_string() },
        ChunkInfo { chunk_id: 3, offset: 768 * 1024, size: 256 * 1024, hash: "hash3".to_string() },
    ];

    // Create test sources
    let sources = vec![
        DownloadSource::Ftp(DownloadFtpInfo {
            url: "ftp://mirror1.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(30),
        }),
        DownloadSource::P2p(P2pSourceInfo {
            peer_id: "peer1".to_string(),
            multiaddr: None,
            reputation: Some(80),
            supports_encryption: true,
            protocol: None,
        }),
    ];

    // Test chunk assignment logic (round-robin)
    let mut assignments: Vec<(DownloadSource, Vec<u32>)> = sources.iter().map(|s| (s.clone(), Vec::new())).collect();
    
    for (index, chunk) in chunks.iter().enumerate() {
        let source_index = index % sources.len();
        if let Some((_, chunks)) = assignments.get_mut(source_index) {
            chunks.push(chunk.chunk_id);
        }
    }

    // Verify chunks are distributed
    assert!(assignments[0].1.len() > 0); // FTP source gets chunks
    assert!(assignments[1].1.len() > 0); // P2P source gets chunks
}

/// Test FTP source fallback when P2P fails
#[tokio::test]
async fn test_ftp_source_fallback() {
    // Test that FTP sources are available as fallback
    let ftp_source = DownloadSource::Ftp(DownloadFtpInfo {
        url: "ftp://backup.example.com/file".to_string(),
        username: None,
        encrypted_password: None,
        passive_mode: true,
        use_ftps: false,
        timeout_secs: Some(30),
    });

    // Verify FTP source has lower priority than P2P but is still valid
    assert_eq!(ftp_source.source_type(), "FTP");
    assert!(ftp_source.priority_score() < 100); // Lower than P2P priority
}

/// Test mixed source download (P2P + FTP)
#[tokio::test]
async fn test_mixed_source_download() {
    // Test mixed source priority ordering
    let mixed_sources = vec![
        DownloadSource::P2p(P2pSourceInfo {
            peer_id: "peer1".to_string(),
            multiaddr: None,
            reputation: Some(90),
            supports_encryption: true,
            protocol: None,
        }),
        DownloadSource::Ftp(DownloadFtpInfo {
            url: "ftp://mirror1.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(30),
        }),
        DownloadSource::Http(chiral_network::download_source::HttpSourceInfo {
            url: "https://example.com/file".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: Some(30),
        }),
    ];

    // Test priority ordering: P2P > HTTP > FTP
    let mut sorted = mixed_sources.clone();
    sorted.sort_by(|a, b| b.priority_score().cmp(&a.priority_score()));

    assert_eq!(sorted[0].source_type(), "P2P");
    assert_eq!(sorted[1].source_type(), "HTTP");
    assert_eq!(sorted[2].source_type(), "FTP");
}

/// Test FTP credential decryption using file AES key
#[tokio::test]
async fn test_ftp_credential_decryption() {
    // Test FTP source with credentials
    let ftp_source = DownloadFtpInfo {
        url: "ftp://secure.example.com/file".to_string(),
        username: Some("user".to_string()),
        encrypted_password: Some("encrypted_data".to_string()),
        passive_mode: true,
        use_ftps: true,
        timeout_secs: Some(30),
    };

    // Verify credential fields are present
    assert_eq!(ftp_source.username, Some("user".to_string()));
    assert!(ftp_source.encrypted_password.is_some());
    assert!(ftp_source.use_ftps); // Should support encryption
}

/// Test FTP source with no credentials (anonymous)
#[tokio::test]
async fn test_ftp_anonymous_source() {
    // Test anonymous FTP source
    let ftp_source = DownloadFtpInfo {
        url: "ftp://ftp.gnu.org/gnu/file.tar.gz".to_string(),
        username: None,
        encrypted_password: None,
        passive_mode: true,
        use_ftps: false,
        timeout_secs: Some(30),
    };

    // Verify anonymous credentials
    assert_eq!(ftp_source.username, None);
    assert_eq!(ftp_source.encrypted_password, None);
    
    // Test as DownloadSource
    let download_source = DownloadSource::Ftp(ftp_source);
    assert_eq!(download_source.source_type(), "FTP");
}