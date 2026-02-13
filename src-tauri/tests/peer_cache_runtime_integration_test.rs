use chiral_network::peer_cache::{PeerCache, PeerCacheEntry};
use chiral_network::peer_cache_runtime::{
    build_snapshot_cache, build_warmstart_candidates, canonicalize_bootstrap_set,
    compute_namespace_key, is_address_allowed_for_warmstart, load_or_migrate_peer_cache, now_secs,
    save_namespaced_cache, DhtLifecycleState, NamespaceContext, PeerCacheNamespaceMeta,
};
use futures::future::join_all;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

const PEER_A: &str = "QmYwAPJzv5CZsnAzt8auVTL1YJ5hzyXH8VEkR92pT9XyM2";
const PEER_B: &str = "QmWATWfAtUq8f3m8M4s3B4P4YJ5x9x6vKf7r8T9uV1wXyZ";
const PEER_C: &str = "QmPChd2hVbrJ6U6fN5x8rVh9h1QKpG1Dk8r7T3xY2wZ1Ab";

fn entry(peer_id: &str, address: &str, last_seen: u64) -> PeerCacheEntry {
    PeerCacheEntry {
        peer_id: peer_id.to_string(),
        addresses: vec![address.to_string()],
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

fn namespace_context(temp: &TempDir, key: &str) -> NamespaceContext {
    NamespaceContext {
        namespace_key: key.to_string(),
        namespace_meta: PeerCacheNamespaceMeta {
            port: 4001,
            bootstrap_nodes: vec!["boot-a".to_string()],
            chain_id: None,
        },
        namespace_file: temp.path().join(format!("peer_cache.{}.json", key)),
        legacy_file: temp.path().join("peer_cache.json"),
    }
}

#[tokio::test]
async fn namespace_files_coexist_without_overwrite() {
    let temp = TempDir::new().unwrap();
    let now = now_secs();
    let ctx_a = namespace_context(&temp, "ns-a");
    let ctx_b = namespace_context(&temp, "ns-b");

    let cache_a = PeerCache::from_peers(vec![entry(
        "peer-a",
        &format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A),
        now,
    )]);
    let cache_b = PeerCache::from_peers(vec![entry(
        "peer-b",
        &format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_B),
        now,
    )]);

    save_namespaced_cache(&ctx_a, cache_a, HashMap::new())
        .await
        .unwrap();
    save_namespaced_cache(&ctx_b, cache_b, HashMap::new())
        .await
        .unwrap();

    let loaded_a = load_or_migrate_peer_cache(&ctx_a).await.unwrap();
    let loaded_b = load_or_migrate_peer_cache(&ctx_b).await.unwrap();

    assert_eq!(loaded_a.cache.peers.len(), 1);
    assert_eq!(loaded_b.cache.peers.len(), 1);
    assert_eq!(loaded_a.cache.peers[0].peer_id, "peer-a");
    assert_eq!(loaded_b.cache.peers[0].peer_id, "peer-b");
}

#[tokio::test]
async fn legacy_migration_happens_once_per_namespace_file() {
    let temp = TempDir::new().unwrap();
    let now = now_secs();
    let ctx = namespace_context(&temp, "ns-migrate");

    let legacy = PeerCache::from_peers(vec![entry(
        "legacy-peer",
        &format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A),
        now,
    )]);
    legacy.save_to_file(&ctx.legacy_file).await.unwrap();

    let first = load_or_migrate_peer_cache(&ctx).await.unwrap();
    let second = load_or_migrate_peer_cache(&ctx).await.unwrap();
    assert!(first.legacy_migrated);
    assert!(!second.legacy_migrated);
    assert_eq!(second.cache.peers.len(), 1);
}

#[tokio::test]
async fn mismatch_namespace_sets_flag_but_keeps_cache_payload() {
    let temp = TempDir::new().unwrap();
    let now = now_secs();
    let ctx_a = namespace_context(&temp, "ns-a");
    let mut ctx_b = namespace_context(&temp, "ns-b");

    let cache = PeerCache::from_peers(vec![entry(
        "peer-a",
        &format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A),
        now,
    )]);
    save_namespaced_cache(&ctx_a, cache, HashMap::new())
        .await
        .unwrap();

    // Force ns-b to read ns-a file and validate mismatch path.
    ctx_b.namespace_file = ctx_a.namespace_file.clone();
    let loaded = load_or_migrate_peer_cache(&ctx_b).await.unwrap();
    assert!(loaded.namespace_mismatch);
    assert_eq!(loaded.cache.peers.len(), 1);
}

#[test]
fn namespace_key_is_stable_for_bootstrap_permutations() {
    let a = vec![
        format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A),
        format!("/ip4/2.2.2.2/tcp/4001/p2p/{}", PEER_B),
    ];
    let b = vec![
        format!(" /ip4/2.2.2.2/tcp/4001/p2p/{} ", PEER_B),
        format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A),
    ];
    let key_a = compute_namespace_key(&a, 4001, None, false);
    let key_b = compute_namespace_key(&b, 4001, None, false);
    assert_eq!(key_a, key_b);
}

