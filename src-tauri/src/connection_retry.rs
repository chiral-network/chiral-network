//! Connection Retry & Resilience Framework
//!
//! This module provides a unified retry infrastructure with exponential backoff
//! for connection-based operations across the Chiral Network, including:
//! - WebRTC peer connections
//! - DHT bootstrap and peer connections
//! - General network operations
//!
//! Features:
//! - Configurable exponential backoff
//! - Jitter to prevent thundering herd
//! - Connection state tracking
//! - Health monitoring with automatic recovery
//! - Metrics collection for observability

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

// ============================================================================
// Configuration Constants
// ============================================================================

/// Default maximum retry attempts
pub const DEFAULT_MAX_RETRIES: u32 = 5;

/// Default initial delay in milliseconds
pub const DEFAULT_INITIAL_DELAY_MS: u64 = 500;

/// Default maximum delay in milliseconds (30 seconds)
pub const DEFAULT_MAX_DELAY_MS: u64 = 30_000;

/// Default backoff multiplier
pub const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;

/// Default jitter factor (0.0 to 1.0)
pub const DEFAULT_JITTER_FACTOR: f64 = 0.1;

/// Default health check interval in seconds
pub const DEFAULT_HEALTH_CHECK_INTERVAL_SECS: u64 = 30;

/// Default minimum peers threshold for DHT health
pub const DEFAULT_MIN_PEERS_THRESHOLD: usize = 3;

/// Default reconnection cooldown in seconds
pub const DEFAULT_RECONNECT_COOLDOWN_SECS: u64 = 5;

// ============================================================================
// Retry Configuration
// ============================================================================

/// Configuration for retry behavior with exponential backoff
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_RETRIES,
            initial_delay_ms: DEFAULT_INITIAL_DELAY_MS,
            max_delay_ms: DEFAULT_MAX_DELAY_MS,
            backoff_multiplier: DEFAULT_BACKOFF_MULTIPLIER,
            jitter_factor: DEFAULT_JITTER_FACTOR,
            reset_on_success: true,
        }
    }
}

impl RetryConfig {
    /// Create a config optimized for WebRTC connections
    pub fn for_webrtc() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 15_000,
            backoff_multiplier: 2.0,
            jitter_factor: 0.2,
            reset_on_success: true,
        }
    }

    /// Create a config optimized for DHT bootstrap
    pub fn for_dht_bootstrap() -> Self {
        Self {
            max_attempts: 5,
            initial_delay_ms: 2000,
            max_delay_ms: 60_000,
            backoff_multiplier: 2.0,
            jitter_factor: 0.15,
            reset_on_success: true,
        }
    }

    /// Create a config optimized for DHT peer connections
    pub fn for_dht_peer() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 500,
            max_delay_ms: 10_000,
            backoff_multiplier: 1.5,
            jitter_factor: 0.1,
            reset_on_success: true,
        }
    }

    /// Create a config for aggressive retry (critical operations)
    pub fn aggressive() -> Self {
        Self {
            max_attempts: 10,
            initial_delay_ms: 100,
            max_delay_ms: 5_000,
            backoff_multiplier: 1.5,
            jitter_factor: 0.1,
            reset_on_success: true,
        }
    }

    /// Calculate delay for a given attempt number (0-indexed)
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let base_delay = self.initial_delay_ms as f64
            * self.backoff_multiplier.powi(attempt as i32);
        let capped_delay = base_delay.min(self.max_delay_ms as f64);

        // Apply jitter
        let jitter_range = capped_delay * self.jitter_factor;
        let jitter = if jitter_range > 0.0 {
            (rand::random::<f64>() * 2.0 - 1.0) * jitter_range
        } else {
            0.0
        };

        let final_delay = (capped_delay + jitter).max(0.0) as u64;
        Duration::from_millis(final_delay)
    }

    /// Check if we should retry given the current attempt count
    pub fn should_retry(&self, attempt: u32) -> bool {
        self.max_attempts == 0 || attempt < self.max_attempts
    }
}

// ============================================================================
// Connection State
// ============================================================================

/// State of a connection for retry tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Connection is healthy and active
    Connected,
    /// Connection is being established
    Connecting,
    /// Connection failed, waiting to retry
    Disconnected,
    /// Actively retrying connection
    Retrying,
    /// Max retries exceeded, in backoff
    BackingOff,
    /// Permanently failed (manual intervention needed)
    Failed,
}

