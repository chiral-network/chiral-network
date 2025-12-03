// peer_cache.rs
// Peer list persistence for improved startup performance and network stability
//
// This module implements the peer cache system described in:
// docs/proposals/peer-list-persistence-proposal.md
//
// Key features:
// - Save peer connection data to disk on shutdown
// - Load cached peers on startup for faster reconnection
// - Filter stale peers (older than 7 days)
// - Limit cache size to top 100 peers by reliability score
// - Human-readable JSON format for debugging

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Schema version for forward compatibility
const CACHE_VERSION: u32 = 1;

/// Maximum number of peers to cache
const MAX_CACHED_PEERS: usize = 100;

/// Maximum age of cached peers in seconds (7 days)
const MAX_PEER_AGE_SECS: u64 = 7 * 24 * 60 * 60;

/// A single peer entry in the cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCacheEntry {
    /// Peer ID in libp2p format
    pub peer_id: String,
    
    /// List of known multiaddresses for this peer
    pub addresses: Vec<String>,
    
    /// Last time we saw this peer (Unix timestamp)
    pub last_seen: u64,
    
    /// Number of times we've connected to this peer
    pub connection_count: u32,
    
    /// Number of successful file transfers
    pub successful_transfers: u32,
    
    /// Number of failed file transfers
    pub failed_transfers: u32,
    
    /// Total bytes transferred to/from this peer
    pub total_bytes_transferred: u64,
    
    /// Average latency in milliseconds
    pub average_latency_ms: u32,
    
    /// Whether this peer is a bootstrap node
    pub is_bootstrap: bool,
    
    /// Whether this peer supports circuit relay
    pub supports_relay: bool,
    
    /// Computed reliability score (0.0 to 1.0)
    pub reliability_score: f64,
}

impl PeerCacheEntry {
    /// Create a new peer cache entry from peer metrics
    pub fn from_metrics(
        peer_id: String,
        address: String,
        connection_count: u32,
        successful_transfers: u32,
        failed_transfers: u32,
        total_bytes_transferred: u64,
        average_latency_ms: Option<u64>,
        reliability_score: f64,
        last_seen: u64,
        is_bootstrap: bool,
        supports_relay: bool,
    ) -> Self {
        Self {
            peer_id,
            addresses: vec![address],
            last_seen,
            connection_count,
            successful_transfers,
            failed_transfers,
            total_bytes_transferred,
            average_latency_ms: average_latency_ms.unwrap_or(0) as u32,
            is_bootstrap,
            supports_relay,
            reliability_score,
        }
    }
    
    /// Check if this peer entry is stale (older than MAX_PEER_AGE_SECS)
    pub fn is_stale(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.last_seen) > MAX_PEER_AGE_SECS
    }
    
    /// Merge another peer cache entry's addresses into this one
    pub fn merge_addresses(&mut self, other: &PeerCacheEntry) {
        for addr in &other.addresses {
            if !self.addresses.contains(addr) {
                self.addresses.push(addr.clone());
            }
        }
    }
}

/// The peer cache containing all cached peers
#[derive(Debug, Serialize, Deserialize)]
pub struct PeerCache {
    /// Schema version for forward compatibility
    pub version: u32,
    
    /// Last time the cache was updated (Unix timestamp)
    pub last_updated: u64,
    
    /// List of cached peers
    pub peers: Vec<PeerCacheEntry>,
}

impl PeerCache {
    /// Create a new empty peer cache
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs();
        
