# Payment Checkpoint System - Integration Guide

## Overview

The Payment Checkpoint System implements incremental payments during file downloads, following the spec in `docs/file-sharing.md` (lines 333-393).

**Key Features:**
- ✅ Incremental payment checkpoints (10 MB → 20 MB → 40 MB exponential scaling)
- ✅ Pause serving when payment checkpoint reached
- ✅ Resume download after payment confirmed
- ✅ Support for both "exponential" and "upfront" payment modes
- ✅ Full transaction tracking and history
- ✅ Frontend UI for payment confirmation

## Architecture

### Backend (Rust)

**Files Created:**
- `src-tauri/src/payment_checkpoint.rs` - Core payment checkpoint service
- Commands added to `src-tauri/src/main.rs`

**Key Commands:**
```rust
init_payment_checkpoint(session_id, file_hash, file_size, seeder_address, seeder_peer_id, price_per_mb, payment_mode)
update_payment_checkpoint_progress(session_id, bytes_transferred) -> CheckpointState
record_checkpoint_payment(session_id, transaction_hash, amount_paid)
check_should_pause_serving(session_id) -> bool
get_payment_checkpoint_info(session_id) -> PaymentCheckpointInfo
mark_checkpoint_completed(session_id)
remove_payment_checkpoint_session(session_id)
```

### Frontend (TypeScript/Svelte)

**Files Created:**
- `src/lib/services/paymentCheckpointService.ts` - Frontend service
- `src/lib/components/download/PaymentCheckpointModal.svelte` - Payment UI

## Integration Steps

### Step 1: Initialize Checkpoint Session (Download Start)

When a download begins, initialize a payment checkpoint session:

```typescript
import { paymentCheckpointService } from '$lib/services/paymentCheckpointService';

// When download starts
const sessionId = `download-${fileHash}-${Date.now()}`;

await paymentCheckpointService.initCheckpointSession(
  sessionId,
  fileHash,
  fileSize,
  seederWalletAddress,
  seederPeerId,
  0.001, // Price per MB in Chiral
  'exponential' // or 'upfront'
);
```

### Step 2: Update Progress During Download

As bytes are transferred, update the checkpoint progress:

```typescript
// Inside your download loop or progress callback
const state = await paymentCheckpointService.updateProgress(
  sessionId,
  bytesTransferred
);

// If state is 'waiting_for_payment', the backend has emitted a 'payment_checkpoint_reached' event
// Your UI will handle this via event listener
```

### Step 3: Listen for Checkpoint Events

Set up event listeners when the download page loads:

```typescript
import { onMount } from 'svelte';
import type { PaymentCheckpointEvent } from '$lib/services/paymentCheckpointService';

let checkpointEvent: PaymentCheckpointEvent | null = null;
let showPaymentModal = false;

onMount(async () => {
  // Listen for payment checkpoints
  const unlistenCheckpoint = await paymentCheckpointService.listenToCheckpoints(
    (event) => {
      checkpointEvent = event;
      showPaymentModal = true;

      // Pause the download (implementation depends on your download service)
      pauseDownload(event.sessionId);
    }
  );

  // Listen for payment confirmations
  const unlistenPaid = await paymentCheckpointService.listenToPayments(
    (event) => {
      // Resume the download
      resumeDownload(event.sessionId);
      showPaymentModal = false;
    }
  );

  // Cleanup on unmount
  return () => {
    unlistenCheckpoint();
    unlistenPaid();
  };
});
```

### Step 4: Add Payment Modal to Your UI

```svelte
<script>
  import PaymentCheckpointModal from '$lib/components/download/PaymentCheckpointModal.svelte';
  import { paymentCheckpointService } from '$lib/services/paymentCheckpointService';

  let checkpointEvent: PaymentCheckpointEvent | null = null;
  let showPaymentModal = false;
  let currentFileName = '';

  async function handlePayment(event: CustomEvent<{ transactionHash: string; amount: number }>) {
    const { transactionHash, amount } = event.detail;

    if (!checkpointEvent) return;

    // Record the payment in backend
    await paymentCheckpointService.recordPayment(
      checkpointEvent.sessionId,
      transactionHash,
      amount
    );

    // The 'payment_checkpoint_paid' event will be emitted automatically
    // Your listener will resume the download
  }

  function handleCancelDownload() {
    if (checkpointEvent) {
      // Cancel the download
      cancelDownload(checkpointEvent.sessionId);

      // Clean up checkpoint session
      paymentCheckpointService.removeSession(checkpointEvent.sessionId);
    }

    showPaymentModal = false;
  }
</script>

<PaymentCheckpointModal
  bind:show={showPaymentModal}
  bind:checkpointEvent
  fileName={currentFileName}
  on:pay={handlePayment}
  on:cancel={handleCancelDownload}
  on:close={() => showPaymentModal = false}
/>
```

