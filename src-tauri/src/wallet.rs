//! Wallet module — owns all balance, transaction, and history logic.
//!
//! All RPC calls go through `crate::rpc_client` (connection-pooled).
//! Endpoint resolution uses `crate::geth::effective_rpc_endpoint()`.

use crate::rpc_client;
use once_cell::sync::Lazy;
use rlp::RlpStream;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tiny_keccak::{Hasher, Keccak};

// ============================================================================
// Constants
// ============================================================================

const BURN_ADDRESS: &str = "0x000000000000000000000000000000000000dEaD";

// ============================================================================
// Balance cache (5-second TTL, keyed by lowercase address)
// ============================================================================

static BALANCE_CACHE: Lazy<rpc_client::RpcCache> =
    Lazy::new(|| rpc_client::RpcCache::new(Duration::from_secs(5)));

/// Invalidate the cached balance for an address (call after sending tx).
pub async fn invalidate_balance_cache(address: &str) {
    BALANCE_CACHE.invalidate(&address.to_lowercase()).await;
}

// ============================================================================
// Structs
// ============================================================================

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletBalanceResult {
    pub balance: String,
    pub balance_wei: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransactionMeta {
    pub tx_hash: String,
    pub tx_type: String,
    pub description: String,
    pub file_name: Option<String>,
    pub file_hash: Option<String>,
    pub speed_tier: Option<String>,
    pub recipient_label: Option<String>,
    pub balance_before: Option<String>,
    pub balance_after: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub hash: String,
    pub from: String,
    pub to: String,
    pub value: String,
    pub value_wei: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub status: String,
    pub gas_used: u64,
    pub tx_type: String,
    pub description: String,
    pub file_name: Option<String>,
    pub file_hash: Option<String>,
    pub speed_tier: Option<String>,
    pub recipient_label: Option<String>,
    pub balance_before: Option<String>,
    pub balance_after: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTransactionResult {
    pub hash: String,
    pub status: String,
    pub balance_before: String,
    pub balance_after: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionHistoryResult {
    pub transactions: Vec<Transaction>,
}

pub struct PaymentResult {
    pub tx_hash: String,
    pub balance_before: String,
    pub balance_after: String,
}

// ============================================================================
// Core wallet operations
// ============================================================================

fn rpc_hex_str<'a>(value: &'a serde_json::Value, context: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{context} returned a non-string hex value: {value}"))
}

fn rpc_hex_field_str<'a>(
    value: &'a serde_json::Value,
    field: &str,
    context: &str,
) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{context} missing string `{field}` field"))
}

fn rpc_hex_u64(value: &serde_json::Value, context: &str) -> Result<u64, String> {
    rpc_client::hex_to_u64(rpc_hex_str(value, context)?).map_err(|e| format!("{context}: {e}"))
}

fn rpc_hex_u128(value: &serde_json::Value, context: &str) -> Result<u128, String> {
    rpc_client::hex_to_u128(rpc_hex_str(value, context)?).map_err(|e| format!("{context}: {e}"))
}

fn batch_hex_u64(result: &Result<serde_json::Value, String>, context: &str) -> Result<u64, String> {
    let value = result.as_ref().map_err(|e| format!("{context} RPC failed: {e}"))?;
    rpc_hex_u64(value, context)
}

fn batch_hex_u128(result: &Result<serde_json::Value, String>, context: &str) -> Result<u128, String> {
    let value = result.as_ref().map_err(|e| format!("{context} RPC failed: {e}"))?;
    rpc_hex_u128(value, context)
}

/// Get wallet balance (uses 5s cache).
pub async fn get_balance(endpoint: &str, address: &str) -> Result<WalletBalanceResult, String> {
    let cache_key = address.to_lowercase();

    // Check cache first
    if let Some(cached) = BALANCE_CACHE.get(&cache_key).await {
        let wei = rpc_hex_u128(&cached, "cached eth_getBalance")?;
        return Ok(WalletBalanceResult {
            balance: rpc_client::wei_to_chi_string(wei),
            balance_wei: wei.to_string(),
        });
    }

    let result = rpc_client::call(endpoint, "eth_getBalance", serde_json::json!([address, "latest"])).await?;
    let balance_wei = rpc_hex_u128(&result, "eth_getBalance")?;

    // Cache the raw hex result
    BALANCE_CACHE.set(cache_key, result).await;

    Ok(WalletBalanceResult {
        balance: rpc_client::wei_to_chi_string(balance_wei),
        balance_wei: balance_wei.to_string(),
    })
}

