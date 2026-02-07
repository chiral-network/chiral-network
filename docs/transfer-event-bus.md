# Transfer Event Bus

## Overview

The Transfer Event Bus is a typed, protocol-agnostic event system for communicating file transfer lifecycle events from the Rust backend to the frontend UI. It provides a standardized infrastructure for tracking and responding to transfer state changes across all protocols (HTTP, FTP, P2P, WebRTC, BitTorrent).

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Rust Backend                                 │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │ HTTP Handler │  │ FTP Handler  │  │  BitTorrent  │           │
│  │[OK]Integrated  │  │ [OK]Integrated│  │  Handler [OK]  │          │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘           │
│         │                  │                  │                 │
│  ┌──────┴───────┐  ┌──────┴───────┐  ┌──────┴───────┐           │
│  │HttpDownload  │  │  download_   │  │bittorrent_   │           │
│  │Client [OK]    │  │  restart [OK]  │  │  │handler [OK] │           │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘           │
│         │                  │                  │                 │
│         └──────────────────┴──────────────────┘                 │
│                            │                                    │
│                  ┌─────────[v]──────────┐                         │
│                  │ TransferEventBus   │                         │
│                  │  (transfer_events) │                         │
│                  └─────────┬──────────┘                         │
│                            │                                    │
│              ┌─────────────┼─────────────┐                      │
│              │             │             │                      │
│    ┌─────────[v]───┐  ┌─────[v]─────┐  ┌────[v]────────┐              │
│    │Tauri Emitter│  │Analytics  │  │MultiSource  │              │
│    │             │  │Service [OK] │  │Download [OK] │              │
│    └─────────┬───┘  └───────────┘  └─────────────┘              │
└──────────────┼──────────────────────────────────────────────────┘
               │
      IPC Event Channel
               │
┌──────────────[v]──────────────────────────────────────────────────┐
│                  Frontend (Svelte)                              │
│                                                                 │
│                  ┌──────────────────┐                           │
│                  │  Tauri Listener  │                           │
│                  │  (App.svelte) [OK] │                          │
│                  └────────┬─────────┘                           │
│                           │                                     │
│                  ┌────────[v]─────────┐                           │
│                  │transferEventsStore│                          │
│                  │ (Svelte Writable)│                           │
│                  └────────┬─────────┘                           │
│                           │                                     │
│         ┌─────────────────┼─────────────────┐                   │
│         │                 │                 │                   │
│    ┌────[v]────┐     ┌─────[v]──────┐    ┌────[v]─────┐               │
│    │Download │     │  Progress  │    │Analytics │               │
│    │  Page   │     │    Bar     │    │   Page   │               │
│    └─────────┘     └────────────┘    └──────────┘               │
└─────────────────────────────────────────────────────────────────┘
```

## Protocol Integration Status

The Transfer Event Bus is now fully integrated across all major protocol handlers:

| Protocol Handler | File | Status | Events Emitted |
|-----------------|------|--------|----------------|
| HTTP Protocol | `protocols/http.rs` | [OK] Integrated | Full lifecycle |
| FTP Protocol | `protocols/ftp.rs` | [OK] Integrated | Full lifecycle |
| BitTorrent Protocol | `protocols/bittorrent.rs` | [OK] Integrated | Full lifecycle |
| HTTP Download Client | `http_download.rs` | [OK] Integrated | Full lifecycle + ChunkFailed |
| Download Restart | `download_restart.rs` | [OK] Integrated | Queue/Pause/Resume/Progress |
| File Transfer | `file_transfer.rs` | [OK] Integrated | Start/Complete/Failed |
| BitTorrent Handler | `bittorrent_handler.rs` | [OK] Integrated | Progress/Pause/Resume |
| Multi-Source Download | `multi_source_download.rs` | [OK] Integrated | Full lifecycle |
| Analytics Service | `analytics.rs` | [OK] Integrated | Event subscriber |

## Event Types

### Lifecycle Events

The system provides 13 typed events covering the complete transfer lifecycle:

1. **Queued** - Transfer added to queue
2. **Started** - Transfer begins (sources discovered)
3. **SourceConnected** - A source (peer/server) connected
4. **SourceDisconnected** - A source disconnected
5. **ChunkCompleted** - A chunk downloaded successfully
6. **ChunkFailed** - A chunk download failed
7. **Progress** - Periodic progress update
8. **Paused** - Transfer paused
9. **Resumed** - Transfer resumed
10. **Completed** - Transfer finished successfully
11. **Failed** - Transfer failed permanently
12. **Canceled** - Transfer canceled by user
13. **SpeedUpdate** - Real-time speed update

## Backend Usage

### Core API

```rust
use crate::transfer_events::*;

