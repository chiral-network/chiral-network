//! Bounded signed inference discovery. Kademlia supplies candidates, never trusted
//! records; manifests are merged until a fixed deadline, including after bad replies.
use crate::{
    offer_registry::{self, OfferRegistry},
    protocol::{self, InferenceOffer, NetworkIdentity, Signed},
    provider_config::DhtConfig,
};
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, StreamExt};
use libp2p::{
    connection_limits, identify, identity, kad, noise, ping, relay, request_response as rr,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, StreamProtocol, Swarm,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    io,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

const MAX_PEERS: usize = 64;
const MAX_LOCAL: usize = 64;
const PAGE: usize = 8;
// A persisted Ed25519 identity is 68 bytes; the bound only refuses a non-key file.
const MAX_IDENTITY_BYTES: u64 = 4096;
const WINDOW: Duration = Duration::from_secs(8);
// Keep the 256-connection budget, reserving 64 slots from inbound-only floods.
// Two connections per identity allow simultaneous inbound/outbound dialing.
// Pending limits bound handshakes separately; 64 outgoing allows all 32
// bootstrap peers plus discovery dials, and caps combined pending work at 128.
const MAX_PENDING_INCOMING: u32 = 64;
const MAX_PENDING_OUTGOING: u32 = 64;
const MAX_ESTABLISHED_INCOMING: u32 = 192;
const MAX_ESTABLISHED: u32 = 256;
const MAX_ESTABLISHED_PER_PEER: u32 = 2;

fn connection_limits() -> connection_limits::ConnectionLimits {
    connection_limits::ConnectionLimits::default()
        .with_max_pending_incoming(Some(MAX_PENDING_INCOMING))
        .with_max_pending_outgoing(Some(MAX_PENDING_OUTGOING))
        .with_max_established_incoming(Some(MAX_ESTABLISHED_INCOMING))
        .with_max_established_outgoing(Some(MAX_ESTABLISHED))
        .with_max_established(Some(MAX_ESTABLISHED))
        .with_max_established_per_peer(Some(MAX_ESTABLISHED_PER_PEER))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRequest {
    network: NetworkIdentity,
    model: Option<String>,
    offset: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestResponse {
    offers: Vec<Signed<InferenceOffer>>,
    next: Option<usize>,
}
#[derive(Clone, Default)]
struct ManifestCodec;
async fn read_frame<T: AsyncRead + Unpin + Send, V: serde::de::DeserializeOwned>(
    io: &mut T,
    max: usize,
) -> io::Result<V> {
    let mut length = [0u8; 4];
    io.read_exact(&mut length).await?;
    let length = u32::from_be_bytes(length) as usize;
    if length > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "manifest frame too large",
        ));
    }
    let mut bytes = vec![0; length];
    io.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
async fn write_frame<T: AsyncWrite + Unpin + Send, V: Serialize>(
    io: &mut T,
    value: V,
    max: usize,
) -> io::Result<()> {
    let bytes =
        serde_json::to_vec(&value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if bytes.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "manifest frame too large",
        ));
    }
    io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    io.write_all(&bytes).await?;
    io.close().await
}
#[async_trait::async_trait]
impl rr::Codec for ManifestCodec {
    type Protocol = StreamProtocol;
    type Request = ManifestRequest;
    type Response = ManifestResponse;
    async fn read_request<T: AsyncRead + Unpin + Send>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request> {
        read_frame(io, 2048).await
    }
    async fn read_response<T: AsyncRead + Unpin + Send>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response> {
        read_frame(io, 262_144).await
    }
    async fn write_request<T: AsyncWrite + Unpin + Send>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        value: Self::Request,
    ) -> io::Result<()> {
        write_frame(io, value, 2048).await
    }
    async fn write_response<T: AsyncWrite + Unpin + Send>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        value: Self::Response,
    ) -> io::Result<()> {
        write_frame(io, value, 262_144).await
    }
}
#[derive(NetworkBehaviour)]
struct Behaviour {
    // Admission must run before any protocol allocates a connection handler.
    limits: connection_limits::Behaviour,
    kad: kad::Behaviour<kad::store::MemoryStore>,
    manifest: rr::Behaviour<ManifestCodec>,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    relay: relay::Behaviour,
}
enum Command {
    Publish(Signed<InferenceOffer>, oneshot::Sender<Result<(), String>>),
    Withdraw(String, oneshot::Sender<()>),
    Discover(
        Option<String>,
        oneshot::Sender<Result<Vec<Signed<InferenceOffer>>, String>>,
    ),
    Status(oneshot::Sender<serde_json::Value>),
    Stop(oneshot::Sender<()>),
}
#[derive(Clone)]
pub struct DhtService {
    commands: mpsc::Sender<Command>,
    peer: String,
}
struct Search {
    query: kad::QueryId,
    model: Option<String>,
    deadline: tokio::time::Instant,
    seen: HashSet<PeerId>,
    reply: oneshot::Sender<Result<Vec<Signed<InferenceOffer>>, String>>,
    manifests: HashMap<PeerId, PeerManifest>,
}
struct Pending {
    query: kad::QueryId,
    peer: PeerId,
    offset: usize,
}
/// What one search received from one peer's manifest, page by page.
#[derive(Default)]
struct PeerManifest {
    /// IDs of the offers the registry accepted or idempotently replayed.
    received: HashSet<String>,
    /// Every revision carried by an offer on any page.
    revisions: BTreeSet<u64>,
    /// A page after the first held no offers.
    empty_later_page: bool,
    /// An offer or page was rejected, so the manifest is partial.
    partial: bool,
}
impl PeerManifest {
    /// Whether pages ending with a `next` of `null` at `last_offset` are a complete, valid
    /// manifest from one snapshot. `/1` pages carry no snapshot marker. One page is one
    /// snapshot; longer pages are only when no later page is empty and every offer carries
    /// one revision, as the native provider signs each refresh with one revision.
    fn complete(&self, last_offset: usize) -> bool {
        !self.partial
            && (last_offset == 0 || (!self.empty_later_page && self.revisions.len() <= 1))
    }
}
/// An offer this peer serves in its own manifest.
struct LocalOffer {
    id: String,
    offer: Signed<InferenceOffer>,
}
/// Local offers keyed, and therefore paged, by namespace.
type LocalManifest = BTreeMap<String, LocalOffer>;

