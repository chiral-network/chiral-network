// peer_cache_test.rs
// Comprehensive tests for peer list persistence
//
// Tests cover:
// - Peer cache entry creation and validation
// - Cache serialization and deserialization
// - Stale peer filtering
// - Cache size limiting
// - File I/O operations
// - Edge cases and error handling

use chiral_network::peer_cache::{PeerCache, PeerCacheEntry, get_peer_cache_path};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use tokio;

#[test]
fn test_peer_cache_entry_from_metrics() {
    let entry = PeerCacheEntry::from_metrics(
        "12D3KooWTest123".to_string(),
        "/ip4/192.168.1.100/tcp/4001".to_string(),
        15,
        12,
        3,
        5242880,
        Some(50),
        0.85,
        1700000000,
        false,
        true,
    );
    
    assert_eq!(entry.peer_id, "12D3KooWTest123");
    assert_eq!(entry.addresses.len(), 1);
    assert_eq!(entry.addresses[0], "/ip4/192.168.1.100/tcp/4001");
    assert_eq!(entry.connection_count, 15);
    assert_eq!(entry.successful_transfers, 12);
    assert_eq!(entry.failed_transfers, 3);
    assert_eq!(entry.total_bytes_transferred, 5242880);
    assert_eq!(entry.average_latency_ms, 50);
    assert_eq!(entry.reliability_score, 0.85);
    assert!(!entry.is_bootstrap);
    assert!(entry.supports_relay);
}

#[test]
fn test_peer_cache_entry_is_stale() {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Fresh peer (1 hour ago)
    let fresh_entry = PeerCacheEntry {
        peer_id: "fresh".to_string(),
        addresses: vec![],
        last_seen: current_time - 3600,
        connection_count: 1,
        successful_transfers: 1,
        failed_transfers: 0,
        total_bytes_transferred: 1024,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    };
    
    assert!(!fresh_entry.is_stale(current_time));
    
    // Stale peer (8 days ago)
    let stale_entry = PeerCacheEntry {
        peer_id: "stale".to_string(),
        addresses: vec![],
        last_seen: current_time - (8 * 24 * 60 * 60),
        connection_count: 1,
        successful_transfers: 1,
        failed_transfers: 0,
        total_bytes_transferred: 1024,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    };
    
    assert!(stale_entry.is_stale(current_time));
}

#[test]
fn test_peer_cache_entry_merge_addresses() {
    let mut entry1 = PeerCacheEntry {
        peer_id: "peer1".to_string(),
        addresses: vec!["/ip4/1.1.1.1/tcp/4001".to_string()],
        last_seen: 1700000000,
        connection_count: 1,
        successful_transfers: 0,
        failed_transfers: 0,
        total_bytes_transferred: 0,
        average_latency_ms: 0,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.5,
    };
    
    let entry2 = PeerCacheEntry {
        peer_id: "peer1".to_string(),
        addresses: vec![
            "/ip4/1.1.1.1/tcp/4001".to_string(), // Duplicate
            "/ip4/2.2.2.2/tcp/4001".to_string(), // New
        ],
        last_seen: 1700000000,
        connection_count: 1,
        successful_transfers: 0,
        failed_transfers: 0,
        total_bytes_transferred: 0,
        average_latency_ms: 0,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.5,
    };
    
    entry1.merge_addresses(&entry2);
    
    assert_eq!(entry1.addresses.len(), 2);
    assert!(entry1.addresses.contains(&"/ip4/1.1.1.1/tcp/4001".to_string()));
    assert!(entry1.addresses.contains(&"/ip4/2.2.2.2/tcp/4001".to_string()));
}

#[test]
fn test_peer_cache_new() {
    let cache = PeerCache::new();
    
    assert_eq!(cache.version, 1);
    assert!(cache.peers.is_empty());
    assert!(cache.last_updated > 0);
}

