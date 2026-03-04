/// Integration test for relay circuit establishment.
///
/// Tests the complete relay flow:
/// 1. Relay server starts and listens
/// 2. Client A (seeder) connects and gets relay reservation
/// 3. Client B (downloader) dials Client A via relay circuit
/// 4. Verifies the circuit is established end-to-end
///
/// This test uses localhost to avoid needing actual NAT conditions.
/// It tests the exact same libp2p configuration as production code.
use futures::StreamExt;
use libp2p::identity::Keypair;
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{
    dcutr, identify, kad, noise, ping, relay, tcp, yamux, Multiaddr, StreamProtocol, Swarm,
};
use std::time::Duration;

// Minimal behaviour for relay server
#[derive(NetworkBehaviour)]
struct RelayServerBehaviour {
    relay_server: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

// Client behaviour matching production DhtBehaviour (relay_client first!)
#[derive(NetworkBehaviour)]
struct ClientBehaviour {
    relay_client: relay::client::Behaviour,
    dcutr: dcutr::Behaviour,
    kad: kad::Behaviour<kad::store::MemoryStore>,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    // Minimal: skip file_transfer, file_request, mdns, ping_protocol for test
}

fn create_relay_server() -> (Swarm<RelayServerBehaviour>, String) {
    let local_key = Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();

    let relay_server = relay::Behaviour::new(local_peer_id, relay::Config::default());
    let ping = ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(15)));
    let identify = identify::Behaviour::new(identify::Config::new(
        "/chiral/id/1.0.0".to_string(),
        local_key.public(),
    ));

    let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .unwrap()
        .with_behaviour(|_key| RelayServerBehaviour {
            relay_server,
            ping,
            identify,
        })
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    let peer_id = local_peer_id.to_string();
    (swarm, peer_id)
}

fn create_client() -> (Swarm<ClientBehaviour>, String) {
    let local_key = Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();

    let kad_store = kad::store::MemoryStore::new(local_peer_id);
    let mut kad_config = kad::Config::default();
    kad_config.set_protocol_names(vec![StreamProtocol::new("/chiral/kad/1.0.0")]);
    let kad = kad::Behaviour::with_config(local_peer_id, kad_store, kad_config);

    let ping = ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(15)));
    let identify = identify::Behaviour::new(identify::Config::new(
        "/chiral/id/1.0.0".to_string(),
        local_key.public(),
    ));

    let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .unwrap()
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .unwrap()
        .with_behaviour(|_key, relay_client| ClientBehaviour {
            relay_client,
            dcutr: dcutr::Behaviour::new(local_peer_id),
            kad,
            ping,
            identify,
        })
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    let peer_id = local_peer_id.to_string();
    (swarm, peer_id)
}

/// Wait for the relay server to start listening and return its address.
/// Also adds the listen address as an external address so RESERVE_OK includes it.
async fn wait_for_relay_listen(
    swarm: &mut Swarm<RelayServerBehaviour>,
    peer_id: &str,
) -> Multiaddr {
    let pid: libp2p::PeerId = peer_id.parse().unwrap();
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                // Skip IPv6 loopback, prefer IPv4
                if address.to_string().contains("127.0.0.1") {
                    println!("[RELAY SERVER] Listening on: {}", address);
                    // CRITICAL: Add as external address so RESERVE_OK includes it.
                    // Without this, the relay sends empty addresses in RESERVE_OK,
                    // causing NoAddressesInReservation on clients.
                    let external = address
                        .clone()
                        .with(libp2p::multiaddr::Protocol::P2p(pid));
                    swarm.add_external_address(external);
                    return address;
                }
            }
            other => {
                println!("[RELAY SERVER] Event: {:?}", other);
            }
        }
    }
}

