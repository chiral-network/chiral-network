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

use crate::analytics::AnalyticsService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter};
use tracing::{debug, error};

/// Current version of the event schema for backwards compatibility
pub const EVENT_SCHEMA_VERSION: &str = "1.0.0";

// ============================================================================
// Search Events (DHT metadata search lifecycle)
// ============================================================================

/// Search lifecycle events - covers all stages of a DHT file search
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchEvent {
    /// Search initiated
    Started(SearchStartedEvent),

    /// Basic metadata found in DHT
    MetadataFound(MetadataFoundEvent),

    /// Provider peers discovered
    ProvidersFound(ProvidersFoundEvent),

    /// General seeder info received (wallet, default price)
    SeederGeneralInfo(SeederGeneralInfoEvent),

    /// File-specific seeder info received (protocols, prices)
    SeederFileInfo(SeederFileInfoEvent),

    /// Search completed successfully
    Complete(SearchCompleteEvent),

    /// Search timed out with partial results
    Timeout(SearchTimeoutEvent),
}

/// Event when a search is initiated
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStartedEvent {
    pub file_hash: String,
    pub timestamp: u64,
}

/// Event when basic metadata is found in DHT
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataFoundEvent {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub created_at: u64,
    pub mime_type: Option<String>,
}

/// Event when provider peers are discovered
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersFoundEvent {
    pub file_hash: String,
    pub providers: Vec<String>,
    pub count: usize,
}

/// Event when general seeder info is received
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeederGeneralInfoEvent {
    pub file_hash: String,
    pub seeder_index: usize,
    pub peer_id: String,
    pub wallet_address: String,
    pub default_price_per_mb: f64,
}

/// Event when file-specific seeder info is received
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeederFileInfoEvent {
    pub file_hash: String,
    pub seeder_index: usize,
    pub peer_id: String,
    pub price_per_mb: Option<f64>,
    pub supported_protocols: Vec<String>,
    pub protocol_details: serde_json::Value,
}

/// Event when search completes successfully
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCompleteEvent {
    pub file_hash: String,
    pub total_seeders: usize,
    pub duration_ms: u64,
}

/// Event when search times out with partial results
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTimeoutEvent {
    pub file_hash: String,
    pub partial_seeders: usize,
    pub missing_count: usize,
}

// ============================================================================
// App Event (Unified wrapper for all event types)
// ============================================================================

/// Unified app event that wraps all event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum AppEvent {
    Transfer(TransferEvent),
    Search(SearchEvent),
}

// ============================================================================
// Transfer Events
// ============================================================================

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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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
    pub protocol: String, // "WebRTC" | "BitTorrent" | "HTTP" | "FTP" | "ED2K"
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

/// The main event bus for emitting all app events (transfers + searches)
#[derive(Clone)]
pub struct AppEventBus {
    app_handle: AppHandle,
}

/// Type alias for backwards compatibility
pub type TransferEventBus = AppEventBus;

impl AppEventBus {
    /// Create a new event bus with the given app handle
    pub fn new(app_handle: AppHandle) -> Self {
        debug!("Initializing AppEventBus");
        Self { app_handle }
    }

