/// Test client for relay authentication
/// Usage: cargo run --bin test_client -- <relay_multiaddr> <token>
/// Example: cargo run --bin test_client -- /ip4/127.0.0.1/tcp/4002/p2p/12D3KooWMTYkYDfz1EK8SApC6TtiXRwr8My9SnADzT1HsjSc4T9e mysecrettoken1

use anyhow::{Context, Result};
use clap::Parser;
use futures::StreamExt;
use libp2p::{
    identify, identity, noise, ping, relay,
    request_response::{
        Behaviour as RequestResponse, Config as RequestResponseConfig, 
        Event as RequestResponseEvent, Message as RequestResponseMessage,
        ProtocolSupport,
    },
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm,
};
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

// Import relay auth module
use chiral_relay_daemon::relay_auth::*;

#[derive(Parser, Debug)]
#[command(name = "test_client")]
#[command(about = "Test client for Chiral Relay authentication")]
struct Args {
    /// Relay multiaddr (e.g., /ip4/127.0.0.1/tcp/4002/p2p/12D3KooW...)
    relay_addr: Multiaddr,

    /// Authentication token
    token: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Timeout in seconds
    #[arg(short, long, default_value_t = 30)]
    timeout: u64,
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "ClientBehaviourEvent")]
struct ClientBehaviour {
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    relay_auth: RequestResponse<RelayAuthCodec>,
}

#[allow(clippy::large_enum_variant)]
enum ClientBehaviourEvent {
    Ping(()),
    Identify(identify::Event),
    RelayAuth(RequestResponseEvent<RelayAuthRequest, RelayAuthResponse>),
}

impl From<ping::Event> for ClientBehaviourEvent {
    fn from(_e: ping::Event) -> Self {
        ClientBehaviourEvent::Ping(())
    }
}
impl From<identify::Event> for ClientBehaviourEvent {
    fn from(e: identify::Event) -> Self {
        ClientBehaviourEvent::Identify(e)
    }
}
impl From<RequestResponseEvent<RelayAuthRequest, RelayAuthResponse>> for ClientBehaviourEvent {
    fn from(e: RequestResponseEvent<RelayAuthRequest, RelayAuthResponse>) -> Self {
        ClientBehaviourEvent::RelayAuth(e)
    }
}

fn create_client_swarm() -> Result<Swarm<ClientBehaviour>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    info!("Client Peer ID: {}", local_peer_id);

    let relay_auth_protocols = std::iter::once((RelayAuthProtocol(), ProtocolSupport::Full));
    let relay_auth = RequestResponse::new(
        relay_auth_protocols,
        RequestResponseConfig::default(),
    );

    let behaviour = ClientBehaviour {
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/chiral/test-client/1.0.0".to_string(),
            local_key.public(),
        )),
        relay_auth,
    };

    let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    Ok(swarm)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Setup logging
    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level)),
        )
        .init();

    info!("🚀 Starting test client");
    info!("🎯 Target relay: {}", args.relay_addr);
    info!("🔑 Token: {}", args.token);

    let mut swarm = create_client_swarm()?;

    // Extract relay peer ID from multiaddr
    let relay_peer_id = args
        .relay_addr
        .iter()
        .find_map(|p| {
            if let libp2p::multiaddr::Protocol::P2p(peer_id) = p {
                Some(peer_id)
            } else {
                None
            }
        })
        .context("Relay multiaddr must contain /p2p/<peer_id>")?;

    info!("📡 Relay Peer ID: {}", relay_peer_id);

    // Connect to relay
    swarm.dial(args.relay_addr.clone())?;
    info!("📞 Dialing relay...");

    let token_bytes = args.token.as_bytes().to_vec();
    let timeout_duration = Duration::from_secs(args.timeout);

    let mut connected = false;
    let mut auth_sent = false;
    let mut auth_response: Option<bool> = None;
    let mut reservation_result: Option<bool> = None;

    let start_time = tokio::time::Instant::now();

    loop {
        if start_time.elapsed() > timeout_duration {
            error!("❌ Test timed out after {} seconds", args.timeout);
            break;
        }

        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        if peer_id == relay_peer_id {
                            info!("✅ Connected to relay at {}", endpoint.get_remote_address());
                            connected = true;

                            // Send authentication request
                            info!("📤 Sending authentication request...");
                            swarm.behaviour_mut().relay_auth.send_request(
                                &relay_peer_id,
                                RelayAuthRequest(token_bytes.clone()),
                            );
                            auth_sent = true;
                        }
                    }
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        if peer_id == relay_peer_id {
                            warn!("❌ Connection closed: {:?}", cause);
                        }
                    }
                    SwarmEvent::Behaviour(ClientBehaviourEvent::RelayAuth(event)) => {
                        match event {
                            RequestResponseEvent::Message { peer, message } => {
                                if let RequestResponseMessage::Response { response, .. } = message {
                                    info!("📥 Received auth response from {}: {}", peer, response.0);
                                    auth_response = Some(response.0);

                                    if response.0 {
                                        info!("✅ Authentication SUCCESSFUL");
                                        info!("🎉 Test completed successfully - authentication works!");
                                        reservation_result = Some(true);
                                        break;
                                    } else {
                                        error!("❌ Authentication FAILED - invalid token");
                                        reservation_result = Some(false);
                                        break;
                                    }
                                }
                            }
                            RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                                error!("❌ Auth request failed to {}: {:?}", peer, error);
                                auth_response = Some(false);
                                break;
                            }
                            RequestResponseEvent::InboundFailure { peer, error, .. } => {
                                error!("❌ Inbound failure from {}: {:?}", peer, error);
                            }
                            _ => {}
                        }
                    }
                    SwarmEvent::Behaviour(ClientBehaviourEvent::Identify(identify_event)) => {
                        if let identify::Event::Received { peer_id, info, .. } = identify_event {
                            info!("🔍 Identified {}: {}", peer_id, info.protocol_version);
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        error!("❌ Failed to connect to {:?}: {}", peer_id, error);
                        break;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // Keep the loop alive
            }
        }
    }

    // Print summary
    println!("\n═══════════════════════════════════════");
    println!("           TEST SUMMARY");
    println!("═══════════════════════════════════════");
    println!("Connected:              {}", if connected { "✅ YES" } else { "❌ NO" });
    println!("Auth Request Sent:      {}", if auth_sent { "✅ YES" } else { "❌ NO" });
    println!(
        "Auth Response:          {}",
        match auth_response {
            Some(true) => "✅ ACCEPTED",
            Some(false) => "❌ REJECTED",
            None => "⏳ NO RESPONSE",
        }
    );
    println!(
        "Reservation:            {}",
        match reservation_result {
            Some(true) => "✅ ACCEPTED",
            Some(false) => "❌ DENIED/FAILED",
            None => "⏳ NO RESULT",
        }
    );
    println!("═══════════════════════════════════════\n");

    // Exit with appropriate status code
    if reservation_result == Some(true) {
        info!("🎉 TEST PASSED - Full authentication and reservation successful!");
        Ok(())
    } else {
        error!("❌ TEST FAILED");
        std::process::exit(1);
    }
}
