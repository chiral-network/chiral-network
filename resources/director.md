# Chiral Network - Director's Architectural Specification

**Document Version:** 1.0
**Date:** January 2025
**Project Status:** Phase 2 Complete, Phase 3 In Progress
**Critical Gap:** Files are local-only, need true DHT-based distribution

---

## Document Purpose

This document answers the critical architectural questions for Chiral Network based on the **current implementation state**. The project has a working UI and P2P infrastructure, but files are not yet distributed across the network. This document defines what must be done to bridge that gap.

---

## Current Implementation Status

### ✅ What's Working

**Phase 1 - UI & Desktop Integration (COMPLETE):**
- Modern Svelte 5 + TypeScript frontend
- Tauri 2 desktop application
- Drag & drop file interface
- Download queue management (priority, pause/resume)
- Multi-language support (EN, ES, ZH, KO)
- Settings management
- Analytics dashboard (with mock data)

**Phase 2 - P2P Infrastructure (COMPLETE):**
- libp2p v0.54 fully integrated
- Kademlia DHT implementation
- WebRTC data channels
- Peer discovery (mDNS + libp2p)
- NAT traversal (STUN, relay)
- Noise protocol encryption

### ❌ What's NOT Working

**Critical Missing Piece:**
- **Network Integration:** Files stored locally only, NOT distributed to DHT
- No actual DHT announce when files uploaded
- No DHT lookup when downloading
- Files disappear when uploader closes app

**Other Gaps:**
- Real-time network stats use mock data
- Bandwidth tracking uses mock data
- Geolocation not implemented
- Proxy routing not implemented
- Encryption infrastructure exists but not applied to transfers
- Anonymous routing not implemented

---

## 1. 🎯 Scope & Business Intent

### What is the core business problem this system solves?

**Problem Statement:**
Existing file sharing requires either:
1. **Centralized services** (Dropbox, Google Drive) - privacy concerns, vendor lock-in, costs
2. **Manual P2P** (BitTorrent + trackers) - still needs centralized discovery

**Our Solution:**
Fully decentralized P2P file sharing where:
- Files distributed across volunteer nodes
- Discovery via DHT (no tracker)
- Content-addressed (cryptographic verification)
- Non-commercial (no marketplace)

**Current Reality:**
We have the **infrastructure** (libp2p + DHT) and **UI** (Svelte + Tauri) but files are **local-only**. The critical next step is making files truly distributed.

### Who is the end-user and what is their single most important use case?

**Primary User:**

**Name:** Alex - University Student
**Context:** Group project with 4 teammates
**Pain Point:** Can't email 200MB dataset, doesn't trust cloud storage

**Primary Use Case:**
> "I want to share my research dataset with teammates without uploading to Google Drive."

**Current Workflow (Broken):**
1. Alex uploads file → ✅ Works (file stored locally)
2. Gets file hash → ✅ Works (hash generated)
3. Shares hash with teammate → ✅ Works (UI shows hash)
4. Teammate downloads → ❌ **FAILS** (no DHT announce, file not found)

**Target Workflow:**
1. Upload file → Announce to DHT → Store locally
2. Teammate searches DHT → Finds Alex's peer ID → Downloads directly
3. Both become seeders

### What is the absolute minimum viable product (MVP)?

Based on README.md, we're currently at **"MVP - 1 feature away"**:

**Already Complete:**
- ✅ Desktop UI (all pages working)
- ✅ File upload interface
- ✅ Download queue
- ✅ libp2p + Kademlia DHT integrated
- ✅ WebRTC data channels
- ✅ Local file storage

**Missing for MVP:**
- ❌ **DHT Integration** (THE critical gap):
  - On upload: Announce file availability to DHT
  - On download: Query DHT for providers
  - Transfer chunks via WebRTC from peer

**MVP Success Criteria:**
1. Upload file on Computer A
2. Get file hash
3. Enter hash on Computer B (same network)
4. Computer B finds Computer A via DHT
5. Download completes successfully
6. File hashes match

### How will we measure the success of this system?

**Immediate Success (MVP):**
- [ ] Two computers can exchange file via DHT (not direct connection)
- [ ] Download works when uploader is only provider
- [ ] Hash verification passes
- [ ] No data corruption