// Create event bus (requires app_handle from Tauri)
let event_bus = TransferEventBus::new(app_handle);

// Emit a queued event
event_bus.emit_queued(TransferQueuedEvent {
    transfer_id: "download-123".to_string(),
    file_hash: "abc123".to_string(),
    file_name: "example.pdf".to_string(),
    file_size: 1_048_576,
    output_path: "/tmp/example.pdf".to_string(),
    priority: TransferPriority::Normal,
    queued_at: current_timestamp_ms(),
    queue_position: 1,
    estimated_sources: 5,
});

// Emit a progress update
event_bus.emit_progress(TransferProgressEvent {
    transfer_id: "download-123".to_string(),
    downloaded_bytes: 524_288,
    total_bytes: 1_048_576,
    completed_chunks: 2,
    total_chunks: 4,
    progress_percentage: 50.0,
    download_speed_bps: 1_048_576.0,
    upload_speed_bps: 0.0,
    eta_seconds: Some(1),
    active_sources: 3,
    timestamp: current_timestamp_ms(),
});

// Emit completion
event_bus.emit_completed(TransferCompletedEvent {
    transfer_id: "download-123".to_string(),
    file_hash: "abc123".to_string(),
    file_name: "example.pdf".to_string(),
    file_size: 1_048_576,
    output_path: "/tmp/example.pdf".to_string(),
    completed_at: current_timestamp_ms(),
    duration_seconds: 10,
    average_speed_bps: 104_857.6,
    total_chunks: 4,
    sources_used: vec![],
});
```

### Protocol Handler Integration

Protocol handlers use an **opt-in constructor pattern** to enable event bus integration. This maintains backward compatibility while allowing event emission for UI updates.

#### HTTP Protocol Handler

```rust
use crate::protocols::http::HttpProtocolHandler;

// Without event bus (existing behavior)
let handler = HttpProtocolHandler::new();

// With event bus (enables UI updates)
let handler = HttpProtocolHandler::with_event_bus(app_handle.clone());

// With custom timeout and event bus
let handler = HttpProtocolHandler::with_timeout_and_event_bus(60, app_handle.clone());
```

**Events emitted by HTTP handler:**

| Method | Events |
|--------|--------|
| `download_with_progress()` | TransferStarted -> SourceConnected -> TransferProgress (throttled) -> TransferCompleted/TransferFailed |
| `cancel_download()` | TransferCanceled |

#### FTP Protocol Handler

```rust
use crate::protocols::ftp::FtpProtocolHandler;

// Without event bus (existing behavior)
let handler = FtpProtocolHandler::new();

// With event bus
let handler = FtpProtocolHandler::with_event_bus(app_handle.clone());

// With custom config and event bus
let handler = FtpProtocolHandler::with_config_and_event_bus(config, app_handle.clone());
```

**Events emitted by FTP handler:**

| Method | Events |
|--------|--------|
| `download()` | TransferStarted -> SourceConnected -> ChunkCompleted -> SourceDisconnected -> TransferCompleted/TransferFailed |
| `pause_download()` | TransferPaused |
| `cancel_download()` | TransferCanceled |

#### BitTorrent Protocol Handler

```rust
use crate::protocols::bittorrent::BitTorrentProtocolHandler;

// Without event bus (existing behavior)
let handler = BitTorrentProtocolHandler::new(session_handle);