#[test]
fn test_peer_cache_from_peers() {
    let entries = vec![
        PeerCacheEntry {
            peer_id: "peer1".to_string(),
            addresses: vec![],
            last_seen: 1700000000,
            connection_count: 1,
            successful_transfers: 1,
            failed_transfers: 0,
            total_bytes_transferred: 1024,
            average_latency_ms: 50,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.8,
        },
    ];
    
    let cache = PeerCache::from_peers(entries);
    
    assert_eq!(cache.version, 1);
    assert_eq!(cache.peers.len(), 1);
    assert_eq!(cache.peers[0].peer_id, "peer1");
}

#[test]
fn test_peer_cache_filter_stale_peers() {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut cache = PeerCache::new();
    
    // Add fresh peer
    cache.peers.push(PeerCacheEntry {
        peer_id: "fresh1".to_string(),
        addresses: vec![],
        last_seen: current_time - 3600, // 1 hour ago
        connection_count: 1,
        successful_transfers: 1,
        failed_transfers: 0,
        total_bytes_transferred: 1024,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    });
    
    // Add another fresh peer
    cache.peers.push(PeerCacheEntry {
        peer_id: "fresh2".to_string(),
        addresses: vec![],
        last_seen: current_time - 7200, // 2 hours ago
        connection_count: 1,
        successful_transfers: 1,
        failed_transfers: 0,
        total_bytes_transferred: 1024,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    });
    
    // Add stale peer
    cache.peers.push(PeerCacheEntry {
        peer_id: "stale".to_string(),
        addresses: vec![],
        last_seen: current_time - (8 * 24 * 60 * 60), // 8 days ago
        connection_count: 1,
        successful_transfers: 1,
        failed_transfers: 0,
        total_bytes_transferred: 1024,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    });
    
    cache.filter_stale_peers();
    
    assert_eq!(cache.peers.len(), 2);
    assert!(cache.peers.iter().all(|p| p.peer_id != "stale"));
}

#[test]
fn test_peer_cache_sort_and_limit() {
    let mut cache = PeerCache::new();
    
    // Add 150 peers with varying reliability scores
    for i in 0..150 {
        cache.peers.push(PeerCacheEntry {
            peer_id: format!("peer_{}", i),
            addresses: vec![],
            last_seen: 1700000000,
            connection_count: i,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: (i as f64) / 150.0,
        });
    }
    
    cache.sort_and_limit();
    
    // Should be limited to 100 peers
    assert_eq!(cache.peers.len(), 100);
    
    // Should be sorted by reliability score (highest first)
    for i in 1..cache.peers.len() {
        assert!(
            cache.peers[i - 1].reliability_score >= cache.peers[i].reliability_score,
            "Peers should be sorted by reliability score"
        );
    }
    
    // Highest scoring peers should be kept
    assert!(cache.peers[0].reliability_score > 0.9);
}

#[tokio::test]
async fn test_peer_cache_save_and_load() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("test_peer_cache.json");
    
    // Create cache with some peers
    let mut cache = PeerCache::new();
    cache.peers.push(PeerCacheEntry {
        peer_id: "12D3KooWTest".to_string(),
        addresses: vec!["/ip4/192.168.1.100/tcp/4001".to_string()],
        last_seen: 1700000000,
        connection_count: 10,
        successful_transfers: 8,
        failed_transfers: 2,
        total_bytes_transferred: 5242880,
        average_latency_ms: 45,
        is_bootstrap: false,
        supports_relay: true,
        reliability_score: 0.8,
    });
    
    // Save cache
    cache.save_to_file(&cache_path).await.unwrap();
    
    // Verify file exists
    assert!(cache_path.exists());
    
    // Load cache
    let loaded_cache = PeerCache::load_from_file(&cache_path).await.unwrap();
    
    // Verify loaded data
    assert_eq!(loaded_cache.version, 1);
    assert_eq!(loaded_cache.peers.len(), 1);
    assert_eq!(loaded_cache.peers[0].peer_id, "12D3KooWTest");
    assert_eq!(loaded_cache.peers[0].connection_count, 10);
    assert_eq!(loaded_cache.peers[0].successful_transfers, 8);
    assert_eq!(loaded_cache.peers[0].reliability_score, 0.8);
}

