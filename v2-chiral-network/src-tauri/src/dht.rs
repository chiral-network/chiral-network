use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use crate::speed_tiers::SpeedTier;
use libp2p::{
    kad, mdns, noise, ping, relay, dcutr,
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
use std::path::PathBuf;
use sha2::{Sha256, Digest};
use std::io::{Read as _, Seek as _, SeekFrom};

/// Custom CBOR codec with appropriate size limits for chunked file transfers.
/// Individual chunks are 256 KB, but FileInfo responses can be large for big files
/// (chunk hashes array). 32 MB covers files up to ~100 GB.
mod cbor_codec {
    use async_trait::async_trait;
    use cbor4ii::core::error::DecodeError;
    use futures::prelude::*;
    use libp2p::request_response;
    use libp2p::swarm::StreamProtocol;
    use serde::{de::DeserializeOwned, Serialize};
    use std::{collections::TryReserveError, convert::Infallible, io, marker::PhantomData};

    /// Max request size: 1 MB (requests are small metadata)
    const REQUEST_SIZE_MAXIMUM: u64 = 1 * 1024 * 1024;
    /// Max response size: 32 MB (FileInfo with chunk hashes for large files)
    const RESPONSE_SIZE_MAXIMUM: u64 = 32 * 1024 * 1024;

    pub type Behaviour<Req, Resp> = request_response::Behaviour<Codec<Req, Resp>>;

    pub struct Codec<Req, Resp> {
        phantom: PhantomData<(Req, Resp)>,
    }

    impl<Req, Resp> Default for Codec<Req, Resp> {
        fn default() -> Self {
            Codec {
                phantom: PhantomData,
            }
        }
    }

    impl<Req, Resp> Clone for Codec<Req, Resp> {
        fn clone(&self) -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl<Req, Resp> request_response::Codec for Codec<Req, Resp>
    where
        Req: Send + Serialize + DeserializeOwned,
        Resp: Send + Serialize + DeserializeOwned,
    {
        type Protocol = StreamProtocol;
        type Request = Req;
        type Response = Resp;

        async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Req>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut vec = Vec::new();
            io.take(REQUEST_SIZE_MAXIMUM).read_to_end(&mut vec).await?;
            cbor4ii::serde::from_slice(vec.as_slice()).map_err(decode_into_io_error)
        }

        async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Resp>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut vec = Vec::new();
            io.take(RESPONSE_SIZE_MAXIMUM).read_to_end(&mut vec).await?;
            cbor4ii::serde::from_slice(vec.as_slice()).map_err(decode_into_io_error)
        }

        async fn write_request<T>(
            &mut self,
            _: &Self::Protocol,
            io: &mut T,
            req: Self::Request,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let data: Vec<u8> =
                cbor4ii::serde::to_vec(Vec::new(), &req).map_err(encode_into_io_error)?;
            io.write_all(data.as_ref()).await?;
            Ok(())
        }

        async fn write_response<T>(
            &mut self,
            _: &Self::Protocol,
            io: &mut T,
            resp: Self::Response,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let data: Vec<u8> =
                cbor4ii::serde::to_vec(Vec::new(), &resp).map_err(encode_into_io_error)?;
            io.write_all(data.as_ref()).await?;
            Ok(())
        }
    }

    fn decode_into_io_error(err: cbor4ii::serde::DecodeError<Infallible>) -> io::Error {
        match err {
            cbor4ii::serde::DecodeError::Core(DecodeError::Read(e)) => {
                io::Error::new(io::ErrorKind::Other, e)
            }
            cbor4ii::serde::DecodeError::Core(e @ DecodeError::Unsupported { .. }) => {
                io::Error::new(io::ErrorKind::Unsupported, e)
            }
            cbor4ii::serde::DecodeError::Core(e @ DecodeError::Eof { .. }) => {
                io::Error::new(io::ErrorKind::UnexpectedEof, e)
            }
            cbor4ii::serde::DecodeError::Core(e) => io::Error::new(io::ErrorKind::InvalidData, e),
            cbor4ii::serde::DecodeError::Custom(e) => {
                io::Error::new(io::ErrorKind::Other, e.to_string())
            }
        }
    }

    fn encode_into_io_error(err: cbor4ii::serde::EncodeError<TryReserveError>) -> io::Error {
        io::Error::new(io::ErrorKind::Other, err)
    }
}

/// Get bootstrap nodes for the Chiral Network DHT
pub fn get_bootstrap_nodes() -> Vec<String> {
    vec![
        // Primary bootstrap node with relay server (IPv4 + IPv6)
        "/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWKuwDRp7DWzPYNNgqihvcSy9C7yECFH3HVnERdFtrVfzE".to_string(),
        "/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWKuwDRp7DWzPYNNgqihvcSy9C7yECFH3HVnERdFtrVfzE".to_string(),
        // Additional bootstrap node
        "/ip4/134.199.240.145/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE".to_string(),
    ]
}

/// Get unique peer IDs of all bootstrap nodes
pub fn get_bootstrap_peer_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for addr_str in get_bootstrap_nodes() {
        if let Ok(addr) = addr_str.parse::<Multiaddr>() {
            if let Some(peer_id) = extract_peer_id_from_multiaddr(&addr) {
                let id_str = peer_id.to_string();
                if !ids.contains(&id_str) {
                    ids.push(id_str);
                }
            }
        }
    }
    ids
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

// File transfer protocol messages (for direct file push / ChiralDrop)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferRequest {
    pub transfer_id: String,
    pub file_name: String,
    pub file_data: Vec<u8>,
    /// Price in wei (as string for CBOR safety). "0" or empty = free.
    #[serde(default)]
    pub price_wei: String,
    /// Sender's wallet address for receiving payment.
    #[serde(default)]
    pub sender_wallet: String,
    /// SHA-256 hash of the file (needed for paid downloads via chunked protocol).
    #[serde(default)]
    pub file_hash: String,
    /// Actual file size in bytes (for paid transfers where file_data is empty).
    #[serde(default)]
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferResponse {
    pub transfer_id: String,
    pub accepted: bool,
    pub error: Option<String>,
}

/// Chunk size for file transfers: 256 KB
const CHUNK_SIZE: usize = 256 * 1024;
/// Maximum retries per chunk before aborting
const MAX_CHUNK_RETRIES: u8 = 3;

