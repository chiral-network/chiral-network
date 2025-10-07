/// Chiral Network - Dedicated Circuit Relay v2 Daemon
///
/// This is a standalone relay node that helps NAT-traversal for Chiral Network peers.
/// It implements libp2p Circuit Relay v2 protocol to allow peers behind restrictive NATs
/// to communicate with each other.
///
/// Usage:
///   chiral-relay --port 4001 --external-address /ip4/YOUR_PUBLIC_IP/tcp/4001
///
/// Features:
/// - Circuit Relay v2 with configurable reservation limits
/// - AutoNAT v2 server for reachability detection
/// - Identify protocol for peer information
/// - Health check endpoint via metrics
/// - Graceful shutdown handling

use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use libp2p::{
    autonat, identify, identity,
    multiaddr::Protocol,
    noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId,
};
use std::{
    net::Ipv4Addr,
    path::PathBuf,
    time::Duration,
};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "chiral-relay")]
#[command(about = "Chiral Network Circuit Relay v2 Daemon", long_about = None)]
struct Args {
    /// TCP port to listen on
    #[arg(short, long, default_value_t = 4001)]
    port: u16,

    /// External address for this relay node (e.g., /ip4/1.2.3.4/tcp/4001)
    /// This address will be advertised to other peers
    #[arg(short, long)]
    external_address: Option<Multiaddr>,

    /// Maximum number of concurrent relay reservations
    #[arg(long, default_value_t = 128)]
    max_reservations: usize,

    /// Maximum number of concurrent relay circuits
    #[arg(long, default_value_t = 16)]
    max_circuits: usize,

    /// Path to store the node's identity key (persists peer ID across restarts)
    #[arg(long)]
    identity_path: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Write PID file to this path
    #[arg(long)]
    pid_file: Option<PathBuf>,

    /// Write metrics/status JSON to this path periodically
    #[arg(long)]
    metrics_file: Option<PathBuf>,
}

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    autonat: autonat::Behaviour,
}

