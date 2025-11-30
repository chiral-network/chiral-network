use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Mutex;
use tokio::time::sleep;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::transfer_events::TransferEventBus;

// ============================================================================
// Bandwidth Events
// ============================================================================

/// Events emitted by the bandwidth controller
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BandwidthEvent {
    /// Bandwidth limits have been changed
    LimitsChanged(LimitsChangedEvent),
    
    /// Transfer is being throttled (waiting for tokens)
    Throttled(ThrottledEvent),
    
    /// Throttling has ended (tokens available)
    ThrottleReleased(ThrottleReleasedEvent),
    
    /// Periodic bandwidth usage statistics
    UsageStats(BandwidthUsageEvent),
}

/// Event when bandwidth limits are changed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsChangedEvent {
    pub upload_limit_kbps: u64,
    pub download_limit_kbps: u64,
    pub previous_upload_limit_kbps: u64,
    pub previous_download_limit_kbps: u64,
    pub timestamp: u64,
}

/// Event when a transfer is throttled
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottledEvent {
    pub transfer_id: Option<String>,
    pub direction: String, // "upload" or "download"
    pub requested_bytes: usize,
    pub available_tokens: f64,
    pub wait_duration_ms: u64,
    pub timestamp: u64,
}

/// Event when throttling is released
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleReleasedEvent {
    pub transfer_id: Option<String>,
    pub direction: String,
    pub waited_ms: u64,
    pub timestamp: u64,
}

/// Periodic bandwidth usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthUsageEvent {
    pub upload_bytes_used: u64,
    pub download_bytes_used: u64,
    pub upload_limit_kbps: u64,
    pub download_limit_kbps: u64,
    pub upload_utilization_percent: f64,
    pub download_utilization_percent: f64,
    pub period_seconds: u64,
    pub timestamp: u64,
}

/// Get current Unix timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// Bandwidth Controller
// ============================================================================

/// Simple token-bucket based bandwidth controller shared between upload and download paths.
pub struct BandwidthController {
    inner: Mutex<Inner>,
    event_bus: Option<Arc<TransferEventBus>>,
}

struct Inner {
    upload: TokenBucket,
    download: TokenBucket,
    // Usage tracking for statistics
    upload_bytes_used: u64,
    download_bytes_used: u64,
    stats_last_reset: Instant,
}

