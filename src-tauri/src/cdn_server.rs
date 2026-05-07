//! CDN server — always-on file hosting with on-chain payment verification.
//!
//! Simplified rebuild of the previous in-line CDN in `chiral_daemon.rs`:
//!
//! - **Fixed price.** The old marketplace-median pricing did per-host DHT
//!   lookups on every `/api/cdn/pricing` call. With only a handful of
//!   hosts, the median wasn't a useful signal, and the DHT lookups made
//!   pricing fetches multi-second. Now the price is a single number,
//!   configurable via `CHIRAL_CDN_PRICE_CHI_PER_MB_MONTH`. Pricing calls
//!   return instantly with pure arithmetic.
//! - **No stale re-query cache.** The old code cached the pricing
//!   computation for 30s as a workaround for the DHT-lookup latency.
//!   Fixed pricing makes the cache redundant.
//! - **One registry mutex.** All reads and writes go through
//!   `with_registry`, which loads from disk under the lock, runs the
//!   caller's closure against the in-memory vector, writes back, and
//!   releases. No more scattered `load → mutate → save` calls that
//!   could race when two handlers fire concurrently.
//!
//! Same external API (status / pricing / upload / list / delete /
//! update-price) so the desktop Hosts page keeps working unchanged.

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex as AsyncMutex;

use crate::dht::DhtService;
use crate::network;

/// One-line description of a transaction's `from` / `to` / `value`
/// for use in upload-mismatch error messages. Best-effort: any RPC
/// failure becomes "<unable to fetch>" so the user still gets the
/// "expected …" half of the message.
async fn describe_tx(tx_hash: &str) -> String {
    let endpoints = crate::geth::wallet_rpc_endpoints();
    let v = match crate::rpc_client::call_with_fallbacks(
        &endpoints,
        "eth_getTransactionByHash",
        serde_json::json!([tx_hash]),
    )
    .await
    {
        Ok(v) if !v.is_null() => v,
        _ => return "<unable to fetch tx>".to_string(),
    };
    let from = v.get("from").and_then(|x| x.as_str()).unwrap_or("?");
    let to = v.get("to").and_then(|x| x.as_str()).unwrap_or("?");
    let value_hex = v.get("value").and_then(|x| x.as_str()).unwrap_or("0x0");
    let value_wei = crate::rpc_client::hex_to_u128(value_hex);
    format!("from={from} to={to} amount={value_wei}")
}

/// Cost of an upload in wei: `ceil(price_wei_per_mb_month * bytes * days
/// / (1 MiB * 30 days))`. Pure u128 math — no f64 truncation, no
/// percentage tolerance. Saturates on overflow to `u128::MAX` so an
/// attacker cannot wrap to a small value via huge inputs (file size is
/// already capped to 500 MiB at the call site, so saturation is
/// defensive only).
fn required_upload_wei(price_wei_per_mb_month: u128, bytes: u128, days: u128) -> u128 {
    let denom: u128 = 1024 * 1024 * 30;
    let numer = price_wei_per_mb_month
        .saturating_mul(bytes)
        .saturating_mul(days);
    if numer == 0 {
        return 0;
    }
    // ceil division: (numer + denom - 1) / denom.
    numer.saturating_add(denom - 1) / denom
}

// ============================================================================
// Types + state
// ============================================================================

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CdnEntry {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub owner_wallet: String,
    pub price_chi_per_month: String,
    pub download_price_chi: String,
    pub payment_tx: String,
    pub uploaded_at: u64,
    pub expires_at: u64,
}

/// One site hosted on this CDN — the always-on counterpart to
/// `/sites/<id>` served by `hosting_server.rs` against the user's local
/// daemon. CDN-hosted sites stay reachable when their owner is offline.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CdnSiteEntry {
    /// Site ID (same opaque ID the user has locally, so we can correlate
    /// HostedSite ↔ CdnSiteEntry without an extra mapping).
    pub site_id: String,
    /// Optional human-readable label (carried over from the local site).
    #[serde(default)]
    pub name: String,
    pub owner_wallet: String,
    /// Total bytes summed across every file in the site.
    pub total_size_bytes: u64,
    pub file_count: u32,
    pub price_chi_per_month: String,
    pub payment_tx: String,
    pub uploaded_at: u64,
    pub expires_at: u64,
}

pub struct CdnState {
    pub storage_dir: PathBuf,
    pub registry_path: PathBuf,
    pub registry: AsyncMutex<Vec<CdnEntry>>,
    /// Subdirectory (`<storage_dir>/sites/<site_id>/<rel_path>`) holding
    /// every CDN-hosted site's filesystem tree.
    pub sites_dir: PathBuf,
    pub sites_registry_path: PathBuf,
    pub sites_registry: AsyncMutex<Vec<CdnSiteEntry>>,
    pub wallet_address: String,
    /// Hex-encoded private key matching `wallet_address`. Required so
    /// the CDN can sign `ChunkResponse::FileInfo` envelopes (FM-A09);
    /// without it, downloaders reject every CDN-served file as
    /// unsigned. Operators set it via the `CHIRAL_CDN_PRIVATE_KEY`
    /// env var at process start. Empty string means "unsigned mode" —
    /// CDN serves only free files until the operator wires a key.
    pub wallet_private_key: String,
    pub price_wei_per_mb_month: u128,
    pub dht: Arc<AsyncMutex<Option<Arc<DhtService>>>>,
}

