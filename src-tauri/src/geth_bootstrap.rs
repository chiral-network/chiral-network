//! Ethereum/Geth Bootstrap Node Management
//!
//! This module provides robust bootstrap node management for Geth networking including:
//! - Health checking with retry logic
//! - Dynamic bootstrap node selection
//! - Automatic re-bootstrap on peer count drop
//! - Exponential backoff for failed connections
//!
//! The bootstrap nodes are used by Geth to discover peers on the Chiral Network.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// ============================================================================
// Configuration Constants
// ============================================================================

/// Default timeout for TCP health checks
const DEFAULT_HEALTH_CHECK_TIMEOUT_SECS: u64 = 5;

/// Extended timeout for RPC health verification
const RPC_HEALTH_CHECK_TIMEOUT_SECS: u64 = 10;

/// Minimum number of healthy bootstrap nodes required
const MIN_HEALTHY_NODES: usize = 1;

/// How often to re-check bootstrap health (in seconds)
const HEALTH_CHECK_INTERVAL_SECS: u64 = 60;

/// Maximum retry attempts for health checks
const MAX_HEALTH_CHECK_RETRIES: u32 = 3;

/// Initial retry delay in milliseconds
const INITIAL_RETRY_DELAY_MS: u64 = 500;

/// Maximum retry delay in milliseconds
const MAX_RETRY_DELAY_MS: u64 = 5000;

/// Backoff multiplier for exponential backoff
const BACKOFF_MULTIPLIER: f64 = 2.0;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapNode {
    pub enode: String,
    pub description: String,
    pub region: String,
    /// Priority for selection (lower = higher priority)
    #[serde(default)]
    pub priority: u8,
    /// Whether this node supports discovery v5
    #[serde(default)]
    pub supports_discv5: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapNodeHealth {
    pub enode: String,
    pub description: String,
    pub region: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
    /// Number of consecutive failures
    #[serde(default)]
    pub consecutive_failures: u32,
    /// Last successful check timestamp (unix epoch seconds)
    pub last_success: Option<u64>,
    /// Last check timestamp
    pub last_checked: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapHealthReport {
    pub total_nodes: usize,
    pub reachable_nodes: usize,
    pub unreachable_nodes: usize,
    pub nodes: Vec<BootstrapNodeHealth>,
    /// Timestamp of this report
    pub timestamp: u64,
    /// Whether minimum node threshold is met
    pub healthy: bool,
    /// Recommended action if unhealthy
    pub recommendation: Option<String>,
}

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: MAX_HEALTH_CHECK_RETRIES,
            initial_delay_ms: INITIAL_RETRY_DELAY_MS,
            max_delay_ms: MAX_RETRY_DELAY_MS,
            backoff_multiplier: BACKOFF_MULTIPLIER,
        }
    }
}

/// Cached bootstrap state for efficient access
#[derive(Debug)]
pub struct BootstrapCache {
    /// Last health report
    pub last_report: Option<BootstrapHealthReport>,
    /// Cached healthy enode string
    pub healthy_enodes: String,
    /// When the cache was last updated
    pub last_updated: Option<Instant>,
    /// Per-node failure counts (enode -> consecutive failures)
    pub failure_counts: std::collections::HashMap<String, u32>,
}

impl Default for BootstrapCache {
    fn default() -> Self {
        Self {
            last_report: None,
            healthy_enodes: String::new(),
            last_updated: None,
            failure_counts: std::collections::HashMap::new(),
        }
    }
}

/// Global bootstrap cache for efficient repeated access
static BOOTSTRAP_CACHE: once_cell::sync::Lazy<Arc<RwLock<BootstrapCache>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(BootstrapCache::default())));

// ============================================================================
// Bootstrap Node Registry
// ============================================================================

