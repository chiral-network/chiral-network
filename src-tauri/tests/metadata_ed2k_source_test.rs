// Import the structs and error from the dht module
use chiral_network::dht::{Ed2kError, Ed2kSourceInfo, FileMetadata};
use serde_json;

#[test]
fn test_serialize_ed2k_source_info() {
    let info = Ed2kSourceInfo {
        server_url: "ed2k://|server|1.2.3.4|4661|/".to_string(),
        file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
        file_size: 123456789,
        file_name: Some("test.iso".to_string()),
        sources: Some(vec!["192.168.1.1:4000".to_string()]),
        timeout: Some(60),
    };

    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"server_url\":\"ed2k://|server|1.2.3.4|4661|/\""));
    assert!(json.contains("\"file_hash\":\"31D6CFE0D16AE931B73C59D7E0C089C0\""));
    assert!(json.contains("\"file_size\":123456789"));
    assert!(json.contains("\"file_name\":\"test.iso\""));
    assert!(json.contains("\"sources\":[\"192.168.1.1:4000\"]"));
    assert!(json.contains("\"timeout\":60"));
}

#[test]
fn test_deserialize_ed2k_source_info() {
    let json = r#"
    {
        "server_url": "ed2k://|server|1.2.3.4|4661|/",
        "file_hash": "31D6CFE0D16AE931B73C59D7E0C089C0",
        "file_size": 123456789,
        "file_name": "test.iso",
        "sources": ["192.168.1.1:4000"],
        "timeout": 60
    }
    "#;

    let info: Ed2kSourceInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.server_url, "ed2k://|server|1.2.3.4|4661|/");
    assert_eq!(info.file_hash, "31D6CFE0D16AE931B73C59D7E0C089C0");
    assert_eq!(info.file_size, 123456789);
    assert_eq!(info.file_name, Some("test.iso".to_string()));
    assert_eq!(info.sources, Some(vec!["192.168.1.1:4000".to_string()]));
    assert_eq!(info.timeout, Some(60));
}

#[test]
fn test_parse_ed2k_file_link() {
    let link = "ed2k://|file|ubuntu-22.04.iso|3774873600|31D6CFE0D16AE931B73C59D7E0C089C0|/";
    let info = Ed2kSourceInfo::from_ed2k_link(link).unwrap();

    assert_eq!(info.file_name, Some("ubuntu-22.04.iso".to_string()));
    assert_eq!(info.file_size, 3774873600);
    assert_eq!(info.file_hash, "31D6CFE0D16AE931B73C59D7E0C089C0");
    assert!(info.server_url.is_empty()); // File links don't have a server URL
    assert!(info.sources.is_none());
}

#[test]
fn test_parse_ed2k_server_link() {
    let link = "ed2k://|server|176.103.48.36|4661|/";
    let info = Ed2kSourceInfo::from_ed2k_link(link).unwrap();

    assert_eq!(info.server_url, "ed2k://|server|176.103.48.36|4661|/");
    assert!(info.file_hash.is_empty()); // Server links don't have file info
    assert_eq!(info.file_size, 0);
    assert!(info.file_name.is_none());
}

#[test]
fn test_parse_invalid_ed2k_links() {
    // Malformed prefix
    let link1 = "http://|file|name|123|hash|/";
    assert_eq!(
        Ed2kSourceInfo::from_ed2k_link(link1).unwrap_err(),
        Ed2kError::InvalidLink(link1.to_string())
    );

    // Unknown type
    let link2 = "ed2k://|folder|name|123|hash|/";
    assert_eq!(
        Ed2kSourceInfo::from_ed2k_link(link2).unwrap_err(),
        Ed2kError::InvalidLink("Unknown link type: folder".to_string())
    );

    // File link missing parts
    let link3 = "ed2k://|file|name|123|/";
    assert_eq!(
        Ed2kSourceInfo::from_ed2k_link(link3).unwrap_err(),
        Ed2kError::MissingPart("File link requires name, size, and hash")
    );

    // Server link missing parts
    let link4 = "ed2k://|server|1.2.3.4|/";
    assert_eq!(
        Ed2kSourceInfo::from_ed2k_link(link4).unwrap_err(),
        Ed2kError::MissingPart("Server link requires ip and port")
    );
    
    // Invalid file size
    let link5 = "ed2k://|file|name|notanumber|hash|/";
     assert_eq!(
        Ed2kSourceInfo::from_ed2k_link(link5).unwrap_err(),
        Ed2kError::InvalidFileSize("notanumber".to_string())
    );
}

