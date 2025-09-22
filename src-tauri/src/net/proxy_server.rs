
use libp2p::futures::StreamExt;
use libp2p::relay::Behaviour as Relay;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::SwarmBuilder;
use libp2p::{identity, Multiaddr, PeerId, tcp, noise, yamux};
use libp2p::relay::client::Behaviour as RelayClientBehaviour;
use std::error::Error;
use tracing::info;

pub async fn run_proxy_server(port: u16, _trusted_tokens: Vec<String>) -> Result<(), Box<dyn Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {:?}", local_peer_id);

    // SwarmBuilder: TCP + RelayClient + with_behaviour(두 인자) + build()
    let mut swarm: Swarm<RelayClientBehaviour> =
        SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?  // 함수 포인터 서명에 맞게 ok
            .with_relay_client(noise::Config::new, yamux::Config::default)?                 // 마찬가지
            .with_behaviour(|_keypair, relay_client| {
                // 별도 Config 없이, 만들어진 relay_client 그대로 사용
                Ok(relay_client)
            })?
            .build(); // 여기서 바로 Swarm 완성 (Swarm::new + Default::default() 불필요)

    let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
    swarm.listen_on(listen_addr)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
            }
            SwarmEvent::Behaviour(event) => {
                info!("Relay client event: {:?}", event);
            }
            _ => {}
        }
    }
}