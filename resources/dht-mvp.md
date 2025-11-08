# DHT-Based Distributed Storage - MVP Specification

## Executive Summary

This document defines the Minimum Viable Product (MVP) for implementing DHT-based distributed storage in Chiral Network. The MVP focuses on proving the core concept: **files can be distributed, discovered, and retrieved across a P2P network using a DHT**.

**Timeline:** 4 weeks
**Lines of Code:** ~1,000 (800 Rust + 200 Frontend)
**Core Functionality:** Upload file → Get CID → Share CID → Download from peer

This MVP intentionally strips away all non-essential features to focus on demonstrating the fundamental distributed storage workflow.

## MVP Philosophy

### What Makes This an MVP?

**Single Goal:** Prove that DHT-based file distribution works between two peers.

**Core Questions Answered:**
- ✅ Can we chunk files efficiently?
- ✅ Can we generate content-addressed identifiers (CIDs)?
- ✅ Can we announce content availability to a DHT?
- ✅ Can peers discover each other via DHT?
- ✅ Can peers transfer chunks over libp2p?
- ✅ Can we reassemble files correctly?

**Deferred Questions:**
- ❌ How fast is it? (Performance optimization - later)
- ❌ Is it reliable? (Replication - later)
- ❌ Is the UI polished? (UX improvements - later)
- ❌ Can it handle many files? (Scalability - later)

### Simplification Strategy

| Feature | Full Proposal | MVP | Rationale |
|---------|--------------|-----|-----------|
| **DHT** | Custom routing table | libp2p default Kademlia | No need to reinvent DHT |
| **Content Structure** | Merkle DAG | Flat chunk list | Prove chunks work first |
| **Storage** | Blockstore + Datastore | Blockstore only | One database is enough |
| **Replication** | Automatic 20-node replication | No replication | Uploader keeps file |
| **UI** | 3 components + visualization | Single page | Focus on backend |
| **Events** | Real-time progress | Blocking operations | Simpler state management |
| **Network** | mDNS + Bootstrap | Bootstrap only | Reduce complexity |

## Simplified Architecture

```
┌─────────────────────────────────────────────┐
│         Svelte Frontend (1 file)            │
│   [Upload Button] [CID Input] [Download]   │
└──────────────────┬──────────────────────────┘
                   │ Tauri Commands (2)
┌──────────────────┴──────────────────────────┐
│         Tauri Integration Layer             │
│   upload_file()      download_file()        │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────┴──────────────────────────┐
│           Rust Backend (6 files)            │
├─────────────────────────────────────────────┤
│  DHT (libp2p Kademlia)  │  Content (CID)    │
│  Storage (sled)         │  Network (libp2p) │
└─────────────────────────────────────────────┘
```

**Module Count:** 6 Rust files + 1 Svelte file + main entry point

## MVP Backend Implementation

### Module Structure

```
src-tauri/src/
├── lib.rs              # 50 lines  - Tauri app entry
├── dht.rs              # 200 lines - DHT wrapper
├── content.rs          # 150 lines - CID & chunking
├── storage.rs          # 100 lines - Blockstore
├── network.rs          # 200 lines - libp2p setup
└── commands.rs         # 150 lines - 2 Tauri commands
```

**Total:** ~850 lines of Rust

### 1. DHT Wrapper (`dht.rs`)

**Purpose:** Minimal wrapper around libp2p Kademlia for content announcement and discovery.

