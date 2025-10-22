# Relay Error Protocol

## Overview

The Relay Error Protocol is a comprehensive error handling and failover system for Circuit Relay v2 connections in Chiral Network. It provides intelligent retry logic, automatic relay pool management, health scoring, and multi-relay failover capabilities.

## Architecture

### Key Components

1. **RelayErrorService** (`src/lib/services/relayErrorService.ts`)
   - Central service managing relay connections and error handling
   - Implements exponential backoff retry strategy
   - Maintains relay health scores and connection state
   - Provides automatic failover to backup relays

2. **RelayErrorMonitor** (`src/lib/components/RelayErrorMonitor.svelte`)
   - UI component displaying relay pool status
   - Real-time health monitoring and error visualization
   - Statistics dashboard for relay performance

3. **DhtHealth Interface** (`src/lib/dht.ts`)
   - Extended with relay error tracking fields
   - Provides metrics for relay connections and failures

## Core Features

### 1. Relay Pool Management

The service maintains a pool of relay nodes with the following properties:

```typescript
interface RelayNode {
  id: string;                       // Peer ID
  multiaddr: string;                // Full multiaddr
  state: RelayConnectionState;      // Current connection state
  healthScore: number;              // 0-100 health score
  lastAttempt: number | null;       // Last connection attempt
  lastSuccess: number | null;       // Last successful connection
  consecutiveFailures: number;      // Consecutive failure count
  totalAttempts: number;            // Total attempts
  totalSuccesses: number;           // Total successes
  avgLatency: number;               // Average latency in ms
  reservationExpiry: number | null; // Reservation expiry timestamp
  isPrimary: boolean;               // Whether this is a preferred relay
  errors: RelayError[];             // Recent error history
}
```

### 2. Error Classification

The protocol categorizes errors into specific types for better diagnosis:

- **CONNECTION_REFUSED**: Relay actively refused the connection
- **CONNECTION_TIMEOUT**: Relay didn't respond within timeout
- **RESERVATION_FAILED**: Failed to reserve relay slot
- **RESERVATION_EXPIRED**: Relay reservation expired
- **RELAY_OVERLOADED**: Relay at capacity
- **RELAY_UNREACHABLE**: Can't reach relay at all
- **NETWORK_ERROR**: General network issues
- **AUTHENTICATION_FAILED**: Authentication problems
- **PROTOCOL_ERROR**: Protocol-level errors
- **UNKNOWN**: Unclassified errors

### 3. Connection States

Relays transition through various states during the connection lifecycle:

```
IDLE → CONNECTING → [CONNECTED | RESERVING → RESERVED]
                ↓
           RETRYING ← (on failure)
                ↓
           [FALLBACK | FAILED]
```

### 4. Exponential Backoff Retry

When a connection fails, the service implements exponential backoff:

1. Initial delay: 1000ms (configurable)
2. Each retry: `delay = min(delay * 2, maxRetryDelay)`
3. Max retries: 3 (configurable)
4. Max delay: 30000ms (30 seconds, configurable)

Example retry sequence:
- Attempt 1: immediate
- Attempt 2: wait 1s
- Attempt 3: wait 2s
- Attempt 4: wait 4s
- Give up after 4 attempts

### 5. Health Scoring

Each relay maintains a health score (0-100) that affects selection priority:

**Score Increases:**
- Successful connection: +10 points (max 100)

**Score Decreases:**
- Failed connection: -15 points (configurable)
- Multiple consecutive failures compound the penalty

**Minimum Threshold:**
- Relays below 20 health score are not attempted (configurable)

### 6. Relay Selection Algorithm

When selecting a relay, the service prioritizes:

1. **Primary relays** (from preferred list)
2. **Recently successful** relays (within 1 minute)
3. **Highest health score**

### 7. Automatic Failover

If connection to a relay fails after all retries:

1. Mark relay as FAILED
2. Reduce health score
3. Log error details
4. Attempt connection to next best relay
5. Mark new relay as FALLBACK
6. Update active relay if successful

## Usage

### Initialization

