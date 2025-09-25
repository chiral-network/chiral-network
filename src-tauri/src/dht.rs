use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use futures_util::StreamExt;
use libp2p::multiaddr::Protocol;
use async_std::net::IpAddr;
use libp2p::tcp;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, warn};

use libp2p::{
    identify::{self, Event as IdentifyEvent},
    identity,
    kad::{
        self, store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig,
        Event as KademliaEvent, GetRecordOk, Mode, PutRecordOk, QueryResult, Record,
    },
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    ping::{self, Behaviour as Ping, Event as PingEvent},
    request_response as rr,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, warn};
const EXPECTED_PROTOCOL_VERSION: &str = "/chiral/1.0.0";
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
}

#[derive(NetworkBehaviour)]
struct DhtBehaviour {
    kademlia: Kademlia<MemoryStore>,
    identify: identify::Behaviour,
    mdns: Mdns,
    ping: ping::Behaviour,
    proxy_rr: rr::Behaviour<ProxyCodec>,
}

#[derive(Debug)]
pub enum DhtCommand {
    PublishFile(FileMetadata),
    SearchFile(String),
    ConnectPeer(String),
    GetPeerCount(oneshot::Sender<usize>),
    Echo {
        peer: PeerId,
        payload: Vec<u8>,
        tx: oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Shutdown(oneshot::Sender<()>),
    StopPublish(String),
}

#[derive(Debug, Clone, Serialize)]
pub enum DhtEvent {
    PeerDiscovered(String),
    PeerConnected(String),    // Replaced by ProxyStatus
    PeerDisconnected(String), // Replaced by ProxyStatus
    FileDiscovered(FileMetadata),
    FileNotFound(String),
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
}

// ------Proxy Protocol Implementation------
#[derive(Clone, Debug, Default)]
struct ProxyCodec;

#[derive(Debug, Clone)]
struct EchoRequest(pub Vec<u8>);
#[derive(Debug, Clone)]
struct EchoResponse(pub Vec<u8>);

// 4byte LE length prefix
async fn read_framed<T: AsyncRead + Unpin + Send>(io: &mut T) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    io.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut data = vec![0u8; len];
    io.read_exact(&mut data).await?;
    Ok(data)
}
async fn write_framed<T: AsyncWrite + Unpin + Send>(
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
        T: AsyncRead + Unpin + Send,
    {
        Ok(EchoRequest(read_framed(io).await?))
    }
    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
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
        T: AsyncWrite + Unpin + Send,
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
        T: AsyncWrite + Unpin + Send,
    {
        write_framed(io, data).await
    }
}
// ------End Proxy Protocol Implementation------