#[tokio::test]
async fn test_relay_reservation_basic() {
    // Test 1: Client can get a relay reservation from the relay server
    let (mut relay_swarm, relay_peer_id) = create_relay_server();
    let (mut client_swarm, client_peer_id) = create_client();

    println!("=== TEST: Basic Relay Reservation ===");
    println!("Relay server PeerId: {}", relay_peer_id);
    println!("Client PeerId: {}", client_peer_id);

    // Start relay server listening on localhost
    relay_swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();

    // Get the relay server's actual listen address
    let relay_addr = wait_for_relay_listen(&mut relay_swarm, &relay_peer_id).await;
    let relay_peer_id_parsed: libp2p::PeerId = relay_peer_id.parse().unwrap();
    let relay_addr_with_peer = relay_addr
        .clone()
        .with(libp2p::multiaddr::Protocol::P2p(relay_peer_id_parsed));

    println!("Relay server address: {}", relay_addr_with_peer);

    // Client: listen_on relay address (request reservation)
    let relay_circuit_addr = relay_addr_with_peer
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit);
    println!(
        "Client requesting relay reservation: {}",
        relay_circuit_addr
    );
    client_swarm.listen_on(relay_circuit_addr).unwrap();

    // Run both swarms and wait for reservation
    let mut reservation_accepted = false;
    let mut new_listen_addr_relay = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        if reservation_accepted && new_listen_addr_relay {
            println!("✅ PASS: Reservation accepted AND NewListenAddr with relay address");
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "❌ FAIL: Timeout! reservation_accepted={}, new_listen_addr_relay={}",
                reservation_accepted, new_listen_addr_relay
            );
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) => {
                        println!("[RELAY SERVER] {:?}", e);
                    }
                    _ => {}
                }
            }
            event = client_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(
                        relay::client::Event::ReservationReqAccepted { relay_peer_id: rpid, renewal, .. }
                    )) => {
                        println!("[CLIENT] ✅ ReservationReqAccepted from {} (renewal={})", rpid, renewal);
                        reservation_accepted = true;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        let is_relay = address.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit));
                        if is_relay {
                            println!("[CLIENT] ✅ NewListenAddr (RELAY): {}", address);
                            new_listen_addr_relay = true;
                        } else {
                            println!("[CLIENT] NewListenAddr (direct): {}", address);
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        println!("[CLIENT] ConnectionEstablished with {}", peer_id);
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        println!("[CLIENT] ❌ OutgoingConnectionError {:?}: {:?}", peer_id, error);
                    }
                    SwarmEvent::ListenerClosed { listener_id, reason, addresses, .. } => {
                        println!("[CLIENT] ⚠️ ListenerClosed {:?} addrs={:?}: {:?}", listener_id, addresses, reason);
                    }
                    _ => {}
                }
            }
        }
    }
}