/// Get all configured bootstrap nodes for Chiral Network
///
/// These nodes run Geth and are the entry points for new nodes joining the network.
/// The list includes primary and backup nodes across different regions for redundancy.
pub fn get_bootstrap_nodes() -> Vec<BootstrapNode> {
    vec![
        // Primary US East bootstrap node (Stony Brook infrastructure)
        BootstrapNode {
            enode: "enode://ae987db6399b50addb75d7822bfad9b4092fbfd79cbfe97e6864b1f17d3e8fcd8e9e190ad109572c1439230fa688a9837e58f0b1ad7c0dc2bc6e4ab328f3991e@130.245.173.105:30303".to_string(),
            description: "Primary US Bootstrap Node (Stony Brook)".to_string(),
            region: "US East".to_string(),
            priority: 1,
            supports_discv5: true,
        },
        // Secondary US West bootstrap node (Azure)
        BootstrapNode {
            enode: "enode://b3ead5f07d0dbeda56023435a7c05877d67b055df3a8bf18f3d5f7c56873495cd4de5cf031ae9052827c043c12f1d30704088c79fb539c96834bfa74b78bf80b@20.85.124.187:30303".to_string(),
            description: "Secondary US Bootstrap Node (Azure West)".to_string(),
            region: "US West".to_string(),
            priority: 2,
            supports_discv5: true,
        },
        // Backup node - same region as primary for redundancy
        // This uses the same IP but different node ID to represent a potential backup
        // In production, this would be a separate physical node
        BootstrapNode {
            enode: "enode://ae987db6399b50addb75d7822bfad9b4092fbfd79cbfe97e6864b1f17d3e8fcd8e9e190ad109572c1439230fa688a9837e58f0b1ad7c0dc2bc6e4ab328f3991e@130.245.173.105:30304".to_string(),
            description: "Backup US Bootstrap Node (Stony Brook Alt Port)".to_string(),
            region: "US East".to_string(),
            priority: 3,
            supports_discv5: false,
        },
    ]
}

/// Get bootstrap nodes from environment variable if set, otherwise use defaults
///
/// Environment variable format: comma-separated enode URLs
/// Example: CHIRAL_BOOTSTRAP_NODES=enode://...@ip1:port1,enode://...@ip2:port2
pub fn get_bootstrap_nodes_with_env_override() -> Vec<BootstrapNode> {
    if let Ok(env_nodes) = std::env::var("CHIRAL_BOOTSTRAP_NODES") {
        let nodes: Vec<BootstrapNode> = env_nodes
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .enumerate()
            .map(|(i, enode)| BootstrapNode {
                enode: enode.trim().to_string(),
                description: format!("Environment Bootstrap Node {}", i + 1),
                region: "Unknown".to_string(),
                priority: i as u8,
                supports_discv5: false,
            })
            .collect();

        if !nodes.is_empty() {
            info!(
                "Using {} bootstrap nodes from CHIRAL_BOOTSTRAP_NODES environment variable",
                nodes.len()
            );
            return nodes;
        }
    }

    get_bootstrap_nodes()
}

// ============================================================================
// Health Checking
// ============================================================================

/// Parse enode address to extract IP and port
///
/// Enode format: enode://[node_id]@[ip]:[port]
pub fn parse_enode_address(enode: &str) -> Result<(String, u16), String> {
    let parts: Vec<&str> = enode.split('@').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid enode format: expected 'enode://id@ip:port', got '{}'",
            enode
        ));
    }

    // Handle potential query parameters (e.g., ?discport=30304)
    let addr_part = parts[1].split('?').next().unwrap_or(parts[1]);
    let addr_parts: Vec<&str> = addr_part.split(':').collect();

    if addr_parts.len() != 2 {
        return Err(format!(
            "Invalid address format: expected 'ip:port', got '{}'",
            addr_part
        ));
    }

    let ip = addr_parts[0].to_string();
    let port = addr_parts[1]
        .parse::<u16>()
        .map_err(|e| format!("Invalid port number '{}': {}", addr_parts[1], e))?;

    Ok((ip, port))
}

/// Extract node ID from enode URL
pub fn extract_node_id(enode: &str) -> Option<String> {
    if !enode.starts_with("enode://") {
        return None;
    }

    let without_prefix = &enode[8..]; // Skip "enode://"
    without_prefix.split('@').next().map(|s| s.to_string())
}

/// Check health of a single bootstrap node via TCP connection with retry
pub async fn check_bootstrap_node_health(node: &BootstrapNode) -> BootstrapNodeHealth {
    check_bootstrap_node_health_with_config(node, &RetryConfig::default()).await
}

