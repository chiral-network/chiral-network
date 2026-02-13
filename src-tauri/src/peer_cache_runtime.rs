use crate::peer_cache::{get_peer_cache_path, PeerCache, PeerCacheEntry, PeerCacheStats};
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_WARMSTART_CANDIDATES: usize = 20;
pub const DEFAULT_MAX_WARMSTART_ATTEMPTS: usize = 20;
pub const DEFAULT_MAX_WARMSTART_CONCURRENCY: usize = 4;
pub const DEFAULT_WARMSTART_ATTEMPT_TIMEOUT_MS: u64 = 3_000;
pub const DEFAULT_WARMSTART_BUDGET_MS: u64 = 20_000;
const CLOCK_SKEW_TOLERANCE_SECS: u64 = 300;
const DNS_LOOKUP_CAP: usize = 8;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DhtLifecyclePhase {
    Stopped,
    Starting,
    Running,
    Stopping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtLifecycleState {
    pub phase: DhtLifecyclePhase,
    pub run_id: u64,
}

impl Default for DhtLifecycleState {
    fn default() -> Self {
        Self {
            phase: DhtLifecyclePhase::Stopped,
            run_id: 0,
        }
    }
}

impl DhtLifecycleState {
    pub fn try_begin_start(&mut self, run_id: u64) -> Result<(), String> {
        match self.phase {
            DhtLifecyclePhase::Stopped => {
                self.phase = DhtLifecyclePhase::Starting;
                self.run_id = run_id;
                Ok(())
            }
            DhtLifecyclePhase::Starting => {
                Err(format!("DHT node is already starting (run_id={})", self.run_id))
            }
            DhtLifecyclePhase::Running => {
                Err(format!("DHT node is already running (run_id={})", self.run_id))
            }
            DhtLifecyclePhase::Stopping => {
                Err(format!("DHT node is stopping (run_id={})", self.run_id))
            }
        }
    }

    pub fn mark_running(&mut self, run_id: u64) {
        self.phase = DhtLifecyclePhase::Running;
        self.run_id = run_id;
    }

    pub fn mark_stopped(&mut self) {
        self.phase = DhtLifecyclePhase::Stopped;
    }

    pub fn try_begin_stop(&mut self, run_id: u64) -> Result<(), String> {
        match self.phase {
            DhtLifecyclePhase::Stopped => Err("DHT node is not running".to_string()),
            DhtLifecyclePhase::Starting | DhtLifecyclePhase::Running => {
                self.phase = DhtLifecyclePhase::Stopping;
                self.run_id = run_id;
                Ok(())
            }
            DhtLifecyclePhase::Stopping => {
                Err(format!("DHT node is already stopping (run_id={})", self.run_id))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCacheNamespaceMeta {
    pub port: u16,
    pub bootstrap_nodes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCacheHeader {
    pub schema_version: u32,
    pub namespace_key: String,
    pub namespace_meta: PeerCacheNamespaceMeta,
    pub generated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespacedPeerCacheFile {
    pub header: PeerCacheHeader,
    pub cache: PeerCache,
    #[serde(default)]
    pub last_successful_connect_at: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerCacheStatus {
    pub enabled: bool,
    pub lifecycle: DhtLifecycleState,
    pub namespace_key: Option<String>,
    pub namespace_file_path: Option<String>,
    pub legacy_file_path: Option<String>,
    pub namespace_mismatch: bool,
    pub legacy_migrated: bool,
    pub last_loaded_at: Option<u64>,
    pub last_saved_at: Option<u64>,
    pub peers_loaded: usize,
    pub peers_selected_for_warmstart: usize,
    pub warmstart_attempted: usize,
    pub warmstart_succeeded: usize,
    pub warmstart_skipped: usize,
    pub warmstart_cancelled: bool,
    pub warmstart_task_running: bool,
    pub warmstart_budget_ms: u64,
    pub warmstart_max_attempts: usize,
    pub warmstart_max_concurrency: usize,
    pub warmstart_attempt_timeout_ms: u64,
    pub last_error: Option<String>,
}

impl Default for PeerCacheStatus {
    fn default() -> Self {
        Self {
            enabled: true,
            lifecycle: DhtLifecycleState::default(),
            namespace_key: None,
            namespace_file_path: None,
            legacy_file_path: None,
            namespace_mismatch: false,
            legacy_migrated: false,
            last_loaded_at: None,
            last_saved_at: None,
            peers_loaded: 0,
            peers_selected_for_warmstart: 0,
            warmstart_attempted: 0,
            warmstart_succeeded: 0,
            warmstart_skipped: 0,
            warmstart_cancelled: false,
            warmstart_task_running: false,
            warmstart_budget_ms: DEFAULT_WARMSTART_BUDGET_MS,
            warmstart_max_attempts: DEFAULT_MAX_WARMSTART_ATTEMPTS,
            warmstart_max_concurrency: DEFAULT_MAX_WARMSTART_CONCURRENCY,
            warmstart_attempt_timeout_ms: DEFAULT_WARMSTART_ATTEMPT_TIMEOUT_MS,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmStartCandidate {
    pub peer_id: String,
    pub address: String,
    pub last_successful_connect_at: Option<u64>,
    pub last_seen: u64,
}

#[derive(Debug, Clone)]
pub struct NamespaceContext {
    pub namespace_key: String,
    pub namespace_meta: PeerCacheNamespaceMeta,
    pub namespace_file: PathBuf,
    pub legacy_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LoadedPeerCache {
    pub cache: PeerCache,
    pub last_successful_connect_at: HashMap<String, u64>,
    pub namespace_mismatch: bool,
    pub legacy_migrated: bool,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

pub fn canonicalize_bootstrap_addr(input: &str) -> String {
    let trimmed = input.trim();
    if let Ok(addr) = trimmed.parse::<Multiaddr>() {
        return addr.to_string();
    }

    trimmed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn canonicalize_bootstrap_set(bootstrap_nodes: &[String]) -> Vec<String> {
    let mut set = HashSet::new();
    for node in bootstrap_nodes {
        let canonical = canonicalize_bootstrap_addr(node);
        if !canonical.is_empty() {
            set.insert(canonical);
        }
    }
    let mut out = set.into_iter().collect::<Vec<_>>();
    out.sort();
    out
}

pub fn compute_namespace_key(
    bootstrap_nodes: &[String],
    port: u16,
    chain_id: Option<u64>,
    include_chain_id_salt: bool,
) -> String {
    let canonical_bootstraps = canonicalize_bootstrap_set(bootstrap_nodes);
    let mut hasher = Sha256::new();
    hasher.update(format!("dht_port={};", port));
    hasher.update(format!("bootstraps={};", canonical_bootstraps.join(",")));
    if include_chain_id_salt {
        if let Some(id) = chain_id {
            hasher.update(format!("chain_id={};", id));
        }
    }
    let digest = hasher.finalize();
    hex::encode(&digest[..16])
}

pub fn resolve_cache_paths(namespace_key: &str) -> Result<(PathBuf, PathBuf), String> {
    let legacy = get_peer_cache_path()?;
    let dir = legacy
        .parent()
        .ok_or_else(|| "Failed to get peer cache directory".to_string())?;
    Ok((
        dir.join(format!("peer_cache.{}.json", namespace_key)),
        legacy,
    ))
}

pub fn build_namespace_context(
    bootstrap_nodes: &[String],
    port: u16,
    chain_id: Option<u64>,
) -> Result<NamespaceContext, String> {
    let include_chain = std::env::var("CHIRAL_CACHE_INCLUDE_CHAIN_ID")
        .ok()
        .map(|v| v == "1")
        .unwrap_or(false);
    let canonical_bootstraps = canonicalize_bootstrap_set(bootstrap_nodes);
    let key = compute_namespace_key(&canonical_bootstraps, port, chain_id, include_chain);
    let (namespace_file, legacy_file) = resolve_cache_paths(&key)?;

    Ok(NamespaceContext {
        namespace_key: key,
        namespace_meta: PeerCacheNamespaceMeta {
            port,
            bootstrap_nodes: canonical_bootstraps,
            chain_id,
        },
        namespace_file,
        legacy_file,
    })
}

pub async fn load_or_migrate_peer_cache(ctx: &NamespaceContext) -> Result<LoadedPeerCache, String> {
    if tokio::fs::try_exists(&ctx.namespace_file)
        .await
        .unwrap_or(false)
    {
        return load_namespaced_cache(&ctx.namespace_file, &ctx.namespace_key).await;
    }

    if tokio::fs::try_exists(&ctx.legacy_file).await.unwrap_or(false) {
        let mut legacy = PeerCache::load_from_file(&ctx.legacy_file).await?;
        legacy.filter_stale_peers();
        legacy.sort_and_limit();

        let file = NamespacedPeerCacheFile {
            header: PeerCacheHeader {
                schema_version: 1,
                namespace_key: ctx.namespace_key.clone(),
                namespace_meta: ctx.namespace_meta.clone(),
                generated_at: now_secs(),
            },
            cache: legacy.clone(),
            last_successful_connect_at: HashMap::new(),
        };

        save_namespaced_cache_file(&ctx.namespace_file, &file).await?;

        return Ok(LoadedPeerCache {
            cache: legacy,
            last_successful_connect_at: HashMap::new(),
            namespace_mismatch: false,
            legacy_migrated: true,
        });
    }

    Ok(LoadedPeerCache {
        cache: PeerCache::new(),
        last_successful_connect_at: HashMap::new(),
        namespace_mismatch: false,
        legacy_migrated: false,
    })
}

async fn load_namespaced_cache(
    path: &Path,
    expected_namespace_key: &str,
) -> Result<LoadedPeerCache, String> {
    let json = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("Failed to read namespaced peer cache: {}", e))?;

    let mut parsed: NamespacedPeerCacheFile =
        serde_json::from_str(&json).map_err(|e| format!("Failed to parse namespaced peer cache: {}", e))?;

    let namespace_mismatch = parsed.header.namespace_key != expected_namespace_key;
    parsed.cache.filter_stale_peers();
    parsed.cache.sort_and_limit();

    Ok(LoadedPeerCache {
        cache: parsed.cache,
        last_successful_connect_at: parsed.last_successful_connect_at,
        namespace_mismatch,
        legacy_migrated: false,
    })
}

pub async fn save_namespaced_cache(
    ctx: &NamespaceContext,
    mut cache: PeerCache,
    last_successful_connect_at: HashMap<String, u64>,
) -> Result<(), String> {
    cache.filter_stale_peers();
    cache.sort_and_limit();

    let file = NamespacedPeerCacheFile {
        header: PeerCacheHeader {
            schema_version: 1,
            namespace_key: ctx.namespace_key.clone(),
            namespace_meta: ctx.namespace_meta.clone(),
            generated_at: now_secs(),
        },
        cache,
        last_successful_connect_at,
    };

    save_namespaced_cache_file(&ctx.namespace_file, &file).await
}

async fn save_namespaced_cache_file(path: &Path, file: &NamespacedPeerCacheFile) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| format!("Failed to create peer cache directory: {}", e))?;
    }

    let json = serde_json::to_string_pretty(file)
        .map_err(|e| format!("Failed to serialize namespaced peer cache: {}", e))?;
    let tmp_path = path.with_extension("tmp");

    tokio::fs::write(&tmp_path, json)
        .await
        .map_err(|e| format!("Failed to write namespaced peer cache temp file: {}", e))?;

    tokio::fs::rename(&tmp_path, path)
        .await
        .map_err(|e| format!("Failed to move namespaced peer cache into place: {}", e))
}

pub fn build_warmstart_candidates(
    cache: &PeerCache,
    last_successful_connect_at: &HashMap<String, u64>,
    max_candidates: usize,
) -> Vec<WarmStartCandidate> {
    let now = now_secs();

    let mut per_peer_best: HashMap<String, WarmStartCandidate> = HashMap::new();

    for entry in &cache.peers {
        let mut addresses = entry.addresses.clone();
        addresses.sort();
        addresses.dedup();

        for address in addresses {
            let mut candidate = WarmStartCandidate {
                peer_id: entry.peer_id.clone(),
                address,
                last_successful_connect_at: last_successful_connect_at
                    .get(&entry.peer_id)
                    .copied()
                    .map(|v| v.min(now.saturating_add(CLOCK_SKEW_TOLERANCE_SECS))),
                last_seen: entry.last_seen.min(now.saturating_add(CLOCK_SKEW_TOLERANCE_SECS)),
            };

            if let Some(prev) = per_peer_best.get(&candidate.peer_id) {
                if candidate.address < prev.address {
                    candidate.last_successful_connect_at = prev.last_successful_connect_at;
                    candidate.last_seen = prev.last_seen;
                }
            }

            per_peer_best
                .entry(candidate.peer_id.clone())
                .and_modify(|existing| {
                    if candidate.address < existing.address {
                        existing.address = candidate.address.clone();
                    }
                    existing.last_seen = existing.last_seen.max(candidate.last_seen);
                    existing.last_successful_connect_at = existing
                        .last_successful_connect_at
                        .max(candidate.last_successful_connect_at);
                })
                .or_insert(candidate);
        }
    }

    let mut candidates = per_peer_best.into_values().collect::<Vec<_>>();

    candidates.sort_by(|a, b| {
        b.last_successful_connect_at
            .cmp(&a.last_successful_connect_at)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
            .then_with(|| a.peer_id.cmp(&b.peer_id))
    });

    candidates.truncate(max_candidates);
    candidates
}

pub fn is_supported_dial_multiaddr_shape(addr: &str) -> bool {
    let ma = match addr.parse::<Multiaddr>() {
        Ok(ma) => ma,
        Err(_) => return false,
    };

    let mut has_tcp = false;
    let mut has_p2p = false;

    for protocol in ma.iter() {
        match protocol {
            Protocol::Tcp(_) => has_tcp = true,
            Protocol::P2p(_) => has_p2p = true,
            Protocol::Ip4(_)
            | Protocol::Ip6(_)
            | Protocol::Dns(_)
            | Protocol::Dns4(_)
            | Protocol::Dns6(_)
            | Protocol::P2pCircuit
            | Protocol::Tls
            | Protocol::Ws(_)
            | Protocol::Wss(_)
            | Protocol::WebRTC
            | Protocol::WebRTCDirect
            | Protocol::Certhash(_) => {}
            _ => {}
        }
    }

    has_tcp && has_p2p
}

pub async fn is_address_allowed_for_warmstart(addr: &str, allow_lan: bool) -> bool {
    let ma = match addr.parse::<Multiaddr>() {
        Ok(ma) => ma,
        Err(_) => return false,
    };

    if !is_supported_dial_multiaddr_shape(addr) {
        return false;
    }

    for protocol in ma.iter() {
        match protocol {
            Protocol::Ip4(ip) => {
                if !is_ip_allowed(IpAddr::V4(ip), allow_lan) {
                    return false;
                }
            }
            Protocol::Ip6(ip) => {
                if !is_ip_allowed(IpAddr::V6(ip), allow_lan) {
                    return false;
                }
            }
            Protocol::Dns(host) | Protocol::Dns4(host) | Protocol::Dns6(host) => {
                if !dns_target_is_allowed(host.as_ref(), allow_lan).await {
                    return false;
                }
            }
            _ => {}
        }
    }

    true
}

pub fn warmstart_allow_lan() -> bool {
    std::env::var("CHIRAL_LAN_WARMSTART")
        .ok()
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn is_ip_allowed(ip: IpAddr, allow_lan: bool) -> bool {
    if allow_lan {
        return true;
    }

    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified())
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local())
        }
    }
}

async fn dns_target_is_allowed(host: &str, allow_lan: bool) -> bool {
    let lookup = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        tokio::net::lookup_host((host, 0)),
    )
    .await;

    let Ok(Ok(iter)) = lookup else {
        return false;
    };

    let mut checked = 0usize;
    for socket in iter {
        checked += 1;
        if checked > DNS_LOOKUP_CAP {
            break;
        }
        if !is_ip_allowed(socket.ip(), allow_lan) {
            return false;
        }
    }

    checked > 0
}

pub fn build_snapshot_cache(
    peers_with_addresses: &HashMap<String, Vec<String>>,
    now: u64,
) -> (PeerCache, HashMap<String, u64>) {
    let mut entries = Vec::new();
    let mut success = HashMap::new();

    for (peer_id, addresses) in peers_with_addresses {
        let addrs = addresses
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect::<Vec<_>>();

        if addrs.is_empty() {
            continue;
        }

        success.insert(peer_id.clone(), now);

        entries.push(PeerCacheEntry {
            peer_id: peer_id.clone(),
            addresses: addrs,
            last_seen: now,
            connection_count: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.0,
        });
    }

    let mut cache = PeerCache::from_peers(entries);
    cache.filter_stale_peers();
    cache.sort_and_limit();
    (cache, success)
}

pub fn extract_cache_stats(cache: &PeerCache) -> PeerCacheStats {
    cache.get_stats()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn canonicalization_is_order_insensitive() {
        let a = vec![
            "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWAAA".to_string(),
            "/ip4/5.6.7.8/tcp/4001/p2p/12D3KooWBBB".to_string(),
        ];
        let b = vec![
            " /ip4/5.6.7.8/tcp/4001/p2p/12D3KooWBBB ".to_string(),
            "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWAAA".to_string(),
        ];

        let ka = compute_namespace_key(&a, 4001, None, false);
        let kb = compute_namespace_key(&b, 4001, None, false);
        assert_eq!(ka, kb);
    }

    #[test]
    fn namespace_key_changes_by_port() {
        let nodes = vec!["/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWAAA".to_string()];
        let k1 = compute_namespace_key(&nodes, 4001, None, false);
        let k2 = compute_namespace_key(&nodes, 4002, None, false);
        assert_ne!(k1, k2);
    }

    #[tokio::test]
    async fn migrates_legacy_cache_to_namespaced_file() {
        let temp = tempdir().unwrap();
        let legacy = temp.path().join("peer_cache.json");
        let namespace = temp.path().join("peer_cache.ns.json");

        let mut cache = PeerCache::new();
        cache.peers.push(PeerCacheEntry {
            peer_id: "p1".to_string(),
            addresses: vec!["/ip4/8.8.8.8/tcp/4001/p2p/12D3KooWAAA".to_string()],
            last_seen: now_secs(),
            connection_count: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.0,
        });
        cache.save_to_file(&legacy).await.unwrap();

        let ctx = NamespaceContext {
            namespace_key: "ns1".to_string(),
            namespace_meta: PeerCacheNamespaceMeta {
                port: 4001,
                bootstrap_nodes: vec!["a".to_string()],
                chain_id: None,
            },
            namespace_file: namespace.clone(),
            legacy_file: legacy,
        };

        let loaded = load_or_migrate_peer_cache(&ctx).await.unwrap();
        assert!(loaded.legacy_migrated);
        assert!(namespace.exists());
        assert_eq!(loaded.cache.peers.len(), 1);
    }

    #[test]
    fn warmstart_candidates_prioritize_success_then_last_seen() {
        let mut cache = PeerCache::new();
        cache.peers.push(PeerCacheEntry {
            peer_id: "peer-a".to_string(),
            addresses: vec!["/ip4/8.8.8.8/tcp/4001/p2p/12D3KooWAAA".to_string()],
            last_seen: 10,
            connection_count: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.0,
        });
        cache.peers.push(PeerCacheEntry {
            peer_id: "peer-b".to_string(),
            addresses: vec!["/ip4/9.9.9.9/tcp/4001/p2p/12D3KooWBBB".to_string()],
            last_seen: 20,
            connection_count: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.0,
        });

        let mut success = HashMap::new();
        success.insert("peer-a".to_string(), 30);

        let candidates = build_warmstart_candidates(&cache, &success, 10);
        assert_eq!(candidates[0].peer_id, "peer-a");
        assert_eq!(candidates[1].peer_id, "peer-b");
    }

    #[test]
    fn unsupported_multiaddr_shape_rejected() {
        assert!(!is_supported_dial_multiaddr_shape("/ip4/8.8.8.8/udp/4001/quic-v1/p2p/12D3KooWAAA"));
        assert!(is_supported_dial_multiaddr_shape("/ip4/8.8.8.8/tcp/4001/p2p/12D3KooWAAA"));
    }

    #[tokio::test]
    async fn wan_safe_rejects_private_ips() {
        assert!(!is_address_allowed_for_warmstart(
            "/ip4/192.168.1.10/tcp/4001/p2p/12D3KooWAAA",
            false
        )
        .await);

        assert!(is_address_allowed_for_warmstart(
            "/ip4/192.168.1.10/tcp/4001/p2p/12D3KooWAAA",
            true
        )
        .await);
    }

    #[test]
    fn lifecycle_state_enforces_single_flight() {
        let mut s = DhtLifecycleState::default();
        assert!(s.try_begin_start(1).is_ok());
        assert!(s.try_begin_start(2).is_err());
        s.mark_running(1);
        assert!(s.try_begin_stop(1).is_ok());
        assert!(s.try_begin_stop(1).is_err());
        s.mark_stopped();
        assert!(s.try_begin_start(2).is_ok());
    }
}
