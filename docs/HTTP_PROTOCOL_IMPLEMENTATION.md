# HTTP File Transfer Protocol Implementation

## Technical Specification Document

**Project:** Chiral Network
**PR Reference:** #543
**Author:** whales-aynur-tariq
**Date:** October 28, 2025
**Status:** Core Implementation Complete, Awaiting Review

---

## Abstract

This document describes the design and implementation of an HTTP Range-based file transfer protocol for Chiral Network. The implementation adds HTTP as the first of four required core protocols (HTTP, WebTorrent, BitTorrent, ed2k) in a multi-protocol decentralized architecture. The system uses RFC 7233 HTTP Range requests for on-demand chunking, Kademlia DHT for peer discovery, and Axum/Reqwest for server/client implementations in Rust. This protocol serves as the default transfer mechanism for nodes with public IP addresses and establishes the foundation for a decoupled payment layer shared across all protocols.

---

## 1. Introduction

### 1.1 Design Goals

The HTTP protocol implementation was designed with the following objectives:

1. **Simplicity** - Minimize complexity by using standard HTTP semantics
2. **Efficiency** - Enable parallel chunk downloads while preventing network saturation
3. **Compatibility** - Work with any HTTP client (browsers, curl, wget, etc.)
4. **Integration** - Seamless integration with existing DHT-based peer discovery
5. **Foundation** - Establish patterns for subsequent protocol implementations

### 1.2 Scope

This implementation covers:

- HTTP file server with Range request support (RFC 7233)
- HTTP download client with parallel chunk retrieval
- DHT integration for automatic HTTP URL publishing
- Frontend UI for protocol selection
- File metadata management

Out of scope:

- File encryption
- Authentication/authorization
- HTTPS/TLS support

---

## 2. System Architecture

### 2.1 Multi-Protocol Architecture Context

Chiral Network implements a **decoupled layered architecture** with multiple transport protocols:

**Style 1: Public Protocols with Out-of-Band Payment (Primary)**

- HTTP/HTTPS - Default for public IP nodes (this implementation)
- WebTorrent - Default for NAT'd nodes (planned)
- BitTorrent - Efficient P2P swarming (planned)
- ed2k - Multi-source downloads (planned)

**Style 2: Private Protocol with In-Band Payment (Future)**

- Bitswap-like or WebRPC protocol (design phase)

The key architectural principle is **complete decoupling** of the payment layer from data transfer protocols. All protocols share a common blockchain-based payment settlement mechanism that operates independently of the chosen transfer protocol.

### 2.2 HTTP Protocol Position

HTTP serves as the **default protocol for nodes with public IP addresses** per project specifications. The protocol selection strategy:

```
IF node has public IP THEN
    Default seeding protocol = HTTP
ELSE IF node is behind NAT THEN
    Default seeding protocol = WebTorrent
END IF

FOR downloads:
    Client selects best protocol(s) from available seeders
    May use multiple protocols simultaneously for different chunks
END FOR
```

### 2.3 Component Overview

The HTTP implementation consists of four main components:

1. **HTTP Server** (`http_server.rs`) - Axum-based file serving with Range support
2. **HTTP Download Client** (`http_download.rs`) - Reqwest-based parallel chunk downloader
3. **DHT Integration** (`dht.rs`, `main.rs`) - Automatic HTTP URL publishing to Kademlia DHT
4. **Frontend UI** (`DownloadSearchSection.svelte`, `PeerSelectionModal.svelte`) - Protocol selection and download initiation

---

## 3. Design Evolution

### 3.1 Initial Design (Deprecated)

The original architecture used a **manifest-based chunking system**:

```
Server (Seeder):
  1. Chunk files into 256KB encrypted pieces
  2. Encrypt and store chunks on disk
  3. Generate manifest listing all chunks
  4. Serve manifest at GET /files/{hash}/manifest
  5. Serve individual chunks at GET /chunks/{encrypted_hash}

Client (Downloader):
  1. Fetch manifest
  2. Download chunks in parallel
  3. Reassemble file
  4. Defer decryption
```

**Problems with this approach:**

- Increased storage overhead (original file + encrypted chunks)
- Complex state management (manifest generation, chunk tracking)
- Extra encryption/decryption steps
- More endpoints to maintain
- Divergence from standard HTTP semantics

### 3.2 Current Design (Range-Based)

