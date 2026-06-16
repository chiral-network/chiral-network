//! Relay-side share and site registry with HTTP reverse proxy + WebSocket tunnel.
//!
//! The relay never stores file data. It keeps mappings from share tokens and
//! site IDs to the owner's local server. When a visitor requests content:
//!
//! 1. If the owner has an active WebSocket tunnel, the request is forwarded
//!    through the tunnel (works behind NAT without port forwarding).
//! 2. Otherwise, the relay tries a direct HTTP proxy to the origin URL.
//! 3. If both fail, an offline error page is shown.

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket},
        ConnectInfo, Extension, Path, Query, WebSocketUpgrade,
    },
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct ShareRegistration {
    pub token: String,
    pub origin_url: String,
    pub owner_wallet: String,
    pub registered_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SiteRegistration {
    pub site_id: String,
    pub origin_url: String,
    pub owner_wallet: String,
    pub registered_at: u64,
}

#[derive(Clone)]
pub struct RelayShareRegistry {
    pub shares: Arc<RwLock<HashMap<String, ShareRegistration>>>,
    pub sites: Arc<RwLock<HashMap<String, SiteRegistration>>>,
    persist_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedRegistry {
    shares: Vec<ShareRegistration>,
    #[serde(default)]
    sites: Vec<SiteRegistration>,
}

impl RelayShareRegistry {
    pub fn new(data_dir: PathBuf) -> Self {
        let persist_path = data_dir.join("chiral-relay-shares").join("registry.json");
        Self {
            shares: Arc::new(RwLock::new(HashMap::new())),
            sites: Arc::new(RwLock::new(HashMap::new())),
            persist_path,
        }
    }

    pub async fn load_from_disk(&self) {
        if let Ok(data) = std::fs::read_to_string(&self.persist_path) {
            if let Ok(reg) = serde_json::from_str::<PersistedRegistry>(&data) {
                let mut share_map = self.shares.write().await;
                for s in reg.shares {
                    share_map.insert(s.token.clone(), s);
                }
                let share_count = share_map.len();
                drop(share_map);

                let mut site_map = self.sites.write().await;
                for s in reg.sites {
                    site_map.insert(s.site_id.clone(), s);
                }
                let site_count = site_map.len();
                drop(site_map);

                println!(
                    "[RELAY-SHARE] Loaded {} share + {} site registrations from disk",
                    share_count, site_count
                );
            }
        }
    }

    async fn persist(&self) {
        let share_map = self.shares.read().await;
        let site_map = self.sites.read().await;
        let reg = PersistedRegistry {
            shares: share_map.values().cloned().collect(),
            sites: site_map.values().cloned().collect(),
        };
        drop(share_map);
        drop(site_map);
        if let Some(parent) = self.persist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&reg) {
            let _ = std::fs::write(&self.persist_path, json);
        }
    }

    // --- Share methods ---

    pub async fn register(&self, reg: ShareRegistration) {
        let mut map = self.shares.write().await;
        map.insert(reg.token.clone(), reg);
        drop(map);
        self.persist().await;
    }

    pub async fn unregister(&self, token: &str) -> bool {
        let mut map = self.shares.write().await;
        let removed = map.remove(token).is_some();
        drop(map);
        if removed {
            self.persist().await;
        }
        removed
    }

    pub async fn lookup(&self, token: &str) -> Option<ShareRegistration> {
        let map = self.shares.read().await;
        map.get(token).cloned()
    }

    // --- Site methods ---

    pub async fn register_site(&self, reg: SiteRegistration) {
        let mut map = self.sites.write().await;
        map.insert(reg.site_id.clone(), reg);
        drop(map);
        self.persist().await;
    }

    pub async fn unregister_site(&self, site_id: &str) -> bool {
        let mut map = self.sites.write().await;
        let removed = map.remove(site_id).is_some();
        drop(map);
        if removed {
            self.persist().await;
        }
        removed
    }

    pub async fn lookup_site(&self, site_id: &str) -> Option<SiteRegistration> {
        let map = self.sites.read().await;
        map.get(site_id).cloned()
    }
}

// ---------------------------------------------------------------------------
// WebSocket tunnel registry
// ---------------------------------------------------------------------------

/// A pending tunnel request: the relay sends a TunnelRequest over the WS and
/// waits on the oneshot for the client's TunnelResponse.
type TunnelResponder = oneshot::Sender<TunnelResponse>;

/// Messages sent relay → client over the WebSocket.
#[derive(Serialize, Deserialize)]
struct TunnelRequest {
    id: String,
    path: String,
}

/// Messages sent client → relay over the WebSocket.
#[derive(Serialize, Deserialize)]
struct TunnelResponse {
    id: String,
    status: u16,
    #[serde(default)]
    headers: HashMap<String, String>,
    /// Base64-encoded body
    body: String,
}

/// Active tunnel: a sender half of an mpsc channel to push requests into the
/// WebSocket writer task, which forwards them to the connected client.
type TunnelSender = tokio::sync::mpsc::Sender<(TunnelRequest, TunnelResponder)>;

/// Global registry of active tunnels keyed by resource key (e.g. "site:abc" or
/// "share:xyz").
pub struct TunnelRegistry {
    tunnels: RwLock<HashMap<String, TunnelSender>>,
}

impl TunnelRegistry {
    pub fn new() -> Self {
        Self {
            tunnels: RwLock::new(HashMap::new()),
        }
    }

    async fn register(&self, key: String, sender: TunnelSender) {
        self.tunnels.write().await.insert(key, sender);
    }

    async fn unregister(&self, key: &str) {
        self.tunnels.write().await.remove(key);
    }

    /// Send a request through the tunnel and wait for the response.
    /// Cleans up the pending map entry on timeout to prevent memory leaks.
    async fn request(&self, key: &str, path: String) -> Option<TunnelResponse> {
        let sender = {
            let map = self.tunnels.read().await;
            map.get(key).cloned()
        };
        let sender = sender?;

        let id = uuid::Uuid::new_v4().to_string();
        let (resp_tx, resp_rx) = oneshot::channel();

        let req = TunnelRequest {
            id: id.clone(),
            path,
        };

        if sender.send((req, resp_tx)).await.is_err() {
            // Tunnel disconnected — unregister it
            self.unregister(key).await;
            return None;
        }

        // Wait up to 15s for the client to respond (reduced from 30s)
        match tokio::time::timeout(std::time::Duration::from_secs(15), resp_rx).await {
            Ok(Ok(resp)) => Some(resp),
            _ => {
                // Timeout or channel dropped — the pending entry is already consumed
                // by the responder being dropped, so no leak here. But if the tunnel
                // is consistently timing out, remove it.
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Request/response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RegisterRequest {
    token: String,
    origin_url: String,
    owner_wallet: String,
    /// ECDSA signature by `owner_wallet` over `register_payload("share",
    /// token, owner_wallet, origin_url)`. Required: without it, any HTTP
    /// caller could overwrite a known token to redirect visitors to an
    /// attacker-chosen URL (FM-A05).
    #[serde(default)]
    signature: String,
}

#[derive(Deserialize)]
struct SiteRegisterRequest {
    site_id: String,
    origin_url: String,
    owner_wallet: String,
    /// ECDSA signature by `owner_wallet` over `register_payload("site",
    /// site_id, owner_wallet, origin_url)`. See `RegisterRequest`.
    #[serde(default)]
    signature: String,
}

const REGISTER_TAG: &[u8] = b"chiral-relay-register-v1";

/// Length-prefixed canonical bytes that the registrant must sign.
/// Binds operation kind, the resource id, the owner wallet, and the
/// declared origin URL together so a captured signature for one
/// (token, origin) pair can't be replayed for a different one.
pub fn register_payload(
    operation: &str,
    id: &str,
    owner_wallet: &str,
    origin_url: &str,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + origin_url.len());
    out.extend_from_slice(REGISTER_TAG);
    for part in [
        operation.as_bytes(),
        id.as_bytes(),
        owner_wallet.as_bytes(),
        origin_url.as_bytes(),
    ] {
        out.extend_from_slice(&(part.len() as u32).to_le_bytes());
        out.extend_from_slice(part);
    }
    out
}

fn is_valid_wallet(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

const PRIVATE_ORIGIN_ALLOWLIST_ENV: &str = "CHIRAL_RELAY_SHARE_PRIVATE_ORIGIN_ALLOWLIST";

#[derive(Clone, Debug, PartialEq, Eq)]
enum PrivateOriginAllowEntry {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

impl PrivateOriginAllowEntry {
    fn contains(&self, ip: IpAddr) -> bool {
        match (self, ip) {
            (Self::V4 { network, prefix }, IpAddr::V4(v4)) => {
                let mask = prefix_mask_v4(*prefix);
                (u32::from(v4) & mask) == *network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(v6)) => {
                let mask = prefix_mask_v6(*prefix);
                (u128::from(v6) & mask) == *network
            }
            _ => false,
        }
    }
}

fn prefix_mask_v4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn prefix_mask_v6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

fn parse_private_origin_allow_entry(raw: &str) -> Result<PrivateOriginAllowEntry, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty allowlist entry".to_string());
    }

    let (ip_raw, prefix_raw) = trimmed.split_once('/').unwrap_or((trimmed, ""));
    let ip: IpAddr = ip_raw
        .parse()
        .map_err(|_| format!("invalid IP allowlist entry `{}`", trimmed))?;

    match ip {
        IpAddr::V4(v4) => {
            let prefix = if prefix_raw.is_empty() {
                32
            } else {
                prefix_raw
                    .parse::<u8>()
                    .map_err(|_| format!("invalid IPv4 prefix in `{}`", trimmed))?
            };
            if prefix > 32 {
                return Err(format!("IPv4 prefix out of range in `{}`", trimmed));
            }
            let mask = prefix_mask_v4(prefix);
            Ok(PrivateOriginAllowEntry::V4 {
                network: u32::from(v4) & mask,
                prefix,
            })
        }
        IpAddr::V6(v6) => {
            let prefix = if prefix_raw.is_empty() {
                128
            } else {
                prefix_raw
                    .parse::<u8>()
                    .map_err(|_| format!("invalid IPv6 prefix in `{}`", trimmed))?
            };
            if prefix > 128 {
                return Err(format!("IPv6 prefix out of range in `{}`", trimmed));
            }
            let mask = prefix_mask_v6(prefix);
            Ok(PrivateOriginAllowEntry::V6 {
                network: u128::from(v6) & mask,
                prefix,
            })
        }
    }
}

fn parse_private_origin_allowlist(raw: &str) -> Vec<PrivateOriginAllowEntry> {
    raw.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            match parse_private_origin_allow_entry(entry) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    eprintln!("[RELAY-SHARE] Ignoring invalid private-origin allowlist entry: {e}");
                    None
                }
            }
        })
        .collect()
}

