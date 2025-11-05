use crate::encryption::{decrypt_aes_key, encrypt_aes_key, EncryptedAesKeyBundle, FileEncryption};
use crate::file_transfer::FileTransferService;
use crate::keystore::Keystore;
use crate::bandwidth::BandwidthController;
use crate::manager::{ChunkInfo, FileManifest};
use crate::stream_auth::{AuthMessage, StreamAuthService};
use aes_gcm::aead::Aead;
use aes_gcm::{AeadCore, KeyInit};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::bytes::Bytes;
use tauri::Emitter;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

const CHUNK_SIZE: usize = 16384; // 16KB chunks

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCFileRequest {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub requester_peer_id: String,
    pub recipient_public_key: Option<String>, // For encrypted transfers
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCChatMessage {
    pub message_id: String,
    pub encrypted_payload: Vec<u8>, // The E2EE message from your crypto layer
    pub timestamp: u64,
    pub signature: Vec<u8>, // Signature of the payload to verify authenticity
}

/// Sent by a downloader to request the full file manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCManifestRequest {
    pub file_hash: String, // The Merkle Root
}

/// Sent by a seeder in response to a manifest request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCManifestResponse {
    pub file_hash: String,     // The Merkle Root, to match the request
    pub manifest_json: String, // The full FileManifest, serialized to JSON
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    pub file_hash: String,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub data: Vec<u8>,
    pub checksum: String,
    pub encrypted_key_bundle: Option<EncryptedAesKeyBundle>, // For encrypted transfers
    pub auth_message: Option<AuthMessage>,                   // Stream authentication
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

/// A new enum to wrap different message types for clarity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebRTCMessage {
    FileRequest(WebRTCFileRequest),
    ManifestRequest(WebRTCManifestRequest),
    ManifestResponse(WebRTCManifestResponse),
    FileChunk(FileChunk),
    ChatMessage(WebRTCChatMessage),
}

pub struct WebRTCService {
    cmd_tx: mpsc::Sender<WebRTCCommand>,
    event_tx: mpsc::Sender<WebRTCEvent>,
    event_rx: Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>,
    connections: Arc<Mutex<HashMap<String, PeerConnection>>>,
    file_transfer_service: Arc<FileTransferService>,
    app_handle: tauri::AppHandle,
    keystore: Arc<Mutex<Keystore>>,
    active_private_key: Arc<Mutex<Option<String>>>,
    stream_auth: Arc<Mutex<StreamAuthService>>, // Stream authentication
    bandwidth: Arc<BandwidthController>,
}

impl WebRTCService {
    pub async fn new(
        app_handle: tauri::AppHandle,
        file_transfer_service: Arc<FileTransferService>,
        keystore: Arc<Mutex<Keystore>>,
        bandwidth: Arc<BandwidthController>,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        let connections = Arc::new(Mutex::new(HashMap::new()));
        let active_private_key = Arc::new(Mutex::new(None));

        // Spawn the WebRTC service task
        let stream_auth = Arc::new(Mutex::new(StreamAuthService::new()));
        tokio::spawn(Self::run_webrtc_service(
            app_handle.clone(),
            cmd_rx,
            event_tx.clone(),
            connections.clone(),
            file_transfer_service.clone(),
            keystore.clone(),
            active_private_key.clone(),
            stream_auth.clone(),
            bandwidth.clone(),
        ));

        Ok(WebRTCService {
            cmd_tx,
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            connections,
            app_handle,
            file_transfer_service,
            keystore,
            active_private_key,
            stream_auth,
            bandwidth,
        })
    }

    /// Set the active private key for decryption operations
    pub async fn set_active_private_key(&self, private_key: Option<String>) {
        let mut key_guard = self.active_private_key.lock().await;
        *key_guard = private_key;
    }

