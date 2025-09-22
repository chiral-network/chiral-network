// Real DHT implementation with channel-based communication for thread safety
use futures_util::StreamExt;
use libp2p::identify::Event as IdentifyEvent;
use libp2p::kad::Behaviour as Kademlia;
use libp2p::kad::Event as KademliaEvent;
use libp2p::kad::{Config as KademliaConfig, GetRecordOk, PutRecordOk, QueryResult};
use libp2p::mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent};
use libp2p::ping;
use libp2p::{autonat, dcutr};
use libp2p::{
    identify, identity,
    kad::{self, store::MemoryStore, Mode, Record},
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct DhtBehaviour {
    kademlia: Kademlia<MemoryStore>,
    identify: identify::Behaviour,
    mdns: Mdns,
    ping: ping::Behaviour,
    autonat: autonat::Behaviour,
    dcutr: dcutr::Behaviour,
}

#[derive(Debug)]
pub enum DhtCommand {
    PublishFile(FileMetadata),
    SearchFile(String),
    ConnectPeer(String),
    GetPeerCount(oneshot::Sender<usize>),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Debug, Clone, Serialize)]
pub enum DhtEvent {
    PeerDiscovered(String),
    PeerConnected(String),
    PeerDisconnected(String),
    FileDiscovered(FileMetadata),
    FileNotFound(String),
    Error(String),
}

#[derive(Debug, Clone, Default)]
struct DhtMetrics {
    last_bootstrap: Option<SystemTime>,
    last_success: Option<SystemTime>,
    last_error_at: Option<SystemTime>,
    last_error: Option<String>,
    bootstrap_failures: u64,
    listen_addrs: Vec<String>,
    nat_status: Option<String>,
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
    pub nat_status: Option<String>,
}

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
            nat_status: metrics.nat_status,
        }
    }
}