fn private_origin_allowlist_from_env() -> Vec<PrivateOriginAllowEntry> {
    std::env::var(PRIVATE_ORIGIN_ALLOWLIST_ENV)
        .ok()
        .map(|raw| parse_private_origin_allowlist(&raw))
        .unwrap_or_default()
}

fn origin_host(origin_url: &str) -> Result<String, String> {
    let lower = origin_url.trim().to_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("http://") {
        r
    } else if let Some(r) = lower.strip_prefix("https://") {
        r
    } else {
        return Err("origin_url scheme must be http:// or https://".to_string());
    };
    // Strip any user-info, path, port to isolate the host.
    let after_userinfo = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
    let host_with_port = after_userinfo
        .split('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");
    if host_with_port.is_empty() {
        return Err("origin_url has no host".to_string());
    }
    // Strip port — careful with IPv6 literal brackets.
    let host = if let Some(rest) = host_with_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host_with_port.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        return Err("origin_url has no host".to_string());
    }
    Ok(host.to_string())
}

fn allowlist_contains(allowlist: &[PrivateOriginAllowEntry], ip: IpAddr) -> bool {
    allowlist.iter().any(|entry| entry.contains(ip))
}

fn is_cgnat_v4(v4: Ipv4Addr) -> bool {
    let oct = v4.octets();
    oct[0] == 100 && (oct[1] & 0xC0) == 64
}

fn validate_ipv4_origin(v4: Ipv4Addr, allowlist: &[PrivateOriginAllowEntry]) -> Result<(), String> {
    if v4 == Ipv4Addr::UNSPECIFIED {
        return Err("origin_url 0.0.0.0 is not routable".to_string());
    }
    if v4.is_loopback() {
        // Allowed — fix_origin_url substitutes at request time.
        return Ok(());
    }
    if v4.is_link_local() {
        return Err(format!("origin_url IP {} is link-local — not allowed", v4));
    }
    if v4.is_broadcast() || v4.is_multicast() {
        return Err(format!(
            "origin_url IP {} is broadcast / multicast — not allowed",
            v4
        ));
    }
    if v4.is_private() || is_cgnat_v4(v4) {
        if allowlist_contains(allowlist, IpAddr::V4(v4)) {
            return Ok(());
        }
        return Err(format!(
            "origin_url IP {} is private / CGNAT — not allowed without {}",
            v4, PRIVATE_ORIGIN_ALLOWLIST_ENV
        ));
    }
    Ok(())
}

