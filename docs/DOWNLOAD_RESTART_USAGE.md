# Download Restart Usage Guide

**Implementation Owner:** Nick (Team Hawks)
**Status:** ✅ Complete and Ready for Integration

---

## Quick Start

### 1. Using the UI Component

Add the download restart control component to any Svelte page:

```svelte
<script lang="ts">
  import DownloadRestartControls from '$lib/components/download/DownloadRestartControls.svelte'
</script>

<DownloadRestartControls
  downloadId=""
  sourceUrl="http://localhost:8080/myfile.bin"
  destinationPath="/Users/you/Downloads/myfile.bin"
  expectedSha256={null}
/>
```

The component will:
- ✅ Show start/pause/resume buttons based on state
- ✅ Display real-time progress
- ✅ Show banners for restart scenarios
- ✅ Prevent double-starts automatically
- ✅ Listen for backend events

---

### 2. Using Tauri Commands Directly

From your frontend TypeScript/JavaScript:

```typescript
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

// Start a download
const downloadId = await invoke<string>('start_download_restart', {
  request: {
    download_id: null, // Auto-generate UUID
    source_url: 'http://localhost:8080/file.bin',
    destination_path: '/Users/you/Downloads/file.bin',
    expected_sha256: 'abc123...' // Optional
  }
})

// Pause download
await invoke('pause_download_restart', { downloadId })

// Resume download
await invoke('resume_download_restart', { downloadId })

// Get current status
const status = await invoke('get_download_status_restart', { downloadId })

// Listen for real-time updates
const unlisten = await listen('download_status', (event) => {
  console.log('Download status:', event.payload)
  // {
  //   download_id: "...",
  //   state: "Downloading",
  //   bytes_downloaded: 5242880,
  //   expected_size: 10485760,
  //   etag: "\"abc123\"",
  //   lease_exp: 1704067200,
  //   last_error: null
  // }
})
```

---

### 3. Download States

The download progresses through these states:

```
[Idle] → [Handshake] → [PreparingHead] → [PreflightStorage] →
[ValidatingMetadata] → [Downloading] → [VerifyingSha] →
[FinalizingIo] → [Completed]
```

**Pause Path:**
```
[Downloading] → [Paused] → (user action) → [AwaitingResume] →
[PreparingHead] → [Downloading]
```

**Failure Path:**
```
[any state] → [Failed] → (user retry) → [AwaitingResume]
```

**Restart Scenarios:**
- `Restarting`: Server changed (weak ETag, size mismatch, etc.)
- `LeaseExpired`: Download lease expired, requesting new lease

---

### 4. Error Handling

All errors are mapped to human-readable messages:

| Error Code | UI Message |
|-----------|-----------|
| `DOWNLOAD_NOT_FOUND` | "Download not found. It may have been removed." |
| `STORAGE_EXHAUSTED` | "Insufficient disk space. Please free up space and try again." |
| `DOWNLOAD_SOURCE_ERROR` | "Download source error: {details}" |
| `IO_ERROR` | "File system error: {details}" |

**Restart Banners:**
- **Weak ETag:** Yellow warning banner
- **Range Unsupported:** Yellow warning banner
- **HTTP 416:** Yellow warning banner
- **Disk Full:** Red error banner
- **Lease Expired:** Blue info banner

---

### 5. Running the Demo

Test the pause/resume functionality end-to-end:

```bash
cd demo
./http-transfer.sh
```

**What it does:**
1. Creates a 10 MB test file
2. Starts HTTP server (Node A)
3. Downloads 50% of file (Node B)
4. Simulates process restart
5. Resumes from saved offset
6. Verifies SHA-256 hash matches

**Expected output:**
```
[DEMO] Step 1: Creating test file (10485760 bytes)...
✓ Test file created
[DEMO] Expected SHA-256: abc123...
[DEMO] Step 2: Starting Node A HTTP server on port 8080...
✓ Node A HTTP server running (PID: 12345)
...
╔════════════════════════════════════════════════════════════╗
║                   DEMO SUCCESSFUL                          ║
╚════════════════════════════════════════════════════════════╝
```

---

### 6. Running Tests

**Snapshot Tests:**
```bash
cd src-tauri
cargo test download_restart -- --nocapture
```

**All Tests:**
```bash
cargo test
```

**Review Snapshots (after changes):**
```bash
cargo insta review
```

---

### 7. CI Integration

The download restart demo runs automatically on:
- Push to `main` (when relevant files change)
- Pull requests
- Manual workflow dispatch

**Workflow:** `.github/workflows/download-restart-demo.yml`

**What CI tests:**
- ✅ Demo script execution
- ✅ Metadata file creation
- ✅ File size verification
- ✅ SHA-256 hash validation
- ✅ Snapshot tests
- ✅ Cross-platform (Ubuntu + macOS)

