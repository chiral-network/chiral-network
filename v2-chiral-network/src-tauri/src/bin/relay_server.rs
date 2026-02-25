//! Standalone v2-compatible relay server for the Chiral Network bootstrap node.
//!
//! Uses the same libp2p 0.53 + relay 0.17 as v2 peers, ensuring protocol compatibility.
//! Also runs an HTTP gateway for hosting static sites uploaded by peers.
//!
//! Usage:
//!   relay_server [--port PORT] [--secret SECRET] [--http-port HTTP_PORT]
//!
//! The secret is used to derive a deterministic keypair for a stable PeerId.

use libp2p::{
    kad, noise, ping, relay, identify,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, StreamProtocol,
};
use futures::StreamExt;
use std::error::Error;
use std::sync::Arc;
use sha2::{Sha256, Digest};

use chiral_network_v2_lib::drive_api::DriveState;
use chiral_network_v2_lib::hosting_server::{self, HostingServerState};

#[derive(NetworkBehaviour)]
struct RelayServerBehaviour {
    relay_server: relay::Behaviour,
    kad: kad::Behaviour<kad::store::MemoryStore>,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

fn keypair_from_secret(secret: &str) -> libp2p::identity::Keypair {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let hash = hasher.finalize();
    // ed25519 needs exactly 32 bytes for the secret key
    let secret_key = libp2p::identity::ed25519::SecretKey::try_from_bytes(hash.to_vec())
        .expect("SHA-256 produces valid 32-byte ed25519 secret key");
    let ed25519_keypair = libp2p::identity::ed25519::Keypair::from(secret_key);
    libp2p::identity::Keypair::from(ed25519_keypair)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();

    let mut port: u16 = 4001;
    let mut http_port: u16 = 8080;
    let mut secret = String::from("chiral-relay-server-default");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().expect("Invalid port number");
                    i += 2;
                } else {
                    eprintln!("--port requires a value");
                    std::process::exit(1);
                }
            }
            "--http-port" => {
                if i + 1 < args.len() {
                    http_port = args[i + 1].parse().expect("Invalid HTTP port number");
                    i += 2;
                } else {
                    eprintln!("--http-port requires a value");
                    std::process::exit(1);
                }
            }
            "--secret" => {
                if i + 1 < args.len() {
                    secret = args[i + 1].clone();
                    i += 2;
                } else {
                    eprintln!("--secret requires a value");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    // Generate deterministic keypair from secret
    let local_key = keypair_from_secret(&secret);
    let local_peer_id = PeerId::from(local_key.public());

    println!("=== Chiral Network v2 Relay Server ===");
    println!("PeerId: {}", local_peer_id);
    println!("P2P Port: {}", port);
    println!("HTTP Port: {}", http_port);

    // -----------------------------------------------------------------------
    // Start HTTP gateway for hosting
    // -----------------------------------------------------------------------

    let hosting_state = Arc::new(HostingServerState::new());
    hosting_state.load_from_disk().await;

    // Initialize Drive state
    let drive_state = Arc::new(DriveState::new());
    drive_state.load_from_disk_async().await;
    println!("Drive state loaded from disk");

    let (http_shutdown_tx, http_shutdown_rx) = tokio::sync::oneshot::channel();

    match hosting_server::start_gateway_server(
        Arc::clone(&hosting_state),
        Some(Arc::clone(&drive_state)),
        http_port,
        http_shutdown_rx,
    )
    .await
    {
        Ok(addr) => println!("HTTP gateway listening on http://{}", addr),
        Err(e) => eprintln!("WARNING: Failed to start HTTP gateway: {}", e),
    }

    // Keep shutdown sender alive — drop it on process exit
    let _http_shutdown = http_shutdown_tx;

    // -----------------------------------------------------------------------
    // Configure libp2p
    // -----------------------------------------------------------------------

    // Configure Kademlia
    let kad_store = kad::store::MemoryStore::new(local_peer_id);
    let mut kad_config = kad::Config::default();
    kad_config.set_protocol_names(vec![StreamProtocol::new("/chiral/kad/1.0.0")]);
    let mut kad = kad::Behaviour::with_config(local_peer_id, kad_store, kad_config);
    kad.set_mode(Some(kad::Mode::Server));

    // Configure Identify - accept both v1 and v2 protocol versions
    let identify = identify::Behaviour::new(
        identify::Config::new(
            "/chiral/id/1.0.0".to_string(),
            local_key.public(),
        ),
    );

    let ping = ping::Behaviour::new(
        ping::Config::new().with_interval(std::time::Duration::from_secs(15)),
    );

    // Configure relay server with limits sized for file transfers.
    // Default max_circuit_bytes is 128 KiB — far too small for chunked file
    // transfers (each chunk is 256 KB). Increase to allow full downloads.
    let mut relay_config = relay::Config::default();
    relay_config.max_circuit_bytes = 1 << 27; // 128 MiB per circuit
    relay_config.max_circuit_duration = std::time::Duration::from_secs(30 * 60); // 30 min
    relay_config.max_circuits = 256;
    relay_config.max_circuits_per_peer = 16;
    let relay_server = relay::Behaviour::new(local_peer_id, relay_config);

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_key| {
            RelayServerBehaviour {
                relay_server,
                kad,
                ping,
                identify,
            }
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(3600)))
        .build();

    // Listen on specified port (all interfaces)
    let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
    swarm.listen_on(listen_addr.clone())?;
    println!("Listening on /ip4/0.0.0.0/tcp/{}", port);

    // Also listen on IPv6
    let listen_addr_v6: Multiaddr = format!("/ip6/::/tcp/{}", port).parse()?;
    swarm.listen_on(listen_addr_v6)?;
    println!("Listening on /ip6/::/tcp/{}", port);

    // CRITICAL: Add external addresses explicitly so relay RESERVE_OK responses
    // include the server's public addresses. Without this, the first RESERVE_OK
    // would contain EMPTY addresses (because Identify hasn't discovered external
    // addresses yet), causing clients to fail with NoAddressesInReservation.
    // The relay client then resets Reservation to None, denying all STOP requests
    // with NO_RESERVATION.
    //
    // We also add listen addresses as external since 0.0.0.0 is not routable.
    // On a server, the actual IPs will be discovered by Identify, but we need
    // at least one address available before the first Identify exchange.
    println!("Adding external addresses from listen interfaces...");

    // Collect all listen addresses, then add routable ones as external.
    // Skip unspecified (0.0.0.0, ::), loopback (127.x, ::1), and link-local (fe80::).
    let mut external_addrs: Vec<Multiaddr> = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("[LISTEN] New listen address: {}", address);
                        let addr_str = address.to_string();
                        // Skip non-routable addresses
                        let is_routable = !addr_str.contains("0.0.0.0")
                            && !addr_str.contains("127.0.0.1")
                            && !addr_str.contains("/0/")
                            && !addr_str.contains("/::1/")
                            && !addr_str.contains("fe80::");
                        if is_routable {
                            external_addrs.push(address);
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                // If we already have addresses and no new ones are coming, stop waiting
                if !external_addrs.is_empty() {
                    break;
                }
            }
        }
    }
    if external_addrs.is_empty() {
        // Fallback: add the explicit listen address
        let fallback_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}/p2p/{}", port, local_peer_id).parse()?;
        println!("[EXTERNAL] Fallback: {}", fallback_addr);
        swarm.add_external_address(fallback_addr);
    } else {
        for addr in external_addrs {
            let external_addr = addr.with(libp2p::multiaddr::Protocol::P2p(local_peer_id));
            println!("[EXTERNAL] Adding: {}", external_addr);
            swarm.add_external_address(external_addr);
        }
    }

    println!("\nRelay server ready. Waiting for connections...\n");

    // Print the multiaddr that peers should use
    println!("Peers should connect to:");
    println!("  /ip4/<YOUR_PUBLIC_IP>/tcp/{}/p2p/{}", port, local_peer_id);
    println!();

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(event) => {
                match event {
                    RelayServerBehaviourEvent::RelayServer(event) => {
                        println!("[RELAY] {:?}", event);
                    }
                    RelayServerBehaviourEvent::Kad(event) => {
                        match &event {
                            kad::Event::RoutingUpdated { peer, .. } => {
                                println!("[KAD] Routing updated for peer: {}", peer);
                            }
                            kad::Event::InboundRequest { request } => {
                                println!("[KAD] Inbound request: {:?}", request);
                            }
                            _ => {}
                        }
                    }
                    RelayServerBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. }) => {
                        println!("[IDENTIFY] Peer {}: protocol={}, agent={}, addrs={:?}",
                            peer_id, info.protocol_version, info.agent_version, info.listen_addrs);

                        // Add peer's listen addresses to Kademlia
                        for addr in &info.listen_addrs {
                            swarm.behaviour_mut().kad.add_address(&peer_id, addr.clone());
                        }
                    }
                    RelayServerBehaviourEvent::Identify(_) => {}
                    RelayServerBehaviourEvent::Ping(_) => {}
                }
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("[LISTEN] New listen address: {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, .. } => {
                println!("[CONN] Connection established with {} (total: {}, addr: {})",
                    peer_id, num_established, endpoint.get_remote_address());
            }
            SwarmEvent::ConnectionClosed { peer_id, num_established, cause, .. } => {
                println!("[CONN] Connection closed with {} (remaining: {}, cause: {:?})",
                    peer_id, num_established, cause);
            }
            SwarmEvent::IncomingConnection { local_addr, send_back_addr, .. } => {
                println!("[CONN] Incoming connection: local={}, remote={}", local_addr, send_back_addr);
            }
            SwarmEvent::IncomingConnectionError { local_addr, send_back_addr, error, .. } => {
                println!("[ERROR] Incoming connection error: local={}, remote={}, err={:?}",
                    local_addr, send_back_addr, error);
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                if let Some(peer) = peer_id {
                    println!("[ERROR] Outgoing connection error to {}: {:?}", peer, error);
                }
            }
            SwarmEvent::ExternalAddrConfirmed { address, .. } => {
                println!("[ADDR] External address confirmed: {}", address);
            }
            _ => {}
        }
    }
}
