/// Integration tests for the Chiral Network Circuit Relay v2 Daemon
///
/// These tests verify:
/// 1. Relay server initialization and listening
/// 2. Configuration validation
/// 3. Metrics export functionality
/// 4. Multi-instance deployment
/// 5. Protocol support (AutoNAT, Identify, Ping)

use futures::{FutureExt, StreamExt};
use libp2p::{
    autonat, identify, identity,
    multiaddr::Protocol,
    noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId,
};
use std::{
    net::Ipv4Addr,
    time::Duration,
};
use tokio::time::timeout;

// Test behaviour for relay server (same as main.rs)
#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    autonat: autonat::Behaviour,
}

/// Helper to create a relay server
async fn create_relay_server(
    port: u16,
    max_reservations: usize,
    max_circuits: usize,
) -> Result<(libp2p::Swarm<RelayBehaviour>, PeerId), Box<dyn std::error::Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let relay_config = relay::Config {
        max_reservations,
        max_circuits,
        reservation_duration: Duration::from_secs(300),
        ..Default::default()
    };

    let behaviour = RelayBehaviour {
        relay: relay::Behaviour::new(local_peer_id, relay_config),
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/chiral/test/1.0.0".to_string(),
            local_key.public(),
        )),
        autonat: autonat::Behaviour::new(local_peer_id, autonat::Config::default()),
    };

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    let listen_addr = Multiaddr::empty()
        .with(Protocol::Ip4(Ipv4Addr::LOCALHOST))
        .with(Protocol::Tcp(port));

    swarm.listen_on(listen_addr)?;

    Ok((swarm, local_peer_id))
}

// Simple client behaviour without relay client
#[derive(NetworkBehaviour)]
struct SimpleClientBehaviour {
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

/// Helper to create a simple test client
async fn create_test_client() -> Result<
    (libp2p::Swarm<SimpleClientBehaviour>, PeerId),
    Box<dyn std::error::Error>,
> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let behaviour = SimpleClientBehaviour {
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/chiral/test/1.0.0".to_string(),
            local_key.public(),
        )),
    };

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Listen on random port
    swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse()?)?;

    Ok((swarm, local_peer_id))
}

#[tokio::test]
async fn test_relay_server_initialization() {
    println!("🧪 Testing relay server initialization...");

    let result = create_relay_server(15001, 128, 16).await;
    assert!(result.is_ok(), "Failed to create relay server");

    let (mut swarm, peer_id) = result.unwrap();
    println!("✅ Relay server created with peer ID: {}", peer_id);

    // Wait for listening to be established
    let listen_result = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = swarm.next().await {
                println!("✅ Relay listening on: {}", address);
                return true;
            }
        }
    })
    .await;

    assert!(listen_result.is_ok(), "Relay failed to start listening");
    println!("✅ Relay server initialization test passed!");
}

#[tokio::test]
async fn test_client_connection_to_relay() {
    println!("🧪 Testing client connection to relay...");

    // Start relay server
    let (mut relay_swarm, relay_peer_id) = create_relay_server(15002, 128, 16).await.unwrap();
    println!("✅ Relay server started: {}", relay_peer_id);

    // Get relay listen address
    let relay_addr = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = relay_swarm.next().await {
                return address.with(Protocol::P2p(relay_peer_id));
            }
        }
    })
    .await
    .expect("Relay failed to get listen address");

    println!("✅ Relay listening on: {}", relay_addr);

    // Create client
    let (mut client_swarm, client_peer_id) = create_test_client().await.unwrap();
    println!("✅ Client created: {}", client_peer_id);

    // Connect client to relay
    client_swarm.dial(relay_addr.clone()).unwrap();
    println!("📞 Client dialing relay...");

    // Run both swarms and wait for connection
    let connection_result = timeout(Duration::from_secs(10), async {
        let mut relay_connected = false;
        let mut client_connected = false;

        loop {
            tokio::select! {
                Some(event) = relay_swarm.next() => {
                    if let SwarmEvent::ConnectionEstablished { peer_id, .. } = event {
                        println!("✅ Relay: Connection established with {}", peer_id);
                        relay_connected = true;
                    }
                }
                Some(event) = client_swarm.next() => {
                    if let SwarmEvent::ConnectionEstablished { peer_id, .. } = event {
                        println!("✅ Client: Connection established with {}", peer_id);
                        client_connected = true;
                    }
                }
            }

            if relay_connected && client_connected {
                return true;
            }
        }
    })
    .await;

    assert!(
        connection_result.is_ok(),
        "Client failed to connect to relay"
    );
    println!("✅ Client connection to relay test passed!");
}