**Technical Metrics:**
| Metric | Current | Target |
|--------|---------|--------|
| DHT announce success rate | 0% (not implemented) | 95% |
| Peer discovery time | N/A | < 5 seconds |
| File transfer success rate | 0% (local only) | 90% |
| Download speed | N/A | > 1 MB/s |

**Functional Metrics:**
- **Phase 3 Completion:** 50% done (encryption exists, need distribution)
- **Feature Parity:** UI says "P2P" but actually local-only - need to fix
- **User Expectation:** App promises distributed storage, must deliver

---

## 2. ⚙️ Functional Requirements

### What are the specific inputs and outputs of the system?

#### Upload Flow (Current vs. Target)

**Current Implementation:**
```typescript
Input: { file_path: "/path/to/file.zip" }
Output: {
  hash: "sha256_abcd1234...",
  status: "stored_locally"  // ❌ Not announced to DHT
}
```

**Target Implementation:**
```typescript
Input: { file_path: "/path/to/file.zip" }
Output: {
  hash: "sha256_abcd1234...",
  chunks_stored: 400,
  dht_announced: true,        // ✅ NEW
  providers: [local_peer_id], // ✅ NEW
  status: "available_on_network"
}
```

#### Download Flow (Current vs. Target)

**Current Implementation:**
```typescript
Input: { file_hash: "sha256_abcd..." }
Output: Error("File not found") // ❌ Only checks local storage
```

**Target Implementation:**
```typescript
Input: { file_hash: "sha256_abcd..." }
Process:
  1. Check local storage (cache hit?)
  2. If not found, query DHT for providers ✅ NEW
  3. Connect to provider via WebRTC ✅ NEW
  4. Download chunks
  5. Verify integrity
Output: {
  file_path: "/downloads/file.zip",
  downloaded_from: [peer_id_1, peer_id_2],
  verified: true
}
```

### What are all the external systems this must integrate with?

**Already Integrated:**
- ✅ Operating System (file system, TCP sockets via Tauri)
- ✅ libp2p network (transport, encryption, NAT traversal)
- ✅ Kademlia DHT (peer routing - but not used yet)

**Needs Integration:**
- ❌ **DHT for content routing** (infrastructure exists, need to call it)
  - Announce file availability (provide operation)
  - Query for providers (find_providers operation)

**No External Services Required:**
- ❌ No cloud servers
- ❌ No authentication services
- ❌ No blockchain nodes (mining is separate feature)
- ❌ No trackers or discovery servers

**Bootstrap Nodes (Required):**
Currently the app needs bootstrap peer addresses to join DHT:
- Format: `/ip4/x.x.x.x/tcp/4001/p2p/12D3KooW...`
- Need: 2-3 hardcoded bootstrap nodes
- Status: ❓ Unknown if configured

### How should the system handle specific errors?

#### Error Scenarios Based on Current State

| Error | Current Behavior | Target Behavior |
|-------|------------------|-----------------|
| **File not in DHT** | "File not found" (checks local only) | "No peers have this file. Uploader may be offline." |
| **DHT query timeout** | N/A (not implemented) | Retry 3x, then show "Network unavailable" |
| **Peer disconnected during transfer** | N/A | Automatically find alternate provider from DHT |
| **Chunk verification failed** | N/A | Reject chunk, request from different peer |
| **Bootstrap node unreachable** | Unknown | Cannot join network, show offline mode |
| **No peers found** | N/A | "Network empty. Be the first seeder!" |

**Critical Error Handling Needed:**

1. **Upload Phase:**
   - DHT announce fails → Retry 3x, warn user but keep file locally
   - Local storage full → Show clear error before chunking

2. **Download Phase:**
   - No providers found → Clear message (not "file not found")
   - Provider goes offline mid-transfer → Seamlessly switch to another
   - Corrupt chunk → Re-request from same or different peer

---

## 3. 📈 Non-Functional Requirements (NFRs)

### Performance

**Current Performance:**
- Upload to local storage: Fast (limited by disk speed)
- UI responsiveness: Good (Svelte 5 is fast)
- Download: N/A (not implemented across network)

**Target Performance:**

| Operation | Target (P50) | Target (P95) | Notes |
|-----------|--------------|--------------|-------|
| Hash 100MB file | 2s | 5s | Already implemented |
| DHT announce (per file) | 2s | 5s | Need to implement |
| DHT lookup (find providers) | 1s | 3s | DHT exists, need to call |
| Download 100MB (1 peer) | 30s | 60s | WebRTC exists, need chunking |
| Download 100MB (3 peers) | 15s | 30s | Post-MVP: parallel downloads |

