use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};
use sha2::{Digest, Sha256};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use crate::FileTransferService;

const CHUNK_SIZE: usize = 16384; // 16KB chunks

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCFileRequest {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub requester_peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    pub file_hash: String,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub data: Vec<u8>,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub file_hash: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub chunks_transferred: u32,
    pub total_chunks: u32,
    pub percentage: f32,
}

pub struct PeerConnection {
    pub peer_id: String,
    pub is_connected: bool,
    pub active_transfers: HashMap<String, ActiveTransfer>,
    pub last_activity: Instant,
    pub peer_connection: Option<Arc<RTCPeerConnection>>,
    pub data_channel: Option<Arc<RTCDataChannel>>,
    pub pending_chunks: HashMap<String, Vec<FileChunk>>, // file_hash -> chunks
    pub received_chunks: HashMap<String, HashMap<u32, FileChunk>>, // file_hash -> chunk_index -> chunk
}

#[derive(Debug)]
pub struct ActiveTransfer {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub total_chunks: u32,
    pub chunks_sent: u32,
    pub bytes_sent: u64,
    pub start_time: Instant,
}

#[derive(Debug)]
pub enum WebRTCCommand {
    EstablishConnection {
        peer_id: String,
        offer: String,
    },
    HandleAnswer {
        peer_id: String,
        answer: String,
    },
    AddIceCandidate {
        peer_id: String,
        candidate: String,
    },
    SendFileRequest {
        peer_id: String,
        request: WebRTCFileRequest,
    },
    SendFileChunk {
        peer_id: String,
        chunk: FileChunk,
    },
    RequestFileChunk {
        peer_id: String,
        file_hash: String,
        chunk_index: u32,
    },
    CloseConnection {
        peer_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum WebRTCEvent {
    ConnectionEstablished {
        peer_id: String,
    },
    ConnectionFailed {
        peer_id: String,
        error: String,
    },
    OfferCreated {
        peer_id: String,
        offer: String,
    },
    AnswerReceived {
        peer_id: String,
        answer: String,
    },
    IceCandidate {
        peer_id: String,
        candidate: String,
    },
    FileRequestReceived {
        peer_id: String,
        request: WebRTCFileRequest,
    },
    FileChunkReceived {
        peer_id: String,
        chunk: FileChunk,
    },
    FileChunkRequested {
        peer_id: String,
        file_hash: String,
        chunk_index: u32,
    },
    TransferProgress {
        peer_id: String,
        progress: TransferProgress,
    },
    TransferCompleted {
        peer_id: String,
        file_hash: String,
    },
    TransferFailed {
        peer_id: String,
        file_hash: String,
        error: String,
    },
}

pub struct WebRTCService {
    cmd_tx: mpsc::Sender<WebRTCCommand>,
    event_tx: mpsc::Sender<WebRTCEvent>,
    event_rx: Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>,
    connections: Arc<Mutex<HashMap<String, PeerConnection>>>,
    file_transfer_service: Arc<FileTransferService>,
}

impl WebRTCService {
    pub async fn new(file_transfer_service: Arc<FileTransferService>) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        let connections = Arc::new(Mutex::new(HashMap::new()));

        // Spawn the WebRTC service task
        tokio::spawn(Self::run_webrtc_service(
            cmd_rx,
            event_tx.clone(),
            connections.clone(),
            file_transfer_service.clone(),
        ));

        Ok(WebRTCService {
            cmd_tx,
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            connections,
            file_transfer_service,
        })
    }

    async fn run_webrtc_service(
        mut cmd_rx: mpsc::Receiver<WebRTCCommand>,
        event_tx: mpsc::Sender<WebRTCEvent>,
        connections: Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: Arc<FileTransferService>,
    ) {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                WebRTCCommand::EstablishConnection { peer_id, offer } => {
                    Self::handle_establish_connection(&peer_id, &offer, &event_tx, &connections, &file_transfer_service).await;
                }
                WebRTCCommand::HandleAnswer { peer_id, answer } => {
                    Self::handle_answer(&peer_id, &answer, &connections).await;
                }
                WebRTCCommand::AddIceCandidate { peer_id, candidate } => {
                    Self::handle_ice_candidate(&peer_id, &candidate, &connections).await;
                }
                WebRTCCommand::SendFileRequest { peer_id, request } => {
                    Self::handle_file_request(&peer_id, &request, &event_tx, &file_transfer_service, &connections).await;
                }
                WebRTCCommand::SendFileChunk { peer_id, chunk } => {
                    Self::handle_send_chunk(&peer_id, &chunk, &connections).await;
                }
                WebRTCCommand::RequestFileChunk { peer_id, file_hash, chunk_index } => {
                    Self::handle_request_chunk(&peer_id, &file_hash, chunk_index, &event_tx, &connections).await;
                }
                WebRTCCommand::CloseConnection { peer_id } => {
                    Self::handle_close_connection(&peer_id, &connections).await;
                }
            }
        }
    }

    async fn handle_establish_connection(
        peer_id: &str,
        offer_sdp: &str,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: &Arc<FileTransferService>,
    ) {
        info!("Establishing WebRTC connection with peer: {}", peer_id);

        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection
        let config = RTCConfiguration::default();
        let peer_connection = match api.new_peer_connection(config).await {
            Ok(pc) => Arc::new(pc),
            Err(e) => {
                error!("Failed to create peer connection: {}", e);
                let _ = event_tx.send(WebRTCEvent::ConnectionFailed {
                    peer_id: peer_id.to_string(),
                    error: e.to_string(),
                }).await;
                return;
            }
        };

        // Create data channel
        let data_channel = match peer_connection.create_data_channel("file-transfer", None).await {
            Ok(dc) => dc,
            Err(e) => {
                error!("Failed to create data channel: {}", e);
                let _ = event_tx.send(WebRTCEvent::ConnectionFailed {
                    peer_id: peer_id.to_string(),
                    error: e.to_string(),
                }).await;
                return;
            }
        };

        // Set up data channel event handlers
        let event_tx_clone = event_tx.clone();
        let peer_id_clone = peer_id.to_string();
        let file_transfer_service_clone = file_transfer_service.clone();
        let connections_clone = connections.clone();

        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();

            Box::pin(async move {
                Self::handle_data_channel_message(&peer_id, &msg, &event_tx, &file_transfer_service, &connections).await;
            })
        }));

        // Set up peer connection event handlers
        let event_tx_clone = event_tx.clone();
        let peer_id_clone = peer_id.to_string();
        let data_channel_clone = data_channel.clone();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(candidate_str) = serde_json::to_string(&candidate.to_json().unwrap_or_default()) {
                        let _ = event_tx.send(WebRTCEvent::IceCandidate {
                            peer_id,
                            candidate: candidate_str,
                        }).await;
                    }
                }
            })
        }));

        peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let _data_channel = data_channel_clone.clone();

            Box::pin(async move {
                match state {
                    RTCPeerConnectionState::Connected => {
                        info!("WebRTC connection established with peer: {}", peer_id);
                        let _ = event_tx.send(WebRTCEvent::ConnectionEstablished {
                            peer_id,
                        }).await;
                    }
                    RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                        info!("WebRTC connection closed with peer: {}", peer_id);
                    }
                    _ => {}
                }
            })
        }));

        // Set remote description from offer
        let offer = match serde_json::from_str::<RTCSessionDescription>(offer_sdp) {
            Ok(offer) => offer,
            Err(e) => {
                error!("Failed to parse offer SDP: {}", e);
                let _ = event_tx.send(WebRTCEvent::ConnectionFailed {
                    peer_id: peer_id.to_string(),
                    error: format!("Invalid offer SDP: {}", e),
                }).await;
                return;
            }
        };

        if let Err(e) = peer_connection.set_remote_description(offer).await {
            error!("Failed to set remote description: {}", e);
            let _ = event_tx.send(WebRTCEvent::ConnectionFailed {
                peer_id: peer_id.to_string(),
                error: e.to_string(),
            }).await;
            return;
        }

        // Create answer
        let answer = match peer_connection.create_answer(None).await {
            Ok(answer) => answer,
            Err(e) => {
                error!("Failed to create answer: {}", e);
                let _ = event_tx.send(WebRTCEvent::ConnectionFailed {
                    peer_id: peer_id.to_string(),
                    error: e.to_string(),
                }).await;
                return;
            }
        };

        // Set local description
        if let Err(e) = peer_connection.set_local_description(answer).await {
            error!("Failed to set local description: {}", e);
            let _ = event_tx.send(WebRTCEvent::ConnectionFailed {
                peer_id: peer_id.to_string(),
                error: e.to_string(),
            }).await;
            return;
        }

        // Send answer
        if let Some(local_desc) = peer_connection.local_description().await {
            if let Ok(answer_str) = serde_json::to_string(&local_desc) {
                let _ = event_tx.send(WebRTCEvent::AnswerReceived {
                    peer_id: peer_id.to_string(),
                    answer: answer_str,
                }).await;
            }
        }

        // Store connection
        let mut conns = connections.lock().await;
        let connection = PeerConnection {
            peer_id: peer_id.to_string(),
            is_connected: false, // Will be set to true when connected
            active_transfers: HashMap::new(),
            last_activity: Instant::now(),
            peer_connection: Some(peer_connection),
            data_channel: Some(data_channel),
            pending_chunks: HashMap::new(),
            received_chunks: HashMap::new(),
        };
        conns.insert(peer_id.to_string(), connection);
    }

    async fn handle_answer(peer_id: &str, answer_sdp: &str, connections: &Arc<Mutex<HashMap<String, PeerConnection>>>) {
        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            if let Some(pc) = &connection.peer_connection {
                let answer = match serde_json::from_str::<RTCSessionDescription>(answer_sdp) {
                    Ok(answer) => answer,
                    Err(e) => {
                        error!("Failed to parse answer SDP: {}", e);
                        return;
                    }
                };

                if let Err(e) = pc.set_remote_description(answer).await {
                    error!("Failed to set remote description: {}", e);
                }
            }
        }
    }

    async fn handle_ice_candidate(peer_id: &str, candidate_str: &str, connections: &Arc<Mutex<HashMap<String, PeerConnection>>>) {
        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            if let Some(pc) = &connection.peer_connection {
                let candidate_init = match serde_json::from_str::<RTCIceCandidateInit>(candidate_str) {
                    Ok(candidate) => candidate,
                    Err(e) => {
                        error!("Failed to parse ICE candidate: {}", e);
                        return;
                    }
                };

                if let Err(e) = pc.add_ice_candidate(candidate_init).await {
                    error!("Failed to add ICE candidate: {}", e);
                }
            }
        }
    }

    async fn handle_file_request(
        peer_id: &str,
        request: &WebRTCFileRequest,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
        info!("Handling file request from peer {}: {}", peer_id, request.file_hash);

        // Check if we have the file locally
        let stored_files = file_transfer_service.get_stored_files().await.unwrap_or_default();
        let has_file = stored_files.iter().any(|(hash, _)| hash == &request.file_hash);

        if has_file {
            // Start sending file chunks
            Self::start_file_transfer(peer_id, request, event_tx, file_transfer_service, connections).await;
        } else {
            let _ = event_tx.send(WebRTCEvent::TransferFailed {
                peer_id: peer_id.to_string(),
                file_hash: request.file_hash.clone(),
                error: "File not found locally".to_string(),
            }).await;
        }
    }

    async fn handle_send_chunk(peer_id: &str, chunk: &FileChunk, connections: &Arc<Mutex<HashMap<String, PeerConnection>>>) {
        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            if let Some(dc) = &connection.data_channel {
                // Serialize chunk and send over data channel
                match serde_json::to_string(chunk) {
                    Ok(chunk_json) => {
                        if let Err(e) = dc.send_text(chunk_json).await {
                            error!("Failed to send chunk over data channel: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize chunk: {}", e);
                    }
                }
            }
        }
    }

    async fn handle_request_chunk(
        peer_id: &str,
        file_hash: &str,
        chunk_index: u32,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        _connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
        let _ = event_tx.send(WebRTCEvent::FileChunkRequested {
            peer_id: peer_id.to_string(),
            file_hash: file_hash.to_string(),
            chunk_index,
        }).await;
    }

    async fn handle_close_connection(peer_id: &str, connections: &Arc<Mutex<HashMap<String, PeerConnection>>>) {
        info!("Closing WebRTC connection with peer: {}", peer_id);
        let mut conns = connections.lock().await;
        if let Some(mut connection) = conns.remove(peer_id) {
            if let Some(pc) = connection.peer_connection.take() {
                let _ = pc.close().await;
            }
        }
    }

    async fn handle_data_channel_message(
        peer_id: &str,
        msg: &DataChannelMessage,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
        if let Ok(text) = std::str::from_utf8(&msg.data) {
            // Try to parse as FileChunk
            if let Ok(chunk) = serde_json::from_str::<FileChunk>(text) {
                // Handle received chunk
                Self::process_incoming_chunk(&chunk, file_transfer_service, connections, event_tx, peer_id).await;
                let _ = event_tx.send(WebRTCEvent::FileChunkReceived {
                    peer_id: peer_id.to_string(),
                    chunk,
                }).await;
            }
            // Try to parse as WebRTCFileRequest
            else if let Ok(request) = serde_json::from_str::<WebRTCFileRequest>(text) {
                let _ = event_tx.send(WebRTCEvent::FileRequestReceived {
                    peer_id: peer_id.to_string(),
                    request: request.clone(),
                }).await;
                // Actually handle the file request to start transfer
                Self::handle_file_request(peer_id, &request, event_tx, file_transfer_service, connections).await;
            }
        }
    }

    async fn start_file_transfer(
        peer_id: &str,
        request: &WebRTCFileRequest,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
        // Get file data from local storage
        let file_data = match file_transfer_service.get_file_data(&request.file_hash).await {
            Some(data) => data,
            None => {
                let _ = event_tx.send(WebRTCEvent::TransferFailed {
                    peer_id: peer_id.to_string(),
                    file_hash: request.file_hash.clone(),
                    error: "File data not available".to_string(),
                }).await;
                return;
            }
        };

        info!("Starting real file transfer of {} ({} bytes) to peer {}",
              request.file_name, file_data.len(), peer_id);

        // Calculate total chunks
        let total_chunks = ((file_data.len() as f64) / CHUNK_SIZE as f64).ceil() as u32;

        // Initialize transfer tracking in connections
        {
            let mut conns = connections.lock().await;
            if let Some(connection) = conns.get_mut(peer_id) {
                let transfer = ActiveTransfer {
                    file_hash: request.file_hash.clone(),
                    file_name: request.file_name.clone(),
                    file_size: file_data.len() as u64,
                    total_chunks,
                    chunks_sent: 0,
                    bytes_sent: 0,
                    start_time: Instant::now(),
                };
                connection.active_transfers.insert(request.file_hash.clone(), transfer);
            }
        }

        // Send file chunks over WebRTC data channel
        for chunk_index in 0..total_chunks {
            let start = (chunk_index as usize) * CHUNK_SIZE;
            let end = (start + CHUNK_SIZE).min(file_data.len());
            let chunk_data: Vec<u8> = file_data[start..end].to_vec();

            // Calculate checksum for chunk
            let checksum = Self::calculate_chunk_checksum(&chunk_data);

            let chunk = FileChunk {
                file_hash: request.file_hash.clone(),
                chunk_index,
                total_chunks,
                data: chunk_data,
                checksum,
            };

            // Send chunk via WebRTC data channel
            Self::handle_send_chunk(peer_id, &chunk, connections).await;

            // Update progress
            {
                let mut conns = connections.lock().await;
                if let Some(connection) = conns.get_mut(peer_id) {
                    if let Some(transfer) = connection.active_transfers.get_mut(&request.file_hash) {
                        transfer.chunks_sent += 1;
                        transfer.bytes_sent += chunk.data.len() as u64;

                        // Send progress update
                        let progress = TransferProgress {
                            file_hash: request.file_hash.clone(),
                            bytes_transferred: transfer.bytes_sent,
                            total_bytes: transfer.file_size,
                            chunks_transferred: transfer.chunks_sent,
                            total_chunks: transfer.total_chunks,
                            percentage: (transfer.chunks_sent as f32 / transfer.total_chunks as f32) * 100.0,
                        };

                        let _ = event_tx.send(WebRTCEvent::TransferProgress {
                            peer_id: peer_id.to_string(),
                            progress,
                        }).await;
                    }
                }
            }

            // Small delay to avoid overwhelming
            sleep(Duration::from_millis(10)).await;
        }

        // Mark transfer as completed
        {
            let mut conns = connections.lock().await;
            if let Some(connection) = conns.get_mut(peer_id) {
                if let Some(transfer) = connection.active_transfers.get_mut(&request.file_hash) {
                    transfer.chunks_sent = total_chunks;
                    transfer.bytes_sent = file_data.len() as u64;
                }
            }
        }

        let _ = event_tx.send(WebRTCEvent::TransferCompleted {
            peer_id: peer_id.to_string(),
            file_hash: request.file_hash.clone(),
        }).await;
    }

    async fn process_incoming_chunk(
        chunk: &FileChunk,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        peer_id: &str,
    ) {
        // Verify chunk checksum
        let calculated_checksum = Self::calculate_chunk_checksum(&chunk.data);
        if calculated_checksum != chunk.checksum {
            warn!("Chunk checksum mismatch for file {}", chunk.file_hash);
            return;
        }

        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            // Store chunk
            let chunks = connection.received_chunks
                .entry(chunk.file_hash.clone())
                .or_insert_with(HashMap::new);
            chunks.insert(chunk.chunk_index, chunk.clone());

            // Check if we have all chunks for this file
            if let Some(total_chunks) = chunks.values().next().map(|c| c.total_chunks) {
                if chunks.len() == total_chunks as usize {
                    // Assemble file
                    Self::assemble_file_from_chunks(&chunk.file_hash, chunks, file_transfer_service, event_tx, peer_id).await;
                }
            }
        }
    }

    async fn assemble_file_from_chunks(
        file_hash: &str,
        chunks: &HashMap<u32, FileChunk>,
        file_transfer_service: &Arc<FileTransferService>,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        peer_id: &str,
    ) {
        // Sort chunks by index
        let mut sorted_chunks: Vec<_> = chunks.values().collect();
        sorted_chunks.sort_by_key(|c| c.chunk_index);

        // Concatenate chunk data
        let mut file_data = Vec::new();
        for chunk in sorted_chunks {
            file_data.extend_from_slice(&chunk.data);
        }

        // Store the assembled file
        let file_name = format!("downloaded_{}", file_hash);
        file_transfer_service.store_file_data(file_hash.to_string(), file_name, file_data);

        let _ = event_tx.send(WebRTCEvent::TransferCompleted {
            peer_id: peer_id.to_string(),
            file_hash: file_hash.to_string(),
        }).await;
    }

    fn calculate_chunk_checksum(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    pub async fn create_offer(&self, peer_id: String) -> Result<String, String> {
        info!("Creating WebRTC offer for peer: {}", peer_id);

        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection
        let config = RTCConfiguration::default();
        let peer_connection = match api.new_peer_connection(config).await {
            Ok(pc) => Arc::new(pc),
            Err(e) => {
                error!("Failed to create peer connection: {}", e);
                return Err(e.to_string());
            }
        };

        // Create data channel
        let data_channel = match peer_connection.create_data_channel("file-transfer", None).await {
            Ok(dc) => dc,
            Err(e) => {
                error!("Failed to create data channel: {}", e);
                return Err(e.to_string());
            }
        };

        // Set up data channel event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();
        let file_transfer_service_clone = Arc::new(self.file_transfer_service.clone());
        let connections_clone = Arc::new(self.connections.clone());

        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();

            Box::pin(async move {
                Self::handle_data_channel_message(&peer_id, &msg, &event_tx, &file_transfer_service, &connections).await;
            })
        }));

        // Set up peer connection event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();
        let data_channel_clone = data_channel.clone();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(candidate_str) = serde_json::to_string(&candidate.to_json().unwrap_or_default()) {
                        let _ = event_tx.send(WebRTCEvent::IceCandidate {
                            peer_id,
                            candidate: candidate_str,
                        }).await;
                    }
                }
            })
        }));

        peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let _data_channel = data_channel_clone.clone();

            Box::pin(async move {
                match state {
                    RTCPeerConnectionState::Connected => {
                        info!("WebRTC connection established with peer: {}", peer_id);
                        let _ = event_tx.send(WebRTCEvent::ConnectionEstablished {
                            peer_id,
                        }).await;
                    }
                    RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                        info!("WebRTC connection closed with peer: {}", peer_id);
                    }
                    _ => {}
                }
            })
        }));

        // Create offer
        let offer = match peer_connection.create_offer(None).await {
            Ok(offer) => offer,
            Err(e) => {
                error!("Failed to create offer: {}", e);
                return Err(e.to_string());
            }
        };

        // Set local description
        if let Err(e) = peer_connection.set_local_description(offer).await {
            error!("Failed to set local description: {}", e);
            return Err(e.to_string());
        }

        // Store connection
        let mut conns = self.connections.lock().await;
        let connection = PeerConnection {
            peer_id: peer_id.clone(),
            is_connected: false,
            active_transfers: HashMap::new(),
            last_activity: Instant::now(),
            peer_connection: Some(peer_connection.clone()),
            data_channel: Some(data_channel),
            pending_chunks: HashMap::new(),
            received_chunks: HashMap::new(),
        };
        conns.insert(peer_id, connection);

        // Return offer SDP
        if let Some(local_desc) = peer_connection.local_description().await {
            match serde_json::to_string(&local_desc) {
                Ok(offer_str) => Ok(offer_str),
                Err(e) => Err(format!("Failed to serialize offer: {}", e)),
            }
        } else {
            Err("No local description available".to_string())
        }
    }

    pub async fn establish_connection_with_answer(&self, peer_id: String, answer: String) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::HandleAnswer { peer_id, answer })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn establish_connection_with_offer(&self, peer_id: String, offer: String) -> Result<String, String> {
        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection
        let config = RTCConfiguration::default();
        let peer_connection = match api.new_peer_connection(config).await {
            Ok(pc) => Arc::new(pc),
            Err(e) => {
                error!("Failed to create peer connection: {}", e);
                return Err(e.to_string());
            }
        };

        // Create data channel
        let data_channel = match peer_connection.create_data_channel("file-transfer", None).await {
            Ok(dc) => dc,
            Err(e) => {
                error!("Failed to create data channel: {}", e);
                return Err(e.to_string());
            }
        };

        // Set up data channel event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();
        let file_transfer_service_clone = Arc::new(self.file_transfer_service.clone());
        let connections_clone = Arc::new(self.connections.clone());

        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();

            Box::pin(async move {
                Self::handle_data_channel_message(&peer_id, &msg, &event_tx, &file_transfer_service, &connections).await;
            })
        }));

        // Set up peer connection event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();
        let data_channel_clone = data_channel.clone();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(candidate_str) = serde_json::to_string(&candidate.to_json().unwrap_or_default()) {
                        let _ = event_tx.send(WebRTCEvent::IceCandidate {
                            peer_id,
                            candidate: candidate_str,
                        }).await;
                    }
                }
            })
        }));

        peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let _data_channel = data_channel_clone.clone();

            Box::pin(async move {
                match state {
                    RTCPeerConnectionState::Connected => {
                        info!("WebRTC connection established with peer: {}", peer_id);
                        let _ = event_tx.send(WebRTCEvent::ConnectionEstablished {
                            peer_id,
                        }).await;
                    }
                    RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                        info!("WebRTC connection closed with peer: {}", peer_id);
                    }
                    _ => {}
                }
            })
        }));

        // Set remote description from offer
        let offer_desc = match serde_json::from_str::<RTCSessionDescription>(offer.as_str()) {
            Ok(offer) => offer,
            Err(e) => {
                error!("Failed to parse offer SDP: {}", e);
                return Err(format!("Invalid offer SDP: {}", e));
            }
        };

        if let Err(e) = peer_connection.set_remote_description(offer_desc).await {
            error!("Failed to set remote description: {}", e);
            return Err(e.to_string());
        }

        // Create answer
        let answer = match peer_connection.create_answer(None).await {
            Ok(answer) => answer,
            Err(e) => {
                error!("Failed to create answer: {}", e);
                return Err(e.to_string());
            }
        };

        // Set local description
        if let Err(e) = peer_connection.set_local_description(answer).await {
            error!("Failed to set local description: {}", e);
            return Err(e.to_string());
        }

        // Store connection
        let mut conns = self.connections.lock().await;
        let connection = PeerConnection {
            peer_id: peer_id.clone(),
            is_connected: false, // Will be set to true when connected
            active_transfers: HashMap::new(),
            last_activity: Instant::now(),
            peer_connection: Some(peer_connection.clone()),
            data_channel: Some(data_channel),
            pending_chunks: HashMap::new(),
            received_chunks: HashMap::new(),
        };
        conns.insert(peer_id, connection);

        // Return answer SDP
        if let Some(local_desc) = peer_connection.local_description().await {
            match serde_json::to_string(&local_desc) {
                Ok(answer_str) => Ok(answer_str),
                Err(e) => Err(format!("Failed to serialize answer: {}", e)),
            }
        } else {
            Err("No local description available".to_string())
        }
    }

    pub async fn send_file_request(&self, peer_id: String, request: WebRTCFileRequest) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::SendFileRequest { peer_id, request })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn send_file_chunk(&self, peer_id: String, chunk: FileChunk) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::SendFileChunk { peer_id, chunk })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn add_ice_candidate(&self, peer_id: String, candidate: String) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::AddIceCandidate { peer_id, candidate })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn request_file_chunk(&self, peer_id: String, file_hash: String, chunk_index: u32) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::RequestFileChunk { peer_id, file_hash, chunk_index })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn close_connection(&self, peer_id: String) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::CloseConnection { peer_id })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn drain_events(&self, max: usize) -> Vec<WebRTCEvent> {
        let mut events = Vec::new();
        let mut event_rx = self.event_rx.lock().await;

        for _ in 0..max {
            match event_rx.try_recv() {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }

        events
    }

    pub async fn get_connection_status(&self, peer_id: &str) -> bool {
        let connections = self.connections.lock().await;
        connections.get(peer_id).map(|c| c.is_connected).unwrap_or(false)
    }
}