### Step 5: Integrate with WebRTC/HTTP Download Services

The download services need to check for payment checkpoints and pause when required.

**WebRTC Example:**

```rust
// In src-tauri/src/webrtc_service.rs or similar

// Before sending each chunk
let should_pause = check_should_pause_serving(&session_id).await?;
if should_pause {
    // Wait for payment
    info!("Pausing WebRTC transfer: waiting for payment checkpoint");

    // Poll until payment is received (state changes from WaitingForPayment)
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let should_pause = check_should_pause_serving(&session_id).await?;
        if !should_pause {
            info!("Payment received, resuming WebRTC transfer");
            break;
        }
    }
}

// Continue sending chunks
send_chunk(data).await?;
```

**HTTP Example:**

```rust
// In src-tauri/src/http_download.rs or HTTP server

// Before serving next chunk
let should_pause = state.payment_checkpoint.should_pause_serving(&session_id).await?;
if should_pause {
    // Return 402 Payment Required
    return Ok(Response::builder()
        .status(402)
        .header("X-Payment-Required", "true")
        .header("X-Checkpoint-Session", &session_id)
        .body("Payment required to continue".into())?);
}

// Serve the chunk
Ok(Response::new(chunk_data.into()))
```

## Payment Flow Diagram

```
┌─────────────┐
│ Download    │
│ Starts      │
└──────┬──────┘
       │
       ▼
┌─────────────────────────┐
│ Init Checkpoint Session │
│ - 10 MB first checkpoint│
└──────┬──────────────────┘
       │
       ▼
┌──────────────────┐
│ Transfer Chunks  │ ◄──────┐
└──────┬───────────┘        │
       │                    │
       ▼                    │
┌──────────────────────┐    │
│ Update Progress      │    │
│ Backend checks if    │    │
│ checkpoint reached   │    │
└──────┬───────────────┘    │
       │                    │
       ├─ No ──────────────┘
       │
       │ Yes (10 MB reached)
       ▼
┌───────────────────────────┐
│ Emit Event:               │
│ payment_checkpoint_reached│
└──────┬────────────────────┘
       │
       ▼
┌──────────────────┐
│ Frontend Shows   │
│ Payment Modal    │
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│ User Pays        │
│ 0.01 Chiral      │
└──────┬───────────┘
       │
       ▼
┌──────────────────────┐
│ Record Payment       │
│ - Transaction hash   │
│ - Next checkpoint:   │
│   30 MB (10+20)      │
└──────┬───────────────┘
       │
       ▼
┌───────────────────────┐
│ Emit Event:           │
│ payment_checkpoint_paid│
└──────┬────────────────┘
       │
       ▼
┌──────────────────┐
│ Resume Download  │ ◄──┐
└──────┬───────────┘    │
       │                │
       └────────────────┘
       (Loop until file complete)
```

## Exponential Scaling Example

For a 100 MB file with price 0.001 Chiral/MB:

| Checkpoint | Bytes Transferred | Amount to Pay | Cumulative Paid | Remaining |
|------------|-------------------|---------------|-----------------|-----------|
| 1st        | 10 MB             | 0.01 Chiral   | 0.01            | 0.09      |
| 2nd        | 30 MB (+20 MB)    | 0.02 Chiral   | 0.03            | 0.07      |
| 3rd        | 70 MB (+40 MB)    | 0.04 Chiral   | 0.07            | 0.03      |
| Complete   | 100 MB (+30 MB)   | 0.03 Chiral   | 0.10            | 0.00      |

**Total: 0.10 Chiral (100 MB × 0.001 Chiral/MB)**

## Testing

### Manual Testing

1. **Start a download:**
   ```bash
   # In Tauri dev mode
   npm run tauri:dev
   ```

2. **Trigger a checkpoint:**
   - Download a file larger than 10 MB
   - Watch console for: `💰 Payment checkpoint reached`

3. **Verify payment modal appears**

4. **Pay and confirm download resumes**

### Automated Testing

```typescript
// tests/paymentCheckpoint.test.ts
import { describe, it, expect } from 'vitest';
import { paymentCheckpointService } from '$lib/services/paymentCheckpointService';

describe('Payment Checkpoint Service', () => {
  it('should initialize checkpoint session', async () => {
    await paymentCheckpointService.initCheckpointSession(
      'test-session',
      'test-hash',
      100 * 1024 * 1024,
      '0x123',
      'peer-123',
      0.001,
      'exponential'
    );

    const info = await paymentCheckpointService.getCheckpointInfo('test-session');
    expect(info.session_id).toBe('test-session');
  });

  it('should detect checkpoint at 10 MB', async () => {
    const state = await paymentCheckpointService.updateProgress(
      'test-session',
      10 * 1024 * 1024
    );

    expect(state).toBe('waiting_for_payment');
  });
});
```

