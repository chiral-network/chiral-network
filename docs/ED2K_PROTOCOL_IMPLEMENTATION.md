# ED2K Source Implementation – Unified Source Abstraction

## Summary
This document describes how ED2K is integrated into the Unified Source Abstraction layer. ED2K is treated as a chunk-based, multi-source distribution protocol with strong hash-based validation. Unlike FTP or HTTP, ED2K does not support random file access or byte ranges; it transfers files in fixed 9.28 MB chunks made up of 256 KB sub-chunks.  

The integration allows the orchestrator to request specific chunks, resolve sources dynamically, and verify integrity through MD4-based hashing.

---

## Protocol Overview
The ED2K protocol is a peer-assisted file distribution system used by eDonkey and eMule clients. It provides:

- File identification via MD4-based ED2K hashes  
- Chunk-level segmentation  
- Multi-peer parallel downloading  
- Source discovery through servers or peer exchange  

The protocol revolves around retrieving 256 KB sub-chunks from peers, combining them into 9.28 MB ED2K chunks, then verifying each chunk’s MD4 digest.

---

## Key Features
- **Metadata-Driven Transfers**: The application uses ED2K hashes and chunk maps to locate sources.
- **Chunk-Based Downloading**: Files are logically segmented into 9.28 MB chunks, each composed of 36 × 256 KB sub-chunks.
- **Multi-Source Fetching**: The orchestrator can distribute chunk requests across many sources to maximize throughput.
- **Merklized Integrity Model (ED2K Style)**: ED2K uses a flat list of MD4 chunk hashes; if there is more than one chunk, a root MD4 hash is also used.
- **Consistency With Unified Source Interface**: Maintains the `SourceType`, `ChunkRequest`, `ChunkResponse`, and validation architecture shared across protocols.

---

## Key Files & Components

### Rust Backend
- `src/core/sources/ed2k/mod.rs`: Main module for ED2K logic.  
- `src/core/sources/ed2k/client.rs`: Handles ED2K server communication and peer queries.  
- `src/core/sources/ed2k/chunking.rs`: Defines 256 KB -> 9.28 MB chunk assembly logic.  
- `src/core/sources/ed2k/types.rs`: ED2K metadata structs (hash, chunk map, peer description).  
- `src/core/sources/common.rs`: Shared traits implemented by ED2K.  

---

## Detailed Behavior

### Chunk Structure
ED2K uses:

- **256 KB sub-chunks (“blocks”)** - 36 of them form a **9.28 MB ED2K chunk** - Each chunk gets its own MD4 hash  
- If file has >1 chunk, a root hash (MD4 over all chunk hashes) is also used

#### Chunk Math
```text
ED2K_CHUNK_SIZE = 9_728_000 bytes
ED2K_BLOCK_SIZE = 256_000 bytes
CHUNKS = ceil(file_size / CHUNK_SIZE)
```

### Source Resolution

To fetch data, the ED2K client:

1.  Connects to configured ED2K servers
2.  Submits a file hash lookup
3.  Receives a list of peers holding the file
4.  Validates peers
5.  Reports them to the orchestrator
6.  The orchestrator schedules chunk downloads across peers

---

### Chunk Fetching Pipeline


#### 1. Orchestrator requests a chunk

Issues a `ChunkRequest { chunk_index, source_type: ED2K }`.

#### 2. ED2K client assigns sub-chunk requests

For each 256 KB sub-chunk:

* Select a peer that claims to have it
* Open a TCP session
* Send ED2K block request
* Receive raw bytes

#### 3. Combine into a 9.28 MB chunk

Once all parts arrive:

```text
[block0][block1]...[block35] -> Combined chunk buffer
```

#### 4. Validate with MD4
* Compute MD4(chunk)
* Compare with expected ED2K chunk hash
* If mismatch -> mark peer unreliable -> retry.

---

### Retry Logic

If a sub-chunk fails:

* Peer is penalized
* Another peer is selected
* Re-request missing pieces
* Partial chunk progress is preserved

---

### Serialization Format

#### ED2K Metadata

```json
{
  "hash": "<ed2k-root-hash>",
  "size": 123456789,
  "chunks": [
    {"index": 0, "hash": "<md4>"},
    {"index": 1, "hash": "<md4>"}
  ],
  "sources": [
    {"ip": "x.x.x.x", "port": 4662, "availability": [...]}
  ]
}
```

### Chunk Structure
```json
{
  "type": "ed2k",
  "chunk_index": 3
}
```

### Chunk Response
```json
{
  "chunk_index": 3,
  "data": "<base64>",
  "valid": true
}
```

# ED2K Data Fetching & Verification Implementation
## Overview
The orchestrator integrates ED2K as a source by:

* Extracting Ed2kSourceInfo from the FileMetadata during "start download"

* Assigning 256 KB sub-chunks to ED2K sources like any other source

* Connecting to ED2K server using Ed2kClient

* Grouping all assigned sub-chunks into 9.28 MB chunks

* Downloading each chunk

* Verifying each chunk’s MD4 hash

* Splitting verified chunks into 256 KB sub-chunks for storage

## Key Features
* Sub-Chunk Integrity Awareness: Track and retry each 256 KB block individually

* Peer Reliability Scoring: Peers that send corrupted or incomplete data are deprioritized

* Hash-First Operation: Verify metadata before download

* Resume & Partial Assembly: Incomplete chunks resume where left off

## Integration Points
## Backend -> Orchestrator
Reports available peers and chunk availability maps

### Frontend
Displays sources, availability, and chunk-level progress

## Testing
### Unit Tests
* Chunk boundary math

* MD4 hashing correctness

* Sub-chunk assembly

* Corruption detection

* Retry behavior

### Integration Tests
* Multi-source parallel fetch

* Peer failover

* Server discovery flow

* Combined chunk hashing

### End-to-End Tests
* Fetch file from N peers

* Intentionally corrupt blocks

* Validate recovery and consistency

### Security Considerations
* Hash Trust Model: MD4 hash ensures downloaded chunks are correct if metadata is trusted

* Peer Hostile Behavior: Penalize peers sending invalid data

* Connection Sanitation: Enforce timeouts, check payload lengths, drop invalid connections

* No Arbitrary Write Surfaces: Data written only to preallocated chunk buffers