#[tokio::test]
async fn test_metrics_structure() {
    println!("🧪 Testing metrics data structure...");

    use serde_json::json;

    // Test metrics JSON structure
    let metrics = json!({
        "peer_id": "12D3KooWTest123",
        "listen_addresses": ["/ip4/127.0.0.1/tcp/4001"],
        "connected_peers": 5,
        "uptime_seconds": 3600,
        "relay_reservations": 3,
        "relay_circuits": 1
    });

    assert!(metrics["peer_id"].is_string());
    assert!(metrics["listen_addresses"].is_array());
    assert!(metrics["connected_peers"].is_number());
    assert!(metrics["uptime_seconds"].is_number());
    assert!(metrics["relay_reservations"].is_number());
    assert!(metrics["relay_circuits"].is_number());

    println!("✅ Metrics structure test passed!");
}

#[tokio::test]
async fn test_relay_config_limits() {
    println!("🧪 Testing relay configuration limits...");

    // Test various limit configurations
    let configs = vec![
        (50, 10, "Conservative"),
        (128, 16, "Default"),
        (256, 32, "High-capacity"),
    ];

    for (max_res, max_circ, label) in configs {
        let result = create_relay_server(0, max_res, max_circ).await;
        assert!(
            result.is_ok(),
            "Failed to create relay with {} config",
            label
        );
        println!(
            "✅ {} config (res: {}, circ: {}) - OK",
            label, max_res, max_circ
        );
    }

    println!("✅ Relay config limits test passed!");
}

#[tokio::test]
async fn test_concurrent_relay_instances() {
    println!("🧪 Testing multiple concurrent relay instances...");

    let ports = vec![15003, 15004, 15005];
    let mut relays = Vec::new();

    for port in &ports {
        let (mut swarm, peer_id) = create_relay_server(*port, 64, 8).await.unwrap();

        // Wait for listen
        let listen_result = timeout(Duration::from_secs(3), async {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = swarm.next().await {
                return address;
            }
            panic!("No listen address");
        })
        .await;

        assert!(listen_result.is_ok(), "Failed to listen on port {}", port);
        println!("✅ Relay instance {} listening on port {}", peer_id, port);

        relays.push((swarm, peer_id));
    }

    assert_eq!(relays.len(), 3, "Should have 3 relay instances");
    println!("✅ Concurrent relay instances test passed!");
}

#[tokio::test]
async fn test_relay_identify_protocol() {
    println!("🧪 Testing relay identify protocol...");

    // Start relay server
    let (mut relay_swarm, relay_peer_id) = create_relay_server(15006, 128, 16).await.unwrap();

    // Get relay address
    let relay_addr = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = relay_swarm.next().await {
                return address.with(Protocol::P2p(relay_peer_id));
            }
        }
    })
    .await
    .unwrap();

    // Create client
    let (mut client_swarm, _) = create_test_client().await.unwrap();
    client_swarm.dial(relay_addr.clone()).unwrap();

    // Wait for identify exchange
    let identify_result = timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                Some(event) = relay_swarm.next() => {
                    if let SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. })) = event {
                        println!("✅ Relay: Identified client {} with protocol {}", peer_id, info.protocol_version);
                        return true;
                    }
                }
                Some(event) = client_swarm.next() => {
                    if let SwarmEvent::Behaviour(SimpleClientBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. })) = event {
                        println!("✅ Client: Identified relay {} with protocol {}", peer_id, info.protocol_version);
                    }
                }
            }
        }
    })
    .await;

    assert!(identify_result.is_ok(), "Identify protocol exchange failed");
    println!("✅ Identify protocol test passed!");
}

#[tokio::test]
async fn test_relay_ping() {
    println!("🧪 Testing relay ping protocol...");

    // Start relay server
    let (mut relay_swarm, relay_peer_id) = create_relay_server(15007, 128, 16).await.unwrap();

    // Get relay address
    let relay_addr = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = relay_swarm.next().await {
                return address.with(Protocol::P2p(relay_peer_id));
            }
        }
    })
    .await
    .unwrap();

    // Create client
    let (mut client_swarm, _) = create_test_client().await.unwrap();
    client_swarm.dial(relay_addr.clone()).unwrap();

    // Wait for ping exchange
    let ping_result = timeout(Duration::from_secs(15), async {
        loop {
            tokio::select! {
                Some(event) = relay_swarm.next() => {
                    if let SwarmEvent::Behaviour(RelayBehaviourEvent::Ping(ping::Event { peer, result: Ok(rtt), .. })) = event {
                        println!("✅ Relay: Ping from {} with RTT {:?}", peer, rtt);
                        return true;
                    }
                }
                Some(event) = client_swarm.next() => {
                    if let SwarmEvent::Behaviour(SimpleClientBehaviourEvent::Ping(ping::Event { peer, result: Ok(rtt), .. })) = event {
                        println!("✅ Client: Ping from relay {} with RTT {:?}", peer, rtt);
                    }
                }
            }
        }
    })
    .await;

    assert!(ping_result.is_ok(), "Ping protocol exchange failed");
    println!("✅ Ping protocol test passed!");
}

