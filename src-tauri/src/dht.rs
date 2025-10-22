use crate::encryption::EncryptedAesKeyBundle;
use x25519_dalek::PublicKey;
use serde_bytes;

// ------ Key Request Protocol Implementation ------
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRequestProtocol;

impl AsRef<str> for KeyRequestProtocol {
    fn as_ref(&self) -> &str {
        "/chiral/key-request/1.0.0"
    }
}

#[derive(Clone, Debug, Default)]
pub struct KeyRequestCodec;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyRequest {
    pub merkle_root: String,
    #[serde(with = "serde_bytes")]
    pub recipient_public_key: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyResponse {
    pub encrypted_bundle: Option<EncryptedAesKeyBundle>,
    pub error: Option<String>,
}

#[async_trait::async_trait]
impl rr::Codec for KeyRequestCodec {
    type Protocol = KeyRequestProtocol;
    type Request = KeyRequest;
    type Response = KeyResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> std::io::Result<Self::Request>
    where
        T: FAsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        serde_json::from_slice(&data).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> std::io::Result<Self::Response>
    where
        T: FAsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        serde_json::from_slice(&data).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(&mut self, _: &Self::Protocol, io: &mut T, request: Self::Request) -> std::io::Result<()>
    where
        T: FAsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&request).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }

    async fn write_response<T>(&mut self, _: &Self::Protocol, io: &mut T, response: Self::Response) -> std::io::Result<()>
    where
        T: FAsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&response).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }
}
use async_std::fs;
use async_std::path::Path;
use async_trait::async_trait;
use blockstore::{
    block::{Block, CidError},
    RedbBlockstore,
};
use ethers::prelude::*;
use tokio::task::JoinHandle;

pub use cid::Cid;
use futures::future::{BoxFuture, FutureExt};
use futures::io::{AsyncRead as FAsyncRead, AsyncWrite as FAsyncWrite};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use futures_util::StreamExt;
use libp2p::multiaddr::Protocol;
pub use multihash_codetable::{Code, MultihashDigest};
use relay::client::Event as RelayClientEvent;
use rs_merkle::{Hasher, MerkleTree};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    str::FromStr,
};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{debug, error, info, trace, warn};

use crate::peer_selection::{PeerMetrics, PeerSelectionService, SelectionStrategy};
use crate::webrtc_service::{get_webrtc_service, FileChunk};
use crate::manager::Sha256Hasher;
use std::io::{self};
use tokio_socks::tcp::Socks5Stream;

use std::pin::Pin;
use std::task::{Context, Poll};

// Import the missing types
use crate::file_transfer::FileTransferService;
use crate::manager::ChunkManager;
use std::error::Error;

// Trait alias to abstract over async I/O types used by proxy transport
pub trait AsyncIo: FAsyncRead + FAsyncWrite + Unpin + Send {}
impl<T: FAsyncRead + FAsyncWrite + Unpin + Send> AsyncIo for T {}

use libp2p::{
    autonat::v2,
    core::{
        muxing::StreamMuxerBox,
        // FIXED E0432: ListenerEvent is removed, only import what is available.
        transport::{Boxed, DialOpts, ListenerId, Transport, TransportError, TransportEvent},
    },
    dcutr,
    identify::{self, Event as IdentifyEvent},
    identity,
    kad::{
        self, store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig,
        Event as KademliaEvent, GetRecordOk, Mode, PutRecordOk, QueryResult, Record,
    },
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    ping::{self, Behaviour as Ping, Event as PingEvent},
    relay, request_response as rr,
    swarm::{behaviour::toggle, NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use rand::rngs::OsRng;
const EXPECTED_PROTOCOL_VERSION: &str = "/chiral/1.0.0";
const MAX_MULTIHASH_LENGHT: usize = 64;
pub const RAW_CODEC: u64 = 0x55;
/// Heartbeat interval (how often we refresh our provider entry).
const FILE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15); // More frequent updates
/// File seeder TTL – if no heartbeat lands within this window, drop the entry.
const FILE_HEARTBEAT_TTL: Duration = Duration::from_secs(90); // Longer TTL with grace period
pub struct PendingKeywordIndex;

/// Extracts a set of unique, searchable keywords from a filename.
fn extract_keywords(file_name: &str) -> Vec<String> {
    // 1. Sanitize: remove the file extension and convert to lowercase.
    let name_without_ext = std::path::Path::new(file_name)
        .file_stem()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
        .to_lowercase();

    // 2. Split the name into words based on common non-alphanumeric delimiters.
    let keywords: std::collections::HashSet<String> = name_without_ext
        .split(|c: char| !c.is_alphanumeric())
        // 3. Filter out empty strings and common short words (e.g., "a", "of").
        .filter(|s| !s.is_empty() && s.len() > 2)
        .map(String::from)
        .collect(); // Using a HashSet automatically handles duplicates.

    // 4. Return the unique keywords as a Vec.
    keywords.into_iter().collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    /// The Merkle root of the original file chunks, used as the primary identifier for integrity.
    pub merkle_root: String,
    pub file_name: String,
    pub file_size: u64,
    pub file_data: Vec<u8>, // holds the actual file data
    pub seeders: Vec<String>,
    pub created_at: u64,
    pub mime_type: Option<String>,
    /// Whether the file is encrypted
    pub is_encrypted: bool,
    /// The encryption method used (e.g., "AES-256-GCM")
    pub encryption_method: Option<String>,
    /// Fingerprint of the encryption key for identification.
    /// This is now deprecated in favor of the merkle_root.
    pub key_fingerprint: Option<String>,
    // --- VERSIONING FIELDS ---
    pub version: Option<u32>,
    pub parent_hash: Option<String>,
    /// The root CID(s) for retrieving the file from Bitswap. Usually one.
    pub cids: Option<Vec<Cid>>,
    /// For encrypted files, this contains the encrypted AES key and other info.
    pub encrypted_key_bundle: Option<crate::encryption::EncryptedAesKeyBundle>,
    pub is_root: bool,
    pub download_path: Option<String>,
    /// Price in Chiral tokens set by the uploader
    #[serde(default)]
    pub price: Option<f64>,
    /// Ethereum address of the uploader (for payment)
    #[serde(default)]
    pub uploader_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeederHeartbeat {
    peer_id: String,
    expires_at: u64,
    last_heartbeat: u64,
}

#[derive(Debug, Clone)]
struct FileHeartbeatCacheEntry {
    heartbeats: Vec<SeederHeartbeat>,
    metadata: serde_json::Value,
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

fn merge_heartbeats(mut a: Vec<SeederHeartbeat>, mut b: Vec<SeederHeartbeat>) -> Vec<SeederHeartbeat> {
    let mut merged = Vec::new();
    let mut seen_peers = std::collections::HashSet::new();
    let now = unix_timestamp();

    // Create sets to track which peers appear in both vectors
    let a_peers: HashSet<String> = a.iter().map(|hb| hb.peer_id.clone()).collect();
    let b_peers: HashSet<String> = b.iter().map(|hb| hb.peer_id.clone()).collect();
    let common_peers: HashSet<_> = a_peers.intersection(&b_peers).cloned().collect();

    // Filter and collect entries in one pass instead of using retain
    let filtered_a: Vec<_> = a.into_iter()
        .filter(|hb| {
            common_peers.contains(&hb.peer_id) || 
            hb.expires_at > now.saturating_sub(30) // 30s grace period
        })
        .collect();

    let filtered_b: Vec<_> = b.into_iter()
        .filter(|hb| {
            common_peers.contains(&hb.peer_id) || 
            hb.expires_at > now.saturating_sub(30) // 30s grace period
        })
        .collect();

    // Now work with the filtered vectors
    a = filtered_a;
    b = filtered_b;

    // Sort both vectors by peer_id for deterministic merging
    a.sort_by(|x, y| x.peer_id.cmp(&y.peer_id));
    b.sort_by(|x, y| x.peer_id.cmp(&y.peer_id));

    let mut a_iter = a.into_iter();
    let mut b_iter = b.into_iter();
    
    let mut next_a = a_iter.next();
    let mut next_b = b_iter.next();

    while let (Some(a_entry), Some(b_entry)) = (&next_a, &next_b) {
        match a_entry.peer_id.cmp(&b_entry.peer_id) {
            std::cmp::Ordering::Equal => {
                // For equal peer IDs, create a merged entry that:
                // 1. Takes the most recent heartbeat timestamp
                // 2. Uses the latest expiry time
                // 3. Extends the expiry if it's an active seeder (recent heartbeat)
                let latest_heartbeat = std::cmp::max(a_entry.last_heartbeat, b_entry.last_heartbeat);
                let latest_expiry = std::cmp::max(a_entry.expires_at, b_entry.expires_at);
                
                // If this is an active seeder (recent heartbeat), extend its expiry
                let new_expiry = if now.saturating_sub(latest_heartbeat) < FILE_HEARTBEAT_INTERVAL.as_secs() {
                    now.saturating_add(FILE_HEARTBEAT_TTL.as_secs())
                } else {
                    latest_expiry
                };
                
                let entry = SeederHeartbeat {
                    peer_id: a_entry.peer_id.clone(),
                    expires_at: new_expiry,
                    last_heartbeat: latest_heartbeat,
                };
                
                if !seen_peers.contains(&entry.peer_id) {
                    seen_peers.insert(entry.peer_id.clone());
                    merged.push(entry);
                }
                
                next_a = a_iter.next();
                next_b = b_iter.next();
            }
            std::cmp::Ordering::Less => {
                if !seen_peers.contains(&a_entry.peer_id) {
                    seen_peers.insert(a_entry.peer_id.clone());
                    merged.push(a_entry.clone());
                }
                next_a = a_iter.next();
            }
            std::cmp::Ordering::Greater => {
                if !seen_peers.contains(&b_entry.peer_id) {
                    seen_peers.insert(b_entry.peer_id.clone());
                    merged.push(b_entry.clone());
                }
                next_b = b_iter.next();
            }
        }
    }

    // Add remaining entries from a
    while let Some(entry) = next_a {
        if !seen_peers.contains(&entry.peer_id) {
            seen_peers.insert(entry.peer_id.clone());
            merged.push(entry);
        }
        next_a = a_iter.next();
    }

    // Add remaining entries from b
    while let Some(entry) = next_b {
        if !seen_peers.contains(&entry.peer_id) {
            seen_peers.insert(entry.peer_id.clone());
            merged.push(entry);
        }
        next_b = b_iter.next();
    }

    merged
}

fn prune_heartbeats(mut entries: Vec<SeederHeartbeat>, now: u64) -> Vec<SeederHeartbeat> {
    // Add a more generous grace period to prevent premature pruning
    // Use 30 seconds which is between the heartbeat interval (15s) and TTL (90s)
    let prune_threshold = now.saturating_sub(30); // 30 second grace period
    entries.retain(|hb| hb.expires_at > prune_threshold);
    entries.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
    entries
}

fn upsert_heartbeat(entries: &mut Vec<SeederHeartbeat>, peer_id: &str, now: u64) {
    let expires_at = now.saturating_add(FILE_HEARTBEAT_TTL.as_secs());
    
    // First remove any expired entries
    entries.retain(|hb| hb.expires_at > now);
    
    // Then update or add the new heartbeat
    if let Some(entry) = entries.iter_mut().find(|hb| hb.peer_id == peer_id) {
        entry.expires_at = expires_at;
        entry.last_heartbeat = now;
    } else {
        entries.push(SeederHeartbeat {
            peer_id: peer_id.to_string(),
            expires_at,
            last_heartbeat: now,
        });
    }
    
    // Sort by peer_id for consistent ordering
    entries.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
}

fn heartbeats_to_peer_list(entries: &[SeederHeartbeat]) -> Vec<String> {
    entries.iter().map(|hb| hb.peer_id.clone()).collect()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NatReachabilityState {
    Unknown,
    Public,
    Private,
}

impl Default for NatReachabilityState {
    fn default() -> Self {
        NatReachabilityState::Unknown
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NatConfidence {
    Low,
    Medium,
    High,
}

impl Default for NatConfidence {
    fn default() -> Self {
        NatConfidence::Low
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatHistoryItem {
    pub state: NatReachabilityState,
    pub confidence: NatConfidence,
    pub timestamp: u64,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
struct ReachabilityRecord {
    state: NatReachabilityState,
    confidence: NatConfidence,
    timestamp: SystemTime,
    summary: Option<String>,
}
/// thread-safe, mutable block store

#[derive(NetworkBehaviour)]
struct DhtBehaviour {
    kademlia: Kademlia<MemoryStore>,
    identify: identify::Behaviour,
    mdns: toggle::Toggle<Mdns>,
    bitswap: beetswap::Behaviour<MAX_MULTIHASH_LENGHT, RedbBlockstore>,
    ping: ping::Behaviour,
    proxy_rr: rr::Behaviour<ProxyCodec>,
    webrtc_signaling_rr: rr::Behaviour<WebRTCSignalingCodec>,
    key_request: rr::Behaviour<KeyRequestCodec>,
    autonat_client: toggle::Toggle<v2::client::Behaviour>,
    autonat_server: toggle::Toggle<v2::server::Behaviour>,
    relay_client: relay::client::Behaviour,
    relay_server: toggle::Toggle<relay::Behaviour>,
    dcutr: toggle::Toggle<dcutr::Behaviour>,
}
#[derive(Debug)]
pub enum DhtCommand {
    PublishFile {
        metadata: FileMetadata,
        response_tx: oneshot::Sender<FileMetadata>,
    },
    SearchFile(String),
    DownloadFile(FileMetadata, String),
    ConnectPeer(String),
    ConnectToPeerById(PeerId),
    DisconnectPeer(PeerId),
    SetPrivacyProxies {
        addresses: Vec<String>,
    },
    GetPeerCount(oneshot::Sender<usize>),
    Echo {
        peer: PeerId,
        payload: Vec<u8>,
        tx: oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Shutdown(oneshot::Sender<()>),
    StopPublish(String),
    HeartbeatFile {
        file_hash: String,
    },
    GetProviders {
        file_hash: String,
        sender: oneshot::Sender<Result<Vec<String>, String>>,
    },
    SendWebRTCOffer {
        peer: PeerId,
        offer_request: WebRTCOfferRequest,
        sender: oneshot::Sender<Result<WebRTCAnswerResponse, String>>,
    },
    SendMessageToPeer {
        target_peer_id: PeerId,
        message: serde_json::Value,
    },
    StoreBlock {
        cid: Cid,
        data: Vec<u8>,
    },
        StoreBlocks {
            blocks: Vec<(Cid, Vec<u8>)>, 
            root_cid: Cid,
            metadata: FileMetadata,
        },
        RequestFileAccess {
            seeder: PeerId,
            merkle_root: String,
            recipient_public_key: PublicKey,
            sender: oneshot::Sender<Result<EncryptedAesKeyBundle, String>>,
        },
    }
#[derive(Debug, Clone, Serialize)]
pub enum DhtEvent {
    // PeerDiscovered(String),
    // PeerConnected(String),
    // PeerDisconnected(String),
    PeerDiscovered {
        peer_id: String,
        addresses: Vec<String>,
    },
    PeerConnected {
        peer_id: String,
        address: Option<String>,
    },
    PeerDisconnected {
        peer_id: String,
    },
    FileDiscovered(FileMetadata),
    FileNotFound(String),
    DownloadedFile(FileMetadata),
    FileDownloaded {
        file_hash: String,
    },
    Error(String),
    Info(String),
    Warning(String),
    PublishedFile(FileMetadata),
    ProxyStatus {
        id: String,
        address: String,
        status: String,
        latency_ms: Option<u64>,
        error: Option<String>,
    },
    PeerRtt {
        peer: String,
        rtt_ms: u64,
    },
    EchoReceived {
        from: String,
        utf8: Option<String>,
        bytes: usize,
    },
    NatStatus {
        state: NatReachabilityState,
        confidence: NatConfidence,
        last_error: Option<String>,
        summary: Option<String>,
    },
    BitswapDataReceived {
        query_id: String,
        data: Vec<u8>,
    },
    BitswapError {
        query_id: String,
        error: String,
    },
    ReputationEvent {
        peer_id: String,
        event_type: String,
        impact: f64,
        data: serde_json::Value,
    },
    BitswapChunkDownloaded {
        file_hash: String,
        chunk_index: u32,
        total_chunks: u32,
        chunk_size: usize,
    },
    PaymentNotificationReceived {
        from_peer: String,
        payload: serde_json::Value,
    },
}

struct RelayState {
    blacklist: HashSet<PeerId>,
}

// ------------ Proxy Manager Structs and Enums ------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyMode {
    Off,
    Prefer,
    Strict,
}

impl PrivacyMode {
    pub fn from_str(mode: &str) -> Self {
        match mode.to_lowercase().as_str() {
            "strict" => PrivacyMode::Strict,
            "off" => PrivacyMode::Off,
            "prefer" => PrivacyMode::Prefer,
            _ => PrivacyMode::Prefer,
        }
    }
}

struct ProxyManager {
    targets: std::collections::HashSet<PeerId>,
    capable: std::collections::HashSet<PeerId>,
    online: std::collections::HashSet<PeerId>,
    relay_pending: std::collections::HashSet<PeerId>,
    relay_ready: std::collections::HashSet<PeerId>,
    // Privacy routing state
    privacy_routing_enabled: bool,
    trusted_proxy_nodes: std::collections::HashSet<PeerId>,
    privacy_mode: PrivacyMode,
    manual_trusted: std::collections::HashSet<PeerId>,
}

impl ProxyManager {
    fn set_target(&mut self, id: PeerId) {
        self.targets.insert(id);
    }
    fn clear_target(&mut self, id: &PeerId) {
        self.targets.remove(id);
    }
    fn set_capable(&mut self, id: PeerId) {
        self.capable.insert(id);
    }
    fn set_online(&mut self, id: PeerId) {
        self.online.insert(id);
    }
    fn set_offline(&mut self, id: &PeerId) {
        self.online.remove(id);
    }
    fn remove_all(&mut self, id: &PeerId) {
        self.targets.remove(id);
        self.capable.remove(id);
        self.online.remove(id);
        self.relay_pending.remove(id);
        self.relay_ready.remove(id);
        self.trusted_proxy_nodes.remove(id);
        self.manual_trusted.remove(id);
    }
    fn is_proxy(&self, id: &PeerId) -> bool {
        self.targets.contains(id) || self.capable.contains(id)
    }
    fn mark_relay_pending(&mut self, id: PeerId) -> bool {
        if self.relay_ready.contains(&id) {
            return false;
        }
        self.relay_pending.insert(id)
    }
    fn mark_relay_ready(&mut self, id: PeerId) -> bool {
        self.relay_pending.remove(&id);
        self.relay_ready.insert(id)
    }
    fn has_relay_request(&self, id: &PeerId) -> bool {
        self.relay_pending.contains(id) || self.relay_ready.contains(id)
    }

    // Privacy routing methods
    fn enable_privacy_routing(&mut self, mode: PrivacyMode) {
        self.privacy_routing_enabled = mode != PrivacyMode::Off;
        self.privacy_mode = mode;
        info!(
            "Privacy routing enabled in proxy manager (mode: {:?})",
            mode
        );
    }

    fn disable_privacy_routing(&mut self) {
        self.privacy_routing_enabled = false;
        self.privacy_mode = PrivacyMode::Off;
        info!("Privacy routing disabled in proxy manager");
    }

    fn is_privacy_routing_enabled(&self) -> bool {
        self.privacy_routing_enabled
    }

    fn privacy_mode(&self) -> PrivacyMode {
        self.privacy_mode
    }

    fn add_trusted_proxy_node(&mut self, peer_id: PeerId) {
        self.trusted_proxy_nodes.insert(peer_id);
    }

    fn remove_trusted_proxy_node(&mut self, peer_id: &PeerId) {
        self.trusted_proxy_nodes.remove(peer_id);
        self.manual_trusted.remove(peer_id);
    }

    fn is_trusted_proxy_node(&self, peer_id: &PeerId) -> bool {
        self.trusted_proxy_nodes.contains(peer_id)
    }

    fn get_trusted_proxy_nodes(&self) -> &std::collections::HashSet<PeerId> {
        &self.trusted_proxy_nodes
    }

    fn set_manual_trusted(&mut self, peers: &[PeerId]) {
        for peer in self.manual_trusted.drain() {
            self.trusted_proxy_nodes.remove(&peer);
        }

        for peer in peers {
            self.manual_trusted.insert(peer.clone());
            self.trusted_proxy_nodes.insert(peer.clone());
        }
    }

    fn select_proxy_for_routing(&self, target_peer: &PeerId) -> Option<PeerId> {
        if !self.privacy_routing_enabled {
            return None;
        }

        // Select a trusted proxy node that's online and not the target itself
        self.trusted_proxy_nodes
            .iter()
            .find(|&&proxy_id| {
                proxy_id != *target_peer
                    && self.online.contains(&proxy_id)
                    && self.capable.contains(&proxy_id)
            })
            .cloned()
    }
}

impl Default for ProxyManager {
    fn default() -> Self {
        Self {
            targets: std::collections::HashSet::new(),
            capable: std::collections::HashSet::new(),
            online: std::collections::HashSet::new(),
            relay_pending: std::collections::HashSet::new(),
            relay_ready: std::collections::HashSet::new(),
            privacy_routing_enabled: false,
            trusted_proxy_nodes: std::collections::HashSet::new(),
            privacy_mode: PrivacyMode::Off,
            manual_trusted: std::collections::HashSet::new(),
        }
    }
}

struct PendingEcho {
    peer: PeerId,
    tx: oneshot::Sender<Result<Vec<u8>, String>>,
}

// Runtime type for ProxyManager
type ProxyMgr = Arc<Mutex<ProxyManager>>;

// ----------------------------------------------------------

#[derive(Debug, Clone)]
enum SearchResponse {
    Found(FileMetadata),
    NotFound,
}

#[derive(Debug)]
struct PendingSearch {
    id: u64,
    sender: oneshot::Sender<SearchResponse>,
}

#[derive(Debug)]
struct PendingProviderQuery {
    id: u64,
    sender: oneshot::Sender<Result<Vec<String>, String>>,
}

#[derive(Debug, Clone, Default)]
struct DhtMetrics {
    last_bootstrap: Option<SystemTime>,
    last_success: Option<SystemTime>,
    last_error_at: Option<SystemTime>,
    last_error: Option<String>,
    bootstrap_failures: u64,
    listen_addrs: Vec<String>,
    reachability_state: NatReachabilityState,
    reachability_confidence: NatConfidence,
    last_reachability_change: Option<SystemTime>,
    last_probe_at: Option<SystemTime>,
    last_reachability_error: Option<String>,
    observed_addrs: Vec<String>,
    reachability_history: VecDeque<ReachabilityRecord>,
    success_streak: u32,
    failure_streak: u32,
    autonat_enabled: bool,
    // AutoRelay metrics
    autorelay_enabled: bool,
    active_relay_peer_id: Option<String>,
    relay_reservation_status: Option<String>,
    last_reservation_success: Option<SystemTime>,
    last_reservation_failure: Option<SystemTime>,
    reservation_renewals: u64,
    reservation_evictions: u64,
    // DCUtR metrics
    dcutr_enabled: bool,
    dcutr_hole_punch_attempts: u64,
    dcutr_hole_punch_successes: u64,
    dcutr_hole_punch_failures: u64,
    last_dcutr_success: Option<SystemTime>,
    last_dcutr_failure: Option<SystemTime>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DhtMetricsSnapshot {
    pub peer_count: usize,
    pub last_bootstrap: Option<u64>,
    pub last_peer_event: Option<u64>,
    pub last_error: Option<String>,
    pub last_error_at: Option<u64>,
    pub bootstrap_failures: u64,
    pub listen_addrs: Vec<String>,
    pub relay_listen_addrs: Vec<String>,
    pub reachability: NatReachabilityState,
    pub reachability_confidence: NatConfidence,
    pub last_reachability_change: Option<u64>,
    pub last_probe_at: Option<u64>,
    pub last_reachability_error: Option<String>,
    pub observed_addrs: Vec<String>,
    pub reachability_history: Vec<NatHistoryItem>,
    pub autonat_enabled: bool,
    // AutoRelay metrics
    pub autorelay_enabled: bool,
    pub active_relay_peer_id: Option<String>,
    pub relay_reservation_status: Option<String>,
    pub last_reservation_success: Option<u64>,
    pub last_reservation_failure: Option<u64>,
    pub reservation_renewals: u64,
    pub reservation_evictions: u64,
    // DCUtR metrics
    pub dcutr_enabled: bool,
    pub dcutr_hole_punch_attempts: u64,
    pub dcutr_hole_punch_successes: u64,
    pub dcutr_hole_punch_failures: u64,
    pub last_dcutr_success: Option<u64>,
    pub last_dcutr_failure: Option<u64>,
}

// ------Proxy Protocol Implementation------
#[derive(Clone, Debug, Default)]
struct ProxyCodec;

#[derive(Clone, Debug, Default)]
struct WebRTCSignalingCodec;

#[derive(Debug, Clone)]
struct EchoRequest(pub Vec<u8>);
#[derive(Debug, Clone)]
struct EchoResponse(pub Vec<u8>);

// WebRTC Signaling Protocol
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebRTCOfferRequest {
    pub offer_sdp: String,
    pub file_hash: String,
    pub requester_peer_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebRTCAnswerResponse {
    pub answer_sdp: String,
}

// 4byte LE length prefix
async fn read_framed<T: FAsyncRead + Unpin + Send>(io: &mut T) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    io.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut data = vec![0u8; len];
    io.read_exact(&mut data).await?;
    Ok(data)
}
async fn write_framed<T: FAsyncWrite + Unpin + Send>(
    io: &mut T,
    data: Vec<u8>,
) -> std::io::Result<()> {
    io.write_all(&(data.len() as u32).to_le_bytes()).await?;
    io.write_all(&data).await?;
    io.flush().await
}

#[async_trait::async_trait]
impl rr::Codec for ProxyCodec {
    type Protocol = String;
    type Request = EchoRequest;
    type Response = EchoResponse;

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Request>
    where
        // CORRECTED: FAsyncRead is now correctly defined via the new imports
        T: FAsyncRead + Unpin + Send,
    {
        Ok(EchoRequest(read_framed(io).await?))
    }
    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Response>
    where
        // CORRECTED: FAsyncRead is now correctly defined via the new imports
        T: FAsyncRead + Unpin + Send,
    {
        Ok(EchoResponse(read_framed(io).await?))
    }
    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        EchoRequest(data): EchoRequest,
    ) -> std::io::Result<()>
    where
        // CORRECTED: FAsyncWrite is now correctly defined via the new imports
        T: FAsyncWrite + Unpin + Send,
    {
        write_framed(io, data).await
    }
    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        EchoResponse(data): EchoResponse,
    ) -> std::io::Result<()>
    where
        // CORRECTED: FAsyncWrite is now correctly defined via the new imports
        T: FAsyncWrite + Unpin + Send,
    {
        write_framed(io, data).await
    }
}

// ------WebRTC Signaling Protocol Implementation------
#[async_trait::async_trait]
impl rr::Codec for WebRTCSignalingCodec {
    type Protocol = String;
    type Request = WebRTCOfferRequest;
    type Response = WebRTCAnswerResponse;

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Request>
    where
        T: FAsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        let request: WebRTCOfferRequest = serde_json::from_slice(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(request)
    }
    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Response>
    where
        T: FAsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        let response: WebRTCAnswerResponse = serde_json::from_slice(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(response)
    }
    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        request: WebRTCOfferRequest,
    ) -> std::io::Result<()>
    where
        T: FAsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&request)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }
    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        response: WebRTCAnswerResponse,
    ) -> std::io::Result<()>
    where
        T: FAsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&response)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }
}
#[derive(Clone)]
struct Socks5Transport {
    proxy: SocketAddr,
}

#[async_trait]
impl Transport for Socks5Transport {
    type Output = Box<dyn AsyncIo>;
    type Error = io::Error;
    type ListenerUpgrade = futures::future::Pending<Result<Self::Output, Self::Error>>;
    // FIXED E0412: Use imported BoxFuture
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    // FIXED E0050, E0046: Corrected implementation
    fn listen_on(
        &mut self,
        _id: ListenerId,
        _addr: libp2p::Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        Err(TransportError::Other(io::Error::new(
            io::ErrorKind::Other,
            "SOCKS5 transport does not support listening",
        )))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }

    fn dial(
        &mut self,
        addr: libp2p::Multiaddr,
        _opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let proxy = self.proxy;

        // Convert Multiaddr to string for SOCKS5 connection
        let target = match addr_to_socket_addr(&addr) {
            Some(socket_addr) => socket_addr.to_string(),
            None => {
                return Err(TransportError::Other(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid address for SOCKS5",
                )))
            }
        };

        Ok(async move {
            let stream = Socks5Stream::connect(proxy, target)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

            // CORRECT: Convert tokio stream to futures stream via .compat().
            let compat = stream.compat();
            // The compat stream correctly implements FAsyncRead/FAsyncWrite required by AsyncIo.
            Ok(Box::new(compat) as Box<dyn AsyncIo>)
        }
        .boxed())
    }
}

// Helper function to convert Multiaddr to SocketAddr
fn addr_to_socket_addr(addr: &libp2p::Multiaddr) -> Option<SocketAddr> {
    use libp2p::multiaddr::Protocol;

    let mut iter = addr.iter();
    match (iter.next(), iter.next()) {
        (Some(Protocol::Ip4(ip)), Some(Protocol::Tcp(port))) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        (Some(Protocol::Ip6(ip)), Some(Protocol::Tcp(port))) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        _ => None,
    }
}

pub fn build_relay_listen_addr(base: &Multiaddr) -> Option<Multiaddr> {
    let mut out = base.clone();
    let has_p2p = out.iter().any(|p| matches!(p, Protocol::P2p(_)));
    if !has_p2p {
        return None;
    }
    out.push(Protocol::P2pCircuit);
    Some(out)
}

fn is_relay_candidate(peer_id: &PeerId, relay_candidates: &HashSet<String>) -> bool {
    if relay_candidates.is_empty() {
        return false;
    }

    let peer_str = peer_id.to_string();
    relay_candidates.iter().any(|candidate| {
        // Check if the candidate multiaddr contains this peer ID
        candidate.contains(&peer_str)
    })
}

fn peer_id_from_multiaddr_str(s: &str) -> Option<PeerId> {
    if let Ok(ma) = s.parse::<Multiaddr>() {
        let mut last_p2p: Option<PeerId> = None;
        for p in ma.iter() {
            if let Protocol::P2p(mh) = p {
                if let Ok(pid) = PeerId::from_multihash(mh.into()) {
                    last_p2p = Some(pid);
                }
            }
        }
        return last_p2p;
    }

    if let Ok(pid) = s.parse::<PeerId>() {
        return Some(pid);
    }
    None
}

fn should_try_relay(
    pid: &PeerId,
    relay_candidates: &HashSet<String>,
    blacklist: &HashSet<PeerId>,
    cooldown: &HashMap<PeerId, Instant>,
) -> bool {
    // 1) Check if the peer ID is in the preferred/bootstrap candidates
    if relay_candidates.is_empty() {
        return false;
    }
    let peer_str = pid.to_string();
    let in_candidates = relay_candidates.iter().any(|cand| cand.contains(&peer_str));
    if !in_candidates {
        return false;
    }
    // 2) Check permanent blacklist
    if blacklist.contains(pid) {
        tracing::debug!("skip blacklisted relay candidate {}", pid);
        return false;
    }
    // 3) Check cooldown
    if let Some(until) = cooldown.get(pid) {
        if Instant::now() < *until {
            tracing::debug!("skip cooldown relay candidate {} until {:?}", pid, until);
            return false;
        }
    }
    true
}

/// candidates(HashSet<String>) → (PeerId, Multiaddr)
fn filter_relay_candidates(
    relay_candidates: &HashSet<String>,
    blacklist: &HashSet<PeerId>,
    cooldown: &HashMap<PeerId, Instant>,
) -> Vec<(PeerId, Multiaddr)> {
    let now = Instant::now();
    let mut out = Vec::new();
    for cand in relay_candidates {
        if let Ok(ma) = cand.parse::<Multiaddr>() {
            // PeerId extraction
            let mut pid_opt: Option<PeerId> = None;
            for p in ma.iter() {
                if let Protocol::P2p(mh) = p {
                    if let Ok(pid) = PeerId::from_multihash(mh.into()) {
                        pid_opt = Some(pid);
                    }
                }
            }
            if let Some(pid) = pid_opt {
                if !blacklist.contains(&pid) {
                    if let Some(until) = cooldown.get(&pid) {
                        if Instant::now() < *until {
                            tracing::debug!(
                                "skip cooldown relay candidate {} until {:?}",
                                pid,
                                until
                            );
                            continue;
                        }
                    }
                    out.push((pid, ma.clone()));
                } else {
                    tracing::debug!("skip blacklisted relay candidate {}", pid);
                }
            }
        }
    }
    out
}

fn extract_relay_peer(address: &Multiaddr) -> Option<PeerId> {
    use libp2p::multiaddr::Protocol;

    let mut last_p2p: Option<PeerId> = None;
    for protocol in address.iter() {
        match protocol {
            Protocol::P2p(peer_id) => {
                last_p2p = Some(peer_id.clone());
            }
            Protocol::P2pCircuit => {
                return last_p2p.clone();
            }
            _ => {}
        }
    }
    None
}

enum RelayTransportOutput {
    Relay(relay::client::Connection),
    Direct(Box<dyn AsyncIo>),
}

impl FAsyncRead for RelayTransportOutput {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        // SAFETY: We never move the inner value after pinning, so projecting via
        // `get_unchecked_mut` and re-pinning each variant is sound.
        unsafe {
            match self.get_unchecked_mut() {
                RelayTransportOutput::Relay(conn) => Pin::new_unchecked(conn).poll_read(cx, buf),
                RelayTransportOutput::Direct(stream) => {
                    Pin::new_unchecked(stream.as_mut()).poll_read(cx, buf)
                }
            }
        }
    }
}

