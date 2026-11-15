//! Bounded signed inference discovery. Kademlia supplies candidates, never trusted
//! records; manifests are merged until a fixed deadline, including after bad replies.
use crate::{
    offer_registry::OfferRegistry,
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
    collections::{BTreeMap, HashMap, HashSet},
    io,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

const MAX_PEERS: usize = 64;
const MAX_LOCAL: usize = 64;
const PAGE: usize = 8;
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
    received: HashMap<PeerId, HashSet<String>>,
    invalid: HashSet<PeerId>,
}
struct Pending {
    query: kad::QueryId,
    peer: PeerId,
    offset: usize,
}

fn load_identity(path: &Path) -> Result<identity::Keypair, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if std::fs::metadata(path)
                    .map_err(|e| e.to_string())?
                    .permissions()
                    .mode()
                    & 0o077
                    != 0
                {
                    return Err("persisted peer identity must be owner-only".into());
                }
            }
            identity::Keypair::from_protobuf_encoding(&bytes)
                .map_err(|e| format!("invalid persisted peer identity: {e}"))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let key = identity::Keypair::generate_ed25519();
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(path).map_err(|e| e.to_string())?;
            use std::io::Write;
            file.write_all(&key.to_protobuf_encoding().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(key)
        }
        Err(e) => Err(e.to_string()),
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
        let key = load_identity(&config.identity_file)?;
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
    local: &BTreeMap<String, Signed<InferenceOffer>>,
    advertised: &mut HashSet<kad::RecordKey>,
) -> Result<(), String> {
    let mut wanted = HashSet::new();
    if !local.is_empty() {
        wanted.insert(index(network, None));
    }
    for offer in local.values() {
        wanted.insert(index(network, Some(&offer.payload.model.id)));
    }
    for key in advertised.difference(&wanted) {
        swarm.behaviour_mut().kad.stop_providing(key);
    }
    for key in wanted.difference(advertised) {
        swarm
            .behaviour_mut()
            .kad
            .start_providing(key.clone())
            .map_err(|e| e.to_string())?;
    }
    *advertised = wanted;
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
    let mut local: BTreeMap<String, Signed<InferenceOffer>> = BTreeMap::new();
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
                    let result = if offer.payload.peer_id != swarm.local_peer_id().to_string() { Err("offer peer ID does not match persisted identity".into()) }
                    else if local.len() >= MAX_LOCAL && !local.values().any(|o| o.payload.provider_wallet == offer.payload.provider_wallet && o.payload.deployment_id == offer.payload.deployment_id && o.payload.model.id == offer.payload.model.id) { Err("local manifest is full".into()) }
                    else { registry.insert(offer.clone()).and_then(|id| {
                        local.retain(|_,old| !(old.payload.provider_wallet == offer.payload.provider_wallet && old.payload.deployment_id == offer.payload.deployment_id && old.payload.model.id == offer.payload.model.id));
                        local.insert(id,offer); sync_indexes(&mut swarm,&network,&local,&mut advertised)
                    }) }; let _ = reply.send(result);
                }
                Some(Command::Withdraw(id,reply)) => { local.remove(&id); registry.withdraw(&id); let _ = sync_indexes(&mut swarm,&network,&local,&mut advertised); let _ = reply.send(()); }
                Some(Command::Discover(model,reply)) => {
                    if searches.len() >= 4 { let _ = reply.send(Err("discovery is busy".into())); continue; }
                    let query = swarm.behaviour_mut().kad.get_providers(index(&network,model.as_deref()));
                    let mut search = Search { query,model,deadline:tokio::time::Instant::now()+WINDOW,seen:HashSet::new(),received:HashMap::new(),invalid:HashSet::new(),reply };
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
                local.retain(|id,o| { if o.payload.expires_at <= clock { registry.withdraw(id); false } else { true } });
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
                        let now = protocol::now().unwrap_or(u64::MAX);
                        let all: Vec<_> = local.values().filter(|o| o.payload.expires_at>now && request.model.as_ref().map_or(true,|m| &o.payload.model.id==m)).collect();
                        let offers = all.iter().skip(request.offset).take(PAGE).map(|o| (*o).clone()).collect();
                        let next = (all.len()>request.offset+PAGE).then_some(request.offset+PAGE);
                        let _ = swarm.behaviour_mut().manifest.send_response(channel,ManifestResponse {offers,next});
                    }
                    rr::Message::Response {request_id,response} => {
                        let Some(p) = pending.remove(&request_id) else { continue; };
                        if p.peer != peer || response.offers.len()>PAGE { continue; }
                        if let Some(search) = searches.get_mut(&p.query) {
                            for offer in response.offers {
                                if offer.payload.peer_id != peer.to_string() || !search.model.as_ref().map_or(true,|m| &offer.payload.model.id==m) { search.invalid.insert(peer); continue; }
                                match registry.insert(offer) {
                                    Ok(id) => { search.received.entry(peer).or_default().insert(id); }
                                    Err(_) => { search.invalid.insert(peer); }
                                }
                            }
                            if response.next == Some(p.offset+PAGE) { request_peer(&mut swarm,&network,search,&mut pending,peer,p.offset+PAGE); }
                            else if response.next.is_none() && !search.invalid.contains(&peer) {
                                // An authenticated complete manifest can withdraw this peer's
                                // cached offers only. Invalid/partial replies never erase votes.
                                for offer in registry.list() {
                                    if offer.payload.peer_id == peer.to_string() && search.model.as_ref().map_or(true,|m| &offer.payload.model.id==m) {
                                        if let Ok(id) = protocol::offer_id(&offer.payload) {
                                            if !search.received.get(&peer).is_some_and(|ids| ids.contains(&id)) { registry.withdraw(&id); }
                                        }
                                    }
                                }
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
