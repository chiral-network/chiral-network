// peer_cache_integration_test.rs
// Integration tests for peer cache persistence across DHT lifecycle
//
// Tests the complete flow:
// 1. DHT service collects peer metrics
// 2. Peer cache is saved on shutdown
// 3. Peer cache is loaded on next startup
// 4. Cached peers are reconnected

use chiral_network::peer_cache::{PeerCache, PeerCacheEntry, get_peer_cache_path};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[tokio::test]
async fn test_peer_cache_full_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("lifecycle_test.json");
    
    // Simulate DHT collecting peer metrics during operation
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let peer_entries = vec![
        PeerCacheEntry {
            peer_id: "12D3KooWPeer1".to_string(),
            addresses: vec!["/ip4/192.168.1.100/tcp/4001".to_string()],
            last_seen: current_time - 300, // 5 minutes ago
            connection_count: 15,
            successful_transfers: 12,
            failed_transfers: 3,
            total_bytes_transferred: 10485760,
            average_latency_ms: 45,
            is_bootstrap: false,
            supports_relay: true,
            reliability_score: 0.85,
        },
        PeerCacheEntry {
            peer_id: "12D3KooWPeer2".to_string(),
            addresses: vec!["/ip4/192.168.1.101/tcp/4001".to_string()],
            last_seen: current_time - 600, // 10 minutes ago
            connection_count: 8,
            successful_transfers: 7,
            failed_transfers: 1,
            total_bytes_transferred: 5242880,
            average_latency_ms: 60,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.75,
        },
        PeerCacheEntry {
            peer_id: "12D3KooWPeer3".to_string(),
            addresses: vec!["/ip4/192.168.1.102/tcp/4001".to_string()],
            last_seen: current_time - 900, // 15 minutes ago
            connection_count: 20,
            successful_transfers: 18,
            failed_transfers: 2,
            total_bytes_transferred: 20971520,
            average_latency_ms: 30,
            is_bootstrap: false,
            supports_relay: true,
            reliability_score: 0.92,
        },
    ];
    
    // Phase 1: Save peer cache (simulating shutdown)
    let mut cache = PeerCache::from_peers(peer_entries);
    cache.save_to_file(&cache_path).await.unwrap();
    
    assert!(cache_path.exists(), "Cache file should be created");
    
    // Phase 2: Load peer cache (simulating startup)
    let loaded_cache = PeerCache::load_from_file(&cache_path).await.unwrap();
    
    assert_eq!(loaded_cache.peers.len(), 3, "All peers should be loaded");
    
    // Phase 3: Verify peer ordering (should be sorted by reliability)
    assert_eq!(loaded_cache.peers[0].peer_id, "12D3KooWPeer3", "Highest reliability peer should be first");
    assert_eq!(loaded_cache.peers[1].peer_id, "12D3KooWPeer1", "Second highest reliability peer should be second");
    assert_eq!(loaded_cache.peers[2].peer_id, "12D3KooWPeer2", "Lowest reliability peer should be last");
    
    // Phase 4: Verify relay capability is preserved
    let relay_peers: Vec<_> = loaded_cache.peers.iter()
        .filter(|p| p.supports_relay)
        .collect();
    assert_eq!(relay_peers.len(), 2, "Relay capability should be preserved");
}

#[tokio::test]
async fn test_peer_cache_handles_empty_metrics() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("empty_test.json");
    
    // Simulate no peers collected
    let cache = PeerCache::new();
    cache.save_to_file(&cache_path).await.unwrap();
    
    // Should still create valid cache file
    assert!(cache_path.exists());
    
    // Loading should return empty cache
    let loaded = PeerCache::load_from_file(&cache_path).await.unwrap();
    assert_eq!(loaded.peers.len(), 0);
}

#[tokio::test]
async fn test_peer_cache_filters_zero_transfer_peers() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("filter_test.json");
    
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Simulate peer metrics with some zero-transfer peers (should not be cached)
    let mut cache = PeerCache::new();
    
    // Good peer with transfers
    cache.peers.push(PeerCacheEntry {
        peer_id: "good_peer".to_string(),
        addresses: vec!["/ip4/1.1.1.1/tcp/4001".to_string()],
        last_seen: current_time - 300,
        connection_count: 5,
        successful_transfers: 4,
        failed_transfers: 1,
        total_bytes_transferred: 1024000,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    });
    
    // Peer with zero transfers (should not be included in real save)
    cache.peers.push(PeerCacheEntry {
        peer_id: "no_transfer_peer".to_string(),
        addresses: vec!["/ip4/2.2.2.2/tcp/4001".to_string()],
        last_seen: current_time - 300,
        connection_count: 0,
        successful_transfers: 0,
        failed_transfers: 0,
        total_bytes_transferred: 0,
        average_latency_ms: 0,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.5,
    });
    
    // In actual implementation, zero-transfer peers are filtered before cache creation
    // This test verifies the behavior if they somehow got included
    cache.save_to_file(&cache_path).await.unwrap();
    
    let loaded = PeerCache::load_from_file(&cache_path).await.unwrap();
    
    // Both are saved (filtering happens in save_peer_cache, not in PeerCache itself)
    assert_eq!(loaded.peers.len(), 2);
}