#[derive(serde::Serialize)]
struct Metrics {
    peer_id: String,
    listen_addresses: Vec<String>,
    connected_peers: usize,
    uptime_seconds: u64,
    relay_reservations: usize,
    relay_circuits: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Setup logging
    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(log_level)),
        )
        .init();

    info!("🚀 Starting Chiral Network Relay Daemon");

    // Load or generate identity
    let local_key = if let Some(path) = &args.identity_path {
        if path.exists() {
            info!("📂 Loading identity from {}", path.display());
            let bytes = std::fs::read(path)?;
            identity::Keypair::from_protobuf_encoding(&bytes)?
        } else {
            info!("🔑 Generating new identity and saving to {}", path.display());
            let keypair = identity::Keypair::generate_ed25519();
            let bytes = keypair.to_protobuf_encoding()?;
            std::fs::write(path, bytes)?;
            keypair
        }
    } else {
        info!("🔑 Generating ephemeral identity (use --identity-path to persist)");
        identity::Keypair::generate_ed25519()
    };

    let local_peer_id = PeerId::from(local_key.public());
    info!("📋 Peer ID: {}", local_peer_id);

    // Write PID file if requested
    if let Some(pid_path) = &args.pid_file {
        let pid = std::process::id();
        std::fs::write(pid_path, pid.to_string())?;
        info!("📝 PID {} written to {}", pid, pid_path.display());
    }

    // Configure relay behaviour with limits
    let relay_config = relay::Config {
        max_reservations: args.max_reservations,
        max_circuits: args.max_circuits,
        reservation_duration: Duration::from_secs(3600), // 1 hour
        ..Default::default()
    };

    let behaviour = RelayBehaviour {
        relay: relay::Behaviour::new(local_peer_id, relay_config),
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/chiral/relay/1.0.0".to_string(),
            local_key.public(),
        )),
        autonat: autonat::Behaviour::new(local_peer_id, autonat::Config::default()),
    };

    // Build the swarm using the manual approach compatible with libp2p 0.54
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

    // Listen on specified port
    let listen_addr = Multiaddr::empty()
        .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
        .with(Protocol::Tcp(args.port));

    swarm.listen_on(listen_addr.clone())?;
    info!("👂 Listening on {}", listen_addr);

    // Add external address if provided
    if let Some(external) = args.external_address {
        swarm.add_external_address(external.clone());
        info!("🌐 External address: {}", external);
        info!(
            "📋 Full multiaddr: {}/p2p/{}",
            external, local_peer_id
        );
    }

    let start_time = std::time::Instant::now();
    let mut connected_peers = 0usize;
    let mut reservation_count = 0usize;
    let mut circuit_count = 0usize;

    // Main event loop
    info!("✅ Relay daemon is running");
    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("🎧 New listen address: {}", address);
                    }
                    SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(relay_event)) => {
                        match relay_event {
                            relay::Event::ReservationReqAccepted { src_peer_id, .. } => {
                                reservation_count += 1;
                                info!("✅ Reservation accepted for peer: {}", src_peer_id);
                            }
                            relay::Event::ReservationReqDenied { src_peer_id } => {
                                warn!("⚠️  Reservation denied for peer: {}", src_peer_id);
                            }
                            relay::Event::ReservationTimedOut { src_peer_id } => {
                                reservation_count = reservation_count.saturating_sub(1);
                                debug!("⏱️  Reservation timed out for peer: {}", src_peer_id);
                            }
                            relay::Event::ReservationReqAcceptFailed { src_peer_id, error } => {
                                error!("❌ Failed to accept reservation from {}: {:?}", src_peer_id, error);
                            }
                            relay::Event::ReservationReqDenyFailed { src_peer_id, error } => {
                                error!("❌ Failed to deny reservation from {}: {:?}", src_peer_id, error);
                            }
                            relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id } => {
                                circuit_count += 1;
                                info!("🔗 Circuit established: {} -> {}", src_peer_id, dst_peer_id);
                            }
                            relay::Event::CircuitReqDenied { src_peer_id, dst_peer_id } => {
                                warn!("⚠️  Circuit denied: {} -> {}", src_peer_id, dst_peer_id);
                            }
                            relay::Event::CircuitReqDenyFailed { src_peer_id, dst_peer_id, error } => {
                                error!("❌ Failed to deny circuit {} -> {}: {:?}", src_peer_id, dst_peer_id, error);
                            }
                            relay::Event::CircuitReqAcceptFailed { src_peer_id, dst_peer_id, error } => {
                                error!("❌ Failed to accept circuit {} -> {}: {:?}", src_peer_id, dst_peer_id, error);
                            }
                            relay::Event::CircuitReqOutboundConnectFailed { src_peer_id, dst_peer_id, error } => {
                                error!("❌ Outbound connection failed {} -> {}: {:?}", src_peer_id, dst_peer_id, error);
                            }
                            relay::Event::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                                circuit_count = circuit_count.saturating_sub(1);
                                debug!("❌ Circuit closed: {} -> {}", src_peer_id, dst_peer_id);
                            }
                        }
                    }
                    SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(identify_event)) => {
                        if let identify::Event::Received { peer_id, info, .. } = identify_event {
                            debug!("🔍 Identified peer {}: {:?}", peer_id, info.protocol_version);
                        }
                    }
                    SwarmEvent::Behaviour(RelayBehaviourEvent::Ping(ping_event)) => {
                        match ping_event.result {
                            Ok(rtt) => {
                                debug!("🏓 Ping from {:?}: {:?}", ping_event.peer, rtt);
                            }
                            Err(e) => {
                                debug!("⚠️  Ping failed from {:?}: {:?}", ping_event.peer, e);
                            }
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        connected_peers += 1;
                        info!("🤝 Connection established with peer: {} (total: {})", peer_id, connected_peers);
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        connected_peers = connected_peers.saturating_sub(1);
                        info!("👋 Connection closed with peer: {} (total: {})", peer_id, connected_peers);
                    }
                    SwarmEvent::IncomingConnectionError { error, .. } => {
                        debug!("⚠️  Incoming connection error: {}", error);
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        debug!("⚠️  Outgoing connection error to {:?}: {}", peer_id, error);
                    }
                    _ => {}
                }

                // Periodically write metrics if configured
                if let Some(metrics_path) = &args.metrics_file {
                    let metrics = Metrics {
                        peer_id: local_peer_id.to_string(),
                        listen_addresses: swarm
                            .listeners()
                            .map(|a| a.to_string())
                            .collect(),
                        connected_peers,
                        uptime_seconds: start_time.elapsed().as_secs(),
                        relay_reservations: reservation_count,
                        relay_circuits: circuit_count,
                    };
                    if let Err(e) = std::fs::write(metrics_path, serde_json::to_string_pretty(&metrics)?) {
                        error!("Failed to write metrics: {}", e);
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("⚠️  Received SIGINT, shutting down gracefully...");
                break;
            }
        }
    }

    // Cleanup
    if let Some(pid_path) = &args.pid_file {
        let _ = std::fs::remove_file(pid_path);
    }

    info!("✅ Relay daemon stopped");
    Ok(())
}
