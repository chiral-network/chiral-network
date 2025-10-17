use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

// Import reputation system for blockchain integration
use crate::reputation::{ReputationEvent, EventType, ReputationSystem};

/// Peer performance metrics used for smart selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMetrics {
    pub peer_id: String,
    pub address: String,
    pub latency_ms: Option<u64>,
    pub bandwidth_kbps: Option<u64>,
    pub reliability_score: f64,    // 0.0 to 1.0
    pub uptime_score: f64,         // 0.0 to 1.0
    pub success_rate: f64,         // 0.0 to 1.0 (successful transfers)
    pub last_seen: u64,            // Unix timestamp
    pub transfer_count: u64,       // Total transfers attempted
    pub successful_transfers: u64, // Successful transfers
    pub failed_transfers: u64,     // Failed transfers
    pub total_bytes_transferred: u64,
    pub encryption_support: bool, // Supports encrypted transfers
    pub malicious_reports: u64,   // Number of malicious behavior reports
}

impl PeerMetrics {
    pub fn new(peer_id: String, address: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            peer_id,
            address,
            latency_ms: None,
            bandwidth_kbps: None,
            reliability_score: 0.5, // Start neutral
            uptime_score: 0.5,
            success_rate: 0.5,
            last_seen: now,
            transfer_count: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            encryption_support: false,
            malicious_reports: 0,
        }
    }

    /// Update metrics after a successful transfer
    pub fn record_successful_transfer(&mut self, bytes: u64, duration_ms: u64) {
        self.transfer_count += 1;
        self.successful_transfers += 1;
        self.total_bytes_transferred += bytes;
        self.last_seen = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Calculate bandwidth from this transfer
        if duration_ms > 0 {
            let bandwidth = (bytes * 8) / (duration_ms); // bits per ms = kbps
            self.bandwidth_kbps = Some(
                self.bandwidth_kbps
                    .map(|existing| (existing + bandwidth) / 2) // Moving average
                    .unwrap_or(bandwidth),
            );
        }

        self.update_scores();
    }

    /// Update metrics after a failed transfer
    pub fn record_failed_transfer(&mut self, error_type: &str) {
        self.transfer_count += 1;
        self.failed_transfers += 1;
        self.last_seen = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Penalize certain error types more heavily
        let penalty = match error_type {
            "timeout" => 0.1,
            "connection_refused" => 0.2,
            "encryption_error" => 0.15,
            _ => 0.05,
        };

        self.reliability_score = (self.reliability_score - penalty).max(0.0);
        self.update_scores();
    }

    /// Update latency measurement
    pub fn update_latency(&mut self, latency_ms: u64) {
        self.latency_ms = Some(
            self.latency_ms
                .map(|existing| (existing + latency_ms) / 2) // Moving average
                .unwrap_or(latency_ms),
        );
        self.last_seen = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Set encryption support capability
    pub fn set_encryption_support(&mut self, supported: bool) {
        self.encryption_support = supported;
    }

    /// Report malicious behavior from this peer
    /// A single report drastically reduces the peer's score
    pub fn report_malicious_behavior(&mut self, severity: &str) {
        self.malicious_reports += 1;

        // Apply severe penalty based on severity
        let penalty = match severity {
            "minor" => 0.2,    // Suspicious behavior, unusual patterns
            "moderate" => 0.5, // Clear policy violation, corrupted data
            "severe" => 0.9,   // Malicious attack, intentional harm
            _ => 0.3,          // Default moderate penalty
        };

        self.reliability_score = (self.reliability_score - penalty).max(0.0);
        self.update_scores();
    }

    /// Recalculate derived scores based on current metrics
    fn update_scores(&mut self) {
        // Update success rate
        if self.transfer_count > 0 {
            self.success_rate = self.successful_transfers as f64 / self.transfer_count as f64;
        }

        // Update reliability score (weighted combination of factors)
        let success_weight = 0.4;
        let latency_weight = 0.3;
        let uptime_weight = 0.3;

        let latency_score = self
            .latency_ms
            .map(|lat| (1000.0 - lat.min(1000) as f64) / 1000.0) // Better latency = higher score
            .unwrap_or(0.5);

        self.reliability_score = (success_weight * self.success_rate
            + latency_weight * latency_score
            + uptime_weight * self.uptime_score)
            .min(1.0);
    }

    /// Get overall peer quality score using weighted formula (0.0 to 1.0)
    /// Formula: LocalScore = (w_r * reliability) + (w_u * uptime) + (w_s * success_rate) + (w_b * bandwidth) - (p_a * age_penalty) - (p_m * malicious_penalty)
    pub fn get_quality_score(&self, prefer_encrypted: bool) -> f64 {
        // Weight constants for scoring formula
        let w_reliability = 0.25;
        let w_uptime = 0.20;
        let w_success = 0.25;
        let w_bandwidth = 0.20;
        let p_age = 0.0001; // Age penalty coefficient
        let p_malicious = 0.3; // Heavy penalty for malicious reports

        // Normalize bandwidth to 0.0-1.0 scale
        // Assume max bandwidth of 10 Mbps (10,000 kbps) for normalization
        let bandwidth_score = self
            .bandwidth_kbps
            .map(|bw| (bw as f64 / 10_000.0).min(1.0))
            .unwrap_or(0.0);

        // Age penalty calculation
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let age_seconds = now.saturating_sub(self.last_seen);
        let age_penalty = if age_seconds > 300 {
            // 5 minutes threshold
            (age_seconds - 300) as f64 * p_age
        } else {
            0.0
        };

        // Malicious behavior penalty (compounds with number of reports)
        let malicious_penalty = (self.malicious_reports as f64) * p_malicious;

        // Calculate base weighted score
        let base_score = (w_reliability * self.reliability_score)
            + (w_uptime * self.uptime_score)
            + (w_success * self.success_rate)
            + (w_bandwidth * bandwidth_score);

        // Encryption bonus (if preferred)
        let encryption_bonus = if prefer_encrypted && self.encryption_support {
            0.1
        } else {
            0.0
        };

        // Final score with all adjustments
        (base_score + encryption_bonus - age_penalty - malicious_penalty)
            .max(0.0)
            .min(1.0)
    }
}

