use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::command;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PeerMetrics {
    pub peer_id: String,
    pub success_count: usize,
    pub failure_count: usize,
    pub avg_response_time: f64,
    pub last_response_time: u64,
    pub consecutive_failures: usize,
    pub backoff_until: u128,
    pub bandwidth: f64,
    pub last_seen: u128,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HealthDecision {
    pub should_use: bool,
    pub reason: String,
    pub weight: f64,
    pub max_concurrent: usize,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HealthConfig {
    pub max_failure_rate: f64,
    pub min_response_time: u64,
    pub max_response_time: u64,
    pub backoff_base_ms: u64,
    pub backoff_multiplier: f64,
    pub max_backoff_ms: u64,
    pub offline_threshold_ms: u64,
    pub min_bandwidth: f64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            max_failure_rate: 0.3,
            min_response_time: 50,
            max_response_time: 30_000,
            backoff_base_ms: 1_000,
            backoff_multiplier: 2.0,
            max_backoff_ms: 300_000,
            offline_threshold_ms: 60_000,
            min_bandwidth: 1024.0,
        }
    }
}

pub struct PeerHealthManager {
    config: HealthConfig,
    peers: HashMap<String, PeerMetrics>,
}

impl PeerHealthManager {
    pub fn new(cfg: Option<HealthConfig>) -> Self {
        Self {
            config: cfg.unwrap_or_default(),
            peers: HashMap::new(),
        }
    }

    pub fn init_peer(&mut self, peer_id: String) {
        if self.peers.contains_key(&peer_id) {
            return;
        }
        self.peers.insert(
            peer_id.clone(),
            PeerMetrics {
                peer_id,
                success_count: 0,
                failure_count: 0,
                avg_response_time: 1000.0,
                last_response_time: 0,
                consecutive_failures: 0,
                backoff_until: 0,
                bandwidth: 10_240.0,
                last_seen: now_ms(),
            },
        );
    }

    pub fn record_success(&mut self, peer_id: String, response_time_ms: u64, bytes_transferred: usize) {
        if !self.peers.contains_key(&peer_id) {
            self.init_peer(peer_id.clone());
        }
        if let Some(m) = self.peers.get_mut(&peer_id) {
            m.success_count = m.success_count.saturating_add(1);
            m.consecutive_failures = 0;
            m.backoff_until = 0;
            m.last_seen = now_ms();
            m.last_response_time = response_time_ms;

            // EMA for response time
            let alpha = 0.2_f64;
            let rt = response_time_ms as f64;
            if rt.is_finite() {
                m.avg_response_time = m.avg_response_time * (1.0 - alpha) + rt * alpha;
            }

            // bandwidth estimate bytes/sec
            if response_time_ms > 0 && bytes_transferred > 0 {
                let bw = (bytes_transferred as f64) * 1000.0 / (response_time_ms as f64);
                if bw.is_finite() {
                    m.bandwidth = m.bandwidth * (1.0 - alpha) + bw * alpha;
                }
            }
        }
    }

    pub fn record_failure(&mut self, peer_id: String, _reason: String) {
        if !self.peers.contains_key(&peer_id) {
            self.init_peer(peer_id.clone());
        }
        if let Some(m) = self.peers.get_mut(&peer_id) {
            m.failure_count = m.failure_count.saturating_add(1);
            m.consecutive_failures = m.consecutive_failures.saturating_add(1);
            m.last_seen = now_ms();

            // exponential backoff
            let exp = (self.config.backoff_multiplier).powf((m.consecutive_failures.saturating_sub(1)) as f64);
            let base = self.config.backoff_base_ms as f64;
            let mut backoff = (base * exp) as u128;
            if backoff as u64 > self.config.max_backoff_ms {
                backoff = self.config.max_backoff_ms as u128;
            }
            m.backoff_until = now_ms() + backoff;
        }
    }

    pub fn get_health_decision(&self, peer_id: String) -> HealthDecision {
        match self.peers.get(&peer_id) {
            None => HealthDecision {
                should_use: true,
                reason: "healthy".to_string(),
                weight: 0.5,
                max_concurrent: 1,
            },
            Some(m) => {
                let now = now_ms();
                if now.saturating_sub(m.last_seen) > (self.config.offline_threshold_ms as u128) {
                    return HealthDecision {
                        should_use: false,
                        reason: "offline".to_string(),
                        weight: 0.0,
                        max_concurrent: 0,
                    };
                }

                let total: usize = m.success_count + m.failure_count;
                let failure_rate = if total > 0 { (m.failure_count as f64) / (total as f64) } else { 0.0 };

                if total >= 5 && failure_rate > self.config.max_failure_rate {
                    return HealthDecision {
                        should_use: false,
                        reason: "unreliable".to_string(),
                        weight: 0.1,
                        max_concurrent: 1,
                    };
                }

                if now < m.backoff_until {
                    return HealthDecision {
                        should_use: false,
                        reason: "backoff".to_string(),
                        weight: 0.0,
                        max_concurrent: 0,
                    };
                }

                if (m.avg_response_time as u64) > self.config.max_response_time {
                    return HealthDecision {
                        should_use: false,
                        reason: "too-slow".to_string(),
                        weight: 0.2,
                        max_concurrent: 1,
                    };
                }

                if m.bandwidth < self.config.min_bandwidth {
                    return HealthDecision {
                        should_use: true,
                        reason: "too-slow".to_string(),
                        weight: 0.3,
                        max_concurrent: 1,
                    };
                }

                // compute weight
                let response_weight = ((self.config.max_response_time as f64 - m.avg_response_time) / (self.config.max_response_time as f64)).max(0.1);
                let reliability_weight = (1.0 - failure_rate).max(0.1);
                let bandwidth_weight = (m.bandwidth / (self.config.min_bandwidth * 10.0)).min(1.0);

                let mut weight = (response_weight + reliability_weight + bandwidth_weight) / 3.0;
                if !weight.is_finite() {
                    weight = 0.5;
                }
                weight = weight.clamp(0.0, 1.0);

                let max_concurrent = std::cmp::max(1, (weight * 5.0).floor() as usize);

                HealthDecision {
                    should_use: true,
                    reason: "healthy".to_string(),
                    weight,
                    max_concurrent,
                }
            }
        }
    }

