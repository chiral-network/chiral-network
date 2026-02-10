use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Proxy latency information for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyLatencyInfo {
    pub proxy_id: String,
    pub latency_ms: Option<u64>,
    pub last_updated: u64, // timestamp
    pub status: ProxyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyStatus {
    Online,
    Offline,
    Connecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyOptimizationStatus {
    pub total_proxies: usize,
    pub online_proxies: usize,
    pub tested_proxies: usize,
    pub best_proxy_id: Option<String>,
    pub best_latency_ms: Option<u64>,
    pub average_latency_ms: Option<f64>,
    pub should_use_proxy_routing: bool,
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
            last_updated: now_secs(),
            status,
        };
        self.proxy_latencies.insert(proxy_id, info);
    }

    pub fn update_status(&mut self, proxy_id: String, status: ProxyStatus) {
        let latency = self
            .proxy_latencies
            .get(&proxy_id)
            .and_then(|entry| entry.latency_ms);
        self.update_proxy_latency(proxy_id, latency, status);
    }

    pub fn remove_proxy(&mut self, proxy_id: &str) -> bool {
        self.proxy_latencies.remove(proxy_id).is_some()
    }

    pub fn clear(&mut self) -> usize {
        let count = self.proxy_latencies.len();
        self.proxy_latencies.clear();
        count
    }

    pub fn len(&self) -> usize {
        self.proxy_latencies.len()
    }

    /// Get the best proxy based on latency
    pub fn get_best_proxy(&self) -> Option<ProxyLatencyInfo> {
        self.proxy_latencies
            .values()
            .filter(|info| matches!(info.status, ProxyStatus::Online))
            .filter(|info| info.latency_ms.is_some())
            .min_by_key(|info| info.latency_ms.unwrap_or(u64::MAX))
            .cloned()
    }

    /// Get all online proxies sorted by latency
    pub fn get_proxies_by_latency(&self) -> Vec<ProxyLatencyInfo> {
        let mut proxies: Vec<_> = self
            .proxy_latencies
            .values()
            .filter(|info| matches!(info.status, ProxyStatus::Online))
            .cloned()
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

    pub fn get_proxy(&self, proxy_id: &str) -> Option<ProxyLatencyInfo> {
        self.proxy_latencies.get(proxy_id).cloned()
    }

    pub fn get_snapshot(&self, limit: Option<usize>) -> Vec<ProxyLatencyInfo> {
        let cap = limit.unwrap_or(self.proxy_latencies.len());
        let mut entries: Vec<ProxyLatencyInfo> = self.proxy_latencies.values().cloned().collect();
        entries.sort_by(|a, b| {
            let rank = |s: &ProxyStatus| match s {
                ProxyStatus::Online => 0u8,
                ProxyStatus::Connecting => 1u8,
                ProxyStatus::Offline => 2u8,
                ProxyStatus::Error => 3u8,
            };
            rank(&a.status)
                .cmp(&rank(&b.status))
                .then_with(|| a.latency_ms.unwrap_or(u64::MAX).cmp(&b.latency_ms.unwrap_or(u64::MAX)))
                .then_with(|| b.last_updated.cmp(&a.last_updated))
        });
        entries.truncate(cap);
        entries
    }

    pub fn get_status(&self) -> ProxyOptimizationStatus {
        let total = self.proxy_latencies.len();
        let online = self
            .proxy_latencies
            .values()
            .filter(|p| matches!(p.status, ProxyStatus::Online))
            .count();
        let lat_samples: Vec<u64> = self
            .proxy_latencies
            .values()
            .filter_map(|p| p.latency_ms)
            .collect();
        let avg = if lat_samples.is_empty() {
            None
        } else {
            let sum: u128 = lat_samples.iter().map(|x| *x as u128).sum();
            Some(sum as f64 / lat_samples.len() as f64)
        };
        let best = self.get_best_proxy();

        ProxyOptimizationStatus {
            total_proxies: total,
            online_proxies: online,
            tested_proxies: lat_samples.len(),
            best_proxy_id: best.as_ref().map(|b| b.proxy_id.clone()),
            best_latency_ms: best.as_ref().and_then(|b| b.latency_ms),
            average_latency_ms: avg,
            should_use_proxy_routing: self.should_use_proxy_routing(),
        }
    }
}

impl Default for ProxyLatencyService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_best_online_proxy() {
        let mut svc = ProxyLatencyService::new();
        svc.update_proxy_latency("p-a".to_string(), Some(120), ProxyStatus::Online);
        svc.update_proxy_latency("p-b".to_string(), Some(45), ProxyStatus::Online);
        svc.update_proxy_latency("p-c".to_string(), Some(10), ProxyStatus::Offline);

        let best = svc.get_best_proxy().unwrap();
        assert_eq!(best.proxy_id, "p-b");
        assert_eq!(best.latency_ms, Some(45));
    }

    #[test]
    fn score_is_bounded() {
        let mut svc = ProxyLatencyService::new();
        svc.update_proxy_latency("p-a".to_string(), Some(10), ProxyStatus::Online);
        svc.update_proxy_latency("p-b".to_string(), Some(9999), ProxyStatus::Online);
        svc.update_proxy_latency("p-c".to_string(), None, ProxyStatus::Offline);

        let s_a = svc.get_proxy_score("p-a");
        let s_b = svc.get_proxy_score("p-b");
        let s_c = svc.get_proxy_score("p-c");
        assert!(s_a >= 0.0 && s_a <= 1.0);
        assert!(s_b >= 0.0 && s_b <= 1.0);
        assert_eq!(s_c, 0.0);
        assert!(s_a > s_b);
    }

    #[test]
    fn status_summary_reports_counts() {
        let mut svc = ProxyLatencyService::new();
        svc.update_proxy_latency("p-a".to_string(), Some(50), ProxyStatus::Online);
        svc.update_proxy_latency("p-b".to_string(), Some(70), ProxyStatus::Online);
        svc.update_proxy_latency("p-c".to_string(), None, ProxyStatus::Connecting);

        let status = svc.get_status();
        assert_eq!(status.total_proxies, 3);
        assert_eq!(status.online_proxies, 2);
        assert_eq!(status.tested_proxies, 2);
        assert_eq!(status.best_proxy_id.as_deref(), Some("p-a"));
        assert_eq!(status.best_latency_ms, Some(50));
        assert!(status.average_latency_ms.unwrap() > 0.0);
        assert!(status.should_use_proxy_routing);
    }

    #[test]
    fn snapshot_is_sorted_and_limited() {
        let mut svc = ProxyLatencyService::new();
        svc.update_proxy_latency("p-a".to_string(), Some(50), ProxyStatus::Online);
        svc.update_proxy_latency("p-b".to_string(), Some(20), ProxyStatus::Online);
        svc.update_proxy_latency("p-c".to_string(), None, ProxyStatus::Connecting);
        svc.update_proxy_latency("p-d".to_string(), None, ProxyStatus::Offline);

        let snap = svc.get_snapshot(Some(2));
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].proxy_id, "p-b");
        assert_eq!(snap[1].proxy_id, "p-a");
    }

    #[test]
    fn remove_proxy_works() {
        let mut svc = ProxyLatencyService::new();
        svc.update_proxy_latency("p-a".to_string(), Some(11), ProxyStatus::Online);
        assert!(svc.remove_proxy("p-a"));
        assert!(!svc.remove_proxy("p-a"));
        assert!(svc.get_proxy("p-a").is_none());
    }

    #[test]
    fn clear_and_len_work() {
        let mut svc = ProxyLatencyService::new();
        svc.update_proxy_latency("p-a".to_string(), Some(11), ProxyStatus::Online);
        svc.update_proxy_latency("p-b".to_string(), None, ProxyStatus::Offline);
        assert_eq!(svc.len(), 2);
        assert_eq!(svc.clear(), 2);
        assert_eq!(svc.len(), 0);
    }

    #[test]
    fn update_status_preserves_latency() {
        let mut svc = ProxyLatencyService::new();
        svc.update_proxy_latency("p-a".to_string(), Some(33), ProxyStatus::Online);
        svc.update_status("p-a".to_string(), ProxyStatus::Error);
        let p = svc.get_proxy("p-a").unwrap();
        assert_eq!(p.latency_ms, Some(33));
        assert!(matches!(p.status, ProxyStatus::Error));
    }
}