impl DhtMetricsSnapshot {
    fn from(metrics: DhtMetrics, peer_count: usize) -> Self {
        fn to_secs(ts: SystemTime) -> Option<u64> {
            ts.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
        }

        DhtMetricsSnapshot {
            peer_count,
            last_bootstrap: metrics.last_bootstrap.and_then(to_secs),
            last_peer_event: metrics.last_success.and_then(to_secs),
            last_error: metrics.last_error,
            last_error_at: metrics.last_error_at.and_then(to_secs),
            bootstrap_failures: metrics.bootstrap_failures,
            listen_addrs: metrics.listen_addrs,
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
) {
    // Periodic bootstrap interval
    let mut shutdown_ack: Option<oneshot::Sender<()>> = None;

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
                    Some(DhtCommand::GetPeerCount(tx)) => {
                        let count = connected_peers.lock().await.len();
                        let _ = tx.send(count);
                    }
                    Some(DhtCommand::Echo { peer, payload, tx }) => {
                        let id = swarm.behaviour_mut().proxy_rr.send_request(&peer, EchoRequest(payload));
                        pending_echo.lock().await.insert(id, tx);
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
                        handle_identify_event(identify_event, &mut swarm, &event_tx).await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Mdns(mdns_event)) => {
                        handle_mdns_event(mdns_event, &mut swarm, &event_tx).await;
                    }
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Ping(ev)) => {
                        match ev {
                            libp2p::ping::Event { peer, result: Ok(rtt), .. } => {
                                let is_connected = connected_peers.lock().await.contains(&peer);

                                let show_as_proxy = {
                                    let t = proxy_targets.lock().await;
                                    let c = proxy_capable.lock().await;
                                    t.contains(&peer) || c.contains(&peer)
                                };

                                if is_connected && show_as_proxy {
                                    let _ = event_tx
                                        .send(DhtEvent::PeerRtt {
                                            peer: peer.to_string(),
                                            rtt_ms: rtt.as_millis() as u64,
                                        })
                                        .await;
                                } else {
                                    debug!("skip rtt update for non-proxy/offline peer {}", peer);
                                }
                            }
                            libp2p::ping::Event { peer, result: Err(libp2p::ping::Failure::Timeout), .. } => {
                                let _ = event_tx
                                    .send(DhtEvent::Error(format!("Ping timeout {}", peer)))
                                    .await;
                            }
                            libp2p::ping::Event { peer, result: Err(e), .. } => {
                                warn!("ping error with {}: {}", peer, e);
                                // If needed, also send to UI
                                // let _ = event_tx.send(DhtEvent::Error(format!("Ping error {}: {}", peer, e))).await;
                            }
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        info!("✅ CONNECTION ESTABLISHED with peer: {}", peer_id);
                        info!("   Endpoint: {:?}", endpoint);

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

async fn handle_identify_event(
    event: IdentifyEvent,
    swarm: &mut Swarm<DhtBehaviour>,
    _event_tx: &mpsc::Sender<DhtEvent>,
) {
    match event {
        IdentifyEvent::Received { peer_id, info, .. } => {
            info!("Identified peer {}: {:?}", peer_id, info.protocol_version);
            // Add identified peer to Kademlia routing table
            if info.protocol_version != EXPECTED_PROTOCOL_VERSION {
                warn!(
                    "Peer {} has a mismatched protocol version: '{}'. Expected: '{}'. Banning peer.",
                    peer_id,
                    info.protocol_version,
                    EXPECTED_PROTOCOL_VERSION
                );
            } else {
                for addr in info.listen_addrs {
                    if not_loopback(&addr){
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                    }
                }
            }
        }
        IdentifyEvent::Sent { peer_id, .. } => {
            debug!("Sent identify info to {}", peer_id);
        }
        _ => {}
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
                if not_loopback(&multiaddr){
                swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, multiaddr);
                let _ = event_tx
                    .send(DhtEvent::PeerDiscovered(peer_id.to_string()))
                    .await;
                }
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

impl DhtService {
    pub async fn echo(&self, peer_id: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let peer: PeerId = peer_id
            .parse()
            .map_err(|e| format!("invalid peer id: {e}"))?;
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::Echo { peer, payload, tx })
            .await
            .map_err(|e| format!("send echo cmd: {e}"))?;
        rx.await.map_err(|e| format!("echo await: {e}"))?
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
}

impl DhtService {
    pub async fn new(
        port: u16,
        bootstrap_nodes: Vec<String>,
        secret: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
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
        // Align with docs: shorter queries, higher replication
        kad_cfg.set_query_timeout(Duration::from_secs(10));
        let bootstrap_interval = Duration::from_secs(30);

        kad_cfg.set_periodic_bootstrap_interval(Some(bootstrap_interval));

        // Replication factor of 3 (as per spec table)
        if let Some(nz) = std::num::NonZeroUsize::new(3) {
            kad_cfg.set_replication_factor(nz);
        }
        let mut kademlia = Kademlia::with_config(local_peer_id, store, kad_cfg);

        // Set Kademlia to server mode to accept incoming connections
        kademlia.set_mode(Some(Mode::Server));

        // Create identify behaviour
        let identify = identify::Behaviour::new(identify::Config::new(
            EXPECTED_PROTOCOL_VERSION.to_string(),
            local_key.public(),
        ));

        // mDNS for local peer discovery
        let mdns = Mdns::new(Default::default(), local_peer_id)?;

        // Request-Response behaviour
        let rr_cfg = rr::Config::default();
        let protocols =
            std::iter::once(("/chiral/proxy/1.0.0".to_string(), rr::ProtocolSupport::Full));
        let proxy_rr = rr::Behaviour::new(protocols, rr_cfg);

        let behaviour = DhtBehaviour {
            kademlia,
            identify,
            mdns,
            ping: Ping::new(ping::Config::new()),
            proxy_rr,
        };
        let tcp_config = tcp::Config::default().nodelay(true);
        // Create the swarm
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp_config,
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )?
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

        // Spawn the DHT node task
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
        })
    }

    pub async fn run(&self) {
        // The node is already running in a spawned task
        info!("DHT node is running");
    }

    pub async fn publish_file(&self, metadata: FileMetadata) -> Result<(), String> {
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
}

// Not filtering out some locals because SBU peer connections are being done over local IPs
fn is_global_v4(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    
    // Check if it's a global address by excluding all local/private ranges
    !(
        // RFC1918 private ranges
        // (octets[0] == 10) ||
        // (octets[0] == 172 && (16..=31).contains(&octets[1])) ||
        // (octets[0] == 192 && octets[1] == 168) ||
        
        // Loopback (127.0.0.0/8)
        (octets[0] == 127) ||
        
        // // Link-local (169.254.0.0/16)
        // (octets[0] == 169 && octets[1] == 254) ||
        
        // // Multicast (224.0.0.0/4)
        // (octets[0] >= 224 && octets[0] <= 239) ||
        
        // // Reserved/experimental (240.0.0.0/4)
        // (octets[0] >= 240) ||
        
        // // This network (0.0.0.0/8) - though rarely seen
        // (octets[0] == 0) ||
        
        // Broadcast address
        (octets == [255, 255, 255, 255])
    )
}

fn is_global_v6(ip: std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    let seg0 = segments[0];
    
    !(
        // Loopback (::1)
        ip.is_loopback() ||
        
        // Unspecified (::)
        ip.is_unspecified() ||
        
        // Link-local (fe80::/10)
        (seg0 & 0xffc0 == 0xfe80) ||
        
        // Unique local (fc00::/7)
        (seg0 & 0xfe00 == 0xfc00) ||
        
        // Site-local (deprecated, fec0::/10)
        (seg0 & 0xffc0 == 0xfec0) ||
        
        // Multicast (ff00::/8)
        (seg0 & 0xff00 == 0xff00) ||
        
        // IPv4-mapped IPv6 addresses (::ffff:0:0/96) - check if the mapped IPv4 is local
        (ip.to_ipv4_mapped().map_or(false, |v4| !is_global_v4(v4)))
    )
}

fn not_loopback(ip: &Multiaddr) -> bool {
    if let Some(ip) = multiaddr_to_ip(ip) {
        match ip {
            IpAddr::V4(v4) => is_global_v4(v4),
            IpAddr::V6(v6) => is_global_v6(v6),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_command_stops_dht_service() {
        let service = match DhtService::new(0, Vec::new(), None).await {
            Ok(service) => service,
            Err(err) => {
                let message = err.to_string();
                let lowered = message.to_ascii_lowercase();
                if lowered.contains("permission denied") || lowered.contains("not permitted") {
                    eprintln!(
                        "skipping shutdown_command_stops_dht_service: {message} (likely sandboxed)"
                    );
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
    }

    #[test]
    fn metrics_snapshot_carries_listen_addrs() {
        let mut metrics = DhtMetrics::default();
        metrics.record_listen_addr(&"/ip4/127.0.0.1/tcp/4001".parse::<Multiaddr>().unwrap());
        metrics.record_listen_addr(&"/ip4/0.0.0.0/tcp/4001".parse::<Multiaddr>().unwrap());

        let snapshot = DhtMetricsSnapshot::from(metrics, 5);
        assert_eq!(snapshot.peer_count, 5);
        assert_eq!(snapshot.listen_addrs.len(), 2);
        assert!(snapshot
            .listen_addrs
            .contains(&"/ip4/127.0.0.1/tcp/4001".to_string()));
        assert!(snapshot
            .listen_addrs
            .contains(&"/ip4/0.0.0.0/tcp/4001".to_string()));
    }
}
