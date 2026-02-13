use crate::peer_cache::{get_peer_cache_path, PeerCache, PeerCacheEntry, PeerCacheStats};
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::future::Future;
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
            DhtLifecyclePhase::Starting => Err(format!(
                "DHT node is already starting (run_id={})",
                self.run_id
            )),
            DhtLifecyclePhase::Running => Err(format!(
                "DHT node is already running (run_id={})",
                self.run_id
            )),
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
            DhtLifecyclePhase::Stopping => Err(format!(
                "DHT node is already stopping (run_id={})",
                self.run_id
            )),
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
    pub namespace_chain_id_missing: bool,
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
    pub warmstart_last_reason: Option<WarmstartReasonCode>,
    pub warmstart_policy_mode: WarmstartPolicyMode,
    pub last_warmstart_started_at: Option<u64>,
    pub last_warmstart_completed_at: Option<u64>,
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
            namespace_chain_id_missing: false,
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
            warmstart_last_reason: None,
            warmstart_policy_mode: WarmstartPolicyMode::Wan,
            last_warmstart_started_at: None,
            last_warmstart_completed_at: None,
            warmstart_budget_ms: DEFAULT_WARMSTART_BUDGET_MS,
            warmstart_max_attempts: DEFAULT_MAX_WARMSTART_ATTEMPTS,
            warmstart_max_concurrency: DEFAULT_MAX_WARMSTART_CONCURRENCY,
            warmstart_attempt_timeout_ms: DEFAULT_WARMSTART_ATTEMPT_TIMEOUT_MS,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WarmstartReasonCode {
    NamespaceMismatch,
    WarmstartDisabled,
    EmptyCache,
    AllFiltered,
    BudgetExpired,
    Cancelled,
    Success,
    NoSuccess,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WarmstartPolicyMode {
    Wan,
    Lan,
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

    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
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
    let canonical_bootstraps = canonicalize_bootstrap_set(bootstrap_nodes);
    let key = compute_namespace_key(&canonical_bootstraps, port, chain_id, true);
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

    if tokio::fs::try_exists(&ctx.legacy_file)
        .await
        .unwrap_or(false)
    {
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

    let mut parsed: NamespacedPeerCacheFile = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse namespaced peer cache: {}", e))?;

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

async fn save_namespaced_cache_file(
    path: &Path,
    file: &NamespacedPeerCacheFile,
) -> Result<(), String> {
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
    sync_file_to_disk(&tmp_path).await?;

    tokio::fs::rename(&tmp_path, path)
        .await
        .map_err(|e| format!("Failed to move namespaced peer cache into place: {}", e))?;
    sync_parent_dir(path).await?;
    Ok(())
}

async fn sync_file_to_disk(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|e| format!("Failed to open file for fsync: {}", e))?;
        file.sync_all()
            .map_err(|e| format!("Failed to fsync file: {}", e))
    })
    .await
    .map_err(|e| format!("Failed to join file fsync task: {}", e))??;
    Ok(())
}

#[cfg(unix)]
async fn sync_parent_dir(path: &Path) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| "Failed to resolve parent dir for fsync".to_string())?
        .to_path_buf();
    tokio::task::spawn_blocking(move || {
        let dir_file = std::fs::File::open(&dir)
            .map_err(|e| format!("Failed to open parent dir for fsync: {}", e))?;
        dir_file
            .sync_all()
            .map_err(|e| format!("Failed to fsync parent dir: {}", e))
    })
    .await
    .map_err(|e| format!("Failed to join dir fsync task: {}", e))??;
    Ok(())
}

