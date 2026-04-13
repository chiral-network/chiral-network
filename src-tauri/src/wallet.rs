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
use std::path::PathBuf;
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

/// Get wallet balance (uses 5s cache).
pub async fn get_balance(endpoint: &str, address: &str) -> Result<WalletBalanceResult, String> {
    let cache_key = address.to_lowercase();

    // Check cache first
    if let Some(cached) = BALANCE_CACHE.get(&cache_key).await {
        let wei = rpc_client::hex_to_u128(cached.as_str().unwrap_or("0x0"));
        return Ok(WalletBalanceResult {
            balance: rpc_client::wei_to_chi_string(wei),
            balance_wei: wei.to_string(),
        });
    }

    let result = rpc_client::call(endpoint, "eth_getBalance", serde_json::json!([address, "latest"])).await?;
    let balance_hex = result.as_str().unwrap_or("0x0");
    let balance_wei = rpc_client::hex_to_u128(balance_hex);

    // Cache the raw hex result
    BALANCE_CACHE.set(cache_key, result).await;

    Ok(WalletBalanceResult {
        balance: rpc_client::wei_to_chi_string(balance_wei),
        balance_wei: balance_wei.to_string(),
    })
}

/// Send a signed transaction.
pub async fn send_transaction(
    endpoint: &str,
    from_address: &str,
    to_address: &str,
    amount: &str,
    private_key: &str,
) -> Result<SendTransactionResult, String> {
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid private key hex: {}", e))?;
    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(&pk_bytes).map_err(|e| format!("Invalid private key: {}", e))?;
    let amount_wei = parse_chi_to_wei(amount)?;

    // Batch: nonce + balance + gasPrice in one request
    let mut batch = rpc_client::batch();
    let nonce_idx = batch.add("eth_getTransactionCount", serde_json::json!([from_address, "pending"]));
    let bal_idx = batch.add("eth_getBalance", serde_json::json!([from_address, "pending"]));
    let gas_idx = batch.add("eth_gasPrice", serde_json::json!([]));
    let results = batch.execute(endpoint).await?;

    let nonce = rpc_client::hex_to_u64(results[nonce_idx].as_ref().map_err(|e| e.clone())?.as_str().unwrap_or("0x0"));
    let balance_wei = rpc_client::hex_to_u128(results[bal_idx].as_ref().map_err(|e| e.clone())?.as_str().unwrap_or("0x0"));
    let gas_price = {
        let raw = rpc_client::hex_to_u64(results[gas_idx].as_ref().map_err(|e| e.clone())?.as_str().unwrap_or("0x0"));
        if raw == 0 { 1_000_000_000u64 } else { raw }
    };

    let gas_limit: u64 = 21000;
    let chain_id: u64 = crate::geth::CHAIN_ID;
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
    let endpoint = crate::geth::effective_rpc_endpoint();
    let result = send_transaction(&endpoint, from, to, amount_chi, private_key).await?;
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
    let latest_block = rpc_client::hex_to_u64(result.as_str().unwrap_or("0x0"));

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

        let resp = rpc_client::client().post(endpoint).json(&payloads).send().await;
        batches += 1;

        if let Ok(response) = resp {
            if let Ok(results) = response.json::<Vec<serde_json::Value>>().await {
                for item in &results {
                    if let Some(block) = item.get("result") {
                        if let Some(txs) = block.get("transactions").and_then(|t| t.as_array()) {
                            if txs.is_empty() { continue; }
                            let block_ts = block.get("timestamp").and_then(|t| t.as_str())
                                .map(rpc_client::hex_to_u64).unwrap_or(0);
                            let block_num = block.get("number").and_then(|n| n.as_str())
                                .map(rpc_client::hex_to_u64).unwrap_or(0);

                            for tx in txs {
                                let from = tx.get("from").and_then(|f| f.as_str()).unwrap_or("").to_lowercase();
                                let to = tx.get("to").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();

                                if from == address_lower || to == address_lower {
                                    let value_hex = tx.get("value").and_then(|v| v.as_str()).unwrap_or("0x0");
                                    let value_wei = rpc_client::hex_to_u128(value_hex);
                                    let gas_hex = tx.get("gas").and_then(|g| g.as_str()).unwrap_or("0x0");
                                    let gas_used = rpc_client::hex_to_u64(gas_hex);
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
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
        .join("tx_metadata.json")
}

pub fn load_tx_metadata() -> HashMap<String, TransactionMeta> {
    let path = tx_metadata_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

pub fn save_tx_metadata(metadata: &HashMap<String, TransactionMeta>) {
    let path = tx_metadata_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(metadata) {
        let _ = std::fs::write(&path, json);
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

/// Verify a payment transaction on-chain.
/// Checks that the tx exists, is confirmed, sent the correct amount, and went to the correct recipient.
pub async fn verify_payment(
    tx_hash: &str,
    expected_from: &str,
    expected_to: &str,
    expected_amount_wei: u128,
) -> Result<bool, String> {
    let endpoint = crate::geth::effective_rpc_endpoint();

    // Wait for the transaction to be mined (up to 30 seconds)
    let mut receipt = serde_json::Value::Null;
    for attempt in 0..15 {
        receipt = rpc_client::call(
            &endpoint,
            "eth_getTransactionReceipt",
            serde_json::json!([tx_hash]),
        ).await?;

        if !receipt.is_null() {
            break;
        }

        if attempt < 14 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    if receipt.is_null() {
        return Ok(false); // Still not mined after 30s
    }

    // Check status (0x1 = success)
    let status = receipt.get("status").and_then(|s| s.as_str()).unwrap_or("0x0");
    if status != "0x1" {
        return Ok(false); // Transaction failed
    }

    // Get the full transaction to check from/to/value
    let tx = rpc_client::call(
        &endpoint,
        "eth_getTransactionByHash",
        serde_json::json!([tx_hash]),
    ).await?;

    if tx.is_null() {
        return Ok(false);
    }

    let tx_from = tx.get("from").and_then(|f| f.as_str()).unwrap_or("").to_lowercase();
    let tx_to = tx.get("to").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
    let tx_value = tx.get("value").and_then(|v| v.as_str()).map(rpc_client::hex_to_u128).unwrap_or(0);

    // Skip from-check if expected_from is empty (caller doesn't know the sender)
    let from_match = expected_from.is_empty() || tx_from == expected_from.to_lowercase();
    let to_match = tx_to == expected_to.to_lowercase();
    // Allow slight overpayment (gas rounding) but not underpayment
    let amount_match = tx_value >= expected_amount_wei;

    Ok(from_match && to_match && amount_match)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
