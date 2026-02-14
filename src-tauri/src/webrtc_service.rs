use crate::connection_retry::{ConnectionManager, ConnectionState, RetryConfig, WebRtcRetryContext, };
use crate::encryption::{decrypt_aes_key, encrypt_aes_key, EncryptedAesKeyBundle, FileEncryption};
use crate::file_transfer::FileTransferService;
use crate::keystore::Keystore;
use crate::bandwidth::BandwidthController;
use crate::manager::{ChunkInfo, FileManifest};
use crate::multi_source_download::MultiSourceDownloadService;
use crate::payment_checkpoint::PaymentCheckpointService;
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
use tracing::{debug, error, info, warn};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use indicatif::{ProgressBar, ProgressStyle};

lazy_static::lazy_static! {
    /// Global map of progress bars for active downloads, keyed by file hash
    static ref DOWNLOAD_PROGRESS_BARS: Mutex<HashMap<String, ProgressBar>> = Mutex::new(HashMap::new());

    /// Global map of progress bars for active uploads, keyed by (peer_id, file_hash)
    static ref UPLOAD_PROGRESS_BARS: Mutex<HashMap<String, ProgressBar>> = Mutex::new(HashMap::new());

    /// Requested output paths for WebRTC downloads (file_hash → output_path).
    /// This lets the download initiator (GUI/E2E API) control the final save location,
    /// while the assembler lives in this library crate (no access to binary AppState).
    static ref REQUESTED_WEBRTC_DOWNLOAD_OUTPUT_PATHS: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}

/// Record the desired output path for a given WebRTC file hash.
pub async fn set_requested_download_output_path(file_hash: String, output_path: String) {
    let mut map = REQUESTED_WEBRTC_DOWNLOAD_OUTPUT_PATHS.lock().await;
    map.insert(file_hash, output_path);
    // This log is intentionally INFO (not DEBUG) because it is a high-signal clue when
    // diagnosing "WebRTC saved to the wrong folder" issues in real-network runs.
    if let Some((k, v)) = map.iter().last() {
        info!("[PIN] WebRTC requested output path set: {} -> {}", k, v);
    }
}

async fn take_requested_download_output_path(file_hash: &str) -> Option<String> {
    let mut map = REQUESTED_WEBRTC_DOWNLOAD_OUTPUT_PATHS.lock().await;
    map.remove(file_hash)
}

const CHUNK_SIZE: usize = 32768; // 32KB chunks - configured data channel for larger messages (8x improvement over original 4KB)

// --- WebRTC binary framing for file chunks ---
// We send file chunks as *binary* messages instead of JSON text to avoid massive JSON overhead
// (Vec<u8> becomes a large numeric array in JSON, easily exceeding DataChannel max message size).
//
// Frame format (big-endian):
//   0..4   : magic "CHNK"
//   4      : version (1)
//   5      : flags (bit0: encrypted_key_bundle present)
//   6..10  : chunk_index (u32)
//   10..14 : total_chunks (u32)
//   14..16 : file_name_len (u16)
//   16..48 : file_hash (32 bytes, raw)
//   48..80 : checksum (32 bytes, raw sha256)
//   [opt]  : key_bundle_len (u32) + key_bundle_json bytes (if flags bit0 set)
//   ...    : file_name bytes (utf8, file_name_len)
//   ...    : data bytes (rest)
const CHUNK_FRAME_MAGIC: &[u8; 4] = b"CHNK";
const CHUNK_FRAME_VERSION: u8 = 1;
const CHUNK_FRAME_FLAG_KEY_BUNDLE: u8 = 1 << 0;

fn encode_chunk_frame(chunk: &FileChunk) -> Result<Vec<u8>, String> {
    let file_hash_bytes_vec = hex::decode(&chunk.file_hash)
        .map_err(|e| format!("Invalid file_hash hex for chunk framing: {}", e))?;
    if file_hash_bytes_vec.len() != 32 {
        return Err(format!(
            "Invalid file_hash length for chunk framing (expected 32 bytes, got {})",
            file_hash_bytes_vec.len()
        ));
    }
    let mut file_hash_bytes = [0u8; 32];
    file_hash_bytes.copy_from_slice(&file_hash_bytes_vec);

    let checksum_hex = if chunk.checksum.len() == 64 {
        chunk.checksum.clone()
    } else {
        // Fallback: recompute checksum from data if the stored value isn't a 32-byte hex digest.
        let mut hasher = Sha256::default();
        hasher.update(&chunk.data);
        format!("{:x}", hasher.finalize())
    };
    let checksum_vec =
        hex::decode(&checksum_hex).map_err(|e| format!("Invalid checksum hex for framing: {}", e))?;
    if checksum_vec.len() != 32 {
        return Err(format!(
            "Invalid checksum length for chunk framing (expected 32 bytes, got {})",
            checksum_vec.len()
        ));
    }
    let mut checksum_bytes = [0u8; 32];
    checksum_bytes.copy_from_slice(&checksum_vec);

    let file_name_bytes = chunk.file_name.as_bytes();
    if file_name_bytes.len() > u16::MAX as usize {
        return Err("file_name too long for chunk framing".to_string());
    }
    let file_name_len = file_name_bytes.len() as u16;

    let mut flags: u8 = 0;
    let mut key_bundle_json: Vec<u8> = Vec::new();
    if let Some(bundle) = &chunk.encrypted_key_bundle {
        flags |= CHUNK_FRAME_FLAG_KEY_BUNDLE;
        key_bundle_json = serde_json::to_vec(bundle)
            .map_err(|e| format!("Failed to serialize encrypted_key_bundle: {}", e))?;
    }

    let mut out = Vec::with_capacity(
        4 + 1 + 1 + 4 + 4 + 2 + 32 + 32
            + if flags & CHUNK_FRAME_FLAG_KEY_BUNDLE != 0 {
                4 + key_bundle_json.len()
            } else {
                0
            }
            + file_name_bytes.len()
            + chunk.data.len(),
    );

    out.extend_from_slice(CHUNK_FRAME_MAGIC);
    out.push(CHUNK_FRAME_VERSION);
    out.push(flags);
    out.extend_from_slice(&chunk.chunk_index.to_be_bytes());
    out.extend_from_slice(&chunk.total_chunks.to_be_bytes());
    out.extend_from_slice(&file_name_len.to_be_bytes());
    out.extend_from_slice(&file_hash_bytes);
    out.extend_from_slice(&checksum_bytes);

    if flags & CHUNK_FRAME_FLAG_KEY_BUNDLE != 0 {
        let len_u32: u32 = key_bundle_json
            .len()
            .try_into()
            .map_err(|_| "encrypted_key_bundle too large".to_string())?;
        out.extend_from_slice(&len_u32.to_be_bytes());
        out.extend_from_slice(&key_bundle_json);
    }

    out.extend_from_slice(file_name_bytes);
    out.extend_from_slice(&chunk.data);
    Ok(out)
}

fn decode_chunk_frame(data: &[u8]) -> Result<Option<FileChunk>, String> {
    // Fast reject: must start with magic and have the minimal fixed header.
    const MIN_HEADER: usize = 4 + 1 + 1 + 4 + 4 + 2 + 32 + 32;
    if data.len() < MIN_HEADER {
        return Ok(None);
    }
    if &data[0..4] != CHUNK_FRAME_MAGIC {
        return Ok(None);
    }

    let version = data[4];
    if version != CHUNK_FRAME_VERSION {
        return Err(format!("Unsupported chunk frame version: {}", version));
    }

    let flags = data[5];
    let mut pos = 6;

    let chunk_index = u32::from_be_bytes(
        data[pos..pos + 4]
            .try_into()
            .map_err(|_| "Invalid chunk_index bytes".to_string())?,
    );
    pos += 4;
    let total_chunks = u32::from_be_bytes(
        data[pos..pos + 4]
            .try_into()
            .map_err(|_| "Invalid total_chunks bytes".to_string())?,
    );
    pos += 4;
    let file_name_len = u16::from_be_bytes(
        data[pos..pos + 2]
            .try_into()
            .map_err(|_| "Invalid file_name_len bytes".to_string())?,
    ) as usize;
    pos += 2;

    let file_hash_bytes: [u8; 32] = data[pos..pos + 32]
        .try_into()
        .map_err(|_| "Invalid file_hash bytes".to_string())?;
    pos += 32;
    let checksum_bytes: [u8; 32] = data[pos..pos + 32]
        .try_into()
        .map_err(|_| "Invalid checksum bytes".to_string())?;
    pos += 32;

    let encrypted_key_bundle: Option<EncryptedAesKeyBundle> =
        if flags & CHUNK_FRAME_FLAG_KEY_BUNDLE != 0 {
            if data.len() < pos + 4 {
                return Err("Chunk frame truncated before key_bundle_len".to_string());
            }
            let key_len = u32::from_be_bytes(
                data[pos..pos + 4]
                    .try_into()
                    .map_err(|_| "Invalid key_bundle_len bytes".to_string())?,
            ) as usize;
            pos += 4;
            if data.len() < pos + key_len {
                return Err("Chunk frame truncated in key_bundle".to_string());
            }
            let bundle = serde_json::from_slice::<EncryptedAesKeyBundle>(&data[pos..pos + key_len])
                .map_err(|e| format!("Failed to parse encrypted_key_bundle: {}", e))?;
            pos += key_len;
            Some(bundle)
        } else {
            None
        };

    if data.len() < pos + file_name_len {
        return Err("Chunk frame truncated in file_name".to_string());
    }
    let file_name = String::from_utf8(data[pos..pos + file_name_len].to_vec())
        .map_err(|_| "Invalid utf8 in file_name".to_string())?;
    pos += file_name_len;

    let chunk_data = data[pos..].to_vec();

    Ok(Some(FileChunk {
        file_hash: hex::encode(file_hash_bytes),
        file_name,
        chunk_index,
        total_chunks,
        data: chunk_data,
        checksum: hex::encode(checksum_bytes),
        encrypted_key_bundle,
    }))
}

/// Maximum connection retry attempts before giving up
const MAX_CONNECTION_RETRIES: u32 = 3;

/// Initial delay between connection retries (milliseconds)
const INITIAL_RETRY_DELAY_MS: u64 = 1000;

/// Maximum delay between connection retries (milliseconds)
const MAX_RETRY_DELAY_MS: u64 = 15000;