// With event bus
let handler = BitTorrentProtocolHandler::new_with_event_bus(session_handle, app_handle.clone());

// With download directory and event bus
let handler = BitTorrentProtocolHandler::with_download_directory_and_event_bus(
    download_dir, 
    app_handle.clone()
);
```

**Events emitted by BitTorrent handler:**

| Method | Events |
|--------|--------|
| `download()` | TransferStarted -> SourceConnected -> TransferFailed (if start fails) |
| `pause_download()` | TransferPaused |
| `resume_download()` | TransferResumed |
| `cancel_download()` | TransferCanceled |
| `get_download_progress()` | TransferProgress (throttled) -> TransferCompleted/TransferFailed |

#### HTTP Download Client

```rust
use crate::http_download::HttpDownloadClient;

// Without event bus
let client = HttpDownloadClient::new();

// With event bus
let client = HttpDownloadClient::new_with_event_bus(app_handle.clone());

// With peer ID and event bus (for source identification)
let client = HttpDownloadClient::new_with_peer_id_and_event_bus(
    "peer-123".to_string(),
    app_handle.clone()
);

// Download with full event lifecycle
client.download_file_with_events(
    &url,
    &output_path,
    HttpDownloadConfig {
        transfer_id: "download-123".to_string(),
        file_name: "example.pdf".to_string(),
    }
).await?;

// Resume download with events
client.resume_download_with_events(
    &url,
    &output_path,
    resume_offset,
    HttpDownloadConfig { ... }
).await?;
```

**ChunkFailed Event Emission Points:**

The HTTP download client emits `ChunkFailed` events at 4 specific failure points:

1. **Network request failure** - When the HTTP request fails to send
2. **Non-206 HTTP response** - When server doesn't return expected Partial Content status
3. **Failed to read response bytes** - When chunk data cannot be read from response
4. **Chunk size mismatch** - When downloaded chunk size doesn't match expected size

### Progress Throttling

To prevent flooding the frontend with updates, protocol handlers implement **progress throttling**:

```rust
// Progress events are emitted at most every 2 seconds
const PROGRESS_THROTTLE_MS: u64 = 2000;