impl CdnState {
    /// Load the registry from disk at construction so later requests don't
    /// have to. `wallet_address` is the receiver for user CDN payments;
    /// `wallet_private_key` is the matching ECDSA private key used to
    /// sign `FileInfo` envelopes; `dht` is the shared DHT handle so
    /// uploads can register the CDN as a seeder after writing a file.
    pub async fn new(
        wallet_address: String,
        wallet_private_key: String,
        dht: Arc<AsyncMutex<Option<Arc<DhtService>>>>,
    ) -> Self {
        let storage_dir = network::data_dir().join("cdn");
        let registry_path = network::data_dir().join("cdn_registry.json");
        let registry = load_registry(&registry_path).await;
        let sites_dir = storage_dir.join("sites");
        let sites_registry_path = network::data_dir().join("cdn_sites_registry.json");
        let sites_registry = load_sites_registry(&sites_registry_path).await;
        let price_wei_per_mb_month = read_price_env();
        Self {
            storage_dir,
            registry_path,
            registry: AsyncMutex::new(registry),
            sites_dir,
            sites_registry_path,
            sites_registry: AsyncMutex::new(sites_registry),
            wallet_address,
            wallet_private_key,
            price_wei_per_mb_month,
            dht,
        }
    }

    /// Mirror of `with_registry` for the sites registry. Acquires the
    /// per-sites lock, lets the closure mutate the in-memory vec, and
    /// re-persists to disk before releasing.
    async fn with_sites_registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Vec<CdnSiteEntry>) -> R,
    {
        let mut guard = self.sites_registry.lock().await;
        let result = f(&mut guard);
        save_sites_registry(&self.sites_registry_path, &guard).await;
        result
    }

    async fn sites_snapshot(&self) -> Vec<CdnSiteEntry> {
        self.sites_registry.lock().await.clone()
    }

    /// Run a closure with mutable access to the registry, re-persisting
    /// afterward. Serializes against concurrent mutations — previously two
    /// uploads could both load / both mutate / both save, losing one.
    async fn with_registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Vec<CdnEntry>) -> R,
    {
        let mut guard = self.registry.lock().await;
        let result = f(&mut guard);
        save_registry(&self.registry_path, &guard).await;
        result
    }

    async fn snapshot(&self) -> Vec<CdnEntry> {
        self.registry.lock().await.clone()
    }
}