```rust
use libp2p::kad::{Kademlia, KademliaConfig, KademliaEvent, Record, store::MemoryStore};
use libp2p::{PeerId, identity, Swarm};
use std::time::Duration;
use cid::Cid;

/// Minimal DHT manager for MVP
pub struct DhtManager {
    swarm: Swarm<Kademlia<MemoryStore>>,
}

impl DhtManager {
    /// Create new DHT with bootstrap nodes
    pub fn new(bootstrap_peers: Vec<(PeerId, String)>) -> Result<Self, DhtError> {
        // Generate local peer ID
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        // Configure Kademlia with minimal settings
        let mut config = KademliaConfig::default();
        config.set_query_timeout(Duration::from_secs(30));

        // Create store and Kademlia behavior
        let store = MemoryStore::new(local_peer_id);
        let mut kademlia = Kademlia::with_config(local_peer_id, store, config);

        // Add bootstrap nodes
        for (peer_id, addr) in bootstrap_peers {
            kademlia.add_address(&peer_id, addr.parse()?);
        }

        // Bootstrap the DHT
        kademlia.bootstrap()?;

        // Create swarm (simplified transport)
        let transport = libp2p::tcp::tokio::Transport::new(Default::default());
        let swarm = Swarm::new(transport, kademlia, local_peer_id);

        Ok(Self { swarm })
    }

    /// Announce that we have content with this CID
    pub async fn provide(&mut self, cid: &Cid) -> Result<(), DhtError> {
        let key = cid.to_bytes().into();
        self.swarm.behaviour_mut().start_providing(key)?;

        // Wait for providing to complete
        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::Behaviour(KademliaEvent::OutboundQueryProgressed {
                    result: QueryResult::StartProviding(Ok(_)), ..
                }) => {
                    return Ok(());
                }
                SwarmEvent::Behaviour(KademliaEvent::OutboundQueryProgressed {
                    result: QueryResult::StartProviding(Err(e)), ..
                }) => {
                    return Err(DhtError::ProvideFailed(e.to_string()));
                }
                _ => continue,
            }
        }
    }

    /// Find peers that have content with this CID
    pub async fn find_providers(&mut self, cid: &Cid) -> Result<Vec<PeerId>, DhtError> {
        let key = cid.to_bytes().into();
        self.swarm.behaviour_mut().get_providers(key);

        let mut providers = Vec::new();
        let timeout = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::Behaviour(KademliaEvent::OutboundQueryProgressed {
                            result: QueryResult::GetProviders(Ok(ok)), ..
                        }) => {
                            providers.extend(ok.providers);
                            if providers.len() >= 5 {
                                return Ok(providers);
                            }
                        }
                        _ => continue,
                    }
                }
                _ = &mut timeout => {
                    if providers.is_empty() {
                        return Err(DhtError::NoProvidersFound);
                    }
                    return Ok(providers);
                }
            }
        }
    }

    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }
}

#[derive(Debug)]
pub enum DhtError {
    ProvideFailed(String),
    NoProvidersFound,
    BootstrapFailed,
    ConnectionError(String),
}

impl std::error::Error for DhtError {}

impl std::fmt::Display for DhtError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DhtError::ProvideFailed(e) => write!(f, "Failed to provide content: {}", e),
            DhtError::NoProvidersFound => write!(f, "No providers found for content"),
            DhtError::BootstrapFailed => write!(f, "Failed to bootstrap DHT"),
            DhtError::ConnectionError(e) => write!(f, "Connection error: {}", e),
        }
    }
}
```

**Key Simplifications:**
- Uses libp2p's built-in Kademlia (no custom routing table)
- Only 2 operations: `provide()` and `find_providers()`
- Hardcoded 30-second timeouts
- Returns up to 5 providers (enough for MVP)

### 2. Content Layer (`content.rs`)

**Purpose:** CID generation and file chunking.

```rust
use cid::Cid;
use multihash::{Code, MultihashDigest};
use sha2::{Sha256, Digest};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use serde::{Serialize, Deserialize};

const CHUNK_SIZE: usize = 256 * 1024; // 256 KB

/// Generate CID from bytes
pub fn generate_cid(data: &[u8]) -> Cid {
    let hash = Code::Sha2_256.digest(data);
    Cid::new_v1(0x55, hash) // 0x55 = raw codec
}

/// Chunk a file into fixed-size pieces
pub async fn chunk_file(path: &Path) -> Result<Vec<Chunk>, ContentError> {
    let mut file = File::open(path).await?;
    let mut chunks = Vec::new();
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut chunk_index = 0;

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }

        let chunk_data = buffer[..n].to_vec();
        let cid = generate_cid(&chunk_data);

        chunks.push(Chunk {
            index: chunk_index,
            cid,
            data: chunk_data,
            size: n,
        });

        chunk_index += 1;
    }

    Ok(chunks)
}

/// Simple file metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub root_cid: Cid,
    pub name: String,
    pub size: u64,
    pub chunk_cids: Vec<Cid>,
    pub created_at: u64,
}

impl FileInfo {
    pub fn new(name: String, chunks: &[Chunk]) -> Self {
        let chunk_cids: Vec<Cid> = chunks.iter().map(|c| c.cid).collect();

        // Root CID is hash of all chunk CIDs concatenated
        let mut hasher = Sha256::new();
        for cid in &chunk_cids {
            hasher.update(cid.to_bytes());
        }
        let root_hash = Code::Sha2_256.digest(&hasher.finalize());
        let root_cid = Cid::new_v1(0x55, root_hash);

        let size: u64 = chunks.iter().map(|c| c.size as u64).sum();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            root_cid,
            name,
            size,
            chunk_cids,
            created_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub cid: Cid,
    pub data: Vec<u8>,
    pub size: usize,
}

/// Verify that data matches its CID
pub fn verify_chunk(data: &[u8], expected_cid: &Cid) -> bool {
    let computed_cid = generate_cid(data);
    computed_cid == *expected_cid
}

#[derive(Debug)]
pub enum ContentError {
    IoError(std::io::Error),
    InvalidCid,
}

impl From<std::io::Error> for ContentError {
    fn from(e: std::io::Error) -> Self {
        ContentError::IoError(e)
    }
}

impl std::error::Error for ContentError {}

impl std::fmt::Display for ContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ContentError::IoError(e) => write!(f, "IO error: {}", e),
            ContentError::InvalidCid => write!(f, "Invalid CID"),
        }
    }
}
```

