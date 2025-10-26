// Person 2: Metadata Extension Tests
// Tests for FtpSourceInfo struct in dht.rs

use chiral_network::dht::FtpSourceInfo;
use serde_json;

/// Test FtpSourceInfo struct creation
#[test]
fn test_ftp_source_info_creation() {
    let ftp_source = FtpSourceInfo {
        url: "ftp://ftp.example.com/path/to/file.bin".to_string(),
        username: Some("testuser".to_string()),
        encrypted_password: Some("base64_encrypted_password".to_string()),
    };

    assert_eq!(ftp_source.url, "ftp://ftp.example.com/path/to/file.bin");
    assert_eq!(ftp_source.username, Some("testuser".to_string()));
    assert_eq!(ftp_source.encrypted_password, Some("base64_encrypted_password".to_string()));
}

/// Test FtpSourceInfo with anonymous credentials (no username/password)
#[test]
fn test_ftp_source_info_anonymous() {
    let ftp_source = FtpSourceInfo {
        url: "ftp://ftp.gnu.org/gnu/hello/hello-2.10.tar.gz".to_string(),
        username: None,
        encrypted_password: None,
    };

    assert_eq!(ftp_source.url, "ftp://ftp.gnu.org/gnu/hello/hello-2.10.tar.gz");
    assert_eq!(ftp_source.username, None);
    assert_eq!(ftp_source.encrypted_password, None);
}

/// Test FtpSourceInfo serialization to JSON (for DHT storage)
#[test]
fn test_ftp_source_info_serialization() {
    let ftp_source = FtpSourceInfo {
        url: "ftp://ftp.example.com/file.bin".to_string(),
        username: Some("user1".to_string()),
        encrypted_password: Some("encrypted_pass_123".to_string()),
    };

    let json = serde_json::to_string(&ftp_source).expect("Failed to serialize");

    // Check that all fields are present in JSON
    assert!(json.contains("ftp://ftp.example.com/file.bin"));
    assert!(json.contains("user1"));
    assert!(json.contains("encrypted_pass_123"));
}

/// Test FtpSourceInfo deserialization from JSON (from DHT retrieval)
#[test]
fn test_ftp_source_info_deserialization() {
    let json = r#"{
        "url": "ftp://ftp.example.com/file.bin",
        "username": "user1",
        "encryptedPassword": "encrypted_pass_123"
    }"#;

    let ftp_source: FtpSourceInfo = serde_json::from_str(json).expect("Failed to deserialize");

    assert_eq!(ftp_source.url, "ftp://ftp.example.com/file.bin");
    assert_eq!(ftp_source.username, Some("user1".to_string()));
    assert_eq!(ftp_source.encrypted_password, Some("encrypted_pass_123".to_string()));
}

/// Test FtpSourceInfo deserialization with missing optional fields
#[test]
fn test_ftp_source_info_deserialization_anonymous() {
    let json = r#"{
        "url": "ftp://ftp.gnu.org/gnu/file.tar.gz"
    }"#;

    let ftp_source: FtpSourceInfo = serde_json::from_str(json).expect("Failed to deserialize");

    assert_eq!(ftp_source.url, "ftp://ftp.gnu.org/gnu/file.tar.gz");
    assert_eq!(ftp_source.username, None);
    assert_eq!(ftp_source.encrypted_password, None);
}

/// Test FtpSourceInfo serialization/deserialization round-trip
#[test]
fn test_ftp_source_info_roundtrip() {
    let original = FtpSourceInfo {
        url: "ftp://server.example.com:2121/data/file.zip".to_string(),
        username: Some("admin".to_string()),
        encrypted_password: Some("AES256_ENCRYPTED_DATA_HERE".to_string()),
    };

    // Serialize
    let json = serde_json::to_string(&original).expect("Failed to serialize");

    // Deserialize
    let deserialized: FtpSourceInfo = serde_json::from_str(&json).expect("Failed to deserialize");

    // Verify equality
    assert_eq!(deserialized.url, original.url);
    assert_eq!(deserialized.username, original.username);
    assert_eq!(deserialized.encrypted_password, original.encrypted_password);
}

/// Test FtpSourceInfo with FTPS URL
#[test]
fn test_ftp_source_info_ftps() {
    let ftp_source = FtpSourceInfo {
        url: "ftps://secure.example.com/secure/file.bin".to_string(),
        username: Some("secureuser".to_string()),
        encrypted_password: Some("encrypted_secure_pass".to_string()),
    };

    assert_eq!(ftp_source.url, "ftps://secure.example.com/secure/file.bin");
    assert!(ftp_source.url.starts_with("ftps://"));
}

/// Test FtpSourceInfo with custom port
#[test]
fn test_ftp_source_info_custom_port() {
    let ftp_source = FtpSourceInfo {
        url: "ftp://ftp.example.com:2121/path/file.bin".to_string(),
        username: None,
        encrypted_password: None,
    };

    assert_eq!(ftp_source.url, "ftp://ftp.example.com:2121/path/file.bin");
    assert!(ftp_source.url.contains(":2121"));
}

/// Test FtpSourceInfo clone
#[test]
fn test_ftp_source_info_clone() {
    let original = FtpSourceInfo {
        url: "ftp://ftp.example.com/file.bin".to_string(),
        username: Some("user".to_string()),
        encrypted_password: Some("encrypted".to_string()),
    };

    let cloned = original.clone();

    assert_eq!(cloned.url, original.url);
    assert_eq!(cloned.username, original.username);
    assert_eq!(cloned.encrypted_password, original.encrypted_password);
}

/// Test multiple FtpSourceInfo in a Vec (as used in FileMetadata)
#[test]
fn test_multiple_ftp_sources() {
    let sources = vec![
        FtpSourceInfo {
            url: "ftp://mirror1.example.com/file.bin".to_string(),
            username: None,
            encrypted_password: None,
        },
        FtpSourceInfo {
            url: "ftp://mirror2.example.com/file.bin".to_string(),
            username: Some("user".to_string()),
            encrypted_password: Some("encrypted".to_string()),
        },
        FtpSourceInfo {
            url: "ftps://mirror3.example.com/file.bin".to_string(),
            username: Some("admin".to_string()),
            encrypted_password: Some("admin_encrypted".to_string()),
        },
    ];

    assert_eq!(sources.len(), 3);
    assert!(sources[0].username.is_none());
    assert!(sources[1].username.is_some());
    assert!(sources[2].url.starts_with("ftps://"));
}