/// Serves `offer` in place of any earlier offer of its namespace. Keying by namespace
/// rather than offer ID keeps every namespace on its page when a refresh republishes
/// it under a new revision, and so a new offer ID.
fn serve_local(local: &mut LocalManifest, id: String, offer: Signed<InferenceOffer>) {
    local.insert(
        offer_registry::namespace(&offer.payload),
        LocalOffer { id, offer },
    );
}
/// One page of this peer's manifest: unexpired offers in the model scope, in namespace order.
fn manifest_page(
    local: &LocalManifest,
    model: Option<&str>,
    offset: usize,
    now: u64,
) -> ManifestResponse {
    let all: Vec<_> = local
        .values()
        .map(|held| &held.offer)
        .filter(|o| o.payload.expires_at > now && model.is_none_or(|m| o.payload.model.id == m))
        .collect();
    let offers = all.iter().skip(offset).take(PAGE).map(|o| (*o).clone()).collect();
    let next = (all.len() > offset + PAGE).then_some(offset + PAGE);
    ManifestResponse { offers, next }
}
/// Inserts the valid offers of one page that `peer` answered for `offset`, and returns
/// the next offset to request. On the last page of a complete single-snapshot manifest,
/// withdraws that peer's cached offers in the model scope that the manifest omitted.
fn receive_page(
    registry: &OfferRegistry,
    model: Option<&str>,
    peer: &PeerId,
    offset: usize,
    response: ManifestResponse,
    manifest: &mut PeerManifest,
) -> Option<usize> {
    if response.offers.len() > PAGE {
        manifest.partial = true;
        return None;
    }
    if offset > 0 && response.offers.is_empty() {
        manifest.empty_later_page = true;
    }
    let peer_id = peer.to_string();
    for offer in response.offers {
        manifest.revisions.insert(offer.payload.revision);
        if offer.payload.peer_id != peer_id || !model.is_none_or(|m| offer.payload.model.id == m) {
            manifest.partial = true;
            continue;
        }
        match registry.insert(offer) {
            Ok(id) => {
                manifest.received.insert(id);
            }
            Err(_) => manifest.partial = true,
        }
    }
    if response.next == Some(offset + PAGE) {
        return Some(offset + PAGE);
    }
    if response.next.is_none() && manifest.complete(offset) {
        // An authenticated complete manifest can withdraw this peer's cached offers
        // only. Partial, invalid or mixed-snapshot replies never erase offers.
        for offer in registry.list() {
            if offer.payload.peer_id == peer_id && model.is_none_or(|m| offer.payload.model.id == m) {
                if let Ok(id) = protocol::offer_id(&offer.payload) {
                    if !manifest.received.contains(&id) {
                        registry.withdraw(&id);
                    }
                }
            }
        }
    }
    None
}

/// The peer identity is a private key, kept in a protected secret file. An existing file
/// is never replaced: a new peer ID is a different deployment.
fn load_identity(path: &Path) -> Result<identity::Keypair, String> {
    use std::io::{Read, Write};
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            let refuse = |reason: &str| {
                format!(
                    "persisted peer identity is not a protected secret file ({reason}); protect the \
                     existing file in place as an owner-only regular file instead of replacing or \
                     deleting it, because the peer identity must be preserved"
                )
            };
            let file = crate::wallet::open_secret_file(path).map_err(|e| refuse(&e))?;
            let mut bytes = zeroize::Zeroizing::new(Vec::new());
            file.take(MAX_IDENTITY_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| refuse("unreadable"))?;
            if bytes.len() as u64 > MAX_IDENTITY_BYTES {
                return Err("persisted peer identity exceeds its size limit".into());
            }
            identity::Keypair::from_protobuf_encoding(&bytes)
                .map_err(|e| format!("invalid persisted peer identity: {e}"))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
            if let Some(parent) = parent {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let key = identity::Keypair::generate_ed25519();
            let encoded =
                zeroize::Zeroizing::new(key.to_protobuf_encoding().map_err(|e| e.to_string())?);
            let mut file = crate::wallet::create_secret_file(path)?;
            file.write_all(&encoded)
                .and_then(|_| file.sync_all())
                .map_err(|e| format!("cannot persist peer identity: {e}"))?;
            #[cfg(unix)]
            if let Some(parent) = parent {
                std::fs::File::open(parent)
                    .and_then(|dir| dir.sync_all())
                    .map_err(|e| format!("cannot persist peer identity directory: {e}"))?;
            }
            Ok(key)
        }
        Err(e) => Err(format!("cannot inspect persisted peer identity: {e}")),
    }
}

/// A persisted peer identity, loaded under the protected-file rules of `load_identity`, before
/// any node joins the network under it. A provider derives its deployment directory from the
/// peer ID and takes that deployment's lock before `DhtService::start_as` dials or listens
/// (provider-daemon-lifecycle requirement 8).
pub struct PeerIdentity(identity::Keypair);

impl PeerIdentity {
    /// Loads the identity at `path`, or creates and persists a new one when none exists.
    pub fn load(path: &Path) -> Result<Self, String> {
        load_identity(path).map(Self)
    }
    pub fn peer_id(&self) -> String {
        self.0.public().to_peer_id().to_string()
    }
}

