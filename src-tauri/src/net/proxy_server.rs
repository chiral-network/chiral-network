use libp2p::futures::StreamExt;
use libp2p::relay::client::Behaviour as RelayClientBehaviour;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::SwarmBuilder;
use libp2p::{identity, noise, tcp, yamux, Multiaddr, PeerId};
use std::error::Error;
use tracing::info;

pub async fn run_proxy_server(
    port: Option<u16>, // Make port optional
    _trusted_tokens: Vec<String>,
) -> Result<u16, Box<dyn Error>> { // Return the actual port used
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {:?}", local_peer_id);

    let mut swarm: Swarm<RelayClientBehaviour> = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_relay_client(noise::Config::new, yamux::Config::default)?
        .with_behaviour(|_keypair, relay_client| Ok(relay_client))?
        .build();

    // Use provided port or 0 for random
    let port_to_use = port.unwrap_or(0);
    let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port_to_use).parse()?;
    swarm.listen_on(listen_addr)?;
    
    let mut actual_port = port_to_use;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
                
                // Extract actual port from multiaddr
                if let Some(protocol) = address.iter().find_map(|p| {
                    if let libp2p::multiaddr::Protocol::Tcp(port) = p {
                        Some(port)
                    } else {
                        None
                    }
                }) {
                    actual_port = protocol;
                    info!("✅ Proxy server bound to port {}", actual_port);
                }
            }
            SwarmEvent::Behaviour(event) => {
                info!("Relay client event: {:?}", event);
            }
            _ => {}
        }
        
        // Break after first listen address to return the port
        if actual_port != port_to_use {
            break;
        }
    }
    
    Ok(actual_port)
}