**Key Simplifications:**
- Fixed 256KB chunk size (no configuration)
- Flat chunk list (no Merkle DAG)
- Root CID is simple hash of chunk CIDs
- No streaming - reads entire chunks into memory

### 3. Storage Layer (`storage.rs`)

**Purpose:** Simple persistent blockstore using sled.

```rust
use sled::Db;
use cid::Cid;
use std::path::Path;

/// Simple key-value blockstore
pub struct Blockstore {
    db: Db,
}

impl Blockstore {
    /// Open or create blockstore at path
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Store a block (chunk)
    pub fn put(&self, cid: &Cid, data: &[u8]) -> Result<(), StorageError> {
        let key = cid.to_bytes();
        self.db.insert(key, data)?;
        self.db.flush()?;
        Ok(())
    }

    /// Retrieve a block
    pub fn get(&self, cid: &Cid) -> Result<Option<Vec<u8>>, StorageError> {
        let key = cid.to_bytes();
        match self.db.get(key)? {
            Some(bytes) => Ok(Some(bytes.to_vec())),
            None => Ok(None),
        }
    }

    /// Check if block exists
    pub fn has(&self, cid: &Cid) -> Result<bool, StorageError> {
        let key = cid.to_bytes();
        Ok(self.db.contains_key(key)?)
    }

    /// Store file metadata as special key
    pub fn put_file_info(&self, info: &crate::content::FileInfo) -> Result<(), StorageError> {
        let key = format!("fileinfo:{}", info.root_cid);
        let value = serde_json::to_vec(info)?;
        self.db.insert(key.as_bytes(), value)?;
        self.db.flush()?;
        Ok(())
    }

    /// Get file metadata
    pub fn get_file_info(&self, root_cid: &Cid) -> Result<Option<crate::content::FileInfo>, StorageError> {
        let key = format!("fileinfo:{}", root_cid);
        match self.db.get(key.as_bytes())? {
            Some(bytes) => {
                let info = serde_json::from_slice(&bytes)?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }
}

#[derive(Debug)]
pub enum StorageError {
    DatabaseError(sled::Error),
    SerializationError(serde_json::Error),
}

impl From<sled::Error> for StorageError {
    fn from(e: sled::Error) -> Self {
        StorageError::DatabaseError(e)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(e: serde_json::Error) -> Self {
        StorageError::SerializationError(e)
    }
}

impl std::error::Error for StorageError {}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            StorageError::DatabaseError(e) => write!(f, "Database error: {}", e),
            StorageError::SerializationError(e) => write!(f, "Serialization error: {}", e),
        }
    }
}
```

**Key Simplifications:**
- Single database for everything
- No separate metadata store
- No pin management
- Synchronous flush on every write (simpler, slightly slower)

### 4. Network Layer (`network.rs`)

**Purpose:** Minimal libp2p setup for peer-to-peer block exchange.

