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

/// for relay authentication
mod relay_auth;
use relay_auth::*;


use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use libp2p::{
    autonat, identify, identity,
    multiaddr::Protocol,
    noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId,
    request_response::{Behaviour as RequestResponse, Config as RequestResponseConfig, Event as RequestResponseEvent, ProtocolSupport, Message as RequestResponseMessage},
};
use std::{
    net::Ipv4Addr,
    path::PathBuf,
    time::Duration,
    collections::{HashSet},
    sync::{Arc, Mutex},
};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use axum::{
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tauri::AppHandle; // only if not already imported
use serde_json;       // ensure serde_json is in Cargo.toml

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

// Composite event for all behaviours
#[allow(clippy::large_enum_variant)]
enum RelayBehaviourEvent {
    Relay(relay::Event),
    Ping(ping::Event),
    Identify(identify::Event),
    Autonat(()),
    RelayAuth(RequestResponseEvent<RelayAuthRequest, RelayAuthResponse>),
}
impl From<relay::Event> for RelayBehaviourEvent {
    fn from(e: relay::Event) -> Self { RelayBehaviourEvent::Relay(e) }
}
impl From<ping::Event> for RelayBehaviourEvent {
    fn from(e: ping::Event) -> Self { RelayBehaviourEvent::Ping(e) }
}
impl From<identify::Event> for RelayBehaviourEvent {
    fn from(e: identify::Event) -> Self { RelayBehaviourEvent::Identify(e) }
}
impl From<autonat::Event> for RelayBehaviourEvent {
    fn from(_e: autonat::Event) -> Self { RelayBehaviourEvent::Autonat(()) }
}
impl From<RequestResponseEvent<RelayAuthRequest, RelayAuthResponse>> for RelayBehaviourEvent {
    fn from(e: RequestResponseEvent<RelayAuthRequest, RelayAuthResponse>) -> Self {
        RelayBehaviourEvent::RelayAuth(e)
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "RelayBehaviourEvent")]
struct RelayBehaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    autonat: autonat::Behaviour,
    relay_auth: RequestResponse<RelayAuthCodec>,
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

// Start a lightweight HTTP server on a random available port
async fn start_http_server(app_handle: tauri::AppHandle) -> Result<u16, String> {
    // Define a small status router
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/metrics", get(|| async {
            serde_json::json!({
                "status": "running",
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            }).to_string()
        }));

    // Bind to port 0 for OS-assigned random port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind TCP listener: {}", e))?;

    // Retrieve assigned port
    let local_addr = listener.local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?;
    let port = local_addr.port();

    info!("🌐 HTTP server started on port {}", port);

    // Notify frontend or other services if needed
    let _ = app_handle.emit("http_server_port", port);

    // Spawn server task
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("HTTP server error: {}", e);
        }
    });

    Ok(port)
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

    // === TOKEN SETUP ===
    // Replace with your real tokens!
    let tokens: HashSet<Vec<u8>> = [
        b"mysecrettoken1".to_vec(),
        b"mysecrettoken2".to_vec(),
    ].iter().cloned().collect();
    // Track authenticated peers
    let authed_peers: Arc<Mutex<HashSet<PeerId>>> = Arc::new(Mutex::new(HashSet::new()));

    // Relay Auth protocol setup
    let relay_auth_protocols = std::iter::once((RelayAuthProtocol(), ProtocolSupport::Full));
    let relay_auth = RequestResponse::new(
        relay_auth_protocols,
        RequestResponseConfig::default(),
    );

    // === RELAY CONFIG ===
    // Note: In libp2p 0.54, reservation_handler is not supported
    // Authentication will be handled at the application level
    let mut relay_config = relay::Config::default();
    relay_config.max_reservations = args.max_reservations;
    relay_config.max_reservations_per_peer = args.max_reservations;
    relay_config.max_circuits = args.max_circuits;
    relay_config.max_circuits_per_peer = args.max_circuits;
    relay_config.max_circuit_duration = Duration::from_secs(3600); // 1 hour

    let authed_peers_for_limiter = authed_peers.clone();
    relay_config.reservation_rate_limiters.push(Box::new(
        move |peer_id: PeerId, _addr: &Multiaddr, _now: web_time::Instant| {
            match authed_peers_for_limiter.lock() {
                Ok(peers) => peers.contains(&peer_id),
                Err(_) => false,
            }
        },
    ));

    let behaviour = RelayBehaviour {
        relay: relay::Behaviour::new(local_peer_id, relay_config),
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/chiral/relay/1.0.0".to_string(),
            local_key.public(),
        )),
        autonat: autonat::Behaviour::new(local_peer_id, autonat::Config::default()),
        relay_auth,
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
    //placing for port randomization
    // Start HTTP server on a random port
    let http_port = start_http_server(app.handle().clone())
        .await
        .expect("Failed to start HTTP server");
    info!("✅ HTTP server listening on http://127.0.0.1:{}", http_port);
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
                                if authed_peers.lock().unwrap().contains(&src_peer_id) {
                                    reservation_count += 1;
                                    info!("✅ Reservation accepted for authenticated peer: {}", src_peer_id);
                                } else {
                                    warn!("⚠️  Reservation accepted for unauthenticated peer: {} (should not happen)", src_peer_id);
                                }
                            }
                            relay::Event::ReservationReqDenied { src_peer_id } => {
                                if authed_peers.lock().unwrap().contains(&src_peer_id) {
                                    warn!("⚠️  Reservation denied for authenticated peer: {}", src_peer_id);
                                } else {
                                    info!("🔒 Reservation denied for unauthenticated peer: {}", src_peer_id);
                                }
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
                    SwarmEvent::Behaviour(RelayBehaviourEvent::RelayAuth(event)) => {
                        match event {
                            RequestResponseEvent::Message { peer, message } => {
                                match message {
                                    RequestResponseMessage::Request { request, channel, .. } => {
                                        let accepted = tokens.contains(&request.0);
                                        if accepted {
                                            authed_peers.lock().unwrap().insert(peer);
                                            info!("✅ Authenticated peer for relay: {}", peer);
                                        } else {
                                            warn!("❌ Invalid relay token from peer: {}", peer);
                                        }
                                        swarm.behaviour_mut().relay_auth.send_response(channel, RelayAuthResponse(accepted)).unwrap();
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
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
                        let was_authed = authed_peers.lock().unwrap().remove(&peer_id);
                        if was_authed {
                            info!("🔒 Peer {} disconnected, removed from authenticated list", peer_id);
                        }
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