impl FAsyncWrite for RelayTransportOutput {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        unsafe {
            match self.get_unchecked_mut() {
                RelayTransportOutput::Relay(conn) => Pin::new_unchecked(conn).poll_write(cx, buf),
                RelayTransportOutput::Direct(stream) => {
                    Pin::new_unchecked(stream.as_mut()).poll_write(cx, buf)
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        // SAFETY: See comment in `poll_read`; variants remain pinned in place.
        unsafe {
            match self.get_unchecked_mut() {
                RelayTransportOutput::Relay(conn) => Pin::new_unchecked(conn).poll_flush(cx),
                RelayTransportOutput::Direct(stream) => {
                    Pin::new_unchecked(stream.as_mut()).poll_flush(cx)
                }
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        // SAFETY: See comment in `poll_read`; variants remain pinned in place.
        unsafe {
            match self.get_unchecked_mut() {
                RelayTransportOutput::Relay(conn) => Pin::new_unchecked(conn).poll_close(cx),
                RelayTransportOutput::Direct(stream) => {
                    Pin::new_unchecked(stream.as_mut()).poll_close(cx)
                }
            }
        }
    }
}

impl DhtMetricsSnapshot {
    fn from(metrics: DhtMetrics, peer_count: usize) -> Self {
        fn to_secs(ts: SystemTime) -> Option<u64> {
            ts.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
        }

        let DhtMetrics {
            last_bootstrap,
            last_success,
            last_error_at,
            last_error,
            bootstrap_failures,
            listen_addrs,
            reachability_state,
            reachability_confidence,
            last_reachability_change,
            last_probe_at,
            last_reachability_error,
            observed_addrs,
            reachability_history,
            autonat_enabled,
            // AutoRelay metrics
            autorelay_enabled,
            active_relay_peer_id,
            relay_reservation_status,
            last_reservation_success,
            last_reservation_failure,
            reservation_renewals,
            reservation_evictions,
            // DCUtR metrics
            dcutr_enabled,
            dcutr_hole_punch_attempts,
            dcutr_hole_punch_successes,
            dcutr_hole_punch_failures,
            last_dcutr_success,
            last_dcutr_failure,
            ..
        } = metrics;

        // Derive relay listen addresses (those that include p2p-circuit)
        let relay_listen_addrs: Vec<String> = listen_addrs
            .iter()
            .filter(|a| a.contains("p2p-circuit"))
            .cloned()
            .collect();

        let history: Vec<NatHistoryItem> = reachability_history
            .into_iter()
            .map(|record| NatHistoryItem {
                state: record.state,
                confidence: record.confidence,
                timestamp: record
                    .timestamp
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs())
                    .unwrap_or_default(),
                summary: record.summary,
            })
            .collect();

        DhtMetricsSnapshot {
            peer_count,
            last_bootstrap: last_bootstrap.and_then(to_secs),
            last_peer_event: last_success.and_then(to_secs),
            last_error,
            last_error_at: last_error_at.and_then(to_secs),
            bootstrap_failures,
            listen_addrs,
            relay_listen_addrs,
            reachability: reachability_state,
            reachability_confidence,
            last_reachability_change: last_reachability_change.and_then(to_secs),
            last_probe_at: last_probe_at.and_then(to_secs),
            last_reachability_error,
            observed_addrs,
            reachability_history: history,
            autonat_enabled,
            // AutoRelay metrics
            autorelay_enabled,
            active_relay_peer_id,
            relay_reservation_status,
            last_reservation_success: last_reservation_success.and_then(to_secs),
            last_reservation_failure: last_reservation_failure.and_then(to_secs),
            reservation_renewals,
            reservation_evictions,
            // DCUtR metrics
            dcutr_enabled,
            dcutr_hole_punch_attempts,
            dcutr_hole_punch_successes,
            dcutr_hole_punch_failures,
            last_dcutr_success: last_dcutr_success.and_then(to_secs),
            last_dcutr_failure: last_dcutr_failure.and_then(to_secs),
        }
    }
}

impl DhtMetrics {
    fn record_listen_addr(&mut self, addr: &Multiaddr) {
        let addr_str = addr.to_string();
        if !self
            .listen_addrs
            .iter()
            .any(|existing| existing == &addr_str)
        {
            self.listen_addrs.push(addr_str);
        }
    }

    fn record_observed_addr(&mut self, addr: &Multiaddr) {
        let addr_str = addr.to_string();
        if self
            .observed_addrs
            .iter()
            .any(|existing| existing == &addr_str)
        {
            return;
        }
        self.observed_addrs.push(addr_str);
        if self.observed_addrs.len() > 8 {
            self.observed_addrs.remove(0);
        }
    }

    fn remove_observed_addr(&mut self, addr: &Multiaddr) {
        let addr_str = addr.to_string();
        self.observed_addrs.retain(|existing| existing != &addr_str);
    }

    fn confidence_from_streak(&self, streak: u32) -> NatConfidence {
        match streak {
            0 | 1 => NatConfidence::Low,
            2 | 3 => NatConfidence::Medium,
            _ => NatConfidence::High,
        }
    }

    fn push_history(&mut self, record: ReachabilityRecord) {
        self.reachability_history.push_front(record);
        if self.reachability_history.len() > 10 {
            self.reachability_history.pop_back();
        }
    }

    fn update_reachability(&mut self, state: NatReachabilityState, summary: Option<String>) {
        let now = SystemTime::now();
        self.last_probe_at = Some(now);

        match state {
            NatReachabilityState::Public => {
                self.success_streak = self.success_streak.saturating_add(1);
                self.failure_streak = 0;
                self.last_reachability_error = None;
                self.reachability_confidence = self.confidence_from_streak(self.success_streak);
            }
            NatReachabilityState::Private => {
                self.failure_streak = self.failure_streak.saturating_add(1);
                self.success_streak = 0;
                if let Some(ref s) = summary {
                    self.last_reachability_error = Some(s.clone());
                }
                self.reachability_confidence = self.confidence_from_streak(self.failure_streak);
            }
            NatReachabilityState::Unknown => {
                self.success_streak = 0;
                self.failure_streak = 0;
                self.reachability_confidence = NatConfidence::Low;
                self.last_reachability_error = summary.clone();
            }
        }

        let state_changed = self.reachability_state != state;
        self.reachability_state = state;

        if state_changed {
            self.last_reachability_change = Some(now);
        }

        if state_changed || summary.is_some() {
            self.push_history(ReachabilityRecord {
                state,
                confidence: self.reachability_confidence,
                timestamp: now,
                summary,
            });
        }
    }

    fn note_probe_failure(&mut self, error: String) {
        self.last_reachability_error = Some(error);
    }
}

async fn notify_pending_searches(
    pending: &Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    key: &str,
    response: SearchResponse,
) {
    let waiters = {
        let mut pending = pending.lock().await;
        pending.remove(key)
    };

    if let Some(waiters) = waiters {
        for waiter in waiters {
            let _ = waiter.sender.send(response.clone());
        }
    }
}

async fn run_dht_node(
    mut swarm: Swarm<DhtBehaviour>,
    peer_id: PeerId,
    mut cmd_rx: mpsc::Receiver<DhtCommand>,
    event_tx: mpsc::Sender<DhtEvent>,
    connected_peers: Arc<Mutex<HashSet<PeerId>>>,
    metrics: Arc<Mutex<DhtMetrics>>,
    pending_echo: Arc<Mutex<HashMap<rr::OutboundRequestId, PendingEcho>>>,
    pending_searches: Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    proxy_mgr: ProxyMgr,
    peer_selection: Arc<Mutex<PeerSelectionService>>,
    received_chunks: Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: Option<Arc<FileTransferService>>,
    chunk_manager: Option<Arc<ChunkManager>>,
    pending_webrtc_offers: Arc<
        Mutex<
            HashMap<rr::OutboundRequestId, oneshot::Sender<Result<WebRTCAnswerResponse, String>>>,
        >,
    >,
    pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>>,
    root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>>,
    active_downloads: Arc<Mutex<HashMap<String, Arc<Mutex<ActiveDownload>>>>>,
    get_providers_queries: Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>>,
    seeder_heartbeats_cache: Arc<Mutex<HashMap<String, FileHeartbeatCacheEntry>>>,
    pending_heartbeat_updates: Arc<Mutex<HashSet<String>>>,
    pending_keyword_indexes: Arc<Mutex<HashMap<kad::QueryId, PendingKeywordIndex>>>,
    is_bootstrap: bool,
    enable_autorelay: bool,
    relay_candidates: HashSet<String>,
    chunk_size: usize,
    bootstrap_peer_ids: HashSet<PeerId>,
) {
    // Track peers that support relay (discovered via identify protocol)
    let relay_capable_peers: Arc<Mutex<HashMap<PeerId, Vec<Multiaddr>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut dht_maintenance_interval = tokio::time::interval(Duration::from_secs(30 * 60));
    dht_maintenance_interval.tick().await;
    // fast heartbeat-driven updater: run at FILE_HEARTBEAT_INTERVAL to keep provider records fresh
    let mut heartbeat_maintenance_interval = tokio::time::interval(FILE_HEARTBEAT_INTERVAL);
    heartbeat_maintenance_interval.tick().await;
    // Periodic bootstrap interval

    /// Creates a proper circuit relay address for connecting through a relay peer
    /// Returns a properly formatted Multiaddr for circuit relay connections
    fn create_circuit_relay_address(
        relay_peer_id: &PeerId,
        target_peer_id: &PeerId,
    ) -> Result<Multiaddr, String> {
        // For Circuit Relay v2, the address format is typically:
        // /p2p/{relay_peer_id}/p2p-circuit
        // The target peer is specified in the relay reservation/request

        let relay_addr = Multiaddr::empty()
            .with(Protocol::P2p(*relay_peer_id))
            .with(Protocol::P2pCircuit);

        // Validate the constructed address
        if relay_addr.to_string().contains(&relay_peer_id.to_string()) {
            info!("Created circuit relay address: {}", relay_addr);
            Ok(relay_addr)
        } else {
            Err(format!(
                "Failed to create valid circuit relay address for relay {}",
                relay_peer_id
            ))
        }
    }

    /// Enhanced circuit relay address creation with multiple fallback strategies
    fn create_circuit_relay_address_robust(
        relay_peer_id: &PeerId,
        target_peer_id: &PeerId,
    ) -> Multiaddr {
        // Strategy 1: Standard Circuit Relay v2 address
        match create_circuit_relay_address(relay_peer_id, target_peer_id) {
            Ok(addr) => return addr,
            Err(e) => {
                warn!("Standard relay address creation failed: {}", e);
            }
        }

        // Strategy 2: Try with relay port specification (if available)
        // Some relay implementations may require explicit port specification
        let relay_with_port = Multiaddr::empty()
            .with(Protocol::P2p(*relay_peer_id))
            .with(Protocol::Tcp(4001)) // Default libp2p port
            .with(Protocol::P2pCircuit);

        if relay_with_port
            .to_string()
            .contains(&relay_peer_id.to_string())
        {
            info!(
                "Created circuit relay address with port: {}",
                relay_with_port
            );
            return relay_with_port;
        }

        // Strategy 3: Fallback to basic circuit address
        warn!("Using basic fallback circuit relay address construction");
        Multiaddr::empty()
            .with(Protocol::P2p(*relay_peer_id))
            .with(Protocol::P2pCircuit)
    }

    let mut shutdown_ack: Option<oneshot::Sender<()>> = None;
    let mut ping_failures: HashMap<PeerId, u8> = HashMap::new();
    let mut relay_blacklist: HashSet<PeerId> = HashSet::new();
    let mut relay_cooldown: HashMap<PeerId, Instant> = HashMap::new();
    let mut last_tried_relay: Option<PeerId> = None;

    let queries: HashMap<beetswap::QueryId, u32> = HashMap::new();
    let downloaded_chunks: HashMap<usize, Vec<u8>> = HashMap::new();
    let current_metadata: Option<FileMetadata> = None;

    #[derive(Debug, Clone, Copy)]
    enum RelayErrClass {
        Permanent,
        Transient,
    }

    fn classify_err_str(s: &str) -> RelayErrClass {
        if s.contains("Reservation(Unsupported)") || s.contains("Denied") {
            RelayErrClass::Permanent
        } else {
            RelayErrClass::Transient
        }
    }

    fn parse_peer_id_from_ma(ma: &Multiaddr) -> Option<PeerId> {
        use libp2p::multiaddr::Protocol;
        let mut out = None;
        for p in ma.iter() {
            if let Protocol::P2p(mh) = p {
                if let Ok(pid) = PeerId::from_multihash(mh.into()) {
                    out = Some(pid);
                }
            }
        }
        out
    }

    let mut filtered_relays: Vec<(PeerId, Multiaddr)> = Vec::new();
    for cand in &relay_candidates {
        if let Ok(base) = cand.parse::<Multiaddr>() {
            if let Some(pid) = parse_peer_id_from_ma(&base) {
                if relay_blacklist.contains(&pid) {
                    tracing::debug!("skip blacklisted relay candidate {}", pid);
                    continue;
                }
                if let Some(until) = relay_cooldown.get(&pid) {
                    if Instant::now() < *until {
                        tracing::debug!("skip cooldown relay candidate {} until {:?}", pid, until);
                        continue;
                    }
                }
                filtered_relays.push((pid, base));
            }
        }
    }

    if filtered_relays.is_empty() {
        tracing::warn!("No usable relay candidates after blacklist/cooldown filtering");
    } else {
        tracing::info!("Using {} filtered relay candidates", filtered_relays.len());
        for (i, (pid, addr)) in filtered_relays.iter().take(5).enumerate() {
            tracing::info!("   Filtered {}: {} via {}", i + 1, pid, addr);
        }
    }

    for (pid, mut base_addr) in filtered_relays {
        use libp2p::multiaddr::Protocol;
        last_tried_relay = Some(pid);
        base_addr.push(Protocol::P2pCircuit);
        tracing::info!("📡 Attempting to listen via relay {} at {}", pid, base_addr);
        if let Err(e) = swarm.listen_on(base_addr.clone()) {
            tracing::warn!("listen_on via relay {} failed: {}", pid, e);
            // Temporary failure: 10min cooldown
            relay_cooldown.insert(pid, Instant::now() + Duration::from_secs(600));
        }
    }

    // First attempt: filter candidates + try listen_on /p2p-circuit
    let filtered_relays =
        filter_relay_candidates(&relay_candidates, &relay_blacklist, &relay_cooldown);
    if filtered_relays.is_empty() {
        tracing::warn!("No usable relay candidates after blacklist/cooldown filtering");
    } else {
        tracing::info!("Using {} filtered relay candidates", filtered_relays.len());
        for (i, (pid, addr)) in filtered_relays.iter().take(5).enumerate() {
            tracing::info!("   Filtered {}: {} via {}", i + 1, pid, addr);
        }
        for (pid, mut base_addr) in filtered_relays {
            last_tried_relay = Some(pid);
            base_addr.push(Protocol::P2pCircuit);
            tracing::info!("📡 Attempting to listen via relay {} at {}", pid, base_addr);
            if let Err(e) = swarm.listen_on(base_addr.clone()) {
                tracing::warn!("listen_on via relay {} failed: {}", pid, e);
                // Temporary failure: 10min cooldown
                relay_cooldown.insert(pid, Instant::now() + Duration::from_secs(600));
            }
        }
    }

    'outer: loop {
        tokio::select! {
            // periodic maintenance tick - prune expired seeder heartbeats and update DHT
            // Fast heartbeat tick — refresh DHT records for files this node is actively seeding
            _ = heartbeat_maintenance_interval.tick() => {
                let now = unix_timestamp();
                let my_id = peer_id.to_string();
                let mut updated_records: Vec<(String, Vec<u8>)> = Vec::new();

                {
                    let mut cache = seeder_heartbeats_cache.lock().await;
                    for (file_hash, entry) in cache.iter_mut() {
                        // Prune expired entries first
                        entry.heartbeats = prune_heartbeats(entry.heartbeats.clone(), now);

                        // Only refresh records if this node is listed as a seeder
                        if entry.heartbeats.iter().any(|hb| hb.peer_id == my_id) {
                            // ensure our own heartbeat is up-to-date in cache
                            for hb in entry.heartbeats.iter_mut() {
                                if hb.peer_id == my_id {
                                    hb.last_heartbeat = now;
                                    hb.expires_at = now.saturating_add(FILE_HEARTBEAT_TTL.as_secs());
                                }
                            }

                            // update metadata fields
                            let seeder_strings = heartbeats_to_peer_list(&entry.heartbeats);
                            entry.metadata["seeders"] = serde_json::Value::Array(
                                seeder_strings
                                    .iter()
                                    .cloned()
                                    .map(serde_json::Value::String)
                                    .collect(),
                            );
                            entry.metadata["seederHeartbeats"] =
                                serde_json::to_value(&entry.heartbeats)
                                    .unwrap_or_else(|_| serde_json::Value::Array(vec![]));

                            if let Ok(bytes) = serde_json::to_vec(&entry.metadata) {
                                updated_records.push((file_hash.clone(), bytes));
                            }
                        }
                    }
                } // release cache lock

                // Perform DHT updates for seeder heartbeats (non-blocking best-effort)
                        // Push updated records to Kademlia for each updated file
                        for (file_hash, bytes) in updated_records {
                            let key = kad::RecordKey::new(&file_hash.as_bytes());
                            let record = Record {
                                key: key.clone(),
                                value: bytes.clone(),
                                publisher: Some(peer_id.clone()),
                                expires: None,
                            };
                            if let Err(e) =
                                swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One)
                            {
                                warn!("Failed to refresh DHT record after disconnect for {}: {}", file_hash, e);
                            } else {
                                debug!("Refreshed DHT record for {} after peer {} disconnected", file_hash, peer_id);
                            }

                            // notify UI with updated metadata so frontend refreshes immediately
                            if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                if let (Some(merkle_root), Some(file_name), Some(file_size), Some(created_at)) = (
                                    json_val.get("merkle_root").and_then(|v| v.as_str()),
                                    json_val.get("file_name").and_then(|v| v.as_str()),
                                    json_val.get("file_size").and_then(|v| v.as_u64()),
                                    json_val.get("created_at").and_then(|v| v.as_u64()),
                                ) {
                                    let seeders = json_val
                                        .get("seeders")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                                        .unwrap_or_default();

                                    let metadata = FileMetadata {
                                        merkle_root: merkle_root.to_string(),
                                        file_name: file_name.to_string(),
                                        file_size,
                                        file_data: Vec::new(),
                                        seeders,
                                        created_at,
                                        mime_type: json_val.get("mime_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        is_encrypted: json_val.get("is_encrypted").and_then(|v| v.as_bool()).unwrap_or(false),
                                        encryption_method: json_val.get("encryption_method").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        key_fingerprint: json_val.get("key_fingerprint").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        version: json_val.get("version").and_then(|v| v.as_u64()).map(|u| u as u32),
                                        parent_hash: json_val.get("parent_hash").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        cids: json_val.get("cids").and_then(|v| serde_json::from_value::<Option<Vec<Cid>>>(v.clone()).ok()).unwrap_or(None),
                                        encrypted_key_bundle: json_val.get("encryptedKeyBundle").and_then(|v| serde_json::from_value::<Option<crate::encryption::EncryptedAesKeyBundle>>(v.clone()).ok()).unwrap_or(None),
                                        is_root: json_val.get("is_root").and_then(|v| v.as_bool()).unwrap_or(true),
                                        price: json_val.get("price").and_then(|v| v.as_f64()),
                                        uploader_address: json_val.get("uploader_address").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        ..Default::default()
                                    };
                                    let _ = event_tx.send(DhtEvent::FileDiscovered(metadata)).await;
                                }
                            }
                        }
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(DhtCommand::Shutdown(ack)) => {
                        info!("Received shutdown signal for DHT node");
                        shutdown_ack = Some(ack);
                        break 'outer;
                    }
                    Some(DhtCommand::PublishFile { mut metadata, response_tx }) => {
                        // If file_data is NOT empty (non-encrypted files or inline data),
                        // create blocks, generate a Merkle root, and a root CID.
                        if !metadata.file_data.is_empty() {
                            let blocks = split_into_blocks(&metadata.file_data, chunk_size);
                            let mut block_cids = Vec::new();
                            let mut original_chunk_hashes: Vec<[u8; 32]> = Vec::new();

                            for (idx, block) in blocks.iter().enumerate() {
                                let cid = match block.cid() {
                                    Ok(c) => c,
                                    Err(e) => {
                                        error!("failed to get cid for block: {}", e);
                                        let _ = event_tx.send(DhtEvent::Error(format!("failed to get cid for block: {}", e))).await;
                                        return;
                                    }
                                };
                                // Also hash the original data for the Merkle root
                                original_chunk_hashes.push(Sha256Hasher::hash(block.data()));

                                println!("block {} size={} cid={}", idx, block.data().len(), cid);

                                match swarm.behaviour_mut().bitswap.insert_block::<MAX_MULTIHASH_LENGHT>(cid.clone(), block.data().to_vec()){
                                    Ok(_) => {
                                        info!("📦 Stored block {} (size: {} bytes) in Bitswap blockstore", cid, block.data().len());
                                    },
                                    Err(e) => {
                                        error!("failed to store block {}: {}", cid, e);
                                        let _ = event_tx.send(DhtEvent::Error(format!("failed to store block {}: {}", cid, e))).await;
                                        return;
                                    }
                                };
                                block_cids.push(cid);
                            }

                            // Build the Merkle tree from original chunk hashes
                            let merkle_tree = MerkleTree::<Sha256Hasher>::from_leaves(&original_chunk_hashes);
                            let merkle_root = merkle_tree.root().ok_or("Failed to compute Merkle root").unwrap();

                            // Create root block containing just the CIDs
                            let root_block_data = match serde_json::to_vec(&block_cids) {
                                Ok(data) => data,
                                Err(e) => {
                                    eprintln!("Failed to serialize CIDs: {}", e);
                                    return;
                                }
                            };

                            // Store root block in Bitswap
                            let root_cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(&root_block_data));
                            match swarm.behaviour_mut().bitswap.insert_block::<MAX_MULTIHASH_LENGHT>(root_cid.clone(), root_block_data.clone()) {
                                Ok(_) => {
                                    info!("🌳 Stored ROOT block {} (size: {} bytes, contains {} CIDs) in Bitswap blockstore", root_cid, root_block_data.len(), block_cids.len());
                                },
                                Err(e) => {
                                    error!("failed to store root block: {}", e);
                                    let _ = event_tx.send(DhtEvent::Error(format!("failed to store root block: {}", e))).await;
                                    return;
                                }
                            }

                            // The file_hash is the Merkle Root. The root_cid is for retrieval.
                            metadata.merkle_root = hex::encode(merkle_root);
                            metadata.cids = Some(vec![root_cid]); // Store root CID for bitswap retrieval
                            metadata.file_data.clear(); // Don't store full data in DHT record

                            println!("Publishing file with root CID: {} (merkle_root: {:?})",
                                root_cid, metadata.merkle_root);
                        } else {
                            // File data is empty - chunks and root block are already in Bitswap
                            // (from streaming upload or pre-processed encrypted file)
                            // Use the provided file_hash (which should already be a CID)
                            println!("Publishing file with pre-computed Merkle root: {} and CID: {:?}",
                                metadata.merkle_root, metadata.cids);
                        }

                        let now = unix_timestamp();
                        let peer_id_str = peer_id.to_string();
                        let existing_heartbeats = {
                            let cache = seeder_heartbeats_cache.lock().await;
                            cache
                                .get(&metadata.merkle_root)
                                .map(|entry| entry.heartbeats.clone())
                                .unwrap_or_default()
                        };
                        let mut heartbeat_entries = existing_heartbeats;
                        upsert_heartbeat(&mut heartbeat_entries, &peer_id_str, now);
                        let active_heartbeats = prune_heartbeats(heartbeat_entries, now);
                        metadata.seeders = heartbeats_to_peer_list(&active_heartbeats);

                        // Store minimal metadata in DHT
                        println!("💾 DHT: About to serialize metadata with price: {:?}, uploader: {:?}", metadata.price, metadata.uploader_address);

                        let dht_metadata = serde_json::json!({
                            "file_hash":metadata.merkle_root,
                            "merkle_root": metadata.merkle_root,
                            "file_name": metadata.file_name,
                            "file_size": metadata.file_size,
                            "created_at": metadata.created_at,
                            "mime_type": metadata.mime_type,
                            "is_encrypted": metadata.is_encrypted,
                            "encryption_method": metadata.encryption_method,
                            "key_fingerprint": metadata.key_fingerprint,
                            "version": metadata.version,
                            "parent_hash": metadata.parent_hash,
                            "cids": metadata.cids, // The root CID for Bitswap
                            "encrypted_key_bundle": metadata.encrypted_key_bundle,
                            "seeders": metadata.seeders,
                            "seederHeartbeats": active_heartbeats,
                            "price": metadata.price,
                            "uploader_address": metadata.uploader_address,
                        });

                        println!("💾 DHT: Serialized metadata JSON: {}", serde_json::to_string(&dht_metadata).unwrap_or_else(|_| "error".to_string()));

                        {
                            let mut cache = seeder_heartbeats_cache.lock().await;
                            cache.insert(
                                metadata.merkle_root.clone(),
                                FileHeartbeatCacheEntry {
                                    heartbeats: active_heartbeats.clone(),
                                    metadata: dht_metadata.clone(),
                                },
                            );
                        }

                        let record_key = kad::RecordKey::new(&metadata.merkle_root.as_bytes());
                        {
                            let mut pending = pending_heartbeat_updates.lock().await;
                            pending.insert(metadata.merkle_root.clone());
                        }
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .get_record(record_key.clone());

                        let dht_record_data = match serde_json::to_vec(&dht_metadata) {
                            Ok(data) => data,
                            Err(e) => {
                                eprintln!("Failed to serialize DHT metadata: {}", e);
                                return;
                            }
                        };

                        let record = Record {
                                    key: record_key.clone(),
                                    value: dht_record_data,
                                    publisher: Some(peer_id),
                                    expires: None,
                                };

                        // Determine appropriate quorum based on number of connected peers
                        let connected_peers_count = connected_peers.lock().await.len();
                        let min_replication_peers = 3; // Require at least 3 peers for stronger replication
                        
                        let quorum = if connected_peers_count >= min_replication_peers {
                            info!("Using Quorum::All for file {} ({} peers available)", metadata.merkle_root, connected_peers_count);
                            kad::Quorum::All
                        } else {
                            info!("Using Quorum::One for file {} (only {} peers available)", metadata.merkle_root, connected_peers_count);
                            kad::Quorum::One
                        };

                        match swarm.behaviour_mut().kademlia.put_record(record, quorum) {
                            Ok(query_id) => {
                                info!("started providing file: {}, query id: {:?} with quorum {:?}", 
                                    metadata.merkle_root, query_id, quorum);
                            }
                            Err(e) => {
                                error!("failed to start providing file {}: {}", metadata.merkle_root, e);
                                let _ = event_tx.send(DhtEvent::Error(format!("failed to start providing: {}", e))).await;
                            }
                        }

                        // Register this peer as a provider for the file
                        let provider_key = kad::RecordKey::new(&metadata.merkle_root.as_bytes());
                        match swarm.behaviour_mut().kademlia.start_providing(provider_key) {
                            Ok(query_id) => {
                                info!("registered as provider for file: {}, query id: {:?}", metadata.merkle_root, query_id);
                            }
                            Err(e) => {
                                error!("failed to register as provider for file {}: {}", metadata.merkle_root, e);
                                let _ = event_tx.send(DhtEvent::Error(format!("failed to register as provider: {}", e))).await;
                            }
                        }
                        // Task 1: Keyword Extraction
                        let keywords = extract_keywords(&metadata.file_name);
                        info!(
                            "Extracted {} keywords for file '{}': {:?}",
                            keywords.len(),
                            metadata.file_name,
                            keywords
                        );
                        // Task 2: DHT Indexing
                        // TODO: implement the "read-modify-write" logic inside this loop.
                        for keyword in keywords {
                            let index_key_str = format!("idx:{}", keyword);
                            let _index_key = kad::RecordKey::new(&index_key_str);

                            // TODO: Implement the read-modify-write logic to update keyword indexes.
                            // 1. Call swarm.behaviour_mut().kademlia.get_record(index_key.clone())
                            // 2. In the KademliaEvent handler for GetRecordOk, deserialize the value (a list of hashes).
                            // 3. Add the new metadata.merkle_root to the list.
                            // 4. Serialize the updated list.
                            // 5. Create a new Record and call swarm.behaviour_mut().kademlia.put_record(...).

                            info!("TODO - Register keyword '{}' with file hash '{}'", keyword, metadata.merkle_root);
                        }
                        // notify frontend
                        let _ = event_tx.send(DhtEvent::PublishedFile(metadata.clone())).await;
                        // store in file_uploaded_cache
                        let _ = response_tx.send(metadata.clone());
                    }
                    Some(DhtCommand::StoreBlocks { blocks, root_cid, mut metadata }) => {
                        // 1. Store all encrypted data blocks in bitswap
                        for (cid, data) in blocks {
                            if let Err(e) = swarm.behaviour_mut().bitswap.insert_block::<MAX_MULTIHASH_LENGHT>(cid.clone(), data) {
                                error!("Failed to store encrypted block {} in bitswap: {}", cid, e);
                                let _ = event_tx.send(DhtEvent::Error(format!("Failed to store block {}: {}", cid, e))).await;
                                continue 'outer; // Abort this publish operation
                            }
                        }

                        // 2. Update metadata with the root CID
                        metadata.cids = Some(vec![root_cid]);

                        let now = unix_timestamp();
                        let peer_id_str = peer_id.to_string();
                        let existing_heartbeats = {
                            let cache = seeder_heartbeats_cache.lock().await;
                            cache
                                .get(&metadata.merkle_root)
                                .map(|entry| entry.heartbeats.clone())
                                .unwrap_or_default()
                        };
                        let mut heartbeat_entries = existing_heartbeats;
                        upsert_heartbeat(&mut heartbeat_entries, &peer_id_str, now);
                        let active_heartbeats = prune_heartbeats(heartbeat_entries, now);
                        metadata.seeders = heartbeats_to_peer_list(&active_heartbeats);

                        // 3. Create and publish the DHT record pointing to the file
                        let dht_metadata = serde_json::json!({
                            "merkle_root": metadata.merkle_root,
                            "file_name": metadata.file_name,
                            "file_size": metadata.file_size,
                            "created_at": metadata.created_at,
                            "mime_type": metadata.mime_type,
                            "is_encrypted": metadata.is_encrypted,
                            "encryption_method": metadata.encryption_method,
                            "cids": metadata.cids,
                            "encrypted_key_bundle": metadata.encrypted_key_bundle,
                            "version": metadata.version,
                            "parent_hash": metadata.parent_hash,
                            "seeders": metadata.seeders,
                            "seederHeartbeats": active_heartbeats,
                        });

                        {
                            let mut cache = seeder_heartbeats_cache.lock().await;
                            cache.insert(
                                metadata.merkle_root.clone(),
                                FileHeartbeatCacheEntry {
                                    heartbeats: active_heartbeats.clone(),
                                    metadata: dht_metadata.clone(),
                                },
                            );
                        }

                        let record_key = kad::RecordKey::new(&metadata.merkle_root.as_bytes());
                        {
                            let mut pending = pending_heartbeat_updates.lock().await;
                            pending.insert(metadata.merkle_root.clone());
                        }
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .get_record(record_key.clone());

                        let record_value = serde_json::to_vec(&dht_metadata).map_err(|e| e.to_string()).unwrap();
                        let record = Record {
                            key: record_key.clone(),
                            value: record_value,
                            publisher: Some(peer_id),
                            expires: None,
                        };

                        if let Err(e) = swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One) {
                            error!("Failed to put record for encrypted file {}: {}", metadata.merkle_root, e);
                        }

                        // 4. Announce self as provider
                        let provider_key = kad::RecordKey::new(&metadata.merkle_root.as_bytes());
                        if let Err(e) = swarm.behaviour_mut().kademlia.start_providing(provider_key) {
                            error!("Failed to start providing encrypted file {}: {}", metadata.merkle_root, e);
                        }

                        info!("Successfully published and started providing encrypted file: {}", metadata.merkle_root);
                        let _ = event_tx.send(DhtEvent::PublishedFile(metadata)).await;
                    }
                    Some(DhtCommand::DownloadFile(mut file_metadata, download_path)) =>{
                        let root_cid_result = file_metadata.cids.as_ref()
                            .and_then(|cids| cids.first())
                            .ok_or_else(|| {
                                let msg = format!("No root CID found for file with Merkle root: {}", file_metadata.merkle_root);
                                error!("{}", msg);
                                msg
                            });

                        let root_cid = match root_cid_result {
                            Ok(cid) => cid.clone(),
                            Err(e) => { let _ = event_tx.send(DhtEvent::Error(e)).await; continue; }
                        };

                        info!("🔽 Starting Bitswap download for file: {} (root CID: {})", file_metadata.file_name, root_cid);
                        info!("📊 File has {} known seeders: {:?}", file_metadata.seeders.len(), file_metadata.seeders);

                        // Check if we're connected to any seeders
                        let connected = connected_peers.lock().await;
                        let connected_seeders: Vec<_> = file_metadata.seeders.iter()
                            .filter(|seeder| {
                                if let Ok(peer_id) = seeder.parse::<PeerId>() {
                                    connected.contains(&peer_id)
                                } else {
                                    false
                                }
                            })
                            .collect();

                        if connected_seeders.is_empty() {
                            warn!("⚠️  Not connected to any seeders for file {}!", file_metadata.file_name);
                            warn!("   Available seeders: {:?}", file_metadata.seeders);
                            warn!("   Connected peers: {:?}", connected.iter().map(|p| p.to_string()).collect::<Vec<_>>());
                            let _ = event_tx.send(DhtEvent::Error(
                                format!("Not connected to any seeders for file {}. Please ensure at least one seeder is online and connected.", file_metadata.file_name)
                            )).await;
                            continue;
                        }

                        info!("✅ Connected to {}/{} seeders", connected_seeders.len(), file_metadata.seeders.len());

                        // Request the root block which contains the CIDs
                        let root_query_id = swarm.behaviour_mut().bitswap.get(&root_cid);
                        info!("📤 Sent Bitswap GET request for root block (query_id: {:?})", root_query_id);

                        file_metadata.download_path = Some(download_path);
                        // Store the root query ID to handle when we get the root block
                        info!("INSERTING INTO ROOT QUERY MAPPING");
                        root_query_mapping.lock().await.insert(root_query_id, file_metadata);
                    }

                    Some(DhtCommand::StopPublish(file_hash)) => {
                        let key = kad::RecordKey::new(&file_hash);
                        let removed = swarm.behaviour_mut().kademlia.remove_record(&key);
                        debug!(
                            "StopPublish: removed record for {} (removed={:?})",
                            file_hash, removed
                        );

                        // Ask Kademlia to stop providing this file (so provider records are removed)
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .stop_providing(&key);

                        // Also proactively publish an updated DHT record with no seeders so remote nodes
                        // that fetch the JSON record see that there are no seeders immediately.
                        // Build minimal "empty" metadata
                        let empty_meta = serde_json::json!({
                            "merkle_root": file_hash,
                            "file_name": serde_json::Value::Null,
                            "file_size": 0u64,
                            "created_at": unix_timestamp(),
                            "seeders": Vec::<String>::new(),
                            "seederHeartbeats": Vec::<SeederHeartbeat>::new()
                        });
                        if let Ok(bytes) = serde_json::to_vec(&empty_meta) {
                            let record = Record {
                                key: kad::RecordKey::new(&file_hash.as_bytes()),
                                value: bytes,
                                publisher: Some(peer_id.clone()),
                                expires: None,
                            };
                            if let Err(e) =
                                swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One)
                            {
                                warn!("Failed to publish empty record for {}: {}", file_hash, e);
                            } else {
                                debug!("Published empty seeder record for {}", file_hash);
                            }
                        }

                        seeder_heartbeats_cache.lock().await.remove(&file_hash);
                        pending_heartbeat_updates
                            .lock()
                            .await
                            .remove(&file_hash);
                        debug!("Cleared cached heartbeat state for {}", file_hash);
                    }
                    Some(DhtCommand::HeartbeatFile { file_hash }) => {
                        let now = unix_timestamp();
                        let peer_id_str = peer_id.to_string();
                        let mut serialized_record: Option<Vec<u8>> = None;

                        {
                            let mut cache = seeder_heartbeats_cache.lock().await;
                            if let Some(entry) = cache.get_mut(&file_hash) {
                                upsert_heartbeat(&mut entry.heartbeats, &peer_id_str, now);
                                entry.heartbeats = prune_heartbeats(entry.heartbeats.clone(), now);

                                let seeder_strings = heartbeats_to_peer_list(&entry.heartbeats);
                                entry.metadata["seeders"] = serde_json::Value::Array(
                                    seeder_strings
                                        .iter()
                                        .cloned()
                                        .map(serde_json::Value::String)
                                        .collect(),
                                );
                                entry.metadata["seederHeartbeats"] =
                                    serde_json::to_value(&entry.heartbeats)
                                        .unwrap_or_else(|_| serde_json::Value::Array(vec![]));

                                match serde_json::to_vec(&entry.metadata) {
                                    Ok(bytes) => serialized_record = Some(bytes),
                                    Err(e) => {
                                        error!(
                                            "Failed to serialize heartbeat metadata for {}: {}",
                                            file_hash, e
                                        );
                                    }
                                }
                            }
                        }

                        if let Some(record_bytes) = serialized_record {
                            pending_heartbeat_updates
                                .lock()
                                .await
                                .remove(&file_hash);

                            let key = kad::RecordKey::new(&file_hash.as_bytes());
                            let record = Record {
                                key,
                                value: record_bytes,
                                publisher: Some(peer_id),
                                expires: None,
                            };

                            // Determine appropriate quorum based on number of connected peers
                        let connected_peers_count = connected_peers.lock().await.len();
                        let min_replication_peers = 3; // Require at least 3 peers for stronger replication
                        
                        let quorum = if connected_peers_count >= min_replication_peers {
                            debug!("Using Quorum::All for heartbeat update of {} ({} peers available)", 
                                file_hash, connected_peers_count);
                            kad::Quorum::All
                        } else {
                            debug!("Using Quorum::One for heartbeat update of {} (only {} peers available)", 
                                file_hash, connected_peers_count);
                            kad::Quorum::One
                        };

                        match swarm
                            .behaviour_mut()
                            .kademlia
                            .put_record(record, quorum)
                        {
                            Ok(query_id) => {
                                debug!(
                                    "Refreshed heartbeat for {} with quorum {:?} (query id: {:?})",
                                    file_hash, quorum, query_id
                                );
                            }
                            Err(e) => {
                                error!(
                                    "Failed to update heartbeat record for {}: {}",
                                    file_hash, e
                                );
                            }
                            }

                            let provider_key = kad::RecordKey::new(&file_hash.as_bytes());
                            if let Err(e) =
                                swarm.behaviour_mut().kademlia.start_providing(provider_key)
                            {
                                debug!(
                                    "Failed to refresh provider record for {}: {}",
                                    file_hash, e
                                );
                            }
                        } else {
                            pending_heartbeat_updates
                                .lock()
                                .await
                                .insert(file_hash.clone());

                            debug!(
                                "No cached metadata for {}; fetching record before heartbeat",
                                file_hash
                            );
                            let key = kad::RecordKey::new(&file_hash.as_bytes());
                            let _ = swarm.behaviour_mut().kademlia.get_record(key);
                        }
                    }
                    Some(DhtCommand::SearchFile(file_hash)) => {
                        let key = kad::RecordKey::new(&file_hash.as_bytes());
                        let query_id = swarm.behaviour_mut().kademlia.get_record(key);
                        info!("Searching for file: {} (query: {:?})", file_hash, query_id);
                    }
                    Some(DhtCommand::SetPrivacyProxies { addresses }) => {
                        info!("Updating privacy proxy targets ({} addresses)", addresses.len());

                        let mut parsed_entries: Vec<(String, Multiaddr, Option<PeerId>)> = Vec::new();

                        for address in addresses {
                            match address.parse::<Multiaddr>() {
                                Ok(multiaddr) => {
                                    let maybe_peer_id = multiaddr.iter().find_map(|protocol| {
                                        if let libp2p::multiaddr::Protocol::P2p(peer_id) = protocol {
                                            Some(peer_id.clone())
                                        } else {
                                            None
                                        }
                                    });

                                    parsed_entries.push((address, multiaddr, maybe_peer_id));
                                }
                                Err(error) => {
                                    warn!("Invalid privacy proxy address '{}': {}", address, error);
                                    let _ = event_tx
                                        .send(DhtEvent::Error(format!(
                                            "Invalid proxy address '{}': {}",
                                            address, error
                                        )))
                                        .await;
                                }
                            }
                        }

                        let manual_peers: Vec<PeerId> = parsed_entries
                            .iter()
                            .filter_map(|(_, _, maybe_peer)| maybe_peer.clone())
                            .collect();

                        {
                            let mut mgr = proxy_mgr.lock().await;
                            mgr.set_manual_trusted(&manual_peers);
                        }

                        for (addr_str, multiaddr, maybe_peer_id) in parsed_entries {
                            match swarm.dial(multiaddr.clone()) {
                                Ok(_) => {
                                    if let Some(peer_id) = &maybe_peer_id {
                                        info!(
                                            "Dialing trusted privacy proxy {} via {}",
                                            peer_id, multiaddr
                                        );
                                    } else {
                                        info!("Dialing privacy proxy at {}", multiaddr);
                                    }
                                }
                                Err(error) => {
                                    warn!("Failed to dial privacy proxy {}: {}", addr_str, error);
                                    let _ = event_tx
                                        .send(DhtEvent::Error(format!(
                                            "Failed to dial proxy {}: {}",
                                            addr_str, error
                                        )))
                                        .await;
                                }
                            }
                        }
                    }
                    Some(DhtCommand::ConnectPeer(addr)) => {
                        info!("Attempting to connect to: {}", addr);
                        if let Ok(multiaddr) = addr.parse::<Multiaddr>() {
                            let maybe_peer_id = multiaddr.iter().find_map(|p| {
                                if let libp2p::multiaddr::Protocol::P2p(peer_id) = p {
                                    Some(peer_id.clone())
                                } else {
                                    None
                                }
                            });

                            if let Some(peer_id) = maybe_peer_id.clone() {
                                // Check if the address contains a private IP
                                let has_private_ip = multiaddr.iter().any(|p| {
                                    if let Protocol::Ip4(ipv4) = p {
                                        is_private_or_loopback_v4(ipv4)
                                    } else {
                                        false
                                    }
                                });

                                // If private IP detected, try relay connection via any relay-capable peer
                                if has_private_ip {
                                    info!("🔍 Detected private IP address in {}", multiaddr);

                                    // Get list of relay-capable peers we've discovered
                                    let relay_peers = relay_capable_peers.lock().await;

                                    if !relay_peers.is_empty() {
                                        info!("🔄 Found {} relay-capable peers, attempting relay connection", relay_peers.len());

                                        // Try to use the first available relay-capable peer
                                        // Clone the data we need before dropping the lock
                                        let relay_option = relay_peers.iter().next().map(|(id, addrs)| {
                                            (*id, addrs.first().cloned())
                                        });

                                        drop(relay_peers); // Release lock before dialing

                                        if let Some((relay_peer_id, Some(relay_addr))) = relay_option {
                                            info!("📡 Attempting to connect to {} via relay peer {}", peer_id, relay_peer_id);

                                            // Build proper circuit relay address
                                            // Format: /ip4/{relay_ip}/tcp/{relay_port}/p2p/{relay_peer_id}/p2p-circuit/p2p/{target_peer_id}
                                            let mut circuit_addr = relay_addr.clone();

                                            // Ensure the relay address includes the relay peer ID
                                            if !circuit_addr.iter().any(|p| matches!(p, Protocol::P2p(_))) {
                                                circuit_addr.push(Protocol::P2p(relay_peer_id));
                                            }

                                            circuit_addr.push(Protocol::P2pCircuit);
                                            circuit_addr.push(Protocol::P2p(peer_id));

                                            info!("  Using relay circuit address: {}", circuit_addr);

                                            match swarm.dial(circuit_addr.clone()) {
                                                Ok(_) => {
                                                    info!("✓ Relay connection requested successfully");
                                                    let _ = event_tx.send(DhtEvent::Info(format!(
                                                        "Connecting to private network peer {} via relay {}", peer_id, relay_peer_id
                                                    ))).await;
                                                    continue; // Skip direct dial, use relay only
                                                }
                                                Err(e) => {
                                                    warn!("Relay connection failed: {}, falling back to direct dial", e);
                                                    // Fall through to direct dial attempt
                                                }
                                            }
                                        }
                                    } else {
                                        drop(relay_peers); // Release lock
                                        info!("⚠️ No relay-capable peers discovered yet. Trying direct connection.");
                                        info!("   Tip: Enable 'Relay Server' in Settings to help others connect!");
                                    }
                                }
                                {
                                    let mut mgr = proxy_mgr.lock().await;
                                    mgr.set_target(peer_id.clone());
                                    let use_proxy_routing = mgr.is_privacy_routing_enabled();

                                    if use_proxy_routing {
                                        if let Some(proxy_peer_id) = mgr.select_proxy_for_routing(&peer_id) {
                                            drop(mgr);

                                            info!(
                                                "Using privacy routing through proxy {} to reach {}",
                                                proxy_peer_id, peer_id
                                            );

                                            let circuit_addr =
                                                create_circuit_relay_address_robust(&proxy_peer_id, &peer_id);
                                            info!(
                                                "Attempting circuit relay connection via {} to {}",
                                                proxy_peer_id, peer_id
                                            );

                                            match swarm.dial(circuit_addr.clone()) {
                                                Ok(_) => {
                                                    info!(
                                                        "Requested circuit relay connection to {} via proxy {}",
                                                        peer_id, proxy_peer_id
                                                    );
                                                    continue;
                                                }
                                                Err(e) => {
                                                    error!(
                                                        "Failed to dial via circuit relay {}: {}",
                                                        circuit_addr, e
                                                    );
                                                    let _ = event_tx
                                                        .send(DhtEvent::Error(format!(
                                                            "Circuit relay failed: {}",
                                                            e
                                                        )))
                                                        .await;
                                                    if {
                                                        let mgr = proxy_mgr.lock().await;
                                                        mgr.privacy_mode() == PrivacyMode::Strict
                                                    } {
                                                        {
                                                            let mut mgr = proxy_mgr.lock().await;
                                                            mgr.clear_target(&peer_id);
                                                        }
                                                        continue;
                                                    }
                                                }
                                            }
                                        } else {
                                            drop(mgr);
                                            warn!(
                                                "No suitable proxy available for privacy routing to {}",
                                                peer_id
                                            );
                                            let _ = event_tx
                                                .send(DhtEvent::Error(format!(
                                                    "No trusted proxy available to reach {}",
                                                    peer_id
                                                )))
                                                .await;
                                            if {
                                                let mgr = proxy_mgr.lock().await;
                                                mgr.privacy_mode() == PrivacyMode::Strict
                                            } {
                                                {
                                                    let mut mgr = proxy_mgr.lock().await;
                                                    mgr.clear_target(&peer_id);
                                                }
                                                continue;
                                            }
                                        }
                                    }
                                }

                                let should_request = {
                                    let mut mgr = proxy_mgr.lock().await;
                                    let should_request = !mgr.has_relay_request(&peer_id);
                                    if should_request {
                                        mgr.mark_relay_pending(peer_id.clone());
                                    }
                                    should_request
                                };

                                if should_request {
                                    if let Some(relay_addr) = build_relay_listen_addr(&multiaddr) {
                                        match swarm.listen_on(relay_addr.clone()) {
                                            Ok(_) => {
                                                info!("Requested relay reservation via {}", relay_addr);
                                                let _ = event_tx
                                                    .send(DhtEvent::ProxyStatus {
                                                        id: peer_id.to_string(),
                                                        address: relay_addr.to_string(),
                                                        status: "relay_pending".into(),
                                                        latency_ms: None,
                                                        error: None,
                                                    })
                                                    .await;
                                            }
                                            Err(err) => {
                                                warn!(
                                                    "Failed to request relay reservation via {}: {}",
                                                    relay_addr, err
                                                );
                                                let mut mgr = proxy_mgr.lock().await;
                                                mgr.relay_pending.remove(&peer_id);
                                                let _ = event_tx
                                                    .send(DhtEvent::ProxyStatus {
                                                        id: peer_id.to_string(),
                                                        address: relay_addr.to_string(),
                                                        status: "relay_error".into(),
                                                        latency_ms: None,
                                                        error: Some(err.to_string()),
                                                    })
                                                    .await;
                                            }
                                        }
                                    } else {
                                        warn!("Cannot derive relay listen address from {}", multiaddr);
                                    }
                                }

                                match swarm.dial(multiaddr.clone()) {
                                    Ok(_) => {
                                        info!("Requested direct connection to: {}", addr);
                                        info!("  Multiaddr: {}", multiaddr);
                                        info!("  Waiting for ConnectionEstablished event...");
                                    }
                                    Err(e) => {
                                        error!("Failed to dial {}: {}", addr, e);
                                        let _ = event_tx
                                            .send(DhtEvent::Error(format!("Failed to connect: {}", e)))
                                            .await;
                                    }
                                }
                            } else {
                                error!("No peer ID found in multiaddr: {}", addr);
                                let _ = event_tx
                                    .send(DhtEvent::Error(format!("Invalid address format: {}", addr)))
                                    .await;
                            }
                        } else {
                            error!("Invalid multiaddr format: {}", addr);
                            let _ = event_tx
                                .send(DhtEvent::Error(format!("Invalid address: {}", addr)))
                                .await;
                        }
                    }
                    Some(DhtCommand::ConnectToPeerById(peer_id)) => {
                        info!("Attempting to connect to peer by ID: {}", peer_id);

                        // First check if we're already connected to this peer
                        let connected_peers = connected_peers.lock().await;
                        if connected_peers.contains(&peer_id) {
                            info!("Already connected to peer {}", peer_id);
                            // let _ = event_tx.send(DhtEvent::PeerConnected(peer_id.to_string())).await;
                            let _ = event_tx
                                .send(DhtEvent::PeerConnected {
                                    peer_id: peer_id.to_string(),
                                    address: None,
                                })
                                .await;
                            return;
                        }
                        drop(connected_peers);

                        // Query the DHT for known addresses of this peer
                        info!("Querying DHT for addresses of peer {}", peer_id);
                        let _query_id = swarm.behaviour_mut().kademlia.get_closest_peers(peer_id);

                        // Connection attempts will be handled when GetClosestPeers results are received
                        let _ = event_tx.send(DhtEvent::Info(format!("Searching for peer {} addresses...", peer_id))).await;
                    }
                    Some(DhtCommand::DisconnectPeer(peer_id)) => {
                        let _ = swarm.disconnect_peer_id(peer_id.clone());
                        proxy_mgr.lock().await.remove_all(&peer_id);
                    }