impl Default for ConnectionState {
    fn default() -> Self {
        ConnectionState::Disconnected
    }
}

/// Tracking information for a single connection
#[derive(Debug, Clone)]
pub struct ConnectionTracker {
    /// Unique identifier for this connection
    pub id: String,
    /// Current state of the connection
    pub state: ConnectionState,
    /// Number of consecutive failures
    pub consecutive_failures: u32,
    /// Total number of connection attempts
    pub total_attempts: u32,
    /// Total number of successful connections
    pub total_successes: u32,
    /// Last successful connection timestamp
    pub last_success: Option<Instant>,
    /// Last failure timestamp
    pub last_failure: Option<Instant>,
    /// Last error message
    pub last_error: Option<String>,
    /// When the next retry is scheduled
    pub next_retry_at: Option<Instant>,
    /// Retry configuration for this connection
    pub config: RetryConfig,
    /// Creation timestamp
    pub created_at: Instant,
}

impl ConnectionTracker {
    /// Create a new connection tracker
    pub fn new(id: String, config: RetryConfig) -> Self {
        Self {
            id,
            state: ConnectionState::Disconnected,
            consecutive_failures: 0,
            total_attempts: 0,
            total_successes: 0,
            last_success: None,
            last_failure: None,
            last_error: None,
            next_retry_at: None,
            config,
            created_at: Instant::now(),
        }
    }

    /// Record a successful connection
    pub fn record_success(&mut self) {
        self.state = ConnectionState::Connected;
        self.last_success = Some(Instant::now());
        self.total_successes += 1;
        if self.config.reset_on_success {
            self.consecutive_failures = 0;
        }
        self.next_retry_at = None;
        self.last_error = None;
        debug!("Connection {} succeeded (total: {})", self.id, self.total_successes);
    }

    /// Record a failed connection attempt
    pub fn record_failure(&mut self, error: impl Into<String>) {
        self.consecutive_failures += 1;
        self.total_attempts += 1;
        self.last_failure = Some(Instant::now());
        self.last_error = Some(error.into());

        if self.config.should_retry(self.consecutive_failures) {
            let delay = self.config.calculate_delay(self.consecutive_failures - 1);
            self.next_retry_at = Some(Instant::now() + delay);
            self.state = ConnectionState::BackingOff;
            debug!(
                "Connection {} failed (attempt {}), retrying in {:?}",
                self.id, self.consecutive_failures, delay
            );
        } else {
            self.state = ConnectionState::Failed;
            warn!(
                "Connection {} permanently failed after {} attempts",
                self.id, self.consecutive_failures
            );
        }
    }

    /// Check if ready to retry
    pub fn is_ready_to_retry(&self) -> bool {
        match self.state {
            ConnectionState::BackingOff => {
                if let Some(retry_at) = self.next_retry_at {
                    Instant::now() >= retry_at
                } else {
                    true
                }
            }
            ConnectionState::Disconnected => true,
            _ => false,
        }
    }

    /// Start a retry attempt
    pub fn start_retry(&mut self) {
        self.state = ConnectionState::Retrying;
        self.total_attempts += 1;
    }

    /// Reset the tracker to initial state
    pub fn reset(&mut self) {
        self.state = ConnectionState::Disconnected;
        self.consecutive_failures = 0;
        self.next_retry_at = None;
        self.last_error = None;
    }

    /// Get time until next retry (if any)
    pub fn time_until_retry(&self) -> Option<Duration> {
        self.next_retry_at.map(|retry_at| {
            let now = Instant::now();
            if retry_at > now {
                retry_at - now
            } else {
                Duration::ZERO
            }
        })
    }
}

// ============================================================================
// Connection Manager
// ============================================================================

/// Manages multiple connections with retry logic
pub struct ConnectionManager {
    /// All tracked connections
    connections: Arc<RwLock<HashMap<String, ConnectionTracker>>>,
    /// Default configuration for new connections
    default_config: RetryConfig,
}