impl BandwidthController {
    /// Create a new bandwidth controller without event bus integration
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                upload: TokenBucket::unlimited(),
                download: TokenBucket::unlimited(),
                upload_bytes_used: 0,
                download_bytes_used: 0,
                stats_last_reset: Instant::now(),
            }),
            event_bus: None,
        }
    }
    
    /// Create a new bandwidth controller with event bus integration
    pub fn with_event_bus(event_bus: Arc<TransferEventBus>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                upload: TokenBucket::unlimited(),
                download: TokenBucket::unlimited(),
                upload_bytes_used: 0,
                download_bytes_used: 0,
                stats_last_reset: Instant::now(),
            }),
            event_bus: Some(event_bus),
        }
    }
    
    /// Set the event bus after construction
    pub async fn set_event_bus(&mut self, event_bus: Arc<TransferEventBus>) {
        self.event_bus = Some(event_bus);
    }

    pub async fn set_limits(&self, upload_kbps: u64, download_kbps: u64) {
        let mut inner = self.inner.lock().await;
        
        // Capture previous limits for event
        let prev_upload = inner.upload.limit_kbps();
        let prev_download = inner.download.limit_kbps();
        
        inner.upload.set_limit(upload_kbps);
        inner.download.set_limit(download_kbps);
        
        // Emit limits changed event
        if let Some(bus) = &self.event_bus {
            let event = BandwidthEvent::LimitsChanged(LimitsChangedEvent {
                upload_limit_kbps: upload_kbps,
                download_limit_kbps: download_kbps,
                previous_upload_limit_kbps: prev_upload,
                previous_download_limit_kbps: prev_download,
                timestamp: current_timestamp_ms(),
            });
            emit_bandwidth_event(bus, event);
        }
        
        debug!(
            "Bandwidth limits set: upload={}KB/s, download={}KB/s",
            upload_kbps, download_kbps
        );
    }
    
    /// Get current bandwidth limits
    pub async fn get_limits(&self) -> (u64, u64) {
        let inner = self.inner.lock().await;
        (inner.upload.limit_kbps(), inner.download.limit_kbps())
    }

    pub async fn acquire_upload(&self, bytes: usize) {
        self.acquire(bytes, Direction::Upload, None).await;
    }

    pub async fn acquire_download(&self, bytes: usize) {
        self.acquire(bytes, Direction::Download, None).await;
    }
    
    /// Acquire upload bandwidth with transfer tracking
    pub async fn acquire_upload_for_transfer(&self, bytes: usize, transfer_id: &str) {
        self.acquire(bytes, Direction::Upload, Some(transfer_id.to_string())).await;
    }
    
    /// Acquire download bandwidth with transfer tracking
    pub async fn acquire_download_for_transfer(&self, bytes: usize, transfer_id: &str) {
        self.acquire(bytes, Direction::Download, Some(transfer_id.to_string())).await;
    }

    async fn acquire(&self, bytes: usize, direction: Direction, transfer_id: Option<String>) {
        if bytes == 0 {
            return;
        }

        let throttle_start = Instant::now();
        let mut was_throttled = false;
        let mut total_wait_ms: u64 = 0;

        loop {
            let wait = {
                let mut inner = self.inner.lock().await;
                let bucket = match direction {
                    Direction::Upload => &mut inner.upload,
                    Direction::Download => &mut inner.download,
                };
                
                let available_tokens = bucket.available_tokens();
                let result = bucket.consume(bytes);
                
                // Emit throttled event if we need to wait
                if let Some(delay) = &result {
                    if !delay.is_zero() && !was_throttled {
                        was_throttled = true;
                        if let Some(bus) = &self.event_bus {
                            let event = BandwidthEvent::Throttled(ThrottledEvent {
                                transfer_id: transfer_id.clone(),
                                direction: direction.as_str().to_string(),
                                requested_bytes: bytes,
                                available_tokens,
                                wait_duration_ms: delay.as_millis() as u64,
                                timestamp: current_timestamp_ms(),
                            });
                            emit_bandwidth_event(bus, event);
                        }
                    }
                }
                
                result
            };

            match wait {
                None => break,
                Some(delay) if delay.is_zero() => break,
                Some(delay) => {
                    total_wait_ms += delay.as_millis() as u64;
                    sleep(delay).await;
                }
            }
        }
        
        // Track usage
        {
            let mut inner = self.inner.lock().await;
            match direction {
                Direction::Upload => inner.upload_bytes_used += bytes as u64,
                Direction::Download => inner.download_bytes_used += bytes as u64,
            }
        }
        
        // Emit throttle released event if we were throttled
        if was_throttled {
            if let Some(bus) = &self.event_bus {
                let event = BandwidthEvent::ThrottleReleased(ThrottleReleasedEvent {
                    transfer_id,
                    direction: direction.as_str().to_string(),
                    waited_ms: throttle_start.elapsed().as_millis() as u64,
                    timestamp: current_timestamp_ms(),
                });
                emit_bandwidth_event(bus, event);
            }
        }
    }
    
    /// Get and reset usage statistics
    /// 
    /// Returns (upload_bytes, download_bytes, period_seconds) and resets counters
    pub async fn get_and_reset_usage(&self) -> (u64, u64, u64) {
        let mut inner = self.inner.lock().await;
        let upload = inner.upload_bytes_used;
        let download = inner.download_bytes_used;
        let period = inner.stats_last_reset.elapsed().as_secs();
        
        inner.upload_bytes_used = 0;
        inner.download_bytes_used = 0;
        inner.stats_last_reset = Instant::now();
        
        (upload, download, period)
    }
    
    /// Emit current usage statistics event
    pub async fn emit_usage_stats(&self) {
        let (upload_bytes, download_bytes, period) = self.get_and_reset_usage().await;
        let (upload_limit, download_limit) = self.get_limits().await;
        
        // Calculate utilization percentages
        let upload_util = if upload_limit > 0 && period > 0 {
            let max_bytes = upload_limit * 1024 * period;
            (upload_bytes as f64 / max_bytes as f64) * 100.0
        } else {
            0.0
        };
        
        let download_util = if download_limit > 0 && period > 0 {
            let max_bytes = download_limit * 1024 * period;
            (download_bytes as f64 / max_bytes as f64) * 100.0
        } else {
            0.0
        };
        
        if let Some(bus) = &self.event_bus {
            let event = BandwidthEvent::UsageStats(BandwidthUsageEvent {
                upload_bytes_used: upload_bytes,
                download_bytes_used: download_bytes,
                upload_limit_kbps: upload_limit,
                download_limit_kbps: download_limit,
                upload_utilization_percent: upload_util,
                download_utilization_percent: download_util,
                period_seconds: period,
                timestamp: current_timestamp_ms(),
            });
            emit_bandwidth_event(bus, event);
        }
    }
    
    /// Check if bandwidth is currently limited
    pub async fn is_limited(&self) -> (bool, bool) {
        let inner = self.inner.lock().await;
        (
            inner.upload.limit_bytes_per_sec.is_some(),
            inner.download.limit_bytes_per_sec.is_some(),
        )
    }
}