```typescript
import { relayErrorService } from '$lib/services/relayErrorService';

// Initialize with preferred relays
await relayErrorService.initialize(
  ['/ip4/relay.example.com/tcp/4001/p2p/QmRelayId'],
  true // enable auto-discovery
);
```

### Connect to Relay

```typescript
// Connect to best available relay
const result = await relayErrorService.connectToRelay();

if (result.success) {
  console.log('Connected to relay:', result.relayId);
  console.log('Latency:', result.latency, 'ms');
} else {
  console.error('Connection failed:', result.error);
}

// Connect to specific relay
const result = await relayErrorService.connectToRelay('QmSpecificRelayId');
```

### Monitor Relay Health

```typescript
// Subscribe to relay pool
relayErrorService.relayPool.subscribe(pool => {
  console.log('Relay pool size:', pool.size);
});

// Subscribe to active relay
relayErrorService.activeRelay.subscribe(relay => {
  if (relay) {
    console.log('Active relay:', relay.id);
    console.log('Health score:', relay.healthScore);
  }
});

// Subscribe to errors
relayErrorService.errorLog.subscribe(errors => {
  console.log('Recent errors:', errors.length);
});
```

### Reservation Management

The service automatically monitors and renews reservations:

```typescript
// Manual reservation monitoring (automatic in background)
await relayErrorService.monitorReservations();
```

Reservations are automatically renewed when:
- Less than 5 minutes (configurable) remain
- Relay is still active and healthy

## Configuration

Default configuration can be customized:

```typescript
const customConfig = {
  maxRetries: 5,                    // Max retry attempts
  initialRetryDelay: 2000,          // Initial delay (2s)
  maxRetryDelay: 60000,             // Max delay (60s)
  backoffMultiplier: 2,             // Exponential multiplier
  reservationRenewalThreshold: 600, // Renew at 10 min remaining
  healthScoreDecay: 10,             // Health reduction per failure
  errorHistoryLimit: 20,            // Max errors to track
  connectionTimeout: 15000,         // Connection timeout (15s)
  autoDiscoverRelays: true,         // Auto-discover via DHT
  minHealthScore: 30                // Min score to attempt connection
};

const service = RelayErrorService.getInstance(customConfig);
```

## UI Integration

### Relay Page

The Relay page (`src/pages/Relay.svelte`) integrates the error protocol:

1. Initializes service on mount
2. Displays RelayErrorMonitor component
3. Shows real-time relay health and statistics
4. Provides error log visualization

### RelayErrorMonitor Component

Features:
- **Statistics Dashboard**: Total relays, connected relays, healthy relays, error count
- **Active Relay Card**: Current relay details with health score and metrics
- **Relay Pool List**: All relays with state, health, and error history
- **Recent Errors**: Categorized error log with timestamps

## Error Flow Example

### Scenario: Node A wants to connect to Node B, both behind NAT

1. **Discovery**: A and B find relay C via DHT
2. **Connection Attempt**:
   ```
   A → C: Connect request
   C → A: Connection refused (overloaded)
   ```
3. **Error Handling**:
   - Categorize error as RELAY_OVERLOADED
   - Log error with timestamp
   - Reduce C's health score (-15)
   - Wait 1s (backoff)
4. **Retry 1**:
   ```
   A → C: Connect request
   C → A: Timeout
   ```
   - Categorize as CONNECTION_TIMEOUT
   - Reduce health score (-15, now 70)
   - Wait 2s
5. **Retry 2**:
   ```
   A → C: Connect request
   C → A: Timeout
   ```
   - Health score now 55
   - Wait 4s
6. **Fallback**:
   - Mark C as FAILED
   - Select relay D (second best)
   - Attempt connection:
     ```
     A → D: Connect request
     D → A: Connection accepted
     D ← A: Reservation request
     D → A: Reservation granted (expires in 1h)
     ```
7. **Success**:
   - Mark D as RESERVED
   - Increase D's health score (+10)
   - Set D as active relay
   - Use D for NAT traversal with B

### Scenario: Reservation Expiry

1. **Background Monitoring** (every 30s):
   - Check relay D reservation expiry
   - 4 minutes remaining
