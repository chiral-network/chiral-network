# DHT-Based Distributed Storage Architecture Proposal

## Executive Summary

This proposal outlines a comprehensive architecture for implementing true DHT-based distributed storage in Chiral Network, transforming it from a local-only system to a fully decentralized P2P storage network inspired by IPFS and Filecoin.

## System Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Svelte Frontend (UI)                     │
│  File Management │ Network Viz │ DHT Status │ Progress     │
└─────────────────────┬───────────────────────────────────────┘
                      │ Tauri Commands/Events
┌─────────────────────┴───────────────────────────────────────┐
│                  Tauri Integration Layer                    │
│  Commands Bridge │ Event Emitter │ FS Access │ IPC         │
└─────────────────────┬───────────────────────────────────────┘
                      │
┌─────────────────────┴───────────────────────────────────────┐
│                    Rust Backend Core                        │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   DHT Layer  │  │Content Layer │  │Storage Layer │     │
│  │  (Kademlia)  │  │  (CID/DAG)   │  │ (Local/Dist) │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │Network Layer │  │ Replication  │  │   Protocol   │     │
│  │  (libp2p)    │  │   Manager    │  │   Handler    │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

## 1. Rust Backend Architecture

### 1.1 Core Module Structure

```
src-tauri/src/
├── dht/
│   ├── mod.rs              # DHT module exports
│   ├── kademlia.rs         # Kademlia DHT implementation
│   ├── routing_table.rs    # K-bucket routing table
│   ├── peer_id.rs          # Peer identification
│   └── rpc.rs              # DHT RPC handlers
├── content/
│   ├── mod.rs              # Content module exports
│   ├── cid.rs              # Content ID generation
│   ├── chunker.rs          # File chunking
│   ├── dag.rs              # Merkle DAG structures
│   └── hasher.rs           # Hashing utilities
├── storage/
│   ├── mod.rs              # Storage module exports
│   ├── blockstore.rs       # Local block storage
│   ├── datastore.rs        # Metadata storage
│   └── pinning.rs          # Pin management
├── network/
│   ├── mod.rs              # Network module exports
│   ├── transport.rs        # libp2p transport setup
│   ├── swarm.rs            # Swarm behavior
│   ├── protocols/
│   │   ├── bitswap.rs      # Block exchange protocol
│   │   └── identify.rs     # Peer identification
│   └── discovery.rs        # Peer discovery
├── replication/
│   ├── mod.rs              # Replication module exports
│   ├── strategy.rs         # Replication strategies
│   └── scheduler.rs        # Replication scheduling
└── api/
    ├── commands.rs         # Tauri commands
    └── events.rs           # Event definitions
```

### 1.2 DHT/Kademlia Implementation

**File: `src-tauri/src/dht/kademlia.rs`**

```rust
use libp2p::{
    kad::{Kademlia, KademliaConfig, KademliaEvent, QueryResult, Record},
    PeerId, Multiaddr,
};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Kademlia DHT manager for content routing
pub struct DhtManager {
    kademlia: Kademlia<MemoryStore>,
    local_peer_id: PeerId,
    query_channels: HashMap<QueryId, mpsc::Sender<QueryResult>>,
}

impl DhtManager {
    /// Initialize DHT with bootstrap nodes
    pub fn new(local_peer_id: PeerId, bootstrap_nodes: Vec<(PeerId, Multiaddr)>) -> Self {
        let mut config = KademliaConfig::default();
        config.set_replication_factor(20.try_into().unwrap()); // Replicate to 20 nodes
        config.set_query_timeout(Duration::from_secs(60));

        let store = MemoryStore::new(local_peer_id);
        let mut kademlia = Kademlia::with_config(local_peer_id, store, config);

        // Add bootstrap nodes
        for (peer_id, addr) in bootstrap_nodes {
            kademlia.add_address(&peer_id, addr);
        }

        Self {
            kademlia,
            local_peer_id,
            query_channels: HashMap::new(),
        }
    }

    /// Publish content to DHT (provide)
    pub async fn provide(&mut self, cid: Cid) -> Result<(), DhtError> {
        let key = cid_to_key(&cid);
        let query_id = self.kademlia.start_providing(key)?;

        // Store query for result tracking
        let (tx, mut rx) = mpsc::channel(1);
        self.query_channels.insert(query_id, tx);

        // Wait for query completion
        match rx.recv().await {
            Some(QueryResult::StartProviding(Ok(_))) => Ok(()),
            Some(QueryResult::StartProviding(Err(e))) => Err(DhtError::ProvideFailed(e)),
            _ => Err(DhtError::UnexpectedResult),
        }
    }

    /// Find providers for content
    pub async fn find_providers(&mut self, cid: Cid, count: usize) -> Result<Vec<PeerId>, DhtError> {
        let key = cid_to_key(&cid);
        let query_id = self.kademlia.get_providers(key);

        let (tx, mut rx) = mpsc::channel(1);
        self.query_channels.insert(query_id, tx);

        let mut providers = Vec::new();

        // Collect providers as they arrive
        while let Some(result) = rx.recv().await {
            match result {
                QueryResult::GetProviders(Ok(GetProvidersOk { providers: new_providers, .. })) => {
                    providers.extend(new_providers);
                    if providers.len() >= count {
                        break;
                    }
                }
                QueryResult::GetProviders(Err(e)) => return Err(DhtError::LookupFailed(e)),
                _ => continue,
            }
        }

        Ok(providers)
    }

    /// Store metadata record in DHT
    pub async fn put_record(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), DhtError> {
        let record = Record {
            key: Key::new(&key),
            value,
            publisher: Some(self.local_peer_id),
            expires: None,
        };

        let query_id = self.kademlia.put_record(record, Quorum::Majority)?;

        let (tx, mut rx) = mpsc::channel(1);
        self.query_channels.insert(query_id, tx);

        match rx.recv().await {
            Some(QueryResult::PutRecord(Ok(_))) => Ok(()),
            Some(QueryResult::PutRecord(Err(e))) => Err(DhtError::PutFailed(e)),
            _ => Err(DhtError::UnexpectedResult),
        }
    }

    /// Retrieve metadata record from DHT
    pub async fn get_record(&mut self, key: Vec<u8>) -> Result<Vec<u8>, DhtError> {
        let query_id = self.kademlia.get_record(Key::new(&key), Quorum::One);

        let (tx, mut rx) = mpsc::channel(1);
        self.query_channels.insert(query_id, tx);

        match rx.recv().await {
            Some(QueryResult::GetRecord(Ok(GetRecordOk { records, .. }))) => {
                records.first()
                    .map(|r| r.record.value.clone())
                    .ok_or(DhtError::RecordNotFound)
            }
            Some(QueryResult::GetRecord(Err(e))) => Err(DhtError::GetFailed(e)),
            _ => Err(DhtError::UnexpectedResult),
        }
    }

    /// Bootstrap DHT by connecting to known nodes
    pub async fn bootstrap(&mut self) -> Result<(), DhtError> {
        let query_id = self.kademlia.bootstrap()?;

        let (tx, mut rx) = mpsc::channel(1);
        self.query_channels.insert(query_id, tx);

        match rx.recv().await {
            Some(QueryResult::Bootstrap(Ok(_))) => Ok(()),
            Some(QueryResult::Bootstrap(Err(e))) => Err(DhtError::BootstrapFailed(e)),
            _ => Err(DhtError::UnexpectedResult),
        }
    }
}

/// Convert CID to DHT key
fn cid_to_key(cid: &Cid) -> Key {
    Key::new(&cid.to_bytes())
}

#[derive(Debug)]
pub enum DhtError {
    ProvideFailed(String),
    LookupFailed(String),
    PutFailed(String),
    GetFailed(String),
    BootstrapFailed(String),
    RecordNotFound,
    UnexpectedResult,
}
```

