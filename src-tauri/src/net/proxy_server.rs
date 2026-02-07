use libp2p::futures::StreamExt;
use libp2p::ping::Behaviour as Ping;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::SwarmBuilder;
use libp2p::{identity, noise, tcp, yamux, Multiaddr, PeerId};
use std::error::Error;
use tracing::info;

pub async fn run_proxy_server(
    port: u16,
    _trusted_tokens: Vec<String>,
) -> Result<(), Box<dyn Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {:?}", local_peer_id);

    let mut swarm: Swarm<Ping> = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| Ping::default())?
        .build();

    let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
    swarm.listen_on(listen_addr)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
            }
            SwarmEvent::Behaviour(event) => {
                info!("Proxy server event: {:?}", event);
            }
            _ => {}
        }
    }
}