```rust
use libp2p::{
    core::upgrade,
    noise, tcp, yamux, Transport, PeerId, Multiaddr,
    request_response::{self, ProtocolSupport, RequestResponse, RequestResponseEvent},
};
use std::time::Duration;
use cid::Cid;
use serde::{Serialize, Deserialize};

// Simple request/response protocol for blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRequest {
    pub cid: Cid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockResponse {
    pub data: Option<Vec<u8>>,
}

/// Minimal network manager
pub struct NetworkManager {
    swarm: Swarm<RequestResponse<BlockCodec>>,
}

impl NetworkManager {
    pub fn new() -> Result<Self, NetworkError> {
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        // Build transport: TCP + Noise encryption + Yamux multiplexing
        let transport = tcp::tokio::Transport::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::NoiseAuthenticated::xx(&local_key)?)
            .multiplex(yamux::YamuxConfig::default())
            .boxed();

        // Request-response protocol for block exchange
        let protocol = [(b"/chiral/blocks/1.0.0", ProtocolSupport::Full)];
        let cfg = request_response::Config::default()
            .with_request_timeout(Duration::from_secs(30));
        let behaviour = RequestResponse::new(BlockCodec, protocol, cfg);

        let swarm = Swarm::new(transport, behaviour, local_peer_id);

        Ok(Self { swarm })
    }

    /// Connect to a peer
    pub async fn connect(&mut self, peer_id: PeerId, addr: Multiaddr) -> Result<(), NetworkError> {
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Request a block from a peer
    pub async fn request_block(&mut self, peer: PeerId, cid: Cid) -> Result<Vec<u8>, NetworkError> {
        let request = BlockRequest { cid };
        let request_id = self.swarm.behaviour_mut().send_request(&peer, request);

        // Wait for response
        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::Behaviour(RequestResponseEvent::Message {
                    message: RequestResponseMessage::Response { response, .. },
                    ..
                }) => {
                    if let Some(data) = response.data {
                        return Ok(data);
                    } else {
                        return Err(NetworkError::BlockNotFound);
                    }
                }
                _ => continue,
            }
        }
    }

    /// Handle incoming block requests (runs in background)
    pub async fn handle_requests(&mut self, blockstore: Arc<Blockstore>) {
        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::Behaviour(RequestResponseEvent::Message {
                    message: RequestResponseMessage::Request { request, channel, .. },
                    ..
                }) => {
                    // Look up block in our store
                    let data = blockstore.get(&request.cid).ok().flatten();
                    let response = BlockResponse { data };

                    // Send response
                    self.swarm.behaviour_mut()
                        .send_response(channel, response)
                        .ok();
                }
                _ => {}
            }
        }
    }
}

// Codec for serializing requests/responses
struct BlockCodec;

impl request_response::Codec for BlockCodec {
    type Protocol = &'static [u8];
    type Request = BlockRequest;
    type Response = BlockResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T)
        -> std::io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        bincode::deserialize(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T)
        -> std::io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        bincode::deserialize(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(&mut self, _: &Self::Protocol, io: &mut T, req: Self::Request)
        -> std::io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes = bincode::serialize(&req)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        io.write_all(&bytes).await
    }

    async fn write_response<T>(&mut self, _: &Self::Protocol, io: &mut T, res: Self::Response)
        -> std::io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes = bincode::serialize(&res)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        io.write_all(&bytes).await
    }
}

#[derive(Debug)]
pub enum NetworkError {
    ConnectionFailed(String),
    BlockNotFound,
    RequestTimeout,
}

impl std::error::Error for NetworkError {}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NetworkError::ConnectionFailed(e) => write!(f, "Connection failed: {}", e),
            NetworkError::BlockNotFound => write!(f, "Block not found on peer"),
            NetworkError::RequestTimeout => write!(f, "Request timed out"),
        }
    }
}
```

**Key Simplifications:**
- Simple request/response protocol (not Bitswap)
- Synchronous block requests (one at a time)
- No connection pooling
- Fixed 30-second timeout

### 5. Tauri Commands (`commands.rs`)

**Purpose:** Two blocking commands for upload and download.

