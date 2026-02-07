pub mod models;
// pub mod protocol;
pub use self::models::*;
use bon::Builder;
// use self::protocol::*;
use crate::config::CHAIN_ID;
use crate::download_source::HttpSourceInfo;
use crate::encryption::EncryptedAesKeyBundle;
use serde_bytes;
use x25519_dalek::PublicKey;
/// Helper function to deserialize CIDs from JSON values that may be strings or Cid objects.
/// This handles the transition from Cid objects to string serialization.
fn deserialize_cids_from_json(value: &serde_json::Value) -> Option<Vec<Cid>> {
    // First try to deserialize as Vec<String> (new format)
    if let Ok(strings) = serde_json::from_value::<Option<Vec<String>>>(value.clone()) {
        if let Some(cid_strings) = strings {
            let mut cids = Vec::new();
            for cid_str in cid_strings {
                match cid_str.parse::<Cid>() {
                    Ok(cid) => cids.push(cid),
                    Err(e) => {
                        warn!("Failed to parse CID from string '{}': {}", cid_str, e);
                        continue;
                    }
                }
            }
            return Some(cids);
        }
    }

    // Fallback: try to deserialize as Vec<Cid> (old format, if it still exists)
    if let Ok(cids) = serde_json::from_value::<Option<Vec<Cid>>>(value.clone()) {
        return cids;
    }

    None
}

/// Merges two FileMetadata instances for the same file uploaded via different protocols.
/// This preserves all protocol-specific information while keeping the most recent common fields.
fn merge_file_metadata(
    existing: crate::dht::models::FileMetadata,
    new: crate::dht::models::FileMetadata,
) -> crate::dht::models::FileMetadata {
    // Keep the most recent metadata as base, but merge protocol-specific fields
    let mut merged = new.clone();

    // Merge seeders (combine unique addresses)
    let mut all_seeders = existing.seeders.clone();
    all_seeders.extend(new.seeders.clone());
    all_seeders.sort();
    all_seeders.dedup();
    merged.seeders = all_seeders;

    // Merge FTP sources (if any)
    if let (Some(existing_ftp), Some(new_ftp)) = (&existing.ftp_sources, &new.ftp_sources) {
        let mut merged_ftp = existing_ftp.clone();
        merged_ftp.extend(new_ftp.clone());
        // Remove duplicates based on URL
        merged_ftp.sort_by(|a, b| a.url.cmp(&b.url));
        merged_ftp.dedup_by(|a, b| a.url == b.url);
        merged.ftp_sources = Some(merged_ftp);
    } else if existing.ftp_sources.is_some() {
        merged.ftp_sources = existing.ftp_sources.clone();
    }
    // If new has FTP sources, they're already in merged (since we cloned new)

    // Merge ED2K sources (if any)
    if let (Some(existing_ed2k), Some(new_ed2k)) = (&existing.ed2k_sources, &new.ed2k_sources) {
        let mut merged_ed2k = existing_ed2k.clone();
        merged_ed2k.extend(new_ed2k.clone());
        // Remove duplicates based on server_url and file_hash
        merged_ed2k.sort_by(|a, b| match a.server_url.cmp(&b.server_url) {
            std::cmp::Ordering::Equal => a.file_hash.cmp(&b.file_hash),
            other => other,
        });
        merged_ed2k.dedup_by(|a, b| a.server_url == b.server_url && a.file_hash == b.file_hash);
        merged.ed2k_sources = Some(merged_ed2k);
    } else if existing.ed2k_sources.is_some() {
        merged.ed2k_sources = existing.ed2k_sources.clone();
    }

    // Merge HTTP sources (if any)
    if let (Some(existing_http), Some(new_http)) = (&existing.http_sources, &new.http_sources) {
        let mut merged_http = existing_http.clone();
        merged_http.extend(new_http.clone());
        // Remove duplicates based on URL
        merged_http.sort_by(|a, b| a.url.cmp(&b.url));
        merged_http.dedup_by(|a, b| a.url == b.url);
        merged.http_sources = Some(merged_http);
    } else if existing.http_sources.is_some() {
        merged.http_sources = existing.http_sources.clone();
    }

    // Merge CIDs (IPFS content identifiers)
    if let (Some(existing_cids), Some(new_cids)) = (&existing.cids, &new.cids) {
        let mut merged_cids = existing_cids.clone();
        merged_cids.extend(new_cids.clone());
        merged_cids.sort();
        merged_cids.dedup();
        merged.cids = Some(merged_cids);
    } else if existing.cids.is_some() {
        merged.cids = existing.cids.clone();
    }

    // Keep BitTorrent-specific fields from whichever has them (prefer new)
    if existing.info_hash.is_some() && new.info_hash.is_none() {
        merged.info_hash = existing.info_hash.clone();
    }
    if existing.trackers.is_some() && new.trackers.is_none() {
        merged.trackers = existing.trackers.clone();
    }

    // For other fields, we keep the new values (most recent upload)
    // This includes: file_name, file_size, created_at, price, uploader_address, etc.

    merged
}

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

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Request>
    where
        T: FAsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        serde_json::from_slice(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
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
        serde_json::from_slice(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        request: Self::Request,
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
        response: Self::Response,
    ) -> std::io::Result<()>
    where
        T: FAsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&response)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
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
use tokio;

pub use cid::Cid;
use futures::future::{BoxFuture, FutureExt};
use futures::io::{AsyncRead as FAsyncRead, AsyncWrite as FAsyncWrite};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use futures_util::StreamExt;
pub use multihash_codetable::{Code, MultihashDigest};
use rs_merkle::{Hasher, MerkleTree};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    str::FromStr,
};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{debug, error, info, trace, warn};

use crate::manager::Sha256Hasher;
use crate::peer_selection::{PeerMetrics, PeerSelectionService, SelectionStrategy};
use crate::reputation::{TransactionVerdict, VerdictOutcome};
use crate::webrtc_service::{get_webrtc_service, FileChunk};
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
use anyhow::Result;

// Rate limiting for connection error logs (log at most once every 30 seconds)
static LAST_CONNECTION_ERROR_LOG: AtomicU64 = AtomicU64::new(0);

use libp2p::{
    autonat::v2,
    core::{
        transport::{DialOpts, ListenerId, Transport, TransportError, TransportEvent},
    },
    identify::{self, Event as IdentifyEvent},
    identity,
    kad::{
        self, store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig,
        Event as KademliaEvent, GetRecordOk, Mode, PutRecordOk, QueryResult, Record,
    },
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    multiaddr::Protocol,
    noise,
    ping::{self, Behaviour as Ping, Event as PingEvent},
    request_response as rr,
    swarm::{behaviour::toggle, NetworkBehaviour, SwarmEvent},
    tcp, upnp, yamux, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use rand::rngs::OsRng;
const EXPECTED_PROTOCOL_VERSION: &str = "/chiral/1.0.0";
const MAX_MULTIHASH_LENGHT: usize = 64;
/// Prefix for DHT records that map a torrent info_hash to a Chiral Merkle root.
const INFO_HASH_PREFIX: &str = "info_hash_idx::";
pub const RAW_CODEC: u64 = 0x55;

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
    upnp: toggle::Toggle<upnp::tokio::Behaviour>,
}
#[derive(Debug)]
pub enum DhtCommand {
    PublishFile {
        metadata: FileMetadata,
        response_tx: oneshot::Sender<FileMetadata>,
    },
    SearchByInfohash {
        info_hash: String,
        sender: oneshot::Sender<Option<FileMetadata>>,
    },
    SearchPeersByInfohash {
        info_hash: String,
        sender: oneshot::Sender<Result<Vec<String>, String>>,
    },
    SearchFile {
        file_hash: String,
        sender: oneshot::Sender<Result<Option<FileMetadata>, String>>,
    },
    DownloadFile(FileMetadata, String),
    ConnectPeer(String),
    ConnectPeerMultiaddr(Multiaddr),
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
    GetProviders {
        file_hash: String,
        sender: oneshot::Sender<Result<Vec<String>, String>>,
    },
    GetPeerAddresses {
        peer_ids: Vec<PeerId>,
        sender: oneshot::Sender<HashMap<PeerId, Vec<Multiaddr>>>,
    },
    SendWebRTCOffer {
        peer: PeerId,
        offer_request: WebRTCOfferRequest,
        sender: oneshot::Sender<Result<WebRTCAnswerResponse, String>>,
    },
    StoreBlock {
        cid: Cid,
        data: Vec<u8>,
    },
    StoreBlocks {
        blocks: Vec<(Cid, Vec<u8>)>,
        root_cid: Cid,
        metadata: FileMetadata,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    RequestFileAccess {
        seeder: PeerId,
        merkle_root: String,
        recipient_public_key: PublicKey,
        sender: oneshot::Sender<Result<EncryptedAesKeyBundle, String>>,
    },
    AnnounceTorrent {
        info_hash: String,
    },
    PutDhtValue {
        key: String,
        value: Vec<u8>,
        sender: oneshot::Sender<Result<(), String>>,
    },
    GetDhtValue {
        key: String,
        sender: oneshot::Sender<Result<Option<Vec<u8>>, String>>,
    },
    /// Re-bootstrap the DHT to discover new peers
    ReBootstrap {
        sender: oneshot::Sender<Result<usize, String>>,
    },
    /// Check DHT health and optionally trigger recovery
    HealthCheck {
        min_peers: usize,
        auto_recover: bool,
        sender: oneshot::Sender<DhtHealthStatus>,
    },
}

/// Health status of the DHT network
#[derive(Debug, Clone, Serialize)]
pub struct DhtHealthStatus {
    pub healthy: bool,
    pub peer_count: usize,
    pub min_required: usize,
    pub bootstrap_failures: u64,
    pub last_bootstrap_secs_ago: Option<u64>,
    pub recommendation: Option<String>,
    pub recovery_triggered: bool,
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

// ------------ Proxy Manager Structs and Enums ------------
const PROXY_PING_RTT_TTL: Duration = Duration::from_secs(60);
const PROXY_PING_RTT_MAX: usize = 512;

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

#[derive(Clone, Copy)]
struct ProxyPingRtt {
    ping_rtt_ms: u64,
    updated_at: Instant,
}

struct ProxyManager {
    targets: std::collections::HashSet<PeerId>,
    capable: std::collections::HashSet<PeerId>,
    online: std::collections::HashSet<PeerId>,
    ping_rtt: std::collections::HashMap<PeerId, ProxyPingRtt>,
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
    fn ping_rtt_ttl() -> Duration {
        PROXY_PING_RTT_TTL.max(Duration::from_secs(30))
    }
    fn ping_rtt_ms(&self, id: &PeerId) -> Option<u64> {
        let row = self.ping_rtt.get(id)?;
        if row.updated_at.elapsed() > Self::ping_rtt_ttl() {
            return None;
        }
        Some(row.ping_rtt_ms)
    }
    fn prune_ping_rtt(&mut self) {
        let ttl = Self::ping_rtt_ttl();
        let online = self.online.clone();
        self.ping_rtt
            .retain(|peer, row| online.contains(peer) && row.updated_at.elapsed() <= ttl);
        if self.ping_rtt.len() <= PROXY_PING_RTT_MAX {
            return;
        }

        let mut by_age: Vec<(PeerId, Instant)> = self
            .ping_rtt
            .iter()
            .map(|(id, row)| (id.clone(), row.updated_at))
            .collect();
        by_age.sort_by_key(|(_, ts)| *ts);
        let extra = self.ping_rtt.len().saturating_sub(PROXY_PING_RTT_MAX);
        for (peer, _) in by_age.into_iter().take(extra) {
            self.ping_rtt.remove(&peer);
        }
    }
    fn set_ping_rtt(&mut self, id: PeerId, ping_rtt_ms: u64) {
        self.ping_rtt.insert(
            id,
            ProxyPingRtt {
                ping_rtt_ms,
                updated_at: Instant::now(),
            },
        );
        self.prune_ping_rtt();
    }
    fn set_offline(&mut self, id: &PeerId) {
        self.online.remove(id);
        self.ping_rtt.remove(id);
    }
    fn remove_all(&mut self, id: &PeerId) {
        self.targets.remove(id);
        self.capable.remove(id);
        self.online.remove(id);
        self.ping_rtt.remove(id);
        self.relay_pending.remove(id);
        self.relay_ready.remove(id);
        self.trusted_proxy_nodes.remove(id);
        self.manual_trusted.remove(id);
    }
    fn is_proxy(&self, id: &PeerId) -> bool {
        self.targets.contains(id) || self.capable.contains(id)
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

        // Select a trusted proxy that's online/capable and prefer the one with
        // lowest fresh ping RTT. Unknown/stale ping RTT sorts last.
        self.trusted_proxy_nodes
            .iter()
            .filter(|&&proxy_id| {
                proxy_id != *target_peer
                    && self.online.contains(&proxy_id)
                    && self.capable.contains(&proxy_id)
            })
            .min_by(|a, b| {
                let rtt_a = self.ping_rtt_ms(a).unwrap_or(u64::MAX);
                let rtt_b = self.ping_rtt_ms(b).unwrap_or(u64::MAX);
                rtt_a
                    .cmp(&rtt_b)
                    .then_with(|| a.to_string().cmp(&b.to_string()))
            })
            .cloned()
    }