        Self {
            version: CACHE_VERSION,
            last_updated: now,
            peers: Vec::new(),
        }
    }
    
    /// Create a peer cache from a list of entries
    pub fn from_peers(peers: Vec<PeerCacheEntry>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs();
        
        Self {
            version: CACHE_VERSION,
            last_updated: now,
            peers,
        }
    }
    
    /// Filter and return only non-stale peers
    pub fn filter_stale_peers(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_secs();
        
        let initial_count = self.peers.len();
        self.peers.retain(|peer| !peer.is_stale(now));
        let removed = initial_count - self.peers.len();
        
        if removed > 0 {
            info!("Filtered {} stale peers from cache", removed);
        }
    }
    
    /// Sort peers by reliability score (highest first) and limit to MAX_CACHED_PEERS
    pub fn sort_and_limit(&mut self) {
        // Sort by reliability score descending, then by connection count
        self.peers.sort_by(|a, b| {
            b.reliability_score
                .partial_cmp(&a.reliability_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.connection_count.cmp(&a.connection_count))
        });
        
        // Limit to MAX_CACHED_PEERS
        if self.peers.len() > MAX_CACHED_PEERS {
            let removed = self.peers.len() - MAX_CACHED_PEERS;
            self.peers.truncate(MAX_CACHED_PEERS);
            debug!("Limited cache to {} peers (removed {})", MAX_CACHED_PEERS, removed);
        }
    }
    
    /// Save the peer cache to a JSON file with atomic write
    pub async fn save_to_file(&self, path: &Path) -> Result<(), String> {
        // Serialize to JSON with pretty printing for debugging
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize peer cache: {}", e))?;
        
        // Use atomic write pattern: write to temp file, then rename
        let temp_path = path.with_extension("tmp");
        
        tokio::fs::write(&temp_path, json)
            .await
            .map_err(|e| format!("Failed to write peer cache to temp file: {}", e))?;
        
        tokio::fs::rename(&temp_path, path)
            .await
            .map_err(|e| format!("Failed to rename peer cache file: {}", e))?;
        
        info!("Saved {} peers to cache at {:?}", self.peers.len(), path);
        Ok(())
    }
    
    /// Load a peer cache from a JSON file
    pub async fn load_from_file(path: &Path) -> Result<Self, String> {
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            debug!("Peer cache file does not exist: {:?}", path);
            return Ok(Self::new());
        }
        
        let json = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read peer cache file: {}", e))?;
        
        let cache: Self = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse peer cache JSON: {}", e))?;
        
        // Validate version
        if cache.version > CACHE_VERSION {
            warn!(
                "Peer cache version {} is newer than supported version {}",
                cache.version, CACHE_VERSION
            );
        }
        
        info!("Loaded {} peers from cache at {:?}", cache.peers.len(), path);
        Ok(cache)
    }
    
    /// Delete the peer cache file (used when cache is corrupted)
    pub async fn delete_file(path: &Path) -> Result<(), String> {
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            tokio::fs::remove_file(path)
                .await
                .map_err(|e| format!("Failed to delete peer cache file: {}", e))?;
            warn!("Deleted corrupted peer cache at {:?}", path);
        }
        Ok(())
    }
    
    /// Get statistics about the peer cache
    pub fn get_stats(&self) -> PeerCacheStats {
        let relay_count = self.peers.iter().filter(|p| p.supports_relay).count();
        let bootstrap_count = self.peers.iter().filter(|p| p.is_bootstrap).count();
        
        let avg_reliability = if !self.peers.is_empty() {
            self.peers.iter().map(|p| p.reliability_score).sum::<f64>() / self.peers.len() as f64
        } else {
            0.0
        };
        
        let total_transfers = self.peers.iter()
            .map(|p| p.successful_transfers as u64 + p.failed_transfers as u64)
            .sum();
        
        let total_bytes = self.peers.iter()
            .map(|p| p.total_bytes_transferred)
            .sum();
        
        PeerCacheStats {
            total_peers: self.peers.len(),
            relay_capable_peers: relay_count,
            bootstrap_peers: bootstrap_count,
            average_reliability: avg_reliability,
            total_transfers,
            total_bytes_transferred: total_bytes,
        }
    }
}

/// Statistics about the peer cache
#[derive(Debug, Clone)]
pub struct PeerCacheStats {
    pub total_peers: usize,
    pub relay_capable_peers: usize,
    pub bootstrap_peers: usize,
    pub average_reliability: f64,
    pub total_transfers: u64,
    pub total_bytes_transferred: u64,
}

