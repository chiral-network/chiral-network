use async_trait::async_trait;
use blockstore::{
    block::{Block, CidError},
    InMemoryBlockstore,
};
pub use cid::Cid;
use futures::future::{BoxFuture, FutureExt};
use futures::io::{AsyncRead as FAsyncRead, AsyncWrite as FAsyncWrite};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use futures_util::StreamExt;
use libp2p::multiaddr::Protocol;
use sha2::{Digest, Sha256};
pub use multihash_codetable::{Code, MultihashDigest};
use rs_merkle::{Hasher, MerkleTree};
use crate::manager::Sha256Hasher;
use relay::client::Event as RelayClientEvent;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{debug, error, info, warn};

use crate::peer_selection::{PeerMetrics, PeerSelectionService, SelectionStrategy};
use crate::webrtc_service::{get_webrtc_service, FileChunk};
use std::io::{self};
use tokio_socks::tcp::Socks5Stream;

use std::pin::Pin;
use std::task::{Context, Poll};

// Import the missing types
use crate::file_transfer::FileTransferService;
use std::error::Error;

// Trait alias to abstract over async I/O types used by proxy transport
pub trait AsyncIo: FAsyncRead + FAsyncWrite + Unpin + Send {}
impl<T: FAsyncRead + FAsyncWrite + Unpin + Send> AsyncIo for T {}