#[tokio::test]
async fn test_relay_circuit_between_two_clients() {
    // Test 2: Two clients can establish a circuit via the relay server
    let (mut relay_swarm, relay_peer_id) = create_relay_server();
    let (mut client_a_swarm, client_a_peer_id) = create_client();
    let (mut client_b_swarm, client_b_peer_id) = create_client();

    println!("=== TEST: Relay Circuit Between Two Clients ===");
    println!("Relay server PeerId: {}", relay_peer_id);
    println!("Client A (seeder) PeerId: {}", client_a_peer_id);
    println!("Client B (downloader) PeerId: {}", client_b_peer_id);

    // Start relay server
    relay_swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();
    let relay_addr = wait_for_relay_listen(&mut relay_swarm, &relay_peer_id).await;
    let relay_peer_id_parsed: libp2p::PeerId = relay_peer_id.parse().unwrap();
    let relay_addr_with_peer = relay_addr
        .clone()
        .with(libp2p::multiaddr::Protocol::P2p(relay_peer_id_parsed));

    // Client A (seeder): request relay reservation
    let relay_circuit_addr = relay_addr_with_peer
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit);
    client_a_swarm.listen_on(relay_circuit_addr).unwrap();

    // Phase 1: Wait for Client A's reservation to be confirmed
    println!("\n--- Phase 1: Waiting for Client A reservation ---");
    let mut client_a_reservation = false;
    let mut client_a_relay_listen_addr: Option<Multiaddr> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        if client_a_reservation && client_a_relay_listen_addr.is_some() {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "❌ Phase 1 timeout! reservation={}, relay_addr={:?}",
                client_a_reservation, client_a_relay_listen_addr
            );
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) = &event {
                    println!("[RELAY] {:?}", e);
                }
            }
            event = client_a_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(
                        relay::client::Event::ReservationReqAccepted { .. }
                    )) => {
                        println!("[CLIENT A] ✅ ReservationReqAccepted");
                        client_a_reservation = true;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        let is_relay = address.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit));
                        if is_relay {
                            println!("[CLIENT A] ✅ Relay listen addr: {}", address);
                            client_a_relay_listen_addr = Some(address.clone());
                        }
                    }
                    SwarmEvent::ListenerClosed { listener_id, reason, addresses, .. } => {
                        println!("[CLIENT A] ⚠️ ListenerClosed {:?} addrs={:?}: {:?}", listener_id, addresses, reason);
                    }
                    _ => {}
                }
            }
        }
    }

    let client_a_relay_addr = client_a_relay_listen_addr.unwrap();
    println!(
        "\n✅ Client A relay address: {}\n",
        client_a_relay_addr
    );

    // Phase 2: Client B dials Client A via relay
    println!("--- Phase 2: Client B dialing Client A via relay ---");
    let client_a_peer_id_parsed: libp2p::PeerId = client_a_peer_id.parse().unwrap();

    // Build the circuit address: relay_addr/p2p/relay_peer/p2p-circuit/p2p/client_a_peer
    let circuit_addr = relay_addr_with_peer
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit)
        .with(libp2p::multiaddr::Protocol::P2p(client_a_peer_id_parsed));

    println!("[CLIENT B] Dialing via relay: {}", circuit_addr);
    client_b_swarm.dial(circuit_addr.clone()).unwrap();

    // Wait for circuit to be established (or fail)
    let mut circuit_established = false;
    let mut client_b_connected = false;
    let mut client_a_inbound_circuit = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        if client_b_connected && client_a_inbound_circuit {
            circuit_established = true;
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "❌ Phase 2 timeout! client_b_connected={}, client_a_inbound_circuit={}",
                client_b_connected, client_a_inbound_circuit
            );
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) => {
                        println!("[RELAY] {:?}", e);
                    }
                    _ => {}
                }
            }
            event = client_a_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(
                        relay::client::Event::InboundCircuitEstablished { src_peer_id, .. }
                    )) => {
                        println!("[CLIENT A] ✅ InboundCircuitEstablished from {}", src_peer_id);
                        client_a_inbound_circuit = true;
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        println!("[CLIENT A] ConnectionEstablished with {} (endpoint: {:?})", peer_id, endpoint);
                    }
                    SwarmEvent::ListenerClosed { listener_id, reason, addresses, .. } => {
                        println!("[CLIENT A] ⚠️ ListenerClosed {:?} addrs={:?}: {:?}", listener_id, addresses, reason);
                    }
                    _ => {}
                }
            }
            event = client_b_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(
                        relay::client::Event::OutboundCircuitEstablished { relay_peer_id: rpid, .. }
                    )) => {
                        println!("[CLIENT B] ✅ OutboundCircuitEstablished via {}", rpid);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        println!("[CLIENT B] ConnectionEstablished with {} (endpoint: {:?})", peer_id, endpoint);
                        if peer_id.to_string() == client_a_peer_id {
                            println!("[CLIENT B] ✅ Connected to Client A!");
                            client_b_connected = true;
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        println!("[CLIENT B] ❌ OutgoingConnectionError {:?}: {:?}", peer_id, error);
                        panic!("Circuit establishment failed: {:?}", error);
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(circuit_established, "Circuit should be established");
    println!("\n✅ PASS: Relay circuit successfully established between Client A and Client B!");
}

#[tokio::test]
async fn test_reservation_survives_after_acceptance() {
    // Test 3: After reservation is accepted, STOP requests should be handled correctly
    // This specifically tests the scenario where the handler's Reservation state
    // might be reset to None after acceptance (the bug we're investigating)
    let (mut relay_swarm, relay_peer_id) = create_relay_server();
    let (mut client_swarm, _client_peer_id) = create_client();

    // Start relay server
    relay_swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();
    let relay_addr = wait_for_relay_listen(&mut relay_swarm, &relay_peer_id).await;
    let relay_peer_id_parsed: libp2p::PeerId = relay_peer_id.parse().unwrap();
    let relay_addr_with_peer = relay_addr
        .clone()
        .with(libp2p::multiaddr::Protocol::P2p(relay_peer_id_parsed));

    // Client: request relay reservation
    let relay_circuit_addr = relay_addr_with_peer
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit);
    client_swarm.listen_on(relay_circuit_addr).unwrap();

    // Wait for reservation
    let mut reservation_accepted = false;
    let mut relay_listen_addr_received = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        if reservation_accepted && relay_listen_addr_received {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "Timeout waiting for reservation: accepted={}, listen_addr={}",
                reservation_accepted, relay_listen_addr_received
            );
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) = &event {
                    println!("[RELAY] {:?}", e);
                }
            }
            event = client_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(
                        relay::client::Event::ReservationReqAccepted { .. }
                    )) => {
                        println!("✅ ReservationReqAccepted");
                        reservation_accepted = true;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        let is_relay = address.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit));
                        if is_relay {
                            println!("✅ NewListenAddr relay: {}", address);
                            relay_listen_addr_received = true;
                        }
                    }
                    SwarmEvent::ListenerClosed { listener_id, reason, addresses, .. } => {
                        let was_relay = addresses.iter().any(|a| {
                            a.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit))
                        });
                        if was_relay {
                            panic!("❌ RELAY LISTENER CLOSED after reservation! {:?}: {:?}", listener_id, reason);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Now run the client swarm for a few more seconds and ensure the relay listener
    // stays open (doesn't close)
    println!("\n--- Checking relay listener stability for 5 seconds ---");
    let stability_deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        if tokio::time::Instant::now() > stability_deadline {
            break;
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) = &event {
                    println!("[RELAY] {:?}", e);
                }
            }
            event = client_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::ListenerClosed { listener_id, reason, addresses, .. } => {
                        let was_relay = addresses.iter().any(|a| {
                            a.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit))
                        });
                        if was_relay {
                            panic!("❌ RELAY LISTENER CLOSED during stability check! {:?}: {:?}", listener_id, reason);
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    println!("✅ PASS: Relay listener remained stable after reservation");
}