fn index(network: &NetworkIdentity, model: Option<&str>) -> kad::RecordKey {
    let bytes = protocol::canonical_json(&(network, model)).expect("network index serializes");
    kad::RecordKey::new(&protocol::digest(&bytes))
}
fn build_swarm(
    key: identity::Keypair,
    network: &NetworkIdentity,
) -> Result<Swarm<Behaviour>, String> {
    let peer = key.public().to_peer_id();
    let scope = protocol::hash_hex(&protocol::canonical_json(network)?);
    let mut kc = kad::Config::default();
    kc.set_protocol_names(vec![StreamProtocol::try_from_owned(format!(
        "/chiral/inference/kad/1/{scope}"
    ))
    .map_err(|e| e.to_string())?]);
    kc.set_query_timeout(WINDOW);
    kc.set_provider_record_ttl(Some(Duration::from_secs(600)));
    kc.set_provider_publication_interval(Some(Duration::from_secs(120)));
    let mut sc = kad::store::MemoryStoreConfig::default();
    sc.max_records = 0;
    sc.max_provided_keys = 128;
    sc.max_providers_per_key = MAX_PEERS;
    let mut kad =
        kad::Behaviour::with_config(peer, kad::store::MemoryStore::with_config(peer, sc), kc);
    kad.set_mode(Some(kad::Mode::Server));
    let manifest = rr::Behaviour::with_codec(
        ManifestCodec,
        [(
            StreamProtocol::new("/chiral/inference/manifest/1"),
            rr::ProtocolSupport::Full,
        )],
        rr::Config::default()
            .with_request_timeout(Duration::from_secs(4))
            .with_max_concurrent_streams(64),
    );
    let identify = identify::Behaviour::new(
        identify::Config::new(format!("/chiral/inference/id/1/{scope}"), key.public())
            .with_agent_version(crate::version::agent_version_string()),
    );
    let mut relay_config = relay::Config::default();
    relay_config.max_reservations = 128;
    relay_config.max_reservations_per_peer = 2;
    relay_config.max_circuits = 128;
    relay_config.max_circuits_per_peer = 4;
    let behaviour = Behaviour {
        limits: connection_limits::Behaviour::new(connection_limits()),
        kad,
        manifest,
        identify,
        ping: ping::Behaviour::default(),
        relay: relay::Behaviour::new(peer, relay_config),
    };
    Ok(libp2p::SwarmBuilder::with_existing_identity(key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|e| e.to_string())?
        .with_behaviour(|_| behaviour)
        .map_err(|e| e.to_string())?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(120)))
        .build())
}

impl DhtService {
    pub async fn start(
        config: DhtConfig,
        network: NetworkIdentity,
        registry: Arc<OfferRegistry>,
    ) -> Result<Self, String> {
        let identity = PeerIdentity::load(&config.identity_file)?;
        Self::start_as(identity, config, network, registry).await
    }