/// Smart peer selection algorithms
#[derive(Debug, Clone)]
pub enum SelectionStrategy {
    /// Select peers with lowest latency
    FastestFirst,
    /// Select peers with highest reliability
    MostReliable,
    /// Select peers with highest bandwidth
    HighestBandwidth,
    /// Balanced selection considering multiple factors
    Balanced,
    /// Prefer peers with encryption support
    EncryptionPreferred,
    /// Load balancing across multiple good peers
    LoadBalanced,
    /// Use blockchain reputation for selection
    BlockchainReputation,
}

/// Peer selection service for smart routing decisions
pub struct PeerSelectionService {
    metrics: HashMap<String, PeerMetrics>,
    selection_history: HashMap<String, u64>, // peer_id -> last_selected_timestamp
    reputation_system: Option<Arc<tokio::sync::Mutex<ReputationSystem>>>, // Optional blockchain reputation system
    active_private_key: Option<String>, // Current active private key for blockchain transactions
    blockchain_reputation_cache: HashMap<String, (f64, u64)>, // peer_id -> (score, timestamp)
    cache_duration_seconds: u64, // How long to cache blockchain reputation scores
}

impl PeerSelectionService {
    pub fn new() -> Self {
        Self {
            metrics: HashMap::new(),
            selection_history: HashMap::new(),
            reputation_system: None,
            active_private_key: None,
            blockchain_reputation_cache: HashMap::new(),
            cache_duration_seconds: 300, // Cache for 5 minutes
        }
    }

    /// Set the blockchain reputation system for automatic event creation
    pub fn set_reputation_system(&mut self, reputation_system: Arc<tokio::sync::Mutex<ReputationSystem>>) {
        self.reputation_system = Some(reputation_system);
        info!("Peer selection service connected to blockchain reputation system");
    }

    /// Set the active private key for blockchain transactions
    pub fn set_active_private_key(&mut self, private_key: Option<String>) {
        let has_key = private_key.is_some();
        self.active_private_key = private_key;
        if has_key {
            info!("Peer selection service updated with active private key for blockchain transactions");
        } else {
            info!("Peer selection service cleared active private key");
        }
    }

    /// Get blockchain reputation score for a peer with caching
    async fn get_blockchain_reputation_score(&mut self, peer_id: &str) -> Result<f64, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Check cache first
        if let Some((cached_score, cache_timestamp)) = self.blockchain_reputation_cache.get(peer_id) {
            if now - cache_timestamp < self.cache_duration_seconds {
                debug!("Using cached blockchain reputation score for peer {}: {:.3}", peer_id, cached_score);
                return Ok(*cached_score);
            }
        }