    async fn run_webrtc_service(
        app_handle: tauri::AppHandle,
        mut cmd_rx: mpsc::Receiver<WebRTCCommand>,
        event_tx: mpsc::Sender<WebRTCEvent>,
        connections: Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: Arc<FileTransferService>,
        keystore: Arc<Mutex<Keystore>>,
        active_private_key: Arc<Mutex<Option<String>>>,
        stream_auth: Arc<Mutex<StreamAuthService>>,
        bandwidth: Arc<BandwidthController>,
    ) {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                WebRTCCommand::EstablishConnection { peer_id, offer } => {
                    Self::handle_establish_connection(
                        &app_handle,
                        &peer_id,
                        &offer,
                        &event_tx,
                        &connections,
                        &file_transfer_service,
                        &keystore,
                        &active_private_key,
                        &stream_auth,
                        &bandwidth,
                    )
                    .await;
                }
                WebRTCCommand::HandleAnswer { peer_id, answer } => {
                    Self::handle_answer(&peer_id, &answer, &connections).await;
                }
                WebRTCCommand::AddIceCandidate { peer_id, candidate } => {
                    Self::handle_ice_candidate(&peer_id, &candidate, &connections).await;
                }
                WebRTCCommand::SendFileRequest { peer_id, request } => {
                    Self::handle_file_request(
                        &peer_id,
                        &request,
                        &event_tx,
                        &file_transfer_service,
                        &connections,
                        &keystore,
                        &stream_auth,
                        &bandwidth,
                    )
                    .await;
                }
                WebRTCCommand::SendFileChunk { peer_id, chunk } => {
                    Self::handle_send_chunk(&peer_id, &chunk, &connections, &bandwidth).await;
                }
                WebRTCCommand::RequestFileChunk {
                    peer_id,
                    file_hash,
                    chunk_index,
                } => {
                    Self::handle_request_chunk(
                        &peer_id,
                        &file_hash,
                        chunk_index,
                        &event_tx,
                        &connections,
                    )
                    .await;
                }
                WebRTCCommand::CloseConnection { peer_id } => {
                    Self::handle_close_connection(&peer_id, &connections).await;
                }
            }
        }
    }

    async fn handle_establish_connection(
        app_handle: &tauri::AppHandle,
        peer_id: &str,
        offer_sdp: &str,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: &Arc<FileTransferService>,
        keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        stream_auth: &Arc<Mutex<StreamAuthService>>,
        bandwidth: &Arc<BandwidthController>,
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
                let _ = event_tx
                    .send(WebRTCEvent::ConnectionFailed {
                        peer_id: peer_id.to_string(),
                        error: e.to_string(),
                    })
                    .await;
                return;
            }
        };

        // Create data channel
        let data_channel = match peer_connection
            .create_data_channel("file-transfer", None)
            .await
        {
            Ok(dc) => dc,
            Err(e) => {
                error!("Failed to create data channel: {}", e);
                let _ = event_tx
                    .send(WebRTCEvent::ConnectionFailed {
                        peer_id: peer_id.to_string(),
                        error: e.to_string(),
                    })
                    .await;
                return;
            }
        };

        // Set up data channel event handlers
        let event_tx_clone = event_tx.clone();
        let peer_id_clone = peer_id.to_string();
        let file_transfer_service_clone = file_transfer_service.clone();
        let connections_clone = connections.clone();
        let keystore_clone = keystore.clone();
        let active_private_key_clone = Arc::new(active_private_key.clone());
        let stream_auth_clone = stream_auth.clone();
        let bandwidth_clone = bandwidth.clone();

        let app_handle_clone = app_handle.clone();
        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();
            let keystore = keystore_clone.clone();
            let active_private_key = active_private_key_clone.clone();
            let stream_auth = stream_auth_clone.clone();
            let bandwidth = bandwidth_clone.clone();

            let app_handle_for_task = app_handle_clone.clone();
            Box::pin(async move {
                Self::handle_data_channel_message(
                    &peer_id,
                    &msg,
                    &event_tx,
                    &file_transfer_service,
                    &connections,
                    &keystore,
                    &active_private_key,
                    &stream_auth,
                    app_handle_for_task,
                    bandwidth,
                )
                .await;
            })
        }));

        // Set up peer connection event handlers
        let event_tx_clone = event_tx.clone();
        let peer_id_clone = peer_id.to_string();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(candidate_str) =
                        serde_json::to_string(&candidate.to_json().unwrap_or_default())
                    {
                        let _ = event_tx
                            .send(WebRTCEvent::IceCandidate {
                                peer_id,
                                candidate: candidate_str,
                            })
                            .await;
                    }
                }
            })
        }));

        peer_connection.on_peer_connection_state_change(Box::new(
            move |state: RTCPeerConnectionState| {
                let event_tx = event_tx_clone.clone();
                let peer_id = peer_id_clone.clone();

                Box::pin(async move {
                    match state {
                        RTCPeerConnectionState::Connected => {
                            info!("WebRTC connection established with peer: {}", peer_id);
                            let _ = event_tx
                                .send(WebRTCEvent::ConnectionEstablished { peer_id })
                                .await;
                        }
                        RTCPeerConnectionState::Disconnected
                        | RTCPeerConnectionState::Failed
                        | RTCPeerConnectionState::Closed => {
                            info!("WebRTC connection closed with peer: {}", peer_id);
                        }
                        _ => {}
                    }
                })
            },
        ));

        // Set remote description from offer
        let offer = match serde_json::from_str::<RTCSessionDescription>(offer_sdp) {
            Ok(offer) => offer,
            Err(e) => {
                error!("Failed to parse offer SDP: {}", e);
                let _ = event_tx
                    .send(WebRTCEvent::ConnectionFailed {
                        peer_id: peer_id.to_string(),
                        error: format!("Invalid offer SDP: {}", e),
                    })
                    .await;
                return;
            }
        };

        if let Err(e) = peer_connection.set_remote_description(offer).await {
            error!("Failed to set remote description: {}", e);
            let _ = event_tx
                .send(WebRTCEvent::ConnectionFailed {
                    peer_id: peer_id.to_string(),
                    error: e.to_string(),
                })
                .await;
            return;
        }

        // Create answer
        let answer = match peer_connection.create_answer(None).await {
            Ok(answer) => answer,
            Err(e) => {
                error!("Failed to create answer: {}", e);
                let _ = event_tx
                    .send(WebRTCEvent::ConnectionFailed {
                        peer_id: peer_id.to_string(),
                        error: e.to_string(),
                    })
                    .await;
                return;
            }
        };

        // Set local description
        if let Err(e) = peer_connection.set_local_description(answer).await {
            error!("Failed to set local description: {}", e);
            let _ = event_tx
                .send(WebRTCEvent::ConnectionFailed {
                    peer_id: peer_id.to_string(),
                    error: e.to_string(),
                })
                .await;
            return;
        }

        // Send answer
        if let Some(local_desc) = peer_connection.local_description().await {
            if let Ok(answer_str) = serde_json::to_string(&local_desc) {
                let _ = event_tx
                    .send(WebRTCEvent::AnswerReceived {
                        peer_id: peer_id.to_string(),
                        answer: answer_str,
                    })
                    .await;
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

    async fn handle_answer(
        peer_id: &str,
        answer_sdp: &str,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
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

    async fn handle_ice_candidate(
        peer_id: &str,
        candidate_str: &str,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            if let Some(pc) = &connection.peer_connection {
                let candidate_init =
                    match serde_json::from_str::<RTCIceCandidateInit>(candidate_str) {
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
        keystore: &Arc<Mutex<Keystore>>,
        stream_auth: &Arc<Mutex<StreamAuthService>>,
        bandwidth: &Arc<BandwidthController>,
    ) {
        info!(
            "Handling file request from peer {}: {}",
            peer_id, request.file_hash
        );

        // Check if we have the file locally
        let stored_files = file_transfer_service
            .get_stored_files()
            .await
            .unwrap_or_default();
        let has_file = stored_files
            .iter()
            .any(|(hash, _)| hash == &request.file_hash);

        if has_file {
            // Start sending file chunks
            if let Err(e) = Self::start_file_transfer(
                peer_id,
                request,
                event_tx,
                file_transfer_service,
                connections,
                keystore,
                stream_auth,
                bandwidth,
            )
            .await
            {
                let _ = event_tx
                    .send(WebRTCEvent::TransferFailed {
                        peer_id: peer_id.to_string(),
                        file_hash: request.file_hash.clone(),
                        error: format!("Failed to start file transfer: {}", e),
                    })
                    .await;
            }
        } else {
            let _ = event_tx
                .send(WebRTCEvent::TransferFailed {
                    peer_id: peer_id.to_string(),
                    file_hash: request.file_hash.clone(),
                    error: "File not found locally".to_string(),
                })
                .await;
        }
    }

    async fn handle_send_chunk(
        peer_id: &str,
        chunk: &FileChunk,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        bandwidth: &Arc<BandwidthController>,
    ) {
        bandwidth.acquire_upload(chunk.data.len()).await;

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
        let _ = event_tx
            .send(WebRTCEvent::FileChunkRequested {
                peer_id: peer_id.to_string(),
                file_hash: file_hash.to_string(),
                chunk_index,
            })
            .await;
    }

    async fn handle_close_connection(
        peer_id: &str,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
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
        keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        stream_auth: &Arc<Mutex<StreamAuthService>>,
        app_handle: tauri::AppHandle,
        bandwidth: Arc<BandwidthController>,
    ) {
        if let Ok(text) = std::str::from_utf8(&msg.data) {
            // Try to parse as FileChunk
            if let Ok(chunk) = serde_json::from_str::<FileChunk>(text) {
                // Handle received chunk
                Self::process_incoming_chunk(
                    &chunk,
                    file_transfer_service,
                    connections,
                    event_tx,
                    peer_id,
                    keystore,
                    &active_private_key,
                    stream_auth,
                    &app_handle,
                    &bandwidth,
                )
                .await;
                let _ = event_tx
                    .send(WebRTCEvent::FileChunkReceived {
                        peer_id: peer_id.to_string(),
                        chunk,
                    })
                    .await;
            }
            // Try to parse as WebRTCFileRequest
            else if let Ok(request) = serde_json::from_str::<WebRTCFileRequest>(text) {
                let _ = event_tx
                    .send(WebRTCEvent::FileRequestReceived {
                        peer_id: peer_id.to_string(),
                        request: request.clone(),
                    })
                    .await;
                // Actually handle the file request to start transfer
                Self::handle_file_request(
                    peer_id,
                    &request,
                    event_tx,
                    file_transfer_service,
                    connections,
                    keystore,
                    stream_auth,
                    &bandwidth,
                )
                .await;
            }
            // Try to parse as a generic WebRTCMessage
            else if let Ok(message) = serde_json::from_str::<WebRTCMessage>(text) {
                match message {
                    WebRTCMessage::FileRequest(request) => {
                        let _ = event_tx
                            .send(WebRTCEvent::FileRequestReceived {
                                peer_id: peer_id.to_string(),
                                request: request.clone(),
                            })
                            .await;
                        Self::handle_file_request(
                            peer_id,
                            &request,
                            event_tx,
                            file_transfer_service,
                            connections,
                            keystore,
                            stream_auth,
                            &bandwidth,
                        )
                        .await;
                    }
                    WebRTCMessage::ManifestRequest(request) => {
                        info!("Received manifest request for file: {}", request.file_hash);

                        // Check if we have the file
                        let stored_files = file_transfer_service
                            .get_stored_files()
                            .await
                            .unwrap_or_default();
                        let has_file = stored_files
                            .iter()
                            .any(|(hash, _)| hash == &request.file_hash);

                        if has_file {
                            // Get file data
                            if let Some(file_data) = file_transfer_service
                                .get_file_data(&request.file_hash)
                                .await
                            {
                                // Get metadata
                                let storage_dir = file_transfer_service.get_storage_path();
                                let metadata_path =
                                    storage_dir.join(format!("{}.meta", request.file_hash));
                                let is_encrypted =
                                    if tokio::fs::metadata(&metadata_path).await.is_ok() {
                                        let metadata_content =
                                            tokio::fs::read_to_string(&metadata_path)
                                                .await
                                                .unwrap_or_default();
                                        let metadata: serde_json::Value =
                                            serde_json::from_str(&metadata_content)
                                                .unwrap_or_default();
                                        metadata
                                            .get("is_encrypted")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false)
                                    } else {
                                        false
                                    };

                                let encrypted_key_bundle = if is_encrypted {
                                    let encmeta_path =
                                        storage_dir.join(format!("{}.encmeta", request.file_hash));
                                    if tokio::fs::metadata(&encmeta_path).await.is_ok() {
                                        let encmeta_content =
                                            tokio::fs::read_to_string(&encmeta_path)
                                                .await
                                                .unwrap_or_default();
                                        let encmeta: serde_json::Value =
                                            serde_json::from_str(&encmeta_content)
                                                .unwrap_or_default();
                                        encmeta
                                            .get("encrypted_key_bundle")
                                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };

                                // Calculate chunks
                                let mut chunks = Vec::new();
                                let total_chunks =
                                    ((file_data.len() as f64) / CHUNK_SIZE as f64).ceil() as u32;
                                for chunk_index in 0..total_chunks {
                                    let start = (chunk_index as usize) * CHUNK_SIZE;
                                    let end = (start + CHUNK_SIZE).min(file_data.len());
                                    let chunk_data = &file_data[start..end];
                                    let chunk_hash = Self::calculate_chunk_checksum(chunk_data);
                                    chunks.push(ChunkInfo {
                                        index: chunk_index,
                                        hash: chunk_hash.clone(),
                                        size: (end - start),
                                        encrypted_hash: chunk_hash,
                                        encrypted_size: (end - start),
                                    });
                                }
                                let manifest = FileManifest {
                                    merkle_root: request.file_hash.clone(),
                                    chunks,
                                    encrypted_key_bundle,
                                };

                                let manifest_json = serde_json::to_string(&manifest).unwrap();

                                let response = WebRTCManifestResponse {
                                    file_hash: request.file_hash,
                                    manifest_json,
                                };

                                // Send the response
                                let message = WebRTCMessage::ManifestResponse(response);
                                let message_json = serde_json::to_string(&message).unwrap();

                                // Send over data channel
                                let mut conns = connections.lock().await;
                                if let Some(connection) = conns.get_mut(peer_id) {
                                    if let Some(dc) = &connection.data_channel {
                                        if let Err(e) = dc.send_text(message_json).await {
                                            error!("Failed to send manifest response: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    WebRTCMessage::ManifestResponse(response) => {
                        info!("Received manifest response for a file download.");
                        // Downloader receives this. We can emit a specific event or handle it directly.
                        // For simplicity, we can have the main download logic listen for this.
                    }
                    WebRTCMessage::FileChunk(chunk) => {
                        Self::process_incoming_chunk(
                            &chunk,
                            file_transfer_service,
                            connections,
                            event_tx,
                            peer_id,
                            keystore,
                            &active_private_key,
                            stream_auth,
                            &app_handle,
                            &bandwidth,
                        )
                        .await;
                    }
                    WebRTCMessage::ChatMessage(chat_message) => {
                        info!("Received chat message {} from peer {}", chat_message.message_id, peer_id);
                        // Just forward the entire message to the frontend.
                        // The frontend will be responsible for decryption.
                        if let Err(e) = app_handle.emit("incoming_chat_message", chat_message) {
                            error!("Failed to emit incoming_chat_message event: {}", e);
                        }
                    }
                }
            }
        }
    }

    async fn start_file_transfer(
        peer_id: &str,
        request: &WebRTCFileRequest,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        keystore: &Arc<Mutex<Keystore>>,
        stream_auth: &Arc<Mutex<StreamAuthService>>,
        bandwidth: &Arc<BandwidthController>,
    ) -> Result<(), String> {
        // Get file data from local storage
        let file_data = match file_transfer_service
            .get_file_data(&request.file_hash)
            .await
        {
            Some(data) => data,
            None => {
                let _ = event_tx
                    .send(WebRTCEvent::TransferFailed {
                        peer_id: peer_id.to_string(),
                        file_hash: request.file_hash.clone(),
                        error: "File data not available".to_string(),
                    })
                    .await;
                return Ok(());
            }
        };

        info!(
            "Starting real file transfer of {} ({} bytes) to peer {}",
            request.file_name,
            file_data.len(),
            peer_id
        );

        // Calculate total chunks
        let total_chunks = ((file_data.len() as f64) / CHUNK_SIZE as f64).ceil() as u32;

        // For unencrypted transfers, establish HMAC session
        if request.recipient_public_key.is_none() {
            let session_id = format!("{}-{}", peer_id, request.file_hash);
            let mut auth_service = stream_auth.lock().await;

            // Generate HMAC key for this session
            let hmac_key = StreamAuthService::generate_hmac_key();

            // Create session with HMAC key (will be replaced by key exchange)
            match auth_service.create_session(session_id.clone(), hmac_key) {
                Ok(_) => {
                    info!(
                        "Created HMAC session for unencrypted transfer: {}",
                        session_id
                    );
                }
                Err(e) => {
                    warn!("Failed to create HMAC session: {}", e);
                    // Continue without authentication as fallback
                }
            }
        }

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
                connection
                    .active_transfers
                    .insert(request.file_hash.clone(), transfer);
            }
        }

        // Send file chunks over WebRTC data channel
        for chunk_index in 0..total_chunks {
            let start = (chunk_index as usize) * CHUNK_SIZE;
            let end = (start + CHUNK_SIZE).min(file_data.len());
            let chunk_data: Vec<u8> = file_data[start..end].to_vec();

            let (final_chunk_data, encrypted_key_bundle, auth_message) =
                if let Some(ref recipient_key) = request.recipient_public_key {
                    // Encrypted transfer - no HMAC authentication needed (AES-256-GCM provides AEAD)
                    match Self::encrypt_chunk_for_peer(&chunk_data, recipient_key, keystore).await {
                        Ok((encrypted_data, key_bundle)) => {
                            (encrypted_data, Some(key_bundle), None)
                        }
                        Err(e) => {
                            let _ = event_tx
                                .send(WebRTCEvent::TransferFailed {
                                    peer_id: peer_id.to_string(),
                                    file_hash: request.file_hash.clone(),
                                    error: format!("Encryption failed: {}", e),
                                })
                                .await;
                            return Err(format!("Encryption failed: {}", e));
                        }
                    }
                } else {
                    // Unencrypted transfer - use HMAC authentication
                    let session_id = format!("{}-{}", peer_id, request.file_hash);
                    let mut auth_service = stream_auth.lock().await;

                    // Create authenticated chunk
                    match auth_service.create_authenticated_chunk(
                        &session_id,
                        &chunk_data,
                        chunk_index,
                        &request.file_hash,
                    ) {
                        Ok(auth_msg) => (chunk_data, None, Some(auth_msg)),
                        Err(e) => {
                            warn!("Failed to create authenticated chunk: {}", e);
                            // Fallback to unauthenticated chunk
                            (chunk_data, None, None)
                        }
                    }
                };

            // Calculate checksum for the final data (encrypted or not)
            let checksum = Self::calculate_chunk_checksum(&final_chunk_data);

            let chunk = FileChunk {
                file_hash: request.file_hash.clone(),
                chunk_index,
                total_chunks,
                data: final_chunk_data,
                checksum,
                encrypted_key_bundle,
                auth_message, // HMAC authentication for unencrypted transfers only
            };

            // Send chunk via WebRTC data channel
            Self::handle_send_chunk(peer_id, &chunk, connections, bandwidth).await;

            // Update progress
            {
                let mut conns = connections.lock().await;
                if let Some(connection) = conns.get_mut(peer_id) {
                    if let Some(transfer) = connection.active_transfers.get_mut(&request.file_hash)
                    {
                        transfer.chunks_sent += 1;
                        transfer.bytes_sent += chunk.data.len() as u64;

                        // Send progress update
                        let progress = TransferProgress {
                            file_hash: request.file_hash.clone(),
                            bytes_transferred: transfer.bytes_sent,
                            total_bytes: transfer.file_size,
                            chunks_transferred: transfer.chunks_sent,
                            total_chunks: transfer.total_chunks,
                            percentage: (transfer.chunks_sent as f32
                                / transfer.total_chunks as f32)
                                * 100.0,
                        };

                        let _ = event_tx
                            .send(WebRTCEvent::TransferProgress {
                                peer_id: peer_id.to_string(),
                                progress,
                            })
                            .await;
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

        let _ = event_tx
            .send(WebRTCEvent::TransferCompleted {
                peer_id: peer_id.to_string(),
                file_hash: request.file_hash.clone(),
            })
            .await;
        Ok(())
    }

    async fn process_incoming_chunk(
        chunk: &FileChunk,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        peer_id: &str,
        _keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        stream_auth: &Arc<Mutex<StreamAuthService>>,
        app_handle: &tauri::AppHandle,
        bandwidth: &Arc<BandwidthController>,
    ) {
        // 1. Verify stream authentication first
        if let Some(ref auth_msg) = chunk.auth_message {
            let mut auth_service = stream_auth.lock().await;
            let session_id = format!("{}-{}", peer_id, chunk.file_hash);

            if !auth_service
                .verify_data(&session_id, auth_msg)
                .unwrap_or(false)
            {
                warn!(
                    "Stream authentication failed for chunk from peer {}",
                    peer_id
                );
                return;
            }
        }

        // 2. Decrypt chunk data if it was encrypted
        let final_chunk_data = if let Some(ref encrypted_key_bundle) = chunk.encrypted_key_bundle {
            // Get the active private key for decryption
            let private_key_opt = {
                let key_guard = active_private_key.lock().await;
                key_guard.clone()
            };

            if let Some(private_key) = private_key_opt {
                match Self::decrypt_chunk_from_peer(&chunk.data, encrypted_key_bundle, &private_key)
                    .await
                {
                    Ok(decrypted_data) => decrypted_data,
                    Err(e) => {
                        warn!("Failed to decrypt chunk from peer {}: {}", peer_id, e);
                        chunk.data.clone() // Return encrypted data as fallback
                    }
                }
            } else {
                warn!(
                    "Encrypted chunk received but no active private key available for peer: {}",
                    peer_id
                );
                chunk.data.clone() // Return encrypted data as fallback
            }
        } else {
            chunk.data.clone()
        };

        // Verify chunk checksum
        let chunk_len = final_chunk_data.len();
        let calculated_checksum = Self::calculate_chunk_checksum(&final_chunk_data);
        if calculated_checksum != chunk.checksum {
            warn!("Chunk checksum mismatch for file {}", chunk.file_hash);
            return;
        }

        bandwidth.acquire_download(chunk_len).await;

        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            // Store chunk
            let chunks = connection
                .received_chunks
                .entry(chunk.file_hash.clone())
                .or_insert_with(HashMap::new);
            chunks.insert(chunk.chunk_index, chunk.clone());

            // Check if we have all chunks for this file
            if let Some(total_chunks) = chunks.values().next().map(|c| c.total_chunks) {
                if chunks.len() == total_chunks as usize {
                    // Assemble file
                    Self::assemble_file_from_chunks(
                        &chunk.file_hash,
                        chunks,
                        file_transfer_service,
                        event_tx,
                        peer_id,
                        &app_handle,
                    )
                    .await;
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
    app_handle: &tauri::AppHandle, // Add this parameter
    ) {
    // Sort chunks by index
    let mut sorted_chunks: Vec<_> = chunks.values().collect();
    sorted_chunks.sort_by_key(|c| c.chunk_index);

    // Get file name from the first chunk
    let file_name = sorted_chunks
        .first()
        .map(|c| c.file_hash.clone())
        .unwrap_or_else(|| format!("downloaded_{}", file_hash));

    // Concatenate chunk data
    let mut file_data = Vec::new();
    for chunk in sorted_chunks {
        file_data.extend_from_slice(&chunk.data);
    }

    let file_size = file_data.len();

    // Store the assembled file internally
    file_transfer_service
        .store_file_data(file_hash.to_string(), file_name.clone(), file_data.clone())
        .await;

    // NEW: Emit event to frontend with complete file data
    if let Err(e) = app_handle.emit("webrtc_download_complete", serde_json::json!({
        "fileHash": file_hash,
        "fileName": file_name,
        "fileSize": file_size,
        "data": file_data, // Send the actual file data
    })) {
        error!("Failed to emit webrtc_download_complete event: {}", e);
    }

    let _ = event_tx
        .send(WebRTCEvent::TransferCompleted {
            peer_id: peer_id.to_string(),
            file_hash: file_hash.to_string(),
        })
        .await;
}

    fn calculate_chunk_checksum(data: &[u8]) -> String {
        let mut hasher = Sha256::default();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    pub async fn create_offer(&self, peer_id: String) -> Result<String, String> {
        info!("Creating WebRTC offer for peer: {}", peer_id);

        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection
        let config = RTCConfiguration::default();
        let peer_connection: Arc<RTCPeerConnection> = match api.new_peer_connection(config).await {
            Ok(pc) => Arc::new(pc),
            Err(e) => {
                error!("Failed to create peer connection: {}", e);
                return Err(e.to_string());
            }
        };

        // Create data channel
        let data_channel = match peer_connection
            .create_data_channel("file-transfer", None)
            .await
        {
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
        let keystore_clone = Arc::new(self.keystore.clone());
        let active_private_key_clone = Arc::new(self.active_private_key.clone());
        let stream_auth_clone = Arc::new(self.stream_auth.clone());
        let bandwidth_clone = self.bandwidth.clone();

        let app_handle_clone = self.app_handle.clone();
        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();
            let keystore = keystore_clone.clone();
            let active_private_key = active_private_key_clone.clone();
            let stream_auth = stream_auth_clone.clone();
            let bandwidth = bandwidth_clone.clone();

            let app_handle_for_task = app_handle_clone.clone();
            Box::pin(async move {
                Self::handle_data_channel_message(
                    &peer_id,
                    &msg,
                    &event_tx,
                    &file_transfer_service,
                    &connections,
                    &keystore,
                    &active_private_key,
                    &stream_auth,
                    app_handle_for_task,
                    bandwidth,
                )
                .await;
            })
        }));

        // Set up peer connection event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(candidate_str) =
                        serde_json::to_string(&candidate.to_json().unwrap_or_default())
                    {
                        let _ = event_tx
                            .send(WebRTCEvent::IceCandidate {
                                peer_id,
                                candidate: candidate_str,
                            })
                            .await;
                    }
                }
            })
        }));

        peer_connection.on_peer_connection_state_change(Box::new(
            move |state: RTCPeerConnectionState| {
                let event_tx = event_tx_clone.clone();
                let peer_id = peer_id_clone.clone();

                Box::pin(async move {
                    match state {
                        RTCPeerConnectionState::Connected => {
                            info!("WebRTC connection established with peer: {}", peer_id);
                            let _ = event_tx
                                .send(WebRTCEvent::ConnectionEstablished { peer_id })
                                .await;
                        }
                        RTCPeerConnectionState::Disconnected
                        | RTCPeerConnectionState::Failed
                        | RTCPeerConnectionState::Closed => {
                            info!("WebRTC connection closed with peer: {}", peer_id);
                        }
                        _ => {}
                    }
                })
            },
        ));

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

    pub async fn establish_connection_with_answer(
        &self,
        peer_id: String,
        answer: String,
    ) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::HandleAnswer { peer_id, answer })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn establish_connection_with_offer(
        &self,
        peer_id: String,
        offer: String,
    ) -> Result<String, String> {
        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection
        let config = RTCConfiguration::default();
        let peer_connection: Arc<RTCPeerConnection> = match api.new_peer_connection(config).await {
            Ok(pc) => Arc::new(pc),
            Err(e) => {
                error!("Failed to create peer connection: {}", e);
                return Err(e.to_string());
            }
        };

        // Create data channel
        let data_channel = match peer_connection
            .create_data_channel("file-transfer", None)
            .await
        {
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
        let keystore_clone = Arc::new(self.keystore.clone());
        let active_private_key_clone = Arc::new(self.active_private_key.clone());
        let stream_auth_clone = Arc::new(self.stream_auth.clone());
        let bandwidth_clone = self.bandwidth.clone();

        let app_handle_clone = self.app_handle.clone();
        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();
            let keystore = keystore_clone.clone();
            let active_private_key = active_private_key_clone.clone();
            let stream_auth = stream_auth_clone.clone();
            let bandwidth = bandwidth_clone.clone();

            let app_handle_for_task = app_handle_clone.clone();
            Box::pin(async move {
                Self::handle_data_channel_message(
                    &peer_id,
                    &msg,
                    &event_tx,
                    &file_transfer_service,
                    &connections,
                    &keystore,
                    &active_private_key,
                    &stream_auth,
                    app_handle_for_task,
                    bandwidth,
                )
                .await;
            })
        }));

        // Set up peer connection event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(candidate_str) =
                        serde_json::to_string(&candidate.to_json().unwrap_or_default())
                    {
                        let _ = event_tx
                            .send(WebRTCEvent::IceCandidate {
                                peer_id,
                                candidate: candidate_str,
                            })
                            .await;
                    }
                }
            })
        }));

        peer_connection.on_peer_connection_state_change(Box::new(
            move |state: RTCPeerConnectionState| {
                let event_tx = event_tx_clone.clone();
                let peer_id = peer_id_clone.clone();

                Box::pin(async move {
                    match state {
                        RTCPeerConnectionState::Connected => {
                            info!("WebRTC connection established with peer: {}", peer_id);
                            let _ = event_tx
                                .send(WebRTCEvent::ConnectionEstablished { peer_id })
                                .await;
                        }
                        RTCPeerConnectionState::Disconnected
                        | RTCPeerConnectionState::Failed
                        | RTCPeerConnectionState::Closed => {
                            info!("WebRTC connection closed with peer: {}", peer_id);
                        }
                        _ => {}
                    }
                })
            },
        ));

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

    pub async fn send_file_request(
        &self,
        peer_id: String,
        request: WebRTCFileRequest,
    ) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::SendFileRequest { peer_id, request })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn send_data(
        &self,
        peer_id: &str,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let conns = self.connections.lock().await;
        if let Some(connection) = conns.get(peer_id) {
            if let Some(dc) = &connection.data_channel {
                let bytes_data = Bytes::from(data);
                dc.send(&bytes_data).await.map_err(|e| e.to_string())?;
                Ok(())
            } else {
                Err("Data channel not available".to_string())
            }
        } else {
            Err("Peer connection not found".to_string())
        }
    }

    pub async fn send_file_chunk(&self, peer_id: String, chunk: FileChunk) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::SendFileChunk { peer_id, chunk })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn add_ice_candidate(
        &self,
        peer_id: String,
        candidate: String,
    ) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::AddIceCandidate { peer_id, candidate })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn request_file_chunk(
        &self,
        peer_id: String,
        file_hash: String,
        chunk_index: u32,
    ) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::RequestFileChunk {
                peer_id,
                file_hash,
                chunk_index,
            })
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
        connections
            .get(peer_id)
            .map(|c| c.is_connected)
            .unwrap_or(false)
    }

    /// Encrypt a chunk using AES-GCM with a randomly generated key, then encrypt the key with recipient's public key
    async fn encrypt_chunk_for_peer(
        chunk_data: &[u8],
        recipient_public_key_hex: &str,
        _keystore: &Arc<Mutex<Keystore>>,
    ) -> Result<(Vec<u8>, EncryptedAesKeyBundle), String> {
        use x25519_dalek::PublicKey;

        // Generate random AES key for this chunk
        let aes_key = FileEncryption::generate_random_key();

        // Parse recipient's public key
        let recipient_public_key_bytes = hex::decode(recipient_public_key_hex)
            .map_err(|e| format!("Invalid recipient public key: {}", e))?;
        let recipient_public_key_bytes: [u8; 32] = recipient_public_key_bytes
            .try_into()
            .map_err(|_| "Invalid recipient public key length")?;
        let recipient_public_key = PublicKey::from(recipient_public_key_bytes);

        // Encrypt the AES key with recipient's public key (ECIES)
        let encrypted_key_bundle = encrypt_aes_key(&aes_key, &recipient_public_key)?;

        // Encrypt the chunk data with AES-GCM
        let key = aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&aes_key);
        let cipher = aes_gcm::Aes256Gcm::new(key);
        let nonce = aes_gcm::Aes256Gcm::generate_nonce(&mut aes_gcm::aead::OsRng);

        let encrypted_data = cipher
            .encrypt(&nonce, chunk_data)
            .map_err(|e| format!("Chunk encryption failed: {}", e))?;

        // Prepend nonce to encrypted data
        let mut result = nonce.to_vec();
        result.extend(encrypted_data);

        Ok((result, encrypted_key_bundle))
    }

    /// Decrypt a chunk using the encrypted AES key bundle and recipient's private key
    async fn decrypt_chunk_from_peer(
        encrypted_data: &[u8],
        encrypted_key_bundle: &EncryptedAesKeyBundle,
        recipient_private_key: &str,
    ) -> Result<Vec<u8>, String> {
        use x25519_dalek::StaticSecret;

        // Parse recipient's private key
        let recipient_private_key_bytes = hex::decode(recipient_private_key)
            .map_err(|e| format!("Invalid recipient private key: {}", e))?;
        let recipient_private_key_bytes: [u8; 32] = recipient_private_key_bytes
            .try_into()
            .map_err(|_| "Invalid recipient private key length")?;
        let recipient_private_key = StaticSecret::from(recipient_private_key_bytes);

        // Decrypt the AES key using recipient's private key
        let aes_key = decrypt_aes_key(encrypted_key_bundle, &recipient_private_key)?;

        // Extract nonce and encrypted data
        if encrypted_data.len() < 12 {
            return Err("Encrypted data too short".to_string());
        }
        let nonce = aes_gcm::Nonce::from_slice(&encrypted_data[..12]);
        let ciphertext = &encrypted_data[12..];

        // Decrypt the chunk data with AES-GCM
        let key = aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&aes_key);
        let cipher = aes_gcm::Aes256Gcm::new(key);

        let decrypted_data = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Chunk decryption failed: {}", e))?;

        Ok(decrypted_data)
    }
}

