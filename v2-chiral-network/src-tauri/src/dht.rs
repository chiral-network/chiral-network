use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use libp2p::{
    kad, mdns, noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Swarm, PeerId,
    identify,
};
use futures::StreamExt;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use std::str::FromStr;

enum SwarmCommand {
    Ping(PeerId),
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub id: String,
    pub address: String,
    pub multiaddrs: Vec<String>,
    pub last_seen: i64,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStats {
    pub connected_peers: usize,
    pub total_peers: usize,
}

#[derive(NetworkBehaviour)]
struct DhtBehaviour {
    kad: kad::Behaviour<kad::store::MemoryStore>,
    mdns: mdns::tokio::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

pub struct DhtService {
    peers: Arc<Mutex<Vec<PeerInfo>>>,
    is_running: Arc<Mutex<bool>>,
    local_peer_id: Arc<Mutex<Option<String>>>,
    command_sender: Arc<Mutex<Option<mpsc::UnboundedSender<SwarmCommand>>>>,
}

impl DhtService {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
            local_peer_id: Arc::new(Mutex::new(None)),
            command_sender: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start(&self, app: tauri::AppHandle) -> Result<String, String> {
        let mut running = self.is_running.lock().await;
        if *running {
            return Err("DHT already running".to_string());
        }
        
        // Create libp2p swarm
        let (swarm, peer_id) = create_swarm().await.map_err(|e| e.to_string())?;
        
        // Store peer ID
        let mut local_id = self.local_peer_id.lock().await;
        *local_id = Some(peer_id.clone());
        drop(local_id);
        
        *running = true;
        
        // Create command channel
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let mut cmd_sender = self.command_sender.lock().await;
        *cmd_sender = Some(cmd_tx);
        drop(cmd_sender);
        
        // Spawn event loop
        let peers_clone = self.peers.clone();
        let is_running_clone = self.is_running.clone();
        
        tokio::spawn(async move {
            event_loop(swarm, peers_clone, is_running_clone, app, cmd_rx).await;
        });
        
        Ok(format!("DHT started with peer ID: {}", peer_id))
    }

    pub async fn stop(&self) -> Result<(), String> {
        let mut running = self.is_running.lock().await;
        *running = false;
        
        let mut peers = self.peers.lock().await;
        peers.clear();
        
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        *self.is_running.lock().await
    }

    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        self.peers.lock().await.clone()
    }

    pub async fn get_stats(&self) -> NetworkStats {
        let peers = self.peers.lock().await;
        NetworkStats {
            connected_peers: peers.len(),
            total_peers: peers.len(),
        }
    }
    
    pub async fn get_peer_id(&self) -> Option<String> {
        self.local_peer_id.lock().await.clone()
    }
    
    pub async fn ping_peer(&self, peer_id: String, app: tauri::AppHandle) -> Result<String, String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let peer_id_parsed = PeerId::from_str(&peer_id).map_err(|e| e.to_string())?;
            tx.send(SwarmCommand::Ping(peer_id_parsed)).map_err(|e| e.to_string())?;
            let _ = app.emit("ping-sent", peer_id.clone());
            Ok(format!("Ping sent to {}", peer_id))
        } else {
            Err("DHT not running".to_string())
        }
    }
}

async fn create_swarm() -> Result<(Swarm<DhtBehaviour>, String), Box<dyn Error>> {
    let local_key = libp2p::identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    
    println!("Local peer ID: {}", local_peer_id);
    
    let kad_store = kad::store::MemoryStore::new(local_peer_id);
    let kad = kad::Behaviour::new(local_peer_id, kad_store);
    
    let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;
    let ping = ping::Behaviour::new(ping::Config::new());
    
    let identify_config = identify::Config::new(
        "/chiral/id/1.0.0".to_string(),
        local_key.public(),
    );
    let identify = identify::Behaviour::new(identify_config);
    
    let behaviour = DhtBehaviour {
        kad,
        mdns,
        ping,
        identify,
    };
    
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(60)))
        .build();
    
    // Listen on all interfaces
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    
    Ok((swarm, local_peer_id.to_string()))
}