## Configuration

### Payment Modes

**Exponential (Recommended):**
- Builds trust incrementally
- Lower risk for downloader
- Checkpoints: 10 → 20 → 40 → 80 MB
- Default mode

**Upfront:**
- Pay for entire file before download
- For trusted seeders only
- No intermediate checkpoints

### Checkpoint Parameters

Modify in `src-tauri/src/payment_checkpoint.rs`:

```rust
const INITIAL_CHECKPOINT_MB: u64 = 10; // First checkpoint at 10 MB
const MIN_CHECKPOINT_MB: u64 = 1;      // Minimum checkpoint size
```

## Troubleshooting

### Payment checkpoint not triggering

**Check:**
1. Checkpoint session initialized before download starts
2. Progress updates being called with correct session ID
3. Event listeners registered properly

**Debug:**
```typescript
const info = await paymentCheckpointService.getCheckpointInfo(sessionId);
console.log('Checkpoint info:', info);
```

### Download not resuming after payment

**Check:**
1. Payment recorded successfully (check transaction hash)
2. `payment_checkpoint_paid` event listener attached
3. Download service polling `check_should_pause_serving`

**Debug:**
```rust
let should_pause = check_should_pause_serving(&session_id).await?;
println!("Should pause: {}", should_pause);
```

### Balance issues

**Check:**
1. Wallet has sufficient balance
2. Price calculation correct (`fileSize * pricePerMB`)
3. Gas fees included in balance check

## Next Steps

### Phase 2: Multi-Source Support

Currently, checkpoints work for single-source downloads. To support multi-source:

1. Coordinate checkpoints across all sources
2. Pause all sources when checkpoint reached
3. Resume all sources after payment
4. Handle partial payments for different seeders

### Phase 3: Payment Channels

For frequent downloads, implement payment channels to reduce transaction fees:

1. Open payment channel with seeder
2. Make off-chain micropayments per chunk
3. Close channel when download completes

### Phase 4: Reputation Integration

Adjust checkpoint intervals based on seeder reputation:

- High reputation: Larger checkpoints or upfront payment
- Low reputation: Smaller checkpoints (5 MB instead of 10 MB)
- Untrusted: Pay-per-chunk (1 MB checkpoints)

## API Reference

### PaymentCheckpointService

#### Methods

##### `initCheckpointSession()`
Initialize a new payment checkpoint session.

**Parameters:**
- `sessionId: string` - Unique session identifier
- `fileHash: string` - File hash being downloaded
- `fileSize: number` - Total file size in bytes
- `seederAddress: string` - Seeder's wallet address
- `seederPeerId: string` - Seeder's peer ID
- `pricePerMb: number` - Price per MB in Chiral
- `paymentMode: 'exponential' | 'upfront'` - Payment mode

**Returns:** `Promise<void>`

##### `updateProgress()`
Update download progress and check for checkpoints.

**Parameters:**
- `sessionId: string` - Session identifier
- `bytesTransferred: number` - Total bytes transferred

**Returns:** `Promise<CheckpointState>`

##### `recordPayment()`
Record a payment for the current checkpoint.

**Parameters:**
- `sessionId: string` - Session identifier
- `transactionHash: string` - Blockchain transaction hash
- `amountPaid: number` - Amount paid in Chiral

**Returns:** `Promise<void>`

##### `getCheckpointInfo()`
Get full checkpoint information.

**Parameters:**
- `sessionId: string` - Session identifier

**Returns:** `Promise<PaymentCheckpointInfo>`

#### Events

##### `payment_checkpoint_reached`
Emitted when a payment checkpoint is reached.

**Payload:**
```typescript
{
  sessionId: string;
  fileHash: string;
  fileName: string;
  checkpointMb: number;
  amountChiral: number;
  bytesTransferred: number;
  seederAddress: string;
  seederPeerId: string;
}
```

##### `payment_checkpoint_paid`
Emitted when a checkpoint payment is confirmed.

**Payload:**
```typescript
{
  sessionId: string;
  transactionHash: string;
  amountPaid: number;
}
```

## Contributing

When adding new download protocols (BitTorrent, FTP, ED2K, etc.):

1. Initialize checkpoint session when download starts
2. Call `update_payment_checkpoint_progress` after each chunk
3. Check `check_should_pause_serving` before serving chunks
4. Handle pause/resume logic

## License

Part of the Chiral Network project.

---

**Status:** ✅ Phase 1 Complete - Core functionality implemented

**Last Updated:** December 2024