/// Creates a WebRTC configuration with STUN and TURN servers for NAT traversal.
/// Without ICE servers, WebRTC connections will fail for users behind NAT (majority of users).
/// 
/// TURN servers are required for symmetric NAT (common in universities/corporate networks).
fn create_rtc_configuration() -> RTCConfiguration {
    RTCConfiguration {
        ice_servers: vec![
            // Google STUN servers (reliable, no auth needed)
            RTCIceServer {
                urls: vec![
                    "stun:stun.l.google.com:19302".to_string(),
                    "stun:stun1.l.google.com:19302".to_string(),
                    "stun:stun2.l.google.com:19302".to_string(),
                    "stun:stun3.l.google.com:19302".to_string(),
                ],
                ..Default::default()
            },
            // Evan Brass experimental TURN server (free, public)
            RTCIceServer {
                urls: vec![
                    "turn:stun.evan-brass.net".to_string(),
                    "turn:stun.evan-brass.net?transport=tcp".to_string(),
                    "stun:stun.evan-brass.net".to_string(),
                ],
                username: "guest".to_string(),
                credential: "password".to_string(),
                credential_type: RTCIceCredentialType::Password,
            },
        ],
        ..Default::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRTCFileRequest {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub requester_peer_id: String,
    pub recipient_public_key: Option<String>, // For encrypted transfers
}

/// Sent by a downloader to request the full file manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRTCManifestRequest {
    pub file_hash: String, // The Merkle Root
}

/// Sent by a seeder in response to a manifest request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRTCManifestResponse {
    pub file_hash: String,     // The Merkle Root, to match the request
    pub manifest_json: String, // The full FileManifest, serialized to JSON
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChunk {
    pub file_hash: String,
    pub file_name: String, // Add file_name field to preserve original filename
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub data: Vec<u8>,
    pub checksum: String,
    pub encrypted_key_bundle: Option<EncryptedAesKeyBundle>, // For encrypted transfers
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub pending_chunks: HashMap<String, Vec<FileChunk>>, // file_hash → chunks
    pub received_chunks: HashMap<String, HashMap<u32, FileChunk>>, // file_hash → chunk_index -> chunk
    pub acked_chunks: HashMap<String, std::collections::HashSet<u32>>, // file_hash → acked chunk indices
    pub pending_acks: HashMap<String, u32>, // file_hash → number of unacked chunks
    /// Retry context for connection resilience
    pub retry_context: Option<WebRtcRetryContext>,
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
    /// Retry a failed connection with exponential backoff
    RetryConnection {
        peer_id: String,
        offer: Option<String>,
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
    /// Connection is being retried after failure
    ConnectionRetrying {
        peer_id: String,
        attempt: u32,
        max_attempts: u32,
        next_retry_ms: u64,
    },
    /// Connection permanently failed after all retries exhausted
    ConnectionPermanentlyFailed {
        peer_id: String,
        total_attempts: u32,
        last_error: String,
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

/// ACK message sent by downloader to confirm chunk receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkAck {
    pub file_hash: String,
    pub chunk_index: u32,
    pub ready_for_more: bool, // Signal to send more chunks
}

/// A new enum to wrap different message types for clarity.
/// Note: The tag is case-insensitive for matching frontend messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WebRTCMessage {
    #[serde(alias = "file_request", alias = "FileRequest")]
    FileRequest(WebRTCFileRequest),
    #[serde(alias = "ManifestRequest")]
    ManifestRequest(WebRTCManifestRequest),
    #[serde(alias = "ManifestResponse")]
    ManifestResponse(WebRTCManifestResponse),
    #[serde(alias = "FileChunk")]
    FileChunk(FileChunk),
    #[serde(alias = "ChunkAck")]
    ChunkAck(ChunkAck),
}

pub struct WebRTCService {
    cmd_tx: mpsc::Sender<WebRTCCommand>,
    event_tx: mpsc::Sender<WebRTCEvent>,
    event_rx: Arc<Mutex<mpsc::Receiver<WebRTCEvent>>>,
    connections: Arc<Mutex<HashMap<String, PeerConnection>>>,
    file_transfer_service: Arc<FileTransferService>,
    // Optional: in headless mode we don't have a Tauri AppHandle, so we skip emitting UI events.
    app_handle: Option<tauri::AppHandle>,
    keystore: Arc<Mutex<Keystore>>,
    active_private_key: Arc<Mutex<Option<String>>>,
    bandwidth: Arc<BandwidthController>,
    /// Connection manager for retry logic
    connection_manager: Arc<ConnectionManager>,
    /// Multi-source download service for hash verification and chunk management
    multi_source_service: Option<Arc<MultiSourceDownloadService>>,
    /// Payment checkpoint service for incremental payments during file transfers
    payment_checkpoint: Option<Arc<PaymentCheckpointService>>,
}

impl WebRTCService {
    pub async fn new(
        app_handle: tauri::AppHandle,
        file_transfer_service: Arc<FileTransferService>,
        keystore: Arc<Mutex<Keystore>>,
        bandwidth: Arc<BandwidthController>,
    ) -> Result<Self, String> {
        Self::new_with_multi_source_opt(
            Some(app_handle),
            file_transfer_service,
            keystore,
            bandwidth,
            None,
            None,
        )
        .await
    }

    /// Create a new WebRTCService with optional MultiSourceDownloadService for hash verification
    /// and optional PaymentCheckpointService for incremental payments
    pub async fn new_with_multi_source(
        app_handle: tauri::AppHandle,
        file_transfer_service: Arc<FileTransferService>,
        keystore: Arc<Mutex<Keystore>>,
        bandwidth: Arc<BandwidthController>,
        multi_source_service: Option<Arc<MultiSourceDownloadService>>,
        payment_checkpoint: Option<Arc<PaymentCheckpointService>>,
    ) -> Result<Self, String> {
        Self::new_with_multi_source_opt(
            Some(app_handle),
            file_transfer_service,
            keystore,
            bandwidth,
            multi_source_service,
            payment_checkpoint,
        )
        .await
    }

    /// Headless constructor: no AppHandle, so no UI events are emitted.
    pub async fn new_headless(
        file_transfer_service: Arc<FileTransferService>,
        keystore: Arc<Mutex<Keystore>>,
        bandwidth: Arc<BandwidthController>,
        multi_source_service: Option<Arc<MultiSourceDownloadService>>,
    ) -> Result<Self, String> {
        Self::new_with_multi_source_opt(
            None,
            file_transfer_service,
            keystore,
            bandwidth,
            multi_source_service,
            None,
        )
        .await
    }

    async fn new_with_multi_source_opt(
        app_handle: Option<tauri::AppHandle>,
        file_transfer_service: Arc<FileTransferService>,
        keystore: Arc<Mutex<Keystore>>,
        bandwidth: Arc<BandwidthController>,
        multi_source_service: Option<Arc<MultiSourceDownloadService>>,
        payment_checkpoint: Option<Arc<PaymentCheckpointService>>,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(1000); // Increased capacity for high-throughput transfers
        let connections = Arc::new(Mutex::new(HashMap::new()));
        let active_private_key = Arc::new(Mutex::new(None));
        
        // Initialize connection manager with WebRTC-optimized retry config
        let connection_manager = Arc::new(ConnectionManager::new(RetryConfig::for_webrtc()));

        // Spawn the WebRTC service task
        let connection_manager_clone = connection_manager.clone();
        let multi_source_service_clone = multi_source_service.clone();
        let payment_checkpoint_clone = payment_checkpoint.clone();
        tokio::spawn(Self::run_webrtc_service(
            app_handle.clone(),
            cmd_rx,
            event_tx.clone(),
            connections.clone(),
            file_transfer_service.clone(),
            keystore.clone(),
            active_private_key.clone(),
            bandwidth.clone(),
            connection_manager_clone,
            multi_source_service_clone,
            payment_checkpoint_clone,
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
            bandwidth,
            connection_manager,
            multi_source_service,
            payment_checkpoint,
        })
    }

    /// Set the multi-source download service for hash verification
    pub async fn set_multi_source_service(&self, service: Option<Arc<MultiSourceDownloadService>>) {
        // Note: This requires updating the service in the spawned task
        // For now, we'll pass it through the constructor
        // If dynamic updates are needed, we can add a channel for this
    }

    /// Set the active private key for decryption operations
    pub async fn set_active_private_key(&self, private_key: Option<String>) {
        let mut key_guard = self.active_private_key.lock().await;
        *key_guard = private_key;
    }
    
    /// Get connection statistics
    pub async fn get_connection_stats(&self) -> crate::connection_retry::ConnectionManagerStats {
        self.connection_manager.get_stats().await
    }
    
    /// Manually trigger retry for a failed connection
    pub async fn retry_connection(&self, peer_id: &str) -> Result<(), String> {
        self.cmd_tx
            .send(WebRTCCommand::RetryConnection {
                peer_id: peer_id.to_string(),
                offer: None,
            })
            .await
            .map_err(|e| format!("Failed to send retry command: {}", e))
    }

    async fn run_webrtc_service(
        app_handle: Option<tauri::AppHandle>,
        mut cmd_rx: mpsc::Receiver<WebRTCCommand>,
        event_tx: mpsc::Sender<WebRTCEvent>,
        connections: Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: Arc<FileTransferService>,
        keystore: Arc<Mutex<Keystore>>,
        active_private_key: Arc<Mutex<Option<String>>>,
        bandwidth: Arc<BandwidthController>,
        connection_manager: Arc<ConnectionManager>,
        multi_source_service: Option<Arc<MultiSourceDownloadService>>,
        payment_checkpoint: Option<Arc<PaymentCheckpointService>>,
    ) {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                WebRTCCommand::EstablishConnection { peer_id, offer } => {
                    let Some(app_handle) = app_handle.as_ref() else {
                        warn!("WebRTC establish_connection requested in headless mode (no AppHandle). Skipping.");
                        continue;
                    };
                    Self::handle_establish_connection_with_retry(
                        app_handle,
                        &peer_id,
                        &offer,
                        &event_tx,
                        &connections,
                        &file_transfer_service,
                        &keystore,
                        &active_private_key,
                        &bandwidth,
                        &connection_manager,
                        multi_source_service.as_ref(),
                        &payment_checkpoint,
                    )
                    .await;
                }
                WebRTCCommand::HandleAnswer { peer_id, answer } => {
                    Self::handle_answer(&peer_id, &answer, &connections, &connection_manager).await;
                }
                WebRTCCommand::AddIceCandidate { peer_id, candidate } => {
                    Self::handle_ice_candidate(&peer_id, &candidate, &connections).await;
                }
                WebRTCCommand::SendFileRequest { peer_id, request } => {
                    info!("📤 Sending file request to peer {} for file {}", peer_id, request.file_hash);
                    // Send the file request over the data channel to the peer
                    Self::send_file_request_to_peer(&peer_id, &request, &connections).await;
                }
                WebRTCCommand::SendFileChunk { peer_id, chunk } => {
                    if let Err(e) = Self::handle_send_chunk(&peer_id, &chunk, &connections, &bandwidth).await {
                        error!("Failed to send file chunk to {}: {}", peer_id, e);
                    }
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
                    Self::handle_close_connection(&peer_id, &connections, &connection_manager).await;
                }
                WebRTCCommand::RetryConnection { peer_id, offer } => {
                    let Some(app_handle) = app_handle.as_ref() else {
                        warn!("WebRTC retry_connection requested in headless mode (no AppHandle). Skipping.");
                        continue;
                    };
                    Self::handle_retry_connection(
                        app_handle,
                        &peer_id,
                        offer.as_deref(),
                        &event_tx,
                        &connections,
                        &file_transfer_service,
                        &keystore,
                        &active_private_key,
                        &bandwidth,
                        &connection_manager,
                        multi_source_service.as_ref(),
                        &payment_checkpoint,
                    )
                    .await;
                }
            }
        }
    }
    
    /// Handle connection establishment with retry tracking
    async fn handle_establish_connection_with_retry(
        app_handle: &tauri::AppHandle,
        peer_id: &str,
        offer_sdp: &str,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: &Arc<FileTransferService>,
        keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        bandwidth: &Arc<BandwidthController>,
        connection_manager: &Arc<ConnectionManager>,
        multi_source_service: Option<&Arc<MultiSourceDownloadService>>,
        payment_checkpoint: &Option<Arc<PaymentCheckpointService>>,
    ) {
        // Get or create tracker for this peer
        let mut tracker = connection_manager.get_or_create(peer_id).await;
        tracker.start_retry();

        // Attempt connection
        let result = Self::handle_establish_connection_internal(
            app_handle,
            peer_id,
            offer_sdp,
            event_tx,
            connections,
            file_transfer_service,
            keystore,
            active_private_key,
            bandwidth,
            multi_source_service,
            payment_checkpoint,
        )
        .await;
        
        match result {
            Ok(()) => {
                tracker.record_success();
                connection_manager.update(tracker).await;
                info!("WebRTC connection to {} established successfully", peer_id);
            }
            Err(error) => {
                tracker.record_failure(&error);
                let state = tracker.state;
                let attempts = tracker.consecutive_failures;
                let config = tracker.config.clone();
                connection_manager.update(tracker).await;
                
                if state == ConnectionState::Failed {
                    // All retries exhausted
                    let _ = event_tx
                        .send(WebRTCEvent::ConnectionPermanentlyFailed {
                            peer_id: peer_id.to_string(),
                            total_attempts: attempts,
                            last_error: error,
                        })
                        .await;
                } else {
                    // Will retry - notify with backoff info
                    let delay = config.calculate_delay(attempts - 1);
                    let _ = event_tx
                        .send(WebRTCEvent::ConnectionRetrying {
                            peer_id: peer_id.to_string(),
                            attempt: attempts,
                            max_attempts: config.max_attempts,
                            next_retry_ms: delay.as_millis() as u64,
                        })
                        .await;
                }
            }
        }
    }
    
    /// Handle retry of a failed connection
    async fn handle_retry_connection(
        app_handle: &tauri::AppHandle,
        peer_id: &str,
        offer_sdp: Option<&str>,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: &Arc<FileTransferService>,
        keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        bandwidth: &Arc<BandwidthController>,
        connection_manager: &Arc<ConnectionManager>,
        multi_source_service: Option<&Arc<MultiSourceDownloadService>>,
        payment_checkpoint: &Option<Arc<PaymentCheckpointService>>,
    ) {
        let tracker = connection_manager.get_or_create(peer_id).await;
        
        // Check if we should retry
        if !tracker.is_ready_to_retry() {
            if let Some(wait_time) = tracker.time_until_retry() {
                debug!(
                    "Connection {} not ready to retry, waiting {:?}",
                    peer_id, wait_time
                );
                return;
            }
        }
        
        // Get stored offer from connection if not provided
        let offer = if let Some(o) = offer_sdp {
            o.to_string()
        } else {
            // Try to get from existing connection's retry context
            let conns = connections.lock().await;
            if let Some(conn) = conns.get(peer_id) {
                if let Some(ref ctx) = conn.retry_context {
                    if let Some(ref stored_offer) = ctx.last_offer {
                        stored_offer.clone()
                    } else {
                        warn!("No stored offer for retry of connection {}", peer_id);
                        return;
                    }
                } else {
                    warn!("No retry context for connection {}", peer_id);
                    return;
                }
            } else {
                warn!("No connection found for retry: {}", peer_id);
                return;
            }
        };
        
        info!("Retrying connection to peer {} (attempt {})", peer_id, tracker.consecutive_failures + 1);

        Self::handle_establish_connection_with_retry(
            app_handle,
            peer_id,
            &offer,
            event_tx,
            connections,
            file_transfer_service,
            keystore,
            active_private_key,
            bandwidth,
            connection_manager,
            multi_source_service,
            payment_checkpoint,
        )
        .await;
    }
    
    /// Internal connection establishment (without retry tracking)
    async fn handle_establish_connection_internal(
        app_handle: &tauri::AppHandle,
        peer_id: &str,
        offer_sdp: &str,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        file_transfer_service: &Arc<FileTransferService>,
        keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        bandwidth: &Arc<BandwidthController>,
        multi_source_service: Option<&Arc<MultiSourceDownloadService>>,
        payment_checkpoint: &Option<Arc<PaymentCheckpointService>>,
    ) -> Result<(), String> {
        // Call the existing implementation but return Result
        Self::handle_establish_connection(
            app_handle,
            peer_id,
            offer_sdp,
            event_tx,
            connections,
            file_transfer_service,
            keystore,
            active_private_key,
            bandwidth,
            multi_source_service,
            payment_checkpoint,
        )
        .await;
        
        // Check if connection was established by looking at the connection state
        let conns = connections.lock().await;
        if let Some(conn) = conns.get(peer_id) {
            if conn.peer_connection.is_some() {
                Ok(())
            } else {
                Err("Connection failed to establish".to_string())
            }
        } else {
            Err("Connection not found after establishment attempt".to_string())
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
        bandwidth: &Arc<BandwidthController>,
        multi_source_service: Option<&Arc<MultiSourceDownloadService>>,
        payment_checkpoint: &Option<Arc<PaymentCheckpointService>>,
    ) {
        info!("Establishing WebRTC connection with peer: {}", peer_id);

        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection with ICE servers for NAT traversal
        let config = create_rtc_configuration();
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

        // Create data channel with configuration for larger messages
        let mut dc_config = RTCDataChannelInit::default();
        dc_config.ordered = Some(true);  // Ensure ordered delivery for file chunks

        let data_channel = match peer_connection
            .create_data_channel("file-transfer", Some(dc_config))
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
        let bandwidth_clone = bandwidth.clone();
        let multi_source_service_clone = multi_source_service.cloned();
        let payment_checkpoint_clone = payment_checkpoint.clone();

        let app_handle_clone = app_handle.clone();
        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();
            let keystore = keystore_clone.clone();
            let active_private_key = active_private_key_clone.clone();
            let bandwidth = bandwidth_clone.clone();
            let multi_source_service = multi_source_service_clone.clone();
            let payment_checkpoint = payment_checkpoint_clone.clone();

            let app_handle_for_task = app_handle_clone.clone();
            // IMPORTANT: Spawn the handler as a separate task to avoid blocking the data channel
            // If we await here, the data channel can't receive more messages until this completes
            tokio::spawn(async move {
                Self::handle_data_channel_message(
                    &peer_id,
                    &msg,
                    &event_tx,
                    &file_transfer_service,
                    &connections,
                    &keystore,
                    &active_private_key,
                    Some(app_handle_for_task),
                    bandwidth,
                    multi_source_service.as_ref(),
                    &payment_checkpoint,
                )
                .await;
            });
            Box::pin(async {})
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
                    // Log the candidate type for debugging NAT traversal
                    let candidate_type = candidate.typ.to_string();
                    info!("ICE candidate generated for {}: type={}", peer_id, candidate_type);
                    
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
                } else {
                    info!("ICE gathering complete (null candidate received)");
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
                        RTCPeerConnectionState::Failed => {
                            error!("WebRTC connection failed for peer: {}", peer_id);
                        }
                        RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Closed => {
                            info!("WebRTC connection closed with peer: {}", peer_id);
                        }
                        _ => {
                            info!("WebRTC peer connection state: {:?} for peer: {}", state, peer_id);
                        }
                    }
                })
            },
        ));

        // Add ICE connection state handler for debugging NAT traversal issues
        let peer_id_for_ice_state = peer_id.to_string();
        peer_connection.on_ice_connection_state_change(Box::new(
            move |state: RTCIceConnectionState| {
                let peer_id = peer_id_for_ice_state.clone();
                Box::pin(async move {
                    match state {
                        RTCIceConnectionState::Checking => {
                            info!("ICE: Checking connectivity for peer: {}", peer_id);
                        }
                        RTCIceConnectionState::Connected => {
                            info!("ICE: Connected to peer: {} - NAT traversal successful!", peer_id);
                        }
                        RTCIceConnectionState::Completed => {
                            info!("ICE: Completed for peer: {} - All candidates checked", peer_id);
                        }
                        RTCIceConnectionState::Failed => {
                            error!("ICE: Failed for peer: {} - NAT traversal failed, TURN may not be working", peer_id);
                        }
                        RTCIceConnectionState::Disconnected => {
                            warn!("ICE: Disconnected from peer: {}", peer_id);
                        }
                        RTCIceConnectionState::Closed => {
                            info!("ICE: Closed for peer: {}", peer_id);
                        }
                        _ => {
                            debug!("ICE: State {:?} for peer: {}", state, peer_id);
                        }
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

        // Store connection with retry context
        let mut conns = connections.lock().await;
        let mut retry_ctx = WebRtcRetryContext::new(peer_id.to_string(), false);
        retry_ctx.last_offer = Some(offer_sdp.to_string());
        
        let connection = PeerConnection {
            peer_id: peer_id.to_string(),
            is_connected: false, // Will be set to true when connected
            active_transfers: HashMap::new(),
            last_activity: Instant::now(),
            peer_connection: Some(peer_connection),
            data_channel: Some(data_channel),
            pending_chunks: HashMap::new(),
            received_chunks: HashMap::new(),
            acked_chunks: HashMap::new(),
            pending_acks: HashMap::new(),
            retry_context: Some(retry_ctx),
        };
        conns.insert(peer_id.to_string(), connection);
    }

    async fn handle_answer(
        peer_id: &str,
        answer_sdp: &str,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        connection_manager: &Arc<ConnectionManager>,
    ) {
        // Check if the answer is an error message from the seeder
        if answer_sdp.starts_with("error:") {
            error!("Seeder {} returned error: {}", peer_id, answer_sdp);

            // Record failure in connection manager
            connection_manager.record_failure(peer_id, answer_sdp).await;

            // Remove the failed connection
            let mut conns = connections.lock().await;
            conns.remove(peer_id);

            // Log a helpful error message
            if answer_sdp.contains("webrtc-service-unavailable") {
                error!("Seeder does not have WebRTC service running. Try using Bitswap protocol instead.");
            }
            return;
        }

        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            if let Some(pc) = &connection.peer_connection {
                let answer = match serde_json::from_str::<RTCSessionDescription>(answer_sdp) {
                    Ok(answer) => answer,
                    Err(e) => {
                        error!("Failed to parse answer SDP: {}", e);
                        connection_manager.record_failure(peer_id, format!("Invalid answer SDP: {}", e)).await;
                        return;
                    }
                };

                if let Err(e) = pc.set_remote_description(answer).await {
                    error!("Failed to set remote description: {}", e);
                    connection_manager.record_failure(peer_id, format!("Failed to set remote description: {}", e)).await;
                } else {
                    // Answer was set successfully - connection is progressing
                    debug!("Successfully set remote description for peer {}", peer_id);
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

    async fn send_file_request_to_peer(
        peer_id: &str,
        request: &WebRTCFileRequest,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
    ) {
        use webrtc::data_channel::data_channel_state::RTCDataChannelState;

        info!("Sending file request to peer {} for file {}", peer_id, request.file_hash);

        // Wait for data channel to open (with timeout)
        let start = Instant::now();
        let timeout = Duration::from_secs(10);

        let dc = loop {
            let conns = connections.lock().await;
            if let Some(connection) = conns.get(peer_id) {
                if let Some(dc) = &connection.data_channel {
                    let state = dc.ready_state();
                    if state == RTCDataChannelState::Open {
                        break dc.clone();
                    }
                    if state == RTCDataChannelState::Closed || state == RTCDataChannelState::Closing {
                        error!("Data channel is closed or closing for peer {}", peer_id);
                        return;
                    }
                }
                // Data channel not yet available or not open - check timeout
                if start.elapsed() > timeout {
                    if connection.data_channel.is_none() {
                        error!("Timeout waiting for data channel to be assigned for peer {}", peer_id);
                    } else {
                        error!("Timeout waiting for data channel to open for peer {}", peer_id);
                    }
                    return;
                }
            } else {
                error!("Peer {} not found in connections", peer_id);
                return;
            }
            drop(conns); // Release lock before sleeping
            sleep(Duration::from_millis(50)).await;
        };

        // Serialize request and send over data channel
        match serde_json::to_string(request) {
            Ok(request_json) => {
                info!("[MSG] Sending file request JSON to peer {}: {}", peer_id, request_json);
                if let Err(e) = dc.send_text(request_json).await {
                    error!("Failed to send file request over data channel: {}", e);
                } else {
                    info!("✅ File request sent successfully to peer {}", peer_id);
                }
            }
            Err(e) => {
                error!("Failed to serialize file request: {}", e);
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
        bandwidth: &Arc<BandwidthController>,
        payment_checkpoint: &Option<Arc<PaymentCheckpointService>>,
    ) {
        info!(
            "📥 Handling file request from peer {}: {} (file_name: {})",
            peer_id, request.file_hash, request.file_name
        );

        // Check if we have the file locally
        let stored_files = file_transfer_service
            .get_stored_files()
            .await
            .unwrap_or_default();
        
        info!("[DIR] Checking {} stored files for hash {}", stored_files.len(), request.file_hash);
        
        let has_file = stored_files
            .iter()
            .any(|(hash, _)| hash == &request.file_hash);

        info!("[DIR] File {} found: {}", request.file_hash, has_file);

        if has_file {
            // Spawn file transfer as a separate task so the message handler
            // can continue processing incoming ACKs concurrently
            let peer_id = peer_id.to_string();
            let request = request.clone();
            let event_tx = event_tx.clone();
            let file_transfer_service = file_transfer_service.clone();
            let connections = connections.clone();
            let keystore = keystore.clone();
            let bandwidth = bandwidth.clone();
            let payment_checkpoint = payment_checkpoint.clone();

            tokio::spawn(async move {
                info!("🚀 Spawned file transfer task for {} to peer {}", request.file_hash, peer_id);
                match Self::start_file_transfer(
                    &peer_id,
                    &request,
                    &event_tx,
                    &file_transfer_service,
                    &connections,
                    &keystore,
                    &bandwidth,
                    &payment_checkpoint,
                )
                .await
                {
                    Ok(_) => {
                        info!("✅ File transfer completed successfully for {} to peer {}", request.file_hash, peer_id);
                    }
                    Err(e) => {
                        error!("❌ File transfer failed for {} to peer {}: {}", request.file_hash, peer_id, e);
                        let _ = event_tx
                            .send(WebRTCEvent::TransferFailed {
                                peer_id: peer_id.clone(),
                                file_hash: request.file_hash.clone(),
                                error: format!("Failed to start file transfer: {}", e),
                            })
                            .await;
                    }
                }
            });
        } else {
            error!("❌ File {} not found locally - cannot fulfill request from peer {}", request.file_hash, peer_id);
            // Log available files for debugging
            let available_hashes: Vec<_> = stored_files.iter().map(|(h, _)| h.clone()).collect();
            info!("[DIR] Available file hashes: {:?}", available_hashes);
            
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
    ) -> Result<(), String> {
        debug!("📤 handle_send_chunk: chunk {} for peer {}, acquiring bandwidth", chunk.chunk_index, peer_id);
        bandwidth.acquire_upload(chunk.data.len()).await;
        debug!("📤 handle_send_chunk: bandwidth acquired for chunk {}", chunk.chunk_index);

        // Wait for data channel to open (with timeout)
        use webrtc::data_channel::data_channel_state::RTCDataChannelState;
        let start = Instant::now();
        let timeout = Duration::from_secs(10);

        let dc = loop {
            let conns = connections.lock().await;
            if let Some(connection) = conns.get(peer_id) {
                if let Some(dc) = &connection.data_channel {
                    let state = dc.ready_state();
                    if state == RTCDataChannelState::Open {
                        if start.elapsed().as_millis() > 100 {
                            info!("🌐 Data channel ready after {}ms for peer {}", start.elapsed().as_millis(), peer_id);
                        }
                        break dc.clone();
                    }
                    if state == RTCDataChannelState::Closed || state == RTCDataChannelState::Closing {
                        error!("Data channel is closed or closing for peer {}", peer_id);
                        return Err(format!("Data channel closed for peer {}", peer_id));
                    }
                } else {
                    // Log waiting for data channel (but only occasionally)
                    if start.elapsed().as_millis() % 1000 < 100 && start.elapsed().as_millis() > 100 {
                        debug!("⏳ Waiting for data channel to be assigned for peer {} ({}ms elapsed)", peer_id, start.elapsed().as_millis());
                    }
                }
                // Data channel not yet available or not open - check timeout
                if start.elapsed() > timeout {
                    if connection.data_channel.is_none() {
                        error!("Timeout waiting for data channel to be assigned for peer {}", peer_id);
                        return Err(format!("Data channel never assigned for peer {}", peer_id));
                    } else {
                        error!("Timeout waiting for data channel to open for peer {}", peer_id);
                        return Err(format!("Data channel timeout for peer {}", peer_id));
                    }
                }
            } else {
                error!("Peer {} not found in connections", peer_id);
                return Err(format!("Peer {} not found", peer_id));
            }
            drop(conns); // Release lock before sleeping
            sleep(Duration::from_millis(50)).await;
        };

        // Encode chunk as a binary frame and send over data channel.
        // This avoids huge JSON overhead and prevents "outbound packet larger than maximum message size".
        let payload = match encode_chunk_frame(chunk) {
            Ok(frame) => frame,
            Err(e) => {
                // Fallback to JSON only if framing fails for unexpected inputs.
                warn!("Chunk framing failed; falling back to JSON send_text: {}", e);
                let chunk_json =
                    serde_json::to_string(chunk).map_err(|e| format!("Failed to serialize chunk: {}", e))?;
                // Check buffer before sending - wait if buffer is too full
                let max_buffered: usize = 2 * 1024 * 1024; // 2MB max buffer
                let start_wait = Instant::now();
                loop {
                    let buffered = dc.buffered_amount().await;
                    if buffered < max_buffered {
                        break;
                    }
                    if start_wait.elapsed() > Duration::from_secs(10) {
                        error!(
                            "❌ Timeout waiting for data channel buffer to drain (buffered: {} bytes)",
                            buffered
                        );
                        return Err("Data channel buffer timeout".to_string());
                    }
                    sleep(Duration::from_millis(1)).await;
                }
                dc.send_text(chunk_json)
                    .await
                    .map_err(|e| format!("Failed to send chunk (json): {}", e))?;
                return Ok(());
            }
        };

        // Check buffer before sending - wait if buffer is too full
        let max_buffered: usize = 2 * 1024 * 1024; // 2MB max buffer
        let start_wait = Instant::now();
        loop {
            let buffered = dc.buffered_amount().await;
            if buffered < max_buffered {
                break;
            }
            if start_wait.elapsed() > Duration::from_secs(10) {
                error!(
                    "❌ Timeout waiting for data channel buffer to drain (buffered: {} bytes)",
                    buffered
                );
                return Err("Data channel buffer timeout".to_string());
            }
            sleep(Duration::from_millis(1)).await;
        }

        let bytes_data = Bytes::from(payload);
        dc.send(&bytes_data)
            .await
            .map_err(|e| format!("Failed to send chunk (binary): {}", e))?;

        Ok(())
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
        connection_manager: &Arc<ConnectionManager>,
    ) {
        info!("Closing WebRTC connection with peer: {}", peer_id);
        let mut conns = connections.lock().await;
        if let Some(mut connection) = conns.remove(peer_id) {
            if let Some(pc) = connection.peer_connection.take() {
                let _ = pc.close().await;
            }
        }
        // Remove from connection manager tracking
        connection_manager.remove(peer_id).await;
    }

    async fn handle_data_channel_message(
        peer_id: &str,
        msg: &DataChannelMessage,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        app_handle: Option<tauri::AppHandle>,
        bandwidth: Arc<BandwidthController>,
        multi_source_service: Option<&Arc<MultiSourceDownloadService>>,
        payment_checkpoint: &Option<Arc<PaymentCheckpointService>>,
    ) {
        debug!("[MSG] Data channel message received from peer {}: {} bytes", peer_id, msg.data.len());

        // First, try to decode as a binary-framed FileChunk (preferred, avoids JSON overhead).
        match decode_chunk_frame(&msg.data) {
            Ok(Some(chunk)) => {
                // Create or update progress bar
                {
                    let mut bars = DOWNLOAD_PROGRESS_BARS.lock().await;
                    let pb = bars.entry(chunk.file_hash.clone()).or_insert_with(|| {
                        let pb = ProgressBar::new(chunk.total_chunks as u64);
                        pb.set_style(
                            ProgressStyle::default_bar()
                                .template("📦 {msg} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%)")
                                .unwrap()
                                .progress_chars("=>-"),
                        );
                        pb.set_message(format!("Downloading {}", &chunk.file_hash[..8]));
                        pb
                    });
                    pb.set_position((chunk.chunk_index + 1) as u64);
                }

                // Handle received chunk
                Self::process_incoming_chunk(
                    &chunk,
                    file_transfer_service,
                    connections,
                    event_tx,
                    peer_id,
                    keystore,
                    &active_private_key,
                    app_handle.as_ref(),
                    &bandwidth,
                    multi_source_service,
                )
                .await;
                let _ = event_tx
                    .send(WebRTCEvent::FileChunkReceived {
                        peer_id: peer_id.to_string(),
                        chunk,
                    })
                    .await;
                return;
            }
            Ok(None) => {
                // Not a chunk frame; fall through to text-based parsing.
            }
            Err(e) => {
                // If a peer sends a malformed chunk frame, treat it as a transfer failure but don't crash.
                warn!("Failed to decode chunk frame from {}: {}", peer_id, e);
            }
        }

        if let Ok(text) = std::str::from_utf8(&msg.data) {
            // Log first 500 chars of message for debugging
            let preview = if text.len() > 500 { &text[..500] } else { text };
            debug!("📝 Message preview from {}: {}", peer_id, preview);
            
            // Try to parse as FileChunk first (most common)
            if let Ok(chunk) = serde_json::from_str::<FileChunk>(text) {
                // Create or update progress bar
                {
                    let mut bars = DOWNLOAD_PROGRESS_BARS.lock().await;
                    let pb = bars.entry(chunk.file_hash.clone()).or_insert_with(|| {
                        let pb = ProgressBar::new(chunk.total_chunks as u64);
                        pb.set_style(ProgressStyle::default_bar()
                            .template("📦 {msg} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {bytes_per_sec} ETA: {eta}")
                            .unwrap()
                            .progress_chars("=>-"));
                        pb.set_message(format!("Downloading {}", &chunk.file_hash[..8]));
                        pb
                    });
                    pb.inc(1); // Increment by 1 so indicatif can calculate speed
                }

                // Handle received chunk
                Self::process_incoming_chunk(
                    &chunk,
                    file_transfer_service,
                    connections,
                    event_tx,
                    peer_id,
                    keystore,
                    &active_private_key,
                    app_handle.as_ref(),
                    &bandwidth,
                    multi_source_service,
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
                    &bandwidth,
                    &payment_checkpoint,
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
                            &bandwidth,
                            &payment_checkpoint,
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
                            app_handle.as_ref(),
                            &bandwidth,
                            multi_source_service,
                        )
                        .await;
                    }
                    WebRTCMessage::ChunkAck(ack) => {
                        // Handle ACK from downloader
                        let mut conns = connections.lock().await;
                        if let Some(connection) = conns.get_mut(peer_id) {
                            // Record this chunk as ACKed
                            let acked = connection.acked_chunks
                                .entry(ack.file_hash.clone())
                                .or_insert_with(std::collections::HashSet::new);
                            acked.insert(ack.chunk_index);

                            // Decrement pending ACK count
                            if let Some(pending) = connection.pending_acks.get_mut(&ack.file_hash) {
                                if *pending > 0 {
                                    *pending -= 1;
                                }
                            }

                            info!("Received ACK for chunk {} of file {} from peer {}",
                                  ack.chunk_index, ack.file_hash, peer_id);
                        }
                    }
                }
            } else {
                // None of the parsing attempts succeeded - log for debugging
                warn!("⚠️ Failed to parse data channel message from peer {}. Message preview: {}", 
                      peer_id, 
                      if text.len() > 200 { &text[..200] } else { text });
            }
        } else {
            warn!("⚠️ Received non-UTF8 data from peer {} ({} bytes)", peer_id, msg.data.len());
        }
    }

    async fn start_file_transfer(
        peer_id: &str,
        request: &WebRTCFileRequest,
        event_tx: &mpsc::Sender<WebRTCEvent>,
        file_transfer_service: &Arc<FileTransferService>,
        connections: &Arc<Mutex<HashMap<String, PeerConnection>>>,
        keystore: &Arc<Mutex<Keystore>>,
        bandwidth: &Arc<BandwidthController>,
        payment_checkpoint: &Option<Arc<PaymentCheckpointService>>,
    ) -> Result<(), String> {
        // Wait for data channel to be available (race condition fix)
        // The on_data_channel callback stores the channel in a spawned task,
        // so it may not be available immediately when this function is called.
        use webrtc::data_channel::data_channel_state::RTCDataChannelState;
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        
        loop {
            let dc_state = {
                let conns = connections.lock().await;
                if let Some(connection) = conns.get(peer_id) {
                    connection.data_channel.as_ref().map(|dc| dc.ready_state())
                } else {
                    return Err(format!("Peer {} not found in connections at start of file transfer", peer_id));
                }
            };
            
            match dc_state {
                Some(RTCDataChannelState::Open) => {
                    info!("✅ Data channel ready for file transfer to peer {}", peer_id);
                    break;
                }
                Some(RTCDataChannelState::Closed) | Some(RTCDataChannelState::Closing) => {
                    error!("❌ Data channel closed/closing for peer {} before file transfer started", peer_id);
                    return Err(format!("Data channel closed for peer {}", peer_id));
                }
                Some(state) => {
                    if start.elapsed() > timeout {
                        error!("❌ Timeout waiting for data channel to open (state: {:?}) for peer {}", state, peer_id);
                        return Err(format!("Data channel timeout (state: {:?})", state));
                    }
                    debug!("⏳ Waiting for data channel to open (state: {:?}) for peer {}", state, peer_id);
                }
                None => {
                    if start.elapsed() > timeout {
                        error!("❌ Timeout waiting for data channel to be assigned for peer {}", peer_id);
                        return Err("Data channel not assigned".to_string());
                    }
                    debug!("⏳ Waiting for data channel to be assigned for peer {}", peer_id);
                }
            }
            
            sleep(Duration::from_millis(50)).await;
        }
        
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

        // Calculate total chunks
        let total_chunks = ((file_data.len() as f64) / CHUNK_SIZE as f64).ceil() as u32;

        info!(
            "Starting real file transfer of {} ({} bytes, {} chunks) to peer {}",
            request.file_name,
            file_data.len(),
            total_chunks,
            peer_id
        );

        // Initialize payment checkpoint session if service available
        if let Some(checkpoint_service) = payment_checkpoint {
            let session_id = format!("{}_{}", request.file_hash, peer_id);
            let file_size = file_data.len() as u64;

            checkpoint_service.init_session(
                session_id.clone(),
                request.file_hash.clone(),
                file_size,
                "seeder_address".to_string(), // TODO: Get from request or config
                peer_id.to_string(),
                0.001, // TODO: Get price from request or config
                "exponential".to_string(),
            ).await.map_err(|e| format!("Failed to init checkpoint: {}", e))?;

            info!("✅ Payment checkpoint session initialized: {}", session_id);
        }

        // NOTE: HMAC authentication is disabled for WebRTC transfers.
        // WebRTC already provides transport-level security via DTLS.
        // The previous HMAC implementation had a key exchange race condition
        // where chunks were sent before the receiver had the shared secret.

        // Initialize transfer tracking in connections
        {
            let mut conns = connections.lock().await;
            info!("[LOCK] Acquired connections lock for transfer tracking, peer: {}", peer_id);
            if let Some(connection) = conns.get_mut(peer_id) {
                info!("📝 Initializing transfer tracking for peer {}, data_channel present: {}", 
                      peer_id, connection.data_channel.is_some());
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

        // Flow control constants
        const BATCH_SIZE: u32 = 100; // Send 100 chunks before checking ACKs (increased from 10)
        const MAX_PENDING_ACKS: u32 = 200; // Maximum unacked chunks before pausing (increased from 20)
        const ACK_WAIT_TIMEOUT_MS: u64 = 5000; // Timeout waiting for ACKs

        // Initialize pending ACK counter
        {
            let mut conns = connections.lock().await;
            info!("[LOCK] Acquired connections lock for pending_acks init, peer: {}", peer_id);
            if let Some(connection) = conns.get_mut(peer_id) {
                info!("✅ Found peer {} in connections, initializing pending_acks", peer_id);
                connection.pending_acks.insert(request.file_hash.clone(), 0);
                connection.acked_chunks.insert(request.file_hash.clone(), std::collections::HashSet::new());
            } else {
                error!("❌ Peer {} not found in connections when initializing pending_acks", peer_id);
                return Err(format!("Peer {} not found in connections", peer_id));
            }
        }

        info!("📦 Starting chunk loop for {} chunks to peer {}", total_chunks, peer_id);

        // Debug: log data channel state before starting loop
        {
            let conns = connections.lock().await;
            if let Some(connection) = conns.get(peer_id) {
                if let Some(dc) = &connection.data_channel {
                    info!("🌐 Data channel state before loop: {:?} for peer {}", dc.ready_state(), peer_id);
                } else {
                    error!("⚠️ No data channel found for peer {} before starting loop!", peer_id);
                }
            } else {
                error!("⚠️ Peer {} not in connections before starting loop!", peer_id);
            }
        }

        // Send file chunks over WebRTC data channel with flow control
        for chunk_index in 0..total_chunks {
            // Log EVERY chunk for first 100 to debug stall
            if chunk_index < 100 {
                info!("[LOOP] LOOP: Starting chunk {} for peer {}", chunk_index, peer_id);
            }

            // Log first few chunks, last chunk, and every 50th chunk
            if chunk_index < 10 || chunk_index == total_chunks - 1 || chunk_index % 50 == 0 {
                info!("📤 Processing chunk {}/{} for peer {}", chunk_index + 1, total_chunks, peer_id);
            }

            // Log every 20 chunks to track progress
            if chunk_index % 20 == 0 {
                info!("📊 Transfer progress: chunk {}/{} ({}%) to peer {}",
                    chunk_index, total_chunks,
                    (chunk_index as f32 / total_chunks as f32 * 100.0) as u32,
                    peer_id);
            }

            // Check if should pause for payment checkpoint
            if let Some(checkpoint_service) = payment_checkpoint {
                let session_id = format!("{}_{}", request.file_hash, peer_id);

                loop {
                    let should_pause = checkpoint_service
                        .should_pause_serving(&session_id)
                        .await
                        .unwrap_or(false);

                    if !should_pause {
                        break;
                    }

                    info!("[PAUSE]  Paused at chunk {} - waiting for payment", chunk_index);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }

            // Flow control: wait if too many pending ACKs
            let wait_start = Instant::now();
            let mut timeout_count = 0;
            debug!("🔄 Chunk {}: Entering flow control check", chunk_index);
            loop {
                let pending_count = {
                    let conns = connections.lock().await;
                    let count = conns.get(peer_id)
                        .and_then(|c| c.pending_acks.get(&request.file_hash).copied())
                        .unwrap_or(0);
                    // Log if peer not found or file_hash not in pending_acks
                    if conns.get(peer_id).is_none() {
                        error!("⚠️ Chunk {}: Peer {} NOT FOUND in connections during flow control!", chunk_index, peer_id);
                    }
                    count
                };
                
                // Log pending count for first 10 chunks
                if chunk_index < 10 || chunk_index % 100 == 0 {
                    info!("🔄 Chunk {}: pending_count={}, MAX_PENDING_ACKS={}", chunk_index, pending_count, MAX_PENDING_ACKS);
                }

                if pending_count < MAX_PENDING_ACKS {
                    break;
                }
                
                // Log when we're actually waiting for ACKs
                if chunk_index % 20 == 0 || pending_count >= MAX_PENDING_ACKS - 2 {
                    warn!("⏳ Chunk {}: Waiting for ACKs (pending={}, max={})", chunk_index, pending_count, MAX_PENDING_ACKS);
                }

                // Timeout check
                if wait_start.elapsed().as_millis() > ACK_WAIT_TIMEOUT_MS as u128 {
                    timeout_count += 1;
                    warn!("ACK timeout #{} waiting for peer {} (pending: {}, chunk: {}/{})", 
                          timeout_count, peer_id, pending_count, chunk_index, total_chunks);
                    
                    // After 3 consecutive timeouts, check if connection is still alive
                    if timeout_count >= 3 {
                        let dc_state = {
                            let conns = connections.lock().await;
                            conns.get(peer_id)
                                .and_then(|c| c.data_channel.as_ref())
                                .map(|dc| dc.ready_state())
                        };
                        
                        if let Some(state) = dc_state {
                            use webrtc::data_channel::data_channel_state::RTCDataChannelState;
                            if state != RTCDataChannelState::Open {
                                error!("Data channel no longer open (state: {:?}), aborting transfer", state);
                                let _ = event_tx
                                    .send(WebRTCEvent::TransferFailed {
                                        peer_id: peer_id.to_string(),
                                        file_hash: request.file_hash.clone(),
                                        error: "Connection lost - data channel closed".to_string(),
                                    })
                                    .await;
                                return Err("Data channel closed".to_string());
                            }
                        }
                    }
                    break;
                }

                // Wait a bit before checking again
                sleep(Duration::from_millis(50)).await;
            }
            
            // Log after flow control (for first 100 chunks)
            if chunk_index < 100 {
                info!("[UNLOCK] FLOW_CONTROL_PASSED: chunk {} for peer {}", chunk_index, peer_id);
            }

            let start = (chunk_index as usize) * CHUNK_SIZE;
            let end = (start + CHUNK_SIZE).min(file_data.len());
            let chunk_data: Vec<u8> = file_data[start..end].to_vec();

            let (final_chunk_data, encrypted_key_bundle) =
                if let Some(ref recipient_key) = request.recipient_public_key {
                    // Encrypted transfer - no HMAC authentication needed (AES-256-GCM provides AEAD)
                    if chunk_index < 100 {
                        info!("[LOCK] Encrypting chunk {} for peer {}", chunk_index, peer_id);
                    }
                    match Self::encrypt_chunk_for_peer(&chunk_data, recipient_key, keystore).await {
                        Ok((encrypted_data, key_bundle)) => {
                            if chunk_index < 100 {
                                info!("[LOCK] Encryption done for chunk {}", chunk_index);
                            }
                            (encrypted_data, Some(key_bundle))
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
                    // Unencrypted transfer - WebRTC provides transport security via DTLS
                    // No additional HMAC authentication needed (was causing ACK deadlock)
                    (chunk_data, None)
                };

            // Calculate checksum for the final data (encrypted or not)
            let checksum = Self::calculate_chunk_checksum(&final_chunk_data);

            let chunk = FileChunk {
                file_hash: request.file_hash.clone(),
                file_name: request.file_name.clone(), // Include original filename
                chunk_index,
                total_chunks,
                data: final_chunk_data,
                checksum,
                encrypted_key_bundle,
            };

            // Create or update upload progress bar
            {
                let pb_key = format!("{}:{}", peer_id, &request.file_hash);
                let mut bars = UPLOAD_PROGRESS_BARS.lock().await;
                let pb = bars.entry(pb_key).or_insert_with(|| {
                    let pb = ProgressBar::new(total_chunks as u64);
                    pb.set_style(ProgressStyle::default_bar()
                        .template("📤 {msg} [{bar:40.green/blue}] {pos}/{len} ({percent}%) {bytes_per_sec} ETA: {eta}")
                        .unwrap()
                        .progress_chars("=>-"));
                    pb.set_message(format!("Uploading {}", &request.file_hash[..8]));
                    pb
                });
                pb.inc(1); // Increment by 1 so indicatif can calculate speed
            }

            // Send chunk via WebRTC data channel - abort transfer if send fails
            if let Err(e) = Self::handle_send_chunk(peer_id, &chunk, connections, bandwidth).await {
                error!("Failed to send chunk {}/{} to peer {}: {}", chunk_index, total_chunks, peer_id, e);
                let _ = event_tx
                    .send(WebRTCEvent::TransferFailed {
                        peer_id: peer_id.to_string(),
                        file_hash: request.file_hash.clone(),
                        error: format!("Connection lost: {}", e),
                    })
                    .await;
                return Err(format!("Transfer aborted: {}", e));
            }

            // Update payment checkpoint progress after sending chunk
            if let Some(checkpoint_service) = payment_checkpoint {
                let session_id = format!("{}_{}", request.file_hash, peer_id);
                let bytes_transferred = ((chunk_index as u64 + 1) * CHUNK_SIZE as u64).min(file_data.len() as u64);

                checkpoint_service
                    .update_progress(&session_id, bytes_transferred)
                    .await
                    .map_err(|e| format!("Failed to update checkpoint progress: {}", e))?;
            }

            // Increment pending ACK count (only if send succeeded)
            // IMPORTANT: Don't hold lock while sending events to avoid deadlock
            let progress_to_send = {
                let mut conns = connections.lock().await;
                if let Some(connection) = conns.get_mut(peer_id) {
                    *connection.pending_acks.entry(request.file_hash.clone()).or_insert(0) += 1;

                    if let Some(transfer) = connection.active_transfers.get_mut(&request.file_hash)
                    {
                        transfer.chunks_sent += 1;
                        transfer.bytes_sent += chunk.data.len() as u64;

                        // Prepare progress update (send outside lock)
                        Some(TransferProgress {
                            file_hash: request.file_hash.clone(),
                            bytes_transferred: transfer.bytes_sent,
                            total_bytes: transfer.file_size,
                            chunks_transferred: transfer.chunks_sent,
                            total_chunks: transfer.total_chunks,
                            percentage: (transfer.chunks_sent as f32
                                / transfer.total_chunks as f32)
                                * 100.0,
                        })
                    } else {
                        None
                    }
                } else {
                    // This should never happen - peer not in connections after successful send
                    error!("❌ CRITICAL: Peer {} disappeared from connections after sending chunk {}!", peer_id, chunk_index);
                    None
                }
            };
            
            // Send progress event OUTSIDE the lock to avoid deadlock
            // Use try_send to avoid blocking if channel is full - progress events are not critical
            if let Some(progress) = progress_to_send {
                match event_tx.try_send(WebRTCEvent::TransferProgress {
                    peer_id: peer_id.to_string(),
                    progress,
                }) {
                    Ok(_) => {},
                    Err(e) => {
                        // Channel full - skip this progress event, not critical
                        if chunk_index < 100 || chunk_index % 100 == 0 {
                            warn!("📊 Progress event skipped for chunk {} (channel full): {}", chunk_index, e);
                        }
                    }
                }
            }
            
            // Log completion of chunk processing (every 100 chunks to avoid spam)
            if chunk_index % 100 == 0 && chunk_index > 0 {
                info!("✅ Chunk {} sent and tracked successfully for peer {}", chunk_index, peer_id);
            }

            // Only yield to other tasks every 10 chunks (no artificial delays)
            if (chunk_index + 1) % 10 == 0 {
                tokio::task::yield_now().await;
            }
        }

        // Finish and remove upload progress bar
        {
            let pb_key = format!("{}:{}", peer_id, &request.file_hash);
            if let Some(pb) = UPLOAD_PROGRESS_BARS.lock().await.remove(&pb_key) {
                pb.finish_with_message(format!("✅ Uploaded {}", &request.file_hash[..8]));
            }
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

        // Mark payment checkpoint session as completed
        if let Some(checkpoint_service) = payment_checkpoint {
            let session_id = format!("{}_{}", request.file_hash, peer_id);
            checkpoint_service
                .mark_completed(&session_id)
                .await
                .map_err(|e| format!("Failed to mark checkpoint complete: {}", e))?;

            info!("✅ Payment checkpoint session completed: {}", session_id);
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
        keystore: &Arc<Mutex<Keystore>>,
        active_private_key: &Arc<Mutex<Option<String>>>,
        app_handle: Option<&tauri::AppHandle>,
        bandwidth: &Arc<BandwidthController>,
        multi_source_service: Option<&Arc<MultiSourceDownloadService>>,
    ) {
        // NOTE: HMAC authentication removed - WebRTC DTLS provides transport security.

        // 1. Decrypt chunk data if it was encrypted
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

        // 2. Verify chunk checksum
        let chunk_len = final_chunk_data.len();
        let calculated_checksum = Self::calculate_chunk_checksum(&final_chunk_data);
        if calculated_checksum != chunk.checksum {
            warn!("Chunk checksum mismatch for file {}", chunk.file_hash);
            return;
        }

        // 3. Verify SHA-256 hash if multi-source service is available
        if let Some(service) = multi_source_service {
            if service.verify_chunk_for_download(
                &chunk.file_hash,
                chunk.chunk_index,
                &final_chunk_data,
                peer_id,
            )
            .await
            .is_err()
            {
                warn!(
                    "WebRTC chunk {} hash verification failed for file {}",
                    chunk.chunk_index, chunk.file_hash
                );
                return; // Don't store the chunk
            } else {
                debug!(
                    "WebRTC chunk {} hash verification passed for file {}",
                    chunk.chunk_index, chunk.file_hash
                );
            }
        }

        bandwidth.acquire_download(chunk_len).await;

        // Get data channel reference before locking connections
        let dc_for_ack = {
            let conns = connections.lock().await;
            conns.get(peer_id)
                .and_then(|c| c.data_channel.clone())
        };

        let mut conns = connections.lock().await;
        if let Some(connection) = conns.get_mut(peer_id) {
            // Store chunk
            let chunks = connection
                .received_chunks
                .entry(chunk.file_hash.clone())
                .or_insert_with(HashMap::new);
            chunks.insert(chunk.chunk_index, chunk.clone());

            // Emit progress to frontend
            if let Some(total_chunks) = chunks.values().next().map(|c| c.total_chunks) {
                let progress_percentage = (chunks.len() as f32 / total_chunks as f32) * 100.0;
                let bytes_received = chunks.len() as u64 * CHUNK_SIZE as u64;
                let estimated_total_size = total_chunks as u64 * CHUNK_SIZE as u64;

                if let Some(app_handle) = app_handle {
                    if let Err(e) = app_handle.emit("webrtc_download_progress", serde_json::json!({
                        "fileHash": chunk.file_hash,
                        "progress": progress_percentage,
                        "chunksReceived": chunks.len(),
                        "totalChunks": total_chunks,
                        "bytesReceived": bytes_received,
                        "totalBytes": estimated_total_size,
                    })) {
                        warn!("Failed to emit progress event: {}", e);
                    }
                }

                if chunks.len() == total_chunks as usize {
                    // Finish and remove progress bar
                    if let Some(pb) = DOWNLOAD_PROGRESS_BARS.lock().await.remove(&chunk.file_hash) {
                        pb.finish_with_message(format!("✅ Downloaded {}", &chunk.file_hash[..8]));
                    }

                    // Assemble file
                    Self::assemble_file_from_chunks(
                        &chunk.file_hash,
                        chunks,
                        file_transfer_service,
                        event_tx,
                        peer_id,
                        app_handle,
                    )
                    .await;
                }
            }
        }

        // Send ACK after releasing the lock to avoid blocking
        if let Some(dc) = dc_for_ack {
            let ack = ChunkAck {
                file_hash: chunk.file_hash.clone(),
                chunk_index: chunk.chunk_index,
                ready_for_more: true,
            };
            let ack_message = WebRTCMessage::ChunkAck(ack);
            if let Ok(ack_json) = serde_json::to_string(&ack_message) {
                if let Err(e) = dc.send_text(ack_json).await {
                    error!("❌ Failed to send ACK for chunk {}: {}", chunk.chunk_index, e);
                }
            }
        } else {
            warn!("⚠️ No data channel available to send ACK for chunk {} to peer", chunk.chunk_index);
        }
    }

    async fn assemble_file_from_chunks(
    file_hash: &str,
    chunks: &HashMap<u32, FileChunk>,
    _file_transfer_service: &Arc<FileTransferService>,
    event_tx: &mpsc::Sender<WebRTCEvent>,
    peer_id: &str,
    app_handle: Option<&tauri::AppHandle>,
    ) {
    // Sort chunks by index
    let mut sorted_chunks: Vec<_> = chunks.values().collect();
    sorted_chunks.sort_by_key(|c| c.chunk_index);

    // Get file name from the first chunk
    let raw_file_name = sorted_chunks
        .first()
        .map(|c| c.file_name.clone()) // Use file_name instead of file_hash
        .unwrap_or_else(|| format!("downloaded_{}", file_hash));

    // Ensure we only use a safe basename (avoid path traversal / separators).
    let file_name = std::path::Path::new(&raw_file_name)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("downloaded_{}", file_hash));

    // Compute final size without concatenating into a giant Vec<u8>.
    let file_size: usize = sorted_chunks.iter().map(|c| c.data.len()).sum();

    // Choose output path:
    // - If the caller (GUI / E2E API) requested a specific output_path, honor it.
    // - Otherwise, fall back to the configured download directory from settings.
    let requested_output_path: Option<std::path::PathBuf> =
        take_requested_download_output_path(file_hash)
            .await
            .map(std::path::PathBuf::from);

    let output_path: std::path::PathBuf = if let Some(p) = requested_output_path {
        // If the requested path is an existing directory, write the file inside it.
        if p.exists() && p.is_dir() {
            p.join(&file_name)
        } else {
            p
        }
    } else {
        // Resolve download directory (same single source of truth as the frontend command).
        let storage_path = match crate::download_paths::get_download_directory_opt(app_handle) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to resolve download directory: {}", e);
                return;
            }
        };

        // Ensure directory exists
        if let Err(e) = crate::download_paths::ensure_directory_exists(&storage_path).await {
            error!("Failed to ensure download directory exists ({}): {}", storage_path, e);
            return;
        }

        std::path::Path::new(&storage_path).join(&file_name)
    };

    // High-signal diagnostic: shows whether we honored a requested output path or fell back.
    if output_path.to_string_lossy().contains("chiral-e2e-downloads") {
        info!("📦 WebRTC assembling into E2E output path: {:?}", output_path);
    } else {
        info!("📦 WebRTC assembling into default output path: {:?}", output_path);
    }

    // Ensure output parent directory exists (covers requested output paths too).
    if let Some(parent) = output_path.parent() {
        let parent_str = parent.to_string_lossy().to_string();
        if let Err(e) = crate::download_paths::ensure_directory_exists(&parent_str).await {
            error!("Failed to ensure output directory exists ({}): {}", parent_str, e);
            return;
        }
    }

    // Stream chunks to disk in order (avoid IPC + JSON serialization of raw bytes).
    use tokio::io::AsyncWriteExt;
    let file = match tokio::fs::File::create(&output_path).await {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to create output file {:?}: {}", output_path, e);
            return;
        }
    };
    let mut writer = tokio::io::BufWriter::with_capacity(1024 * 1024, file); // 1MB buffer
    for chunk in &sorted_chunks {
        if let Err(e) = writer.write_all(&chunk.data).await {
            error!("Failed to write chunk {} to {:?}: {}", chunk.chunk_index, output_path, e);
            return;
        }
    }
    if let Err(e) = writer.flush().await {
        error!("Failed to flush output file {:?}: {}", output_path, e);
        return;
    }

    // NOTE: We do NOT call store_file_data here because:
    // 1. That function is for uploading/seeding files, not downloads
    // 2. It creates hash-named files + .meta files in storage
    // 3. The frontend handles saving the file with proper name via webrtc_download_complete event

    // Emit event to frontend with final output path (no huge byte array over IPC).
    if let Some(app_handle) = app_handle {
        if let Err(e) = app_handle.emit("webrtc_download_complete", serde_json::json!({
            "fileHash": file_hash,
            "fileName": file_name,
            "fileSize": file_size,
            "outputPath": output_path.to_string_lossy().to_string(),
        })) {
            error!("Failed to emit webrtc_download_complete event: {}", e);
        }
    }

    // Mapping cleanup happens via take_requested_download_output_path().

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

        // Close any existing connection to this peer first
        {
            let mut conns = self.connections.lock().await;
            if let Some(old_conn) = conns.remove(&peer_id) {
                info!("🔄 Closing existing WebRTC connection to peer {} before creating new offer", peer_id);
                if let Some(old_pc) = old_conn.peer_connection {
                    if let Err(e) = old_pc.close().await {
                        warn!("Error closing old peer connection: {}", e);
                    }
                }
                // Give some time for the old connection to fully close
                drop(conns);
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }

        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection with ICE servers for NAT traversal
        let config = create_rtc_configuration();
        let peer_connection: Arc<RTCPeerConnection> = match api.new_peer_connection(config).await {
            Ok(pc) => Arc::new(pc),
            Err(e) => {
                error!("Failed to create peer connection: {}", e);
                return Err(e.to_string());
            }
        };

        // Create data channel with configuration for larger messages
        let mut dc_config = RTCDataChannelInit::default();
        dc_config.ordered = Some(true);  // Ensure ordered delivery for file chunks

        let data_channel = match peer_connection
            .create_data_channel("file-transfer", Some(dc_config))
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
        let bandwidth_clone = self.bandwidth.clone();
        let multi_source_service_clone = self.multi_source_service.clone();
        let payment_checkpoint_clone = self.payment_checkpoint.clone();

        let app_handle_clone = self.app_handle.clone();
        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_tx = event_tx_clone.clone();
            let peer_id = peer_id_clone.clone();
            let file_transfer_service = file_transfer_service_clone.clone();
            let connections = connections_clone.clone();
            let keystore = keystore_clone.clone();
            let active_private_key = active_private_key_clone.clone();
            let bandwidth = bandwidth_clone.clone();
            let multi_source_service = multi_source_service_clone.clone();
            let payment_checkpoint = payment_checkpoint_clone.clone();

            let app_handle_for_task = app_handle_clone.clone();
            // IMPORTANT: Spawn the handler as a separate task to avoid blocking the data channel
            tokio::spawn(async move {
                Self::handle_data_channel_message(
                    &peer_id,
                    &msg,
                    &event_tx,
                    &file_transfer_service,
                    &connections,
                    &keystore,
                    &active_private_key,
                    app_handle_for_task,
                    bandwidth,
                    multi_source_service.as_ref(),
                    &payment_checkpoint,
                )
                .await;
            });
            Box::pin(async {})
        }));

        // Set up peer connection event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        // Create channel to signal ICE gathering complete
        let (ice_complete_tx, mut ice_complete_rx) = tokio::sync::mpsc::channel::<()>(1);

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();
            let ice_complete_tx = ice_complete_tx.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    info!("[ICE] ICE candidate generated for peer {}: {}", peer_id, candidate.address);
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
                } else {
                    info!("✅ ICE gathering complete for peer {}", peer_id);
                    let _ = ice_complete_tx.send(()).await;
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
                        RTCPeerConnectionState::Failed => {
                            error!("WebRTC connection failed for peer: {}", peer_id);
                        }
                        RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Closed => {
                            info!("WebRTC connection closed with peer: {}", peer_id);
                        }
                        _ => {
                            info!("WebRTC peer connection state: {:?} for peer: {}", state, peer_id);
                        }
                    }
                })
            },
        ));

        // Add ICE connection state handler for debugging NAT traversal issues
        let peer_id_for_ice_state = peer_id.to_string();
        peer_connection.on_ice_connection_state_change(Box::new(
            move |state: RTCIceConnectionState| {
                let peer_id = peer_id_for_ice_state.clone();
                Box::pin(async move {
                    match state {
                        RTCIceConnectionState::Checking => {
                            info!("ICE: Checking connectivity for peer: {}", peer_id);
                        }
                        RTCIceConnectionState::Connected => {
                            info!("ICE: Connected to peer: {} - NAT traversal successful!", peer_id);
                        }
                        RTCIceConnectionState::Completed => {
                            info!("ICE: Completed for peer: {} - All candidates checked", peer_id);
                        }
                        RTCIceConnectionState::Failed => {
                            error!("ICE: Failed for peer: {} - NAT traversal failed, TURN may not be working", peer_id);
                        }
                        RTCIceConnectionState::Disconnected => {
                            warn!("ICE: Disconnected from peer: {}", peer_id);
                        }
                        RTCIceConnectionState::Closed => {
                            info!("ICE: Closed for peer: {}", peer_id);
                        }
                        _ => {
                            debug!("ICE: State {:?} for peer: {}", state, peer_id);
                        }
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

        // Wait for ICE gathering to complete (with timeout)
        info!("⏳ Waiting for ICE gathering to complete for peer {}...", peer_id);
        let ice_timeout = tokio::time::Duration::from_secs(10);
        match tokio::time::timeout(ice_timeout, ice_complete_rx.recv()).await {
            Ok(Some(())) => {
                info!("✅ ICE gathering completed successfully for peer {}", peer_id);
            }
            Ok(None) => {
                warn!("ICE gathering channel closed unexpectedly for peer {}", peer_id);
            }
            Err(_) => {
                warn!("⚠️  ICE gathering timeout ({}s) for peer {}, proceeding anyway", ice_timeout.as_secs(), peer_id);
            }
        }

        // Store connection with retry context (this is an outbound connection)
        let mut conns = self.connections.lock().await;
        let retry_ctx = WebRtcRetryContext::new(peer_id.clone(), true);
        
        let connection = PeerConnection {
            peer_id: peer_id.clone(),
            is_connected: false,
            active_transfers: HashMap::new(),
            last_activity: Instant::now(),
            peer_connection: Some(peer_connection.clone()),
            data_channel: Some(data_channel),
            pending_chunks: HashMap::new(),
            received_chunks: HashMap::new(),
            acked_chunks: HashMap::new(),
            pending_acks: HashMap::new(),
            retry_context: Some(retry_ctx),
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
        // Check if the answer is an error message from the seeder
        if answer.starts_with("error:") {
            if answer.contains("webrtc-service-unavailable") {
                return Err("Seeder does not have WebRTC service enabled. Please try using Bitswap protocol instead.".to_string());
            }
            return Err(format!("Seeder returned error: {}", answer));
        }

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
        // Close any existing connection to this peer first
        {
            let mut conns = self.connections.lock().await;
            if let Some(old_conn) = conns.remove(&peer_id) {
                info!("🔄 Closing existing WebRTC connection to peer {} before establishing new one", peer_id);
                if let Some(old_pc) = old_conn.peer_connection {
                    if let Err(e) = old_pc.close().await {
                        warn!("Error closing old peer connection: {}", e);
                    }
                }
                // Give some time for the old connection to fully close
                drop(conns);
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }

        // Create WebRTC API
        let api = APIBuilder::new().build();

        // Create peer connection with ICE servers for NAT traversal
        let config = create_rtc_configuration();
        let peer_connection: Arc<RTCPeerConnection> = match api.new_peer_connection(config).await {
            Ok(pc) => Arc::new(pc),
            Err(e) => {
                error!("Failed to create peer connection: {}", e);
                return Err(e.to_string());
            }
        };

        // Answerer should NOT create data channel - it will receive it via on_data_channel
        // Set up handler to receive data channel from offerer
        let event_tx_for_dc = self.event_tx.clone();
        let peer_id_for_dc = peer_id.clone();
        let file_transfer_service_for_dc = self.file_transfer_service.clone();
        let connections_for_dc = self.connections.clone();
        let keystore_for_dc = self.keystore.clone();
        let active_private_key_for_dc = self.active_private_key.clone();
        let bandwidth_for_dc = self.bandwidth.clone();
        let app_handle_for_dc = self.app_handle.clone();
        let multi_source_service_for_dc = self.multi_source_service.clone();
        let payment_checkpoint_for_dc = self.payment_checkpoint.clone();

        info!("Setting up on_data_channel callback for peer: {}", peer_id);

        peer_connection.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
            info!("✅ CALLBACK FIRED! Received data channel from offerer: {}", data_channel.label());

            let event_tx = event_tx_for_dc.clone();
            let peer_id = peer_id_for_dc.clone();
            let file_transfer_service = file_transfer_service_for_dc.clone();
            let connections = connections_for_dc.clone();
            let keystore = keystore_for_dc.clone();
            let active_private_key = active_private_key_for_dc.clone();
            let bandwidth = bandwidth_for_dc.clone();
            let app_handle = app_handle_for_dc.clone();
            let multi_source_service = multi_source_service_for_dc.clone();
            let payment_checkpoint = payment_checkpoint_for_dc.clone();

            // Set up message handler for received data channel
            // IMPORTANT: Spawn the handler as a separate task to avoid blocking the data channel
            data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
                let event_tx = event_tx.clone();
                let peer_id = peer_id.clone();
                let file_transfer_service = file_transfer_service.clone();
                let connections = connections.clone();
                let keystore = keystore.clone();
                let active_private_key = active_private_key.clone();
                let bandwidth = bandwidth.clone();
                let app_handle_for_task = app_handle.clone();
                let multi_source_service = multi_source_service.clone();
                let payment_checkpoint = payment_checkpoint.clone();

                tokio::spawn(async move {
                    Self::handle_data_channel_message(
                        &peer_id,
                        &msg,
                        &event_tx,
                        &file_transfer_service,
                        &connections,
                        &keystore,
                        &active_private_key,
                        app_handle_for_task,
                        bandwidth,
                        multi_source_service.as_ref(),
                        &payment_checkpoint,
                    )
                    .await;
                });
                Box::pin(async {})
            }));

            // Store data channel in connections
            let connections_clone = connections_for_dc.clone();
            let peer_id_clone = peer_id_for_dc.clone();
            let data_channel_clone = data_channel.clone();

            tokio::spawn(async move {
                info!("🔍 Attempting to store data channel for peer {}", peer_id_clone);
                let mut conns = connections_clone.lock().await;
                if let Some(connection) = conns.get_mut(&peer_id_clone) {
                    connection.data_channel = Some(data_channel_clone);
                    info!("✅ Successfully stored received data channel for peer {}", peer_id_clone);
                } else {
                    error!("❌ FAILED to store data channel - peer {} not found in connections map!", peer_id_clone);
                }
            });

            Box::pin(async {})
        }));

        // Set up peer connection event handlers
        let event_tx_clone = self.event_tx.clone();
        let peer_id_clone = peer_id.clone();

        let event_tx_for_ice = event_tx_clone.clone();
        let peer_id_for_ice = peer_id_clone.clone();

        // Create channel to signal ICE gathering complete
        let (ice_complete_tx, mut ice_complete_rx) = tokio::sync::mpsc::channel::<()>(1);

        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let event_tx = event_tx_for_ice.clone();
            let peer_id = peer_id_for_ice.clone();
            let ice_complete_tx = ice_complete_tx.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    info!("[ICE] ICE candidate generated for peer {}: {}", peer_id, candidate.address);
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
                } else {
                    info!("✅ ICE gathering complete for peer {}", peer_id);
                    let _ = ice_complete_tx.send(()).await;
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
                        RTCPeerConnectionState::Failed => {
                            error!("WebRTC connection failed for peer: {}", peer_id);
                        }
                        RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Closed => {
                            info!("WebRTC connection closed with peer: {}", peer_id);
                        }
                        _ => {
                            info!("WebRTC peer connection state: {:?} for peer: {}", state, peer_id);
                        }
                    }
                })
            },
        ));

        // Add ICE connection state handler for debugging NAT traversal issues
        let peer_id_for_ice_state = peer_id.to_string();
        peer_connection.on_ice_connection_state_change(Box::new(
            move |state: RTCIceConnectionState| {
                let peer_id = peer_id_for_ice_state.clone();
                Box::pin(async move {
                    match state {
                        RTCIceConnectionState::Checking => {
                            info!("ICE: Checking connectivity for peer: {}", peer_id);
                        }
                        RTCIceConnectionState::Connected => {
                            info!("ICE: Connected to peer: {} - NAT traversal successful!", peer_id);
                        }
                        RTCIceConnectionState::Completed => {
                            info!("ICE: Completed for peer: {} - All candidates checked", peer_id);
                        }
                        RTCIceConnectionState::Failed => {
                            error!("ICE: Failed for peer: {} - NAT traversal failed, TURN may not be working", peer_id);
                        }
                        RTCIceConnectionState::Disconnected => {
                            warn!("ICE: Disconnected from peer: {}", peer_id);
                        }
                        RTCIceConnectionState::Closed => {
                            info!("ICE: Closed for peer: {}", peer_id);
                        }
                        _ => {
                            debug!("ICE: State {:?} for peer: {}", state, peer_id);
                        }
                    }
                })
            },
        ));

        // Store connection BEFORE set_remote_description so on_data_channel callback can find it
        // (data_channel will be set via on_data_channel callback when it fires during set_remote_description)
        info!("Storing peer connection in map BEFORE set_remote_description for peer: {}", peer_id);
        let mut conns = self.connections.lock().await;
        let mut retry_ctx = WebRtcRetryContext::new(peer_id.clone(), false);
        retry_ctx.last_offer = Some(offer.clone());
        
        let connection = PeerConnection {
            peer_id: peer_id.clone(),
            is_connected: false, // Will be set to true when connected
            active_transfers: HashMap::new(),
            last_activity: Instant::now(),
            peer_connection: Some(peer_connection.clone()),
            data_channel: None, // Will be set when received via on_data_channel
            pending_chunks: HashMap::new(),
            received_chunks: HashMap::new(),
            acked_chunks: HashMap::new(),
            pending_acks: HashMap::new(),
            retry_context: Some(retry_ctx),
        };
        conns.insert(peer_id.clone(), connection);
        info!("✅ Peer {} stored in connections map, now calling set_remote_description", peer_id);
        drop(conns); // Release lock before calling set_remote_description

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

        // Wait for ICE gathering to complete (with timeout)
        info!("⏳ Waiting for ICE gathering to complete for peer {}...", peer_id);
        let ice_timeout = tokio::time::Duration::from_secs(10);
        match tokio::time::timeout(ice_timeout, ice_complete_rx.recv()).await {
            Ok(Some(())) => {
                info!("✅ ICE gathering completed successfully for peer {}", peer_id);
            }
            Ok(None) => {
                warn!("ICE gathering channel closed unexpectedly for peer {}", peer_id);
            }
            Err(_) => {
                warn!("⚠️  ICE gathering timeout ({}s) for peer {}, proceeding anyway", ice_timeout.as_secs(), peer_id);
            }
        }

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

    /// Check if there's an existing open WebRTC connection with data channel to a peer
    pub async fn has_open_connection(&self, peer_id: &str) -> bool {
        use webrtc::data_channel::data_channel_state::RTCDataChannelState;
        
        let conns = self.connections.lock().await;
        if let Some(conn) = conns.get(peer_id) {
            if let Some(dc) = &conn.data_channel {
                return dc.ready_state() == RTCDataChannelState::Open;
            }
        }
        false
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

/// Set the global WebRTC service to an existing instance.
/// This should be called after creating the WebRTCService to share the same instance globally.
pub async fn set_webrtc_service(service: Arc<WebRTCService>) {
    let mut global_service = WEBRTC_SERVICE.lock().await;
    *global_service = Some(service);
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