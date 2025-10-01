use async_trait::async_trait;
use futures::future::{BoxFuture, FutureExt};
use futures::io::{AsyncRead as FAsyncRead, AsyncWrite as FAsyncWrite};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use futures_util::StreamExt;
use tokio_util::compat::TokioAsyncReadCompatExt;

use blockstore::{
    block::{Block, CidError},
    Blockstore, InMemoryBlockstore,
};
use cid::Cid;
use libp2p::multiaddr::Protocol;
use multihash_codetable::{Code, MultihashDigest};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, warn};

use crate::peer_selection::{PeerMetrics, PeerSelectionService, SelectionStrategy};
use crate::webrtc_service::{FileChunk, get_webrtc_service};
use std::io::{self};
use tokio_socks::tcp::Socks5Stream;

use std::pin::Pin;
use std::task::{Context, Poll};

// Import the missing types
use std::error::Error;
use crate::file_transfer::FileTransferService;

// Trait alias to abstract over async I/O types used by proxy transport
pub trait AsyncIo: FAsyncRead + FAsyncWrite + Unpin + Send {}
impl<T: FAsyncRead + FAsyncWrite + Unpin + Send> AsyncIo for T {}

use libp2p::core::upgrade::Version;
use libp2p::{
    autonat::v2,
    core::{
        muxing::StreamMuxerBox,
        // FIXED E0432: ListenerEvent is removed, only import what is available.
        transport::{Boxed, DialOpts, ListenerId, Transport, TransportError, TransportEvent},
    },
    identify::{self, Event as IdentifyEvent},
    identity,
    kad::{
        self, store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig,
        Event as KademliaEvent, GetRecordOk, Mode, PutRecordOk, QueryResult, Record,
    },
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    noise,
    ping::{self, Behaviour as Ping, Event as PingEvent},
    request_response as rr,
    swarm::{behaviour::toggle, NetworkBehaviour, SwarmEvent},
    tcp, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use rand::rngs::OsRng;
const EXPECTED_PROTOCOL_VERSION: &str = "/chiral/1.0.0";
const MAX_MULTIHASH_LENGHT: usize = 64;
const RAW_CODEC: u64 = 0x55;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    /// The Merkle root of the file's chunks, which serves as its unique identifier.
    pub file_hash: String, // This is the Merkle Root
    pub file_name: String,
    pub file_size: u64,
    pub seeders: Vec<String>,
    pub created_at: u64,
    pub mime_type: Option<String>,
    /// Whether the file is encrypted
    pub is_encrypted: bool,
    /// The encryption method used (e.g., "AES-256-GCM")
    pub encryption_method: Option<String>,
    /// Fingerprint of the encryption key for identification
    pub key_fingerprint: Option<String>,
    // --- VERSIONING FIELDS ---
    pub version: Option<u32>,
    pub parent_hash: Option<String>,
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
}

#[derive(Debug)]
pub enum DhtCommand {
    PublishFile(FileMetadata),
    SearchFile(String),
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
}

