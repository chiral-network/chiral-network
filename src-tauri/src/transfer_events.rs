// Transfer Event Bus - Typed event system for file transfer lifecycle
//
// This module provides a unified, typed event system for all transfer operations
// across different protocols (HTTP, FTP, P2P, BitTorrent). It standardizes how
// transfer state changes are communicated from the backend to the UI.
//
// Design principles:
// - Protocol-agnostic: Works with HTTP, FTP, WebRTC, BitTorrent, etc.
// - Type-safe: All events are strongly typed with serde for JSON serialization
// - Comprehensive: Covers the full transfer lifecycle from queue to completion
// - Observable: Easy to subscribe to specific event types or all events
// - Debuggable: All events carry contextual information for troubleshooting

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use tauri::{AppHandle, Emitter};
use tracing::{debug, error};

/// Current version of the event schema for backwards compatibility
pub const EVENT_SCHEMA_VERSION: &str = "1.0.0";

/// Primary transfer lifecycle events - covers all stages of a file transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransferEvent {
    /// Transfer added to queue but not yet started
    Queued(TransferQueuedEvent),
    
    /// Transfer is starting (discovering sources, initializing connections)
    Started(TransferStartedEvent),
    
    /// A source (peer/server) has connected successfully
    SourceConnected(SourceConnectedEvent),
    
    /// A source failed to connect or disconnected
    SourceDisconnected(SourceDisconnectedEvent),
    
    /// A chunk was downloaded and verified successfully
    ChunkCompleted(ChunkCompletedEvent),
    
    /// A chunk download failed (will be retried if possible)
    ChunkFailed(ChunkFailedEvent),
    
    /// Progress update (periodic updates during transfer)
    Progress(TransferProgressEvent),
    
    /// Transfer was paused by user or system
    Paused(TransferPausedEvent),
    
    /// Transfer was resumed after being paused
    Resumed(TransferResumedEvent),
    
    /// Transfer completed successfully
    Completed(TransferCompletedEvent),
    
    /// Transfer failed permanently (no more retries)
    Failed(TransferFailedEvent),
    
    /// Transfer was canceled by user
    Canceled(TransferCanceledEvent),
    
    /// Speed/bandwidth update (more frequent than progress updates)
    SpeedUpdate(SpeedUpdateEvent),
}

/// Event when a transfer is added to the download queue
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferQueuedEvent {
    pub transfer_id: String,
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub output_path: String,
    pub priority: TransferPriority,
    pub queued_at: u64, // Unix timestamp in milliseconds
    pub queue_position: usize,
    pub estimated_sources: usize,
}

/// Event when a transfer actually begins
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStartedEvent {
    pub transfer_id: String,
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub total_chunks: u32,
    pub chunk_size: usize,
    pub started_at: u64,
    pub available_sources: Vec<SourceInfo>,
    pub selected_sources: Vec<String>, // Source IDs that were selected
}

/// Event when a source successfully connects
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceConnectedEvent {
    pub transfer_id: String,
    pub source_id: String,
    pub source_type: SourceType,
    pub source_info: SourceInfo,
    pub connected_at: u64,
    pub assigned_chunks: Vec<u32>,
}

/// Event when a source disconnects or fails
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDisconnectedEvent {
    pub transfer_id: String,
    pub source_id: String,
    pub source_type: SourceType,
    pub disconnected_at: u64,
    pub reason: DisconnectReason,
    pub chunks_completed: u32,
    pub will_retry: bool,
}

/// Event when a chunk is successfully downloaded and verified
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkCompletedEvent {
    pub transfer_id: String,
    pub chunk_id: u32,
    pub chunk_size: usize,
    pub source_id: String,
    pub source_type: SourceType,
    pub completed_at: u64,
    pub download_duration_ms: u64,
    pub verified: bool, // Whether hash verification passed
}

/// Event when a chunk download fails
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkFailedEvent {
    pub transfer_id: String,
    pub chunk_id: u32,
    pub source_id: String,
    pub source_type: SourceType,
    pub failed_at: u64,
    pub error: String,
    pub retry_count: u32,
    pub will_retry: bool,
    pub next_retry_at: Option<u64>,
}

