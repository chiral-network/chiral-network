# Heartbeat System Overview

The heartbeat system is a peer liveness tracking mechanism that keeps the seeder list for files accurate and up-to-date. It ensures that when multiple peers are seeding a file, all of them remain visible to downloaders even if some peers temporarily disconnect.

## Core Components

### 1. SeederHeartbeat Struct
```
struct SeederHeartbeat {
peer_id: String,
expires_at: u64,
last_heartbeat: u64,
}
```

- **peer_id**: Identifies a seeding peer  
- **last_heartbeat**: Unix timestamp of the most recent heartbeat update  
- **expires_at**: When this heartbeat entry should be considered stale (current time + TTL)

### 2. FileHeartbeatCacheEntry 
```
struct FileHeartbeatCacheEntry {
heartbeats: Vec<SeederHeartbeat>,
metadata: serde_json::Value,
}
```

- Caches heartbeat data for each file locally  
- Stores the latest DHT metadata alongside heartbeats for quick lookups

### 3. Timing Constants 
```
const FILE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const FILE_HEARTBEAT_TTL: Duration = Duration::from_secs(90);
```

- Interval: Every 15 seconds, active seeders refresh their heartbeat records  
- TTL: Heartbeats are valid for 90 seconds; if no refresh arrives, entries are pruned

## How It Works

### A. Publishing a File 

When a peer publishes a file (PublishFile command):

- Fetch existing heartbeats from the local cache for that file  
- Upsert the local peer's heartbeat via `upsert_heartbeat()`:  
  - Updates the peer's timestamp to "now"  
  - Sets `expires_at = now + TTL (90 seconds)`  
  - If the peer is already in the list, updates its entry; otherwise adds it  
- Publish to DHT with `put_record()`:  
  - The updated heartbeat list (including all known seeders) is serialized into the DHT record  
  - Remote peers querying the DHT see all active seeders

### B. Heartbeat Maintenance (Periodic Refresh)

Every 15 seconds, the `heartbeat_maintenance_interval` tick fires:  

- Iterate over cached files  
- For each file this node is seeding:  
  - Call `upsert_heartbeat()` to refresh its own entry's timestamp and expiry  
  - Call `prune_heartbeats()` to remove stale peers (older than 30-second grace period)  
  - Rebuild the "seeders" array from active heartbeats  
  - Republish the updated record to DHT with merged seeder list  

### C. Merging Multiple Sources (`merge_heartbeats`)

When the node fetches a DHT record from another peer:  

- Compare two heartbeat lists (local cache vs. remote DHT record)  
- For peers in both lists:  
  - Keep the most recent heartbeat timestamp  
  - Use the maximum expiry time  
  - If the peer has a recent heartbeat (within 15 seconds), extend its TTL to 90 seconds  
- For unique peers in either list:  
  - Retain them if they haven't expired (30-second grace period)  
- Return merged result: All seeders that are either recently active or already known to be online  

### D. Pruning Stale Entries (`prune_heartbeats`)

fn prune_heartbeats(mut entries: Vec<SeederHeartbeat>, now: u64) -> Vec<SeederHeartbeat> {
let prune_threshold = now.saturating_sub(30); // 30 second grace period
entries.retain(|hb| hb.expires_at > prune_threshold);
entries.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
entries
}

- Removes heartbeats that haven't been refreshed (expired > 30 seconds ago)  
- The 30-second grace period provides a buffer between the 15-second refresh interval and the 90-second TTL

### E. Handling Peer Disconnects

When a peer disconnects (ConnectionClosed event):  

- Remove its heartbeat from all files in the cache  
- Prune any expired heartbeats  
- Republish updated DHT records with the remaining seeders  
- Emit `FileDiscovered` event so the UI updates immediately

### F. Stopping Publication (Seeder Removal)

When a peer stops seeding (StopPublish command):  

- Remove the local peer's entry from all cached heartbeats  
- Prune expired entries  
- Republish the updated metadata to DHT with remaining seeders  
- Emit `FileDiscovered` event with the new seeder list  
- If no seeders remain, publish an empty record as a fallback

## Data Flow Diagram
```
┌─────────────────────────────────────────────────────────────┐
│ LocalNode (Peer A) seeding File X │
└─────────────────────────────────────────────────────────────┘
│
├──> Publish File X
│ ├─ Create SeederHeartbeat { peer_a, expires_at: now+90s }
│ └─ Put to DHT: { seeders: [peer_a], heartbeats: [...] }
│
├──> Every 15 seconds (Heartbeat Maintenance)
│ ├─ Refresh Peer A's expires_at = now + 90s
│ ├─ Prune expired entries (> 30s old)
│ └─ Republish to DHT if changed
│
└──> Remote Peer B searches for File X
├─ Receive DHT record: { seeders: [peer_a], heartbeats: [...] }
├─ Merge with any cached data
├─ Extract: seeders = [peer_a]
└─ UI shows: "1 Seeder Available"

┌─────────────────────────────────────────────────────────────┐
│ Both Peer A & Peer C seeding File X │
└─────────────────────────────────────────────────────────────┘
│
├──> Peer A refreshes: { peer_a, expires_at: now+90s }
├──> Peer C heartbeat merged: { peer_c, expires_at: now+90s }
│
└──> Republish to DHT
└─ { seeders: [peer_a, peer_c], heartbeats: [...] }
└─ UI shows: "2 Seeders Available"

┌─────────────────────────────────────────────────────────────┐
│ Peer C stops seeding (StopPublish) │
└─────────────────────────────────────────────────────────────┘
│
├─ Remove Peer C's heartbeat from cache
├─ Republish to DHT: { seeders: [peer_a], heartbeats: [...] }
│
└─> Remote downloaders see update
└─ UI shows: "1 Seeder Available"

```
## Key Design Decisions

| Feature                | Purpose                                                                        |
|------------------------|--------------------------------------------------------------------------------|
| **15-second refresh interval** | Keeps seeders visible even with intermittent connectivity                   |
| **90-second TTL**              | Allows 6 refresh cycles before expiry (safety margin)                      |
| **30-second grace period**     | Prevents flapping during slow network conditions                           |
| **Merge strategy**             | Preserves seeders from multiple DHT sources; extends expiry for active peers|
| **Local cache**                | Enables instant UI updates before DHT propagates changes                  |
| **Automatic pruning**          | Removes stale entries so the seeder list stays clean                      |

This system ensures that seeder counts remain stable and accurate across the network, even when peers connect/disconnect frequently.

