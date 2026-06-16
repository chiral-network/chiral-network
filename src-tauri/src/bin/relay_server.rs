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

use chiral_network::hosting_server::{self, HostingServerState};
use chiral_network::rating_storage::RatingState;
use chiral_network::relay_share_proxy::RelayShareRegistry;

#[derive(NetworkBehaviour)]
struct RelayServerBehaviour {
    relay_server: relay::Behaviour,
    kad: kad::Behaviour<kad::store::MemoryStore>,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelayServerArgs {
    port: u16,
    http_port: u16,
    secret: String,
}

impl Default for RelayServerArgs {
    fn default() -> Self {
        Self {
            port: 4001,
            http_port: 8080,
            secret: String::from("chiral-relay-server-default"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn relay_server_args_use_default_ports() {
        let parsed =
            parse_relay_server_args(&args(&["relay_server"])).expect("default args should parse");

        assert_eq!(parsed.port, 4001);
        assert_eq!(parsed.http_port, 8080);
        assert_eq!(parsed.secret, "chiral-relay-server-default");
    }

    #[test]
    fn relay_server_args_accept_port_overrides() {
        let parsed = parse_relay_server_args(&args(&[
            "relay_server",
            "--port",
            "4100",
            "--http-port",
            "8181",
            "--secret",
            "custom-secret",
        ]))
        .expect("valid args should parse");

        assert_eq!(parsed.port, 4100);
        assert_eq!(parsed.http_port, 8181);
        assert_eq!(parsed.secret, "custom-secret");
    }

    #[test]
    fn relay_server_args_reject_invalid_port() {
        let err = parse_relay_server_args(&args(&["relay_server", "--port", "not-a-port"]))
            .expect_err("invalid port should be rejected");

        assert!(err.contains("--port"));
        assert!(err.contains("not-a-port"));
    }

    #[test]
    fn relay_server_args_reject_invalid_http_port() {
        let err = parse_relay_server_args(&args(&["relay_server", "--http-port", "70000"]))
            .expect_err("out-of-range HTTP port should be rejected");

        assert!(err.contains("--http-port"));
        assert!(err.contains("70000"));
    }
}

fn parse_port_arg(flag: &str, value: &str) -> Result<u16, String> {
    value.parse::<u16>().map_err(|_| {
        format!(
            "{} requires a valid port number from 0 to 65535, got '{}'",
            flag, value
        )
    })
}

fn parse_relay_server_args(args: &[String]) -> Result<RelayServerArgs, String> {
    let mut parsed = RelayServerArgs::default();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                if i + 1 < args.len() {
                    parsed.port = parse_port_arg("--port", &args[i + 1])?;
                    i += 2;
                } else {
                    return Err("--port requires a value".to_string());
                }
            }
            "--http-port" => {
                if i + 1 < args.len() {
                    parsed.http_port = parse_port_arg("--http-port", &args[i + 1])?;
                    i += 2;
                } else {
                    return Err("--http-port requires a value".to_string());
                }
            }
            "--secret" => {
                if i + 1 < args.len() {
                    parsed.secret = args[i + 1].clone();
                    i += 2;
                } else {
                    return Err("--secret requires a value".to_string());
                }
            }
            _ => {
                return Err(format!("Unknown argument: {}", args[i]));
            }
        }
    }