**File: `src-tauri/src/dht/routing_table.rs`**

```rust
use libp2p::PeerId;
use std::collections::HashMap;

const K: usize = 20; // K-bucket size (Kademlia parameter)

/// K-bucket routing table for peer management
pub struct RoutingTable {
    buckets: Vec<KBucket>,
    local_id: PeerId,
}

impl RoutingTable {
    pub fn new(local_id: PeerId) -> Self {
        let buckets = (0..256).map(|_| KBucket::new(K)).collect();
        Self { buckets, local_id }
    }

    /// Add peer to appropriate bucket
    pub fn add_peer(&mut self, peer_id: PeerId, addr: Multiaddr) -> Result<(), RoutingError> {
        let distance = xor_distance(&self.local_id, &peer_id);
        let bucket_index = distance.leading_zeros() as usize;

        self.buckets[bucket_index].insert(peer_id, addr)
    }

    /// Find K closest peers to target
    pub fn find_closest(&self, target: &PeerId, count: usize) -> Vec<(PeerId, Multiaddr)> {
        let distance = xor_distance(&self.local_id, target);
        let bucket_index = distance.leading_zeros() as usize;

        let mut peers = Vec::new();

        // Search bucket and neighbors
        for offset in 0..256 {
            if bucket_index + offset < 256 {
                peers.extend(self.buckets[bucket_index + offset].peers());
            }
            if offset > 0 && bucket_index >= offset {
                peers.extend(self.buckets[bucket_index - offset].peers());
            }

            if peers.len() >= count {
                break;
            }
        }

        // Sort by distance to target
        peers.sort_by_key(|(id, _)| xor_distance(id, target));
        peers.truncate(count);

        peers
    }
}

struct KBucket {
    peers: HashMap<PeerId, Multiaddr>,
    capacity: usize,
}

impl KBucket {
    fn new(capacity: usize) -> Self {
        Self {
            peers: HashMap::new(),
            capacity,
        }
    }

    fn insert(&mut self, peer_id: PeerId, addr: Multiaddr) -> Result<(), RoutingError> {
        if self.peers.len() >= self.capacity && !self.peers.contains_key(&peer_id) {
            return Err(RoutingError::BucketFull);
        }

        self.peers.insert(peer_id, addr);
        Ok(())
    }

    fn peers(&self) -> Vec<(PeerId, Multiaddr)> {
        self.peers.iter()
            .map(|(id, addr)| (*id, addr.clone()))
            .collect()
    }
}

/// Calculate XOR distance between two peer IDs
fn xor_distance(a: &PeerId, b: &PeerId) -> u256 {
    let a_bytes = a.to_bytes();
    let b_bytes = b.to_bytes();

    let mut distance = [0u8; 32];
    for i in 0..32 {
        distance[i] = a_bytes[i] ^ b_bytes[i];
    }

    u256::from_be_bytes(distance)
}

#[derive(Debug)]
pub enum RoutingError {
    BucketFull,
}
```

### 1.3 Content Addressing & Chunking

**File: `src-tauri/src/content/cid.rs`**

```rust
use multihash::{Code, MultihashDigest};
use cid::Cid;
use sha2::{Sha256, Digest};

/// Generate Content ID (CID) from data
pub fn generate_cid(data: &[u8]) -> Cid {
    // Hash data with SHA-256
    let hash = Code::Sha2_256.digest(data);

    // Create CIDv1 with dag-pb codec
    Cid::new_v1(0x70, hash) // 0x70 = dag-pb
}

/// Generate CID from file
pub async fn generate_file_cid(path: &Path) -> Result<Cid, ContentError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 8192];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    let hash = hasher.finalize();
    let multihash = Code::Sha2_256.digest(&hash);

    Ok(Cid::new_v1(0x70, multihash))
}

/// Verify data matches CID
pub fn verify_cid(data: &[u8], expected_cid: &Cid) -> bool {
    let computed_cid = generate_cid(data);
    computed_cid == *expected_cid
}
```

**File: `src-tauri/src/content/chunker.rs`**

```rust
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt};

const DEFAULT_CHUNK_SIZE: usize = 256 * 1024; // 256 KB

/// Chunk file into fixed-size blocks
pub struct Chunker {
    chunk_size: usize,
}

impl Chunker {
    pub fn new(chunk_size: usize) -> Self {
        Self { chunk_size }
    }

    pub fn default() -> Self {
        Self::new(DEFAULT_CHUNK_SIZE)
    }

    /// Chunk file and return list of (chunk_data, chunk_cid)
    pub async fn chunk_file(&self, path: &Path) -> Result<Vec<(Bytes, Cid)>, ChunkError> {
        let mut file = tokio::fs::File::open(path).await?;
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];

        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            let chunk_data = Bytes::copy_from_slice(&buffer[..n]);
            let chunk_cid = generate_cid(&chunk_data);

            chunks.push((chunk_data, chunk_cid));
        }

        Ok(chunks)
    }

    /// Chunk data stream
    pub async fn chunk_stream<R: AsyncRead + Unpin>(
        &self,
        reader: &mut R,
    ) -> Result<Vec<(Bytes, Cid)>, ChunkError> {
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];

        loop {
            let n = reader.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            let chunk_data = Bytes::copy_from_slice(&buffer[..n]);
            let chunk_cid = generate_cid(&chunk_data);

            chunks.push((chunk_data, chunk_cid));
        }

        Ok(chunks)
    }
}

#[derive(Debug)]
pub enum ChunkError {
    IoError(std::io::Error),
}

impl From<std::io::Error> for ChunkError {
    fn from(e: std::io::Error) -> Self {
        ChunkError::IoError(e)
    }
}
```