#[derive(Debug, Clone, Serialize)]
pub enum DhtEvent {
    PeerDiscovered(String),
    PeerConnected(String),    // Replaced by ProxyStatus
    PeerDisconnected(String), // Replaced by ProxyStatus
    FileDiscovered(FileMetadata),
    FileNotFound(String),
    FileDownloaded { file_hash: String },
    Error(String),
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

async fn is_proxy_peer(
    id: &PeerId,
    proxy_targets: &Arc<Mutex<HashSet<PeerId>>>,
    proxy_capable: &Arc<Mutex<HashSet<PeerId>>>,
) -> bool {
    let t = proxy_targets.lock().await;
    if t.contains(id) {
        return true;
    }
    drop(t);
    let c = proxy_capable.lock().await;
    c.contains(id)
}

async fn run_dht_node(
    mut swarm: Swarm<DhtBehaviour>,
    peer_id: PeerId,
    mut cmd_rx: mpsc::Receiver<DhtCommand>,
    event_tx: mpsc::Sender<DhtEvent>,
    connected_peers: Arc<Mutex<HashSet<PeerId>>>,
    metrics: Arc<Mutex<DhtMetrics>>,
    pending_echo: Arc<
        Mutex<HashMap<rr::OutboundRequestId, oneshot::Sender<Result<Vec<u8>, String>>>>,
    >,
    pending_searches: Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    proxy_targets: Arc<Mutex<HashSet<PeerId>>>,
    proxy_capable: Arc<Mutex<HashSet<PeerId>>>,
    peer_selection: Arc<Mutex<PeerSelectionService>>,
    received_chunks: Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: Option<Arc<FileTransferService>>,
    pending_webrtc_offers: Arc<Mutex<HashMap<rr::OutboundRequestId, oneshot::Sender<Result<WebRTCAnswerResponse, String>>>>>,
    is_bootstrap: bool,
) {
    // Periodic bootstrap interval
    let mut shutdown_ack: Option<oneshot::Sender<()>> = None;
    let mut ping_failures: HashMap<PeerId, u8> = HashMap::new();

    'outer: loop {
        tokio::select! {


            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(DhtCommand::Shutdown(ack)) => {
                        info!("Received shutdown signal for DHT node");
                        shutdown_ack = Some(ack);
                        break 'outer;
                    }
                    Some(DhtCommand::PublishFile(metadata)) => {
                        let key = kad::RecordKey::new(&metadata.file_hash.as_bytes());
                        match serde_json::to_vec(&metadata) {
                            Ok(value) => {
                                let record = Record {
                                    key,
                                    value,
                                    publisher: Some(peer_id),
                                    expires: None,
                                };

                                match swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One) {
                                    Ok(_) => {
                                        info!("Published file metadata: {}", metadata.file_hash);
                                    }
                                    Err(e) => {
                                        error!("Failed to publish file metadata {}: {}", metadata.file_hash, e);
                                        let _ = event_tx.send(DhtEvent::Error(format!("Failed to publish: {}", e))).await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to serialize file metadata {}: {}", metadata.file_hash, e);
                                let _ = event_tx.send(DhtEvent::Error(format!("Failed to serialize metadata: {}", e))).await;
                            }
                        }
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
                            if let Some(p2p) = multiaddr.iter().find_map(|p| {
                                if let libp2p::multiaddr::Protocol::P2p(peer) = p { Some(peer) } else { None }
                            }) {
                                proxy_targets.lock().await.insert(PeerId::from(p2p));
                            }
                            match swarm.dial(multiaddr.clone()) {
                                Ok(_) => {
                                    info!("✓ Initiated connection to: {}", addr);
                                    info!("  Multiaddr: {}", multiaddr);
                                    info!("  Waiting for ConnectionEstablished event...");
                                }
                                Err(e) => {
                                    error!("✗ Failed to dial {}: {}", addr, e);
                                    let _ = event_tx.send(DhtEvent::Error(format!("Failed to connect: {}", e))).await;
                                }
                            }
                        } else {
                            error!("✗ Invalid multiaddr format: {}", addr);
                            let _ = event_tx.send(DhtEvent::Error(format!("Invalid address: {}", addr))).await;
                        }
                    }
                    Some(DhtCommand::DisconnectPeer(peer_id)) => {
                        info!("Requesting disconnect from peer: {}", peer_id);
                        let was_proxy = is_proxy_peer(&peer_id, &proxy_targets, &proxy_capable).await;
                        let peer_id_str = peer_id.to_string();

                        match swarm.disconnect_peer_id(peer_id.clone()) {
                            Ok(()) => {
                                if was_proxy {
                                    proxy_targets.lock().await.remove(&peer_id);
                                    proxy_capable.lock().await.remove(&peer_id);

                                    let _ = event_tx
                                        .send(DhtEvent::ProxyStatus {
                                            id: peer_id_str,
                                            address: String::new(),
                                            status: "offline".to_string(),
                                            latency_ms: None,
                                            error: None,
                                        })
                                        .await;
                                }
                            }
                            Err(e) => {
                                // Use Debug formatting (`{:?}`) because the error type is `()`, not `Display`.
                                error!("Failed to disconnect from {}: {:?}", peer_id, e);
                                let _ = event_tx
                                    .send(DhtEvent::Error(format!(
                                        "Failed to disconnect from {}: {:?}",
                                        peer_id, e
                                    )))
                                    .await;
                            }
                        }
                    }

                    Some(DhtCommand::GetPeerCount(tx)) => {
                        let count = connected_peers.lock().await.len();
                        let _ = tx.send(count);
                    }
                    Some(DhtCommand::Echo { peer, payload, tx }) => {
                        let id = swarm.behaviour_mut().proxy_rr.send_request(&peer, EchoRequest(payload));
                        pending_echo.lock().await.insert(id, tx);
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
                        handle_identify_event(identify_event, &mut swarm, &event_tx, metrics.clone()).await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Mdns(mdns_event)) => {
                        if !is_bootstrap{
                            handle_mdns_event(mdns_event, &mut swarm, &event_tx).await;
                        }
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Bitswap(bitswap)) => match bitswap {
                        beetswap::Event::GetQueryResponse { query_id, data } => {
                            // Handle successful Bitswap response
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

                                let show_as_proxy = {
                                    let t = proxy_targets.lock().await;
                                    let c = proxy_capable.lock().await;
                                    t.contains(&peer) || c.contains(&peer)
                                };

                                if is_connected && show_as_proxy {
                                    let _ = event_tx
                                        .send(DhtEvent::PeerRtt {
                                            peer: peer.to_string(),
                                            rtt_ms,
                                        })
                                        .await;

                                        ping_failures.remove(&peer);
                                } else {
                                    debug!("skip rtt update for non-proxy/offline peer {}", peer);
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
                    SwarmEvent::ExternalAddrConfirmed { address, .. } => {
                        handle_external_addr_confirmed(&address, &metrics, &event_tx).await;
                    }
                    SwarmEvent::ExternalAddrExpired { address, .. } => {
                        handle_external_addr_expired(&address, &metrics, &event_tx).await;
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

                        if is_proxy_peer(&peer_id, &proxy_targets, &proxy_capable).await {
                            let remote_addr_str = endpoint.get_remote_address().to_string();
                            let _ = event_tx.send(DhtEvent::ProxyStatus {
                                id: peer_id.to_string(),
                                address: remote_addr_str,
                                status: "online".to_string(),
                                latency_ms: None,
                                error: None,
                            }).await;
                        } else {
                            debug!("connection is non-proxy peer; skip ProxyStatus emit: {}", peer_id);
                        }

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

                        if is_proxy_peer(&peer_id, &proxy_targets, &proxy_capable).await {
                            let _ = event_tx.send(DhtEvent::ProxyStatus {
                                id: peer_id.to_string(),
                                address: "".to_string(),
                                status: "offline".to_string(),
                                latency_ms: None,
                                error: cause.as_ref().map(|c| c.to_string()),
                            }).await;

                            {
                                let mut peers = connected_peers.lock().await;
                                peers.remove(&peer_id);
                            }

                            {
                                let mut t = proxy_targets.lock().await;
                                t.remove(&peer_id);
                            }
                            {
                                let mut c = proxy_capable.lock().await;
                                c.remove(&peer_id);
                            }

                            info!("Disconnected from {}, cleaned up proxy sets", peer_id);
                        } else {
                            debug!("non-proxy peer closed; skip ProxyStatus: {}", peer_id);
                        }

                        let peers_count = {
                            let mut peers = connected_peers.lock().await;
                            peers.remove(&peer_id);
                            peers.len()
                        };
                        info!("   Remaining connected peers: {}", peers_count);
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("📡 Now listening on: {}", address);
                        if let Ok(mut m) = metrics.try_lock() {
                            m.record_listen_addr(&address);
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        if let Some(pid) = peer_id {
                            if is_proxy_peer(&pid, &proxy_targets, &proxy_capable).await {
                                let _ = event_tx.send(DhtEvent::ProxyStatus {
                                    id: pid.to_string(),
                                    address: "".into(),
                                    status: "offline".into(),
                                    latency_ms: None,
                                    error: Some(error.to_string()),
                                }).await;
                            }
                        }
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
                                    proxy_capable.lock().await.insert(peer);

                                    // 1) Notify UI of peer status
                                    let _ = event_tx.send(DhtEvent::ProxyStatus {
                                        id: peer.to_string(),
                                        address: String::new(),
                                        status: "online".into(),
                                        latency_ms: None, error: None,
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
                                    proxy_capable.lock().await.insert(peer);
                                    let _ = event_tx.send(DhtEvent::ProxyStatus {
                                        id: peer.to_string(),
                                        address: String::new(),
                                        status: "online".into(),
                                        latency_ms: None, error: None,
                                    }).await;

                                    if let Some(tx) = pending_echo.lock().await.remove(&request_id) {
                                        let EchoResponse(data) = response;
                                        let _ = tx.send(Ok(data));
                                    }
                                }
                            },

                            RREvent::OutboundFailure { request_id, error, .. } => {
                                if matches!(error, libp2p::request_response::OutboundFailure::UnsupportedProtocols) {
                                    // Optional: negative cache for capability
                                }
                                if let Some(tx) = pending_echo.lock().await.remove(&request_id) {
                                    let _ = tx.send(Err(format!("outbound failure: {error:?}")));
                                }
                            }

                            RREvent::InboundFailure { error, .. } => {
                                warn!("inbound failure: {error:?}");
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
                        // Try to parse file metadata from record value
                        if let Ok(metadata) =
                            serde_json::from_slice::<FileMetadata>(&peer_record.record.value)
                        {
                            let notify_metadata = metadata.clone();
                            let file_hash = notify_metadata.file_hash.clone();
                            let _ = event_tx.send(DhtEvent::FileDiscovered(metadata)).await;
                            notify_pending_searches(
                                pending_searches,
                                &file_hash,
                                SearchResponse::Found(notify_metadata),
                            )
                            .await;
                        } else {
                            debug!("Received non-file metadata record");
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
) {
    match event {
        IdentifyEvent::Received { peer_id, info, .. } => {
            info!("Identified peer {}: {:?}", peer_id, info.protocol_version);
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
                    if not_loopback(&addr) {
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
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

async fn handle_external_addr_confirmed(
    addr: &Multiaddr,
    metrics: &Arc<Mutex<DhtMetrics>>,
    event_tx: &mpsc::Sender<DhtEvent>,
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

    if !nat_enabled {
        return;
    }

    let _ = event_tx
        .send(DhtEvent::NatStatus {
            state,
            confidence,
            last_error,
            summary,
        })
        .await;
}

async fn handle_external_addr_expired(
    addr: &Multiaddr,
    metrics: &Arc<Mutex<DhtMetrics>>,
    event_tx: &mpsc::Sender<DhtEvent>,
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

        if !nat_enabled {
            return;
        }

        let _ = event_tx
            .send(DhtEvent::NatStatus {
                state,
                confidence,
                last_error,
                summary,
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
pub fn build_custom_transport(
    keypair: identity::Keypair,
    proxy_address: Option<String>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>, Box<dyn Error>> {
    let noise_keys = noise::Config::new(&keypair)?;
    let yamux_config = libp2p::yamux::Config::default();

    // CORRECTED: The full transport stack is now built inside each branch
    // to ensure the final types are identical.
    if let Some(proxy) = proxy_address {
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

        Ok(socks5_transport
            .upgrade(Version::V1)
            .authenticate(noise_keys)
            .multiplex(yamux_config)
            .timeout(Duration::from_secs(30))
            .boxed())
    } else {
        info!("Direct P2P connection mode.");
        let direct_tcp = tcp::tokio::Transport::new(tcp::Config::default())
            .map(|s, _| Box::new(s.0.compat()) as Box<dyn AsyncIo>);

        Ok(direct_tcp
            .upgrade(Version::V1)
            .authenticate(noise_keys)
            .multiplex(yamux_config)
            .timeout(Duration::from_secs(30))
            .boxed())
    }
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
            .send(DhtCommand::SendWebRTCOffer { peer: peer_id, offer_request, sender: tx })
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
    pending_echo:
        Arc<Mutex<HashMap<rr::OutboundRequestId, oneshot::Sender<Result<Vec<u8>, String>>>>>,
    pending_searches: Arc<Mutex<HashMap<String, Vec<PendingSearch>>>>,
    search_counter: Arc<AtomicU64>,
    proxy_targets: Arc<Mutex<HashSet<PeerId>>>,
    proxy_capable: Arc<Mutex<HashSet<PeerId>>>,
    peer_selection: Arc<Mutex<PeerSelectionService>>,
    file_metadata_cache: Arc<Mutex<HashMap<String, FileMetadata>>>,
    received_chunks: Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: Option<Arc<FileTransferService>>,
    pending_webrtc_offers: Arc<Mutex<HashMap<rr::OutboundRequestId, oneshot::Sender<Result<WebRTCAnswerResponse, String>>>>>,
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
    ) -> Result<Self, Box<dyn Error>> {
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
        let bootstrap_interval = Duration::from_secs(30);
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

        let webrtc_protocols =
            std::iter::once(("/chiral/webrtc-signaling/1.0.0".to_string(), rr::ProtocolSupport::Full));
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
        let behaviour = DhtBehaviour {
            kademlia,
            identify,
            mdns,
            bitswap,
            ping: Ping::new(ping::Config::new()),
            proxy_rr,
            webrtc_signaling_rr,
            autonat_client: toggle::Toggle::from(autonat_client_behaviour),
            autonat_server: toggle::Toggle::from(autonat_server_behaviour),
        };

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

        // Use the new SOCKS5-aware transport builder
        let transport = build_custom_transport(local_key.clone(), proxy_address)?;

        // Create the swarm
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_other_transport(|_| Ok(transport))
            .expect("Failed to create libp2p transport")
            .with_behaviour(|_| behaviour)?
            .with_swarm_config(
                |c| c.with_idle_connection_timeout(Duration::from_secs(300)), // 5 minutes
            )
            .build();

        // Listen on the specified port
        let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
        swarm.listen_on(listen_addr)?;
        info!("DHT listening on port: {}", port);

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
        let proxy_targets = Arc::new(Mutex::new(HashSet::new()));
        let proxy_capable = Arc::new(Mutex::new(HashSet::new()));
        let peer_selection = Arc::new(Mutex::new(PeerSelectionService::new()));
        let pending_webrtc_offers = Arc::new(Mutex::new(HashMap::new()));

        {
            let mut guard = metrics.lock().await;
            guard.autonat_enabled = enable_autonat;
        }

        // Spawn the DHT node task
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
            proxy_targets.clone(),
            proxy_capable.clone(),
            peer_selection.clone(),
            received_chunks_clone.clone(),
            file_transfer_service.clone(),
            pending_webrtc_offers.clone(),
            is_bootstrap,
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
            proxy_targets,
            proxy_capable,
            peer_selection,
            file_metadata_cache: Arc::new(Mutex::new(HashMap::new())),
            received_chunks: received_chunks_clone,
            file_transfer_service,
            pending_webrtc_offers,
        })
    }

    pub async fn run(&self) {
        // The node is already running in a spawned task
        info!("DHT node is running");
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
            .filter(|m| m.file_name == file_name)
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
        created_at: u64,
        mime_type: Option<String>,
        is_encrypted: bool,
        encryption_method: Option<String>,
        key_fingerprint: Option<String>,
    ) -> Result<FileMetadata, String> {
        let latest = self
            .get_latest_version_by_file_name(file_name.clone())
            .await?;
        let (version, parent_hash) = match latest {
            Some(ref prev) => (
                prev.version.map(|v| v + 1).unwrap_or(2),
                Some(prev.file_hash.clone()),
            ),
            None => (1, None),
        };
        Ok(FileMetadata {
            file_hash,
            file_name,
            file_size,
            seeders: vec![],
            created_at,
            mime_type,
            is_encrypted,
            encryption_method,
            key_fingerprint,
            version: Some(version),
            parent_hash,
        })
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

    pub async fn search_metadata(
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
        connected_peers.iter().map(|peer_id| peer_id.to_string()).collect()
    }

    pub async fn echo(&self, peer_id: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let target_peer_id: PeerId = peer_id.parse()
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

        rx.await.map_err(|e| format!("Echo response error: {}", e))?
    }

    pub async fn send_message_to_peer(
        &self,
        peer_id: &str,
        message: serde_json::Value,
    ) -> Result<(), String> {
        let target_peer_id: PeerId = peer_id.parse()
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

    pub async fn shutdown(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        if self.cmd_tx.send(DhtCommand::Shutdown(tx)).await.is_err() {
            return Ok(());
        }

        rx.await.map_err(|e| e.to_string())?;

        Ok(())
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
                    let file_chunks = chunks_map.entry(chunk.file_hash.clone()).or_insert_with(HashMap::new);
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
                    Self::assemble_file_from_chunks(&chunk.file_hash, received_chunks, file_transfer_service, event_tx).await;
                }

                let _ = event_tx.send(DhtEvent::BitswapDataReceived {
                    query_id: format!("{:?}", query_id),
                    data: data.to_vec(),
                }).await;
            }
            Err(e) => {
                warn!("Failed to parse Bitswap data as FileChunk: {}", e);
                // Emit raw data event for debugging
                let _ = event_tx.send(DhtEvent::BitswapDataReceived {
                    query_id: format!("{:?}", query_id),
                    data: data.to_vec(),
                }).await;
            }
        }
    }

    /// Get seeders for a specific file (searches DHT for providers)
    async fn get_seeders_for_file(&self, file_hash: &str) -> Vec<String> {
        // Implement real DHT provider record queries
        // Send command to DHT task to query provider records for this file
        let (tx, rx) = oneshot::channel();

        if let Err(e) = self.cmd_tx.send(DhtCommand::GetProviders {
            file_hash: file_hash.to_string(),
            sender: tx,
        }).await {
            warn!("Failed to send GetProviders command: {}", e);
            return Vec::new();
        }

    // Wait for response with timeout
    match tokio::time::timeout(Duration::from_secs(5), rx).await {
        Ok(Ok(Ok(providers))) => {
            info!("Found {} providers for file: {}", providers.len(), file_hash);
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
        let mut sorted_chunks: Vec<FileChunk> = file_chunks.drain().map(|(_, chunk)| chunk).collect();
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
        file_transfer_service.store_file_data(file_hash.to_string(), file_name, file_data);

        info!("Successfully assembled file {} from {} chunks", file_hash, chunk_count);

        let _ = event_tx.send(DhtEvent::FileDownloaded {
            file_hash: file_hash.to_string(),
        }).await;
    }
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
                let file_chunks = chunks_map.entry(chunk.file_hash.clone()).or_insert_with(HashMap::new);
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
                assemble_file_from_chunks(&chunk.file_hash, received_chunks, file_transfer_service, event_tx).await;
            }

            let _ = event_tx.send(DhtEvent::BitswapDataReceived {
                query_id: format!("{:?}", query_id),
                data: data.to_vec(),
            }).await;
        }
        Err(e) => {
            warn!("Failed to parse Bitswap data as FileChunk: {}", e);
            // Emit raw data event for debugging
            let _ = event_tx.send(DhtEvent::BitswapDataReceived {
                query_id: format!("{:?}", query_id),
                data: data.to_vec(),
            }).await;
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
        let mut sorted_chunks: Vec<FileChunk> = file_chunks.drain().map(|(_, chunk)| chunk).collect();
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
        file_transfer_service.store_file_data(file_hash.to_string(), file_name, file_data);

        info!("Successfully assembled file {} from {} chunks", file_hash, chunk_count);

        let _ = event_tx.send(DhtEvent::FileDownloaded {
            file_hash: file_hash.to_string(),
        }).await;
    }
}

fn not_loopback(ip: &Multiaddr) -> bool {
    if let Some(ip) = multiaddr_to_ip(ip) {
        ip.is_loopback()
    } else {
        false
    }
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

struct StringBlock(pub String);

impl Block<64> for StringBlock {
    fn cid(&self) -> Result<Cid, CidError> {
        let hash = Code::Sha2_256.digest(self.0.as_ref());
        Ok(Cid::new_v1(RAW_CODEC, hash))
    }

    fn data(&self) -> &[u8] {
        self.0.as_ref()
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
            assert!(guard
                .listen_addrs
                .contains(&listen_addr.to_string()));
            assert!(guard
                .listen_addrs
                .contains(&secondary_addr.to_string()));
        }

        record_identify_push_metrics(&metrics, &info);

        let guard = metrics.lock().await;
        assert_eq!(guard.listen_addrs.len(), 2);
    }
}