                    Some(DhtCommand::GetPeerCount(tx)) => {
                        let count = connected_peers.lock().await.len();
                        let _ = tx.send(count);
                    }
                    Some(DhtCommand::Echo { peer, payload, tx }) => {
                        let id = swarm.behaviour_mut().proxy_rr.send_request(&peer, EchoRequest(payload));
                        pending_echo.lock().await.insert(id, PendingEcho { peer, tx });
                    }
                    Some(DhtCommand::GetProviders { file_hash, sender }) => {
                        // Query provider records for this file hash
                        let key = kad::RecordKey::new(&file_hash.as_bytes());
                        let query_id = swarm.behaviour_mut().kademlia.get_providers(key);
                        info!("Querying providers for file: {} (query_id: {:?})", file_hash, query_id);

                        // Store the query_id -> (file_hash, start_time) mapping for error handling and timeout detection
                        get_providers_queries.lock().await.insert(query_id, (file_hash.clone(), std::time::Instant::now()));

                        // Store the query for async handling
                        let pending_query = PendingProviderQuery {
                            id: 0, // Not used for matching
                            sender,
                        };
                        pending_provider_queries.lock().await.insert(file_hash, pending_query);
                    }
                    Some(DhtCommand::SendWebRTCOffer { peer, offer_request, sender }) => {
                        let id = swarm.behaviour_mut().webrtc_signaling_rr.send_request(&peer, offer_request);
                        pending_webrtc_offers.lock().await.insert(id, sender);
                    }
                    Some(DhtCommand::SendMessageToPeer { target_peer_id, message }) => {
                        // TODO: Implement a proper messaging protocol
                        // For now, we'll use the proxy protocol to send messages
                        // In a real implementation, this could use a dedicated messaging protocol
                        match serde_json::to_vec(&message) {
                            Ok(message_data) => {
                                // Send the message directly using the proxy protocol
                                let request_id = swarm.behaviour_mut().proxy_rr.send_request(&target_peer_id, EchoRequest(message_data));
                                info!("Sent message to peer {} with request ID {:?}", target_peer_id, request_id);
                            }
                            Err(e) => {
                                error!("Failed to serialize message: {}", e);
                            }
                        }
                    }
                    Some(DhtCommand::StoreBlock { cid, data }) => {
                        match swarm.behaviour_mut().bitswap.insert_block::<MAX_MULTIHASH_LENGHT>(cid, data) {
                            Ok(_) => {
                                debug!("Successfully stored block in Bitswap");
                            }
                            Err(e) => {
                                error!("Failed to store block in Bitswap: {}", e);
                            }
                        }
                    }
                    Some(DhtCommand::RequestFileAccess { .. }) => {
                        todo!();
                    }
                    None => {
                        info!("DHT command channel closed; shutting down node task");
                        break 'outer;
                    }
                }
            }

            event = swarm.next() => if let Some(event) = event {
                match event {
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Kademlia(kad_event)) => {
                        handle_kademlia_event(
                            kad_event,
                            &mut swarm,
                            &peer_id,
                            &connected_peers,
                            &event_tx,
                            &pending_searches,
                            &pending_provider_queries,
                            &get_providers_queries,
                            &seeder_heartbeats_cache,
                            &pending_heartbeat_updates,
                            &pending_keyword_indexes,
                        )
                        .await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Identify(identify_event)) => {
                        handle_identify_event(
                            identify_event,
                            &mut swarm,
                            &event_tx,
                            metrics.clone(),
                            enable_autorelay,
                            &relay_candidates,
                            &proxy_mgr,
                            relay_capable_peers.clone(),
                        )
                        .await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Mdns(mdns_event)) => {
                        if !is_bootstrap{
                            handle_mdns_event(mdns_event, &mut swarm, &event_tx).await;
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::RelayClient(relay_event)) => {
                        match relay_event {
                            RelayClientEvent::ReservationReqAccepted { relay_peer_id, .. } => {
                                info!("✅ Relay reservation accepted from {}", relay_peer_id);
                                let mut mgr = proxy_mgr.lock().await;
                                let newly_ready = mgr.mark_relay_ready(relay_peer_id);
                                drop(mgr);

                                // Update AutoRelay metrics
                                {
                                    let mut m = metrics.lock().await;
                                    m.active_relay_peer_id = Some(relay_peer_id.to_string());
                                    m.relay_reservation_status = Some("accepted".to_string());
                                    m.last_reservation_success = Some(SystemTime::now());
                                    m.reservation_renewals += 1;
                                }

                                if newly_ready {
                                    let _ = event_tx
                                        .send(DhtEvent::ProxyStatus {
                                            id: relay_peer_id.to_string(),
                                            address: String::new(),
                                            status: "relay_ready".into(),
                                            latency_ms: None,
                                            error: None,
                                        })
                                        .await;
                                    let _ = event_tx
                                        .send(DhtEvent::Info(format!(
                                            "Connected to relay: {}",
                                            relay_peer_id
                                        )))
                                        .await;
                                }
                            }
                            RelayClientEvent::OutboundCircuitEstablished { relay_peer_id, .. } => {
                                info!("🔗 Outbound relay circuit established via {}", relay_peer_id);
                                proxy_mgr.lock().await.set_online(relay_peer_id);
                                let _ = event_tx
                                    .send(DhtEvent::ProxyStatus {
                                        id: relay_peer_id.to_string(),
                                        address: String::new(),
                                        status: "relay_circuit".into(),
                                        latency_ms: None,
                                        error: None,
                                    })
                                    .await;
                            }
                            RelayClientEvent::InboundCircuitEstablished { src_peer_id, .. } => {
                                info!("📥 Inbound relay circuit established from {}", src_peer_id);
                                let _ = event_tx
                                    .send(DhtEvent::ProxyStatus {
                                        id: src_peer_id.to_string(),
                                        address: String::new(),
                                        status: "relay_inbound".into(),
                                        latency_ms: None,
                                        error: None,
                                    })
                                    .await;
                            }
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::RelayServer(relay_server_event)) => {
                        use relay::Event as RelayEvent;
                        match relay_server_event {
                            RelayEvent::ReservationReqAccepted { src_peer_id, .. } => {
                                info!("🔁 Relay server: Accepted reservation from {}", src_peer_id);
                                let _ = event_tx
                                    .send(DhtEvent::Info(format!(
                                        "Acting as relay for peer {}",
                                        src_peer_id
                                    )))
                                    .await;

                                // Emit reputation event
                                let _ = event_tx
                                    .send(DhtEvent::ReputationEvent {
                                        peer_id: src_peer_id.to_string(),
                                        event_type: "RelayReservationAccepted".to_string(),
                                        impact: 5.0,
                                        data: serde_json::json!({
                                            "timestamp": SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                        }),
                                    })
                                    .await;
                            }
                            RelayEvent::ReservationReqDenied { src_peer_id, .. } => {
                                debug!("🔁 Relay server: Denied reservation from {}", src_peer_id);

                                // Emit reputation event
                                let _ = event_tx
                                    .send(DhtEvent::ReputationEvent {
                                        peer_id: src_peer_id.to_string(),
                                        event_type: "RelayRefused".to_string(),
                                        impact: -2.0,
                                        data: serde_json::json!({
                                            "reason": "reservation_denied",
                                            "timestamp": SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                        }),
                                    })
                                    .await;
                            }
                            RelayEvent::ReservationTimedOut { src_peer_id } => {
                                debug!("🔁 Relay server: Reservation timed out for {}", src_peer_id);

                                // Emit reputation event
                                let _ = event_tx
                                    .send(DhtEvent::ReputationEvent {
                                        peer_id: src_peer_id.to_string(),
                                        event_type: "RelayTimeout".to_string(),
                                        impact: -10.0,
                                        data: serde_json::json!({
                                            "reason": "reservation_timeout",
                                            "timestamp": SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                        }),
                                    })
                                    .await;
                            }
                            RelayEvent::CircuitReqDenied { src_peer_id, dst_peer_id, .. } => {
                                debug!("🔁 Relay server: Denied circuit from {} to {}", src_peer_id, dst_peer_id);

                                // Emit reputation event
                                let _ = event_tx
                                    .send(DhtEvent::ReputationEvent {
                                        peer_id: src_peer_id.to_string(),
                                        event_type: "RelayRefused".to_string(),
                                        impact: -2.0,
                                        data: serde_json::json!({
                                            "reason": "circuit_denied",
                                            "dst_peer_id": dst_peer_id.to_string(),
                                            "timestamp": SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                        }),
                                    })
                                    .await;
                            }
                            RelayEvent::CircuitReqAccepted { src_peer_id, dst_peer_id, .. } => {
                                info!("🔁 Relay server: Established circuit from {} to {}", src_peer_id, dst_peer_id);
                                let _ = event_tx
                                    .send(DhtEvent::Info(format!(
                                        "Relaying traffic from {} to {}",
                                        src_peer_id, dst_peer_id
                                    )))
                                    .await;

                                // Emit reputation event
                                let _ = event_tx
                                    .send(DhtEvent::ReputationEvent {
                                        peer_id: src_peer_id.to_string(),
                                        event_type: "RelayCircuitEstablished".to_string(),
                                        impact: 10.0,
                                        data: serde_json::json!({
                                            "dst_peer_id": dst_peer_id.to_string(),
                                            "timestamp": SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                        }),
                                    })
                                    .await;
                            }
                            RelayEvent::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                                debug!("🔁 Relay server: Circuit closed between {} and {}", src_peer_id, dst_peer_id);

                                // Emit reputation event
                                let _ = event_tx
                                    .send(DhtEvent::ReputationEvent {
                                        peer_id: src_peer_id.to_string(),
                                        event_type: "RelayCircuitSuccessful".to_string(),
                                        impact: 15.0,
                                        data: serde_json::json!({
                                            "dst_peer_id": dst_peer_id.to_string(),
                                            "timestamp": SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs(),
                                        }),
                                    })
                                    .await;
                            }
                            // Handle deprecated relay events (libp2p handles logging internally)
                            _ => {}
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Bitswap(bitswap)) => match bitswap {
                        beetswap::Event::GetQueryResponse { query_id, data } => {
                            info!("📥 Received Bitswap block (query_id: {:?}, size: {} bytes)", query_id, data.len());

                            // Check if this is a root block query first
                            if let Some(metadata) = root_query_mapping.lock().await.remove(&query_id) {
                                info!("✅ This is a ROOT BLOCK for file: {}", metadata.merkle_root);

                                // This is the root block containing CIDs - parse and request all data blocks
                                match serde_json::from_slice::<Vec<Cid>>(&data) {
                                    Ok(cids) => {

                                        // Create queries map for this file's data blocks
                                        let mut file_queries = HashMap::new();

                                        for (i, cid) in cids.iter().enumerate() {
                                            let block_query_id = swarm.behaviour_mut().bitswap.get(cid);
                                            file_queries.insert(block_query_id, i as u32);
                                        }

                                        // Calculate chunk size based on file size and number of chunks
                                        let total_chunks = cids.len() as u64;
                                        // assume 256kb
                                        let chunk_size = 256 * 1024;

                                        // Pre-calculate chunk offsets
                                        let chunk_offsets: Vec<u64> = (0..total_chunks)
                                            .map(|i| i * chunk_size)
                                            .collect();

                                        info!("Chunk offsets: {:?}", chunk_offsets);

                                        info!("About to create ActiveDownload for file: {}", metadata.merkle_root);
                                        let download_path = PathBuf::from_str(metadata.download_path.as_ref().expect("Error: download_path not defined"));
                                        let download_path = match download_path {
                                            Ok(path) => get_available_download_path(path).await,
                                            Err(e) => {
                                                error!("Invalid download path for file {}: {}", metadata.merkle_root, e);
                                                return;
                                            }
                                        };

                                    // Create active download with memory-mapped file
                            match ActiveDownload::new(
                                metadata.clone(),
                                file_queries,
                                &download_path,
                                metadata.file_size,
                                chunk_offsets,
                            ) {
                                Ok(active_download) => {
                                    let active_download = Arc::new(tokio::sync::Mutex::new(active_download));

                                    info!("Successfully created ActiveDownload");

                                    active_downloads.lock().await.insert(
                                        metadata.merkle_root.clone(),
                                        Arc::clone(&active_download),
                                    );

                                    info!(
                                        "Inserted into active_downloads map. Started tracking download for file {} with {} chunks (chunk_size: {} bytes)",
                                        metadata.merkle_root, cids.len(), chunk_size
                                    );
                                }
                                Err(e) => {
                                    error!(
                                        "FAILED to create memory-mapped file for {}: {}",
                                        metadata.merkle_root, e
                                    );
                                }
                            }

                                    }
                                    Err(e) => {
                                        error!("Failed to parse root block as CIDs array for file {}: {}",
                                            metadata.merkle_root, e);
                                    }
                                }
                            } else {
                                // This is a data block query - find the corresponding file and handle it

                                let mut completed_downloads = Vec::new();

                                // Check all active downloads for this query_id
                                {
                                    let mut active_downloads_guard = active_downloads.lock().await;

                                    let mut found = false;
                                    for (file_hash, active_download_lock) in active_downloads_guard.iter_mut() {
                                        let mut active_download = active_download_lock.lock().await;
                                        if let Some(chunk_index) = active_download.queries.remove(&query_id) {
                                            found = true;

                                            // This query belongs to this file - write the chunk to disk
                                            let offset = active_download.chunk_offsets
                                                .get(chunk_index as usize)
                                                .copied()
                                                .unwrap_or_else(|| {
                                                    error!("No offset found for chunk_index: {}", chunk_index);
                                                    0
                                                });


                                            match active_download.write_chunk(chunk_index, &data, offset) {
                                                Ok(_) => {
                                                    info!("Successfully wrote chunk {}/{} for file {}",
                                                        chunk_index + 1,
                                                        active_download.total_chunks,
                                                        file_hash);

                                                    let _ = event_tx.send(DhtEvent::BitswapChunkDownloaded {
                                                        file_hash: file_hash.clone(),
                                                        chunk_index,
                                                        total_chunks: active_download.total_chunks,
                                                        chunk_size: data.len(),
                                                    }).await;
                                                }
                                                Err(e) => {
                                                    error!("Failed to write chunk {} to disk for file {}: {}",
                                                        chunk_index, file_hash, e);
                                                    break;
                                                }
                                            }

                                           // In the "all chunks downloaded" section:
                                            if active_download.is_complete() {
                                                // Flush and finalize the file (rename .tmp to final name)
                                                // No need to check for encryption at this level, handle decryption
                                                // inside DownloadedFile event or some other handler above this level.
                                                info!("Finalizing file...");
                                                match active_download.finalize() {
                                                    Ok(_) => {
                                                        info!("Successfully finalized file");
                                                    }
                                                    Err(e) => {
                                                        error!("Failed to finalize file {}: {}", file_hash, e);
                                                        break;
                                                    }
                                                }
                                                // no longer storing file data in completed metadata because file is being written directly to disk
                                                let completed_metadata = active_download.metadata.clone();
                                                completed_downloads.push(completed_metadata);
                                            }
                                            break;
                                        }
                                    }

                                    if !found {
                                        warn!("Received chunk for unknown query_id: {:?}", query_id);
                                    }
                                }

                                // Send completion events for finished downloads
                             // Send completion events for finished downloads
                                for metadata in completed_downloads {
                                    info!("Emitting DownloadedFile event for: {}", metadata.merkle_root);

                                    if let Err(e) = event_tx.send(DhtEvent::DownloadedFile(metadata.clone())).await {
                                        error!("Failed to send DownloadedFile event: {}", e);
                                    }

                                    // Just remove from active downloads - file is already finalized
                                    info!("Removing from active_downloads...");
                                    active_downloads.lock().await.remove(&metadata.merkle_root);
                                }
                            }
                        }
                        beetswap::Event::GetQueryError { query_id, error } => {
                            // Handle Bitswap query error
                            error!("❌ Bitswap query {:?} failed: {:?}", query_id, error);

                            // Clean up any active downloads that contain this failed query
                            {
                                let mut active_downloads_guard = active_downloads.lock().await;
                                let mut failed_files = Vec::new();

                                for (file_hash, active_download_lock) in active_downloads_guard.iter_mut() {
                                        let mut active_download = active_download_lock.lock().await;
                                    if active_download.queries.remove(&query_id).is_some() {
                                        warn!("Query {:?} failed for file {}, removing from active downloads", query_id, file_hash);
                                        failed_files.push(file_hash.clone());
                                    }
                                }

                                // Remove failed downloads from active downloads
                                for file_hash in failed_files {
                                    active_downloads_guard.remove(&file_hash);
                                }
                            }

                            let _ = event_tx.send(DhtEvent::BitswapError {
                                query_id: format!("{:?}", query_id),
                                error: format!("{:?}", error),
                            }).await;
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Ping(ev)) => {
                        match ev {
                            libp2p::ping::Event { peer, result: Ok(rtt), .. } => {
                                let is_connected = connected_peers.lock().await.contains(&peer);
                                let rtt_ms = rtt.as_millis() as u64;
                                debug!("Ping from peer {}: {} ms (connected: {})", peer, rtt_ms, is_connected);

                                // Update peer selection metrics with latency
                                {
                                    let mut selection = peer_selection.lock().await;
                                    selection.update_peer_latency(&peer.to_string(), rtt_ms);
                                }

                                let show = proxy_mgr.lock().await.is_proxy(&peer);

                                if show {
                                    let _ = event_tx
                                        .send(DhtEvent::PeerRtt {
                                            peer: peer.to_string(),
                                            rtt_ms,
                                        })
                                        .await;

                                        ping_failures.remove(&peer);
                                } else {
                                    // Ignore
                                }
                            }
                            libp2p::ping::Event { peer, result: Err(libp2p::ping::Failure::Timeout), .. } => {
                                let _ = event_tx
                                    .send(DhtEvent::Error(format!("Ping timeout {}", peer)))
                                    .await;
                                let count = ping_failures.entry(peer).or_insert(0);
                                *count += 1;
                                if *count >= 3 {
                                    swarm.behaviour_mut().kademlia.remove_peer(&peer);
                                    ping_failures.remove(&peer);
                                    let _ = event_tx.send(DhtEvent::Error(format!(
                                        "Peer {} removed after 3 failed pings", peer
                                    ))).await;
                                }
                            }
                            libp2p::ping::Event { peer, result: Err(e), .. } => {
                                warn!("ping error with {}: {}", peer, e);
                                let count = ping_failures.entry(peer).or_insert(0);
                                *count += 1;
                                if *count >= 3 {
                                    swarm.behaviour_mut().kademlia.remove_peer(&peer);
                                    ping_failures.remove(&peer);
                                    let _ = event_tx.send(DhtEvent::Error(format!(
                                        "Peer {} removed after 3 failed pings", peer
                                    ))).await;
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::AutonatClient(ev)) => {
                        handle_autonat_client_event(ev, &metrics, &event_tx).await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::AutonatServer(ev)) => {
                        debug!(?ev, "AutoNAT server event");
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Dcutr(ev)) => {
                        handle_dcutr_event(ev, &metrics, &event_tx).await;
                    }
                    SwarmEvent::ExternalAddrConfirmed { address, .. } => {
                        handle_external_addr_confirmed(&address, &metrics, &event_tx, &proxy_mgr)
                            .await;
                    }
                    SwarmEvent::ExternalAddrExpired { address, .. } => {
                        handle_external_addr_expired(&address, &metrics, &event_tx, &proxy_mgr)
                            .await;
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        let remote_addr = endpoint.get_remote_address().clone();

                        // Initialize peer metrics for smart selection
                        {
                            let mut selection = peer_selection.lock().await;
                            let peer_metrics = PeerMetrics::new(
                                peer_id.to_string(),
                                // endpoint.get_remote_address().to_string(),
                                remote_addr.to_string(),
                            );
                            selection.update_peer_metrics(peer_metrics);
                        }

                        // Add peer to Kademlia routing table
                        // swarm.behaviour_mut().kademlia.add_address(&peer_id, endpoint.get_remote_address().clone());
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, remote_addr.clone());

                        let peers_count = {
                            let mut peers = connected_peers.lock().await;
                            peers.insert(peer_id);
                            peers.len()
                        };
                        if let Ok(mut m) = metrics.try_lock() {
                            m.last_success = Some(SystemTime::now());
                        }
                        // info!("✅ Connected to {} via {}", peer_id, endpoint.get_remote_address());
                        info!("✅ Connected to {} via {}", peer_id, remote_addr);
                        info!("   Total connected peers: {}", peers_count);
                        let _ = event_tx
                            .send(DhtEvent::PeerConnected {
                                peer_id: peer_id.to_string(),
                                address: Some(remote_addr.to_string()),
                            })
                            .await;
                    }
                     SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        warn!("❌ DISCONNECTED from peer: {}", peer_id);
                        warn!("   Cause: {:?}", cause);

                        let peers_count = {
                            let mut peers = connected_peers.lock().await;
                            peers.remove(&peer_id);
                            peers.len()
                        };

                        // Remove proxy state
                        proxy_mgr.lock().await.remove_all(&peer_id);

                        // Immediately remove disconnected peer from seeder heartbeat cache
                        let pid_str = peer_id.to_string();
                        let mut updated_records: Vec<(String, Vec<u8>)> = Vec::new();
                        {
                            let mut cache = seeder_heartbeats_cache.lock().await;
                            let now = unix_timestamp();
                            let mut to_remove_keys: Vec<String> = Vec::new();
                            for (file_hash, entry) in cache.iter_mut() {
                                let before = entry.heartbeats.len();
                                // remove any heartbeats for this peer
                                entry.heartbeats.retain(|hb| hb.peer_id != pid_str);

                                // prune expired while we're here
                                entry.heartbeats = prune_heartbeats(entry.heartbeats.clone(), now);

                                if entry.heartbeats.len() != before {
                                    // update metadata fields
                                    let seeder_strings = heartbeats_to_peer_list(&entry.heartbeats);
                                    entry.metadata["seeders"] = serde_json::Value::Array(
                                        seeder_strings
                                            .iter()
                                            .cloned()
                                            .map(serde_json::Value::String)
                                            .collect(),
                                    );
                                    entry.metadata["seederHeartbeats"] =
                                        serde_json::to_value(&entry.heartbeats)
                                            .unwrap_or_else(|_| serde_json::Value::Array(vec![]));

                                    // If no seeders left we can drop the cache entry (and optionally stop providing)
                                    if entry.heartbeats.is_empty() {
                                        to_remove_keys.push(file_hash.clone());
                                    } else if let Ok(bytes) = serde_json::to_vec(&entry.metadata) {
                                        updated_records.push((file_hash.clone(), bytes));
                                    }
                                }
                            }
                            for k in to_remove_keys {
                                cache.remove(&k);
                            }
                        } // release cache lock

                        // Push updated records to Kademlia for each updated file
                        for (file_hash, bytes) in updated_records {
                            let key = kad::RecordKey::new(&file_hash.as_bytes());
                            let record = Record {
                                key: key.clone(),
                                value: bytes.clone(),
                                publisher: Some(peer_id.clone()),
                                expires: None,
                            };
                            if let Err(e) =
                                swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One)
                            {
                                warn!("Failed to refresh DHT record after disconnect for {}: {}", file_hash, e);
                            } else {
                                debug!("Refreshed DHT record for {} after peer {} disconnected", file_hash, peer_id);
                            }

                            // notify UI with updated metadata so frontend refreshes immediately
                            if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                if let (Some(merkle_root), Some(file_name), Some(file_size), Some(created_at)) = (
                                    json_val.get("merkle_root").and_then(|v| v.as_str()),
                                    json_val.get("file_name").and_then(|v| v.as_str()),
                                    json_val.get("file_size").and_then(|v| v.as_u64()),
                                    json_val.get("created_at").and_then(|v| v.as_u64()),
                                ) {
                                    let seeders = json_val
                                        .get("seeders")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                                        .unwrap_or_default();

                                    let metadata = FileMetadata {
                                        merkle_root: merkle_root.to_string(),
                                        file_name: file_name.to_string(),
                                        file_size,
                                        file_data: Vec::new(),
                                        seeders,
                                        created_at,
                                        mime_type: json_val.get("mime_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        is_encrypted: json_val.get("is_encrypted").and_then(|v| v.as_bool()).unwrap_or(false),
                                        encryption_method: json_val.get("encryption_method").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        key_fingerprint: json_val.get("key_fingerprint").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        version: json_val.get("version").and_then(|v| v.as_u64()).map(|u| u as u32),
                                        parent_hash: json_val.get("parent_hash").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        cids: json_val.get("cids").and_then(|v| serde_json::from_value::<Option<Vec<Cid>>>(v.clone()).ok()).unwrap_or(None),
                                        encrypted_key_bundle: json_val.get("encryptedKeyBundle").and_then(|v| serde_json::from_value::<Option<crate::encryption::EncryptedAesKeyBundle>>(v.clone()).ok()).unwrap_or(None),
                                        is_root: json_val.get("is_root").and_then(|v| v.as_bool()).unwrap_or(true),
                                        price: json_val.get("price").and_then(|v| v.as_f64()),
                                        uploader_address: json_val.get("uploader_address").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        ..Default::default()
                                    };
                                    let _ = event_tx.send(DhtEvent::FileDiscovered(metadata)).await;
                                }
                            }
                        }
                        info!("   Remaining connected peers: {}", peers_count);
                        let _ = event_tx
                            .send(DhtEvent::PeerDisconnected {
                                peer_id: peer_id.to_string(),
                            })
                            .await;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        if address.iter().any(|component| matches!(component, Protocol::P2pCircuit)) {
                            swarm.add_external_address(address.clone());
                            debug!("Advertised relay external address: {}", address);
                        }
                        if let Ok(mut m) = metrics.try_lock() {
                            m.record_listen_addr(&address);
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        if let Ok(mut m) = metrics.try_lock() {
                            m.last_error = Some(error.to_string());
                            m.last_error_at = Some(SystemTime::now());
                            if let Some(pid) = peer_id {
                                if bootstrap_peer_ids.contains(&pid) {
                                    m.bootstrap_failures = m.bootstrap_failures.saturating_add(1);
                                }
                            }
                        }

                        if let Some(pid) = peer_id {
                            error!("❌ Outgoing connection error to {}: {}", pid, error);

                            // If the error contains a multiaddr, check if it's plausibly reachable
                            if let Some(bad_ma) = extract_multiaddr_from_error_str(&error.to_string()) {
                                if !ma_plausibly_reachable(&bad_ma) {
                                    swarm.behaviour_mut().kademlia.remove_address(&pid, &bad_ma);
                                    debug!("🧹 Removed unreachable addr for {}: {}", pid, bad_ma);
                                }
                            }

                            let is_bootstrap = bootstrap_peer_ids.contains(&pid);
                            if error.to_string().contains("rsa") {
                                error!("   ℹ Hint: This node uses RSA keys. Enable 'rsa' feature if needed.");
                            } else if error.to_string().contains("Timeout") {
                                if is_bootstrap {
                                    warn!("   ℹ Hint: Bootstrap nodes may be unreachable or overloaded.");
                                } else {
                                    warn!("   ℹ Hint: Peer may be unreachable (timeout).");
                                }
                            } else if error.to_string().contains("Connection refused") {
                                if is_bootstrap {
                                    warn!("   ℹ Hint: Bootstrap nodes are not accepting connections.");
                                } else {
                                    warn!("   ℹ Hint: Peer is not accepting connections.");
                                }
                            } else if error.to_string().contains("Transport") {
                                warn!("   ℹ Hint: Transport protocol negotiation failed.");
                            }
                        } else {
                            error!("❌ Outgoing connection error to unknown peer: {}", error);
                        }
                        let _ = event_tx.send(DhtEvent::Error(format!("Connection failed: {}", error))).await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::ProxyRr(ev)) => {
                        use libp2p::request_response::{Event as RREvent, Message};
                        match ev {
                            RREvent::Message { peer, message } => match message {
                                // Echo server
                                Message::Request { request, channel, .. } => {
                                    proxy_mgr.lock().await.set_capable(peer);
                                    proxy_mgr.lock().await.set_online(peer);
                                    let _ = event_tx.send(DhtEvent::ProxyStatus {
                                        id: peer.to_string(),
                                        address: String::new(),
                                        status: "online".into(),
                                        latency_ms: None,
                                        error: None,
                                    }).await;
                                    let EchoRequest(data) = request;

                                    // Check if this is a payment notification
                                    if let Ok(json_str) = std::str::from_utf8(&data) {
                                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                                            if parsed.get("type").and_then(|v| v.as_str()) == Some("payment_notification") {
                                                // This is a payment notification, emit special event
                                                if let Some(payload) = parsed.get("payload") {
                                                    info!("💰 Received payment notification from peer {}: {:?}", peer, payload);
                                                    let _ = event_tx.send(DhtEvent::PaymentNotificationReceived {
                                                        from_peer: peer.to_string(),
                                                        payload: payload.clone(),
                                                    }).await;
                                                }
                                            }
                                        }
                                    }

                                    // 2) Showing received data to UI (for non-payment messages)
                                    let preview = std::str::from_utf8(&data).ok().map(|s| s.to_string());
                                    let _ = event_tx.send(DhtEvent::EchoReceived {
                                        from: peer.to_string(),
                                        utf8: preview,
                                        bytes: data.len(),
                                    }).await;

                                    // 3) Echo response
                                    swarm.behaviour_mut().proxy_rr
                                        .send_response(channel, EchoResponse(data))
                                        .unwrap_or_else(|e| error!("send_response failed: {e:?}"));
                                }
                                // Client response
                                Message::Response { request_id, response } => {
                                    proxy_mgr.lock().await.set_capable(peer);
                                    proxy_mgr.lock().await.set_online(peer);
                                    let _ = event_tx.send(DhtEvent::ProxyStatus {
                                        id: peer.to_string(),
                                        address: String::new(),
                                        status: "online".into(),
                                        latency_ms: None,
                                        error: None,
                                    }).await;

                                    if let Some(PendingEcho { tx, .. }) = pending_echo.lock().await.remove(&request_id) {
                                        let EchoResponse(data) = response;
                                        let _ = tx.send(Ok(data));
                                    }
                                }
                            },

                            RREvent::OutboundFailure { request_id, error, .. } => {
                                if let Some(PendingEcho { peer, tx }) = pending_echo.lock().await.remove(&request_id) {
                                    let _ = tx.send(Err(format!("outbound failure: {error:?}")));

                                    {
                                        let mut pm = proxy_mgr.lock().await;
                                        pm.set_offline(&peer);
                                    }
                                    let _ = event_tx.send(DhtEvent::ProxyStatus {
                                        id: peer.to_string(),
                                        address: String::new(),
                                        status: "offline".into(),
                                        latency_ms: None,
                                        error: Some(error.to_string()),
                                    }).await;
                                } else {
                                    warn!("OutboundFailure for unknown request_id {:?}: {:?}", request_id, error);
                                }
                            }

                            RREvent::InboundFailure { peer, error, .. } => {
                                {
                                    let mut pm = proxy_mgr.lock().await;
                                    pm.set_offline(&peer);
                                }
                                let _ = event_tx.send(DhtEvent::ProxyStatus {
                                    id: peer.to_string(),
                                    address: String::new(),
                                    status: "offline".into(),
                                    latency_ms: None,
                                    error: Some(error.to_string()),
                                }).await;
                            }

                            RREvent::ResponseSent { .. } => {}
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::WebrtcSignalingRr(ev)) => {
                        use libp2p::request_response::{Event as RREvent, Message};
                        match ev {
                            RREvent::Message { peer, message } => match message {
                                // WebRTC offer request
                                Message::Request { request, channel, .. } => {
                                    let WebRTCOfferRequest { offer_sdp, file_hash, requester_peer_id: _requester_peer_id } = request;
                                    info!("Received WebRTC offer from {} for file {}", peer, file_hash);

                                    // Get WebRTC service to handle the offer
                                    if let Some(webrtc_service) = get_webrtc_service().await {
                                        // Create WebRTC answer using the WebRTC service
                                        match webrtc_service.establish_connection_with_offer(peer.to_string(), offer_sdp).await {
                                            Ok(answer_sdp) => {
                                                info!("Created WebRTC answer for peer {}", peer);
                                                swarm.behaviour_mut().webrtc_signaling_rr
                                                    .send_response(channel, WebRTCAnswerResponse { answer_sdp })
                                                    .unwrap_or_else(|e| error!("send_response failed: {e:?}"));
                                            }
                                            Err(e) => {
                                                error!("Failed to create WebRTC answer for peer {}: {}", peer, e);
                                                let error_answer = "error:failed-to-create-answer".to_string();
                                                swarm.behaviour_mut().webrtc_signaling_rr
                                                    .send_response(channel, WebRTCAnswerResponse { answer_sdp: error_answer })
                                                    .unwrap_or_else(|e| error!("send_response failed: {e:?}"));
                                            }
                                        }
                                    } else {
                                        error!("WebRTC service not available for handling offer from peer {}", peer);
                                        let error_answer = "error:webrtc-service-unavailable".to_string();
                                        swarm.behaviour_mut().webrtc_signaling_rr
                                            .send_response(channel, WebRTCAnswerResponse { answer_sdp: error_answer })
                                            .unwrap_or_else(|e| error!("send_response failed: {e:?}"));
                                    }
                                }
                                // WebRTC answer response
                                Message::Response { request_id, response } => {
                                    let WebRTCAnswerResponse { ref answer_sdp } = response;
                                    info!("Received WebRTC answer: {}", answer_sdp);

                                    if let Some(tx) = pending_webrtc_offers.lock().await.remove(&request_id) {
                                        let _ = tx.send(Ok(response));
                                    }
                                }
                            },
                            RREvent::OutboundFailure { request_id, error, .. } => {
                                warn!("WebRTC signaling outbound failure: {error:?}");
                                if let Some(tx) = pending_webrtc_offers.lock().await.remove(&request_id) {
                                    let _ = tx.send(Err(format!("outbound failure: {error:?}")));
                                }
                            }
                            RREvent::InboundFailure { error, .. } => {
                                warn!("WebRTC signaling inbound failure: {error:?}");
                            }
                            RREvent::ResponseSent { .. } => {}
                        }
                    }
                    SwarmEvent::IncomingConnectionError { error, .. } => {
                        if let Ok(mut m) = metrics.try_lock() {
                            m.last_error = Some(error.to_string());
                            m.last_error_at = Some(SystemTime::now());
                            m.bootstrap_failures = m.bootstrap_failures.saturating_add(1);
                        }
                        error!("❌ Incoming connection error: {}", error);
                    }
                    SwarmEvent::ListenerClosed { reason, .. } => {
                        if reason.is_ok() {
                            trace!("ListenerClosed Ok; ignoring");
                        } else {
                            let s = format!("{:?}", reason);
                            if let Some(pid) = last_tried_relay.take() {
                                match classify_err_str(&s) {
                                    RelayErrClass::Permanent => {
                                        relay_blacklist.insert(pid);
                                        warn!("🧱 {} marked permanent (unsupported/denied)", pid);
                                    }
                                    RelayErrClass::Transient => {
                                        relay_cooldown.insert(pid, Instant::now() + Duration::from_secs(600));
                                        warn!("⏳ {} cooldown 10m (transient failure): {}", pid, s);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                info!("DHT swarm stream ended; shutting down node task");
                break 'outer;
            }
        }
    }

    connected_peers.lock().await.clear();
    info!("DHT node task exiting");
    if let Some(ack) = shutdown_ack {
        let _ = ack.send(());
    }
}

fn extract_bootstrap_peer_ids(bootstrap_nodes: &[String]) -> HashSet<PeerId> {
    use libp2p::multiaddr::Protocol;
    use libp2p::{Multiaddr, PeerId};

    bootstrap_nodes
        .iter()
        .filter_map(|s| s.parse::<Multiaddr>().ok())
        .filter_map(|ma| {
            ma.iter().find_map(|p| {
                if let Protocol::P2p(mh) = p {
                    PeerId::from_multihash(mh.into()).ok()
                } else {
                    None
                }
            })
        })
        .collect()
}

async fn handle_kademlia_event(
    event: KademliaEvent,
    swarm: &mut Swarm<DhtBehaviour>,
    local_peer_id: &PeerId,
    connected_peers: &Arc<Mutex<HashSet<PeerId>>>,
    event_tx: &mpsc::Sender<DhtEvent>,
    pending_searches: &Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    pending_provider_queries: &Arc<Mutex<HashMap<String, PendingProviderQuery>>>,
    get_providers_queries: &Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>>,
    seeder_heartbeats_cache: &Arc<Mutex<HashMap<String, FileHeartbeatCacheEntry>>>,
    pending_heartbeat_updates: &Arc<Mutex<HashSet<String>>>,
    pending_keyword_indexes: &Arc<Mutex<HashMap<kad::QueryId, PendingKeywordIndex>>>,
) {
    match event {
        KademliaEvent::RoutingUpdated { peer, .. } => {
            debug!("Routing table updated with peer: {}", peer);
        }
        KademliaEvent::UnroutablePeer { peer } => {
            warn!("Peer {} is unroutable", peer);
        }
        KademliaEvent::RoutablePeer { peer, address, .. } => {
            debug!("Peer {} became routable", peer);
            if !ma_plausibly_reachable(&address) {
                swarm
                    .behaviour_mut()
                    .kademlia
                    .remove_address(&peer, &address);
                debug!(
                    "⏭️ Kad RoutablePeer ignored (unreachable): {} -> {}",
                    peer, address
                );
            } else {
                debug!("✅ Kad RoutablePeer accepted: {} -> {}", peer, address);
            }
        }
        KademliaEvent::OutboundQueryProgressed { result, .. } => {
            match result {
                QueryResult::GetRecord(Ok(ok)) => match ok {
                    GetRecordOk::FoundRecord(peer_record) => {
                        // Try to parse DHT record as essential metadata JSON
                        if let Ok(metadata_json) =
                            serde_json::from_slice::<serde_json::Value>(&peer_record.record.value)
                        {
                            // Construct FileMetadata from the JSON
                            if let (
                                Some(file_hash),
                                Some(file_name),
                                Some(file_size),
                                Some(created_at),
                            ) = (
                                // Use merkle_root as the primary identifier
                                metadata_json.get("merkle_root").and_then(|v| v.as_str()),
                                metadata_json.get("file_name").and_then(|v| v.as_str()),
                                metadata_json.get("file_size").and_then(|v| v.as_u64()),
                                metadata_json.get("created_at").and_then(|v| v.as_u64()),
                            ) {
                                let peer_from_record =
                                    peer_record.peer.clone().map(|p| p.to_string());
                                let now = unix_timestamp();

                                let mut heartbeat_entries = metadata_json
                                    .get("seederHeartbeats")
                                    .and_then(|v| {
                                        serde_json::from_value::<Vec<SeederHeartbeat>>(v.clone())
                                            .ok()
                                    })
                                    .unwrap_or_default();

                                let fallback_seeders: Vec<String> = metadata_json
                                    .get("seeders")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect()
                                    })
                                    .unwrap_or_default();

                                if heartbeat_entries.is_empty() && !fallback_seeders.is_empty() {
                                    heartbeat_entries = fallback_seeders
                                        .iter()
                                        .map(|peer| SeederHeartbeat {
                                            peer_id: peer.clone(),
                                            expires_at: now
                                                .saturating_add(FILE_HEARTBEAT_TTL.as_secs()),
                                            last_heartbeat: now,
                                        })
                                        .collect();
                                }

                                if heartbeat_entries.is_empty() {
                                    if let Some(peer_id_str) = peer_from_record.clone() {
                                        heartbeat_entries.push(SeederHeartbeat {
                                            peer_id: peer_id_str,
                                            expires_at: now
                                                .saturating_add(FILE_HEARTBEAT_TTL.as_secs()),
                                            last_heartbeat: now,
                                        });
                                    }
                                }

                                let mut pending_refresh = false;
                                {
                                    let mut pending = pending_heartbeat_updates.lock().await;
                                    if pending.remove(file_hash) {
                                        pending_refresh = true;
                                    }
                                }

                                if pending_refresh {
                                    upsert_heartbeat(
                                        &mut heartbeat_entries,
                                        &local_peer_id.to_string(),
                                        now,
                                    );
                                }

                                let active_heartbeats = prune_heartbeats(heartbeat_entries, now);
                                let active_seeders = heartbeats_to_peer_list(&active_heartbeats);

                                let existing_entry = {
                                    let cache = seeder_heartbeats_cache.lock().await;
                                    cache.get(file_hash).cloned()
                                };

                                let merged_heartbeats = if let Some(entry) = existing_entry {
                                    merge_heartbeats(entry.heartbeats, active_heartbeats.clone())
                                } else {
                                    active_heartbeats.clone()
                                };

                                let mut merged_seeders = heartbeats_to_peer_list(&merged_heartbeats);
                                if merged_seeders.is_empty() && !fallback_seeders.is_empty() {
                                    merged_seeders = fallback_seeders.clone();
                                }

                                let recorded_seeders_set: HashSet<String> =
                                    active_seeders.into_iter().collect();
                                let merged_seeders_set: HashSet<String> =
                                    merged_seeders.iter().cloned().collect();

                                let mut needs_publish = pending_refresh;
                                if merged_seeders_set != recorded_seeders_set {
                                    needs_publish = true;
                                }

                                let mut updated_metadata_json = metadata_json.clone();
                                updated_metadata_json["seeders"] = serde_json::Value::Array(
                                    merged_seeders
                                        .iter()
                                        .cloned()
                                        .map(serde_json::Value::String)
                                        .collect(),
                                );
                                updated_metadata_json["seederHeartbeats"] =
                                    serde_json::to_value(&merged_heartbeats)
                                        .unwrap_or_else(|_| serde_json::Value::Array(vec![]));

                                {
                                    let mut cache = seeder_heartbeats_cache.lock().await;
                                    cache.insert(
                                        file_hash.to_string(),
                                        FileHeartbeatCacheEntry {
                                            heartbeats: merged_heartbeats.clone(),
                                            metadata: updated_metadata_json.clone(),
                                        },
                                    );
                                }

                                let serialized_refresh = if needs_publish {
                                    match serde_json::to_vec(&updated_metadata_json) {
                                        Ok(bytes) => Some(bytes),
                                        Err(e) => {
                                            error!(
                                                "Failed to serialize refreshed heartbeat record for {}: {}",
                                                file_hash, e
                                            );
                                            None
                                        }
                                    }
                                } else {
                                    None
                                };

                                if let Some(bytes) = serialized_refresh {
                                    let key = kad::RecordKey::new(&file_hash.as_bytes());
                                    let record = Record {
                                        key,
                                        value: bytes,
                                        publisher: Some(local_peer_id.clone()),
                                        expires: None,
                                    };

                                    if let Err(e) = swarm
                                        .behaviour_mut()
                                        .kademlia
                                        .put_record(record, kad::Quorum::One)
                                    {
                                        error!(
                                            "Failed to publish refreshed heartbeat record for {}: {}",
                                            file_hash, e
                                        );
                                    }

                                    let provider_key = kad::RecordKey::new(&file_hash.as_bytes());
                                    if let Err(e) =
                                        swarm.behaviour_mut().kademlia.start_providing(provider_key)
                                    {
                                        debug!(
                                            "Failed to refresh provider record for {}: {}",
                                            file_hash, e
                                        );
                                    }
                                }

                                let metadata = FileMetadata {
                                    merkle_root: file_hash.to_string(),
                                    file_name: file_name.to_string(),
                                    file_size,
                                    file_data: Vec::new(), // Will be populated during download
                                    seeders: if merged_seeders.is_empty() {
                                        peer_from_record
                                            .clone()
                                            .into_iter()
                                            .collect::<Vec<String>>()
                                    } else {
                                        merged_seeders.clone()
                                    },
                                    created_at,
                                    mime_type: metadata_json
                                        .get("mime_type")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    is_encrypted: metadata_json
                                        .get("is_encrypted")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false),
                                    encryption_method: metadata_json
                                        .get("encryption_method")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    key_fingerprint: metadata_json
                                        .get("key_fingerprint")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    version: metadata_json
                                        .get("version")
                                        .and_then(|v| v.as_u64())
                                        .map(|v| v as u32),
                                    parent_hash: metadata_json
                                        .get("parent_hash")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    cids: metadata_json.get("cids").and_then(|v| {
                                        serde_json::from_value::<Option<Vec<Cid>>>(v.clone())
                                            .unwrap_or(None)
                                    }),
                                    encrypted_key_bundle: metadata_json
                                        .get("encryptedKeyBundle")
                                        .and_then(|v| {
                                            // The field name is camelCase in the JSON
                                            serde_json::from_value::<
                                                Option<crate::encryption::EncryptedAesKeyBundle>,
                                            >(v.clone())
                                            .unwrap_or(None)
                                        }),
                                    is_root: metadata_json
                                        .get("is_root")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(true),
                                    price: metadata_json.get("price").and_then(|v| v.as_f64()),
                                    uploader_address: metadata_json
                                        .get("uploader_address")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    ..Default::default()
                                };

                                println!("🔎 DHT: Retrieved metadata from DHT - price: {:?}, uploader: {:?}", metadata.price, metadata.uploader_address);

                                let notify_metadata = metadata.clone();
                                let file_hash = notify_metadata.merkle_root.clone();
                                info!(
                                    "File discovered: {} ({})",
                                    notify_metadata.file_name, file_hash
                                );
                                let _ = event_tx.send(DhtEvent::FileDiscovered(metadata)).await;

                                // only for synchronous_search_metadata
                                notify_pending_searches(
                                    pending_searches,
                                    &file_hash,
                                    SearchResponse::Found(notify_metadata),
                                )
                                .await;
                            } else {
                                debug!("DHT record missing required fields");
                            }
                        } else {
                            debug!("Received non-JSON DHT record");
                        }
                    }
                    GetRecordOk::FinishedWithNoAdditionalRecord { .. } => {
                        // No additional records; do nothing here
                    }
                },
                QueryResult::GetRecord(Err(err)) => {
                    warn!("GetRecord error: {:?}", err);
                    // If the error includes the key, emit FileNotFound
                    if let kad::GetRecordError::NotFound { key, .. } = err {
                        let file_hash = String::from_utf8_lossy(key.as_ref()).to_string();
                        let _ = event_tx
                            .send(DhtEvent::FileNotFound(file_hash.clone()))
                            .await;
                        notify_pending_searches(
                            pending_searches,
                            &file_hash,
                            SearchResponse::NotFound,
                        )
                        .await;
                    }
                }
                QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                    debug!("PutRecord succeeded for key: {:?}", key);
                }
                QueryResult::PutRecord(Err(err)) => {
                    warn!("PutRecord error: {:?}", err);
                    let _ = event_tx
                        .send(DhtEvent::Error(format!("PutRecord failed: {:?}", err)))
                        .await;
                }
                QueryResult::GetClosestPeers(Ok(ok)) => match ok {
                    kad::GetClosestPeersOk { key, peers } => {
                        let target_peer_id = match PeerId::from_bytes(&key) {
                            Ok(peer_id) => peer_id,
                            Err(e) => {
                                warn!("Failed to parse peer ID from GetClosestPeers key: {}", e);
                                return;
                            }
                        };

                        info!(
                            "Found {} closest peers for target peer {}",
                            peers.len(),
                            target_peer_id
                        );

                        // Attempt to connect to the discovered peers
                        let mut connection_attempts = 0;
                        for peer_info in &peers {
                            // Check if this peer is already connected
                            let is_connected = {
                                let connected = connected_peers.lock().await;
                                connected.contains(&peer_info.peer_id)
                            };

                            if is_connected {
                                info!("Peer {} is already connected", peer_info.peer_id);
                                continue;
                            }

                            // Try to connect using available addresses
                            let mut connected = false;
                            for addr in &peer_info.addrs {
                                if not_loopback(addr) {
                                    info!(
                                        "Attempting to connect to peer {} at {}",
                                        peer_info.peer_id, addr
                                    );
                                    // Add address to Kademlia routing table
                                    swarm
                                        .behaviour_mut()
                                        .kademlia
                                        .add_address(&peer_info.peer_id, addr.clone());

                                    // Attempt direct connection
                                    match swarm.dial(addr.clone()) {
                                        Ok(_) => {
                                            info!(
                                                "✅ Initiated connection to peer {} at {}",
                                                peer_info.peer_id, addr
                                            );
                                            connected = true;
                                            connection_attempts += 1;
                                            break; // Successfully initiated connection, no need to try other addresses
                                        }
                                        Err(e) => {
                                            debug!(
                                                "Failed to dial peer {} at {}: {}",
                                                peer_info.peer_id, addr, e
                                            );
                                        }
                                    }
                                }
                            }

                            if !connected {
                                info!(
                                    "Could not connect to peer {} with any available address",
                                    peer_info.peer_id
                                );
                            }
                        }

                        let _ = event_tx
                            .send(DhtEvent::Info(format!(
                            "Found {} peers close to target peer {}, attempted connections to {}",
                            peers.len(),
                            target_peer_id,
                            connection_attempts
                        )))
                            .await;
                    }
                },
                QueryResult::GetClosestPeers(Err(err)) => {
                    warn!("GetClosestPeers query failed: {:?}", err);
                    let _ = event_tx
                        .send(DhtEvent::Error(format!("Peer discovery failed: {:?}", err)))
                        .await;
                }
                QueryResult::GetProviders(Ok(ok)) => {
                    if let kad::GetProvidersOk::FoundProviders { key, providers } = ok {
                        let file_hash = String::from_utf8_lossy(key.as_ref()).to_string();
                        info!(
                            "Found {} providers for file: {}",
                            providers.len(),
                            file_hash
                        );

                        // Convert providers to string format
                        let provider_strings: Vec<String> =
                            providers.iter().map(|p| p.to_string()).collect();

                        // Find and notify the pending query
                        let mut pending_queries = pending_provider_queries.lock().await;
                        if let Some(pending_query) = pending_queries.remove(&file_hash) {
                            let _ = pending_query.sender.send(Ok(provider_strings));
                        } else {
                            warn!("No pending provider query found for file: {}", file_hash);
                        }
                    }
                }
                QueryResult::GetProviders(Err(err)) => {
                    // Implement proper GetProviders error handling with timeout tracking
                    warn!("GetProviders query failed: {:?}", err);

                    // Check for timed-out queries and clean them up
                    let mut timed_out_queries = Vec::new();
                    {
                        let get_providers_guard = get_providers_queries.lock().await;
                        let now = std::time::Instant::now();
                        let timeout_duration = std::time::Duration::from_secs(30); // 30 second timeout

                        // Find queries that have timed out
                        for (query_id, (file_hash, start_time)) in get_providers_guard.iter() {
                            if now.duration_since(*start_time) > timeout_duration {
                                timed_out_queries.push((*query_id, file_hash.clone()));
                            }
                        }

                        // For remaining queries, mark them as failed since we can't match errors exactly
                        for (query_id, (file_hash, _)) in get_providers_guard.iter() {
                            if !timed_out_queries.iter().any(|(_, fh)| fh == file_hash) {
                                timed_out_queries.push((*query_id, file_hash.clone()));
                            }
                        }
                    }

                    // Handle all failed/timed-out queries
                    for (query_id, file_hash) in timed_out_queries {
                        warn!(
                            "Cleaning up GetProviders query for file: {} (failed or timed out)",
                            file_hash
                        );
                        get_providers_queries.lock().await.remove(&query_id);

                        if let Some(pending_query) =
                            pending_provider_queries.lock().await.remove(&file_hash)
                        {
                            let _ = pending_query.sender.send(Err(format!(
                                "GetProviders query failed or timed out for file {}: {:?}",
                                file_hash, err
                            )));
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}
async fn handle_identify_event(
    event: IdentifyEvent,
    swarm: &mut Swarm<DhtBehaviour>,
    event_tx: &mpsc::Sender<DhtEvent>,
    metrics: Arc<Mutex<DhtMetrics>>,
    enable_autorelay: bool,
    relay_candidates: &HashSet<String>,
    proxy_mgr: &ProxyMgr,
    relay_capable_peers: Arc<Mutex<HashMap<PeerId, Vec<Multiaddr>>>>,
) {
    match event {
        IdentifyEvent::Received { peer_id, info, .. } => {
            let hop_proto = "/libp2p/circuit/relay/0.2.0/hop";
            let supports_relay = info.protocols.iter().any(|p| p.as_ref() == hop_proto);

            if supports_relay {
                info!(
                    "🛰️ Peer {} supports relay (HOP protocol advertised)",
                    peer_id
                );

                // Store this peer as relay-capable with its listen addresses
                let reachable_addrs: Vec<Multiaddr> = info
                    .listen_addrs
                    .iter()
                    .filter(|addr| ma_plausibly_reachable(addr))
                    .cloned()
                    .collect();

                if !reachable_addrs.is_empty() {
                    let mut relay_peers = relay_capable_peers.lock().await;
                    relay_peers.insert(peer_id, reachable_addrs.clone());
                    info!(
                        "✅ Added {} to relay-capable peers list ({} addresses)",
                        peer_id,
                        reachable_addrs.len()
                    );
                    for (i, addr) in reachable_addrs.iter().enumerate().take(3) {
                        info!("   Relay address {}: {}", i + 1, addr);
                    }
                }
            }

            let listen_addrs = info.listen_addrs.clone();

            // identify::Event::Received { peer_id, info, .. } => { ... }
            for addr in info.listen_addrs.iter() {
                info!("  📍 Peer {} listen addr: {}", peer_id, addr);

                if ma_plausibly_reachable(addr) {
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                } else {
                    debug!(
                        "⏭️ Ignoring unreachable listen addr from {}: {}",
                        peer_id, addr
                    );
                }

                // Relay Setting: from candidate's "public base", create /p2p-circuit
                if enable_autorelay && is_relay_candidate(&peer_id, relay_candidates) {
                    if let Some(base_str) = relay_candidates
                        .iter()
                        .find(|s| s.contains(&peer_id.to_string()))
                    {
                        if let Ok(base) = base_str.parse::<Multiaddr>() {
                            if let Some(relay_addr) = build_relay_listen_addr(&base) {
                                info!(
                                    "📡 Attempting to listen via relay {} at {}",
                                    peer_id, relay_addr
                                );
                                if let Err(e) = swarm.listen_on(relay_addr.clone()) {
                                    warn!(
                                        "Failed to listen on relay address {}: {}",
                                        relay_addr, e
                                    );
                                } else {
                                    info!("📡 Attempting to listen via relay peer {}", peer_id);
                                }
                            } else {
                                debug!("⚠️ Could not derive relay listen addr from base: {}", base);
                            }
                        } else {
                            debug!("⚠️ Invalid relay base multiaddr: {}", base_str);
                        }
                    } else {
                        debug!("⚠️ No relay base in preferred_relays for {}", peer_id);
                    }
                }
            }
        }
        IdentifyEvent::Pushed { peer_id, info, .. } => {
            info!(
                "Pushed identify update to {} (listen addrs: {})",
                peer_id,
                info.listen_addrs.len()
            );
            record_identify_push_metrics(&metrics, &info).await;
        }
        IdentifyEvent::Sent { peer_id, .. } => {
            debug!("Sent identify info to {}", peer_id);
        }
        IdentifyEvent::Error { peer_id, error, .. } => {
            warn!("Identify protocol error with {}: {}", peer_id, error);
            let _ = event_tx
                .send(DhtEvent::Error(format!(
                    "Identify error with {}: {}",
                    peer_id, error
                )))
                .await;
        }
    }
}

async fn handle_mdns_event(
    event: MdnsEvent,
    swarm: &mut Swarm<DhtBehaviour>,
    event_tx: &mpsc::Sender<DhtEvent>,
) {
    match event {
        MdnsEvent::Discovered(list) => {
            let mut discovered: HashMap<PeerId, Vec<String>> = HashMap::new();
            for (peer_id, multiaddr) in list {
                debug!("mDNS discovered peer {} at {}", peer_id, multiaddr);
                if ma_plausibly_reachable(&multiaddr) {
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, multiaddr.clone());
                } else {
                    debug!(
                        "⏭️  mDNS discovered (ignored unreachable): {} @ {}",
                        peer_id, multiaddr
                    );
                }
                discovered
                    .entry(peer_id)
                    .or_default()
                    .push(multiaddr.to_string());
            }
            for (peer_id, addresses) in discovered {
                let _ = event_tx
                    .send(DhtEvent::PeerDiscovered {
                        peer_id: peer_id.to_string(),
                        addresses,
                    })
                    .await;
            }
        }
        MdnsEvent::Expired(list) => {
            for (peer_id, multiaddr) in list {
                debug!("mDNS expired peer {} at {}", peer_id, multiaddr);
                swarm
                    .behaviour_mut()
                    .kademlia
                    .remove_address(&peer_id, &multiaddr);
            }
        }
    }
}

async fn handle_ping_event(event: PingEvent) {
    match event {
        ping::Event { result, .. } => {
            debug!("Ping result: {:?}", result);
        }
    }
}

async fn handle_autonat_client_event(
    event: v2::client::Event,
    metrics: &Arc<Mutex<DhtMetrics>>,
    event_tx: &mpsc::Sender<DhtEvent>,
) {
    let v2::client::Event {
        tested_addr,
        server,
        bytes_sent,
        result,
    } = event;

    let mut metrics_guard = metrics.lock().await;
    if !metrics_guard.autonat_enabled {
        return;
    }

    let addr_str = tested_addr.to_string();
    let server_str = server.to_string();
    let (state, summary) = match result {
        Ok(()) => {
            metrics_guard.record_observed_addr(&tested_addr);
            info!(
                server = %server_str,
                address = %addr_str,
                bytes = bytes_sent,
                "AutoNAT probe succeeded"
            );
            (
                NatReachabilityState::Public,
                Some(format!(
                    "Confirmed reachability via {addr_str} (server {server_str})"
                )),
            )
        }
        Err(err) => {
            let err_msg = err.to_string();
            warn!(
                server = %server_str,
                address = %addr_str,
                error = %err_msg,
                bytes = bytes_sent,
                "AutoNAT probe failed"
            );
            (
                NatReachabilityState::Private,
                Some(format!(
                    "Probe via {addr_str} (server {server_str}) failed: {err_msg}"
                )),
            )
        }
    };

    metrics_guard.update_reachability(state, summary.clone());
    let nat_state = metrics_guard.reachability_state;
    let confidence = metrics_guard.reachability_confidence;
    let last_error = metrics_guard.last_reachability_error.clone();
    drop(metrics_guard);

    let _ = event_tx
        .send(DhtEvent::NatStatus {
            state: nat_state,
            confidence,
            last_error,
            summary,
        })
        .await;
}

async fn handle_dcutr_event(
    event: dcutr::Event,
    metrics: &Arc<Mutex<DhtMetrics>>,
    event_tx: &mpsc::Sender<DhtEvent>,
) {
    let mut metrics_guard = metrics.lock().await;
    if !metrics_guard.dcutr_enabled {
        return;
    }

    let dcutr::Event {
        remote_peer_id,
        result,
    } = event;

    metrics_guard.dcutr_hole_punch_attempts += 1;

    match result {
        Ok(_connection_id) => {
            metrics_guard.dcutr_hole_punch_successes += 1;
            metrics_guard.last_dcutr_success = Some(SystemTime::now());
            info!(
                peer = %remote_peer_id,
                successes = metrics_guard.dcutr_hole_punch_successes,
                "DCUtR: hole-punch succeeded, upgraded to direct connection"
            );
            drop(metrics_guard);
            let _ = event_tx
                .send(DhtEvent::Info(format!(
                    "✓ Direct connection established with peer {} (hole-punch succeeded)",
                    remote_peer_id
                )))
                .await;
        }
        Err(error) => {
            metrics_guard.dcutr_hole_punch_failures += 1;
            metrics_guard.last_dcutr_failure = Some(SystemTime::now());
            warn!(
                peer = %remote_peer_id,
                error = %error,
                failures = metrics_guard.dcutr_hole_punch_failures,
                "DCUtR: hole-punch failed"
            );
            drop(metrics_guard);
            let _ = event_tx
                .send(DhtEvent::Warning(format!(
                    "✗ Direct connection upgrade to peer {} failed: {}",
                    remote_peer_id, error
                )))
                .await;
        }
    }
}

async fn handle_external_addr_confirmed(
    addr: &Multiaddr,
    metrics: &Arc<Mutex<DhtMetrics>>,
    event_tx: &mpsc::Sender<DhtEvent>,
    proxy_mgr: &ProxyMgr,
) {
    let mut metrics_guard = metrics.lock().await;
    let nat_enabled = metrics_guard.autonat_enabled;
    metrics_guard.record_observed_addr(addr);
    if metrics_guard.reachability_state == NatReachabilityState::Public {
        drop(metrics_guard);
        return;
    }
    let summary = Some(format!("External address confirmed: {}", addr));
    metrics_guard.update_reachability(NatReachabilityState::Public, summary.clone());
    let state = metrics_guard.reachability_state;
    let confidence = metrics_guard.reachability_confidence;
    let last_error = metrics_guard.last_reachability_error.clone();
    drop(metrics_guard);

    if nat_enabled {
        let _ = event_tx
            .send(DhtEvent::NatStatus {
                state,
                confidence,
                last_error,
                summary: summary.clone(),
            })
            .await;
    }

    if let Some(relay_peer_id) = extract_relay_peer(addr) {
        // Update relay metrics to reflect an active (listening) relay external address
        if let Ok(mut m) = metrics.try_lock() {
            m.active_relay_peer_id = Some(relay_peer_id.to_string());
            m.relay_reservation_status = Some("active".to_string());
        }
        let mut mgr = proxy_mgr.lock().await;
        let newly_ready = mgr.mark_relay_ready(relay_peer_id.clone());
        drop(mgr);
        let status = if newly_ready {
            "relay_ready"
        } else {
            "relay_address"
        };
        let _ = event_tx
            .send(DhtEvent::ProxyStatus {
                id: relay_peer_id.to_string(),
                address: addr.to_string(),
                status: status.into(),
                latency_ms: None,
                error: None,
            })
            .await;
    }
}

async fn handle_external_addr_expired(
    addr: &Multiaddr,
    metrics: &Arc<Mutex<DhtMetrics>>,
    event_tx: &mpsc::Sender<DhtEvent>,
    proxy_mgr: &ProxyMgr,
) {
    let summary_text = format!("External address expired: {}", addr);
    let mut metrics_guard = metrics.lock().await;
    let nat_enabled = metrics_guard.autonat_enabled;
    metrics_guard.remove_observed_addr(addr);

    if metrics_guard.observed_addrs.is_empty()
        && metrics_guard.reachability_state != NatReachabilityState::Unknown
    {
        let summary = Some(summary_text);
        metrics_guard.update_reachability(NatReachabilityState::Unknown, summary.clone());
        let state = metrics_guard.reachability_state;
        let confidence = metrics_guard.reachability_confidence;
        let last_error = metrics_guard.last_reachability_error.clone();
        drop(metrics_guard);

        if nat_enabled {
            let _ = event_tx
                .send(DhtEvent::NatStatus {
                    state,
                    confidence,
                    last_error,
                    summary: summary.clone(),
                })
                .await;
        }
    }

    if let Some(relay_peer_id) = extract_relay_peer(addr) {
        // Mark relay as expired in metrics
        if let Ok(mut m) = metrics.try_lock() {
            m.relay_reservation_status = Some("expired".to_string());
            m.active_relay_peer_id = None;
            m.reservation_evictions = m.reservation_evictions.saturating_add(1);
        }
        let mut mgr = proxy_mgr.lock().await;
        mgr.relay_ready.remove(&relay_peer_id);
        mgr.relay_pending.remove(&relay_peer_id);
        drop(mgr);
        let _ = event_tx
            .send(DhtEvent::ProxyStatus {
                id: relay_peer_id.to_string(),
                address: addr.to_string(),
                status: "relay_expired".into(),
                latency_ms: None,
                error: None,
            })
            .await;
    }
}

impl Socks5Transport {
    pub fn new(proxy: SocketAddr) -> Self {
        Self { proxy }
    }
}

/// Build a libp2p transport, optionally tunneling through a SOCKS5 proxy.
/// - Output type is unified to (PeerId, StreamMuxerBox).
/// - Dial preference: Relay first, then Direct TCP (or SOCKS5 TCP if proxy is set).
pub fn build_transport_with_relay(
    keypair: &identity::Keypair,
    relay_transport: relay::client::Transport,
    proxy_address: Option<String>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>, Box<dyn Error>> {
    use libp2p::{
        core::{muxing::StreamMuxerBox, transport::Boxed, upgrade::Version},
        noise, tcp, yamux, Transport as _,
    };
    use std::{io, net::SocketAddr, time::Duration};

    // === Upgrade stack for direct TCP/SOCKS5 paths ===
    let noise_cfg = noise::Config::new(keypair)?;
    let yamux_cfg = yamux::Config::default();

    // TCP/SOCKS5 → (PeerId, StreamMuxerBox)
    let into_muxed = |t: Boxed<Box<dyn AsyncIo>>| {
        t.upgrade(Version::V1Lazy)
            .authenticate(noise_cfg.clone())
            .multiplex(yamux_cfg.clone())
            .timeout(Duration::from_secs(20))
            .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
            .boxed()
    };

    // --- Direct TCP path ---
    let tcp_base: Boxed<Box<dyn AsyncIo>> =
        tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
            .map(|s, _| -> Box<dyn AsyncIo> { Box::new(s.0.compat()) })
            .boxed();

    // --- SOCKS5 path (optional) ---
    let direct_tcp_muxed: Boxed<(PeerId, StreamMuxerBox)> = match proxy_address {
        Some(proxy) => {
            info!(
                "SOCKS5 enabled. Routing all P2P TCP dialing traffic via {}",
                proxy
            );
            let proxy_addr: SocketAddr = proxy.parse().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid proxy address: {}", e),
                )
            })?;
            let socks5: Boxed<Box<dyn AsyncIo>> = Socks5Transport::new(proxy_addr).boxed();
            into_muxed(socks5)
        }
        None => into_muxed(tcp_base),
    };

    // --- Relay path: Connection → (PeerId, StreamMuxerBox)
    // Apply the same upgrade stack to the relay transport
    let relay_muxed: Boxed<(PeerId, StreamMuxerBox)> = relay_transport
        .upgrade(Version::V1Lazy)
        .authenticate(noise_cfg.clone())
        .multiplex(yamux_cfg.clone())
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    // --- Combine: Relay first, then Direct ---
    let layered: Boxed<(PeerId, StreamMuxerBox)> =
        libp2p::core::transport::OrTransport::new(relay_muxed, direct_tcp_muxed)
            .map(|either, _| match either {
                futures::future::Either::Left(v) => v,
                futures::future::Either::Right(v) => v,
            })
            .boxed();

    Ok(layered)
}

impl DhtService {
    pub async fn send_webrtc_offer(
        &self,
        peer: String,
        offer_request: WebRTCOfferRequest,
    ) -> Result<oneshot::Receiver<Result<WebRTCAnswerResponse, String>>, String> {
        let peer_id: PeerId = peer.parse().map_err(|e| format!("invalid peer id: {e}"))?;
        let (tx, rx) = oneshot::channel();

        self.cmd_tx
            .send(DhtCommand::SendWebRTCOffer {
                peer: peer_id,
                offer_request,
                sender: tx,
            })
            .await
            .map_err(|e| format!("send webrtc offer cmd: {e}"))?;

        Ok(rx)
    }
}

// Public API for the DHT
pub struct DhtService {
    cmd_tx: mpsc::Sender<DhtCommand>,
    event_rx: Arc<Mutex<mpsc::Receiver<DhtEvent>>>,
    peer_id: String,
    connected_peers: Arc<Mutex<HashSet<PeerId>>>,
    connected_addrs: HashMap<PeerId, Vec<Multiaddr>>,
    metrics: Arc<Mutex<DhtMetrics>>,
    pending_echo: Arc<Mutex<HashMap<rr::OutboundRequestId, PendingEcho>>>,
    pending_searches: Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    search_counter: Arc<AtomicU64>,
    proxy_mgr: ProxyMgr,
    peer_selection: Arc<Mutex<PeerSelectionService>>,
    file_metadata_cache: Arc<Mutex<HashMap<String, FileMetadata>>>,
    received_chunks: Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: Option<Arc<FileTransferService>>,
    // chunk_manager: Option<Arc<ChunkManager>>, // Not needed here
    pending_webrtc_offers: Arc<
        Mutex<
            HashMap<rr::OutboundRequestId, oneshot::Sender<Result<WebRTCAnswerResponse, String>>>,
        >,
    >,
    pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>>,
    root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>>,
    active_downloads: Arc<Mutex<HashMap<String, Arc<Mutex<ActiveDownload>>>>>,
    get_providers_queries: Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>>,
    chunk_size: usize,
    file_heartbeat_state: Arc<Mutex<HashMap<String, FileHeartbeatState>>>,
    seeder_heartbeats_cache: Arc<Mutex<HashMap<String, FileHeartbeatCacheEntry>>>,
    pending_heartbeat_updates: Arc<Mutex<HashSet<String>>>,
}
use memmap2::MmapMut;
use std::fs::OpenOptions;

#[derive(Debug)]
struct ActiveDownload {
    metadata: FileMetadata,
    queries: HashMap<beetswap::QueryId, u32>,
    temp_file_path: PathBuf,  // Path with .tmp suffix
    final_file_path: PathBuf, // Final path without .tmp
    mmap: Arc<std::sync::Mutex<MmapMut>>,
    received_chunks: Arc<std::sync::Mutex<HashSet<u32>>>,
    total_chunks: u32,
    chunk_offsets: Vec<u64>,
}

impl ActiveDownload {
    fn new(
        metadata: FileMetadata,
        queries: HashMap<beetswap::QueryId, u32>,
        download_dir: &PathBuf,
        total_size: u64,
        chunk_offsets: Vec<u64>,
    ) -> std::io::Result<Self> {
        let total_chunks = queries.len() as u32;

        // Create final filename based on the actual file name
        // let final_file_path = download_dir.join(&metadata.file_name);
        let final_file_path = download_dir.clone();
        // Create temporary filename with .tmp suffix
        let temp_file_path = download_dir.with_extension("tmp");
        info!("Creating temp file at: {:?}", temp_file_path);
        info!("Will rename to: {:?} when complete", final_file_path);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&temp_file_path)?;

        file.set_len(total_size)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        Ok(Self {
            metadata,
            queries,
            temp_file_path,
            final_file_path,
            mmap: Arc::new(std::sync::Mutex::new(mmap)),
            received_chunks: Arc::new(std::sync::Mutex::new(HashSet::new())),
            total_chunks,
            chunk_offsets,
        })
    }

    fn write_chunk(&self, chunk_index: u32, data: &[u8], offset: u64) -> std::io::Result<()> {
        let mut mmap = self.mmap.lock().unwrap();
        let start = offset as usize;
        let end = start + data.len();

        if end > mmap.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Chunk {} would exceed file bounds", chunk_index),
            ));
        }

        mmap[start..end].copy_from_slice(data);
        self.received_chunks.lock().unwrap().insert(chunk_index);

        Ok(())
    }

    fn is_complete(&self) -> bool {
        self.queries.is_empty()
            && self.received_chunks.lock().unwrap().len() == self.total_chunks as usize
    }

    fn flush(&self) -> std::io::Result<()> {
        self.mmap.lock().unwrap().flush()
    }

    fn read_complete_file(&self) -> std::io::Result<Vec<u8>> {
        let mmap = self.mmap.lock().unwrap();
        Ok(mmap.to_vec())
    }

    /// Finalize the download by renaming .tmp file to final filename
    fn finalize(&self) -> std::io::Result<()> {
        // First, flush to ensure all data is written
        self.flush()?;

        // Drop the mmap to release the file handle
        drop(self.mmap.lock().unwrap());

        info!(
            "Renaming {:?} to {:?}",
            self.temp_file_path, self.final_file_path
        );

        // Rename the temp file to the final file
        std::fs::rename(&self.temp_file_path, &self.final_file_path)?;

        info!("Successfully finalized file: {:?}", self.final_file_path);
        Ok(())
    }

    /// Clean up temp file (only call if download fails/is cancelled)
    fn cleanup(&self) {
        if self.temp_file_path.exists() {
            if let Err(e) = std::fs::remove_file(&self.temp_file_path) {
                error!(
                    "Failed to cleanup temp file {:?}: {}",
                    self.temp_file_path, e
                );
            } else {
                info!("Cleaned up temp file: {:?}", self.temp_file_path);
            }
        }
    }

    fn progress(&self) -> f32 {
        let received = self.received_chunks.lock().unwrap().len() as f32;
        received / self.total_chunks as f32
    }

    fn chunks_received(&self) -> usize {
        self.received_chunks.lock().unwrap().len()
    }
}

impl Clone for ActiveDownload {
    fn clone(&self) -> Self {
        Self {
            metadata: self.metadata.clone(),
            queries: self.queries.clone(),
            temp_file_path: self.temp_file_path.clone(),
            final_file_path: self.final_file_path.clone(),
            mmap: Arc::clone(&self.mmap),
            received_chunks: Arc::clone(&self.received_chunks),
            total_chunks: self.total_chunks,
            chunk_offsets: self.chunk_offsets.clone(),
        }
    }
}

impl Drop for ActiveDownload {
    fn drop(&mut self) {
        // Only cleanup temp file if this is the last reference and file wasn't finalized
        if Arc::strong_count(&self.mmap) == 1 {
            self.cleanup();
        }
    }
}
#[derive(Debug)]
struct FileHeartbeatState {
    /// Handle to the periodic heartbeat task so we can cancel it when seeding stops.
    task: JoinHandle<()>,
}

impl DhtService {
    pub async fn new(
        port: u16,
        bootstrap_nodes: Vec<String>,
        secret: Option<String>,
        is_bootstrap: bool,
        enable_autonat: bool,
        autonat_probe_interval: Option<Duration>,
        autonat_servers: Vec<String>,
        proxy_address: Option<String>,
        file_transfer_service: Option<Arc<FileTransferService>>,
        chunk_manager: Option<Arc<ChunkManager>>,
        chunk_size_kb: Option<usize>, // Chunk size in KB (default 256)
        cache_size_mb: Option<usize>, // Cache size in MB (default 1024)
        enable_autorelay: bool,
        preferred_relays: Vec<String>,
        enable_relay_server: bool,
        blockstore_db_path: Option<&Path>,
    ) -> Result<Self, Box<dyn Error>> {
        // ---- Hotfix: finalize AutoRelay flag (bootstrap OFF + ENV OFF)
        let mut final_enable_autorelay = enable_autorelay;
        if is_bootstrap {
            final_enable_autorelay = false;
            info!("AutoRelay disabled on bootstrap (hotfix).");
        }
        if std::env::var("CHIRAL_DISABLE_AUTORELAY").ok().as_deref() == Some("1") {
            final_enable_autorelay = false;
            info!("AutoRelay disabled via env CHIRAL_DISABLE_AUTORELAY=1");
        }
        // Convert chunk size from KB to bytes
        let chunk_size = chunk_size_kb.unwrap_or(256) * 1024; // Default 256 KB
        let cache_size = cache_size_mb.unwrap_or(1024); // Default 1024 MB
        let blockstore = if let Some(path) = blockstore_db_path {
            if let Some(path_str) = path.to_str() {
                info!("Attempting to use blockstore from disk: {}", path_str);
            }

            match RedbBlockstore::open(path).await {
                Ok(store) => {
                    info!("Successfully opened blockstore from disk");
                    Arc::new(store)
                }
                Err(e) => {
                    warn!("Failed to open blockstore from disk ({}), falling back to in-memory storage", e);
                    Arc::new(RedbBlockstore::in_memory()?)
                }
            }
        } else {
            info!("Using in-memory blockstore");
            Arc::new(RedbBlockstore::in_memory()?)
        };
        // Generate a new keypair for this node
        // If a secret is provided, derive a stable 32-byte seed via SHA-256(secret)
        // Otherwise, generate a fresh random key.
        let local_key = match secret {
            Some(secret_str) => {
                let mut hasher = Sha256::new();
                hasher.update(secret_str.as_bytes());
                let digest = hasher.finalize();
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&digest[..32]);
                identity::Keypair::ed25519_from_bytes(seed)?
            }
            None => identity::Keypair::generate_ed25519(),
        };
        let local_peer_id = PeerId::from(local_key.public());
        let peer_id_str = local_peer_id.to_string();

        // Create a Kademlia behaviour with tuned configuration
        let store = MemoryStore::new(local_peer_id);
        let mut kad_cfg = KademliaConfig::new(StreamProtocol::new("/chiral/kad/1.0.0"));
        let bootstrap_interval = Duration::from_secs(1);
        if is_bootstrap {
            // These settings result in node to not provide files, only acts as a router
            kad_cfg.set_record_ttl(Some(Duration::from_secs(0)));
            kad_cfg.set_provider_record_ttl(Some(Duration::from_secs(0)));

            // ensures bootstrap node only keeps active peers in its routing table
            kad_cfg.set_periodic_bootstrap_interval(None);
        } else {
            // Only enable periodic bootstrap if we have bootstrap nodes
            // This prevents "No known peers" warnings when running standalone
            if !bootstrap_nodes.is_empty() {
                kad_cfg.set_periodic_bootstrap_interval(Some(bootstrap_interval));
            } else {
                kad_cfg.set_periodic_bootstrap_interval(None);
                info!("Periodic bootstrap disabled - no bootstrap nodes configured");
            }
        }

        // Align with docs: shorter queries, higher replication
        kad_cfg.set_query_timeout(Duration::from_secs(30));

        // Replication factor of 3 (as per spec table)
        if let Some(nz) = std::num::NonZeroUsize::new(3) {
            kad_cfg.set_replication_factor(nz);
        }
        let mut kademlia = Kademlia::with_config(local_peer_id, store, kad_cfg);

        // Set Kademlia to server mode to accept incoming connections
        kademlia.set_mode(Some(Mode::Server));

        // Create identify behaviour with proactive push updates
        let identify_config =
            identify::Config::new(EXPECTED_PROTOCOL_VERSION.to_string(), local_key.public())
                .with_agent_version(format!("chiral-network/{}", env!("CARGO_PKG_VERSION")))
                .with_push_listen_addr_updates(true);
        let identify = identify::Behaviour::new(identify_config);

        // mDNS for local peer discovery
        let disable_mdns_env = std::env::var("CHIRAL_DISABLE_MDNS").ok().as_deref() == Some("1");
        let mdns_opt = if disable_mdns_env {
            tracing::info!("mDNS disabled via env CHIRAL_DISABLE_MDNS=1");
            None
        } else {
            Some(Mdns::new(Default::default(), local_peer_id)?)
        };

        // Request-Response behaviours
        let rr_cfg = rr::Config::default();
        let proxy_protocols =
            std::iter::once(("/chiral/proxy/1.0.0".to_string(), rr::ProtocolSupport::Full));
        let proxy_rr = rr::Behaviour::new(proxy_protocols, rr_cfg.clone());

        let webrtc_protocols = std::iter::once((
            "/chiral/webrtc-signaling/1.0.0".to_string(),
            rr::ProtocolSupport::Full,
        ));
        let webrtc_signaling_rr = rr::Behaviour::new(webrtc_protocols, rr_cfg.clone());

        let key_request_protocols =
            std::iter::once((KeyRequestProtocol, rr::ProtocolSupport::Full));
        let key_request = rr::Behaviour::new(key_request_protocols, rr_cfg);

        let probe_interval = autonat_probe_interval.unwrap_or(Duration::from_secs(30));
        let autonat_client_behaviour = if enable_autonat {
            info!(
                "AutoNAT enabled (probe interval: {}s)",
                probe_interval.as_secs()
            );
            Some(v2::client::Behaviour::new(
                OsRng,
                v2::client::Config::default().with_probe_interval(probe_interval),
            ))
        } else {
            None
        };
        let autonat_server_behaviour = if is_bootstrap && enable_autonat {
            Some(v2::server::Behaviour::new(OsRng))
        } else {
            None
        };

        let bitswap = beetswap::Behaviour::new(blockstore);
        let (relay_transport, relay_client_behaviour) = relay::client::new(local_peer_id);
        let autonat_client_toggle = toggle::Toggle::from(autonat_client_behaviour);
        let autonat_server_toggle = toggle::Toggle::from(autonat_server_behaviour);
        let mdns_toggle = toggle::Toggle::from(mdns_opt);

        // DCUtR requires relay to be enabled
        let dcutr_behaviour = if enable_autonat {
            info!("DCUtR enabled (requires relay for hole-punching coordination)");
            Some(dcutr::Behaviour::new(local_peer_id))
        } else {
            None
        };
        let dcutr_toggle = toggle::Toggle::from(dcutr_behaviour);

        // Relay server configuration
        let relay_server_behaviour = if enable_relay_server {
            info!("🔁 Relay server enabled - this node can relay traffic for others");
            Some(relay::Behaviour::new(
                local_peer_id,
                relay::Config::default(),
            ))
        } else {
            None
        };
        let relay_server_toggle = toggle::Toggle::from(relay_server_behaviour);

        let mut behaviour = Some(DhtBehaviour {
            kademlia,
            identify,
            mdns: mdns_toggle,
            bitswap,
            ping: Ping::new(ping::Config::new()),
            proxy_rr,
            webrtc_signaling_rr,
            key_request,
            autonat_client: autonat_client_toggle,
            autonat_server: autonat_server_toggle,
            relay_client: relay_client_behaviour,
            relay_server: relay_server_toggle,
            dcutr: dcutr_toggle,
        });

        let bootstrap_set: HashSet<String> = bootstrap_nodes.iter().cloned().collect();
        let mut autonat_targets: HashSet<String> = if enable_autonat && !autonat_servers.is_empty()
        {
            autonat_servers.into_iter().collect()
        } else {
            HashSet::new()
        };
        if enable_autonat {
            autonat_targets.extend(bootstrap_set.iter().cloned());
        }

        // Configure AutoRelay relay candidate discovery (use finalized flag)
        let relay_candidates: HashSet<String> = if final_enable_autorelay {
            if !preferred_relays.is_empty() {
                info!(
                    "🔗 AutoRelay enabled with {} preferred relays",
                    preferred_relays.len()
                );
                for (i, relay) in preferred_relays.iter().enumerate().take(5) {
                    info!("   Relay {}: {}", i + 1, relay);
                }
                preferred_relays.into_iter().collect()
            } else {
                info!(
                    "🔗 AutoRelay enabled, using {} bootstrap nodes as relay candidates",
                    bootstrap_set.len()
                );
                for (i, node) in bootstrap_set.iter().enumerate().take(5) {
                    info!("   Candidate {}: {}", i + 1, node);
                }
                bootstrap_set.iter().cloned().collect()
            }
        } else {
            HashSet::new()
        };

        // Use the new relay-aware transport builder
        let transport = build_transport_with_relay(&local_key, relay_transport, proxy_address)?;

        // Create the swarm
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_other_transport(|_| Ok(transport))
            .expect("Failed to create libp2p transport")
            .with_behaviour(move |_| behaviour.take().expect("behaviour already taken"))?
            .with_swarm_config(
                |c| c.with_idle_connection_timeout(Duration::from_secs(300)), // 5 minutes
            )
            .build();

        // Listen on the specified port
        let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
        swarm.listen_on(listen_addr)?;

        // ---- advertise external addresses so relay reservations include routable addrs
        let mut ext_addrs: Vec<Multiaddr> = Vec::new();

        // 1) If CHIRAL_PUBLIC_IP is set, use it as the advertised external address
        if let Ok(pub_ip) = std::env::var("CHIRAL_PUBLIC_IP") {
            if let Ok(ma) = format!("/ip4/{}/tcp/{}", pub_ip, port).parse() {
                ext_addrs.push(ma);
            } else {
                tracing::warn!("CHIRAL_PUBLIC_IP is set but invalid: {}", pub_ip);
            }
        }

        // Register external addresses with the swarm (pin with high score)
        for ma in ext_addrs {
            swarm.add_external_address(ma);
        }

        // Connect to bootstrap nodes
        // NOTE: Bootstrap nodes are explicitly configured, so we trust them
        // and don't filter based on reachability (important for relay servers and local testing)
        let mut successful_connections = 0;
        let total_bootstrap_nodes = bootstrap_nodes.len();
        for bootstrap_addr in &bootstrap_nodes {
            if let Ok(addr) = bootstrap_addr.parse::<Multiaddr>() {
                // WAN Mode: skip unroutable bootstrap addresses
                let wan_mode = enable_autonat || enable_autorelay;
                if !ma_plausibly_reachable(&addr) {
                    warn!("⏭️  Skipping unreachable bootstrap addr: {}", addr);
                    continue;
                }
                match swarm.dial(addr.clone()) {
                    Ok(_) => {
                        successful_connections += 1;
                        // Add bootstrap nodes to Kademlia routing table if it has a peer ID
                        if let Some(peer_id) = addr.iter().find_map(|p| {
                            if let libp2p::multiaddr::Protocol::P2p(peer) = p {
                                Some(peer)
                            } else {
                                None
                            }
                        }) {
                            swarm
                                .behaviour_mut()
                                .kademlia
                                .add_address(&peer_id, addr.clone());
                        }
                    }
                    Err(e) => warn!("✗ Failed to dial bootstrap {}: {}", bootstrap_addr, e),
                }
            } else {
                warn!("✗ Invalid bootstrap address format: {}", bootstrap_addr);
            }
        }

        if enable_autonat {
            for server_addr in &autonat_targets {
                if bootstrap_set.contains(server_addr) {
                    continue;
                }
                match server_addr.parse::<Multiaddr>() {
                    Ok(addr) => match swarm.dial(addr.clone()) {
                        Ok(_) => {
                            info!("Dialing AutoNAT server: {}", server_addr);
                        }
                        Err(e) => {
                            debug!("Failed to dial AutoNAT server {}: {}", server_addr, e);
                        }
                    },
                    Err(e) => warn!("Invalid AutoNAT server address {}: {}", server_addr, e),
                }
            }
        }

        // Trigger initial bootstrap if we have any bootstrap nodes (even if connection failed)
        if !bootstrap_nodes.is_empty() {
            let _ = swarm.behaviour_mut().kademlia.bootstrap();
            if successful_connections == 0 {
                warn!(
                    "⚠ No bootstrap connections succeeded - node will operate in standalone mode"
                );
                warn!("  Other nodes can still connect to this node directly");
            }
        } else {
            info!("No bootstrap nodes provided - starting in standalone mode");
        }

        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        let connected_peers = Arc::new(Mutex::new(HashSet::new()));
        let metrics = Arc::new(Mutex::new(DhtMetrics::default()));
        let pending_echo = Arc::new(Mutex::new(HashMap::new()));
        let pending_searches = Arc::new(Mutex::new(HashMap::new()));
        let search_counter = Arc::new(AtomicU64::new(1));
        let proxy_mgr: ProxyMgr = Arc::new(Mutex::new(ProxyManager::default()));
        let peer_selection = Arc::new(Mutex::new(PeerSelectionService::new()));
        let pending_webrtc_offers = Arc::new(Mutex::new(HashMap::new()));
        let pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let active_downloads: Arc<Mutex<HashMap<String, Arc<Mutex<ActiveDownload>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let get_providers_queries_local: Arc<
            Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>,
        > = Arc::new(Mutex::new(HashMap::new()));
        let seeder_heartbeats_cache: Arc<Mutex<HashMap<String, FileHeartbeatCacheEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let file_heartbeat_state: Arc<Mutex<HashMap<String, FileHeartbeatState>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_heartbeat_updates: Arc<Mutex<HashSet<String>>> =
            Arc::new(Mutex::new(HashSet::new()));
        let pending_keyword_indexes: Arc<Mutex<HashMap<kad::QueryId, PendingKeywordIndex>>> =
            Arc::new(Mutex::new(HashMap::new()));

        {
            let mut guard = metrics.lock().await;
            guard.autonat_enabled = enable_autonat;
            guard.autorelay_enabled = final_enable_autorelay;
            guard.dcutr_enabled = enable_autonat; // DCUtR enabled when AutoNAT is enabled
        }

        // Spawn the Dht node task
        let received_chunks_clone = Arc::new(Mutex::new(HashMap::new()));
        let bootstrap_peer_ids = extract_bootstrap_peer_ids(&bootstrap_nodes);

        tokio::spawn(run_dht_node(
            swarm,
            local_peer_id,
            cmd_rx,
            event_tx,
            connected_peers.clone(),
            metrics.clone(),
            pending_echo.clone(),
            pending_searches.clone(),
            proxy_mgr.clone(),
            peer_selection.clone(),
            received_chunks_clone.clone(),
            file_transfer_service.clone(),
            chunk_manager,
            pending_webrtc_offers.clone(),
            pending_provider_queries.clone(),
            root_query_mapping.clone(),
            active_downloads.clone(),
            get_providers_queries_local.clone(),
            seeder_heartbeats_cache.clone(),
            pending_heartbeat_updates.clone(),
            pending_keyword_indexes.clone(),
            is_bootstrap,
            final_enable_autorelay,
            relay_candidates,
            chunk_size,
            bootstrap_peer_ids,
        ));

        Ok(DhtService {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            peer_id: peer_id_str,
            connected_peers,
            connected_addrs: HashMap::new(),
            metrics,
            pending_echo,
            pending_searches,
            search_counter,
            proxy_mgr,
            peer_selection,
            file_metadata_cache: Arc::new(Mutex::new(HashMap::new())),
            received_chunks: received_chunks_clone,
            file_transfer_service,
            // chunk_manager is not stored in DhtService, only passed to the task
            pending_webrtc_offers,
            pending_provider_queries,
            root_query_mapping,
            active_downloads,
            get_providers_queries: get_providers_queries_local,
            chunk_size,
            file_heartbeat_state,
            seeder_heartbeats_cache,
            pending_heartbeat_updates,
        })
    }

    pub fn chunk_size(&self) -> usize {
        // Note: This might need to be adjusted if chunk_manager is the source of truth
        self.chunk_size
    }

    async fn start_file_heartbeat(&self, file_hash: &str) -> Result<(), String> {
        let file_hash_owned = file_hash.to_string();

        {
            let mut state = self.file_heartbeat_state.lock().await;
            if let Some(existing) = state.get(&file_hash_owned) {
                if !existing.task.is_finished() {
                    debug!("Heartbeat already active for {}", file_hash_owned);
                    return Ok(());
                }
                state.remove(&file_hash_owned);
            }
        }

        let cmd_tx = self.cmd_tx.clone();
        let hash_for_task = file_hash_owned.clone();

        let handle = tokio::spawn(async move {
            debug!("Starting heartbeat loop for {}", hash_for_task);

            if let Err(e) = cmd_tx
                .send(DhtCommand::HeartbeatFile {
                    file_hash: hash_for_task.clone(),
                })
                .await
            {
                warn!("Initial heartbeat send failed for {}: {}", hash_for_task, e);
                return;
            }

            let mut interval = tokio::time::interval(FILE_HEARTBEAT_INTERVAL);
            loop {
                interval.tick().await;
                match cmd_tx
                    .send(DhtCommand::HeartbeatFile {
                        file_hash: hash_for_task.clone(),
                    })
                    .await
                {
                    Ok(_) => {
                        trace!("Heartbeat refreshed for {}", hash_for_task);
                    }
                    Err(e) => {
                        warn!(
                            "Stopping heartbeat loop for {} due to send failure: {}",
                            hash_for_task, e
                        );
                        break;
                    }
                }
            }

            debug!("Heartbeat loop exited for {}", hash_for_task);
        });

        let mut state = self.file_heartbeat_state.lock().await;
        state.insert(file_hash_owned, FileHeartbeatState { task: handle });
        Ok(())
    }

    async fn stop_file_heartbeat(&self, file_hash: &str) {
        let handle = {
            let mut state = self.file_heartbeat_state.lock().await;
            state.remove(file_hash).map(|entry| entry.task)
        };

        if let Some(handle) = handle {
            if !handle.is_finished() {
                handle.abort();
            }
        }

        self.seeder_heartbeats_cache.lock().await.remove(file_hash);
        self.pending_heartbeat_updates
            .lock()
            .await
            .remove(file_hash);
        debug!("Heartbeat tracking stopped for {}", file_hash);
    }

    pub async fn publish_file(&self, metadata: FileMetadata) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();

        self.cmd_tx
            .send(DhtCommand::PublishFile {
                metadata,
                response_tx,
            })
            .await
            .map_err(|e| e.to_string())?;

        let cid_populated_metadata = response_rx.await.map_err(|e| e.to_string())?;

        self.cache_remote_file(&cid_populated_metadata).await;
        self.start_file_heartbeat(&cid_populated_metadata.merkle_root)
            .await?;
        Ok(())
    }

    pub async fn stop_publishing_file(&self, file_hash: String) -> Result<(), String> {
        let file_hash_clone = file_hash.clone();

        self.cmd_tx
            .send(DhtCommand::StopPublish(file_hash))
            .await
            .map_err(|e| e.to_string())?;

        self.stop_file_heartbeat(&file_hash_clone).await;
        Ok(())
    }
    pub async fn cache_remote_file(&self, metadata: &FileMetadata) {
        self.file_metadata_cache
            .lock()
            .await
            .insert(metadata.merkle_root.clone(), metadata.clone());
    }
    /// List all known FileMetadata (from cache, i.e., locally published or discovered)
    pub async fn get_all_file_metadata(&self) -> Result<Vec<FileMetadata>, String> {
        let cache = self.file_metadata_cache.lock().await;
        Ok(cache.values().cloned().collect())
    }

    /// Get all versions for a file name, sorted by version (desc)
    /// Matching is performed case-insensitively so uploads that differ only by
    /// filename case are treated as versions of the same file name.
    pub async fn get_versions_by_file_name(
        &self,
        file_name: String,
    ) -> Result<Vec<FileMetadata>, String> {
        info!(
            "🔍 Backend: Starting search for file versions with name: {}",
            file_name
        );

        let all = self.get_all_file_metadata().await?;
        info!("📁 Backend: Retrieved {} total files from cache", all.len());

        // Perform case-insensitive match on file name to group versions regardless of case
        let target = file_name.to_lowercase();

        let mut versions: Vec<FileMetadata> = all
            .into_iter()
            .filter(|m| m.file_name.to_lowercase() == target) // case-insensitive match - get all versions
            .collect();

        info!(
            "🎯 Backend: Found {} versions matching name '{}'",
            versions.len(),
            file_name
        );

        versions.sort_by(|a, b| b.version.unwrap_or(1).cmp(&a.version.unwrap_or(1)));

        // Clear seeders to avoid network calls during search
        // The seeders will be populated when the user actually tries to download
        for version in &mut versions {
            version.seeders = vec![]; // Clear seeders to prevent network calls
        }

        info!(
            "✅ Backend: Returning {} versions for '{}' (seeders cleared)",
            versions.len(),
            file_name
        );
        Ok(versions)
    }

    /// Get the latest version for a file name
    pub async fn get_latest_version_by_file_name(
        &self,
        file_name: String,
    ) -> Result<Option<FileMetadata>, String> {
        let versions = self.get_versions_by_file_name(file_name).await?;
        Ok(versions.into_iter().max_by_key(|m| m.version.unwrap_or(1)))
    }

    /// Prepare a new FileMetadata for upload (auto-increment version, set parent_hash)
    pub async fn prepare_versioned_metadata(
        &self,
        file_hash: String,
        file_name: String,
        file_size: u64,
        file_data: Vec<u8>,
        created_at: u64,
        mime_type: Option<String>,
        encrypted_key_bundle: Option<crate::encryption::EncryptedAesKeyBundle>,
        is_encrypted: bool,
        encryption_method: Option<String>,
        key_fingerprint: Option<String>,
        price: Option<f64>,
        uploader_address: Option<String>,
    ) -> Result<FileMetadata, String> {
        let latest = self
            .get_latest_version_by_file_name(file_name.clone())
            .await?;

        // When a latest version for the same filename exists, only increment the version
        // if the merkle root (content identifier) is different. If the merkle root is
        // identical we return the same version to avoid creating duplicate versions
        // for the same content.
        let (version, parent_hash, is_root) = match latest {
            Some(ref prev) => {
                if prev.merkle_root == file_hash {
                    // Same content: keep same version and parent/is_root values
                    (
                        prev.version.unwrap_or(1),
                        prev.parent_hash.clone(),
                        prev.is_root,
                    )
                } else {
                    // Different content: increment version and set parent to previous merkle root
                    (
                        prev.version.map(|v| v + 1).unwrap_or(2),
                        Some(prev.merkle_root.clone()),
                        false, // not root if there was a previous version
                    )
                }
            }
            None => (1, None, true), // root if first version
        };
        Ok(FileMetadata {
            merkle_root: file_hash,
            file_name,
            file_size,
            file_data,
            seeders: vec![],
            created_at,
            mime_type,
            is_encrypted,
            encryption_method,
            key_fingerprint,
            version: Some(version),
            encrypted_key_bundle: None,
            parent_hash,
            cids: None,
            is_root,
            download_path: None,
            price,
            uploader_address,
        })
    }

    pub async fn download_file(
        &self,
        file_metadata: FileMetadata,
        download_path: String,
    ) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::DownloadFile(file_metadata, download_path))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn publish_encrypted_file(
        &self,
        metadata: FileMetadata,
        blocks: Vec<(Cid, Vec<u8>)>,
    ) -> Result<(), String> {
        let file_hash = metadata.merkle_root.clone();
        // The root CID is the CID of the list of block CIDs.
        // This needs to be computed before calling the command.
        let block_cids: Vec<Cid> = blocks.iter().map(|(cid, _)| cid.clone()).collect();
        let root_block_data = serde_json::to_vec(&block_cids).map_err(|e| e.to_string())?;
        let root_cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(&root_block_data));

        self.cmd_tx
            .send(DhtCommand::StoreBlocks {
                blocks,
                root_cid,
                metadata,
            })
            .await
            .map_err(|e| e.to_string())?;

        self.start_file_heartbeat(&file_hash).await?;
        Ok(())
    }

    pub async fn search_file(&self, file_hash: String) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::SearchFile(file_hash))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_file(&self, file_hash: String) -> Result<(), String> {
        self.search_file(file_hash).await
    }

    pub async fn search_metadata(&self, file_hash: String, timeout_ms: u64) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::SearchFile(file_hash.clone()))
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn synchronous_search_metadata(
        &self,
        file_hash: String,
        timeout_ms: u64,
    ) -> Result<Option<FileMetadata>, String> {
        if timeout_ms == 0 {
            self.cmd_tx
                .send(DhtCommand::SearchFile(file_hash))
                .await
                .map_err(|e| e.to_string())?;
            return Ok(None);
        }

        let timeout_duration = Duration::from_millis(timeout_ms);
        let waiter_id = self.search_counter.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending_searches.lock().await;
            pending
                .entry(file_hash.clone())
                .or_default()
                .push(PendingSearch {
                    id: waiter_id,
                    sender: tx,
                });
        }

        if let Err(err) = self
            .cmd_tx
            .send(DhtCommand::SearchFile(file_hash.clone()))
            .await
        {
            let mut pending = self.pending_searches.lock().await;
            if let Some(waiters) = pending.get_mut(&file_hash) {
                waiters.retain(|w| w.id != waiter_id);
                if waiters.is_empty() {
                    pending.remove(&file_hash);
                }
            }
            return Err(err.to_string());
        }

        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(SearchResponse::Found(metadata))) => Ok(Some(metadata)),
            Ok(Ok(SearchResponse::NotFound)) => Ok(None),
            Ok(Err(_)) => Err("Search channel closed".into()),
            Err(_) => {
                let mut pending = self.pending_searches.lock().await;
                if let Some(waiters) = pending.get_mut(&file_hash) {
                    waiters.retain(|w| w.id != waiter_id);
                    if waiters.is_empty() {
                        pending.remove(&file_hash);
                    }
                }
                Err("Search timed out".into())
            }
        }
    }

    pub async fn connect_peer(&self, addr: String) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::ConnectPeer(addr))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn connect_to_peer_by_id(&self, peer_id: String) -> Result<(), String> {
        let peer_id: PeerId = peer_id
            .parse()
            .map_err(|e| format!("Invalid peer ID: {}", e))?;
        self.cmd_tx
            .send(DhtCommand::ConnectToPeerById(peer_id))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn disconnect_peer(&self, peer_id: PeerId) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::DisconnectPeer(peer_id))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_peer_id(&self) -> String {
        self.peer_id.clone()
    }

    /// Get multiaddresses for this node (including the peer ID)
    pub async fn get_multiaddresses(&self) -> Vec<String> {
        let metrics = self.metrics.lock().await;
        let peer_id = &self.peer_id;

        metrics.listen_addrs
            .iter()
            .filter(|addr| {
                // Filter out loopback addresses
                !addr.contains("127.0.0.1") && !addr.contains("::1")
            })
            .map(|addr| {
                // Add peer ID if not already present
                if addr.contains("/p2p/") {
                    addr.clone()
                } else {
                    format!("{}/p2p/{}", addr, peer_id)
                }
            })
            .collect()
    }

    pub async fn get_peer_count(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        if self.cmd_tx.send(DhtCommand::GetPeerCount(tx)).await.is_ok() {
            rx.await.unwrap_or(0)
        } else {
            0
        }
    }

    pub async fn get_connected_peers(&self) -> Vec<String> {
        let connected_peers = self.connected_peers.lock().await;
        connected_peers
            .iter()
            .map(|peer_id| peer_id.to_string())
            .collect()
    }

    pub async fn echo(&self, peer_id: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let target_peer_id: PeerId = peer_id
            .parse()
            .map_err(|e| format!("Invalid peer ID: {e}"))?;

        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::Echo {
                peer: target_peer_id,
                payload,
                tx,
            })
            .await
            .map_err(|e| format!("Failed to send echo command: {e}"))?;

        rx.await
            .map_err(|e| format!("Echo response error: {}", e))?
    }

    pub async fn send_message_to_peer(
        &self,
        peer_id: &str,
        message: serde_json::Value,
    ) -> Result<(), String> {
        let target_peer_id: PeerId = peer_id
            .parse()
            .map_err(|e| format!("Invalid peer ID: {}", e))?;

        // Send message through DHT command system
        self.cmd_tx
            .send(DhtCommand::SendMessageToPeer {
                target_peer_id,
                message,
            })
            .await
            .map_err(|e| format!("Failed to send DHT command: {e}"))?;

        Ok(())
    }

    pub async fn update_privacy_proxy_targets(&self, addresses: Vec<String>) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::SetPrivacyProxies { addresses })
            .await
            .map_err(|e| format!("Failed to update privacy proxies: {e}"))
    }

    /// Verifies that a peer actually provides working proxy services through protocol negotiation
    async fn verify_proxy_capabilities(&self, peer_id: &PeerId) -> Result<(), String> {
        // Use the existing echo protocol to verify proxy capabilities
        // Send a special "proxy_verify" message that the peer should echo back if it's a working proxy

        let verification_payload = b"proxy_verify";

        // Send echo request with verification payload through DHT service
        match self
            .echo(peer_id.to_string(), verification_payload.to_vec())
            .await
        {
            Ok(response) => {
                // Check if the response matches our verification payload
                if response == verification_payload {
                    info!(
                        "✅ Proxy capability verified for peer {} via echo test",
                        peer_id
                    );
                    Ok(())
                } else {
                    Err(format!(
                        "Proxy verification failed: unexpected response from peer {}",
                        peer_id
                    ))
                }
            }
            Err(e) => Err(format!(
                "Proxy verification failed: echo request failed for peer {}: {}",
                peer_id, e
            )),
        }
    }

    /// Discovers proxy services through DHT provider queries
    /// Uses DHT provider discovery to find peers advertising proxy services
    async fn discover_proxy_services_through_dht_providers(
        &self,
        proxy_mgr: &mut ProxyManager,
    ) -> usize {
        let mut discovered_and_verified = 0;

        info!("Starting DHT proxy service discovery using provider queries...");

        // Query DHT for peers that provide proxy services
        // Use a standard proxy service identifier that proxy nodes would register as providers for
        let proxy_service_cid = "proxy:service:available"; // This would be a well-known CID for proxy services

        match self
            .query_dht_proxy_providers(proxy_service_cid.to_string())
            .await
        {
            Ok(provider_peers) => {
                for peer_id in provider_peers {
                    if !proxy_mgr.capable.contains(&peer_id) {
                        info!("Discovered proxy provider via DHT: {}", peer_id);

                        // Add to capable list for verification
                        proxy_mgr.set_capable(peer_id.clone());

                        // Verify the discovered proxy
                        match self.verify_proxy_capabilities(&peer_id).await {
                            Ok(_) => {
                                proxy_mgr.add_trusted_proxy_node(peer_id.clone());
                                discovered_and_verified += 1;
                                info!(
                                    "✅ Verified and added DHT-discovered proxy provider: {}",
                                    peer_id
                                );
                            }
                            Err(e) => {
                                warn!(
                                    "❌ DHT proxy provider verification failed for {}: {}",
                                    peer_id, e
                                );
                                proxy_mgr.capable.remove(&peer_id);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("DHT proxy provider discovery failed: {}", e);
            }
        }

        info!(
            "DHT proxy provider discovery completed: {} proxies verified and added",
            discovered_and_verified
        );
        discovered_and_verified
    }

    /// Query DHT for peers providing proxy services using provider records
    /// Returns a list of peer IDs that provide proxy services
    async fn query_dht_proxy_providers(
        &self,
        service_identifier: String,
    ) -> Result<Vec<PeerId>, String> {
        // Create a DHT record key for proxy services
        let key = kad::RecordKey::new(&service_identifier);

        // Query DHT for providers of this service
        // This finds peers that have registered as providers for proxy services
        let (tx, rx) = oneshot::channel();

        // Send command to query providers
        if let Err(e) = self
            .cmd_tx
            .send(DhtCommand::GetProviders {
                file_hash: service_identifier.clone(),
                sender: tx,
            })
            .await
        {
            return Err(format!("Failed to send GetProviders command: {}", e));
        }

        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(15), rx).await {
            Ok(Ok(Ok(provider_strings))) => {
                let total_count = provider_strings.len();

                // Convert string peer IDs to PeerId objects
                let mut peer_ids = Vec::new();
                for peer_string in provider_strings {
                    match peer_string.parse::<PeerId>() {
                        Ok(peer_id) => peer_ids.push(peer_id),
                        Err(e) => {
                            warn!("Failed to parse peer ID from provider string: {}", e);
                        }
                    }
                }

                info!(
                    "Found {} proxy service providers in DHT ({} valid peer IDs)",
                    total_count,
                    peer_ids.len()
                );
                Ok(peer_ids)
            }
            Ok(Ok(Err(e))) => Err(format!("GetProviders command failed: {}", e)),
            Ok(Err(_)) => Err("GetProviders channel closed unexpectedly".to_string()),
            Err(_) => Err("GetProviders query timed out".to_string()),
        }
    }

    pub async fn metrics_snapshot(&self) -> DhtMetricsSnapshot {
        let metrics = self.metrics.lock().await.clone();
        let peer_count = self.connected_peers.lock().await.len();
        DhtMetricsSnapshot::from(metrics, peer_count)
    }

    pub async fn store_block(&self, cid: Cid, data: Vec<u8>) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::StoreBlock { cid, data })
            .await
            .map_err(|e| e.to_string())
    }

    // Drain up to `max` pending events without blocking
    pub async fn drain_events(&self, max: usize) -> Vec<DhtEvent> {
        use tokio::sync::mpsc::error::TryRecvError;
        let mut rx = self.event_rx.lock().await;
        let mut events = Vec::new();
        while events.len() < max {
            match rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        events
    }

    /// Get recommended peers for file download using smart selection
    pub async fn get_recommended_peers_for_download(
        &self,
        file_hash: &str,
        file_size: u64,
        require_encryption: bool,
    ) -> Vec<String> {
        // First get peers that have the file
        let available_peers = self.get_seeders_for_file(file_hash).await;

        if available_peers.is_empty() {
            return Vec::new();
        }

        // Use smart peer selection
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.recommend_peers_for_file(&available_peers, file_size, require_encryption)
    }

    /// Record successful transfer for peer metrics
    pub async fn record_transfer_success(&self, peer_id: &str, bytes: u64, duration_ms: u64) {
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.record_transfer_success(peer_id, bytes, duration_ms);
    }

    /// Record failed transfer for peer metrics
    pub async fn record_transfer_failure(&self, peer_id: &str, error: &str) {
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.record_transfer_failure(peer_id, error);
    }

    /// Update peer encryption support
    pub async fn set_peer_encryption_support(&self, peer_id: &str, supported: bool) {
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.set_peer_encryption_support(peer_id, supported);
    }

    /// Report malicious behavior from a peer
    pub async fn report_malicious_peer(&self, peer_id: &str, severity: &str) {
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.report_malicious_peer(peer_id, severity);
    }

    /// Get all peer metrics for monitoring
    pub async fn get_peer_metrics(&self) -> Vec<PeerMetrics> {
        let peer_selection = self.peer_selection.lock().await;
        peer_selection.get_all_metrics()
    }

    /// Select best peers using a specific strategy
    pub async fn select_peers_with_strategy(
        &self,
        available_peers: &[String],
        count: usize,
        strategy: SelectionStrategy,
        require_encryption: bool,
    ) -> Vec<String> {
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.select_peers(available_peers, count, strategy, require_encryption)
    }

    /// Clean up inactive peer metrics
    pub async fn cleanup_inactive_peers(&self, max_age_seconds: u64) {
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.cleanup_inactive_peers(max_age_seconds);
    }

    /// Discover and verify available peers for a specific file
    pub async fn discover_peers_for_file(
        &self,
        metadata: &FileMetadata, // This now contains the merkle_root
    ) -> Result<Vec<String>, String> {
        info!(
            "Starting peer discovery for file: {} with {} seeders",
            metadata.merkle_root,
            metadata.seeders.len()
        );

        let mut available_peers = Vec::new();
        let connected_peers = self.connected_peers.lock().await;

        // Check which seeders from metadata are currently connected
        for seeder_id in &metadata.seeders {
            if let Ok(peer_id) = seeder_id.parse::<libp2p::PeerId>() {
                if connected_peers.contains(&peer_id) {
                    info!("Seeder {} is currently connected", seeder_id);
                    available_peers.push(seeder_id.clone());
                } else {
                    info!("Seeder {} is not currently connected", seeder_id);
                    // Try to connect to this peer by sending a ConnectToPeerById command
                    // This will query the DHT for the peer's addresses and attempt connection
                    if let Err(e) = self
                        .cmd_tx
                        .send(DhtCommand::ConnectToPeerById(peer_id))
                        .await
                    {
                        warn!(
                            "Failed to send ConnectToPeerById command for {}: {}",
                            seeder_id, e
                        );
                    } else {
                        info!("Attempting to connect to seeder {}", seeder_id);
                    }
                }
            } else {
                warn!("Invalid peer ID in seeders list: {}", seeder_id);
            }
        }

        // If no seeders are connected, the file is not available for download
        if available_peers.is_empty() {
            info!("No seeders are currently connected - file not available for download");
        }

        info!(
            "Peer discovery completed: found {} available peers",
            available_peers.len()
        );
        Ok(available_peers)
    }

    /// Get seeders for a specific file (searches DHT for providers)
    pub async fn get_seeders_for_file(&self, file_hash: &str) -> Vec<String> {
        // Fast path: consult local heartbeat cache and prune expired entries
        let now = unix_timestamp();
        if let Some(entry) = self.seeder_heartbeats_cache.lock().await.get_mut(file_hash) {
            entry.heartbeats = prune_heartbeats(entry.heartbeats.clone(), now);
            entry.metadata["seeders"] = serde_json::Value::Array(
                heartbeats_to_peer_list(&entry.heartbeats)
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            );
            entry.metadata["seederHeartbeats"] = serde_json::to_value(&entry.heartbeats)
                .unwrap_or_else(|_| serde_json::Value::Array(vec![]));

            let peers = heartbeats_to_peer_list(&entry.heartbeats);
            if !peers.is_empty() {
                // return the pruned local view immediately to keep UI responsive/fresh
                return peers;
            }
            // otherwise fall back to querying the DHT providers
        }

        // Send command to DHT task to query provider records for this file
        let (tx, rx) = oneshot::channel();

        if let Err(e) = self
            .cmd_tx
            .send(DhtCommand::GetProviders {
                file_hash: file_hash.to_string(),
                sender: tx,
            })
            .await
        {
            warn!("Failed to send GetProviders command: {}", e);
            return Vec::new();
        }

        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(Ok(providers))) => {
                info!(
                    "Found {} providers for file: {}",
                    providers.len(),
                    file_hash
                );
                // Optionally filter unreachable providers here (try connect/ping) before returning.
                providers
            }
            Ok(Ok(Err(e))) => {
                warn!("GetProviders command failed: {}", e);
                // Fallback to connected peers
                let connected = self.connected_peers.lock().await;
                connected.iter().take(3).map(|p| p.to_string()).collect()
            }
            Ok(Err(e)) => {
                warn!("Receiver error: {}", e);
                // Fallback to connected peers
                let connected = self.connected_peers.lock().await;
                connected.iter().take(3).map(|p| p.to_string()).collect()
            }
            Err(_) => {
                warn!("GetProviders command timed out for file: {}", file_hash);
                // Fallback to connected peers
                let connected = self.connected_peers.lock().await;
                connected.iter().take(3).map(|p| p.to_string()).collect()
            }
        }
    }

    /// Shutdown the Dht service
    pub async fn shutdown(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::Shutdown(tx))
            .await
            .map_err(|e| format!("Failed to send shutdown command: {}", e))?;
        rx.await
            .map_err(|e| format!("Failed to receive shutdown acknowledgment: {}", e))
    }

    /// Enable privacy routing through proxy nodes
    pub async fn enable_privacy_routing(&self, mode: PrivacyMode) -> Result<(), String> {
        let mut proxy_mgr = self.proxy_mgr.lock().await;

        // Enable privacy routing in the proxy manager
        proxy_mgr.enable_privacy_routing(mode);

        // Identify and mark trusted proxy nodes from connected peers
        // Query connected peers for proxy capabilities and establish trust relationships
        let connected_peers_list = {
            let connected = self.connected_peers.lock().await;
            connected.iter().cloned().collect::<Vec<_>>()
        };

        let mut trusted_proxy_count = 0;
        for peer_id in connected_peers_list {
            // Check if this peer is capable of proxy services
            if proxy_mgr.capable.contains(&peer_id) && proxy_mgr.online.contains(&peer_id) {
                // Verify proxy capabilities through protocol negotiation
                match self.verify_proxy_capabilities(&peer_id).await {
                    Ok(_) => {
                        proxy_mgr.add_trusted_proxy_node(peer_id.clone());
                        trusted_proxy_count += 1;
                        info!("✅ Added connected peer {} as trusted proxy node (capability verified)", peer_id);
                    }
                    Err(e) => {
                        warn!(
                            "❌ Proxy capability verification failed for peer {}: {}",
                            peer_id, e
                        );
                        // Remove from capable list if verification fails
                        proxy_mgr.capable.remove(&peer_id);
                    }
                }
            }
        }

        // Query DHT for peers advertising proxy services and add them to verification pipeline
        let dht_proxy_count = self
            .discover_proxy_services_through_dht_providers(&mut proxy_mgr)
            .await;

        let trusted_count = trusted_proxy_count + dht_proxy_count;
        info!(
            "Privacy routing enabled with {} trusted proxy nodes (mode: {:?})",
            trusted_count, mode
        );

        // Send event to notify about privacy routing status
        let _ = self.cmd_tx.send(DhtCommand::SendMessageToPeer {
            target_peer_id: self
                .peer_id
                .parse()
                .map_err(|e| format!("Invalid peer ID: {}", e))?,
            message: serde_json::json!({
                "type": "privacy_routing_enabled",
                "trusted_proxies": trusted_count
            }),
        });

        Ok(())
    }

    /// Disable privacy routing, revert to direct connections
    pub async fn disable_privacy_routing(&self) -> Result<(), String> {
        let mut proxy_mgr = self.proxy_mgr.lock().await;

        // Disable privacy routing in the proxy manager
        proxy_mgr.disable_privacy_routing();

        // Clear trusted proxy nodes
        proxy_mgr.trusted_proxy_nodes.clear();
        proxy_mgr.manual_trusted.clear();

        info!("Privacy routing disabled - reverting to direct connections");

        // Send event to notify about privacy routing status
        let _ = self.cmd_tx.send(DhtCommand::SendMessageToPeer {
            target_peer_id: self
                .peer_id
                .parse()
                .map_err(|e| format!("Invalid peer ID: {}", e))?,
            message: serde_json::json!({
                "type": "privacy_routing_disabled"
            }),
        });

        Ok(())
    }

    /// Generates a proof for a given file chunk and submits it to the blockchain.
    /// This function is called by the blockchain listener upon receiving a challenge.
    pub async fn generate_and_submit_proof(
        &self,
        file_root_hex: String,
        chunk_index: u64,
    ) -> Result<(), String> {
        info!(
            "Generating proof for file root {} and chunk index {}",
            file_root_hex, chunk_index
        );

        // 1. Locate the file manifest from the local cache.
        let manifest = self
            .get_manifest_from_cache(&file_root_hex)
            .await
            .ok_or_else(|| format!("File manifest not found for root: {}", file_root_hex))?;

        // 2. Locate the requested file chunk data.
        let chunk_data = self
            .get_chunk_data(&file_root_hex, chunk_index as usize)
            .await
            .map_err(|e| format!("Failed to locate chunk: {}", e))?;

        // 3. Generate Merkle proof for that chunk.
        let proof = self
            .get_merkle_proof(&manifest, chunk_index as usize)
            .await
            .map_err(|e| format!("Failed to generate Merkle proof: {}", e))?;

        // 4. Submit proof to the smart contract.
        self.submit_to_contract(&file_root_hex, proof, chunk_data, chunk_index)
            .await
            .map_err(|e| format!("Failed to submit proof to contract: {}", e))?;

        Ok(())
    }

    /// Retrieves a file's manifest from the local cache.
    async fn get_manifest_from_cache(&self, file_root_hex: &str) -> Option<FileMetadata> {
        let cache = self.file_metadata_cache.lock().await;
        cache.get(file_root_hex).cloned()
    }

    /// Retrieves the original, unencrypted chunk data from local storage.
    /// This assumes the `FileTransferService` provides access to the underlying storage.
    async fn get_chunk_data(
        &self,
        file_root_hex: &str,
        chunk_index: usize,
    ) -> Result<Vec<u8>, String> {
        if let Some(ft_service) = &self.file_transfer_service {
            let file_data = ft_service
                .get_file_data(file_root_hex)
                .await
                .ok_or_else(|| format!("File data not found for root {}", file_root_hex))?;

            let chunk_size = self.chunk_size();
            let start = chunk_index * chunk_size;
            let end = (start + chunk_size).min(file_data.len());

            if start >= file_data.len() {
                return Err(format!("Chunk index {} is out of bounds", chunk_index));
            }

            Ok(file_data[start..end].to_vec())
        } else {
            Err("FileTransferService is not available".to_string())
        }
    }

    /// Generates a Merkle proof for a specific chunk index.
    async fn get_merkle_proof(
        &self,
        manifest: &FileMetadata,
        chunk_index: usize,
    ) -> Result<Vec<[u8; 32]>, String> {
        // This requires re-calculating the original chunk hashes to build the tree.
        // A more optimized version would store the hashes in the manifest.
        if let Some(ft_service) = &self.file_transfer_service {
            let file_data = ft_service
                .get_file_data(&manifest.merkle_root)
                .await
                .ok_or_else(|| format!("File data not found for root {}", manifest.merkle_root))?;

            let chunk_size = self.chunk_size();
            let original_chunk_hashes: Vec<[u8; 32]> = file_data
                .chunks(chunk_size)
                .map(Sha256Hasher::hash)
                .collect();

            if chunk_index >= original_chunk_hashes.len() {
                return Err(format!(
                    "Chunk index {} out of bounds for proof generation",
                    chunk_index
                ));
            }

            let tree = MerkleTree::<Sha256Hasher>::from_leaves(&original_chunk_hashes);
            let proof = tree.proof(&[chunk_index]);
            Ok(proof.proof_hashes().to_vec())
        } else {
            Err("FileTransferService is not available".to_string())
        }
    }

    /// Placeholder for submitting the proof to the smart contract.
    async fn submit_to_contract(
        &self,
        file_root: &str,
        proof: Vec<[u8; 32]>,
        chunk_data: Vec<u8>,
        chunk_index: u64,
    ) -> Result<(), String> {
        info!(
            "Submitting proof for file root {} to smart contract...",
            file_root
        );

        // This is a simplified example. In a real app, you would get the provider,
        // contract address, and signer from the AppState or configuration.
        let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
            .map_err(|e| format!("Failed to create provider: {}", e))?;
        let client = Arc::new(provider);

        // This private key is for demonstration. In a real app, you would retrieve
        // this securely from the AppState's keystore/active_account_private_key.
        let wallet: LocalWallet =
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .parse()
                .map_err(|e| format!("Failed to parse private key: {}", e))?;
        let signer = SignerMiddleware::new(client.clone(), wallet.with_chain_id(98765u64));

        // The contract address needs to be known. This would come from AppState.
        let contract_address: Address = "0x5FbDB2315678afecb367f032d93F642f64180aa3"
            .parse()
            .map_err(|e| format!("Failed to parse contract address: {}", e))?;

        // Define the contract ABI for the `verifyProof` function.
        // In a larger project, you would use `abigen!` to generate this from the contract JSON.
        abigen!(
            ProofOfStorage,
            r#"[
                function verifyProof(bytes32 fileRoot, bytes32[] calldata proof, bytes calldata chunkData, uint256 chunkIndex) external view returns (bool)
            ]"#,
        );

        let contract = ProofOfStorage::new(contract_address, Arc::new(signer));

        // Prepare arguments for the contract call
        let root_bytes: [u8; 32] = hex::decode(file_root)
            .map_err(|e| format!("Invalid file root hex: {}", e))?
            .try_into()
            .map_err(|_| "File root is not 32 bytes".to_string())?;

        // Call the contract's `verifyProof` method.
        // Note: `verifyProof` is a `view` function, so we use `.call()` which doesn't create a transaction.
        // If it were a state-changing function, we would use `.send()`.
        let is_valid = contract
            .verify_proof(root_bytes, proof, chunk_data.into(), chunk_index.into())
            .call()
            .await
            .map_err(|e| format!("Contract call failed: {}", e))?;

        info!("Proof verification result from contract: {}", is_valid);
        if !is_valid {
            return Err("Proof was rejected by the smart contract.".to_string());
        }

        Ok(())
    }
}