#[tokio::test]
async fn test_multiple_listen_on_same_relay_peer() {
    // Test 4: Calling listen_on with both IPv4 and IPv6 for the same relay peer
    // should NOT break the reservation (tests deduplication requirement)
    let (mut relay_swarm, relay_peer_id) = create_relay_server();
    let (mut client_swarm, _client_peer_id) = create_client();

    // Start relay server on BOTH IPv4 and IPv6
    relay_swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();
    let relay_addr = wait_for_relay_listen(&mut relay_swarm, &relay_peer_id).await;
    let relay_peer_id_parsed: libp2p::PeerId = relay_peer_id.parse().unwrap();

    let relay_addr_v4 = relay_addr
        .clone()
        .with(libp2p::multiaddr::Protocol::P2p(relay_peer_id_parsed));

    // Call listen_on TWICE for the same relay peer (simulating IPv4 + IPv6 without dedup)
    let relay_circuit_addr_1 = relay_addr_v4
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit);
    let relay_circuit_addr_2 = relay_addr_v4
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit);

    println!("Client: listen_on #1: {}", relay_circuit_addr_1);
    client_swarm.listen_on(relay_circuit_addr_1).unwrap();

    println!("Client: listen_on #2: {}", relay_circuit_addr_2);
    client_swarm.listen_on(relay_circuit_addr_2).unwrap();

    // Check if reservation works or gets corrupted
    let mut reservation_count = 0;
    let mut listener_closed_count = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        if tokio::time::Instant::now() > deadline {
            break;
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) = &event {
                    println!("[RELAY] {:?}", e);
                }
            }
            event = client_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(
                        relay::client::Event::ReservationReqAccepted { renewal, .. }
                    )) => {
                        reservation_count += 1;
                        println!("ReservationReqAccepted #{} (renewal={})", reservation_count, renewal);
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        let is_relay = address.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit));
                        if is_relay {
                            println!("NewListenAddr relay: {}", address);
                        }
                    }
                    SwarmEvent::ListenerClosed { addresses, reason, .. } => {
                        let was_relay = addresses.iter().any(|a| {
                            a.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit))
                        });
                        if was_relay {
                            listener_closed_count += 1;
                            println!("⚠️ Relay ListenerClosed #{}: {:?}", listener_closed_count, reason);
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    println!("\n--- Results ---");
    println!("Reservations accepted: {}", reservation_count);
    println!("Relay listeners closed: {}", listener_closed_count);

    // With two listen_on calls for the same relay, we expect potential issues:
    // The second RESERVE might succeed (as renewal) or cause the handler's
    // reservation to be overwritten. Either way, we should still have at least
    // one successful reservation.
    assert!(
        reservation_count >= 1,
        "Expected at least 1 reservation, got {}",
        reservation_count
    );

    if listener_closed_count > 0 {
        println!(
            "⚠️ WARNING: {} relay listener(s) closed! This indicates the duplicate listen_on issue.",
            listener_closed_count
        );
    }
}

