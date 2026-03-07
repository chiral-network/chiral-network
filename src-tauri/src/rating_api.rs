use axum::{
    extract::{Extension, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::rating_storage::{
    self, compute_reputation_for_wallet, ReputationEvent, RatingState, TransferOutcome,
    LOOKBACK_SECS,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitTransferRequest {
    transfer_id: String,
    seeder_wallet: String,
    file_hash: String,
    outcome: TransferOutcome,
    #[serde(default)]
    amount_wei: String,
    #[serde(default)]
    tx_hash: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitRatingRequest {
    transfer_id: String,
    seeder_wallet: String,
    file_hash: String,
    score: u8,
    comment: Option<String>,
}

#[derive(Deserialize)]
struct BatchRequest {
    wallets: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletReputationResponse {
    wallet: String,
    elo: f64,
    base_elo: f64,
    completed_count: usize,
    failed_count: usize,
    transaction_count: usize,
    rating_count: usize,
    total_earned_wei: String,
    events: Vec<ReputationEvent>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchEntry {
    elo: f64,
    completed_count: usize,
    failed_count: usize,
    transaction_count: usize,
    rating_count: usize,
    total_earned_wei: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchResponse {
    reputations: HashMap<String, BatchEntry>,
}

fn get_owner(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-owner")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn is_valid_wallet(addr: &str) -> bool {
    addr.len() == 42
        && addr.starts_with("0x")
        && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_wei(value: &str) -> Result<u128, String> {
    if value.trim().is_empty() {
        return Ok(0);
    }
    value
        .trim()
        .parse::<u128>()
        .map_err(|_| "amountWei must be an integer wei string".to_string())
}

fn parse_hex_u128(hex: &str) -> Result<u128, String> {
    let value = hex.trim_start_matches("0x");
    u128::from_str_radix(value, 16).map_err(|e| format!("Invalid hex value: {}", e))
}

/// Validate paid transfer via on-chain transaction data.
async fn verify_payment_tx(
    tx_hash: &str,
    expected_from: &str,
    expected_to: &str,
    min_value_wei: u128,
) -> Result<(), String> {
    let rpc = crate::geth::rpc_endpoint();
    let client = reqwest::Client::new();

    let tx_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionByHash",
        "params": [tx_hash],
        "id": 1
    });
    let tx_resp = client
        .post(&rpc)
        .json(&tx_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to query tx: {}", e))?;
    let tx_json: serde_json::Value = tx_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse tx response: {}", e))?;
    let tx = tx_json.get("result").ok_or("Tx result missing")?;
    if tx.is_null() {
        return Err("Transaction not found".to_string());
    }

    let tx_from = tx.get("from").and_then(|v| v.as_str()).unwrap_or_default();
    let tx_to = tx.get("to").and_then(|v| v.as_str()).unwrap_or_default();
    if !tx_from.eq_ignore_ascii_case(expected_from) {
        return Err("Transaction sender does not match downloader wallet".to_string());
    }
    if !tx_to.eq_ignore_ascii_case(expected_to) {
        return Err("Transaction recipient does not match seeder wallet".to_string());
    }

    let tx_value_hex = tx
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or("Transaction value missing")?;
    let tx_value = parse_hex_u128(tx_value_hex)?;
    if tx_value < min_value_wei {
        return Err("Transaction value is below expected amount".to_string());
    }

    let receipt_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 2
    });
    let receipt_resp = client
        .post(&rpc)
        .json(&receipt_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to query receipt: {}", e))?;
    let receipt_json: serde_json::Value = receipt_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse receipt response: {}", e))?;
    let receipt = receipt_json.get("result").ok_or("Receipt result missing")?;
    if receipt.is_null() {
        return Err("Transaction not yet confirmed".to_string());
    }

    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err("Transaction failed on-chain".to_string());
    }

    Ok(())
}

/// POST /api/ratings/transfer — submit or update a transfer outcome event.
async fn submit_transfer(
    Extension(state): Extension<Arc<RatingState>>,
    headers: HeaderMap,
    Json(req): Json<SubmitTransferRequest>,
) -> Response {
    let downloader_wallet = match get_owner(&headers) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };

    if req.transfer_id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "transferId is required").into_response();
    }
    if req.file_hash.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "fileHash is required").into_response();
    }
    if req.seeder_wallet.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "seederWallet is required").into_response();
    }
    if downloader_wallet.eq_ignore_ascii_case(&req.seeder_wallet) {
        return (StatusCode::BAD_REQUEST, "Cannot score yourself").into_response();
    }
    if !is_valid_wallet(&req.seeder_wallet) || !is_valid_wallet(&downloader_wallet) {
        return (StatusCode::BAD_REQUEST, "Invalid wallet address").into_response();
    }

    let amount_wei = match parse_wei(&req.amount_wei) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };

    if amount_wei > 0 {
        let tx_hash = match req.tx_hash.as_deref() {
            Some(v) if !v.trim().is_empty() => v.trim(),
            _ => {
                return (StatusCode::BAD_REQUEST, "txHash required for paid transfers")
                    .into_response()
            }
        };
        if let Err(err) = verify_payment_tx(
            tx_hash,
            &downloader_wallet,
            &req.seeder_wallet,
            amount_wei,
        )
        .await
        {
            return (StatusCode::BAD_REQUEST, format!("Payment verification failed: {}", err))
                .into_response();
        }
    }

    let now = rating_storage::now_secs();
    let event_id = rating_storage::generate_event_id(
        &req.transfer_id,
        &req.seeder_wallet,
        &downloader_wallet,
        &req.file_hash,
    );

    let mut m = state.manifest.write().await;
    if let Some(existing) = m.events.iter_mut().find(|e| e.id == event_id) {
        existing.outcome = req.outcome;
        existing.amount_wei = amount_wei.to_string();
        existing.tx_hash = req.tx_hash.filter(|v| !v.trim().is_empty());
        existing.updated_at = now;
        let updated = existing.clone();
        drop(m);
        state.persist().await;
        return (StatusCode::OK, Json(updated)).into_response();
    }

    let event = ReputationEvent {
        id: event_id,
        transfer_id: req.transfer_id.trim().to_string(),
        seeder_wallet: req.seeder_wallet.trim().to_string(),
        downloader_wallet,
        file_hash: req.file_hash.trim().to_string(),
        amount_wei: amount_wei.to_string(),
        outcome: req.outcome,
        tx_hash: req.tx_hash.filter(|v| !v.trim().is_empty()),
        rating_score: None,
        rating_comment: None,
        created_at: now,
        updated_at: now,
    };

    m.events.push(event.clone());
    drop(m);
    state.persist().await;
    (StatusCode::CREATED, Json(event)).into_response()
}