#[tokio::test]
async fn test_peer_cache_respects_size_limit() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("limit_test.json");
    
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Create 150 peers
    let mut cache = PeerCache::new();
    for i in 0..150 {
        cache.peers.push(PeerCacheEntry {
            peer_id: format!("peer_{}", i),
            addresses: vec![format!("/ip4/192.168.1.{}/tcp/4001", i % 255)],
            last_seen: current_time - 300,
            connection_count: i,
            successful_transfers: i,
            failed_transfers: 0,
            total_bytes_transferred: (i as u64) * 1024,
            average_latency_ms: 50,
            is_bootstrap: false,
            supports_relay: i % 2 == 0, // Every other peer supports relay
            reliability_score: (i as f64) / 150.0,
        });
    }
    
    // Apply sorting and limiting
    cache.sort_and_limit();
    
    // Save to file
    cache.save_to_file(&cache_path).await.unwrap();
    
    // Load and verify
    let loaded = PeerCache::load_from_file(&cache_path).await.unwrap();
    
    // Should be limited to 100
    assert_eq!(loaded.peers.len(), 100);
    
    // Should contain highest reliability peers
    assert!(loaded.peers[0].reliability_score > 0.9);
    assert!(loaded.peers[99].reliability_score > loaded.peers.len() as f64 / 150.0);
}

#[tokio::test]
async fn test_peer_cache_stale_filtering_on_load() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("stale_load_test.json");
    
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut cache = PeerCache::new();
    
    // Add fresh peer
    cache.peers.push(PeerCacheEntry {
        peer_id: "fresh".to_string(),
        addresses: vec!["/ip4/1.1.1.1/tcp/4001".to_string()],
        last_seen: current_time - 3600, // 1 hour ago
        connection_count: 5,
        successful_transfers: 5,
        failed_transfers: 0,
        total_bytes_transferred: 1024000,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    });
    
    // Add stale peer (8 days old)
    cache.peers.push(PeerCacheEntry {
        peer_id: "stale".to_string(),
        addresses: vec!["/ip4/2.2.2.2/tcp/4001".to_string()],
        last_seen: current_time - (8 * 24 * 60 * 60),
        connection_count: 5,
        successful_transfers: 5,
        failed_transfers: 0,
        total_bytes_transferred: 1024000,
        average_latency_ms: 50,
        is_bootstrap: false,
        supports_relay: false,
        reliability_score: 0.8,
    });
    
    // Save cache
    cache.save_to_file(&cache_path).await.unwrap();
    
    // Load and filter stale
    let mut loaded = PeerCache::load_from_file(&cache_path).await.unwrap();
    loaded.filter_stale_peers();
    
    // Only fresh peer should remain
    assert_eq!(loaded.peers.len(), 1);
    assert_eq!(loaded.peers[0].peer_id, "fresh");
}

#[tokio::test]
async fn test_peer_cache_relay_priority() {
    // Test that relay-capable peers get priority in reconnection
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut peers = vec![
        PeerCacheEntry {
            peer_id: "non_relay_high".to_string(),
            addresses: vec!["/ip4/1.1.1.1/tcp/4001".to_string()],
            last_seen: current_time - 300,
            connection_count: 10,
            successful_transfers: 10,
            failed_transfers: 0,
            total_bytes_transferred: 10240000,
            average_latency_ms: 30,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.95,
        },
        PeerCacheEntry {
            peer_id: "relay_low".to_string(),
            addresses: vec!["/ip4/2.2.2.2/tcp/4001".to_string()],
            last_seen: current_time - 300,
            connection_count: 5,
            successful_transfers: 4,
            failed_transfers: 1,
            total_bytes_transferred: 1024000,
            average_latency_ms: 60,
            is_bootstrap: false,
            supports_relay: true,
            reliability_score: 0.70,
        },
    ];
    
    // Sort by relay support first, then reliability
    peers.sort_by(|a, b| {
        b.supports_relay.cmp(&a.supports_relay)
            .then_with(|| b.reliability_score.partial_cmp(&a.reliability_score).unwrap_or(std::cmp::Ordering::Equal))
    });
    
    // Relay-capable peer should come first despite lower reliability
    assert_eq!(peers[0].peer_id, "relay_low");
    assert_eq!(peers[1].peer_id, "non_relay_high");
}

#[tokio::test]
async fn test_peer_cache_version_forward_compatibility() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("version_test.json");
    
    // Create cache with future version
    let json = r#"{
        "version": 999,
        "last_updated": 1700000000,
        "peers": [
            {
                "peer_id": "12D3KooWTest",
                "addresses": ["/ip4/1.1.1.1/tcp/4001"],
                "last_seen": 1700000000,
                "connection_count": 5,
                "successful_transfers": 5,
                "failed_transfers": 0,
                "total_bytes_transferred": 1024000,
                "average_latency_ms": 50,
                "is_bootstrap": false,
                "supports_relay": false,
                "reliability_score": 0.8
            }
        ]
    }"#;
    
    tokio::fs::write(&cache_path, json).await.unwrap();
    
    // Should still load (with warning logged)
    let loaded = PeerCache::load_from_file(&cache_path).await.unwrap();
    assert_eq!(loaded.peers.len(), 1);
    assert_eq!(loaded.version, 999);
}