**File: `src-tauri/src/content/dag.rs`**

```rust
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Merkle DAG node for file structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    pub cid: Cid,
    pub links: Vec<DagLink>,
    pub data: Option<Bytes>,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagLink {
    pub name: String,
    pub cid: Cid,
    pub size: u64,
}

impl DagNode {
    /// Create leaf node (contains data)
    pub fn new_leaf(data: Bytes) -> Self {
        let cid = generate_cid(&data);
        let size = data.len() as u64;

        Self {
            cid,
            links: Vec::new(),
            data: Some(data),
            size,
        }
    }

    /// Create intermediate node (contains links)
    pub fn new_intermediate(links: Vec<DagLink>) -> Self {
        let size = links.iter().map(|l| l.size).sum();

        // Serialize node to compute CID
        let serialized = bincode::serialize(&links).unwrap();
        let cid = generate_cid(&serialized);

        Self {
            cid,
            links,
            data: None,
            size,
        }
    }

    /// Build DAG from chunks
    pub fn build_from_chunks(chunks: Vec<(Bytes, Cid)>) -> Self {
        // Create leaf nodes for each chunk
        let leaves: Vec<DagNode> = chunks
            .into_iter()
            .map(|(data, cid)| DagNode {
                cid,
                links: Vec::new(),
                data: Some(data),
                size: data.len() as u64,
            })
            .collect();

        // Build tree bottom-up
        Self::build_tree(leaves)
    }

    /// Build balanced tree from leaf nodes
    fn build_tree(nodes: Vec<DagNode>) -> DagNode {
        if nodes.len() == 1 {
            return nodes.into_iter().next().unwrap();
        }

        const BRANCH_FACTOR: usize = 174; // Max links per node

        let mut current_level = nodes;

        while current_level.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in current_level.chunks(BRANCH_FACTOR) {
                let links: Vec<DagLink> = chunk
                    .iter()
                    .enumerate()
                    .map(|(i, node)| DagLink {
                        name: format!("chunk_{}", i),
                        cid: node.cid,
                        size: node.size,
                    })
                    .collect();

                next_level.push(DagNode::new_intermediate(links));
            }

            current_level = next_level;
        }

        current_level.into_iter().next().unwrap()
    }

    /// Get all CIDs in DAG (for providing to DHT)
    pub fn all_cids(&self) -> Vec<Cid> {
        let mut cids = vec![self.cid];

        for link in &self.links {
            cids.push(link.cid);
        }

        cids
    }
}
```

### 1.4 Storage Layer

**File: `src-tauri/src/storage/blockstore.rs`**

```rust
use sled::{Db, IVec};
use bytes::Bytes;

/// Local block storage using sled database
pub struct Blockstore {
    db: Db,
}

impl Blockstore {
    pub fn new(path: &Path) -> Result<Self, StorageError> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Store block by CID
    pub fn put(&self, cid: &Cid, data: Bytes) -> Result<(), StorageError> {
        let key = cid.to_bytes();
        self.db.insert(key, data.as_ref())?;
        Ok(())
    }

    /// Retrieve block by CID
    pub fn get(&self, cid: &Cid) -> Result<Option<Bytes>, StorageError> {
        let key = cid.to_bytes();

        match self.db.get(key)? {
            Some(data) => Ok(Some(Bytes::copy_from_slice(&data))),
            None => Ok(None),
        }
    }

    /// Check if block exists
    pub fn has(&self, cid: &Cid) -> Result<bool, StorageError> {
        let key = cid.to_bytes();
        Ok(self.db.contains_key(key)?)
    }

    /// Delete block
    pub fn delete(&self, cid: &Cid) -> Result<(), StorageError> {
        let key = cid.to_bytes();
        self.db.remove(key)?;
        Ok(())
    }

    /// Get all stored CIDs
    pub fn list_cids(&self) -> Result<Vec<Cid>, StorageError> {
        let mut cids = Vec::new();

        for result in self.db.iter() {
            let (key, _) = result?;
            let cid = Cid::try_from(key.as_ref())?;
            cids.push(cid);
        }

        Ok(cids)
    }

    /// Get total storage size
    pub fn size(&self) -> Result<u64, StorageError> {
        let mut total = 0u64;

        for result in self.db.iter() {
            let (_, value) = result?;
            total += value.len() as u64;
        }

        Ok(total)
    }
}

#[derive(Debug)]
pub enum StorageError {
    DatabaseError(sled::Error),
    InvalidCid,
}

impl From<sled::Error> for StorageError {
    fn from(e: sled::Error) -> Self {
        StorageError::DatabaseError(e)
    }
}
```

**File: `src-tauri/src/storage/datastore.rs`**

```rust
use serde::{Serialize, Deserialize};
use sled::Db;

/// Metadata store for file information
pub struct Datastore {
    db: Db,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub cid: Cid,
    pub name: String,
    pub size: u64,
    pub mime_type: String,
    pub created_at: u64,
    pub dag_root: Cid,
    pub chunks: Vec<Cid>,
}

impl Datastore {
    pub fn new(path: &Path) -> Result<Self, StorageError> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Store file metadata
    pub fn put_metadata(&self, metadata: &FileMetadata) -> Result<(), StorageError> {
        let key = metadata.cid.to_bytes();
        let value = bincode::serialize(metadata)?;

        self.db.insert(key, value)?;
        Ok(())
    }

    /// Get file metadata
    pub fn get_metadata(&self, cid: &Cid) -> Result<Option<FileMetadata>, StorageError> {
        let key = cid.to_bytes();

        match self.db.get(key)? {
            Some(data) => {
                let metadata: FileMetadata = bincode::deserialize(&data)?;
                Ok(Some(metadata))
            }
            None => Ok(None),
        }
    }

    /// List all stored files
    pub fn list_files(&self) -> Result<Vec<FileMetadata>, StorageError> {
        let mut files = Vec::new();

        for result in self.db.iter() {
            let (_, value) = result?;
            let metadata: FileMetadata = bincode::deserialize(&value)?;
            files.push(metadata);
        }

        Ok(files)
    }

    /// Delete file metadata
    pub fn delete_metadata(&self, cid: &Cid) -> Result<(), StorageError> {
        let key = cid.to_bytes();
        self.db.remove(key)?;
        Ok(())
    }
}
```