// In handler implementation
if now_ms() - self.last_progress_event >= PROGRESS_THROTTLE_MS {
    event_bus.emit_progress(...);
    self.last_progress_event = now_ms();
}
```

### Helper Functions

Protocol handlers use common helper functions for event generation:

```rust
/// Get current timestamp in milliseconds
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Extract filename from URL for display
fn extract_file_name(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

/// Extract display name from magnet link (BitTorrent)
fn extract_display_name(identifier: &str) -> Option<String> {
    // Parses dn= parameter from magnet URI
}
```

### Analytics Integration

The Analytics service subscribes to transfer events for metrics collection:

```rust
impl AnalyticsService {
    /// Handle incoming transfer events for analytics tracking
    pub fn handle_transfer_event(&self, event: &TransferEvent) {
        match event {
            TransferEvent::Completed(e) => {
                self.record_download_completed(
                    &e.file_hash,
                    e.file_size,
                    e.duration_seconds,
                );
            }
            TransferEvent::Failed(e) => {
                self.record_download_failed(&e.file_hash, &e.error);
            }
            // ... handle other events
        }
    }
}
```

In `main.rs`, the analytics service is wired to receive DHT events:

```rust
// DHT event loop integration
DhtEvent::DownloadedFile { hash, size, duration } => {
    analytics.record_download_completed(&hash, size, duration);
}
DhtEvent::PublishedFile { hash, size } => {
    analytics.record_upload_completed(&hash, size);
}
```

### Event Structs

Each event type has a corresponding struct with strongly-typed fields:

**TransferQueuedEvent**
```rust
pub struct TransferQueuedEvent {
    pub transfer_id: String,
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub output_path: String,
    pub priority: TransferPriority,
    pub queued_at: u64,
    pub queue_position: usize,
    pub estimated_sources: usize,
}
```

**TransferProgressEvent**
```rust
pub struct TransferProgressEvent {
    pub transfer_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub progress_percentage: f64,
    pub download_speed_bps: f64,
    pub upload_speed_bps: f64,
    pub eta_seconds: Option<u32>,
    pub active_sources: usize,
    pub timestamp: u64,
}
```

**ChunkFailedEvent**
```rust
pub struct ChunkFailedEvent {
    pub transfer_id: String,
    pub chunk_id: u32,
    pub source_id: String,
    pub source_type: SourceType,
    pub error: String,
    pub error_category: ErrorCategory,
    pub timestamp: u64,
    pub will_retry: bool,
    pub retry_count: u32,
}
```

**TransferResumedEvent**
```rust
pub struct TransferResumedEvent {
    pub transfer_id: String,
    pub file_hash: String,
    pub resumed_at: u64,
    pub bytes_already_downloaded: u64,
    pub remaining_bytes: u64,
    pub resumed_from_checkpoint: bool,
}
```

**SourceInfo**
```rust
pub struct SourceInfo {
    pub id: String,
    pub source_type: SourceType,
    pub address: String,
    pub reputation: Option<f64>,
    pub estimated_speed_bps: Option<f64>,
    pub latency_ms: Option<u32>,
    pub location: Option<String>,
}
```

### Supporting Types

**TransferPriority**
```rust
pub enum TransferPriority {
    Low,
    Normal,
    High,
}
```

**SourceType**
```rust
pub enum SourceType {
    Http,
    Ftp,
    P2p,
    BitTorrent,
    WebRtc,
}
```

**DisconnectReason**
```rust
pub enum DisconnectReason {
    NetworkError,
    Timeout,
    SourceUnavailable,
    ProtocolError,
    UserCanceled,
    Completed,
    RateLimited,
    Other(String),
}
```

**ErrorCategory**
```rust
pub enum ErrorCategory {
    Network,      // Connection failures, HTTP errors
    Protocol,     // Protocol-level errors
    Filesystem,   // File creation/write errors
    Verification, // Hash/integrity check failures
    Authentication,
    NoSources,
    RateLimit,
    Unknown,
}
```

### Utility Functions

```rust
// Get current Unix timestamp in milliseconds
pub fn current_timestamp_ms() -> u64

// Get current Unix timestamp in seconds
pub fn current_timestamp_secs() -> u64

// Calculate progress percentage
pub fn calculate_progress(downloaded: u64, total: u64) -> f64

// Calculate ETA in seconds based on current speed
pub fn calculate_eta(remaining_bytes: u64, speed_bps: f64) -> Option<u32>
```

## Frontend Usage

### Setup in App Component

The frontend subscription is initialized in `App.svelte` during the `onMount` lifecycle:

```typescript
// App.svelte
import { onMount, onDestroy } from 'svelte';
import { subscribeToTransferEvents } from '$lib/stores/transferEventsStore';

let unsubscribe: (() => void) | null = null;

onMount(async () => {
  // Subscribe to all transfer events - THIS IS CRITICAL
  unsubscribe = await subscribeToTransferEvents();
});

onDestroy(() => {
  // Cleanup subscription
  if (unsubscribe) {
    unsubscribe();
  }
});
```

> **Important**: The `subscribeToTransferEvents()` call in `App.svelte` is essential. Without it, no transfer events will be received by the frontend, and downloads will appear invisible to the UI.

### Store API

```typescript
import { 
  transferStore,
  activeTransfers,
  queuedTransfers,
  completedTransfers,
  failedTransfers,
  pausedTransfers,
  formatBytes,
  formatSpeed,
  formatETA,
  getStatusColor
} from '$lib/stores/transferEventsStore';

// Access store state
$transferStore.transfers          // Map<string, Transfer>
$transferStore.activeCount        // number
$transferStore.queuedCount        // number
$transferStore.completedCount     // number
$transferStore.failedCount        // number
$transferStore.totalDownloadSpeed // number (bytes per second)
$transferStore.totalUploadSpeed   // number (bytes per second)

// Access derived stores
$activeTransfers    // Transfer[] - currently downloading
$queuedTransfers    // Transfer[] - waiting to start
$completedTransfers // Transfer[] - successfully finished
$failedTransfers    // Transfer[] - failed downloads
$pausedTransfers    // Transfer[] - user-paused

// Store methods
transferStore.getTransfer(transferId: string)      // Get specific transfer
transferStore.removeTransfer(transferId: string)   // Remove from store
transferStore.clearFinished()                       // Clear completed/failed
transferStore.reset()                              // Reset entire store
```

### Transfer Object

```typescript
interface Transfer {
  transferId: string;
  fileHash: string;
  fileName: string;
  fileSize: number;
  outputPath: string;
  status: TransferStatus;
  priority: TransferPriority;
  
  // Progress tracking
  downloadedBytes: number;
  completedChunks: number;
  totalChunks: number;
  progressPercentage: number;
  
  // Speed tracking
  downloadSpeedBps: number;
  uploadSpeedBps: number;
  etaSeconds?: number;
  
  // Source tracking
  availableSources: SourceInfo[];
  connectedSources: Map<string, ConnectedSource>;
  activeSources: number;
  
  // Timing
  queuedAt?: number;
  startedAt?: number;
  completedAt?: number;
  durationSeconds?: number;
  averageSpeedBps?: number;
  
  // Error tracking
  error?: string;
  errorCategory?: string;
  retryPossible?: boolean;
}
```

### Using in Components

```typescript
// Example: Download progress display
<script lang="ts">
  import { transferStore, formatBytes, formatSpeed, formatETA } from '$lib/stores/transferEventsStore';
  
  export let transferId: string;
  
  $: transfer = transferStore.getTransfer(transferId);
</script>

{#if transfer}
  <div class="transfer">
    <h3>{transfer.fileName}</h3>
    <div class="progress-bar">
      <div style="width: {transfer.progressPercentage}%" />
    </div>
    <div class="stats">
      <span>{formatBytes(transfer.downloadedBytes)} / {formatBytes(transfer.fileSize)}</span>
      <span>{formatSpeed(transfer.downloadSpeedBps)}</span>
      <span>ETA: {formatETA(transfer.etaSeconds)}</span>
      <span>{transfer.activeSources} sources</span>
    </div>
  </div>
{/if}
```

### Utility Functions

```typescript
// Format bytes as human-readable string
formatBytes(1048576) // "1.00 MB"

// Format speed as human-readable string
formatSpeed(1048576) // "1.00 MB/s"

// Format ETA as human-readable string
formatETA(120) // "2m"
formatETA(3661) // "1h 1m"

// Get status color for UI
getStatusColor("downloading") // "blue"
getStatusColor("completed")   // "green"
getStatusColor("failed")      // "red"
```

## Event Channels

The event bus emits to multiple channels:

1. **Typed channels**: `transfer:queued`, `transfer:started`, `transfer:progress`, etc.
2. **Generic channel**: `transfer:event` (receives all events)

This allows components to subscribe to specific event types or all events.

## Best Practices

### Backend

1. **Always use transfer_id**: Generate unique IDs for each transfer (UUID recommended)
2. **Emit events in order**: Follow the lifecycle: queued -> started -> progress... -> completed/failed
3. **Include context**: Always populate relevant fields (file_hash, file_name, etc.)
4. **Progress updates**: Emit progress every 2 seconds (use throttling)
5. **Speed updates**: Can emit more frequently (every 100-500ms) for smooth UI
6. **Error handling**: Always emit failed event with descriptive error messages and ErrorCategory
7. **Cleanup**: Emit canceled event when user cancels, not just silence
8. **Use opt-in constructors**: Use `with_event_bus()` constructors for UI-connected handlers

### Frontend

1. **Subscribe once**: Subscribe to events in App.svelte `onMount`, not in every component
2. **Use derived stores**: Filter transfers using derived stores for better performance
3. **Cleanup**: Always return unsubscribe function from onMount and call in onDestroy
4. **Handle missing transfers**: Check if transfer exists before accessing properties
5. **Use reactive statements**: Leverage Svelte's reactivity (`$:` syntax)
6. **Show user feedback**: Display errors, completion notifications, etc.

## Configuration

### Default Values

The system uses sensible defaults that can be adjusted per use case:

| Setting | Default | Description |
|---------|---------|-------------|
| Progress emit interval | 2 seconds | How often to emit progress events (throttled) |
| Speed update interval | 100-500ms | How often to emit speed updates |
| Chunk size | Varies by protocol | Size of transfer chunks |
| Transfer ID format | UUID v4 | Unique identifier format |

## Testing

### Backend Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_progress() {
        assert_eq!(calculate_progress(0, 100), 0.0);
        assert_eq!(calculate_progress(50, 100), 50.0);
        assert_eq!(calculate_progress(100, 100), 100.0);
    }

    #[test]
    fn test_event_serialization() {
        let event = TransferEvent::Progress(TransferProgressEvent {
            transfer_id: "test".to_string(),
            downloaded_bytes: 500,
            total_bytes: 1000,
            completed_chunks: 5,
            total_chunks: 10,
            progress_percentage: 50.0,
            download_speed_bps: 1024.0,
            upload_speed_bps: 0.0,
            eta_seconds: Some(10),
            active_sources: 2,
            timestamp: 123456789,
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: TransferEvent = serde_json::from_str(&json).unwrap();
        
        assert!(matches!(parsed, TransferEvent::Progress(_)));
    }
}
```

### Frontend Tests

```typescript
import { describe, it, expect } from 'vitest';
import { get } from 'svelte/store';
import { transferStore } from '$lib/stores/transferEventsStore';

describe('Transfer Store', () => {
  it('should handle queued event', () => {
    transferStore.handleEvent({
      type: 'queued',
      transferId: 'test-1',
      fileHash: 'abc123',
      fileName: 'test.txt',
      fileSize: 1024,
      outputPath: '/tmp/test.txt',
      priority: 'normal',
      queuedAt: Date.now(),
      queuePosition: 1,
      estimatedSources: 5,
    });

    const state = get(transferStore);
    expect(state.transfers.size).toBe(1);
    expect(state.queuedCount).toBe(1);
  });
});
```

## Troubleshooting

### Events not received in frontend

**Symptoms**: No transfers appear in the store despite backend emitting events.

**Solutions**:
1. **Check that `subscribeToTransferEvents()` was called in App.svelte** - This is the most common issue
2. Verify backend is emitting events (check Rust logs)
3. Check browser console for subscription errors
4. Ensure Tauri event system is working
5. Verify the protocol handler is using an event-bus-enabled constructor

### Transfers not updating

**Symptoms**: Transfer state becomes stale or doesn't reflect backend changes.

**Solutions**:
1. Verify transfer_id matches between events
2. Check that progress events include all required fields
3. Look for TypeScript errors in browser console
4. Verify reactive statements are properly structured
5. Check progress throttling isn't too aggressive

### Performance issues

**Symptoms**: UI becomes laggy with many active transfers.

**Solutions**:
1. Reduce frequency of progress/speed updates in backend (increase throttle interval)
2. Use derived stores instead of filtering in templates
3. Implement virtual scrolling for large transfer lists
4. Debounce UI updates if needed

### Protocol handler not emitting events

**Symptoms**: A specific protocol (HTTP, FTP, BitTorrent) doesn't show progress.

**Solutions**:
1. Ensure you're using the event-bus-enabled constructor (`with_event_bus()`, `new_with_event_bus()`)
2. Check that `app_handle` is being passed correctly
3. Verify the handler's event emission code paths are being executed
4. Check for errors in the Rust logs

## Related Documentation

- [HTTP Protocol Implementation](HTTP_PROTOCOL_IMPLEMENTATION.md)
- [FTP Source Implementation](FTP_SOURCE_IMPLEMENTATION.md)
- [BitTorrent Implementation Guide](bittorrent-implementation-guide.md)
- [Download Restart](download-restart.md)
- [Architecture](architecture.md)