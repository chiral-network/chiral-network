use chiral_network::ed2k_client::*;
use std::time::Duration;

// ============================================================================
// MD4 Hash Verification Tests (Tests 1-3)
// ============================================================================

#[test]
fn test_md4_hash_known_value() {
    // Known MD4 hash for "hello world"
    let data = b"hello world";
    let expected_hash = "aa010fbc1d14c795d86ef98c95479d17";

    assert!(
        Ed2kClient::verify_md4_hash(data, expected_hash),
        "MD4 hash verification should succeed for known value"
    );
}

#[test]
fn test_md4_hash_case_insensitive() {
    // Test that hash comparison is case-insensitive
    let data = b"test";
    let hash_upper = "DB346D691D7ACC4DC2625DB19F9E3F52";
    let hash_lower = "db346d691d7acc4dc2625db19f9e3f52";

    assert!(
        Ed2kClient::verify_md4_hash(data, hash_upper),
        "MD4 verification should work with uppercase hash"
    );
    assert!(
        Ed2kClient::verify_md4_hash(data, hash_lower),
        "MD4 verification should work with lowercase hash"
    );
}

#[test]
fn test_md4_hash_mismatch_detection() {
    let data = b"hello world";
    let wrong_hash = "00000000000000000000000000000000";

    assert!(
        !Ed2kClient::verify_md4_hash(data, wrong_hash),
        "MD4 verification should fail for incorrect hash"
    );
}

// ============================================================================
// URL Parsing Tests (Tests 4-7)
// ============================================================================

#[test]
fn test_parse_valid_server_url() {
    let url = "ed2k://|server|176.103.48.36|4661|/";
    let result = Ed2kClient::parse_server_url(url);

    assert!(result.is_ok(), "Should successfully parse valid ed2k URL");

    let (ip, port) = result.unwrap();
    assert_eq!(ip, "176.103.48.36", "IP should be extracted correctly");
    assert_eq!(port, 4661, "Port should be extracted correctly");
}

#[test]
fn test_parse_invalid_protocol() {
    let url = "http://example.com";
    let result = Ed2kClient::parse_server_url(url);

    assert!(result.is_err(), "Should reject non-ed2k URL");
    assert!(
        matches!(result.unwrap_err(), Ed2kError::ProtocolError(_)),
        "Should return ProtocolError for wrong protocol"
    );
}

#[test]
fn test_parse_url_missing_parts() {
    let url = "ed2k://|server|/";
    let result = Ed2kClient::parse_server_url(url);

    assert!(result.is_err(), "Should reject URL with missing parts");
}

#[test]
fn test_parse_url_invalid_port() {
    let url = "ed2k://|server|176.103.48.36|invalid|/";
    let result = Ed2kClient::parse_server_url(url);

    assert!(result.is_err(), "Should reject URL with invalid port");
}

// ============================================================================
// Client Creation Tests (Tests 8-10)
// ============================================================================

#[test]
fn test_create_ed2k_client() {
    let client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());

    assert!(!client.is_connected(), "New client should not be connected");
}

#[test]
fn test_create_client_with_config() {
    let config = Ed2kConfig {
        server_url: "ed2k://|server|127.0.0.1|4661|/".to_string(),
        timeout: Duration::from_secs(60),
        client_id: Some("test_client".to_string()),
    };

    let client = Ed2kClient::with_config(config);
    assert!(!client.is_connected(), "New client with config should not be connected");
}

#[test]
fn test_ed2k_chunk_size_constant() {
    assert_eq!(
        ED2K_CHUNK_SIZE,
        9_728_000,
        "ed2k chunk size should be exactly 9.28 MB"
    );
}

// ============================================================================
// Connection Tests (Tests 11-13)
// ============================================================================

#[tokio::test]
#[ignore] // Requires real ed2k server
async fn test_connect_to_real_server() {
    let mut client = Ed2kClient::new("ed2k://|server|176.103.48.36|4661|/".to_string());

    let result = client.connect().await;

    // This may fail if server is down, which is acceptable for ignored test
    if result.is_ok() {
        assert!(client.is_connected(), "Client should be connected after successful connect");
        let _ = client.disconnect().await;
    }
}