#[tokio::test]
async fn test_peer_cache_load_nonexistent_file() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("nonexistent.json");
    
    // Loading non-existent file should return empty cache
    let cache = PeerCache::load_from_file(&cache_path).await.unwrap();
    
    assert_eq!(cache.peers.len(), 0);
}

#[tokio::test]
async fn test_peer_cache_atomic_write() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("atomic_test.json");
    
    let cache = PeerCache::new();
    
    // Save should create temp file first, then rename
    cache.save_to_file(&cache_path).await.unwrap();
    
    // Final file should exist
    assert!(cache_path.exists());
    
    // Temp file should not exist
    let temp_path = cache_path.with_extension("tmp");
    assert!(!temp_path.exists());
}

#[tokio::test]
async fn test_peer_cache_delete_file() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("delete_test.json");
    
    // Create a cache file
    let cache = PeerCache::new();
    cache.save_to_file(&cache_path).await.unwrap();
    assert!(cache_path.exists());
    
    // Delete it
    PeerCache::delete_file(&cache_path).await.unwrap();
    assert!(!cache_path.exists());
    
    // Deleting non-existent file should not error
    PeerCache::delete_file(&cache_path).await.unwrap();
}

#[test]
fn test_peer_cache_serialization() {
    let cache = PeerCache {
        version: 1,
        last_updated: 1700000000,
        peers: vec![
            PeerCacheEntry {
                peer_id: "12D3KooWTest".to_string(),
                addresses: vec!["/ip4/192.168.1.100/tcp/4001".to_string()],
                last_seen: 1700000000,
                connection_count: 10,
                successful_transfers: 8,
                failed_transfers: 2,
                total_bytes_transferred: 5242880,
                average_latency_ms: 45,
                is_bootstrap: false,
                supports_relay: true,
                reliability_score: 0.8,
            },
        ],
    };
    
    // Serialize to JSON
    let json = serde_json::to_string_pretty(&cache).unwrap();
    assert!(json.contains("12D3KooWTest"));
    assert!(json.contains("192.168.1.100"));
    
    // Deserialize back
    let deserialized: PeerCache = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.version, 1);
    assert_eq!(deserialized.peers.len(), 1);
    assert_eq!(deserialized.peers[0].peer_id, "12D3KooWTest");
}

#[test]
fn test_peer_cache_edge_cases() {
    // Empty cache
    let mut empty_cache = PeerCache::new();
    empty_cache.filter_stale_peers();
    empty_cache.sort_and_limit();
    assert_eq!(empty_cache.peers.len(), 0);
    
    // Single peer
    let mut single_cache = PeerCache::new();
    single_cache.peers.push(PeerCacheEntry {
        peer_id: "single".to_string(),
        addresses: vec![],
        last_seen: 1700000000,
        connection_count: 1,
        successful_transfers: 0,
        failed_transfers: 0,
        total_bytes_transferred: 0,
        average_latency_ms: 0,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.5,
    });
    single_cache.sort_and_limit();
    assert_eq!(single_cache.peers.len(), 1);
}

#[tokio::test]
async fn test_peer_cache_corrupted_file() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("corrupted.json");
    
    // Write invalid JSON
    tokio::fs::write(&cache_path, "{ invalid json }").await.unwrap();
    
    // Loading should fail
    let result = PeerCache::load_from_file(&cache_path).await;
    assert!(result.is_err());
}

#[test]
fn test_get_peer_cache_path() {
    // Should successfully return a path
    let result = get_peer_cache_path();
    assert!(result.is_ok());
    
    let path = result.unwrap();
    assert!(path.to_string_lossy().contains("peer_cache.json"));
}