---

## Integration with Other Components

### Matt's DHT Handshake (Control Plane)

When Matt implements the DHT handshake, integrate like this:

```rust
// In download_restart.rs
pub async fn start_download(&self, request: StartDownloadRequest) -> Result<DownloadId, DownloadError> {
    // ... existing code ...

    // TODO: Integrate Matt's handshake
    let handshake_response = dht_service.request_lease(file_id, download_id).await?;

    task.status.etag = Some(handshake_response.etag);
    task.status.lease_exp = Some(handshake_response.lease_exp);
    task.metadata.lease_exp = Some(handshake_response.lease_exp);

    // ... transition to PreparingHead state ...
}
```

### Josh's HTTP Range Client (Data Plane)

When Josh implements the Range client, integrate like this:

```rust
// In download_restart.rs
async fn do_download(&self, download_id: &str) -> Result<(), DownloadError> {
    let task = /* get task */;

    // Use Josh's HTTP client
    let http_client = HttpRangeClient::new();
    let stream = http_client.fetch_range(
        &task.metadata.url,
        task.metadata.bytes_downloaded,
        task.metadata.etag.clone()
    ).await?;

    // Stream to disk with progress events
    while let Some(chunk) = stream.next().await {
        // Write chunk, update progress, emit events
    }
}
```

### Elliot's Persistence Layer

When Elliot implements metadata persistence, integrate like this:

```rust
// In download_restart.rs
async fn persist_metadata(&self, download_id: &str) -> Result<(), DownloadError> {
    let task = /* get task */;

    // Use Elliot's atomic write
    persistence::write_metadata_atomic(
        &task.destination_path,
        &task.metadata
    ).await?;

    Ok(())
}
```

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend (Svelte)                     │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ DownloadRestartControls.svelte                         │  │
│  │  - Start/Pause/Resume buttons                          │  │
│  │  - Progress display                                    │  │
│  │  - Restart banners                                     │  │
│  │  - Event listener (download_status)                    │  │
│  └────────────────────────────────────────────────────────┘  │
│                           ▲                                  │
│                           │ Events                           │
└───────────────────────────┼──────────────────────────────────┘
                            │
                            │ Tauri Commands
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                     Backend (Rust/Tauri)                     │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ DownloadRestartService                                 │  │
│  │  - start_download()                                    │  │
│  │  - pause_download()                                    │  │
│  │  - resume_download()                                   │  │
│  │  - get_status()                                        │  │
│  │  - emit_status() [Events]                              │  │
│  └────────────────────────────────────────────────────────┘  │
│                           │                                  │
│                           │ State Management                 │
│                           ▼                                  │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ HashMap<DownloadId, DownloadTask>                      │  │
│  │  - DownloadStatus (state, progress, errors)            │  │
│  │  - DownloadMetadata (URL, ETag, size, hash)            │  │
│  │  - PathBuf (destination)                               │  │
│  └────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                            │
                            │ Future Integration
                            ▼
            ┌───────────────────────────────┐
            │ Matt: DHT Handshake/Lease     │
            │ Josh: HTTP Range Client       │
            │ Elliot: Metadata Persistence  │
            │ Samridh: Full State Machine   │
            └───────────────────────────────┘
```

---

## Troubleshooting

### Download doesn't start
- **Check:** Is the source URL reachable?
- **Check:** Does the destination directory exist?
- **Check:** Is there sufficient disk space?

### Pause button disabled
- Download must be in `Downloading` or `PersistingProgress` state

### Resume doesn't work
- Download must be in `Paused` or `AwaitingResume` state
- Check console for error messages

### Events not received
- Ensure you're listening for `download_status` events
- Check that `downloadId` matches the event payload

### Demo script fails
- Python 3 must be installed
- Port 8080 must be available
- Check demo logs in console output

---

## File Locations

| Component | Path |
|-----------|------|
| Backend Service | `src-tauri/src/download_restart.rs` |
| UI Component | `src/lib/components/download/DownloadRestartControls.svelte` |
| Tests | `src-tauri/tests/download_restart_test.rs` |
| Demo Script | `demo/http-transfer.sh` |
| CI Workflow | `.github/workflows/download-restart-demo.yml` |
| Documentation | `docs/download-restart.md` |
| Implementation Summary | `IMPLEMENTATION_SUMMARY.md` |

---

## Next Steps for Team

1. **Matt:** Implement DHT handshake and integrate `lease_exp` tracking
2. **Josh:** Implement HTTP Range client and integrate with download service
3. **Elliot:** Implement `.meta.json` atomic persistence
4. **Samridh:** Build full state machine on top of this baseline
5. **Dash:** Create fault injection tests for all restart scenarios

---

**Questions?** Contact Nick (Team Hawks)

**Last Updated:** 2025-11-10
