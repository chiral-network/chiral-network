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
pub use multihash_codetable::{Code, MultihashDigest};
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
const CHUNK_SIZE: usize = 256 * 1024; // 256 KiB (262144 bytes)

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    /// The CID (Content Identifier) used for retrieval from the DHT/Bitswap network.
    /// This is the root CID that points to a block containing child chunk CIDs.
    pub file_hash: String, // This is the root CID
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
    /// Fingerprint of the encryption key for identification
    pub key_fingerprint: Option<String>,
    /// The Merkle root hash for integrity verification (optional, separate from file_hash)
    pub merkle_root: Option<String>,
    // --- VERSIONING FIELDS ---
    pub version: Option<u32>,
    pub parent_hash: Option<String>,
    pub cids: Option<Vec<Cid>>, // list of CIDs for all chunks
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
    dcutr: toggle::Toggle<dcutr::Behaviour>,
}
#[derive(Debug)]
pub enum DhtCommand {
    PublishFile(FileMetadata),
    SearchFile(String),
    DownloadFile(FileMetadata),
    ConnectPeer(String),
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
}

#[derive(Debug, Clone, Serialize)]
pub enum DhtEvent {
    PeerDiscovered(String),
    PeerConnected(String),
    PeerDisconnected(String),
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
#[derive(Debug, Clone, Default)]
struct ProxyManager {
    targets: std::collections::HashSet<PeerId>,
    capable: std::collections::HashSet<PeerId>,
    online: std::collections::HashSet<PeerId>,
    relay_pending: std::collections::HashSet<PeerId>,
    relay_ready: std::collections::HashSet<PeerId>,
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
            reachability: reachability_state,
            reachability_confidence,
            last_reachability_change: last_reachability_change.and_then(to_secs),
            last_probe_at: last_probe_at.and_then(to_secs),
            last_reachability_error,
            observed_addrs,
            reachability_history: history,
            autonat_enabled,
            autorelay_enabled,
            active_relay_peer_id,
            relay_reservation_status,
            last_reservation_success: last_reservation_success.and_then(to_secs),
            last_reservation_failure: last_reservation_failure.and_then(to_secs),
            reservation_renewals,
            reservation_evictions,
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
    is_bootstrap: bool,
    enable_autorelay: bool,
    relay_candidates: HashSet<String>,
    chunk_size: usize,
) {
    // Periodic bootstrap interval
    let mut shutdown_ack: Option<oneshot::Sender<()>> = None;
    let mut ping_failures: HashMap<PeerId, u8> = HashMap::new();
    let mut queries: HashMap<beetswap::QueryId, u32> = HashMap::new();
    let mut downloaded_chunks: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut current_metadata: Option<FileMetadata> = None;

    'outer: loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(DhtCommand::Shutdown(ack)) => {
                        info!("Received shutdown signal for DHT node");
                        shutdown_ack = Some(ack);
                        break 'outer;
                    }
                    Some(DhtCommand::PublishFile(mut metadata)) => {
                        // If file_data is NOT empty (non-encrypted files or inline data),
                        // create blocks and generate a root CID
                        if !metadata.file_data.is_empty() {
                            // Store the Merkle root before processing
                            let original_merkle_root = metadata.merkle_root.clone();

                            let blocks = split_into_blocks(&metadata.file_data, chunk_size);
                            let mut block_cids = Vec::new();
                            for (idx, block) in blocks.iter().enumerate() {
                                let cid = match block.cid() {
                                    Ok(c) => c,
                                    Err(e) => {
                                        error!("failed to get cid for block: {}", e);
                                        let _ = event_tx.send(DhtEvent::Error(format!("failed to get cid for block: {}", e))).await;
                                        return;
                                    }
                                };
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

                            // Clear file data and set file hash to root CID
                            metadata.file_data.clear();
                            metadata.file_hash = root_cid.to_string();
                            // Preserve the Merkle root if it was provided
                            if original_merkle_root.is_some() {
                                metadata.merkle_root = original_merkle_root;
                            }
                            // Don't store CIDs in metadata - they're in the root block now
                            metadata.cids = None;

                            println!("Publishing file with root CID: {} (merkle_root: {:?})",
                                metadata.file_hash, metadata.merkle_root);
                        } else {
                            // File data is empty - chunks and root block are already in Bitswap
                            // (from streaming upload or pre-processed encrypted file)
                            // Use the provided file_hash (which should already be a CID)
                            println!("Publishing file with pre-computed CID: {} (merkle_root: {:?})",
                                metadata.file_hash, metadata.merkle_root);
                        }

                        // Store minimal metadata in DHT
                        let dht_metadata = serde_json::json!({
                            "file_hash": metadata.file_hash,
                            "file_name": metadata.file_name,
                            "file_size": metadata.file_size,
                            "created_at": metadata.created_at,
                            "mime_type": metadata.mime_type,
                            "is_encrypted": metadata.is_encrypted,
                            "encryption_method": metadata.encryption_method,
                            "key_fingerprint": metadata.key_fingerprint,
                            "version": metadata.version,
                            "parent_hash": metadata.parent_hash
                        });

                        let dht_record_data = match serde_json::to_vec(&dht_metadata) {
                            Ok(data) => data,
                            Err(e) => {
                                eprintln!("Failed to serialize DHT metadata: {}", e);
                                return;
                            }
                        };

                        let key = kad::RecordKey::new(&metadata.file_hash.as_bytes());
                        let record = Record {
                                    key,
                                    value: dht_record_data,
                                    publisher: Some(peer_id),
                                    expires: None,
                                };

                        match swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One){
                            Ok(query_id) => {
                                info!("started providing file: {}, query id: {:?}", metadata.file_hash, query_id);
                            }
                            Err(e) => {
                                error!("failed to start providing file {}: {}", metadata.file_hash, e);
                                let _ = event_tx.send(DhtEvent::Error(format!("failed to start providing: {}", e))).await;
                            }
                        }

                        // Register this peer as a provider for the file
                        let provider_key = kad::RecordKey::new(&metadata.file_hash.as_bytes());
                        match swarm.behaviour_mut().kademlia.start_providing(provider_key) {
                            Ok(query_id) => {
                                info!("registered as provider for file: {}, query id: {:?}", metadata.file_hash, query_id);
                            }
                            Err(e) => {
                                error!("failed to register as provider for file {}: {}", metadata.file_hash, e);
                                let _ = event_tx.send(DhtEvent::Error(format!("failed to register as provider: {}", e))).await;
                            }
                        }
                        let _ = event_tx.send(DhtEvent::PublishedFile(metadata)).await;
                    }
                    Some(DhtCommand::DownloadFile(file_metadata)) =>{
                        // currently only able to process one download at a time
                        current_metadata = Some(file_metadata.clone());

                        // Get root CID from file hash
                        let root_cid: Cid = match file_metadata.file_hash.parse() {
                            Ok(cid) => cid,
                            Err(e) => {
                                error!("Invalid root CID in file metadata: {}", e);
                                let _ = event_tx.send(DhtEvent::Error(format!("Invalid root CID: {}", e))).await;
                                return;
                            }
                        };

                        // Request the root block which contains the CIDs
                        let root_query_id = swarm.behaviour_mut().bitswap.get(&root_cid);
                        info!("Requesting root block for file: {}", file_metadata.file_hash);

                        // Store the root query ID to handle when we get the root block
                        // We'll need to modify the Bitswap handling to distinguish root blocks from data blocks
                        // For now, we'll handle this in the Bitswap event handler
                    }