### 1.5 Replication Manager

**File: `src-tauri/src/replication/strategy.rs`**

```rust
use std::collections::HashSet;

/// Replication strategy for distributed storage
pub struct ReplicationStrategy {
    target_replicas: usize,
    min_replicas: usize,
}

impl ReplicationStrategy {
    pub fn new(target_replicas: usize, min_replicas: usize) -> Self {
        Self {
            target_replicas,
            min_replicas,
        }
    }

    /// Determine which peers should store this content
    pub fn select_providers(
        &self,
        cid: &Cid,
        available_peers: &[PeerId],
        current_providers: &HashSet<PeerId>,
    ) -> Vec<PeerId> {
        let needed = self.target_replicas.saturating_sub(current_providers.len());

        if needed == 0 {
            return Vec::new();
        }

        // Select peers deterministically based on CID
        let mut candidates: Vec<_> = available_peers
            .iter()
            .filter(|p| !current_providers.contains(p))
            .map(|p| (*p, xor_distance_cid(cid, p)))
            .collect();

        // Sort by distance to CID (closest peers store content)
        candidates.sort_by_key(|(_, dist)| *dist);

        candidates
            .into_iter()
            .take(needed)
            .map(|(peer, _)| peer)
            .collect()
    }

    /// Check if replication is sufficient
    pub fn is_sufficiently_replicated(&self, replica_count: usize) -> bool {
        replica_count >= self.min_replicas
    }

    /// Calculate urgency for replication (0.0 = not urgent, 1.0 = critical)
    pub fn replication_urgency(&self, current_replicas: usize) -> f64 {
        if current_replicas >= self.target_replicas {
            return 0.0;
        }

        if current_replicas < self.min_replicas {
            return 1.0;
        }

        let deficit = self.target_replicas - current_replicas;
        deficit as f64 / self.target_replicas as f64
    }
}

fn xor_distance_cid(cid: &Cid, peer: &PeerId) -> u256 {
    // XOR distance between CID and PeerID for consistent hashing
    let cid_bytes = cid.to_bytes();
    let peer_bytes = peer.to_bytes();

    let mut distance = [0u8; 32];
    for i in 0..32.min(cid_bytes.len()).min(peer_bytes.len()) {
        distance[i] = cid_bytes[i] ^ peer_bytes[i];
    }

    u256::from_be_bytes(distance)
}
```

**File: `src-tauri/src/replication/scheduler.rs`**

```rust
use tokio::time::{interval, Duration};
use std::collections::{HashMap, HashSet};

/// Schedule and manage replication tasks
pub struct ReplicationScheduler {
    strategy: ReplicationStrategy,
    pending_replications: HashMap<Cid, Vec<PeerId>>,
    active_replications: HashSet<Cid>,
    max_concurrent: usize,
}

impl ReplicationScheduler {
    pub fn new(strategy: ReplicationStrategy, max_concurrent: usize) -> Self {
        Self {
            strategy,
            pending_replications: HashMap::new(),
            active_replications: HashSet::new(),
            max_concurrent,
        }
    }

    /// Schedule content for replication
    pub fn schedule(&mut self, cid: Cid, target_peers: Vec<PeerId>) {
        self.pending_replications.insert(cid, target_peers);
    }

    /// Process replication queue
    pub async fn process_queue(
        &mut self,
        network: &mut NetworkManager,
        blockstore: &Blockstore,
    ) -> Result<(), ReplicationError> {
        let mut interval = interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            // Remove completed replications
            self.active_replications.retain(|cid| {
                // Check if still replicating
                self.is_replicating(cid)
            });

            // Start new replications up to max_concurrent
            let available_slots = self.max_concurrent.saturating_sub(self.active_replications.len());

            let mut to_start: Vec<_> = self.pending_replications
                .iter()
                .take(available_slots)
                .map(|(cid, peers)| (*cid, peers.clone()))
                .collect();

            for (cid, peers) in to_start {
                if let Some(data) = blockstore.get(&cid)? {
                    for peer in peers {
                        // Send block to peer
                        network.send_block(peer, cid, data.clone()).await?;
                    }

                    self.active_replications.insert(cid);
                    self.pending_replications.remove(&cid);
                }
            }
        }
    }

    fn is_replicating(&self, cid: &Cid) -> bool {
        // Check with network if replication is ongoing
        // This would query the network manager
        false
    }
}
```

## 2. Tauri Integration Layer

### 2.1 Command Interface

**File: `src-tauri/src/api/commands.rs`**

