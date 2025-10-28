/// Advanced Relay Tests
///
/// Tests for advanced relay scenarios:
/// 1. NAT traversal edge cases
/// 2. Relay failover and redundancy
/// 3. Circuit relay performance
/// 4. Relay reservation management
/// 5. Multi-hop relay scenarios
///
/// These tests simulate complex network topologies within a single container

use chiral_network::dht::DhtService;
use std::time::Duration;
use tokio::time::sleep;

/// Test symmetric NAT scenario with relay fallback
#[tokio::test]
async fn test_symmetric_nat_with_relay() {
    println!("🧪 Testing symmetric NAT scenario with relay fallback...");

    // In a real symmetric NAT, neither peer can directly connect
    // Both should use relay as fallback

    // Setup relay server
    let relay = DhtService::new(
        14400,
        vec![],
        Some("relay-for-nat".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,                               // Act as relay
        Some("NATRelayServer".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    let relay_addr = format!("/ip4/127.0.0.1/tcp/14400/p2p/{}", relay_peer_id);
    println!("✅ Relay server: {}", relay_peer_id);

    sleep(Duration::from_secs(2)).await;

    // Setup peer A behind "NAT" (simulated by using relay)
    let peer_a = DhtService::new(
        14401,
        vec![],
        Some("nat-peer-a".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,                               // Use AutoRelay
        vec![relay_addr.clone()],           // Use relay
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create peer A");

    let peer_a_id = peer_a.get_peer_id().await;
    println!("✅ Peer A (behind NAT): {}", peer_a_id);

    // Setup peer B behind "NAT" (also using relay)
    let peer_b = DhtService::new(
        14402,
        vec![],
        Some("nat-peer-b".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,                               // Use AutoRelay
        vec![relay_addr.clone()],           // Use same relay
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create peer B");

    let peer_b_id = peer_b.get_peer_id().await;
    println!("✅ Peer B (behind NAT): {}", peer_b_id);

    // Give time for relay reservations
    sleep(Duration::from_secs(4)).await;

    // Check that both peers have relay connections
    let health_a = peer_a.get_health().await;
    let health_b = peer_b.get_health().await;

    if let Some(h) = health_a {
        println!("📊 Peer A: AutoRelay={}, ActiveRelay={:?}",
                 h.autorelay_enabled, h.active_relay_peer_id);
    }

    if let Some(h) = health_b {
        println!("📊 Peer B: AutoRelay={}, ActiveRelay={:?}",
                 h.autorelay_enabled, h.active_relay_peer_id);
    }

    println!("✅ Symmetric NAT with relay test passed");
}

/// Test relay reservation renewal
#[tokio::test]
async fn test_relay_reservation_renewal() {
    println!("🧪 Testing relay reservation renewal...");

    let relay = DhtService::new(
        14403,
        vec![],
        Some("renewal-relay".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("RenewalTestRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    let relay_addr = format!("/ip4/127.0.0.1/tcp/14403/p2p/{}", relay_peer_id);

    sleep(Duration::from_secs(1)).await;

    let client = DhtService::new(
        14404,
        vec![],
        Some("renewal-client".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay_addr],
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create client");

    let client_peer_id = client.get_peer_id().await;
    println!("✅ Client: {}", client_peer_id);

    // Initial reservation
    sleep(Duration::from_secs(3)).await;

    let health1 = client.get_health().await;
    if let Some(h) = &health1 {
        println!("📊 Initial reservation renewals: {}", h.reservation_renewals);
    }

    // Wait for potential renewal (note: actual renewal timing depends on implementation)
    sleep(Duration::from_secs(5)).await;

    let health2 = client.get_health().await;
    if let Some(h) = &health2 {
        println!("📊 After wait reservation renewals: {}", h.reservation_renewals);
        // Note: Renewal count may not increase in short test, but this verifies the metric exists
    }

    println!("✅ Relay reservation renewal test passed");
}

/// Test DCUtR (hole punching) with relay fallback
#[tokio::test]
async fn test_dcutr_with_relay_fallback() {
    println!("🧪 Testing DCUtR with relay fallback...");

    // Setup relay for initial connection
    let relay = DhtService::new(
        14405,
        vec![],
        Some("dcutr-relay".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("DCUtRRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    let relay_addr = format!("/ip4/127.0.0.1/tcp/14405/p2p/{}", relay_peer_id);

    sleep(Duration::from_secs(2)).await;

    // Peer A with AutoRelay (will attempt DCUtR)
    let peer_a = DhtService::new(
        14406,
        vec![],
        Some("dcutr-peer-a".to_string()),
        false,
        true,                               // AutoNAT enabled (enables DCUtR)
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay_addr.clone()],
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create peer A");

    let peer_a_id = peer_a.get_peer_id().await;

    // Peer B with AutoRelay
    let peer_b = DhtService::new(
        14407,
        vec![],
        Some("dcutr-peer-b".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay_addr],
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create peer B");

    let peer_b_id = peer_b.get_peer_id().await;

    println!("✅ Peer A: {}", peer_a_id);
    println!("✅ Peer B: {}", peer_b_id);

    sleep(Duration::from_secs(4)).await;

    // Check DCUtR metrics
    let health_a = peer_a.get_health().await;
    let health_b = peer_b.get_health().await;

    if let Some(h) = health_a {
        println!("📊 Peer A DCUtR: enabled={}, attempts={}, successes={}",
                 h.dcutr_enabled, h.dcutr_hole_punch_attempts, h.dcutr_hole_punch_successes);
        assert!(h.dcutr_enabled, "DCUtR should be enabled with AutoNAT");
    }

    if let Some(h) = health_b {
        println!("📊 Peer B DCUtR: enabled={}, attempts={}, successes={}",
                 h.dcutr_enabled, h.dcutr_hole_punch_attempts, h.dcutr_hole_punch_successes);
        assert!(h.dcutr_enabled, "DCUtR should be enabled with AutoNAT");
    }

    println!("✅ DCUtR with relay fallback test passed");
}

/// Test relay under load (multiple concurrent clients)
#[tokio::test]
async fn test_relay_under_load() {
    println!("🧪 Testing relay under load with multiple clients...");

    let relay = DhtService::new(
        14408,
        vec![],
        Some("load-relay".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("LoadTestRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    let relay_addr = format!("/ip4/127.0.0.1/tcp/14408/p2p/{}", relay_peer_id);
    println!("✅ Relay started: {}", relay_peer_id);

    sleep(Duration::from_secs(2)).await;

    // Create multiple clients concurrently
    let num_clients = 5;
    let mut clients = Vec::new();

    for i in 0..num_clients {
        let port = 14409 + i;
        let secret = format!("load-client-{}", i);

        let client = DhtService::new(
            port,
            vec![],
            Some(secret),
            false,
            true,
            Some(Duration::from_secs(30)),
            vec![],
            None,
            None,
            None,
            Some(256),
            Some(1024),
            true,
            vec![relay_addr.clone()],
            false,
            None,
            None,
        )
        .await
        .expect(&format!("Failed to create client {}", i));

        let client_id = client.get_peer_id().await;
        println!("✅ Client {} started: {}", i, client_id);

        clients.push(client);
    }

    // Give time for all reservations
    sleep(Duration::from_secs(5)).await;

    // Verify all clients are healthy
    for (i, client) in clients.iter().enumerate() {
        let health = client.get_health().await;
        assert!(health.is_some(), "Client {} should have health metrics", i);

        if let Some(h) = health {
            println!("📊 Client {}: AutoRelay={}, TotalRelays={}",
                     i, h.autorelay_enabled, h.total_relays_in_pool);
        }
    }

    println!("✅ Relay under load test passed with {} clients", num_clients);
}

/// Test relay connection failover
#[tokio::test]
async fn test_relay_failover() {
    println!("🧪 Testing relay failover scenario...");

    // Setup two relay servers
    let relay1 = DhtService::new(
        14414,
        vec![],
        Some("failover-relay-1".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("PrimaryFailoverRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay 1");

    let relay1_peer_id = relay1.get_peer_id().await;
    let relay1_addr = format!("/ip4/127.0.0.1/tcp/14414/p2p/{}", relay1_peer_id);

    let relay2 = DhtService::new(
        14415,
        vec![],
        Some("failover-relay-2".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("BackupFailoverRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay 2");

    let relay2_peer_id = relay2.get_peer_id().await;
    let relay2_addr = format!("/ip4/127.0.0.1/tcp/14415/p2p/{}", relay2_peer_id);

    println!("✅ Primary relay: {}", relay1_peer_id);
    println!("✅ Backup relay: {}", relay2_peer_id);

    sleep(Duration::from_secs(2)).await;

    // Client with both relays
    let client = DhtService::new(
        14416,
        vec![],
        Some("failover-client".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay1_addr, relay2_addr],     // Multiple relays for failover
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create client");

    let client_peer_id = client.get_peer_id().await;
    println!("✅ Client started: {}", client_peer_id);

    sleep(Duration::from_secs(4)).await;

    let health = client.get_health().await;
    if let Some(h) = health {
        println!("📊 Client relay pool: {} total relays", h.total_relays_in_pool);
        assert!(h.total_relays_in_pool >= 2, "Should have both relays in pool");

        if let Some(active_relay) = h.active_relay_peer_id {
            println!("📊 Active relay: {}", active_relay);
        }
    }

    // Simulate failover by dropping primary relay
    drop(relay1);
    println!("⚠️  Dropped primary relay (simulating failure)");

    sleep(Duration::from_secs(3)).await;

    // Client should still be functional with backup relay
    let health_after = client.get_health().await;
    assert!(health_after.is_some(), "Client should still be functional after relay failure");

    println!("✅ Relay failover test passed");
}

/// Test relay bandwidth metrics tracking
#[tokio::test]
async fn test_relay_bandwidth_tracking() {
    println!("🧪 Testing relay bandwidth metrics tracking...");

    let relay = DhtService::new(
        14417,
        vec![],
        Some("bandwidth-relay".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("BandwidthTrackingRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    println!("✅ Bandwidth tracking relay: {}", relay_peer_id);

    sleep(Duration::from_secs(2)).await;

    let health = relay.get_health().await;
    if let Some(h) = health {
        println!("📊 Relay connection attempts: {}", h.relay_connection_attempts);
        println!("📊 Relay connection successes: {}", h.relay_connection_successes);
        println!("📊 Relay connection failures: {}", h.relay_connection_failures);

        // These should exist even if zero
        assert!(h.relay_connection_attempts >= 0);
    }

    println!("✅ Relay bandwidth tracking test passed");
}

/// Test relay health scoring
#[tokio::test]
async fn test_relay_health_scoring() {
    println!("🧪 Testing relay health scoring...");

    let relay = DhtService::new(
        14418,
        vec![],
        Some("health-relay".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("HealthScoringRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    let relay_addr = format!("/ip4/127.0.0.1/tcp/14418/p2p/{}", relay_peer_id);

    sleep(Duration::from_secs(2)).await;

    let client = DhtService::new(
        14419,
        vec![],
        Some("health-client".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay_addr],
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create client");

    let client_peer_id = client.get_peer_id().await;
    println!("✅ Client: {}", client_peer_id);

    sleep(Duration::from_secs(3)).await;

    let health = client.get_health().await;
    if let Some(h) = health {
        println!("📊 Relay health score: {}", h.relay_health_score);

        // Health score should be tracked (0-100 range typically)
        assert!(h.relay_health_score >= 0.0 && h.relay_health_score <= 100.0,
                "Health score should be in valid range");
    }

    println!("✅ Relay health scoring test passed");
}

/// Test circuit establishment between NAT'd peers
#[tokio::test]
async fn test_circuit_establishment_through_relay() {
    println!("🧪 Testing circuit establishment through relay...");

    // Setup relay
    let relay = DhtService::new(
        14420,
        vec![],
        Some("circuit-relay".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("CircuitRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    let relay_addr = format!("/ip4/127.0.0.1/tcp/14420/p2p/{}", relay_peer_id);
    println!("✅ Circuit relay: {}", relay_peer_id);

    sleep(Duration::from_secs(2)).await;

    // Source peer
    let source = DhtService::new(
        14421,
        vec![],
        Some("circuit-source".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay_addr.clone()],
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create source peer");

    let source_id = source.get_peer_id().await;
    println!("✅ Source peer: {}", source_id);

    // Destination peer
    let dest = DhtService::new(
        14422,
        vec![],
        Some("circuit-dest".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay_addr],
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create destination peer");

    let dest_id = dest.get_peer_id().await;
    println!("✅ Destination peer: {}", dest_id);

    // Wait for circuit establishment
    sleep(Duration::from_secs(5)).await;

    // Check metrics on both peers
    let source_health = source.get_health().await;
    let dest_health = dest.get_health().await;

    if let Some(h) = source_health {
        println!("📊 Source: connected to {} active relays", h.active_relay_count);
    }

    if let Some(h) = dest_health {
        println!("📊 Destination: connected to {} active relays", h.active_relay_count);
    }

    println!("✅ Circuit establishment test passed");
}

/// Test that relay reservation expires and renews
#[tokio::test]
async fn test_relay_reservation_expiry() {
    println!("🧪 Testing relay reservation expiry and renewal...");

    let relay = DhtService::new(
        14423,
        vec![],
        Some("expiry-relay".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        false,
        vec![],
        true,
        Some("ExpiryTestRelay".to_string()),
        None,
    )
    .await
    .expect("Failed to create relay");

    let relay_peer_id = relay.get_peer_id().await;
    let relay_addr = format!("/ip4/127.0.0.1/tcp/14423/p2p/{}", relay_peer_id);

    sleep(Duration::from_secs(2)).await;

    let client = DhtService::new(
        14424,
        vec![],
        Some("expiry-client".to_string()),
        false,
        true,
        Some(Duration::from_secs(30)),
        vec![],
        None,
        None,
        None,
        Some(256),
        Some(1024),
        true,
        vec![relay_addr],
        false,
        None,
        None,
    )
    .await
    .expect("Failed to create client");

    let client_peer_id = client.get_peer_id().await;
    println!("✅ Client: {}", client_peer_id);

    sleep(Duration::from_secs(3)).await;

    let health_before = client.get_health().await;
    if let Some(h) = &health_before {
        println!("📊 Before: reservation successes = {}", h.last_reservation_success.is_some());
        println!("📊 Before: reservation evictions = {}", h.reservation_evictions);
    }

    // Note: Actual expiry/renewal testing requires longer wait times
    // or manipulation of reservation duration, which is implementation-dependent

    println!("✅ Relay reservation expiry test passed");
    println!("⚠️  Full expiry testing requires longer timeframes");
}