    pub fn get_peer_metrics(&self, peer_id: String) -> Option<PeerMetrics> {
        self.peers.get(&peer_id).cloned()
    }

    pub fn get_all_healthy_peers(&self) -> Vec<(String, HealthDecision)> {
        let mut out = Vec::new();
        for (id, _) in self.peers.iter() {
            let dec = self.get_health_decision(id.clone());
            if dec.should_use {
                out.push((id.clone(), dec));
            }
        }
        out.sort_by(|a, b| b.1.weight.partial_cmp(&a.1.weight).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    pub fn select_peer(&self, exclude: Option<Vec<String>>) -> Option<String> {
        let exclude_set: std::collections::HashSet<String> = exclude.unwrap_or_default().into_iter().collect();
        let healthy = self.get_all_healthy_peers().into_iter().filter(|(id, _)| !exclude_set.contains(id)).collect::<Vec<_>>();
        if healthy.is_empty() { return None; }

        let total: f64 = healthy.iter().map(|(_, d)| d.weight).sum();
        if total == 0.0 { return Some(healthy[0].0.clone()); }

        let mut r = rand::thread_rng().gen_range(0.0..total);
        for (id, d) in healthy {
            if r <= d.weight { return Some(id); }
            r -= d.weight;
        }
        Some(healthy[0].0.clone())
    }

    pub fn cleanup(&mut self, max_age_ms: u128) -> usize {
        let now = now_ms();
        let mut removed = Vec::new();
        for (id, m) in self.peers.iter() {
            if now.saturating_sub(m.last_seen) > max_age_ms {
                removed.push(id.clone());
            }
        }
        for id in removed.iter() { self.peers.remove(id); }
        removed.len()
    }

    pub fn get_stats(&self) -> serde_json::Value {
        let peers: Vec<&PeerMetrics> = self.peers.values().collect();
        let total = peers.len();
        let healthy = self.get_all_healthy_peers().len();
        let avg_failure_rate = if total > 0 {
            peers.iter().map(|p| {
                let tot = p.success_count + p.failure_count;
                if tot == 0 { 0.0 } else { (p.failure_count as f64) / (tot as f64) }
            }).sum::<f64>() / (total as f64)
        } else { 0.0 };
        let avg_response_time = if total > 0 {
            peers.iter().map(|p| p.avg_response_time).sum::<f64>() / (total as f64)
        } else { 0.0 };
        let total_bandwidth = peers.iter().map(|p| p.bandwidth).sum::<f64>();

        serde_json::json!({
            "totalPeers": total,
            "healthyPeers": healthy,
            "avgFailureRate": avg_failure_rate,
            "avgResponseTime": avg_response_time,
            "totalBandwidth": total_bandwidth,
        })
    }
}

// Global manager instance
static PEER_HEALTH: Lazy<Mutex<PeerHealthManager>> = Lazy::new(|| Mutex::new(PeerHealthManager::new(None)));

#[command]
pub fn ph_init_peer(peer_id: String) -> Result<(), String> {
    let mut s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    s.init_peer(peer_id);
    Ok(())
}

#[command]
pub fn ph_record_success(peer_id: String, response_time_ms: u64, bytes_transferred: usize) -> Result<(), String> {
    let mut s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    s.record_success(peer_id, response_time_ms, bytes_transferred);
    Ok(())
}

#[command]
pub fn ph_record_failure(peer_id: String, reason: String) -> Result<(), String> {
    let mut s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    s.record_failure(peer_id, reason);
    Ok(())
}

#[command]
pub fn ph_get_health_decision(peer_id: String) -> Result<HealthDecision, String> {
    let s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_health_decision(peer_id))
}

#[command]
pub fn ph_get_peer_metrics(peer_id: String) -> Result<Option<PeerMetrics>, String> {
    let s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_peer_metrics(peer_id))
}

#[command]
pub fn ph_get_all_healthy_peers() -> Result<Vec<(String, HealthDecision)>, String> {
    let s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_all_healthy_peers())
}

#[command]
pub fn ph_select_peer(exclude: Option<Vec<String>>) -> Result<Option<String>, String> {
    let s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.select_peer(exclude))
}

#[command]
pub fn ph_cleanup(max_age_ms: u128) -> Result<usize, String> {
    let mut s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.cleanup(max_age_ms))
}

#[command]
pub fn ph_get_stats() -> Result<serde_json::Value, String> {
    let s = PEER_HEALTH.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_stats())
}