/// Send a signed transaction.
///
/// Takes an ordered list of RPC endpoints. The pre-tx batch
/// (`nonce + balance + gasPrice`) walks the list and uses the first
/// endpoint that responds; the actual `eth_sendRawTransaction`
/// broadcast then pins to that same endpoint. This preserves the
/// no-double-broadcast invariant — only one endpoint ever sees the
/// signed tx — while letting writes survive a downed primary (e.g.
/// the canonical relay's direct :8545 is loopback-only and clients
/// have to fall back to the :8080 proxy).
pub async fn send_transaction(
    endpoints: &[String],
    from_address: &str,
    to_address: &str,
    amount: &str,
    private_key: &str,
) -> Result<SendTransactionResult, String> {
    if endpoints.is_empty() {
        return Err("send_transaction: no RPC endpoints configured".to_string());
    }
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid private key hex: {}", e))?;
    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(&pk_bytes).map_err(|e| format!("Invalid private key: {}", e))?;
    let amount_wei = parse_chi_to_wei(amount)?;

    // Batch: nonce + balance + gasPrice in one request. Walk the
    // fallback list — first endpoint whose batch succeeds is the one
    // we'll also use for the broadcast.
    let mut last_err = String::new();
    let mut working_endpoint: Option<&str> = None;
    let mut results: Vec<Result<serde_json::Value, String>> = Vec::new();
    for ep in endpoints {
        let mut batch = rpc_client::batch();
        batch.add("eth_getTransactionCount", serde_json::json!([from_address, "pending"]));
        batch.add("eth_getBalance", serde_json::json!([from_address, "pending"]));
        batch.add("eth_gasPrice", serde_json::json!([]));
        match batch.execute(ep).await {
            Ok(r) => {
                results = r;
                working_endpoint = Some(ep.as_str());
                break;
            }
            Err(e) => {
                last_err = format!("{}: {}", ep, e);
            }
        }
    }
    let endpoint = working_endpoint.ok_or_else(|| {
        format!("all RPC endpoints failed pre-tx batch: {}", last_err)
    })?;
    let nonce_idx = 0;
    let bal_idx = 1;
    let gas_idx = 2;

    let nonce = batch_hex_u64(&results[nonce_idx], "eth_getTransactionCount")?;
    let balance_wei = batch_hex_u128(&results[bal_idx], "eth_getBalance")?;
    let gas_price = {
        let raw = batch_hex_u64(&results[gas_idx], "eth_gasPrice")?;
        if raw == 0 { 1_000_000_000u64 } else { raw }
    };

    let gas_limit: u64 = 21000;
    let chain_id: u64 = crate::geth::chain_id();
    let gas_cost = gas_price as u128 * gas_limit as u128;
    let total_cost = amount_wei.checked_add(gas_cost).ok_or("Amount overflow")?;

    let balance_before_chi = rpc_client::wei_to_chi_string(balance_wei);
    let balance_after_chi = rpc_client::wei_to_chi_string(balance_wei.saturating_sub(total_cost));

    if balance_wei < total_cost {
        return Err(format!(
            "Insufficient balance: have {:.6} CHI, need {:.6} CHI (amount) + {:.6} CHI (gas)",
            rpc_client::wei_to_chi(balance_wei),
            rpc_client::wei_to_chi(amount_wei),
            rpc_client::wei_to_chi(gas_cost),
        ));
    }

    let to_bytes = hex::decode(to_address.trim_start_matches("0x"))
        .map_err(|e| format!("Invalid to address: {}", e))?;

    // Sign transaction (EIP-155)
    let unsigned_tx = encode_unsigned_tx(nonce, gas_price as u128, gas_limit, &to_bytes, amount_wei, &[], chain_id);
    let tx_hash_bytes = keccak256(&unsigned_tx);
    let message = Message::from_digest_slice(&tx_hash_bytes).map_err(|e| format!("Failed to create message: {}", e))?;
    let (recovery_id, signature) = secp.sign_ecdsa_recoverable(&message, &secret_key).serialize_compact();
    let v = chain_id * 2 + 35 + recovery_id.to_i32() as u64;
    let r = &signature[0..32];
    let s = &signature[32..64];
    let signed_tx = encode_signed_tx(nonce, gas_price as u128, gas_limit, &to_bytes, amount_wei, &[], v, r, s);
    let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

    // Broadcast
    let tx_hash = broadcast_signed_tx(endpoint, &signed_tx_hex, &balance_before_chi, &balance_after_chi).await?;

    // Invalidate balance cache for sender
    invalidate_balance_cache(from_address).await;

    Ok(SendTransactionResult {
        hash: tx_hash,
        status: "pending".to_string(),
        balance_before: balance_before_chi,
        balance_after: balance_after_chi,
    })
}

/// Broadcast a signed transaction with retry logic for overdraft errors.
async fn broadcast_signed_tx(
    endpoint: &str,
    signed_tx_hex: &str,
    _balance_before: &str,
    _balance_after: &str,
) -> Result<String, String> {
    let send_result = rpc_client::call(endpoint, "eth_sendRawTransaction", serde_json::json!([signed_tx_hex])).await;

    match send_result {
        Ok(val) => {
            if let Some(hash) = val.as_str() {
                return Ok(hash.to_string());
            }
            // Compute hash from signed tx bytes
            Ok(compute_tx_hash(signed_tx_hex))
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("already known") {
                return Ok(compute_tx_hash(signed_tx_hex));
            }
            if msg.contains("overdraft") {
                // Retry up to 15 times with 2s delay
                for _ in 1..=15 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    match rpc_client::call(endpoint, "eth_sendRawTransaction", serde_json::json!([signed_tx_hex])).await {
                        Ok(val) => {
                            if let Some(hash) = val.as_str() {
                                return Ok(hash.to_string());
                            }
                            return Ok(compute_tx_hash(signed_tx_hex));
                        }
                        Err(retry_err) => {
                            let retry_msg = retry_err.to_string();
                            if retry_msg.contains("already known") {
                                return Ok(compute_tx_hash(signed_tx_hex));
                            }
                            if !retry_msg.contains("overdraft") {
                                return Err(format!("Transaction failed on retry: {}", retry_err));
                            }
                        }
                    }
                }
                // If we exhausted retries, still return the computed hash
                Ok(compute_tx_hash(signed_tx_hex))
            } else {
                Err(format!("Transaction failed: {}", e))
            }
        }
    }
}