fn validate_ipv6_origin(v6: Ipv6Addr, allowlist: &[PrivateOriginAllowEntry]) -> Result<(), String> {
    if v6.is_loopback() {
        return Ok(());
    }
    if v6.is_unspecified() || v6.is_multicast() {
        return Err(format!("origin_url IPv6 {} is not routable", v6));
    }
    if v6 == Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254) {
        return Err(format!("origin_url IPv6 {} is a metadata service", v6));
    }
    let seg0 = v6.segments()[0];
    // Link-local fe80::/10 is never allowlisted: it includes local
    // infrastructure and metadata-service style targets.
    if (seg0 & 0xffc0) == 0xfe80 {
        return Err(format!("origin_url IPv6 {} is link-local", v6));
    }
    // Detect IPv4-mapped (::ffff:0:0/96) and walk into v4 rules.
    if let Some(mapped) = v6.to_ipv4() {
        return validate_ipv4_origin(mapped, allowlist);
    }
    // Unique-local fc00::/7 may be reachable through VPN/corporate
    // networks, but only when the relay operator explicitly allows it.
    if (seg0 & 0xfe00) == 0xfc00 {
        if allowlist_contains(allowlist, IpAddr::V6(v6)) {
            return Ok(());
        }
        return Err(format!(
            "origin_url IPv6 {} is unique-local — not allowed without {}",
            v6, PRIVATE_ORIGIN_ALLOWLIST_ENV
        ));
    }
    Ok(())
}

/// Reject origin URLs that point at private/link-local infrastructure.
/// Loopback is permitted at registration time because `fix_origin_url`
/// substitutes the registrant's public IP at request handling.
fn is_safe_origin_url(origin_url: &str) -> Result<(), String> {
    let allowlist = private_origin_allowlist_from_env();
    is_safe_origin_url_with_allowlist(origin_url, &allowlist)
}

/// Validate a relay-share origin URL against the public registration policy.
pub fn validate_relay_share_origin_url(origin_url: &str) -> Result<(), String> {
    is_safe_origin_url(origin_url)
}

fn is_safe_origin_url_with_allowlist(
    origin_url: &str,
    allowlist: &[PrivateOriginAllowEntry],
) -> Result<(), String> {
    let host = origin_host(origin_url)?;
    // If the host is an IP literal, block private / link-local ranges.
    // DNS names pass — DNS rebinding is a separate attack we don't
    // address here (would need proxy-time IP re-validation).
    if let Ok(ip) = host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(v4) => validate_ipv4_origin(v4, allowlist)?,
            IpAddr::V6(v6) => validate_ipv6_origin(v6, allowlist)?,
        }
    }
    Ok(())
}

fn validate_normalized_origin_ip(
    ip: IpAddr,
    allowlist: &[PrivateOriginAllowEntry],
) -> Result<(), String> {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                return Err(format!(
                    "origin_url IP {} is loopback after normalization — not allowed",
                    v4
                ));
            }
            validate_ipv4_origin(v4, allowlist)
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return Err(format!(
                    "origin_url IPv6 {} is loopback after normalization — not allowed",
                    v6
                ));
            }
            if let Some(mapped) = v6.to_ipv4() {
                if mapped.is_loopback() {
                    return Err(format!(
                        "origin_url IP {} is loopback after normalization — not allowed",
                        mapped
                    ));
                }
            }
            validate_ipv6_origin(v6, allowlist)
        }
    }
}