**Resource Limits:**
- Memory: < 200 MB (current: unknown, likely <100MB)
- CPU: < 20% during transfer (current: low, just UI)
- Disk I/O: < 50 MB/s (SSD speeds)
- Network: Utilize available bandwidth (currently 0)

### Scalability

**Current Scale:**
- Users: 1 (yourself, testing locally)
- Files: Unlimited (but all local)
- Network: 0 peers (no P2P transfers)

**Immediate Target (MVP):**
- Users: 2-5 (classmates testing)
- Files: 10-20 files shared
- Network: 5-10 connected peers
- Storage: 1 GB per node

**6-Month Vision:**
- Users: 50-100 active users
- Files: 500 files total
- Network: 50+ nodes
- Storage: 50 GB distributed

**Known Limitations:**
1. **No Replication:** Files only available when uploader online
   - Current: True (local only)
   - MVP: Still true (single seeder)
   - Post-MVP: Add 3-replica minimum

2. **NAT Traversal:** ~30% of users may have issues
   - Current: Unknown success rate
   - MVP: Use STUN (already integrated)
   - Post-MVP: Add relay nodes

### Availability

**Current Availability:**
- Desktop app: Works when launched
- Files: Available only on local machine
- Network: N/A (no network transfers)

**Target Availability:**

| Component | Target | Current |
|-----------|--------|---------|
| Individual node | N/A (user controls) | ✅ |
| File availability | 80% (when seeders online) | 100% (local only) |
| DHT network | 95% uptime | Unknown |
| Bootstrap nodes | Need 99% uptime | ❓ Not set up |

**Failure Scenarios:**

1. **Uploader Goes Offline:**
   - Current: File disappears (local only)
   - MVP: File still disappears (single seeder)
   - Post-MVP: Other seeders have copies (replication)

2. **DHT Partition:**
   - Current: N/A
   - Target: Peers in same partition can still connect
   - Recovery: Auto-reconnect when partition heals

3. **Bootstrap Node Down:**
   - Current: Unknown
   - Target: Fall back to secondary bootstrap
   - Impact: New peers can't join, existing peers unaffected

### Security

**Current Security Posture:**

✅ **Implemented:**
- Noise protocol encryption (transport layer)
- Input validation on UI
- No centralized attack surface

❌ **Not Implemented:**
- Content integrity verification during download
- Authentication between peers
- Encryption at rest
- DoS protection

**Target Security Requirements:**

1. **Content Integrity (CRITICAL):**
   - Status: ❌ Not implemented for network transfers
   - Requirement: Verify SHA-256 hash of every chunk
   - Implementation: Add verification step after WebRTC receive
   - Risk if not fixed: Corrupt data, malicious peers

2. **Transport Security (DONE):**
   - Status: ✅ Noise protocol via libp2p
   - Prevents: Eavesdropping, MITM attacks

3. **DoS Protection (NEEDED):**
   - Status: ❌ Not implemented
   - Requirement: Rate limit DHT queries
   - Implementation: libp2p has defaults, verify enabled
   - Risk: Peer flooding, DHT spam

4. **Sybil Attack (ACCEPT RISK):**
   - Status: ❌ Not protected
   - Risk: Attacker creates fake peer IDs
   - MVP: Accept risk (trust-based network)
   - Post-MVP: Reputation system

**Data Classification:**
- User files: User-controlled, public (anyone with hash)
- File hashes: Public (shared via DHT)
- Peer IDs: Pseudonymous (no real identity)
- Local storage: Private (OS file permissions)

### Maintainability

