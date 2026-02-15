use crate::dht::{DhtService, PrivacyMode};
use crate::AppState;
use chiral_network::proxy_latency::ProxyStatus;
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use tauri::Emitter;
use tauri::State;
// use tracing::info;
use libp2p::PeerId;
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use tokio::net::{lookup_host, TcpStream};
use tokio::time::{timeout, Duration, Instant};
use tracing::{info, warn};

#[derive(Clone, serde::Serialize)]
pub struct ProxyNode {
    pub id: String,
    pub address: String,
    pub status: String,
    pub latency: u32,
    pub error: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySelfTestResult {
    pub id: String,
    pub address: String,
    pub ok: bool,
    pub tcp_connect_ms: Option<u64>,
    // keep old field for compatibility while clients switch
    pub latency_ms: Option<u64>,
    pub ping_rtt_ms: Option<u64>,
    pub error: Option<String>,
    pub tested_at: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySelfTestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub best_id: Option<String>,
    pub best_tcp_connect_ms: Option<u64>,
    pub best_latency_ms: Option<u64>,
    pub results: Vec<ProxySelfTestResult>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxySelfTestAllComplete {
    pub run_id: u64,
    pub total: usize,
    pub completed: usize,
    pub passed: usize,
    pub failed: usize,
    pub cancelled: bool,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Normalize user input into a TCP libp2p multiaddr (no WebSocket).
/// - Keeps `/p2p/<peerid>` suffix if present
/// - For numeric IPs -> `/ip4/...`
/// - For hostnames  -> `/dns4/...`
/// - Treats ws:// and wss:// as plain TCP (drops `/ws`)
pub fn normalize_to_multiaddr(input: &str) -> Result<String, String> {
    let s = input.trim();

    // If it's already a multiaddr, accept as-is.
    if s.starts_with('/') {
        return Ok(s.to_string());
    }

    // Extract optional /p2p/<peer-id> suffix if user pasted it after the url/host:port
    let (base, p2p_suffix) = if let Some((left, right)) = s.split_once("/p2p/") {
        (left, Some(right))
    } else {
        (s, None)
    };

    // Strip known schemes;
    let base = base
        .strip_prefix("ws://")
        .or_else(|| base.strip_prefix("wss://"))
        .or_else(|| base.strip_prefix("tcp://"))
        .unwrap_or(base);

    // Expect host:port
    let (host, port) = base
        .split_once(':')
        .ok_or_else(|| format!("invalid address; expected host:port (got: {input})"))?;

    // Decide ip4 vs dns4
    let proto = if Ipv4Addr::from_str(host).is_ok() {
        "ip4"
    } else {
        "dns4"
    };

    let mut m = format!("/{proto}/{host}/tcp/{port}");
    if let Some(pid) = p2p_suffix {
        // keep any additional path after /p2p/<peerid> (rare, but harmless)
        m.push_str("/p2p/");
        m.push_str(pid);
    }
    Ok(m)
}

fn parse_tcp_target(input: &str) -> Result<(String, u16), String> {
    let normalized = normalize_to_multiaddr(input)?;
    let addr: Multiaddr = normalized
        .parse()
        .map_err(|e| format!("invalid proxy address '{input}': {e}"))?;

    let mut host: Option<String> = None;
    let mut port: Option<u16> = None;
    for p in addr.iter() {
        match p {
            Protocol::Ip4(ip) => host = Some(ip.to_string()),
            Protocol::Ip6(ip) => host = Some(ip.to_string()),
            Protocol::Dns(name) | Protocol::Dns4(name) | Protocol::Dns6(name) => {
                host = Some(name.to_string())
            }
            Protocol::Tcp(p) => port = Some(p),
            _ => {}
        }
    }

    let host = host.ok_or_else(|| format!("missing host in '{input}'"))?;
    let port = port.ok_or_else(|| format!("missing tcp port in '{input}'"))?;
    Ok((host, port))
}

fn canonical_host(host: &str) -> String {
    let lower = host.to_lowercase();
    if lower == "localhost" {
        "127.0.0.1".to_string()
    } else {
        lower
    }
}

fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

fn canonical_tcp_target(input: &str) -> Option<String> {
    parse_tcp_target(input)
        .ok()
        .map(|(host, port)| format_host_port(&canonical_host(&host), port))
}

fn is_configured_proxy_target(proxies: &[ProxyNode], target: &str) -> bool {
    let Some(target_key) = canonical_tcp_target(target) else {
        return false;
    };

    proxies.iter().any(|node| {
        canonical_tcp_target(&node.address)
            .or_else(|| canonical_tcp_target(&node.id))
            .as_deref()
            == Some(target_key.as_str())
    })
}

async fn tcp_probe(host: String, port: u16, timeout_ms: u64) -> Result<u64, String> {
    let bounded_timeout_ms = timeout_ms.max(100);
    let timeout_duration = Duration::from_millis(bounded_timeout_ms);
    let start = Instant::now();

    let probe = async {
        let mut addrs = lookup_host((host.as_str(), port))
            .await
            .map_err(|e| format!("dns resolve failed for {host}:{port}: {e}"))?;
        let target = addrs
            .next()
            .ok_or_else(|| format!("no target address resolved for {host}:{port}"))?;

        TcpStream::connect(target)
            .await
            .map_err(|e| format!("proxy test connection failed: {e}"))?;
        Ok::<(), String>(())
    };

    timeout(timeout_duration, probe)
        .await
        .map_err(|_| format!("proxy test timeout after {}ms", bounded_timeout_ms))??;
    Ok(start.elapsed().as_millis() as u64)
}

fn dedupe_self_test_targets(proxies: &[ProxyNode]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for p in proxies {
        let raw = if p.address.trim().is_empty() {
            p.id.trim()
        } else {
            p.address.trim()
        };
        if raw.is_empty() {
            continue;
        }

        let key = canonical_tcp_target(raw).unwrap_or_else(|| raw.to_lowercase());
        if seen.insert(key) {
            out.push(raw.to_string());
        }
    }

    out
}

fn summarize_self_tests(results: &[ProxySelfTestResult]) -> ProxySelfTestSummary {
    let mut best: Option<(&ProxySelfTestResult, u64)> = None;
    let mut passed = 0usize;

    for result in results {
        if result.ok {
            passed += 1;
        }
        if let Some(lat) = result.tcp_connect_ms {
            match best {
                Some((_, cur)) if lat >= cur => {}
                _ => best = Some((result, lat)),
            }
        }
    }

    ProxySelfTestSummary {
        total: results.len(),
        passed,
        failed: results.len().saturating_sub(passed),
        best_id: best.map(|(res, _)| res.id.clone()),
        best_tcp_connect_ms: best.map(|(_, lat)| lat),
        best_latency_ms: best.map(|(_, lat)| lat),
        results: results.to_vec(),
    }
}

fn summarize_self_test_all(
    run_id: u64,
    total: usize,
    results: &[ProxySelfTestResult],
    cancelled: bool,
) -> ProxySelfTestAllComplete {
    let passed = results.iter().filter(|row| row.ok).count();
    ProxySelfTestAllComplete {
        run_id,
        total,
        completed: results.len(),
        passed,
        failed: results.len().saturating_sub(passed),
        cancelled,
    }
}

async fn apply_test_result(
    app: &tauri::AppHandle,
    state: &AppState,
    target: &str,
    tcp_connect_ms: Option<u64>,
    error: Option<String>,
) -> ProxySelfTestResult {
    let tested_at = now_secs();
    let ok = error.is_none();
    let status_label = if ok { "online" } else { "error" };
    let status_kind = if ok {
        ProxyStatus::Online
    } else {
        ProxyStatus::Error
    };

    let node = {
        let mut proxies = state.proxies.lock().await;
        if let Some(p) = proxies
            .iter_mut()
            .find(|p| p.id == target || p.address == target)
        {
            p.status = status_label.to_string();
            if let Some(ms) = tcp_connect_ms {
                p.latency = ms as u32;
            }
            p.error = error.clone();
            p.clone()
        } else {
            let n = ProxyNode {
                id: target.to_string(),
                address: target.to_string(),
                status: status_label.to_string(),
                latency: tcp_connect_ms.unwrap_or_default() as u32,
                error: error.clone(),
            };
            proxies.push(n.clone());
            n
        }
    };

    {
        let mut svc = state.proxy_latency.lock().await;
        svc.update_proxy_latency(node.id.clone(), tcp_connect_ms, status_kind);
    }

    let _ = app.emit("proxy_status_update", node.clone());

    ProxySelfTestResult {
        id: node.id,
        address: node.address,
        ok,
        tcp_connect_ms,
        latency_ms: tcp_connect_ms,
        ping_rtt_ms: None,
        error,
        tested_at,
    }
}

#[tauri::command]
pub(crate) async fn proxy_connect(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
    _token: String,
) -> Result<(), String> {
    info!("Connecting to proxy: {}", url);

    // 1) optimistic UI
    {
        let mut proxies = state.proxies.lock().await;
        if let Some(p) = proxies.iter_mut().find(|p| p.address == url) {
            p.status = "connecting".into();
            p.error = None;
            p.latency = 999;
            let _ = app.emit("proxy_status_update", p.clone());
        } else {
            // The ID should be the normalized multiaddr, but we don't have it yet.
            // We'll use the URL as a temporary ID and the event pump will fix it.
            let node = ProxyNode {
                id: url.clone(),
                address: url.clone(),
                status: "connecting".into(),
                latency: 999,
                error: None,
            };
            proxies.push(node.clone());
            let _ = app.emit("proxy_status_update", node);
        }
    }

    {
        let mut svc = state.proxy_latency.lock().await;
        svc.update_proxy_latency(url.clone(), None, ProxyStatus::Connecting);
    }

    // 2) dial via DHT
    if let Some(dht) = state.dht.lock().await.as_ref() {
        let multi = normalize_to_multiaddr(&url)?;
        if let Err(err) = dht.connect_peer(multi).await {
            let mut svc = state.proxy_latency.lock().await;
            svc.update_proxy_latency(url.clone(), None, ProxyStatus::Error);
            return Err(err);
        }
        Ok(())
    } else {
        Err("DHT not initialized".into())
    }
}

#[tauri::command]
pub(crate) async fn proxy_disconnect(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    info!("Disconnecting from proxy: {}", url);
    let maybe_peer_id = {
        let mut proxies = state.proxies.lock().await;
        proxies
            .iter_mut()
            .find(|p| p.address == url || p.id == url)
            .map(|p| {
                p.status = "offline".into();
                let _ = app.emit("proxy_status_update", p.clone());
                p.id.clone()
            })
    };

    if let Some(peer_id_str) = maybe_peer_id {
        if let Ok(peer_id) = PeerId::from_str(&peer_id_str) {
            if let Some(dht) = state.dht.lock().await.as_ref() {
                {
                    let mut svc = state.proxy_latency.lock().await;
                    svc.update_proxy_latency(peer_id_str.clone(), None, ProxyStatus::Offline);
                }
                return dht.disconnect_peer(peer_id).await;
            }
        }
    }

    Err("Could not disconnect peer".into())
}

#[tauri::command]
pub(crate) async fn proxy_remove(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    info!("Removing proxy: {}", url);

    let maybe_peer_id = {
        let mut proxies = state.proxies.lock().await;
        let maybe_idx = proxies.iter().position(|p| p.address == url || p.id == url);
        if let Some(idx) = maybe_idx {
            let p = proxies.remove(idx);
            Some(p.id)
        } else {
            None
        }
    };

    if let Some(peer_id_str) = maybe_peer_id {
        if let Ok(peer_id) = PeerId::from_str(&peer_id_str) {
            if let Some(dht) = state.dht.lock().await.as_ref() {
                let _ = dht.disconnect_peer(peer_id).await;
            }
        }
        let mut svc = state.proxy_latency.lock().await;
        svc.remove_proxy(&peer_id_str);
    }

    let _ = app.emit("proxy_reset", ());
    Ok(())
}

#[tauri::command]
pub(crate) async fn list_proxies(state: State<'_, AppState>) -> Result<Vec<ProxyNode>, String> {
    let proxies = state.proxies.lock().await;
    Ok(proxies.clone())
}

#[tauri::command]
pub(crate) async fn proxy_echo(
    state: State<'_, AppState>,
    peer_id: String,
    payload: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let dht_guard = state.dht.lock().await;
    let dht: &DhtService = dht_guard
        .as_ref()
        .ok_or_else(|| "DHT not running".to_string())?;
    dht.echo(peer_id, payload).await
}

#[tauri::command]
pub(crate) async fn proxy_self_test(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    target: String,
    timeout_ms: Option<u64>,
) -> Result<ProxySelfTestResult, String> {
    let t = target.trim();
    if t.is_empty() {
        return Err("target must not be empty".to_string());
    }
    let is_allowed_target = {
        let proxies = state.proxies.lock().await;
        is_configured_proxy_target(&proxies, t)
    };
    if !is_allowed_target {
        return Err("target must match an already configured proxy address or id".to_string());
    }

    let timeout_ms = timeout_ms.unwrap_or(1500);
    let (host, port) = parse_tcp_target(t)?;

    let res = match tcp_probe(host, port, timeout_ms).await {
        Ok(lat_ms) => apply_test_result(&app, &state, t, Some(lat_ms), None).await,
        Err(err) => apply_test_result(&app, &state, t, None, Some(err)).await,
    };
    Ok(res)
}

#[tauri::command]
pub(crate) async fn proxy_self_test_all(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    timeout_ms: Option<u64>,
) -> Result<Vec<ProxySelfTestResult>, String> {
    let timeout_ms = timeout_ms.unwrap_or(1500);
    let run_id = state
        .proxy_self_test_epoch
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1);
    let targets = {
        let proxies = state.proxies.lock().await;
        dedupe_self_test_targets(&proxies)
    };

    let mut cancelled = false;
    let mut out = Vec::with_capacity(targets.len());
    for t in targets.iter() {
        if state.proxy_self_test_epoch.load(Ordering::SeqCst) != run_id {
            cancelled = true;
            break;
        }
        let parsed = parse_tcp_target(&t);
        let res = match parsed {
            Ok((host, port)) => match tcp_probe(host, port, timeout_ms).await {
                Ok(ms) => apply_test_result(&app, &state, &t, Some(ms), None).await,
                Err(err) => apply_test_result(&app, &state, &t, None, Some(err)).await,
            },
            Err(err) => apply_test_result(&app, &state, &t, None, Some(err)).await,
        };
        out.push(res);
    }

    // only the active epoch can publish completion
    if cancelled || state.proxy_self_test_epoch.load(Ordering::SeqCst) != run_id {
        return Err("proxy self-test run cancelled by a newer request".to_string());
    }

    let summary = summarize_self_test_all(run_id, targets.len(), &out, false);
    let _ = app.emit("proxy_self_test_all_complete", summary);
    Ok(out)
}

#[tauri::command]
pub(crate) async fn proxy_self_test_report(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    timeout_ms: Option<u64>,
) -> Result<ProxySelfTestSummary, String> {
    let results = proxy_self_test_all(app, state, timeout_ms).await?;
    Ok(summarize_self_tests(&results))
}

#[tauri::command]
pub(crate) async fn enable_privacy_routing(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    proxy_addresses: Vec<String>,
    mode: Option<String>,
) -> Result<(), String> {
    let privacy_mode = mode
        .as_deref()
        .map(PrivacyMode::from_str)
        .unwrap_or(PrivacyMode::Prefer);

    info!(
        "Enabling privacy routing through {} proxies (mode: {:?})",
        proxy_addresses.len(),
        privacy_mode
    );

    if proxy_addresses.is_empty() {
        return Err("No proxy addresses provided".into());
    }

    // Store the proxy addresses for routing and normalize to multiaddrs
    let mut normalized_proxies: Vec<String> = Vec::new();
    {
        let mut privacy_proxies = state.privacy_proxies.lock().await;
        privacy_proxies.clear();
        for addr in &proxy_addresses {
            match normalize_to_multiaddr(addr) {
                Ok(multiaddr) => {
                    privacy_proxies.push(multiaddr.clone());
                    normalized_proxies.push(multiaddr);
                    info!("Added proxy for privacy routing: {}", addr);
                }
                Err(e) => {
                    warn!("Failed to normalize proxy address {}: {}", addr, e);
                }
            }
        }
    }

    if normalized_proxies.is_empty() {
        return Err("No valid proxy addresses provided".into());
    }

    // Enable privacy routing in DHT service
    if let Some(dht) = state.dht.lock().await.as_ref() {
        dht.update_privacy_proxy_targets(normalized_proxies.clone())
            .await?;
        dht.enable_privacy_routing(privacy_mode).await?;
    } else {
        return Err("DHT not initialized".into());
    }

    let _ = app.emit("privacy_routing_enabled", normalized_proxies.len());
    Ok(())
}

#[tauri::command]
pub(crate) async fn disable_privacy_routing(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Disabling privacy routing");

    // Clear stored proxy addresses
    {
        let mut privacy_proxies = state.privacy_proxies.lock().await;
        privacy_proxies.clear();
    }

    // Disable privacy routing in DHT service
    if let Some(dht) = state.dht.lock().await.as_ref() {
        dht.update_privacy_proxy_targets(Vec::new()).await?;
        dht.disable_privacy_routing().await?;
    } else {
        return Err("DHT not initialized".into());
    }

    let _ = app.emit("privacy_routing_disabled", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn parse_tcp_target_supports_host_port() {
        let (host, port) = parse_tcp_target("127.0.0.1:8080").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8080);
    }

    #[test]
    fn parse_tcp_target_supports_multiaddr() {
        let (host, port) = parse_tcp_target("/ip4/127.0.0.1/tcp/9000").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 9000);
    }

    #[test]
    fn canonical_tcp_target_normalizes_localhost() {
        let localhost = canonical_tcp_target("localhost:9050").unwrap();
        let loopback = canonical_tcp_target("127.0.0.1:9050").unwrap();
        assert_eq!(localhost, loopback);
    }

    #[test]
    fn dedupe_targets_merges_equivalent_ids() {
        let proxies = vec![
            ProxyNode {
                id: "a".into(),
                address: "localhost:9050".into(),
                status: "offline".into(),
                latency: 0,
                error: None,
            },
            ProxyNode {
                id: "b".into(),
                address: "127.0.0.1:9050".into(),
                status: "offline".into(),
                latency: 0,
                error: None,
            },
            ProxyNode {
                id: "c".into(),
                address: "/dns4/example.org/tcp/443".into(),
                status: "offline".into(),
                latency: 0,
                error: None,
            },
        ];
        let deduped = dedupe_self_test_targets(&proxies);
        assert_eq!(deduped.len(), 2);
    }

    #[tokio::test]
    async fn tcp_probe_succeeds_for_local_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let latency = tcp_probe("127.0.0.1".to_string(), port, 1000)
            .await
            .unwrap();
        assert!(latency <= 1000);

        let _ = task.await;
    }