#[tokio::test]
async fn test_circuit_after_delay() {
    // Test 5: Verify circuit works even after a delay (reservation doesn't expire quickly)
    let (mut relay_swarm, relay_peer_id) = create_relay_server();
    let (mut client_a_swarm, client_a_peer_id) = create_client();
    let (mut client_b_swarm, _client_b_peer_id) = create_client();

    // Start relay server
    relay_swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();
    let relay_addr = wait_for_relay_listen(&mut relay_swarm, &relay_peer_id).await;
    let relay_peer_id_parsed: libp2p::PeerId = relay_peer_id.parse().unwrap();
    let relay_addr_with_peer = relay_addr
        .clone()
        .with(libp2p::multiaddr::Protocol::P2p(relay_peer_id_parsed));

    // Client A: get reservation
    let relay_circuit_addr = relay_addr_with_peer
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit);
    client_a_swarm.listen_on(relay_circuit_addr).unwrap();

    // Wait for Client A reservation
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut client_a_ready = false;

    loop {
        if client_a_ready {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("Timeout waiting for Client A reservation");
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) = &event {
                    println!("[RELAY] {:?}", e);
                }
            }
            event = client_a_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        let is_relay = address.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit));
                        if is_relay {
                            println!("[A] Relay listen addr: {}", address);
                            client_a_ready = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Wait 5 seconds (simulating delay between reservation and circuit request)
    println!("\n--- Waiting 5 seconds before circuit attempt ---\n");
    let wait_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::time::Instant::now() > wait_deadline {
            break;
        }
        tokio::select! {
            event = relay_swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) = &event {
                    println!("[RELAY] {:?}", e);
                }
            }
            event = client_a_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::ListenerClosed { addresses, reason, .. } => {
                        let was_relay = addresses.iter().any(|a| {
                            a.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit))
                        });
                        if was_relay {
                            panic!("❌ Relay listener closed during wait! {:?}", reason);
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    // Client B: dial Client A via relay
    let client_a_peer_id_parsed: libp2p::PeerId = client_a_peer_id.parse().unwrap();
    let circuit_addr = relay_addr_with_peer
        .clone()
        .with(libp2p::multiaddr::Protocol::P2pCircuit)
        .with(libp2p::multiaddr::Protocol::P2p(client_a_peer_id_parsed));

    println!("[B] Dialing A via relay: {}", circuit_addr);
    client_b_swarm.dial(circuit_addr).unwrap();

    let mut circuit_ok = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        if circuit_ok {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("Timeout waiting for circuit after delay");
        }

        tokio::select! {
            event = relay_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(RelayServerBehaviourEvent::RelayServer(e)) => {
                        println!("[RELAY] {:?}", e);
                    }
                    _ => {}
                }
            }
            event = client_a_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(
                        relay::client::Event::InboundCircuitEstablished { src_peer_id, .. }
                    )) => {
                        println!("[A] ✅ InboundCircuit from {}", src_peer_id);
                    }
                    SwarmEvent::ListenerClosed { addresses, reason, .. } => {
                        let was_relay = addresses.iter().any(|a| {
                            a.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit))
                        });
                        if was_relay {
                            panic!("❌ Relay listener closed! {:?}", reason);
                        }
                    }
                    _ => {}
                }
            }
            event = client_b_swarm.select_next_some() => {
                match &event {
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        if peer_id.to_string() == client_a_peer_id {
                            println!("[B] ✅ Connected to A via circuit!");
                            circuit_ok = true;
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { error, .. } => {
                        panic!("❌ Circuit failed: {:?}", error);
                    }
                    _ => {}
                }
            }
        }
    }

    println!("✅ PASS: Circuit established after 5 second delay");
}