Based on feedback to simplify the implementation (PR #543 review), the architecture was completely refactored to use **HTTP Range requests** per RFC 7233:

```
Server (Seeder):
  1. Store whole plaintext files (not pre-chunked)
  2. Serve files at GET /files/{hash}
  3. Support Range headers for partial content ("chunks")
  4. Return 206 Partial Content for ranges

Client (Downloader):
  1. Fetch file metadata (size, name)
  2. Calculate byte ranges dynamically using file size (256KB chunks)
  3. Download chunks using Range headers
  4. Reassemble into final file
```

**Advantages of Range-based approach:**

- Standard HTTP semantics (RFC 7233)
- On-demand chunking (no pre-processing)
- Simpler server logic
- Easier to debug

### 3.3 Encryption Decision

The current implementation serves files as **plaintext** (no encryption at the HTTP layer). This decision was made for several reasons:

1. **Simplicity** - Focus on core transfer mechanics first
2. **Architecture** - Encryption is optional per decoupled design
3. **Testability** - Easier to verify file integrity during development
4. **Flexibility** - Users can choose whether to encrypt files

File encryption is planned for later phases as an **optional feature**.

---

## 4. HTTP Server Implementation

### 4.1 Technology Stack

**Framework:** Axum 0.7
**Rationale:** Axum provides:

- High-performance async HTTP server
- Type-safe routing with extractors
- Excellent integration with Tokio ecosystem
- Minimal boilerplate compared to alternatives (Actix, Warp)

**CORS:** tower-http CorsLayer
**Rationale:** Enable web client access from frontend

### 4.2 Server Configuration

```rust
Server Binding: 0.0.0.0:8080
  - Listens on all network interfaces
  - Port 8080 (standard alternative HTTP port)
  - Auto-starts on application launch

Storage Directory: ~/Downloads/Chiral-Network-Storage
  - Shared with FileTransferService
  - Files stored by merkle root hash
  - No duplication of storage
```

### 4.3 Endpoint Specification

#### GET /health

**Purpose:** Health check endpoint
**Response:** 200 OK
**Use Case:** Monitoring, service discovery

```http
GET /health HTTP/1.1
Host: localhost:8080

HTTP/1.1 200 OK
Content-Length: 0
```

#### GET /files/{file_hash}/metadata

**Purpose:** Retrieve file metadata before download
**Parameters:**

- `file_hash` (path) - Merkle root identifier

**Response Body:**

```json
{
  "hash": "QmXYZ123...",
  "name": "document.pdf",
  "size": 1048576,
  "encrypted": false
}
```

**Status Codes:**

- 200 OK - File found
- 404 Not Found - File not registered

#### GET /files/{file_hash}

**Purpose:** Serve file with Range request support
**Parameters:**

- `file_hash` (path) - Merkle root identifier
- `Range` (header, optional) - Byte range to retrieve

**Without Range Header (Full File):**

```http
GET /files/QmXYZ123 HTTP/1.1
Host: localhost:8080

HTTP/1.1 200 OK
Content-Length: 1048576
Content-Type: application/octet-stream

[entire file data]
```

**With Range Header (Partial Content):**

```http
GET /files/QmXYZ123 HTTP/1.1
Host: localhost:8080
Range: bytes=0-262143

HTTP/1.1 206 Partial Content
Content-Range: bytes 0-262143/1048576
Content-Length: 262144
Content-Type: application/octet-stream

[first 256KB of file]
```

**Status Codes:**

- 200 OK - Full file returned (no Range header)
- 206 Partial Content - Range request successful
- 404 Not Found - File not registered
- 416 Range Not Satisfiable - Invalid range requested
- 500 Internal Server Error - File registered but missing on disk

### 4.4 State Management

**HttpServerState Structure:**

```rust
pub struct HttpServerState {
    /// Path to file storage directory
    pub storage_dir: PathBuf,

    /// Maps file_hash → HttpFileMetadata
    pub files: Arc<RwLock<HashMap<String, HttpFileMetadata>>>,
}
```

**File Registration Flow:**

1. User uploads file via `upload_and_publish_file` command
2. File stored in `storage_dir/{merkle_root}`
3. `HttpServerState::register_file(metadata)` called
4. File becomes available at `http://{server_ip}:8080/files/{merkle_root}`

---

## 5. HTTP Download Client Implementation

### 5.1 Technology Stack

**HTTP Library:** Reqwest 0.11
**Rationale:**

- Pure Rust async HTTP client
- Excellent ergonomics and type safety
- Built-in timeout support
- Header manipulation
- Streaming response bodies

### 5.2 Download Algorithm

The download process follows a four-step pipeline:

**Step 1: Fetch File Metadata**
Make a GET request to `/files/{hash}/metadata` to retrieve:

- File name (for display and final save)
- File size (for chunk calculation)
- Encryption status (for potential decryption layer)

**Step 2: Calculate Byte Ranges**
For a 1MB file (1,048,576 bytes):

- Chunk 0: bytes 0-262143 (256KB)
- Chunk 1: bytes 262144-524287 (256KB)
- Chunk 2: bytes 524288-786431 (256KB)
- Chunk 3: bytes 786432-1048575 (262KB - partial)

**Step 3: Download Chunks in Parallel**
**Concurrency Control:**

- Tokio Semaphore limits to 5 concurrent downloads
- Conservative limit accounts for multi-source scenarios
- Example: 3 seeders × 5 chunks each = 15 total parallel downloads

**Step 4: Assemble File**

### 5.3 Progress Tracking

**Progress States:**

```rust
pub enum DownloadStatus {
    FetchingMetadata,  // Step 1: Getting file info
    Downloading,       // Step 2-3: Downloading chunks
    Assembling,        // Step 4: Writing final file
    Completed,         // Success
    Failed,            // Error occurred
}
```

**Progress Data:**

```rust
pub struct HttpDownloadProgress {
    pub file_hash: String,
    pub chunks_total: usize,
    pub chunks_downloaded: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub status: DownloadStatus,
}
```

### 5.4 Error Handling

**Timeout Handling:**

Each chunk request has a 30-second timeout. If a chunk fails:

1. Request times out after 30s
2. Error propagated to download task
3. Entire download fails (no retry in v1)

**Future Enhancement:** Implement per-chunk retry with exponential backoff.

**Validation:**

- Verify 206 Partial Content response
- Verify chunk size matches expected range
- Hash verification of downloaded chunks (implemented in multi_source_download.rs)

---

## 6. DHT Integration

### 6.1 FileMetadata Schema Extension

**Extended Schema with HTTP Sources:**

```rust
pub struct FileMetadata {
    // ... existing fields
    pub http_sources: Option<Vec<HttpSourceInfo>>,
}

pub struct HttpSourceInfo {
    pub url: String,                        // e.g., "http://192.168.1.5:8080"
    pub auth_header: Option<String>,        // Future: Bearer tokens
    pub verify_ssl: bool,                   // Future: HTTPS cert verification
    pub headers: Option<Vec<(String, String)>>, // Future: Custom headers
    pub timeout_secs: Option<u32>,          // Future: Per-source timeout
}
```

**Design Decisions:**

- Array of sources - supports multiple HTTP seeders per file
- Future-proof fields - auth_header, verify_ssl, headers for Phase 2+
- Automatic publishing on file upload in `main.rs`: `upload_and_publish_file` command

### 6.2 Local IP Detection

**Challenge:** HTTP server binds to `0.0.0.0:8080` (all interfaces), but `0.0.0.0` is not routable by other peers.

**Solution:** use `get_local_ip()` function in `headless.rs` to return actual local IP for HTTP URL construction

---

## 7. Frontend Integration

### 7.1 Protocol Selection UI

**Component:** `PeerSelectionModal.svelte`

**New Prop:**

```typescript
export let protocol: "http" | "webrtc" = "http";
```

**Visual Design:**

- Active protocol: Default button styling (highlighted)
- Inactive protocol: Outline styling (subtle)

### 7.2 Download Flow Integration

**Component:** `DownloadSearchSection.svelte`

**State Variables:**

```typescript
let selectedProtocol: "http" | "webrtc" = "http";
let showPeerSelectionModal = false;
let selectedFile: FileMetadata | null = null;
let availablePeers: PeerInfo[] = [];
```

**Download Initiation:**

```typescript
async function confirmPeerSelection() {
  const selectedPeerIds = /* get selected peer IDs */;

  if (selectedProtocol === 'http') {
    await handleHttpDownload(selectedFile, selectedPeerIds);
  } else {
    await handleWebRtcDownload(selectedFile, selectedPeerIds);
  }

  showPeerSelectionModal = false;
}
```

### 7.3 Transfer Event Bus Integration ✅

The HTTP protocol handlers are now fully integrated with the Transfer Event Bus, enabling real-time UI updates for all HTTP downloads.

**Event-Aware Constructors:**

```rust
use crate::protocols::http::HttpProtocolHandler;

// Without event bus (existing behavior)
let handler = HttpProtocolHandler::new();

// With event bus (enables UI updates)
let handler = HttpProtocolHandler::with_event_bus(app_handle.clone());

// With custom timeout and event bus
let handler = HttpProtocolHandler::with_timeout_and_event_bus(60, app_handle.clone());
```

**HTTP Download Client Integration:**

```rust
use crate::http_download::HttpDownloadClient;

// With event bus
let client = HttpDownloadClient::new_with_event_bus(app_handle.clone());

// With peer ID for source identification
let client = HttpDownloadClient::new_with_peer_id_and_event_bus(
    "peer-123".to_string(),
    app_handle.clone()
);

// Download with full event lifecycle
client.download_file_with_events(&url, &output_path, config).await?;

// Resume download with events
client.resume_download_with_events(&url, &output_path, offset, config).await?;
```

**Events Emitted:**

| Method | Events |
|--------|--------|
| `download_with_progress()` | TransferStarted → SourceConnected → TransferProgress (every 2s) → TransferCompleted/TransferFailed |
| `cancel_download()` | TransferCanceled |
| `download_chunks_with_events()` | ChunkCompleted, ChunkFailed (per chunk) |
| `resume_download_with_events()` | TransferResumed → SourceConnected → TransferProgress → TransferCompleted/TransferFailed |

**ChunkFailed Event Points:**

The HTTP download client emits `ChunkFailed` events at 4 failure points:

1. **Network request failure** - HTTP request fails to send
2. **Non-206 HTTP response** - Server doesn't return Partial Content status
3. **Failed to read response bytes** - Chunk data cannot be read
4. **Chunk size mismatch** - Downloaded size doesn't match expected

**Error Categorization:**

Failures are categorized for proper UI display:
- `ErrorCategory::Network` - Connection failures, HTTP errors
- `ErrorCategory::Filesystem` - File creation/write errors

### 7.4 Known UI Limitations

**Issue #1: Download Progress** ✅ **RESOLVED**

- **Previous Problem:** HTTP downloads didn't appear in UI
- **Resolution:** Transfer Event Bus integration now emits all lifecycle events
- **Current State:** HTTP downloads show progress bars, speed, ETA in Download.svelte

**Issue #2: No Server Status Indicator**

- **Problem:** No UI element showing HTTP server running/stopped state
- **Impact:** Users can't verify server is active
- **Planned Fix:** Add status card in Settings or Network page

**Issue #3: File Save Dialog Bug (macOS)**

- **Problem:** Using `filters: [{ extensions: ['*'] }]` appends `.*` to filename
- **Example:** Saving `document.pdf` creates `document.pdf.*`
- **Workaround:** Remove filters parameter or use file extension detection
- **Planned Fix:** Platform-specific dialog handling

---

## 8. Security Considerations

### 8.1 Current Security Posture

**⚠️ WARNING:** This implementation is **not suitable for public internet use** in its current state.

**Missing Security Features:**

1. **No encryption** - Files transmitted in plaintext
2. **No authentication** - Anyone with URL can download
3. **No HTTPS** - HTTP traffic not encrypted
4. **No rate limiting** - Vulnerable to DoS attacks
5. **No access control** - No permission system

**Acceptable Use Cases (Current State):**

- Local testing (127.0.0.1)
- Trusted LAN (private network)
- Development environment

**NOT Acceptable:**

- Public internet
- Sensitive data
- Production use
- Untrusted networks

### 8.2 Privacy Analysis

**IP Address Exposure:**

- HTTP URLs in DHT contain seeder's IP address
- Downloader's IP visible to seeder in HTTP logs

**Download Pattern Analysis:**

- HTTP server logs reveal what files are downloaded

---

## 9. Future Work

### 9.1 UI Progress Tracking ✅ **COMPLETED**

- **Status:** Implemented via Transfer Event Bus integration
- **What was done:**
  - Added event-aware constructors to HTTP handlers
  - Implemented `download_file_with_events()` and `resume_download_with_events()`
  - Added ChunkFailed event emission at 4 failure points
  - Integrated progress throttling (2-second intervals)
  - Connected to frontend via `subscribeToTransferEvents()` in App.svelte

### 9.2 Multi-Source HTTP Integration

- **Goal:** Download different chunks from multiple HTTP seeders
- **Tasks:**
  - Extend chunk scheduler to support HTTP sources
  - Peer selection: allow multiple HTTP seeders
  - Chunk distribution algorithm
  - Aggregate bandwidth from all sources

### 9.3 Automatic Protocol Selection

- **Goal:** Auto-select HTTP/WebTorrent based on network capability
- **Tasks:**
  - Detect public IP vs NAT status (use existing AutoNAT v2)
  - Implement `selectDefaultSeedingProtocol()`:
    - IF public IP → return "http"
    - ELSE → return "webtorrent"
  - Implement `selectDownloadProtocol()` considering:
    - Network conditions (bandwidth, latency)
    - Seeder availability per protocol
    - Client capabilities
  - Remove manual toggle (or make advanced option)