impl Default for PeerCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the path to the peer cache file
pub fn get_peer_cache_path() -> Result<PathBuf, String> {
    use directories::ProjectDirs;
    
    let proj_dirs = ProjectDirs::from("com", "chiral-network", "chiral-network")
        .ok_or("Failed to get project directories")?;
    
    let data_dir = proj_dirs.data_dir();
    
    // Ensure the data directory exists
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("Failed to create data directory: {}", e))?;
    
    Ok(data_dir.join("peer_cache.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_peer_cache_entry_creation() {
        let entry = PeerCacheEntry::from_metrics(
            "12D3KooWTest".to_string(),
            "/ip4/192.168.1.100/tcp/4001".to_string(),
            10,
            8,
            2,
            1024000,
            Some(50),
            0.8,
            1700000000,
            false,
            true,
        );
        
        assert_eq!(entry.peer_id, "12D3KooWTest");
        assert_eq!(entry.connection_count, 10);
        assert_eq!(entry.successful_transfers, 8);
        assert_eq!(entry.reliability_score, 0.8);
    }
    
    #[test]
    fn test_peer_cache_entry_is_stale() {
        let old_entry = PeerCacheEntry {
            peer_id: "old_peer".to_string(),
            addresses: vec![],
            last_seen: 1000000, // Very old timestamp
            connection_count: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.5,
        };
        
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        assert!(old_entry.is_stale(current_time));
    }
    
    #[test]
    fn test_peer_cache_sort_and_limit() {
        let mut cache = PeerCache::new();
        
        // Create 150 peers with varying reliability scores
        for i in 0..150 {
            let entry = PeerCacheEntry {
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
                reliability_score: (i as f64) / 150.0, // Scores from 0.0 to 1.0
            };
            cache.peers.push(entry);
        }
        
        cache.sort_and_limit();
        
        // Should be limited to MAX_CACHED_PEERS
        assert_eq!(cache.peers.len(), MAX_CACHED_PEERS);
        
        // Should be sorted by reliability score (highest first)
        for i in 1..cache.peers.len() {
            assert!(
                cache.peers[i - 1].reliability_score >= cache.peers[i].reliability_score,
                "Peers should be sorted by reliability score"
            );
        }
    }
    
    #[test]
    fn test_peer_cache_filter_stale() {
        let mut cache = PeerCache::new();
        
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Add fresh peer
        cache.peers.push(PeerCacheEntry {
            peer_id: "fresh_peer".to_string(),
            addresses: vec![],
            last_seen: current_time - 3600, // 1 hour ago
            connection_count: 1,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.5,
        });
        
        // Add stale peer
        cache.peers.push(PeerCacheEntry {
            peer_id: "stale_peer".to_string(),
            addresses: vec![],
            last_seen: current_time - (8 * 24 * 60 * 60), // 8 days ago
            connection_count: 1,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.5,
        });
        
        cache.filter_stale_peers();
        
        // Only fresh peer should remain
        assert_eq!(cache.peers.len(), 1);
        assert_eq!(cache.peers[0].peer_id, "fresh_peer");
    }
    
    #[test]
    fn test_peer_cache_stats() {
        let mut cache = PeerCache::new();
        
        // Add some peers with different characteristics
        cache.peers.push(PeerCacheEntry {
            peer_id: "relay1".to_string(),
            addresses: vec![],
            last_seen: 1700000000,
            connection_count: 10,
            successful_transfers: 8,
            failed_transfers: 2,
            total_bytes_transferred: 1024000,
            average_latency_ms: 50,
            is_bootstrap: false,
            supports_relay: true,
            reliability_score: 0.8,
        });
        
        cache.peers.push(PeerCacheEntry {
            peer_id: "bootstrap1".to_string(),
            addresses: vec![],
            last_seen: 1700000000,
            connection_count: 20,
            successful_transfers: 18,
            failed_transfers: 2,
            total_bytes_transferred: 2048000,
            average_latency_ms: 30,
            is_bootstrap: true,
            supports_relay: false,
            reliability_score: 0.9,
        });
        
        cache.peers.push(PeerCacheEntry {
            peer_id: "regular1".to_string(),
            addresses: vec![],
            last_seen: 1700000000,
            connection_count: 5,
            successful_transfers: 4,
            failed_transfers: 1,
            total_bytes_transferred: 512000,
            average_latency_ms: 60,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.7,
        });
        
        let stats = cache.get_stats();
        
        assert_eq!(stats.total_peers, 3);
        assert_eq!(stats.relay_capable_peers, 1);
        assert_eq!(stats.bootstrap_peers, 1);
        assert_eq!(stats.total_transfers, 10 + 20 + 5);
        assert_eq!(stats.total_bytes_transferred, 1024000 + 2048000 + 512000);
        
        // Average reliability should be (0.8 + 0.9 + 0.7) / 3 ≈ 0.8
        assert!((stats.average_reliability - 0.8).abs() < 0.01);
    }
    
    #[test]
    fn test_peer_cache_stats_empty() {
        let cache = PeerCache::new();
        let stats = cache.get_stats();
        
        assert_eq!(stats.total_peers, 0);
        assert_eq!(stats.relay_capable_peers, 0);
        assert_eq!(stats.bootstrap_peers, 0);
        assert_eq!(stats.average_reliability, 0.0);
        assert_eq!(stats.total_transfers, 0);
        assert_eq!(stats.total_bytes_transferred, 0);
    }
}
