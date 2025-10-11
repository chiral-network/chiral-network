use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// Bandwidth usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthStats {
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub last_updated: u64,
}

/// Historical bandwidth data point
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthDataPoint {
    pub timestamp: u64,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub upload_rate_kbps: f64,
    pub download_rate_kbps: f64,
}

/// Network performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceMetrics {
    pub avg_download_speed_kbps: f64,
    pub avg_upload_speed_kbps: f64,
    pub peak_download_speed_kbps: f64,
    pub peak_upload_speed_kbps: f64,
    pub total_connections: u64,
    pub successful_transfers: u64,
    pub failed_transfers: u64,
    pub avg_latency_ms: f64,
}

/// Network activity statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkActivity {
    pub active_uploads: usize,
    pub active_downloads: usize,
    pub queued_downloads: usize,
    pub completed_uploads: u64,
    pub completed_downloads: u64,
    pub total_peers_connected: usize,
    pub unique_peers_all_time: u64,
}

/// Resource contribution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContribution {
    pub storage_contributed_bytes: u64,
    pub bandwidth_contributed_bytes: u64,
    pub files_shared: usize,
    pub total_seedtime_hours: f64,
    pub reputation_score: f64,
}

/// Historical resource contribution data point
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributionDataPoint {
    pub timestamp: u64,
    pub bandwidth_contributed: u64,
    pub storage_contributed: u64,
    pub files_seeded: usize,
}

const MAX_HISTORY_SIZE: usize = 1000;
const HISTORY_INTERVAL_SECONDS: u64 = 60; // Record every minute

