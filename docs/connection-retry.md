# Connection Retry & Resilience Framework

## Overview

The Connection Retry & Resilience Framework provides unified retry infrastructure with exponential backoff for all connection-based operations across the Chiral Network. This module addresses the need for structured retry logic when WebRTC and DHT connections fail due to transient network issues.

**Location**: `src-tauri/src/connection_retry.rs`

## Why This Framework?

Prior to this framework, connection failures in WebRTC and DHT operations would fail silently without retry logic, leading to poor user experience when transient network issues occurred. This module provides:

- Structured retry with exponential backoff
- Jitter to prevent thundering herd problems
- Connection state tracking per peer
- Health monitoring with automatic recovery detection
- Pre-configured profiles optimized for different connection types

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Connection Retry Framework                    │
│                                                                  │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐     │
│  │  RetryConfig   │  │ConnectionTracker│  │ConnectionManager│    │
│  │  (per-type)    │  │  (per-peer)    │  │  (all peers)   │     │
│  └───────┬────────┘  └───────┬────────┘  └───────┬────────┘     │
│          │                    │                   │              │
│          └────────────────────┴───────────────────┘              │
│                               │                                  │
│                    ┌──────────▼──────────┐                       │
│                    │  DhtHealthMonitor   │                       │
│                    │  (network health)   │                       │
│                    └──────────┬──────────┘                       │
│                               │                                  │
│          ┌────────────────────┼────────────────────┐             │
│          │                    │                    │             │
│   ┌──────▼──────┐     ┌──────▼──────┐     ┌──────▼──────┐       │
│   │   WebRTC    │     │     DHT     │     │     DHT     │       │
│   │ Connections │     │  Bootstrap  │     │    Peers    │       │
│   └─────────────┘     └─────────────┘     └─────────────┘       │
└─────────────────────────────────────────────────────────────────┘
```

## Core Components

### RetryConfig

Configuration for retry behavior with exponential backoff.

```rust
pub struct RetryConfig {
    /// Maximum number of retry attempts (0 = infinite)
    pub max_attempts: u32,
    /// Initial delay between retries in milliseconds
    pub initial_delay_ms: u64,
    /// Maximum delay between retries in milliseconds
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Jitter factor to randomize delays (0.0 to 1.0)
    pub jitter_factor: f64,
    /// Whether to reset retry count on success
    pub reset_on_success: bool,
}
```

**Default Constants:**

| Constant | Value | Description |
|----------|-------|-------------|
| `DEFAULT_MAX_RETRIES` | 5 | Maximum retry attempts |
| `DEFAULT_INITIAL_DELAY_MS` | 500 | Initial delay (0.5s) |
| `DEFAULT_MAX_DELAY_MS` | 30,000 | Maximum delay (30s) |
| `DEFAULT_BACKOFF_MULTIPLIER` | 2.0 | Exponential factor |
| `DEFAULT_JITTER_FACTOR` | 0.1 | 10% randomization |

### Pre-configured Profiles

The framework provides optimized configurations for different connection types:

#### WebRTC Connections

```rust
RetryConfig::for_webrtc()
// max_attempts: 3
// initial_delay_ms: 1000
// max_delay_ms: 15,000
// backoff_multiplier: 2.0
// jitter_factor: 0.2
```

Optimized for WebRTC peer connections with shorter timeouts since WebRTC has its own ICE retry mechanisms.

#### DHT Bootstrap

```rust
RetryConfig::for_dht_bootstrap()
// max_attempts: 5
// initial_delay_ms: 2000
// max_delay_ms: 60,000
// backoff_multiplier: 2.0
// jitter_factor: 0.15
```

More aggressive retries for bootstrap nodes since they're critical for network connectivity.

#### DHT Peer Connections

```rust
RetryConfig::for_dht_peer()
// max_attempts: 3
// initial_delay_ms: 1000
// max_delay_ms: 30,000
// backoff_multiplier: 2.0
// jitter_factor: 0.1
```

Balanced configuration for regular DHT peer connections.

### ConnectionTracker

Per-connection state machine that tracks retry attempts and connection health.

```rust
pub struct ConnectionTracker {
    pub connection_id: String,
    pub state: ConnectionState,
    pub attempt_count: u32,
    pub consecutive_failures: u32,
    pub last_attempt: Option<Instant>,
    pub last_success: Option<Instant>,
    pub last_error: Option<String>,
    pub config: RetryConfig,
}

pub enum ConnectionState {
    Idle,
    Connecting,
    Connected,
    Retrying,
    Failed,
    Exhausted,  // All retries used
}
```

**Key Methods:**

```rust
impl ConnectionTracker {
    /// Create a new tracker with the given config
    pub fn new(connection_id: String, config: RetryConfig) -> Self;
    
    /// Record a successful connection
    pub fn record_success(&mut self);
    
    /// Record a failed connection attempt
    pub fn record_failure(&mut self, error: String);
    
    /// Check if we should retry
    pub fn should_retry(&self) -> bool;
    
    /// Get the delay before next retry (with jitter)
    pub fn get_retry_delay(&self) -> Duration;
    
