# Transfer Event Bus

## Overview

The Transfer Event Bus is a typed, protocol-agnostic event system for communicating file transfer lifecycle events from the Rust backend to the frontend UI. It provides a standardized infrastructure for tracking and responding to transfer state changes across all protocols (HTTP, FTP, P2P, WebRTC, BitTorrent).

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Rust Backend                             │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │ HTTP Download│  │ FTP Download │  │ P2P Download │       │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘       │
│         │                  │                  │             │
│         └──────────────────┴──────────────────┘             │
│                            │                                │
│                  ┌─────────▼──────────┐                     │
│                  │ TransferEventBus   │                     │
│                  │  (transfer_events) │                     │
│                  └─────────┬──────────┘                     │
│                            │                                │
│                  ┌─────────▼──────────┐                     │
│                  │   Tauri Emitter    │                     │
│                  └─────────┬──────────┘                     │
└────────────────────────────┼────────────────────────────────┘
                             │
                    IPC Event Channel
                             │
┌────────────────────────────▼────────────────────────────────┐
│                  Frontend (Svelte)                          │
│                                                             │
│                  ┌──────────────────┐                       │
│                  │  Tauri Listener  │                       │
│                  └────────┬─────────┘                       │
│                           │                                 │
│                  ┌────────▼─────────┐                       │
│                  │  transferStore   │                       │
│                  │ (Svelte Writable)│                       │
│                  └────────┬─────────┘                       │
│                           │                                 │
│         ┌─────────────────┼─────────────────┐               │
│         │                 │                 │               │
│    ┌────▼────┐     ┌─────▼──────┐    ┌────▼─────┐           │
│    │Download │     │  Progress  │    │Analytics │           │
│    │  Page   │     │    Bar     │    │   Page   │           │ 
│    └─────────┘     └────────────┘    └──────────┘           │
└─────────────────────────────────────────────────────────────┘
```

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
    Relay,
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
    Network,
    Protocol,
    Filesystem,
    Verification,
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

```typescript
// App.svelte or +layout.svelte
import { onMount } from 'svelte';
import { subscribeToTransferEvents } from '$lib/stores/transferEventsStore';

onMount(async () => {
  // Subscribe to all transfer events
  const unsubscribe = await subscribeToTransferEvents();
  
  // Cleanup on component destroy
  return unsubscribe;
});
```

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
2. **Emit events in order**: Follow the lifecycle: queued → started → progress... → completed/failed
3. **Include context**: Always populate relevant fields (file_hash, file_name, etc.)
4. **Progress updates**: Emit progress every 1-2 seconds, not every chunk
5. **Speed updates**: Can emit more frequently (every 100-500ms) for smooth UI
6. **Error handling**: Always emit failed event with descriptive error messages
7. **Cleanup**: Emit canceled event when user cancels, not just silence

### Frontend

1. **Subscribe once**: Subscribe to events in App.svelte, not in every component
2. **Use derived stores**: Filter transfers using derived stores for better performance
3. **Cleanup**: Always return unsubscribe function from onMount
4. **Handle missing transfers**: Check if transfer exists before accessing properties
5. **Use reactive statements**: Leverage Svelte's reactivity (`$:` syntax)
6. **Show user feedback**: Display errors, completion notifications, etc.

## Configuration

### Default Values

The system uses sensible defaults that can be adjusted per use case:

| Setting | Default | Description |
|---------|---------|-------------|
| Progress emit interval | 1-2 seconds | How often to emit progress events |
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
1. Check that `subscribeToTransferEvents()` was called in App.svelte
2. Verify backend is emitting events (check Rust logs)
3. Check browser console for subscription errors
4. Ensure Tauri event system is working

### Transfers not updating

**Symptoms**: Transfer state becomes stale or doesn't reflect backend changes.

**Solutions**:
1. Verify transfer_id matches between events
2. Check that progress events include all required fields
3. Look for TypeScript errors in browser console
4. Verify reactive statements are properly structured

### Performance issues

**Symptoms**: UI becomes laggy with many active transfers.

**Solutions**:
1. Reduce frequency of progress/speed updates in backend
2. Use derived stores instead of filtering in templates
3. Implement virtual scrolling for large transfer lists
4. Debounce UI updates if needed