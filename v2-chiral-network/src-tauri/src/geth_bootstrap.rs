//! Ethereum/Geth Bootstrap Node Management
//!
//! Provides robust bootstrap node management for Geth networking:
//! - Health checking with retry logic and exponential backoff
//! - Dynamic bootstrap node selection based on latency
//! - Cached results for efficient repeated access
//! - Environment variable override for custom bootstrap nodes

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// ============================================================================
// Configuration
// ============================================================================

/// Default timeout for TCP health checks (seconds)
const HEALTH_CHECK_TIMEOUT_SECS: u64 = 5;

/// Maximum retry attempts for health checks
const MAX_RETRIES: u32 = 2;

/// Initial retry delay (milliseconds)
const INITIAL_RETRY_DELAY_MS: u64 = 300;

/// Maximum retry delay (milliseconds)
const MAX_RETRY_DELAY_MS: u64 = 2000;

/// How long to cache health check results (seconds)
const CACHE_TTL_SECS: u64 = 60;

/// Minimum number of healthy nodes required
const MIN_HEALTHY_NODES: usize = 1;

// ============================================================================
// Data Structures
// ============================================================================

/// A bootstrap node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapNode {
    /// Full enode URL
    pub enode: String,
    /// Human-readable description
    pub name: String,
    /// Geographic region
    pub region: String,
    /// Priority (lower = higher priority)
    pub priority: u8,
}

/// Health check result for a single node
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeHealth {
    pub enode: String,
    pub name: String,
    pub region: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
    pub last_checked: u64,
}

/// Overall bootstrap health report
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapHealthReport {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub nodes: Vec<NodeHealth>,
    pub timestamp: u64,
    pub is_healthy: bool,
    pub healthy_enode_string: String,
}

/// Cached bootstrap state
struct BootstrapCache {
    report: Option<BootstrapHealthReport>,
    last_updated: Option<Instant>,
}

impl Default for BootstrapCache {
    fn default() -> Self {
        Self {
            report: None,
            last_updated: None,
        }
    }
}

/// Global bootstrap cache
static CACHE: Lazy<Arc<RwLock<BootstrapCache>>> =
    Lazy::new(|| Arc::new(RwLock::new(BootstrapCache::default())));

// ============================================================================
// Bootstrap Node Registry
// ============================================================================

/// Get the default bootstrap nodes for Chiral Network
pub fn get_default_nodes() -> Vec<BootstrapNode> {
    vec![
        BootstrapNode {
            enode: "enode://45cc5ba89142b2c82180986f411aa16dbfe6041043d1f7112f08e710f23fdeb7283551ec15ca9d23a0da91ac12e080e014f8c32230a8109d6d0b01be8ca71102@130.245.173.73:30303".into(),
            name: "Primary Bootstrap (Chiral Test)".into(),
            region: "US East".into(),
            priority: 1,
        },
    ]
}

/// Get bootstrap nodes, checking for environment variable override first
pub fn get_nodes() -> Vec<BootstrapNode> {
    // Check for custom bootstrap nodes via environment variable
    if let Ok(custom) = std::env::var("CHIRAL_BOOTSTRAP_NODES") {
        let nodes: Vec<BootstrapNode> = custom
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .enumerate()
            .map(|(i, enode)| BootstrapNode {
                enode: enode.trim().to_string(),
                name: format!("Custom Node {}", i + 1),
                region: "Unknown".into(),
                priority: i as u8,
            })
            .collect();

        if !nodes.is_empty() {
            info!("Using {} custom bootstrap nodes from CHIRAL_BOOTSTRAP_NODES", nodes.len());
            return nodes;
        }
    }

    get_default_nodes()
}

// ============================================================================
// Health Checking
// ============================================================================

/// Parse IP and port from an enode URL
fn parse_enode(enode: &str) -> Result<(String, u16), String> {
    // Format: enode://[node_id]@[ip]:[port]
    let parts: Vec<&str> = enode.split('@').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid enode format: {}", enode));
    }

    // Handle query params like ?discport=30304
    let addr = parts[1].split('?').next().unwrap_or(parts[1]);
    let addr_parts: Vec<&str> = addr.split(':').collect();

    if addr_parts.len() != 2 {
        return Err(format!("Invalid address in enode: {}", addr));
    }

    let ip = addr_parts[0].to_string();
    let port = addr_parts[1]
        .parse::<u16>()
        .map_err(|e| format!("Invalid port: {}", e))?;

    Ok((ip, port))
}