    Ok(parsed)
}

fn relay_secret_key_from_bytes(
    bytes: Vec<u8>,
) -> Result<libp2p::identity::ed25519::SecretKey, String> {
    libp2p::identity::ed25519::SecretKey::try_from_bytes(bytes).map_err(|err| {
        format!(
            "Relay secret did not derive a valid 32-byte Ed25519 key: {:?}",
            err
        )
    })
}

fn keypair_from_secret(secret: &str) -> Result<libp2p::identity::Keypair, String> {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let hash = hasher.finalize();
    // ed25519 needs exactly 32 bytes for the secret key
    let secret_key = relay_secret_key_from_bytes(hash.to_vec())?;
    let ed25519_keypair = libp2p::identity::ed25519::Keypair::from(secret_key);
    Ok(libp2p::identity::Keypair::from(ed25519_keypair))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();

    chiral_network::version::log_policy_key_status();
    let parsed_args = match parse_relay_server_args(&args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let RelayServerArgs {
        port,
        http_port,
        secret,
    } = parsed_args;

    // Generate deterministic keypair from secret
    let local_key = match keypair_from_secret(&secret) {
        Ok(keypair) => keypair,
        Err(err) => {
            eprintln!("ERROR: {}", err);
            std::process::exit(1);
        }
    };
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

    // Initialize Rating state
    let rating_data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("chiral-network");
    let rating_state = Arc::new(RatingState::new(rating_data_dir));
    println!("Rating state loaded from disk");

    // Initialize relay share registry (metadata only — no file storage)
    let relay_share_data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("chiral-network");
    let relay_share_state = Arc::new(RelayShareRegistry::new(relay_share_data_dir));
    relay_share_state.load_from_disk().await;
    println!("Relay share registry loaded from disk");

    let (http_shutdown_tx, http_shutdown_rx) = tokio::sync::oneshot::channel();

    match hosting_server::start_gateway_server(
        Arc::clone(&hosting_state),
        None, // No local Drive API on relay
        Some(Arc::clone(&rating_state)),
        Some(Arc::clone(&relay_share_state)),
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

    // Configure Identify - accept both v1 and v2 protocol versions.
    // Phase 4 of version enforcement: stamp the relay's compile-time
    // version in agent_version so peers can drop us if we get out of
    // date, and we'll likewise drop peers that come in too old.
    let identify = identify::Behaviour::new(
        identify::Config::new(
            "/chiral/id/1.0.0".to_string(),
            local_key.public(),
        )
        .with_agent_version(chiral_network::version::agent_version_string()),
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
                        // Drop peers whose Identify says they're below the
                        // currently-effective min_required. Reads the
                        // RwLock-backed global slot so a signed policy
                        // update tightening the floor takes effect on
                        // live connections without redeploying the relay.
                        {
                            let policy = chiral_network::version::effective_policy();
                            let agent_v = info
                                .agent_version
                                .trim_start_matches("chiral/")
                                .trim_start_matches('v');
                            if chiral_network::version::version_is_below(agent_v, &policy.min_required) {
                                println!(
                                    "🚫 [IDENTIFY] Disconnecting {} — agent_version='{}' < min_required={}",
                                    peer_id, info.agent_version, policy.min_required
                                );
                                let _ = swarm.disconnect_peer_id(peer_id);
                                continue;
                            }
                        }

                        // Only add routable addresses to Kademlia:
                        // - Relay circuit addresses (always reachable)
                        // - Public IPs (not private/link-local/loopback)
                        // This ensures remote peers get usable addresses from FindNode queries.
                        let mut added = 0usize;
                        for addr in &info.listen_addrs {
                            let s = addr.to_string();
                            let is_circuit = s.contains("p2p-circuit");
                            let is_private =
                                s.starts_with("/ip4/127.")
                                || s.starts_with("/ip4/10.")
                                || s.starts_with("/ip4/192.168.")
                                || s.starts_with("/ip6/::1/")
                                || s.starts_with("/ip6/fe80:")
                                || (s.starts_with("/ip4/172.") && {
                                    s.strip_prefix("/ip4/172.")
                                        .and_then(|r| r.split('.').next())
                                        .and_then(|o| o.parse::<u8>().ok())
                                        .map(|o| (16..=31).contains(&o))
                                        .unwrap_or(true)
                                });

                            if is_circuit || !is_private {
                                swarm.behaviour_mut().kad.add_address(&peer_id, addr.clone());
                                added += 1;
                            }
                        }
                        println!("[IDENTIFY] Peer {}: added {}/{} routable addrs to Kademlia",
                            peer_id, added, info.listen_addrs.len());
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
            SwarmEvent::OutgoingConnectionError { .. } => {
                // Routine P2P dial churn — silently ignored.
            }
            SwarmEvent::ExternalAddrConfirmed { address, .. } => {
                println!("[ADDR] External address confirmed: {}", address);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_from_secret_is_deterministic() {
        let first = keypair_from_secret("stable relay secret")
            .expect("secret hash should produce a keypair")
            .public()
            .to_peer_id();
        let second = keypair_from_secret("stable relay secret")
            .expect("secret hash should produce a keypair")
            .public()
            .to_peer_id();

        assert_eq!(first, second);
    }

    #[test]
    fn relay_secret_key_reports_invalid_key_material() {
        let err = match relay_secret_key_from_bytes(vec![0; 31]) {
            Ok(_) => panic!("short key material should fail"),
            Err(err) => err,
        };

        assert!(err.contains("Relay secret"));
        assert!(err.contains("32-byte Ed25519 key"));
    }
}
