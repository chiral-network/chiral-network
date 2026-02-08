use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use crate::speed_tiers::SpeedTier;
use libp2p::{
    kad, mdns, noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Swarm, PeerId, StreamProtocol, Multiaddr,
    identify, request_response,
};
use futures::StreamExt;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use std::str::FromStr;
use std::collections::HashMap;

/// Get bootstrap nodes for the Chiral Network DHT
/// These are the same bootstrap nodes used in v1
pub fn get_bootstrap_nodes() -> Vec<String> {
    vec![
        "/ip4/134.199.240.145/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE".to_string(),
    ]
}

/// Extract peer ID from a multiaddr like /ip4/.../tcp/.../p2p/<peer_id>
fn extract_peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
    use libp2p::multiaddr::Protocol;
    for proto in addr.iter() {
        if let Protocol::P2p(peer_id) = proto {
            return Some(peer_id);
        }
    }
    None
}

/// Remove the /p2p/<peer_id> component from a multiaddr
fn remove_peer_id_from_multiaddr(addr: &Multiaddr) -> Multiaddr {
    use libp2p::multiaddr::Protocol;
    addr.iter()
        .filter(|p| !matches!(p, Protocol::P2p(_)))
        .collect()
}

// Ping protocol messages
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingRequest(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResponse(pub String);

// File transfer protocol messages (for direct file push)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferRequest {
    pub transfer_id: String,
    pub file_name: String,
    pub file_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferResponse {
    pub transfer_id: String,
    pub accepted: bool,
    pub error: Option<String>,
}

// File request protocol messages (for requesting files by hash)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRequestMessage {
    pub request_id: String,
    pub file_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRequestResponse {
    pub request_id: String,
    pub file_hash: String,
    pub file_name: String,
    pub file_data: Option<Vec<u8>>,
    pub error: Option<String>,
}

enum SwarmCommand {
    SendPing(PeerId),
    SendFile {
        peer_id: PeerId,
        transfer_id: String,
        file_name: String,
        file_data: Vec<u8>,
    },
    RequestFile {
        peer_id: PeerId,
        request_id: String,
        file_hash: String,
    },
    PutDhtValue {
        key: String,
        value: String,
        response_tx: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    GetDhtValue {
        key: String,
        response_tx: tokio::sync::oneshot::Sender<Result<Option<String>, String>>,
    },
    HealthCheck {
        response_tx: tokio::sync::oneshot::Sender<DhtHealthInfo>,
    },
    CheckPeerConnected {
        peer_id: PeerId,
        response_tx: tokio::sync::oneshot::Sender<bool>,
    },
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DhtHealthInfo {
    pub running: bool,
    pub peer_id: Option<String>,
    pub listening_addresses: Vec<String>,
    pub connected_peer_count: usize,
    pub kademlia_peers: usize,
    pub bootstrap_nodes: Vec<BootstrapNodeStatus>,
    pub shared_files: usize,
    pub protocols: Vec<String>,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapNodeStatus {
    pub address: String,
    pub reachable: bool,
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
    ping_protocol: request_response::cbor::Behaviour<PingRequest, PingResponse>,
    file_transfer: request_response::cbor::Behaviour<FileTransferRequest, FileTransferResponse>,
    file_request: request_response::cbor::Behaviour<FileRequestMessage, FileRequestResponse>,
}

/// Map of file hash -> file path for files we're seeding
pub type SharedFilesMap = Arc<Mutex<std::collections::HashMap<String, SharedFileInfo>>>;

#[derive(Clone, Debug)]
pub struct SharedFileInfo {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
}

/// Map of request_id -> SpeedTier for rate-limited downloads
pub type DownloadTiersMap = Arc<Mutex<HashMap<String, SpeedTier>>>;

pub struct DhtService {
    peers: Arc<Mutex<Vec<PeerInfo>>>,
    is_running: Arc<Mutex<bool>>,
    local_peer_id: Arc<Mutex<Option<String>>>,
    command_sender: Arc<Mutex<Option<mpsc::UnboundedSender<SwarmCommand>>>>,
    file_transfer_service: Option<Arc<Mutex<crate::file_transfer::FileTransferService>>>,
    shared_files: SharedFilesMap,
    download_tiers: DownloadTiersMap,
}

impl DhtService {
    pub fn new(
        file_transfer_service: Arc<Mutex<crate::file_transfer::FileTransferService>>,
        download_tiers: DownloadTiersMap,
    ) -> Self {
        Self {
            peers: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
            local_peer_id: Arc::new(Mutex::new(None)),
            command_sender: Arc::new(Mutex::new(None)),
            file_transfer_service: Some(file_transfer_service),
            shared_files: Arc::new(Mutex::new(std::collections::HashMap::new())),
            download_tiers,
        }
    }

    /// Register a file for sharing (seeding)
    pub async fn register_shared_file(&self, file_hash: String, file_path: String, file_name: String, file_size: u64) {
        let mut shared = self.shared_files.lock().await;
        println!("=== REGISTERING SHARED FILE ===");
        println!("  Name: {}", file_name);
        println!("  Hash: {}", file_hash);
        println!("  Path: {}", file_path);
        println!("  Size: {} bytes", file_size);
        shared.insert(file_hash.clone(), SharedFileInfo {
            file_path,
            file_name,
            file_size,
        });
        println!("  Total shared files now: {}", shared.len());
        println!("================================");
    }

    /// Unregister a shared file
    pub async fn unregister_shared_file(&self, file_hash: &str) {
        let mut shared = self.shared_files.lock().await;
        if let Some(info) = shared.remove(file_hash) {
            println!("Unregistered shared file: {} (hash: {})", info.file_name, file_hash);
        }
    }

    /// Get shared files map for the event loop
    pub fn get_shared_files(&self) -> SharedFilesMap {
        self.shared_files.clone()
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
        let file_transfer_clone = self.file_transfer_service.clone();
        let shared_files_clone = self.shared_files.clone();
        let download_tiers_clone = self.download_tiers.clone();

        tokio::spawn(async move {
            event_loop(swarm, peers_clone, is_running_clone, app, cmd_rx, file_transfer_clone, shared_files_clone, download_tiers_clone).await;
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
    
    pub async fn get_health(&self) -> DhtHealthInfo {
        let running = *self.is_running.lock().await;
        if !running {
            return DhtHealthInfo {
                running: false,
                peer_id: None,
                listening_addresses: vec![],
                connected_peer_count: 0,
                kademlia_peers: 0,
                bootstrap_nodes: get_bootstrap_nodes().iter().map(|addr| BootstrapNodeStatus {
                    address: addr.clone(),
                    reachable: false,
                }).collect(),
                shared_files: 0,
                protocols: vec![],
            };
        }

        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            if tx.send(SwarmCommand::HealthCheck { response_tx: resp_tx }).is_ok() {
                if let Ok(mut info) = resp_rx.await {
                    info.peer_id = self.local_peer_id.lock().await.clone();
                    info.shared_files = self.shared_files.lock().await.len();
                    return info;
                }
            }
        }

        // Fallback if command channel fails
        let peers = self.peers.lock().await;
        let shared = self.shared_files.lock().await;
        DhtHealthInfo {
            running: true,
            peer_id: self.local_peer_id.lock().await.clone(),
            listening_addresses: vec![],
            connected_peer_count: peers.len(),
            kademlia_peers: 0,
            bootstrap_nodes: vec![],
            shared_files: shared.len(),
            protocols: vec![],
        }
    }

    pub async fn ping_peer(&self, peer_id: String, app: tauri::AppHandle) -> Result<String, String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let peer_id_parsed = PeerId::from_str(&peer_id).map_err(|e| e.to_string())?;
            tx.send(SwarmCommand::SendPing(peer_id_parsed)).map_err(|e| e.to_string())?;
            let _ = app.emit("ping-sent", peer_id.clone());
            Ok(format!("Ping sent to {}", peer_id))
        } else {
            Err("DHT not running".to_string())
        }
    }

    pub async fn send_file(
        &self,
        peer_id: String,
        transfer_id: String,
        file_name: String,
        file_data: Vec<u8>,
    ) -> Result<(), String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let peer_id_parsed = PeerId::from_str(&peer_id).map_err(|e| e.to_string())?;
            tx.send(SwarmCommand::SendFile {
                peer_id: peer_id_parsed,
                transfer_id,
                file_name,
                file_data,
            }).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("DHT not running".to_string())
        }
    }

    /// Store a value in the DHT
    pub async fn put_dht_value(&self, key: String, value: String) -> Result<(), String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            tx.send(SwarmCommand::PutDhtValue {
                key,
                value,
                response_tx,
            }).map_err(|e| e.to_string())?;
            response_rx.await.map_err(|e| e.to_string())?
        } else {
            Err("DHT not running".to_string())
        }
    }

    /// Get a value from the DHT
    pub async fn get_dht_value(&self, key: String) -> Result<Option<String>, String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            tx.send(SwarmCommand::GetDhtValue {
                key,
                response_tx,
            }).map_err(|e| e.to_string())?;
            response_rx.await.map_err(|e| e.to_string())?
        } else {
            Err("DHT not running".to_string())
        }
    }

    /// Check if a specific peer is currently connected to the swarm
    pub async fn is_peer_connected(&self, peer_id: &str) -> Result<bool, String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let peer_id_parsed = PeerId::from_str(peer_id).map_err(|e| e.to_string())?;
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            tx.send(SwarmCommand::CheckPeerConnected {
                peer_id: peer_id_parsed,
                response_tx,
            }).map_err(|e| e.to_string())?;
            response_rx.await.map_err(|e| e.to_string())
        } else {
            Err("DHT not running".to_string())
        }
    }

    /// Request a file from a remote peer by hash
    pub async fn request_file(&self, peer_id: String, file_hash: String, request_id: String) -> Result<(), String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let peer_id_parsed = PeerId::from_str(&peer_id).map_err(|e| e.to_string())?;
            tx.send(SwarmCommand::RequestFile {
                peer_id: peer_id_parsed,
                request_id,
                file_hash,
            }).map_err(|e| e.to_string())?;
            Ok(())
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
    let mut kad = kad::Behaviour::new(local_peer_id, kad_store);
    // Set to server mode to help propagate records
    kad.set_mode(Some(kad::Mode::Server));

    // Add bootstrap nodes to Kademlia routing table
    for addr_str in get_bootstrap_nodes() {
        if let Ok(addr) = addr_str.parse::<Multiaddr>() {
            // Extract peer ID from the multiaddr (last component is /p2p/<peer_id>)
            if let Some(peer_id) = extract_peer_id_from_multiaddr(&addr) {
                // Remove the /p2p/<peer_id> suffix to get the transport address
                let transport_addr = remove_peer_id_from_multiaddr(&addr);
                kad.add_address(&peer_id, transport_addr);
                println!("Added bootstrap node to Kademlia: {}", peer_id);
            }
        }
    }

    let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;
    let ping = ping::Behaviour::new(ping::Config::new());

    let identify_config = identify::Config::new(
        "/chiral/id/1.0.0".to_string(),
        local_key.public(),
    );
    let identify = identify::Behaviour::new(identify_config);

    let ping_protocol = request_response::cbor::Behaviour::new(
        [(StreamProtocol::new("/chiral/ping/1.0.0"), request_response::ProtocolSupport::Full)],
        request_response::Config::default(),
    );

    let file_transfer = request_response::cbor::Behaviour::new(
        [(StreamProtocol::new("/chiral/file-transfer/1.0.0"), request_response::ProtocolSupport::Full)],
        request_response::Config::default(),
    );

    let file_request = request_response::cbor::Behaviour::new(
        [(StreamProtocol::new("/chiral/file-request/1.0.0"), request_response::ProtocolSupport::Full)],
        request_response::Config::default(),
    );

    let behaviour = DhtBehaviour {
        kad,
        mdns,
        ping,
        identify,
        ping_protocol,
        file_transfer,
        file_request,
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

    // Dial bootstrap nodes to establish connections
    for addr_str in get_bootstrap_nodes() {
        if let Ok(addr) = addr_str.parse::<Multiaddr>() {
            match swarm.dial(addr.clone()) {
                Ok(_) => println!("Dialing bootstrap node: {}", addr),
                Err(e) => println!("Failed to dial bootstrap node {}: {:?}", addr, e),
            }
        }
    }

    // Trigger Kademlia bootstrap
    if let Err(e) = swarm.behaviour_mut().kad.bootstrap() {
        println!("Kademlia bootstrap error (expected if no peers yet): {:?}", e);
    }

    Ok((swarm, local_peer_id.to_string()))
}