#[test]
fn test_file_metadata_with_ed2k_sources() {
    let ed2k_info = Ed2kSourceInfo::from_ed2k_link(
        "ed2k://|file|test.iso|12345|HASH123|/"
    ).unwrap();
    
    let metadata = FileMetadata {
        merkle_root: "merkle_root_hash".to_string(),
        file_name: "test.iso".to_string(),
        file_size: 12345,
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
        info_hash: None,
        trackers: None,
        is_root: true,
        download_path: None,
        price: None,
        uploader_address: None,
        ed2k_sources: Some(vec![ed2k_info]), 
    };

    let json = serde_json::to_string(&metadata).unwrap();
    assert!(json.contains("\"ed2kSources\":["));
    assert!(json.contains("\"file_hash\":\"HASH123\""));

    let deserialized: FileMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.ed2k_sources.unwrap().len(), 1);
}

#[test]
fn test_multiple_ed2k_sources() {
    let ed2k_info1 = Ed2kSourceInfo::from_ed2k_link(
        "ed2k://|file|test.iso|12345|HASH123|/"
    ).unwrap();
    // This isn't a valid link, but a valid struct
    let ed2k_info2 = Ed2kSourceInfo {
        server_url: "ed2k://|server|1.2.3.4|4661|/".to_string(),
        file_hash: "HASH123".to_string(),
        file_size: 12345,
        file_name: Some("test.iso".to_string()),
        sources: None, 
        timeout: None,
    };

    let metadata = FileMetadata {
        merkle_root: "merkle_root_hash".to_string(),
        file_name: "test.iso".to_string(),
        file_size: 12345,
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
        info_hash: None,
        trackers: None,
        is_root: true,
        download_path: None,
        price: None,
        uploader_address: None,
        ed2k_sources: Some(vec![ed2k_info1, ed2k_info2]),
    };
    
    let json = serde_json::to_string(&metadata).unwrap();
    let deserialized: FileMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.ed2k_sources.unwrap().len(), 2);
}

#[test]
fn test_empty_none_source_lists() {
    let metadata_empty = FileMetadata {
        merkle_root: "merkle_root_hash".to_string(),
        ed2k_sources: Some(vec![]), // Empty list
        file_name: String::new(), file_size: 0, file_data: vec![], seeders: vec![], created_at: 0,
        mime_type: None, is_encrypted: false, encryption_method: None, key_fingerprint: None,
        parent_hash: None, cids: None, encrypted_key_bundle: None,
        ftp_sources: None, http_sources: None, info_hash: None, trackers: None, is_root: true,
        download_path: None, price: None, uploader_address: None,
    };
    let metadata_none = FileMetadata {
        merkle_root: "merkle_root_hash".to_string(),
        ed2k_sources: None, // None
        file_name: String::new(), file_size: 0, file_data: vec![], seeders: vec![], created_at: 0,
        mime_type: None, is_encrypted: false, encryption_method: None, key_fingerprint: None,
        parent_hash: None, cids: None, encrypted_key_bundle: None,
        ftp_sources: None, http_sources: None, info_hash: None, trackers: None, is_root: true,
        download_path: None, price: None, uploader_address: None,
    };
    let json_empty = serde_json::to_string(&metadata_empty).unwrap();
    assert!(json_empty.contains("\"ed2kSources\":[]"));

    let json_none = serde_json::to_string(&metadata_none).unwrap();
    assert!(!json_none.contains("\"ed2kSources\":"));
}