    /// Reset the tracker state
    pub fn reset(&mut self);
}
```

### ConnectionManager

Thread-safe management of multiple connection trackers.

```rust
pub struct ConnectionManager {
    trackers: Arc<RwLock<HashMap<String, ConnectionTracker>>>,
    default_config: RetryConfig,
}

impl ConnectionManager {
    /// Create a new manager with default config
    pub fn new() -> Self;
    
    /// Create with custom default config
    pub fn with_config(config: RetryConfig) -> Self;
    
    /// Get or create a tracker for a connection
    pub async fn get_or_create(&self, connection_id: &str) -> ConnectionTracker;
    
    /// Get tracker with specific config
    pub async fn get_or_create_with_config(
        &self, 
        connection_id: &str, 
        config: RetryConfig
    ) -> ConnectionTracker;
    
    /// Update tracker state
    pub async fn update(&self, tracker: ConnectionTracker);
    
    /// Remove a tracker
    pub async fn remove(&self, connection_id: &str);
    
    /// Get all trackers in a specific state
    pub async fn get_by_state(&self, state: ConnectionState) -> Vec<ConnectionTracker>;
    
    /// Get statistics
    pub async fn get_stats(&self) -> ConnectionStats;
}
```

### DhtHealthMonitor

Health monitoring for DHT network connectivity with recovery threshold detection.

```rust
pub struct DhtHealthMonitor {
    manager: Arc<ConnectionManager>,
    min_peers_threshold: usize,
    health_check_interval: Duration,
    is_healthy: Arc<RwLock<bool>>,
    last_health_check: Arc<RwLock<Option<Instant>>>,
}

impl DhtHealthMonitor {
    /// Create a new health monitor
    pub fn new(manager: Arc<ConnectionManager>) -> Self;
    
    /// Create with custom thresholds
    pub fn with_thresholds(
        manager: Arc<ConnectionManager>,
        min_peers: usize,
        check_interval: Duration,
    ) -> Self;
    
    /// Check current health status
    pub async fn check_health(&self) -> HealthReport;
    
    /// Get whether network is considered healthy
    pub async fn is_healthy(&self) -> bool;
    
    /// Start background health monitoring
    pub fn start_monitoring(&self) -> tokio::task::JoinHandle<()>;
}