/// Check health of a single node with retry logic
async fn check_node_health(node: &BootstrapNode) -> NodeHealth {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (ip, port) = match parse_enode(&node.enode) {
        Ok(addr) => addr,
        Err(e) => {
            return NodeHealth {
                enode: node.enode.clone(),
                name: node.name.clone(),
                region: node.region.clone(),
                reachable: false,
                latency_ms: None,
                error: Some(e),
                last_checked: now,
            };
        }
    };

    let mut attempts = 0;
    let mut delay = INITIAL_RETRY_DELAY_MS;
    let mut last_error = String::new();

    while attempts <= MAX_RETRIES {
        attempts += 1;
        let start = Instant::now();

        let connect_result = tokio::time::timeout(
            Duration::from_secs(HEALTH_CHECK_TIMEOUT_SECS),
            tokio::net::TcpStream::connect(format!("{}:{}", ip, port)),
        )
        .await;

        match connect_result {
            Ok(Ok(_stream)) => {
                let latency = start.elapsed().as_millis() as u64;
                debug!("Bootstrap node {} reachable ({}ms)", node.name, latency);

                return NodeHealth {
                    enode: node.enode.clone(),
                    name: node.name.clone(),
                    region: node.region.clone(),
                    reachable: true,
                    latency_ms: Some(latency),
                    error: None,
                    last_checked: now,
                };
            }
            Ok(Err(e)) => {
                last_error = format!("Connection failed: {}", e);
            }
            Err(_) => {
                last_error = format!("Timeout ({}s)", HEALTH_CHECK_TIMEOUT_SECS);
            }
        }

        // Retry with exponential backoff
        if attempts <= MAX_RETRIES {
            debug!(
                "Bootstrap node {} check failed (attempt {}/{}), retrying in {}ms",
                node.name, attempts, MAX_RETRIES + 1, delay
            );
            tokio::time::sleep(Duration::from_millis(delay)).await;
            delay = (delay * 2).min(MAX_RETRY_DELAY_MS);
        }
    }

    warn!("Bootstrap node {} unreachable: {}", node.name, last_error);

    NodeHealth {
        enode: node.enode.clone(),
        name: node.name.clone(),
        region: node.region.clone(),
        reachable: false,
        latency_ms: None,
        error: Some(last_error),
        last_checked: now,
    }
}

/// Check health of all bootstrap nodes concurrently
pub async fn check_all_nodes() -> BootstrapHealthReport {
    let nodes = get_nodes();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Check all nodes concurrently
    let health_futures: Vec<_> = nodes.iter().map(check_node_health).collect();
    let mut results = futures::future::join_all(health_futures).await;

    // Sort by latency (healthy nodes first, then by latency)
    results.sort_by(|a, b| {
        match (a.reachable, b.reachable) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (true, true) => a.latency_ms.cmp(&b.latency_ms),
            (false, false) => std::cmp::Ordering::Equal,
        }
    });

    let healthy_count = results.iter().filter(|n| n.reachable).count();
    let is_healthy = healthy_count >= MIN_HEALTHY_NODES;

    // Build enode string from healthy nodes only
    let healthy_enodes: Vec<String> = results
        .iter()
        .filter(|n| n.reachable)
        .map(|n| n.enode.clone())
        .collect();

    let healthy_enode_string = if healthy_enodes.is_empty() {
        // Fallback to all nodes if none are healthy
        warn!("No healthy bootstrap nodes found, falling back to all nodes");
        nodes.iter().map(|n| n.enode.clone()).collect::<Vec<_>>().join(",")
    } else {
        info!("Found {} healthy bootstrap nodes", healthy_count);
        healthy_enodes.join(",")
    };

    let report = BootstrapHealthReport {
        total_nodes: results.len(),
        healthy_nodes: healthy_count,
        nodes: results,
        timestamp: now,
        is_healthy,
        healthy_enode_string,
    };

    // Update cache
    {
        let mut cache = CACHE.write().await;
        cache.report = Some(report.clone());
        cache.last_updated = Some(Instant::now());
    }

    report
}

/// Get healthy bootstrap enode string, using cache if fresh
pub async fn get_healthy_enodes() -> String {
    // Check cache first
    {
        let cache = CACHE.read().await;
        if let (Some(report), Some(updated)) = (&cache.report, cache.last_updated) {
            if updated.elapsed() < Duration::from_secs(CACHE_TTL_SECS) {
                debug!("Using cached bootstrap enodes");
                return report.healthy_enode_string.clone();
            }
        }
    }

    // Cache miss or stale - perform fresh check
    let report = check_all_nodes().await;
    report.healthy_enode_string
}

/// Get cached health report without performing new check
pub async fn get_cached_report() -> Option<BootstrapHealthReport> {
    let cache = CACHE.read().await;
    cache.report.clone()
}

