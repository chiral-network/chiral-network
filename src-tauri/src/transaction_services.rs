// transactions.rs - Transaction handling with enriched error responses
// This module provides Geth RPC interaction with developer-friendly error enrichment

use crate::ethereum::{NETWORK_CONFIG, HTTP_CLIENT, get_balance, get_block_number};
use rlp::Rlp;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct EnrichedApiError {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
    pub suggestion: String,
    pub documentation_url: String,
    pub geth_error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecodedTransaction {
    pub nonce: u64,
    pub gas_price: u128,
    pub gas_limit: u64,
    pub to: Option<String>,
    pub value: u128,
    pub data: Vec<u8>,
    pub v: u64,
    pub r: [u8; 32],
    pub s: [u8; 32],
    pub sender: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BroadcastResponse {
    pub transaction_hash: String,
    pub status: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionReceipt {
    pub transaction_hash: String,
    pub status: String,
    pub block_number: Option<u64>,
    pub block_hash: Option<String>,
    pub transaction_index: Option<u32>,
    pub gas_used: Option<u64>,
    pub effective_gas_price: Option<String>,
    pub confirmations: u64,
    pub from_address: String,
    pub to_address: Option<String>,
    pub value: String,
    pub nonce: u64,
    pub logs: Vec<serde_json::Value>,
    pub confirmation_time: Option<String>,
    pub submission_time: Option<String>,
    pub failure_reason: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionEstimate {
    pub gas_estimate: u64,
    pub gas_price_slow: String,
    pub gas_price_standard: String,
    pub gas_price_fast: String,
    pub total_cost_slow: String,
    pub total_cost_standard: String,
    pub total_cost_fast: String,
    pub sufficient_balance: bool,
    pub valid_recipient: bool,
    pub account_balance: String,
    pub recommended_nonce: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GasPrices {
    pub slow: String,
    pub standard: String,
    pub fast: String,
    pub slow_time: String,
    pub standard_time: String,
    pub fast_time: String,
    pub network_congestion: String,
    pub base_fee: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub network_id: u64,
    pub latest_block: u64,
    pub peer_count: u32,
    pub is_syncing: bool,
    pub sync_progress: Option<f64>,
    pub node_version: String,
    pub network_hashrate: String,
    pub difficulty: String,
    pub average_block_time: u32,
    pub mempool_size: u64,
    pub suggested_gas_price: String,
    pub chain_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionHistoryItem {
    pub transaction_hash: String,
    pub from_address: String,
    pub to_address: Option<String>,
    pub value: String,
    pub status: String,
    pub block_number: Option<u64>,
    pub gas_used: Option<u64>,
    pub gas_price: String,
    pub timestamp: u64,
    pub confirmations: u64,
    pub nonce: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NonceInfo {
    pub address: String,
    pub next_nonce: u64,
    pub pending_count: u64,
    pub confirmed_count: u64,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get the network configuration
pub fn get_network_config() -> &'static crate::ethereum::NetworkConfig {
    &NETWORK_CONFIG
}

/// Validate Ethereum address format
pub fn is_valid_ethereum_address(address: &str) -> bool {
    if !address.starts_with("0x") || address.len() != 42 {
        return false;
    }
    
    hex::decode(&address[2..]).is_ok()
}

/// Decode a signed transaction to extract details for error enrichment
pub fn decode_transaction(signed_tx_hex: &str) -> Result<DecodedTransaction, String> {
    let hex_str = if signed_tx_hex.starts_with("0x") {
        &signed_tx_hex[2..]
    } else {
        signed_tx_hex
    };

    let tx_bytes = hex::decode(hex_str).map_err(|e| format!("Invalid hex: {}", e))?;
    let rlp = Rlp::new(&tx_bytes);

    if rlp.item_count().map_err(|e| format!("RLP decode error: {}", e))? < 9 {
        return Err("Invalid transaction format".to_string());
    }

    let nonce: u64 = rlp.val_at(0).map_err(|e| format!("Invalid nonce: {}", e))?;
    let gas_price: u128 = rlp.val_at(1).map_err(|e| format!("Invalid gas price: {}", e))?;
    let gas_limit: u64 = rlp.val_at(2).map_err(|e| format!("Invalid gas limit: {}", e))?;
    
    let to_bytes: Vec<u8> = rlp.val_at(3).map_err(|e| format!("Invalid to address: {}", e))?;
    let to = if to_bytes.is_empty() {
        None
    } else {
        Some(format!("0x{}", hex::encode(to_bytes)))
    };

    let value: u128 = rlp.val_at(4).map_err(|e| format!("Invalid value: {}", e))?;
    let data: Vec<u8> = rlp.val_at(5).map_err(|e| format!("Invalid data: {}", e))?;
    let v: u64 = rlp.val_at(6).map_err(|e| format!("Invalid v: {}", e))?;
    
    let r_bytes: Vec<u8> = rlp.val_at(7).map_err(|e| format!("Invalid r: {}", e))?;
    let s_bytes: Vec<u8> = rlp.val_at(8).map_err(|e| format!("Invalid s: {}", e))?;

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    
    if r_bytes.len() <= 32 {
        let start = 32 - r_bytes.len();
        r[start..].copy_from_slice(&r_bytes);
    }
    if s_bytes.len() <= 32 {
        let start = 32 - s_bytes.len();
        s[start..].copy_from_slice(&s_bytes);
    }

    // Recover sender address from signature
    let sender = recover_sender(&tx_bytes, v, &r, &s)?;

    Ok(DecodedTransaction {
        nonce,
        gas_price,
        gas_limit,
        to,
        value,
        data,
        v,
        r,
        s,
        sender,
    })
}

/// Recover sender address from transaction signature (simplified)
/// NOTE: This is a placeholder - in production, use proper ECDSA recovery
fn recover_sender(_tx_bytes: &[u8], _v: u64, _r: &[u8; 32], _s: &[u8; 32]) -> Result<String, String> {
    // TODO: Implement proper ECDSA recovery using secp256k1
    // For now, return placeholder - actual recovery would use the transaction hash
    // and signature to recover the public key, then derive the address
    Ok("0x0000000000000000000000000000000000000000".to_string())
}

// ============================================================================
// Error Enrichment
// ============================================================================

/// Enrich Geth error messages with actionable details and suggestions
pub async fn enrich_geth_error(geth_message: &str, signed_tx: &str) -> EnrichedApiError {
    match geth_message {
        msg if msg.contains("nonce too low") => {
            if let Ok(decoded_tx) = decode_transaction(signed_tx) {
                if let Ok(expected_nonce) = get_transaction_count(&decoded_tx.sender).await {
                    let difference = decoded_tx.nonce as i64 - expected_nonce as i64;
                    return EnrichedApiError {
                        code: "NONCE_TOO_LOW".to_string(),
                        message: "The transaction nonce is lower than the next valid nonce for the sender's account".to_string(),
                        details: json!({
                            "sender_address": decoded_tx.sender,
                            "submitted_nonce": decoded_tx.nonce,
                            "expected_nonce": expected_nonce,
                            "difference": difference
                        }),
                        suggestion: "Use the get_address_nonce function to get the correct nonce, then re-sign and resubmit the transaction".to_string(),
                        documentation_url: "https://docs.chiral-network.com/errors#nonce_too_low".to_string(),
                        geth_error: msg.to_string(),
                    };
                }
            }

            EnrichedApiError {
                code: "NONCE_TOO_LOW".to_string(),
                message: "Transaction nonce is too low".to_string(),
                details: json!({}),
                suggestion: "Get the current nonce and re-sign the transaction".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#nonce_too_low".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg if msg.contains("nonce too high") => {
            if let Ok(decoded_tx) = decode_transaction(signed_tx) {
                if let Ok(expected_nonce) = get_transaction_count(&decoded_tx.sender).await {
                    let gap_size = decoded_tx.nonce.saturating_sub(expected_nonce);
                    return EnrichedApiError {
                        code: "NONCE_TOO_HIGH".to_string(),
                        message: "The transaction nonce is higher than expected, indicating a gap in the transaction sequence".to_string(),
                        details: json!({
                            "sender_address": decoded_tx.sender,
                            "submitted_nonce": decoded_tx.nonce,
                            "expected_nonce": expected_nonce,
                            "gap_size": gap_size
                        }),
                        suggestion: "Check for pending transactions or use the get_address_nonce function to get the correct nonce".to_string(),
                        documentation_url: "https://docs.chiral-network.com/errors#nonce_too_high".to_string(),
                        geth_error: msg.to_string(),
                    };
                }
            }

            EnrichedApiError {
                code: "NONCE_TOO_HIGH".to_string(),
                message: "Transaction nonce is too high".to_string(),
                details: json!({}),
                suggestion: "Check for pending transactions and use the correct nonce".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#nonce_too_high".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg if msg.contains("insufficient funds") => {
            if let Ok(decoded_tx) = decode_transaction(signed_tx) {
                // Get the actual balance from ethereum.rs
                let balance_result = get_balance(&decoded_tx.sender).await;
                let balance_wei = match balance_result {
                    Ok(balance_str) => {
                        // Convert balance from ether string to wei
                        if let Ok(balance_ether) = balance_str.parse::<f64>() {
                            (balance_ether * 1e18) as u128
                        } else {
                            0u128
                        }
                    }
                    Err(_) => 0u128,
                };
                    
                let gas_cost = decoded_tx.gas_limit as u128 * decoded_tx.gas_price;
                let total_required = decoded_tx.value + gas_cost;
                let shortfall = total_required.saturating_sub(balance_wei);

                return EnrichedApiError {
                    code: "INSUFFICIENT_FUNDS".to_string(),
                    message: "Account balance is insufficient to cover transaction value and gas costs".to_string(),
                    details: json!({
                        "sender_address": decoded_tx.sender,
                        "account_balance": format!("{:.6} ETH", balance_wei as f64 / 1e18),
                        "transaction_value": format!("{:.6} ETH", decoded_tx.value as f64 / 1e18),
                        "gas_cost": format!("{:.6} ETH", gas_cost as f64 / 1e18),
                        "total_required": format!("{:.6} ETH", total_required as f64 / 1e18),
                        "shortfall": format!("{:.6} ETH", shortfall as f64 / 1e18)
                    }),
                    suggestion: "Either reduce the transaction amount or add more ETH to the sender's account".to_string(),
                    documentation_url: "https://docs.chiral-network.com/errors#insufficient_funds".to_string(),
                    geth_error: msg.to_string(),
                };
            }

            EnrichedApiError {
                code: "INSUFFICIENT_FUNDS".to_string(),
                message: "Insufficient funds for transaction".to_string(),
                details: json!({}),
                suggestion: "Add more funds to the sender's account".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#insufficient_funds".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg if msg.contains("transaction underpriced") || msg.contains("gas price too low") => {
            if let Ok(current_gas_price) = get_gas_price().await {
                let current_price_dec = u64::from_str_radix(&current_gas_price[2..], 16).unwrap_or(0);
                let suggested_price = (current_price_dec as f64 * 1.25) as u64;

                let mut details = json!({
                    "minimum_gas_price": current_gas_price,
                    "suggested_gas_price": format!("0x{:x}", suggested_price),
                    "price_increase_needed": "25%"
                });

                if let Ok(decoded_tx) = decode_transaction(signed_tx) {
                    details["submitted_gas_price"] = json!(decoded_tx.gas_price.to_string());
                }

                return EnrichedApiError {
                    code: "GAS_PRICE_TOO_LOW".to_string(),
                    message: "The transaction gas price is below the network minimum".to_string(),
                    details,
                    suggestion: "Use get_recommended_gas_prices to get current recommended gas prices, then re-sign with higher gas price".to_string(),
                    documentation_url: "https://docs.chiral-network.com/errors#gas_price_too_low".to_string(),
                    geth_error: msg.to_string(),
                };
            }

            EnrichedApiError {
                code: "GAS_PRICE_TOO_LOW".to_string(),
                message: "Gas price too low".to_string(),
                details: json!({}),
                suggestion: "Increase the gas price and retry".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#gas_price_too_low".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg if msg.contains("exceeds block gas limit") => {
            let mut details = json!({
                "max_allowed_gas": 30000000
            });

            if let Ok(decoded_tx) = decode_transaction(signed_tx) {
                details["submitted_gas_limit"] = json!(decoded_tx.gas_limit);
            }

            EnrichedApiError {
                code: "GAS_LIMIT_EXCEEDED".to_string(),
                message: "Transaction gas limit exceeds block gas limit".to_string(),
                details,
                suggestion: "Reduce the gas limit to be within the block gas limit".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#gas_limit_exceeded".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg if msg.contains("replacement transaction underpriced") => {
            EnrichedApiError {
                code: "REPLACEMENT_UNDERPRICED".to_string(),
                message: "Replacement transaction must have higher gas price".to_string(),
                details: json!({
                    "minimum_increase_percent": 20
                }),
                suggestion: "Increase gas price by at least 20% above the existing transaction".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#replacement_underpriced".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg if msg.contains("txpool is full") || msg.contains("pool is full") => {
            EnrichedApiError {
                code: "MEMPOOL_FULL".to_string(),
                message: "Network transaction pool is full".to_string(),
                details: json!({
                    "estimated_wait_time": "30-60 seconds"
                }),
                suggestion: "Wait and retry, or increase gas price for priority processing".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#mempool_full".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg if msg.contains("invalid transaction") || msg.contains("invalid signature") => {
            EnrichedApiError {
                code: "INVALID_TRANSACTION_FORMAT".to_string(),
                message: "Invalid transaction format or signature".to_string(),
                details: json!({
                    "validation_failure": "invalid signature or transaction format"
                }),
                suggestion: format!("Verify the transaction was signed with the correct private key and chain ID ({})", NETWORK_CONFIG.chain_id),
                documentation_url: "https://docs.chiral-network.com/errors#invalid_transaction_format".to_string(),
                geth_error: msg.to_string(),
            }
        },

        msg => {
            EnrichedApiError {
                code: "NETWORK_ERROR".to_string(),
                message: "Network or node error occurred".to_string(),
                details: json!({
                    "raw_error": msg
                }),
                suggestion: "Check network connection and node status, then retry".to_string(),
                documentation_url: "https://docs.chiral-network.com/errors#network_error".to_string(),
                geth_error: msg.to_string(),
            }
        }
    }
}

// ============================================================================
// Core Transaction Functions
// ============================================================================

/// Broadcast a signed transaction to the network with enriched error handling
pub async fn broadcast_raw_transaction(signed_tx: &str) -> Result<BroadcastResponse, EnrichedApiError> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [signed_tx],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| EnrichedApiError {
            code: "NODE_UNAVAILABLE".to_string(),
            message: "Cannot connect to Chiral node".to_string(),
            details: json!({
                "connection_error": e.to_string(),
                "endpoint": NETWORK_CONFIG.rpc_endpoint
            }),
            suggestion: "Ensure the Chiral node is running and accessible".to_string(),
            documentation_url: "https://docs.chiral-network.com/errors#node_unavailable".to_string(),
            geth_error: format!("Connection failed: {}", e),
        })?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| EnrichedApiError {
            code: "INVALID_RESPONSE".to_string(),
            message: "Invalid response from node".to_string(),
            details: json!({
                "parse_error": e.to_string()
            }),
            suggestion: "Check node status and configuration".to_string(),
            documentation_url: "https://docs.chiral-network.com/errors#invalid_response".to_string(),
            geth_error: format!("JSON parse error: {}", e),
        })?;

    if let Some(error) = json_response.get("error") {
        let geth_message = error["message"].as_str().unwrap_or("unknown error");
        let enriched_error = enrich_geth_error(geth_message, signed_tx).await;
        return Err(enriched_error);
    }

    let tx_hash = json_response["result"]
        .as_str()
        .ok_or_else(|| EnrichedApiError {
            code: "INVALID_RESPONSE".to_string(),
            message: "Missing transaction hash in response".to_string(),
            details: json!({}),
            suggestion: "Check node configuration and try again".to_string(),
            documentation_url: "https://docs.chiral-network.com/errors#invalid_response".to_string(),
            geth_error: "Missing transaction hash".to_string(),
        })?;

    Ok(BroadcastResponse {
        transaction_hash: tx_hash.to_string(),
        status: "submitted".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Get transaction receipt and status
pub async fn get_transaction_receipt(tx_hash: &str) -> Result<TransactionReceipt, String> {
    let receipt_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&receipt_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get receipt: {}", e))?;

    let receipt_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse receipt response: {}", e))?;

    if let Some(error) = receipt_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    if receipt_response["result"].is_null() {
        let tx_payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionByHash",
            "params": [tx_hash],
            "id": 1
        });

        let tx_response = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&tx_payload)
            .send()
            .await
            .map_err(|e| format!("Failed to get transaction: {}", e))?;

        let tx_json: serde_json::Value = tx_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse transaction response: {}", e))?;

        if tx_json["result"].is_null() {
            return Ok(TransactionReceipt {
                transaction_hash: tx_hash.to_string(),
                status: "not_found".to_string(),
                block_number: None,
                block_hash: None,
                transaction_index: None,
                gas_used: None,
                effective_gas_price: None,
                confirmations: 0,
                from_address: "".to_string(),
                to_address: None,
                value: "0".to_string(),
                nonce: 0,
                logs: vec![],
                confirmation_time: None,
                submission_time: None,
                failure_reason: None,
                error_message: None,
            });
        }

        let tx = &tx_json["result"];
        let from_address = tx["from"].as_str().unwrap_or("").to_string();
        let to_address = tx["to"].as_str().map(|s| s.to_string());
        let value = tx["value"].as_str().unwrap_or("0x0").to_string();
        let nonce = tx["nonce"].as_str()
            .and_then(|s| u64::from_str_radix(&s[2..], 16).ok())
            .unwrap_or(0);

        return Ok(TransactionReceipt {
            transaction_hash: tx_hash.to_string(),
            status: "pending".to_string(),
            block_number: None,
            block_hash: None,
            transaction_index: None,
            gas_used: None,
            effective_gas_price: None,
            confirmations: 0,
            from_address,
            to_address,
            value,
            nonce,
            logs: vec![],
            confirmation_time: None,
            submission_time: None,
            failure_reason: None,
            error_message: None,
        });
    }

    let receipt = &receipt_response["result"];
    let status = if receipt["status"].as_str() == Some("0x1") {
        "success".to_string()
    } else {
        "failed".to_string()
    };

    let block_number = receipt["blockNumber"].as_str()
        .and_then(|s| u64::from_str_radix(&s[2..], 16).ok());

    let confirmations = if let Some(block_num) = block_number {
        match get_block_number().await {
            Ok(latest) => latest.saturating_sub(block_num),
            Err(_) => 0,
        }
    } else {
        0
    };

    let tx_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionByHash",
        "params": [tx_hash],
        "id": 1
    });

    let tx_response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&tx_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get transaction: {}", e))?;

    let tx_json: serde_json::Value = tx_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse transaction response: {}", e))?;

    let tx = &tx_json["result"];

    Ok(TransactionReceipt {
        transaction_hash: tx_hash.to_string(),
        status: status.clone(),
        block_number,
        block_hash: receipt["blockHash"].as_str().map(|s| s.to_string()),
        transaction_index: receipt["transactionIndex"].as_str()
            .and_then(|s| u32::from_str_radix(&s[2..], 16).ok()),
        gas_used: receipt["gasUsed"].as_str()
            .and_then(|s| u64::from_str_radix(&s[2..], 16).ok()),
        effective_gas_price: receipt["effectiveGasPrice"].as_str().map(|s| s.to_string()),
        confirmations,
        from_address: tx["from"].as_str().unwrap_or("").to_string(),
        to_address: tx["to"].as_str().map(|s| s.to_string()),
        value: tx["value"].as_str().unwrap_or("0x0").to_string(),
        nonce: tx["nonce"].as_str()
            .and_then(|s| u64::from_str_radix(&s[2..], 16).ok())
            .unwrap_or(0),
        logs: receipt["logs"].as_array().unwrap_or(&vec![]).clone(),
        confirmation_time: None,
        submission_time: None,
        failure_reason: if status == "failed" { Some("execution reverted".to_string()) } else { None },
        error_message: None,
    })
}

/// Get the next valid nonce for an address
pub async fn get_transaction_count(address: &str) -> Result<u64, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "pending"],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get transaction count: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let nonce_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid transaction count response")?;

    let nonce = u64::from_str_radix(&nonce_hex[2..], 16)
        .map_err(|e| format!("Failed to parse nonce: {}", e))?;

    Ok(nonce)
}

/// Get detailed nonce information for an address
pub async fn get_address_nonce(address: &str) -> Result<NonceInfo, String> {
    let pending_nonce = get_transaction_count(address).await?;
    
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "latest"],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get confirmed nonce: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let confirmed_nonce = if let Some(nonce_hex) = json_response["result"].as_str() {
        u64::from_str_radix(&nonce_hex[2..], 16).unwrap_or(0)
    } else {
        0
    };

    Ok(NonceInfo {
        address: address.to_string(),
        next_nonce: pending_nonce,
        pending_count: pending_nonce.saturating_sub(confirmed_nonce),
        confirmed_count: confirmed_nonce,
    })
}

/// Estimate gas for a transaction
pub async fn estimate_gas(from: &str, to: &str, value: &str, data: Option<&str>) -> Result<u64, String> {
    let mut params = json!({
        "from": from,
        "to": to,
        "value": value
    });

    if let Some(data) = data {
        params["data"] = json!(data);
    }

    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_estimateGas",
        "params": [params],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to estimate gas: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("Gas estimation failed: {}", error));
    }

    let gas_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid gas estimate response")?;

    let gas = u64::from_str_radix(&gas_hex[2..], 16)
        .map_err(|e| format!("Failed to parse gas estimate: {}", e))?;

    Ok(gas)
}

/// Get current network gas price
pub async fn get_gas_price() -> Result<String, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get gas price: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let gas_price_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid gas price response")?;

    Ok(gas_price_hex.to_string())
}

/// Get recommended gas prices with timing estimates
pub async fn get_recommended_gas_prices() -> Result<GasPrices, String> {
    let base_price = get_gas_price().await?;
    let base_price_dec = u64::from_str_radix(&base_price[2..], 16)
        .map_err(|e| format!("Failed to parse base gas price: {}", e))?;

    let slow = base_price_dec;
    let standard = base_price_dec * 125 / 100;
    let fast = base_price_dec * 150 / 100;

    Ok(GasPrices {
        slow: format!("0x{:x}", slow),
        standard: format!("0x{:x}", standard),
        fast: format!("0x{:x}", fast),
        slow_time: "~2 minutes".to_string(),
        standard_time: "~1 minute".to_string(),
        fast_time: "~30 seconds".to_string(),
        network_congestion: "low".to_string(),
        base_fee: base_price,
    })
}


// Note: estimate_transaction, get_network_status, and get_transaction_history
// would require additional dependencies from ethereum.rs (get_balance, get_peer_count, etc.)
// If needed, these can be added or the functions can accept those values as parameters