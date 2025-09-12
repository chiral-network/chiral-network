use futures_util::StreamExt;
use libp2p::{
    kad::{self, store::MemoryStore, QueryResult, Record},
    mdns::{self},
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
    identity::{self},
    gossipsub::{self, MessageAuthenticity, ValidationMode},
    PeerId, Swarm,
};
use libp2p::kad::Behaviour as Kademlia;
use libp2p::kad::Config as KademliaConfig;
use libp2p::kad::Event as KademliaEvent;
use libp2p::mdns::tokio::Behaviour as Mdns;
use libp2p::gossipsub::Behaviour as Gossipsub;
use libp2p::gossipsub::Event as GossipsubEvent;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn, error};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub seeders: Vec<String>,
    pub created_at: u64,
    pub mime_type: Option<String>,
}

#[derive(NetworkBehaviour)]
pub struct ChiralBehaviour {
    pub kademlia: Kademlia<MemoryStore>,
    pub mdns: Mdns,
    pub gossipsub: Gossipsub,
}

pub struct DhtNode {
    swarm: Swarm<ChiralBehaviour>,
    peer_id: PeerId,
    file_metadata: Arc<Mutex<HashMap<String, FileMetadata>>>,
    command_sender: mpsc::Sender<DhtCommand>,
    event_receiver: mpsc::Receiver<DhtEvent>,
}

#[derive(Debug)]
pub enum DhtCommand {
    PublishFile(FileMetadata),
    GetFile(String),
    RemoveFile(String),
    GetPeers,
    ConnectPeer(String),
}

#[derive(Debug, Clone, Serialize)]
pub enum DhtEvent {
    PeerDiscovered(String),
    PeerConnected(String),
    PeerDisconnected(String),
    FileDiscovered(FileMetadata),
    FileNotFound(String),
    Error(String),
}

impl DhtNode {
    pub async fn new(listen_port: u16, bootstrap_nodes: Vec<String>) -> Result<Self, Box<dyn std::error::Error>> {
        // Generate a new keypair for this node
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        
        info!("Local peer id: {}", local_peer_id);

        // Create a Kademlia behaviour with custom configuration
        let mut kad_config = KademliaConfig::default();
        kad_config.set_query_timeout(Duration::from_secs(60));
        
        let store = MemoryStore::new(local_peer_id);
        let mut kademlia = Kademlia::with_config(local_peer_id, store, kad_config);
        
        // Set the Kademlia protocol for private network
        kademlia.set_mode(Some(kad::Mode::Server));

        // Create mDNS for local peer discovery
        let mdns = Mdns::new(mdns::Config::default(), local_peer_id)?;

        // Create Gossipsub for pub/sub messaging
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(ValidationMode::Strict)
            .build()
            .expect("Valid config");

        let mut gossipsub = Gossipsub::new(
            MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        // Subscribe to file metadata topic
        let topic = gossipsub::IdentTopic::new("chiral-file-metadata");
        gossipsub.subscribe(&topic)?;

        let behaviour = ChiralBehaviour {
            kademlia,
            mdns,
            gossipsub,
        };

        // Build the swarm using the current libp2p API
        let transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .boxed();
            
        let mut swarm = Swarm::new(
            transport,
            behaviour,
            local_peer_id,
            libp2p::swarm::Config::with_executor(tokio::runtime::Handle::current())
                .with_idle_connection_timeout(Duration::from_secs(60)),
        );

        // Listen on the specified port
        let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", listen_port);
        swarm.listen_on(listen_addr.parse()?)?;

        // Add bootstrap nodes
        for addr in bootstrap_nodes {
            if let Ok(multiaddr) = addr.parse() {
                if let Some(peer_id) = extract_peer_id(&addr) {
                    swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr);
                    info!("Added bootstrap node: {}", addr);
                }
            }
        }

        // Bootstrap the Kademlia DHT
        swarm.behaviour_mut().kademlia.bootstrap()?;

        let (cmd_tx, mut cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);

        Ok(DhtNode {
            swarm,
            peer_id: local_peer_id,
            file_metadata: Arc::new(Mutex::new(HashMap::new())),
            command_sender: cmd_tx,
            event_receiver: event_rx,
        })
    }