/// Process received Bitswap chunk data and assemble complete files
async fn process_bitswap_chunk(
    query_id: &beetswap::QueryId,
    data: &[u8],
    event_tx: &mpsc::Sender<DhtEvent>,
    received_chunks: &Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: &Arc<FileTransferService>,
) {
    // Try to parse the data as a FileChunk
    match serde_json::from_slice::<FileChunk>(data) {
        Ok(chunk) => {
            info!(
                "Received chunk {}/{} for file {} ({} bytes)",
                chunk.chunk_index + 1,
                chunk.total_chunks,
                chunk.file_hash,
                chunk.data.len()
            );

            // Store the chunk
            {
                let mut chunks_map = received_chunks.lock().await;
                let file_chunks = chunks_map
                    .entry(chunk.file_hash.clone())
                    .or_insert_with(HashMap::new);
                file_chunks.insert(chunk.chunk_index, chunk.clone());
            }

            // Check if we have all chunks for this file
            let has_all_chunks = {
                let chunks_map = received_chunks.lock().await;
                if let Some(file_chunks) = chunks_map.get(&chunk.file_hash) {
                    file_chunks.len() == chunk.total_chunks as usize
                } else {
                    false
                }
            };

            if has_all_chunks {
                // Assemble the file from all chunks
                assemble_file_from_chunks(
                    &chunk.file_hash,
                    received_chunks,
                    file_transfer_service,
                    event_tx,
                )
                .await;
            }

            let _ = event_tx
                .send(DhtEvent::BitswapDataReceived {
                    query_id: format!("{:?}", query_id),
                    data: data.to_vec(),
                })
                .await;
        }
        Err(e) => {
            warn!("Failed to parse Bitswap data as FileChunk: {}", e);
            // Emit raw data event for debugging
            let _ = event_tx
                .send(DhtEvent::BitswapDataReceived {
                    query_id: format!("{:?}", query_id),
                    data: data.to_vec(),
                })
                .await;
        }
    }
}