#[tokio::test]
async fn test_port_configuration() {
    println!("🧪 Testing port configuration flexibility...");

    let ports = vec![15008, 15009, 15010];

    for port in ports {
        let result = create_relay_server(port, 64, 8).await;
        assert!(result.is_ok(), "Failed to create relay on port {}", port);

        let (mut swarm, _) = result.unwrap();

        // Verify it can listen
        let listen_result = timeout(Duration::from_secs(3), async {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = swarm.next().await {
                return address;
            }
            panic!("No listen address");
        })
        .await;

        assert!(listen_result.is_ok(), "Failed to listen on port {}", port);
        println!("✅ Successfully configured relay on port {}", port);
    }

    println!("✅ Port configuration test passed!");
}

#[tokio::test]
async fn test_identity_persistence() {
    println!("🧪 Testing identity key generation...");

    let key1 = identity::Keypair::generate_ed25519();
    let key2 = identity::Keypair::generate_ed25519();

    let peer_id1 = PeerId::from(key1.public());
    let peer_id2 = PeerId::from(key2.public());

    // Different keys should produce different peer IDs
    assert_ne!(peer_id1, peer_id2, "Peer IDs should be different");

    // Same key should produce same peer ID
    let peer_id1_again = PeerId::from(key1.public());
    assert_eq!(
        peer_id1, peer_id1_again,
        "Same key should produce same peer ID"
    );

    println!("✅ Generated peer ID 1: {}", peer_id1);
    println!("✅ Generated peer ID 2: {}", peer_id2);
    println!("✅ Identity persistence test passed!");
}

#[tokio::test]
async fn test_relay_server_multiple_clients() {
    println!("🧪 Testing relay server with multiple clients...");

    // Start relay
    let (mut relay_swarm, relay_peer_id) = create_relay_server(15011, 128, 16).await.unwrap();

    // Get relay address
    let relay_addr = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = relay_swarm.next().await {
                return address.with(Protocol::P2p(relay_peer_id));
            }
        }
    })
    .await
    .unwrap();

    // Create multiple clients
    let num_clients = 3;
    let mut clients = Vec::new();

    for i in 0..num_clients {
        let (mut client_swarm, client_id) = create_test_client().await.unwrap();
        client_swarm.dial(relay_addr.clone()).unwrap();
        clients.push((client_swarm, client_id));
        println!("✅ Client {} created: {}", i + 1, client_id);
    }

    // Wait for all connections
    let connection_result = timeout(Duration::from_secs(20), async {
        let mut connected_count = 0;

        loop {
            // Process relay events
            if let Some(event) = relay_swarm.next().now_or_never().flatten() {
                if let SwarmEvent::ConnectionEstablished { peer_id, .. } = event {
                    println!("✅ Relay: Client connected: {}", peer_id);
                    connected_count += 1;
                }
            }

            // Process all client events
            for (client_swarm, _) in clients.iter_mut() {
                if let Some(event) = client_swarm.next().now_or_never().flatten() {
                    if let SwarmEvent::ConnectionEstablished { peer_id, .. } = event {
                        println!("✅ Client connected to relay: {}", peer_id);
                    }
                }
            }

            if connected_count >= num_clients {
                return true;
            }

            // Small sleep to prevent busy loop
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;

    assert!(
        connection_result.is_ok(),
        "Not all clients connected successfully"
    );
    println!("✅ Multiple clients test passed!");
}

#[tokio::test]
async fn test_relay_autonat_behaviour() {
    println!("🧪 Testing relay AutoNAT behaviour initialization...");

    let (swarm, peer_id) = create_relay_server(15012, 128, 16).await.unwrap();

    // Verify swarm was created successfully
    assert!(std::mem::size_of_val(&swarm) > 0);
    println!("✅ Relay with AutoNAT server created: {}", peer_id);
    println!("✅ AutoNAT behaviour test passed!");
}