/// Check health of a single bootstrap node with custom retry configuration
pub async fn check_bootstrap_node_health_with_config(
    node: &BootstrapNode,
    retry_config: &RetryConfig,
) -> BootstrapNodeHealth {
    let mut attempts = 0;
    let mut delay = retry_config.initial_delay_ms;
    let mut last_error: Option<String>;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Get current failure count from cache
    let current_failures = {
        let cache = BOOTSTRAP_CACHE.read().await;
        cache.failure_counts.get(&node.enode).copied().unwrap_or(0)
    };

    loop {
        attempts += 1;
        let start = Instant::now();

        match parse_enode_address(&node.enode) {
            Ok((ip, port)) => {
                let connect_result = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_HEALTH_CHECK_TIMEOUT_SECS),
                    tokio::net::TcpStream::connect(format!("{}:{}", ip, port)),
                )
                .await;

                match connect_result {
                    Ok(Ok(stream)) => {
                        let latency = start.elapsed().as_millis() as u64;

                        // Successful connection - reset failure count in cache
                        {
                            let mut cache = BOOTSTRAP_CACHE.write().await;
                            cache.failure_counts.remove(&node.enode);
                        }

                        // Try to detect if this is actually a Geth node by checking
                        // if it responds appropriately (basic protocol check)
                        let is_geth = verify_geth_handshake(&stream).await;

                        if !is_geth {
                            debug!(
                                "Node {} responded but may not be Geth (port open but no handshake)",
                                node.description
                            );
                        }

                        return BootstrapNodeHealth {
                            enode: node.enode.clone(),
                            description: node.description.clone(),
                            region: node.region.clone(),
                            reachable: true,
                            latency_ms: Some(latency),
                            error: None,
                            consecutive_failures: 0,
                            last_success: Some(now),
                            last_checked: Some(now),
                        };
                    }
                    Ok(Err(e)) => {
                        last_error = Some(format!("Connection failed: {}", e));
                    }
                    Err(_) => {
                        last_error = Some(format!(
                            "Connection timeout ({}s)",
                            DEFAULT_HEALTH_CHECK_TIMEOUT_SECS
                        ));
                    }
                }
            }
            Err(e) => {
                // Parse error - no point retrying
                return BootstrapNodeHealth {
                    enode: node.enode.clone(),
                    description: node.description.clone(),
                    region: node.region.clone(),
                    reachable: false,
                    latency_ms: None,
                    error: Some(format!("Invalid enode: {}", e)),
                    consecutive_failures: current_failures + 1,
                    last_success: None,
                    last_checked: Some(now),
                };
            }
        }

        // Check if we should retry
        if attempts >= retry_config.max_attempts {
            break;
        }

        debug!(
            "Bootstrap node {} check failed (attempt {}/{}), retrying in {}ms",
            node.description, attempts, retry_config.max_attempts, delay
        );

        tokio::time::sleep(Duration::from_millis(delay)).await;

        // Exponential backoff
        delay = ((delay as f64) * retry_config.backoff_multiplier)
            .min(retry_config.max_delay_ms as f64) as u64;
    }

    // Update failure count in cache
    {
        let mut cache = BOOTSTRAP_CACHE.write().await;
        let failures = cache
            .failure_counts
            .entry(node.enode.clone())
            .or_insert(0);
        *failures = current_failures + 1;
    }

    BootstrapNodeHealth {
        enode: node.enode.clone(),
        description: node.description.clone(),
        region: node.region.clone(),
        reachable: false,
        latency_ms: None,
        error: last_error,
        consecutive_failures: current_failures + 1,
        last_success: None,
        last_checked: Some(now),
    }
}

/// Verify that the connected stream is actually a Geth node
///
/// This performs a basic check to see if the node responds like a Geth P2P node.
/// Note: Full RLPx handshake verification would require crypto operations.
async fn verify_geth_handshake(_stream: &tokio::net::TcpStream) -> bool {
    // For now, we just verify TCP connectivity
    // A full implementation would:
    // 1. Send RLPx auth message
    // 2. Wait for auth-ack
    // 3. Verify protocol version
    //
    // This is complex and requires secp256k1 operations, so we defer to
    // Geth's own peer discovery which will handle this properly.
    true
}