/// Assemble a complete file from received chunks
async fn assemble_file_from_chunks(
    file_hash: &str,
    received_chunks: &Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: &Arc<FileTransferService>,
    event_tx: &mpsc::Sender<DhtEvent>,
) {
    // Get all chunks for this file
    let chunks = {
        let mut chunks_map = received_chunks.lock().await;
        chunks_map.remove(file_hash)
    };

    if let Some(mut file_chunks) = chunks {
        // Sort chunks by index
        let mut sorted_chunks: Vec<FileChunk> =
            file_chunks.drain().map(|(_, chunk)| chunk).collect();
        sorted_chunks.sort_by_key(|c| c.chunk_index);

        // Get the count before consuming the vector
        let chunk_count = sorted_chunks.len();

        // Concatenate chunk data
        let mut file_data = Vec::new();
        for chunk in sorted_chunks {
            file_data.extend_from_slice(&chunk.data);
        }

        // Store the assembled file
        let file_name = format!("downloaded_{}", file_hash);
        file_transfer_service
            .store_file_data(file_hash.to_string(), file_name, file_data)
            .await;

        info!(
            "Successfully assembled file {} from {} chunks",
            file_hash, chunk_count
        );

        let _ = event_tx
            .send(DhtEvent::FileDownloaded {
                file_hash: file_hash.to_string(),
            })
            .await;
    }
}