```rust
use tauri::State;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::path::Path;

pub struct AppState {
    pub blockstore: Arc<Blockstore>,
    pub dht: Arc<Mutex<DhtManager>>,
    pub network: Arc<Mutex<NetworkManager>>,
}

/// Upload file and announce to DHT
#[tauri::command]
pub async fn upload_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let file_path = Path::new(&path);

    // Chunk the file
    let chunks = crate::content::chunk_file(file_path)
        .await
        .map_err(|e| format!("Failed to chunk file: {}", e))?;

    // Create file info
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let file_info = crate::content::FileInfo::new(file_name, &chunks);

    // Store all chunks
    for chunk in &chunks {
        state.blockstore.put(&chunk.cid, &chunk.data)
            .map_err(|e| format!("Failed to store chunk: {}", e))?;
    }

    // Store file metadata
    state.blockstore.put_file_info(&file_info)
        .map_err(|e| format!("Failed to store metadata: {}", e))?;

    // Announce all chunks to DHT
    let mut dht = state.dht.lock().await;
    for chunk in &chunks {
        dht.provide(&chunk.cid)
            .await
            .map_err(|e| format!("Failed to announce chunk: {}", e))?;
    }

    // Also announce root CID
    dht.provide(&file_info.root_cid)
        .await
        .map_err(|e| format!("Failed to announce file: {}", e))?;

    Ok(file_info.root_cid.to_string())
}

/// Download file from DHT
#[tauri::command]
pub async fn download_file(
    cid_str: String,
    output_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Parse CID
    let root_cid = Cid::try_from(cid_str.as_str())
        .map_err(|e| format!("Invalid CID: {}", e))?;

    // Try to get file info from local store first
    let file_info = if let Some(info) = state.blockstore.get_file_info(&root_cid)
        .map_err(|e| format!("Storage error: {}", e))?
    {
        info
    } else {
        // Need to find who has the metadata
        let mut dht = state.dht.lock().await;
        let providers = dht.find_providers(&root_cid)
            .await
            .map_err(|e| format!("Failed to find providers: {}", e))?;

        if providers.is_empty() {
            return Err("No providers found for this file".to_string());
        }

        // Request metadata from first provider
        let mut network = state.network.lock().await;
        let metadata_bytes = network.request_block(providers[0], root_cid)
            .await
            .map_err(|e| format!("Failed to get metadata: {}", e))?;

        serde_json::from_slice(&metadata_bytes)
            .map_err(|e| format!("Invalid metadata: {}", e))?
    };

    // Download all chunks
    let mut file_data = Vec::new();

    for chunk_cid in &file_info.chunk_cids {
        // Check if we have it locally
        let chunk_data = if let Some(data) = state.blockstore.get(chunk_cid)
            .map_err(|e| format!("Storage error: {}", e))?
        {
            data
        } else {
            // Find providers for this chunk
            let mut dht = state.dht.lock().await;
            let providers = dht.find_providers(chunk_cid)
                .await
                .map_err(|e| format!("Failed to find chunk providers: {}", e))?;

            if providers.is_empty() {
                return Err(format!("No providers for chunk {}", chunk_cid));
            }

            // Download from first provider
            let mut network = state.network.lock().await;
            let data = network.request_block(providers[0], *chunk_cid)
                .await
                .map_err(|e| format!("Failed to download chunk: {}", e))?;

            // Verify chunk integrity
            if !crate::content::verify_chunk(&data, chunk_cid) {
                return Err(format!("Chunk {} failed verification", chunk_cid));
            }

            // Store locally
            state.blockstore.put(chunk_cid, &data)
                .map_err(|e| format!("Failed to store chunk: {}", e))?;

            data
        };

        file_data.extend(chunk_data);
    }

    // Write to output file
    tokio::fs::write(&output_path, file_data)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(())
}
```

**Key Simplifications:**
- Blocking commands (no progress events)
- Sequential chunk downloads (no parallelism)
- No error recovery
- Simple string error messages

### 6. Main Entry (`lib.rs`)