fn compute_tx_hash(signed_tx_hex: &str) -> String {
    if let Ok(tx_bytes) = hex::decode(signed_tx_hex.trim_start_matches("0x")) {
        format!("0x{}", hex::encode(keccak256(&tx_bytes)))
    } else {
        "0x0".to_string()
    }
}

/// Get a transaction receipt.
pub async fn get_receipt(endpoint: &str, tx_hash: &str) -> Result<Option<serde_json::Value>, String> {
    let result = rpc_client::call(endpoint, "eth_getTransactionReceipt", serde_json::json!([tx_hash])).await?;
    if result.is_null() { Ok(None) } else { Ok(Some(result)) }
}

/// Dev faucet — sends 1 CHI to an address.
pub async fn request_faucet(address: &str) -> Result<SendTransactionResult, String> {
    let rpc = crate::geth::rpc_endpoint();
    let faucet = "0x0000000000000000000000000000000000001337";

    let nonce_result = rpc_client::call(&rpc, "eth_getTransactionCount", serde_json::json!([faucet, "latest"])).await?;
    let nonce = nonce_result.as_str().unwrap_or("0x0");

    let tx = serde_json::json!({
        "from": faucet, "to": address,
        "value": "0xde0b6b3a7640000", "gas": "0x5208",
        "gasPrice": "0x0", "nonce": nonce
    });

    let _ = rpc_client::call(&rpc, "personal_unlockAccount", serde_json::json!([faucet, "", 60])).await;
    let result = rpc_client::call(&rpc, "eth_sendTransaction", serde_json::json!([tx])).await
        .map_err(|e| format!("Faucet unavailable. Mine blocks to get CHI. Error: {}", e))?;

    Ok(SendTransactionResult {
        hash: result.as_str().unwrap_or("0x0").to_string(),
        status: "pending".to_string(),
        balance_before: String::new(),
        balance_after: String::new(),
    })
}

/// Public API for dht.rs to send payment transactions.
pub async fn send_payment(
    from: &str, to: &str, amount_chi: &str, private_key: &str,
) -> Result<PaymentResult, String> {
    // Canonical RPC fallback list — see wallet_rpc_endpoints doc.
    // File-payment txs have to be visible to the receiver's geth, which
    // means they can't land on an isolated local chain. The list also
    // lets the relay's :8080 proxy stand in when direct :8545 is
    // unreachable (e.g. canonical relay's loopback-only bind post the
    // 2026-05 lockdown).
    let endpoints = crate::geth::wallet_rpc_endpoints();
    let result = send_transaction(&endpoints, from, to, amount_chi, private_key).await?;
    Ok(PaymentResult {
        tx_hash: result.hash,
        balance_before: result.balance_before,
        balance_after: result.balance_after,
    })
}

// ============================================================================
// Transaction history
// ============================================================================