    /// `start` under `identity`, already loaded from `config.identity_file`: the node listens
    /// and dials its bootstrap peers only from here.
    pub async fn start_as(
        identity: PeerIdentity,
        config: DhtConfig,
        network: NetworkIdentity,
        registry: Arc<OfferRegistry>,
    ) -> Result<Self, String> {
        let key = identity.0;
        let peer = key.public().to_peer_id();
        let mut swarm = build_swarm(key, &network)?;
        swarm
            .listen_on(
                config
                    .listen
                    .parse::<Multiaddr>()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        if config.bootstrap.len() > 32 {
            return Err("too many bootstrap peers".into());
        }
        for raw in config.bootstrap {
            let mut addr: Multiaddr = raw
                .parse()
                .map_err(|e: libp2p::multiaddr::Error| e.to_string())?;
            let Some(libp2p::multiaddr::Protocol::P2p(id)) = addr.pop() else {
                return Err("bootstrap address must end in /p2p/peer-id".into());
            };
            swarm.behaviour_mut().kad.add_address(&id, addr.clone());
            swarm.add_peer_address(id, addr);
            if id != peer {
                let _ = swarm.dial(id);
            }
        }
        let _ = swarm.behaviour_mut().kad.bootstrap();
        let (commands, receiver) = mpsc::channel(64);
        let (ready_tx, ready_rx) = oneshot::channel();
        tokio::spawn(run(swarm, network, registry, receiver, ready_tx));
        tokio::time::timeout(Duration::from_secs(5), ready_rx)
            .await
            .map_err(|_| "DHT listen timeout")?
            .map_err(|_| "DHT listener stopped")??;
        Ok(Self {
            commands,
            peer: peer.to_string(),
        })
    }
    pub fn peer_id(&self) -> String {
        self.peer.clone()
    }
    pub async fn publish(&self, offer: &Signed<InferenceOffer>) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Command::Publish(offer.clone(), tx))
            .await
            .map_err(|_| "DHT stopped")?;
        rx.await.map_err(|_| "DHT stopped")?
    }
    pub async fn withdraw(&self, id: &str) {
        let (tx, rx) = oneshot::channel();
        if self
            .commands
            .send(Command::Withdraw(id.into(), tx))
            .await
            .is_ok()
        {
            let _ = rx.await;
        }
    }
    pub async fn discover(
        &self,
        model: Option<&str>,
    ) -> Result<Vec<Signed<InferenceOffer>>, String> {
        if model.is_some_and(|m| m.is_empty() || m.len() > 256) {
            return Err("invalid model selector".into());
        }
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Command::Discover(model.map(str::to_owned), tx))
            .await
            .map_err(|_| "DHT stopped")?;
        tokio::time::timeout(WINDOW + Duration::from_secs(2), rx)
            .await
            .map_err(|_| "discovery timed out")?
            .map_err(|_| "DHT stopped")?
    }
    pub async fn status(&self) -> serde_json::Value {
        let (tx, rx) = oneshot::channel();
        if self.commands.send(Command::Status(tx)).await.is_err() {
            return serde_json::json!({"running":false,"peer_id":self.peer});
        }
        rx.await.unwrap_or(serde_json::json!({"running":false}))
    }
    pub async fn stop(&self) {
        let (tx, rx) = oneshot::channel();
        if self.commands.send(Command::Stop(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }
}
fn request_peer(
    swarm: &mut Swarm<Behaviour>,
    network: &NetworkIdentity,
    search: &mut Search,
    pending: &mut HashMap<rr::OutboundRequestId, Pending>,
    peer: PeerId,
    offset: usize,
) {
    if peer == *swarm.local_peer_id() || offset >= MAX_LOCAL {
        return;
    }
    if offset == 0 && (search.seen.len() >= MAX_PEERS || !search.seen.insert(peer)) {
        return;
    }
    let id = swarm.behaviour_mut().manifest.send_request(
        &peer,
        ManifestRequest {
            network: network.clone(),
            model: search.model.clone(),
            offset,
        },
    );
    pending.insert(
        id,
        Pending {
            query: search.query,
            peer,
            offset,
        },
    );
}
fn sync_indexes(
    swarm: &mut Swarm<Behaviour>,
    network: &NetworkIdentity,
    local: &LocalManifest,
    advertised: &mut HashSet<kad::RecordKey>,
) -> Result<(), String> {
    let mut wanted = HashSet::new();
    if !local.is_empty() {
        wanted.insert(index(network, None));
    }
    for held in local.values() {
        wanted.insert(index(network, Some(&held.offer.payload.model.id)));
    }
    for key in advertised.difference(&wanted) {
        swarm.behaviour_mut().kad.stop_providing(key);
    }
    advertised.retain(|key| wanted.contains(key));
    // Record each key as it is advertised, so that a later failure leaves no key
    // advertised that the next synchronisation would not stop.
    for key in wanted {
        if !advertised.contains(&key) {
            swarm
                .behaviour_mut()
                .kad
                .start_providing(key.clone())
                .map_err(|e| e.to_string())?;
            advertised.insert(key);
        }
    }
    Ok(())
}
/// Serves `offer` and advertises its index keys. A publication that fails serves and
/// advertises nothing for the offer's namespace, so a caller that withdraws the offers
/// it published when it fails leaves none served.
fn publish_local(
    swarm: &mut Swarm<Behaviour>,
    network: &NetworkIdentity,
    registry: &OfferRegistry,
    local: &mut LocalManifest,
    advertised: &mut HashSet<kad::RecordKey>,
    offer: Signed<InferenceOffer>,
) -> Result<(), String> {
    if offer.payload.peer_id != swarm.local_peer_id().to_string() {
        return Err("offer peer ID does not match persisted identity".into());
    }
    let namespace = offer_registry::namespace(&offer.payload);
    if local.len() >= MAX_LOCAL && !local.contains_key(&namespace) {
        return Err("local manifest is full".into());
    }
    let id = registry.insert(offer.clone())?;
    serve_local(local, id.clone(), offer);
    // Advertising fails when the provider record store is full, which records that
    // other peers store here also fill.
    if let Err(error) = sync_indexes(swarm, network, local, advertised) {
        local.remove(&namespace);
        registry.withdraw(&id);
        let _ = sync_indexes(swarm, network, local, advertised);
        return Err(error);
    }
    Ok(())
}
async fn run(
    mut swarm: Swarm<Behaviour>,
    network: NetworkIdentity,
    registry: Arc<OfferRegistry>,
    mut commands: mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), String>>,
) {
    let mut ready = Some(ready);
    let mut local = LocalManifest::new();
    let mut advertised = HashSet::new();
    let mut searches: HashMap<kad::QueryId, Search> = HashMap::new();
    let mut pending = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let mut bootstrap = tokio::time::interval(Duration::from_secs(30));
    let mut inbound: HashMap<PeerId, (tokio::time::Instant, u32)> = HashMap::new();
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Publish(offer,reply)) => {
                    let _ = reply.send(publish_local(&mut swarm,&network,&registry,&mut local,&mut advertised,offer));
                }
                Some(Command::Withdraw(id,reply)) => { local.retain(|_,held| held.id != id); registry.withdraw(&id); let _ = sync_indexes(&mut swarm,&network,&local,&mut advertised); let _ = reply.send(()); }
                Some(Command::Discover(model,reply)) => {
                    if searches.len() >= 4 { let _ = reply.send(Err("discovery is busy".into())); continue; }
                    let query = swarm.behaviour_mut().kad.get_providers(index(&network,model.as_deref()));
                    let mut search = Search { query,model,deadline:tokio::time::Instant::now()+WINDOW,seen:HashSet::new(),manifests:HashMap::new(),reply };
                    // Connected bootstrap peers are candidates too, not authoritative answers.
                    let peers: Vec<_> = swarm.connected_peers().copied().take(MAX_PEERS).collect();
                    for peer in peers { request_peer(&mut swarm,&network,&mut search,&mut pending,peer,0); }
                    searches.insert(query,search);
                }
                Some(Command::Status(reply)) => { let _ = reply.send(serde_json::json!({"running":true,"peer_id":swarm.local_peer_id().to_string(),"connected_peers":swarm.connected_peers().count(),"listening_addresses":swarm.listeners().map(ToString::to_string).collect::<Vec<_>>(),"published_offers":local.len(),"verified_offers":registry.list().len()})); }
                Some(Command::Stop(reply)) => { let _ = reply.send(()); break; }
                None => break,
            },
            _ = bootstrap.tick() => { let _ = swarm.behaviour_mut().kad.bootstrap(); },
            _ = tick.tick() => {
                let now = tokio::time::Instant::now();
                let finished: Vec<_> = searches.iter().filter(|(_,s)| s.deadline <= now || s.reply.is_closed()).map(|(id,_)| *id).collect();
                for id in finished {
                    if let Some(search) = searches.remove(&id) {
                        if let Some(mut query) = swarm.behaviour_mut().kad.query_mut(&id) { query.finish(); }
                        pending.retain(|_,p: &mut self::Pending| p.query != id);
                        let offers = registry.list().into_iter().filter(|o| search.model.as_ref().map_or(true,|m| &o.payload.model.id == m)).collect();
                        let _ = search.reply.send(Ok(offers));
                    }
                }
                let clock = protocol::now().unwrap_or(u64::MAX);
                local.retain(|_,held| { if held.offer.payload.expires_at <= clock { registry.withdraw(&held.id); false } else { true } });
                let _ = sync_indexes(&mut swarm,&network,&local,&mut advertised);
                inbound.retain(|_,(start,_)| now.duration_since(*start) < Duration::from_secs(30));
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr {address,..} => {
                    if address.iter().any(|p| match p {
                        libp2p::multiaddr::Protocol::Ip4(ip) => ip.is_loopback() || crate::offer_registry::public_ip(ip.into()),
                        libp2p::multiaddr::Protocol::Ip6(ip) => ip.is_loopback() || crate::offer_registry::public_ip(ip.into()),
                        _ => false,
                    }) { swarm.add_external_address(address); }
                    if let Some(ready) = ready.take() { let _ = ready.send(Ok(())); }
                }
                SwarmEvent::ListenerError { error,.. } => { if let Some(ready) = ready.take() { let _ = ready.send(Err(error.to_string())); } }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {peer_id,info,..})) => {
                    let policy = crate::version::effective_policy();
                    let valid_version = crate::version::version_is_below(info.agent_version.trim_start_matches("chiral/"),&policy.min_required) == Ok(false);
                    let scope = protocol::hash_hex(&protocol::canonical_json(&network).expect("network serializes"));
                    if !valid_version || info.protocol_version != format!("/chiral/inference/id/1/{scope}") { let _ = swarm.disconnect_peer_id(peer_id); continue; }
                    for addr in info.listen_addrs.into_iter().take(8) { swarm.behaviour_mut().kad.add_address(&peer_id,addr.clone()); swarm.add_peer_address(peer_id,addr); }
                }
                SwarmEvent::Behaviour(BehaviourEvent::Kad(kad::Event::OutboundQueryProgressed {id,result:kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders {providers,..})),..})) => {
                    if let Some(search) = searches.get_mut(&id) { for peer in providers { request_peer(&mut swarm,&network,search,&mut pending,peer,0); } }
                }
                SwarmEvent::Behaviour(BehaviourEvent::Manifest(rr::Event::Message {peer,message})) => match message {
                    rr::Message::Request {request,channel,..} => {
                        if inbound.len() >= 256 && !inbound.contains_key(&peer) { continue; }
                        let rate = inbound.entry(peer).or_insert((tokio::time::Instant::now(),0)); rate.1 += 1;
                        if rate.1 > 32 || request.network != network || request.offset >= MAX_LOCAL || request.offset % PAGE != 0 || request.model.as_ref().is_some_and(|m| m.len()>256) { continue; }
                        let page = manifest_page(&local,request.model.as_deref(),request.offset,protocol::now().unwrap_or(u64::MAX));
                        let _ = swarm.behaviour_mut().manifest.send_response(channel,page);
                    }
                    rr::Message::Response {request_id,response} => {
                        let Some(p) = pending.remove(&request_id) else { continue; };
                        if p.peer != peer { continue; }
                        if let Some(search) = searches.get_mut(&p.query) {
                            let manifest = search.manifests.entry(peer).or_default();
                            if let Some(next) = receive_page(&registry,search.model.as_deref(),&peer,p.offset,response,manifest) {
                                request_peer(&mut swarm,&network,search,&mut pending,peer,next);
                            }
                        }
                    }
                },
                SwarmEvent::Behaviour(BehaviourEvent::Manifest(rr::Event::OutboundFailure {request_id,..})) => { pending.remove(&request_id); }
                _ => {}
            }
        }
    }
    for (_, search) in searches {
        let _ = search.reply.send(Err("DHT stopped".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexes_bind_network_and_model() {
        let a = NetworkIdentity {
            chain_id: 1,
            genesis_hash: "a".into(),
            deployment_id: "one".into(),
        };
        let mut b = a.clone();
        b.deployment_id = "two".into();
        assert_ne!(index(&a, None), index(&b, None));
        assert_ne!(index(&a, None), index(&a, Some("m")));
        assert_ne!(index(&a, Some("m")), index(&a, Some("n")));
    }
    #[test]
    fn peer_identity_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peer.key");
        assert_eq!(
            load_identity(&path).unwrap().public().to_peer_id(),
            load_identity(&path).unwrap().public().to_peer_id()
        );
        // A new identity is created as a protected secret file.
        crate::wallet::open_secret_file(&path).unwrap();
    }

    // inference-offer-discovery requirement 2, task 2.3: the identity key file is a
    // protected secret, refused without being replaced when it is not protected.
    #[cfg(unix)]
    #[test]
    fn symbolic_link_to_identity_file_is_refused_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("owner-only.key");
        let peer = load_identity(&target).unwrap().public().to_peer_id();
        let bytes = std::fs::read(&target).unwrap();
        let link = dir.path().join("peer.key");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = load_identity(&link).unwrap_err();
        assert!(error.contains("protect the existing file in place"), "{error}");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert_eq!(load_identity(&target).unwrap().public().to_peer_id(), peer);
    }

    #[cfg(unix)]
    #[test]
    fn exposed_identity_file_is_refused_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peer.key");
        load_identity(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let error = load_identity(&path).unwrap_err();
        assert!(error.contains("owner-only"), "{error}");
        assert!(error.contains("protect the existing file in place"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_identity_file_has_a_current_user_only_access_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peer.key");
        let peer = load_identity(&path).unwrap().public().to_peer_id();
        // The shared validator accepts only a protected, current-user-only DACL.
        crate::wallet::open_secret_file(&path).unwrap();
        assert_eq!(load_identity(&path).unwrap().public().to_peer_id(), peer);
        // A file written normally inherits the directory's access list.
        let inherited = dir.path().join("inherited.key");
        let bytes = identity::Keypair::generate_ed25519()
            .to_protobuf_encoding()
            .unwrap();
        std::fs::write(&inherited, &bytes).unwrap();
        let error = load_identity(&inherited).unwrap_err();
        assert!(error.contains("protect the existing file in place"), "{error}");
        assert_eq!(std::fs::read(&inherited).unwrap(), bytes);
    }

    const PROVIDER_KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    /// One provider peer's offers for models `model-0`, `model-1`, …, one namespace each,
    /// all signed with one revision as a native provider refresh signs them.
    fn refresh_offers(
        peer: &PeerId,
        revision: u64,
        models: std::ops::Range<usize>,
        now: u64,
    ) -> Vec<Signed<InferenceOffer>> {
        let template = crate::offer_registry::tests::offer().payload;
        models
            .map(|index| {
                let mut payload = template.clone();
                payload.peer_id = peer.to_string();
                payload.deployment_id = peer.to_string();
                payload.model.id = format!("model-{index}");
                payload.revision = revision;
                payload.issued_at = now;
                payload.expires_at = now + 120;
                protocol::sign(protocol::OFFER_DOMAIN, payload, PROVIDER_KEY).unwrap()
            })
            .collect()
    }
    fn serve(local: &mut LocalManifest, offers: &[Signed<InferenceOffer>]) {
        for offer in offers {
            serve_local(local, protocol::offer_id(&offer.payload).unwrap(), offer.clone());
        }
    }
    fn page_entries(page: &ManifestResponse) -> Vec<(String, u64)> {
        page.offers
            .iter()
            .map(|o| (o.payload.model.id.clone(), o.payload.revision))
            .collect()
    }
    /// The receiver's verified registry, holding `cached`.
    fn cache(cached: &[Signed<InferenceOffer>]) -> OfferRegistry {
        let registry = OfferRegistry::new(cached[0].payload.network.clone(), false);
        for offer in cached {
            registry.insert(offer.clone()).unwrap();
        }
        registry
    }
    fn listed(registry: &OfferRegistry) -> BTreeMap<String, u64> {
        registry
            .list()
            .into_iter()
            .map(|o| (o.payload.model.id, o.payload.revision))
            .collect()
    }

    // Requirement 4, task 2.1: a peer pages its manifest by namespace.
    #[test]
    fn republished_namespaces_keep_their_manifest_pages() {
        // Fixed identities and times make every offer ID, and so any ID order, fixed.
        let peer = identity::Keypair::ed25519_from_bytes([7u8; 32])
            .unwrap()
            .public()
            .to_peer_id();
        let now = 1_800_000_000;
        let mut local = LocalManifest::new();
        serve(&mut local, &refresh_offers(&peer, 5, 0..9, now));
        let first = manifest_page(&local, None, 0, now);
        let last = manifest_page(&local, None, 8, now);
        assert_eq!((first.offers.len(), first.next), (8, Some(8)));
        assert_eq!((last.offers.len(), last.next), (1, None));
        let before = [page_entries(&first), page_entries(&last)];
        // The next refresh republishes every namespace at revision 6, with new offer IDs.
        serve(&mut local, &refresh_offers(&peer, 6, 0..9, now + 30));
        assert_eq!(local.len(), 9);
        let after = [
            page_entries(&manifest_page(&local, None, 0, now)),
            page_entries(&manifest_page(&local, None, 8, now)),
        ];
        for (before, after) in before.iter().zip(&after) {
            assert!(before.iter().all(|(_, revision)| *revision == 5));
            assert!(after.iter().all(|(_, revision)| *revision == 6));
            let names = |page: &Vec<(String, u64)>| page.iter().map(|(m, _)| m.clone()).collect::<Vec<_>>();
            assert_eq!(names(before), names(after), "a republish moved a namespace to another page");
        }
    }

    // Requirement 9, task 2.1: pages from two refreshes are not one snapshot.
    #[test]
    fn manifest_straddling_a_republish_withdraws_nothing() {
        let peer = PeerId::random();
        let now = protocol::now().unwrap();
        let old = refresh_offers(&peer, 5, 0..9, now);
        let new = refresh_offers(&peer, 6, 0..9, now);
        let mut local = LocalManifest::new();
        serve(&mut local, &old);
        let first = manifest_page(&local, None, 0, now);
        // This peer, which pages by namespace, and a peer that pages by offer ID, which can
        // place any namespace on page 8 after the republish (here `model-0`, already
        // received on page 0 at revision 5, so that `model-8` arrives at neither revision).
        serve(&mut local, &new);
        for last in [
            manifest_page(&local, None, 8, now),
            ManifestResponse { offers: vec![new[0].clone()], next: None },
        ] {
            let registry = cache(&old);
            let mut manifest = PeerManifest::default();
            assert_eq!(
                receive_page(&registry, None, &peer, 0, first.clone(), &mut manifest),
                Some(8)
            );
            assert_eq!(receive_page(&registry, None, &peer, 8, last, &mut manifest), None);
            let held = listed(&registry);
            assert_eq!(
                held.keys().cloned().collect::<Vec<_>>(),
                (0..9).map(|i| format!("model-{i}")).collect::<Vec<_>>(),
                "a live cached offer was withdrawn"
            );
            assert!(held.values().all(|revision| matches!(revision, 5 | 6)));
        }
    }

    // Requirement 9, task 2.1: an empty later page is not evidence of a complete manifest.
    #[test]
    fn manifest_that_shrank_between_pages_withdraws_nothing() {
        let peer = PeerId::random();
        let now = protocol::now().unwrap();
        let old = refresh_offers(&peer, 5, 0..9, now);
        let registry = cache(&old);
        let mut local = LocalManifest::new();
        serve(&mut local, &old);
        let mut manifest = PeerManifest::default();
        let first = manifest_page(&local, None, 0, now);
        assert_eq!(receive_page(&registry, None, &peer, 0, first, &mut manifest), Some(8));
        // The peer stops publishing `model-0`, so `model-8` moves to page 0.
        local.retain(|_, held| held.offer.payload.model.id != "model-0");
        let last = manifest_page(&local, None, 8, now);
        assert_eq!((last.offers.len(), last.next), (0, None));
        assert_eq!(receive_page(&registry, None, &peer, 8, last, &mut manifest), None);
        assert_eq!(listed(&registry).len(), 9, "a live cached offer was withdrawn");
    }

    // Requirement 9: a complete single-snapshot manifest still withdraws omitted offers.
    #[test]
    fn complete_snapshot_manifest_withdraws_an_omitted_offer() {
        let peer = PeerId::random();
        let now = protocol::now().unwrap();
        let registry = cache(&refresh_offers(&peer, 5, 0..10, now));
        // The next refresh stops publishing `model-3` and republishes nine at revision 6.
        let mut local = LocalManifest::new();
        let new = refresh_offers(&peer, 6, 0..10, now);
        serve(&mut local, &[&new[..3], &new[4..]].concat());
        let mut manifest = PeerManifest::default();
        let first = manifest_page(&local, None, 0, now);
        assert_eq!(receive_page(&registry, None, &peer, 0, first, &mut manifest), Some(8));
        let last = manifest_page(&local, None, 8, now);
        assert_eq!(receive_page(&registry, None, &peer, 8, last, &mut manifest), None);
        let held = listed(&registry);
        assert_eq!(held.len(), 9);
        assert!(!held.contains_key("model-3"));
        assert!(held.values().all(|revision| *revision == 6));

        // One page is one snapshot, whatever revisions its offers carry.
        let registry = cache(&refresh_offers(&peer, 5, 0..3, now));
        let page = ManifestResponse {
            offers: [
                refresh_offers(&peer, 6, 0..1, now),
                refresh_offers(&peer, 7, 1..2, now),
            ]
            .concat(),
            next: None,
        };
        let mut manifest = PeerManifest::default();
        assert_eq!(receive_page(&registry, None, &peer, 0, page, &mut manifest), None);
        assert_eq!(
            listed(&registry),
            BTreeMap::from([("model-0".into(), 6), ("model-1".into(), 7)])
        );
    }

    // Requirement 11, task 2.2: a publication that fails serves and advertises nothing, so
    // a failed refresh that withdraws the offers it published leaves no offer served.
    #[tokio::test]
    async fn failed_publication_serves_and_advertises_nothing() {
        use kad::store::RecordStore;
        let key = identity::Keypair::generate_ed25519();
        let peer = key.public().to_peer_id();
        let now = protocol::now().unwrap();
        let offer = refresh_offers(&peer, 5, 0..1, now).remove(0);
        let network = offer.payload.network.clone();
        let mut swarm = build_swarm(key, &network).unwrap();
        // Provider records that other peers stored here, as any DHT server peer may,
        // leave room for one more key: the offer's first index key is advertised and its
        // second is refused.
        let other = PeerId::random();
        for n in 0u32..127 {
            let record = kad::ProviderRecord::new(kad::RecordKey::new(&n.to_be_bytes()), other, Vec::new());
            swarm.behaviour_mut().kad.store_mut().add_provider(record).unwrap();
        }
        let registry = OfferRegistry::new(network.clone(), false);
        let mut local = LocalManifest::new();
        let mut advertised = HashSet::new();
        assert!(publish_local(&mut swarm, &network, &registry, &mut local, &mut advertised, offer)
            .is_err());
        assert!(manifest_page(&local, None, 0, now).offers.is_empty(), "a failed publication is served");
        assert!(registry.list().is_empty());
        assert!(advertised.is_empty());
        assert_eq!(swarm.behaviour_mut().kad.store_mut().provided().count(), 0);
    }
    #[tokio::test]
    async fn oversized_manifest_rejected_before_allocation() {
        let mut bytes = futures::io::Cursor::new((262_145u32).to_be_bytes());
        assert!(read_frame::<_, ManifestResponse>(&mut bytes, 262_144)
            .await
            .is_err());
    }
    fn assert_limit(error: libp2p::swarm::ConnectionDenied, expected: u32) {
        assert_eq!(
            error
                .downcast::<connection_limits::Exceeded>()
                .expect("connection limiter must deny admission")
                .limit(),
            expected
        );
    }

    #[test]
    fn pending_connection_admission_is_bounded_in_both_directions() {
        use libp2p::{core::Endpoint, swarm::ConnectionId};
        let mut limits = connection_limits::Behaviour::new(connection_limits());
        let address: Multiaddr = "/ip4/127.0.0.1/tcp/1".parse().unwrap();
        for id in 0..MAX_PENDING_INCOMING {
            limits
                .handle_pending_inbound_connection(
                    ConnectionId::new_unchecked(id as usize),
                    &address,
                    &address,
                )
                .unwrap();
        }
        assert_limit(
            limits
                .handle_pending_inbound_connection(
                    ConnectionId::new_unchecked(1000),
                    &address,
                    &address,
                )
                .unwrap_err(),
            MAX_PENDING_INCOMING,
        );
        // Saturated incoming handshakes do not consume the outgoing bootstrap budget.
        for id in 0..MAX_PENDING_OUTGOING {
            limits
                .handle_pending_outbound_connection(
                    ConnectionId::new_unchecked(2000 + id as usize),
                    None,
                    &[],
                    Endpoint::Dialer,
                )
                .unwrap();
        }
        assert_limit(
            limits
                .handle_pending_outbound_connection(
                    ConnectionId::new_unchecked(3000),
                    None,
                    &[],
                    Endpoint::Dialer,
                )
                .unwrap_err(),
            MAX_PENDING_OUTGOING,
        );
    }

    #[test]
    fn established_connection_admission_reserves_outbound_and_caps_total() {
        use libp2p::{
            core::{ConnectedPoint, Endpoint},
            swarm::{behaviour::ConnectionEstablished, ConnectionId, FromSwarm},
        };
        let mut limits = connection_limits::Behaviour::new(connection_limits());
        let address: Multiaddr = "/ip4/127.0.0.1/tcp/1".parse().unwrap();
        for id in 0..MAX_ESTABLISHED {
            let peer = PeerId::random();
            let connection_id = ConnectionId::new_unchecked(id as usize);
            let endpoint = if id < MAX_ESTABLISHED_INCOMING {
                limits
                    .handle_established_inbound_connection(connection_id, peer, &address, &address)
                    .unwrap();
                ConnectedPoint::Listener {
                    local_addr: address.clone(),
                    send_back_addr: address.clone(),
                }
            } else {
                if id == MAX_ESTABLISHED_INCOMING {
                    assert_limit(
                        limits
                            .handle_established_inbound_connection(
                                ConnectionId::new_unchecked(1000),
                                peer,
                                &address,
                                &address,
                            )
                            .err()
                            .expect("inbound budget must be full"),
                        MAX_ESTABLISHED_INCOMING,
                    );
                }
                limits
                    .handle_established_outbound_connection(
                        connection_id,
                        peer,
                        &address,
                        Endpoint::Dialer,
                    )
                    .unwrap();
                ConnectedPoint::Dialer {
                    address: address.clone(),
                    role_override: Endpoint::Dialer,
                }
            };
            limits.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id: peer,
                connection_id,
                endpoint: &endpoint,
                failed_addresses: &[],
                other_established: 0,
            }));
        }
        // Only 64 outbound connections exist: this must hit the combined total,
        // not the separately configured outbound ceiling or per-identity limit.
        assert_limit(
            limits
                .handle_established_outbound_connection(
                    ConnectionId::new_unchecked(2000),
                    PeerId::random(),
                    &address,
                    Endpoint::Dialer,
                )
                .err()
                .expect("total connection budget must be full"),
            MAX_ESTABLISHED,
        );
    }

    #[tokio::test]
    async fn tcp_noise_yamux_connections_are_limited_per_identity() {
        use libp2p::swarm::{
            dial_opts::{DialOpts, PeerCondition},
            ListenError,
        };
        let network = crate::offer_registry::tests::offer().payload.network;
        let mut provider = build_swarm(identity::Keypair::generate_ed25519(), &network).unwrap();
        provider
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        let address = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let SwarmEvent::NewListenAddr { address, .. } = provider.select_next_some().await
                {
                    break address;
                }
            }
        })
        .await
        .expect("provider must listen");
        let mut attacker = build_swarm(identity::Keypair::generate_ed25519(), &network).unwrap();
        // A hostile dialer need not enforce our limits. Keep its single identity
        // and all accepted sockets alive, forcing concurrent additional dials.
        *attacker.behaviour_mut().limits.limits_mut() =
            connection_limits::ConnectionLimits::default();
        let attempts = MAX_ESTABLISHED_PER_PEER + 4;
        for _ in 0..attempts {
            attacker
                .dial(
                    DialOpts::peer_id(*provider.local_peer_id())
                        .condition(PeerCondition::Always)
                        .addresses(vec![address.clone()])
                        .build(),
                )
                .unwrap();
        }
        let mut accepted = 0;
        let mut denied = 0;
        tokio::time::timeout(Duration::from_secs(10), async {
            while accepted + denied < attempts {
                tokio::select! {
                    event = provider.select_next_some() => match event {
                        SwarmEvent::ConnectionEstablished { peer_id, num_established, .. } => {
                            assert_eq!(peer_id, *attacker.local_peer_id());
                            accepted += 1;
                            assert!(accepted <= MAX_ESTABLISHED_PER_PEER, "one identity exceeded its admission limit");
                            assert!(num_established.get() <= MAX_ESTABLISHED_PER_PEER);
                        }
                        SwarmEvent::IncomingConnectionError { error: ListenError::Denied { cause }, .. } => {
                            assert_limit(cause, MAX_ESTABLISHED_PER_PEER);
                            denied += 1;
                        }
                        SwarmEvent::IncomingConnectionError { error, .. } => panic!("unexpected handshake failure: {error}"),
                        SwarmEvent::ConnectionClosed { cause, .. } => panic!("admitted connection unexpectedly closed: {cause:?}"),
                        _ => {}
                    },
                    _ = attacker.select_next_some() => {}
                }
            }
        }).await.expect("all TCP/Noise/Yamux connection attempts must be admitted or denied");
        assert_eq!(accepted, MAX_ESTABLISHED_PER_PEER);
        assert_eq!(denied, attempts - MAX_ESTABLISHED_PER_PEER);
        assert_eq!(provider.connected_peers().count(), 1);
        assert_eq!(
            provider
                .network_info()
                .connection_counters()
                .num_established(),
            MAX_ESTABLISHED_PER_PEER
        );
    }

    #[tokio::test]
    async fn direct_signed_manifest_discovery_and_withdrawal() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = crate::offer_registry::tests::offer();
        let network = fixture.payload.network.clone();
        let provider_registry = Arc::new(OfferRegistry::new(network.clone(), true));
        let provider = DhtService::start(
            DhtConfig {
                listen: "/ip4/127.0.0.1/tcp/0".into(),
                bootstrap: vec![],
                identity_file: directory.path().join("provider.key"),
            },
            network.clone(),
            provider_registry,
        )
        .await
        .unwrap();
        let mut payload = fixture.payload;
        payload.peer_id = provider.peer_id();
        payload.endpoint = "http://127.0.0.1:8080/".into();
        let first = protocol::sign(
            protocol::OFFER_DOMAIN,
            payload.clone(),
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap();
        provider.publish(&first).await.unwrap();
        payload.deployment_id = "gpu-b".into();
        let second = protocol::sign(
            protocol::OFFER_DOMAIN,
            payload,
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap();
        provider.publish(&second).await.unwrap();
        let status = provider.status().await;
        let address = status["listening_addresses"][0].as_str().unwrap();
        let consumer_registry = Arc::new(OfferRegistry::new(network.clone(), true));
        let consumer = DhtService::start(
            DhtConfig {
                listen: "/ip4/127.0.0.1/tcp/0".into(),
                bootstrap: vec![format!("{address}/p2p/{}", provider.peer_id())],
                identity_file: directory.path().join("consumer.key"),
            },
            network.clone(),
            consumer_registry,
        )
        .await
        .unwrap();
        // A real TCP/noise/yamux/Kademlia exchange, not an in-memory fake catalog.
        let offers = consumer.discover(Some("model")).await.unwrap();
        assert_eq!(offers.len(), 2);
        for offer in &offers {
            protocol::verify(
                protocol::OFFER_DOMAIN,
                offer,
                &offer.payload.provider_wallet,
            )
            .unwrap();
        }
        provider
            .withdraw(&protocol::offer_id(&first.payload).unwrap())
            .await;
        let offers = consumer.discover(None).await.unwrap();
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].payload.deployment_id, "gpu-b");
        consumer.stop().await;
        provider.stop().await;
    }
}