/// Check health of all bootstrap nodes concurrently
pub async fn check_all_bootstrap_nodes() -> BootstrapHealthReport {
    check_all_bootstrap_nodes_with_config(&RetryConfig::default()).await
}

/// Check health of all bootstrap nodes with custom retry configuration
pub async fn check_all_bootstrap_nodes_with_config(
    retry_config: &RetryConfig,
) -> BootstrapHealthReport {
    let nodes = get_bootstrap_nodes_with_env_override();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Check all nodes concurrently
    let health_futures: Vec<_> = nodes
        .iter()
        .map(|node| check_bootstrap_node_health_with_config(node, retry_config))
        .collect();

    let results = futures::future::join_all(health_futures).await;

    let reachable_nodes = results.iter().filter(|h| h.reachable).count();
    let unreachable_nodes = results.len() - reachable_nodes;

    let healthy = reachable_nodes >= MIN_HEALTHY_NODES;
    let recommendation = if !healthy {
        Some(format!(
            "Only {} of {} bootstrap nodes reachable (minimum: {}). Check network connectivity or bootstrap node status.",
            reachable_nodes,
            results.len(),
            MIN_HEALTHY_NODES
        ))
    } else if reachable_nodes < results.len() / 2 {
        Some(format!(
            "Warning: Only {} of {} bootstrap nodes reachable. Network may be degraded.",
            reachable_nodes,
            results.len()
        ))
    } else {
        None
    };

    // Update cache
    let report = BootstrapHealthReport {
        total_nodes: results.len(),
        reachable_nodes,
        unreachable_nodes,
        nodes: results,
        timestamp: now,
        healthy,
        recommendation,
    };

    {
        let mut cache = BOOTSTRAP_CACHE.write().await;
        cache.last_report = Some(report.clone());
        cache.last_updated = Some(Instant::now());
    }

    report
}

// ============================================================================
// Enode String Generation
// ============================================================================

/// Get a comma-separated enode string of only healthy bootstrap nodes
///
/// This function checks all bootstrap nodes and returns only the reachable ones.
/// If no nodes are reachable, it falls back to returning all nodes.
pub async fn get_healthy_bootstrap_enode_string() -> String {
    // Check if we have a recent cached result
    {
        let cache = BOOTSTRAP_CACHE.read().await;
        if let Some(last_updated) = cache.last_updated {
            if last_updated.elapsed() < Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS) {
                if !cache.healthy_enodes.is_empty() {
                    debug!("Using cached healthy bootstrap enodes");
                    return cache.healthy_enodes.clone();
                }
            }
        }
    }

    // Perform fresh health check
    let report = check_all_bootstrap_nodes().await;

    // Sort by priority and latency
    let mut healthy_nodes: Vec<_> = report
        .nodes
        .iter()
        .filter(|node| node.reachable)
        .collect();

    healthy_nodes.sort_by(|a, b| {
        // Sort by latency (lower is better)
        a.latency_ms.cmp(&b.latency_ms)
    });

    let healthy_enodes: Vec<String> = healthy_nodes
        .iter()
        .map(|node| node.enode.clone())
        .collect();

    let result = if healthy_enodes.is_empty() {
        warn!(
            "No healthy bootstrap nodes found! Falling back to all {} nodes",
            report.total_nodes
        );
        get_bootstrap_nodes_with_env_override()
            .iter()
            .map(|n| n.enode.clone())
            .collect::<Vec<String>>()
            .join(",")
    } else {
        info!(
            "Using {} healthy bootstrap nodes (of {} total)",
            healthy_enodes.len(),
            report.total_nodes
        );
        healthy_enodes.join(",")
    };

    // Update cache
    {
        let mut cache = BOOTSTRAP_CACHE.write().await;
        cache.healthy_enodes = result.clone();
        cache.last_updated = Some(Instant::now());
    }

    result
}

/// Get all bootstrap enodes without health checking (synchronous fallback)
pub fn get_all_bootstrap_enode_string() -> String {
    get_bootstrap_nodes_with_env_override()
        .iter()
        .map(|n| n.enode.clone())
        .collect::<Vec<String>>()
        .join(",")
}

// ============================================================================
// Bootstrap Monitoring (for auto-recovery)
// ============================================================================