/// Periodic progress update event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgressEvent {
    pub transfer_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub progress_percentage: f64,
    pub download_speed_bps: f64, // Bytes per second
    pub upload_speed_bps: f64, // For seeding
    pub eta_seconds: Option<u32>,
    pub active_sources: usize,
    pub timestamp: u64,
}

/// Event when transfer is paused
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPausedEvent {
    pub transfer_id: String,
    pub paused_at: u64,
    pub reason: PauseReason,
    pub can_resume: bool,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Event when transfer is resumed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResumedEvent {
    pub transfer_id: String,
    pub resumed_at: u64,
    pub downloaded_bytes: u64,
    pub remaining_bytes: u64,
    pub active_sources: usize,
}

/// Event when transfer completes successfully
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferCompletedEvent {
    pub transfer_id: String,
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub output_path: String,
    pub completed_at: u64,
    pub duration_seconds: u64,
    pub average_speed_bps: f64,
    pub total_chunks: u32,
    pub sources_used: Vec<SourceSummary>,
}

/// Event when transfer fails permanently
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferFailedEvent {
    pub transfer_id: String,
    pub file_hash: String,
    pub failed_at: u64,
    pub error: String,
    pub error_category: ErrorCategory,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub retry_possible: bool,
}

/// Event when transfer is canceled by user
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferCanceledEvent {
    pub transfer_id: String,
    pub canceled_at: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub keep_partial: bool, // Whether to keep partial download
}

/// Speed update event (sent more frequently than progress)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedUpdateEvent {
    pub transfer_id: String,
    pub download_speed_bps: f64,
    pub upload_speed_bps: f64,
    pub timestamp: u64,
}

// ============================================================================
// Supporting Types
// ============================================================================

/// Priority level for transfers
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferPriority {
    Low,
    Normal,
    High,
}

impl Default for TransferPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Type of data source
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Http,
    Ftp,
    P2p,
    BitTorrent,
    WebRtc,
    Relay,
}

/// Information about a data source
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub id: String,
    pub source_type: SourceType,
    pub address: String,
    pub reputation: Option<f64>,
    pub estimated_speed_bps: Option<f64>,
    pub latency_ms: Option<u32>,
    pub location: Option<String>,
}

/// Summary of source contribution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub source_id: String,
    pub source_type: SourceType,
    pub chunks_provided: u32,
    pub bytes_provided: u64,
    pub average_speed_bps: f64,
    pub connection_duration_seconds: u64,
}

/// Reason for disconnection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Reason for pausing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    UserRequested,
    NetworkLost,
    DiskFull,
    NoSourcesAvailable,
    RateLimited,
    SystemSuspend,
    Other(String),
}

/// Category of error for analytics and recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

// ============================================================================
// Event Bus Implementation
// ============================================================================

/// The main event bus for emitting transfer events
#[derive(Clone)]
pub struct TransferEventBus {
    app_handle: AppHandle,
}

impl TransferEventBus {
    /// Create a new event bus with the given app handle
    pub fn new(app_handle: AppHandle) -> Self {
        debug!("Initializing TransferEventBus");
        Self { app_handle }
    }

    /// Emit a transfer event to all listeners
    pub fn emit(&self, event: TransferEvent) {
        let event_type = match &event {
            TransferEvent::Queued(_) => "queued",
            TransferEvent::Started(_) => "started",
            TransferEvent::SourceConnected(_) => "source_connected",
            TransferEvent::SourceDisconnected(_) => "source_disconnected",
            TransferEvent::ChunkCompleted(_) => "chunk_completed",
            TransferEvent::ChunkFailed(_) => "chunk_failed",
            TransferEvent::Progress(_) => "progress",
            TransferEvent::Paused(_) => "paused",
            TransferEvent::Resumed(_) => "resumed",
            TransferEvent::Completed(_) => "completed",
            TransferEvent::Failed(_) => "failed",
            TransferEvent::Canceled(_) => "canceled",
            TransferEvent::SpeedUpdate(_) => "speed_update",
        };

        debug!("Emitting transfer event: {}", event_type);

        // Emit to specific typed channel
        let typed_channel = format!("transfer:{}", event_type);
        if let Err(e) = self.app_handle.emit(&typed_channel, &event) {
            error!("Failed to emit event to {}: {}", typed_channel, e);
        }

        // Also emit to generic channel for listeners who want all events
        if let Err(e) = self.app_handle.emit("transfer:event", &event) {
            error!("Failed to emit event to transfer:event: {}", e);
        }
    }