// Chunked file request protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChunkRequest {
    /// Ask the seeder for file metadata and chunk manifest
    FileInfo {
        request_id: String,
        file_hash: String,
    },
    /// Ask for a specific chunk by index
    Chunk {
        request_id: String,
        file_hash: String,
        chunk_index: u32,
    },
    /// Send proof of payment to seeder
    PaymentProof {
        request_id: String,
        file_hash: String,
        payment_tx: String,
        payer_address: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChunkResponse {
    /// File metadata with per-chunk SHA-256 hashes
    FileInfo {
        request_id: String,
        file_hash: String,
        file_name: String,
        file_size: u64,
        chunk_size: u32,
        total_chunks: u32,
        chunk_hashes: Vec<String>,
        /// Price in wei as string (u128 as string for CBOR safety)
        price_wei: String,
        /// Seeder's wallet address for payment
        wallet_address: String,
        error: Option<String>,
    },
    /// A single chunk of file data
    Chunk {
        request_id: String,
        file_hash: String,
        chunk_index: u32,
        chunk_data: Option<Vec<u8>>,
        chunk_hash: String,
        error: Option<String>,
    },
    /// Acknowledgement of payment verification
    PaymentAck {
        request_id: String,
        file_hash: String,
        accepted: bool,
        error: Option<String>,
    },
}

enum SwarmCommand {
    SendPing(PeerId),
    SendFile {
        peer_id: PeerId,
        transfer_id: String,
        file_name: String,
        file_data: Vec<u8>,
        price_wei: String,
        sender_wallet: String,
        file_hash: String,
        file_size: u64,
    },
    RequestFileInfo {
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
    relay_client: relay::client::Behaviour,
    dcutr: dcutr::Behaviour,
    kad: kad::Behaviour<kad::store::MemoryStore>,
    mdns: mdns::tokio::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    ping_protocol: request_response::cbor::Behaviour<PingRequest, PingResponse>,
    file_transfer: cbor_codec::Behaviour<FileTransferRequest, FileTransferResponse>,
    file_request: cbor_codec::Behaviour<ChunkRequest, ChunkResponse>,
}

/// Map of file hash -> file path for files we're seeding
pub type SharedFilesMap = Arc<Mutex<std::collections::HashMap<String, SharedFileInfo>>>;

#[derive(Clone, Debug)]
pub struct SharedFileInfo {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    /// Cached chunk hashes, computed lazily on first FileInfo request
    pub chunk_hashes: Option<Vec<String>>,
    /// Price in wei (0 = free)
    pub price_wei: u128,
    /// Seeder's wallet address for receiving payment
    pub wallet_address: String,
}

/// Map of request_id -> SpeedTier for rate-limited downloads
pub type DownloadTiersMap = Arc<Mutex<HashMap<String, SpeedTier>>>;

/// Shared reference to the custom download directory setting
pub type DownloadDirectoryRef = Arc<Mutex<Option<String>>>;

/// Credentials for sending payment during download (wallet address + private key)
#[derive(Clone, Debug)]
pub struct DownloadCredentials {
    pub wallet_address: String,
    pub private_key: String,
}

/// Map of request_id -> download credentials for payment during chunked transfer
pub type DownloadCredentialsMap = Arc<Mutex<HashMap<String, DownloadCredentials>>>;

/// Tracks an in-progress chunked download on the downloader side
struct ActiveChunkedDownload {
    request_id: String,
    file_hash: String,
    file_name: String,
    file_size: u64,
    total_chunks: u32,
    chunk_hashes: Vec<String>,
    received_chunks: Vec<bool>,
    output_path: PathBuf,
    bytes_written: u64,
    peer_id: PeerId,
    tier: SpeedTier,
    retry_counts: Vec<u8>,
    current_chunk_index: u32,
    start_time: std::time::Instant,
    /// Whether payment has been confirmed by seeder
    payment_confirmed: bool,
}

/// Map of request_id -> active chunked download state
type ActiveDownloadsMap = HashMap<String, ActiveChunkedDownload>;

/// Compute SHA-256 hashes for each chunk of a file
fn compute_chunk_hashes(file_path: &str) -> Result<Vec<String>, String> {
    let mut file = std::fs::File::open(file_path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    let metadata = file.metadata()
        .map_err(|e| format!("Failed to get file metadata: {}", e))?;
    let file_size = metadata.len();
    let total_chunks = ((file_size as usize + CHUNK_SIZE - 1) / CHUNK_SIZE) as u32;
    let mut hashes = Vec::with_capacity(total_chunks as usize);

    let mut buf = vec![0u8; CHUNK_SIZE];
    for _ in 0..total_chunks {
        let bytes_read = file.read(&mut buf)
            .map_err(|e| format!("Failed to read chunk: {}", e))?;
        let mut hasher = Sha256::new();
        hasher.update(&buf[..bytes_read]);
        hashes.push(hex::encode(hasher.finalize()));
    }

    Ok(hashes)
}

pub struct DhtService {
    peers: Arc<Mutex<Vec<PeerInfo>>>,
    is_running: Arc<Mutex<bool>>,
    local_peer_id: Arc<Mutex<Option<String>>>,
    command_sender: Arc<Mutex<Option<mpsc::UnboundedSender<SwarmCommand>>>>,
    file_transfer_service: Option<Arc<Mutex<crate::file_transfer::FileTransferService>>>,
    shared_files: SharedFilesMap,
    download_tiers: DownloadTiersMap,
    download_directory: DownloadDirectoryRef,
    active_downloads: Arc<Mutex<ActiveDownloadsMap>>,
    download_credentials: DownloadCredentialsMap,
}

impl DhtService {
    pub fn new(
        file_transfer_service: Arc<Mutex<crate::file_transfer::FileTransferService>>,
        download_tiers: DownloadTiersMap,
        download_directory: DownloadDirectoryRef,
        download_credentials: DownloadCredentialsMap,
    ) -> Self {
        Self {
            peers: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
            local_peer_id: Arc::new(Mutex::new(None)),
            command_sender: Arc::new(Mutex::new(None)),
            file_transfer_service: Some(file_transfer_service),
            shared_files: Arc::new(Mutex::new(std::collections::HashMap::new())),
            download_tiers,
            download_directory,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            download_credentials,
        }
    }

    /// Register a file for sharing (seeding)
    pub async fn register_shared_file(&self, file_hash: String, file_path: String, file_name: String, file_size: u64, price_wei: u128, wallet_address: String) {
        let mut shared = self.shared_files.lock().await;
        println!("=== REGISTERING SHARED FILE ===");
        println!("  Name: {}", file_name);
        println!("  Hash: {}", file_hash);
        println!("  Path: {}", file_path);
        println!("  Size: {} bytes", file_size);
        if price_wei > 0 {
            println!("  Price: {} wei", price_wei);
            println!("  Wallet: {}", wallet_address);
        } else {
            println!("  Price: Free");
        }
        shared.insert(file_hash.clone(), SharedFileInfo {
            file_path,
            file_name,
            file_size,
            chunk_hashes: None,
            price_wei,
            wallet_address,
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
        let download_dir_clone = self.download_directory.clone();
        let active_downloads_clone = self.active_downloads.clone();
        let download_credentials_clone = self.download_credentials.clone();

        tokio::spawn(async move {
            event_loop(swarm, peers_clone, is_running_clone, app, cmd_rx, file_transfer_clone, shared_files_clone, download_tiers_clone, download_dir_clone, active_downloads_clone, download_credentials_clone).await;
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
        price_wei: String,
        sender_wallet: String,
        file_hash: String,
        file_size: u64,
    ) -> Result<(), String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let peer_id_parsed = PeerId::from_str(&peer_id).map_err(|e| e.to_string())?;
            tx.send(SwarmCommand::SendFile {
                peer_id: peer_id_parsed,
                transfer_id,
                file_name,
                file_data,
                price_wei,
                sender_wallet,
                file_hash,
                file_size,
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

    /// Request a file from a remote peer by hash (initiates chunked transfer)
    pub async fn request_file(&self, peer_id: String, file_hash: String, request_id: String) -> Result<(), String> {
        let sender = self.command_sender.lock().await;
        if let Some(tx) = sender.as_ref() {
            let peer_id_parsed = PeerId::from_str(&peer_id).map_err(|e| e.to_string())?;
            tx.send(SwarmCommand::RequestFileInfo {
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

/// Verify a payment transaction on-chain.
/// Checks that the transaction was sent to the expected recipient with at least the expected value.
async fn verify_payment_on_chain(
    tx_hash: &str,
    expected_recipient: &str,
    expected_min_wei: u128,
) -> Result<bool, String> {
    let rpc_url = crate::geth::rpc_endpoint();
    let client = reqwest::Client::new();

    // Get transaction by hash
    let tx_resp = client
        .post(&rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionByHash",
            "params": [tx_hash],
            "id": 1
        }))
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let tx_json: serde_json::Value = tx_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    let tx = tx_json.get("result").ok_or("Transaction not found")?;
    if tx.is_null() {
        return Err("Transaction not found on-chain".to_string());
    }

    // Check recipient
    let to = tx.get("to")
        .and_then(|v| v.as_str())
        .ok_or("Transaction has no recipient")?;

    if to.to_lowercase() != expected_recipient.to_lowercase() {
        return Ok(false);
    }

    // Check value
    let value_hex = tx.get("value")
        .and_then(|v| v.as_str())
        .ok_or("Transaction has no value")?;
    let value_hex = value_hex.strip_prefix("0x").unwrap_or(value_hex);
    let value = u128::from_str_radix(value_hex, 16)
        .map_err(|e| format!("Failed to parse value: {}", e))?;

    if value < expected_min_wei {
        return Ok(false);
    }

    // Verify transaction receipt (confirmed) — retry up to 30 times (30 seconds)
    // since the transaction may not be mined immediately
    let max_retries = 30;
    for attempt in 0..max_retries {
        let receipt_resp = client
            .post(&rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getTransactionReceipt",
                "params": [tx_hash],
                "id": 2
            }))
            .send()
            .await
            .map_err(|e| format!("RPC receipt request failed: {}", e))?;

        let receipt_json: serde_json::Value = receipt_resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse receipt: {}", e))?;

        let receipt = receipt_json.get("result");
        if receipt.is_some() && !receipt.unwrap().is_null() {
            let status = receipt.unwrap().get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("0x0");
            return Ok(status == "0x1");
        }

        if attempt < max_retries - 1 {
            println!("⏳ Payment tx {} not confirmed yet, retrying ({}/{})", tx_hash, attempt + 1, max_retries);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    Err("Transaction not confirmed after 30 seconds".to_string())
}

async fn create_swarm() -> Result<(Swarm<DhtBehaviour>, String), Box<dyn Error>> {
    let local_key = libp2p::identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    println!("Local peer ID: {}", local_peer_id);

    let kad_store = kad::store::MemoryStore::new(local_peer_id);
    // Use custom Kademlia protocol name to match v1 bootstrap nodes
    let mut kad_config = kad::Config::default();
    kad_config.set_protocol_names(vec![StreamProtocol::new("/chiral/kad/1.0.0")]);
    let mut kad = kad::Behaviour::with_config(local_peer_id, kad_store, kad_config);
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
    let ping = ping::Behaviour::new(
        ping::Config::new()
            .with_interval(std::time::Duration::from_secs(15))
    );

    let identify_config = identify::Config::new(
        "/chiral/id/1.0.0".to_string(),
        local_key.public(),
    );
    let identify = identify::Behaviour::new(identify_config);

    let ping_protocol = request_response::cbor::Behaviour::new(
        [(StreamProtocol::new("/chiral/ping/1.0.0"), request_response::ProtocolSupport::Full)],
        request_response::Config::default(),
    );

    let file_transfer = cbor_codec::Behaviour::new(
        [(StreamProtocol::new("/chiral/file-transfer/1.0.0"), request_response::ProtocolSupport::Full)],
        request_response::Config::default(),
    );

    let file_request = cbor_codec::Behaviour::new(
        [(StreamProtocol::new("/chiral/file-request/3.0.0"), request_response::ProtocolSupport::Full)],
        request_response::Config::default(),
    );

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_relay_client(
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_key, relay_client| {
            let dcutr = dcutr::Behaviour::new(local_peer_id);
            DhtBehaviour {
                relay_client,
                dcutr,
                kad,
                mdns,
                ping,
                identify,
                ping_protocol,
                file_transfer,
                file_request,
            }
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(3600)))
        .build();

    // Listen on all interfaces (dual-stack: IPv4 + IPv6)
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    swarm.listen_on("/ip6/::/tcp/0".parse()?)?;

    // Request relay reservations via listen_on. Do NOT call swarm.dial() or
    // kad.bootstrap() here — the swarm polls behaviours BEFORE the transport,
    // so Kademlia's bootstrap query would race and dial the relay server before
    // the relay client transport has delivered the ListenReq to the behaviour.
    // Kademlia's connection_id wouldn't match the relay's pending_handler_commands,
    // so the RESERVE command would be lost. Instead, kad.bootstrap() is deferred
    // to the event loop and called after the first relay reservation is confirmed.
    for addr_str in get_bootstrap_nodes() {
        if let Ok(addr) = addr_str.parse::<Multiaddr>() {
            let relay_addr = addr.with(libp2p::multiaddr::Protocol::P2pCircuit);
            match swarm.listen_on(relay_addr.clone()) {
                Ok(id) => println!("✅ Relay listen requested: {} (listener {:?})", relay_addr, id),
                Err(e) => println!("❌ Relay listen failed for {}: {:?}", relay_addr, e),
            }
        }
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
    download_directory: DownloadDirectoryRef,
    active_downloads: Arc<Mutex<ActiveDownloadsMap>>,
    download_credentials: DownloadCredentialsMap,
) {
    // Track pending get queries
    let mut pending_get_queries: HashMap<kad::QueryId, tokio::sync::oneshot::Sender<Result<Option<String>, String>>> = HashMap::new();
    // Map libp2p OutboundRequestId -> our request_id for failure correlation
    let mut outbound_request_map: HashMap<request_response::OutboundRequestId, String> = HashMap::new();
    // Kademlia bootstrap is deferred until after the first relay reservation is confirmed,
    // to prevent Kademlia from racing the relay client for the bootstrap connection.
    let mut kad_bootstrapped = false;
    
    loop {
        let running = *is_running.lock().await;
        if !running {
            break;
        }
        
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(event) => {
                        handle_behaviour_event(event, &peers, &app, &mut swarm, &file_transfer_service, &mut pending_get_queries, &shared_files, &download_tiers, &download_directory, &active_downloads, &mut outbound_request_map, &download_credentials).await;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        // Check if this is a relay circuit address
                        let is_relay = address.iter().any(|p| matches!(p, libp2p::multiaddr::Protocol::P2pCircuit));
                        if is_relay {
                            println!("✅ Relay reservation confirmed! Listening on relay: {}", address);
                            // Add as external address so Identify advertises it to other peers
                            swarm.add_external_address(address.clone());

                            // Now that relay is established, bootstrap Kademlia (deferred from startup
                            // to prevent Kademlia from racing the relay client for the bootstrap connection)
                            if !kad_bootstrapped {
                                kad_bootstrapped = true;
                                if let Err(e) = swarm.behaviour_mut().kad.bootstrap() {
                                    println!("Kademlia bootstrap error: {:?}", e);
                                } else {
                                    println!("✅ Kademlia bootstrap triggered after relay reservation");
                                }
                            }
                        } else {
                            println!("Listening on {:?}", address);
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        println!("Connection established with {:?}", peer_id);

                        // Relay reservations are requested at startup (before dialing bootstrap)
                        // so the relay client transport creates handlers with STOP protocol support.
                        // No need to call listen_on here - just log bootstrap connections.
                        let bootstrap_peer_ids = get_bootstrap_peer_ids();
                        if bootstrap_peer_ids.contains(&peer_id.to_string()) {
                            println!("✅ Connected to bootstrap/relay node: {}", peer_id);
                        }

                        // Add to peers list so ChiralDrop and Network page can see them
                        let peer_id_str = peer_id.to_string();
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64;
                        let addr_str = endpoint.get_remote_address().to_string();

                        {
                            let mut peers_guard = peers.lock().await;
                            if let Some(existing) = peers_guard.iter_mut().find(|p| p.id == peer_id_str) {
                                existing.last_seen = now;
                                if !existing.multiaddrs.contains(&addr_str) {
                                    existing.multiaddrs.push(addr_str);
                                }
                            } else {
                                peers_guard.push(PeerInfo {
                                    id: peer_id_str.clone(),
                                    address: peer_id_str.clone(),
                                    multiaddrs: vec![addr_str],
                                    last_seen: now,
                                });
                            }
                            let _ = app.emit("peer-discovered", peers_guard.clone());
                        }

                        let _ = app.emit("connection-established", peer_id_str);

                        // If this is an incoming connection, notify that we're being pinged
                        if endpoint.is_listener() {
                            println!("Incoming connection from {}", peer_id);
                            let _ = app.emit("ping-received", peer_id.to_string());
                        }
                    }
                    SwarmEvent::ConnectionClosed { peer_id, num_established, .. } => {
                        // Only remove when all connections to this peer are gone
                        if num_established == 0 {
                            let peer_id_str = peer_id.to_string();
                            println!("All connections closed with {}", peer_id_str);
                            let mut peers_guard = peers.lock().await;
                            peers_guard.retain(|p| p.id != peer_id_str);
                            let _ = app.emit("peer-discovered", peers_guard.clone());
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        if let Some(peer) = peer_id {
                            println!("Failed to connect to {:?}: {:?}", peer, error);
                        }
                    }
                    SwarmEvent::IncomingConnection { local_addr, send_back_addr, .. } => {
                        println!("📥 Incoming connection: local={}, remote={}", local_addr, send_back_addr);
                    }
                    SwarmEvent::IncomingConnectionError { local_addr, send_back_addr, error, .. } => {
                        println!("❌ Incoming connection error: local={}, remote={}, error={:?}", local_addr, send_back_addr, error);
                    }
                    SwarmEvent::ListenerClosed { listener_id, reason, addresses, .. } => {
                        println!("⚠️ Listener {:?} closed (addrs: {:?}): {:?}", listener_id, addresses, reason);
                    }
                    SwarmEvent::ListenerError { listener_id, error, .. } => {
                        println!("❌ Listener {:?} error: {:?}", listener_id, error);
                    }
                    SwarmEvent::ExternalAddrConfirmed { address, .. } => {
                        println!("✅ External address confirmed: {}", address);
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
                    SwarmCommand::SendFile { peer_id, transfer_id, file_name, file_data, price_wei, sender_wallet, file_hash, file_size } => {
                        println!("Sending file '{}' to peer {} (price: {} wei, size: {} bytes)", file_name, peer_id, price_wei, file_size);
                        let request = FileTransferRequest {
                            transfer_id: transfer_id.clone(),
                            file_name: file_name.clone(),
                            file_size,
                            file_data,
                            price_wei,
                            sender_wallet,
                            file_hash,
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
                            "/chiral/file-request/3.0.0".to_string(),
                            "/chiral/kad/1.0.0".to_string(),
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
                    SwarmCommand::RequestFileInfo { peer_id, request_id, file_hash } => {
                        println!("Requesting file info for {} from peer {}", file_hash, peer_id);

                        // Check if peer is actually connected before sending request
                        if !swarm.is_connected(&peer_id) {
                            println!("⚠️ Peer {} is not connected, attempting to dial...", peer_id);
                            let mut dialed = false;

                            // First try direct dial (works if we have the peer's address)
                            match swarm.dial(peer_id) {
                                Ok(_) => {
                                    println!("📡 Dialing peer {} directly...", peer_id);
                                    dialed = true;
                                }
                                Err(e) => {
                                    println!("Direct dial failed for {}: {:?}, trying relay...", peer_id, e);
                                }
                            }

                            // If direct dial failed, try via relay through each bootstrap node
                            if !dialed {
                                for bootstrap_addr_str in get_bootstrap_nodes() {
                                    if let Ok(bootstrap_addr) = bootstrap_addr_str.parse::<Multiaddr>() {
                                        let relay_addr = bootstrap_addr
                                            .with(libp2p::multiaddr::Protocol::P2pCircuit)
                                            .with(libp2p::multiaddr::Protocol::P2p(peer_id));
                                        match swarm.dial(relay_addr.clone()) {
                                            Ok(_) => {
                                                println!("📡 Dialing peer {} via relay: {}", peer_id, relay_addr);
                                                dialed = true;
                                                break;
                                            }
                                            Err(e) => {
                                                println!("Relay dial via {} failed: {:?}", bootstrap_addr_str, e);
                                            }
                                        }
                                    }
                                }
                            }

                            if !dialed {
                                println!("❌ Cannot reach peer {}: all dial attempts failed", peer_id);
                                let _ = app.emit("file-download-failed", serde_json::json!({
                                    "requestId": request_id,
                                    "fileHash": file_hash,
                                    "error": format!("Seeder is offline or unreachable (peer: {}...)", &peer_id.to_string()[..8])
                                }));
                                continue;
                            }
                        }

                        let request = ChunkRequest::FileInfo {
                            request_id: request_id.clone(),
                            file_hash: file_hash.clone(),
                        };
                        let req_id = swarm.behaviour_mut().file_request.send_request(&peer_id, request);
                        outbound_request_map.insert(req_id, request_id.clone());
                        println!("File info request sent with ID: {:?}", req_id);
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
    download_directory: &DownloadDirectoryRef,
    active_downloads: &Arc<Mutex<ActiveDownloadsMap>>,
    outbound_request_map: &mut HashMap<request_response::OutboundRequestId, String>,
    download_credentials: &DownloadCredentialsMap,
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
        DhtBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. }) => {
            println!("Identified peer {}: protocol={}, addrs={:?}", peer_id, info.protocol_version, info.listen_addrs);
            // Add all listen addresses to Kademlia so peers can discover each other
            for addr in &info.listen_addrs {
                swarm.behaviour_mut().kad.add_address(&peer_id, addr.clone());
            }
        }
        DhtBehaviourEvent::Identify(_) => {}
        DhtBehaviourEvent::FileTransfer(event) => {
            use request_response::Event;
            match event {
                Event::Message { peer, message } => {
                    match message {
                        request_response::Message::Request { request, channel, .. } => {
                            let is_paid = !request.price_wei.is_empty()
                                && request.price_wei != "0"
                                && request.price_wei.parse::<u128>().unwrap_or(0) > 0;

                            println!("Received file transfer from {}: {} (paid: {})", peer, request.file_name, is_paid);

                            if is_paid {
                                // Paid transfer: file_data is empty, emit event with pricing
                                // so the frontend can prompt the user to accept and pay
                                let actual_size = if request.file_size > 0 {
                                    request.file_size
                                } else {
                                    request.file_data.len() as u64
                                };
                                let _ = app.emit("chiraldrop-paid-request", serde_json::json!({
                                    "transferId": request.transfer_id,
                                    "fromPeerId": peer.to_string(),
                                    "fileName": request.file_name,
                                    "fileHash": request.file_hash,
                                    "fileSize": actual_size,
                                    "priceWei": request.price_wei,
                                    "senderWallet": request.sender_wallet
                                }));
                            } else {
                                // Free transfer: store file data for acceptance
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
                            }

                            // Protocol requires a response
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
                        // === SEEDER SIDE: Handle incoming requests ===
                        request_response::Message::Request { request, channel, .. } => {
                            match request {
                                ChunkRequest::FileInfo { request_id, file_hash } => {
                                    println!("📋 Received FileInfo request from {}: hash={}", peer, file_hash);
                                    let mut shared = shared_files.lock().await;

                                    let response = if let Some(file_info) = shared.get_mut(&file_hash) {
                                        // Compute chunk hashes if not cached
                                        if file_info.chunk_hashes.is_none() {
                                            println!("Computing chunk hashes for {}...", file_info.file_name);
                                            match compute_chunk_hashes(&file_info.file_path) {
                                                Ok(hashes) => {
                                                    file_info.chunk_hashes = Some(hashes);
                                                }
                                                Err(e) => {
                                                    println!("Failed to compute chunk hashes: {}", e);
                                                    let resp = ChunkResponse::FileInfo {
                                                        request_id,
                                                        file_hash,
                                                        file_name: file_info.file_name.clone(),
                                                        file_size: 0,
                                                        chunk_size: CHUNK_SIZE as u32,
                                                        total_chunks: 0,
                                                        chunk_hashes: vec![],
                                                        price_wei: "0".to_string(),
                                                        wallet_address: String::new(),
                                                        error: Some(format!("Failed to read file: {}", e)),
                                                    };
                                                    drop(shared);
                                                    let _ = swarm.behaviour_mut().file_request.send_response(channel, resp);
                                                    return;
                                                }
                                            }
                                        }

                                        let chunk_hashes = file_info.chunk_hashes.as_ref().unwrap();
                                        let price_str = file_info.price_wei.to_string();
                                        let wallet = file_info.wallet_address.clone();
                                        println!("Serving FileInfo for {} ({} bytes, {} chunks, price={} wei) to peer {}",
                                                 file_info.file_name, file_info.file_size, chunk_hashes.len(), price_str, peer);

                                        ChunkResponse::FileInfo {
                                            request_id,
                                            file_hash,
                                            file_name: file_info.file_name.clone(),
                                            file_size: file_info.file_size,
                                            chunk_size: CHUNK_SIZE as u32,
                                            total_chunks: chunk_hashes.len() as u32,
                                            chunk_hashes: chunk_hashes.clone(),
                                            price_wei: price_str,
                                            wallet_address: wallet,
                                            error: None,
                                        }
                                    } else {
                                        println!("File not found: {}", file_hash);
                                        ChunkResponse::FileInfo {
                                            request_id,
                                            file_hash,
                                            file_name: String::new(),
                                            file_size: 0,
                                            chunk_size: CHUNK_SIZE as u32,
                                            total_chunks: 0,
                                            chunk_hashes: vec![],
                                            price_wei: "0".to_string(),
                                            wallet_address: String::new(),
                                            error: Some("File not found".to_string()),
                                        }
                                    };
                                    drop(shared);

                                    if let Err(e) = swarm.behaviour_mut().file_request.send_response(channel, response) {
                                        println!("Failed to send FileInfo response: {:?}", e);
                                    }
                                }
                                ChunkRequest::Chunk { request_id, file_hash, chunk_index } => {
                                    let shared = shared_files.lock().await;

                                    let response = if let Some(file_info) = shared.get(&file_hash) {
                                        // Read the specific chunk from disk
                                        let offset = chunk_index as u64 * CHUNK_SIZE as u64;
                                        match std::fs::File::open(&file_info.file_path) {
                                            Ok(mut file) => {
                                                if let Err(e) = file.seek(SeekFrom::Start(offset)) {
                                                    ChunkResponse::Chunk {
                                                        request_id,
                                                        file_hash,
                                                        chunk_index,
                                                        chunk_data: None,
                                                        chunk_hash: String::new(),
                                                        error: Some(format!("Failed to seek: {}", e)),
                                                    }
                                                } else {
                                                    let mut buf = vec![0u8; CHUNK_SIZE];
                                                    match file.read(&mut buf) {
                                                        Ok(bytes_read) => {
                                                            buf.truncate(bytes_read);
                                                            let mut hasher = Sha256::new();
                                                            hasher.update(&buf);
                                                            let chunk_hash = hex::encode(hasher.finalize());
                                                            ChunkResponse::Chunk {
                                                                request_id,
                                                                file_hash,
                                                                chunk_index,
                                                                chunk_data: Some(buf),
                                                                chunk_hash,
                                                                error: None,
                                                            }
                                                        }
                                                        Err(e) => ChunkResponse::Chunk {
                                                            request_id,
                                                            file_hash,
                                                            chunk_index,
                                                            chunk_data: None,
                                                            chunk_hash: String::new(),
                                                            error: Some(format!("Failed to read chunk: {}", e)),
                                                        },
                                                    }
                                                }
                                            }
                                            Err(e) => ChunkResponse::Chunk {
                                                request_id,
                                                file_hash,
                                                chunk_index,
                                                chunk_data: None,
                                                chunk_hash: String::new(),
                                                error: Some(format!("Failed to open file: {}", e)),
                                            },
                                        }
                                    } else {
                                        ChunkResponse::Chunk {
                                            request_id,
                                            file_hash,
                                            chunk_index,
                                            chunk_data: None,
                                            chunk_hash: String::new(),
                                            error: Some("File not found".to_string()),
                                        }
                                    };
                                    drop(shared);

                                    if let Err(e) = swarm.behaviour_mut().file_request.send_response(channel, response) {
                                        println!("Failed to send chunk response: {:?}", e);
                                    }
                                }
                                ChunkRequest::PaymentProof { request_id, file_hash, payment_tx, payer_address } => {
                                    println!("💰 Received payment proof from {}: tx={}, payer={}", peer, payment_tx, payer_address);
                                    let shared = shared_files.lock().await;

                                    let response = if let Some(file_info) = shared.get(&file_hash) {
                                        if file_info.price_wei == 0 {
                                            // Free file — no payment needed, accept
                                            ChunkResponse::PaymentAck {
                                                request_id,
                                                file_hash,
                                                accepted: true,
                                                error: None,
                                            }
                                        } else {
                                            // Verify payment on-chain
                                            let expected_wallet = file_info.wallet_address.clone();
                                            let expected_price = file_info.price_wei;
                                            drop(shared);

                                            match verify_payment_on_chain(&payment_tx, &expected_wallet, expected_price).await {
                                                Ok(true) => {
                                                    println!("✅ Payment verified for {} from {}", file_hash, payer_address);
                                                    let price_wei_str = expected_price.to_string();
                                                    let _ = app.emit("chiraldrop-payment-received", serde_json::json!({
                                                        "fileHash": file_hash,
                                                        "txHash": payment_tx,
                                                        "priceWei": price_wei_str,
                                                        "fromWallet": payer_address,
                                                        "toWallet": expected_wallet,
                                                    }));
                                                    ChunkResponse::PaymentAck {
                                                        request_id,
                                                        file_hash,
                                                        accepted: true,
                                                        error: None,
                                                    }
                                                }
                                                Ok(false) => {
                                                    println!("❌ Payment verification failed for {}", file_hash);
                                                    ChunkResponse::PaymentAck {
                                                        request_id,
                                                        file_hash,
                                                        accepted: false,
                                                        error: Some("Payment verification failed: insufficient amount or wrong recipient".to_string()),
                                                    }
                                                }
                                                Err(e) => {
                                                    println!("❌ Payment verification error: {}", e);
                                                    ChunkResponse::PaymentAck {
                                                        request_id,
                                                        file_hash,
                                                        accepted: false,
                                                        error: Some(format!("Payment verification error: {}", e)),
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        drop(shared);
                                        ChunkResponse::PaymentAck {
                                            request_id,
                                            file_hash,
                                            accepted: false,
                                            error: Some("File not found".to_string()),
                                        }
                                    };

                                    if let Err(e) = swarm.behaviour_mut().file_request.send_response(channel, response) {
                                        println!("Failed to send PaymentAck response: {:?}", e);
                                    }
                                }
                            }
                        }
                        // === DOWNLOADER SIDE: Handle incoming responses ===
                        request_response::Message::Response { response, request_id: outbound_req_id, .. } => {
                            // Clean up outbound request mapping
                            outbound_request_map.remove(&outbound_req_id);

                            match response {
                                ChunkResponse::FileInfo { request_id, file_hash, file_name, file_size, chunk_size: _, total_chunks, chunk_hashes, price_wei, wallet_address, error } => {
                                    if let Some(err) = error {
                                        println!("❌ FileInfo error: {}", err);
                                        let mut tiers = download_tiers.lock().await;
                                        tiers.remove(&request_id);
                                        let _ = app.emit("file-download-failed", serde_json::json!({
                                            "requestId": request_id,
                                            "fileHash": file_hash,
                                            "error": err
                                        }));
                                        return;
                                    }

                                    let price: u128 = price_wei.parse().unwrap_or(0);
                                    println!("📋 Received FileInfo: {} ({} bytes, {} chunks, price={} wei)", file_name, file_size, total_chunks, price);

                                    // Get speed tier
                                    let tier = {
                                        let tiers = download_tiers.lock().await;
                                        tiers.get(&request_id).cloned().unwrap_or(SpeedTier::Free)
                                    };

                                    // Determine output path
                                    let custom_dir = download_directory.lock().await.clone();
                                    let downloads_dir = if let Some(ref dir) = custom_dir {
                                        let p = PathBuf::from(dir);
                                        if p.exists() && p.is_dir() { p } else {
                                            dirs::download_dir().unwrap_or_else(|| PathBuf::from("."))
                                        }
                                    } else {
                                        dirs::download_dir().unwrap_or_else(|| PathBuf::from("."))
                                    };

                                    let output_name = if file_name.is_empty() {
                                        format!("{}.download", &file_hash[..std::cmp::min(8, file_hash.len())])
                                    } else {
                                        file_name.clone()
                                    };
                                    let output_path = downloads_dir.join(&output_name);

                                    // Create empty output file
                                    if let Err(e) = std::fs::File::create(&output_path) {
                                        println!("❌ Failed to create output file: {}", e);
                                        let _ = app.emit("file-download-failed", serde_json::json!({
                                            "requestId": request_id,
                                            "fileHash": file_hash,
                                            "error": format!("Failed to create output file: {}", e)
                                        }));
                                        return;
                                    }

                                    // Create active download state
                                    let download = ActiveChunkedDownload {
                                        request_id: request_id.clone(),
                                        file_hash: file_hash.clone(),
                                        file_name: output_name,
                                        file_size,
                                        total_chunks,
                                        chunk_hashes,
                                        received_chunks: vec![false; total_chunks as usize],
                                        output_path: output_path.clone(),
                                        bytes_written: 0,
                                        peer_id: peer,
                                        tier,
                                        retry_counts: vec![0u8; total_chunks as usize],
                                        current_chunk_index: 0,
                                        start_time: std::time::Instant::now(),
                                        payment_confirmed: price == 0,
                                    };

                                    {
                                        let mut downloads = active_downloads.lock().await;
                                        downloads.insert(request_id.clone(), download);
                                    }

                                    if price > 0 {
                                        // Paid file: send payment to seeder, then send proof
                                        println!("💰 File requires payment of {} wei to {}", price, wallet_address);
                                        let _ = app.emit("file-payment-processing", serde_json::json!({
                                            "requestId": request_id,
                                            "fileHash": file_hash,
                                            "priceWei": price_wei,
                                            "walletAddress": wallet_address
                                        }));

                                        // Get download credentials for this request
                                        let creds = {
                                            let creds_map = download_credentials.lock().await;
                                            creds_map.get(&request_id).cloned()
                                        };

                                        if let Some(creds) = creds {
                                            // Convert wei to CHI for send_transaction
                                            let cost_chi = crate::speed_tiers::format_wei_as_chi(price);

                                            // Send payment transaction
                                            match crate::send_payment_transaction(
                                                &creds.wallet_address,
                                                &wallet_address,
                                                &cost_chi,
                                                &creds.private_key,
                                            ).await {
                                                Ok(payment) => {
                                                    println!("💰 Payment sent: tx={}", payment.tx_hash);
                                                    // Emit event so frontend can track in transaction history
                                                    let _ = app.emit("chiraldrop-payment-sent", serde_json::json!({
                                                        "requestId": request_id,
                                                        "fileHash": file_hash,
                                                        "fileName": file_name,
                                                        "txHash": payment.tx_hash,
                                                        "priceWei": price_wei,
                                                        "toWallet": wallet_address,
                                                        "fromWallet": creds.wallet_address,
                                                        "balanceBefore": payment.balance_before,
                                                        "balanceAfter": payment.balance_after,
                                                    }));
                                                    // Send payment proof to seeder
                                                    let request = ChunkRequest::PaymentProof {
                                                        request_id: request_id.clone(),
                                                        file_hash: file_hash.clone(),
                                                        payment_tx: payment.tx_hash,
                                                        payer_address: creds.wallet_address.clone(),
                                                    };
                                                    let req_id = swarm.behaviour_mut().file_request.send_request(&peer, request);
                                                    outbound_request_map.insert(req_id, request_id);
                                                }
                                                Err(e) => {
                                                    println!("❌ Payment failed: {}", e);
                                                    let mut downloads = active_downloads.lock().await;
                                                    if let Some(dl) = downloads.remove(&request_id) {
                                                        let _ = std::fs::remove_file(&dl.output_path);
                                                    }
                                                    let mut tiers = download_tiers.lock().await;
                                                    tiers.remove(&request_id);
                                                    let _ = app.emit("file-download-failed", serde_json::json!({
                                                        "requestId": request_id,
                                                        "fileHash": file_hash,
                                                        "error": format!("Payment failed: {}", e)
                                                    }));
                                                }
                                            }
                                        } else {
                                            println!("❌ No wallet credentials for paid download");
                                            let mut downloads = active_downloads.lock().await;
                                            if let Some(dl) = downloads.remove(&request_id) {
                                                let _ = std::fs::remove_file(&dl.output_path);
                                            }
                                            let mut tiers = download_tiers.lock().await;
                                            tiers.remove(&request_id);
                                            let _ = app.emit("file-download-failed", serde_json::json!({
                                                "requestId": request_id,
                                                "fileHash": file_hash,
                                                "error": "Wallet not connected. Connect your wallet to download paid files."
                                            }));
                                        }
                                    } else {
                                        // Free file: request the first chunk immediately
                                        let request = ChunkRequest::Chunk {
                                            request_id: request_id.clone(),
                                            file_hash: file_hash.clone(),
                                            chunk_index: 0,
                                        };
                                        let req_id = swarm.behaviour_mut().file_request.send_request(&peer, request);
                                        outbound_request_map.insert(req_id, request_id);
                                    }
                                }
                                ChunkResponse::Chunk { request_id, file_hash, chunk_index, chunk_data, chunk_hash, error } => {
                                    if let Some(err) = error {
                                        println!("❌ Chunk {} error: {}", chunk_index, err);
                                        // Check if we should retry
                                        let mut downloads = active_downloads.lock().await;
                                        if let Some(dl) = downloads.get_mut(&request_id) {
                                            dl.retry_counts[chunk_index as usize] += 1;
                                            if dl.retry_counts[chunk_index as usize] <= MAX_CHUNK_RETRIES {
                                                println!("🔄 Retrying chunk {} (attempt {})", chunk_index, dl.retry_counts[chunk_index as usize]);
                                                let peer_id = dl.peer_id;
                                                let fh = dl.file_hash.clone();
                                                let rid = dl.request_id.clone();
                                                drop(downloads);
                                                let request = ChunkRequest::Chunk { request_id: rid.clone(), file_hash: fh, chunk_index };
                                                let req_id = swarm.behaviour_mut().file_request.send_request(&peer_id, request);
                                                outbound_request_map.insert(req_id, rid);
                                            } else {
                                                // Max retries exceeded — abort
                                                let dl = downloads.remove(&request_id).unwrap();
                                                let _ = std::fs::remove_file(&dl.output_path);
                                                let mut tiers = download_tiers.lock().await;
                                                tiers.remove(&request_id);
                                                drop(downloads);
                                                let _ = app.emit("file-download-failed", serde_json::json!({
                                                    "requestId": request_id,
                                                    "fileHash": file_hash,
                                                    "error": format!("Chunk {} failed after {} retries: {}", chunk_index, MAX_CHUNK_RETRIES, err)
                                                }));
                                            }
                                        }
                                        return;
                                    }

                                    let chunk_data = match chunk_data {
                                        Some(data) => data,
                                        None => {
                                            println!("❌ Chunk {} has no data and no error", chunk_index);
                                            return;
                                        }
                                    };

                                    // Verify chunk hash against manifest
                                    let mut downloads = active_downloads.lock().await;
                                    if let Some(dl) = downloads.get_mut(&request_id) {
                                        let expected_hash = &dl.chunk_hashes[chunk_index as usize];
                                        let mut hasher = Sha256::new();
                                        hasher.update(&chunk_data);
                                        let computed_hash = hex::encode(hasher.finalize());

                                        if computed_hash != *expected_hash || computed_hash != chunk_hash {
                                            println!("❌ Chunk {} hash mismatch! Expected: {}, Got: {}", chunk_index, expected_hash, computed_hash);
                                            dl.retry_counts[chunk_index as usize] += 1;
                                            if dl.retry_counts[chunk_index as usize] <= MAX_CHUNK_RETRIES {
                                                println!("🔄 Retrying chunk {} due to hash mismatch", chunk_index);
                                                let peer_id = dl.peer_id;
                                                let fh = dl.file_hash.clone();
                                                let rid = dl.request_id.clone();
                                                drop(downloads);
                                                let request = ChunkRequest::Chunk { request_id: rid.clone(), file_hash: fh, chunk_index };
                                                let req_id = swarm.behaviour_mut().file_request.send_request(&peer_id, request);
                                                outbound_request_map.insert(req_id, rid);
                                            } else {
                                                let dl = downloads.remove(&request_id).unwrap();
                                                let _ = std::fs::remove_file(&dl.output_path);
                                                let mut tiers = download_tiers.lock().await;
                                                tiers.remove(&request_id);
                                                drop(downloads);
                                                let _ = app.emit("file-download-failed", serde_json::json!({
                                                    "requestId": request_id,
                                                    "fileHash": file_hash,
                                                    "error": format!("Chunk {} failed integrity check after {} retries", chunk_index, MAX_CHUNK_RETRIES)
                                                }));
                                            }
                                            return;
                                        }

                                        // Write chunk to file (append)
                                        use std::io::Write;
                                        let write_result = std::fs::OpenOptions::new()
                                            .append(true)
                                            .open(&dl.output_path)
                                            .and_then(|mut f| f.write_all(&chunk_data));

                                        if let Err(e) = write_result {
                                            println!("❌ Failed to write chunk {}: {}", chunk_index, e);
                                            let dl = downloads.remove(&request_id).unwrap();
                                            let _ = std::fs::remove_file(&dl.output_path);
                                            let mut tiers = download_tiers.lock().await;
                                            tiers.remove(&request_id);
                                            drop(downloads);
                                            let _ = app.emit("file-download-failed", serde_json::json!({
                                                "requestId": request_id,
                                                "fileHash": file_hash,
                                                "error": format!("Failed to write chunk: {}", e)
                                            }));
                                            return;
                                        }

                                        dl.received_chunks[chunk_index as usize] = true;
                                        dl.bytes_written += chunk_data.len() as u64;
                                        dl.current_chunk_index = chunk_index + 1;

                                        // Emit progress
                                        let progress = (dl.bytes_written as f64 / dl.file_size as f64) * 100.0;
                                        let elapsed = dl.start_time.elapsed().as_secs_f64();
                                        let speed_bps = if elapsed > 0.0 { (dl.bytes_written as f64 / elapsed) as u64 } else { 0 };
                                        let _ = app.emit("download-progress", serde_json::json!({
                                            "requestId": dl.request_id,
                                            "fileHash": dl.file_hash,
                                            "fileName": dl.file_name,
                                            "bytesWritten": dl.bytes_written,
                                            "totalBytes": dl.file_size,
                                            "speedBps": speed_bps,
                                            "progress": progress
                                        }));

                                        // Check if all chunks received
                                        if dl.current_chunk_index >= dl.total_chunks {
                                            // All chunks received — verify full file hash
                                            let output_path = dl.output_path.clone();
                                            let expected_file_hash = dl.file_hash.clone();
                                            let request_id_clone = dl.request_id.clone();
                                            let file_name_clone = dl.file_name.clone();
                                            let file_size = dl.file_size;
                                            let _ = downloads.remove(&request_id);
                                            let mut tiers = download_tiers.lock().await;
                                            tiers.remove(&request_id_clone);
                                            drop(tiers);
                                            drop(downloads);

                                            // Verify full file SHA-256
                                            let app_clone = app.clone();
                                            tokio::spawn(async move {
                                                match tokio::task::spawn_blocking(move || {
                                                    let mut file = std::fs::File::open(&output_path)?;
                                                    let mut hasher = Sha256::new();
                                                    let mut buf = vec![0u8; 256 * 1024];
                                                    loop {
                                                        let n = file.read(&mut buf)?;
                                                        if n == 0 { break; }
                                                        hasher.update(&buf[..n]);
                                                    }
                                                    Ok::<(String, PathBuf), std::io::Error>((hex::encode(hasher.finalize()), output_path))
                                                }).await {
                                                    Ok(Ok((computed_hash, output_path))) => {
                                                        if computed_hash == expected_file_hash {
                                                            println!("✅ File verified and saved: {:?}", output_path);
                                                            let _ = app_clone.emit("file-download-complete", serde_json::json!({
                                                                "requestId": request_id_clone,
                                                                "fileHash": expected_file_hash,
                                                                "fileName": file_name_clone,
                                                                "filePath": output_path.to_string_lossy(),
                                                                "fileSize": file_size,
                                                                "status": "completed"
                                                            }));
                                                        } else {
                                                            println!("❌ Full file hash mismatch! Expected: {}, Got: {}", expected_file_hash, computed_hash);
                                                            let _ = std::fs::remove_file(&output_path);
                                                            let _ = app_clone.emit("file-download-failed", serde_json::json!({
                                                                "requestId": request_id_clone,
                                                                "fileHash": expected_file_hash,
                                                                "error": "File integrity verification failed — hash mismatch after all chunks received"
                                                            }));
                                                        }
                                                    }
                                                    Ok(Err(e)) => {
                                                        println!("❌ Failed to verify file: {}", e);
                                                        let _ = app_clone.emit("file-download-failed", serde_json::json!({
                                                            "requestId": request_id_clone,
                                                            "fileHash": expected_file_hash,
                                                            "error": format!("Failed to verify file: {}", e)
                                                        }));
                                                    }
                                                    Err(e) => {
                                                        println!("❌ Verification task panicked: {}", e);
                                                        let _ = app_clone.emit("file-download-failed", serde_json::json!({
                                                            "requestId": request_id_clone,
                                                            "fileHash": expected_file_hash,
                                                            "error": format!("Verification task failed: {}", e)
                                                        }));
                                                    }
                                                }
                                            });
                                        } else {
                                            // Request next chunk after rate-limit delay
                                            let peer_id = dl.peer_id;
                                            let fh = dl.file_hash.clone();
                                            let rid = dl.request_id.clone();
                                            let next_index = dl.current_chunk_index;
                                            let tier = dl.tier.clone();
                                            drop(downloads);

                                            // Apply rate-limit delay
                                            if let Some(delay) = crate::speed_tiers::chunk_request_delay(CHUNK_SIZE as u32, &tier) {
                                                tokio::time::sleep(delay).await;
                                            }

                                            let request = ChunkRequest::Chunk {
                                                request_id: rid.clone(),
                                                file_hash: fh,
                                                chunk_index: next_index,
                                            };
                                            let req_id = swarm.behaviour_mut().file_request.send_request(&peer_id, request);
                                            outbound_request_map.insert(req_id, rid);
                                        }
                                    }
                                }
                                ChunkResponse::PaymentAck { request_id, file_hash, accepted, error } => {
                                    if !accepted {
                                        let err_msg = error.unwrap_or_else(|| "Payment rejected by seeder".to_string());
                                        println!("❌ Payment rejected for {}: {}", file_hash, err_msg);
                                        let mut downloads = active_downloads.lock().await;
                                        if let Some(dl) = downloads.remove(&request_id) {
                                            let _ = std::fs::remove_file(&dl.output_path);
                                        }
                                        let mut tiers = download_tiers.lock().await;
                                        tiers.remove(&request_id);
                                        let _ = app.emit("file-download-failed", serde_json::json!({
                                            "requestId": request_id,
                                            "fileHash": file_hash,
                                            "error": err_msg
                                        }));
                                        return;
                                    }

                                    println!("✅ Payment accepted for {}, starting chunk download", file_hash);

                                    // Mark payment confirmed and request first chunk
                                    let mut downloads = active_downloads.lock().await;
                                    if let Some(dl) = downloads.get_mut(&request_id) {
                                        dl.payment_confirmed = true;
                                        let peer_id = dl.peer_id;
                                        let fh = dl.file_hash.clone();
                                        let rid = dl.request_id.clone();
                                        drop(downloads);

                                        let request = ChunkRequest::Chunk {
                                            request_id: rid.clone(),
                                            file_hash: fh,
                                            chunk_index: 0,
                                        };
                                        let req_id = swarm.behaviour_mut().file_request.send_request(&peer_id, request);
                                        outbound_request_map.insert(req_id, rid);
                                    }
                                }
                            }
                        }
                    }
                }
                Event::OutboundFailure { peer, error, request_id: outbound_req_id, .. } => {
                    let our_request_id = outbound_request_map.remove(&outbound_req_id);
                    let peer_short = &peer.to_string()[..std::cmp::min(8, peer.to_string().len())];

                    if let Some(request_id) = our_request_id {
                        let mut downloads = active_downloads.lock().await;
                        if let Some(dl) = downloads.get_mut(&request_id) {
                            let chunk_index = dl.current_chunk_index.saturating_sub(1).max(0);
                            dl.retry_counts[chunk_index as usize] += 1;

                            if dl.retry_counts[chunk_index as usize] <= MAX_CHUNK_RETRIES {
                                println!("🔄 Retrying chunk {} after outbound failure (attempt {})", chunk_index, dl.retry_counts[chunk_index as usize]);
                                let peer_id = dl.peer_id;
                                let fh = dl.file_hash.clone();
                                let rid = dl.request_id.clone();
                                drop(downloads);
                                // Wait before retry
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                let request = ChunkRequest::Chunk { request_id: rid.clone(), file_hash: fh, chunk_index };
                                let req_id = swarm.behaviour_mut().file_request.send_request(&peer_id, request);
                                outbound_request_map.insert(req_id, rid);
                                return;
                            } else {
                                // Max retries exceeded
                                let dl = downloads.remove(&request_id).unwrap();
                                let _ = std::fs::remove_file(&dl.output_path);
                                let mut tiers = download_tiers.lock().await;
                                tiers.remove(&request_id);
                            }
                        }
                        drop(downloads);
                    }

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
        DhtBehaviourEvent::RelayClient(event) => {
            match event {
                relay::client::Event::ReservationReqAccepted { relay_peer_id, renewal, .. } => {
                    let action = if renewal { "renewed" } else { "accepted" };
                    println!("🔄 Relay reservation {} by {}", action, relay_peer_id);
                    let _ = app.emit("relay-reservation", serde_json::json!({
                        "relayPeerId": relay_peer_id.to_string(),
                        "renewal": renewal,
                    }));
                }
                relay::client::Event::OutboundCircuitEstablished { relay_peer_id, .. } => {
                    println!("✅ Outbound relay circuit established via {}", relay_peer_id);
                    let _ = app.emit("relay-circuit-established", serde_json::json!({
                        "relayPeerId": relay_peer_id.to_string(),
                        "direction": "outbound",
                    }));
                }
                relay::client::Event::InboundCircuitEstablished { src_peer_id, .. } => {
                    println!("✅ Inbound relay circuit from {}", src_peer_id);
                    let _ = app.emit("relay-circuit-established", serde_json::json!({
                        "srcPeerId": src_peer_id.to_string(),
                        "direction": "inbound",
                    }));
                }
            }
        }
        DhtBehaviourEvent::Dcutr(event) => {
            match &event.result {
                Ok(connection_id) => {
                    println!("🕳️ DCUtR hole-punch succeeded with {} (conn {:?})", event.remote_peer_id, connection_id);
                    let _ = app.emit("dcutr-event", serde_json::json!({
                        "remotePeerId": event.remote_peer_id.to_string(),
                        "success": true,
                    }));
                }
                Err(e) => {
                    println!("🕳️ DCUtR hole-punch failed with {}: {}", event.remote_peer_id, e);
                    let _ = app.emit("dcutr-event", serde_json::json!({
                        "remotePeerId": event.remote_peer_id.to_string(),
                        "success": false,
                        "error": format!("{}", e),
                    }));
                }
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
        assert_eq!(nodes.len(), 3);
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
        let peer_ids = get_bootstrap_peer_ids();
        let mut seen = Vec::new();
        for id in &peer_ids {
            assert!(!seen.contains(id), "Duplicate peer ID: {}", id);
            seen.push(id.clone());
        }
        // Should have 2 unique nodes (one has both IPv4 and IPv6)
        assert_eq!(peer_ids.len(), 2);
    }

    #[test]
    fn test_extract_peer_id_from_valid_multiaddr() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWAHWpUyBsFvgC6fb9jjmtDtKMM1qUChiNjtBDrTTEAY5C"
            .parse()
            .unwrap();
        let peer_id = extract_peer_id_from_multiaddr(&addr);
        assert!(peer_id.is_some());
        assert_eq!(
            peer_id.unwrap().to_string(),
            "12D3KooWAHWpUyBsFvgC6fb9jjmtDtKMM1qUChiNjtBDrTTEAY5C"
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
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWAHWpUyBsFvgC6fb9jjmtDtKMM1qUChiNjtBDrTTEAY5C"
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
    fn test_ipv6_listen_multiaddr_parses() {
        let addr: Multiaddr = "/ip6/::/tcp/0".parse().unwrap();
        assert_eq!(addr.to_string(), "/ip6/::/tcp/0");
    }

    #[test]
    fn test_extract_peer_id_from_ipv6_multiaddr() {
        let addr: Multiaddr = "/ip6/::1/tcp/4001/p2p/12D3KooWAHWpUyBsFvgC6fb9jjmtDtKMM1qUChiNjtBDrTTEAY5C"
            .parse()
            .unwrap();
        let peer_id = extract_peer_id_from_multiaddr(&addr);
        assert!(peer_id.is_some());
        assert_eq!(
            peer_id.unwrap().to_string(),
            "12D3KooWAHWpUyBsFvgC6fb9jjmtDtKMM1qUChiNjtBDrTTEAY5C"
        );
    }

    #[test]
    fn test_remove_peer_id_from_ipv6_multiaddr() {
        let addr: Multiaddr = "/ip6/::1/tcp/4001/p2p/12D3KooWAHWpUyBsFvgC6fb9jjmtDtKMM1qUChiNjtBDrTTEAY5C"
            .parse()
            .unwrap();
        let transport = remove_peer_id_from_multiaddr(&addr);
        assert_eq!(transport.to_string(), "/ip6/::1/tcp/4001");
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
            price_wei: "0".to_string(),
            sender_wallet: String::new(),
            file_hash: String::new(),
            file_size: 5,
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: FileTransferRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request.transfer_id, deserialized.transfer_id);
        assert_eq!(request.file_name, deserialized.file_name);
        assert_eq!(request.file_data, deserialized.file_data);
    }

    #[test]
    fn test_file_transfer_request_paid_serialization() {
        let request = FileTransferRequest {
            transfer_id: "tx-paid-001".to_string(),
            file_name: "paid_file.zip".to_string(),
            file_data: vec![],
            price_wei: "1000000000000000000".to_string(),
            sender_wallet: "0xabc123".to_string(),
            file_hash: "deadbeef".to_string(),
            file_size: 1048576,
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: FileTransferRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.price_wei, "1000000000000000000");
        assert_eq!(deserialized.sender_wallet, "0xabc123");
        assert_eq!(deserialized.file_hash, "deadbeef");
        assert_eq!(deserialized.file_size, 1048576);
        assert!(deserialized.file_data.is_empty());
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
    fn test_chunk_request_file_info_serialization() {
        let req = ChunkRequest::FileInfo {
            request_id: "req-001".to_string(),
            file_hash: "abc123def456".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ChunkRequest = serde_json::from_str(&json).unwrap();
        if let ChunkRequest::FileInfo { request_id, file_hash } = deserialized {
            assert_eq!(request_id, "req-001");
            assert_eq!(file_hash, "abc123def456");
        } else {
            panic!("Expected FileInfo variant");
        }
    }

    #[test]
    fn test_chunk_request_chunk_serialization() {
        let req = ChunkRequest::Chunk {
            request_id: "req-001".to_string(),
            file_hash: "abc123".to_string(),
            chunk_index: 42,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ChunkRequest = serde_json::from_str(&json).unwrap();
        if let ChunkRequest::Chunk { request_id, file_hash, chunk_index } = deserialized {
            assert_eq!(request_id, "req-001");
            assert_eq!(file_hash, "abc123");
            assert_eq!(chunk_index, 42);
        } else {
            panic!("Expected Chunk variant");
        }
    }

    #[test]
    fn test_chunk_response_file_info_serialization() {
        let resp = ChunkResponse::FileInfo {
            request_id: "req-001".to_string(),
            file_hash: "abc123".to_string(),
            file_name: "document.pdf".to_string(),
            file_size: 1048576,
            chunk_size: 262144,
            total_chunks: 4,
            chunk_hashes: vec!["hash0".to_string(), "hash1".to_string(), "hash2".to_string(), "hash3".to_string()],
            price_wei: "1000000000000000".to_string(),
            wallet_address: "0x1234567890abcdef".to_string(),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ChunkResponse = serde_json::from_str(&json).unwrap();
        if let ChunkResponse::FileInfo { total_chunks, chunk_hashes, price_wei, wallet_address, error, .. } = deserialized {
            assert_eq!(total_chunks, 4);
            assert_eq!(chunk_hashes.len(), 4);
            assert_eq!(price_wei, "1000000000000000");
            assert_eq!(wallet_address, "0x1234567890abcdef");
            assert!(error.is_none());
        } else {
            panic!("Expected FileInfo variant");
        }
    }

    #[test]
    fn test_chunk_response_chunk_serialization() {
        let resp = ChunkResponse::Chunk {
            request_id: "req-001".to_string(),
            file_hash: "abc123".to_string(),
            chunk_index: 0,
            chunk_data: Some(vec![10, 20, 30]),
            chunk_hash: "deadbeef".to_string(),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ChunkResponse = serde_json::from_str(&json).unwrap();
        if let ChunkResponse::Chunk { chunk_index, chunk_data, chunk_hash, error, .. } = deserialized {
            assert_eq!(chunk_index, 0);
            assert_eq!(chunk_data.unwrap(), vec![10, 20, 30]);
            assert_eq!(chunk_hash, "deadbeef");
            assert!(error.is_none());
        } else {
            panic!("Expected Chunk variant");
        }
    }

    #[test]
    fn test_chunk_response_error() {
        let resp = ChunkResponse::FileInfo {
            request_id: "req-002".to_string(),
            file_hash: "nonexistent".to_string(),
            file_name: String::new(),
            file_size: 0,
            chunk_size: 262144,
            total_chunks: 0,
            chunk_hashes: vec![],
            price_wei: "0".to_string(),
            wallet_address: String::new(),
            error: Some("File not found".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ChunkResponse = serde_json::from_str(&json).unwrap();
        if let ChunkResponse::FileInfo { error, .. } = deserialized {
            assert_eq!(error.unwrap(), "File not found");
        } else {
            panic!("Expected FileInfo variant");
        }
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
            chunk_hashes: None,
            price_wei: 0,
            wallet_address: String::new(),
        };
        assert_eq!(info.file_name, "file.txt");
        assert_eq!(info.file_size, 1024);
        assert!(info.chunk_hashes.is_none());
        assert_eq!(info.price_wei, 0);
    }

    #[test]
    fn test_shared_file_info_with_price() {
        let info = SharedFileInfo {
            file_path: "/path/to/file.txt".to_string(),
            file_name: "file.txt".to_string(),
            file_size: 1024,
            chunk_hashes: None,
            price_wei: 5_000_000_000_000_000,
            wallet_address: "0xabc123".to_string(),
        };
        assert_eq!(info.price_wei, 5_000_000_000_000_000);
        assert_eq!(info.wallet_address, "0xabc123");
    }

    #[test]
    fn test_chunk_request_payment_proof_serialization() {
        let req = ChunkRequest::PaymentProof {
            request_id: "req-001".to_string(),
            file_hash: "abc123".to_string(),
            payment_tx: "0xdeadbeef".to_string(),
            payer_address: "0x1234".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ChunkRequest = serde_json::from_str(&json).unwrap();
        if let ChunkRequest::PaymentProof { request_id, file_hash, payment_tx, payer_address } = deserialized {
            assert_eq!(request_id, "req-001");
            assert_eq!(file_hash, "abc123");
            assert_eq!(payment_tx, "0xdeadbeef");
            assert_eq!(payer_address, "0x1234");
        } else {
            panic!("Expected PaymentProof variant");
        }
    }

    #[test]
    fn test_chunk_response_payment_ack_serialization() {
        let resp = ChunkResponse::PaymentAck {
            request_id: "req-001".to_string(),
            file_hash: "abc123".to_string(),
            accepted: true,
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ChunkResponse = serde_json::from_str(&json).unwrap();
        if let ChunkResponse::PaymentAck { accepted, error, .. } = deserialized {
            assert!(accepted);
            assert!(error.is_none());
        } else {
            panic!("Expected PaymentAck variant");
        }
    }

    #[test]
    fn test_chunk_response_payment_ack_rejected() {
        let resp = ChunkResponse::PaymentAck {
            request_id: "req-001".to_string(),
            file_hash: "abc123".to_string(),
            accepted: false,
            error: Some("Insufficient payment".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: ChunkResponse = serde_json::from_str(&json).unwrap();
        if let ChunkResponse::PaymentAck { accepted, error, .. } = deserialized {
            assert!(!accepted);
            assert_eq!(error.unwrap(), "Insufficient payment");
        } else {
            panic!("Expected PaymentAck variant");
        }
    }

    #[test]
    fn test_compute_chunk_hashes() {
        // Create a temp file with known content
        let dir = std::env::temp_dir();
        let path = dir.join("chiral_test_chunk_hashes.bin");
        let data = vec![0xABu8; CHUNK_SIZE * 2 + 100]; // 2.x chunks
        std::fs::write(&path, &data).unwrap();

        let hashes = compute_chunk_hashes(path.to_str().unwrap()).unwrap();
        assert_eq!(hashes.len(), 3); // ceil((2*CHUNK_SIZE + 100) / CHUNK_SIZE) = 3
        // Each hash should be a 64-char hex string (SHA-256)
        for h in &hashes {
            assert_eq!(h.len(), 64);
        }

        std::fs::remove_file(&path).unwrap();
    }
}