fn not_loopback(ip: &Multiaddr) -> bool {
    multiaddr_to_ip(ip)
        .map(|ip| !ip.is_loopback())
        .unwrap_or(false)
}

fn multiaddr_to_ip(addr: &Multiaddr) -> Option<IpAddr> {
    for comp in addr.iter() {
        match comp {
            Protocol::Ip4(ipv4) => return Some(IpAddr::V4(ipv4)),
            Protocol::Ip6(ipv6) => return Some(IpAddr::V6(ipv6)),
            _ => {}
        }
    }
    None
}

fn ipv4_in_same_subnet(target: Ipv4Addr, iface_ip: Ipv4Addr, iface_mask: Ipv4Addr) -> bool {
    let t = u32::from(target);
    let i = u32::from(iface_ip);
    let m = u32::from(iface_mask);
    (t & m) == (i & m)
}

/// If multiaddr can be plausibly reached from this machine
/// - Relay paths (p2p-circuit) are allowed
/// - IPv4 loopback (127.0.0.1) is allowed (local testing)
/// - For WAN intent, only public IPv4 addresses are allowed (not private ranges)
fn ma_plausibly_reachable(ma: &Multiaddr) -> bool {
    // Relay paths are allowed
    if ma.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
        return true;
    }
    // Only consider IPv4 (IPv6 can be added if needed)
    if let Some(Protocol::Ip4(v4)) = ma.iter().find(|p| matches!(p, Protocol::Ip4(_))) {
        // Allow loopback for local testing
        if v4.is_loopback() {
            return true;
        }
        // Allow public addresses, reject private
        return !v4.is_private();
    }
    false
}