async fn event_loop(
    mut swarm: Swarm<DhtBehaviour>,
    peers: Arc<Mutex<Vec<PeerInfo>>>,
    is_running: Arc<Mutex<bool>>,
    app: tauri::AppHandle,
    mut cmd_rx: mpsc::UnboundedReceiver<SwarmCommand>,
    file_transfer_service: Option<Arc<Mutex<crate::file_transfer::FileTransferService>>>,
    shared_files: SharedFilesMap,
    download_tiers: DownloadTiersMap,
) {
    // Track pending get queries
    let mut pending_get_queries: HashMap<kad::QueryId, tokio::sync::oneshot::Sender<Result<Option<String>, String>>> = HashMap::new();
    
    loop {
        let running = *is_running.lock().await;
        if !running {
            break;
        }
        
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(event) => {
                        handle_behaviour_event(event, &peers, &app, &mut swarm, &file_transfer_service, &mut pending_get_queries, &shared_files, &download_tiers).await;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Listening on {:?}", address);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        println!("Connection established with {:?}", peer_id);
                        let _ = app.emit("connection-established", peer_id.to_string());
                        
                        // If this is an incoming connection, notify that we're being pinged
                        if endpoint.is_listener() {
                            println!("Incoming connection from {}", peer_id);
                            let _ = app.emit("ping-received", peer_id.to_string());
                        }
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
                    SwarmCommand::SendPing(peer_id) => {
                        println!("Sending custom ping request to: {}", peer_id);
                        let request = PingRequest("PING".to_string());
                        let request_id = swarm.behaviour_mut().ping_protocol.send_request(&peer_id, request);
                        println!("Ping request sent with ID: {:?}", request_id);
                    }
                    SwarmCommand::SendFile { peer_id, transfer_id, file_name, file_data } => {
                        println!("Sending file '{}' to peer {}", file_name, peer_id);
                        let request = FileTransferRequest {
                            transfer_id: transfer_id.clone(),
                            file_name: file_name.clone(),
                            file_data,
                        };
                        let request_id = swarm.behaviour_mut().file_transfer.send_request(&peer_id, request);
                        println!("File transfer request sent with ID: {:?}", request_id);
                        let _ = app.emit("file-transfer-started", serde_json::json!({
                            "transferId": transfer_id,
                            "peerId": peer_id.to_string(),
                            "fileName": file_name
                        }));
                    }
                    SwarmCommand::PutDhtValue { key, value, response_tx } => {
                        println!("Storing DHT value for key: {}", key);
                        let record_key = kad::RecordKey::new(&key);
                        let record = kad::Record {
                            key: record_key,
                            value: value.into_bytes(),
                            publisher: None,
                            expires: None,
                        };
                        match swarm.behaviour_mut().kad.put_record(record, kad::Quorum::One) {
                            Ok(_) => {
                                println!("DHT put initiated for key: {}", key);
                                let _ = response_tx.send(Ok(()));
                            }
                            Err(e) => {
                                println!("Failed to initiate DHT put: {:?}", e);
                                let _ = response_tx.send(Err(format!("Failed to put DHT value: {:?}", e)));
                            }
                        }
                    }
                    SwarmCommand::GetDhtValue { key, response_tx } => {
                        println!("Getting DHT value for key: {}", key);
                        let record_key = kad::RecordKey::new(&key);
                        let query_id = swarm.behaviour_mut().kad.get_record(record_key);
                        pending_get_queries.insert(query_id, response_tx);
                    }
                    SwarmCommand::HealthCheck { response_tx } => {
                        let listeners: Vec<String> = swarm.listeners().map(|a| a.to_string()).collect();
                        let connected: Vec<PeerId> = swarm.connected_peers().cloned().collect();
                        let kad_peers: usize = swarm.behaviour_mut().kad.kbuckets()
                            .map(|b| b.num_entries())
                            .sum();

                        let bootstrap_addrs = get_bootstrap_nodes();
                        let bootstrap_status: Vec<BootstrapNodeStatus> = bootstrap_addrs.iter().map(|addr| {
                            // Check if any connected peer matches a bootstrap node's peer ID
                            let reachable = if let Ok(maddr) = addr.parse::<Multiaddr>() {
                                if let Some(pid) = extract_peer_id_from_multiaddr(&maddr) {
                                    connected.contains(&pid)
                                } else { false }
                            } else { false };
                            BootstrapNodeStatus {
                                address: addr.clone(),
                                reachable,
                            }
                        }).collect();

                        let protocols = vec![
                            "/chiral/id/1.0.0".to_string(),
                            "/chiral/ping/1.0.0".to_string(),
                            "/chiral/file-transfer/1.0.0".to_string(),
                            "/chiral/file-request/1.0.0".to_string(),
                            "/ipfs/kad/1.0.0".to_string(),
                        ];

                        let _ = response_tx.send(DhtHealthInfo {
                            running: true,
                            peer_id: None, // filled in by get_health()
                            listening_addresses: listeners,
                            connected_peer_count: connected.len(),
                            kademlia_peers: kad_peers,
                            bootstrap_nodes: bootstrap_status,
                            shared_files: 0, // filled in by get_health()
                            protocols,
                        });
                    }
                    SwarmCommand::CheckPeerConnected { peer_id, response_tx } => {
                        let is_connected = swarm.is_connected(&peer_id);
                        let _ = response_tx.send(is_connected);
                    }
                    SwarmCommand::RequestFile { peer_id, request_id, file_hash } => {
                        println!("Requesting file {} from peer {}", file_hash, peer_id);

                        // Check if peer is actually connected before sending request
                        if !swarm.is_connected(&peer_id) {
                            println!("⚠️ Peer {} is not connected, attempting to dial...", peer_id);
                            // Try to dial the peer - libp2p may know their address from DHT/Kademlia
                            match swarm.dial(peer_id) {
                                Ok(_) => {
                                    println!("📡 Dialing peer {}...", peer_id);
                                    // Give a short window for connection to establish
                                    // The request will be sent, but if the peer is truly offline,
                                    // libp2p will emit OutboundFailure
                                }
                                Err(e) => {
                                    println!("❌ Cannot reach peer {}: {:?}", peer_id, e);
                                    let _ = app.emit("file-download-failed", serde_json::json!({
                                        "requestId": request_id,
                                        "fileHash": file_hash,
                                        "error": format!("Seeder is offline or unreachable (peer: {}...)", &peer_id.to_string()[..8])
                                    }));
                                    continue;
                                }
                            }
                        }

                        let request = FileRequestMessage {
                            request_id: request_id.clone(),
                            file_hash: file_hash.clone(),
                        };
                        let req_id = swarm.behaviour_mut().file_request.send_request(&peer_id, request);
                        println!("File request sent with ID: {:?}", req_id);
                        let _ = app.emit("file-request-sent", serde_json::json!({
                            "requestId": request_id,
                            "fileHash": file_hash,
                            "peerId": peer_id.to_string()
                        }));
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
    swarm: &mut Swarm<DhtBehaviour>,
    file_transfer_service: &Option<Arc<Mutex<crate::file_transfer::FileTransferService>>>,
    pending_get_queries: &mut HashMap<kad::QueryId, tokio::sync::oneshot::Sender<Result<Option<String>, String>>>,
    shared_files: &SharedFilesMap,
    download_tiers: &DownloadTiersMap,
) {
    match event {
        DhtBehaviourEvent::Mdns(mdns::Event::Discovered(list)) => {
            let mut peers_guard = peers.lock().await;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            
            for (peer_id, multiaddr) in list {
                let peer_id_str = peer_id.to_string();
                println!("Discovered peer: {}", peer_id_str);
                
                // Add peer to Kademlia routing table
                swarm.behaviour_mut().kad.add_address(&peer_id, multiaddr.clone());
                
                // Check if peer already exists
                if let Some(existing) = peers_guard.iter_mut().find(|p| p.id == peer_id_str) {
                    existing.last_seen = now;
                    if !existing.multiaddrs.contains(&multiaddr.to_string()) {
                        existing.multiaddrs.push(multiaddr.to_string());
                    }
                } else {
                    let peer_info = PeerInfo {
                        id: peer_id_str.clone(),
                        address: peer_id_str.clone(),
                        multiaddrs: vec![multiaddr.to_string()],
                        last_seen: now,
                    };
                    peers_guard.push(peer_info);
                }
            }
            
            // Bootstrap Kademlia when we have peers
            if let Err(e) = swarm.behaviour_mut().kad.bootstrap() {
                println!("Kademlia bootstrap error: {:?}", e);
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
        DhtBehaviourEvent::PingProtocol(event) => {
            use request_response::Event;
            match event {
                Event::Message { peer, message } => {
                    match message {
                        request_response::Message::Request { request, channel, .. } => {
                            println!("Received ping request from {}: {:?}", peer, request);
                            let _ = app.emit("ping-received", peer.to_string());
                            // Send pong response
                            let response = PingResponse("PONG".to_string());
                            if let Err(e) = swarm.behaviour_mut().ping_protocol.send_response(channel, response) {
                                println!("Failed to send ping response: {:?}", e);
                            } else {
                                println!("Sent PONG response to {}", peer);
                            }
                        }
                        request_response::Message::Response { response, .. } => {
                            println!("Received ping response from {}: {:?}", peer, response);
                            let _ = app.emit("pong-received", peer.to_string());
                        }
                    }
                }
                Event::OutboundFailure { peer, error, .. } => {
                    println!("Ping request failed to {:?}: {:?}", peer, error);
                }
                Event::InboundFailure { peer, error, .. } => {
                    println!("Inbound ping failed from {:?}: {:?}", peer, error);
                }
                _ => {}
            }
        }
        DhtBehaviourEvent::Kad(kad::Event::OutboundQueryProgressed { id, result, .. }) => {
            match result {
                kad::QueryResult::GetRecord(Ok(kad::GetRecordOk::FoundRecord(record))) => {
                    let key_bytes = record.record.key.as_ref();
                    let key_str = String::from_utf8_lossy(key_bytes);
                    println!("DHT get successful for query {:?}, key: {}", id, key_str);
                    let value = String::from_utf8(record.record.value.clone())
                        .unwrap_or_else(|_| String::new());
                    println!("DHT record value length: {} bytes", value.len());
                    if let Some(tx) = pending_get_queries.remove(&id) {
                        let _ = tx.send(Ok(Some(value)));
                    }
                }
                kad::QueryResult::GetRecord(Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. })) => {
                    println!("DHT get finished with no additional records for query {:?}", id);
                }
                kad::QueryResult::GetRecord(Err(err)) => {
                    println!("DHT get failed for query {:?}: {:?}", id, err);
                    if let Some(tx) = pending_get_queries.remove(&id) {
                        let _ = tx.send(Ok(None));
                    }
                }
                kad::QueryResult::PutRecord(Ok(ok)) => {
                    let key_bytes = ok.key.as_ref();
                    let key_str = String::from_utf8_lossy(key_bytes);
                    println!("DHT put successful for query {:?}, key: {}", id, key_str);
                }
                kad::QueryResult::PutRecord(Err(err)) => {
                    println!("DHT put failed for query {:?}: {:?}", id, err);
                }
                kad::QueryResult::Bootstrap(Ok(result)) => {
                    println!("Kademlia bootstrap successful: {:?} peers", result.num_remaining);
                }
                kad::QueryResult::Bootstrap(Err(err)) => {
                    println!("Kademlia bootstrap failed: {:?}", err);
                }
                _ => {}
            }
        }
        DhtBehaviourEvent::Identify(event) => {
            println!("Identify event: {:?}", event);
        }
        DhtBehaviourEvent::FileTransfer(event) => {
            use request_response::Event;
            match event {
                Event::Message { peer, message } => {
                    match message {
                        request_response::Message::Request { request, channel, .. } => {
                            println!("Received file transfer from {}: {}", peer, request.file_name);
                            let _file_size = request.file_data.len();

                            // Store file data in FileTransferService for later acceptance
                            if let Some(fts) = file_transfer_service {
                                let fts_lock = fts.lock().await;
                                let _ = fts_lock.receive_file_request(
                                    app.clone(),
                                    peer.to_string(),
                                    request.file_name.clone(),
                                    request.file_data.clone(),
                                    request.transfer_id.clone()
                                ).await;
                            }

                            // Auto-accept for now (response is required by protocol)
                            // In a real implementation, we'd wait for user action and cache the channel
                            let response = FileTransferResponse {
                                transfer_id: request.transfer_id.clone(),
                                accepted: true,
                                error: None,
                            };
                            if let Err(e) = swarm.behaviour_mut().file_transfer.send_response(channel, response) {
                                println!("Failed to send file transfer response: {:?}", e);
                            }
                        }
                        request_response::Message::Response { response, .. } => {
                            println!("Received file transfer response from {}: accepted={}", peer, response.accepted);
                            if response.accepted {
                                let _ = app.emit("file-transfer-complete", serde_json::json!({
                                    "transferId": response.transfer_id,
                                    "status": "completed"
                                }));
                            } else {
                                let _ = app.emit("file-transfer-complete", serde_json::json!({
                                    "transferId": response.transfer_id,
                                    "status": "declined",
                                    "error": response.error
                                }));
                            }
                        }
                    }
                }
                Event::OutboundFailure { peer, error, .. } => {
                    let peer_short = &peer.to_string()[..std::cmp::min(8, peer.to_string().len())];
                    let user_error = match &error {
                        request_response::OutboundFailure::DialFailure |
                        request_response::OutboundFailure::UnsupportedProtocols => {
                            format!("Seeder ({}...) is offline or unreachable", peer_short)
                        }
                        request_response::OutboundFailure::Timeout => {
                            format!("Seeder ({}...) did not respond in time", peer_short)
                        }
                        _ => format!("Transfer failed to ({}...): {:?}", peer_short, error)
                    };
                    println!("❌ File transfer failed to {:?}: {:?}", peer, error);
                    let _ = app.emit("file-transfer-failed", serde_json::json!({
                        "peerId": peer.to_string(),
                        "error": user_error
                    }));
                }
                Event::InboundFailure { peer, error, .. } => {
                    println!("Inbound file transfer failed from {:?}: {:?}", peer, error);
                }
                _ => {}
            }
        }
        DhtBehaviourEvent::FileRequest(event) => {
            use request_response::Event;
            match event {
                Event::Message { peer, message } => {
                    match message {
                        request_response::Message::Request { request, channel, .. } => {
                            println!("Received file request from {}: hash={}", peer, request.file_hash);

                            // Look up the file in our shared files
                            let shared = shared_files.lock().await;

                            // Debug: print all shared files
                            println!("Currently sharing {} files:", shared.len());
                            for (hash, info) in shared.iter() {
                                println!("  - {} (hash: {})", info.file_name, hash);
                            }

                            let response = if let Some(file_info) = shared.get(&request.file_hash) {
                                // Read the file and send it
                                match std::fs::read(&file_info.file_path) {
                                    Ok(file_data) => {
                                        println!("Serving file {} ({} bytes) to peer {}", file_info.file_name, file_data.len(), peer);
                                        FileRequestResponse {
                                            request_id: request.request_id.clone(),
                                            file_hash: request.file_hash.clone(),
                                            file_name: file_info.file_name.clone(),
                                            file_data: Some(file_data),
                                            error: None,
                                        }
                                    }
                                    Err(e) => {
                                        println!("Failed to read file {}: {}", file_info.file_path, e);
                                        FileRequestResponse {
                                            request_id: request.request_id.clone(),
                                            file_hash: request.file_hash.clone(),
                                            file_name: file_info.file_name.clone(),
                                            file_data: None,
                                            error: Some(format!("Failed to read file: {}", e)),
                                        }
                                    }
                                }
                            } else {
                                println!("File not found: {}", request.file_hash);
                                FileRequestResponse {
                                    request_id: request.request_id.clone(),
                                    file_hash: request.file_hash.clone(),
                                    file_name: String::new(),
                                    file_data: None,
                                    error: Some("File not found".to_string()),
                                }
                            };
                            drop(shared);

                            if let Err(e) = swarm.behaviour_mut().file_request.send_response(channel, response) {
                                println!("Failed to send file request response: {:?}", e);
                            }
                        }
                        request_response::Message::Response { response, .. } => {
                            println!("📥 Received file response from {}: hash={}, success={}",
                                     peer, response.file_hash, response.file_data.is_some());

                            if let Some(file_data) = response.file_data {
                                // Look up speed tier for this download
                                let tier = {
                                    let mut tiers = download_tiers.lock().await;
                                    tiers.remove(&response.request_id).unwrap_or(SpeedTier::Free)
                                };

                                // Save the file to Downloads folder with rate limiting
                                if let Some(downloads_dir) = dirs::download_dir() {
                                    let file_name = if response.file_name.is_empty() {
                                        format!("{}.download", &response.file_hash[..8])
                                    } else {
                                        response.file_name.clone()
                                    };
                                    let file_path = downloads_dir.join(&file_name);

                                    // Spawn rate-limited write task
                                    let app_clone = app.clone();
                                    let request_id = response.request_id.clone();
                                    let file_hash = response.file_hash.clone();
                                    let file_name_clone = file_name.clone();

                                    tokio::spawn(async move {
                                        println!("⚡ Writing file with {:?} tier rate limiting", tier);
                                        match crate::speed_tiers::rate_limited_write(
                                            &app_clone, &file_path, &file_data, &tier,
                                            &request_id, &file_hash, &file_name_clone,
                                        ).await {
                                            Ok(_) => {
                                                println!("✅ File saved to: {:?}", file_path);
                                                let _ = app_clone.emit("file-download-complete", serde_json::json!({
                                                    "requestId": request_id,
                                                    "fileHash": file_hash,
                                                    "fileName": file_name_clone,
                                                    "filePath": file_path.to_string_lossy(),
                                                    "fileSize": file_data.len(),
                                                    "status": "completed"
                                                }));
                                            }
                                            Err(e) => {
                                                println!("❌ Failed to save file: {}", e);
                                                let _ = app_clone.emit("file-download-failed", serde_json::json!({
                                                    "requestId": request_id,
                                                    "fileHash": file_hash,
                                                    "error": format!("Failed to save file: {}", e)
                                                }));
                                            }
                                        }
                                    });
                                }
                            } else {
                                // Clean up tier entry on failure
                                {
                                    let mut tiers = download_tiers.lock().await;
                                    tiers.remove(&response.request_id);
                                }
                                let _ = app.emit("file-download-failed", serde_json::json!({
                                    "requestId": response.request_id,
                                    "fileHash": response.file_hash,
                                    "error": response.error.unwrap_or_else(|| "Unknown error".to_string())
                                }));
                            }
                        }
                    }
                }
                Event::OutboundFailure { peer, error, .. } => {
                    let peer_short = &peer.to_string()[..std::cmp::min(8, peer.to_string().len())];
                    let user_error = match &error {
                        request_response::OutboundFailure::DialFailure => {
                            format!("Seeder ({}...) is offline or unreachable. They may have disconnected from the network.", peer_short)
                        }
                        request_response::OutboundFailure::UnsupportedProtocols => {
                            format!("Seeder ({}...) is offline or running an incompatible version. The file cannot be downloaded from this peer.", peer_short)
                        }
                        request_response::OutboundFailure::Timeout => {
                            format!("Seeder ({}...) did not respond in time. They may be offline or experiencing network issues.", peer_short)
                        }
                        _ => {
                            format!("Failed to reach seeder ({}...): {:?}", peer_short, error)
                        }
                    };
                    println!("❌ File request failed to {:?}: {:?}", peer, error);
                    let _ = app.emit("file-download-failed", serde_json::json!({
                        "peerId": peer.to_string(),
                        "error": user_error
                    }));
                }
                Event::InboundFailure { peer, error, .. } => {
                    println!("Inbound file request failed from {:?}: {:?}", peer, error);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_nodes_not_empty() {
        let nodes = get_bootstrap_nodes();
        assert!(!nodes.is_empty());
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_bootstrap_nodes_are_valid_multiaddrs() {
        for addr_str in get_bootstrap_nodes() {
            let parsed = addr_str.parse::<Multiaddr>();
            assert!(parsed.is_ok(), "Failed to parse multiaddr: {}", addr_str);
        }
    }

    #[test]
    fn test_bootstrap_nodes_contain_peer_ids() {
        for addr_str in get_bootstrap_nodes() {
            let addr: Multiaddr = addr_str.parse().unwrap();
            let peer_id = extract_peer_id_from_multiaddr(&addr);
            assert!(peer_id.is_some(), "No peer ID in: {}", addr_str);
        }
    }

    #[test]
    fn test_bootstrap_nodes_have_unique_peer_ids() {
        let mut peer_ids = Vec::new();
        for addr_str in get_bootstrap_nodes() {
            let addr: Multiaddr = addr_str.parse().unwrap();
            let peer_id = extract_peer_id_from_multiaddr(&addr).unwrap();
            assert!(!peer_ids.contains(&peer_id), "Duplicate peer ID: {}", peer_id);
            peer_ids.push(peer_id);
        }
    }

    #[test]
    fn test_extract_peer_id_from_valid_multiaddr() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE"
            .parse()
            .unwrap();
        let peer_id = extract_peer_id_from_multiaddr(&addr);
        assert!(peer_id.is_some());
        assert_eq!(
            peer_id.unwrap().to_string(),
            "12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE"
        );
    }

    #[test]
    fn test_extract_peer_id_from_multiaddr_without_p2p() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/4001".parse().unwrap();
        let peer_id = extract_peer_id_from_multiaddr(&addr);
        assert!(peer_id.is_none());
    }

    #[test]
    fn test_remove_peer_id_from_multiaddr() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE"
            .parse()
            .unwrap();
        let transport = remove_peer_id_from_multiaddr(&addr);
        assert_eq!(transport.to_string(), "/ip4/127.0.0.1/tcp/4001");
    }

    #[test]
    fn test_remove_peer_id_from_multiaddr_without_p2p() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/4001".parse().unwrap();
        let transport = remove_peer_id_from_multiaddr(&addr);
        assert_eq!(transport.to_string(), "/ip4/127.0.0.1/tcp/4001");
    }

    #[test]
    fn test_ping_request_serialization() {
        let ping = PingRequest("PING".to_string());
        let json = serde_json::to_string(&ping).unwrap();
        let deserialized: PingRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(ping, deserialized);
    }

    #[test]
    fn test_ping_response_serialization() {
        let pong = PingResponse("PONG".to_string());
        let json = serde_json::to_string(&pong).unwrap();
        let deserialized: PingResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(pong, deserialized);
    }

    #[test]
    fn test_file_transfer_request_serialization() {
        let request = FileTransferRequest {
            transfer_id: "tx-001".to_string(),
            file_name: "test.txt".to_string(),
            file_data: vec![1, 2, 3, 4, 5],
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: FileTransferRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request.transfer_id, deserialized.transfer_id);
        assert_eq!(request.file_name, deserialized.file_name);
        assert_eq!(request.file_data, deserialized.file_data);
    }

    #[test]
    fn test_file_transfer_response_serialization() {
        let response = FileTransferResponse {
            transfer_id: "tx-001".to_string(),
            accepted: true,
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: FileTransferResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.transfer_id, deserialized.transfer_id);
        assert_eq!(response.accepted, deserialized.accepted);
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_file_transfer_response_with_error() {
        let response = FileTransferResponse {
            transfer_id: "tx-002".to_string(),
            accepted: false,
            error: Some("Transfer declined by user".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: FileTransferResponse = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.accepted);
        assert_eq!(deserialized.error.unwrap(), "Transfer declined by user");
    }

    #[test]
    fn test_file_request_message_serialization() {
        let msg = FileRequestMessage {
            request_id: "req-001".to_string(),
            file_hash: "abc123def456".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: FileRequestMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.request_id, deserialized.request_id);
        assert_eq!(msg.file_hash, deserialized.file_hash);
    }

    #[test]
    fn test_file_request_response_with_data() {
        let resp = FileRequestResponse {
            request_id: "req-001".to_string(),
            file_hash: "abc123".to_string(),
            file_name: "document.pdf".to_string(),
            file_data: Some(vec![10, 20, 30]),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: FileRequestResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.file_data.unwrap(), vec![10, 20, 30]);
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_file_request_response_not_found() {
        let resp = FileRequestResponse {
            request_id: "req-002".to_string(),
            file_hash: "nonexistent".to_string(),
            file_name: String::new(),
            file_data: None,
            error: Some("File not found".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: FileRequestResponse = serde_json::from_str(&json).unwrap();
        assert!(deserialized.file_data.is_none());
        assert_eq!(deserialized.error.unwrap(), "File not found");
    }

    #[test]
    fn test_peer_info_serialization() {
        let peer = PeerInfo {
            id: "12D3KooWFYTuQ2FY".to_string(),
            address: "12D3KooWFYTuQ2FY".to_string(),
            multiaddrs: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            last_seen: 1700000000,
        };
        let json = serde_json::to_string(&peer).unwrap();
        // camelCase serialization
        assert!(json.contains("lastSeen"));
        let deserialized: PeerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(peer.id, deserialized.id);
        assert_eq!(peer.multiaddrs, deserialized.multiaddrs);
    }

    #[test]
    fn test_network_stats_serialization() {
        let stats = NetworkStats {
            connected_peers: 5,
            total_peers: 10,
        };
        let json = serde_json::to_string(&stats).unwrap();
        // camelCase serialization
        assert!(json.contains("connectedPeers"));
        assert!(json.contains("totalPeers"));
    }

    #[test]
    fn test_shared_file_info() {
        let info = SharedFileInfo {
            file_path: "/path/to/file.txt".to_string(),
            file_name: "file.txt".to_string(),
            file_size: 1024,
        };
        assert_eq!(info.file_name, "file.txt");
        assert_eq!(info.file_size, 1024);
    }
}
