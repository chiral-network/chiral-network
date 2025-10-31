// Person 3: Unified Source Abstraction Tests
// Tests for DownloadSource enum in download_source.rs

use chiral_network::download_source::{
    DownloadSource, FtpSourceInfo, HttpSourceInfo, P2pSourceInfo,
};
use serde_json;

/// Test P2P source creation and methods
#[test]
fn test_p2p_source_creation() {
    let source = DownloadSource::P2p(P2pSourceInfo {
        peer_id: "12D3KooWExample".to_string(),
        multiaddr: Some("/ip4/127.0.0.1/tcp/4001".to_string()),
        reputation: Some(85),
        supports_encryption: true,
        protocol: Some("webrtc".to_string()),
    });

    assert_eq!(source.source_type(), "P2P");
    assert!(source.supports_encryption());
    assert!(source.priority_score() > 100);
    assert_eq!(source.identifier(), "12D3KooWExample");
}

/// Test HTTP source creation and methods
#[test]
fn test_http_source_creation() {
    let source = DownloadSource::Http(HttpSourceInfo {
        url: "https://example.com/file.zip".to_string(),
        auth_header: None,
        verify_ssl: true,
        headers: None,
        timeout_secs: Some(30),
    });

    assert_eq!(source.source_type(), "HTTP");
    assert!(source.supports_encryption()); // https = encrypted
    assert_eq!(source.priority_score(), 50);
    assert_eq!(source.identifier(), "https://example.com/file.zip");
}

/// Test FTP source creation and methods
#[test]
fn test_ftp_source_creation() {
    let source = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftp://ftp.example.com/pub/file.tar.gz".to_string(),
        username: Some("anonymous".to_string()),
        encrypted_password: None,
        passive_mode: true,
        use_ftps: false,
        timeout_secs: Some(60),
    });

    assert_eq!(source.source_type(), "FTP");
    assert!(!source.supports_encryption()); // ftp (not ftps) = no encryption
    assert_eq!(source.priority_score(), 25);
    assert_eq!(source.display_name(), "FTP: ftp.example.com");
    assert_eq!(source.identifier(), "ftp://ftp.example.com/pub/file.tar.gz");
}

/// Test FTPS (secure FTP) source
#[test]
fn test_ftps_source_encryption() {
    let source = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftps://secure.example.com/file.bin".to_string(),
        username: Some("user".to_string()),
        encrypted_password: Some("encrypted_pass".to_string()),
        passive_mode: true,
        use_ftps: true,
        timeout_secs: Some(30),
    });

    assert_eq!(source.source_type(), "FTP");
    assert!(source.supports_encryption()); // ftps = encrypted
}

/// Test HTTP (non-secure) source
#[test]
fn test_http_non_secure() {
    let source = DownloadSource::Http(HttpSourceInfo {
        url: "http://example.com/file.zip".to_string(), // http not https
        auth_header: None,
        verify_ssl: true,
        headers: None,
        timeout_secs: Some(30),
    });

    assert_eq!(source.source_type(), "HTTP");
    assert!(!source.supports_encryption()); // http = no encryption
}

/// Test priority scoring
#[test]
fn test_priority_scoring() {
    let p2p = DownloadSource::P2p(P2pSourceInfo {
        peer_id: "12D3KooW".to_string(),
        multiaddr: None,
        reputation: Some(90),
        supports_encryption: true,
        protocol: None,
    });

    let http = DownloadSource::Http(HttpSourceInfo {
        url: "https://example.com/file".to_string(),
        auth_header: None,
        verify_ssl: true,
        headers: None,
        timeout_secs: None,
    });

    let ftp = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftp://ftp.example.com/file".to_string(),
        username: None,
        encrypted_password: None,
        passive_mode: true,
        use_ftps: false,
        timeout_secs: None,
    });

    // P2P should have highest priority, FTP lowest
    assert!(p2p.priority_score() > http.priority_score());
    assert!(http.priority_score() > ftp.priority_score());
}

/// Test P2P reputation affects priority
#[test]
fn test_p2p_reputation_priority() {
    let high_rep = DownloadSource::P2p(P2pSourceInfo {
        peer_id: "12D3KooWHigh".to_string(),
        multiaddr: None,
        reputation: Some(95),
        supports_encryption: false,
        protocol: None,
    });

    let low_rep = DownloadSource::P2p(P2pSourceInfo {
        peer_id: "12D3KooWLow".to_string(),
        multiaddr: None,
        reputation: Some(20),
        supports_encryption: false,
        protocol: None,
    });

    assert!(high_rep.priority_score() > low_rep.priority_score());
}

/// Test display names
#[test]
fn test_display_names() {
    let p2p = DownloadSource::P2p(P2pSourceInfo {
        peer_id: "12D3KooWABCDEFGH123456".to_string(),
        multiaddr: None,
        reputation: None,
        supports_encryption: false,
        protocol: None,
    });
    assert_eq!(p2p.display_name(), "P2P peer: 12D3KooW");

    let http = DownloadSource::Http(HttpSourceInfo {
        url: "https://cdn.example.com/files/data.zip".to_string(),
        auth_header: None,
        verify_ssl: true,
        headers: None,
        timeout_secs: None,
    });
    assert_eq!(http.display_name(), "HTTP: cdn.example.com");

    let ftp = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftp://ftp.gnu.org/gnu/file.tar.gz".to_string(),
        username: None,
        encrypted_password: None,
        passive_mode: true,
        use_ftps: false,
        timeout_secs: None,
    });
    assert_eq!(ftp.display_name(), "FTP: ftp.gnu.org");
}

