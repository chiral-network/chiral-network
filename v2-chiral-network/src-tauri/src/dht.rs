use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use libp2p::{
    kad, mdns, noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Swarm, PeerId, StreamProtocol,
    identify, request_response,
};
use futures::StreamExt;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use std::str::FromStr;
use std::collections::HashMap;

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

pub struct DhtService {
    peers: Arc<Mutex<Vec<PeerInfo>>>,
    is_running: Arc<Mutex<bool>>,
    local_peer_id: Arc<Mutex<Option<String>>>,
    command_sender: Arc<Mutex<Option<mpsc::UnboundedSender<SwarmCommand>>>>,
    file_transfer_service: Option<Arc<Mutex<crate::file_transfer::FileTransferService>>>,
    shared_files: SharedFilesMap,
}

impl DhtService {
    pub fn new(file_transfer_service: Arc<Mutex<crate::file_transfer::FileTransferService>>) -> Self {
        Self {
            peers: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
            local_peer_id: Arc::new(Mutex::new(None)),
            command_sender: Arc::new(Mutex::new(None)),
            file_transfer_service: Some(file_transfer_service),
            shared_files: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Register a file for sharing (seeding)
    pub async fn register_shared_file(&self, file_hash: String, file_path: String, file_name: String, file_size: u64) {
        let mut shared = self.shared_files.lock().await;
        println!("Registered shared file: {} (hash: {})", file_name, file_hash);
        shared.insert(file_hash, SharedFileInfo {
            file_path,
            file_name,
            file_size,
        });
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

        tokio::spawn(async move {
            event_loop(swarm, peers_clone, is_running_clone, app, cmd_rx, file_transfer_clone, shared_files_clone).await;
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
                        handle_behaviour_event(event, &peers, &app, &mut swarm, &file_transfer_service, &mut pending_get_queries, &shared_files).await;
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
                    SwarmCommand::RequestFile { peer_id, request_id, file_hash } => {
                        println!("Requesting file {} from peer {}", file_hash, peer_id);
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
                    println!("DHT get successful for query {:?}", id);
                    let value = String::from_utf8(record.record.value.clone())
                        .unwrap_or_else(|_| String::new());
                    if let Some(tx) = pending_get_queries.remove(&id) {
                        let _ = tx.send(Ok(Some(value)));
                    }
                }
                kad::QueryResult::GetRecord(Err(err)) => {
                    println!("DHT get failed for query {:?}: {:?}", id, err);
                    if let Some(tx) = pending_get_queries.remove(&id) {
                        let _ = tx.send(Ok(None));
                    }
                }
                kad::QueryResult::PutRecord(Ok(_)) => {
                    println!("DHT put successful for query {:?}", id);
                }
                kad::QueryResult::PutRecord(Err(err)) => {
                    println!("DHT put failed for query {:?}: {:?}", id, err);
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
                    println!("File transfer failed to {:?}: {:?}", peer, error);
                    let _ = app.emit("file-transfer-failed", serde_json::json!({
                        "peerId": peer.to_string(),
                        "error": format!("{:?}", error)
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
                            println!("Received file response from {}: hash={}, success={}",
                                     peer, response.file_hash, response.file_data.is_some());

                            if let Some(file_data) = response.file_data {
                                // Save the file to Downloads folder
                                if let Some(downloads_dir) = dirs::download_dir() {
                                    let file_name = if response.file_name.is_empty() {
                                        format!("{}.download", &response.file_hash[..8])
                                    } else {
                                        response.file_name.clone()
                                    };
                                    let file_path = downloads_dir.join(&file_name);

                                    match std::fs::write(&file_path, &file_data) {
                                        Ok(_) => {
                                            println!("File saved to: {:?}", file_path);
                                            let _ = app.emit("file-download-complete", serde_json::json!({
                                                "requestId": response.request_id,
                                                "fileHash": response.file_hash,
                                                "fileName": file_name,
                                                "filePath": file_path.to_string_lossy(),
                                                "fileSize": file_data.len(),
                                                "status": "completed"
                                            }));
                                        }
                                        Err(e) => {
                                            println!("Failed to save file: {}", e);
                                            let _ = app.emit("file-download-failed", serde_json::json!({
                                                "requestId": response.request_id,
                                                "fileHash": response.file_hash,
                                                "error": format!("Failed to save file: {}", e)
                                            }));
                                        }
                                    }
                                }
                            } else {
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
                    println!("File request failed to {:?}: {:?}", peer, error);
                    let _ = app.emit("file-download-failed", serde_json::json!({
                        "peerId": peer.to_string(),
                        "error": format!("{:?}", error)
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