**Current Logging:**
- UI: Browser console logs (development)
- Backend: Rust logs (unknown what's logged)

**Target Logging:**

```rust
// Critical events to log
info!("File uploaded: hash={}, size={} bytes", hash, size);
info!("DHT announce started: hash={}", hash);
info!("DHT announce complete: providers={}", count);
info!("Peer discovered: peer_id={}, addr={}", id, addr);
info!("Download started: hash={}, from={}", hash, peer_id);
info!("Chunk received: {}/{}, verified={}", current, total, ok);
warn!("DHT timeout, retrying...");
error!("Download failed: {}", err);
```

**Monitoring Requirements:**

MVP monitoring (local only):
- [ ] Bootstrap connection status
- [ ] DHT routing table size (peer count)
- [ ] Active DHT queries
- [ ] Active file transfers (up/down)
- [ ] Transfer success/failure rate

Display in UI:
- Network status indicator (connected/disconnected)
- Peer count in status bar
- Transfer progress with peer info

---

## 4. 🛠️ Technical & Team Constraints

### What is the team's existing expertise?

**Demonstrated Skills (Based on Current Code):**

✅ **Strong:**
- Svelte 5 (complex UI built)
- TypeScript (type-safe frontend)
- Tauri 2 (desktop integration working)
- UI/UX design (polished interface)
- State management (stores, queues)

✅ **Moderate:**
- Rust (backend structure exists)
- Async programming (Tokio likely used for libp2p)
- P2P networking (libp2p integrated, but not fully utilized)

❓ **Unknown:**
- DHT usage (Kademlia imported but announce/query not called?)
- WebRTC data transfer (channels exist, file chunking?)
- Error handling (how robust is the current code?)

**Learning Gap:**
The team knows HOW to use libp2p + Kademlia (they integrated it), but may not know WHEN to call DHT operations:
- Where to call `dht.provide(hash)` after upload
- Where to call `dht.find_providers(hash)` before download
- How to transfer chunks via WebRTC

**Estimated Effort to Bridge Gap:**
- 1-2 days: Connect upload to DHT announce
- 1-2 days: Connect download to DHT lookup
- 2-3 days: Implement chunked transfer via WebRTC
- 1 day: Add verification and error handling
- **Total: 1 week** to make files truly distributed

### What is the projected cloud budget?

**Current Budget:** $0 (no servers running)

**Needs:**
Bootstrap nodes to help peers discover each other

**Options:**

1. **Free Tier (Recommended for MVP):**
   - DigitalOcean $200 student credit
   - AWS free tier (12 months)
   - Google Cloud free tier
   - **Cost:** $0 for first year

2. **Minimal Production:**
   - 2x VPS for bootstrap nodes
   - $5-10/month each
   - **Total:** $10-20/month

3. **No Cloud Alternative:**
   - Use public IPFS bootstrap nodes
   - Add `/dnsaddr/bootstrap.libp2p.io/`
   - **Cost:** $0 (but less control)

**Recommendation:** Start with public libp2p bootstrap nodes (free), upgrade to own VPS later if needed.

### Are there any mandated technologies?

**Already Chosen (Non-Negotiable):**

✅ **Frontend:**
- Svelte 5 (working)
- TypeScript (working)
- Tailwind CSS (working)
- Tauri 2 (working)

✅ **Backend:**
- Rust (working)
- libp2p v0.54 (integrated)
- Kademlia DHT (integrated)
- WebRTC (integrated)

✅ **Infrastructure:**
- Desktop application (not web app)
- P2P architecture (no servers)

**Flexible (Can Change):**
- File chunking strategy (256KB chunks? Configurable?)
- Storage backend (currently seems to be file system, could use SQLite)
- Serialization format (bincode? protobuf?)

**Not Decided:**
- Bootstrap node addresses (need to configure)
- DHT parameters (K-value, replication factor)
- Transfer chunk size

---

## 5. 🧱 Core Architectural Decisions

### Will this be a monolith, microservice, or serverless function?

**Architecture:** Desktop Application Monolith ✅ (Already Implemented)

```
┌─────────────────────────────────────┐
│   Chiral Network Desktop App        │
│   (Single Tauri Process)            │
├─────────────────────────────────────┤
│  Frontend (Svelte 5 + TS)           │ ← Working
│   ├─ Upload UI (drag & drop)        │ ← Working
│   ├─ Download Queue UI              │ ← Working
│   ├─ Network Monitor UI             │ ← Working (mock data)
│   └─ Settings UI                    │ ← Working
├─────────────────────────────────────┤
│  Tauri IPC Bridge                   │ ← Working
├─────────────────────────────────────┤
│  Backend (Rust)                     │
│   ├─ libp2p + Kademlia DHT          │ ← Integrated ✅
│   ├─ WebRTC Data Channels           │ ← Integrated ✅
│   ├─ File Storage (local)           │ ← Working ✅
│   ├─ DHT Announce                   │ ← NOT CALLED ❌
│   └─ DHT Lookup + Transfer          │ ← NOT CALLED ❌
└─────────────────────────────────────┘
```

**Current State:** Infrastructure exists, integration missing.

### What is the data storage strategy?

**Current Implementation (Based on README):**

1. **Local File Storage:**
   - Files uploaded → Stored in user-selected directory
   - Appears to use file system directly
   - No indication of chunking or database

**Target Implementation:**

1. **Chunked Local Storage:**
```rust
// File structure needed:
~/.chiral/
  ├─ blockstore/        // Chunk database
  │   ├─ chunk_abcd.dat
  │   ├─ chunk_efgh.dat
  │   └─ ...
  ├─ metadata.db        // File metadata (SQLite or sled)
  └─ config.json        // User settings
```

2. **Metadata Schema:**
```rust
struct FileMetadata {
    hash: String,             // SHA-256 of entire file
    name: String,             // Original filename
    size: u64,                // Total bytes
    chunks: Vec<ChunkHash>,   // Ordered list of chunk hashes
    created: Timestamp,
    is_seeding: bool,         // Am I providing this?
}

struct ChunkHash {
    hash: String,             // SHA-256 of chunk
    index: u32,               // Position in file
    size: u32,                // Bytes (256KB except last)
}
```

3. **DHT Storage (Network):**
```rust
// What goes in DHT:
Key: file_hash (SHA-256)
Value: Vec<PeerId>  // List of providers

// Kademlia operation:
dht.start_providing(Key::new(&file_hash))?;  // ← Need to call this
```

**Current Gap:**
- Files stored as complete units (not chunked?)
- No DHT records created
- No provider announcements

**Fix Needed:**
1. Chunk files on upload (256KB each)
2. Store chunks in local blockstore
3. Announce file hash to DHT
4. On download, query DHT then fetch chunks

### How will components communicate?

**Current Communication (Working):**

1. **Frontend → Backend (Tauri IPC):**
```typescript
// Frontend can already call:
await invoke('upload_file', { path: '/path/to/file' });
await invoke('download_file', { hash: 'sha256...' });
```

2. **Backend → libp2p (Direct Rust Calls):**
```rust
// Already integrated (based on README Phase 2 complete):
let swarm = Swarm::new(...);  // ✅ Exists
let dht = Kademlia::new(...);  // ✅ Exists
swarm.behaviour_mut().add_address(...);  // ✅ Can discover peers
```

**Missing Communication:**

3. **Backend → DHT Announce (Need to Add):**
```rust
// After upload, need to call:
async fn announce_file(dht: &mut Kademlia, hash: &str) {
    let key = Key::new(hash.as_bytes());
    dht.start_providing(key)?;  // ← THIS CALL IS MISSING
    // Wait for query to complete...
}
```

4. **Backend → DHT Lookup (Need to Add):**
```rust
// Before download, need to call:
async fn find_providers(dht: &mut Kademlia, hash: &str) -> Vec<PeerId> {
    let key = Key::new(hash.as_bytes());
    dht.get_providers(key);  // ← THIS CALL IS MISSING
    // Collect provider responses...
}
```

5. **Backend → Peer File Transfer (Need to Add):**
```rust
// Connect to provider and request chunks:
async fn download_chunk(
    peer: PeerId,
    chunk_hash: &str,
) -> Result<Vec<u8>, Error> {
    // Use WebRTC data channel (exists) to request chunk
    // Protocol needs to be defined:
    // 1. Send: RequestChunk { hash }
    // 2. Receive: ChunkData { hash, bytes }
}
```

**Communication Matrix:**

| From → To | Protocol | Status | Notes |
|-----------|----------|--------|-------|
| UI → Backend | Tauri IPC | ✅ Working | Commands exist |
| Backend → DHT | Kademlia RPC | ⚠️ Integrated but not called | Need provide/find_providers |
| Backend → Peer | WebRTC | ⚠️ Integrated but not used | Need chunk request protocol |
| Backend → Disk | File I/O | ✅ Working | Need chunking strategy |

---

## 6. 🚨 Critical Implementation Gaps

### Gap #1: DHT Announce Not Called

**Current State:**
- User uploads file
- File stored locally
- UI shows hash
- **NO DHT announce happens**

**What's Needed:**
```rust
// In upload handler (src-tauri/src/commands/upload.rs ?):
#[tauri::command]
async fn upload_file(path: String, state: State<AppState>) -> Result<String, String> {
    // 1. Hash file (probably done)
    let hash = hash_file(&path)?;

    // 2. Chunk file (need to add)
    let chunks = chunk_file(&path, 256*1024)?;

    // 3. Store chunks locally (need to add)
    for chunk in chunks {
        state.blockstore.put(&chunk.hash, &chunk.data)?;
    }

    // 4. CRITICAL: Announce to DHT (MISSING)
    let mut dht = state.dht.lock().await;
    dht.start_providing(Key::new(hash.as_bytes()))?;  // ← ADD THIS

    Ok(hash)
}
```

**Files to Modify:**
- `src-tauri/src/commands/*.rs` (upload handler)
- Need access to Kademlia instance

### Gap #2: DHT Lookup Not Called

**Current State:**
- User enters hash to download
- Backend checks local storage only
- Returns "not found" error

**What's Needed:**
```rust
#[tauri::command]
async fn download_file(hash: String, state: State<AppState>) -> Result<(), String> {
    // 1. Check local first
    if state.blockstore.has(&hash)? {
        return Ok(()); // Already have it
    }

    // 2. CRITICAL: Query DHT for providers (MISSING)
    let mut dht = state.dht.lock().await;
    dht.get_providers(Key::new(hash.as_bytes()));

    // 3. Wait for provider responses
    let providers = wait_for_providers(&mut dht, &hash).await?;

    if providers.is_empty() {
        return Err("No peers have this file".to_string());
    }

    // 4. Download from first provider (MISSING)
    let peer = providers[0];
    let chunks = download_from_peer(peer, &hash).await?;

    // 5. Verify and assemble
    verify_and_save(chunks, &hash)?;

    Ok(())
}
```

### Gap #3: Chunk Transfer Protocol Not Implemented

**Current State:**
- WebRTC data channels exist
- No protocol defined for requesting chunks

**What's Needed:**
```rust
// Define protocol messages:
enum ChunkProtocol {
    RequestChunk { hash: String },
    ChunkData { hash: String, data: Vec<u8> },
    ChunkNotFound { hash: String },
}

// Implement handler:
async fn handle_chunk_request(
    request: ChunkProtocol,
    blockstore: &Blockstore,
) -> ChunkProtocol {
    match request {
        ChunkProtocol::RequestChunk { hash } => {
            if let Some(data) = blockstore.get(&hash)? {
                ChunkProtocol::ChunkData { hash, data }
            } else {
                ChunkProtocol::ChunkNotFound { hash }
            }
        }
        _ => unreachable!(),
    }
}
```

---

## 7. 📋 Implementation Roadmap

### Current Position: 80% Complete

**Phase 1:** ✅ 100% Complete (UI + Desktop)
**Phase 2:** ✅ 100% Complete (libp2p + DHT infrastructure)
**Phase 3:** ⚠️ 50% Complete (infrastructure ready, missing integration)

### Next Steps to Reach MVP (1-2 Weeks)

**Week 1: Core DHT Integration**

**Day 1-2: Upload → DHT Announce**
- [ ] Add chunking on upload (256KB chunks)
- [ ] Store chunks in blockstore
- [ ] Call `dht.start_providing()` after upload
- [ ] Test: Verify hash appears in DHT

**Day 3-4: Download → DHT Lookup**
- [ ] Add `dht.get_providers()` before download
- [ ] Wait for provider responses
- [ ] Handle "no providers" gracefully
- [ ] Test: Find own uploads as provider

**Day 5-7: Chunk Transfer**
- [ ] Define chunk request protocol
- [ ] Implement request/response handlers
- [ ] Transfer chunks via WebRTC
- [ ] Verify chunk integrity
- [ ] Test: Download file from peer

**Week 2: Testing & Polish**

**Day 1-2: End-to-End Testing**
- [ ] Upload on computer A
- [ ] Download on computer B (same network)
- [ ] Verify file integrity
- [ ] Test with 10MB, 100MB, 1GB files

**Day 3-4: Error Handling**
- [ ] Handle DHT timeout
- [ ] Handle peer disconnection
- [ ] Show meaningful error messages
- [ ] Add retry logic

**Day 5-7: Documentation & Demo**
- [ ] Update README (remove "local-only" warnings)
- [ ] Record demo video
- [ ] Write deployment guide
- [ ] Prepare for user testing

### Post-MVP Enhancements (Phase 3+)

**Phase 3 Completion (Weeks 3-4):**
- [ ] Add 3-replica replication
- [ ] Apply encryption to transfers
- [ ] Implement bandwidth throttling
- [ ] Add parallel downloads

**Phase 4 (Weeks 5-8):**
- [ ] Real network statistics (remove mock data)
- [ ] Proxy routing
- [ ] Geographic peer selection
- [ ] Mobile app exploration

---

## 8. ✅ Acceptance Criteria

### MVP is Complete When:

1. **Two-Computer Test Passes:**
   - [ ] Start app on Computer A and B (same network)
   - [ ] Upload 10MB file on A
   - [ ] Copy hash from A
   - [ ] Paste hash in B, click download
   - [ ] Download completes in < 60 seconds
   - [ ] SHA-256 hash matches

2. **DHT Integration Confirmed:**
   - [ ] Upload triggers DHT announce (verify in logs)
   - [ ] Download queries DHT first (verify in logs)
   - [ ] Peer discovery works via DHT
   - [ ] No hardcoded IP addresses used

3. **File Integrity Verified:**
   - [ ] Every chunk verified during download
   - [ ] Final file hash matches upload hash
   - [ ] No silent corruption

4. **README Accuracy:**
   - [ ] Remove ❌ from "Network Integration"
   - [ ] Update status to reflect DHT working
   - [ ] Remove "mock data" warnings where fixed

### Definition of "Production Ready" (Post-MVP)

- [ ] Works across different networks (NAT traversal proven)
- [ ] 3+ replicas per file (redundancy)
- [ ] Handles peer churn (providers come/go)
- [ ] 90%+ download success rate
- [ ] Real-time statistics (no mock data)

---

## 9. 🔍 Technical Debt & Known Issues

### Current Technical Debt (Based on README)

**High Priority (Blocking MVP):**
1. ❌ Files are local-only (THE critical issue)
2. ❌ No DHT content routing
3. ❌ No chunk transfer protocol

**Medium Priority (Affects UX):**
4. ❌ Analytics use mock data (misleading)
5. ❌ No real bandwidth tracking
6. ❌ Encryption exists but not applied

**Low Priority (Post-MVP):**
7. ❌ No geolocation
8. ❌ No proxy routing
9. ❌ Mining rewards not calculated from blockchain

### Code Quality Concerns

**Questions to Answer:**
- Where is the DHT instance stored in backend?
- How to access it from upload/download handlers?
- Is there existing chunking code?
- What's the current WebRTC usage?

**Suggested Investigation:**
```bash
# Find DHT usage:
rg "Kademlia" src-tauri/

# Find upload handler:
rg "upload_file" src-tauri/

# Find download handler:
rg "download_file" src-tauri/
```

---

## 10. 📚 Success Metrics Dashboard

### Current State (Reality)

| Metric | Status | Notes |
|--------|--------|-------|
| UI Complete | ✅ 100% | All pages working |
| libp2p Integrated | ✅ 100% | Infrastructure ready |
| DHT Integrated | ⚠️ 50% | Imported but not called |
| File Distribution | ❌ 0% | Local-only |
| Network Transfers | ❌ 0% | No P2P transfers |
| MVP Progress | ⚠️ 80% | 1-2 weeks away |

### Target State (MVP)

| Metric | Target | Measurement |
|--------|--------|-------------|
| Upload Success | 95% | Files announced to DHT |
| Peer Discovery | 90% | Find providers within 5s |
| Download Success | 85% | Complete transfer |
| Transfer Speed | > 1 MB/s | Single peer |
| Hash Verification | 100% | All chunks verified |

---

## 11. 🎯 Summary: The One Thing That Matters

**Current Reality:**
- ✅ Beautiful UI built
- ✅ P2P infrastructure integrated (libp2p + Kademlia DHT)
- ❌ **But files are still local-only**

**The Critical Gap:**
After upload: Not calling `dht.start_providing(hash)`
Before download: Not calling `dht.get_providers(hash)`

**Estimated Effort:** 40-60 hours (1-2 weeks full-time, or 2-4 weeks part-time)

**Confidence Level:** High - infrastructure exists, just need to connect the pieces.

---

## Approval

This document reflects the **actual current state** of Chiral Network based on README.md. The path forward is clear: bridge the gap between the working infrastructure and the UI by implementing DHT-based content routing.

**Project Lead:** _________________________
**Date:** _____________

---

**End of Document**