async fn event_loop(
    mut swarm: Swarm<DhtBehaviour>,
    peers: Arc<Mutex<Vec<PeerInfo>>>,
    is_running: Arc<Mutex<bool>>,
    app: tauri::AppHandle,
    mut cmd_rx: mpsc::UnboundedReceiver<SwarmCommand>,
) {
    loop {
        let running = *is_running.lock().await;
        if !running {
            break;
        }
        
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(event) => {
                        handle_behaviour_event(event, &peers, &app).await;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Listening on {:?}", address);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        println!("Connection established with {:?}", peer_id);
                        let _ = app.emit("connection-established", peer_id.to_string());
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        if let Some(peer) = peer_id {
                            println!("Failed to connect to {:?}: {:?}", peer, error);
                        }
                    }
                    _ => {}
                }
            }
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    SwarmCommand::Ping(peer_id) => {
                        println!("Attempting to dial and ping peer: {}", peer_id);
                        
                        // Get the peer's multiaddrs from our peer list
                        let peers_guard = peers.lock().await;
                        let peer_id_str = peer_id.to_string();
                        
                        if let Some(peer_info) = peers_guard.iter().find(|p| p.id == peer_id_str) {
                            // Try to dial each multiaddr
                            for addr_str in &peer_info.multiaddrs {
                                if let Ok(addr) = addr_str.parse::<libp2p::Multiaddr>() {
                                    println!("Dialing peer {} at {}", peer_id, addr);
                                    match swarm.dial(addr) {
                                        Ok(_) => {
                                            println!("Dial initiated to {}", peer_id);
                                            break;
                                        }
                                        Err(e) => {
                                            println!("Failed to dial {}: {:?}", peer_id, e);
                                        }
                                    }
                                }
                            }
                        } else {
                            println!("Peer {} not found in peer list", peer_id);
                        }
                    }
                }
            }
        }
        
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
}

async fn handle_behaviour_event(
    event: DhtBehaviourEvent,
    peers: &Arc<Mutex<Vec<PeerInfo>>>,
    app: &tauri::AppHandle,
) {
    match event {
        DhtBehaviourEvent::Mdns(mdns::Event::Discovered(list)) => {
            let mut peers_guard = peers.lock().await;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            
            for (peer_id, multiaddrs) in list {
                let peer_id_str = peer_id.to_string();
                println!("Discovered peer: {}", peer_id_str);
                
                // Check if peer already exists
                if let Some(existing) = peers_guard.iter_mut().find(|p| p.id == peer_id_str) {
                    existing.last_seen = now;
                    existing.multiaddrs = multiaddrs.iter().map(|m| m.to_string()).collect();
                } else {
                    let peer_info = PeerInfo {
                        id: peer_id_str.clone(),
                        address: peer_id_str.clone(),
                        multiaddrs: multiaddrs.iter().map(|m| m.to_string()).collect(),
                        last_seen: now,
                    };
                    peers_guard.push(peer_info);
                }
            }
            
            // Emit event to frontend
            let _ = app.emit("peer-discovered", peers_guard.clone());
        }
        DhtBehaviourEvent::Mdns(mdns::Event::Expired(list)) => {
            let mut peers_guard = peers.lock().await;
            for (peer_id, _) in list {
                let peer_id_str = peer_id.to_string();
                peers_guard.retain(|p| p.id != peer_id_str);
                println!("Peer expired: {}", peer_id_str);
            }
        }
        DhtBehaviourEvent::Ping(event) => {
            match event.result {
                Ok(rtt) => {
                    println!("Ping to {} succeeded: {:?}", event.peer, rtt);
                    let _ = app.emit("pong-received", event.peer.to_string());
                }
                Err(e) => {
                    println!("Ping to {} failed: {:?}", event.peer, e);
                }
            }
        }
        DhtBehaviourEvent::Identify(event) => {
            println!("Identify event: {:?}", event);
        }
        _ => {}
    }
}