/// Parsing multiaddr from error string is heuristic and may not be reliable
fn extract_multiaddr_from_error_str(s: &str) -> Option<Multiaddr> {
    // Example: "Failed to negotiate ... [(/ip4/172.17.0.3/tcp/4001/p2p/12D...: : Timeout ...)]"
    // Try to find the first occurrence of "/ip" and extract until a delimiter
    if let Some(start) = s.find("/ip") {
        // Roughly cut the delimiter to ) ] space, etc.
        let tail = &s[start..];
        let end = tail
            .find(|c: char| c == ')' || c == ']' || c == ' ')
            .unwrap_or_else(|| tail.len());
        let cand = &tail[..end];
        return cand.parse::<Multiaddr>().ok();
    }
    None
}

/// Check if an IPv4 address is private or loopback
fn is_private_or_loopback_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || o[0] == 127
}

async fn record_identify_push_metrics(metrics: &Arc<Mutex<DhtMetrics>>, info: &identify::Info) {
    if let Ok(mut metrics_guard) = metrics.try_lock() {
        for addr in &info.listen_addrs {
            metrics_guard.record_listen_addr(addr);
        }
    }
}

pub struct StringBlock(pub String);
pub struct ByteBlock(pub Vec<u8>);

impl Block<64> for ByteBlock {
    fn cid(&self) -> Result<Cid, CidError> {
        let hash = Code::Sha2_256.digest(&self.0);
        Ok(Cid::new_v1(RAW_CODEC, hash))
    }