impl ConnectionManager {
    /// Create a new connection manager
    pub fn new(default_config: RetryConfig) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            default_config,
        }
    }

    /// Get or create a tracker for a connection
    pub async fn get_or_create(&self, id: &str) -> ConnectionTracker {
        let connections = self.connections.read().await;
        if let Some(tracker) = connections.get(id) {
            return tracker.clone();
        }
        drop(connections);

        let tracker = ConnectionTracker::new(id.to_string(), self.default_config.clone());
        let mut connections = self.connections.write().await;
        connections.insert(id.to_string(), tracker.clone());
        tracker
    }

    /// Update a connection tracker
    pub async fn update(&self, tracker: ConnectionTracker) {
        let mut connections = self.connections.write().await;
        connections.insert(tracker.id.clone(), tracker);
    }

    /// Record success for a connection
    pub async fn record_success(&self, id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(tracker) = connections.get_mut(id) {
            tracker.record_success();
        }
    }

    /// Record failure for a connection
    pub async fn record_failure(&self, id: &str, error: impl Into<String>) {
        let mut connections = self.connections.write().await;
        if let Some(tracker) = connections.get_mut(id) {
            tracker.record_failure(error);
        }
    }

    /// Get all connections ready to retry
    pub async fn get_ready_to_retry(&self) -> Vec<String> {
        let connections = self.connections.read().await;
        connections
            .iter()
            .filter(|(_, tracker)| tracker.is_ready_to_retry())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get connection stats
    pub async fn get_stats(&self) -> ConnectionManagerStats {
        let connections = self.connections.read().await;
        let mut stats = ConnectionManagerStats::default();

        for tracker in connections.values() {
            stats.total_connections += 1;
            match tracker.state {
                ConnectionState::Connected => stats.connected += 1,
                ConnectionState::Connecting | ConnectionState::Retrying => stats.connecting += 1,
                ConnectionState::Disconnected => stats.disconnected += 1,
                ConnectionState::BackingOff => stats.backing_off += 1,
                ConnectionState::Failed => stats.failed += 1,
            }
            stats.total_attempts += tracker.total_attempts as u64;
            stats.total_successes += tracker.total_successes as u64;
        }

        stats
    }

    /// Remove a connection from tracking
    pub async fn remove(&self, id: &str) {
        let mut connections = self.connections.write().await;
        connections.remove(id);
    }

    /// Clear all failed connections
    pub async fn clear_failed(&self) {
        let mut connections = self.connections.write().await;
        connections.retain(|_, tracker| tracker.state != ConnectionState::Failed);
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectionManagerStats {
    pub total_connections: usize,
    pub connected: usize,
    pub connecting: usize,
    pub disconnected: usize,
    pub backing_off: usize,
    pub failed: usize,
    pub total_attempts: u64,
    pub total_successes: u64,
}

// ============================================================================
// Health Monitor
// ============================================================================

/// Configuration for health monitoring
#[derive(Debug, Clone)]
pub struct HealthMonitorConfig {
    /// Interval between health checks
    pub check_interval: Duration,
    /// Minimum number of healthy connections required
    pub min_healthy_connections: usize,
    /// Whether to automatically attempt recovery
    pub auto_recovery: bool,
    /// Cooldown between recovery attempts
    pub recovery_cooldown: Duration,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(DEFAULT_HEALTH_CHECK_INTERVAL_SECS),
            min_healthy_connections: DEFAULT_MIN_PEERS_THRESHOLD,
            auto_recovery: true,
            recovery_cooldown: Duration::from_secs(DEFAULT_RECONNECT_COOLDOWN_SECS),
        }
    }
}

/// Health status of a connection pool
#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub connected_count: usize,
    pub min_required: usize,
    pub last_check: u64,
    pub recommendation: Option<String>,
}

// ============================================================================
// Retry Executor
// ============================================================================

/// Execute an async operation with retry logic
pub async fn with_retry<T, E, F, Fut>(
    config: &RetryConfig,
    operation_name: &str,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0u32;

    loop {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    info!(
                        "{} succeeded after {} attempts",
                        operation_name,
                        attempt + 1
                    );
                }
                return Ok(result);
            }
            Err(e) => {
                attempt += 1;

                if !config.should_retry(attempt) {
                    error!(
                        "{} failed permanently after {} attempts: {}",
                        operation_name, attempt, e
                    );
                    return Err(e);
                }

                let delay = config.calculate_delay(attempt - 1);
                warn!(
                    "{} failed (attempt {}), retrying in {:?}: {}",
                    operation_name, attempt, delay, e
                );

                sleep(delay).await;
            }
        }
    }
}

