# Peer List Persistence Proposal

**Status:** Draft  
**Author:** Development Team  
**Date:** November 18, 2025  
**Priority:** High  
**Complexity:** Low  

---

## Problem Statement

Currently, when the Chiral Network application closes, **all peer connection data is lost forever**. On every startup, the node must:

1. Connect to bootstrap nodes from scratch
2. Rediscover all peers through DHT queries
3. Rebuild the entire routing table
4. Re-establish all connections

This creates several critical issues:

### User Impact
- **Slow startup times** - 30-60 seconds to rediscover peers
- **Inconsistent experience** - Different peers on each session
- **Lost reputation data** - No memory of reliable vs. unreliable peers
- **Wasted bandwidth** - Repeated discovery queries

### Network Impact
- **Bootstrap node strain** - Every restart hits bootstrap nodes
- **Discovery overhead** - Redundant DHT queries for known peers
- **Network churn** - Unstable peer connections across restarts

### Missing Functionality
- No persistent peer history
- No connection quality metrics across sessions
- No peer reliability tracking over time
- No preferred peer lists

---

## Proposed Solution

Implement **peer list persistence** by saving peer connection data to disk and loading it on startup. This creates a "warm cache" of known-good peers that can be reconnected immediately.

### What Gets Saved

```json
{
  "version": 1,
  "last_updated": 1700000000,
  "peers": [
    {
      "peer_id": "12D3KooW...",
      "addresses": [
        "/ip4/192.168.1.100/tcp/4001",
        "/ip6/::1/tcp/4001"
      ],
      "last_seen": 1700000000,
      "connection_count": 15,
      "successful_transfers": 23,
      "failed_transfers": 2,
      "total_bytes_transferred": 524288000,
      "average_latency_ms": 45,
      "is_bootstrap": false,
      "supports_relay": true,
      "reliability_score": 0.92
    }
  ]
}
```

### Storage Location

**File:** `~/.chiral/peer_cache.json`  
- Uses existing app data directory structure
- Platform-agnostic path resolution
- Human-readable JSON format for debugging

---

## Implementation Details

### 1. Data Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCacheEntry {
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub last_seen: u64,
    pub connection_count: u32,
    pub successful_transfers: u32,
    pub failed_transfers: u32,
    pub total_bytes_transferred: u64,
    pub average_latency_ms: u32,
    pub is_bootstrap: bool,
    pub supports_relay: bool,
    pub reliability_score: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerCache {
    pub version: u32,
    pub last_updated: u64,
    pub peers: Vec<PeerCacheEntry>,
}
```

### 2. Save Logic

**Where:** `src-tauri/src/dht.rs` (in `DhtService::shutdown()`)

```rust
async fn save_peer_cache(&self) -> Result<(), String> {
    let app_data_dir = get_app_data_dir()?;
    let cache_path = app_data_dir.join("peer_cache.json");
    
    let peers = self.get_connected_peers_with_stats().await;
    
    let cache = PeerCache {
        version: 1,
        last_updated: current_timestamp(),
        peers: peers.into_iter()
            .filter(|p| p.connection_count > 0) // Only save peers we've actually connected to
            .take(100) // Limit to top 100 peers
            .collect(),
    };
    
    let json = serde_json::to_string_pretty(&cache)?;
    tokio::fs::write(&cache_path, json).await?;
    
    Ok(())
}
```

**Trigger:** Automatically called in `stop_dht_node` command (line 1591 in main.rs)

### 3. Load Logic

**Where:** `src-tauri/src/dht.rs` (in `DhtService::new()`)

```rust
async fn load_peer_cache(&self) -> Result<Vec<PeerCacheEntry>, String> {
    let app_data_dir = get_app_data_dir()?;
    let cache_path = app_data_dir.join("peer_cache.json");
    
    if !cache_path.exists() {
        return Ok(Vec::new());
    }
    
    let json = tokio::fs::read_to_string(&cache_path).await?;
    let cache: PeerCache = serde_json::from_str(&json)?;
    
    // Filter out stale peers (older than 7 days)
    let max_age = 7 * 24 * 60 * 60; // 7 days in seconds
    let now = current_timestamp();
    
    Ok(cache.peers.into_iter()
        .filter(|p| (now - p.last_seen) < max_age)
        .collect())
}
```

**Usage:** After loading, attempt to connect to cached peers in parallel with bootstrap nodes

```rust
// In start_dht_node command (line 1419 in main.rs)
let cached_peers = dht_service.load_peer_cache().await?;

// Try cached peers first (non-blocking)
for peer in cached_peers {
    let _ = dht_service.connect_peer(peer.addresses[0].clone()).await;
}