    /// Emit an app event to all listeners
    pub fn emit_app(&self, event: AppEvent) {
        match &event {
            AppEvent::Transfer(transfer_event) => self.emit(transfer_event.clone()),
            AppEvent::Search(search_event) => self.emit_search(search_event.clone()),
        }

        // Also emit to unified app:event channel
        if let Err(e) = self.app_handle.emit("app:event", &event) {
            error!("Failed to emit event to app:event: {}", e);
        }
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

    /// Emit a search event to all listeners
    pub fn emit_search(&self, event: SearchEvent) {
        let event_type = match &event {
            SearchEvent::Started(_) => "started",
            SearchEvent::MetadataFound(_) => "metadata_found",
            SearchEvent::ProvidersFound(_) => "providers_found",
            SearchEvent::SeederGeneralInfo(_) => "seeder_general_info",
            SearchEvent::SeederFileInfo(_) => "seeder_file_info",
            SearchEvent::Complete(_) => "complete",
            SearchEvent::Timeout(_) => "timeout",
        };

        debug!("Emitting search event: {}", event_type);

        // Emit to specific typed channel
        let typed_channel = format!("search:{}", event_type);
        if let Err(e) = self.app_handle.emit(&typed_channel, &event) {
            error!("Failed to emit event to {}: {}", typed_channel, e);
        }

        // Also emit to generic channel for listeners who want all search events
        if let Err(e) = self.app_handle.emit("search:event", &event) {
            error!("Failed to emit event to search:event: {}", e);
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

    // =========================================================================
    // Search Event Helpers
    // =========================================================================

    /// Helper to emit search started event
    pub fn emit_search_started(&self, event: SearchStartedEvent) {
        self.emit_search(SearchEvent::Started(event));
    }

    /// Helper to emit metadata found event
    pub fn emit_metadata_found(&self, event: MetadataFoundEvent) {
        self.emit_search(SearchEvent::MetadataFound(event));
    }

    /// Helper to emit providers found event
    pub fn emit_providers_found(&self, event: ProvidersFoundEvent) {
        self.emit_search(SearchEvent::ProvidersFound(event));
    }

    /// Helper to emit seeder general info event
    pub fn emit_seeder_general_info(&self, event: SeederGeneralInfoEvent) {
        self.emit_search(SearchEvent::SeederGeneralInfo(event));
    }

    /// Helper to emit seeder file info event
    pub fn emit_seeder_file_info(&self, event: SeederFileInfoEvent) {
        self.emit_search(SearchEvent::SeederFileInfo(event));
    }

    /// Helper to emit search complete event
    pub fn emit_search_complete(&self, event: SearchCompleteEvent) {
        self.emit_search(SearchEvent::Complete(event));
    }

    /// Helper to emit search timeout event
    pub fn emit_search_timeout(&self, event: SearchTimeoutEvent) {
        self.emit_search(SearchEvent::Timeout(event));
    }

    // =========================================================================
    // Analytics Integration
    // =========================================================================

    /// Emit a transfer event to all listeners AND update analytics
    ///
    /// This method should be used when you want to emit an event and also
    /// update the backend analytics service in a single call.
    pub async fn emit_with_analytics(&self, event: TransferEvent, analytics: &Arc<AnalyticsService>) {
        // Emit to frontend
        self.emit(event.clone());

        // Update backend analytics
        analytics.handle_transfer_event(&event).await;

        debug!("Emitted event with analytics update");
    }

    /// Helper to emit queued event with analytics
    pub async fn emit_queued_with_analytics(&self, event: TransferQueuedEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Queued(event), analytics).await;
    }

    /// Helper to emit started event with analytics
    pub async fn emit_started_with_analytics(&self, event: TransferStartedEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Started(event), analytics).await;
    }

    /// Helper to emit source connected event with analytics
    pub async fn emit_source_connected_with_analytics(&self, event: SourceConnectedEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::SourceConnected(event), analytics).await;
    }

    /// Helper to emit progress event with analytics
    pub async fn emit_progress_with_analytics(&self, event: TransferProgressEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Progress(event), analytics).await;
    }

    /// Helper to emit completed event with analytics
    pub async fn emit_completed_with_analytics(&self, event: TransferCompletedEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Completed(event), analytics).await;
    }

    /// Helper to emit failed event with analytics
    pub async fn emit_failed_with_analytics(&self, event: TransferFailedEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Failed(event), analytics).await;
    }

    /// Helper to emit canceled event with analytics
    pub async fn emit_canceled_with_analytics(&self, event: TransferCanceledEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Canceled(event), analytics).await;
    }

    /// Helper to emit paused event with analytics
    pub async fn emit_paused_with_analytics(&self, event: TransferPausedEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Paused(event), analytics).await;
    }

    /// Helper to emit resumed event with analytics
    pub async fn emit_resumed_with_analytics(&self, event: TransferResumedEvent, analytics: &Arc<AnalyticsService>) {
        self.emit_with_analytics(TransferEvent::Resumed(event), analytics).await;
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
            protocol: "WebRTC".to_string(),
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
                assert_eq!(e.protocol, "WebRTC");
                assert_eq!(e.file_name, "test.txt");
            }
            _ => panic!("Wrong event type"),
        }
    }
}