pub async fn get_transaction_history(
    endpoint: &str,
    address: &str,
    metadata: &HashMap<String, TransactionMeta>,
) -> Result<TransactionHistoryResult, String> {
    let result = rpc_client::call(endpoint, "eth_blockNumber", serde_json::json!([])).await?;
    let latest_block = rpc_hex_u64(&result, "eth_blockNumber")?;

    let mut transactions = Vec::new();
    let address_lower = address.to_lowercase();

    const MAX_BLOCKS: u64 = 3000;
    const BATCH_SIZE: u64 = 50;
    const MAX_BATCHES: u64 = 20;
    const MAX_DURATION: Duration = Duration::from_secs(4);

    let first_block = latest_block.saturating_sub(MAX_BLOCKS - 1);
    let mut cursor = latest_block;
    let started = std::time::Instant::now();
    let mut batches = 0u64;
    let http_client = rpc_client::client()?;

    'outer: loop {
        if batches >= MAX_BATCHES || started.elapsed() >= MAX_DURATION { break; }

        let batch_start = cursor.saturating_sub(BATCH_SIZE - 1).max(first_block);
        let payloads: Vec<serde_json::Value> = (batch_start..=cursor).rev().enumerate()
            .map(|(i, num)| serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [format!("0x{:x}", num), true],
                "id": i + 1
            }))
            .collect();

        let resp = http_client.post(endpoint).json(&payloads).send().await;
        batches += 1;

        if let Ok(response) = resp {
            if let Ok(results) = response.json::<Vec<serde_json::Value>>().await {
                for item in &results {
                    if let Some(block) = item.get("result") {
                        if let Some(txs) = block.get("transactions").and_then(|t| t.as_array()) {
                            if txs.is_empty() { continue; }
                            let block_ts = rpc_client::hex_to_u64(rpc_hex_field_str(
                                block,
                                "timestamp",
                                "eth_getBlockByNumber block",
                            )?)
                            .map_err(|e| format!("eth_getBlockByNumber block timestamp: {e}"))?;
                            let block_num = rpc_client::hex_to_u64(rpc_hex_field_str(
                                block,
                                "number",
                                "eth_getBlockByNumber block",
                            )?)
                            .map_err(|e| format!("eth_getBlockByNumber block number: {e}"))?;

                            for tx in txs {
                                let from = tx.get("from").and_then(|f| f.as_str()).unwrap_or("").to_lowercase();
                                let to = tx.get("to").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();

                                if from == address_lower || to == address_lower {
                                    let value_wei = rpc_client::hex_to_u128(rpc_hex_field_str(
                                        tx,
                                        "value",
                                        "eth_getBlockByNumber transaction",
                                    )?)
                                    .map_err(|e| format!("eth_getBlockByNumber transaction value: {e}"))?;
                                    let gas_used = rpc_client::hex_to_u64(rpc_hex_field_str(
                                        tx,
                                        "gas",
                                        "eth_getBlockByNumber transaction",
                                    )?)
                                    .map_err(|e| format!("eth_getBlockByNumber transaction gas: {e}"))?;
                                    let tx_hash = tx.get("hash").and_then(|h| h.as_str()).unwrap_or("");
                                    let tx_from = tx.get("from").and_then(|f| f.as_str()).unwrap_or("");
                                    let tx_to = tx.get("to").and_then(|t| t.as_str()).unwrap_or("");

                                    let (tt, desc, fname, fhash, stier, rlabel, bbefore, bafter) =
                                        classify_transaction(tx_hash, tx_from, tx_to, address, metadata);

                                    transactions.push(Transaction {
                                        hash: tx_hash.to_string(), from: tx_from.to_string(), to: tx_to.to_string(),
                                        value: rpc_client::wei_to_chi_string(value_wei), value_wei: value_wei.to_string(),
                                        block_number: block_num, timestamp: block_ts, status: "confirmed".to_string(),
                                        gas_used, tx_type: tt, description: desc,
                                        file_name: fname, file_hash: fhash, speed_tier: stier,
                                        recipient_label: rlabel, balance_before: bbefore, balance_after: bafter,
                                    });
                                    if transactions.len() >= 50 { break 'outer; }
                                }
                            }
                        }
                    }
                }
            }
        } else { break; }

        if batch_start <= first_block { break; }
        cursor = batch_start - 1;
    }

    transactions.sort_by(|a, b| b.block_number.cmp(&a.block_number));
    Ok(TransactionHistoryResult { transactions })
}

// ============================================================================
// Transaction metadata persistence
// ============================================================================

fn tx_metadata_path() -> PathBuf {
    crate::network::data_dir().join("tx_metadata.json")
}

pub fn load_tx_metadata() -> HashMap<String, TransactionMeta> {
    let path = tx_metadata_path();
    load_tx_metadata_from_path(&path)
}

fn load_tx_metadata_from_path(path: &Path) -> HashMap<String, TransactionMeta> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(e) => {
            eprintln!(
                "[Wallet] Failed to read transaction metadata {}: {}; starting with no transaction metadata",
                path.display(),
                e
            );
            return HashMap::new();
        }
    };

    match serde_json::from_slice(&data) {
        Ok(metadata) => metadata,
        Err(e) => {
            match quarantine_malformed_tx_metadata(path) {
                Ok(quarantine) => eprintln!(
                    "[Wallet] Malformed transaction metadata {} quarantined at {}: {}",
                    path.display(),
                    quarantine.display(),
                    e
                ),
                Err(quarantine_err) => eprintln!(
                    "[Wallet] Malformed transaction metadata {} could not be quarantined: {}; starting with no transaction metadata",
                    path.display(),
                    quarantine_err
                ),
            }
            HashMap::new()
        }
    }
}

fn quarantine_malformed_tx_metadata(path: &Path) -> Result<PathBuf, String> {
    let quarantine = malformed_tx_metadata_quarantine_path(path)?;
    std::fs::rename(path, &quarantine).map_err(|e| {
        format!(
            "rename {} to {}: {}",
            path.display(),
            quarantine.display(),
            e
        )
    })?;
    Ok(quarantine)
}