    #[cfg(test)]
    fn set_ping_rtt_at(&mut self, id: PeerId, ping_rtt_ms: u64, updated_at: Instant) {
        self.ping_rtt.insert(
            id,
            ProxyPingRtt {
                ping_rtt_ms,
                updated_at,
            },
        );
    }
}

impl Default for ProxyManager {
    fn default() -> Self {
        Self {
            targets: std::collections::HashSet::new(),
            capable: std::collections::HashSet::new(),
            online: std::collections::HashSet::new(),
            ping_rtt: std::collections::HashMap::new(),
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
struct PendingInfohashSearch {
    id: u64,
    sender: oneshot::Sender<Option<FileMetadata>>,
}

#[derive(Debug)]
struct PendingProviderQuery {
    id: u64,
    sender: oneshot::Sender<Result<Vec<String>, String>>,
}

// Helper function to construct FileMetadata from JSON (simplified version for search results)
fn construct_file_metadata_from_json_simple(
    metadata_json: &serde_json::Value,
    file_hash: &str,
    file_name: &str,
    file_size: u64,
    created_at: u64,
) -> FileMetadata {
    FileMetadata {
        merkle_root: file_hash.to_string(),
        file_name: file_name.to_string(),
        file_size,
        file_data: Vec::new(), // Will be populated during download
        seeders: metadata_json
            .get("seeders")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        created_at,
        mime_type: metadata_json
            .get("mimeType")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        is_encrypted: metadata_json
            .get("isEncrypted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        encryption_method: metadata_json
            .get("encryptionMethod")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        key_fingerprint: metadata_json
            .get("keyFingerprint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        parent_hash: metadata_json
            .get("parentHash")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        cids: metadata_json
            .get("cids")
            .and_then(|v| deserialize_cids_from_json(v)),
        encrypted_key_bundle: metadata_json.get("encryptedKeyBundle").and_then(|v| {
            // The field name is camelCase in the JSON
            serde_json::from_value::<Option<crate::encryption::EncryptedAesKeyBundle>>(v.clone())
                .unwrap_or(None)
        }),
        info_hash: metadata_json
            .get("infoHash")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        trackers: metadata_json
            .get("trackers")
            .and_then(|v| serde_json::from_value::<Option<Vec<String>>>(v.clone()).unwrap_or(None)),
        is_root: metadata_json
            .get("is_root")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        price: metadata_json
            .get("price")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        http_sources: metadata_json.get("httpSources").and_then(|v| {
            serde_json::from_value::<Option<Vec<HttpSourceInfo>>>(v.clone()).unwrap_or(None)
        }),
        ed2k_sources: metadata_json.get("ed2kSources").and_then(|v| {
            serde_json::from_value::<Option<Vec<Ed2kSourceInfo>>>(v.clone()).unwrap_or(None)
        }),
        uploader_address: metadata_json
            .get("uploader_address")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        ftp_sources: metadata_json.get("ftpSources").and_then(|v| {
            serde_json::from_value::<Option<Vec<FtpSourceInfo>>>(v.clone()).unwrap_or(None)
        }),
        download_path: None,
        manifest: metadata_json
            .get("manifest")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

// Add this struct after PendingProviderQuery around line 650:
#[derive(Debug)]
struct PendingSearchQuery {
    file_hash: String,
    record_query_id: Option<kad::QueryId>,
    providers_query_id: Option<kad::QueryId>,
    sender: oneshot::Sender<Result<Option<FileMetadata>, String>>,
    start_time: std::time::Instant,
    found_record: Option<FileMetadata>,
    found_providers: Option<Vec<String>>,
}

impl PendingSearchQuery {
    fn new(
        file_hash: String,
        sender: oneshot::Sender<Result<Option<FileMetadata>, String>>,
    ) -> Self {
        Self {
            file_hash,
            record_query_id: None,
            providers_query_id: None,
            sender,
            start_time: std::time::Instant::now(),
            found_record: None,
            found_providers: None,
        }
    }

    fn is_complete(&self) -> bool {
        self.found_record.is_some() // Only complete when we have the actual metadata
    }

    fn finalize(self) -> Result<Option<FileMetadata>, String> {
        if let Some(metadata) = self.found_record {
            Ok(Some(metadata))
        } else if let Some(_providers) = self.found_providers {
            // If we found providers but no metadata record, we could potentially
            // query the providers for metadata, but for now return None
            Ok(None)
        } else {
            Ok(None)
        }
    }
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
    pending_infohash_searches: Arc<Mutex<HashMap<kad::QueryId, PendingInfohashSearch>>>,
    peer_selection: Arc<Mutex<PeerSelectionService>>,
    received_chunks: Arc<Mutex<HashMap<String, HashMap<u32, FileChunk>>>>,
    file_transfer_service: Option<Arc<FileTransferService>>,
    webrtc_service: Option<Arc<crate::webrtc_service::WebRTCService>>,
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
    pending_provider_registrations: Arc<Mutex<HashSet<String>>>,
    file_metadata_cache: Arc<Mutex<HashMap<String, FileMetadata>>>,
    pending_dht_queries: Arc<
        Mutex<HashMap<kad::QueryId, oneshot::Sender<Result<Option<Vec<u8>>, String>>>>,
    >,
    pending_key_requests: Arc<
        Mutex<
            HashMap<rr::OutboundRequestId, oneshot::Sender<Result<EncryptedAesKeyBundle, String>>>,
        >,
    >,
    pending_search_queries: Arc<Mutex<HashMap<kad::QueryId, PendingSearchQuery>>>,
    is_bootstrap: bool,
    chunk_size: usize,
    bootstrap_peer_ids: HashSet<PeerId>,
    pure_client_mode: bool,
    force_server_mode: bool,
) {
    let mut dht_maintenance_interval = tokio::time::interval(Duration::from_secs(30 * 60));
    dht_maintenance_interval.tick().await;

    let mut shutdown_ack: Option<oneshot::Sender<()>> = None;
    let mut ping_failures: HashMap<PeerId, u8> = HashMap::new();

    let queries: HashMap<beetswap::QueryId, u32> = HashMap::new();
    let downloaded_chunks: HashMap<usize, Vec<u8>> = HashMap::new();
    let current_metadata: Option<FileMetadata> = None;

    'outer: loop {
        tokio::select! {
                            cmd = cmd_rx.recv() => {
                                match cmd {
                                    Some(DhtCommand::Shutdown(ack)) => {
                                        info!("Received shutdown signal for DHT node");
                                        shutdown_ack = Some(ack);
                                        break 'outer;
                                    }
                                    Some(DhtCommand::PublishFile { metadata, response_tx }) => {
            let now = unix_timestamp();
            let peer_id_str = peer_id.to_string();

            // 1. Resolve naming: use merkle_root as the identifier
            info!("🔍 DEBUG DHT PUBLISH: Local peer_id = {}", peer_id_str);
            info!("🔍 DEBUG DHT PUBLISH: Merkle root = {}", metadata.merkle_root);

            // 2. Define merged_metadata by checking the local cache
            // This fixes the "cannot find value merged_metadata" errors
            let merged_metadata = {
                let cache = file_metadata_cache.lock().await;
                if let Some(existing) = cache.get(&metadata.merkle_root) {
                    merge_file_metadata(existing.clone(), metadata.clone())
                } else {
                    metadata.clone()
                }
            };

            // 3. Update local cache with the merged result
            {
                let mut cache = file_metadata_cache.lock().await;
                cache.insert(merged_metadata.merkle_root.clone(), merged_metadata.clone());
            }

            // 4. Create the JSON for DHT storage
            //
            // IMPORTANT:
            // We MUST go through serde for FileMetadata here to ensure:
            // - field renames (merkleRoot/fileName/...) are correct
            // - CID custom serialization runs (cids → Vec<String>)
            //
            // Then we add a few legacy/extra fields for backwards compatibility.
            let mut dht_metadata = serde_json::to_value(&merged_metadata).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(obj) = dht_metadata.as_object_mut() {
                // Keep legacy fields used by older readers / debug tooling.
                obj.insert("file_hash".to_string(), serde_json::json!(merged_metadata.merkle_root));

                // Some historical paths used snake_case keys; add them as aliases so mixed networks can interop.
                obj.insert("merkle_root".to_string(), serde_json::json!(merged_metadata.merkle_root));
                obj.insert("file_name".to_string(), serde_json::json!(merged_metadata.file_name));
                obj.insert("file_size".to_string(), serde_json::json!(merged_metadata.file_size));
                obj.insert("created_at".to_string(), serde_json::json!(merged_metadata.created_at));
                obj.insert("mime_type".to_string(), serde_json::json!(merged_metadata.mime_type));
                obj.insert("is_encrypted".to_string(), serde_json::json!(merged_metadata.is_encrypted));
                obj.insert("encryption_method".to_string(), serde_json::json!(merged_metadata.encryption_method));
                obj.insert("key_fingerprint".to_string(), serde_json::json!(merged_metadata.key_fingerprint));
                obj.insert("parent_hash".to_string(), serde_json::json!(merged_metadata.parent_hash));
                obj.insert("encrypted_key_bundle".to_string(), serde_json::json!(merged_metadata.encrypted_key_bundle));
                obj.insert("info_hash".to_string(), serde_json::json!(merged_metadata.info_hash));
                obj.insert("uploader_address".to_string(), serde_json::json!(merged_metadata.uploader_address));
            }

            let record_key = kad::RecordKey::new(&merged_metadata.merkle_root.as_bytes());

            let dht_record_data = match serde_json::to_vec(&dht_metadata) {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to serialize DHT metadata: {}", e);
                    return;
                }
            };

            let record = Record {
                key: record_key.clone(),
                value: dht_record_data,
                publisher: Some(peer_id),
                expires: None,
            };

            let connected_peers_count = connected_peers.lock().await.len();
            let replication_factor = 3;

            let quorum = if connected_peers_count >= 3 {
                let half_up = (connected_peers_count + 1) / 2;
                let target = std::cmp::min(replication_factor, std::cmp::max(1, half_up));
                if let Some(n) = std::num::NonZeroUsize::new(target) {
                    kad::Quorum::N(n)
                } else {
                    kad::Quorum::One
                }
            } else {
                kad::Quorum::One
            };

            match swarm.behaviour_mut().kademlia.put_record(record, quorum) {
                Ok(_) => {
                    // FIX: Use indexing for JSON value access instead of dot notation
                    info!("put file: {}", dht_metadata["file_hash"]);
                }
                Err(e) => {
                    error!("failed to put file {}: {}", merged_metadata.merkle_root, e);
                    let _ = event_tx.send(DhtEvent::Error(format!("failed to start providing: {}", e))).await;
                }
            }

            match swarm.behaviour_mut().kademlia.start_providing(record_key) {
                Ok(_) => {
                    // FIX: Use indexing for JSON value access instead of dot notation
                    info!("providing file: {}", dht_metadata["file_hash"]);
                }
                Err(e) => {
                    error!("failed to start providing file {}: {}", merged_metadata.merkle_root, e);
                    let _ = event_tx.send(DhtEvent::Error(format!("failed to start providing: {}", e))).await;
                }
            }

            // 5. Notify frontend and respond to the command
            info!("🔍 DEBUG DHT: Sending PublishedFile event for {}", merged_metadata.merkle_root);
            let _ = event_tx.send(DhtEvent::PublishedFile(merged_metadata.clone())).await;

            if let Some(info_hash) = &merged_metadata.info_hash {
                let index_key = format!("{}{}", INFO_HASH_PREFIX, info_hash);
                let index_record = Record::new(index_key.as_bytes().to_vec(), merged_metadata.merkle_root.as_bytes().to_vec());
                let _ = swarm.behaviour_mut().kademlia.put_record(index_record, quorum);
            }

            let _ = response_tx.send(merged_metadata);
        }
                                    Some(DhtCommand::StoreBlocks { blocks, root_cid, mut metadata, response_tx }) => {
                                        // StoreBlocks is used for publish paths that must guarantee `metadata.cids`
                                        // and the existence of the root CID block in bitswap.
                                        //
                                        // Root block format: JSON array of per-block CID strings.
                                        let block_cid_strings: Vec<String> =
                                            blocks.iter().map(|(cid, _)| cid.to_string()).collect();
                                        let root_block_data = match serde_json::to_vec(&block_cid_strings) {
                                            Ok(v) => v,
                                            Err(e) => {
                                                error!("Failed to serialize root CID list: {}", e);
                                                let _ = response_tx.send(Err(format!(
                                                    "Failed to serialize root CID list: {}",
                                                    e
                                                )));
                                                continue 'outer; // Abort this publish operation
                                            }
                                        };

                                        // 1) Store the root block itself (downloaders fetch this first).
                                        if let Err(e) = swarm
                                            .behaviour_mut()
                                            .bitswap
                                            .insert_block::<MAX_MULTIHASH_LENGHT>(root_cid.clone(), root_block_data)
                                        {
                                            error!("Failed to store root block {} in bitswap: {}", root_cid, e);
                                            let _ = event_tx
                                                .send(DhtEvent::Error(format!(
                                                    "Failed to store root block {}: {}",
                                                    root_cid, e
                                                )))
                                                .await;
                                            let _ = response_tx.send(Err(format!(
                                                "Failed to store root block {}: {}",
                                                root_cid, e
                                            )));
                                            continue 'outer;
                                        }

                                        // 2) Store all data blocks in bitswap.
                                        for (cid, data) in blocks {
                                            if let Err(e) = swarm
                                                .behaviour_mut()
                                                .bitswap
                                                .insert_block::<MAX_MULTIHASH_LENGHT>(cid.clone(), data)
                                            {
                                                error!("Failed to store block {} in bitswap: {}", cid, e);
                                                let _ = event_tx
                                                    .send(DhtEvent::Error(format!(
                                                        "Failed to store block {}: {}",
                                                        cid, e
                                                    )))
                                                    .await;
                                                let _ = response_tx.send(Err(format!(
                                                    "Failed to store block {}: {}",
                                                    cid, e
                                                )));
                                                continue 'outer; // Abort this publish operation
                                            }
                                        }

                                        // 3. Update metadata with the root CID
                                        metadata.cids = Some(vec![root_cid.clone()]);

                                            // Serialize CIDs as strings for DHT storage.
                                            // (This manual JSON construction bypasses FileMetadata's custom serde hooks.)
                                            let cids_as_strings: Option<Vec<String>> = metadata
                                                .cids
                                                .as_ref()
                                                .map(|v| v.iter().map(|c| c.to_string()).collect());

                                            // 3. Create and publish the DHT record pointing to the file (use camelCase keys)
                                            let dht_metadata = serde_json::json!({
                                                "merkleRoot": metadata.merkle_root,
                                                "fileName": metadata.file_name,
                                                "fileSize": metadata.file_size,
                                                "createdAt": metadata.created_at,
                                                "mimeType": metadata.mime_type,
                                                "isEncrypted": metadata.is_encrypted,
                                                "encryptionMethod": metadata.encryption_method,
                                                "keyFingerprint": metadata.key_fingerprint,
                                                "cids": cids_as_strings,
                                                "encryptedKeyBundle": metadata.encrypted_key_bundle,
                                                "ftpSources": metadata.ftp_sources,
                                                "ed2kSources": metadata.ed2k_sources,
                                                "httpSources": metadata.http_sources,
                                                "infoHash": metadata.info_hash,
                                                "trackers": metadata.trackers,
                                                "parentHash": metadata.parent_hash,
                                                "price": metadata.price,
                                                "uploader_address": metadata.uploader_address,
                                                "seeders": metadata.seeders,
                                            });

                                            let record_key =
                                                kad::RecordKey::new(&metadata.merkle_root.as_bytes());
                                            let record_value = match serde_json::to_vec(&dht_metadata) {
                                                Ok(val) => val,
                                                Err(e) => {
                                                    warn!("Failed to serialize DHT metadata: {}", e);
                                                    let _ = response_tx.send(Err(format!(
                                                        "Failed to serialize DHT metadata: {}",
                                                        e
                                                    )));
                                                    continue 'outer;
                                                }
                                            };
                                            let record = Record {
                                                key: record_key.clone(),
                                                value: record_value,
                                                publisher: Some(peer_id),
                                                expires: None,
                                            };
                                        if let Err(e) = swarm
                                            .behaviour_mut()
                                            .kademlia
                                            .put_record(record, kad::Quorum::One)
                                        {
                                            error!(
                                                "Failed to put record for file {}: {}",
                                                metadata.merkle_root, e
                                            );
                                            let _ = response_tx.send(Err(format!(
                                                "Failed to publish DHT record for {}: {}",
                                                metadata.merkle_root, e
                                            )));
                                            continue 'outer;
                                        }

                                            // 4. Announce self as provider (only if we have dialable addrs)
                                            if !swarm_has_dialable_addr(&swarm) {
                                                warn!("🛑 Not registering encrypted file {} as provider: no dialable address (set CHIRAL_PUBLIC_IP or enable UPnP)", metadata.merkle_root);
                                                {
                                                    let mut pending = pending_provider_registrations.lock().await;
                                                    pending.insert(metadata.merkle_root.clone());
                                                }
                                                let _ = event_tx
                                                    .send(DhtEvent::Warning(format!(
                                                        "Not registering {} as provider: no dialable address (set CHIRAL_PUBLIC_IP or enable UPnP)",
                                                        metadata.merkle_root
                                                    )))
                                                    .await;
                                            } else {
                                                let provider_key = kad::RecordKey::new(&metadata.merkle_root.as_bytes());
                                                if let Err(e) = swarm.behaviour_mut().kademlia.start_providing(provider_key) {
                                                    error!("Failed to start providing encrypted file {}: {}", metadata.merkle_root, e);
                                                }
                                            }

                                            // Cache the published encrypted file locally
                                            // Merge with existing metadata if it exists (for multi-protocol support)
                                            {
                                                let mut cache = file_metadata_cache.lock().await;
                                                let merged_metadata = if let Some(existing) = cache.get(&metadata.merkle_root) {
                                                    merge_file_metadata(existing.clone(), metadata.clone())
                                                } else {
                                                    metadata.clone()
                                                };
                                                cache.insert(metadata.merkle_root.clone(), merged_metadata);
                                            }
                                            info!("Cached published encrypted file {} locally", metadata.merkle_root);

                                            info!("Successfully published and started providing encrypted file: {}", metadata.merkle_root);
                                            let _ = event_tx.send(DhtEvent::PublishedFile(metadata)).await;
                                            // Acknowledge completion so publish_file() can return only after DHT record is written.
                                            let _ = response_tx.send(Ok(()));
                                        }
                                    Some(DhtCommand::DownloadFile(mut file_metadata, download_path)) =>{
                                        info!("🎬 DownloadFile command received for: {} to: {}", file_metadata.file_name, download_path);
                                        info!("🎬 file has cids: {:?}", file_metadata.cids);
                                        // Dual-lookup check: If the merkle_root is an info_hash, resolve it first.
                                        if file_metadata.merkle_root.starts_with("info_hash:") {
                                            let info_hash = file_metadata.merkle_root.clone();
                                            info!("Download initiated with info_hash, resolving to merkle_root: {}", info_hash);
                                            match synchronous_search_by_infohash(&mut swarm, &info_hash).await {
                                                Ok(Some(resolved_metadata)) => {
                                                    info!("Resolved info_hash to merkle_root: {}", resolved_metadata.merkle_root);
                                                    file_metadata = resolved_metadata; // Replace with the full metadata
                                                }
                                                Ok(None) => {
                                                    let _ = event_tx.send(DhtEvent::Error(format!("Could not find file for info_hash: {}", info_hash))).await;
                                                    continue;
                                                }
                                                Err(e) => {
                                                    let _ = event_tx.send(DhtEvent::Error(format!("Error resolving info_hash {}: {}", info_hash, e))).await;
                                                    continue;
                                                }
                                            }
                                        }

                                        // --------------------------------------------------------------------
                                        // Bitswap download path (requires cids + enabled bitswap handler)
                                        // --------------------------------------------------------------------
                                        let enable_bitswap = std::env::var("CHIRAL_ENABLE_BITSWAP").ok().is_some()
                                            || std::env::var("CHIRAL_E2E_API_PORT").ok().is_some();

                                        if enable_bitswap {
                                            if let Some(cids) = &file_metadata.cids {
                                                if !cids.is_empty() {
                                                    if file_metadata.seeders.is_empty() {
                                                        let _ = event_tx.send(DhtEvent::Error("No seeders found".to_string())).await;
                                                        continue;
                                                    }

                                                    // Set the target path so the bitswap handler can write to disk.
                                                    file_metadata.download_path = Some(download_path.clone());

                                                    // Request the root CID (contains the list of chunk CIDs).
                                                    let root_cid = cids[0].clone();
                                                    let peer_id = match PeerId::from_str(&file_metadata.seeders[0]) {
                                                        Ok(id) => id,
                                                        Err(e) => {
                                                            let _ = event_tx.send(DhtEvent::Error(format!("Invalid seeder peer id: {}", e))).await;
                                                            continue;
                                                        }
                                                    };

                                                    let query_id = swarm.behaviour_mut().bitswap.get_from(&root_cid, peer_id);
                                                    root_query_mapping.lock().await.insert(query_id, file_metadata.clone());

                                                    info!(
                                                        "🎬 Started Bitswap download: rootCid={} queryId={:?} file={}",
                                                        root_cid, query_id, file_metadata.merkle_root
                                                    );
                                                    continue;
                                                }
                                            }
                                        }

                                        // Calculate total chunks from cids or file size
                                        let total_chunks = if let Some(cids) = &file_metadata.cids {
                                            cids.len() as u32
                                        } else {
                                            // If no CIDs, calculate from file size (assume 256KB chunks)
                                            let chunk_size = 256 * 1024;
                                            ((file_metadata.file_size + chunk_size - 1) / chunk_size) as u32
                                        };

                                        if total_chunks == 0 {
                                            let _ = event_tx.send(DhtEvent::Error("File has no chunks".to_string())).await;
                                            continue;
                                        }

                                        if file_metadata.seeders.is_empty() {
                                            let _ = event_tx.send(DhtEvent::Error("No seeders found".to_string())).await;
                                            return;
                                        }

                                        // Use WebRTC service if available, otherwise fall back to error
                                        if let Some(webrtc_service) = &webrtc_service {
                                            // Select first seeder
                                            let seeder = &file_metadata.seeders[0];

                                            info!("Starting WebRTC download from seeder {}: {} ({} chunks)",
                                                  seeder, file_metadata.file_name, total_chunks);

                                            file_metadata.download_path = Some(download_path.clone());

                                            // Request all chunks via WebRTC
                                            let file_hash = file_metadata.merkle_root.clone();
                                            for chunk_index in 0..total_chunks {
                                                if let Err(e) = webrtc_service
                                                    .request_file_chunk(
                                                        seeder.clone(),
                                                        file_hash.clone(),
                                                        chunk_index
                                                    )
                                                    .await
                                                {
                                                    error!("Failed to request chunk {} for file {}: {}",
                                                           chunk_index, file_hash, e);
                                                }
                                            }

                                            info!("Requested {} chunks for file {}", total_chunks, file_hash);
                                        } else {
                                            let _ = event_tx.send(DhtEvent::Error(
                                                "WebRTC service not available for download".to_string()
                                            )).await;
                                        }
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
                                            // Build minimal "empty" metadata (use camelCase keys)
                                            let empty_meta = serde_json::json!({
                                                "merkleRoot": file_hash,
                                                "fileName": serde_json::Value::Null,
                                                "fileSize": 0u64,
                                                "createdAt": unix_timestamp(),
                                                "seeders": Vec::<String>::new(),
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

                                            debug!("StopPublish completed for {}", file_hash);
                                        }
                                    Some(DhtCommand::SearchFile { file_hash, sender }) => {
                                        info!("🔍 Received search command for file: {}", file_hash);
                                        info!("🔍 Initiating DHT queries for file search");
                                        // Query both the metadata record AND the provider records
                                        // This ensures we find the file even if only provider announcements exist
                                        let key = kad::RecordKey::new(&file_hash.as_bytes());

                                        // Create a pending search query to track both lookups
                                        let mut pending_query = PendingSearchQuery::new(file_hash.clone(), sender);

                                        // Start record lookup
                                        let record_query_id = swarm.behaviour_mut().kademlia.get_record(key.clone());
                                        info!("🔍 Searching for file metadata: {} (record query: {:?})", file_hash, record_query_id);
                                        pending_query.record_query_id = Some(record_query_id);

                                        // Track both queries under the record query ID (primary)
                                        pending_search_queries.lock().await.insert(record_query_id, pending_query);
                                    }
                                    Some(DhtCommand::SearchByInfohash { info_hash, sender }) => {
                                        let index_key = format!("{}{}", INFO_HASH_PREFIX, info_hash);
                                        let record_key = kad::RecordKey::new(&index_key.as_bytes());
                                        let query_id = swarm.behaviour_mut().kademlia.get_record(record_key.clone());
                                        info!("Searching for info_hash index: {} (query: {:?})", index_key, query_id);

                                        // Store the sender so we can respond when the query completes.
                                        // This is the first step of the two-step lookup.
                                        let search = PendingInfohashSearch { id: 0, sender };
                                        pending_infohash_searches.lock().await.insert(query_id, search);
                                    }
                                    Some(DhtCommand::SearchPeersByInfohash { info_hash, sender }) => {
                                        let key = kad::RecordKey::new(&info_hash.as_bytes());
                                        let query_id = swarm.behaviour_mut().kademlia.get_providers(key);
                                        info!("Searching for torrent providers (info_hash): {} (query: {:?})", info_hash, query_id);

                                        get_providers_queries.lock().await.insert(query_id, (info_hash.clone(), std::time::Instant::now()));
                                        let pending_query = PendingProviderQuery {
                                            id: 0,
                                            sender,
                                        };
                                        pending_provider_queries.lock().await.insert(info_hash, pending_query);
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
                                            // Skip self-connection attempts for privacy proxies
                                            if let Some(peer_id_in_addr) = &maybe_peer_id {
                                                if peer_id_in_addr == &peer_id {
                                                    continue;
                                                }
                                            }

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

                                                {
                                                    let mut mgr = proxy_mgr.lock().await;
                                                    mgr.set_target(peer_id.clone());
                                                }

                                                match swarm.dial(multiaddr.clone()) {
                                                    Ok(_) => {
                                                        info!("Requested direct connection to: {}", addr);
                                                        info!("  Multiaddr: {}", multiaddr);
                                                        info!("  Waiting for ConnectionEstablished event...");
                                                    }
                                                    Err(e) => {
                                                        if let Some(fallback_addr) =
                                                            fallback_dial_addr_without_terminal_peer(
                                                                &multiaddr,
                                                            )
                                                        {
                                                            warn!(
                                                                "Dial with terminal /p2p failed, retrying stripped dial addr={} fallback={} err={}",
                                                                multiaddr, fallback_addr, e
                                                            );
                                                            match swarm.dial(fallback_addr.clone()) {
                                                                Ok(_) => {
                                                                    info!(
                                                                        "Requested direct connection using stripped dial addr: {}",
                                                                        fallback_addr
                                                                    );
                                                                    info!("  Waiting for ConnectionEstablished event...");
                                                                }
                                                                Err(retry_err) => {
                                                                    error!(
                                                                        "Failed to dial {} (fallback {}): {}",
                                                                        addr, fallback_addr, retry_err
                                                                    );
                                                                    let _ = event_tx
                                                                        .send(DhtEvent::Error(format!(
                                                                            "Failed to connect: {}",
                                                                            retry_err
                                                                        )))
                                                                        .await;
                                                                }
                                                            }
                                                        } else {
                                                            error!("Failed to dial {}: {}", addr, e);
                                                            let _ = event_tx
                                                                .send(DhtEvent::Error(format!(
                                                                    "Failed to connect: {}",
                                                                    e
                                                                )))
                                                                .await;
                                                        }
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
                                    Some(DhtCommand::ConnectPeerMultiaddr(multiaddr)) => {
                                        info!("Attempting to connect to: {}", multiaddr);
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

                                            {
                                                let mut mgr = proxy_mgr.lock().await;
                                                mgr.set_target(peer_id.clone());
                                            }

                                            match swarm.dial(multiaddr.clone()) {
                                                Ok(_) => {
                                                    info!("Requested direct connection to: {}", multiaddr);
                                                    info!("  Waiting for ConnectionEstablished event...");
                                                }
                                                Err(e) => {
                                                    if let Some(fallback_addr) =
                                                        fallback_dial_addr_without_terminal_peer(
                                                            &multiaddr,
                                                        )
                                                    {
                                                        warn!(
                                                            "Dial with terminal /p2p failed, retrying stripped dial addr={} fallback={} err={}",
                                                            multiaddr, fallback_addr, e
                                                        );
                                                        match swarm.dial(fallback_addr.clone()) {
                                                            Ok(_) => {
                                                                info!(
                                                                    "Requested direct connection using stripped dial addr: {}",
                                                                    fallback_addr
                                                                );
                                                                info!(
                                                                    "  Waiting for ConnectionEstablished event..."
                                                                );
                                                            }
                                                            Err(retry_err) => {
                                                                error!(
                                                                    "Failed to dial {} (fallback {}): {}",
                                                                    multiaddr, fallback_addr, retry_err
                                                                );
                                                                let _ = event_tx
                                                                    .send(DhtEvent::Error(format!(
                                                                        "Failed to connect: {}",
                                                                        retry_err
                                                                    )))
                                                                    .await;
                                                            }
                                                        }
                                                    } else {
                                                        error!("Failed to dial {}: {}", multiaddr, e);
                                                        let _ = event_tx
                                                            .send(DhtEvent::Error(format!(
                                                                "Failed to connect: {}",
                                                                e
                                                            )))
                                                            .await;
                                                    }

                                                    if has_private_ip {
                                                        warn!(
                                                            "⚠️ Failed to dial private IP address - this may indicate NAT/firewall issues"
                                                        );
                                                    }
                                                }
                                            }
                                        } else {
                                            error!("No peer ID found in multiaddr: {}", multiaddr);
                                            let _ = event_tx
                                                .send(DhtEvent::Error(format!(
                                                    "Invalid address format: {}",
                                                    multiaddr
                                                )))
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

                                        // Store the query_id → (file_hash, start_time) mapping for error handling and timeout detection
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
                                    Some(DhtCommand::RequestFileAccess { seeder, merkle_root, recipient_public_key, sender }) => {
                                        info!("Requesting file access from seeder {} for file {}", seeder, merkle_root);

                                        // Convert PublicKey to Vec<u8> for the KeyRequest
                                        let recipient_pk_bytes = recipient_public_key.to_bytes().to_vec();

                                        // Create the key request
                                        let key_request = KeyRequest {
                                            merkle_root: merkle_root.clone(),
                                            recipient_public_key: recipient_pk_bytes,
                                        };

                                        // Send the request using the key_request behavior
                                        let request_id = swarm.behaviour_mut().key_request.send_request(&seeder, key_request);

                                        // Store the pending request
                                        pending_key_requests.lock().await.insert(request_id, sender);

                                        info!("Sent key request to seeder {} for file {} (request_id: {:?})", seeder, merkle_root, request_id);
                                    }
                                    Some(DhtCommand::AnnounceTorrent { info_hash }) => {
                                        let key = kad::RecordKey::new(&info_hash);
                                        match swarm.behaviour_mut().kademlia.start_providing(key) {
                                            Ok(query_id) => {
                                                info!("Started providing torrent with info_hash: {}, query_id: {:?}", info_hash, query_id);
                                                let _ = event_tx.send(DhtEvent::Info(format!("Announced torrent: {}", info_hash))).await;
                                            }
                                            Err(e) => {
                                                error!("Failed to start providing torrent {}: {}", info_hash, e);
                                                let _ = event_tx.send(DhtEvent::Error(format!("Failed to announce torrent: {}", e))).await;
                                            }
                                        }
                                    }
                                    Some(DhtCommand::PutDhtValue { key, value, sender }) => {
                                        info!("[KEY] Storing DHT value with key: {} ({} bytes)", key, value.len());
                                        let record_key = kad::RecordKey::new(&key);
                                        let record = kad::Record {
                                            key: record_key,
                                            value,
                                            publisher: None,
                                            expires: None,
                                        };

                                        match swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One) {
                                            Ok(query_id) => {
                                                info!("✅ DHT put started: key={}, query_id={:?}", key, query_id);
                                                let _ = sender.send(Ok(()));
                                            }
                                            Err(e) => {
                                                error!("❌ DHT put failed for key {}: {}", key, e);
                                                let _ = sender.send(Err(format!("Failed to store in DHT: {}", e)));
                                            }
                                        }
                                    }
                                    Some(DhtCommand::GetDhtValue { key, sender }) => {
                                        info!("🔍 Fetching DHT value with key: {}", key);
                                        let record_key = kad::RecordKey::new(&key);
                                        let query_id = swarm.behaviour_mut().kademlia.get_record(record_key);
                                        info!("🔍 DHT get started: key={}, query_id={:?}", key, query_id);

                                        // Store the sender to respond when we get the Kademlia result
                                        pending_dht_queries.lock().await.insert(query_id, sender);
                                    }
                                    Some(DhtCommand::ReBootstrap { sender }) => {
                                        info!("🔄 Re-bootstrapping DHT to discover new peers...");
                                        let initial_peer_count = connected_peers.lock().await.len();

                                        // Trigger Kademlia bootstrap
                                        match swarm.behaviour_mut().kademlia.bootstrap() {
                                            Ok(query_id) => {
                                                info!("✅ Re-bootstrap initiated (query_id: {:?})", query_id);

                                                // Update metrics
                                                {
                                                    let mut m = metrics.lock().await;
                                                    m.last_bootstrap = Some(SystemTime::now());
                                                }

                                                // Wait a bit for bootstrap to find peers
                                                tokio::spawn(async move {
                                                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                                    // Note: We can't accurately report new peers here since bootstrap is async
                                                    // The sender will get a success result, actual peer count changes happen over time
                                                    let _ = sender.send(Ok(0)); // 0 indicates bootstrap started but count unknown
                                                });
                                            }
                                            Err(e) => {
                                                error!("❌ Re-bootstrap failed: {:?}", e);
                                                let mut m = metrics.lock().await;
                                                m.bootstrap_failures = m.bootstrap_failures.saturating_add(1);
                                                m.last_error = Some(format!("Re-bootstrap failed: {:?}", e));
                                                m.last_error_at = Some(SystemTime::now());
                                                drop(m);
                                                let _ = sender.send(Err(format!("Re-bootstrap failed: {:?}", e)));
                                            }
                                        }
                                    }
                                    Some(DhtCommand::HealthCheck { min_peers, auto_recover, sender }) => {
                                        let peer_count = connected_peers.lock().await.len();
                                        let m = metrics.lock().await;

                                        let last_bootstrap_secs_ago = m.last_bootstrap.map(|t| {
                                            SystemTime::now()
                                                .duration_since(t)
                                                .map(|d| d.as_secs())
                                                .unwrap_or(0)
                                        });

                                        let healthy = peer_count >= min_peers;
                                        let bootstrap_failures = m.bootstrap_failures;
                                        drop(m);

                                        let recommendation = if !healthy {
                                            if peer_count == 0 {
                                                Some("No peers connected. Check network connectivity and bootstrap nodes.".to_string())
                                            } else {
                                                Some(format!(
                                                    "Peer count ({}) below minimum ({}). Consider re-bootstrapping.",
                                                    peer_count, min_peers
                                                ))
                                            }
                                        } else {
                                            None
                                        };

                                        let mut recovery_triggered = false;

                                        // Auto-recover if unhealthy and requested
                                        if !healthy && auto_recover {
                                            info!("🔄 Auto-recovery: triggering re-bootstrap (peers: {}, min: {})", peer_count, min_peers);
                                            if let Ok(_) = swarm.behaviour_mut().kademlia.bootstrap() {
                                                recovery_triggered = true;
                                                let mut m = metrics.lock().await;
                                                m.last_bootstrap = Some(SystemTime::now());
                                            }
                                        }

                                        let _ = sender.send(DhtHealthStatus {
                                            healthy,
                                            peer_count,
                                            min_required: min_peers,
                                            bootstrap_failures,
                                            last_bootstrap_secs_ago,
                                            recommendation,
                                            recovery_triggered,
                                        });
                                    }
                                    Some(DhtCommand::GetPeerAddresses { peer_ids, sender }) => {
                                        let mut addresses_map = HashMap::new();

                                        // First, collect all k-bucket entries to avoid lifetime issues
                                        let all_entries: Vec<_> = swarm
                                            .behaviour_mut()
                                            .kademlia
                                            .kbuckets()
                                            .flat_map(|bucket| {
                                                bucket.iter().map(|entry| {
                                                    (*entry.node.key.preimage(), entry.node.value.iter().cloned().collect::<Vec<_>>())
                                                }).collect::<Vec<_>>()
                                            })
                                            .collect();

                                        // Now find addresses for requested peer IDs
                                        for peer_id in peer_ids {
                                            if let Some((_, addrs)) = all_entries.iter().find(|(id, _)| id == &peer_id) {
                                                if !addrs.is_empty() {
                                                    addresses_map.insert(peer_id, addrs.clone());
                                                }
                                            }
                                        }

                                        let _ = sender.send(addresses_map);
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
                                            &pending_infohash_searches,
                                            &file_metadata_cache,
                                            &pending_dht_queries,
                                            &pending_search_queries,
                                        )
                                        .await;
                                    }
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::Identify(identify_event)) => {
                                        handle_identify_event(
                                            identify_event,
                                            &mut swarm,
                                            &event_tx,
                                            metrics.clone(),
                                            &proxy_mgr,
                                            &peer_selection,
                                            &peer_id,
                                        )
                                        .await;
                                    }
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::Mdns(mdns_event)) if !is_bootstrap => {
                                        if !is_bootstrap{
                                            handle_mdns_event(mdns_event, &mut swarm, &event_tx, &peer_id).await;
                                        }
                                    }
                                    // Bitswap handler is enabled in E2E runs (or when CHIRAL_ENABLE_BITSWAP is set).
                                    // This is required for Bitswap-based downloads to complete.
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::Bitswap(bitswap))
                                        if !is_bootstrap
                                            && (std::env::var("CHIRAL_ENABLE_BITSWAP").ok().is_some()
                                                || std::env::var("CHIRAL_E2E_API_PORT").ok().is_some()) =>
                                    match bitswap {
                                        beetswap::Event::GetQueryResponse { query_id, data } => {
                                            info!("📥 Received Bitswap block (query_id: {:?}, size: {} bytes)", query_id, data.len());

                                            // Check if this is a root block query first
                                            if let Some(metadata) = root_query_mapping.lock().await.remove(&query_id) {
                                                info!("✅ This is a ROOT BLOCK for file: {}", metadata.merkle_root);

                                                // This is the root block containing CIDs - parse and request all data blocks
                                                // Try multiple formats for backward compatibility
                                                let parse_result: Result<Vec<String>, String> =
                                                    // Try 1: Vec<String> (current format)
                                                    serde_json::from_slice::<Vec<String>>(&data)
                                                        .map_err(|e| format!("Vec<String>: {}", e))
                                                        .or_else(|_| {
                                                            // Try 2: Single CID string (legacy format)
                                                            serde_json::from_slice::<String>(&data)
                                                                .map(|s| vec![s])
                                                                .map_err(|e| format!("String: {}", e))
                                                        })
                                                        .or_else(|_| {
                                                            // Try 3: Vec<Vec<u8>> (binary CID array - legacy format)
                                                            serde_json::from_slice::<Vec<Vec<u8>>>(&data)
                                                                .map_err(|e| format!("Vec<Vec<u8>>: {}", e))
                                                                .and_then(|cid_bytes_array| {
                                                                    info!("Parsing root block as Vec<Vec<u8>> (binary CID format)");
                                                                    let mut cid_strings = Vec::new();
                                                                    for cid_bytes in cid_bytes_array {
                                                                        match Cid::try_from(cid_bytes.as_slice()) {
                                                                            Ok(cid) => {
                                                                                info!("Successfully parsed binary CID: {}", cid);
                                                                                cid_strings.push(cid.to_string());
                                                                            }
                                                                            Err(e) => {
                                                                                return Err(format!("Failed to parse CID from bytes: {}", e));
                                                                            }
                                                                        }
                                                                    }
                                                                    Ok(cid_strings)
                                                                })
                                                        })
                                                        .or_else(|_| {
                                                            // Try 4: IPLD CID wrapper format
                                                            #[derive(serde::Deserialize)]
                                                            struct CidWrapper {
                                                                #[serde(rename = "/")]
                                                                link: String,
                                                            }

                                                            serde_json::from_slice::<Vec<CidWrapper>>(&data)
                                                                .map(|wrappers| wrappers.into_iter().map(|w| w.link).collect())
                                                                .map_err(|e| format!("Vec<CidWrapper>: {}", e))
                                                        });

                                                match parse_result {
                                                    Ok(cid_strings) => {
                                                        // Convert CID strings to Cid objects
                                                        let mut cids = Vec::new();
                                                        for cid_str in cid_strings {
                                                            match cid_str.parse::<Cid>() {
                                                                Ok(cid) => cids.push(cid),
                                                                Err(e) => {
                                                                    error!("Failed to parse CID from string '{}': {}", cid_str, e);
                                                                    continue;
                                                                }
                                                            }
                                                        }

                                                        if cids.is_empty() {
                                                            error!("No valid CIDs found in root block for file {}", metadata.merkle_root);
                                                            continue;
                                                        }

                                                        // Create queries map for this file's data blocks
                                                        let mut file_queries = HashMap::new();
                                                        let peer_id = match PeerId::from_str(&metadata.seeders[0]) {
                                                            Ok(id) => id.clone(),
                                                            Err(e) => {let _ = event_tx.send(DhtEvent::Error(e.to_string())).await; continue; }
                                                        };

                                                        for (i, cid) in cids.iter().enumerate() {
                                                            // Request the root block which contains the CIDs
                                                            let block_query_id = swarm.behaviour_mut().bitswap.get_from(&cid, peer_id);
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
                                                        let download_path = match metadata.download_path.as_ref() {
                                                            Some(path_str) => PathBuf::from_str(path_str),
                                                            None => {
                                                                error!("Download path not defined for file: {}", metadata.merkle_root);
                                                                return;
                                                            }
                                                        };
                                                        let download_path = match download_path {
                                                            Ok(path) => get_available_download_path(path).await,
                                                            Err(e) => {
                                                                error!("Invalid download path for file {}: {}", metadata.merkle_root, e);
                                                                return;
                                                            }
                                                        };

                                                        // Clone file_queries since it will be moved to ActiveDownload::new()
                                                        let file_queries_clone = file_queries.clone();

                                                    // Create active download with memory-mapped file
                                                    match ActiveDownload::new(
                                                        metadata.clone(),
                                                        file_queries_clone,
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
                                                            "🎬 Started tracking download for file {} with {} chunks (chunk_size: {} bytes)",
                                                            metadata.merkle_root, cids.len(), chunk_size
                                                        );

                                                        // Log the query mappings for debugging (clone again since it was moved)
                                                        for (qid, idx) in &file_queries {
                                                            debug!("Query {:?} -> chunk {}", qid, idx);
                                                        }
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
                                                        let data_preview = String::from_utf8_lossy(&data);
                                                        error!("Failed to parse root block in any supported format for file {}", metadata.merkle_root);
                                                        error!("  Tried formats: Vec<String>, String, Vec<Vec<u8>>, Vec<CidWrapper>");
                                                        error!("  Errors: {}", e);
                                                        error!("  Raw data (first 200 bytes): {}",
                                                            if data_preview.len() > 200 { &data_preview[..200] } else { &data_preview });
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
                                                        info!("📦 Processing successful chunk {} for file {}", chunk_index, file_hash);
                                                        found = true;

                                                            // This query belongs to this file - write the chunk to disk
                                                            let offset = active_download.chunk_offsets
                                                                .get(chunk_index as usize)
                                                                .copied()
                                                                .unwrap_or_else(|| {
                                                                    error!("No offset found for chunk_index: {}", chunk_index);
                                                                    0
                                                                });


                                                            if let Err(e) = active_download.write_chunk(chunk_index, &data, offset) {
                                                                error!("Failed to write chunk {} to disk for file {}: {}",
                                                                    chunk_index, file_hash, e);
                                                                break;
                                                            }

                                                            info!("Successfully wrote chunk {}/{} for file {}",
                                                                chunk_index + 1,
                                                                active_download.total_chunks,
                                                                file_hash);

                                                            info!("📤 Emitting BitswapChunkDownloaded event: file_hash={}, chunk={}/{}",
                                                                file_hash, chunk_index, active_download.total_chunks);

                                                            let _ = event_tx.send(DhtEvent::BitswapChunkDownloaded {
                                                                file_hash: file_hash.clone(),
                                                                chunk_index,
                                                                total_chunks: active_download.total_chunks,
                                                                chunk_size: data.len(),
                                                            }).await;

                                                            // --- Reputation System Integration ---
                                                            // Reward the peer who sent this chunk.
                                                            // The `peer` ID is part of the GetQueryResponse event.
                                                            // The `peer` field was removed. We get the seeder from the metadata.
                                                            let seeder = match active_download.metadata.seeders.first() {
                                                                Some(s) => s.clone(),
                                                                None => continue, // Should not happen if we got a response
                                                            };

                                                            let _ = event_tx.send(DhtEvent::ReputationEvent {
                                                                peer_id: seeder.to_string(),
                                                                event_type: "TorrentChunkSeeded".to_string(),
                                                                impact: 2.0, // Use the default impact from EventType
                                                                data: serde_json::json!({
                                                                    "file_hash": file_hash,
                                                                    "chunk_index": chunk_index,
                                                                    "chunk_size": data.len(),
                                                                    "timestamp": unix_timestamp(),
                                                                }),
                                                            }).await;
                                                            debug!(
                                                                "Rewarded peer {} for seeding chunk {} of file {}",
                                                                seeder,
                                                                chunk_index,
                                                                file_hash
                                                            );

                                                           // Check if download is complete after receiving this chunk
                let queries_remaining = active_download.queries.len();
                let received_count = active_download.received_chunks.lock().unwrap().len();
                let total_expected = active_download.total_chunks as usize;

                info!("File {} progress: {}/{} chunks received, {} queries remaining",
                      file_hash, received_count, total_expected, queries_remaining);

                if active_download.is_complete() {
                    info!("🎉 Download complete for file {}! Finalizing...", file_hash);
                    // Flush and finalize the file
                    match active_download.finalize() {
                        Ok(_) => {
                            info!("Successfully finalized file {}", file_hash);
                        }
                        Err(e) => {
                            error!("Failed to finalize file {}: {}", file_hash, e);
                            break;
                        }
                    }

                    // Create completed metadata with the correct absolute path
                    let mut completed_metadata = active_download.metadata.clone();
                    completed_metadata.download_path = Some(
                        active_download.final_file_path
                            .to_string_lossy()
                            .to_string()
                    );
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
                                        beetswap::Event::GetQueryError {
                                            query_id,
                                            error,
                                        } => {
                                            // Handle Bitswap query error
                                            error!("❌ Bitswap query {:?} failed: {:?}", query_id, error);

                                            // Check if any active downloads contain this failed query
                                            {
                                                let mut active_downloads_guard = active_downloads.lock().await;
                                                let mut completed_downloads = Vec::new();

                                                for (file_hash, active_download_lock) in active_downloads_guard.iter_mut() {
                                                    let mut active_download = active_download_lock.lock().await;
                                                    if active_download.queries.remove(&query_id).is_some() {
                                                        warn!("❌ Query {:?} failed for file {}, but continuing with remaining chunks", query_id, file_hash);

                                                        // Check if download is still complete with remaining chunks
                                                        if active_download.is_complete() {
                                                            info!("File {} completed despite failed chunk", file_hash);
                                                            // Flush and finalize the file
                                                            match active_download.finalize() {
                                                                Ok(_) => {
                                                                    info!("Successfully finalized file {}", file_hash);
                                                                }
                                                                Err(e) => {
                                                                    error!("Failed to finalize file {}: {}", file_hash, e);
                                                                    continue;
                                                                }
                                                            }

                                                            // Create completed metadata with the correct absolute path
                                                            let mut completed_metadata = active_download.metadata.clone();
                                                            completed_metadata.download_path = Some(
                                                                active_download.final_file_path
                                                                    .to_string_lossy()
                                                                    .to_string()
                                                            );
                                                            completed_downloads.push(completed_metadata);
                                                        }
                                                    }
                                                }

                                                // Remove completed downloads from active downloads
                                                for metadata in &completed_downloads {
                                                    active_downloads_guard.remove(&metadata.merkle_root);
                                                }

                                                // Send completion events for finished downloads
                                                for metadata in completed_downloads {
                                                    info!("Emitting DownloadedFile event for: {} (after chunk failure)", metadata.merkle_root);
                                                    if let Err(e) = event_tx.send(DhtEvent::DownloadedFile(metadata)).await {
                                                        error!("Failed to send DownloadedFile event: {}", e);
                                                    }
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

                                                let show = {
                                                    let mut mgr = proxy_mgr.lock().await;
                                                    mgr.set_ping_rtt(peer.clone(), rtt_ms);
                                                    mgr.is_proxy(&peer)
                                                };

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
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::AutonatClient(ev)) if !is_bootstrap => {
                                        handle_autonat_client_event(&mut swarm, ev, &metrics, &event_tx).await;
                                    }
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::AutonatServer(ev)) if !is_bootstrap => {
                                        debug!(?ev, "AutoNAT server event");
                                    }
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::Upnp(upnp_event)) => {
                                        handle_upnp_event(upnp_event, &mut swarm, &event_tx).await;
                                    }
                                    SwarmEvent::ExternalAddrConfirmed { address, .. } if !is_bootstrap => {
                                        handle_external_addr_confirmed(&mut swarm, &address, &metrics, &event_tx, &proxy_mgr, &pending_provider_registrations, pure_client_mode, force_server_mode)
                                            .await;
                                    }
                                    SwarmEvent::ExternalAddrExpired { address, .. } if !is_bootstrap => {
                                        handle_external_addr_expired(&address, &metrics, &event_tx, &proxy_mgr)
                                            .await;
                                    }
                                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, .. } => {
                                        let remote_addr = endpoint.get_remote_address().clone();

                                        // Initialize peer metrics for smart selection
                                        {
                                            let mut selection = peer_selection.lock().await;
                                            let peer_metrics = PeerMetrics::new(
                                                peer_id.to_string(),
                                                remote_addr.to_string(),
                                            );
                                            selection.update_peer_metrics(peer_metrics);
                                        }

                                        // Add peer to Kademlia routing table
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

                                        info!("✅ Connected to {} (connection #{})", peer_id, num_established);
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
                                        swarm.behaviour_mut().kademlia.remove_peer(&peer_id);

                                        let peers_count = {
                                            let mut peers = connected_peers.lock().await;
                                            peers.remove(&peer_id);
                                            peers.len()
                                        };
                                        if !is_bootstrap{
                                            // Remove proxy state
                                            proxy_mgr.lock().await.remove_all(&peer_id);
                                        }

                                        let _ = event_tx
                                            .send(DhtEvent::PeerDisconnected {
                                                peer_id: peer_id.to_string(),
                                            })
                                            .await;
                                    }
                                    SwarmEvent::NewListenAddr { address, .. } => {
                                        // Attempt to find an IPv4 protocol within the Multiaddr
                                        if let Some(Protocol::Ip4(v4)) = address.iter().find(|p| matches!(p, Protocol::Ip4(_))) {

                                            // Determine reachability: allow all in tests, otherwise reject loopback/private
                                            let is_reachable = if cfg!(test) {
                                                true
                                            } else {
                                                !v4.is_loopback() && !v4.is_private()
                                            };

                                            if is_reachable {
                                                // Record in metrics for monitoring
                                                if let Ok(mut m) = metrics.try_lock() {
                                                    m.record_listen_addr(&address);
                                                }

                                                swarm.add_external_address(address.clone());
                                                info!("✅ Advertising reachable address: {}", address);
                                            }
                                        }
                                    }
                                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                                        if let Ok(mut m) = metrics.try_lock() {
                                            m.last_error = Some(error.to_string());
                                            m.last_error_at = Some(SystemTime::now());
                                            m.bootstrap_failures = m.bootstrap_failures.saturating_add(1);
                                        }
                                        if let Some(pid) = peer_id {
                                            let is_bootstrap = bootstrap_peer_ids.contains(&pid);
                                            let error_str = error.to_string();

                                            swarm.behaviour_mut().kademlia.remove_peer(&pid);
                                            // Only log error for addresses that should be reachable
                                                // Rate limit connection errors to once every 30 seconds
                                                let now = SystemTime::now()
                                                    .duration_since(UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_millis() as u64;
                                                let last_log = LAST_CONNECTION_ERROR_LOG.load(Ordering::Relaxed);
                                                if now.saturating_sub(last_log) >= 30_000 { // 30 seconds
                                                    LAST_CONNECTION_ERROR_LOG.store(now, Ordering::Relaxed);
                                                    error!("❌ Outgoing connection error to {}: {}", pid, error);
                                                }

                                                if error_str.contains("rsa") {
                                                    error!("   ℹ️ Hint: This node uses RSA keys. Enable 'rsa' feature if needed.");
                                                } else if error_str.contains("Timeout") {
                                                    if is_bootstrap {
                                                        warn!("   ℹ️ Hint: Bootstrap nodes may be unreachable or overloaded.");
                                                    } else {
                                                        warn!("   ℹ️ Hint: Peer may be unreachable (timeout).");
                                                    }
                                                } else if error_str.contains("Connection refused") {
                                                    if is_bootstrap {
                                                        warn!("   ℹ️ Hint: Bootstrap nodes are not accepting connections.");
                                                    } else {
                                                        warn!("   ℹ️ Hint: Peer is not accepting connections.");
                                                    }
                                                } else if error_str.contains("Transport") {
                                                    warn!("   ℹ️ Hint: Transport protocol negotiation failed.");
                                                }
                                        } else {
                                            // Rate limit connection errors to once every 30 seconds
                                            let now = SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis() as u64;
                                            let last_log = LAST_CONNECTION_ERROR_LOG.load(Ordering::Relaxed);
                                            if now.saturating_sub(last_log) >= 30_000 { // 30 seconds
                                                LAST_CONNECTION_ERROR_LOG.store(now, Ordering::Relaxed);
                                                error!("❌ Outgoing connection error to unknown peer: {}", error);
                                            }
                                        }
                                        let _ = event_tx.send(DhtEvent::Error(format!("Connection failed: {}", error))).await;
                                    }
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::ProxyRr(ev)) if !is_bootstrap => {
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
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::WebrtcSignalingRr(ev)) if !is_bootstrap => {
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
                                    SwarmEvent::IncomingConnectionError { error, .. } if !is_bootstrap => {
                                        if let Ok(mut m) = metrics.try_lock() {
                                            m.last_error = Some(error.to_string());
                                            m.last_error_at = Some(SystemTime::now());
                                            m.bootstrap_failures = m.bootstrap_failures.saturating_add(1);
                                        }
                                    }
                                    SwarmEvent::Behaviour(DhtBehaviourEvent::KeyRequest(ev)) => {
                                        use libp2p::request_response::{Event as RREvent, Message};
                                        match ev {
                                            // Incoming key request (we're the seeder)
                                            RREvent::Message { peer, message } => match message {
                                                Message::Request { request, channel, .. } => {
                                                    let KeyRequest { merkle_root, recipient_public_key } = request;
                                                    info!("Received key request from peer {} for file {}", peer, merkle_root);

                                                    // Look up file metadata in cache
                                                    let file_metadata_cache_guard = file_metadata_cache.lock().await;
                                                    let result = if let Some(metadata) = file_metadata_cache_guard.get(&merkle_root) {
                                                        // Check if file has encrypted key bundle
                                                        if let Some(key_bundle) = &metadata.encrypted_key_bundle {
                                                            info!("Found encrypted key bundle for file {} (merkle_root: {})", metadata.file_name, merkle_root);
                                                            Ok(KeyResponse {
                                                                encrypted_bundle: Some(key_bundle.clone()),
                                                                error: None,
                                                            })
                                                        } else {
                                                            warn!("File {} found but no encrypted key bundle available", merkle_root);
                                                            Ok(KeyResponse {
                                                                encrypted_bundle: None,
                                                                error: Some("File found but no encrypted key bundle available".to_string()),
                                                            })
                                                        }
                                                    } else {
                                                        warn!("File not found in cache for merkle_root: {}", merkle_root);
                                                        Ok(KeyResponse {
                                                            encrypted_bundle: None,
                                                            error: Some(format!("File not found: {}", merkle_root)),
                                                        })
                                                    };

                                                    drop(file_metadata_cache_guard);

                                                    // Send response
                                                    match result {
                                                        Ok(response) => {
                                                            swarm.behaviour_mut().key_request
                                                                .send_response(channel, response)
                                                                .unwrap_or_else(|e| error!("Failed to send key response: {e:?}"));
                                                        }
                                                        Err(e) => {
                                                            error!("Error processing key request: {}", e);
                                                            let error_response = KeyResponse {
                                                                encrypted_bundle: None,
                                                                error: Some(e),
                                                            };
                                                            swarm.behaviour_mut().key_request
                                                                .send_response(channel, error_response)
                                                                .unwrap_or_else(|e| error!("Failed to send error response: {e:?}"));
                                                        }
                                                    }
                                                }
                                                // Key response (we're the requester)
                                                Message::Response { request_id, response } => {
                                                    let KeyResponse { encrypted_bundle, error } = response;

                                                    if let Some(tx) = pending_key_requests.lock().await.remove(&request_id) {
                                                        match (encrypted_bundle, error) {
                                                            (Some(bundle), None) => {
                                                                info!("Received encrypted key bundle for request {:?}", request_id);
                                                                let _ = tx.send(Ok(bundle));
                                                            }
                                                            (None, Some(err)) => {
                                                                warn!("Key request failed: {}", err);
                                                                let _ = tx.send(Err(err));
                                                            }
                                                            (None, None) => {
                                                                warn!("Key request returned empty response");
                                                                let _ = tx.send(Err("Empty response from seeder".to_string()));
                                                            }
                                                            (Some(_), Some(err)) => {
                                                                warn!("Key request returned both bundle and error, using error: {}", err);
                                                                let _ = tx.send(Err(err));
                                                            }
                                                        }
                                                    } else {
                                                        warn!("Received key response for unknown request_id {:?}", request_id);
                                                    }
                                                }
                                            },
                                            RREvent::OutboundFailure { request_id, error, .. } => {
                                                warn!("Key request outbound failure: {error:?}");
                                                if let Some(tx) = pending_key_requests.lock().await.remove(&request_id) {
                                                    let _ = tx.send(Err(format!("Outbound failure: {error:?}")));
                                                }
                                            }
                                            RREvent::InboundFailure { error, .. } => {
                                                warn!("Key request inbound failure: {error:?}");
                                            }
                                            RREvent::ResponseSent { .. } => {}
                                        }
                                    }
                                    SwarmEvent::ListenerClosed { reason, .. } if !is_bootstrap => {
                                        if reason.is_ok() {
                                            trace!("ListenerClosed Ok; ignoring");
                                        } else {
                                            let s = format!("{:?}", reason);
                                            warn!("Listener closed with error: {}", s);
                                        }
                                    }
                                    _ => {}
                                }
                            } else {
                                info!("DHT swarm stream ended; shutting down node task");
                                break 'outer;
                            }
                        }

        // Poll WebRTC events for file chunk reception and download completion
        if let Some(webrtc) = &webrtc_service {
            let events = webrtc.drain_events(100).await;
            for event in events {
                match event {
                    crate::webrtc_service::WebRTCEvent::FileChunkReceived { peer_id, chunk } => {
                        info!(
                            "📥 Received WebRTC chunk {}/{} from peer {} for file {}",
                            chunk.chunk_index + 1,
                            chunk.total_chunks,
                            peer_id,
                            chunk.file_hash
                        );
                    }
                    crate::webrtc_service::WebRTCEvent::TransferProgress { peer_id, progress } => {
                        info!(
                            "📊 Transfer progress from {}: {:.1}%",
                            peer_id, progress.percentage
                        );
                    }
                    crate::webrtc_service::WebRTCEvent::TransferCompleted {
                        peer_id,
                        file_hash,
                    } => {
                        info!(
                            "✅ WebRTC transfer completed: {} from peer {}",
                            file_hash, peer_id
                        );

                        // Look up file metadata and emit DownloadedFile event
                        let cache = file_metadata_cache.lock().await;
                        if let Some(metadata) = cache.get(&file_hash) {
                            let _ = event_tx
                                .send(DhtEvent::DownloadedFile(metadata.clone()))
                                .await;
                            info!("Emitted DownloadedFile event for {}", file_hash);
                        } else {
                            warn!(
                                "File metadata not found in cache for completed download: {}",
                                file_hash
                            );
                        }
                    }
                    crate::webrtc_service::WebRTCEvent::TransferFailed {
                        peer_id,
                        file_hash,
                        error,
                    } => {
                        error!(
                            "❌ WebRTC transfer failed: {} from peer {}: {}",
                            file_hash, peer_id, error
                        );
                        let _ = event_tx
                            .send(DhtEvent::Error(format!(
                                "WebRTC transfer failed for {}: {}",
                                file_hash, error
                            )))
                            .await;
                    }
                    crate::webrtc_service::WebRTCEvent::FileChunkRequested {
                        peer_id,
                        file_hash,
                        chunk_index,
                    } => {
                        info!(
                            "📤 Peer {} requested chunk {} of file {}",
                            peer_id, chunk_index, file_hash
                        );

                        // Look up file metadata and serve the chunk
                        let cache = file_metadata_cache.lock().await;
                        if let Some(metadata) = cache.get(&file_hash).cloned() {
                            drop(cache); // Release lock before async operations

                            // Get file data from file transfer service
                            if let Some(ft_service) = &file_transfer_service {
                                match ft_service.get_file_data(&file_hash).await {
                                    Some(file_data) => {
                                        // Calculate chunk boundaries
                                        let start = (chunk_index as usize) * chunk_size;
                                        let end = (start + chunk_size).min(file_data.len());

                                        if start < file_data.len() {
                                            let chunk_data = file_data[start..end].to_vec();

                                            // Calculate total chunks
                                            let total_chunks = ((file_data.len() + chunk_size - 1)
                                                / chunk_size)
                                                as u32;

                                            // Calculate checksum
                                            let checksum = {
                                                use sha2::{Digest, Sha256};
                                                let mut hasher = Sha256::new();
                                                hasher.update(&chunk_data);
                                                format!("{:x}", hasher.finalize())
                                            };

                                            // Create chunk struct
                                            let chunk = crate::webrtc_service::FileChunk {
                                                file_hash: file_hash.clone(),
                                                file_name: metadata.file_name.clone(),
                                                chunk_index,
                                                total_chunks,
                                                data: chunk_data,
                                                checksum,
                                                encrypted_key_bundle: metadata
                                                    .encrypted_key_bundle
                                                    .clone(),
                                            };

                                            // Send chunk to peer
                                            if let Err(e) =
                                                webrtc.send_file_chunk(peer_id.clone(), chunk).await
                                            {
                                                error!(
                                                    "Failed to send chunk {} to {}: {}",
                                                    chunk_index, peer_id, e
                                                );
                                            } else {
                                                info!(
                                                    "✅ Sent chunk {} to peer {}",
                                                    chunk_index, peer_id
                                                );
                                            }
                                        } else {
                                            warn!(
                                                "Chunk index {} out of bounds for file {}",
                                                chunk_index, file_hash
                                            );
                                        }
                                    }
                                    None => {
                                        warn!("File data not found for {}", file_hash);
                                    }
                                }
                            } else {
                                warn!("FileTransferService not available to serve chunks");
                            }
                        } else {
                            warn!(
                                "File metadata not found in cache for chunk request: {}",
                                file_hash
                            );
                        }
                    }
                    crate::webrtc_service::WebRTCEvent::ConnectionEstablished { peer_id } => {
                        info!("WebRTC connection established with {}", peer_id);
                    }
                    crate::webrtc_service::WebRTCEvent::ConnectionFailed { peer_id, error } => {
                        warn!("WebRTC connection failed with {}: {}", peer_id, error);
                    }
                    _ => {
                        // Ignore other WebRTC events (signaling, ICE, etc.)
                    }
                }
            }
        }
    }

    connected_peers.lock().await.clear();
    info!("DHT node task exiting");
    if let Some(ack) = shutdown_ack {
        let _ = ack.send(());
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

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
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
    pending_infohash_searches: &Arc<Mutex<HashMap<kad::QueryId, PendingInfohashSearch>>>,
    file_metadata_cache: &Arc<Mutex<HashMap<String, FileMetadata>>>,
    pending_dht_queries: &Arc<
        Mutex<HashMap<kad::QueryId, oneshot::Sender<Result<Option<Vec<u8>>, String>>>>,
    >,
    pending_search_queries: &Arc<Mutex<HashMap<kad::QueryId, PendingSearchQuery>>>,
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
        }
        KademliaEvent::OutboundQueryProgressed { id, result, .. } => {
            match result {
                QueryResult::GetRecord(Ok(ok)) => match ok {
                    GetRecordOk::FoundRecord(peer_record) => {
                        // Check if this is a response to a generic DHT value query (e.g., reputation verdicts)
                        if let Some(sender) = pending_dht_queries.lock().await.remove(&id) {
                            info!(
                                "✅ DHT get successful: found {} bytes",
                                peer_record.record.value.len()
                            );
                            let _ = sender.send(Ok(Some(peer_record.record.value.clone())));
                            return; // Don't process further as this was a raw DHT query
                        }

                        // Check if this is a response to a file search query
                        if let Some(pending_search) =
                            pending_search_queries.lock().await.remove(&id)
                        {
                            let search_file_hash = pending_search.file_hash.clone();
                            info!(
                                "📥 Received search result for query ID: {:?}, searching for: {}",
                                id, search_file_hash
                            );

                            // This is a search result - parse it and send it back
                            match serde_json::from_slice::<serde_json::Value>(
                                &peer_record.record.value,
                            ) {
                                Ok(metadata_json) => {
                                    // Debug: Log the raw metadata JSON
                                    info!("🔍 Raw metadata JSON: {}", metadata_json);

                                    // Construct FileMetadata from the JSON
                                    // NOTE: We accept both snake_case and camelCase because different publish paths
                                    // historically stored different key styles (e.g., Bitswap vs legacy paths).
                                    let merkle_root = metadata_json
                                        .get("merkle_root")
                                        .and_then(|v| v.as_str())
                                        .or_else(|| {
                                            metadata_json.get("merkleRoot").and_then(|v| v.as_str())
                                        });
                                    let file_name = metadata_json
                                        .get("file_name")
                                        .and_then(|v| v.as_str())
                                        .or_else(|| {
                                            metadata_json.get("fileName").and_then(|v| v.as_str())
                                        });
                                    let file_size = metadata_json
                                        .get("file_size")
                                        .and_then(|v| v.as_u64())
                                        .or_else(|| {
                                            metadata_json.get("fileSize").and_then(|v| v.as_u64())
                                        });
                                    let created_at = metadata_json
                                        .get("created_at")
                                        .and_then(|v| v.as_u64())
                                        .or_else(|| {
                                            metadata_json.get("createdAt").and_then(|v| v.as_u64())
                                        });

                                    info!("🔍 Parsed fields - merkleRoot: {:?}, fileName: {:?}, fileSize: {:?}, createdAt: {:?}", merkle_root, file_name, file_size, created_at);

                                    if let (
                                        Some(file_hash),
                                        Some(file_name_val),
                                        Some(file_size_val),
                                        Some(created_at_val),
                                    ) = (merkle_root, file_name, file_size, created_at)
                                    {
                                        info!("🔍 Found metadata record - merkleRoot: {}, searching for: {}", file_hash, search_file_hash);
                                        // Verify this is the file we were searching for
                                        if file_hash == search_file_hash {
                                            info!(
                                                "⚙️ Constructing metadata for found file: {}",
                                                file_hash
                                            );
                                            let mut metadata =
                                                construct_file_metadata_from_json_simple(
                                                    &metadata_json,
                                                    file_hash,
                                                    file_name_val,
                                                    file_size_val,
                                                    created_at_val,
                                                );
                                            info!("⚙️ Metadata constructed successfully");

                                            // Merge providers with existing seeders from metadata
                                            info!("⚙️ Merging providers with metadata seeders");
                                            if let Some(providers) = &pending_search.found_providers
                                            {
                                                // Add providers that aren't already in seeders
                                                for provider in providers {
                                                    if !metadata.seeders.contains(provider) {
                                                        metadata.seeders.push(provider.clone());
                                                    }
                                                }
                                                info!("✅ Found searched file: {} ({}) with {} seeders (merged from metadata + providers)", file_name_val, file_hash, metadata.seeders.len());
                                            } else {
                                                info!("✅ Found searched file: {} ({}) with {} seeders from metadata", file_name_val, file_hash, metadata.seeders.len());
                                            }

                                            // Merge with local cache to preserve multi-protocol metadata
                                            // This ensures that if we uploaded via both WebRTC and Bitswap locally,
                                            // the search result will include CIDs from local cache even if DHT
                                            // record only has one protocol's data
                                            info!("⚙️ Merging with local cache");
                                            {
                                                let cache = file_metadata_cache.lock().await;
                                                if let Some(cached) =
                                                    cache.get(&metadata.merkle_root)
                                                {
                                                    info!("🔍 Merging search result with local cache for {}", metadata.merkle_root);
                                                    metadata = merge_file_metadata(
                                                        cached.clone(),
                                                        metadata,
                                                    );
                                                }
                                            }

                                            // Send event to frontend for search results
                                            info!("🌐 Sending DhtEvent::FileDiscovered for file: {} (CIDs: {:?}, FTP: {})",
                                            metadata.file_name,
                                            metadata.cids.as_ref().map(|v| v.len()),
                                            metadata.ftp_sources.as_ref().map(|v| v.len()).unwrap_or(0));
                                            let _ = event_tx
                                                .send(DhtEvent::FileDiscovered(metadata.clone()))
                                                .await;
                                            info!(
                                                "🌐 Sending result through channel for file: {}",
                                                metadata.file_name
                                            );
                                            let _ = pending_search.sender.send(Ok(Some(metadata)));
                                            info!("✅ Search result processing completed successfully");
                                            return; // Successfully handled the search result
                                        } else {
                                            info!("❌ Hash mismatch - found metadata for {} but searching for {}", file_hash, search_file_hash);
                                        }
                                    } else {
                                        debug!("Received incomplete metadata record during search");
                                    }
                                }
                                Err(e) => {
                                    warn!("❌ Failed to parse metadata JSON: {}", e);
                                    info!("❌ Raw metadata bytes: {:?}", &peer_record.record.value);
                                }
                            }

                            // If we get here, put the search back for retry
                            let _ = pending_search_queries
                                .lock()
                                .await
                                .insert(id, pending_search);
                            return;
                        }
                        if let Ok(metadata_json) =
                            serde_json::from_slice::<serde_json::Value>(&peer_record.record.value)
                        {
                            // Check if this is a response to an info_hash index lookup
                            if let Some(search) = pending_infohash_searches.lock().await.remove(&id)
                            {
                                if let Ok(merkle_root) =
                                    String::from_utf8(peer_record.record.value.clone())
                                {
                                    info!("Resolved info_hash to merkle_root: {}", merkle_root);
                                    // Now, initiate the second step: search for the actual file metadata
                                    let record_key = kad::RecordKey::new(&merkle_root.as_bytes());
                                    let final_query_id =
                                        swarm.behaviour_mut().kademlia.get_record(record_key);

                                    // We need to re-insert the sender to be notified when the *second* query finishes.
                                    // This is a simplification. A more robust solution would use a state machine
                                    // to track multi-step queries. For now, we'll just re-use the infohash search map.
                                    // This assumes no overlapping queries for the same initial info_hash.
                                    pending_infohash_searches
                                        .lock()
                                        .await
                                        .insert(final_query_id, search);

                                    info!("Initiating second-step search for merkle_root: {} (query: {:?})", merkle_root, final_query_id);
                                } else {
                                    warn!("Failed to decode info_hash index value as string.");
                                    let _ = search.sender.send(None);
                                }
                                return; // End processing for this event here.
                            }

                            // Construct FileMetadata from the JSON
                            if let (
                                Some(file_hash),
                                Some(file_name),
                                Some(file_size),
                                Some(created_at),
                            ) = (
                                // Use merkle_root as the primary identifier
                                metadata_json.get("merkleRoot").and_then(|v| v.as_str()),
                                metadata_json.get("fileName").and_then(|v| v.as_str()),
                                metadata_json.get("fileSize").and_then(|v| v.as_u64()),
                                metadata_json.get("createdAt").and_then(|v| v.as_u64()),
                            ) {
                                let peer_from_record =
                                    peer_record.peer.clone().map(|p| p.to_string());

                                // Simply use seeders array directly
                                let seeders: Vec<String> = metadata_json
                                    .get("seeders")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect()
                                    })
                                    .unwrap_or_default();

// (dropped during rebase) heartbeat refresh merge logic from old branch; upstream main removed heartbeat.
                                let metadata = FileMetadata {
                                    merkle_root: file_hash.to_string(),
                                    file_name: file_name.to_string(),
                                    file_size,
                                    file_data: Vec::new(), // Will be populated during download
                                    seeders: if seeders.is_empty() {
                                        peer_from_record
                                            .clone()
                                            .into_iter()
                                            .collect::<Vec<String>>()
                                    } else {
                                        seeders.clone()
                                    },
                                    created_at,
                                    mime_type: metadata_json
                                        .get("mimeType")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    is_encrypted: metadata_json
                                        .get("isEncrypted")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false),
                                    encryption_method: metadata_json
                                        .get("encryptionMethod")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    key_fingerprint: metadata_json
                                        .get("keyFingerprint")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    parent_hash: metadata_json
                                        .get("parentHash")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    cids: metadata_json
                                        .get("cids")
                                        .and_then(|v| deserialize_cids_from_json(v)),
                                    encrypted_key_bundle: metadata_json
                                        .get("encryptedKeyBundle")
                                        .and_then(|v| {
                                            // The field name is camelCase in the JSON
                                            serde_json::from_value::<
                                                Option<crate::encryption::EncryptedAesKeyBundle>,
                                            >(v.clone())
                                            .unwrap_or(None)
                                        }),
                                    info_hash: metadata_json
                                        .get("infoHash")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    trackers: metadata_json.get("trackers").and_then(|v| {
                                        serde_json::from_value::<Option<Vec<String>>>(v.clone())
                                            .unwrap_or(None)
                                    }),
                                    is_root: metadata_json
                                        .get("is_root")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(true),
                                    price: metadata_json
                                        .get("price")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.0),
                                    http_sources: metadata_json.get("httpSources").and_then(|v| {
                                        serde_json::from_value::<Option<Vec<HttpSourceInfo>>>(
                                            v.clone(),
                                        )
                                        .unwrap_or(None)
                                    }),
                                    ed2k_sources: metadata_json.get("ed2kSources").and_then(|v| {
                                        serde_json::from_value::<Option<Vec<Ed2kSourceInfo>>>(
                                            v.clone(),
                                        )
                                        .unwrap_or(None)
                                    }),
                                    ftp_sources: metadata_json.get("ftpSources").and_then(|v| {
                                        serde_json::from_value::<Option<Vec<FtpSourceInfo>>>(
                                            v.clone(),
                                        )
                                        .unwrap_or(None)
                                    }),
                                    uploader_address: metadata_json
                                        .get("uploader_address")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    ..Default::default()
                                };

                                let notify_metadata = metadata.clone();
                                let file_hash = notify_metadata.merkle_root.clone();

                                // Cache the discovered file so subsequent searches don't need DHT queries
                                // DHT data is authoritative - always update cache with latest network state
                                let mut cache = file_metadata_cache.lock().await;
                                if let Some(existing) = cache.get(&file_hash) {
                                    // Merge if both exist to preserve any local-only fields
                                    let merged =
                                        merge_file_metadata(existing.clone(), metadata.clone());
                                    cache.insert(file_hash.clone(), merged);
                                    info!(
                                        "Merged DHT discovery with existing cache for file {}",
                                        file_hash
                                    );
                                } else {
                                    // No existing cache entry, just store DHT data
                                    cache.insert(file_hash.clone(), metadata.clone());
                                    info!("Cached discovered file {} from DHT", file_hash);
                                }

                                info!(
                                    "File discovered: {} ({})",
                                    notify_metadata.file_name, file_hash
                                );
                                // Don't send FileDiscovered events for general discoveries to avoid interfering with searches
                                // The frontend will discover files through other means (like browsing)
                                // // Don't send FileDiscovered events for general discoveries to avoid interfering with searches
                                // let _ = event_tx.send(DhtEvent::FileDiscovered(metadata)).await;

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
                        // Check if this was an infohash search that found no record
                        if let Some(search) = pending_infohash_searches.lock().await.remove(&id) {
                            info!("Infohash lookup completed: no record found");
                            let _ = search.sender.send(None);
                            return; // End processing for this event here.
                        }

                        // No additional records; do nothing here for other queries
                    }
                },
                QueryResult::GetRecord(Err(err)) => {
                    warn!("GetRecord error: {:?}", err);

                    // Check if this was a failed DHT value query
                    if let Some(sender) = pending_dht_queries.lock().await.remove(&id) {
                        info!("❌ DHT get failed: {:?}", err);
                        let _ = sender.send(Ok(None)); // Return None on error rather than Err
                        return;
                    }

                    // If the error includes the key, emit FileNotFound
                    if let kad::GetRecordError::NotFound { key, .. } = err {
                        let file_hash = String::from_utf8_lossy(key.as_ref()).to_string();

                        // Also check if this was a failed info_hash lookup
                        if let Some(search) = pending_infohash_searches.lock().await.remove(&id) {
                            warn!("Infohash or subsequent merkle_root lookup failed for query {:?}: Not Found", id);
                            let _ = search.sender.send(None);
                        }

                        // Don't immediately emit FileNotFound - wait to see if providers query succeeds
                        // The providers query was already initiated in SearchFile command
                        info!("Metadata record not found for {}, checking if providers query will succeed", file_hash);

                        // Set a delayed FileNotFound emission only if providers also aren't found
                        // This is handled by a timeout mechanism in the frontend
                        tokio::spawn({
                            let event_tx = event_tx.clone();
                            let file_hash = file_hash.clone();
                            let pending_searches = pending_searches.clone();
                            let get_providers_queries = get_providers_queries.clone();
                            async move {
                                // Wait for provider queries to complete before declaring not found
                                // Kademlia queries can take up to 30s, so give providers 5s to respond
                                tokio::time::sleep(Duration::from_secs(5)).await;

                                // Check if a providers query is still pending for this file
                                let has_pending_providers = {
                                    let queries = get_providers_queries.lock().await;
                                    queries.values().any(|(hash, _)| hash == &file_hash)
                                };

                                if !has_pending_providers {
                                    // No providers query pending, emit FileNotFound
                                    info!(
                                        "No providers found for {}, emitting FileNotFound",
                                        file_hash
                                    );
                                    let _ = event_tx
                                        .send(DhtEvent::FileNotFound(file_hash.clone()))
                                        .await;
                                    notify_pending_searches(
                                        &pending_searches,
                                        &file_hash,
                                        SearchResponse::NotFound,
                                    )
                                    .await;
                                }
                            }
                        });
                    }
                }
                QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                    let key_str = String::from_utf8_lossy(key.as_ref());
                    info!("✅ PutRecord completed successfully for key: {}", key_str);

                    // Check if this is an info_hash index
                    if key_str.starts_with(INFO_HASH_PREFIX) {
                        info!("✅ Info_hash index record stored in DHT: {}", key_str);
                    }
                }
                QueryResult::PutRecord(Err(err)) => {
                    error!("❌ PutRecord failed: {:?}", err);
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

                            // Try to connect using parallel address strategy for better success rate
                            let reachable_addrs: Vec<_> = peer_info
                                .addrs
                                .iter()
                                .filter(|addr| ma_plausibly_reachable(addr))
                                .collect();

                            if !reachable_addrs.is_empty() {
                                info!(
                                    "Attempting {} parallel connections to peer {}",
                                    reachable_addrs.len(),
                                    peer_info.peer_id
                                );

                                // Add all addresses to Kademlia routing table first
                                for addr in &reachable_addrs {
                                    swarm
                                        .behaviour_mut()
                                        .kademlia
                                        .add_address(&peer_info.peer_id, (*addr).clone());
                                }

                                // Dial all reachable addresses in parallel - libp2p will use fastest
                                let mut dial_success = false;
                                for addr in reachable_addrs {
                                    match swarm.dial(addr.clone()) {
                                        Ok(_) => {
                                            debug!(
                                                "Initiated connection to peer {} at {}",
                                                peer_info.peer_id, addr
                                            );
                                            connection_attempts += 1;
                                            dial_success = true;
                                        }
                                        Err(e) => {
                                            debug!(
                                                "Failed to dial peer {} at {}: {}",
                                                peer_info.peer_id, addr, e
                                            );
                                        }
                                    }
                                }

                                if dial_success {
                                    info!(
                                        "✅ Initiated {} connection attempts to peer {}",
                                        connection_attempts, peer_info.peer_id
                                    );
                                }
                            } else {
                                info!(
                                    "No reachable addresses found for peer {}",
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
                QueryResult::GetProviders(process_result) => {
                    match process_result {
                        Ok(kad::GetProvidersOk::FoundProviders { key, providers }) => {
                            info!("The array is: {:?}", providers);

                            let file_hash = String::from_utf8_lossy(key.as_ref()).to_string();

                            // Remove from pending queries tracking
                            get_providers_queries.lock().await.remove(&id);

                            info!(
                                "Found {} providers for file: {}",
                                providers.len(),
                                file_hash
                            );

                            // Convert providers to string format
                            let provider_strings: Vec<String> =
                                providers.iter().map(|p| p.to_string()).collect();

                            // Update pending search queries with found providers (for SearchFile queries)
                            // This is needed because SearchFile runs both GetRecord and GetProviders in parallel
                            {
                                let mut search_queries = pending_search_queries.lock().await;
                                // Find the pending search query by file_hash and update its found_providers
                                for (_query_id, pending_search) in search_queries.iter_mut() {
                                    if pending_search.file_hash == file_hash {
                                        pending_search.found_providers =
                                            Some(provider_strings.clone());
                                        info!(
                                            "✅ Updated pending search for {} with {} providers",
                                            file_hash,
                                            provider_strings.len()
                                        );
                                        break;
                                    }
                                }
                            }

                            // Provider results - check for direct queries first
                            // Check for direct provider queries (not from SearchFile)
                            let mut pending_queries = pending_provider_queries.lock().await;
                            if let Some(pending_query) = pending_queries.remove(&file_hash) {
                                let _ = pending_query.sender.send(Ok(provider_strings.clone()));
                            }
                        }
                        Ok(kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. }) => {
                            // Check if we have a pending query for this ID to get the file hash
                            let mut queries = get_providers_queries.lock().await;
                            if let Some((file_hash, _)) = queries.remove(&id) {
                                // No providers found (empty list) and no pending query
                                // This means both metadata and provider queries returned nothing
                                info!(
                                    "Provider query returned 0 providers for {}, file not found",
                                    file_hash
                                );

                                // Notify pending searches that the file was not found
                                notify_pending_searches(
                                    &pending_searches,
                                    &file_hash,
                                    SearchResponse::NotFound,
                                )
                                .await;

                                // Emit FileNotFound event
                                let _ = event_tx
                                    .send(DhtEvent::FileNotFound(file_hash.clone()))
                                    .await;
                            }
                        }
                        Err(err) => {
                            warn!("GetProviders query failed: {:?}", err);

                            // Extract file hash from error for proper cleanup
                            let kad::GetProvidersError::Timeout { key, .. } = &err;
                            let file_hash = String::from_utf8_lossy(key.as_ref()).to_string();

                            // Remove from pending queries tracking
                            get_providers_queries.lock().await.remove(&id);

                            // Notify pending searches
                            info!(
                                "Provider query failed for {}, notifying as not found",
                                file_hash
                            );
                            notify_pending_searches(
                                &pending_searches,
                                &file_hash,
                                SearchResponse::NotFound,
                            )
                            .await;
                            let _ = event_tx.send(DhtEvent::FileNotFound(file_hash)).await;
                        }
                    }
                }
                // QueryResult::Bootstrap(Ok(BootstrapOk {
                //     peer,
                //     num_remaining,
                // })) => {
                //     println!(
                //         "✅ Bootstrap query succeeded. Peer: {peer}, remaining: {num_remaining}"
                //     );
                //     if num_remaining == 0 {
                //         println!("🎯 Bootstrap fully complete!");
                //     }
                // }
                // QueryResult::Bootstrap(Err(BootstrapError::Timeout { peer, .. })) => {
                //     eprintln!("[TIME] Bootstrap timed out; contacted peers: {:?}", peer);
                // }
                // QueryResult::Bootstrap(Err(e)) => {
                //     eprintln!("❌ Bootstrap failed: {:?}", e);
                // }
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
    proxy_mgr: &ProxyMgr,
    peer_selection: &Arc<Mutex<PeerSelectionService>>,
    local_peer_id: &PeerId,
) {
    match event {
        IdentifyEvent::Received { peer_id, info, .. } => {
            info!("Identified peer {}: {:?}", peer_id, info.protocol_version);
            // Add identified peer to Kademlia routing table
            if info.protocol_version != EXPECTED_PROTOCOL_VERSION {
                warn!(
                    "Peer {} has a mismatched protocol version: '{}'. Expected: '{}'. Removing peer.",
                    peer_id,
                    info.protocol_version,
                    EXPECTED_PROTOCOL_VERSION
                );
                swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
            } else {
                let mut added_reachable = false;
                for addr in info.listen_addrs.clone() {
                    // Filter out loopback and private addresses (like Docker 172.17.x.x IPs)
                    // Only add addresses that are plausibly reachable from the internet
                    if ma_plausibly_reachable(&addr) {
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                        added_reachable = true;
                    } else {
                        debug!(
                            "Skipping unreachable address from peer {}: {}",
                            peer_id, addr
                        );
                    }
                }

                // If nothing was added (no public addrs), keep a single non-loopback
                // address as a fallback to avoid ending up with an empty address set.
                if !added_reachable {
                    if let Some(fallback) = info
                        .listen_addrs
                        .iter()
                        .find(|addr| ma_non_loopback_ipv4(addr))
                        .cloned()
                    {
                        debug!(
                            "Keeping fallback non-loopback address for peer {}: {}",
                            peer_id, fallback
                        );
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, fallback);
                    }
                }
            }
            // Skip processing our own peer info to prevent self-connection attempts
            if &peer_id == local_peer_id {
                return;
            }

        }
        IdentifyEvent::Pushed { peer_id, info, .. } => {}
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
    local_peer_id: &PeerId,
) {
    match event {
        MdnsEvent::Discovered(list) => {
            let mut discovered: HashMap<PeerId, Vec<String>> = HashMap::new();
            for (peer_id, multiaddr) in list {
                info!("mDNS discovered peer {} at {}", peer_id, multiaddr);
                // Skip self-discoveries to prevent self-connection attempts
                if peer_id == *local_peer_id {
                    continue;
                }
                match swarm.dial(multiaddr.clone()) {
                    Ok(_) => {
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, multiaddr.clone());
                        discovered
                            .entry(peer_id)
                            .or_insert_with(Vec::new)
                            .push(multiaddr.to_string());
                    }
                    Err(e) => warn!("❌ Failed to dial bootstrap {}: {}", multiaddr, e),
                }
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
                info!("mDNS expired peer {} at {}", peer_id, multiaddr);
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
    swarm: &mut Swarm<DhtBehaviour>,
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
    swarm.add_external_address(tested_addr.clone());
    info!(
        "Added {} to external address from autonat observed address.",
        tested_addr
    );

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
    let was_public = metrics_guard.reachability_state == NatReachabilityState::Public;
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

async fn handle_upnp_event(
    event: upnp::Event,
    swarm: &mut Swarm<DhtBehaviour>,
    event_tx: &mpsc::Sender<DhtEvent>,
) {
    match event {
        upnp::Event::NewExternalAddr(addr) => {
            info!("🌐 UPnP: Successfully mapped external address: {}", addr);

            // Add the external address to the swarm
            swarm.add_external_address(addr.clone());

            // Notify the UI
            let _ = event_tx
                .send(DhtEvent::Info(format!(
                    "✅ UPnP port mapping successful: {}",
                    addr
                )))
                .await;
        }
        upnp::Event::ExpiredExternalAddr(addr) => {
            warn!("[TIME] UPnP: External address expired: {}", addr);

            let _ = event_tx
                .send(DhtEvent::Warning(format!(
                    "UPnP port mapping expired: {}",
                    addr
                )))
                .await;
        }
        upnp::Event::GatewayNotFound => {
            warn!("⚠️  UPnP: No UPnP gateway found on network");
            warn!("    - Make sure your router supports UPnP/IGD");
            warn!("    - Check if UPnP is enabled in router settings");
            let _ = event_tx
                .send(DhtEvent::Info(
                    "UPnP not available on this network".to_string(),
                ))
                .await;
        }
        upnp::Event::NonRoutableGateway => {
            warn!("⚠️  UPnP: Gateway is not routable");
            warn!("    - Your router may be behind another NAT (carrier-grade NAT)");
            warn!("    - Direct connections may not be possible");

            let _ = event_tx
                .send(DhtEvent::Warning(
                    "UPnP gateway not routable - behind CGNAT?".to_string(),
                ))
                .await;
        }
    }
}

async fn flush_pending_providers(
    swarm: &mut Swarm<DhtBehaviour>,
    pending: &Arc<Mutex<HashSet<String>>>,
    event_tx: &mpsc::Sender<DhtEvent>,
) {
    if !swarm_has_dialable_addr(swarm) {
        return;
    }
    let hashes: Vec<String> = {
        let mut guard = pending.lock().await;
        guard.drain().collect()
    };
    for file_hash in hashes {
        let provider_key = kad::RecordKey::new(&file_hash.as_bytes());
        match swarm.behaviour_mut().kademlia.start_providing(provider_key) {
            Ok(_) => {
                info!("[ANNOUNCE] Re-announced provider record for {}", file_hash);
            }
            Err(e) => {
                warn!(
                    "Failed to re-announce provider record for {}: {}",
                    file_hash, e
                );
                let _ = event_tx
                    .send(DhtEvent::Warning(format!(
                        "Failed to re-announce provider for {}: {}",
                        file_hash, e
                    )))
                    .await;
            }
        }
    }
}

async fn handle_external_addr_confirmed(
    swarm: &mut Swarm<DhtBehaviour>,
    addr: &Multiaddr,
    metrics: &Arc<Mutex<DhtMetrics>>,
    event_tx: &mpsc::Sender<DhtEvent>,
    proxy_mgr: &ProxyMgr,
    pending_provider_registrations: &Arc<Mutex<HashSet<String>>>,
    pure_client_mode: bool,
    force_server_mode: bool,
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

    // Upgrade Kademlia to Server mode now that we're publicly reachable
    // This allows other nodes to fetch DHT records from us
    // Skip upgrade if in pure-client mode (cannot act as DHT server)
    if pure_client_mode {
        info!(
            "⚠️  Pure client mode enabled - staying in Client mode despite public reachability at {}",
            addr
        );
        info!("   Note: Node cannot seed files or act as DHT server in pure-client mode");
    } else {
        swarm.behaviour_mut().kademlia.set_mode(Some(Mode::Server));
        info!(
            "🔄 Upgraded Kademlia to Server mode - node is publicly reachable at {}",
            addr
        );
    }

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

    // Now that we have a confirmed reachable address, re-announce any pending providers.
    flush_pending_providers(swarm, pending_provider_registrations, event_tx).await;
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

}

impl Socks5Transport {
    pub fn new(proxy: SocketAddr) -> Self {
        Self { proxy }
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
    ed25519_secret_key: Arc<[u8; 32]>, // Store ed25519 secret for signing verdicts
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
    webrtc_service: Option<Arc<crate::webrtc_service::WebRTCService>>,
    // chunk_manager: Option<Arc<ChunkManager>>, // Not needed here
    pending_webrtc_offers: Arc<
        Mutex<
            HashMap<rr::OutboundRequestId, oneshot::Sender<Result<WebRTCAnswerResponse, String>>>,
        >,
    >,
    pending_key_requests: Arc<
        Mutex<
            HashMap<rr::OutboundRequestId, oneshot::Sender<Result<EncryptedAesKeyBundle, String>>>,
        >,
    >,
    pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>>,
    root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>>,
    active_downloads: Arc<Mutex<HashMap<String, Arc<Mutex<ActiveDownload>>>>>,
    get_providers_queries: Arc<Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>>,
    chunk_size: usize,
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
        download_path: &PathBuf, // Already the full file path from get_available_download_path
        total_size: u64,
        chunk_offsets: Vec<u64>,
    ) -> std::io::Result<Self> {
        let total_chunks = queries.len() as u32;

        // download_path is already the complete file path
        let final_file_path = download_path.clone();

        // Create temp file by replacing extension with .tmp
        let mut temp_file_path = download_path.clone();
        temp_file_path.set_extension("tmp");

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
        let mut mmap = self.mmap.lock().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Mutex lock failed: {}", e),
            )
        })?;
        let start = offset as usize;
        let end = start + data.len();

        if end > mmap.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Chunk {} would exceed file bounds", chunk_index),
            ));
        }

        mmap[start..end].copy_from_slice(data);
        self.received_chunks
            .lock()
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Mutex lock failed: {}", e),
                )
            })?
            .insert(chunk_index);

        Ok(())
    }

    fn is_complete(&self) -> bool {
        let queries_empty = self.queries.is_empty();
        let received_count = self.received_chunks.lock().unwrap().len();
        let has_all_chunks = received_count == self.total_chunks as usize;
        let complete = queries_empty && has_all_chunks;

        if complete {
            info!(
                "🎉 Download completion check: queries_empty={}, received_chunks={}/{}, COMPLETE!",
                queries_empty, received_count, self.total_chunks
            );
        } else {
            debug!(
                "Download progress check: queries_empty={}, received_chunks={}/{}",
                queries_empty, received_count, self.total_chunks
            );
        }

        complete
    }

    fn flush(&self) -> std::io::Result<()> {
        self.mmap
            .lock()
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Mutex lock failed: {}", e),
                )
            })?
            .flush()
    }

    fn read_complete_file(&self) -> std::io::Result<Vec<u8>> {
        let mmap = self.mmap.lock().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Mutex lock failed: {}", e),
            )
        })?;
        Ok(mmap.to_vec())
    }

    /// Finalize the download by renaming .tmp file to final filename
    fn finalize(&self) -> std::io::Result<()> {
        // First, flush to ensure all data is written
        self.flush()?;

        // Drop the mmap to release the file handle
        if let Ok(mmap_guard) = self.mmap.lock() {
            drop(mmap_guard);
        }

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
        let received = self
            .received_chunks
            .lock()
            .map(|chunks| chunks.len())
            .unwrap_or(0) as f32;
        received / self.total_chunks as f32
    }

    fn chunks_received(&self) -> usize {
        self.received_chunks
            .lock()
            .map(|chunks| chunks.len())
            .unwrap_or(0)
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

#[derive(Builder)]
#[builder(derive(Clone, Debug, Into))]
pub struct DhtConfig<'a> {
    #[builder(default)]
    pub port: u16,
    #[builder(default)]
    pub bootstrap_nodes: Vec<String>,
    pub secret: Option<String>,
    #[builder(default)]
    pub is_bootstrap: bool,
    #[builder(default)]
    pub enable_autonat: bool,
    #[builder(default = Duration::from_secs(30))]
    pub autonat_probe_interval: Duration,
    #[builder(default)]
    pub autonat_servers: Vec<String>,
    pub proxy_address: Option<String>,
    pub chunk_size_kb: Option<usize>,
    pub cache_size_mb: Option<usize>,
    #[builder(default)]
    pub enable_upnp: bool,
    pub blockstore_db_path: Option<&'a Path>,
    #[builder(default)]
    pub pure_client_mode: bool,
    #[builder(default)]
    pub force_server_mode: bool,
    pub publish_ttl: Option<Duration>,
}

impl DhtService {
    // should maybe be migrated to use DhtConfig at some point...
    async fn _new(
        port: u16,
        bootstrap_nodes: Vec<String>,
        secret: Option<String>,
        is_bootstrap: bool,
        enable_autonat: bool,
        autonat_probe_interval: Duration,
        autonat_servers: Vec<String>,
        proxy_address: Option<String>,
        file_transfer_service: Option<Arc<FileTransferService>>,
        webrtc_service: Option<Arc<crate::webrtc_service::WebRTCService>>,
        chunk_manager: Option<Arc<ChunkManager>>,
        chunk_size_kb: Option<usize>, // Chunk size in KB (default 256)
        cache_size_mb: Option<usize>, // Cache size in MB (default 1024)
        enable_upnp: bool,
        blockstore_db_path: Option<&Path>,
        pure_client_mode: bool,
        force_server_mode: bool,
        publish_ttl: Option<Duration>,
    ) -> Result<Self, Box<dyn Error>> {
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
        let (local_key, ed25519_secret_key) = match secret {
            Some(secret_str) => {
                let mut hasher = Sha256::new();
                hasher.update(secret_str.as_bytes());
                let digest = hasher.finalize();
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&digest[..32]);
                let keypair = identity::Keypair::ed25519_from_bytes(seed.clone())?;
                (keypair, seed)
            }
            None => {
                // For generated keypairs, we need to extract the secret
                // Generate from a random seed so we can keep the seed
                use rand::RngCore;
                let mut seed = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut seed);
                let keypair = identity::Keypair::ed25519_from_bytes(seed.clone())?;
                (keypair, seed)
            }
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
            // this is for mostly testing, in real world, should probably be in the hours
            let republication_interval = publish_ttl.map(|ttl| ttl / 10);
            kad_cfg.set_provider_record_ttl(publish_ttl);
            kad_cfg.set_provider_publication_interval(republication_interval);

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

        // Start in Client mode - will switch to Server after AutoNAT confirms public reachability
        // This prevents NAT'd nodes from advertising unreachable addresses in the DHT
        // which would cause other peers to fail when trying to fetch records from them
        // Developer override: force Server mode immediately if requested (for testing/debugging)
        if force_server_mode {
            kademlia.set_mode(Some(Mode::Server));
            info!("⚠️  Starting Kademlia in FORCED Server mode (developer override)");
            info!("   Note: This may cause connectivity issues if behind NAT/firewall");
        } else {
            kademlia.set_mode(Some(Mode::Client));
            info!("Starting Kademlia in Client mode (waiting for AutoNAT confirmation)");
        }

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

        let probe_interval = autonat_probe_interval;
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
        let autonat_client_toggle = toggle::Toggle::from(autonat_client_behaviour);
        let autonat_server_toggle = toggle::Toggle::from(autonat_server_behaviour);
        let mdns_toggle = toggle::Toggle::from(mdns_opt);

        // UPnP configuration for automatic port mapping
        let upnp_behaviour = if enable_upnp {
            info!("🌐 UPnP enabled - attempting automatic port mapping");
            Some(upnp::tokio::Behaviour::default())
        } else {
            info!("UPnP disabled");
            None
        };
        let upnp_toggle = toggle::Toggle::from(upnp_behaviour);
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

        // Create the swarm
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )?
            // .with_quic() seems to destablize peer connect/download, disabled for now until solution
            .with_behaviour(move |_| {
                // Configure ping with more aggressive keep-alive to prevent connection drops
                let ping_config = ping::Config::new()
                    .with_interval(Duration::from_secs(15)) // Ping every 15 seconds (default is 15s)
                    .with_timeout(Duration::from_secs(20)); // Timeout after 20 seconds (default is 20s)

                DhtBehaviour {
                    kademlia,
                    identify,
                    mdns: mdns_toggle,
                    bitswap,
                    ping: Ping::new(ping_config),
                    proxy_rr,
                    webrtc_signaling_rr,
                    key_request,
                    autonat_client: autonat_client_toggle,
                    autonat_server: autonat_server_toggle,
                    upnp: upnp_toggle,
                }
            })?
            .with_swarm_config(
                |c| c.with_idle_connection_timeout(Duration::from_secs(300)), // 5 minutes
            )
            .build();

        // Always listen on the specified port
        let tcp_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
        swarm.listen_on(tcp_addr)?;

        // QUIC also bound to the same port (udp), seems to destablize peer connect/download, disabled for now until solution
        // let quic_addr: Multiaddr = format!("/ip4/0.0.0.0/udp/{}/quic-v1", port).parse()?;
        // swarm.listen_on(quic_addr)?;
        {
            let mut addrs_to_remove: Vec<(PeerId, Multiaddr)> = Vec::new();

            // kbuckets() already returns an iterator, use it directly
            for bucket in swarm.behaviour_mut().kademlia.kbuckets() {
                for entry in bucket.iter() {
                    let peer_id = entry.node.key.preimage();
                    // entry.node.value is of type Addresses, which implements IntoIterator
                    // We need to iterate over it and clone each address
                    for addr in entry.node.value.iter() {
                        if !ma_plausibly_reachable(addr) {
                            addrs_to_remove.push((*peer_id, addr.clone()));
                        }
                    }
                }
            }

            for (peer_id, addr) in addrs_to_remove {
                swarm
                    .behaviour_mut()
                    .kademlia
                    .remove_address(&peer_id, &addr);
                debug!(
                    "[CLEAN] Cleaned up unreachable address at startup: {} -> {}",
                    peer_id, addr
                );
            }
        }

        // ---- advertise external addresses so peers can reach us
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
        // and don't filter based on reachability (important for local testing)
        let mut successful_connections = 0;
        let total_bootstrap_nodes = bootstrap_nodes.len();
        for bootstrap_addr in &bootstrap_nodes {
            if let Ok(addr) = bootstrap_addr.parse::<Multiaddr>() {
                // WAN Mode: skip unroutable bootstrap addresses
                // LAN Mode: allow private/loopback addresses for local development and testing
                let wan_mode = enable_autonat;
                if wan_mode && !ma_plausibly_reachable(&addr) {
                    warn!(
                        "[SKIP]  [WAN Mode] Skipping unreachable bootstrap addr: {}",
                        addr
                    );
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
                    Err(e) => warn!("❌ Failed to dial bootstrap {}: {}", bootstrap_addr, e),
                }
            } else {
                warn!("❌ Invalid bootstrap address format: {}", bootstrap_addr);
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

        // Trigger initial bootstrap only if we successfully connected to at least one bootstrap node
        // Kademlia bootstrap requires at least one peer in the routing table to work
        if !bootstrap_nodes.is_empty() {
            if successful_connections > 0 {
                let _ = swarm.behaviour_mut().kademlia.bootstrap();
                info!(
                    "✅ Starting Kademlia bootstrap with {} bootstrap connection(s)",
                    successful_connections
                );
            } else {
                warn!("⚠️ No bootstrap connections succeeded - cannot bootstrap DHT");
                warn!("  Node will operate in standalone mode until peers connect");
                warn!("  Consider checking network connectivity and bootstrap node addresses");
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
        let pending_key_requests = Arc::new(Mutex::new(HashMap::new()));
        let pending_provider_queries: Arc<Mutex<HashMap<String, PendingProviderQuery>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let root_query_mapping: Arc<Mutex<HashMap<beetswap::QueryId, FileMetadata>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let active_downloads: Arc<Mutex<HashMap<String, Arc<Mutex<ActiveDownload>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let get_providers_queries_local: Arc<
            Mutex<HashMap<kad::QueryId, (String, std::time::Instant)>>,
        > = Arc::new(Mutex::new(HashMap::new()));
        let pending_infohash_searches: Arc<Mutex<HashMap<kad::QueryId, PendingInfohashSearch>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_dht_queries: Arc<
            Mutex<HashMap<kad::QueryId, oneshot::Sender<Result<Option<Vec<u8>>, String>>>>,
        > = Arc::new(Mutex::new(HashMap::new()));
        // Add this initialization around line 6100 after pending_dht_queries:
        let pending_search_queries: Arc<Mutex<HashMap<kad::QueryId, PendingSearchQuery>>> =
            Arc::new(Mutex::new(HashMap::new()));
        {
            let mut guard = metrics.lock().await;
            guard.autonat_enabled = enable_autonat;
        }

        // Spawn the Dht node task
        let received_chunks_clone = Arc::new(Mutex::new(HashMap::new()));
        let bootstrap_peer_ids = extract_bootstrap_peer_ids(&bootstrap_nodes);
        let file_metadata_cache_local: Arc<Mutex<HashMap<String, FileMetadata>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_provider_registrations: Arc<Mutex<HashSet<String>>> =
            Arc::new(Mutex::new(HashSet::new()));

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
            pending_infohash_searches.clone(),
            peer_selection.clone(),
            received_chunks_clone.clone(),
            file_transfer_service.clone(),
            webrtc_service.clone(),
            chunk_manager,
            pending_webrtc_offers.clone(),
            pending_provider_queries.clone(),
            root_query_mapping.clone(),
            active_downloads.clone(),
            get_providers_queries_local.clone(),
            pending_provider_registrations.clone(),
            file_metadata_cache_local.clone(),
            pending_dht_queries.clone(),
            pending_key_requests.clone(),
            pending_search_queries.clone(),
            is_bootstrap,
            chunk_size,
            bootstrap_peer_ids,
            pure_client_mode,
            force_server_mode,
        ));

        Ok(DhtService {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            peer_id: peer_id_str,
            ed25519_secret_key: Arc::new(ed25519_secret_key),
            connected_peers,
            connected_addrs: HashMap::new(),
            metrics,
            pending_echo,
            pending_searches,
            search_counter,
            proxy_mgr,
            peer_selection,
            file_metadata_cache: file_metadata_cache_local,
            received_chunks: received_chunks_clone,
            file_transfer_service,
            webrtc_service,
            // chunk_manager is not stored in DhtService, only passed to the task
            pending_webrtc_offers,
            pending_key_requests,
            pending_provider_queries,
            root_query_mapping,
            active_downloads,
            get_providers_queries: get_providers_queries_local,
            chunk_size,
        })
    }

    pub async fn new(
        config: DhtConfig<'_>,
        file_transfer_service: Option<Arc<FileTransferService>>,
        webrtc_service: Option<Arc<crate::webrtc_service::WebRTCService>>,
        chunk_manager: Option<Arc<ChunkManager>>,
    ) -> Result<Self, Box<dyn Error>> {
        // Call the existing function by destructuring the config
        Self::_new(
            config.port,
            config.bootstrap_nodes,
            config.secret,
            config.is_bootstrap,
            config.enable_autonat,
            config.autonat_probe_interval,
            config.autonat_servers,
            config.proxy_address,
            file_transfer_service,
            webrtc_service,
            chunk_manager,
            config.chunk_size_kb,
            config.cache_size_mb,
            config.enable_upnp,
            config.blockstore_db_path,
            config.pure_client_mode,
            config.force_server_mode,
            config.publish_ttl,
        )
        .await
    }

    pub fn chunk_size(&self) -> usize {
        // Note: This might need to be adjusted if chunk_manager is the source of truth
        self.chunk_size
    }

    pub async fn publish_file(
        &self,
        mut metadata: FileMetadata,
        ftp_sources: Option<Vec<FtpSourceInfo>>,
    ) -> Result<(), String> {
        // Add FTP sources to metadata before publishing
        if let Some(sources) = ftp_sources {
            metadata.ftp_sources = Some(sources.into_iter().map(|s| s.for_dht_storage()).collect());
        }

        // --- Bitswap publish responsibility (one-shot fix) ---
        //
        // In headless E2E, the uploader may call publish_file with `file_data` populated but `cids` unset.
        // If we just write a DHT record, downloaders have no root CID and Bitswap cannot start.
        //
        // We treat "inline file_data present + no cids" as the Bitswap publish path and:
        // - split file_data into bitswap blocks
        // - compute the root CID (CID of the JSON list of block CIDs)
        // - store blocks + root block in bitswap via StoreBlocks command
        // - publish metadata with `cids=[root_cid]` and WITHOUT inline file_data (avoid DHT size limits)
        let needs_bitswap_cids = (metadata.cids.as_ref().map(|v| v.is_empty()).unwrap_or(true))
            && !metadata.file_data.is_empty()
            && !metadata.is_encrypted
            && metadata.http_sources.is_none()
            && metadata.ftp_sources.is_none()
            && metadata.ed2k_sources.is_none()
            && metadata.info_hash.is_none();

        if needs_bitswap_cids {
            let file_hash = metadata.merkle_root.clone();

            // Build blocks from raw file bytes.
            let chunk_size = self.chunk_size();
            let blocks_raw = split_into_blocks(&metadata.file_data, chunk_size);

            let mut blocks: Vec<(Cid, Vec<u8>)> = Vec::with_capacity(blocks_raw.len());
            let mut block_cid_strings: Vec<String> = Vec::with_capacity(blocks_raw.len());
            for b in blocks_raw.iter() {
                let cid = b.cid().map_err(|e| e.to_string())?;
                block_cid_strings.push(cid.to_string());
                blocks.push((cid, b.data().to_vec()));
            }

            // Root CID is CID(list_of_block_cids_as_strings).
            let root_block_data =
                serde_json::to_vec(&block_cid_strings).map_err(|e| e.to_string())?;
            let root_cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(&root_block_data));

            // Sanitize metadata: never store inline bytes in DHT records/cache.
            metadata.file_data.clear();
            metadata.cids = Some(vec![root_cid.clone()]);

            // Update local cache with the sanitized metadata (so UI/local search sees cids).
            {
                let mut cache = self.file_metadata_cache.lock().await;
                if let Some(existing) = cache.get(&metadata.merkle_root) {
                    metadata = merge_file_metadata(existing.clone(), metadata);
                }
                cache.insert(metadata.merkle_root.clone(), metadata.clone());
            }

            let (response_tx, response_rx) = oneshot::channel();
            self.cmd_tx
                .send(DhtCommand::StoreBlocks {
                    blocks,
                    root_cid,
                    metadata,
                    response_tx,
                })
                .await
                .map_err(|e| e.to_string())?;

            // Wait until the DHT task has stored blocks and published the DHT record.
            response_rx.await.map_err(|e| e.to_string())??;

            // Upstream main removed heartbeat-based refreshing; publishing already registers providers in the DHT task.
            return Ok(());
        }

        // Merge with existing cached metadata to preserve multi-protocol fields
        // This ensures uploading via a second protocol doesn't lose data from the first
        {
            let mut cache = self.file_metadata_cache.lock().await;
            if let Some(existing) = cache.get(&metadata.merkle_root) {
                metadata = merge_file_metadata(existing.clone(), metadata);
            }
            cache.insert(metadata.merkle_root.clone(), metadata.clone());
        }

        let (response_tx, response_rx) = oneshot::channel();

        self.cmd_tx
            .send(DhtCommand::PublishFile {
                metadata,
                response_tx,
            })
            .await
            .map_err(|e| e.to_string())?;

        let cid_populated_metadata = response_rx.await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn stop_publishing_file(&self, file_hash: String) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::StopPublish(file_hash))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub async fn cache_remote_file(&self, metadata: &FileMetadata) {
        self.file_metadata_cache
            .lock()
            .await
            .insert(metadata.merkle_root.clone(), metadata.clone());
    }

    /// Promote a freshly downloaded file to a seeder by publishing its metadata back to the DHT
    /// and registering as a provider.
    pub async fn promote_downloaded_file(&self, metadata: FileMetadata) -> Result<(), String> {
        // Avoid storing inline data when re-publishing an already-downloaded file.
        let ftp_sources = metadata
            .ftp_sources
            .clone()
            .map(|sources| sources.into_iter().map(|s| s.for_dht_storage()).collect());

        let mut sanitized = metadata;
        sanitized.file_data.clear();

        // Ensure the download timestamp is set for peers that rely on ordering.
        if sanitized.created_at == 0 {
            sanitized.created_at = unix_timestamp();
        }

        // Make sure our own peer ID is represented as a seeder.
        if !sanitized.seeders.iter().any(|peer| peer == &self.peer_id) {
            sanitized.seeders.push(self.peer_id.clone());
        }

        self.publish_file(sanitized, ftp_sources).await
    }
    /// List all known FileMetadata (from cache, i.e., locally published or discovered)
    pub async fn get_all_file_metadata(&self) -> Result<Vec<FileMetadata>, String> {
        let cache = self.file_metadata_cache.lock().await;
        Ok(cache.values().cloned().collect())
    }

    /// Prepare a new FileMetadata for upload
    pub async fn prepare_file_metadata(
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
        price: f64,
        uploader_address: Option<String>,
    ) -> Result<FileMetadata, String> {
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
            encrypted_key_bundle: None,
            parent_hash: None,
            cids: None,
            is_root: true,
            download_path: None,
            price,
            uploader_address,
            ftp_sources: None,
            http_sources: None,
            info_hash: None,
            trackers: None,
            ed2k_sources: None,
            manifest: None,
        })
    }