/// Execute an async operation with retry and custom tracker
pub async fn with_retry_tracked<T, E, F, Fut>(
    tracker: &mut ConnectionTracker,
    operation_name: &str,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    loop {
        // Wait if we're in backoff
        if let Some(wait_time) = tracker.time_until_retry() {
            if !wait_time.is_zero() {
                debug!(
                    "{}: waiting {:?} before retry",
                    operation_name, wait_time
                );
                sleep(wait_time).await;
            }
        }

        tracker.start_retry();

        match operation().await {
            Ok(result) => {
                tracker.record_success();
                return Ok(result);
            }
            Err(e) => {
                let error_msg = e.to_string();
                tracker.record_failure(&error_msg);

                if tracker.state == ConnectionState::Failed {
                    error!(
                        "{} permanently failed after {} attempts: {}",
                        operation_name, tracker.consecutive_failures, error_msg
                    );
                    return Err(e);
                }

                // Will retry on next loop iteration
            }
        }
    }
}

// ============================================================================
// DHT Health Monitor
// ============================================================================

/// Health monitor specifically for DHT connections
pub struct DhtHealthMonitor {
    /// Configuration
    config: HealthMonitorConfig,
    /// Last health check time
    last_check: Arc<Mutex<Option<Instant>>>,
    /// Last recovery attempt time
    last_recovery: Arc<Mutex<Option<Instant>>>,
    /// Whether monitor is running
    running: Arc<Mutex<bool>>,
}