use libp2p::core::upgrade::Version;
use libp2p::{
    autonat::v2,
    core::{
        muxing::StreamMuxerBox,
        // FIXED E0432: ListenerEvent is removed, only import what is available.
        transport::{
            choice::OrTransport, Boxed, DialOpts, ListenerId, Transport, TransportError,
            TransportEvent,
        },
    },
    dcutr,
    identify::{self, Event as IdentifyEvent},
    identity,
    kad::{
        self, store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig,
        Event as KademliaEvent, GetRecordOk, Mode, PutRecordOk, QueryResult, Record,
    },
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    noise,
    ping::{self, Behaviour as Ping, Event as PingEvent},
    relay, request_response as rr,
    swarm::{behaviour::toggle, NetworkBehaviour, SwarmEvent},
    tcp, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use rand::rngs::OsRng;
const EXPECTED_PROTOCOL_VERSION: &str = "/chiral/1.0.0";
const MAX_MULTIHASH_LENGHT: usize = 64;
pub const RAW_CODEC: u64 = 0x55;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub is_root: bool,
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
    mdns: Mdns,
    bitswap: beetswap::Behaviour<MAX_MULTIHASH_LENGHT, InMemoryBlockstore<MAX_MULTIHASH_LENGHT>>,
    ping: ping::Behaviour,
    proxy_rr: rr::Behaviour<ProxyCodec>,
    webrtc_signaling_rr: rr::Behaviour<WebRTCSignalingCodec>,
    autonat_client: toggle::Toggle<v2::client::Behaviour>,
    autonat_server: toggle::Toggle<v2::server::Behaviour>,
    relay_client: relay::client::Behaviour,
    relay_server: toggle::Toggle<relay::Behaviour>,
    dcutr: toggle::Toggle<dcutr::Behaviour>,
}
#[derive(Debug)]
pub enum DhtCommand {
    PublishFile(FileMetadata),
    SearchFile(String),
    DownloadFile(FileMetadata),
    ConnectPeer(String),
    ConnectToPeerById(PeerId),
    DisconnectPeer(PeerId),
    GetPeerCount(oneshot::Sender<usize>),
    Echo {
        peer: PeerId,
        payload: Vec<u8>,
        tx: oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Shutdown(oneshot::Sender<()>),
    StopPublish(String),
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
    PutRecord {
        key: String,
        value: Vec<u8>,
        sender: oneshot::Sender<Result<(), String>>,
    },
    GetRecord {
        key: String,
        sender: oneshot::Sender<Result<Option<Vec<u8>>, String>>,
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
        info!("Privacy routing enabled in proxy manager (mode: {:?})", mode);
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
    }

    fn is_trusted_proxy_node(&self, peer_id: &PeerId) -> bool {
        self.trusted_proxy_nodes.contains(peer_id)
    }

    fn get_trusted_proxy_nodes(&self) -> &std::collections::HashSet<PeerId> {
        &self.trusted_proxy_nodes
    }

    fn select_proxy_for_routing(&self, target_peer: &PeerId) -> Option<PeerId> {
        if !self.privacy_routing_enabled {
            return None;
        }

        // Select a trusted proxy node that's online and not the target itself
        self.trusted_proxy_nodes
            .iter()
            .find(|&&proxy_id| {
                proxy_id != *target_peer &&
                self.online.contains(&proxy_id) &&
                self.capable.contains(&proxy_id)
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

fn build_relay_listen_addr(base: &Multiaddr) -> Option<Multiaddr> {
    let mut addr = base.clone();
    match addr.pop() {
        Some(libp2p::multiaddr::Protocol::P2p(_)) => {
            addr.push(libp2p::multiaddr::Protocol::P2pCircuit);
            Some(addr)
        }
        _ => None,
    }
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

/// Check if an address is likely unreachable from the public internet
/// Returns true if the address should be filtered out
fn is_unreachable_address(addr: &Multiaddr) -> bool {
    use libp2p::multiaddr::Protocol;
    use std::net::IpAddr;
    
    for protocol in addr.iter() {
        if let Protocol::Ip4(ip) = protocol {
            let octets = ip.octets();
            
            // Docker bridge networks (172.17.0.0/16, 172.18.0.0/16, etc.)
            if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) {
                return true;
            }
            
            // Private networks that are typically not routable
            // 10.0.0.0/8
            if octets[0] == 10 {
                return true;
            }
            
            // 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            
            // Localhost
            if octets[0] == 127 {
                return true;
            }
        }
    }
    
    false
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
            autorelay_enabled,
            active_relay_peer_id,
            relay_reservation_status,
            last_reservation_success,
            last_reservation_failure,
            reservation_renewals,
            reservation_evictions,
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
    pending_webrtc_offers: Arc<
        Mutex<
            HashMap<rr::OutboundRequestId, oneshot::Sender<Result<WebRTCAnswerResponse, String>>>,
        >,
    >,
    pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>>,
    root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>>,
    active_downloads: Arc<Mutex<HashMap<String, ActiveDownload>>>,
    get_providers_queries: Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>>,
    is_bootstrap: bool,
    enable_autorelay: bool,
    relay_candidates: HashSet<String>,
    chunk_size: usize,
) {
    // Track put/get record query IDs
    let mut put_record_senders: HashMap<kad::QueryId, oneshot::Sender<Result<(), String>>> = HashMap::new();
    let mut get_record_senders: HashMap<kad::QueryId, oneshot::Sender<Result<Option<Vec<u8>>, String>>> = HashMap::new();
    
    let mut dht_maintenance_interval = tokio::time::interval(Duration::from_secs(30 * 60)); 
    dht_maintenance_interval.tick().await; 
    // Periodic bootstrap interval

    /// Creates a proper circuit relay address for connecting through a relay peer
    /// Returns a properly formatted Multiaddr for circuit relay connections
    fn create_circuit_relay_address(relay_peer_id: &PeerId, target_peer_id: &PeerId) -> Result<Multiaddr, String> {
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
            Err(format!("Failed to create valid circuit relay address for relay {}", relay_peer_id))
        }
    }

    /// Enhanced circuit relay address creation with multiple fallback strategies
    fn create_circuit_relay_address_robust(relay_peer_id: &PeerId, target_peer_id: &PeerId) -> Multiaddr {
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

        if relay_with_port.to_string().contains(&relay_peer_id.to_string()) {
            info!("Created circuit relay address with port: {}", relay_with_port);
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

    'outer: loop {
        tokio::select! {
            _= dht_maintenance_interval.tick() => {
                info!("Triggering periodic DHT maintenanace: Kademlia bootstrap and Record Refresh.");
                let _ = swarm.behaviour_mut().kademlia.bootstrap();
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(DhtCommand::Shutdown(ack)) => {
                        info!("Received shutdown signal for DHT node");
                        shutdown_ack = Some(ack);
                        break 'outer;
                    }
                    Some(DhtCommand::PublishFile(mut metadata)) => {
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

                                match swarm.behaviour_mut().bitswap.insert_block::<MAX_MULTIHASH_LENGHT>(cid.clone(), block.data().to_vec())                          {
                                    Ok(_) => {},
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
                            match swarm.behaviour_mut().bitswap.insert_block::<MAX_MULTIHASH_LENGHT>(root_cid.clone(), root_block_data) {
                                Ok(_) => {},
                                Err(e) => {
                                    error!("failed to store root block: {}", e);
                                    let _ = event_tx.send(DhtEvent::Error(format!("failed to store root block: {}", e))).await;
                                    return;
                                }
                            }

                            // The file_hash is the Merkle Root. The root_cid is for retrieval.
                            metadata.merkle_root = hex::encode(merkle_root);
                            metadata.cids = Some(vec![root_cid]); // Store the root CID for bitswap retrieval
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

                        // Store minimal metadata in DHT
                        let dht_metadata = serde_json::json!({
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
                        });

                        let dht_record_data = match serde_json::to_vec(&dht_metadata) {
                            Ok(data) => data,
                            Err(e) => {
                                eprintln!("Failed to serialize DHT metadata: {}", e);
                                return;
                            }
                        };

                        let key = kad::RecordKey::new(&metadata.merkle_root.as_bytes());
                        let record = Record {
                                    key,
                                    value: dht_record_data,
                                    publisher: Some(peer_id),
                                    expires: None,
                                };

                        match swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One){
                            Ok(query_id) => {
                                info!("started providing file: {}, query id: {:?}", metadata.merkle_root, query_id);
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
                        let _ = event_tx.send(DhtEvent::PublishedFile(metadata)).await;
                    }
                    Some(DhtCommand::DownloadFile(file_metadata)) =>{
                        // Get root CID from file hash
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
                        // Request the root block which contains the CIDs
                        let root_query_id = swarm.behaviour_mut().bitswap.get(&root_cid);

                        // Store the root query ID to handle when we get the root block
                        root_query_mapping.lock().await.insert(root_query_id, file_metadata);
                    }

                    Some(DhtCommand::StopPublish(file_hash)) => {
                        let key = kad::RecordKey::new(&file_hash);
                        // Remove the record
                        // swarm.behaviour_mut().kademlia.stop_providing(&key);
                        swarm.behaviour_mut().kademlia.remove_record(&key)
                    }
                    Some(DhtCommand::SearchFile(file_hash)) => {
                        let key = kad::RecordKey::new(&file_hash.as_bytes());
                        let query_id = swarm.behaviour_mut().kademlia.get_record(key);
                        info!("Searching for file: {} (query: {:?})", file_hash, query_id);
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
                    Some(DhtCommand::PutRecord { key, value, sender }) => {
                        let record = Record {
                            key: kad::RecordKey::new(&key),
                            value,
                            publisher: Some(peer_id),
                            expires: None,
                        };
                        
                        match swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One) {
                            Ok(query_id) => {
                                info!("Started put_record for key: {}, query id: {:?}", key, query_id);
                                // Store the sender to respond when the query completes
                                put_record_senders.insert(query_id, sender);
                            }
                            Err(e) => {
                                error!("Failed to put_record for key {}: {}", key, e);
                                let _ = sender.send(Err(format!("Failed to put record: {}", e)));
                            }
                        }
                    }
                    Some(DhtCommand::GetRecord { key, sender }) => {
                        let record_key = kad::RecordKey::new(&key);
                        let query_id = swarm.behaviour_mut().kademlia.get_record(record_key);
                        info!("Started get_record for key: {}, query id: {:?}", key, query_id);
                        // Store the sender to respond when the query completes
                        get_record_senders.insert(query_id, sender);
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
                            &connected_peers,
                            &event_tx,
                            &pending_searches,
                            &pending_provider_queries,
                            &get_providers_queries,
                            &mut put_record_senders,
                            &mut get_record_senders,
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
                            }
                            RelayEvent::ReservationReqDenied { src_peer_id, .. } => {
                                debug!("🔁 Relay server: Denied reservation from {}", src_peer_id);
                            }
                            RelayEvent::ReservationTimedOut { src_peer_id } => {
                                debug!("🔁 Relay server: Reservation timed out for {}", src_peer_id);
                            }
                            RelayEvent::CircuitReqDenied { src_peer_id, dst_peer_id, .. } => {
                                debug!("🔁 Relay server: Denied circuit from {} to {}", src_peer_id, dst_peer_id);
                            }
                            RelayEvent::CircuitReqAccepted { src_peer_id, dst_peer_id, .. } => {
                                info!("🔁 Relay server: Established circuit from {} to {}", src_peer_id, dst_peer_id);
                                let _ = event_tx
                                    .send(DhtEvent::Info(format!(
                                        "Relaying traffic from {} to {}",
                                        src_peer_id, dst_peer_id
                                    )))
                                    .await;
                            }
                            RelayEvent::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                                debug!("🔁 Relay server: Circuit closed between {} and {}", src_peer_id, dst_peer_id);
                            }
                            // Handle deprecated relay events (libp2p handles logging internally)
                            _ => {}
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Bitswap(bitswap)) => match bitswap {
                        beetswap::Event::GetQueryResponse { query_id, data } => {
                            // Check if this is a root block query first
                            if let Some(metadata) = root_query_mapping.lock().await.remove(&query_id) {
                                // This is the root block containing CIDs - parse and request all data blocks
                                if let Ok(cids) = serde_json::from_slice::<Vec<Cid>>(&data) {
                                    info!("Received root block for file {} with {} CIDs", metadata.merkle_root, cids.len());

                                    // Create queries map for this file's data blocks
                                    let mut file_queries = HashMap::new();

                                    for (i, cid) in cids.iter().enumerate() {
                                        let block_query_id = swarm.behaviour_mut().bitswap.get(cid);
                                        file_queries.insert(block_query_id, i as u32);
                                    }

                                    // Create active download tracking for this file
                                    let active_download = ActiveDownload {
                                        metadata: metadata.clone(),
                                        queries: file_queries,
                                        downloaded_chunks: HashMap::new(),
                                    };

                                    // Store the active download
                                    active_downloads.lock().await.insert(metadata.merkle_root.clone(), active_download);

                                    info!("Started tracking download for file {} with {} chunks", metadata.merkle_root, cids.len());
                                } else {
                                    error!("Failed to parse root block as CIDs array for file {}", metadata.merkle_root);
                                }
                            } else {
                                // This is a data block query - find the corresponding file and handle it
                                let mut completed_downloads = Vec::new();

                                // Check all active downloads for this query_id
                                {
                                    let mut active_downloads_guard = active_downloads.lock().await;
                                    for (file_hash, active_download) in active_downloads_guard.iter_mut() {
                                        if let Some(chunk_index) = active_download.queries.remove(&query_id) {
                                            // This query belongs to this file - store the chunk
                                            active_download.downloaded_chunks.insert(chunk_index, data.clone());

                                            // Check if all chunks for this file are downloaded
                                            if active_download.queries.is_empty() {
                                                info!("All chunks downloaded for file {}", file_hash);

                                                // Reassemble the file
                                                let mut file_data = Vec::new();
                                                for i in 0..active_download.downloaded_chunks.len() as u32 {
                                    if let Some(chunk) = active_download.downloaded_chunks.get(&(i as u32)) {
                                                        file_data.extend_from_slice(chunk);
                                                    }
                                                }

                                                // Create the completed metadata
                                                let mut completed_metadata = active_download.metadata.clone();
                                                completed_metadata.file_data = file_data;

                                                completed_downloads.push(completed_metadata);
                                            }
                                            break;
                                        }
                                    }
                                }

                                // Send completion events for finished downloads
                                for metadata in completed_downloads {
                                    let _ = event_tx.send(DhtEvent::DownloadedFile(metadata)).await;
                                }
                            }

                            info!("Bitswap query {:?} succeeded - received {} bytes", query_id, data.len());

                            // Process the received data - this is a file chunk that was requested
                            // Parse the chunk data and assemble the complete file
                            if let Some(ref ft_service) = file_transfer_service {
                                process_bitswap_chunk(&query_id, &data, &event_tx, &received_chunks, ft_service).await;
                            } else {
                                warn!("File transfer service not available, cannot process Bitswap chunk");
                            }
                        }
                        beetswap::Event::GetQueryError { query_id, error } => {
                            // Handle Bitswap query error
                            warn!("Bitswap query {:?} failed: {:?}", query_id, error);

                            // Clean up any active downloads that contain this failed query
                            {
                                let mut active_downloads_guard = active_downloads.lock().await;
                                let mut failed_files = Vec::new();

                                for (file_hash, active_download) in active_downloads_guard.iter_mut() {
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
                        proxy_mgr.lock().await.remove_all(&peer_id);
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
                            m.bootstrap_failures = m.bootstrap_failures.saturating_add(1);
                        }
                        if let Some(peer_id) = peer_id {
                            error!("❌ Outgoing connection error to {}: {}", peer_id, error);
                            // Check if this is a bootstrap connection error
                            if error.to_string().contains("rsa") {
                                error!("   ℹ Hint: This node uses RSA keys. Enable 'rsa' feature if needed.");
                            } else if error.to_string().contains("Timeout") {
                                warn!("   ℹ Hint: Bootstrap nodes may be unreachable or overloaded.");
                            } else if error.to_string().contains("Connection refused") {
                                warn!("   ℹ Hint: Bootstrap nodes are not accepting connections.");
                            } else if error.to_string().contains("Transport") {
                                warn!("   ℹ Hint: Transport protocol negotiation failed.");
                            }
                            swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
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

                                    // 2) Showing received data to UI
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

async fn handle_kademlia_event(
    event: KademliaEvent,
    swarm: &mut Swarm<DhtBehaviour>,
    connected_peers: &Arc<Mutex<HashSet<PeerId>>>,
    event_tx: &mpsc::Sender<DhtEvent>,
    pending_searches: &Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    pending_provider_queries: &Arc<Mutex<HashMap<String, PendingProviderQuery>>>,
    get_providers_queries: &Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>>,
    put_record_senders: &mut HashMap<kad::QueryId, oneshot::Sender<Result<(), String>>>,
    get_record_senders: &mut HashMap<kad::QueryId, oneshot::Sender<Result<Option<Vec<u8>>, String>>>,
) {
    match event {
        KademliaEvent::RoutingUpdated { peer, .. } => {
            debug!("Routing table updated with peer: {}", peer);
        }
        KademliaEvent::UnroutablePeer { peer } => {
            warn!("Peer {} is unroutable", peer);
        }
        KademliaEvent::RoutablePeer { peer, .. } => {
            debug!("Peer {} became routable", peer);
        }
        KademliaEvent::OutboundQueryProgressed { result, id, .. } => {
            match result {
                QueryResult::GetRecord(Ok(ok)) => match ok {
                    GetRecordOk::FoundRecord(peer_record) => {
                        // Check if this is a query we're tracking for pool discovery
                        if let Some(sender) = get_record_senders.remove(&id) {
                            info!("GetRecord succeeded for pool query: {:?}", id);
                            let _ = sender.send(Ok(Some(peer_record.record.value.clone())));
                        } else {
                            // This is a file metadata query (existing behavior)
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
                            ) = ( // Use merkle_root as the primary identifier
                                metadata_json.get("merkle_root").and_then(|v| v.as_str()),
                                metadata_json.get("file_name").and_then(|v| v.as_str()),
                                metadata_json.get("file_size").and_then(|v| v.as_u64()),
                                metadata_json.get("created_at").and_then(|v| v.as_u64()),
                            ) {
                                let metadata = FileMetadata {
                                    merkle_root: file_hash.to_string(),
                                    file_name: file_name.to_string(),
                                    file_size,
                                    file_data: Vec::new(), // Will be populated during download
                                    seeders: vec![peer_record
                                        .peer
                                        .map(|p| p.to_string())
                                        .unwrap_or_default()],
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
                                    is_root: metadata_json
                                        .get("is_root")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(true),
                                };

                                let notify_metadata = metadata.clone();
                                let file_hash = notify_metadata.merkle_root.clone();
                                info!("File discovered: {} ({})", notify_metadata.file_name, file_hash);
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
                        } // end of else block for file metadata query
                    }
                    GetRecordOk::FinishedWithNoAdditionalRecord { .. } => {
                        // Check if this is a query we're tracking for pool discovery
                        if let Some(sender) = get_record_senders.remove(&id) {
                            info!("GetRecord finished with no record found: {:?}", id);
                            let _ = sender.send(Ok(None));
                        }
                        // No additional records; do nothing here
                    }
                },
                QueryResult::GetRecord(Err(err)) => {
                    // Check if this is a query we're tracking for pool discovery
                    if let Some(sender) = get_record_senders.remove(&id) {
                        warn!("GetRecord failed for pool query: {:?}", err);
                        let _ = sender.send(Err(format!("GetRecord failed: {:?}", err)));
                    } else {
                        // Existing error handling for file metadata queries
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
                }
                QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                    // Check if this is a query we're tracking for pool announcements
                    if let Some(sender) = put_record_senders.remove(&id) {
                        info!("PutRecord succeeded for pool: {:?}", key);
                        let _ = sender.send(Ok(()));
                    } else {
                        // Existing behavior for file publishing
                        debug!("PutRecord succeeded for key: {:?}", key);
                    }
                }
                QueryResult::PutRecord(Err(err)) => {
                    // Check if this is a query we're tracking for pool announcements
                    if let Some(sender) = put_record_senders.remove(&id) {
                        warn!("PutRecord failed for pool query: {:?}", err);
                        let _ = sender.send(Err(format!("PutRecord failed: {:?}", err)));
                    } else {
                        // Existing error handling
                        warn!("PutRecord error: {:?}", err);
                        let _ = event_tx
                            .send(DhtEvent::Error(format!("PutRecord failed: {:?}", err)))
                            .await;
                    }
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

                        info!("Found {} closest peers for target peer {}", peers.len(), target_peer_id);

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
                                    info!("Attempting to connect to peer {} at {}", peer_info.peer_id, addr);
                                    // Add address to Kademlia routing table
                                    swarm.behaviour_mut().kademlia.add_address(&peer_info.peer_id, addr.clone());

                                    // Attempt direct connection
                                    match swarm.dial(addr.clone()) {
                                        Ok(_) => {
                                            info!("✅ Initiated connection to peer {} at {}", peer_info.peer_id, addr);
                                            connected = true;
                                            connection_attempts += 1;
                                            break; // Successfully initiated connection, no need to try other addresses
                                        }
                                        Err(e) => {
                                            debug!("Failed to dial peer {} at {}: {}", peer_info.peer_id, addr, e);
                                        }
                                    }
                                }
                            }

                            if !connected {
                                info!("Could not connect to peer {} with any available address", peer_info.peer_id);
                            }
                        }

                        let _ = event_tx.send(DhtEvent::Info(format!(
                            "Found {} peers close to target peer {}, attempted connections to {}",
                            peers.len(),
                            target_peer_id,
                            connection_attempts
                        ))).await;
                    }
                },
                QueryResult::GetClosestPeers(Err(err)) => {
                    warn!("GetClosestPeers query failed: {:?}", err);
                    let _ = event_tx.send(DhtEvent::Error(format!("Peer discovery failed: {:?}", err))).await;
                }
                QueryResult::GetProviders(Ok(ok)) => {
                    if let kad::GetProvidersOk::FoundProviders { key, providers } = ok {
                        let file_hash = String::from_utf8_lossy(key.as_ref()).to_string();
                        info!("Found {} providers for file: {}", providers.len(), file_hash);

                        // Convert providers to string format
                        let provider_strings: Vec<String> = providers.iter().map(|p| p.to_string()).collect();

                        // Find and notify the pending query
                        let mut pending_queries = pending_provider_queries.lock().await;
                        if let Some(pending_query) = pending_queries.remove(&file_hash) {
                            let _ = pending_query.sender.send(Ok(provider_strings));
                        } else {
                            warn!("No pending provider query found for file: {}", file_hash);
                        }
                    }
                },
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
                        warn!("Cleaning up GetProviders query for file: {} (failed or timed out)", file_hash);
                        get_providers_queries.lock().await.remove(&query_id);

                        if let Some(pending_query) = pending_provider_queries.lock().await.remove(&file_hash) {
                            let _ = pending_query.sender.send(Err(format!("GetProviders query failed or timed out for file {}: {:?}", file_hash, err)));
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
) {
    match event {
        IdentifyEvent::Received { peer_id, info, .. } => {
            info!(
                "🔍 Identified peer {}: {:?} (listen_addrs: {})",
                peer_id,
                info.protocol_version,
                info.listen_addrs.len()
            );

            // Log AutoRelay debug info
            if enable_autorelay {
                let is_candidate = is_relay_candidate(&peer_id, relay_candidates);
                info!(
                    "  AutoRelay check: is_relay_candidate={}, total_candidates={}",
                    is_candidate,
                    relay_candidates.len()
                );
                if !relay_candidates.is_empty() {
                    info!(
                        "  Relay candidates: {:?}",
                        relay_candidates.iter().take(3).collect::<Vec<_>>()
                    );
                }
            }

            if info.protocol_version != EXPECTED_PROTOCOL_VERSION {
                warn!(
                    "Peer {} has a mismatched protocol version: '{}'. Expected: '{}'. Removing peer.",
                    peer_id,
                    info.protocol_version,
                    EXPECTED_PROTOCOL_VERSION
                );
                swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
            } else {
                let listen_addrs = info.listen_addrs.clone();
                if let Ok(mut metrics_guard) = metrics.try_lock() {
                    metrics_guard.record_observed_addr(&info.observed_addr);
                }
                // for addr in info.listen_addrs {
                for addr in listen_addrs.iter() {
                    // Filter out unreachable addresses (Docker IPs, private IPs without relay)
                    if is_unreachable_address(addr) {
                        debug!("  🚫 Skipping unreachable addr: {}", addr);
                        continue;
                    }
                    
                    info!("  📍 Peer {} listen addr: {}", peer_id, addr);
                    if not_loopback(&addr) {
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, addr.clone());

                        // AutoRelay: Check if this peer is a relay candidate
                        if enable_autorelay && is_relay_candidate(&peer_id, relay_candidates) {
                            info!("  🎯 Relay candidate matched! Attempting relay setup...");

                            // Listen on relay address for incoming connections
                            if let Some(relay_addr) = build_relay_listen_addr(&addr) {
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
                                    info!("✅ Listening via relay peer {}", peer_id);
                                }
                            }
                        }
                    }
                }
                // let mut addresses: Vec<String> = listen_addrs.iter().map(|a| a.to_string()).collect();
                // if let Some(observed) = info.observed_addr {
                //     addresses.push(observed.to_string());
                // }
                let mut addresses: Vec<String> = listen_addrs.iter().map(|a| a.to_string()).collect();
addresses.push(info.observed_addr.to_string());

                let _ = event_tx
                    .send(DhtEvent::PeerDiscovered {
                        peer_id: peer_id.to_string(),
                        addresses,
                    })
                    .await;
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
                if not_loopback(&multiaddr) {
                    swarm
                        .behaviour_mut()
                        .kademlia
                        // .add_address(&peer_id, multiaddr);
                        .add_address(&peer_id, multiaddr.clone());
                }
                discovered
                    .entry(peer_id)
                    .or_default()
                    .push(multiaddr.to_string());
            }
            for (peer_id, addresses) in discovered {
                let _ = event_tx
                    // .send(DhtEvent::PeerDiscovered(peer_id.to_string()))
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
pub fn build_transport_with_relay(
    keypair: &identity::Keypair,
    relay_transport: relay::client::Transport,
    proxy_address: Option<String>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>, Box<dyn Error>> {
    let noise_keys = noise::Config::new(keypair)?;
    let yamux_config = libp2p::yamux::Config::default();

    let transport = match (proxy_address, relay_transport) {
        (Some(proxy), relay_transport) => {
            info!(
                "SOCKS5 enabled. Routing all P2P dialing traffic via {}",
                proxy
            );
            let proxy_addr = proxy.parse::<SocketAddr>().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid proxy address: {}", e),
                )
            })?;
            let socks5_transport = Socks5Transport::new(proxy_addr);

            OrTransport::new(relay_transport, socks5_transport)
                .map(|either, _| match either {
                    futures::future::Either::Left(conn) => RelayTransportOutput::Relay(conn),
                    futures::future::Either::Right(stream) => RelayTransportOutput::Direct(stream),
                })
                .upgrade(Version::V1)
                .authenticate(noise_keys)
                .multiplex(yamux_config)
                .timeout(Duration::from_secs(10))
                .boxed()
        }
        (None, relay_transport) => {
            let direct_tcp = tcp::tokio::Transport::new(tcp::Config::default())
                .map(|s, _| Box::new(s.0.compat()) as Box<dyn AsyncIo>);

            OrTransport::new(relay_transport, direct_tcp)
                .map(|either, _| match either {
                    futures::future::Either::Left(conn) => RelayTransportOutput::Relay(conn),
                    futures::future::Either::Right(stream) => RelayTransportOutput::Direct(stream),
                })
                .upgrade(Version::V1)
                .authenticate(noise_keys)
                .multiplex(yamux_config)
                .timeout(Duration::from_secs(10))
                .boxed()
        }
    };

    Ok(transport)
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
    metrics: Arc<Mutex<DhtMetrics>>,
    pending_echo: Arc<Mutex<HashMap<rr::OutboundRequestId, PendingEcho>>>,
    pending_searches: Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    search_counter: Arc<AtomicU64>,
    proxy_mgr: ProxyMgr,
    peer_selection: Arc<Mutex<PeerSelectionService>>,
    file_metadata_cache: Arc<Mutex<HashMap<String, FileMetadata>>>,
    received_chunks: Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: Option<Arc<FileTransferService>>,
    pending_webrtc_offers: Arc<
        Mutex<
            HashMap<rr::OutboundRequestId, oneshot::Sender<Result<WebRTCAnswerResponse, String>>>,
        >,
    >,
    pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>>,
    root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>>, // Track root query IDs for file downloads
    active_downloads: Arc<Mutex<HashMap<String, ActiveDownload>>>, // Track active file downloads
    get_providers_queries: Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>>, // Track GetProviders query_id -> (file_hash, start_time) mapping
    chunk_size: usize, // Configurable chunk size in bytes
}

/// Tracks an active file download with its associated queries and chunks
#[derive(Debug, Clone)]
struct ActiveDownload {
    metadata: FileMetadata,
    queries: HashMap<beetswap::QueryId, u32>, // query_id -> chunk_index
    downloaded_chunks: HashMap<u32, Vec<u8>>, // chunk_index -> data
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
        chunk_size_kb: Option<usize>, // Chunk size in KB (default 256)
        cache_size_mb: Option<usize>, // Cache size in MB (default 1024)
        enable_autorelay: bool,
        preferred_relays: Vec<String>,
        enable_relay_server: bool,
    ) -> Result<Self, Box<dyn Error>> {
        // Convert chunk size from KB to bytes
        let chunk_size = chunk_size_kb.unwrap_or(256) * 1024; // Default 256 KB
        let cache_size = cache_size_mb.unwrap_or(1024); // Default 1024 MB

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
        let mdns = Mdns::new(Default::default(), local_peer_id)?;

        // Request-Response behaviours
        let rr_cfg = rr::Config::default();
        let proxy_protocols =
            std::iter::once(("/chiral/proxy/1.0.0".to_string(), rr::ProtocolSupport::Full));
        let proxy_rr = rr::Behaviour::new(proxy_protocols, rr_cfg.clone());

        let webrtc_protocols = std::iter::once((
            "/chiral/webrtc-signaling/1.0.0".to_string(),
            rr::ProtocolSupport::Full,
        ));
        let webrtc_signaling_rr = rr::Behaviour::new(webrtc_protocols, rr_cfg);

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
        let autonat_server_behaviour = if enable_autonat {
            Some(v2::server::Behaviour::new(OsRng))
        } else {
            None
        };

        let blockstore = Arc::new(InMemoryBlockstore::new());
        let bitswap = beetswap::Behaviour::new(blockstore);
        let (relay_transport, relay_client_behaviour) = relay::client::new(local_peer_id);
        let autonat_client_toggle = toggle::Toggle::from(autonat_client_behaviour);
        let autonat_server_toggle = toggle::Toggle::from(autonat_server_behaviour);

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
            mdns,
            bitswap,
            ping: Ping::new(ping::Config::new()),
            proxy_rr,
            webrtc_signaling_rr,
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

        // Configure AutoRelay relay candidate discovery
        let relay_candidates: HashSet<String> = if enable_autorelay {
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

        // Connect to bootstrap nodes
        let mut successful_connections = 0;
        let total_bootstrap_nodes = bootstrap_nodes.len();
        for bootstrap_addr in &bootstrap_nodes {
            if let Ok(addr) = bootstrap_addr.parse::<Multiaddr>() {
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
        let pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>> = Arc::new(Mutex::new(HashMap::new()));
        let root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>> = Arc::new(Mutex::new(HashMap::new()));
        let active_downloads: Arc<Mutex<HashMap<String, ActiveDownload>>> = Arc::new(Mutex::new(HashMap::new()));
        let get_providers_queries_local: Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>> = Arc::new(Mutex::new(HashMap::new()));

        {
            let mut guard = metrics.lock().await;
            guard.autonat_enabled = enable_autonat;
            guard.autorelay_enabled = enable_autorelay;
            guard.dcutr_enabled = enable_autonat; // DCUtR enabled when AutoNAT is enabled
        }

        // Spawn the Dht node task
        let received_chunks_clone = Arc::new(Mutex::new(HashMap::new()));
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
            pending_webrtc_offers.clone(),
            pending_provider_queries.clone(),
            root_query_mapping.clone(),
            active_downloads.clone(),
            get_providers_queries_local.clone(),
            is_bootstrap,
            enable_autorelay,
            relay_candidates,
            chunk_size,
        ));

        Ok(DhtService {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            peer_id: peer_id_str,
            connected_peers,
            metrics,
            pending_echo,
            pending_searches,
            search_counter,
            proxy_mgr,
            peer_selection,
            file_metadata_cache: Arc::new(Mutex::new(HashMap::new())),
            received_chunks: received_chunks_clone,
            file_transfer_service,
            pending_webrtc_offers,
            pending_provider_queries,
            root_query_mapping,
            active_downloads,
            get_providers_queries: get_providers_queries_local,
            chunk_size,
        })
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    pub async fn publish_file(&self, metadata: FileMetadata) -> Result<(), String> {
        self.file_metadata_cache
            .lock()
            .await
            .insert(metadata.merkle_root.clone(), metadata.clone());
        self.cmd_tx
            .send(DhtCommand::PublishFile(metadata))
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn stop_publishing_file(&self, file_hash: String) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::StopPublish(file_hash))
            .await
            .map_err(|e| e.to_string())
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
    pub async fn get_versions_by_file_name(
        &self,
        file_name: String,
    ) -> Result<Vec<FileMetadata>, String> {
        let all = self.get_all_file_metadata().await?;
        let mut versions: Vec<FileMetadata> = all
            .into_iter()
            .filter(|m| m.file_name == file_name) // Remove is_root filter - get all versions
            .collect();
        versions.sort_by(|a, b| b.version.unwrap_or(1).cmp(&a.version.unwrap_or(1)));
        // For each version, try to find seeders (peers that have this file)
        for version in &mut versions {
            version.seeders = self.get_seeders_for_file(&version.merkle_root).await;
        }

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
        is_encrypted: bool,
        encryption_method: Option<String>,
        key_fingerprint: Option<String>,
    ) -> Result<FileMetadata, String> {
        let latest = self
            .get_latest_version_by_file_name(file_name.clone())
            .await?;

        let (version, parent_hash, is_root) = match latest {
            Some(ref prev) => (
                prev.version.map(|v| v + 1).unwrap_or(2),
                Some(prev.merkle_root.clone()),
                false, // not root if there was a previous version
            ),
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
            parent_hash,
            cids: None,
            is_root, // Use computed value, not hardcoded true
        })
    }

    pub async fn download_file(&self, file_metadata: FileMetadata) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::DownloadFile(file_metadata))
            .await
            .map_err(|e| e.to_string())
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

    /// Stores a key-value record in the DHT
    /// This can be used for P2P pool announcements and other distributed data
    pub async fn put_record(&self, key: &str, value: Vec<u8>) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::PutRecord {
                key: key.to_string(),
                value,
                sender: tx,
            })
            .await
            .map_err(|e| format!("Failed to send put_record command: {}", e))?;

        rx.await
            .map_err(|e| format!("Put record response error: {}", e))?
    }

    /// Retrieves a value for a key from the DHT
    /// Returns None if the key is not found
    /// This can be used for P2P pool discovery
    pub async fn get_record(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::GetRecord {
                key: key.to_string(),
                sender: tx,
            })
            .await
            .map_err(|e| format!("Failed to send get_record command: {}", e))?;

        rx.await
            .map_err(|e| format!("Get record response error: {}", e))?
    }

    /// Verifies that a peer actually provides working proxy services through protocol negotiation
    async fn verify_proxy_capabilities(
        &self,
        peer_id: &PeerId,
    ) -> Result<(), String> {
        // Use the existing echo protocol to verify proxy capabilities
        // Send a special "proxy_verify" message that the peer should echo back if it's a working proxy

        let verification_payload = b"proxy_verify";

        // Send echo request with verification payload through DHT service
        match self.echo(peer_id.to_string(), verification_payload.to_vec()).await {
            Ok(response) => {
                // Check if the response matches our verification payload
                if response == verification_payload {
                    info!("✅ Proxy capability verified for peer {} via echo test", peer_id);
                    Ok(())
                } else {
                    Err(format!("Proxy verification failed: unexpected response from peer {}", peer_id))
                }
            }
            Err(e) => {
                Err(format!("Proxy verification failed: echo request failed for peer {}: {}", peer_id, e))
            }
        }
    }

    /// Discovers proxy services through DHT provider queries
    /// Uses DHT provider discovery to find peers advertising proxy services
    async fn discover_proxy_services_through_dht_providers(&self, proxy_mgr: &mut ProxyManager) -> usize {
        let mut discovered_and_verified = 0;

        info!("Starting DHT proxy service discovery using provider queries...");

        // Query DHT for peers that provide proxy services
        // Use a standard proxy service identifier that proxy nodes would register as providers for
        let proxy_service_cid = "proxy:service:available"; // This would be a well-known CID for proxy services

        match self.query_dht_proxy_providers(proxy_service_cid.to_string()).await {
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
                                info!("✅ Verified and added DHT-discovered proxy provider: {}", peer_id);
                            }
                            Err(e) => {
                                warn!("❌ DHT proxy provider verification failed for {}: {}", peer_id, e);
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

        info!("DHT proxy provider discovery completed: {} proxies verified and added", discovered_and_verified);
        discovered_and_verified
    }

    /// Query DHT for peers providing proxy services using provider records
    /// Returns a list of peer IDs that provide proxy services
    async fn query_dht_proxy_providers(&self, service_identifier: String) -> Result<Vec<PeerId>, String> {
        // Create a DHT record key for proxy services
        let key = kad::RecordKey::new(&service_identifier);

        // Query DHT for providers of this service
        // This finds peers that have registered as providers for proxy services
        let (tx, rx) = oneshot::channel();

        // Send command to query providers
        if let Err(e) = self.cmd_tx.send(DhtCommand::GetProviders {
            file_hash: service_identifier.clone(),
            sender: tx,
        }).await {
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

                info!("Found {} proxy service providers in DHT ({} valid peer IDs)", total_count, peer_ids.len());
                Ok(peer_ids)
            }
            Ok(Ok(Err(e))) => {
                Err(format!("GetProviders command failed: {}", e))
            }
            Ok(Err(_)) => {
                Err("GetProviders channel closed unexpectedly".to_string())
            }
            Err(_) => {
                Err("GetProviders query timed out".to_string())
            }
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
                    if let Err(e) = self.cmd_tx.send(DhtCommand::ConnectToPeerById(peer_id)).await {
                        warn!("Failed to send ConnectToPeerById command for {}: {}", seeder_id, e);
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
                        warn!("❌ Proxy capability verification failed for peer {}: {}", peer_id, e);
                        // Remove from capable list if verification fails
                        proxy_mgr.capable.remove(&peer_id);
                    }
                }
            }
        }

        // Query DHT for peers advertising proxy services and add them to verification pipeline
        let dht_proxy_count = self.discover_proxy_services_through_dht_providers(&mut proxy_mgr).await;

        let trusted_count = trusted_proxy_count + dht_proxy_count;
        info!(
            "Privacy routing enabled with {} trusted proxy nodes (mode: {:?})",
            trusted_count,
            mode
        );

        // Send event to notify about privacy routing status
        let _ = self.cmd_tx.send(DhtCommand::SendMessageToPeer {
            target_peer_id: self.peer_id.parse().map_err(|e| format!("Invalid peer ID: {}", e))?,
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

        info!("Privacy routing disabled - reverting to direct connections");

        // Send event to notify about privacy routing status
        let _ = self.cmd_tx.send(DhtCommand::SendMessageToPeer {
            target_peer_id: self.peer_id.parse().map_err(|e| format!("Invalid peer ID: {}", e))?,
            message: serde_json::json!({
                "type": "privacy_routing_disabled"
            }),
        });

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

async fn record_identify_push_metrics(
    metrics: &Arc<Mutex<DhtMetrics>>,
    info: &identify::Info,
) {
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
            Some(256),  // chunk_size_kb
            Some(1024), // cache_size_mb
            false,      // enable_autorelay
            Vec::new(), // preferred_relays
            false,      // enable_relay_server
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