    fn data(&self) -> &[u8] {
        &self.0
    }
}

pub fn split_into_blocks(bytes: &[u8], chunk_size: usize) -> Vec<ByteBlock> {
    let mut blocks = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let end = (i + chunk_size).min(bytes.len());
        let slice = &bytes[i..end];
        // Store raw bytes - no conversion needed
        blocks.push(ByteBlock(slice.to_vec()));
        i = end;
    }
    blocks
}

async fn get_available_download_path(path: PathBuf) -> PathBuf {
    // Helper function to get the temp file path
    let get_temp_path = |p: &PathBuf| -> PathBuf {
        p.with_extension(format!(
            "{}.tmp",
            p.extension().and_then(|s| s.to_str()).unwrap_or("")
        ))
    };

    // Check if both the final path and temp path are available
    let temp_path = get_temp_path(&path);
    let path_exists = fs::metadata(&path).await.is_ok();
    let temp_exists = fs::metadata(&temp_path).await.is_ok();

    if !path_exists && !temp_exists {
        return path;
    }

    let parent = path.parent().unwrap_or(Path::new(".").into());
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let extension = path.extension().and_then(|s| s.to_str());

    let mut counter = 1;
    loop {
        let new_name = match extension {
            Some(ext) => format!("{} ({}).{}", stem, counter, ext),
            None => format!("{} ({})", stem, counter),
        };

        let new_path = parent.join(new_name);
        let new_temp_path = get_temp_path(&new_path);

        // Check if both the final path and temp path are available
        let new_path_exists = fs::metadata(&new_path).await.is_ok();
        let new_temp_exists = fs::metadata(&new_temp_path).await.is_ok();

        if !new_path_exists && !new_temp_exists {
            return new_path;
        }

        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_command_stops_dht_service() {
        let service = match DhtService::new(
            0,
            Vec::new(),
            None,
            false,
            false,
            None,
            Vec::new(),
            None,
            None,
            None,
            Some(256),  // chunk_size_kb
            Some(1024), // cache_size_mb
            false,      // enable_autorelay
            Vec::new(), // preferred_relays
            false,      // enable_relay_server
            None,
        )
        .await
        {
            Ok(service) => service,
            Err(err) => {
                let message = err.to_string();
                let lowered = message.to_ascii_lowercase();
                if lowered.contains("permission denied") || lowered.contains("not permitted") {
                    // skipping shutdown_command_stops_dht_service (likely sandboxed)
                    return;
                }
                panic!("start service: {message}");
            }
        };

        service.shutdown().await.expect("shutdown");

        // Subsequent calls should gracefully no-op
        assert_eq!(service.get_peer_count().await, 0);

        let snapshot = service.metrics_snapshot().await;
        assert_eq!(snapshot.peer_count, 0);
        assert_eq!(snapshot.reachability, NatReachabilityState::Unknown);
    }

    #[test]
    fn metrics_snapshot_carries_listen_addrs() {
        let mut metrics = DhtMetrics::default();
        metrics.record_listen_addr(&"/ip4/127.0.0.1/tcp/4001".parse::<Multiaddr>().unwrap());
        metrics.record_listen_addr(&"/ip4/0.0.0.0/tcp/4001".parse::<Multiaddr>().unwrap());
        // Duplicate should be ignored
        metrics.record_listen_addr(&"/ip4/127.0.0.1/tcp/4001".parse::<Multiaddr>().unwrap());

        let snapshot = DhtMetricsSnapshot::from(metrics, 5);
        assert_eq!(snapshot.peer_count, 5);
        assert_eq!(snapshot.listen_addrs.len(), 2);
        assert!(snapshot
            .listen_addrs
            .contains(&"/ip4/127.0.0.1/tcp/4001".to_string()));
        assert!(snapshot
            .listen_addrs
            .contains(&"/ip4/0.0.0.0/tcp/4001".to_string()));
        assert!(snapshot.observed_addrs.is_empty());
        assert!(snapshot.reachability_history.is_empty());
    }

    #[tokio::test]
    async fn identify_push_records_listen_addrs() {
        let metrics = Arc::new(Mutex::new(DhtMetrics::default()));
        let listen_addr: Multiaddr = "/ip4/10.0.0.1/tcp/4001".parse().unwrap();
        let secondary_addr: Multiaddr = "/ip4/192.168.0.1/tcp/4001".parse().unwrap();
        let info = identify::Info {
            public_key: identity::Keypair::generate_ed25519().public(),
            protocol_version: EXPECTED_PROTOCOL_VERSION.to_string(),
            agent_version: "test-agent/1.0.0".to_string(),
            listen_addrs: vec![listen_addr.clone(), secondary_addr.clone()],
            protocols: vec![StreamProtocol::new("/chiral/test/1.0.0")],
            observed_addr: "/ip4/127.0.0.1/tcp/4001".parse().unwrap(),
        };

        record_identify_push_metrics(&metrics, &info);

        {
            let guard = metrics.lock().await;
            assert_eq!(guard.listen_addrs.len(), 2);
            assert!(guard.listen_addrs.contains(&listen_addr.to_string()));
            assert!(guard.listen_addrs.contains(&secondary_addr.to_string()));
        }

        record_identify_push_metrics(&metrics, &info);

        let guard = metrics.lock().await;
        assert_eq!(guard.listen_addrs.len(), 2);
    }
}