```rust
use tauri::State;
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize)]
pub struct UploadProgress {
    pub file_name: String,
    pub cid: String,
    pub chunks_total: usize,
    pub chunks_completed: usize,
    pub bytes_uploaded: u64,
    pub percentage: f64,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub cid: String,
    pub file_name: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub percentage: f64,
    pub peers_count: usize,
}

/// Upload file to distributed network
#[tauri::command]
pub async fn upload_file(
    path: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let path = Path::new(&path);

    // Generate CID
    let root_cid = generate_file_cid(path).await
        .map_err(|e| format!("Failed to generate CID: {}", e))?;

    // Chunk file
    let chunker = Chunker::default();
    let chunks = chunker.chunk_file(path).await
        .map_err(|e| format!("Failed to chunk file: {}", e))?;

    let total_chunks = chunks.len();

    // Store chunks locally
    let blockstore = &state.blockstore;
    for (i, (data, cid)) in chunks.iter().enumerate() {
        blockstore.put(cid, data.clone())
            .map_err(|e| format!("Failed to store chunk: {}", e))?;

        // Emit progress
        app_handle.emit_all("upload-progress", UploadProgress {
            file_name: path.file_name().unwrap().to_string_lossy().to_string(),
            cid: root_cid.to_string(),
            chunks_total: total_chunks,
            chunks_completed: i + 1,
            bytes_uploaded: data.len() as u64 * (i + 1) as u64,
            percentage: ((i + 1) as f64 / total_chunks as f64) * 100.0,
        }).ok();
    }

    // Build DAG
    let dag = DagNode::build_from_chunks(chunks);

    // Store metadata
    let metadata = FileMetadata {
        cid: root_cid,
        name: path.file_name().unwrap().to_string_lossy().to_string(),
        size: tokio::fs::metadata(path).await.unwrap().len(),
        mime_type: mime_guess::from_path(path).first_or_octet_stream().to_string(),
        created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        dag_root: dag.cid,
        chunks: dag.all_cids(),
    };

    state.datastore.put_metadata(&metadata)
        .map_err(|e| format!("Failed to store metadata: {}", e))?;

    // Announce to DHT
    let dht = &mut state.dht.lock().await;
    for cid in dag.all_cids() {
        dht.provide(cid).await
            .map_err(|e| format!("Failed to announce to DHT: {}", e))?;
    }

    Ok(root_cid.to_string())
}

/// Download file from distributed network
#[tauri::command]
pub async fn download_file(
    cid: String,
    output_path: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let root_cid = Cid::try_from(cid.as_str())
        .map_err(|e| format!("Invalid CID: {}", e))?;

    // Get metadata from DHT
    let metadata_key = format!("metadata:{}", root_cid).into_bytes();
    let dht = &mut state.dht.lock().await;
    let metadata_bytes = dht.get_record(metadata_key).await
        .map_err(|e| format!("Failed to get metadata: {}", e))?;

    let metadata: FileMetadata = bincode::deserialize(&metadata_bytes)
        .map_err(|e| format!("Failed to parse metadata: {}", e))?;

    // Find providers for all chunks
    let mut downloaded_bytes = 0u64;
    let total_size = metadata.size;

    let mut file = tokio::fs::File::create(&output_path).await
        .map_err(|e| format!("Failed to create output file: {}", e))?;

    for chunk_cid in metadata.chunks {
        // Check local storage first
        if let Some(data) = state.blockstore.get(&chunk_cid)
            .map_err(|e| format!("Storage error: {}", e))? {

            file.write_all(&data).await
                .map_err(|e| format!("Failed to write chunk: {}", e))?;

            downloaded_bytes += data.len() as u64;
        } else {
            // Find providers
            let providers = dht.find_providers(chunk_cid, 5).await
                .map_err(|e| format!("Failed to find providers: {}", e))?;

            if providers.is_empty() {
                return Err(format!("No providers found for chunk {}", chunk_cid));
            }

            // Download from first available provider
            let network = &state.network;
            let data = network.request_block(providers[0], chunk_cid).await
                .map_err(|e| format!("Failed to download chunk: {}", e))?;

            // Verify chunk
            if !verify_cid(&data, &chunk_cid) {
                return Err(format!("Chunk verification failed for {}", chunk_cid));
            }

            // Store locally
            state.blockstore.put(&chunk_cid, data.clone())
                .map_err(|e| format!("Failed to store chunk: {}", e))?;

            // Write to file
            file.write_all(&data).await
                .map_err(|e| format!("Failed to write chunk: {}", e))?;

            downloaded_bytes += data.len() as u64;
        }

        // Emit progress
        app_handle.emit_all("download-progress", DownloadProgress {
            cid: root_cid.to_string(),
            file_name: metadata.name.clone(),
            total_size,
            downloaded: downloaded_bytes,
            percentage: (downloaded_bytes as f64 / total_size as f64) * 100.0,
            peers_count: providers.len(),
        }).ok();
    }

    Ok(())
}

/// Get list of files in local storage
#[tauri::command]
pub async fn list_local_files(
    state: State<'_, AppState>,
) -> Result<Vec<FileMetadata>, String> {
    state.datastore.list_files()
        .map_err(|e| format!("Failed to list files: {}", e))
}

/// Get DHT network statistics
#[tauri::command]
pub async fn get_dht_stats(
    state: State<'_, AppState>,
) -> Result<DhtStats, String> {
    let dht = state.dht.lock().await;

    Ok(DhtStats {
        peer_count: dht.peer_count(),
        bucket_stats: dht.bucket_stats(),
        queries_active: dht.active_queries(),
        providers_count: dht.providers_count(),
    })
}

#[derive(Serialize)]
pub struct DhtStats {
    pub peer_count: usize,
    pub bucket_stats: Vec<usize>,
    pub queries_active: usize,
    pub providers_count: usize,
}

/// Find content providers
#[tauri::command]
pub async fn find_content_providers(
    cid: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let cid = Cid::try_from(cid.as_str())
        .map_err(|e| format!("Invalid CID: {}", e))?;

    let dht = &mut state.dht.lock().await;
    let providers = dht.find_providers(cid, 20).await
        .map_err(|e| format!("Failed to find providers: {}", e))?;

    Ok(providers.iter().map(|p| p.to_base58()).collect())
}
```

### 2.2 Event System

**File: `src-tauri/src/api/events.rs`**

```rust
use tauri::{Manager, AppHandle};
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum NetworkEvent {
    PeerConnected { peer_id: String, address: String },
    PeerDisconnected { peer_id: String },
    BlockReceived { cid: String, size: u64, from_peer: String },
    BlockSent { cid: String, size: u64, to_peer: String },
    DhtQueryStarted { query_id: String, query_type: String },
    DhtQueryCompleted { query_id: String, results_count: usize },
    ReplicationScheduled { cid: String, target_peers: Vec<String> },
    ReplicationCompleted { cid: String, replicas: usize },
}

pub struct EventEmitter {
    app_handle: AppHandle,
}

impl EventEmitter {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    pub fn emit(&self, event: NetworkEvent) {
        self.app_handle.emit_all("network-event", event).ok();
    }

    pub fn emit_upload_progress(&self, progress: UploadProgress) {
        self.app_handle.emit_all("upload-progress", progress).ok();
    }

    pub fn emit_download_progress(&self, progress: DownloadProgress) {
        self.app_handle.emit_all("download-progress", progress).ok();
    }
}
```

## 3. Svelte Frontend

### 3.1 DHT Visualization Component

**File: `src/lib/components/DhtVisualization.svelte`**

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/tauri';
  import { listen } from '@tauri-apps/api/event';

  interface DhtStats {
    peer_count: number;
    bucket_stats: number[];
    queries_active: number;
    providers_count: number;
  }

  let stats: DhtStats | null = null;
  let unsubscribe: (() => void) | null = null;

  onMount(async () => {
    // Initial load
    await loadStats();

    // Listen for updates
    unsubscribe = await listen('dht-stats-updated', (event) => {
      stats = event.payload as DhtStats;
    });

    // Poll for updates
    const interval = setInterval(loadStats, 5000);

    return () => {
      clearInterval(interval);
      if (unsubscribe) unsubscribe();
    };
  });

  async function loadStats() {
    try {
      stats = await invoke('get_dht_stats');
    } catch (error) {
      console.error('Failed to load DHT stats:', error);
    }
  }

  function getBucketColor(count: number): string {
    if (count === 0) return 'bg-gray-200';
    if (count < 5) return 'bg-blue-300';
    if (count < 10) return 'bg-blue-500';
    return 'bg-blue-700';
  }