enum Direction {
    Upload,
    Download,
}

impl Direction {
    fn as_str(&self) -> &'static str {
        match self {
            Direction::Upload => "upload",
            Direction::Download => "download",
        }
    }
}

struct TokenBucket {
    limit_bytes_per_sec: Option<f64>,
    tokens: f64,
    capacity: f64,
    last_refill: Instant,
    limit_kbps_value: u64, // Store the KB/s value for reporting
}

impl TokenBucket {
    fn unlimited() -> Self {
        Self {
            limit_bytes_per_sec: None,
            tokens: f64::INFINITY,
            capacity: f64::INFINITY,
            last_refill: Instant::now(),
            limit_kbps_value: 0,
        }
    }
    
    /// Get the current limit in KB/s (0 = unlimited)
    fn limit_kbps(&self) -> u64 {
        self.limit_kbps_value
    }
    
    /// Get currently available tokens
    fn available_tokens(&self) -> f64 {
        self.tokens
    }

    fn set_limit(&mut self, limit_kbps: u64) {
        self.limit_kbps_value = limit_kbps;
        
        if limit_kbps == 0 {
            self.limit_bytes_per_sec = None;
            self.tokens = f64::INFINITY;
            self.capacity = f64::INFINITY;
            self.last_refill = Instant::now();
            return;
        }

        let limit = (limit_kbps as f64) * 1024.0; // Convert KB/s to bytes/s.
        self.limit_bytes_per_sec = Some(limit);
        self.capacity = limit * 2.0; // Allow up to ~2 seconds of burst.
        self.tokens = self.tokens.min(self.capacity);
        self.last_refill = Instant::now();
    }

    fn consume(&mut self, bytes: usize) -> Option<Duration> {
        let limit = match self.limit_bytes_per_sec {
            None => return None,
            Some(limit) if limit <= f64::EPSILON => return None,
            Some(limit) => limit,
        };

        self.refill(limit);

        let required = bytes as f64;
        if self.tokens >= required {
            self.tokens -= required;
            None
        } else {
            let deficit = required - self.tokens;
            self.tokens = 0.0;
            let wait_secs = deficit / limit;
            if wait_secs <= 0.0 {
                None
            } else {
                Some(Duration::from_secs_f64(wait_secs))
            }
        }
    }

    fn refill(&mut self, limit: f64) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed <= 0.0 {
            return;
        }

        let new_tokens = elapsed * limit;
        self.tokens = (self.tokens + new_tokens).min(self.capacity);
        self.last_refill = now;
    }
}

// ============================================================================
// Event Emission Helper
// ============================================================================

/// Emit a bandwidth event to the frontend
fn emit_bandwidth_event(bus: &TransferEventBus, event: BandwidthEvent) {
    let event_type = match &event {
        BandwidthEvent::LimitsChanged(_) => "limits_changed",
        BandwidthEvent::Throttled(_) => "throttled",
        BandwidthEvent::ThrottleReleased(_) => "throttle_released",
        BandwidthEvent::UsageStats(_) => "usage_stats",
    };
    
    debug!("Emitting bandwidth event: {} - {:?}", event_type, event);
    
    // Note: Full event emission would require extending TransferEventBus
    // to support bandwidth events with app_handle.emit("bandwidth:{}", event)
    // For now, events are logged for debugging/observability
    let _ = (bus, event); // Acknowledge usage to avoid warnings
}