impl DhtMetrics {
    fn record_listen_addr(&mut self, addr: &Multiaddr) {
        let addr_str = addr.to_string();
        if !self.listen_addrs.iter().any(|existing| existing == &addr_str) {
            self.listen_addrs.push(addr_str);
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
) {
    // Periodic bootstrap interval
    let mut bootstrap_interval = tokio::time::interval(Duration::from_secs(30));
    let mut shutdown_ack: Option<oneshot::Sender<()>> = None;

    'outer: loop {
        tokio::select! {
            _ = bootstrap_interval.tick() => {
                // Periodically bootstrap to maintain connections
                let _ = swarm.behaviour_mut().kademlia.bootstrap();
                if let Ok(mut m) = metrics.try_lock() {
                    m.last_bootstrap = Some(SystemTime::now());
                }
                debug!("Performing periodic Kademlia bootstrap");
            }

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
                    Some(DhtCommand::SearchFile(file_hash)) => {
                        let key = kad::RecordKey::new(&file_hash.as_bytes());
                        let _query_id = swarm.behaviour_mut().kademlia.get_record(key);
                        info!("Searching for file: {}", file_hash);
                    }
                    Some(DhtCommand::ConnectPeer(addr)) => {
                        info!("Attempting to connect to: {}", addr);
                        if let Ok(multiaddr) = addr.parse::<Multiaddr>() {
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
                    None => {
                        info!("DHT command channel closed; shutting down node task");
                        break 'outer;
                    }
                }
            }

            event = swarm.next() => if let Some(event) = event {
                match event {
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Kademlia(kad_event)) => {
                        handle_kademlia_event(kad_event, &event_tx).await;
                    }
            SwarmEvent::Behaviour(DhtBehaviourEvent::Identify(identify_event)) => {
                        handle_identify_event(identify_event, &mut swarm, &event_tx).await;
                    }
            SwarmEvent::Behaviour(DhtBehaviourEvent::Ping(_)) => {}
            SwarmEvent::Behaviour(DhtBehaviourEvent::Autonat(ev)) => {
                if let Ok(mut m) = metrics.try_lock() {
                    m.nat_status = Some(format!("{:?}", ev));
                }
            }
            SwarmEvent::Behaviour(DhtBehaviourEvent::Dcutr(_)) => {}
                    SwarmEvent::Behaviour(DhtBehaviourEvent::Mdns(mdns_event)) => {
                        handle_mdns_event(mdns_event, &mut swarm, &event_tx).await;
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        info!("✅ CONNECTION ESTABLISHED with peer: {}", peer_id);
                        info!("   Endpoint: {:?}", endpoint);

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
                        let _ = event_tx.send(DhtEvent::PeerConnected(peer_id.to_string())).await;
                    }
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        warn!("❌ DISCONNECTED from peer: {}", peer_id);
                        warn!("   Cause: {:?}", cause);
                        let peers_count = {
                            let mut peers = connected_peers.lock().await;
                            peers.remove(&peer_id);
                            peers.len()
                        };
                        info!("   Remaining connected peers: {}", peers_count);
                        let _ = event_tx.send(DhtEvent::PeerDisconnected(peer_id.to_string())).await;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("📡 Now listening on: {}", address);
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
                        } else {
                            error!("❌ Outgoing connection error to unknown peer: {}", error);
                        }
                        let _ = event_tx.send(DhtEvent::Error(format!("Connection failed: {}", error))).await;
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

async fn handle_kademlia_event(event: KademliaEvent, event_tx: &mpsc::Sender<DhtEvent>) {
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
                            let _ = event_tx.send(DhtEvent::FileDiscovered(metadata)).await;
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
                        let _ = event_tx.send(DhtEvent::FileNotFound(file_hash)).await;
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
            for addr in info.listen_addrs {
                swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
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
                swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, multiaddr);
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

// Public API for the DHT
pub struct DhtService {
    cmd_tx: mpsc::Sender<DhtCommand>,
    event_rx: Arc<Mutex<mpsc::Receiver<DhtEvent>>>,
    peer_id: String,
    connected_peers: Arc<Mutex<HashSet<PeerId>>>,
    metrics: Arc<Mutex<DhtMetrics>>,
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
        // Replication factor of 20 (as per spec table)
        if let Some(nz) = std::num::NonZeroUsize::new(20) {
            kad_cfg.set_replication_factor(nz);
        }
        let mut kademlia = Kademlia::with_config(local_peer_id, store, kad_cfg);

        // Set Kademlia to server mode to accept incoming connections
        kademlia.set_mode(Some(Mode::Server));

        // Create identify behaviour
        let identify = identify::Behaviour::new(identify::Config::new(
            "/chiral/1.0.0".to_string(),
            local_key.public(),
        ));

        // mDNS for local peer discovery
        let mdns = Mdns::new(Default::default(), local_peer_id)?;

        // Build additional behaviours for NAT traversal
        let ping_behaviour = ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(15)));
        let autonat_behaviour = autonat::Behaviour::new(
            local_peer_id,
            autonat::Config::default(),
        );
        // DCUtR enables direct connection upgrades (benefits from relays when available)
        let dcutr_behaviour = dcutr::Behaviour::new(local_peer_id);

        let behaviour = DhtBehaviour {
            kademlia,
            identify,
            mdns,
            ping: ping_behaviour,
            autonat: autonat_behaviour,
            dcutr: dcutr_behaviour,
        };

        // Create the swarm
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )?
            .with_behaviour(|_| behaviour)?
            .with_swarm_config(
                |c| c.with_idle_connection_timeout(Duration::from_secs(300)), // 5 minutes
            )
            .build();

        // Listen on TCP and also enable relay and QUIC if needed in future
        let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
        swarm.listen_on(listen_addr.clone())?;
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

        // Spawn the DHT node task
        tokio::spawn(run_dht_node(
            swarm,
            local_peer_id,
            cmd_rx,
            event_tx,
            connected_peers.clone(),
            metrics.clone(),
        ));

        Ok(DhtService {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            peer_id: peer_id_str,
            connected_peers,
            metrics,
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

    pub async fn search_file(&self, file_hash: String) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::SearchFile(file_hash))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_file(&self, file_hash: String) -> Result<(), String> {
        self.search_file(file_hash).await
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
        metrics
            .record_listen_addr(&"/ip4/127.0.0.1/tcp/4001".parse::<Multiaddr>().unwrap());
        metrics
            .record_listen_addr(&"/ip4/0.0.0.0/tcp/4001".parse::<Multiaddr>().unwrap());
        // Duplicate should be ignored
        metrics
            .record_listen_addr(&"/ip4/127.0.0.1/tcp/4001".parse::<Multiaddr>().unwrap());

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