</script>

<div class="dht-visualization p-4 bg-white rounded-lg shadow">
  <h2 class="text-xl font-bold mb-4">DHT Network Status</h2>

  {#if stats}
    <div class="grid grid-cols-2 gap-4 mb-6">
      <div class="stat-card p-3 bg-blue-50 rounded">
        <div class="text-sm text-gray-600">Connected Peers</div>
        <div class="text-2xl font-bold">{stats.peer_count}</div>
      </div>

      <div class="stat-card p-3 bg-green-50 rounded">
        <div class="text-sm text-gray-600">Active Queries</div>
        <div class="text-2xl font-bold">{stats.queries_active}</div>
      </div>

      <div class="stat-card p-3 bg-purple-50 rounded">
        <div class="text-sm text-gray-600">Known Providers</div>
        <div class="text-2xl font-bold">{stats.providers_count}</div>
      </div>

      <div class="stat-card p-3 bg-orange-50 rounded">
        <div class="text-sm text-gray-600">Routing Table Size</div>
        <div class="text-2xl font-bold">
          {stats.bucket_stats.reduce((sum, count) => sum + count, 0)}
        </div>
      </div>
    </div>

    <div class="routing-table">
      <h3 class="text-lg font-semibold mb-2">Routing Table (K-Buckets)</h3>
      <div class="buckets-grid grid grid-cols-16 gap-1">
        {#each stats.bucket_stats as count, index}
          <div
            class="bucket w-4 h-4 {getBucketColor(count)} rounded-sm"
            title="Bucket {index}: {count} peers"
          />
        {/each}
      </div>
      <div class="legend flex gap-4 mt-2 text-xs text-gray-600">
        <span><span class="inline-block w-3 h-3 bg-gray-200 rounded-sm"></span> Empty</span>
        <span><span class="inline-block w-3 h-3 bg-blue-300 rounded-sm"></span> 1-4 peers</span>
        <span><span class="inline-block w-3 h-3 bg-blue-500 rounded-sm"></span> 5-9 peers</span>
        <span><span class="inline-block w-3 h-3 bg-blue-700 rounded-sm"></span> 10+ peers</span>
      </div>
    </div>
  {:else}
    <div class="loading">Loading DHT statistics...</div>
  {/if}
</div>
```

### 3.2 File Upload Component

**File: `src/lib/components/FileUpload.svelte`**

```svelte
<script lang="ts">
  import { invoke } from '@tauri-apps/api/tauri';
  import { listen } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/api/dialog';
  import { onMount } from 'svelte';
  import { Upload, Check, AlertCircle } from 'lucide-svelte';

  interface UploadProgress {
    file_name: string;
    cid: string;
    chunks_total: number;
    chunks_completed: number;
    bytes_uploaded: number;
    percentage: number;
  }

  let uploads: Map<string, UploadProgress> = new Map();
  let isDragging = false;

  onMount(async () => {
    const unsubscribe = await listen('upload-progress', (event) => {
      const progress = event.payload as UploadProgress;
      uploads.set(progress.cid, progress);
      uploads = uploads; // Trigger reactivity
    });

    return unsubscribe;
  });

  async function selectFiles() {
    const selected = await open({
      multiple: true,
      filters: [{
        name: 'All Files',
        extensions: ['*']
      }]
    });

    if (selected && Array.isArray(selected)) {
      for (const path of selected) {
        await uploadFile(path);
      }
    } else if (selected && typeof selected === 'string') {
      await uploadFile(selected);
    }
  }

  async function uploadFile(path: string) {
    try {
      const cid = await invoke('upload_file', { path });
      console.log('File uploaded with CID:', cid);
    } catch (error) {
      console.error('Upload failed:', error);
    }
  }

  function handleDragOver(event: DragEvent) {
    event.preventDefault();
    isDragging = true;
  }

  function handleDragLeave() {
    isDragging = false;
  }

  async function handleDrop(event: DragEvent) {
    event.preventDefault();
    isDragging = false;

    const files = event.dataTransfer?.files;
    if (files) {
      for (let i = 0; i < files.length; i++) {
        // Note: In Tauri, we need to use the file path
        // This requires additional platform-specific handling
        console.log('File dropped:', files[i].name);
      }
    }
  }
</script>

<div class="file-upload p-6 bg-white rounded-lg shadow">
  <h2 class="text-2xl font-bold mb-4">Upload Files to Network</h2>

  <div
    class="drop-zone border-2 border-dashed rounded-lg p-8 text-center transition-colors
           {isDragging ? 'border-blue-500 bg-blue-50' : 'border-gray-300'}"
    on:dragover={handleDragOver}
    on:dragleave={handleDragLeave}
    on:drop={handleDrop}
  >
    <Upload class="w-12 h-12 mx-auto mb-4 text-gray-400" />
    <p class="text-lg mb-2">Drag and drop files here</p>
    <p class="text-sm text-gray-500 mb-4">or</p>
    <button
      class="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700"
      on:click={selectFiles}
    >
      Select Files
    </button>
  </div>

  {#if uploads.size > 0}
    <div class="uploads-list mt-6">
      <h3 class="text-lg font-semibold mb-3">Active Uploads</h3>

      {#each Array.from(uploads.values()) as upload}
        <div class="upload-item p-3 mb-2 bg-gray-50 rounded">
          <div class="flex justify-between items-center mb-2">
            <span class="font-medium">{upload.file_name}</span>
            {#if upload.percentage >= 100}
              <Check class="w-5 h-5 text-green-600" />
            {:else}
              <span class="text-sm text-gray-600">{upload.percentage.toFixed(1)}%</span>
            {/if}
          </div>

          <div class="progress-bar w-full h-2 bg-gray-200 rounded-full overflow-hidden">
            <div
              class="progress-fill h-full bg-blue-600 transition-all duration-300"
              style="width: {upload.percentage}%"
            />
          </div>

          <div class="details flex justify-between mt-2 text-xs text-gray-500">
            <span>{upload.chunks_completed} / {upload.chunks_total} chunks</span>
            <span>CID: {upload.cid.slice(0, 12)}...</span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
```

### 3.3 File Download Component

**File: `src/lib/components/FileDownload.svelte`**

```svelte
<script lang="ts">
  import { invoke } from '@tauri-apps/api/tauri';
  import { listen } from '@tauri-apps/api/event';
  import { save } from '@tauri-apps/api/dialog';
  import { onMount } from 'svelte';
  import { Download, Search } from 'lucide-svelte';

  interface DownloadProgress {
    cid: string;
    file_name: string;
    total_size: number;
    downloaded: number;
    percentage: number;
    peers_count: number;
  }

  let cidInput = '';
  let downloads: Map<string, DownloadProgress> = new Map();
  let providers: string[] = [];
  let searching = false;

  onMount(async () => {
    const unsubscribe = await listen('download-progress', (event) => {
      const progress = event.payload as DownloadProgress;
      downloads.set(progress.cid, progress);
      downloads = downloads;
    });

    return unsubscribe;
  });

  async function searchProviders() {
    if (!cidInput) return;

    searching = true;
    try {
      providers = await invoke('find_content_providers', { cid: cidInput });
    } catch (error) {
      console.error('Provider search failed:', error);
      providers = [];
    } finally {
      searching = false;
    }
  }

  async function startDownload() {
    if (!cidInput) return;

    const outputPath = await save({
      filters: [{
        name: 'All Files',
        extensions: ['*']
      }]
    });

    if (!outputPath) return;

    try {
      await invoke('download_file', {
        cid: cidInput,
        outputPath
      });
    } catch (error) {
      console.error('Download failed:', error);
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(2) + ' KB';
    if (bytes < 1024 * 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(2) + ' MB';
    return (bytes / (1024 * 1024 * 1024)).toFixed(2) + ' GB';
  }
</script>

<div class="file-download p-6 bg-white rounded-lg shadow">
  <h2 class="text-2xl font-bold mb-4">Download Files from Network</h2>

  <div class="search-section mb-6">
    <label class="block text-sm font-medium mb-2">Content ID (CID)</label>
    <div class="flex gap-2">
      <input
        type="text"
        bind:value={cidInput}
        placeholder="QmXxx... or bafxxx..."
        class="flex-1 px-3 py-2 border rounded focus:outline-none focus:ring-2 focus:ring-blue-500"
      />
      <button
        class="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 flex items-center gap-2"
        on:click={searchProviders}
        disabled={searching}
      >
        <Search class="w-4 h-4" />
        {searching ? 'Searching...' : 'Find Providers'}
      </button>
    </div>
  </div>

  {#if providers.length > 0}
    <div class="providers-section mb-6">
      <h3 class="text-lg font-semibold mb-2">Available Providers</h3>
      <div class="providers-list space-y-2">
        {#each providers as provider}
          <div class="provider-item p-2 bg-gray-50 rounded text-sm font-mono">
            {provider}
          </div>
        {/each}
      </div>

      <button
        class="mt-4 px-4 py-2 bg-green-600 text-white rounded hover:bg-green-700 flex items-center gap-2"
        on:click={startDownload}
      >
        <Download class="w-4 h-4" />
        Start Download
      </button>
    </div>
  {/if}

  {#if downloads.size > 0}
    <div class="downloads-list mt-6">
      <h3 class="text-lg font-semibold mb-3">Active Downloads</h3>

      {#each Array.from(downloads.values()) as download}
        <div class="download-item p-3 mb-2 bg-gray-50 rounded">
          <div class="flex justify-between items-center mb-2">
            <span class="font-medium">{download.file_name}</span>
            <span class="text-sm text-gray-600">{download.percentage.toFixed(1)}%</span>
          </div>

          <div class="progress-bar w-full h-2 bg-gray-200 rounded-full overflow-hidden">
            <div
              class="progress-fill h-full bg-green-600 transition-all duration-300"
              style="width: {download.percentage}%"
            />
          </div>

          <div class="details flex justify-between mt-2 text-xs text-gray-500">
            <span>{formatBytes(download.downloaded)} / {formatBytes(download.total_size)}</span>
            <span>{download.peers_count} peers</span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
```

## 4. Implementation Roadmap

### Phase 1: Core DHT (Weeks 1-2)
1. Implement Kademlia DHT with libp2p
2. Set up routing table and peer discovery
3. Implement basic RPC handlers (FIND_NODE, FIND_VALUE, STORE)
4. Test with local network

**Deliverables:**
- Working Kademlia DHT
- Routing table with K-bucket implementation
- Basic peer discovery
- Unit tests for DHT operations

**Key Metrics:**
- Successfully maintain routing table with 20+ peers
- Query response time < 2 seconds
- Peer discovery rate > 5 peers/minute

### Phase 2: Content Layer (Weeks 3-4)
1. Implement CID generation and verification
2. Build file chunking system
3. Create Merkle DAG structures
4. Implement blockstore and datastore

**Deliverables:**
- CID generation library
- Chunking algorithm with configurable size
- DAG construction and traversal
- Local storage (blockstore + datastore)

**Key Metrics:**
- CID generation < 100ms for 1MB files
- Chunking throughput > 50 MB/s
- Storage operations < 10ms latency

### Phase 3: Network Integration (Weeks 5-6)
1. Integrate libp2p transport layer
2. Implement Bitswap-like block exchange
3. Add peer discovery mechanisms (mDNS, bootstrap)
4. Test file transfer between peers

**Deliverables:**
- Full libp2p integration
- Block exchange protocol
- Multi-peer file transfer
- Network diagnostic tools

**Key Metrics:**
- File transfer speed > 1 MB/s between peers
- Successful peer discovery via mDNS
- Connection success rate > 90%

### Phase 4: Replication (Weeks 7-8)
1. Implement replication strategy
2. Build replication scheduler
3. Add content pinning
4. Test network resilience

**Deliverables:**
- Replication strategy with configurable redundancy
- Automated replication scheduler
- Pin management system
- Network resilience tests

**Key Metrics:**
- Target replication factor achieved within 5 minutes
- Pin retention rate > 99%
- Recovery from node failures < 30 seconds

### Phase 5: Frontend Integration (Weeks 9-10)
1. Create Tauri commands
2. Build event system
3. Implement Svelte UI components
4. End-to-end testing

**Deliverables:**
- Complete Tauri command API
- Real-time event system
- Svelte UI components
- E2E test suite

**Key Metrics:**
- UI response time < 100ms
- Event latency < 50ms
- E2E test coverage > 80%

### Phase 6: Optimization (Weeks 11-12)
1. Performance tuning
2. Error handling improvements
3. Add metrics and monitoring
4. Production deployment preparation

**Deliverables:**
- Performance optimization report
- Comprehensive error handling
- Monitoring dashboard
- Deployment documentation

**Key Metrics:**
- Upload speed > 10 MB/s
- Download speed > 10 MB/s
- Memory usage < 500 MB
- CPU usage < 30% during idle

## 5. Testing Strategy

### Unit Tests

**DHT Layer:**
- `test_kademlia_routing()` - Verify routing table operations
- `test_provide_content()` - Test content announcement
- `test_find_providers()` - Test provider lookup
- `test_put_get_record()` - Test DHT record storage
- `test_bootstrap()` - Test network bootstrap

**Content Layer:**
- `test_cid_generation()` - Verify CID correctness
- `test_chunking()` - Test file chunking
- `test_dag_construction()` - Test DAG building
- `test_cid_verification()` - Test integrity checking

**Storage Layer:**
- `test_blockstore_operations()` - Test block storage
- `test_metadata_storage()` - Test metadata operations
- `test_pinning()` - Test pin management

### Integration Tests

**Multi-Peer Transfer:**
```rust
#[tokio::test]
async fn test_multi_peer_file_transfer() {
    // Start 5 peer nodes
    let nodes = start_test_nodes(5).await;

    // Upload file on node 0
    let cid = nodes[0].upload_file("test.dat").await.unwrap();

    // Wait for DHT propagation
    sleep(Duration::from_secs(5)).await;

    // Download on node 4
    let result = nodes[4].download_file(cid).await;

    assert!(result.is_ok());
    assert_eq!(verify_file_hash(&result.unwrap()), true);
}
```

**DHT Replication:**
```rust
#[tokio::test]
async fn test_dht_replication() {
    let nodes = start_test_nodes(50).await;

    // Store content
    let cid = nodes[0].provide_content("test data").await.unwrap();

    // Wait for replication
    sleep(Duration::from_secs(10)).await;

    // Query providers
    let providers = nodes[25].find_providers(cid).await.unwrap();

    // Should have at least 20 replicas (replication factor)
    assert!(providers.len() >= 20);
}
```

**Network Resilience:**
```rust
#[tokio::test]
async fn test_network_resilience() {
    let mut nodes = start_test_nodes(20).await;

    let cid = nodes[0].upload_file("important.dat").await.unwrap();

    // Wait for replication
    sleep(Duration::from_secs(10)).await;

    // Kill 5 random nodes
    for _ in 0..5 {
        let idx = rand::random::<usize>() % nodes.len();
        nodes.remove(idx);
    }

    // Should still be able to download
    let result = nodes[5].download_file(cid).await;
    assert!(result.is_ok());
}
```

### End-to-End Tests

**Upload Workflow:**
1. User selects file
2. File is chunked
3. Chunks stored locally
4. DAG constructed
5. Announced to DHT
6. Verify providers found

**Download Workflow:**
1. User enters CID
2. Query DHT for providers
3. Download chunks from peers
4. Verify chunk integrity
5. Reassemble file
6. Verify final file hash

**Network Monitoring:**
1. Start application
2. Connect to network
3. Monitor peer count
4. Monitor DHT queries
5. Monitor bandwidth usage
6. Verify UI updates

## 6. Key Dependencies

### Rust Crates
```toml
[dependencies]
libp2p = { version = "0.54", features = ["kad", "tcp", "noise", "yamux", "mdns"] }
tokio = { version = "1.35", features = ["full"] }
sled = "0.34"
cid = "0.11"
multihash = "0.19"
sha2 = "0.10"
bytes = "1.5"
bincode = "1.3"
serde = { version = "1.0", features = ["derive"] }
tauri = { version = "2.0", features = ["shell-open"] }
```

### NPM Packages
```json
{
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "svelte": "^4.2.0",
    "lucide-svelte": "^0.294.0",
    "tailwindcss": "^3.3.0"
  }
}
```

## 7. Performance Considerations

### Optimization Strategies

1. **Chunk Size Selection:**
   - Default: 256 KB (balance between overhead and granularity)
   - Configurable based on file size
   - Larger chunks for big files (up to 1 MB)

2. **Concurrent Operations:**
   - Parallel chunk upload (max 10 concurrent)
   - Parallel chunk download (max 10 concurrent)
   - Batched DHT announcements

3. **Caching:**
   - In-memory LRU cache for frequently accessed blocks
   - DHT routing table cache
   - Provider cache with TTL

4. **Network Efficiency:**
   - Connection pooling
   - Persistent connections to frequent peers
   - Batch DHT queries where possible

## 8. Security Considerations

### Content Integrity
- All content addressed by cryptographic hash (SHA-256)
- CID verification on every chunk download
- Reject invalid chunks immediately

### Network Security
- Noise protocol for encrypted transport
- Peer authentication via libp2p
- Rate limiting on DHT queries
- DoS protection via connection limits

### Privacy
- Content IDs reveal file hash (not reversible)
- Peer IDs are pseudonymous
- No centralized tracking
- Optional proxy routing for anonymity

## 9. Monitoring & Observability

### Metrics to Track
- **DHT Metrics:**
  - Routing table size
  - Query success rate
  - Query latency
  - Provider record count

- **Storage Metrics:**
  - Total blocks stored
  - Storage utilization
  - Block retrieval latency
  - Pin count

- **Network Metrics:**
  - Connected peers
  - Bandwidth usage (up/down)
  - Connection success rate
  - Block transfer rate

- **Replication Metrics:**
  - Replication factor per CID
  - Replication latency
  - Failed replication attempts
  - Replication queue size

## Conclusion

This architecture provides a robust foundation for true DHT-based distributed storage in Chiral Network. The modular design allows for incremental implementation and testing while maintaining clean separation between layers. The system leverages proven technologies (libp2p, Kademlia) while adding application-specific optimizations for file sharing use cases.

Key advantages of this approach:

1. **Decentralization:** True P2P with no single point of failure
2. **Scalability:** DHT scales logarithmically with network size
3. **Content Addressing:** Immutable content with cryptographic verification
4. **Replication:** Automatic redundancy for availability
5. **Modularity:** Clean separation enables independent testing and optimization

The 12-week roadmap provides a clear path from core DHT implementation through to production-ready distributed storage system.