async fn load_registry(path: &PathBuf) -> Vec<CdnEntry> {
    match tokio::fs::read_to_string(path).await {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

async fn save_registry(path: &PathBuf, entries: &[CdnEntry]) {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = tokio::fs::write(path, json).await;
    }
}

async fn load_sites_registry(path: &PathBuf) -> Vec<CdnSiteEntry> {
    match tokio::fs::read_to_string(path).await {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

async fn save_sites_registry(path: &PathBuf, entries: &[CdnSiteEntry]) {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = tokio::fs::write(path, json).await;
    }
}

/// Price per MB per month, in wei. Operator sets `CHIRAL_CDN_PRICE_CHI_PER_MB_MONTH`
/// as a CHI decimal (e.g. `0.001`); default 0.001 CHI = 1e15 wei.
fn read_price_env() -> u128 {
    const DEFAULT_WEI: u128 = 1_000_000_000_000_000; // 0.001 CHI
    match std::env::var("CHIRAL_CDN_PRICE_CHI_PER_MB_MONTH") {
        Ok(v) => crate::wallet::parse_chi_to_wei(&v).unwrap_or(DEFAULT_WEI),
        Err(_) => DEFAULT_WEI,
    }
}

// ============================================================================
// Routes
// ============================================================================

/// `axum::Router` with every `/api/cdn/*` route mounted. `chiral_daemon`
/// composes this into its top-level router.
///
/// CDN-hosted sites use a `/cdn/sites/<id>/*` URL prefix to avoid
/// colliding with the daemon's existing `/sites/<id>/*` route, which is
/// served by `hosting_server.rs` against the local-only `hosted_sites.json`
/// (empty on a CDN-only deployment, but the route slot is still claimed).
pub fn router(state: Arc<CdnState>) -> Router {
    Router::new()
        .route(
            "/api/cdn/upload",
            post(upload).layer(DefaultBodyLimit::max(500 * 1024 * 1024)),
        )
        .route("/api/cdn/files", get(list))
        .route("/api/cdn/files/:file_hash", delete(delete_file).put(update_price))
        .route("/api/cdn/pricing", get(pricing))
        .route("/api/cdn/status", get(status))
        // ── CDN-hosted sites ──────────────────────────────────────────
        .route(
            "/api/cdn/sites/upload",
            post(upload_site).layer(DefaultBodyLimit::max(500 * 1024 * 1024)),
        )
        .route("/api/cdn/sites", get(list_sites))
        .route("/api/cdn/sites/:site_id", delete(delete_site))
        .route("/cdn/sites/:site_id", get(serve_site_redirect))
        .route("/cdn/sites/:site_id/", get(serve_site_root))
        .route("/cdn/sites/:site_id/*path", get(serve_site_file))
        .with_state(state)
}

/// GET /api/cdn/status — service identity + counts + unit price.
async fn status(State(s): State<Arc<CdnState>>) -> Response {
    let peer_id = match s.dht.lock().await.as_ref() {
        Some(d) => d.get_peer_id().await.unwrap_or_default(),
        None => String::new(),
    };
    let now = now_secs();
    let active: Vec<CdnEntry> = s.snapshot().await.into_iter().filter(|e| e.expires_at > now).collect();
    Json(json!({
        "status": "online",
        "peerId": peer_id,
        "walletAddress": s.wallet_address,
        "chainId": crate::geth::chain_id(),
        "networkName": network::active().name,
        "activeFiles": active.len(),
        "totalStorageBytes": active.iter().map(|f| f.file_size).sum::<u64>(),
        "uniqueOwners": active
            .iter()
            .map(|f| f.owner_wallet.to_lowercase())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "pricing": {
            "pricePerMbMonthChi": wei_to_chi(s.price_wei_per_mb_month),
            "pricePerMbMonthWei": s.price_wei_per_mb_month.to_string(),
            "source": "fixed",
        }
    }))
    .into_response()
}

/// GET /api/cdn/pricing?bytes=N&durationDays=Y — pure arithmetic, no DHT.
///
/// `bytes` is the preferred input; pass the exact file byte count so
/// the quote matches what `upload` will compute via the same
/// `required_upload_wei` helper. `sizeMb` is accepted as a legacy
/// fallback (rounded UP to the next byte boundary so the quote is
/// never lower than what the upload would charge — under-quoting was
/// the cause of the "Payment details mismatch amount=…" error users
/// hit after the f64 pricing path lost a few ten-thousandths of a
/// CHI vs the exact u128 upload path).
async fn pricing(
    State(s): State<Arc<CdnState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let duration_days: u64 = params
        .get("durationDays")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let bytes: u128 = if let Some(b) = params.get("bytes").and_then(|s| s.parse().ok()) {
        b
    } else {
        // Legacy fallback: convert sizeMb → bytes, ceil-rounded so the
        // quote always meets-or-exceeds the upload-time requirement.
        let size_mb: f64 = params
            .get("sizeMb")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        (size_mb * (1024.0 * 1024.0)).ceil() as u128
    };
    let total_wei = required_upload_wei(s.price_wei_per_mb_month, bytes, duration_days as u128);
    Json(json!({
        "bytes": bytes.to_string(),
        "durationDays": duration_days,
        "pricePerMbMonthChi": wei_to_chi(s.price_wei_per_mb_month),
        "pricePerMbMonthWei": s.price_wei_per_mb_month.to_string(),
        "totalCostChi": wei_to_chi(total_wei),
        "totalCostWei": total_wei.to_string(),
        "source": "fixed",
    }))
    .into_response()
}

/// GET /api/cdn/files?owner=0xABC — list this CDN's hosted files,
/// optionally scoped to one owner.
async fn list(
    State(s): State<Arc<CdnState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let owner_filter = params.get("owner").cloned().unwrap_or_default().to_lowercase();
    let now = now_secs();
    let files: Vec<CdnEntry> = s
        .snapshot()
        .await
        .into_iter()
        .filter(|e| {
            e.expires_at > now
                && (owner_filter.is_empty() || e.owner_wallet.to_lowercase() == owner_filter)
        })
        .collect();
    let total_bytes: u64 = files.iter().map(|f| f.file_size).sum();
    Json(json!({
        "files": files,
        "totalFiles": files.len(),
        "storageUsedBytes": total_bytes,
    }))
    .into_response()
}

/// POST /api/cdn/upload — receive a file, verify payment, store + register in DHT.
///
/// Payment verification runs in parallel with the multipart body upload so
/// the client only waits `max(block_time, upload_time)` instead of both
/// serially. All the metadata (payment tx hash, wallet, duration, download
/// price) comes in headers so the verification can start before the body
/// is fully received.
async fn upload(
    State(s): State<Arc<CdnState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let payment_tx = hdr(&headers, "X-Payment-Tx");
    let owner_wallet = hdr(&headers, "X-Owner-Wallet");
    let duration_days: u64 = hdr(&headers, "X-Duration-Days").parse().unwrap_or(30);
    let download_price_chi = {
        let v = hdr(&headers, "X-Download-Price-Chi");
        if v.is_empty() { "0".to_string() } else { v }
    };

    if payment_tx.is_empty() || owner_wallet.is_empty() {
        return err(StatusCode::BAD_REQUEST, "X-Payment-Tx and X-Owner-Wallet headers required");
    }
    if s.wallet_address.is_empty() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "CDN wallet not configured");
    }

    // Kick off block-wait in parallel with body upload — the tx hash is in
    // the header so we don't have to wait for the body first.
    let tx_for_task = payment_tx.clone();
    let mined_task = tokio::spawn(async move {
        crate::wallet::wait_for_tx_mined(&tx_for_task).await
    });

    // Receive file from multipart.
    let mut file_name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return err(StatusCode::BAD_REQUEST, &format!("Multipart error: {e}")),
        };
        if field.name() == Some("file") {
            file_name = field.file_name().map(String::from);
            match field.bytes().await {
                Ok(b) => file_data = Some(b.to_vec()),
                Err(e) => return err(StatusCode::BAD_REQUEST, &format!("Read file: {e}")),
            }
        }
    }
    let file_name = match file_name.filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => return err(StatusCode::BAD_REQUEST, "Multipart file field missing"),
    };
    let file_data = match file_data {
        Some(d) if !d.is_empty() => d,
        _ => return err(StatusCode::BAD_REQUEST, "Empty file"),
    };
    if file_data.len() > 500 * 1024 * 1024 {
        return err(StatusCode::BAD_REQUEST, "File exceeds 500MB limit");
    }

    let file_size = file_data.len() as u64;
    // Exact integer pricing — no f64. `required_wei = ceil(price * bytes * days
    // / (1 MiB * 30 days))` so a buyer paying any sub-wei boundary still owes
    // the next whole wei. The earlier `* 95 / 100` "5% tolerance" was a CHI→wei
    // rounding-error excuse for a slack two thousand million times the actual
    // physical rounding (1 wei out of 1e18); systematic 5% underpayment was
    // the result.
    let required_wei = required_upload_wei(
        s.price_wei_per_mb_month,
        file_data.len() as u128,
        duration_days as u128,
    );
    let min_accepted_wei = required_wei;

    // Join the parallel mining wait.
    let mined = match mined_task.await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Verify failed: {e}")),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Verify task panicked: {e}")),
    };
    if !mined {
        return err(
            StatusCode::PAYMENT_REQUIRED,
            &format!("Payment not confirmed in time. Tx: {payment_tx}"),
        );
    }

    // Tx is mined; now check from/to/value. On mismatch we re-fetch the
    // tx and tell the user *what* went wrong (which usually pinpoints
    // the cause — e.g. payment sent to a stale CDN wallet address from
    // a cached session).
    match crate::wallet::verify_tx_details(&payment_tx, &owner_wallet, &s.wallet_address, min_accepted_wei).await {
        Ok(true) => {}
        Ok(false) => {
            let observed = describe_tx(&payment_tx).await;
            return err(
                StatusCode::PAYMENT_REQUIRED,
                &format!(
                    "Payment details mismatch. Expected from={owner_wallet} to={} amount>={min_accepted_wei}. Observed: {observed}",
                    s.wallet_address
                ),
            );
        }
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Detail check: {e}")),
    }

    // Hash + write to disk.
    let file_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&file_data);
        hex::encode(hasher.finalize())
    };
    let _ = tokio::fs::create_dir_all(&s.storage_dir).await;
    let file_path = s.storage_dir.join(&file_hash);
    if let Err(e) = tokio::fs::write(&file_path, &file_data).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Write file: {e}"));
    }

    // Register in DHT so clients searching by hash find the CDN as a seeder.
    let now = now_secs();
    let expires = now + duration_days * 86400;
    let download_price_wei = parse_chi_or_zero(&download_price_chi);
    if let Some(dht) = s.dht.lock().await.as_ref() {
        register_in_dht(
            dht,
            &file_hash,
            &file_path,
            &file_name,
            file_size,
            download_price_wei,
            &s.wallet_address,
            &s.wallet_private_key,
            now,
        )
        .await;
    }

    // Persist registry entry.
    s.with_registry(|r| {
        r.retain(|e| e.file_hash != file_hash);
        r.push(CdnEntry {
            file_hash: file_hash.clone(),
            file_name: file_name.clone(),
            file_size,
            owner_wallet: owner_wallet.clone(),
            price_chi_per_month: wei_to_chi(s.price_wei_per_mb_month),
            download_price_chi: download_price_chi.clone(),
            payment_tx: payment_tx.clone(),
            uploaded_at: now,
            expires_at: expires,
        });
    })
    .await;

    Json(json!({
        "status": "uploaded",
        "fileHash": file_hash,
        "fileName": file_name,
        "fileSize": file_size,
        "expiresAt": expires,
        "pricing": {
            "pricePerMbMonthChi": wei_to_chi(s.price_wei_per_mb_month),
            "totalCostChi": wei_to_chi(required_wei),
            "totalCostWei": required_wei.to_string(),
            "durationDays": duration_days,
            "source": "fixed",
        }
    }))
    .into_response()
}