fn is_safe_normalized_origin_url(origin_url: &str) -> Result<(), String> {
    let allowlist = private_origin_allowlist_from_env();
    let host = origin_host(origin_url).map_err(|e| format!("normalized {e}"))?;
    if let Ok(ip) = host.parse::<IpAddr>() {
        validate_normalized_origin_ip(ip, &allowlist).map_err(|e| format!("normalized {e}"))?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct ProxyQuery {
    #[serde(flatten)]
    params: HashMap<String, String>,
}

#[derive(Deserialize)]
struct TunnelQuery {
    /// "site" or "share"
    #[serde(rename = "type")]
    resource_type: String,
    /// The site_id or share token
    id: String,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Registration API handlers
// ---------------------------------------------------------------------------

/// Replace 0.0.0.0 or 127.0.0.1 in origin URL with the client's real IP.
/// e.g. "http://0.0.0.0:9419" + client_ip 1.2.3.4 → "http://1.2.3.4:9419"
fn fix_origin_url(origin_url: &str, client_ip: std::net::IpAddr) -> String {
    for placeholder in &["0.0.0.0", "127.0.0.1", "localhost"] {
        if origin_url.contains(placeholder) {
            return origin_url.replace(placeholder, &client_ip.to_string());
        }
    }
    origin_url.to_string()
}

fn normalize_origin_for_preflight(
    origin_url: &str,
    client_ip: std::net::IpAddr,
) -> Result<String, String> {
    let origin = fix_origin_url(origin_url, client_ip);
    is_safe_normalized_origin_url(&origin)?;
    Ok(origin)
}

async fn preflight_origin_reachable(origin_url: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        // The origin host has already passed `is_safe_origin_url`; do not
        // follow attacker-controlled redirects to a different network target.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("origin_url preflight client could not be initialized: {e}"))?;

    client
        .get(origin_url.trim_end_matches('/'))
        .send()
        .await
        .map(|_| ())
        .map_err(|e| {
            format!(
                "origin_url is not reachable from this relay: {e}. Ensure the owner server is running, the host/port is reachable from the relay, and firewall/NAT rules allow inbound HTTP."
            )
        })
}

/// POST /api/drive/relay-register — register a share origin
async fn register_share(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<RegisterRequest>,
) -> Response {
    if req.token.is_empty() || req.origin_url.is_empty() {
        return (StatusCode::BAD_REQUEST, "token and origin_url required").into_response();
    }
    let owner = req.owner_wallet.trim().to_lowercase();
    if !is_valid_wallet(&owner) {
        return (
            StatusCode::BAD_REQUEST,
            "owner_wallet must be 0x-hex (42 chars)",
        )
            .into_response();
    }
    if let Err(e) = is_safe_origin_url(&req.origin_url) {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }
    if req.signature.is_empty() {
        return (StatusCode::UNAUTHORIZED, "signature required").into_response();
    }
    let payload = register_payload("share", &req.token, &owner, &req.origin_url);
    if !crate::wallet::verify_signature(&payload, &req.signature, &owner) {
        return (
            StatusCode::UNAUTHORIZED,
            "signature did not verify against owner_wallet",
        )
            .into_response();
    }
    // First-claim-wins: only the wallet that owns the existing record
    // (verified at registration time) may overwrite it.
    if let Some(existing) = state.lookup(&req.token).await {
        if existing.owner_wallet.to_lowercase() != owner {
            return (
                StatusCode::FORBIDDEN,
                format!("token {} is already claimed by another wallet", req.token),
            )
                .into_response();
        }
    }
    let origin = match normalize_origin_for_preflight(&req.origin_url, addr.ip()) {
        Ok(origin) => origin,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    if let Err(e) = preflight_origin_reachable(&origin).await {
        return (StatusCode::BAD_GATEWAY, e).into_response();
    }
    println!(
        "[RELAY-SHARE] Registering share token={} origin={} (raw={}) owner={}",
        req.token, origin, req.origin_url, owner
    );
    state
        .register(ShareRegistration {
            token: req.token,
            origin_url: origin,
            owner_wallet: owner,
            registered_at: now_secs(),
        })
        .await;
    (StatusCode::OK, "Registered").into_response()
}

/// DELETE /api/drive/relay-register/:token — unregister a share. Only
/// the wallet that originally registered the share may unregister it
/// — verified via the X-Owner / X-Owner-Sig owner-proof headers
/// (`auth::verify_owner_proof`). Without this, any HTTP caller could
/// delete another peer's claim.
async fn unregister_share(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path(token): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let path_for_proof = format!("/api/drive/relay-register/{}", token);
    let claimant = match crate::auth::verify_owner_proof(
        &headers,
        &axum::http::Method::DELETE,
        &path_for_proof,
    ) {
        Ok(addr) => addr,
        Err(e) => return (StatusCode::UNAUTHORIZED, e).into_response(),
    };
    match state.lookup(&token).await {
        Some(existing) if existing.owner_wallet.to_lowercase() == claimant => {
            state.unregister(&token).await;
            println!("[RELAY-SHARE] Unregistered share token={}", token);
            (StatusCode::OK, "Unregistered").into_response()
        }
        Some(_) => (
            StatusCode::FORBIDDEN,
            "only the registered owner may unregister this share",
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Share not found").into_response(),
    }
}

// ---------------------------------------------------------------------------
// WebSocket tunnel endpoint
// ---------------------------------------------------------------------------

/// GET /api/tunnel/ws?type=site&id=xxx — WebSocket tunnel for NAT traversal.
///
/// The client (site/share owner) connects here after publishing. The relay
/// forwards incoming visitor requests through this WebSocket.
async fn tunnel_ws_handler(
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Query(q): Query<TunnelQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let key = format!("{}:{}", q.resource_type, q.id);
    println!("[TUNNEL] WebSocket upgrade for key={}", key);
    ws.on_upgrade(move |socket| handle_tunnel_ws(socket, key, tunnel_reg))
}

async fn handle_tunnel_ws(socket: WebSocket, key: String, tunnel_reg: Arc<TunnelRegistry>) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Channel for the proxy handlers to send requests into this tunnel
    let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<(TunnelRequest, TunnelResponder)>(32);

    tunnel_reg.register(key.clone(), req_tx).await;
    println!("[TUNNEL] Connected: {}", key);

    // Map of pending request IDs → responders
    let pending: Arc<RwLock<HashMap<String, TunnelResponder>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let pending_for_read = Arc::clone(&pending);

    // Task: read responses from the WebSocket client
    let read_key = key.clone();
    let read_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(resp) = serde_json::from_str::<TunnelResponse>(&text) {
                        let mut map = pending_for_read.write().await;
                        if let Some(tx) = map.remove(&resp.id) {
                            let _ = tx.send(resp);
                        }
                    }
                }
                Message::Close(_) => {
                    println!("[TUNNEL] Client closed: {}", read_key);
                    break;
                }
                _ => {}
            }
        }
    });

    // Task: forward requests from proxy handlers to the WebSocket client
    let write_task = tokio::spawn(async move {
        // Periodic pings to keep the connection alive + cleanup stale pending entries
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                req = req_rx.recv() => {
                    match req {
                        Some((tunnel_req, responder)) => {
                            let id = tunnel_req.id.clone();
                            pending.write().await.insert(id, responder);
                            let json = serde_json::to_string(&tunnel_req).unwrap_or_default();
                            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = ping_interval.tick() => {
                    // Clean up stale pending entries (responder dropped = channel closed)
                    {
                        let mut map = pending.write().await;
                        map.retain(|_, tx| !tx.is_closed());
                    }
                    if ws_tx.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Wait for either task to finish (connection dropped)
    tokio::select! {
        _ = read_task => {}
        _ = write_task => {}
    }

    tunnel_reg.unregister(&key).await;
    println!("[TUNNEL] Disconnected: {}", key);
}

// ---------------------------------------------------------------------------
// Reverse proxy helpers
// ---------------------------------------------------------------------------

/// Build the query string from the flattened params map.
fn build_query_string(params: &HashMap<String, String>) -> String {
    if params.is_empty() {
        return String::new();
    }
    let qs: Vec<String> = params.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
    format!("?{}", qs.join("&"))
}

/// Try the WebSocket tunnel first; if unavailable fall back to direct HTTP proxy.
async fn proxy_via_tunnel_or_http(
    tunnel_reg: &Arc<TunnelRegistry>,
    tunnel_key: &str,
    path: &str,
    direct_url: &str,
) -> Response {
    // Try tunnel first
    if let Some(resp) = tunnel_reg.request(tunnel_key, path.to_string()).await {
        return tunnel_response_to_axum(resp);
    }

    // Fall back to direct HTTP proxy (works if port is forwarded)
    proxy_request_direct(direct_url).await
}

/// Convert a TunnelResponse into an Axum HTTP response.
fn tunnel_response_to_axum(resp: TunnelResponse) -> Response {
    use base64::Engine;
    let status = StatusCode::from_u16(resp.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let body_bytes = base64::engine::general_purpose::STANDARD
        .decode(&resp.body)
        .unwrap_or_default();

    let mut headers = axum::http::HeaderMap::new();
    for (k, v) in &resp.headers {
        if let Ok(name) = axum::http::header::HeaderName::from_bytes(k.as_bytes()) {
            if let Ok(hv) = axum::http::HeaderValue::from_str(v) {
                headers.insert(name, hv);
            }
        }
    }

    (status, headers, body_bytes).into_response()
}

/// Forward a GET request to the target URL directly and stream the response back.
async fn proxy_request_direct(target: &str) -> Response {
    if let Err(e) = is_safe_origin_url(target) {
        eprintln!("[RELAY-SHARE] Blocking direct proxy to disallowed origin: {e}");
        return (
            StatusCode::FORBIDDEN,
            Html(offline_page(
                "The registered origin is blocked by relay policy.",
            )),
        )
            .into_response();
    }

    let client = match crate::rpc_client::client() {
        Ok(client) => client,
        Err(e) => {
            eprintln!("[RELAY-SHARE] Shared HTTP client unavailable: {e}");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(offline_page(
                    "The owner is currently offline. Please try again later.",
                )),
            )
                .into_response();
        }
    };
    let upstream = match client.get(target).send().await {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(offline_page(
                    "The owner is currently offline. Please try again later.",
                )),
            )
                .into_response();
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    // Forward relevant headers (convert reqwest HeaderValue -> axum HeaderValue)
    let mut headers = axum::http::HeaderMap::new();
    for key in &[
        "content-type",
        "content-length",
        "content-disposition",
        "cache-control",
        "etag",
    ] {
        if let Some(val) = upstream.headers().get(*key) {
            if let Ok(name) = axum::http::header::HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(hv) = axum::http::HeaderValue::from_bytes(val.as_bytes()) {
                    headers.insert(name, hv);
                }
            }
        }
    }

    // Stream the response body
    let stream = upstream.bytes_stream();
    let body = Body::from_stream(stream);

    (status, headers, body).into_response()
}

// ---------------------------------------------------------------------------
// Drive share proxy handlers
// ---------------------------------------------------------------------------

/// Proxy GET /drive/:token to the sharer's local server.
async fn proxy_share_root(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path(token): Path<String>,
    Query(q): Query<ProxyQuery>,
) -> Response {
    let reg = match state.lookup(&token).await {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Html(offline_page("Share link not found")),
            )
                .into_response()
        }
    };

    let qs = build_query_string(&q.params);
    let path = format!("/drive/{}{}", token, qs);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("share:{}", token);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

/// Proxy GET /drive/:token/*path to the sharer's local server.
async fn proxy_share_path(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path((token, subpath)): Path<(String, String)>,
    Query(q): Query<ProxyQuery>,
) -> Response {
    let reg = match state.lookup(&token).await {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Html(offline_page("Share link not found")),
            )
                .into_response()
        }
    };

    let qs = build_query_string(&q.params);
    let path = format!("/drive/{}/{}{}", token, subpath, qs);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("share:{}", token);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

// ---------------------------------------------------------------------------
// Site registration API handlers
// ---------------------------------------------------------------------------

/// POST /api/sites/relay-register — register a site origin
async fn register_site(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<SiteRegisterRequest>,
) -> Response {
    if req.site_id.is_empty() || req.origin_url.is_empty() {
        return (StatusCode::BAD_REQUEST, "site_id and origin_url required").into_response();
    }
    let owner = req.owner_wallet.trim().to_lowercase();
    if !is_valid_wallet(&owner) {
        return (
            StatusCode::BAD_REQUEST,
            "owner_wallet must be 0x-hex (42 chars)",
        )
            .into_response();
    }
    if let Err(e) = is_safe_origin_url(&req.origin_url) {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }
    if req.signature.is_empty() {
        return (StatusCode::UNAUTHORIZED, "signature required").into_response();
    }
    let payload = register_payload("site", &req.site_id, &owner, &req.origin_url);
    if !crate::wallet::verify_signature(&payload, &req.signature, &owner) {
        return (
            StatusCode::UNAUTHORIZED,
            "signature did not verify against owner_wallet",
        )
            .into_response();
    }
    if let Some(existing) = state.lookup_site(&req.site_id).await {
        if existing.owner_wallet.to_lowercase() != owner {
            return (
                StatusCode::FORBIDDEN,
                format!(
                    "site_id {} is already claimed by another wallet",
                    req.site_id
                ),
            )
                .into_response();
        }
    }
    let origin = match normalize_origin_for_preflight(&req.origin_url, addr.ip()) {
        Ok(origin) => origin,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    if let Err(e) = preflight_origin_reachable(&origin).await {
        return (StatusCode::BAD_GATEWAY, e).into_response();
    }
    println!(
        "[RELAY-SITE] Registering site={} origin={} (raw={}) owner={}",
        req.site_id, origin, req.origin_url, owner
    );
    state
        .register_site(SiteRegistration {
            site_id: req.site_id,
            origin_url: origin,
            owner_wallet: owner,
            registered_at: now_secs(),
        })
        .await;
    (StatusCode::OK, "Registered").into_response()
}

/// DELETE /api/sites/relay-register/:site_id — unregister a site. Only
/// the wallet that originally registered the site may unregister it.
async fn unregister_site(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path(site_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let path_for_proof = format!("/api/sites/relay-register/{}", site_id);
    let claimant = match crate::auth::verify_owner_proof(
        &headers,
        &axum::http::Method::DELETE,
        &path_for_proof,
    ) {
        Ok(addr) => addr,
        Err(e) => return (StatusCode::UNAUTHORIZED, e).into_response(),
    };
    match state.lookup_site(&site_id).await {
        Some(existing) if existing.owner_wallet.to_lowercase() != claimant => {
            return (
                StatusCode::FORBIDDEN,
                "only the registered owner may unregister this site",
            )
                .into_response();
        }
        None => return (StatusCode::NOT_FOUND, "Site not found").into_response(),
        Some(_) => {}
    }
    if state.unregister_site(&site_id).await {
        println!("[RELAY-SITE] Unregistered site={}", site_id);
        (StatusCode::OK, "Unregistered").into_response()
    } else {
        (StatusCode::NOT_FOUND, "Site not found").into_response()
    }
}

// ---------------------------------------------------------------------------
// Site reverse proxy handlers
// ---------------------------------------------------------------------------

/// Proxy GET /sites/:site_id to redirect (matching local server behavior).
async fn proxy_site_redirect(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Path(site_id): Path<String>,
) -> Response {
    if state.lookup_site(&site_id).await.is_none() {
        return (StatusCode::NOT_FOUND, Html(offline_page("Site not found"))).into_response();
    }
    (
        StatusCode::MOVED_PERMANENTLY,
        [("Location", format!("/sites/{}/", site_id))],
        "",
    )
        .into_response()
}

/// Proxy GET /sites/:site_id/ to the owner's local server.
async fn proxy_site_root(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path(site_id): Path<String>,
) -> Response {
    let reg = match state.lookup_site(&site_id).await {
        Some(r) => r,
        None => {
            return (StatusCode::NOT_FOUND, Html(offline_page("Site not found"))).into_response()
        }
    };
    let path = format!("/sites/{}/", site_id);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("site:{}", site_id);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

/// Proxy GET /sites/:site_id/*path to the owner's local server.
async fn proxy_site_path(
    Extension(state): Extension<Arc<RelayShareRegistry>>,
    Extension(tunnel_reg): Extension<Arc<TunnelRegistry>>,
    Path((site_id, subpath)): Path<(String, String)>,
) -> Response {
    let reg = match state.lookup_site(&site_id).await {
        Some(r) => r,
        None => {
            return (StatusCode::NOT_FOUND, Html(offline_page("Site not found"))).into_response()
        }
    };
    let path = format!("/sites/{}/{}", site_id, subpath);
    let direct_url = format!("{}{}", reg.origin_url, path);
    let tunnel_key = format!("site:{}", site_id);
    proxy_via_tunnel_or_http(&tunnel_reg, &tunnel_key, &path, &direct_url).await
}

// ---------------------------------------------------------------------------
// HTML template
// ---------------------------------------------------------------------------

fn offline_page(msg: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Network</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white flex items-center justify-center min-h-screen">
<div class="bg-gray-800 rounded-xl p-8 max-w-md w-full mx-4 shadow-2xl text-center">
<div class="w-16 h-16 bg-gray-700 rounded-full flex items-center justify-center mx-auto mb-4">
<svg class="w-8 h-8 text-red-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18.364 5.636a9 9 0 010 12.728M5.636 18.364a9 9 0 010-12.728M12 9v4m0 4h.01"/></svg>
</div>
<h1 class="text-xl font-bold mb-2">Unavailable</h1>
<p class="text-gray-400 text-sm mb-4">{}</p>
<p class="text-xs text-gray-500">Shared via Chiral Network</p>
</div></body></html>"#,
        msg
    )
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the relay share proxy router. Uses Extension for state injection.
pub fn relay_share_routes(
    state: Arc<RelayShareRegistry>,
    tunnel_reg: Arc<TunnelRegistry>,
) -> Router {
    Router::new()
        // Drive share registration API
        .route("/api/drive/relay-register", post(register_share))
        .route("/api/drive/relay-register/:token", delete(unregister_share))
        // Drive share proxy routes
        .route("/drive/:token", get(proxy_share_root))
        .route("/drive/:token/*path", get(proxy_share_path))
        // Site registration API
        .route("/api/sites/relay-register", post(register_site))
        .route(
            "/api/sites/relay-register/:site_id",
            delete(unregister_site),
        )
        // Site proxy routes
        .route("/sites/:site_id", get(proxy_site_redirect))
        .route("/sites/:site_id/", get(proxy_site_root))
        .route("/sites/:site_id/*path", get(proxy_site_path))
        // WebSocket tunnel
        .route("/api/tunnel/ws", get(tunnel_ws_handler))
        .layer(Extension(state))
        .layer(Extension(tunnel_reg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    const TEST_PRIVATE_KEY: &str =
        "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // -----------------------------------------------------------------------
    // is_safe_origin_url — FM-A04
    // -----------------------------------------------------------------------

    #[test]
    fn safe_origin_accepts_public_dns_and_loopback() {
        assert!(is_safe_origin_url("http://example.com:9419/").is_ok());
        assert!(is_safe_origin_url("https://chiral.network/path").is_ok());
        assert!(is_safe_origin_url("http://127.0.0.1:9419").is_ok());
        assert!(is_safe_origin_url("http://localhost:9419").is_ok());
        assert!(is_safe_origin_url("http://203.0.113.5:9419").is_ok());
    }

    #[test]
    fn safe_origin_rejects_private_link_local_and_metadata() {
        let allowlist = [];
        // RFC1918
        assert!(is_safe_origin_url_with_allowlist("http://10.0.0.1:8500", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://192.168.1.1:8080", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://172.16.1.1:8080", &allowlist).is_err());
        // Link-local (AWS / cloud metadata, etc.)
        assert!(is_safe_origin_url_with_allowlist("http://169.254.169.254/", &allowlist).is_err());
        // CGNAT
        assert!(is_safe_origin_url_with_allowlist("http://100.64.0.1/", &allowlist).is_err());
        // 0.0.0.0
        assert!(is_safe_origin_url_with_allowlist("http://0.0.0.0:8080/", &allowlist).is_err());
        // Multicast
        assert!(is_safe_origin_url_with_allowlist("http://224.0.0.1/", &allowlist).is_err());
        // IPv6 unique-local + link-local
        assert!(is_safe_origin_url_with_allowlist("http://[fc00::1]:8080/", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://[fe80::1]:8080/", &allowlist).is_err());
    }

    #[test]
    fn safe_origin_accepts_explicitly_allowed_private_networks() {
        let allowlist = parse_private_origin_allowlist("10.0.0.0/8,100.64.0.0/10,fc00::/7");

        assert!(is_safe_origin_url_with_allowlist("http://10.2.3.4:9419", &allowlist).is_ok());
        assert!(is_safe_origin_url_with_allowlist("http://100.64.12.34:9419", &allowlist).is_ok());
        assert!(is_safe_origin_url_with_allowlist("http://[fc00::1234]:9419", &allowlist).is_ok());

        assert!(is_safe_origin_url_with_allowlist("http://192.168.1.10:9419", &allowlist).is_err());
    }

    #[test]
    fn safe_origin_accepts_explicit_private_ip_allowlist_entries() {
        let allowlist = parse_private_origin_allowlist("192.168.1.42,fd00::42");

        assert!(is_safe_origin_url_with_allowlist("http://192.168.1.42:9419", &allowlist).is_ok());
        assert!(is_safe_origin_url_with_allowlist("http://192.168.1.43:9419", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://[fd00::42]:9419", &allowlist).is_ok());
        assert!(is_safe_origin_url_with_allowlist("http://[fd00::43]:9419", &allowlist).is_err());
    }

    #[test]
    fn safe_origin_never_allows_link_local_metadata_or_non_routable_targets() {
        let allowlist = parse_private_origin_allowlist("0.0.0.0/0,::/0");

        assert!(is_safe_origin_url_with_allowlist("http://10.0.0.1:9419", &allowlist).is_ok());
        assert!(is_safe_origin_url_with_allowlist("http://169.254.169.254/", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://169.254.1.10/", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://0.0.0.0:9419/", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://224.0.0.1/", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://[fd00:ec2::254]/", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://[fe80::1]:9419/", &allowlist).is_err());
        assert!(is_safe_origin_url_with_allowlist("http://[::]/", &allowlist).is_err());
    }

    #[test]
    fn safe_origin_rejects_unsupported_schemes() {
        assert!(is_safe_origin_url("file:///etc/passwd").is_err());
        assert!(is_safe_origin_url("ftp://attacker.com/").is_err());
        assert!(is_safe_origin_url("gopher://1.2.3.4/").is_err());
    }

    #[test]
    fn safe_origin_handles_userinfo_and_brackets() {
        let allowlist = [];
        // Userinfo is stripped before host check.
        assert!(
            is_safe_origin_url_with_allowlist("http://user:pass@10.0.0.1/", &allowlist).is_err()
        );
        assert!(is_safe_origin_url("http://user:pass@example.com/").is_ok());
        // IPv4-mapped IPv6 walks into v4 rules.
        assert!(
            is_safe_origin_url_with_allowlist("http://[::ffff:10.0.0.1]/", &allowlist).is_err()
        );
        assert!(is_safe_origin_url("http://[::ffff:203.0.113.5]/").is_ok());
    }

    #[test]
    fn register_payload_distinguishes_share_vs_site() {
        let a = register_payload("share", "abc", "0xowner", "http://example/");
        let b = register_payload("site", "abc", "0xowner", "http://example/");
        assert_ne!(a, b);
    }

    #[test]
    fn register_payload_is_injective_under_field_shift() {
        // Length-prefix protects against attacker-controlled NUL/colon
        // shifts across (id, owner_wallet, origin_url).
        let a = register_payload("share", "abc:def", "0xowner", "http://x/");
        let b = register_payload("share", "abc", "def:0xowner", "http://x/");
        assert_ne!(a, b);
    }

    // -----------------------------------------------------------------------
    // fix_origin_url
    // -----------------------------------------------------------------------

    #[test]
    fn test_fix_origin_url_replaces_quad_zero() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5));
        assert_eq!(
            fix_origin_url("http://0.0.0.0:9419", ip),
            "http://203.0.113.5:9419"
        );
    }

    #[test]
    fn test_fix_origin_url_replaces_localhost() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(
            fix_origin_url("http://localhost:9419", ip),
            "http://10.0.0.1:9419"
        );
    }

    #[test]
    fn test_fix_origin_url_replaces_loopback() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42));
        assert_eq!(
            fix_origin_url("http://127.0.0.1:9419/path", ip),
            "http://192.168.1.42:9419/path"
        );
    }

    #[test]
    fn test_fix_origin_url_noop_when_no_placeholder() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(
            fix_origin_url("http://203.0.113.5:9419", ip),
            "http://203.0.113.5:9419"
        );
    }

    #[test]
    fn test_normalize_origin_for_preflight_allows_public_substitution() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5));

        assert_eq!(
            normalize_origin_for_preflight("http://localhost:9419", ip).unwrap(),
            "http://203.0.113.5:9419"
        );
    }

    #[test]
    fn test_normalize_origin_for_preflight_rejects_unsafe_substitution() {
        let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let private = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));

        let loopback_err = normalize_origin_for_preflight("http://localhost:9419", loopback)
            .expect_err("loopback normalized target should fail closed");
        assert!(loopback_err.contains("normalized origin_url"));
        assert!(loopback_err.contains("loopback"));

        let private_err = normalize_origin_for_preflight("http://localhost:9419", private)
            .expect_err("private normalized target should fail closed");
        assert!(private_err.contains("normalized origin_url"));
        assert!(private_err.contains("private"));
    }

    async fn one_shot_http_response(response: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(&response).await;
            }
        });
        format!("http://{}", addr)
    }

    async fn one_shot_http_origin() -> String {
        one_shot_http_response(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec())
            .await
    }

    async fn one_shot_redirect_origin(location: &str) -> String {
        one_shot_http_response(
            format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n")
                .into_bytes(),
        )
        .await
    }

    async fn closed_loopback_origin() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{}", addr)
    }

    fn test_owner_wallet() -> String {
        let signature = crate::wallet::sign_message(TEST_PRIVATE_KEY, b"owner").unwrap();
        crate::wallet::recover_signer(b"owner", &signature).unwrap()
    }

    fn signed_register_request(token: &str, origin_url: &str) -> RegisterRequest {
        let owner_wallet = test_owner_wallet();
        let payload = register_payload("share", token, &owner_wallet, origin_url);
        let signature = crate::wallet::sign_message(TEST_PRIVATE_KEY, &payload).unwrap();
        RegisterRequest {
            token: token.to_string(),
            origin_url: origin_url.to_string(),
            owner_wallet,
            signature,
        }
    }

    fn signed_site_register_request(site_id: &str, origin_url: &str) -> SiteRegisterRequest {
        let owner_wallet = test_owner_wallet();
        let payload = register_payload("site", site_id, &owner_wallet, origin_url);
        let signature = crate::wallet::sign_message(TEST_PRIVATE_KEY, &payload).unwrap();
        SiteRegisterRequest {
            site_id: site_id.to_string(),
            origin_url: origin_url.to_string(),
            owner_wallet,
            signature,
        }
    }

    #[tokio::test]
    async fn preflight_origin_accepts_reachable_http_origin() {
        let origin = one_shot_http_origin().await;

        preflight_origin_reachable(&origin)
            .await
            .expect("reachable origin should pass preflight");
    }

    #[tokio::test]
    async fn preflight_origin_reports_unreachable_origin() {
        let origin = closed_loopback_origin().await;

        let err = preflight_origin_reachable(&origin)
            .await
            .expect_err("closed origin should fail preflight");

        assert!(err.contains("not reachable"));
        assert!(err.contains("firewall/NAT"));
    }

    #[tokio::test]
    async fn preflight_origin_does_not_follow_redirects() {
        let closed_target = closed_loopback_origin().await;
        let origin = one_shot_redirect_origin(&closed_target).await;

        preflight_origin_reachable(&origin)
            .await
            .expect("redirect response should count as origin reachability without following it");
    }

    #[tokio::test]
    async fn register_share_rejects_loopback_normalized_origin_before_preflight() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RelayShareRegistry::new(dir.path().to_path_buf()));
        let req = signed_register_request("unsafe-token", "http://localhost:9");

        let response = register_share(
            Extension(registry.clone()),
            ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 51111))),
            Json(req),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(registry.lookup("unsafe-token").await.is_none());

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("normalized origin_url"));
        assert!(body.contains("loopback"));
    }

    #[tokio::test]
    async fn register_site_rejects_private_normalized_origin_before_preflight() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RelayShareRegistry::new(dir.path().to_path_buf()));
        let req = signed_site_register_request("unsafe-site", "http://localhost:9");

        let response = register_site(
            Extension(registry.clone()),
            ConnectInfo(SocketAddr::from(([10, 0, 0, 7], 51111))),
            Json(req),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(registry.lookup_site("unsafe-site").await.is_none());

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("normalized origin_url"));
        assert!(body.contains("private"));
    }

    // -----------------------------------------------------------------------
    // build_query_string
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_query_string_empty() {
        let params: HashMap<String, String> = HashMap::new();
        assert_eq!(build_query_string(&params), "");
    }

    #[test]
    fn test_build_query_string_single_param() {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        let qs = build_query_string(&params);
        assert_eq!(qs, "?page=1");
    }

    #[test]
    fn test_build_query_string_multiple_params() {
        let mut params = HashMap::new();
        params.insert("a".to_string(), "1".to_string());
        params.insert("b".to_string(), "2".to_string());
        let qs = build_query_string(&params);
        // HashMap iteration order is not guaranteed, so check both possibilities
        assert!(qs == "?a=1&b=2" || qs == "?b=2&a=1");
    }

    // -----------------------------------------------------------------------
    // now_secs
    // -----------------------------------------------------------------------

    #[test]
    fn test_now_secs_reasonable_timestamp() {
        let ts = now_secs();
        // Should be after 2024-01-01 (1704067200) and before 2100-01-01 (4102444800)
        assert!(
            ts > 1_704_067_200,
            "timestamp {} is too far in the past",
            ts
        );
        assert!(
            ts < 4_102_444_800,
            "timestamp {} is too far in the future",
            ts
        );
    }

    // -----------------------------------------------------------------------
    // RelayShareRegistry
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_registry_new() {
        let dir = tempfile::tempdir().unwrap();
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());
        let shares = registry.shares.read().await;
        let sites = registry.sites.read().await;
        assert!(shares.is_empty());
        assert!(sites.is_empty());
    }

    #[tokio::test]
    async fn test_registry_register_and_lookup_share() {
        let dir = tempfile::tempdir().unwrap();
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());

        let reg = ShareRegistration {
            token: "abc123".to_string(),
            origin_url: "http://10.0.0.1:9419".to_string(),
            owner_wallet: "0xWALLET".to_string(),
            registered_at: now_secs(),
        };
        registry.register(reg).await;

        let found = registry.lookup("abc123").await;
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.token, "abc123");
        assert_eq!(found.origin_url, "http://10.0.0.1:9419");
        assert_eq!(found.owner_wallet, "0xWALLET");
    }

    #[tokio::test]
    async fn test_registry_lookup_missing_share() {
        let dir = tempfile::tempdir().unwrap();
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());
        assert!(registry.lookup("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_registry_unregister_share() {
        let dir = tempfile::tempdir().unwrap();
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());

        let reg = ShareRegistration {
            token: "tok1".to_string(),
            origin_url: "http://10.0.0.1:9419".to_string(),
            owner_wallet: "0xWALLET".to_string(),
            registered_at: now_secs(),
        };
        registry.register(reg).await;

        assert!(registry.unregister("tok1").await);
        assert!(registry.lookup("tok1").await.is_none());
        // Unregistering again returns false
        assert!(!registry.unregister("tok1").await);
    }

    #[tokio::test]
    async fn test_registry_register_and_lookup_site() {
        let dir = tempfile::tempdir().unwrap();
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());

        let reg = SiteRegistration {
            site_id: "my-site".to_string(),
            origin_url: "http://10.0.0.1:9419".to_string(),
            owner_wallet: "0xSITEOWNER".to_string(),
            registered_at: now_secs(),
        };
        registry.register_site(reg).await;

        let found = registry.lookup_site("my-site").await;
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.site_id, "my-site");
        assert_eq!(found.origin_url, "http://10.0.0.1:9419");
        assert_eq!(found.owner_wallet, "0xSITEOWNER");
    }

    #[tokio::test]
    async fn test_registry_lookup_missing_site() {
        let dir = tempfile::tempdir().unwrap();
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());
        assert!(registry.lookup_site("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_registry_unregister_site() {
        let dir = tempfile::tempdir().unwrap();
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());

        let reg = SiteRegistration {
            site_id: "site-1".to_string(),
            origin_url: "http://10.0.0.1:9419".to_string(),
            owner_wallet: "0xOWNER".to_string(),
            registered_at: now_secs(),
        };
        registry.register_site(reg).await;

        assert!(registry.unregister_site("site-1").await);
        assert!(registry.lookup_site("site-1").await.is_none());
        assert!(!registry.unregister_site("site-1").await);
    }

    #[tokio::test]
    async fn test_registry_persist_and_load() {
        let dir = tempfile::tempdir().unwrap();

        // Register a share and a site, then drop the registry
        {
            let registry = RelayShareRegistry::new(dir.path().to_path_buf());
            registry
                .register(ShareRegistration {
                    token: "persist-tok".to_string(),
                    origin_url: "http://1.2.3.4:9419".to_string(),
                    owner_wallet: "0xW".to_string(),
                    registered_at: 1000,
                })
                .await;
            registry
                .register_site(SiteRegistration {
                    site_id: "persist-site".to_string(),
                    origin_url: "http://1.2.3.4:9419".to_string(),
                    owner_wallet: "0xS".to_string(),
                    registered_at: 2000,
                })
                .await;
        }

        // Create a fresh registry and load from disk
        let registry = RelayShareRegistry::new(dir.path().to_path_buf());
        registry.load_from_disk().await;

        let share = registry.lookup("persist-tok").await;
        assert!(share.is_some());
        assert_eq!(share.unwrap().owner_wallet, "0xW");

        let site = registry.lookup_site("persist-site").await;
        assert!(site.is_some());
        assert_eq!(site.unwrap().owner_wallet, "0xS");
    }
}