    pub async fn start(mut self) -> (mpsc::Sender<DhtCommand>, mpsc::Receiver<DhtEvent>) {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        
        let event_sender = event_tx.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Handle incoming commands
                    Some(command) = cmd_rx.recv() => {
                        match command {
                            DhtCommand::PublishFile(metadata) => {
                                self.publish_file(metadata).await;
                            }
                            DhtCommand::GetFile(file_hash) => {
                                self.get_file(&file_hash).await;
                            }
                            DhtCommand::RemoveFile(file_hash) => {
                                self.remove_file(&file_hash).await;
                            }
                            DhtCommand::GetPeers => {
                                let peers = self.get_connected_peers();
                                info!("Connected peers: {:?}", peers);
                            }
                            DhtCommand::ConnectPeer(addr) => {
                                self.connect_to_peer(&addr).await;
                            }
                        }
                    }
                    
                    // Handle swarm events
                    event = self.swarm.next() => if let Some(event) = event {
                        match event {
                            SwarmEvent::Behaviour(ChiralBehaviourEvent::Kademlia(kad_event)) => {
                                self.handle_kademlia_event(kad_event, &event_sender).await;
                            }
                            SwarmEvent::Behaviour(ChiralBehaviourEvent::Mdns(mdns_event)) => {
                                self.handle_mdns_event(mdns_event, &event_sender).await;
                            }
                            SwarmEvent::Behaviour(ChiralBehaviourEvent::Gossipsub(gossip_event)) => {
                                self.handle_gossipsub_event(gossip_event, &event_sender).await;
                            }
                            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                                info!("Connection established with: {}", peer_id);
                                let _ = event_sender.send(DhtEvent::PeerConnected(peer_id.to_string())).await;
                            }
                            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                                info!("Connection closed with: {}", peer_id);
                                let _ = event_sender.send(DhtEvent::PeerDisconnected(peer_id.to_string())).await;
                            }
                            SwarmEvent::NewListenAddr { address, .. } => {
                                info!("Listening on: {}", address);
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        (cmd_tx, event_rx)
    }

    async fn publish_file(&mut self, metadata: FileMetadata) {
        let key = format!("file:{}", metadata.file_hash);
        let value = serde_json::to_vec(&metadata).unwrap();
        
        let record = Record {
            key: key.as_bytes().to_vec().into(),
            value,
            publisher: Some(self.peer_id),
            expires: None,
        };

        match self.swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One) {
            Ok(_) => {
                info!("Published file metadata: {}", metadata.file_hash);
                
                // Also publish via gossipsub
                let topic = gossipsub::IdentTopic::new("chiral-file-metadata");
                if let Ok(json) = serde_json::to_string(&metadata) {
                    let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, json.as_bytes());
                }
            }
            Err(e) => {
                error!("Failed to publish file metadata: {}", e);
            }
        }
    }

    async fn get_file(&mut self, file_hash: &str) {
        let key = format!("file:{}", file_hash);
        self.swarm.behaviour_mut().kademlia.get_record(key.as_bytes().to_vec().into());
        info!("Searching for file: {}", file_hash);
    }

    async fn remove_file(&mut self, file_hash: &str) {
        let key = format!("file:{}", file_hash);
        self.swarm.behaviour_mut().kademlia.remove_record(&key.as_bytes().to_vec().into());
        info!("Removed file from DHT: {}", file_hash);
    }

    fn get_connected_peers(&self) -> Vec<String> {
        self.swarm
            .connected_peers()
            .map(|p| p.to_string())
            .collect()
    }

    async fn connect_to_peer(&mut self, addr: &str) {
        match addr.parse() {
            Ok(multiaddr) => {
                match self.swarm.dial(multiaddr) {
                    Ok(_) => info!("Dialing peer: {}", addr),
                    Err(e) => error!("Failed to dial peer: {}", e),
                }
            }
            Err(e) => error!("Invalid multiaddr: {}", e),
        }
    }

    async fn handle_kademlia_event(&mut self, event: KademliaEvent, event_sender: &mpsc::Sender<DhtEvent>) {
        match event {
            KademliaEvent::OutboundQueryProgressed { result, .. } => {
                match result {
                    QueryResult::GetRecord(Ok(result)) => if let Some(kad::PeerRecord { record, .. }) = result.records.first() {
                        if let Ok(metadata) = serde_json::from_slice::<FileMetadata>(&record.value) {
                            info!("Found file metadata: {}", metadata.file_hash);
                            let _ = event_sender.send(DhtEvent::FileDiscovered(metadata)).await;
                        }
                    }
                    QueryResult::GetRecord(Err(e)) => {
                        warn!("Failed to get record: {:?}", e);
                        let _ = event_sender.send(DhtEvent::FileNotFound("File not found".to_string())).await;
                    }
                    QueryResult::PutRecord(Ok(_)) => {
                        info!("Successfully stored record in DHT");
                    }
                    QueryResult::PutRecord(Err(e)) => {
                        error!("Failed to store record: {:?}", e);
                    }
                    _ => {}
                }
            }
            KademliaEvent::RoutingUpdated { peer, .. } => {
                debug!("Routing table updated with peer: {}", peer);
            }
            _ => {}
        }
    }

    async fn handle_mdns_event(&mut self, event: mdns::Event, event_sender: &mpsc::Sender<DhtEvent>) {
        match event {
            mdns::Event::Discovered(peers) => {
                for (peer_id, addr) in peers {
                    info!("Discovered peer via mDNS: {} at {}", peer_id, addr);
                    self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                    let _ = self.swarm.dial(addr);
                    let _ = event_sender.send(DhtEvent::PeerDiscovered(peer_id.to_string())).await;
                }
            }
            mdns::Event::Expired(peers) => {
                for (peer_id, _) in peers {
                    info!("mDNS peer expired: {}", peer_id);
                }
            }
        }
    }

    async fn handle_gossipsub_event(&mut self, event: GossipsubEvent, event_sender: &mpsc::Sender<DhtEvent>) {
        match event {
            GossipsubEvent::Message { message, .. } => {
                if let Ok(metadata) = serde_json::from_slice::<FileMetadata>(&message.data) {
                    info!("Received file metadata via gossipsub: {}", metadata.file_hash);
                    let _ = event_sender.send(DhtEvent::FileDiscovered(metadata)).await;
                }
            }
            GossipsubEvent::Subscribed { peer_id, topic } => {
                debug!("Peer {} subscribed to topic {}", peer_id, topic);
            }
            _ => {}
        }
    }
}