/// DELETE /api/cdn/files/:file_hash?owner=0xABC — unregister + delete file.
async fn delete_file(
    State(s): State<Arc<CdnState>>,
    AxumPath(file_hash): AxumPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let owner = params.get("owner").cloned().unwrap_or_default().to_lowercase();
    if owner.is_empty() {
        return err(StatusCode::BAD_REQUEST, "owner query param required");
    }
    let removed = s
        .with_registry(|r| {
            let before = r.len();
            r.retain(|e| !(e.file_hash == file_hash && e.owner_wallet.to_lowercase() == owner));
            before != r.len()
        })
        .await;
    if !removed {
        return err(StatusCode::NOT_FOUND, "File not found or not owned by this wallet");
    }
    let _ = tokio::fs::remove_file(s.storage_dir.join(&file_hash)).await;
    if let Some(dht) = s.dht.lock().await.as_ref() {
        unregister_in_dht(dht, &file_hash).await;
    }
    Json(json!({ "status": "deleted", "fileHash": file_hash })).into_response()
}

/// PUT /api/cdn/files/:file_hash — change the download price (seeder price
/// downstream clients pay when they fetch from this CDN).
async fn update_price(
    State(s): State<Arc<CdnState>>,
    AxumPath(file_hash): AxumPath<String>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let owner = body["owner"].as_str().unwrap_or("").to_lowercase();
    let new_price = match &body["downloadPriceChi"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => "0".to_string(),
    };
    if owner.is_empty() {
        return err(StatusCode::BAD_REQUEST, "owner required");
    }

    let found_entry = s
        .with_registry(|r| {
            r.iter_mut()
                .find(|e| e.file_hash == file_hash && e.owner_wallet.to_lowercase() == owner)
                .map(|e| {
                    e.download_price_chi = new_price.clone();
                    e.clone()
                })
        })
        .await;
    let entry = match found_entry {
        Some(e) => e,
        None => return err(StatusCode::NOT_FOUND, "File not found or not owned by this wallet"),
    };

    // Re-register in DHT with the new price so searches pick it up.
    let new_price_wei = parse_chi_or_zero(&new_price);
    if let Some(dht) = s.dht.lock().await.as_ref() {
        let file_path = s.storage_dir.join(&file_hash);
        if file_path.exists() {
            register_in_dht(
                dht,
                &file_hash,
                &file_path,
                &entry.file_name,
                entry.file_size,
                new_price_wei,
                &s.wallet_address,
                &s.wallet_private_key,
                entry.uploaded_at,
            )
            .await;
        }
    }
    Json(json!({ "status": "updated", "downloadPriceChi": new_price })).into_response()
}

// ============================================================================
// Background tasks
// ============================================================================

/// Re-register every non-expired CDN file in the DHT on startup. Run once
/// after the DHT service is ready.
pub async fn reseed_on_startup(state: Arc<CdnState>) {
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    let dht = {
        let guard = state.dht.lock().await;
        guard.as_ref().cloned()
    };
    let Some(dht) = dht else { return };
    let now = now_secs();
    let active: Vec<CdnEntry> = state.snapshot().await.into_iter().filter(|e| e.expires_at > now).collect();
    for entry in &active {
        let file_path = state.storage_dir.join(&entry.file_hash);
        if !file_path.exists() {
            continue;
        }
        let download_price_wei = parse_chi_or_zero(&entry.download_price_chi);
        register_in_dht(
            &dht,
            &entry.file_hash,
            &file_path,
            &entry.file_name,
            entry.file_size,
            download_price_wei,
            &state.wallet_address,
            &state.wallet_private_key,
            entry.uploaded_at,
        )
        .await;
    }
    if !active.is_empty() {
        println!("[CDN] Re-seeded {} files on startup", active.len());
    }
}