// Singleton instance
use lazy_static::lazy_static;

lazy_static! {
    static ref WEBRTC_SERVICE: Mutex<Option<Arc<WebRTCService>>> = Mutex::new(None);
}

pub async fn init_webrtc_service(file_transfer_service: Arc<FileTransferService>) -> Result<(), String> {
    let mut service = WEBRTC_SERVICE.lock().await;
    if service.is_none() {
        let webrtc_service = WebRTCService::new(file_transfer_service).await?;
        *service = Some(Arc::new(webrtc_service));
    }
    Ok(())
}

pub async fn get_webrtc_service() -> Option<Arc<WebRTCService>> {
    WEBRTC_SERVICE.lock().await.clone()
}

impl FileTransferService {
    pub async fn initiate_p2p_download(
        &self,
        file_hash: String,
        peer_id: String,
        output_path: String,
    ) -> Result<(), String> {
        info!("Initiating P2P download: {} from peer {}", file_hash, peer_id);

        // Send file request over WebRTC
        if let Some(webrtc_service) = get_webrtc_service().await {
            let request = WebRTCFileRequest {
                file_hash: file_hash.clone(),
                file_name: "downloaded_file".to_string(), // Will be updated when we get metadata
                file_size: 0, // Will be updated
                requester_peer_id: "local_peer".to_string(), // Should be actual local peer ID
            };

            webrtc_service.send_file_request(peer_id, request).await?;
        } else {
            return Err("WebRTC service not available".to_string());
        }

        Ok(())
    }
}