fn extract_peer_id(addr: &str) -> Option<PeerId> {
    // Extract peer ID from multiaddr like /ip4/1.2.3.4/tcp/4001/p2p/QmPeerId
    if let Some(p2p_part) = addr.split("/p2p/").nth(1) {
        p2p_part.parse().ok()
    } else {
        None
    }
}

// Public API for the DHT
pub struct DhtService {
    command_sender: mpsc::Sender<DhtCommand>,
    event_receiver: Arc<Mutex<mpsc::Receiver<DhtEvent>>>,
}

impl DhtService {
    pub async fn new(port: u16, bootstrap_nodes: Vec<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let node = DhtNode::new(port, bootstrap_nodes).await?;
        let (cmd_sender, event_receiver) = node.start().await;
        
        Ok(DhtService {
            command_sender: cmd_sender,
            event_receiver: Arc::new(Mutex::new(event_receiver)),
        })
    }

    pub async fn publish_file(&self, metadata: FileMetadata) -> Result<(), String> {
        self.command_sender
            .send(DhtCommand::PublishFile(metadata))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_file(&self, file_hash: String) -> Result<(), String> {
        self.command_sender
            .send(DhtCommand::GetFile(file_hash))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn remove_file(&self, file_hash: String) -> Result<(), String> {
        self.command_sender
            .send(DhtCommand::RemoveFile(file_hash))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn connect_peer(&self, addr: String) -> Result<(), String> {
        self.command_sender
            .send(DhtCommand::ConnectPeer(addr))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_next_event(&self) -> Option<DhtEvent> {
        let mut receiver = self.event_receiver.lock().await;
        receiver.recv().await
    }
}