pub struct AnalyticsService {
    bandwidth_history: Arc<Mutex<VecDeque<BandwidthDataPoint>>>,
    contribution_history: Arc<Mutex<VecDeque<ContributionDataPoint>>>,
    current_bandwidth: Arc<Mutex<BandwidthStats>>,
    performance: Arc<Mutex<PerformanceMetrics>>,
    network_activity: Arc<Mutex<NetworkActivity>>,
    resource_contribution: Arc<Mutex<ResourceContribution>>,
    last_history_update: Arc<Mutex<u64>>,
    unique_peers: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl AnalyticsService {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            bandwidth_history: Arc::new(Mutex::new(VecDeque::new())),
            contribution_history: Arc::new(Mutex::new(VecDeque::new())),
            current_bandwidth: Arc::new(Mutex::new(BandwidthStats {
                upload_bytes: 0,
                download_bytes: 0,
                last_updated: now,
            })),
            performance: Arc::new(Mutex::new(PerformanceMetrics {
                avg_download_speed_kbps: 0.0,
                avg_upload_speed_kbps: 0.0,
                peak_download_speed_kbps: 0.0,
                peak_upload_speed_kbps: 0.0,
                total_connections: 0,
                successful_transfers: 0,
                failed_transfers: 0,
                avg_latency_ms: 0.0,
            })),
            network_activity: Arc::new(Mutex::new(NetworkActivity {
                active_uploads: 0,
                active_downloads: 0,
                queued_downloads: 0,
                completed_uploads: 0,
                completed_downloads: 0,
                total_peers_connected: 0,
                unique_peers_all_time: 0,
            })),
            resource_contribution: Arc::new(Mutex::new(ResourceContribution {
                storage_contributed_bytes: 0,
                bandwidth_contributed_bytes: 0,
                files_shared: 0,
                total_seedtime_hours: 0.0,
                reputation_score: 5.0,
            })),
            last_history_update: Arc::new(Mutex::new(now)),
            unique_peers: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    /// Record bytes uploaded
    pub async fn record_upload(&self, bytes: u64) {
        let mut bandwidth = self.current_bandwidth.lock().await;
        bandwidth.upload_bytes += bytes;
        bandwidth.last_updated = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut contribution = self.resource_contribution.lock().await;
        contribution.bandwidth_contributed_bytes += bytes;

        self.maybe_record_history().await;
    }

    /// Record bytes downloaded
    pub async fn record_download(&self, bytes: u64) {
        let mut bandwidth = self.current_bandwidth.lock().await;
        bandwidth.download_bytes += bytes;
        bandwidth.last_updated = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.maybe_record_history().await;
    }

    /// Record a transfer completion with performance data
    pub async fn record_transfer(
        &self,
        bytes: u64,
        duration_ms: u64,
        is_upload: bool,
        success: bool,
    ) {
        if duration_ms == 0 {
            return;
        }

        let speed_kbps = (bytes * 8) as f64 / duration_ms as f64; // bits per ms = kbps

        let mut perf = self.performance.lock().await;
        perf.total_connections += 1;

        if success {
            perf.successful_transfers += 1;

            if is_upload {
                // Update average upload speed (moving average)
                if perf.avg_upload_speed_kbps == 0.0 {
                    perf.avg_upload_speed_kbps = speed_kbps;
                } else {
                    perf.avg_upload_speed_kbps =
                        (perf.avg_upload_speed_kbps * 0.8) + (speed_kbps * 0.2);
                }
                perf.peak_upload_speed_kbps = perf.peak_upload_speed_kbps.max(speed_kbps);
            } else {
                // Update average download speed (moving average)
                if perf.avg_download_speed_kbps == 0.0 {
                    perf.avg_download_speed_kbps = speed_kbps;
                } else {
                    perf.avg_download_speed_kbps =
                        (perf.avg_download_speed_kbps * 0.8) + (speed_kbps * 0.2);
                }
                perf.peak_download_speed_kbps = perf.peak_download_speed_kbps.max(speed_kbps);
            }
        } else {
            perf.failed_transfers += 1;
        }
    }

    /// Update latency metric
    pub async fn record_latency(&self, latency_ms: f64) {
        let mut perf = self.performance.lock().await;
        if perf.avg_latency_ms == 0.0 {
            perf.avg_latency_ms = latency_ms;
        } else {
            perf.avg_latency_ms = (perf.avg_latency_ms * 0.9) + (latency_ms * 0.1);
        }
    }

    /// Update network activity
    pub async fn update_network_activity(
        &self,
        active_uploads: usize,
        active_downloads: usize,
        queued_downloads: usize,
    ) {
        let mut activity = self.network_activity.lock().await;
        activity.active_uploads = active_uploads;
        activity.active_downloads = active_downloads;
        activity.queued_downloads = queued_downloads;
    }

    /// Record upload completion
    pub async fn record_upload_completed(&self) {
        let mut activity = self.network_activity.lock().await;
        activity.completed_uploads += 1;
    }

    /// Record download completion
    pub async fn record_download_completed(&self) {
        let mut activity = self.network_activity.lock().await;
        activity.completed_downloads += 1;
    }

    /// Record peer connection
    pub async fn record_peer_connected(&self, peer_id: String) {
        let mut peers = self.unique_peers.lock().await;
        let is_new = peers.insert(peer_id);

        let mut activity = self.network_activity.lock().await;
        activity.total_peers_connected = peers.len();
        if is_new {
            activity.unique_peers_all_time += 1;
        }
    }

    /// Update storage contribution
    pub async fn update_storage_contribution(&self, bytes: u64, files_count: usize) {
        let mut contribution = self.resource_contribution.lock().await;
        contribution.storage_contributed_bytes = bytes;
        contribution.files_shared = files_count;
    }

    /// Update seedtime
    pub async fn add_seedtime_hours(&self, hours: f64) {
        let mut contribution = self.resource_contribution.lock().await;
        contribution.total_seedtime_hours += hours;
    }

    /// Maybe record a historical data point
    async fn maybe_record_history(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut last_update = self.last_history_update.lock().await;

        if now - *last_update >= HISTORY_INTERVAL_SECONDS {
            *last_update = now;
            drop(last_update);

            let bandwidth = self.current_bandwidth.lock().await.clone();
            let contribution = self.resource_contribution.lock().await.clone();

            // Calculate rates (bytes per interval)
            let mut history = self.bandwidth_history.lock().await;

            let (upload_rate, download_rate) = if let Some(last) = history.back() {
                let time_diff = (now - last.timestamp) as f64;
                let upload_diff = bandwidth.upload_bytes.saturating_sub(last.upload_bytes) as f64;
                let download_diff =
                    bandwidth.download_bytes.saturating_sub(last.download_bytes) as f64;

                (
                    (upload_diff * 8.0) / (time_diff * 1000.0),   // kbps
                    (download_diff * 8.0) / (time_diff * 1000.0), // kbps
                )
            } else {
                (0.0, 0.0)
            };

            history.push_back(BandwidthDataPoint {
                timestamp: now,
                upload_bytes: bandwidth.upload_bytes,
                download_bytes: bandwidth.download_bytes,
                upload_rate_kbps: upload_rate,
                download_rate_kbps: download_rate,
            });

            if history.len() > MAX_HISTORY_SIZE {
                history.pop_front();
            }
            drop(history);

            // Record contribution history
            let mut contrib_history = self.contribution_history.lock().await;
            contrib_history.push_back(ContributionDataPoint {
                timestamp: now,
                bandwidth_contributed: contribution.bandwidth_contributed_bytes,
                storage_contributed: contribution.storage_contributed_bytes,
                files_seeded: contribution.files_shared,
            });

            if contrib_history.len() > MAX_HISTORY_SIZE {
                contrib_history.pop_front();
            }
        }
    }

    /// Get current bandwidth statistics
    pub async fn get_bandwidth_stats(&self) -> BandwidthStats {
        self.current_bandwidth.lock().await.clone()
    }

    /// Get bandwidth history
    pub async fn get_bandwidth_history(&self, limit: Option<usize>) -> Vec<BandwidthDataPoint> {
        let history = self.bandwidth_history.lock().await;
        let limit = limit.unwrap_or(MAX_HISTORY_SIZE);
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Get performance metrics
    pub async fn get_performance_metrics(&self) -> PerformanceMetrics {
        self.performance.lock().await.clone()
    }

    /// Get network activity
    pub async fn get_network_activity(&self) -> NetworkActivity {
        self.network_activity.lock().await.clone()
    }

    /// Get resource contribution
    pub async fn get_resource_contribution(&self) -> ResourceContribution {
        self.resource_contribution.lock().await.clone()
    }

    /// Get contribution history
    pub async fn get_contribution_history(
        &self,
        limit: Option<usize>,
    ) -> Vec<ContributionDataPoint> {
        let history = self.contribution_history.lock().await;
        let limit = limit.unwrap_or(MAX_HISTORY_SIZE);
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Reset all statistics (for testing or user request)
    pub async fn reset_stats(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        *self.current_bandwidth.lock().await = BandwidthStats {
            upload_bytes: 0,
            download_bytes: 0,
            last_updated: now,
        };

        *self.performance.lock().await = PerformanceMetrics {
            avg_download_speed_kbps: 0.0,
            avg_upload_speed_kbps: 0.0,
            peak_download_speed_kbps: 0.0,
            peak_upload_speed_kbps: 0.0,
            total_connections: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            avg_latency_ms: 0.0,
        };

        self.bandwidth_history.lock().await.clear();
        self.contribution_history.lock().await.clear();
    }
}

impl Clone for AnalyticsService {
    fn clone(&self) -> Self {
        Self {
            bandwidth_history: Arc::clone(&self.bandwidth_history),
            contribution_history: Arc::clone(&self.contribution_history),
            current_bandwidth: Arc::clone(&self.current_bandwidth),
            performance: Arc::clone(&self.performance),
            network_activity: Arc::clone(&self.network_activity),
            resource_contribution: Arc::clone(&self.resource_contribution),
            last_history_update: Arc::clone(&self.last_history_update),
            unique_peers: Arc::clone(&self.unique_peers),
        }
    }
}