/// Test Display trait
#[test]
fn test_display_trait() {
    let source = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftp://example.com/file".to_string(),
        username: None,
        encrypted_password: None,
        passive_mode: true,
        use_ftps: false,
        timeout_secs: None,
    });

    let display_string = format!("{}", source);
    assert_eq!(display_string, "FTP: example.com");
}

/// Test serialization to JSON
#[test]
fn test_serialization() {
    let source = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftp://ftp.example.com/file.bin".to_string(),
        username: Some("user".to_string()),
        encrypted_password: Some("encrypted".to_string()),
        passive_mode: true,
        use_ftps: false,
        timeout_secs: Some(60),
    });

    let json = serde_json::to_string(&source).expect("Failed to serialize");

    // Check type tag is present
    assert!(json.contains("\"type\""));
    assert!(json.contains("\"url\""));
}

/// Test deserialization from JSON
#[test]
fn test_deserialization_ftp() {
    let json = r#"{
        "type": "ftp",
        "url": "ftp://ftp.example.com/file.bin",
        "username": "user",
        "encryptedPassword": "encrypted",
        "passiveMode": true,
        "useFtps": false,
        "timeoutSecs": 60
    }"#;

    let source: DownloadSource = serde_json::from_str(json).expect("Failed to deserialize");

    match source {
        DownloadSource::Ftp(info) => {
            assert_eq!(info.url, "ftp://ftp.example.com/file.bin");
            assert_eq!(info.username, Some("user".to_string()));
            assert_eq!(info.encrypted_password, Some("encrypted".to_string()));
            assert!(info.passive_mode);
            assert!(!info.use_ftps);
            assert_eq!(info.timeout_secs, Some(60));
        }
        _ => panic!("Expected FTP source"),
    }
}

/// Test deserialization of P2P source
#[test]
fn test_deserialization_p2p() {
    let json = r#"{
        "type": "p2p",
        "peerId": "12D3KooWExample",
        "multiaddr": "/ip4/127.0.0.1/tcp/4001",
        "reputation": 85,
        "supportsEncryption": true,
        "protocol": "webrtc"
    }"#;

    let source: DownloadSource = serde_json::from_str(json).expect("Failed to deserialize");

    match source {
        DownloadSource::P2p(info) => {
            assert_eq!(info.peer_id, "12D3KooWExample");
            assert_eq!(info.reputation, Some(85));
            assert!(info.supports_encryption);
        }
        _ => panic!("Expected P2P source"),
    }
}

/// Test deserialization of HTTP source
#[test]
fn test_deserialization_http() {
    let json = r#"{
        "type": "http",
        "url": "https://example.com/file.zip",
        "verifySsl": true,
        "timeoutSecs": 30
    }"#;

    let source: DownloadSource = serde_json::from_str(json).expect("Failed to deserialize");

    match source {
        DownloadSource::Http(info) => {
            assert_eq!(info.url, "https://example.com/file.zip");
            assert!(info.verify_ssl);
            assert_eq!(info.timeout_secs, Some(30));
        }
        _ => panic!("Expected HTTP source"),
    }
}

/// Test round-trip serialization/deserialization
#[test]
fn test_roundtrip_serialization() {
    let original = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftp://server.example.com/data.bin".to_string(),
        username: Some("admin".to_string()),
        encrypted_password: Some("encrypted_data".to_string()),
        passive_mode: true,
        use_ftps: false,
        timeout_secs: Some(120),
    });

    // Serialize
    let json = serde_json::to_string(&original).expect("Failed to serialize");

    // Deserialize
    let deserialized: DownloadSource = serde_json::from_str(&json).expect("Failed to deserialize");

    // Verify type
    assert_eq!(original.source_type(), deserialized.source_type());
    assert_eq!(original.identifier(), deserialized.identifier());
}

/// Test clone functionality
#[test]
fn test_clone() {
    let original = DownloadSource::Ftp(FtpSourceInfo {
        url: "ftp://example.com/file".to_string(),
        username: Some("user".to_string()),
        encrypted_password: None,
        passive_mode: true,
        use_ftps: false,
        timeout_secs: Some(30),
    });

    let cloned = original.clone();

    assert_eq!(original.source_type(), cloned.source_type());
    assert_eq!(original.identifier(), cloned.identifier());
    assert_eq!(original.priority_score(), cloned.priority_score());
}

/// Test multiple sources in a Vec (typical usage scenario)
#[test]
fn test_multiple_sources_mixed() {
    let sources = vec![
        DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooW1".to_string(),
            multiaddr: None,
            reputation: Some(90),
            supports_encryption: true,
            protocol: Some("webrtc".to_string()),
        }),
        DownloadSource::Http(HttpSourceInfo {
            url: "https://cdn1.example.com/file.zip".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: Some(30),
        }),
        DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://ftp.example.com/file.zip".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(60),
        }),
    ];

    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].source_type(), "P2P");
    assert_eq!(sources[1].source_type(), "HTTP");
    assert_eq!(sources[2].source_type(), "FTP");
}

/// Test source sorting by priority
#[test]
fn test_source_priority_sorting() {
    let mut sources = vec![
        DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://ftp.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: None,
        }),
        DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooW1".to_string(),
            multiaddr: None,
            reputation: Some(80),
            supports_encryption: true,
            protocol: None,
        }),
        DownloadSource::Http(HttpSourceInfo {
            url: "https://example.com/file".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: None,
        }),
    ];

    // Sort by priority (descending)
    sources.sort_by(|a, b| b.priority_score().cmp(&a.priority_score()));

    // After sorting: P2P, HTTP, FTP
    assert_eq!(sources[0].source_type(), "P2P");
    assert_eq!(sources[1].source_type(), "HTTP");
    assert_eq!(sources[2].source_type(), "FTP");
}