// ============================================================================
// Default Implementation
// ============================================================================

impl Default for BandwidthController {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_unlimited() {
        let bucket = TokenBucket::unlimited();
        assert!(bucket.limit_bytes_per_sec.is_none());
        assert_eq!(bucket.limit_kbps(), 0);
    }

    #[test]
    fn test_token_bucket_set_limit() {
        let mut bucket = TokenBucket::unlimited();
        bucket.set_limit(100); // 100 KB/s
        
        assert!(bucket.limit_bytes_per_sec.is_some());
        assert_eq!(bucket.limit_kbps(), 100);
        assert_eq!(bucket.limit_bytes_per_sec.unwrap(), 100.0 * 1024.0);
    }

    #[test]
    fn test_token_bucket_consume_under_limit() {
        let mut bucket = TokenBucket::unlimited();
        bucket.set_limit(100); // 100 KB/s = 102400 bytes/s
        
        // Consuming less than available should return None (no wait)
        let result = bucket.consume(1024);
        assert!(result.is_none());
    }

    #[test]
    fn test_token_bucket_consume_over_limit() {
        let mut bucket = TokenBucket::unlimited();
        bucket.set_limit(1); // 1 KB/s = 1024 bytes/s
        bucket.tokens = 0.0; // Drain tokens
        
        // Consuming more than available should return Some(duration)
        let result = bucket.consume(1024);
        assert!(result.is_some());
        
        // Should need to wait ~1 second for 1KB at 1KB/s
        let wait = result.unwrap();
        assert!(wait.as_secs_f64() > 0.9 && wait.as_secs_f64() < 1.1);
    }

    #[test]
    fn test_token_bucket_unlimited_no_wait() {
        let mut bucket = TokenBucket::unlimited();
        
        // Unlimited bucket should never require waiting
        let result = bucket.consume(1_000_000);
        assert!(result.is_none());
    }

    #[test]
    fn test_direction_as_str() {
        assert_eq!(Direction::Upload.as_str(), "upload");
        assert_eq!(Direction::Download.as_str(), "download");
    }

    #[tokio::test]
    async fn test_bandwidth_controller_new() {
        let controller = BandwidthController::new();
        let (upload_limited, download_limited) = controller.is_limited().await;
        
        assert!(!upload_limited);
        assert!(!download_limited);
    }

    #[tokio::test]
    async fn test_bandwidth_controller_set_limits() {
        let controller = BandwidthController::new();
        controller.set_limits(100, 200).await;
        
        let (upload, download) = controller.get_limits().await;
        assert_eq!(upload, 100);
        assert_eq!(download, 200);
        
        let (upload_limited, download_limited) = controller.is_limited().await;
        assert!(upload_limited);
        assert!(download_limited);
    }

    #[tokio::test]
    async fn test_bandwidth_controller_usage_tracking() {
        let controller = BandwidthController::new();
        
        // Acquire some bandwidth
        controller.acquire_upload(1024).await;
        controller.acquire_download(2048).await;
        
        // Get usage stats
        let (upload, download, _period) = controller.get_and_reset_usage().await;
        assert_eq!(upload, 1024);
        assert_eq!(download, 2048);
        
        // After reset, should be zero
        let (upload2, download2, _) = controller.get_and_reset_usage().await;
        assert_eq!(upload2, 0);
        assert_eq!(download2, 0);
    }

    #[test]
    fn test_bandwidth_event_serialization() {
        let event = BandwidthEvent::LimitsChanged(LimitsChangedEvent {
            upload_limit_kbps: 100,
            download_limit_kbps: 200,
            previous_upload_limit_kbps: 0,
            previous_download_limit_kbps: 0,
            timestamp: 1234567890,
        });
        
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("limits_changed"));
        assert!(json.contains("100"));
        assert!(json.contains("200"));
    }

    #[test]
    fn test_throttled_event_serialization() {
        let event = BandwidthEvent::Throttled(ThrottledEvent {
            transfer_id: Some("test-123".to_string()),
            direction: "download".to_string(),
            requested_bytes: 65536,
            available_tokens: 1024.0,
            wait_duration_ms: 500,
            timestamp: 1234567890,
        });
        
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("throttled"));
        assert!(json.contains("test-123"));
        assert!(json.contains("download"));
    }
}