#[test]
fn test_source_deduplication() {
    let info1 = Ed2kSourceInfo { server_url: "server1".to_string(), file_hash: String::new(), file_size: 0, file_name: None, sources: None, timeout: None };
    let info2 = Ed2kSourceInfo { server_url: "server2".to_string(), file_hash: String::new(), file_size: 0, file_name: None, sources: None, timeout: None };
    let info1_dup = Ed2kSourceInfo { server_url: "server1".to_string(), file_hash: String::new(), file_size: 0, file_name: None, sources: None, timeout: None };

    let mut sources = vec![info1, info2, info1_dup];
    
    // Simple deduplication logic 
    sources.sort_by(|a, b| a.server_url.cmp(&b.server_url));
    sources.dedup_by(|a, b| a.server_url == b.server_url);
    
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].server_url, "server1");
    assert_eq!(sources[1].server_url, "server2");
}

#[test]
fn test_clone_and_equality() {
    let info1 = Ed2kSourceInfo::from_ed2k_link(
        "ed2k://|file|test.iso|12345|HASH123|/"
    ).unwrap();
    let info2 = info1.clone();
    
    // We didn't derive PartialEq, but we can check fields
    assert_eq!(info1.file_hash, info2.file_hash);
    assert_eq!(info1.file_size, info2.file_size);
    assert_eq!(info1.file_name, info2.file_name);
}

#[test]
fn test_dht_storage_round_trip() {
    // This simulates storing in DHT and retrieving
    let info = Ed2kSourceInfo {
        server_url: "server1".to_string(),
        file_hash: "HASH123".to_string(),
        file_size: 12345,
        file_name: Some("test.iso".to_string()),
        sources: Some(vec!["1.1.1.1:1".to_string()]),
        timeout: None,
    };

    let metadata_in = FileMetadata {
        merkle_root: "merkle".to_string(),
        ed2k_sources: Some(vec![info]),
        file_name: "test.iso".to_string(), file_size: 12345, file_data: vec![], seeders: vec![], created_at: 0,
        mime_type: None, is_encrypted: false, encryption_method: None, key_fingerprint: None,
        parent_hash: None, cids: None, encrypted_key_bundle: None,
        ftp_sources: None, http_sources: None, info_hash: None, trackers: None, is_root: true,
        download_path: None, price: None, uploader_address: None,
    };

    let json_data = serde_json::to_vec(&metadata_in).unwrap();
    // ... (simulate storing json_data in DHT) ...
    // ... (simulate retrieving json_data from DHT) ...
    let metadata_out: FileMetadata = serde_json::from_slice(&json_data).unwrap();

    assert_eq!(metadata_in.merkle_root, metadata_out.merkle_root);
    let sources_out = metadata_out.ed2k_sources.unwrap();
    assert_eq!(sources_out.len(), 1);
    assert_eq!(sources_out[0].file_hash, "HASH123");
    assert_eq!(sources_out[0].sources.as_ref().unwrap()[0], "1.1.1.1:1");
}

#[test]
fn test_legacy_compatibility_missing_fields() {
    // Simulate loading old data that doesn't have the `ed2k_sources` field
    let old_json = r#"
    {
        "merkleRoot": "merkle_root_hash",
        "fileName": "legacy_file.dat",
        "fileSize": 1000,
        "seeders": [],
        "createdAt": 12345,
        "isEncrypted": false,
        "isRoot": true
    }
    "#;

    let metadata: FileMetadata = serde_json::from_str(old_json).unwrap();
    
    // The `ed2k_sources` field should be `None`
    assert!(metadata.ed2k_sources.is_none());
    assert_eq!(metadata.file_name, "legacy_file.dat");
}