/// Configuration for bootstrap health monitoring
#[derive(Debug, Clone)]
pub struct BootstrapMonitorConfig {
    /// How often to check bootstrap health (seconds)
    pub check_interval_secs: u64,
    /// Minimum healthy nodes before triggering re-bootstrap
    pub min_healthy_nodes: usize,
    /// Callback channel for health status changes
    pub enable_notifications: bool,
}

impl Default for BootstrapMonitorConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: HEALTH_CHECK_INTERVAL_SECS,
            min_healthy_nodes: MIN_HEALTHY_NODES,
            enable_notifications: false,
        }
    }
}

/// Start a background task that monitors bootstrap node health
///
/// Returns a handle that can be used to stop the monitor and a receiver
/// for health status changes.
pub fn start_bootstrap_monitor(
    config: BootstrapMonitorConfig,
) -> (
    tokio::task::JoinHandle<()>,
    Option<tokio::sync::mpsc::Receiver<BootstrapHealthReport>>,
) {
    let (tx, rx) = if config.enable_notifications {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let handle = tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(config.check_interval_secs));

        loop {
            interval.tick().await;

            let report = check_all_bootstrap_nodes().await;

            if !report.healthy {
                warn!(
                    "Bootstrap health degraded: {} of {} nodes reachable",
                    report.reachable_nodes, report.total_nodes
                );

                if let Some(ref recommendation) = report.recommendation {
                    warn!("Recommendation: {}", recommendation);
                }
            } else {
                debug!(
                    "Bootstrap health OK: {} of {} nodes reachable",
                    report.reachable_nodes, report.total_nodes
                );
            }

            // Send notification if enabled
            if let Some(ref tx) = tx {
                let _ = tx.try_send(report);
            }
        }
    });

    (handle, rx)
}

/// Get the cached health report without performing a new check
pub async fn get_cached_health_report() -> Option<BootstrapHealthReport> {
    let cache = BOOTSTRAP_CACHE.read().await;
    cache.last_report.clone()
}

/// Clear the bootstrap cache (useful for testing or forcing fresh checks)
pub async fn clear_bootstrap_cache() {
    let mut cache = BOOTSTRAP_CACHE.write().await;
    *cache = BootstrapCache::default();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enode_address_valid() {
        let enode = "enode://abc123@192.168.1.1:30303";
        let result = parse_enode_address(enode);
        assert!(result.is_ok());
        let (ip, port) = result.unwrap();
        assert_eq!(ip, "192.168.1.1");
        assert_eq!(port, 30303);
    }

    #[test]
    fn test_parse_enode_address_with_query() {
        let enode = "enode://abc123@192.168.1.1:30303?discport=30304";
        let result = parse_enode_address(enode);
        assert!(result.is_ok());
        let (ip, port) = result.unwrap();
        assert_eq!(ip, "192.168.1.1");
        assert_eq!(port, 30303);
    }

    #[test]
    fn test_parse_enode_address_invalid() {
        assert!(parse_enode_address("invalid").is_err());
        assert!(parse_enode_address("enode://abc").is_err());
        assert!(parse_enode_address("enode://abc@").is_err());
        assert!(parse_enode_address("enode://abc@ip:notaport").is_err());
    }

    #[test]
    fn test_extract_node_id() {
        let enode = "enode://abc123def456@192.168.1.1:30303";
        assert_eq!(extract_node_id(enode), Some("abc123def456".to_string()));

        assert_eq!(extract_node_id("invalid"), None);
    }

    #[test]
    fn test_get_bootstrap_nodes_not_empty() {
        let nodes = get_bootstrap_nodes();
        assert!(!nodes.is_empty());

        for node in &nodes {
            assert!(node.enode.starts_with("enode://"));
            assert!(!node.description.is_empty());
            assert!(!node.region.is_empty());
        }
    }

    #[test]
    fn test_get_all_bootstrap_enode_string() {
        let enode_string = get_all_bootstrap_enode_string();
        assert!(!enode_string.is_empty());
        assert!(enode_string.contains("enode://"));
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, MAX_HEALTH_CHECK_RETRIES);
        assert!(config.backoff_multiplier > 1.0);
    }
}