#[test]
fn namespace_key_changes_with_port() {
    let bootstraps = vec![format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A)];
    let key_a = compute_namespace_key(&bootstraps, 4001, None, false);
    let key_b = compute_namespace_key(&bootstraps, 4002, None, false);
    assert_ne!(key_a, key_b);
}

#[test]
fn namespace_key_chain_id_changes_when_included() {
    let bootstraps = vec![format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A)];
    let key_a = compute_namespace_key(&bootstraps, 4001, Some(1), true);
    let key_b = compute_namespace_key(&bootstraps, 4001, Some(11155111), true);
    assert_ne!(key_a, key_b);
}

#[test]
fn namespace_key_chain_id_is_ignored_when_not_included() {
    let bootstraps = vec![format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A)];
    let key_a = compute_namespace_key(&bootstraps, 4001, Some(1), false);
    let key_b = compute_namespace_key(&bootstraps, 4001, Some(11155111), false);
    assert_eq!(key_a, key_b);
}

#[test]
fn canonicalize_bootstrap_set_is_sorted_and_unique() {
    let set = canonicalize_bootstrap_set(&[
        format!(" /ip4/2.2.2.2/tcp/4001/p2p/{} ", PEER_B),
        format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A),
        format!("/ip4/1.1.1.1/tcp/4001/p2p/{}", PEER_A),
    ]);
    assert_eq!(set.len(), 2);
    assert!(set[0] < set[1]);
}

#[tokio::test]
async fn lifecycle_parallel_start_has_single_winner() {
    let state = Arc::new(Mutex::new(DhtLifecycleState::default()));
    let mut tasks = Vec::new();
    for run_id in 1..=10 {
        let state = state.clone();
        tasks.push(tokio::spawn(async move {
            let mut guard = state.lock().await;
            guard.try_begin_start(run_id).is_ok()
        }));
    }

    let results = join_all(tasks).await;
    let success = results
        .into_iter()
        .map(|r| r.unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(success, 1);
}

#[test]
fn lifecycle_allows_stop_from_starting_and_restart_after_stopped() {
    let mut state = DhtLifecycleState::default();
    assert!(state.try_begin_start(1).is_ok());
    assert!(state.try_begin_stop(1).is_ok());
    state.mark_stopped();
    assert!(state.try_begin_start(2).is_ok());
}

#[tokio::test]
async fn lifecycle_parallel_stop_has_single_winner() {
    let mut initial = DhtLifecycleState::default();
    initial.mark_running(42);
    let state = Arc::new(Mutex::new(initial));

    let mut tasks = Vec::new();
    for _ in 0..10 {
        let state = state.clone();
        tasks.push(tokio::spawn(async move {
            let mut guard = state.lock().await;
            guard.try_begin_stop(42).is_ok()
        }));
    }

    let results = join_all(tasks).await;
    let success = results
        .into_iter()
        .map(|r| r.unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(success, 1);
}

#[test]
fn lifecycle_rejects_start_while_stopping() {
    let mut state = DhtLifecycleState::default();
    state.mark_running(5);
    assert!(state.try_begin_stop(5).is_ok());
    assert!(state.try_begin_start(6).is_err());
}

#[tokio::test]
async fn snapshot_save_and_load_roundtrip_preserves_success_map() {
    let temp = TempDir::new().unwrap();
    let now = now_secs();
    let ctx = namespace_context(&temp, "ns-roundtrip");

    let mut peers = HashMap::new();
    peers.insert(
        "peer-a".to_string(),
        vec![format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A)],
    );
    peers.insert(
        "peer-b".to_string(),
        vec![format!("/ip4/9.9.9.9/tcp/4001/p2p/{}", PEER_B)],
    );

    let (cache, success_map) = build_snapshot_cache(&peers, now);
    save_namespaced_cache(&ctx, cache, success_map.clone())
        .await
        .unwrap();

    let loaded = load_or_migrate_peer_cache(&ctx).await.unwrap();
    assert_eq!(loaded.cache.peers.len(), 2);
    assert_eq!(loaded.last_successful_connect_at, success_map);
}

#[test]
fn warmstart_candidates_limit_is_applied_deterministically() {
    let now = now_secs();
    let mut cache = PeerCache::new();
    let mut success = HashMap::new();

    for i in 0..20 {
        let peer = format!("peer-{}", i);
        cache.peers.push(entry(
            &peer,
            &format!("/ip4/8.8.8.{}/tcp/4001/p2p/{}", i + 1, PEER_A),
            now - i as u64,
        ));
        success.insert(peer, now + (20 - i) as u64);
    }

    let candidates = build_warmstart_candidates(&cache, &success, 5);
    assert_eq!(candidates.len(), 5);
    assert_eq!(candidates[0].peer_id, "peer-0");
}

#[tokio::test]
async fn warmstart_address_policy_allows_public_ip_in_wan_mode() {
    let allowed =
        is_address_allowed_for_warmstart(&format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", PEER_A), false)
            .await;
    assert!(allowed);
}

#[tokio::test]
async fn warmstart_address_policy_rejects_dns_localhost_in_wan_mode() {
    let allowed = is_address_allowed_for_warmstart(
        &format!("/dns4/localhost/tcp/4001/p2p/{}", PEER_C),
        false,
    )
    .await;
    assert!(!allowed);
}