#[cfg(not(unix))]
async fn sync_parent_dir(_path: &Path) -> Result<(), String> {
    // Directory fsync portability is OS-dependent; file fsync is still enforced.
    Ok(())
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
                last_seen: entry
                    .last_seen
                    .min(now.saturating_add(CLOCK_SKEW_TOLERANCE_SECS)),
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
            | Protocol::P2pCircuit => {}
            _ => return false,
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

pub fn warmstart_enabled() -> bool {
    std::env::var("CHIRAL_DISABLE_WARMSTART")
        .ok()
        .map(|v| v != "1")
        .unwrap_or(true)
}

pub fn warmstart_should_cancel(cancel_run: u64, run_id: u64) -> bool {
    cancel_run == run_id
}

pub fn warmstart_run_active(lifecycle: &DhtLifecycleState, run_id: u64) -> bool {
    lifecycle.run_id == run_id
        && (lifecycle.phase == DhtLifecyclePhase::Running
            || lifecycle.phase == DhtLifecyclePhase::Starting)
}

pub async fn run_snapshot_then_teardown<S, T>(
    snapshot: S,
    teardown: T,
) -> Result<(), String>
where
    S: Future<Output = Result<(), String>>,
    T: Future<Output = Result<(), String>>,
{
    snapshot.await?;
    teardown.await
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
    use std::sync::Arc;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;
    use tokio::sync::Mutex as AsyncMutex;

    const PEER_A: &str = "QmYwAPJzv5CZsnAzt8auVTL1YJ5hzyXH8VEkR92pT9XyM2";
    const PEER_B: &str = "QmWATWfAtUq8f3m8M4s3B4P4YJ5x9x6vKf7r8T9uV1wXyZ";
    const PEER_C: &str = "QmPChd2hVbrJ6U6fN5x8rVh9h1QKpG1Dk8r7T3xY2wZ1Ab";

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn make_entry(peer_id: &str, addresses: Vec<String>, last_seen: u64) -> PeerCacheEntry {
        PeerCacheEntry {
            peer_id: peer_id.to_string(),
            addresses,
            last_seen,
            connection_count: 0,
            successful_transfers: 0,
            failed_transfers: 0,
            total_bytes_transferred: 0,
            average_latency_ms: 0,
            is_bootstrap: false,
            supports_relay: false,
            reliability_score: 0.0,
        }
    }

    #[test]
    fn canonicalization_is_order_insensitive() {
        let a = vec![
            format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A),
            format!("/ip4/5.6.7.8/tcp/4001/p2p/{}", PEER_B),
        ];
        let b = vec![
            format!(" /ip4/5.6.7.8/tcp/4001/p2p/{} ", PEER_B),
            format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A),
        ];

        let ka = compute_namespace_key(&a, 4001, None, false);
        let kb = compute_namespace_key(&b, 4001, None, false);
        assert_eq!(ka, kb);
    }

    #[test]
    fn canonicalize_bootstrap_addr_trims_and_normalizes_multiaddr() {
        let raw = format!("  /ip4/1.2.3.4/tcp/4001/p2p/{}  ", PEER_A);
        let canonical = canonicalize_bootstrap_addr(&raw);
        assert_eq!(canonical, format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A));
    }

    #[test]
    fn canonicalize_bootstrap_addr_collapses_whitespace_for_non_multiaddr() {
        let canonical = canonicalize_bootstrap_addr("  alpha   beta   gamma ");
        assert_eq!(canonical, "alpha beta gamma");
    }

    #[test]
    fn canonicalize_bootstrap_set_deduplicates_and_sorts() {
        let set = canonicalize_bootstrap_set(&[
            format!(" /ip4/2.2.2.2/tcp/4001/p2p/{} ", PEER_B),
            format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A),
            format!("/ip4/2.2.2.2/tcp/4001/p2p/{}", PEER_B),
        ]);
        assert_eq!(set.len(), 2);
        assert!(set[0] < set[1]);
    }

    #[test]
    fn namespace_key_changes_by_port() {
        let nodes = vec![format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A)];
        let k1 = compute_namespace_key(&nodes, 4001, None, false);
        let k2 = compute_namespace_key(&nodes, 4002, None, false);
        assert_ne!(k1, k2);
    }

    #[test]
    fn namespace_key_chain_id_changes_when_opted_in() {
        let nodes = vec![format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A)];
        let k1 = compute_namespace_key(&nodes, 4001, Some(1), true);
        let k2 = compute_namespace_key(&nodes, 4001, Some(11155111), true);
        assert_ne!(k1, k2);
    }

    #[test]
    fn namespace_key_ignores_chain_id_when_opted_out() {
        let nodes = vec![format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A)];
        let k1 = compute_namespace_key(&nodes, 4001, Some(1), false);
        let k2 = compute_namespace_key(&nodes, 4001, Some(11155111), false);
        assert_eq!(k1, k2);
    }

    #[test]
    fn resolve_cache_paths_produces_namespaced_filename() {
        let (namespace_file, legacy_file) = resolve_cache_paths("deadbeef").unwrap();
        assert!(namespace_file
            .to_string_lossy()
            .contains("peer_cache.deadbeef.json"));
        assert!(legacy_file.to_string_lossy().ends_with("peer_cache.json"));
        assert_eq!(namespace_file.parent(), legacy_file.parent());
    }

    #[test]
    fn build_namespace_context_canonicalizes_bootstraps() {
        let input = vec![
            format!(" /ip4/5.6.7.8/tcp/4001/p2p/{} ", PEER_B),
            format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A),
        ];
        let ctx = build_namespace_context(&input, 4001, None).unwrap();
        assert_eq!(ctx.namespace_meta.bootstrap_nodes.len(), 2);
        assert!(
            ctx.namespace_meta.bootstrap_nodes[0] < ctx.namespace_meta.bootstrap_nodes[1],
            "bootstrap set should be stored in canonical sorted order"
        );
    }

    #[test]
    fn build_namespace_context_includes_chain_id_when_present() {
        let input = vec![format!("/ip4/1.2.3.4/tcp/4001/p2p/{}", PEER_A)];
        let with_chain_1 = build_namespace_context(&input, 4001, Some(1)).unwrap();
        let with_chain_2 = build_namespace_context(&input, 4001, Some(11155111)).unwrap();
        let without_chain = build_namespace_context(&input, 4001, None).unwrap();

        assert_eq!(
            with_chain_1.namespace_key,
            compute_namespace_key(&input, 4001, Some(1), true)
        );
        assert_ne!(with_chain_1.namespace_key, with_chain_2.namespace_key);
        assert_ne!(with_chain_1.namespace_key, without_chain.namespace_key);
    }

    #[tokio::test]
    async fn load_or_migrate_returns_empty_cache_when_no_files_exist() {
        let temp = tempdir().unwrap();
        let ctx = NamespaceContext {
            namespace_key: "ns-empty".to_string(),
            namespace_meta: PeerCacheNamespaceMeta {
                port: 4001,
                bootstrap_nodes: vec!["a".to_string()],
                chain_id: None,
            },
            namespace_file: temp.path().join("peer_cache.ns-empty.json"),
            legacy_file: temp.path().join("peer_cache.json"),
        };

        let loaded = load_or_migrate_peer_cache(&ctx).await.unwrap();
        assert!(!loaded.legacy_migrated);
        assert!(!loaded.namespace_mismatch);
        assert_eq!(loaded.cache.peers.len(), 0);
    }

    #[tokio::test]
    async fn migrates_legacy_cache_to_namespaced_file() {
        let temp = tempdir().unwrap();
        let legacy = temp.path().join("peer_cache.json");
        let namespace = temp.path().join("peer_cache.ns.json");

        let mut cache = PeerCache::new();
        cache.peers.push(make_entry(
            "p1",
            vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
            now_secs(),
        ));
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
        cache.peers.push(make_entry(
            "peer-a",
            vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
            10,
        ));
        cache.peers.push(make_entry(
            "peer-b",
            vec![format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_B)],
            20,
        ));

        let mut success = HashMap::new();
        success.insert("peer-a".to_string(), 30);

        let candidates = build_warmstart_candidates(&cache, &success, 10);
        assert_eq!(candidates[0].peer_id, "peer-a");
        assert_eq!(candidates[1].peer_id, "peer-b");
    }

    #[test]
    fn warmstart_candidates_place_missing_success_after_known_success() {
        let mut cache = PeerCache::new();
        cache.peers.push(make_entry(
            "peer-known",
            vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
            100,
        ));
        cache.peers.push(make_entry(
            "peer-unknown",
            vec![format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_B)],
            200,
        ));

        let mut success = HashMap::new();
        success.insert("peer-known".to_string(), 50);

        let candidates = build_warmstart_candidates(&cache, &success, 10);
        assert_eq!(candidates[0].peer_id, "peer-known");
        assert_eq!(candidates[1].peer_id, "peer-unknown");
    }

    #[test]
    fn warmstart_candidates_tie_break_by_peer_id() {
        let mut cache = PeerCache::new();
        cache.peers.push(make_entry(
            "peer-b",
            vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_B)],
            100,
        ));
        cache.peers.push(make_entry(
            "peer-a",
            vec![format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_A)],
            100,
        ));

        let candidates = build_warmstart_candidates(&cache, &HashMap::new(), 10);
        assert_eq!(candidates[0].peer_id, "peer-a");
        assert_eq!(candidates[1].peer_id, "peer-b");
    }

    #[test]
    fn warmstart_candidates_choose_lexicographically_smallest_address_per_peer() {
        let mut cache = PeerCache::new();
        cache.peers.push(make_entry(
            "peer-a",
            vec![
                format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_A),
                format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A),
            ],
            100,
        ));

        let candidates = build_warmstart_candidates(&cache, &HashMap::new(), 10);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].address.contains("/ip4/1.1.1.1/"));
    }

    #[test]
    fn warmstart_candidates_deduplicate_duplicate_addresses() {
        let mut cache = PeerCache::new();
        let addr = format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A);
        cache
            .peers
            .push(make_entry("peer-a", vec![addr.clone(), addr.clone()], 100));

        let candidates = build_warmstart_candidates(&cache, &HashMap::new(), 10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].address, addr);
    }

    #[test]
    fn warmstart_candidates_clamp_future_timestamps() {
        let now = now_secs();
        let mut cache = PeerCache::new();
        cache.peers.push(make_entry(
            "peer-a",
            vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
            now + 10_000,
        ));

        let mut success = HashMap::new();
        success.insert("peer-a".to_string(), now + 10_000);

        let candidates = build_warmstart_candidates(&cache, &success, 10);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].last_seen <= now + CLOCK_SKEW_TOLERANCE_SECS);
        assert!(
            candidates[0].last_successful_connect_at.unwrap_or_default()
                <= now + CLOCK_SKEW_TOLERANCE_SECS
        );
    }

    #[test]
    fn warmstart_candidates_respect_limit() {
        let mut cache = PeerCache::new();
        for i in 0..25 {
            cache.peers.push(make_entry(
                &format!("peer-{}", i),
                vec![format!("/ip4/8.8.8.{}/tcp/4001/p2p/{}", i + 1, PEER_A)],
                i as u64,
            ));
        }

        let candidates = build_warmstart_candidates(&cache, &HashMap::new(), 7);
        assert_eq!(candidates.len(), 7);
    }

    #[test]
    fn unsupported_multiaddr_shape_rejected() {
        assert!(!is_supported_dial_multiaddr_shape(&format!(
            "/ip4/8.8.8.8/udp/4001/quic-v1/p2p/{}",
            PEER_A
        )));
        assert!(is_supported_dial_multiaddr_shape(&format!(
            "/ip4/8.8.8.8/tcp/4001/p2p/{}",
            PEER_A
        )));
    }

    #[test]
    fn supported_multiaddr_requires_both_tcp_and_p2p() {
        assert!(!is_supported_dial_multiaddr_shape("/ip4/8.8.8.8/tcp/4001"));
        assert!(!is_supported_dial_multiaddr_shape(&format!(
            "/p2p/{}",
            PEER_A
        )));
    }

    #[test]
    fn supported_multiaddr_allows_dns_tcp_p2p() {
        assert!(is_supported_dial_multiaddr_shape(&format!(
            "/dns4/example.com/tcp/4001/p2p/{}",
            PEER_A
        )));
    }

    #[test]
    fn supported_multiaddr_allows_tcp_relay_shape() {
        assert!(is_supported_dial_multiaddr_shape(&format!(
            "/ip4/1.1.1.1/tcp/4001/p2p/{}/p2p-circuit/p2p/{}",
            PEER_A, PEER_B
        )));
    }

    #[test]
    fn supported_multiaddr_rejects_websocket_and_tls_wrappers() {
        assert!(!is_supported_dial_multiaddr_shape(&format!(
            "/ip4/8.8.8.8/tcp/4001/ws/p2p/{}",
            PEER_A
        )));
        assert!(!is_supported_dial_multiaddr_shape(&format!(
            "/ip4/8.8.8.8/tcp/4001/tls/p2p/{}",
            PEER_A
        )));
    }

    #[test]
    fn invalid_multiaddr_shape_rejected() {
        assert!(!is_supported_dial_multiaddr_shape("not-a-multiaddr"));
    }

    #[tokio::test]
    async fn wan_safe_rejects_private_ips() {
        assert!(
            !is_address_allowed_for_warmstart(
                &format!("/ip4/192.168.1.10/tcp/4001/p2p/{}", PEER_A),
                false
            )
            .await
        );

        assert!(
            is_address_allowed_for_warmstart(
                &format!("/ip4/192.168.1.10/tcp/4001/p2p/{}", PEER_A),
                true
            )
            .await
        );
    }

    #[tokio::test]
    async fn wan_safe_rejects_loopback_ip() {
        assert!(
            !is_address_allowed_for_warmstart(
                &format!("/ip4/127.0.0.1/tcp/4001/p2p/{}", PEER_A),
                false
            )
            .await
        );
    }

    #[tokio::test]
    async fn wan_safe_rejects_ipv6_loopback_and_unique_local() {
        assert!(
            !is_address_allowed_for_warmstart(&format!("/ip6/::1/tcp/4001/p2p/{}", PEER_A), false)
                .await
        );
        assert!(
            !is_address_allowed_for_warmstart(
                &format!("/ip6/fd00::1/tcp/4001/p2p/{}", PEER_A),
                false
            )
            .await
        );
    }

    #[tokio::test]
    async fn wan_safe_rejects_localhost_dns() {
        assert!(
            !is_address_allowed_for_warmstart(
                &format!("/dns4/localhost/tcp/4001/p2p/{}", PEER_A),
                false
            )
            .await
        );
        assert!(
            is_address_allowed_for_warmstart(
                &format!("/dns4/localhost/tcp/4001/p2p/{}", PEER_A),
                true
            )
            .await
        );
    }

    #[tokio::test]
    async fn wan_safe_rejects_unresolvable_dns() {
        assert!(
            !is_address_allowed_for_warmstart(
                &format!(
                    "/dns4/nonexistent-warmstart-peer.invalid/tcp/4001/p2p/{}",
                    PEER_A
                ),
                false
            )
            .await
        );
    }

    #[tokio::test]
    async fn address_policy_rejects_invalid_multiaddr() {
        assert!(!is_address_allowed_for_warmstart("invalid", false).await);
    }

    #[test]
    fn warmstart_allow_lan_env_defaults_false() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            std::env::remove_var("CHIRAL_LAN_WARMSTART");
        }
        assert!(!warmstart_allow_lan());
    }

    #[test]
    fn warmstart_allow_lan_env_reads_one_as_true() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            std::env::set_var("CHIRAL_LAN_WARMSTART", "1");
        }
        assert!(warmstart_allow_lan());
        unsafe {
            std::env::remove_var("CHIRAL_LAN_WARMSTART");
        }
    }

    #[test]
    fn warmstart_allow_lan_env_reads_other_values_as_false() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            std::env::set_var("CHIRAL_LAN_WARMSTART", "true");
        }
        assert!(!warmstart_allow_lan());
        unsafe {
            std::env::remove_var("CHIRAL_LAN_WARMSTART");
        }
    }

    #[test]
    fn warmstart_enabled_defaults_true() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            std::env::remove_var("CHIRAL_DISABLE_WARMSTART");
        }
        assert!(warmstart_enabled());
    }

    #[test]
    fn warmstart_enabled_respects_disable_env() {
        let _guard = env_lock().lock().unwrap();
        unsafe {
            std::env::set_var("CHIRAL_DISABLE_WARMSTART", "1");
        }
        assert!(!warmstart_enabled());
        unsafe {
            std::env::remove_var("CHIRAL_DISABLE_WARMSTART");
        }
    }

    #[test]
    fn warmstart_run_active_requires_matching_run_id_and_active_phase() {
        let mut lifecycle = DhtLifecycleState::default();
        lifecycle.mark_running(10);
        assert!(warmstart_run_active(&lifecycle, 10));
        assert!(!warmstart_run_active(&lifecycle, 11));
        lifecycle.phase = DhtLifecyclePhase::Stopping;
        assert!(!warmstart_run_active(&lifecycle, 10));
    }

    #[test]
    fn warmstart_should_cancel_matches_run_id_only() {
        assert!(warmstart_should_cancel(7, 7));
        assert!(!warmstart_should_cancel(8, 7));
    }

    #[test]
    fn ip_policy_rejects_v6_unique_local_by_default() {
        let ip: IpAddr = "fd00::1".parse().unwrap();
        assert!(!is_ip_allowed(ip, false));
        assert!(is_ip_allowed(ip, true));
    }

    #[tokio::test]
    async fn dns_policy_rejects_localhost_when_lan_disabled() {
        assert!(!dns_target_is_allowed("localhost", false).await);
    }

    #[tokio::test]
    async fn dns_policy_accepts_localhost_when_lan_enabled() {
        assert!(dns_target_is_allowed("localhost", true).await);
    }

    #[tokio::test]
    async fn dns_policy_rejects_unknown_host() {
        assert!(!dns_target_is_allowed("nonexistent-warmstart-peer.invalid", false).await);
    }

    #[test]
    fn snapshot_cache_builds_entries_and_success_map() {
        let now = now_secs();
        let mut peers = HashMap::new();
        peers.insert(
            "peer-a".to_string(),
            vec![
                format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A),
                "   ".to_string(),
            ],
        );
        peers.insert("peer-b".to_string(), vec![]);

        let (cache, success) = build_snapshot_cache(&peers, now);
        assert_eq!(cache.peers.len(), 1);
        assert_eq!(cache.peers[0].peer_id, "peer-a");
        assert_eq!(success.get("peer-a"), Some(&now));
        assert!(!success.contains_key("peer-b"));
    }

    #[test]
    fn extract_cache_stats_reports_counts() {
        let cache = PeerCache::from_peers(vec![
            PeerCacheEntry {
                peer_id: "peer-a".to_string(),
                addresses: vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
                last_seen: 1,
                connection_count: 0,
                successful_transfers: 1,
                failed_transfers: 0,
                total_bytes_transferred: 100,
                average_latency_ms: 0,
                is_bootstrap: true,
                supports_relay: true,
                reliability_score: 0.0,
            },
            PeerCacheEntry {
                peer_id: "peer-b".to_string(),
                addresses: vec![format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_B)],
                last_seen: 1,
                connection_count: 0,
                successful_transfers: 0,
                failed_transfers: 1,
                total_bytes_transferred: 50,
                average_latency_ms: 0,
                is_bootstrap: false,
                supports_relay: false,
                reliability_score: 0.0,
            },
        ]);

        let stats = extract_cache_stats(&cache);
        assert_eq!(stats.total_peers, 2);
        assert_eq!(stats.relay_capable_peers, 1);
        assert_eq!(stats.bootstrap_peers, 1);
        assert_eq!(stats.total_transfers, 2);
        assert_eq!(stats.total_bytes_transferred, 150);
    }

    #[test]
    fn peer_cache_status_defaults_include_no_warmstart_timestamps() {
        let status = PeerCacheStatus::default();
        assert!(status.last_warmstart_started_at.is_none());
        assert!(status.last_warmstart_completed_at.is_none());
    }

    #[test]
    fn peer_cache_status_serializes_warmstart_timestamps() {
        let mut status = PeerCacheStatus::default();
        status.last_warmstart_started_at = Some(10);
        status.last_warmstart_completed_at = Some(20);
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json.get("lastWarmstartStartedAt").and_then(|v| v.as_u64()), Some(10));
        assert_eq!(
            json.get("lastWarmstartCompletedAt")
                .and_then(|v| v.as_u64()),
            Some(20)
        );
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

    #[test]
    fn lifecycle_mark_running_updates_phase_and_run_id() {
        let mut s = DhtLifecycleState::default();
        s.mark_running(7);
        assert_eq!(s.phase, DhtLifecyclePhase::Running);
        assert_eq!(s.run_id, 7);
    }

    #[test]
    fn lifecycle_try_begin_stop_fails_when_already_stopped() {
        let mut s = DhtLifecycleState::default();
        assert!(s.try_begin_stop(1).is_err());
    }

    #[tokio::test]
    async fn load_namespaced_cache_detects_namespace_mismatch() {
        let now = now_secs();
        let temp = tempdir().unwrap();
        let path = temp.path().join("peer_cache.ns1.json");
        let file = NamespacedPeerCacheFile {
            header: PeerCacheHeader {
                schema_version: 1,
                namespace_key: "ns-actual".to_string(),
                namespace_meta: PeerCacheNamespaceMeta {
                    port: 4001,
                    bootstrap_nodes: vec!["a".to_string()],
                    chain_id: None,
                },
                generated_at: now,
            },
            cache: PeerCache::from_peers(vec![make_entry(
                "peer-a",
                vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
                now,
            )]),
            last_successful_connect_at: HashMap::new(),
        };
        tokio::fs::write(&path, serde_json::to_string_pretty(&file).unwrap())
            .await
            .unwrap();

        let loaded = load_namespaced_cache(&path, "ns-expected").await.unwrap();
        assert!(loaded.namespace_mismatch);
        assert_eq!(loaded.cache.peers.len(), 1);
    }

    #[tokio::test]
    async fn load_or_migrate_prefers_namespaced_file_when_both_exist() {
        let now = now_secs();
        let temp = tempdir().unwrap();
        let legacy_path = temp.path().join("peer_cache.json");
        let namespace_path = temp.path().join("peer_cache.ns.json");

        let mut legacy = PeerCache::new();
        legacy.peers.push(make_entry(
            "legacy",
            vec!["/ip4/1.1.1.1/tcp/4001".to_string()],
            10,
        ));
        legacy.save_to_file(&legacy_path).await.unwrap();

        let namespaced = NamespacedPeerCacheFile {
            header: PeerCacheHeader {
                schema_version: 1,
                namespace_key: "ns1".to_string(),
                namespace_meta: PeerCacheNamespaceMeta {
                    port: 4001,
                    bootstrap_nodes: vec!["a".to_string()],
                    chain_id: None,
                },
                generated_at: now,
            },
            cache: PeerCache::from_peers(vec![make_entry(
                "namespaced",
                vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
                now,
            )]),
            last_successful_connect_at: HashMap::new(),
        };
        tokio::fs::write(
            &namespace_path,
            serde_json::to_string_pretty(&namespaced).unwrap(),
        )
        .await
        .unwrap();

        let ctx = NamespaceContext {
            namespace_key: "ns1".to_string(),
            namespace_meta: PeerCacheNamespaceMeta {
                port: 4001,
                bootstrap_nodes: vec!["a".to_string()],
                chain_id: None,
            },
            namespace_file: namespace_path,
            legacy_file: legacy_path,
        };
        let loaded = load_or_migrate_peer_cache(&ctx).await.unwrap();
        assert!(!loaded.legacy_migrated);
        assert_eq!(loaded.cache.peers.len(), 1);
        assert_eq!(loaded.cache.peers[0].peer_id, "namespaced");
    }

    #[tokio::test]
    async fn save_namespaced_cache_round_trip() {
        let now = now_secs();
        let temp = tempdir().unwrap();
        let ctx = NamespaceContext {
            namespace_key: "ns-round".to_string(),
            namespace_meta: PeerCacheNamespaceMeta {
                port: 4001,
                bootstrap_nodes: vec!["a".to_string()],
                chain_id: None,
            },
            namespace_file: temp.path().join("peer_cache.ns-round.json"),
            legacy_file: temp.path().join("peer_cache.json"),
        };

        let cache = PeerCache::from_peers(vec![make_entry(
            "peer-a",
            vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
            now,
        )]);
        let mut success = HashMap::new();
        success.insert("peer-a".to_string(), 33);

        save_namespaced_cache(&ctx, cache, success.clone())
            .await
            .unwrap();

        let loaded = load_or_migrate_peer_cache(&ctx).await.unwrap();
        assert_eq!(loaded.cache.peers.len(), 1);
        assert_eq!(loaded.last_successful_connect_at, success);
    }

    #[tokio::test]
    async fn snapshot_then_teardown_runs_in_order() {
        let events = Arc::new(AsyncMutex::new(Vec::<&'static str>::new()));
        let snapshot_events = events.clone();
        let teardown_events = events.clone();

        run_snapshot_then_teardown(
            async move {
                snapshot_events.lock().await.push("snapshot");
                Ok(())
            },
            async move {
                teardown_events.lock().await.push("teardown");
                Ok(())
            },
        )
        .await
        .unwrap();

        let events = events.lock().await.clone();
        assert_eq!(events, vec!["snapshot", "teardown"]);
    }

    #[tokio::test]
    async fn snapshot_then_teardown_skips_teardown_on_snapshot_error() {
        let events = Arc::new(AsyncMutex::new(Vec::<&'static str>::new()));
        let teardown_events = events.clone();

        let result = run_snapshot_then_teardown(
            async { Err::<(), String>("snapshot failed".to_string()) },
            async move {
                teardown_events.lock().await.push("teardown");
                Ok(())
            },
        )
        .await;

        assert!(result.is_err());
        let events = events.lock().await.clone();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn corrupt_namespaced_cache_returns_parse_error() {
        let temp = tempdir().unwrap();
        let ctx = NamespaceContext {
            namespace_key: "ns-corrupt".to_string(),
            namespace_meta: PeerCacheNamespaceMeta {
                port: 4001,
                bootstrap_nodes: vec!["a".to_string()],
                chain_id: Some(1),
            },
            namespace_file: temp.path().join("peer_cache.ns-corrupt.json"),
            legacy_file: temp.path().join("peer_cache.json"),
        };
        tokio::fs::write(&ctx.namespace_file, "{ invalid json")
            .await
            .unwrap();

        let err = load_or_migrate_peer_cache(&ctx).await.unwrap_err();
        assert!(err.contains("Failed to parse namespaced peer cache"));
    }

    #[test]
    fn warmstart_candidates_preserve_best_last_seen_across_addresses() {
        let cache = PeerCache::from_peers(vec![
            make_entry(
                "peer-a",
                vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
                10,
            ),
            make_entry(
                "peer-a",
                vec![format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_A)],
                99,
            ),
        ]);

        let candidates = build_warmstart_candidates(&cache, &HashMap::new(), 10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].last_seen, 99);
    }

    #[test]
    fn warmstart_candidates_include_multiple_distinct_peers() {
        let cache = PeerCache::from_peers(vec![
            make_entry(
                "peer-a",
                vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
                1,
            ),
            make_entry(
                "peer-b",
                vec![format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_B)],
                2,
            ),
            make_entry(
                "peer-c",
                vec![format!("/ip4/7.7.7.7/tcp/4001/p2p/{}", PEER_C)],
                3,
            ),
        ]);
        let candidates = build_warmstart_candidates(&cache, &HashMap::new(), 10);
        assert_eq!(candidates.len(), 3);
    }
}