// Singleton instance
use lazy_static::lazy_static;

lazy_static! {
    static ref WEBRTC_SERVICE: Mutex<Option<Arc<WebRTCService>>> = Mutex::new(None);
}

pub async fn init_webrtc_service(
    file_transfer_service: Arc<FileTransferService>,
    app_handle: tauri::AppHandle,
    keystore: Arc<Mutex<Keystore>>,
    bandwidth: Arc<BandwidthController>,
) -> Result<(), String> {
    let mut service = WEBRTC_SERVICE.lock().await;
    if service.is_none() {
        let webrtc_service =
            WebRTCService::new(app_handle, file_transfer_service, keystore, bandwidth).await?;
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
        _output_path: String,
    ) -> Result<(), String> {
        info!(
            "Initiating P2P download: {} from peer {}",
            file_hash, peer_id
        );

        // Send file request over WebRTC
        if let Some(webrtc_service) = get_webrtc_service().await {
            let request = WebRTCFileRequest {
                file_hash: file_hash.clone(),
                file_name: "downloaded_file".to_string(), // Will be updated when we get metadata
                file_size: 0,                             // Will be updated
                requester_peer_id: "local_peer".to_string(), // Should be actual local peer ID
                recipient_public_key: None,               // No encryption for basic downloads
            };

            webrtc_service.send_file_request(peer_id, request).await?;
        } else {
            return Err("WebRTC service not available".to_string());
        }

        Ok(())
    }
}