/// Every 60 seconds, drop expired entries from the registry + disk + DHT.
pub async fn expiration_loop(state: Arc<CdnState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        let now = now_secs();
        let expired = state
            .with_registry(|r| {
                let expired: Vec<CdnEntry> = r.iter().filter(|e| e.expires_at <= now).cloned().collect();
                r.retain(|e| e.expires_at > now);
                expired
            })
            .await;
        if !expired.is_empty() {
            let dht = state.dht.lock().await.as_ref().cloned();
            for entry in &expired {
                let _ = tokio::fs::remove_file(state.storage_dir.join(&entry.file_hash)).await;
                if let Some(ref d) = dht {
                    unregister_in_dht(d, &entry.file_hash).await;
                }
            }
            println!("[CDN] Expiration cleanup removed {} files", expired.len());
        }

        // Same housekeeping for hosted sites — drop expired registry rows
        // and rm -rf each site's directory so we don't pay disk for sites
        // whose hosting term is up.
        let expired_sites = state
            .with_sites_registry(|r| {
                let expired: Vec<CdnSiteEntry> = r.iter().filter(|e| e.expires_at <= now).cloned().collect();
                r.retain(|e| e.expires_at > now);
                expired
            })
            .await;
        if !expired_sites.is_empty() {
            for site in &expired_sites {
                let _ = tokio::fs::remove_dir_all(state.sites_dir.join(&site.site_id)).await;
            }
            println!(
                "[CDN] Expiration cleanup removed {} sites",
                expired_sites.len()
            );
        }
    }
}

// ============================================================================
// DHT helpers
// ============================================================================

async fn register_in_dht(
    dht: &Arc<DhtService>,
    file_hash: &str,
    file_path: &std::path::Path,
    file_name: &str,
    file_size: u64,
    download_price_wei: u128,
    cdn_wallet: &str,
    cdn_private_key: &str,
    created_at: u64,
) {
    dht.register_shared_file(
        file_hash.to_string(),
        file_path.to_string_lossy().to_string(),
        file_name.to_string(),
        file_size,
        download_price_wei,
        cdn_wallet.to_string(),
        cdn_private_key.to_string(),
    )
    .await;
    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    let our_addrs = dht.get_listening_addresses().await;

    // Without a signing key the CDN can populate its local shared_files
    // map (so a peer that already knows where to find this hash could
    // still receive bytes), but cannot publish DHT records that any
    // FM-A07/A08-aware client will accept. Fail-fast and log instead of
    // writing unsigned records — they'd just be dropped on read and
    // confuse operators looking at DHT state.
    if cdn_private_key.is_empty() || cdn_wallet.is_empty() {
        println!(
            "[CDN] DHT publish for {} skipped — wallet key not configured (set CHIRAL_WALLET_KEY_FILE)",
            file_hash
        );
        let _ = (peer_id, our_addrs, created_at);
        return;
    }

    // Always publish the metadata blob — both on initial upload AND on
    // every startup re-seed. The CDN is the canonical seeder for files
    // uploaded to it, so it owns this record and re-publishing
    // refreshes the Kademlia record TTL. An earlier optimization
    // gated this on a `blob_present` check, which interacted badly
    // with first-hit Kademlia: a stale local copy would short-circuit
    // the put, then expire from the local store, and the file would
    // become unreachable on search. Multiple peers signing the same
    // chiral_file_<hash> key under their own wallet is harmless —
    // verify_publisher accepts whichever blob the reader sees.
    let key = format!("chiral_file_{file_hash}");
    match crate::try_make_signed_file_metadata(
        file_hash,
        file_name,
        file_size,
        "WebRTC",
        cdn_wallet,
        Some(cdn_private_key),
    ) {
        Some(metadata) => match serde_json::to_string(&metadata) {
            Ok(json_str) => {
                if let Err(e) = dht.put_dht_value(key, json_str).await {
                    println!(
                        "[CDN] FileMetadata blob put failed for {}: {}",
                        file_hash, e
                    );
                }
            }
            Err(e) => println!("[CDN] Failed to serialize FileMetadata for {}: {}", file_hash, e),
        },
        None => println!(
            "[CDN] Failed to sign FileMetadata for {} — record not published",
            file_hash
        ),
    }
    let _ = created_at;

    // The CDN is just another seeder in the provider-records model: publish
    // a signed per-seeder record + register as a Kademlia provider.
    match crate::try_make_signed_seeder(
        &peer_id,
        file_hash,
        &download_price_wei.to_string(),
        cdn_wallet,
        our_addrs,
        Some(cdn_private_key),
    ) {
        Some(seeder_entry) => {
            if let Err(e) = crate::publish_seeder_entry(dht, file_hash, &seeder_entry).await {
                println!("[CDN] Provider publish failed for {}: {}", file_hash, e);
            }
        }
        None => println!(
            "[CDN] Failed to sign SeederInfo for {} — provider record not published",
            file_hash
        ),
    }
}

async fn unregister_in_dht(dht: &Arc<DhtService>, file_hash: &str) {
    dht.unregister_shared_file(file_hash).await;
    // Stop being a Kademlia provider; the immutable blob is left alone.
    let _ = crate::remove_seeder_entry(dht, file_hash).await;

    // Stage 2: stop being a Kademlia provider for this file.
    let _ = crate::remove_seeder_entry(dht, file_hash).await;
}

// ============================================================================
// Site hosting on the CDN — always-on counterpart to local hosting_server.
//
// Wire format for `POST /api/cdn/sites/upload`:
//   - X-Site-Id          (required) opaque id (8+ chars [a-z0-9-_])
//   - X-Site-Name        (optional) display name
//   - X-Owner-Wallet     (required)
//   - X-Payment-Tx       (required)
//   - X-Duration-Days    (default 30)
//   - body: multipart/form-data with one or more `file` fields. Each
//     field's `filename` is the file's path relative to the site root
//     (e.g. "index.html", "assets/style.css"). Path traversal is rejected.
// ============================================================================

