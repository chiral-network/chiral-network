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

pub struct CdnState {
    pub storage_dir: PathBuf,
    pub registry_path: PathBuf,
    pub registry: AsyncMutex<Vec<CdnEntry>>,
    pub wallet_address: String,
    pub price_wei_per_mb_month: u128,
    pub dht: Arc<AsyncMutex<Option<Arc<DhtService>>>>,
}

impl CdnState {
    /// Load the registry from disk at construction so later requests don't
    /// have to. `wallet_address` is the receiver for user CDN payments;
    /// `dht` is the shared DHT handle so uploads can register the CDN as a
    /// seeder after writing a file.
    pub async fn new(
        wallet_address: String,
        dht: Arc<AsyncMutex<Option<Arc<DhtService>>>>,
    ) -> Self {
        let storage_dir = network::data_dir().join("cdn");
        let registry_path = network::data_dir().join("cdn_registry.json");
        let registry = load_registry(&registry_path).await;
        let price_wei_per_mb_month = read_price_env();
        Self {
            storage_dir,
            registry_path,
            registry: AsyncMutex::new(registry),
            wallet_address,
            price_wei_per_mb_month,
            dht,
        }
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

/// GET /api/cdn/pricing?sizeMb=X&durationDays=Y — pure arithmetic, no DHT.
async fn pricing(
    State(s): State<Arc<CdnState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let size_mb: f64 = params.get("sizeMb").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let duration_days: f64 = params.get("durationDays").and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let months = duration_days / 30.0;
    let total_wei = (s.price_wei_per_mb_month as f64 * size_mb * months) as u128;
    Json(json!({
        "sizeMb": size_mb,
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
    let file_mb = file_data.len() as f64 / (1024.0 * 1024.0);
    let months = duration_days as f64 / 30.0;
    let required_wei = (s.price_wei_per_mb_month as f64 * file_mb * months) as u128;
    let min_accepted_wei = required_wei * 95 / 100; // 5% tolerance for CHI→wei rounding

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

    // Tx is mined; now check from/to/value.
    match crate::wallet::verify_tx_details(&payment_tx, &owner_wallet, &s.wallet_address, min_accepted_wei).await {
        Ok(true) => {}
        Ok(false) => {
            return err(
                StatusCode::PAYMENT_REQUIRED,
                &format!(
                    "Payment details mismatch. Expected from={owner_wallet} to={} amount>={min_accepted_wei}",
                    s.wallet_address
                ),
            )
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
        if expired.is_empty() {
            continue;
        }
        let dht = state.dht.lock().await.as_ref().cloned();
        for entry in &expired {
            let _ = tokio::fs::remove_file(state.storage_dir.join(&entry.file_hash)).await;
            if let Some(ref d) = dht {
                unregister_in_dht(d, &entry.file_hash).await;
            }
        }
        println!("[CDN] Expiration cleanup removed {} files", expired.len());
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
    created_at: u64,
) {
    dht.register_shared_file(
        file_hash.to_string(),
        file_path.to_string_lossy().to_string(),
        file_name.to_string(),
        file_size,
        download_price_wei,
        cdn_wallet.to_string(),
    )
    .await;
    let peer_id = dht.get_peer_id().await.unwrap_or_default();
    let our_addrs = dht.get_listening_addresses().await;

    // Immutable file-metadata blob: write only if absent. The CDN is not
    // necessarily the original publisher, so we don't overwrite an existing
    // publisher-signed record.
    let key = format!("chiral_file_{file_hash}");
    let blob_present = matches!(dht.get_dht_value(key.clone()).await, Ok(Some(_)));
    if !blob_present {
        let metadata = json!({
            "hash": file_hash,
            "fileName": file_name,
            "fileSize": file_size,
            "protocol": "WebRTC",
            "createdAt": created_at,
            "walletAddress": cdn_wallet,
            "publisherSignature": "",
        });
        let _ = dht.put_dht_value(key, metadata.to_string()).await;
    }

    // The CDN is just another seeder in the provider-records model: publish
    // a per-seeder record + register as a Kademlia provider.
    let seeder_entry = crate::SeederInfo {
        peer_id: peer_id.clone(),
        price_wei: download_price_wei.to_string(),
        wallet_address: cdn_wallet.to_string(),
        multiaddrs: our_addrs,
        signature: String::new(),
    };
    if let Err(e) = crate::publish_seeder_entry(dht, file_hash, &seeder_entry).await {
        println!("[CDN] Provider publish failed for {}: {}", file_hash, e);
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

fn wei_to_chi(wei: u128) -> String {
    let chi = wei as f64 / 1e18;
    if chi < 0.000001 {
        format!("{chi:.18}").trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        format!("{chi:.6}")
    }
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
        assert_eq!(wei_to_chi(1_000_000_000_000_000), "0.001000");
    }

    #[test]
    fn wei_to_chi_display_zero() {
        assert_eq!(wei_to_chi(0), "0");
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
}