// Then connect to bootstrap nodes as backup
for bootstrap_addr in bootstrap_nodes {
    let _ = dht_service.connect_peer(bootstrap_addr).await;
}
```

### 4. Update Logic

**When:** After successful file transfer or peer interaction

```rust
pub async fn update_peer_stats(&self, peer_id: &str, success: bool, bytes: u64) {
    // Update in-memory peer metrics
    // This data will be saved on shutdown
}
```

---

## Integration Points

### Existing Code Modifications

1. **`stop_dht_node` (main.rs:1591)**
   ```rust
   if let Some(dht) = dht {
       // NEW: Save peer cache before shutdown
       let _ = dht.save_peer_cache().await;
       
       (*dht).shutdown().await
           .map_err(|e| format!("Failed to stop DHT: {}", e))?;
   }
   ```

2. **`start_dht_node` (main.rs:1419)**
   ```rust
   let dht_service = DhtService::new(/* ... */).await?;
   
   // NEW: Load cached peers and attempt reconnection
   let cached_peers = dht_service.load_peer_cache().await.ok();
   if let Some(peers) = cached_peers {
       dht_service.reconnect_cached_peers(peers).await;
   }
   ```

3. **Peer Selection (peer_selection.rs)**
   - Already tracks peer metrics (connection_count, bytes_transferred, etc.)
   - Just needs serialization to disk

---

## Benefits

### Immediate Wins

✅ **5-10x faster startup** - Reconnect to known peers in <5 seconds  
✅ **Zero code changes required** in frontend  
✅ **Reduced bootstrap load** - Less strain on public bootstrap nodes  
✅ **Better peer diversity** - Reconnect to previously discovered peers  

### Long-term Value

✅ **Foundation for reputation system** - Persistent peer quality metrics  
✅ **Improved reliability** - Prefer peers with proven track record  
✅ **Network stability** - More consistent peer connections  
✅ **User trust** - Faster, more predictable experience  

---

## Timeline

### Phase 1: Basic Persistence (Day 1-2)
- [ ] Create `PeerCache` and `PeerCacheEntry` structs
- [ ] Implement `save_peer_cache()` in `dht.rs`
- [ ] Implement `load_peer_cache()` in `dht.rs`
- [ ] Add save call to `stop_dht_node` command
- [ ] Add load call to `start_dht_node` command

### Phase 2: Smart Reconnection (Day 3)
- [ ] Implement `reconnect_cached_peers()` with parallel connection attempts
- [ ] Add timeout handling for stale addresses
- [ ] Filter out peers that fail to reconnect

### Phase 3: Cache Management (Day 4)
- [ ] Add cache size limits (max 100 peers)
- [ ] Add age-based eviction (7 day max)
- [ ] Add reliability-based sorting (prefer high-score peers)

### Phase 4: Testing & Polish (Day 5)
- [ ] Test with empty cache (first run)
- [ ] Test with stale cache (old peers)
- [ ] Test with full cache (100+ peers)
- [ ] Add logging for cache hit/miss rates

**Total Effort:** ~5 days

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| **Stale peer addresses** | High | Medium | Filter by age (7 days), validate on reconnect |
| **Cache file corruption** | Low | Medium | Graceful fallback to bootstrap nodes |
| **Disk space usage** | Low | Low | Limit to 100 peers (~50KB max) |
| **Privacy concerns** | Medium | Low | Store peer IDs only (no user data) |
| **Race condition on save** | Low | Low | Use atomic file writes |

---

## Success Criteria

✅ **Startup time** < 10 seconds to connect to 5+ peers (vs. 30-60 seconds currently)  
✅ **Bootstrap load** - 50% reduction in bootstrap node connections  
✅ **Cache hit rate** - 70%+ of cached peers successfully reconnect  
✅ **No data loss** - Cache persists across restarts without corruption  
✅ **No performance impact** - Save/load operations < 100ms  

---

## Future Enhancements

### Phase 2 Features (Not in Initial Scope)
- **Encrypted cache** - Protect peer list with user password
- **Cloud sync** - Share peer cache across devices
- **Peer scoring** - Advanced reliability algorithms
- **Geographic grouping** - Prefer nearby peers for lower latency
- **Automatic cleanup** - Remove peers that consistently fail

### Related Features
- **Simple transfer counter** (from previous proposal) - Uses persistent peer data
- **Reputation system** - Builds on peer cache foundation
- **Smart routing** - Prefer cached high-quality peers

---

## Open Questions

1. **Should bootstrap nodes be cached separately?**  
   → Yes, but always attempt reconnection to them

2. **What happens if cache file is corrupted?**  
   → Log warning, delete file, fall back to bootstrap nodes

3. **Should we cache relay node information?**  
   → Yes, helpful for NAT traversal

4. **How often should we save the cache?**  
   → Only on shutdown (minimize disk I/O)

5. **Should we expose cache stats to UI?**  
   → Later - focus on backend implementation first

---

## Recommendation

**Implement this feature immediately** as it:
- Solves a critical UX issue (slow startup)
- Requires minimal code changes (~200 LOC)
- Provides foundation for future reputation features
- Has no breaking changes or risks

This is a **high-value, low-effort** improvement that addresses real user pain.