fn validate_site_id(id: &str) -> Result<&str, &'static str> {
    if id.is_empty() || id.len() > 64 {
        return Err("Invalid site id");
    }
    let ok = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err("Site id may only contain a-z, 0-9, '-', '_'");
    }
    Ok(id)
}

/// Reject `..`, null, absolute paths, and Windows drive letters from
/// site-relative file paths during upload + serve.
fn validate_site_rel_path(path: &str) -> Result<&str, &'static str> {
    if path.is_empty() {
        return Err("Empty file path");
    }
    if path.contains('\0') || path.starts_with('/') || path.starts_with('\\') {
        return Err("Invalid file path");
    }
    for component in path.split(&['/', '\\']) {
        if component == ".." || component.is_empty() {
            return Err("Path traversal not allowed");
        }
    }
    Ok(path)
}

/// POST /api/cdn/sites/upload — multipart upload of every file in a site.
async fn upload_site(
    State(s): State<Arc<CdnState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let site_id_raw = hdr(&headers, "X-Site-Id");
    let site_id = match validate_site_id(&site_id_raw) {
        Ok(id) => id.to_string(),
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let site_name = {
        let n = hdr(&headers, "X-Site-Name");
        if n.is_empty() { site_id.clone() } else { n }
    };
    let owner_wallet = hdr(&headers, "X-Owner-Wallet");
    let payment_tx = hdr(&headers, "X-Payment-Tx");
    let duration_days: u64 = hdr(&headers, "X-Duration-Days").parse().unwrap_or(30);

    if owner_wallet.is_empty() || payment_tx.is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "X-Owner-Wallet and X-Payment-Tx headers required",
        );
    }
    if s.wallet_address.is_empty() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "CDN wallet not configured");
    }

    // Verify-mining runs in parallel with reading the multipart body.
    let tx_for_task = payment_tx.clone();
    let mined_task = tokio::spawn(async move {
        crate::wallet::wait_for_tx_mined(&tx_for_task).await
    });

    // Buffer all files in memory so we can compute the total size + verify
    // payment before we touch disk. Per-file 50 MB and total 500 MB caps
    // mirror the file-upload path's body limits.
    const MAX_TOTAL: u64 = 500 * 1024 * 1024;
    const MAX_PER_FILE: u64 = 50 * 1024 * 1024;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total: u64 = 0;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return err(StatusCode::BAD_REQUEST, &format!("Multipart error: {e}")),
        };
        if field.name() != Some("file") {
            continue;
        }
        let rel_path = match field.file_name().map(String::from) {
            Some(name) if !name.is_empty() => name,
            _ => return err(StatusCode::BAD_REQUEST, "Multipart file missing filename"),
        };
        if let Err(e) = validate_site_rel_path(&rel_path) {
            return err(StatusCode::BAD_REQUEST, e);
        }
        let bytes = match field.bytes().await {
            Ok(b) => b.to_vec(),
            Err(e) => return err(StatusCode::BAD_REQUEST, &format!("Read file: {e}")),
        };
        if bytes.is_empty() {
            return err(StatusCode::BAD_REQUEST, "Empty file in upload");
        }
        if bytes.len() as u64 > MAX_PER_FILE {
            return err(StatusCode::BAD_REQUEST, "File exceeds 50 MB per-file limit");
        }
        total += bytes.len() as u64;
        if total > MAX_TOTAL {
            return err(StatusCode::BAD_REQUEST, "Site exceeds 500 MB total limit");
        }
        entries.push((rel_path, bytes));
    }
    if entries.is_empty() {
        return err(StatusCode::BAD_REQUEST, "No files in upload");
    }

    let required_wei = required_upload_wei(
        s.price_wei_per_mb_month,
        total as u128,
        duration_days as u128,
    );
    let min_accepted_wei = required_wei;

    // Wait for the tx to mine.
    let mined = match mined_task.await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Verify failed: {e}")),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Verify task panicked: {e}")),
    };
    if !mined {
        return err(
            StatusCode::PAYMENT_REQUIRED,
            &format!("Payment not confirmed in time. Tx: {payment_tx}"),
        );
    }

    // Then sender / recipient / amount.
    match crate::wallet::verify_tx_details(
        &payment_tx,
        &owner_wallet,
        &s.wallet_address,
        min_accepted_wei,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            return err(
                StatusCode::PAYMENT_REQUIRED,
                &format!(
                    "Payment details mismatch. Expected from={owner_wallet} to={} amount>={min_accepted_wei}",
                    s.wallet_address
                ),
            );
        }
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Detail check: {e}")),
    }

    // Write the entire tree under <sites_dir>/<site_id>/. Any pre-existing
    // copy of this site is replaced (re-publish overwrites).
    let site_root = s.sites_dir.join(&site_id);
    let _ = tokio::fs::remove_dir_all(&site_root).await;
    if let Err(e) = tokio::fs::create_dir_all(&site_root).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Create site dir: {e}"));
    }
    for (rel_path, bytes) in &entries {
        let dest = site_root.join(rel_path);
        if let Some(parent) = dest.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Create dir: {e}"));
            }
        }
        if let Err(e) = tokio::fs::write(&dest, bytes).await {
            return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("Write file: {e}"));
        }
    }

    let now = now_secs();
    let expires = now + duration_days * 86400;
    let entry = CdnSiteEntry {
        site_id: site_id.clone(),
        name: site_name,
        owner_wallet: owner_wallet.clone(),
        total_size_bytes: total,
        file_count: entries.len() as u32,
        price_chi_per_month: wei_to_chi(s.price_wei_per_mb_month),
        payment_tx: payment_tx.clone(),
        uploaded_at: now,
        expires_at: expires,
    };
    let entry_for_resp = entry.clone();
    s.with_sites_registry(|r| {
        r.retain(|e| e.site_id != site_id);
        r.push(entry);
    })
    .await;

    Json(json!({
        "status": "uploaded",
        "siteId": site_id,
        "fileCount": entry_for_resp.file_count,
        "totalSizeBytes": entry_for_resp.total_size_bytes,
        "expiresAt": expires,
        "pricing": {
            "pricePerMbMonthChi": wei_to_chi(s.price_wei_per_mb_month),
            "totalCostChi": wei_to_chi(required_wei),
            "totalCostWei": required_wei.to_string(),
            "durationDays": duration_days,
            "source": "fixed",
        }
    }))
    .into_response()
}