/// POST /api/ratings — submit/update 1-5 user rating for a transfer event.
async fn submit_rating(
    Extension(state): Extension<Arc<RatingState>>,
    headers: HeaderMap,
    Json(req): Json<SubmitRatingRequest>,
) -> Response {
    let rater_wallet = match get_owner(&headers) {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };

    if req.score < 1 || req.score > 5 {
        return (StatusCode::BAD_REQUEST, "Score must be between 1 and 5").into_response();
    }
    if req.transfer_id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "transferId is required").into_response();
    }
    if req.seeder_wallet.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "seederWallet is required").into_response();
    }
    if req.file_hash.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "fileHash is required").into_response();
    }
    if rater_wallet.eq_ignore_ascii_case(&req.seeder_wallet) {
        return (StatusCode::BAD_REQUEST, "Cannot rate yourself").into_response();
    }
    if let Some(ref c) = req.comment {
        if c.len() > 500 {
            return (StatusCode::BAD_REQUEST, "Comment must be 500 characters or less")
                .into_response();
        }
    }

    let event_id = rating_storage::generate_event_id(
        &req.transfer_id,
        &req.seeder_wallet,
        &rater_wallet,
        &req.file_hash,
    );

    let now = rating_storage::now_secs();
    let mut m = state.manifest.write().await;
    let Some(existing) = m.events.iter_mut().find(|e| e.id == event_id) else {
        return (StatusCode::BAD_REQUEST, "Transfer event not found").into_response();
    };

    if existing.outcome != TransferOutcome::Completed {
        return (StatusCode::BAD_REQUEST, "Ratings require a completed transfer").into_response();
    }

    existing.rating_score = Some(req.score);
    existing.rating_comment = req.comment.filter(|c| !c.trim().is_empty());
    existing.updated_at = now;
    let updated = existing.clone();
    drop(m);
    state.persist().await;
    (StatusCode::OK, Json(updated)).into_response()
}

/// GET /api/ratings/:wallet — wallet reputation summary + events.
async fn get_reputation(
    Extension(state): Extension<Arc<RatingState>>,
    Path(wallet): Path<String>,
) -> Response {
    let now = rating_storage::now_secs();
    let m = state.manifest.read().await;
    let snapshot = compute_reputation_for_wallet(&m.events, &wallet, now);

    let mut events: Vec<ReputationEvent> = m
        .events
        .iter()
        .filter(|e| e.seeder_wallet.eq_ignore_ascii_case(&wallet))
        .filter(|e| now.saturating_sub(e.created_at) <= LOOKBACK_SECS)
        .cloned()
        .collect();
    events.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Json(WalletReputationResponse {
        wallet,
        elo: snapshot.elo,
        base_elo: snapshot.base_elo,
        completed_count: snapshot.completed_count,
        failed_count: snapshot.failed_count,
        transaction_count: snapshot.transaction_count,
        rating_count: snapshot.rating_count,
        total_earned_wei: snapshot.total_earned_wei,
        events,
    })
    .into_response()
}

/// POST /api/ratings/batch — batch reputation lookup.
async fn batch_reputation(
    Extension(state): Extension<Arc<RatingState>>,
    Json(req): Json<BatchRequest>,
) -> Response {
    let now = rating_storage::now_secs();
    let m = state.manifest.read().await;
    let mut out = HashMap::new();

    for wallet in &req.wallets {
        let snap = compute_reputation_for_wallet(&m.events, wallet, now);
        out.insert(
            wallet.clone(),
            BatchEntry {
                elo: snap.elo,
                completed_count: snap.completed_count,
                failed_count: snap.failed_count,
                transaction_count: snap.transaction_count,
                rating_count: snap.rating_count,
                total_earned_wei: snap.total_earned_wei,
            },
        );
    }

    Json(BatchResponse { reputations: out }).into_response()
}

pub fn rating_routes(state: Arc<RatingState>) -> Router {
    Router::new()
        .route("/api/ratings/transfer", post(submit_transfer))
        .route("/api/ratings/batch", post(batch_reputation))
        .route("/api/ratings", post(submit_rating))
        .route("/api/ratings/:wallet", get(get_reputation))
        .layer(Extension(state))
}