pub struct HealthReport {
    pub is_healthy: bool,
    pub connected_peers: usize,
    pub min_peers_threshold: usize,
    pub failed_connections: usize,
    pub recovering_connections: usize,
    pub timestamp: u64,
}
```

## Async Retry Helpers

The framework provides async helper functions for wrapping operations with retry logic:

### Basic Retry

```rust
/// Execute an async operation with retry logic
pub async fn with_retry<F, Fut, T, E>(
    config: &RetryConfig,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
```

**Usage:**

```rust
let config = RetryConfig::for_webrtc();

let result = with_retry(&config, || async {
    establish_webrtc_connection(peer_id).await
}).await;
```

### Tracked Retry

```rust
/// Execute with retry and update connection tracker
pub async fn with_retry_tracked<F, Fut, T, E>(
    tracker: &mut ConnectionTracker,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
```

**Usage:**

```rust
let mut tracker = manager.get_or_create("peer-123").await;

let result = with_retry_tracked(&mut tracker, || async {
    connect_to_peer("peer-123").await
}).await;

manager.update(tracker).await;
```

## Delay Calculation

The retry delay is calculated using exponential backoff with jitter:

```rust
fn calculate_delay(&self) -> Duration {
    let base_delay = self.config.initial_delay_ms as f64 
        * self.config.backoff_multiplier.powi(self.attempt_count as i32);
    
    let capped_delay = base_delay.min(self.config.max_delay_ms as f64);
    
    // Apply jitter: delay * (1 - jitter_factor/2 + random * jitter_factor)
    let jitter_range = capped_delay * self.config.jitter_factor;
    let jitter = (random::<f64>() - 0.5) * jitter_range;
    
    Duration::from_millis((capped_delay + jitter) as u64)
}
```

**Example delay progression (default config):**

| Attempt | Base Delay | With 10% Jitter |
|---------|------------|-----------------|
| 1 | 500ms | 475-525ms |
| 2 | 1,000ms | 950-1,050ms |
| 3 | 2,000ms | 1,900-2,100ms |
| 4 | 4,000ms | 3,800-4,200ms |
| 5 | 8,000ms | 7,600-8,400ms |

## Integration Examples

### WebRTC Connection with Retry

```rust
use crate::connection_retry::{ConnectionManager, RetryConfig, with_retry_tracked};

async fn establish_peer_connection(
    manager: &ConnectionManager,
    peer_id: &str,
) -> Result<PeerConnection, Error> {
    let config = RetryConfig::for_webrtc();
    let mut tracker = manager.get_or_create_with_config(peer_id, config).await;
    
    let result = with_retry_tracked(&mut tracker, || async {
        // Your WebRTC connection logic
        create_rtc_connection(peer_id).await
    }).await;
    
    manager.update(tracker).await;
    result
}
```

### DHT Bootstrap with Health Monitoring

```rust
use crate::connection_retry::{ConnectionManager, DhtHealthMonitor, RetryConfig};

async fn bootstrap_dht(
    bootstrap_nodes: &[String],
) -> Result<(), Error> {
    let manager = Arc::new(ConnectionManager::new());
    let monitor = DhtHealthMonitor::new(manager.clone());
    
    // Start background health monitoring
    let _health_task = monitor.start_monitoring();
    
    for node in bootstrap_nodes {
        let config = RetryConfig::for_dht_bootstrap();
        let mut tracker = manager.get_or_create_with_config(node, config).await;
        
        match with_retry_tracked(&mut tracker, || async {
            connect_to_bootstrap(node).await
        }).await {
            Ok(_) => info!("Connected to bootstrap node: {}", node),
            Err(e) => warn!("Failed to connect to {}: {}", node, e),
        }
        
        manager.update(tracker).await;
    }
    
    // Check overall health
    let health = monitor.check_health().await;
    if !health.is_healthy {
        warn!("DHT bootstrap incomplete: only {} peers connected", 
              health.connected_peers);
    }
    
    Ok(())
}
```

### Infinite Retry Mode

For critical connections that must eventually succeed:

```rust
let config = RetryConfig {
    max_attempts: 0,  // 0 = infinite retries
    initial_delay_ms: 1000,
    max_delay_ms: 60_000,
    ..Default::default()
};

// This will retry forever until success
let result = with_retry(&config, || async {
    critical_connection().await
}).await;
```

## Testing

The module includes comprehensive unit tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delay_calculation_exponential_growth() {
        let config = RetryConfig::default();
        let mut tracker = ConnectionTracker::new("test".to_string(), config);
        
        let delays: Vec<_> = (0..5).map(|_| {
            tracker.record_failure("test".to_string());
            tracker.get_retry_delay()
        }).collect();
        
        // Verify exponential growth (accounting for jitter)
        for i in 1..delays.len() {
            assert!(delays[i] > delays[i-1]);
        }
    }

    #[test]
    fn test_max_attempts_enforcement() {
        let config = RetryConfig {
            max_attempts: 3,
            ..Default::default()
        };
        let mut tracker = ConnectionTracker::new("test".to_string(), config);
        
        for _ in 0..3 {
            tracker.record_failure("test".to_string());
        }
        
        assert!(!tracker.should_retry());
        assert_eq!(tracker.state, ConnectionState::Exhausted);
    }

    #[test]
    fn test_infinite_retry_mode() {
        let config = RetryConfig {
            max_attempts: 0,  // infinite
            ..Default::default()
        };
        let mut tracker = ConnectionTracker::new("test".to_string(), config);
        
        for _ in 0..100 {
            tracker.record_failure("test".to_string());
            assert!(tracker.should_retry());
        }
    }

    #[test]
    fn test_success_resets_state() {
        let config = RetryConfig::default();
        let mut tracker = ConnectionTracker::new("test".to_string(), config);
        
        tracker.record_failure("error".to_string());
        tracker.record_failure("error".to_string());
        assert_eq!(tracker.consecutive_failures, 2);
        
        tracker.record_success();
        assert_eq!(tracker.consecutive_failures, 0);
        assert_eq!(tracker.state, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn test_retry_helper() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 10,  // Fast for tests
            ..Default::default()
        };
        
        let attempt_count = Arc::new(AtomicU32::new(0));
        let count = attempt_count.clone();
        
        let result = with_retry(&config, || {
            let c = count.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err("not yet")
                } else {
                    Ok("success")
                }
            }
        }).await;
        
        assert_eq!(result, Ok("success"));
        assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
    }
}
```

## Metrics & Observability

The framework provides statistics for monitoring:

```rust
pub struct ConnectionStats {
    pub total_connections: usize,
    pub connected: usize,
    pub connecting: usize,
    pub retrying: usize,
    pub failed: usize,
    pub exhausted: usize,
    pub total_attempts: u64,
    pub total_failures: u64,
    pub success_rate: f64,
}

// Get current stats
let stats = manager.get_stats().await;
info!("Connection stats: {} connected, {} retrying, {:.1}% success rate",
      stats.connected, stats.retrying, stats.success_rate * 100.0);
```

## Future Enhancements

Planned improvements for the framework:

1. **Circuit Breaker Pattern**: Automatically stop retrying when a service is clearly down
2. **Adaptive Backoff**: Adjust delays based on observed network conditions  
3. **Priority Queuing**: Prioritize critical connections over less important ones
4. **Metrics Export**: Prometheus/OpenTelemetry integration for observability
5. **Per-Peer Rate Limiting**: Prevent overwhelming specific peers with retry attempts

## Related Documentation

- [NAT Traversal](nat-traversal.md) - Network address translation and connectivity
- [WebRTC](webrtc.md) - WebRTC implementation details
- [Network Protocol](network-protocol.md) - P2P networking protocols
- [Bootstrap Health Integration](BOOTSTRAP_HEALTH_INTEGRATION.md) - Bootstrap node monitoring