2. **Renewal Trigger**:
   - Less than 5 minutes remaining
   - Attempt renewal:
     ```
     A → D: Renew reservation
     D → A: Renewal granted (new expiry: +1h)
     ```
3. **Success**:
   - Update relay D's expiry timestamp
   - Log renewal success

## Best Practices

1. **Configure Multiple Preferred Relays**: Always provide 3-5 preferred relays for redundancy
2. **Enable Auto-Discovery**: Let the service discover additional relays via DHT
3. **Monitor Health Scores**: Regularly check relay pool health via UI or stores
4. **Clear Error Logs**: Periodically clear error logs to prevent memory growth
5. **Adjust Configuration**: Tune retry delays and health decay based on network conditions

## Backend Integration

The relay error protocol works with the Rust backend via Tauri commands:

### Expected Backend Commands

```rust
// Connect to peer/relay
#[tauri::command]
async fn connect_to_peer(peer_address: String) -> Result<(), String>

// Get DHT peer ID
#[tauri::command]
async fn get_dht_peer_id() -> Result<Option<String>, String>

// Start DHT with relay config
#[tauri::command]
async fn start_dht_node(
    port: u16,
    bootstrap_nodes: Vec<String>,
    enable_autorelay: Option<bool>,
    preferred_relays: Option<Vec<String>>,
    enable_relay_server: Option<bool>,
) -> Result<String, String>

// Get DHT health (includes relay metrics)
#[tauri::command]
async fn get_dht_health() -> Result<DhtHealth, String>
```

### Backend Responsibilities

The Rust backend should:

1. Implement Circuit Relay v2 protocol
2. Track relay reservations and expiry
3. Report relay metrics to DhtHealth
4. Handle reservation renewals
5. Emit events for relay state changes

## Testing

### Unit Tests

Test individual components:

```typescript
// Test relay selection
const relay = relayErrorService.selectBestRelay();
assert(relay.isPrimary || relay.healthScore >= 80);

// Test error categorization
const error = relayErrorService.categorizeError(
  new Error('Connection timeout')
);
assert(error === RelayErrorType.CONNECTION_TIMEOUT);
```

### Integration Tests

Test complete flow:

```typescript
// Test retry with fallback
await relayErrorService.initialize([relayA, relayB]);
const result = await relayErrorService.connectToRelay();
assert(result.success);
assert(result.relayId === relayA || result.relayId === relayB);
```

## Troubleshooting

### Problem: All relays failing

**Symptoms:**
- activeRelay is null
- All relays have low health scores
- Error log shows repeated timeouts

**Solutions:**
1. Check network connectivity
2. Verify relay multiaddrs are correct
3. Check firewall settings
4. Enable auto-discovery
5. Try different bootstrap nodes

### Problem: Reservations expiring frequently

**Symptoms:**
- lastReservationFailure increasing
- reservationExpiry shows short times

**Solutions:**
1. Reduce reservationRenewalThreshold
2. Check relay server stability
3. Verify network isn't dropping connections
4. Check backend reservation logic

### Problem: High retry count

**Symptoms:**
- Many errors with high retryCount
- Connection taking long time

**Solutions:**
1. Increase maxRetries
2. Adjust backoff timing
3. Add more preferred relays
4. Check relay server capacity

## Future Enhancements

1. **Relay Reputation System**: Integrate with existing reputation system
2. **Geographic Optimization**: Prefer relays closer to user
3. **Bandwidth Tracking**: Monitor relay bandwidth usage
4. **Relay Discovery Protocol**: Implement DHT-based relay discovery
5. **Advanced Analytics**: ML-based relay quality prediction
6. **Relay Server Load Balancing**: Smart distribution across multiple relays
7. **WebRTC DataChannel Optimization**: Use relay metrics to optimize transfers

## References

- [libp2p Circuit Relay v2 Spec](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md)
- [AutoRelay Spec](https://github.com/libp2p/specs/blob/master/relay/autorelay.md)
- [DCUtR Spec](https://github.com/libp2p/specs/blob/master/relay/dcutr.md)
- [NAT Traversal in libp2p](https://blog.libp2p.io/2022-12-19-libp2p-hole-punching/)

## License

Part of Chiral Network - MIT License
