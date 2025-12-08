use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Proxy latency information for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyLatencyInfo {
    pub proxy_id: String,
    pub latency_ms: Option<u64>,
    pub last_updated: u64, // timestamp
    pub status: ProxyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyStatus {
    Online,
    Offline,
    Connecting,
    Error,
}

/// Service for tracking and optimizing proxy latencies
pub struct ProxyLatencyService {
    proxy_latencies: HashMap<String, ProxyLatencyInfo>,
}

impl ProxyLatencyService {
    pub fn new() -> Self {
        Self {
            proxy_latencies: HashMap::new(),
        }
    }

    /// Update latency information for a proxy
    pub fn update_proxy_latency(
        &mut self,
        proxy_id: String,
        latency_ms: Option<u64>,
        status: ProxyStatus,
    ) {
        let info = ProxyLatencyInfo {
            proxy_id: proxy_id.clone(),
            latency_ms,
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            status,
        };
        self.proxy_latencies.insert(proxy_id, info);
    }

    /// Get the best proxy based on latency
    pub fn get_best_proxy(&self) -> Option<&ProxyLatencyInfo> {
        self.proxy_latencies
            .values()
            .filter(|info| matches!(info.status, ProxyStatus::Online))
            .filter(|info| info.latency_ms.is_some())
            .min_by_key(|info| info.latency_ms.unwrap_or(u64::MAX))
    }

    /// Get all online proxies sorted by latency
    pub fn get_proxies_by_latency(&self) -> Vec<&ProxyLatencyInfo> {
        let mut proxies: Vec<_> = self
            .proxy_latencies
            .values()
            .filter(|info| matches!(info.status, ProxyStatus::Online))
            .collect();

        proxies.sort_by_key(|info| info.latency_ms.unwrap_or(u64::MAX));
        proxies
    }

    /// Check if we should prefer proxy routing based on available proxies
    pub fn should_use_proxy_routing(&self) -> bool {
        self.get_best_proxy().is_some()
    }

    /// Get latency score for a proxy (lower is better)
    pub fn get_proxy_score(&self, proxy_id: &str) -> f64 {
        if let Some(info) = self.proxy_latencies.get(proxy_id) {
            match (&info.status, info.latency_ms) {
                (ProxyStatus::Online, Some(latency)) => {
                    // Convert latency to score (lower latency = higher score)
                    // Score range: 0.0 (worst) to 1.0 (best)
                    let max_acceptable_latency = 1000.0; // 1 second
                    (max_acceptable_latency - latency as f64).max(0.0) / max_acceptable_latency
                }
                (ProxyStatus::Online, None) => 0.5, // Unknown latency but online
                _ => 0.0,                           // Offline or error
            }
        } else {
            0.0 // No info available
        }
    }
}

impl Default for ProxyLatencyService {
    fn default() -> Self {
        Self::new()
    }
}