/// GET /api/cdn/sites?owner=0xABC — list non-expired sites, optionally
/// scoped to one owner.
async fn list_sites(
    State(s): State<Arc<CdnState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let owner_filter = params.get("owner").cloned().unwrap_or_default().to_lowercase();
    let now = now_secs();
    let sites: Vec<CdnSiteEntry> = s
        .sites_snapshot()
        .await
        .into_iter()
        .filter(|e| {
            e.expires_at > now
                && (owner_filter.is_empty() || e.owner_wallet.to_lowercase() == owner_filter)
        })
        .collect();
    let total_bytes: u64 = sites.iter().map(|e| e.total_size_bytes).sum();
    Json(json!({
        "sites": sites,
        "totalSites": sites.len(),
        "storageUsedBytes": total_bytes,
    }))
    .into_response()
}

/// DELETE /api/cdn/sites/:site_id?owner=0xABC — remove site files + registry.
async fn delete_site(
    State(s): State<Arc<CdnState>>,
    AxumPath(site_id): AxumPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let owner = params.get("owner").cloned().unwrap_or_default().to_lowercase();
    if owner.is_empty() {
        return err(StatusCode::BAD_REQUEST, "owner query param required");
    }
    let removed = s
        .with_sites_registry(|r| {
            let before = r.len();
            r.retain(|e| !(e.site_id == site_id && e.owner_wallet.to_lowercase() == owner));
            before != r.len()
        })
        .await;
    if !removed {
        return err(StatusCode::NOT_FOUND, "Site not found or not owned by this wallet");
    }
    let site_root = s.sites_dir.join(&site_id);
    let _ = tokio::fs::remove_dir_all(&site_root).await;
    Json(json!({ "status": "deleted", "siteId": site_id })).into_response()
}

async fn serve_site_redirect(AxumPath(site_id): AxumPath<String>) -> Response {
    (
        StatusCode::PERMANENT_REDIRECT,
        [("Location", format!("/cdn/sites/{}/", site_id))],
    )
        .into_response()
}

async fn serve_site_root(
    State(s): State<Arc<CdnState>>,
    AxumPath(site_id): AxumPath<String>,
) -> Response {
    serve_site_path_inner(&s, &site_id, "index.html").await
}

async fn serve_site_file(
    State(s): State<Arc<CdnState>>,
    AxumPath((site_id, file_path)): AxumPath<(String, String)>,
) -> Response {
    let path = if file_path.is_empty() || file_path == "/" {
        "index.html"
    } else {
        &file_path
    };
    serve_site_path_inner(&s, &site_id, path).await
}

async fn serve_site_path_inner(s: &CdnState, site_id: &str, requested_path: &str) -> Response {
    if validate_site_id(site_id).is_err() {
        return err(StatusCode::BAD_REQUEST, "Invalid site id");
    }
    if let Err(e) = validate_site_rel_path(requested_path) {
        return err(StatusCode::BAD_REQUEST, e);
    }
    // Verify the site is in the registry and not expired.
    let now = now_secs();
    let known = s
        .sites_snapshot()
        .await
        .into_iter()
        .any(|e| e.site_id == site_id && e.expires_at > now);
    if !known {
        return err(StatusCode::NOT_FOUND, "Site not found");
    }

    let site_root = s.sites_dir.join(site_id);
    let resolved = site_root.join(requested_path);
    let canonical = match resolved.canonicalize() {
        Ok(p) => p,
        Err(_) => return err(StatusCode::NOT_FOUND, "File not found"),
    };
    let canonical_root = match site_root.canonicalize() {
        Ok(p) => p,
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "Site dir error"),
    };
    if !canonical.starts_with(&canonical_root) {
        return err(StatusCode::FORBIDDEN, "Path traversal not allowed");
    }
    let data = match tokio::fs::read(&canonical).await {
        Ok(d) => d,
        Err(_) => return err(StatusCode::NOT_FOUND, "File not found"),
    };
    let ext = canonical.extension().and_then(|e| e.to_str()).unwrap_or("");
    let content_type = crate::hosting::mime_from_extension(ext);
    (
        StatusCode::OK,
        [
            ("Content-Type", content_type.to_string()),
            ("Content-Length", data.len().to_string()),
            ("Cache-Control", "public, max-age=3600".to_string()),
        ],
        data,
    )
        .into_response()
}

// ============================================================================
// Small utils
// ============================================================================