```rust
mod dht;
mod content;
mod storage;
mod network;
mod commands;

use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Initialize storage
            let storage_path = app.path_resolver()
                .app_data_dir()
                .unwrap()
                .join("blockstore");
            let blockstore = Arc::new(storage::Blockstore::open(&storage_path)?);

            // Initialize DHT with bootstrap nodes
            let bootstrap_nodes = vec![
                // Add bootstrap node addresses here
                // (PeerId, "/ip4/x.x.x.x/tcp/4001".to_string())
            ];
            let dht = Arc::new(Mutex::new(dht::DhtManager::new(bootstrap_nodes)?));

            // Initialize network
            let network = Arc::new(Mutex::new(network::NetworkManager::new()?));

            // Store in app state
            app.manage(commands::AppState {
                blockstore,
                dht,
                network,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::upload_file,
            commands::download_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

## MVP Frontend

### Single Page UI (`src/App.svelte`)

```svelte
<script lang="ts">
  import { invoke } from '@tauri-apps/api/tauri';
  import { open, save } from '@tauri-apps/api/dialog';

  let status = 'Ready';
  let cidInput = '';
  let uploadedCid = '';
  let isUploading = false;
  let isDownloading = false;

  async function handleUpload() {
    try {
      isUploading = true;
      status = 'Selecting file...';

      const filePath = await open({
        multiple: false,
        filters: [{ name: 'All Files', extensions: ['*'] }]
      });

      if (!filePath || typeof filePath !== 'string') {
        status = 'No file selected';
        return;
      }

      status = 'Uploading...';
      const cid = await invoke('upload_file', { path: filePath });

      uploadedCid = cid as string;
      status = `Upload complete! CID: ${uploadedCid}`;
    } catch (error) {
      status = `Upload failed: ${error}`;
      console.error(error);
    } finally {
      isUploading = false;
    }
  }

  async function handleDownload() {
    if (!cidInput.trim()) {
      status = 'Please enter a CID';
      return;
    }

    try {
      isDownloading = true;
      status = 'Selecting download location...';

      const outputPath = await save({
        filters: [{ name: 'All Files', extensions: ['*'] }]
      });

      if (!outputPath) {
        status = 'Download cancelled';
        return;
      }

      status = 'Downloading...';
      await invoke('download_file', {
        cidStr: cidInput,
        outputPath
      });

      status = `Download complete! Saved to: ${outputPath}`;
    } catch (error) {
      status = `Download failed: ${error}`;
      console.error(error);
    } finally {
      isDownloading = false;
    }
  }

  function copyCid() {
    navigator.clipboard.writeText(uploadedCid);
    status = 'CID copied to clipboard!';
  }
</script>