fn malformed_tx_metadata_quarantine_path(path: &Path) -> Result<PathBuf, String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("clock before UNIX_EPOCH: {e}"))?
        .as_secs();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tx_metadata.json");
    for attempt in 0..1000 {
        let suffix = if attempt == 0 {
            format!("malformed-{timestamp}")
        } else {
            format!("malformed-{timestamp}-{attempt}")
        };
        let candidate = path.with_file_name(format!("{file_name}.{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Ok(path.with_file_name(format!("{file_name}.malformed-{timestamp}-overflow")))
}

pub fn save_tx_metadata(metadata: &HashMap<String, TransactionMeta>) {
    let path = tx_metadata_path();
    save_tx_metadata_to_path(metadata, &path);
}

fn save_tx_metadata_to_path(metadata: &HashMap<String, TransactionMeta>, path: &Path) {
    match std::fs::read(path) {
        Ok(data) => {
            if serde_json::from_slice::<HashMap<String, TransactionMeta>>(&data).is_err() {
                eprintln!(
                    "[Wallet] Refusing to overwrite malformed transaction metadata at {}; fix or remove it manually",
                    path.display()
                );
                return;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!(
                "[Wallet] Refusing to overwrite unreadable transaction metadata at {}: {}; fix or remove it manually",
                path.display(),
                e
            );
            return;
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(metadata) {
        let _ = std::fs::write(path, json);
    }
}

pub fn record_meta(metadata: &mut HashMap<String, TransactionMeta>, meta: TransactionMeta) {
    metadata.insert(meta.tx_hash.clone(), meta);
    save_tx_metadata(metadata);
}

// ============================================================================
// Transaction classification
// ============================================================================

fn classify_transaction(
    tx_hash: &str, from: &str, to: &str, address: &str,
    metadata: &HashMap<String, TransactionMeta>,
) -> (String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) {
    let addr_lower = address.to_lowercase();
    let to_lower = to.to_lowercase();
    let from_lower = from.to_lowercase();
    let burn_lower = BURN_ADDRESS.to_lowercase();

    if let Some(meta) = metadata.get(tx_hash) {
        return (meta.tx_type.clone(), meta.description.clone(), meta.file_name.clone(),
            meta.file_hash.clone(), meta.speed_tier.clone(), meta.recipient_label.clone(),
            meta.balance_before.clone(), meta.balance_after.clone());
    }

    if to_lower == burn_lower && from_lower == addr_lower {
        return ("download_payment".to_string(), "Download payment".to_string(),
            None, None, None, Some("Burn Address (Download)".to_string()), None, None);
    }
    if from_lower == addr_lower {
        return ("send".to_string(), format!("Sent to {}", &to[..std::cmp::min(10, to.len())]),
            None, None, None, None, None, None);
    }
    if to_lower == addr_lower {
        return ("receive".to_string(), format!("Received from {}", &from[..std::cmp::min(10, from.len())]),
            None, None, None, None, None, None);
    }
    ("unknown".to_string(), "Transaction".to_string(), None, None, None, None, None, None)
}

// ============================================================================
// CHI / Wei conversion
// ============================================================================

pub fn parse_chi_to_wei(amount: &str) -> Result<u128, String> {
    let amount = amount.trim();
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 { return Err("Invalid amount format".to_string()); }

    let whole: u128 = if parts[0].is_empty() { 0 }
    else { parts[0].parse().map_err(|_| "Invalid amount".to_string())? };

    let frac_wei = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 18 {
            frac_str[..18].parse::<u128>().map_err(|_| "Invalid amount".to_string())?
        } else {
            format!("{:0<18}", frac_str).parse::<u128>().map_err(|_| "Invalid amount".to_string())?
        }
    } else { 0u128 };

    whole.checked_mul(1_000_000_000_000_000_000u128)
        .and_then(|w| w.checked_add(frac_wei))
        .ok_or("Amount overflow".to_string())
}

// ============================================================================
// Transaction encoding helpers (EIP-155)
// ============================================================================

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut output);
    output
}

fn encode_unsigned_tx(nonce: u64, gas_price: u128, gas_limit: u64, to: &[u8], value: u128, data: &[u8], chain_id: u64) -> Vec<u8> {
    let mut s = RlpStream::new_list(9);
    s.append(&nonce); s.append(&gas_price); s.append(&gas_limit);
    s.append(&to.to_vec()); s.append(&value); s.append(&data.to_vec());
    s.append(&chain_id); s.append(&0u8); s.append(&0u8);
    s.out().to_vec()
}

fn encode_signed_tx(nonce: u64, gas_price: u128, gas_limit: u64, to: &[u8], value: u128, data: &[u8], v: u64, r: &[u8], s: &[u8]) -> Vec<u8> {
    let mut stream = RlpStream::new_list(9);
    stream.append(&nonce); stream.append(&gas_price); stream.append(&gas_limit);
    stream.append(&to.to_vec()); stream.append(&value); stream.append(&data.to_vec());
    stream.append(&v);
    rlp_append_bytes_as_uint(&mut stream, r);
    rlp_append_bytes_as_uint(&mut stream, s);
    stream.out().to_vec()
}

fn rlp_append_bytes_as_uint(stream: &mut RlpStream, bytes: &[u8]) {
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    let stripped = &bytes[first_nonzero..];
    if stripped.is_empty() { stream.append(&0u8); }
    else { stream.append(&stripped.to_vec()); }
}

// ============================================================================
// ECDSA signing & verification (for signed DHT records)
// ============================================================================

/// Sign arbitrary data with a wallet private key.
/// Returns a hex-encoded recoverable signature (65 bytes: r[32] + s[32] + v[1]).
pub fn sign_message(private_key_hex: &str, data: &[u8]) -> Result<String, String> {
    let pk_hex = private_key_hex.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid key: {}", e))?;
    let secp = Secp256k1::new();
    let secret = SecretKey::from_slice(&pk_bytes).map_err(|e| format!("Invalid key: {}", e))?;

    // Ethereum personal_sign style: keccak256("\x19Ethereum Signed Message:\n" + len + data)
    let prefix = format!("\x19Ethereum Signed Message:\n{}", data.len());
    let mut prefixed = prefix.as_bytes().to_vec();
    prefixed.extend_from_slice(data);
    let hash = keccak256(&prefixed);

    let message = Message::from_digest_slice(&hash).map_err(|e| format!("Hash error: {}", e))?;
    let (recovery_id, signature) = secp.sign_ecdsa_recoverable(&message, &secret).serialize_compact();

    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature);
    sig_bytes[64] = recovery_id.to_i32() as u8 + 27; // Ethereum v value
    Ok(hex::encode(sig_bytes))
}

/// Verify a signature and recover the signer's Ethereum address.
/// Returns the lowercase 0x-prefixed address if valid, or an error.
pub fn recover_signer(data: &[u8], signature_hex: &str) -> Result<String, String> {
    let sig_bytes = hex::decode(signature_hex.trim_start_matches("0x"))
        .map_err(|e| format!("Invalid signature hex: {}", e))?;
    if sig_bytes.len() != 65 {
        return Err(format!("Signature must be 65 bytes, got {}", sig_bytes.len()));
    }

    let prefix = format!("\x19Ethereum Signed Message:\n{}", data.len());
    let mut prefixed = prefix.as_bytes().to_vec();
    prefixed.extend_from_slice(data);
    let hash = keccak256(&prefixed);

    // EIP-2 low-s enforcement. secp256k1 signatures are malleable —
    // for any valid `(r, s)`, `(r, n - s)` recovers the same pubkey,
    // producing two valid signatures over the same payload. Reject
    // high-s so signature hex is unique per (key, message) and any
    // future caller using the hex as a dedup key is sound.
    // n / 2 = 0x7FFFFFFF FFFFFFFF FFFFFFFF FFFFFFFF 5D576E73 57A4501D DFE92F46 681B20A0
    const SECP256K1_HALF_N: [u8; 32] = [
        0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE,
        0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B,
        0xBF, 0xD2, 0x5E, 0x8C, 0xD0, 0x36, 0x41, 0x40,
    ];
    {
        let s_bytes: &[u8] = &sig_bytes[32..64];
        if s_bytes > &SECP256K1_HALF_N[..] {
            return Err("Signature has non-canonical (high-s) form".to_string());
        }
    }
    let recovery_id = secp256k1::ecdsa::RecoveryId::from_i32((sig_bytes[64] as i32) - 27)
        .map_err(|_| "Invalid recovery ID")?;
    let recoverable = secp256k1::ecdsa::RecoverableSignature::from_compact(&sig_bytes[..64], recovery_id)
        .map_err(|e| format!("Invalid signature: {}", e))?;

    let secp = Secp256k1::new();
    let message = Message::from_digest_slice(&hash).map_err(|e| format!("Hash error: {}", e))?;
    let pubkey = secp.recover_ecdsa(&message, &recoverable)
        .map_err(|e| format!("Recovery failed: {}", e))?;

    // Derive Ethereum address: keccak256(uncompressed_pubkey[1..65])[12..32]
    let pubkey_bytes = pubkey.serialize_uncompressed();
    let addr_hash = keccak256(&pubkey_bytes[1..]);
    let address = format!("0x{}", hex::encode(&addr_hash[12..]));
    Ok(address.to_lowercase())
}

/// Verify that a signature was produced by the claimed wallet address.
pub fn verify_signature(data: &[u8], signature_hex: &str, expected_address: &str) -> bool {
    match recover_signer(data, signature_hex) {
        Ok(recovered) => recovered == expected_address.to_lowercase(),
        Err(_) => false,
    }
}

// ============================================================================
// On-chain payment verification
// ============================================================================

/// Wait for a tx to be mined and return whether it succeeded.
/// Polls more aggressively early, then backs off — most txs confirm in 1–2
/// blocks (~15s). Ceiling ~28s, after which we give up. Returns Ok(false) if
/// the tx never mines or mines with status != 0x1.
pub async fn wait_for_tx_mined(tx_hash: &str) -> Result<bool, String> {
    // 500ms × 4 (2s) + 1s × 6 (6s) + 2s × 10 (20s) + 5s × 12 (60s)
    // ≈ 88s ceiling. The old 28s schedule was too tight on freshnet —
    // blocks sometimes take 15-60s to propagate from the miner to a
    // remote relay's RPC node, and the user's CDN upload would fail
    // with "Payment not confirmed in time" on a tx that DID land
    // moments later, forcing a re-payment because the frontend then
    // sent a fresh send_transaction. Tighter early polling (sub-second)
    // catches the common fast case; the longer tail catches slow
    // propagation without forcing the user to pay twice.
    const DELAYS_MS: &[u64] = &[
        500, 500, 500, 500,
        1000, 1000, 1000, 1000, 1000, 1000,
        2000, 2000, 2000, 2000, 2000, 2000, 2000, 2000, 2000, 2000,
        5000, 5000, 5000, 5000, 5000, 5000, 5000, 5000, 5000, 5000, 5000, 5000,
    ];
    // Use the canonical-RPC fallback list (direct + relay /api/chain/rpc
    // proxy), not effective_rpc_endpoint(). Payment verification should
    // always read from the canonical chain anyway — and a CDN whose
    // local Geth is broken (port closed, crashed, isolated fork) used
    // to fail payment-mining waits with "Connection refused to
    // 127.0.0.1:8545" instead of falling through to the canonical RPC.
    let endpoints = crate::geth::wallet_rpc_endpoints();
    for (i, delay_ms) in DELAYS_MS.iter().enumerate() {
        let receipt = rpc_client::call_with_fallbacks(
            &endpoints,
            "eth_getTransactionReceipt",
            serde_json::json!([tx_hash]),
        ).await?;
        if !receipt.is_null() {
            let status = receipt.get("status").and_then(|s| s.as_str()).unwrap_or("0x0");
            return Ok(status == "0x1");
        }
        if i < DELAYS_MS.len() - 1 {
            tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;
        }
    }
    Ok(false)
}

/// Check tx details (from/to/value) against expectations. One RPC call, no polling.
/// Caller is responsible for ensuring the tx is already mined (use wait_for_tx_mined first).
pub async fn verify_tx_details(
    tx_hash: &str,
    expected_from: &str,
    expected_to: &str,
    expected_amount_wei: u128,
) -> Result<bool, String> {
    // Same canonical-RPC fallback rationale as wait_for_tx_mined above.
    let endpoints = crate::geth::wallet_rpc_endpoints();
    let tx = rpc_client::call_with_fallbacks(
        &endpoints,
        "eth_getTransactionByHash",
        serde_json::json!([tx_hash]),
    ).await?;
    if tx.is_null() {
        return Ok(false);
    }
    let tx_from = tx.get("from").and_then(|f| f.as_str()).unwrap_or("").to_lowercase();
    let tx_to = tx.get("to").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
    let tx_value = rpc_client::hex_to_u128(rpc_hex_field_str(
        &tx,
        "value",
        "eth_getTransactionByHash",
    )?)
    .map_err(|e| format!("eth_getTransactionByHash value: {e}"))?;
    // EIP-155 / chain-id check: a tx mined on the chiral chain must
    // carry the chiral chainId. Without this, a signed tx replayed from
    // any other EVM chain with the same address pair would pass.
    let expected_chain_id = crate::geth::chain_id() as u128;
    let tx_chain_id = tx
        .get("chainId")
        .map(|value| {
            let hex = rpc_hex_str(value, "eth_getTransactionByHash chainId")?;
            rpc_client::hex_to_u128(hex)
                .map_err(|e| format!("eth_getTransactionByHash chainId: {e}"))
        })
        .transpose()?;
    if let Some(observed) = tx_chain_id {
        if observed != expected_chain_id {
            return Ok(false);
        }
    }
    let from_match = expected_from.is_empty() || tx_from == expected_from.to_lowercase();
    let to_match = tx_to == expected_to.to_lowercase();
    let amount_match = tx_value >= expected_amount_wei;
    Ok(from_match && to_match && amount_match)
}

/// Verify a payment transaction on-chain.
/// Checks that the tx exists, is confirmed, sent the correct amount, and went to the correct recipient.
pub async fn verify_payment(
    tx_hash: &str,
    expected_from: &str,
    expected_to: &str,
    expected_amount_wei: u128,
) -> Result<bool, String> {
    if !wait_for_tx_mined(tx_hash).await? {
        return Ok(false);
    }
    verify_tx_details(tx_hash, expected_from, expected_to, expected_amount_wei).await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_tx_metadata_path(root: &Path) -> PathBuf {
        root.join("tx_metadata.json")
    }

    fn transaction_meta_fixture(tx_hash: &str) -> TransactionMeta {
        TransactionMeta {
            tx_hash: tx_hash.to_string(),
            tx_type: "send".to_string(),
            description: "Sent to 0xabc".to_string(),
            file_name: Some("report.pdf".to_string()),
            file_hash: Some("file-hash".to_string()),
            speed_tier: Some("fast".to_string()),
            recipient_label: Some("Alice".to_string()),
            balance_before: Some("10.0".to_string()),
            balance_after: Some("9.5".to_string()),
        }
    }

    fn metadata_fixture() -> HashMap<String, TransactionMeta> {
        let meta = transaction_meta_fixture("0xabc123");
        HashMap::from([(meta.tx_hash.clone(), meta)])
    }

    fn quarantined_tx_metadata_paths(root: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("tx_metadata.json.malformed-"))
            })
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn load_tx_metadata_missing_file_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_tx_metadata_path(dir.path());

        assert!(load_tx_metadata_from_path(&path).is_empty());
        assert!(!path.exists());
        assert!(quarantined_tx_metadata_paths(dir.path()).is_empty());
    }

    #[test]
    fn load_tx_metadata_reads_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_tx_metadata_path(dir.path());
        let metadata = metadata_fixture();
        fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();

        let loaded = load_tx_metadata_from_path(&path);

        assert_eq!(loaded.len(), 1);
        let meta = loaded.get("0xabc123").unwrap();
        assert_eq!(meta.tx_hash, "0xabc123");
        assert_eq!(meta.description, "Sent to 0xabc");
        assert_eq!(meta.file_name.as_deref(), Some("report.pdf"));
        assert!(path.exists());
        assert!(quarantined_tx_metadata_paths(dir.path()).is_empty());
    }

    #[test]
    fn load_tx_metadata_quarantines_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_tx_metadata_path(dir.path());
        let original = b"{not valid json";
        fs::write(&path, original).unwrap();

        assert!(load_tx_metadata_from_path(&path).is_empty());

        let quarantines = quarantined_tx_metadata_paths(dir.path());
        assert_eq!(quarantines.len(), 1);
        assert!(!path.exists());
        assert_eq!(fs::read(&quarantines[0]).unwrap(), &original[..]);
    }

    #[test]
    fn load_tx_metadata_quarantines_invalid_utf8_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_tx_metadata_path(dir.path());
        let original = vec![0xff, 0xfe, b'{', b'}'];
        fs::write(&path, &original).unwrap();

        assert!(load_tx_metadata_from_path(&path).is_empty());

        let quarantines = quarantined_tx_metadata_paths(dir.path());
        assert_eq!(quarantines.len(), 1);
        assert!(!path.exists());
        assert_eq!(fs::read(&quarantines[0]).unwrap(), original);
    }

    #[test]
    fn save_tx_metadata_refuses_to_overwrite_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_tx_metadata_path(dir.path());
        let original = b"{not valid json";
        fs::write(&path, original).unwrap();

        save_tx_metadata_to_path(&metadata_fixture(), &path);

        assert_eq!(fs::read(&path).unwrap(), &original[..]);
        assert!(quarantined_tx_metadata_paths(dir.path()).is_empty());
    }

    #[test]
    fn save_tx_metadata_refuses_to_overwrite_invalid_utf8_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_tx_metadata_path(dir.path());
        let original = vec![0xff, 0xfe, b'{', b'}'];
        fs::write(&path, &original).unwrap();

        save_tx_metadata_to_path(&metadata_fixture(), &path);

        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(quarantined_tx_metadata_paths(dir.path()).is_empty());
    }

    #[test] fn test_whole() { assert_eq!(parse_chi_to_wei("1").unwrap(), 1_000_000_000_000_000_000); }
    #[test] fn test_zero() { assert_eq!(parse_chi_to_wei("0").unwrap(), 0); }
    #[test] fn test_fraction() { assert_eq!(parse_chi_to_wei("0.001").unwrap(), 1_000_000_000_000_000); }
    #[test] fn test_large() { assert_eq!(parse_chi_to_wei("100").unwrap(), 100_000_000_000_000_000_000); }
    #[test] fn test_smallest_wei() { assert_eq!(parse_chi_to_wei("0.000000000000000001").unwrap(), 1); }
    #[test] fn test_overflow() { assert!(parse_chi_to_wei("1000000000000000000000").is_err()); }
    #[test] fn test_non_numeric() { assert!(parse_chi_to_wei("abc").is_err()); }
    #[test] fn test_multiple_dots() { assert!(parse_chi_to_wei("1.2.3").is_err()); }
    #[test] fn test_empty() { assert_eq!(parse_chi_to_wei("").unwrap(), 0); }
    #[test] fn test_whitespace() { assert_eq!(parse_chi_to_wei(" 1.5 ").unwrap(), 1_500_000_000_000_000_000); }

    // Signing tests
    const TEST_PRIVATE_KEY: &str = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn test_sign_and_recover() {
        let data = b"hello chiral network";
        let sig = sign_message(TEST_PRIVATE_KEY, data).unwrap();
        assert_eq!(sig.len(), 130); // 65 bytes = 130 hex chars

        let recovered = recover_signer(data, &sig).unwrap();
        assert!(recovered.starts_with("0x"));
        assert_eq!(recovered.len(), 42);
    }

    #[test]
    fn test_verify_correct_signer() {
        let data = b"file metadata payload";
        let sig = sign_message(TEST_PRIVATE_KEY, data).unwrap();
        let signer = recover_signer(data, &sig).unwrap();

        assert!(verify_signature(data, &sig, &signer));
    }

    #[test]
    fn test_verify_wrong_signer() {
        let data = b"file metadata payload";
        let sig = sign_message(TEST_PRIVATE_KEY, data).unwrap();

        assert!(!verify_signature(data, &sig, "0x0000000000000000000000000000000000000000"));
    }

    #[test]
    fn test_verify_tampered_data() {
        let data = b"original data";
        let sig = sign_message(TEST_PRIVATE_KEY, data).unwrap();
        let signer = recover_signer(data, &sig).unwrap();

        // Tampered data should not verify
        assert!(!verify_signature(b"tampered data", &sig, &signer));
    }

    #[test]
    fn test_sign_empty_data() {
        let sig = sign_message(TEST_PRIVATE_KEY, b"").unwrap();
        let recovered = recover_signer(b"", &sig).unwrap();
        assert!(recovered.starts_with("0x"));
    }
}