impl DhtHealthMonitor {
    pub fn new(config: HealthMonitorConfig) -> Self {
        Self {
            config,
            last_check: Arc::new(Mutex::new(None)),
            last_recovery: Arc::new(Mutex::new(None)),
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Check if recovery should be triggered based on peer count
    pub async fn should_trigger_recovery(&self, current_peer_count: usize) -> bool {
        if !self.config.auto_recovery {
            return false;
        }

        if current_peer_count >= self.config.min_healthy_connections {
            return false;
        }

        // Check cooldown
        let last_recovery = self.last_recovery.lock().await;
        if let Some(last) = *last_recovery {
            if last.elapsed() < self.config.recovery_cooldown {
                return false;
            }
        }

        true
    }

    /// Record that recovery was attempted
    pub async fn record_recovery_attempt(&self) {
        let mut last_recovery = self.last_recovery.lock().await;
        *last_recovery = Some(Instant::now());
    }

    /// Get health status
    pub async fn get_health_status(&self, current_peer_count: usize) -> HealthStatus {
        let healthy = current_peer_count >= self.config.min_healthy_connections;
        let recommendation = if !healthy {
            Some(format!(
                "Peer count ({}) below minimum ({}). Consider re-bootstrapping.",
                current_peer_count, self.config.min_healthy_connections
            ))
        } else {
            None
        };

        HealthStatus {
            healthy,
            connected_count: current_peer_count,
            min_required: self.config.min_healthy_connections,
            last_check: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            recommendation,
        }
    }
}

// ============================================================================
// WebRTC Connection Retry
// ============================================================================

/// Retry context for WebRTC connections
#[derive(Debug, Clone)]
pub struct WebRtcRetryContext {
    pub peer_id: String,
    pub tracker: ConnectionTracker,
    /// Whether this is an outbound connection attempt
    pub is_outbound: bool,
    /// SDP offer for reconnection (if available)
    pub last_offer: Option<String>,
}

impl WebRtcRetryContext {
    pub fn new(peer_id: String, is_outbound: bool) -> Self {
        Self {
            peer_id: peer_id.clone(),
            tracker: ConnectionTracker::new(peer_id, RetryConfig::for_webrtc()),
            is_outbound,
            last_offer: None,
        }
    }

    /// Check if we should attempt reconnection
    pub fn should_reconnect(&self) -> bool {
        self.tracker.is_ready_to_retry() && self.is_outbound
    }

    /// Record successful connection
    pub fn record_connected(&mut self) {
        self.tracker.record_success();
    }

    /// Record connection failure
    pub fn record_failed(&mut self, error: impl Into<String>) {
        self.tracker.record_failure(error);
    }

    /// Get retry delay if in backoff
    pub fn get_retry_delay(&self) -> Option<Duration> {
        self.tracker.time_until_retry()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_delay_calculation() {
        let config = RetryConfig {
            max_attempts: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 10000,
            backoff_multiplier: 2.0,
            jitter_factor: 0.0, // No jitter for predictable testing
            reset_on_success: true,
        };

        // First retry: 1000ms
        let delay0 = config.calculate_delay(0);
        assert_eq!(delay0.as_millis(), 1000);

        // Second retry: 2000ms
        let delay1 = config.calculate_delay(1);
        assert_eq!(delay1.as_millis(), 2000);

        // Third retry: 4000ms
        let delay2 = config.calculate_delay(2);
        assert_eq!(delay2.as_millis(), 4000);

        // Fourth retry: 8000ms
        let delay3 = config.calculate_delay(3);
        assert_eq!(delay3.as_millis(), 8000);

        // Fifth retry: capped at 10000ms
        let delay4 = config.calculate_delay(4);
        assert_eq!(delay4.as_millis(), 10000);
    }

    #[test]
    fn test_should_retry() {
        let config = RetryConfig {
            max_attempts: 3,
            ..Default::default()
        };

        assert!(config.should_retry(0));
        assert!(config.should_retry(1));
        assert!(config.should_retry(2));
        assert!(!config.should_retry(3));
        assert!(!config.should_retry(4));
    }

    #[test]
    fn test_infinite_retry() {
        let config = RetryConfig {
            max_attempts: 0, // Infinite
            ..Default::default()
        };

        assert!(config.should_retry(0));
        assert!(config.should_retry(100));
        assert!(config.should_retry(1000));
    }

    #[test]
    fn test_connection_tracker_success() {
        let mut tracker = ConnectionTracker::new(
            "test-peer".to_string(),
            RetryConfig::default(),
        );

        assert_eq!(tracker.state, ConnectionState::Disconnected);
        assert_eq!(tracker.consecutive_failures, 0);

        tracker.record_success();

        assert_eq!(tracker.state, ConnectionState::Connected);
        assert_eq!(tracker.total_successes, 1);
        assert!(tracker.last_success.is_some());
    }

    #[test]
    fn test_connection_tracker_failure_and_retry() {
        let mut tracker = ConnectionTracker::new(
            "test-peer".to_string(),
            RetryConfig {
                max_attempts: 3,
                initial_delay_ms: 100,
                ..Default::default()
            },
        );

        // First failure
        tracker.record_failure("Connection refused");
        assert_eq!(tracker.state, ConnectionState::BackingOff);
        assert_eq!(tracker.consecutive_failures, 1);
        assert!(tracker.next_retry_at.is_some());

        // Second failure
        tracker.record_failure("Timeout");
        assert_eq!(tracker.consecutive_failures, 2);

        // Third failure - should be final
        tracker.record_failure("Network unreachable");
        assert_eq!(tracker.state, ConnectionState::Failed);
        assert_eq!(tracker.consecutive_failures, 3);
    }

    #[test]
    fn test_connection_tracker_reset_on_success() {
        let mut tracker = ConnectionTracker::new(
            "test-peer".to_string(),
            RetryConfig::default(),
        );

        tracker.record_failure("Error 1");
        tracker.record_failure("Error 2");
        assert_eq!(tracker.consecutive_failures, 2);

        tracker.record_success();
        assert_eq!(tracker.consecutive_failures, 0);
        assert_eq!(tracker.state, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn test_connection_manager() {
        let manager = ConnectionManager::new(RetryConfig::default());

        // Create tracker
        let tracker = manager.get_or_create("peer-1").await;
        assert_eq!(tracker.id, "peer-1");

        // Record success
        manager.record_success("peer-1").await;

        let stats = manager.get_stats().await;
        assert_eq!(stats.total_connections, 1);
        assert_eq!(stats.connected, 1);
    }

    #[tokio::test]
    async fn test_with_retry_success() {
        let config = RetryConfig::default();
        let mut attempts = 0;

        let result: Result<i32, &str> = with_retry(&config, "test_op", || {
            attempts += 1;
            async { Ok(42) }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn test_with_retry_eventual_success() {
        let config = RetryConfig {
            max_attempts: 5,
            initial_delay_ms: 10, // Fast for testing
            ..Default::default()
        };
        let attempts = Arc::new(Mutex::new(0));
        let attempts_clone = attempts.clone();

        let result: Result<i32, String> = with_retry(&config, "test_op", || {
            let attempts = attempts_clone.clone();
            async move {
                let mut count = attempts.lock().await;
                *count += 1;
                if *count < 3 {
                    Err(format!("Attempt {} failed", count))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(*attempts.lock().await, 3);
    }
}