                    Some(DhtCommand::StopPublish(file_hash)) => {
                        let key = kad::RecordKey::new(&file_hash);
                        // Remove the record
                        // swarm.behaviour_mut().kademlia.stop_providing(&key);
                        swarm.behaviour_mut().kademlia.remove_record(&key)
                    }
                    Some(DhtCommand::SearchFile(file_hash)) => {
                        let key = kad::RecordKey::new(&file_hash.as_bytes());
                        let _query_id = swarm.behaviour_mut().kademlia.get_record(key);
                        info!("Searching for file: {}", file_hash);
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
                                let mut mgr = proxy_mgr.lock().await;
                                mgr.set_target(peer_id);
                                let should_request = !mgr.has_relay_request(&peer_id);
                                if should_request {
                                    mgr.mark_relay_pending(peer_id);
                                }
                                drop(mgr);

                                if should_request {
                                    if let Some(relay_addr) = build_relay_listen_addr(&multiaddr) {
                                        match swarm.listen_on(relay_addr.clone()) {
                                            Ok(_) => {
                                                info!(
                                                    "Requested relay reservation via {}",
                                                    relay_addr
                                                );
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
                                        warn!(
                                            "Cannot derive relay listen address from {}",
                                            multiaddr
                                        );
                                    }
                                }
                            }

                            match swarm.dial(multiaddr.clone()) {
                                Ok(_) => {
                                    info!("Requested connection to: {}", addr);
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
                            error!("Invalid multiaddr format: {}", addr);
                            let _ = event_tx
                                .send(DhtEvent::Error(format!("Invalid address: {}", addr)))
                                .await;
                        }
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

                        // For now, return connected peers as providers
                        // In a full implementation, we'd wait for the provider query results
                        let connected_peers = connected_peers.lock().await;
                        let providers: Vec<String> = connected_peers.iter().take(3).map(|p| p.to_string()).collect();

                        // Send the response
                        let _ = sender.send(Ok(providers));
                    }
                    Some(DhtCommand::SendWebRTCOffer { peer, offer_request, sender }) => {
                        let id = swarm.behaviour_mut().webrtc_signaling_rr.send_request(&peer, offer_request);
                        pending_webrtc_offers.lock().await.insert(id, sender);
                    }
                    Some(DhtCommand::SendMessageToPeer { target_peer_id, message }) => {
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
                            &event_tx,
                            &pending_searches,
                        )
                        .await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Identify(identify_event)) => {
                        handle_identify_event(identify_event, &mut swarm, &event_tx, metrics.clone(), enable_autorelay, &relay_candidates).await;
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
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Bitswap(bitswap)) => match bitswap {
                        beetswap::Event::GetQueryResponse { query_id, data } => {
                            // Handle successful Bitswap response
                            if let Some(metadata) = &current_metadata {
                                // Check if this is the root block (contains CIDs array)
                                if let Ok(cids) = serde_json::from_slice::<Vec<Cid>>(&data) {
                                    info!("Received root block with {} CIDs", cids.len());
                                    // This is the root block containing CIDs - request all data blocks
                                    for (i, cid) in cids.iter().enumerate() {
                                        let block_query_id = swarm.behaviour_mut().bitswap.get(cid);
                                        queries.insert(block_query_id, i as u32);
                                    }
                                } else {
                                    // This is a regular data block
                                    match queries.get(&query_id) {
                                        Some(index) => {
                                            downloaded_chunks.insert(*index as usize, data.clone());
                                            queries.remove(&query_id);
                                            if queries.is_empty() {
                                                info!("all requested cids have been downloaded.");
                                                // reassemble file from downloaded chunks
                                                let mut file = Vec::new();
                                                for i in 0..=downloaded_chunks.len()-1 {
                                                    file.extend_from_slice(&downloaded_chunks.remove(&i).unwrap());
                                                }
                                                if let Some(metadata) = current_metadata.as_mut() {
                                                        metadata.file_data = file; // OK, file_data is Vec<u8>
                                                    }
                                                if let Some(metadata) = current_metadata.take() {
                                                    let _ = event_tx.send(DhtEvent::DownloadedFile(metadata)).await;
                                                }
                                                downloaded_chunks.clear();
                                                current_metadata = None;
                                            }
                                        }
                                        None => {
                                            // This might be an unexpected block - ignore for now
                                        }
                                    }
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
                        info!("✅ CONNECTION ESTABLISHED with peer: {}", peer_id);
                        info!("   Endpoint: {:?}", endpoint);

                        // Initialize peer metrics for smart selection
                        {
                            let mut selection = peer_selection.lock().await;
                            let peer_metrics = PeerMetrics::new(
                                peer_id.to_string(),
                                endpoint.get_remote_address().to_string(),
                            );
                            selection.update_peer_metrics(peer_metrics);
                        }

                        // Add peer to Kademlia routing table
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, endpoint.get_remote_address().clone());

                        let peers_count = {
                            let mut peers = connected_peers.lock().await;
                            peers.insert(peer_id);
                            peers.len()
                        };
                        if let Ok(mut m) = metrics.try_lock() {
                            m.last_success = Some(SystemTime::now());
                        }
                        info!("   Total connected peers: {}", peers_count);
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
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("📡 Now listening on: {}", address);
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
                                    let WebRTCOfferRequest { offer_sdp, file_hash, requester_peer_id } = request;
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
    event_tx: &mpsc::Sender<DhtEvent>,
    pending_searches: &Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
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
                                metadata_json.get("file_hash").and_then(|v| v.as_str()),
                                metadata_json.get("file_name").and_then(|v| v.as_str()),
                                metadata_json.get("file_size").and_then(|v| v.as_u64()),
                                metadata_json.get("created_at").and_then(|v| v.as_u64()),
                            ) {
                                let metadata = FileMetadata {
                                    file_hash: file_hash.to_string(),
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
                                    merkle_root: metadata_json
                                        .get("merkle_root")
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
                                    cids: None, // CIDs are in the root block
                                    is_root: metadata_json
                                        .get("is_root")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(true),
                                };

                                let notify_metadata = metadata.clone();
                                let file_hash = notify_metadata.file_hash.clone();
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
                _ => {}
            }
        }
        _ => {}
    }
}

fn record_identify_push_metrics(metrics: &Arc<Mutex<DhtMetrics>>, info: &identify::Info) {
    if let Ok(mut metrics_guard) = metrics.try_lock() {
        for addr in &info.listen_addrs {
            metrics_guard.record_listen_addr(addr);
        }
    }
}
async fn handle_identify_event(
    event: IdentifyEvent,
    swarm: &mut Swarm<DhtBehaviour>,
    event_tx: &mpsc::Sender<DhtEvent>,
    metrics: Arc<Mutex<DhtMetrics>>,
    enable_autorelay: bool,
    relay_candidates: &HashSet<String>,
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
                if let Ok(mut metrics_guard) = metrics.try_lock() {
                    metrics_guard.record_observed_addr(&info.observed_addr);
                }
                for addr in info.listen_addrs {
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
            }
        }
        IdentifyEvent::Pushed { peer_id, info, .. } => {
            info!(
                "Pushed identify update to {} (listen addrs: {})",
                peer_id,
                info.listen_addrs.len()
            );
            record_identify_push_metrics(&metrics, &info);
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
            for (peer_id, multiaddr) in list {
                debug!("mDNS discovered peer {} at {}", peer_id, multiaddr);
                if not_loopback(&multiaddr) {
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, multiaddr);
                }
                let _ = event_tx
                    .send(DhtEvent::PeerDiscovered(peer_id.to_string()))
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
                .timeout(Duration::from_secs(30))
                .boxed()
        }
        (None, relay_transport) => {
            info!("Direct P2P connection mode.");
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
                .timeout(Duration::from_secs(30))
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
    chunk_size: usize, // Configurable chunk size in bytes
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
        anonymous_mode: bool,
    ) -> Result<Self, Box<dyn Error>> {
        // Convert chunk size from KB to bytes
        let chunk_size = chunk_size_kb.unwrap_or(256) * 1024; // Default 256 KB
        let _cache_size = cache_size_mb.unwrap_or(1024); // Default 1024 MB

        info!(
            "DHT Configuration: chunk_size={} KB, cache_size={} MB",
            chunk_size / 1024,
            _cache_size
        );

        if anonymous_mode {
            info!(
                "Anonymous routing enabled: enforcing proxy-only egress and loopback listener"
            );
        }
        // Generate a new keypair for this node
        // Generate a keypair either from the secret or randomly
        let local_key = match secret {
            Some(secret_str) => {
                let secret_bytes = secret_str.as_bytes();
                let mut seed = [0u8; 32];
                for (i, &b) in secret_bytes.iter().take(32).enumerate() {
                    seed[i] = b;
                }
                identity::Keypair::ed25519_from_bytes(seed)?
            }
            None => identity::Keypair::generate_ed25519(),
        };
        let local_peer_id = PeerId::from(local_key.public());
        let peer_id_str = local_peer_id.to_string();

        info!("Local peer id: {}", local_peer_id);

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

        if anonymous_mode {
            kademlia.set_mode(Some(Mode::Client));
        } else {
            // Set Kademlia to server mode to accept incoming connections
            kademlia.set_mode(Some(Mode::Server));
        }

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

        let enable_autonat = if anonymous_mode {
            info!("Anonymous mode: disabling AutoNAT probes");
            false
        } else {
            enable_autonat
        };

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
            info!("AutoNAT disabled");
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

        // DCUtR requires relay to be enabled and leaks reachability, so skip in anonymous mode
        let dcutr_behaviour = if enable_autonat && !anonymous_mode {
            info!("DCUtR enabled (requires relay for hole-punching coordination)");
            Some(dcutr::Behaviour::new(local_peer_id))
        } else {
            if anonymous_mode {
                info!("Anonymous mode: DCUtR disabled to avoid direct hole-punch attempts");
            } else {
                info!("DCUtR disabled (autonat is disabled)");
            }
            None
        };
        let dcutr_toggle = toggle::Toggle::from(dcutr_behaviour);

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
            info!("AutoRelay disabled");
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

        // Listen on the specified port (loopback-only for anonymous routing)
        if anonymous_mode {
            let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse()?;
            swarm.listen_on(listen_addr.clone())?;
            info!(
                "Anonymous mode active - libp2p listening on {} (no public interfaces)",
                listen_addr
            );
        } else {
            let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
            swarm.listen_on(listen_addr.clone())?;
            info!("DHT listening on port: {}", port);
        }

        // Connect to bootstrap nodes
        info!("Bootstrap nodes to connect: {:?}", bootstrap_nodes);
        let mut successful_connections = 0;
        let total_bootstrap_nodes = bootstrap_nodes.len();
        for bootstrap_addr in &bootstrap_nodes {
            info!("Attempting to connect to bootstrap: {}", bootstrap_addr);
            if let Ok(addr) = bootstrap_addr.parse::<Multiaddr>() {
                match swarm.dial(addr.clone()) {
                    Ok(_) => {
                        info!("✓ Initiated connection to bootstrap: {}", bootstrap_addr);
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
            info!(
                "Triggered initial Kademlia bootstrap (attempted {}/{} connections)",
                successful_connections, total_bootstrap_nodes
            );
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
            chunk_size,
        })
    }

    pub async fn run(&self) {
        // The node is already running in a spawned task
        info!("DHT node is running");
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    pub async fn publish_file(&self, metadata: FileMetadata) -> Result<(), String> {
        self.file_metadata_cache
            .lock()
            .await
            .insert(metadata.file_hash.clone(), metadata.clone());
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
            .insert(metadata.file_hash.clone(), metadata.clone());
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
            version.seeders = self.get_seeders_for_file(&version.file_hash).await;
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
                Some(prev.file_hash.clone()),
                false, // not root if there was a previous version
            ),
            None => (1, None, true), // root if first version
        };
        Ok(FileMetadata {
            file_hash,
            file_name,
            file_size,
            file_data,
            seeders: vec![],
            created_at,
            mime_type,
            is_encrypted,
            encryption_method,
            key_fingerprint,
            merkle_root: None,
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
            .map_err(|e| format!("Invalid peer ID: {}", e))?;

        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::Echo {
                peer: target_peer_id,
                payload,
                tx,
            })
            .await
            .map_err(|e| format!("Failed to send echo command: {}", e))?;

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
            .map_err(|e| format!("Failed to send DHT command: {}", e))?;

        Ok(())
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
        metadata: &FileMetadata,
    ) -> Result<Vec<String>, String> {
        info!(
            "Starting peer discovery for file: {} with {} seeders",
            metadata.file_hash,
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
                    // TODO: Try to connect to this peer
                }
            } else {
                warn!("Invalid peer ID in seeders list: {}", seeder_id);
            }
        }

        // If no seeders are connected, the file is not available for download
        if available_peers.is_empty() {
            info!("No seeders are currently connected - file not available for download");
            // TODO: In the future, we could try to connect to offline seeders
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

    /// Shutdown the DHT service
    pub async fn shutdown(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::Shutdown(tx))
            .await
            .map_err(|e| format!("Failed to send shutdown command: {}", e))?;
        rx.await
            .map_err(|e| format!("Failed to receive shutdown acknowledgment: {}", e))
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
            false,
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
        service.run().await;

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