/// Clear the cache (useful for forcing fresh health check)
pub async fn clear_cache() {
    let mut cache = CACHE.write().await;
    cache.report = None;
    cache.last_updated = None;
}

/// Get all bootstrap enodes without health checking (synchronous fallback)
pub fn get_all_enodes() -> String {
    get_nodes()
        .iter()
        .map(|n| n.enode.clone())
        .collect::<Vec<_>>()
        .join(",")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enode_valid() {
        let enode = "enode://abc123@192.168.1.1:30303";
        let result = parse_enode(enode);
        assert!(result.is_ok());
        let (ip, port) = result.unwrap();
        assert_eq!(ip, "192.168.1.1");
        assert_eq!(port, 30303);
    }

    #[test]
    fn test_parse_enode_with_query() {
        let enode = "enode://abc123@192.168.1.1:30303?discport=30304";
        let result = parse_enode(enode);
        assert!(result.is_ok());
        let (ip, port) = result.unwrap();
        assert_eq!(ip, "192.168.1.1");
        assert_eq!(port, 30303);
    }

    #[test]
    fn test_parse_enode_invalid() {
        assert!(parse_enode("invalid").is_err());
        assert!(parse_enode("enode://abc").is_err());
    }

    #[test]
    fn test_get_default_nodes() {
        let nodes = get_default_nodes();
        assert!(!nodes.is_empty());
        for node in &nodes {
            assert!(node.enode.starts_with("enode://"));
        }
    }

    #[test]
    fn test_get_all_enodes() {
        let enodes = get_all_enodes();
        assert!(!enodes.is_empty());
        assert!(enodes.contains("enode://"));
    }

    #[test]
    fn test_parse_enode_extracts_correct_port() {
        let enode = "enode://abc@10.0.0.1:30303";
        let (ip, port) = parse_enode(enode).unwrap();
        assert_eq!(ip, "10.0.0.1");
        assert_eq!(port, 30303);
    }

    #[test]
    fn test_parse_enode_missing_at_sign() {
        assert!(parse_enode("enode://abc123").is_err());
    }

    #[test]
    fn test_parse_enode_missing_port() {
        assert!(parse_enode("enode://abc@192.168.1.1").is_err());
    }

    #[test]
    fn test_parse_enode_invalid_port() {
        assert!(parse_enode("enode://abc@192.168.1.1:notaport").is_err());
    }

    #[test]
    fn test_default_nodes_have_required_fields() {
        for node in get_default_nodes() {
            assert!(!node.name.is_empty(), "Node name should not be empty");
            assert!(!node.region.is_empty(), "Node region should not be empty");
            assert!(node.enode.starts_with("enode://"), "Enode should start with enode://");
            assert!(node.enode.contains("@"), "Enode should contain @");
            assert!(node.enode.contains(":30303"), "Enode should contain port 30303");
        }
    }

    #[test]
    fn test_get_nodes_returns_defaults_without_env() {
        let nodes = get_nodes();
        let defaults = get_default_nodes();
        assert_eq!(nodes.len(), defaults.len());
    }

    #[test]
    fn test_bootstrap_node_priorities() {
        let nodes = get_default_nodes();
        assert_eq!(nodes[0].priority, 1);
    }

    #[test]
    fn test_node_health_serialization() {
        let health = NodeHealth {
            enode: "enode://abc@127.0.0.1:30303".to_string(),
            name: "Test Node".to_string(),
            region: "US East".to_string(),
            reachable: true,
            latency_ms: Some(42),
            error: None,
            last_checked: 1700000000,
        };
        let json = serde_json::to_string(&health).unwrap();
        assert!(json.contains("latencyMs"));
        assert!(json.contains("lastChecked"));
        let deserialized: NodeHealth = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "Test Node");
        assert_eq!(deserialized.latency_ms, Some(42));
    }

    #[test]
    fn test_bootstrap_health_report_serialization() {
        let report = BootstrapHealthReport {
            total_nodes: 2,
            healthy_nodes: 1,
            nodes: vec![],
            timestamp: 1700000000,
            is_healthy: true,
            healthy_enode_string: "enode://test@127.0.0.1:30303".to_string(),
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("totalNodes"));
        assert!(json.contains("healthyNodes"));
        assert!(json.contains("isHealthy"));
        assert!(json.contains("healthyEnodeString"));
    }

    #[test]
    fn test_get_all_enodes_comma_separated() {
        let enodes = get_all_enodes();
        let parts: Vec<&str> = enodes.split(',').collect();
        assert_eq!(parts.len(), get_default_nodes().len());
        for part in parts {
            assert!(part.starts_with("enode://"));
        }
    }
}