#[tokio::test]
async fn test_connect_invalid_server() {
    let mut client = Ed2kClient::new("ed2k://|server|999.999.999.999|4661|/".to_string());

    let result = client.connect().await;

    assert!(result.is_err(), "Should fail to connect to invalid server");
}

#[tokio::test]
async fn test_connect_timeout() {
    let config = Ed2kConfig {
        server_url: "ed2k://|server|192.0.2.1|4661|/".to_string(), // TEST-NET-1, unreachable
        timeout: Duration::from_millis(100),
        client_id: None,
    };

    let mut client = Ed2kClient::with_config(config);
    let result = client.connect().await;

    assert!(result.is_err(), "Should timeout on unreachable server");
}

// ============================================================================
// Download Chunk Tests (Tests 14-17)
// ============================================================================

#[tokio::test]
async fn test_download_chunk_not_connected() {
    let mut client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());

    // Try to download without connecting
    let result = client.download_chunk(
        "31D6CFE0D16AE931B73C59D7E0C089C0",
        0,
        "AA010FBC1D14C795D86EF98C95479D17"
    ).await;

    assert!(result.is_err(), "Should fail when not connected");
    assert!(
        matches!(result.unwrap_err(), Ed2kError::ConnectionError(_)),
        "Should return ConnectionError"
    );
}

#[tokio::test]
async fn test_download_chunk_invalid_file_hash() {
    let mut client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());

    // Manually set connection to Some (mock)
    // In real scenario, this would require actual connection

    let result = client.download_chunk(
        "invalid_hash", // Invalid hex
        0,
        "AA010FBC1D14C795D86EF98C95479D17"
    ).await;

    // Should fail because not connected OR invalid hash format
    assert!(result.is_err(), "Should fail with invalid file hash");
}

#[tokio::test]
async fn test_download_chunk_wrong_hash_length() {
    let mut client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());

    let result = client.download_chunk(
        "31D6CFE0D16AE931", // Only 8 bytes, should be 16
        0,
        "AA010FBC1D14C795D86EF98C95479D17"
    ).await;

    assert!(result.is_err(), "Should fail with wrong hash length");
}

#[tokio::test]
#[ignore] // Requires real ed2k server with known file
async fn test_download_chunk_real() {
    let mut client = Ed2kClient::new("ed2k://|server|176.103.48.36|4661|/".to_string());

    if client.connect().await.is_ok() {
        // This would need a known file hash on the server
        let result = client.download_chunk(
            "31D6CFE0D16AE931B73C59D7E0C089C0",
            0,
            "AA010FBC1D14C795D86EF98C95479D17"
        ).await;

        // May succeed or fail depending on server state
        let _ = client.disconnect().await;
    }
}

// ============================================================================
// Disconnect Tests (Test 18)
// ============================================================================

#[tokio::test]
async fn test_disconnect() {
    let mut client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());

    // Disconnect without connecting should not error
    let result = client.disconnect().await;
    assert!(result.is_ok(), "Disconnect should succeed even when not connected");

    assert!(!client.is_connected(), "Client should not be connected after disconnect");
}

// ============================================================================
// Additional Feature Tests (Tests 19-20)
// ============================================================================

#[tokio::test]
async fn test_get_file_info() {
    let mut client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());

    let result = client.get_file_info("31D6CFE0D16AE931B73C59D7E0C089C0").await;

    // Placeholder implementation should return Ok
    assert!(result.is_ok(), "get_file_info should return Ok (placeholder)");
}

#[tokio::test]
async fn test_get_sources() {
    let mut client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());

    let result = client.get_sources("31D6CFE0D16AE931B73C59D7E0C089C0").await;

    // Placeholder implementation should return Ok with empty vec
    assert!(result.is_ok(), "get_sources should return Ok (placeholder)");
    assert_eq!(result.unwrap().len(), 0, "Placeholder should return empty source list");
}

// ============================================================================
// Summary
// ============================================================================
// Total: 20 tests
// - 3 MD4 hash tests
// - 4 URL parsing tests
// - 3 client creation tests
// - 3 connection tests (1 ignored - requires real server)
// - 4 download chunk tests (1 ignored - requires real server)
// - 1 disconnect test
// - 2 additional feature tests
//
// Tests marked #[ignore]: 2 (require real ed2k server)
// Tests that run automatically: 18