    #[tokio::test]
    async fn tcp_probe_fails_for_closed_port() {
        let err = tcp_probe("127.0.0.1".to_string(), 9, 200)
            .await
            .unwrap_err();
        assert!(err.contains("failed") || err.contains("timeout"));
    }

    #[test]
    fn summarize_self_tests_picks_best() {
        let rows = vec![
            ProxySelfTestResult {
                id: "p-a".to_string(),
                address: "a".to_string(),
                ok: true,
                tcp_connect_ms: Some(50),
                latency_ms: Some(50),
                ping_rtt_ms: None,
                error: None,
                tested_at: 1,
            },
            ProxySelfTestResult {
                id: "p-b".to_string(),
                address: "b".to_string(),
                ok: false,
                tcp_connect_ms: None,
                latency_ms: None,
                ping_rtt_ms: None,
                error: Some("bad".to_string()),
                tested_at: 2,
            },
            ProxySelfTestResult {
                id: "p-c".to_string(),
                address: "c".to_string(),
                ok: true,
                tcp_connect_ms: Some(10),
                latency_ms: Some(10),
                ping_rtt_ms: None,
                error: None,
                tested_at: 3,
            },
        ];
        let summary = summarize_self_tests(&rows);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.best_id.as_deref(), Some("p-c"));
        assert_eq!(summary.best_tcp_connect_ms, Some(10));
        assert_eq!(summary.best_latency_ms, Some(10));
    }
}