    pub async fn download_file(
        &self,
        file_metadata: FileMetadata,
        download_path: String,
    ) -> Result<(), String> {
        info!(
            "📥 DhtService::download_file called for: {} to: {}",
            file_metadata.file_name, download_path
        );
        info!(
            "📥 file has {} seeders, cids present: {}",
            file_metadata.seeders.len(),
            file_metadata.cids.is_some()
        );
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
        let block_cid_strings: Vec<String> =
            blocks.iter().map(|(cid, _)| cid.to_string()).collect();
        let root_block_data = serde_json::to_vec(&block_cid_strings).map_err(|e| e.to_string())?;
        let root_cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(&root_block_data));

        let (response_tx, response_rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::StoreBlocks {
                blocks,
                root_cid,
                metadata,
                response_tx,
            })
            .await
            .map_err(|e| e.to_string())?;

        response_rx.await.map_err(|e| e.to_string())??;

        // Upstream main removed heartbeat-based refreshing; publishing already registers providers in the DHT task.
        Ok(())
    }

    pub async fn announce_torrent(&self, info_hash: String) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::AnnounceTorrent { info_hash })
            .await
            .map_err(|e| e.to_string())
    }

    // Fix the search_file method around line 6464:
    pub async fn search_file(&self, file_hash: String) -> Result<(), String> {
        // Create a dummy channel since this is fire-and-forget
        let (sender, _receiver) = oneshot::channel();

        self.cmd_tx
            .send(DhtCommand::SearchFile { file_hash, sender })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_file(&self, file_hash: String) -> Result<(), String> {
        self.search_file(file_hash).await
    }

    // Fix the search_metadata method around line 6474:
    pub async fn search_metadata(&self, file_hash: String, timeout_ms: u64) -> Result<(), String> {
        // Create a dummy channel since this is fire-and-forget
        let (sender, _receiver) = oneshot::channel();

        self.cmd_tx
            .send(DhtCommand::SearchFile { file_hash, sender })
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn synchronous_search_metadata(
        &self,
        file_hash: String,
        timeout_ms: u64,
    ) -> Result<Option<FileMetadata>, String> {
        info!(
            "🔍 Starting search for file: {} (timeout: {}ms)",
            file_hash, timeout_ms
        );

        // Skip local cache to always get fresh metadata from DHT
        // This ensures we have the latest protocols, seeders, and availability info
        info!("Querying DHT for fresh metadata for file {}...", file_hash);

        if timeout_ms == 0 {
            let (sender, _receiver) = oneshot::channel();
            self.cmd_tx
                .send(DhtCommand::SearchFile { file_hash, sender })
                .await
                .map_err(|e| e.to_string())?;
            return Ok(None);
        }

        let timeout_duration = Duration::from_millis(timeout_ms);
        let (tx, rx) = oneshot::channel();

        // Send the validated search command
        info!("🔍 Sending DHT search command for file: {}", file_hash);
        if let Err(err) = self
            .cmd_tx
            .send(DhtCommand::SearchFile {
                file_hash: file_hash.clone(),
                sender: tx,
            })
            .await
        {
            error!("❌ Failed to send DHT search command: {}", err);
            return Err(err.to_string());
        }
        info!("✅ DHT search command sent successfully");

        // Wait for the validated result
        // Check DHT health before waiting
        // let health = self.check_health(3, false).await;
        // info!(
        // "🔍 DHT health before search - peers: {}, healthy: {}",
        // health.peer_count, health.healthy
        // );
        info!("⏳ Waiting for search result with {}ms timeout", timeout_ms);
        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(Ok(Some(metadata)))) => {
                info!("✅ Search succeeded for file: {}", metadata.merkle_root);
                // Cache the result locally
                {
                    let mut cache = self.file_metadata_cache.lock().await;
                    cache.insert(metadata.merkle_root.clone(), metadata.clone());
                }
                Ok(Some(metadata))
            }
            Ok(Ok(Ok(None))) => {
                info!("❌ Search found no results for file: {}", file_hash);
                Ok(None)
            }
            Ok(Ok(Err(e))) => {
                error!("❌ Search error for file {}: {}", file_hash, e);
                Err(format!("Search error: {}", e))
            }
            Ok(Err(_)) => {
                error!("❌ Search channel closed for file: {}", file_hash);
                Err("Search channel closed".into())
            }
            Err(_) => {
                warn!(
                    "[TIME] Search timed out for file: {} (after {}ms)",
                    file_hash, timeout_ms
                );
                warn!("[TIME] Timeout occurred - no result received through channel");
                // Check if this might be due to connectivity issues
                // let health = self.check_health(5, false).await;
                // if health.peer_count < 5 {
                //     warn!(
                //         "⚠️ Low peer count ({} peers, minimum {}) may affect search reliability",
                //         health.peer_count, health.min_required
                //     );
                // }
                // if health.bootstrap_failures > 0 {
                //     warn!(
                //         "⚠️ Bootstrap failures detected ({}), network connectivity may be degraded",
                //         health.bootstrap_failures
                //     );
                // }
                Ok(None) // Timeout - file not found
            }
        }
    }

    pub async fn connect_peer(&self, addr: String) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::ConnectPeer(addr))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn connect_peer_multiaddr(&self, addr: Multiaddr) -> Result<(), String> {
        self.cmd_tx
            .send(DhtCommand::ConnectPeerMultiaddr(addr))
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

    pub async fn get_peer_addresses(
        &self,
        peer_ids: Vec<String>,
    ) -> Result<HashMap<String, Vec<String>>, String> {
        let parsed_ids: Vec<PeerId> = peer_ids
            .into_iter()
            .filter_map(|id| id.parse().ok())
            .collect();

        if parsed_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::GetPeerAddresses {
                peer_ids: parsed_ids,
                sender: tx,
            })
            .await
            .map_err(|e| e.to_string())?;

        let result_map = rx.await.map_err(|e| e.to_string())?;

        // Convert back to String keys and values for the caller
        let final_map = result_map
            .into_iter()
            .map(|(peer_id, addrs)| {
                (
                    peer_id.to_string(),
                    addrs.into_iter().map(|a| a.to_string()).collect(),
                )
            })
            .collect();

        Ok(final_map)
    }

    /// Get multiaddresses for this node (including the peer ID)
    pub async fn get_multiaddresses(&self) -> Vec<String> {
        let metrics = self.metrics.lock().await;
        let peer_id = &self.peer_id;

        metrics
            .listen_addrs
            .iter()
            .filter(|addr| {
                #[cfg(test)]
                {
                    // In tests, we actually NEED loopback to connect local nodes
                    true
                }
                #[cfg(not(test))]
                {
                    !addr.contains("127.0.0.1") && !addr.contains("::1")
                }
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

    /// Best-effort check that the internal DHT command loop is still alive.
    /// This is stronger than `get_connected_peers()` (which reads a shared cache directly)
    /// because it requires the background task (receiver) to still be running.
    pub async fn is_command_channel_alive(&self) -> bool {
        let (tx, _rx) = oneshot::channel();
        self.cmd_tx.send(DhtCommand::GetPeerCount(tx)).await.is_ok()
    }

    pub async fn get_connected_peers(&self) -> Vec<String> {
        let connected_peers = self.connected_peers.lock().await;
        connected_peers
            .iter()
            .map(|peer_id| peer_id.to_string())
            .collect()
    }

    /// Trigger a re-bootstrap to discover new peers
    /// Returns the number of new peers discovered
    pub async fn re_bootstrap(&self) -> Result<usize, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::ReBootstrap { sender: tx })
            .await
            .map_err(|e| format!("Failed to send re-bootstrap command: {}", e))?;

        rx.await
            .map_err(|e| format!("Re-bootstrap response error: {}", e))?
    }

    /// Check DHT health and optionally trigger automatic recovery
    ///
    /// # Arguments
    /// * `min_peers` - Minimum number of peers required for healthy status
    /// * `auto_recover` - Whether to automatically trigger re-bootstrap if unhealthy
    ///
    /// # Returns
    /// Health status including peer count and recommendations
    pub async fn check_health(&self, min_peers: usize, auto_recover: bool) -> DhtHealthStatus {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(DhtCommand::HealthCheck {
                min_peers,
                auto_recover,
                sender: tx,
            })
            .await
            .is_ok()
        {
            rx.await.unwrap_or_else(|_| DhtHealthStatus {
                healthy: false,
                peer_count: 0,
                min_required: min_peers,
                bootstrap_failures: 0,
                last_bootstrap_secs_ago: None,
                recommendation: Some("Health check failed to complete".to_string()),
                recovery_triggered: false,
            })
        } else {
            DhtHealthStatus {
                healthy: false,
                peer_count: 0,
                min_required: min_peers,
                bootstrap_failures: 0,
                last_bootstrap_secs_ago: None,
                recommendation: Some("Failed to send health check command".to_string()),
                recovery_triggered: false,
            }
        }
    }

    /// Check if the DHT is healthy (has minimum required peers)
    pub async fn is_healthy(&self, min_peers: usize) -> bool {
        let peer_count = self.connected_peers.lock().await.len();
        peer_count >= min_peers
    }

    /// Start a background health monitoring task
    ///
    /// This task periodically checks DHT health and triggers recovery if needed.
    /// Returns a handle to cancel the monitoring task.
    pub fn start_health_monitor(
        self: &Arc<Self>,
        check_interval_secs: u64,
        min_peers: usize,
    ) -> tokio::task::JoinHandle<()> {
        let dht_service = Arc::clone(self);

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(check_interval_secs));

            // Skip the first tick (which fires immediately)
            interval.tick().await;

            loop {
                interval.tick().await;

                let peer_count = dht_service.connected_peers.lock().await.len();

                if peer_count < min_peers {
                    warn!(
                        "DHT health check: peer count ({}) below minimum ({}), triggering recovery",
                        peer_count, min_peers
                    );

                    // Check full health and trigger auto-recovery
                    let status = dht_service.check_health(min_peers, true).await;

                    if status.recovery_triggered {
                        info!("DHT recovery triggered, waiting for bootstrap to complete...");
                    } else if !status.healthy {
                        warn!(
                            "DHT recovery was not triggered: {:?}",
                            status.recommendation
                        );
                    }
                } else {
                    debug!(
                        "DHT health check: {} peers connected (min: {})",
                        peer_count, min_peers
                    );
                }
            }
        })
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

        // Automatically publish a positive reputation verdict after successful transfer
        drop(peer_selection); // Release lock before async work
        if let Err(e) = self
            .publish_transfer_verdict(peer_id, VerdictOutcome::Good, bytes)
            .await
        {
            tracing::warn!(
                "Failed to publish reputation verdict for {}: {}",
                peer_id,
                e
            );
        }
    }

    /// Publish a reputation verdict for a peer after a transfer
    async fn publish_transfer_verdict(
        &self,
        peer_id: &str,
        outcome: VerdictOutcome,
        bytes: u64,
    ) -> Result<(), String> {
        // Create verdict
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut verdict = TransactionVerdict {
            target_id: peer_id.to_string(),
            tx_hash: None, // File transfers don't have blockchain transactions
            outcome: outcome.clone(),
            details: Some(format!(
                "File transfer: {} bytes in {} outcome",
                bytes,
                match &outcome {
                    VerdictOutcome::Good => "successful",
                    VerdictOutcome::Disputed => "disputed",
                    VerdictOutcome::Bad => "failed",
                }
            )),
            metric: Some(format!("transfer_bytes:{}", bytes)),
            issued_at: now,
            issuer_id: String::new(),  // Will be set by sign_with
            issuer_seq_no: 0,          // Simple counter, could be improved
            issuer_sig: String::new(), // Will be set by sign_with
            tx_receipt: None,
            evidence_blobs: None,
        };

        // Sign the verdict using the stored ed25519 secret key
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&*self.ed25519_secret_key);

        // Sign the verdict
        verdict
            .sign_with(&signing_key, &self.peer_id, 0)
            .map_err(|e| format!("Failed to sign verdict: {}", e))?;

        // Use target-only key so all verdicts about a peer are stored in one place
        let dht_key = TransactionVerdict::dht_key_for_target(peer_id);

        // Serialize verdict to JSON
        let verdict_json = serde_json::to_vec(&verdict)
            .map_err(|e| format!("Failed to serialize verdict: {}", e))?;

        // Store in DHT using PutDhtValue command
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::PutDhtValue {
                key: dht_key.clone(),
                value: verdict_json,
                sender: tx,
            })
            .await
            .map_err(|e| format!("Failed to send DHT command: {}", e))?;

        // Wait for result
        rx.await
            .map_err(|e| format!("Failed to receive DHT response: {}", e))??;
        tracing::info!(
            "✅ Published {} verdict for peer {} (key: {}...)",
            match outcome {
                VerdictOutcome::Good => "positive",
                VerdictOutcome::Disputed => "disputed",
                VerdictOutcome::Bad => "negative",
            },
            peer_id,
            &dht_key[..16]
        );

        Ok(())
    }

    /// Record failed transfer for peer metrics
    pub async fn record_transfer_failure(&self, peer_id: &str, error: &str) {
        let mut peer_selection = self.peer_selection.lock().await;
        peer_selection.record_transfer_failure(peer_id, error);

        // Automatically publish a negative reputation verdict after failed transfer
        drop(peer_selection); // Release lock before async work
        if let Err(e) = self
            .publish_transfer_verdict(peer_id, VerdictOutcome::Bad, 0)
            .await
        {
            tracing::warn!(
                "Failed to publish negative reputation verdict for {}: {}",
                peer_id,
                e
            );
        }
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

    /// Get peer metrics for all currently connected DHT peers
    /// This ensures the reputation system shows all connected peers, even if they don't have transfer history
    pub async fn get_connected_peer_metrics(&self) -> Vec<PeerMetrics> {
        let connected_peers = self.get_connected_peers().await;
        let mut peer_selection = self.peer_selection.lock().await;

        let mut all_metrics = Vec::new();

        for peer_id_str in connected_peers {
            // Try to get existing metrics, or create new default metrics if not found
            if let Some(metrics) = peer_selection.get_peer_metrics(&peer_id_str) {
                all_metrics.push(metrics.clone());
            } else {
                // Create default metrics for connected peers without metrics history
                // This can happen when peers are newly connected or haven't had any transfers yet
                let default_metrics = PeerMetrics::new(
                    peer_id_str.clone(),
                    "unknown".to_string(), // Address might not be available
                );
                // Update the peer selection cache with the new metrics
                peer_selection.update_peer_metrics(default_metrics.clone());
                all_metrics.push(default_metrics);
            }
        }

        all_metrics
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
        let mut pending_connections = Vec::new();

        // First pass: check which seeders are connected, queue connection attempts for others
        {
            let connected_peers = self.connected_peers.lock().await;

            for seeder_id in &metadata.seeders {
                if let Ok(peer_id) = seeder_id.parse::<libp2p::PeerId>() {
                    if connected_peers.contains(&peer_id) {
                        info!("Seeder {} is currently connected", seeder_id);
                        available_peers.push(seeder_id.clone());
                    } else {
                        info!(
                            "Seeder {} is not currently connected, will attempt connection",
                            seeder_id
                        );
                        pending_connections.push((seeder_id.clone(), peer_id));
                    }
                } else {
                    warn!("Invalid peer ID in seeders list: {}", seeder_id);
                }
            }
        }

        // If we already have connected peers, return them
        if !available_peers.is_empty() {
            info!("Found {} already connected seeders", available_peers.len());
            return Ok(available_peers);
        }

        // Initiate connection attempts for pending peers
        for (seeder_id, peer_id) in &pending_connections {
            if let Err(e) = self
                .cmd_tx
                .send(DhtCommand::ConnectToPeerById(*peer_id))
                .await
            {
                warn!(
                    "Failed to send ConnectToPeerById command for {}: {}",
                    seeder_id, e
                );
            } else {
                info!("Initiated connection attempt to seeder {}", seeder_id);
            }
        }

        // Wait for connections to establish with polling.
        // Default is conservative (5s) but real networks can need longer.
        let total_wait_ms: u64 = std::env::var("E2E_SEEDER_CONNECT_WAIT_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                std::env::var("CHIRAL_SEEDER_CONNECT_WAIT_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .map(|v: u64| v.clamp(1_000u64, 120_000u64))
            .unwrap_or(5_000);

        let poll_ms: u64 = 500;
        let max_wait_iterations: u64 = std::cmp::max(1, total_wait_ms / poll_ms);
        for i in 0..max_wait_iterations {
            tokio::time::sleep(Duration::from_millis(poll_ms)).await;

            let connected_peers = self.connected_peers.lock().await;
            for (seeder_id, peer_id) in &pending_connections {
                if connected_peers.contains(peer_id) && !available_peers.contains(seeder_id) {
                    info!(
                        "Seeder {} connected after {} ms",
                        seeder_id,
                        (i + 1) * poll_ms
                    );
                    available_peers.push(seeder_id.clone());
                }
            }
            drop(connected_peers);

            // If all pending connections succeeded or we found at least one peer, we can stop
            if !available_peers.is_empty() {
                break;
            }

            if i == max_wait_iterations / 2 {
                info!(
                    "Still waiting for seeder connections... ({}/{})",
                    i + 1,
                    max_wait_iterations
                );
            }
        }

        if available_peers.is_empty() {
            info!("No seeders could be reached after connection attempts - file not available for download");
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
        info!("getting seeders");
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

        // Wait for response with timeout - increased to 10s for better DHT propagation
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(Ok(providers))) => {
                info!(
                    "Found {} providers for file: {}",
                    providers.len(),
                    file_hash
                );
                info!("🔍 DEBUG DHT: Providers from DHT query = {:?}", providers);
                // Optionally filter unreachable providers here (try connect/ping) before returning.
                providers
            }
            Ok(Ok(Err(e))) => {
                warn!("GetProviders command failed for {}: {}", file_hash, e);
                warn!("🔍 DEBUG DHT: No providers found via DHT query - returning empty list");
                // Return empty list - don't fall back to random connected peers as they won't have the file
                Vec::new()
            }
            Ok(Err(e)) => {
                warn!("GetProviders receiver error for {}: {}", file_hash, e);
                warn!("🔍 DEBUG DHT: Channel error - returning empty list");
                // Return empty list - don't fall back to random connected peers
                Vec::new()
            }
            Err(_) => {
                warn!(
                    "GetProviders command timed out for file: {} (waited 10s)",
                    file_hash
                );
                warn!("🔍 DEBUG DHT: Timeout waiting for providers - returning empty list");
                // Return empty list - the file truly has no providers available
                Vec::new()
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
        let provider = Provider::<Http>::try_from(crate::ethereum::NETWORK_CONFIG.rpc_endpoint.as_str())
            .map_err(|e| format!("Failed to create provider: {}", e))?;
        let client = Arc::new(provider);

        // This private key is for demonstration. In a real app, you would retrieve
        // this securely from the AppState's keystore/active_account_private_key.
        let wallet: LocalWallet =
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .parse()
                .map_err(|e| format!("Failed to parse private key: {}", e))?;
        let signer = SignerMiddleware::new(client.clone(), wallet.with_chain_id(*CHAIN_ID));

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

/// A synchronous helper to perform the two-step info_hash lookup.
/// This is not ideal for the main event loop but can be useful for commands.
async fn synchronous_search_by_infohash(
    swarm: &mut Swarm<DhtBehaviour>,
    info_hash: &str,
) -> Result<Option<FileMetadata>, String> {
    // Step 1: Get the merkle_root from the info_hash index.
    let index_key = format!("info_hash:{}", info_hash);
    let record_key = kad::RecordKey::new(&index_key.as_bytes());
    swarm.behaviour_mut().kademlia.get_record(record_key);

    // This part is tricky without a proper request/response setup for internal swarm queries.
    // We'll simulate waiting for the event. In a real scenario, you'd use a channel.
    // For now, this function is more of a structural guide.
    // A proper implementation would require refactoring the event loop to handle multi-stage queries.
    warn!("synchronous_search_by_infohash is a placeholder due to event loop complexity.");
    Err("Synchronous info_hash search not fully implemented.".to_string())
}

impl DhtService {
    pub async fn search_by_infohash(
        &self,
        info_hash: String,
    ) -> Result<Option<FileMetadata>, String> {
        info!("🔍 DHT search_by_infohash called for: {}", info_hash);
        let (sender, receiver) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::SearchByInfohash {
                info_hash: info_hash.clone(),
                sender,
            })
            .await
            .map_err(|e| e.to_string())?;

        let result = receiver.await.map_err(|e| e.to_string())?;
        info!(
            "🔍 DHT search_by_infohash result for {}: {:?}",
            info_hash,
            result.is_some()
        );
        Ok(result)
    }

    /// Store a value in the DHT with the given key
    pub async fn put_dht_value(&self, key: String, value: Vec<u8>) -> Result<(), String> {
        let (sender, receiver) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::PutDhtValue { key, value, sender })
            .await
            .map_err(|e| e.to_string())?;
        receiver.await.map_err(|e| e.to_string())?
    }

    /// Retrieve a value from the DHT by key
    pub async fn get_dht_value(&self, key: String) -> Result<Option<Vec<u8>>, String> {
        let (sender, receiver) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::GetDhtValue { key, sender })
            .await
            .map_err(|e| e.to_string())?;
        receiver.await.map_err(|e| e.to_string())?
    }
}

impl DhtService {
    /// Finds Chiral peers in the DHT that are seeding a torrent with the given info_hash.
    pub async fn search_peers_by_infohash(&self, info_hash: String) -> Result<Vec<String>, String> {
        let (sender, receiver) = oneshot::channel();
        self.cmd_tx
            .send(DhtCommand::SearchPeersByInfohash { info_hash, sender })
            .await
            .map_err(|e| e.to_string())?;

        // Wait for the DHT query to complete
        receiver.await.map_err(|e| e.to_string())?
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
/// - IPv4 loopback (127.0.0.1) is REJECTED (not reachable from remote peers)
/// - For WAN intent, only public IPv4 addresses are allowed (not private ranges)
fn ma_plausibly_reachable(ma: &Multiaddr) -> bool {
    // Only consider IPv4 (IPv6 can be added if needed)
    if let Some(Protocol::Ip4(v4)) = ma.iter().find(|p| matches!(p, Protocol::Ip4(_))) {
        // Reject loopback addresses - they're not reachable from remote peers
        if v4.is_loopback() {
            return false;
        }
        // Allow public addresses, reject private
        return !v4.is_private();
    }
    false
}

/// A softer check that accepts any non-loopback IPv4 address (used as a fallback
/// to avoid publishing with an empty address set). This will allow private LAN
/// addresses but still reject loopback.
fn ma_non_loopback_ipv4(ma: &Multiaddr) -> bool {
    if let Some(Protocol::Ip4(v4)) = ma.iter().find(|p| matches!(p, Protocol::Ip4(_))) {
        return !v4.is_loopback();
    }
    false
}

/// Returns true if the swarm currently has at least one dialable address.
fn swarm_has_dialable_addr(swarm: &Swarm<DhtBehaviour>) -> bool {
    // External addresses (with scores) are the authoritative list to advertise
    if swarm
        .external_addresses()
        .any(|ext| ma_plausibly_reachable(ext))
    {
        return true;
    }

    // As a fallback, look at current listeners
    swarm.listeners().any(|addr| ma_plausibly_reachable(addr))
}

/// Parsing multiaddr from error string is heuristic and may not be reliable
fn extract_multiaddr_from_error_str(s: &str) -> Option<Multiaddr> {
    // Example: "Failed to negotiate ... [(/ip4/172.17.0.3/tcp/4001/p2p/12D...: : Timeout ...)]"
    // Try to find the first occurrence of "/ip" and extract until a delimiter
    if let Some(start) = s.find("/ip") {
        // Extract until we hit ": " (colon followed by space) which indicates end of multiaddr
        let tail = &s[start..];
        let end = tail.find(": ").unwrap_or_else(|| tail.len());
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

fn fallback_dial_addr_without_terminal_peer(addr: &Multiaddr) -> Option<Multiaddr> {
    let has_relay_circuit = addr.iter().any(|p| matches!(p, Protocol::P2pCircuit));
    if has_relay_circuit {
        return None;
    }
    let mut stripped = addr.clone();
    if matches!(stripped.iter().last(), Some(Protocol::P2p(_))) {
        stripped.pop();
        return Some(stripped);
    }
    None
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

    let parent = match path.parent() {
        Some(p) => p,
        None => return path, // If no parent, return original path
    };

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

/// Represents the data parsed from a magnet URI.
#[derive(Debug, PartialEq, Eq)]
pub struct MagnetData {
    pub info_hash: String,
    pub display_name: Option<String>,
    pub trackers: Vec<String>,
}

/// Parses a magnet URI string into a `MagnetData` struct.
///
/// This function extracts the info hash (btih), display name (dn),
/// and tracker URLs (tr) from a standard magnet link.
///
/// # Examples
///
/// ```
/// let magnet_uri = "magnet:?xt=urn:btih:b263275b1e3138b29596356533f685c33103575c&dn=My+Awesome+File&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80";
/// let magnet_data = parse_magnet_uri(magnet_uri).unwrap();
/// assert_eq!(magnet_data.info_hash, "b263275b1e3138b29596356533f685c33103575c");
/// assert_eq!(magnet_data.display_name, Some("My Awesome File".to_string()));
/// assert_eq!(magnet_data.trackers, vec!["udp://tracker.openbittorrent.com:80".to_string()]);
/// ```
pub fn parse_magnet_uri(uri: &str) -> Result<MagnetData, String> {
    if !uri.starts_with("magnet:?") {
        return Err("Invalid magnet URI: must start with 'magnet:?'".to_string());
    }

    let params_str = &uri[8..];
    let params: HashMap<String, Vec<String>> = url::form_urlencoded::parse(params_str.as_bytes())
        .into_owned()
        .fold(HashMap::new(), |mut acc, (key, val)| {
            acc.entry(key.to_lowercase()).or_default().push(val);
            acc
        });

    let info_hash = params
        .get("xt")
        .and_then(|xts| {
            xts.iter()
                .find_map(|xt| xt.strip_prefix("urn:btih:").map(|hash| hash.to_lowercase()))
        })
        .ok_or_else(|| "Magnet URI is missing 'xt' (info hash) parameter".to_string())?;

    let display_name = params.get("dn").and_then(|dns| dns.first().cloned());

    let trackers = params.get("tr").cloned().unwrap_or_default();

    Ok(MagnetData {
        info_hash,
        display_name,
        trackers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest as Sha1Digest, Sha1};
    use std::time::Duration;
    use tokio::time::sleep;
    use tokio::time::timeout;
    fn init() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[test]
    fn fallback_dial_addr_strips_terminal_peer_id_for_direct_addr() {
        let peer = PeerId::random();
        let addr: Multiaddr = format!("/ip4/8.8.8.8/tcp/4001/p2p/{}", peer)
            .parse()
            .unwrap();
        let fallback = fallback_dial_addr_without_terminal_peer(&addr).unwrap();
        assert_eq!(fallback.to_string(), "/ip4/8.8.8.8/tcp/4001");
    }

    #[test]
    fn fallback_dial_addr_disabled_for_relay_addr() {
        let relay = PeerId::random();
        let target = PeerId::random();
        let addr: Multiaddr = format!(
            "/ip4/8.8.8.8/tcp/4001/p2p/{}/p2p-circuit/p2p/{}",
            relay, target
        )
        .parse()
        .unwrap();
        assert!(fallback_dial_addr_without_terminal_peer(&addr).is_none());
    }

    // needed because metrics is used to retrieve listenining addresses
    // and metrics is populated when the swarm receives a listenaddr event
    async fn wait_for_address(node: &DhtService, timeout_secs: u64) -> Vec<String> {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(timeout_secs) {
            let addrs = node.get_multiaddresses().await;
            if !addrs.is_empty() {
                return addrs;
            }
            // Small sleep to prevent burning CPU
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("Timeout: Node failed to report any listen addresses.");
    }

    async fn spawn_test_node(bootstrap_nodes: Vec<String>) -> DhtService {
        let config = DhtConfig::builder().build();

        DhtService::new(config, None, None, None)
            .await
            .expect("Failed to create DhtService")
    }
    #[tokio::test]
    async fn test_node_spawn_and_shutdown() {
        // 1. Spawn a single node
        let node = spawn_test_node(vec![]).await;

        // 2. Verify it has a valid PeerId
        let peer_id = node.get_peer_id().await;
        assert!(!peer_id.is_empty(), "PeerId should not be empty");

        // 3. Verify it's listening (Port 0 should have resolved to a real port)
        let addrs = wait_for_address(&node, 10).await;
        // Note: In some restricted CI environments, this might be empty if loopback is disabled
        // but generally should contain at least one address.
        println!("Node spawned with addresses: {:?}", addrs);

        // 4. Verify initial state
        let peer_count = node.get_peer_count().await;
        assert_eq!(peer_count, 0, "New node should have 0 peers");

        // 5. Test Graceful Shutdown
        // We use a timeout to ensure that if the shutdown task hangs, the test fails
        let shutdown_result = timeout(Duration::from_secs(5), node.shutdown()).await;

        assert!(shutdown_result.is_ok(), "Node shutdown timed out");
        assert!(
            shutdown_result.unwrap().is_ok(),
            "Node shutdown returned an error"
        );
    }
    #[tokio::test]
    async fn test_multi_node_bootstrap_discovery() {
        // 1. Create the Bootstrap Node
        // We force server mode so it answers Kademlia queries immediately.
        let config = DhtConfig::builder()
            .is_bootstrap(true)
            .force_server_mode(true)
            .build();

        let bootstrap_node = DhtService::new(config, None, None, None).await.unwrap();

        let bootstrap_addrs = wait_for_address(&bootstrap_node, 10).await;
        let bootstrap_peer_id = bootstrap_node.get_peer_id().await;

        // We need the specific address that includes the peer ID for dialing
        let bootstrap_addr = bootstrap_addrs[0].clone();
        println!(
            "BootstrapNode spawned with addresses: {:?}",
            bootstrap_addrs
        );

        // 2. Create two client nodes pointing to the bootstrap node
        let node_a = spawn_test_node(vec![bootstrap_addr.clone()]).await;
        let node_b = spawn_test_node(vec![bootstrap_addr.clone()]).await;

        // 3. Assertion Loop
        // Discovery via Kademlia takes time. We poll until the condition is met.
        let mut discovered = false;
        for _ in 0..20 {
            // Try for 10 seconds
            let count_a = node_a.get_peer_count().await;
            let count_b = node_b.get_peer_count().await;

            // Node A should see Bootstrap + Node B (eventually)
            // Node B should see Bootstrap + Node A (eventually)
            if count_a >= 2 && count_b >= 2 {
                discovered = true;
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }

        assert!(
            discovered,
            "Nodes failed to discover each other via bootstrap node"
        );

        // 4. Cleanup
        node_a.shutdown().await.unwrap();
        node_b.shutdown().await.unwrap();
        bootstrap_node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_file_upload_discovery() {
        init();
        // 1. Setup the Network Backbone (Bootstrap Node)
        let b_config = DhtConfig::builder()
            .is_bootstrap(true)
            .force_server_mode(true)
            .build();
        let bootstrap_node = DhtService::new(b_config, None, None, None).await.unwrap();
        let b_addrs = wait_for_address(&bootstrap_node, 10).await;
        let b_addr = b_addrs[0].clone();

        // 2. Setup Uploader (Node A) and Searcher (Node B)
        let node_a = spawn_test_node(vec![b_addr.clone()]).await;
        let node_b = spawn_test_node(vec![b_addr.clone()]).await;

        // Ensure they are connected to the backbone
        let mut connected = false;
        for _ in 0..20 {
            if node_a.get_peer_count().await >= 1 && node_b.get_peer_count().await >= 1 {
                connected = true;
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }
        assert!(connected, "Nodes failed to connect to bootstrap");

        // 3. Prepare and Publish File Metadata (Node A)
        let file_hash =
            "deadbeef12345678deadbeef12345678deadbeef12345678deadbeef12345678".to_string();
        let file_name = "discovery_test.bin".to_string();
        let file_size = 1024 * 1024; // 1MB

        let metadata = node_a
            .prepare_file_metadata(
                file_hash.clone(),
                file_name.clone(),
                file_size,
                vec![0u8; 100], // Small inline data for discovery test
                unix_timestamp(),
                Some("application/octet-stream".into()),
                None,  // No encryption for this basic test
                false, // is_encrypted
                None,  // encryption_method
                None,  // key_fingerprint
                0.0,   // price
                Some(node_a.get_peer_id().await),
            )
            .await
            .unwrap();

        info!("Node A: Publishing file metadata...");
        node_a
            .publish_file(metadata, None)
            .await
            .expect("Failed to publish file");

        // 4. Discover Metadata (Node B)
        // DHT propagation can take a moment. We use a retry loop or the
        // internal timeout of synchronous_search_metadata.
        info!("Node B: Attempting to discover file metadata...");

        let mut discovered_metadata: Option<FileMetadata> = None;
        for i in 0..10 {
            // Search with a 2-second timeout per attempt
            match node_b
                .synchronous_search_metadata(file_hash.clone(), 2000)
                .await
            {
                Ok(Some(meta)) => {
                    discovered_metadata = Some(meta);
                    break;
                }
                _ => {
                    debug!("Attempt {}: Metadata not found yet, retrying...", i + 1);
                    sleep(Duration::from_millis(1000)).await;
                }
            }
        }

        // 5. Verification
        let found = discovered_metadata.expect("Node B failed to discover Node A's file metadata");
        assert_eq!(found.merkle_root, file_hash);
        assert_eq!(found.file_name, file_name);
        assert_eq!(found.file_size, file_size);

        // 6. Cleanup
        node_a.shutdown().await.unwrap();
        node_b.shutdown().await.unwrap();
        bootstrap_node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_multi_uploader_and_node_departure() {
        init();
        // 1. Setup Network Backbone
        let b_config = DhtConfig::builder()
            .is_bootstrap(true)
            .force_server_mode(true)
            .build();
        let bootstrap = DhtService::new(b_config, None, None, None).await.unwrap();
        let b_addr = wait_for_address(&bootstrap, 10).await[0].clone();

        // 2. Setup two Seeders (A, B) and one Searcher (C)
        let seeder_a = spawn_test_node(vec![b_addr.clone()]).await;
        let seeder_b = spawn_test_node(vec![b_addr.clone()]).await;
        let searcher_c = spawn_test_node(vec![b_addr.clone()]).await;

        let mut discovered = false;
        for _ in 0..20 {
            // Try for 10 seconds
            let count_a = seeder_a.get_peer_count().await;
            let count_b = seeder_b.get_peer_count().await;
            let count_c = searcher_c.get_peer_count().await;

            // Node A should see Bootstrap + Node B (eventually)
            // Node B should see Bootstrap + Node A (eventually)
            if count_a >= 3 && count_b >= 3 && count_c >= 3 {
                discovered = true;
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }

        assert!(
            discovered,
            "Nodes failed to discover each other via bootstrap node"
        );

        let file_hash = "cafebabe".repeat(8); // 32-byte hex
        let file_name = "resilience_test.dat".to_string();
        let file_size = 500_000;

        // 3. Both nodes publish the SAME file
        for node in &[&seeder_a, &seeder_b] {
            let meta = node
                .prepare_file_metadata(
                    file_hash.clone(),
                    file_name.clone(),
                    file_size,
                    vec![],
                    unix_timestamp(),
                    None,
                    None,
                    false,
                    None,
                    Some(node.get_peer_id().await),
                    0.0,
                    Some(node.get_peer_id().await),
                )
                .await
                .unwrap();
            node.publish_file(meta, None).await.unwrap();
        }

        // 4. Verify Searcher C sees BOTH seeders
        let mut discovered_metadata: Option<FileMetadata> = None;
        for i in 0..10 {
            // Search with a 2-second timeout per attempt
            match searcher_c
                .synchronous_search_metadata(file_hash.clone(), 2000)
                .await
            {
                Ok(Some(meta)) => {
                    discovered_metadata = Some(meta);
                    break;
                }
                _ => {
                    debug!("Attempt {}: Metadata not found yet, retrying...", i + 1);
                    sleep(Duration::from_millis(1000)).await;
                }
            }
        }
        let found = discovered_metadata.expect("Node B failed to discover Node A's file metadata");
        assert_eq!(found.merkle_root, file_hash);
        assert_eq!(found.file_name, file_name);
        assert_eq!(found.file_size, file_size);

        let mut both_seeders_found = false;
        for _ in 0..10 {
            if let found = searcher_c.get_seeders_for_file(&file_hash.clone()).await {
                println!("The array pretty print:\n{:#?}", found);
                if found.len() >= 2 {
                    assert!(found.contains(&seeder_a.get_peer_id().await));
                    assert!(found.contains(&seeder_b.get_peer_id().await));
                    both_seeders_found = true;
                    break;
                }
            }
            sleep(Duration::from_millis(1000)).await;
        }
        assert!(
            both_seeders_found,
            "Searcher failed to find both seeders for the same hash"
        );

        // 5. Node A leaves the network (Graceful Shutdown)
        info!("[DOWN] Node A leaving the network...");
        seeder_a.shutdown().await.unwrap();

        // 6. Verify Searcher C now only sees Node B
        // We wait for the DHT maintenance/pruning logic to kick in.
        let mut only_seeder_b_remains = false;
        for _ in 0..6 {
            if let found = searcher_c.get_seeders_for_file(&file_hash.clone()).await {
                println!("The array pretty print:\n{:#?}", found);

                let has_a = found.contains(&seeder_a.get_peer_id().await);
                let has_b = found.contains(&seeder_b.get_peer_id().await);

                if !has_a && has_b {
                    only_seeder_b_remains = true;
                    break;
                }
            }
            sleep(Duration::from_millis(1000)).await;
        }

        assert!(
            only_seeder_b_remains,
            "Node A was not pruned from the seeder list after leaving"
        );
        println!("✅ Multi-uploader resilience test passed!");

        // Cleanup
        seeder_b.shutdown().await.unwrap();
        searcher_c.shutdown().await.unwrap();
        bootstrap.shutdown().await.unwrap();
    }
    #[test]
    fn test_parse_magnet_uri_full() {
        let magnet = "magnet:?xt=urn:btih:b263275b1e3138b29596356533f685c33103575c&dn=My+Awesome+File.txt&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969";
        let result = parse_magnet_uri(magnet).unwrap();
        assert_eq!(
            result,
            MagnetData {
                info_hash: "b263275b1e3138b29596356533f685c33103575c".to_string(),
                display_name: Some("My Awesome File.txt".to_string()),
                trackers: vec![
                    "udp://tracker.openbittorrent.com:80".to_string(),
                    "udp://tracker.leechers-paradise.org:6969".to_string(),
                ],
            }
        );
    }

    #[test]
    fn test_parse_magnet_uri_minimal() {
        let magnet = "magnet:?xt=urn:btih:b263275b1e3138b29596356533f685c33103575c";
        let result = parse_magnet_uri(magnet).unwrap();
        assert_eq!(
            result,
            MagnetData {
                info_hash: "b263275b1e3138b29596356533f685c33103575c".to_string(),
                display_name: None,
                trackers: vec![],
            }
        );
    }

    #[test]
    fn test_parse_magnet_uri_case_insensitivity() {
        let magnet = "magnet:?XT=urn:btih:B263275B1E3138B29596356533F685C33103575C";
        let result = parse_magnet_uri(magnet).unwrap();
        assert_eq!(result.info_hash, "b263275b1e3138b29596356533f685c33103575c");
    }

    #[test]
    fn test_parse_magnet_uri_invalid() {
        assert!(parse_magnet_uri("http://example.com").is_err());
        assert!(parse_magnet_uri("magnet:?dn=MyFile").is_err());
    }

    #[test]
    fn test_torrent_piece_hash_verification() {
        // Simulate a torrent file with 3 pieces.
        // Piece size is 16 bytes for this test.
        let piece_size = 16;

        let piece1_data = b"This is piece 1."; // 16 bytes
        let piece2_data = b"This is piece 2!"; // 16 bytes
        let piece3_data = b"Short piece."; // 12 bytes

        // In a real torrent, these hashes would be in the .torrent file's `info.pieces` field.
        let mut hasher = Sha1::new();
        hasher.update(piece1_data);
        let expected_hash1 = hasher.finalize_reset();

        hasher.update(piece2_data);
        let expected_hash2 = hasher.finalize_reset();

        hasher.update(piece3_data);
        let expected_hash3 = hasher.finalize();

        // Simulate receiving the pieces (e.g., from peers).
        let received_piece1 = piece1_data.to_vec();
        let received_piece2 = piece2_data.to_vec();
        let received_piece3 = piece3_data.to_vec();

        // Verify each piece.
        let mut verifier = Sha1::new();
        verifier.update(&received_piece1);
        assert_eq!(verifier.finalize_reset(), expected_hash1);

        verifier.update(&received_piece2);
        assert_eq!(verifier.finalize_reset(), expected_hash2);

        verifier.update(&received_piece3);
        assert_eq!(verifier.finalize(), expected_hash3);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_command_stops_dht_service() {
        let service = spawn_test_node(vec![]).await;
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

    #[test]
    fn proxy_manager_ignores_stale_ping_rtt() {
        let mut mgr = ProxyManager::default();
        mgr.enable_privacy_routing(PrivacyMode::Prefer);

        let target = PeerId::random();
        let stale = PeerId::random();
        let fresh = PeerId::random();

        for peer in [stale.clone(), fresh.clone()] {
            mgr.set_capable(peer);
            mgr.set_online(peer);
            mgr.add_trusted_proxy_node(peer);
        }

        let stale_at = Instant::now() - ProxyManager::ping_rtt_ttl() - Duration::from_secs(1);
        mgr.set_ping_rtt_at(stale, 1, stale_at);
        mgr.set_ping_rtt(fresh, 150);

        assert_eq!(mgr.ping_rtt_ms(&stale), None);
        assert_eq!(mgr.select_proxy_for_routing(&target), Some(fresh));
    }

    #[test]
    fn proxy_manager_ping_rtt_map_is_capped() {
        let mut mgr = ProxyManager::default();
        for _ in 0..(PROXY_PING_RTT_MAX + 64) {
            let peer = PeerId::random();
            mgr.set_online(peer);
            mgr.set_ping_rtt(peer, 42);
        }
        assert!(mgr.ping_rtt.len() <= PROXY_PING_RTT_MAX);
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

        record_identify_push_metrics(&metrics, &info).await;

        {
            let guard = metrics.lock().await;
            assert_eq!(guard.listen_addrs.len(), 2);
            assert!(guard.listen_addrs.contains(&listen_addr.to_string()));
            assert!(guard.listen_addrs.contains(&secondary_addr.to_string()));
        }

        record_identify_push_metrics(&metrics, &info).await;

        let guard = metrics.lock().await;
        assert_eq!(guard.listen_addrs.len(), 2);
    }
}