        // Get fresh score from blockchain
        if let Some(ref rep_system) = self.reputation_system {
            let rep_system = rep_system.clone();
            let peer_id = peer_id.to_string();
            
            // Spawn async task to get blockchain reputation score
            let score = tokio::spawn(async move {
                let rep_system = rep_system.lock().await;
                rep_system.get_peer_reputation_score(&peer_id, None).await
            }).await.map_err(|e| format!("Failed to get blockchain reputation score: {}", e))?;

            let score = score?;
            
            // Cache the score
            self.blockchain_reputation_cache.insert(peer_id.to_string(), (score, now));
            
            debug!("Retrieved blockchain reputation score for peer {}: {:.3}", peer_id, score);
            Ok(score)
        } else {
            warn!("No reputation system available for blockchain reputation score");
            Ok(0.5) // Default neutral score
        }
    }

    /// Clear expired entries from blockchain reputation cache
    fn cleanup_reputation_cache(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.blockchain_reputation_cache.retain(|_, (_, timestamp)| {
            now - *timestamp < self.cache_duration_seconds
        });
    }


    /// Add or update a peer's metrics
    pub fn update_peer_metrics(&mut self, metrics: PeerMetrics) {
        debug!("Updating metrics for peer {}", metrics.peer_id);
        self.metrics.insert(metrics.peer_id.clone(), metrics);
    }

    /// Record a successful transfer for a peer
    pub fn record_transfer_success(&mut self, peer_id: &str, bytes: u64, duration_ms: u64) {
        if let Some(metrics) = self.metrics.get_mut(peer_id) {
            metrics.record_successful_transfer(bytes, duration_ms);
            info!(
                "Recorded successful transfer for peer {}: {} bytes in {}ms",
                peer_id, bytes, duration_ms
            );

            // Create blockchain reputation event for successful transfer
            let transfer_data = serde_json::json!({
                "bytes": bytes,
                "duration_ms": duration_ms,
                "speed_kbps": if duration_ms > 0 { (bytes * 8) / duration_ms } else { 0 },
                "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
            });

            // Spawn async task for reputation event creation
            if let Some(ref rep_system) = self.reputation_system {
                if let Some(ref private_key) = self.active_private_key {
                    let peer_id = peer_id.to_string();
                    let private_key = private_key.clone();
                    let rep_system = rep_system.clone();
                    tokio::spawn(async move {
                        let event = ReputationEvent::new(
                            format!("event_{}_{}", peer_id, SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_millis()),
                            peer_id.clone(),
                            "local_node".to_string(),
                            EventType::FileTransferSuccess,
                            transfer_data,
                            1.0,
                        );

                        let mut rep_system = rep_system.lock().await;
                        if let Err(e) = rep_system.add_reputation_event(event).await {
                            warn!("Failed to add reputation event for peer {}: {}", peer_id, e);
                        } else {
                            debug!("Created reputation event for peer {}: FileTransferSuccess", peer_id);
                        }
                    });
                } else {
                    warn!("No active private key available for reputation event creation");
                }
            }
        }
    }

    /// Record a failed transfer for a peer
    pub fn record_transfer_failure(&mut self, peer_id: &str, error: &str) {
        if let Some(metrics) = self.metrics.get_mut(peer_id) {
            metrics.record_failed_transfer(error);
            warn!("Recorded failed transfer for peer {}: {}", peer_id, error);

            // Create blockchain reputation event for failed transfer
            let failure_data = serde_json::json!({
                "error": error,
                "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
            });

            // Spawn async task for reputation event creation
            if let Some(ref rep_system) = self.reputation_system {
                if let Some(ref private_key) = self.active_private_key {
                    let peer_id = peer_id.to_string();
                    let private_key = private_key.clone();
                    let rep_system = rep_system.clone();
                    tokio::spawn(async move {
                        let event = ReputationEvent::new(
                            format!("event_{}_{}", peer_id, SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_millis()),
                            peer_id.clone(),
                            "local_node".to_string(),
                            EventType::FileTransferFailure,
                            failure_data,
                            1.0,
                        );

                        let mut rep_system = rep_system.lock().await;
                        if let Err(e) = rep_system.add_reputation_event(event).await {
                            warn!("Failed to add reputation event for peer {}: {}", peer_id, e);
                        } else {
                            debug!("Created reputation event for peer {}: FileTransferFailure", peer_id);
                        }
                    });
                } else {
                    warn!("No active private key available for reputation event creation");
                }
            }
        }
    }

    /// Record a successful connection to a peer
    pub fn record_connection_established(&mut self, peer_id: &str, address: &str) {
        // Update or create peer metrics
        if let Some(metrics) = self.metrics.get_mut(peer_id) {
            metrics.last_seen = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        } else {
            let new_metrics = PeerMetrics::new(peer_id.to_string(), address.to_string());
            self.metrics.insert(peer_id.to_string(), new_metrics);
        }

        // Create blockchain reputation event for connection
        let connection_data = serde_json::json!({
            "address": address,
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        });

        // Spawn async task for reputation event creation
        if let Some(ref rep_system) = self.reputation_system {
            if let Some(ref private_key) = self.active_private_key {
                let peer_id = peer_id.to_string();
                let private_key = private_key.clone();
                let rep_system = rep_system.clone();
                tokio::spawn(async move {
                    let event = ReputationEvent::new(
                        format!("event_{}_{}", peer_id, SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis()),
                        peer_id.clone(),
                        "local_node".to_string(),
                        EventType::ConnectionEstablished,
                        connection_data,
                        1.0,
                    );

                    let mut rep_system = rep_system.lock().await;
                    if let Err(e) = rep_system.add_reputation_event(event).await {
                        warn!("Failed to add reputation event for peer {}: {}", peer_id, e);
                    } else {
                        debug!("Created reputation event for peer {}: ConnectionEstablished", peer_id);
                    }
                });
            } else {
                warn!("No active private key available for reputation event creation");
            }
        }

        info!("Recorded connection established for peer {} at {}", peer_id, address);
    }

    /// Record a lost connection to a peer
    pub fn record_connection_lost(&mut self, peer_id: &str, reason: &str) {
        // Create blockchain reputation event for connection loss
        let connection_data = serde_json::json!({
            "reason": reason,
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        });

        // Spawn async task for reputation event creation
        if let Some(ref rep_system) = self.reputation_system {
            if let Some(ref private_key) = self.active_private_key {
                let peer_id = peer_id.to_string();
                let private_key = private_key.clone();
                let rep_system = rep_system.clone();
                tokio::spawn(async move {
                    let event = ReputationEvent::new(
                        format!("event_{}_{}", peer_id, SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis()),
                        peer_id.clone(),
                        "local_node".to_string(),
                        EventType::ConnectionLost,
                        connection_data,
                        1.0,
                    );

                    let mut rep_system = rep_system.lock().await;
                    if let Err(e) = rep_system.add_reputation_event(event).await {
                        warn!("Failed to add reputation event for peer {}: {}", peer_id, e);
                    } else {
                        debug!("Created reputation event for peer {}: ConnectionLost", peer_id);
                    }
                });
            } else {
                warn!("No active private key available for reputation event creation");
            }
        }

        warn!("Recorded connection lost for peer {}: {}", peer_id, reason);
    }

    /// Update latency for a peer
    pub fn update_peer_latency(&mut self, peer_id: &str, latency_ms: u64) {
        if let Some(metrics) = self.metrics.get_mut(peer_id) {
            metrics.update_latency(latency_ms);
        } else {
            // Create new peer metrics if not exists
            let mut new_metrics = PeerMetrics::new(peer_id.to_string(), "unknown".to_string());
            new_metrics.update_latency(latency_ms);
            self.metrics.insert(peer_id.to_string(), new_metrics);
        }
    }

    /// Set encryption support for a peer
    pub fn set_peer_encryption_support(&mut self, peer_id: &str, supported: bool) {
        if let Some(metrics) = self.metrics.get_mut(peer_id) {
            metrics.set_encryption_support(supported);
        }
    }

    /// Report malicious behavior for a peer
    pub fn report_malicious_peer(&mut self, peer_id: &str, severity: &str) {
        if let Some(metrics) = self.metrics.get_mut(peer_id) {
            metrics.report_malicious_behavior(severity);
            warn!(
                "Reported malicious behavior for peer {}: severity={}",
                peer_id, severity
            );
        } else {
            warn!(
                "Cannot report malicious behavior for unknown peer: {}",
                peer_id
            );
        }
    }

    /// Select the best peers for a given strategy (synchronous version for backward compatibility)
    pub fn select_peers_sync(
        &mut self,
        available_peers: &[String],
        count: usize,
        strategy: SelectionStrategy,
        require_encryption: bool,
    ) -> Vec<String> {
        // For blockchain reputation strategy, use default neutral scores
        let adjusted_strategy = match strategy {
            SelectionStrategy::BlockchainReputation => {
                warn!("Blockchain reputation strategy requires async select_peers, falling back to balanced strategy");
                SelectionStrategy::Balanced
            }
            _ => strategy,
        };

        // Use the async version with a simple runtime
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            self.select_peers_async(available_peers, count, adjusted_strategy, require_encryption).await
        })
    }

    /// Select the best peers for a given strategy (async version with blockchain reputation support)
    pub async fn select_peers_async(
        &mut self,
        available_peers: &[String],
        count: usize,
        strategy: SelectionStrategy,
        require_encryption: bool,
    ) -> Vec<String> {
        if available_peers.is_empty() || count == 0 {
            return Vec::new();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Clean up expired cache entries
        self.cleanup_reputation_cache();

        // Filter peers based on requirements and calculate scores
        let mut candidates: Vec<(String, f64)> = Vec::new();
        
        for peer_id in available_peers {
            if let Some(metrics) = self.metrics.get(peer_id) {
                // Skip if encryption required but not supported
                if require_encryption && !metrics.encryption_support {
                    continue;
                }

                // Calculate selection score based on strategy
                let score = match strategy {
                    SelectionStrategy::FastestFirst => metrics
                        .latency_ms
                        .map(|lat| 1000.0 - lat.min(1000) as f64)
                        .unwrap_or(0.0),
                    SelectionStrategy::MostReliable => metrics.reliability_score * 1000.0,
                    SelectionStrategy::HighestBandwidth => {
                        metrics.bandwidth_kbps.unwrap_or(0) as f64
                    }
                    SelectionStrategy::Balanced => {
                        metrics.get_quality_score(false) * 1000.0
                    }
                    SelectionStrategy::EncryptionPreferred => {
                        let base = metrics.get_quality_score(true) * 1000.0;
                        if metrics.encryption_support {
                            base + 100.0
                        } else {
                            base
                        }
                    }
                    SelectionStrategy::LoadBalanced => {
                        let base_score = metrics.get_quality_score(false) * 1000.0;
                        // Penalize recently selected peers to distribute load
                        let last_selected =
                            self.selection_history.get(peer_id).unwrap_or(&0);
                        let time_since_selected = now.saturating_sub(*last_selected);
                        let recency_penalty =
                            if time_since_selected < 60 { 50.0 } else { 0.0 };
                        base_score - recency_penalty
                    }
                    SelectionStrategy::BlockchainReputation => {
                        // Get blockchain reputation score
                        match self.get_blockchain_reputation_score(peer_id).await {
                            Ok(blockchain_score) => {
                                // Combine blockchain reputation with local metrics
                                let local_score = metrics.get_quality_score(false) * 1000.0;
                                let reputation_weight = 0.7; // 70% blockchain reputation, 30% local metrics
                                (blockchain_score * 1000.0 * reputation_weight) + (local_score * (1.0 - reputation_weight))
                            }
                            Err(e) => {
                                warn!("Failed to get blockchain reputation for peer {}: {}", peer_id, e);
                                // Fallback to local metrics only
                                metrics.get_quality_score(false) * 1000.0
                            }
                        }
                    }
                };

                candidates.push((peer_id.clone(), score));
            }
        }

        // Sort by score (descending)
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Select top candidates
        let selected: Vec<String> = candidates
            .into_iter()
            .take(count)
            .map(|(peer_id, _score)| {
                // Record selection time for load balancing
                self.selection_history.insert(peer_id.clone(), now);
                peer_id
            })
            .collect();

        info!(
            "Selected {} peers using strategy {:?}: {:?}",
            selected.len(),
            strategy,
            selected
        );
        selected
    }

    /// Select the best peers for a given strategy (backward compatibility wrapper)
    pub fn select_peers(
        &mut self,
        available_peers: &[String],
        count: usize,
        strategy: SelectionStrategy,
        require_encryption: bool,
    ) -> Vec<String> {
        self.select_peers_sync(available_peers, count, strategy, require_encryption)
    }

    /// Get all peer metrics for monitoring/debugging
    pub fn get_all_metrics(&self) -> Vec<PeerMetrics> {
        self.metrics.values().cloned().collect()
    }

    /// Get blockchain reputation score for a peer (for display purposes)
    pub async fn get_peer_blockchain_reputation(&self, peer_id: &str) -> Result<f64, String> {
        if let Some(ref rep_system) = self.reputation_system {
            let rep_system = rep_system.clone();
            let peer_id = peer_id.to_string();
            
            let score = tokio::spawn(async move {
                let rep_system = rep_system.lock().await;
                rep_system.get_peer_reputation_score(&peer_id, None).await
            }).await.map_err(|e| format!("Failed to get blockchain reputation score: {}", e))?;

            score
        } else {
            Err("Reputation system not available".to_string())
        }
    }

    /// Get cached blockchain reputation scores for all peers
    pub fn get_cached_reputation_scores(&self) -> HashMap<String, f64> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.blockchain_reputation_cache
            .iter()
            .filter(|(_, (_, timestamp))| now - timestamp < self.cache_duration_seconds)
            .map(|(peer_id, (score, _))| (peer_id.clone(), *score))
            .collect()
    }

    /// Get metrics for a specific peer
    pub fn get_peer_metrics(&self, peer_id: &str) -> Option<&PeerMetrics> {
        self.metrics.get(peer_id)
    }

    /// Remove inactive peers (haven't been seen for a while)
    pub fn cleanup_inactive_peers(&mut self, max_age_seconds: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let before_count = self.metrics.len();

        self.metrics
            .retain(|_peer_id, metrics| now.saturating_sub(metrics.last_seen) < max_age_seconds);

        let removed_count = before_count - self.metrics.len();
        if removed_count > 0 {
            info!("Cleaned up {} inactive peers", removed_count);
        }
    }

    /// Get peer recommendation for file transfer
    pub fn recommend_peers_for_file(
        &mut self,
        available_peers: &[String],
        file_size: u64,
        encryption_required: bool,
    ) -> Vec<String> {
        let strategy = if encryption_required {
            SelectionStrategy::EncryptionPreferred
        } else if file_size > 100_000_000 {
            // > 100MB, prefer bandwidth
            SelectionStrategy::HighestBandwidth
        } else {
            // Small files, prefer low latency
            SelectionStrategy::Balanced
        };

        // For large files, select more peers for parallel download
        let peer_count = if file_size > 50_000_000 {
            (available_peers.len().min(5)).max(1)
        } else {
            (available_peers.len().min(2)).max(1)
        };

        self.select_peers(available_peers, peer_count, strategy, encryption_required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_metrics_creation() {
        let metrics = PeerMetrics::new("test_peer".to_string(), "127.0.0.1:8080".to_string());
        assert_eq!(metrics.peer_id, "test_peer");
        assert_eq!(metrics.success_rate, 0.5);
        assert_eq!(metrics.transfer_count, 0);
    }

    #[test]
    fn test_successful_transfer_recording() {
        let mut metrics = PeerMetrics::new("test_peer".to_string(), "127.0.0.1:8080".to_string());
        metrics.record_successful_transfer(1000, 100); // 1KB in 100ms

        assert_eq!(metrics.transfer_count, 1);
        assert_eq!(metrics.successful_transfers, 1);
        assert_eq!(metrics.success_rate, 1.0);
        assert!(metrics.bandwidth_kbps.is_some());
    }

    #[test]
    fn test_peer_selection_service() {
        let mut service = PeerSelectionService::new();

        // Add test peers
        let mut peer1 = PeerMetrics::new("peer1".to_string(), "127.0.0.1:8080".to_string());
        peer1.latency_ms = Some(50);
        peer1.reliability_score = 0.9;

        let mut peer2 = PeerMetrics::new("peer2".to_string(), "127.0.0.1:8081".to_string());
        peer2.latency_ms = Some(200);
        peer2.reliability_score = 0.7;

        service.update_peer_metrics(peer1);
        service.update_peer_metrics(peer2);

        let available = vec!["peer1".to_string(), "peer2".to_string()];
        let selected = service.select_peers(&available, 1, SelectionStrategy::FastestFirst, false);

        assert_eq!(selected[0], "peer1"); // Should select peer with lower latency
    }

    #[test]
    fn test_encryption_filtering() {
        let mut service = PeerSelectionService::new();

        let mut peer1 = PeerMetrics::new("peer1".to_string(), "127.0.0.1:8080".to_string());
        peer1.encryption_support = true;

        let mut peer2 = PeerMetrics::new("peer2".to_string(), "127.0.0.1:8081".to_string());
        peer2.encryption_support = false;

        service.update_peer_metrics(peer1);
        service.update_peer_metrics(peer2);

        let available = vec!["peer1".to_string(), "peer2".to_string()];
        let selected = service.select_peers(&available, 2, SelectionStrategy::Balanced, true);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0], "peer1"); // Only peer with encryption support
    }
}