fn hdr(h: &HeaderMap, key: &str) -> String {
    h.get(key).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

fn err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format wei as a CHI decimal string LOSSLESSLY. The frontend feeds
/// the returned string back into `parse_chi_to_wei` when it builds
/// the upload payment, so any precision loss here makes the buyer
/// pay slightly less than `required_upload_wei` requires and the
/// upload then rejects the tx as "amount too low" — exactly the
/// "amount=601000000000000 (need 601171493530274)" mismatch we hit
/// after the f64-pricing fix landed.
///
/// Format: integer CHI part, optional dot + fractional part with
/// trailing zeros trimmed. Uses pure integer arithmetic so the
/// result, fed back through `parse_chi_to_wei`, returns exactly
/// the input wei.
fn wei_to_chi(wei: u128) -> String {
    const ONE_CHI: u128 = 1_000_000_000_000_000_000;
    let whole = wei / ONE_CHI;
    let frac = wei % ONE_CHI;
    if frac == 0 {
        return whole.to_string();
    }
    let frac_str = format!("{:018}", frac);
    let trimmed = frac_str.trim_end_matches('0');
    format!("{}.{}", whole, trimmed)
}

fn parse_chi_or_zero(s: &str) -> u128 {
    if s.is_empty() || s == "0" {
        0
    } else {
        crate::wallet::parse_chi_to_wei(s).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_arithmetic_one_mb_one_month() {
        let price_per_mb_month: u128 = 1_000_000_000_000_000; // 0.001 CHI
        let size_mb = 1.0_f64;
        let months = 1.0_f64;
        let total = (price_per_mb_month as f64 * size_mb * months) as u128;
        assert_eq!(total, 1_000_000_000_000_000);
    }

    #[test]
    fn pricing_arithmetic_100_mb_30_days() {
        let price_per_mb_month: u128 = 1_000_000_000_000_000;
        let total = (price_per_mb_month as f64 * 100.0 * (30.0 / 30.0)) as u128;
        assert_eq!(total, 100_000_000_000_000_000);
    }

    #[test]
    fn wei_to_chi_display_small() {
        assert_eq!(wei_to_chi(1_000_000_000_000_000), "0.001");
    }

    #[test]
    fn wei_to_chi_display_zero() {
        assert_eq!(wei_to_chi(0), "0");
    }

    #[test]
    fn wei_to_chi_round_trip_is_lossless() {
        // The frontend re-parses the CHI string we return into wei via
        // `parse_chi_to_wei` to build the payment tx. Every legitimate
        // wei value the upload handler might charge MUST round-trip
        // back to itself, otherwise the tx pays slightly less than
        // required_upload_wei and the upload rejects it.
        for wei in [
            0u128,
            1,
            999,
            601_171_493_530_274, // the exact value from the user's mismatch
            1_000_000_000_000_000_000,
            123_456_789_012_345_678_901_234,
            u128::MAX / 2,
        ] {
            let chi = wei_to_chi(wei);
            let back = crate::wallet::parse_chi_to_wei(&chi)
                .unwrap_or_else(|e| panic!("parse_chi_to_wei({chi:?}) failed: {e}"));
            assert_eq!(back, wei, "wei={} → chi={:?} → wei={}", wei, chi, back);
        }
    }

    #[test]
    fn parse_chi_or_zero_handles_zero_and_empty() {
        assert_eq!(parse_chi_or_zero(""), 0);
        assert_eq!(parse_chi_or_zero("0"), 0);
    }

    #[test]
    fn parse_chi_or_zero_handles_valid_chi() {
        assert_eq!(parse_chi_or_zero("0.001"), 1_000_000_000_000_000);
    }

    /// Zero-input identities — the upload handler returns 0 immediately
    /// (no payment required) when any of price/bytes/days is 0.
    #[test]
    fn required_upload_wei_zero_inputs() {
        assert_eq!(required_upload_wei(0, 1, 1), 0);
        assert_eq!(required_upload_wei(1, 0, 1), 0);
        assert_eq!(required_upload_wei(1, 1, 0), 0);
    }

    /// One MiB stored for 30 days at 1 wei/MiB-month should cost
    /// exactly 1 wei. Locks in the unit math so a future refactor
    /// doesn't accidentally double-charge or under-charge.
    #[test]
    fn required_upload_wei_unit_math() {
        let one_mib = 1024u128 * 1024;
        assert_eq!(required_upload_wei(1, one_mib, 30), 1);
        // Doubling the bytes doubles the cost.
        assert_eq!(required_upload_wei(1, one_mib * 2, 30), 2);
        // Doubling the days doubles the cost.
        assert_eq!(required_upload_wei(1, one_mib, 60), 2);
        // Doubling the price doubles the cost.
        assert_eq!(required_upload_wei(2, one_mib, 30), 2);
    }

    /// Sub-MiB or sub-month inputs should round UP, never down — a
    /// floor would let a buyer pay 0 wei to host a small file briefly.
    /// One byte for one day at 1 wei/MiB-month is a tiny fraction; the
    /// ceil rounds it to 1 wei (the smallest unit) instead of dropping
    /// it to 0.
    #[test]
    fn required_upload_wei_ceils_to_at_least_one() {
        assert_eq!(required_upload_wei(1, 1, 1), 1);
        assert_eq!(required_upload_wei(1, 100, 1), 1);
        // Just shy of one full unit also rounds up to 1.
        let one_mib = 1024u128 * 1024;
        assert_eq!(required_upload_wei(1, one_mib - 1, 30), 1);
    }

    /// Saturating multiplication on huge inputs: an attacker submitting
    /// `u128::MAX` bytes shouldn't wrap around to a small value. The
    /// upload site limit (500 MiB) makes this defensive only, but lock
    /// it down so a future bug elsewhere can't slip a wrap-around past
    /// payment verification.
    #[test]
    fn required_upload_wei_saturates_on_overflow() {
        let huge = u128::MAX;
        // Any combination that would naturally overflow saturates to a
        // value at least as large as the inputs (i.e. doesn't wrap to a
        // small attacker-friendly number).
        let r = required_upload_wei(huge, huge, huge);
        assert!(r > 0, "saturation must not produce 0 (would mean free upload)");
        assert!(r >= u128::MAX / (1024 * 1024 * 30), "must be large after saturation");
    }

    /// Common real-world combo: 1 MiB at 0.001 CHI/MiB-month for 30 days
    /// = exactly 0.001 CHI = 1e15 wei. Mirrors the unit math test in
    /// real-money values so a regression in the constants is obvious.
    #[test]
    fn required_upload_wei_realistic_pricing() {
        let price_chi_per_mb_month: u128 = 1_000_000_000_000_000; // 0.001 CHI
        let one_mib = 1024u128 * 1024;
        let cost = required_upload_wei(price_chi_per_mb_month, one_mib, 30);
        assert_eq!(cost, 1_000_000_000_000_000); // 0.001 CHI in wei
    }
}