<main class="container">
  <h1>Chiral Network - DHT File Sharing</h1>

  <section class="upload-section">
    <h2>Upload File</h2>
    <button
      on:click={handleUpload}
      disabled={isUploading}
      class="btn btn-primary"
    >
      {isUploading ? 'Uploading...' : 'Select File to Upload'}
    </button>

    {#if uploadedCid}
      <div class="cid-display">
        <strong>Your CID:</strong>
        <code>{uploadedCid}</code>
        <button on:click={copyCid} class="btn btn-small">Copy</button>
      </div>
    {/if}
  </section>

  <section class="download-section">
    <h2>Download File</h2>
    <div class="input-group">
      <input
        type="text"
        bind:value={cidInput}
        placeholder="Enter CID to download..."
        disabled={isDownloading}
        class="cid-input"
      />
      <button
        on:click={handleDownload}
        disabled={isDownloading || !cidInput.trim()}
        class="btn btn-primary"
      >
        {isDownloading ? 'Downloading...' : 'Download'}
      </button>
    </div>
  </section>

  <div class="status-bar" class:error={status.includes('failed')}>
    <strong>Status:</strong> {status}
  </div>
</main>

<style>
  .container {
    max-width: 800px;
    margin: 0 auto;
    padding: 2rem;
    font-family: system-ui, -apple-system, sans-serif;
  }

  h1 {
    text-align: center;
    color: #333;
    margin-bottom: 2rem;
  }

  section {
    background: #f5f5f5;
    padding: 1.5rem;
    margin-bottom: 1.5rem;
    border-radius: 8px;
  }

  h2 {
    margin-top: 0;
    color: #555;
  }

  .btn {
    padding: 0.75rem 1.5rem;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 1rem;
    transition: opacity 0.2s;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-primary {
    background: #007bff;
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: #0056b3;
  }

  .btn-small {
    padding: 0.25rem 0.75rem;
    font-size: 0.875rem;
    margin-left: 0.5rem;
  }

  .cid-display {
    margin-top: 1rem;
    padding: 1rem;
    background: white;
    border-radius: 4px;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  code {
    flex: 1;
    padding: 0.5rem;
    background: #f0f0f0;
    border-radius: 4px;
    font-family: 'Courier New', monospace;
    font-size: 0.875rem;
    word-break: break-all;
  }

  .input-group {
    display: flex;
    gap: 0.5rem;
  }

  .cid-input {
    flex: 1;
    padding: 0.75rem;
    border: 1px solid #ddd;
    border-radius: 4px;
    font-size: 1rem;
  }

  .status-bar {
    padding: 1rem;
    background: #e7f3ff;
    border-left: 4px solid #007bff;
    border-radius: 4px;
  }

  .status-bar.error {
    background: #ffe7e7;
    border-left-color: #dc3545;
  }
</style>
```

**Key Simplifications:**
- Single page (no routing)
- Blocking operations (UI freezes during upload/download)
- No progress bars
- Simple status messages
- Basic styling

## 4-Week Implementation Plan

### Week 1: Core Backend

**Days 1-2: Project Setup**
- Create new Tauri project
- Add dependencies (libp2p, sled, cid, multihash, tokio)
- Set up module structure

**Days 3-4: Content & Storage**
- Implement `content.rs` (CID generation, chunking)
- Implement `storage.rs` (sled blockstore)
- Write unit tests

**Days 5-7: DHT Wrapper**
- Implement `dht.rs` (Kademlia wrapper)
- Test with local bootstrap node
- Verify provide/find_providers works

**Deliverable:** Backend modules compile and pass unit tests

### Week 2: Network & Integration

**Days 1-3: Network Layer**
- Implement `network.rs` (libp2p transport + request/response)
- Test block exchange between two local nodes
- Handle connection errors gracefully

**Days 4-5: Tauri Commands**
- Implement `commands.rs` (upload_file, download_file)
- Wire up AppState
- Test commands via Tauri dev tools

**Days 6-7: Integration Testing**
- Test complete upload flow
- Test complete download flow
- Fix integration bugs

**Deliverable:** Backend functional, commands work via CLI

### Week 3: Frontend & E2E

**Days 1-3: UI Implementation**
- Create `App.svelte` with upload/download UI
- Wire up Tauri command invocations
- Add basic error handling

**Days 4-5: Testing & Debugging**
- Test upload → download workflow
- Test error cases (no providers, invalid CID)
- Fix UI bugs

**Days 6-7: Multi-Node Testing**
- Run two instances simultaneously
- Upload on instance A
- Download on instance B
- Verify file integrity

**Deliverable:** Working desktop app, E2E tests pass

### Week 4: Polish & Documentation

**Days 1-2: Bug Fixes**
- Fix any remaining issues from Week 3
- Improve error messages
- Add logging

**Days 3-4: Bootstrap Setup**
- Set up at least one public bootstrap node
- Add bootstrap addresses to config
- Test peer discovery

**Days 5-6: Documentation**
- README with setup instructions
- Demo video recording
- Architecture documentation

**Day 7: Final Demo**
- Prepare demo presentation
- Test on clean machine
- Create release build

**Deliverable:** MVP ready for demo

## MVP Success Criteria

### Functional Requirements

✅ **Upload File:**
1. User selects file via file picker
2. File is chunked into 256KB pieces
3. Chunks stored in local blockstore
4. CID is generated and displayed
5. Content announced to DHT

✅ **Download File:**
1. User enters CID
2. DHT queried for providers
3. At least one provider found
4. Chunks downloaded from provider
5. File reassembled and saved
6. Hash verification passes

### Demo Scenario

**Setup:**
- Two computers on same network (or two VM instances)
- Both running the application
- Connected to same bootstrap node

**Steps:**
1. Instance A: Upload `test-video.mp4` (10 MB)
2. Instance A: Copy displayed CID
3. Instance B: Paste CID, click Download
4. Instance B: Save to desktop
5. Verify: `sha256sum` matches on both files

**Success Metrics:**
- Upload completes in < 5 seconds
- Download completes in < 30 seconds
- File hashes match exactly
- No crashes or errors

## Testing Strategy

### Unit Tests (Required)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cid_generation() {
        let data = b"hello world";
        let cid1 = generate_cid(data);
        let cid2 = generate_cid(data);
        assert_eq!(cid1, cid2);
    }

    #[test]
    fn test_chunk_deterministic() {
        // Same file should produce same chunks
        let chunks1 = chunk_file("test.dat").await.unwrap();
        let chunks2 = chunk_file("test.dat").await.unwrap();
        assert_eq!(chunks1.len(), chunks2.len());
        for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
            assert_eq!(c1.cid, c2.cid);
        }
    }

    #[test]
    fn test_blockstore_put_get() {
        let store = Blockstore::open("./test-store").unwrap();
        let data = b"test data";
        let cid = generate_cid(data);

        store.put(&cid, data).unwrap();
        let retrieved = store.get(&cid).unwrap().unwrap();
        assert_eq!(data, &retrieved[..]);
    }
}
```

### Integration Test (Required)

**Test:** Two-node file transfer

```rust
#[tokio::test]
async fn test_two_node_transfer() {
    // Start node 1
    let node1 = start_node(4001).await;

    // Start node 2 with node1 as bootstrap
    let node2 = start_node(4002).await;
    node2.connect_to_bootstrap(node1.peer_id(), node1.address()).await.unwrap();

    // Upload file on node 1
    let test_file = "test-data.bin";
    create_test_file(test_file, 1024 * 1024); // 1 MB
    let cid = node1.upload(test_file).await.unwrap();

    // Wait for DHT propagation
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Download on node 2
    let output = "downloaded.bin";
    node2.download(&cid, output).await.unwrap();

    // Verify files match
    let hash1 = sha256_file(test_file);
    let hash2 = sha256_file(output);
    assert_eq!(hash1, hash2);
}
```

### Manual E2E Test (Required)

**Checklist:**
- [ ] Build release binary
- [ ] Copy to two separate machines
- [ ] Both connect to bootstrap node
- [ ] Upload 5 MB file
- [ ] CID displayed correctly
- [ ] Download completes successfully
- [ ] File opens and works
- [ ] SHA256 hashes match

## Dependencies

### Cargo.toml

```toml
[dependencies]
# Tauri
tauri = { version = "2.0", features = [] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Async runtime
tokio = { version = "1.35", features = ["full"] }

# P2P networking
libp2p = { version = "0.54", features = ["kad", "tcp", "noise", "yamux", "request-response"] }

# Storage
sled = "0.34"

# Content addressing
cid = "0.11"
multihash = { version = "0.19", features = ["sha2"] }
sha2 = "0.10"

# Serialization
bincode = "1.3"
```

### package.json

```json
{
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "svelte": "^4.2.0"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^3.0.0",
    "vite": "^5.0.0"
  }
}
```

## Post-MVP Roadmap

### After Core MVP Works

**Phase 2: Reliability (2 weeks)**
- Add replication (3 replicas minimum)
- Retry failed downloads
- Connection pooling
- Better error messages

**Phase 3: UX Polish (2 weeks)**
- Progress bars
- Multiple simultaneous uploads/downloads
- Drag & drop
- Network statistics

**Phase 4: Performance (2 weeks)**
- Parallel chunk downloads
- Merkle DAG structure
- Caching layer
- Optimize chunk size

**Phase 5: Advanced Features (2 weeks)**
- Pin management
- Metadata search
- File versioning
- Proxy routing

**Phase 6: Production Ready (2 weeks)**
- Comprehensive error handling
- Monitoring dashboard
- Configuration UI
- Deployment automation

## Comparison: Full Proposal vs. MVP

| Aspect | Full Proposal (12 weeks) | MVP (4 weeks) |
|--------|--------------------------|---------------|
| **Lines of Code** | ~3,000 | ~1,000 |
| **Backend Modules** | 15 files | 6 files |
| **Frontend Components** | 3 components | 1 page |
| **DHT** | Custom routing table | libp2p default |
| **Content** | Merkle DAG | Flat chunk list |
| **Storage** | Blockstore + Datastore | Blockstore only |
| **Replication** | Automatic 20-node | None |
| **UI** | Real-time progress | Blocking operations |
| **Network** | mDNS + Bootstrap + Relay | Bootstrap only |
| **Testing** | Unit + Integration + E2E | Basic tests |
| **Dependencies** | 15 crates | 10 crates |
| **Features** | Production-ready | Proof of concept |

## Key Metrics

### Development Velocity

- **Week 1:** 25% complete (Backend foundation)
- **Week 2:** 50% complete (Network integrated)
- **Week 3:** 75% complete (UI functional)
- **Week 4:** 100% complete (MVP demo-ready)

### Code Complexity

- **Average function length:** < 50 lines
- **Maximum file size:** 250 lines
- **Cyclomatic complexity:** < 10 per function
- **Test coverage:** > 60%

### Performance Targets

- **Upload 1 MB file:** < 2 seconds
- **Download 1 MB file:** < 10 seconds
- **DHT lookup:** < 5 seconds
- **Memory usage:** < 100 MB idle

## Conclusion

This MVP demonstrates the fundamental concept of DHT-based distributed storage in **4 weeks with ~1,000 lines of code**. By aggressively simplifying non-essential features, we can prove the core technology works before investing in polish, performance, and production-readiness.

**The MVP answers one question:** *Can we build a working P2P file sharing system using DHT for peer discovery?*

Once that's proven, we can systematically add:
- Replication for reliability
- UI polish for usability
- Performance optimizations for scale
- Advanced features for completeness

This incremental approach reduces risk and allows for course corrections based on real-world testing.