    /// Helper to emit queued event
    pub fn emit_queued(&self, event: TransferQueuedEvent) {
        self.emit(TransferEvent::Queued(event));
    }

    /// Helper to emit started event
    pub fn emit_started(&self, event: TransferStartedEvent) {
        self.emit(TransferEvent::Started(event));
    }

    /// Helper to emit source connected event
    pub fn emit_source_connected(&self, event: SourceConnectedEvent) {
        self.emit(TransferEvent::SourceConnected(event));
    }

    /// Helper to emit source disconnected event
    pub fn emit_source_disconnected(&self, event: SourceDisconnectedEvent) {
        self.emit(TransferEvent::SourceDisconnected(event));
    }

    /// Helper to emit chunk completed event
    pub fn emit_chunk_completed(&self, event: ChunkCompletedEvent) {
        self.emit(TransferEvent::ChunkCompleted(event));
    }

    /// Helper to emit chunk failed event
    pub fn emit_chunk_failed(&self, event: ChunkFailedEvent) {
        self.emit(TransferEvent::ChunkFailed(event));
    }

    /// Helper to emit progress event
    pub fn emit_progress(&self, event: TransferProgressEvent) {
        self.emit(TransferEvent::Progress(event));
    }

    /// Helper to emit paused event
    pub fn emit_paused(&self, event: TransferPausedEvent) {
        self.emit(TransferEvent::Paused(event));
    }

    /// Helper to emit resumed event
    pub fn emit_resumed(&self, event: TransferResumedEvent) {
        self.emit(TransferEvent::Resumed(event));
    }

    /// Helper to emit completed event
    pub fn emit_completed(&self, event: TransferCompletedEvent) {
        self.emit(TransferEvent::Completed(event));
    }

    /// Helper to emit failed event
    pub fn emit_failed(&self, event: TransferFailedEvent) {
        self.emit(TransferEvent::Failed(event));
    }

    /// Helper to emit canceled event
    pub fn emit_canceled(&self, event: TransferCanceledEvent) {
        self.emit(TransferEvent::Canceled(event));
    }

    /// Helper to emit speed update event
    pub fn emit_speed_update(&self, event: SpeedUpdateEvent) {
        self.emit(TransferEvent::SpeedUpdate(event));
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Get current Unix timestamp in milliseconds
pub fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Get current Unix timestamp in seconds
pub fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Calculate progress percentage
pub fn calculate_progress(downloaded: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (downloaded as f64 / total as f64) * 100.0
}

/// Calculate ETA in seconds based on current speed
pub fn calculate_eta(remaining_bytes: u64, speed_bps: f64) -> Option<u32> {
    if speed_bps <= 0.0 || remaining_bytes == 0 {
        return None;
    }
    Some((remaining_bytes as f64 / speed_bps) as u32)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_progress() {
        assert_eq!(calculate_progress(0, 100), 0.0);
        assert_eq!(calculate_progress(50, 100), 50.0);
        assert_eq!(calculate_progress(100, 100), 100.0);
        assert_eq!(calculate_progress(100, 0), 0.0); // Edge case
    }

    #[test]
    fn test_calculate_eta() {
        assert_eq!(calculate_eta(1000, 100.0), Some(10));
        assert_eq!(calculate_eta(0, 100.0), None);
        assert_eq!(calculate_eta(1000, 0.0), None);
    }

    #[test]
    fn test_event_serialization() {
        let event = TransferEvent::Queued(TransferQueuedEvent {
            transfer_id: "test-123".to_string(),
            file_hash: "abc123".to_string(),
            file_name: "test.txt".to_string(),
            file_size: 1024,
            output_path: "/tmp/test.txt".to_string(),
            priority: TransferPriority::Normal,
            queued_at: 1234567890,
            queue_position: 1,
            estimated_sources: 5,
        });

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: TransferEvent = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            TransferEvent::Queued(e) => {
                assert_eq!(e.transfer_id, "test-123");
                assert_eq!(e.file_name, "test.txt");
            }
            _ => panic!("Wrong event type"),
        }
    }
}