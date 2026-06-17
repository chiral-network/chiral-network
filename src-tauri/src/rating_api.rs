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
    self, compute_reputation_for_wallet, RatingState, ReputationEvent, TransferOutcome,
    LOOKBACK_SECS,
};
use crate::reputation::{self, ReputationIssuerKeyRecord, ReputationVerdictPayload};

const MAX_REPUTATION_BATCH_WALLETS: usize = 100;

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
    #[serde(default)]
    issuer_wallet: Option<String>,
    #[serde(default)]
    verdict_signature: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishIssuerKeyRequest {
    verifying_key: String,
    owner_signature: String,
    #[serde(default)]
    updated_at: Option<u64>,
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
    addr.len() == 42 && addr.starts_with("0x") && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_reputation_wallet(wallet: &str) -> Result<String, String> {
    let wallet = wallet.trim();
    if !is_valid_wallet(wallet) {
        return Err("Invalid wallet address".to_string());
    }
    Ok(wallet.to_string())
}

fn validate_reputation_batch_wallets(wallets: &[String]) -> Result<Vec<String>, String> {
    if wallets.is_empty() {
        return Err("wallets must contain at least one wallet address".to_string());
    }
    if wallets.len() > MAX_REPUTATION_BATCH_WALLETS {
        return Err(format!(
            "wallets cannot contain more than {} addresses",
            MAX_REPUTATION_BATCH_WALLETS
        ));
    }

    wallets
        .iter()
        .enumerate()
        .map(|(index, wallet)| {
            validate_reputation_wallet(wallet).map_err(|err| format!("wallets[{index}]: {err}"))
        })
        .collect()
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

fn outcome_label(outcome: TransferOutcome) -> &'static str {
    match outcome {
        TransferOutcome::Completed => "completed",
        TransferOutcome::Failed => "failed",
    }
}

fn transfer_verdict_payload(
    req: &SubmitTransferRequest,
    downloader_wallet: &str,
    amount_wei: u128,
) -> ReputationVerdictPayload {
    ReputationVerdictPayload {
        transfer_id: req.transfer_id.trim().to_string(),
        seeder_wallet: req.seeder_wallet.trim().to_string(),
        downloader_wallet: downloader_wallet.trim().to_string(),
        file_hash: req.file_hash.trim().to_string(),
        amount_wei: amount_wei.to_string(),
        outcome: outcome_label(req.outcome).to_string(),
        tx_hash: req
            .tx_hash
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string),
    }
}

async fn verify_transfer_verdict(
    state: &RatingState,
    req: &SubmitTransferRequest,
    downloader_wallet: &str,
    amount_wei: u128,
) -> Result<(String, String), String> {
    let issuer_wallet = req
        .issuer_wallet
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "issuerWallet is required for reputation verdicts".to_string())?;
    let issuer_wallet = reputation::normalize_wallet(issuer_wallet)?;
    if !issuer_wallet.eq_ignore_ascii_case(downloader_wallet) {
        return Err("issuerWallet must match the authenticated X-Owner wallet".to_string());
    }

    let verdict_signature = req
        .verdict_signature
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "verdictSignature is required for reputation verdicts".to_string())?;

    let issuer_record = state
        .fetch_issuer_key(&issuer_wallet)
        .await?
        .ok_or_else(|| format!("No reputation issuer key published for {issuer_wallet}"))?;
    let verdict = transfer_verdict_payload(req, downloader_wallet, amount_wei);
    reputation::verify_reputation_verdict_for_wallet(
        &issuer_record,
        &issuer_wallet,
        &verdict,
        verdict_signature,
    )?;
    Ok((issuer_wallet, verdict_signature.to_string()))
}

/// POST /api/ratings/issuer-key — publish this wallet's Ed25519 issuer key.
async fn publish_issuer_key(
    Extension(state): Extension<Arc<RatingState>>,
    headers: HeaderMap,
    Json(req): Json<PublishIssuerKeyRequest>,
) -> Response {
    let issuer_wallet = match get_owner(&headers) {
        Some(v) => v.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };

    let updated_at = match req.updated_at {
        Some(value) => value,
        None => match rating_storage::now_secs() {
            Ok(value) => value,
            Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
        },
    };
    let record = ReputationIssuerKeyRecord {
        issuer_wallet,
        verifying_key: req.verifying_key,
        owner_signature: req.owner_signature,
        updated_at,
    };

    if let Err(err) = reputation::validate_issuer_key_record(&record) {
        return (StatusCode::BAD_REQUEST, err).into_response();
    }

    match state.publish_issuer_key(record.clone()).await {
        Ok(()) => (StatusCode::CREATED, Json(record)).into_response(),
        Err(err) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Failed to publish reputation issuer key: {err}"),
        )
            .into_response(),
    }
}

/// GET /api/ratings/issuer-key/:wallet — retrieve a wallet's issuer key record.
async fn get_issuer_key(
    Extension(state): Extension<Arc<RatingState>>,
    Path(wallet): Path<String>,
) -> Response {
    if let Err(err) = reputation::normalize_wallet(&wallet) {
        return (StatusCode::BAD_REQUEST, err).into_response();
    }
    match state.fetch_issuer_key(&wallet).await {
        Ok(Some(record)) => Json(record).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "reputation issuer key not found").into_response(),
        Err(err) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Failed to retrieve reputation issuer key: {err}"),
        )
            .into_response(),
    }
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
                return (
                    StatusCode::BAD_REQUEST,
                    "txHash required for paid transfers",
                )
                    .into_response()
            }
        };
        if let Err(err) =
            verify_payment_tx(tx_hash, &downloader_wallet, &req.seeder_wallet, amount_wei).await
        {
            return (
                StatusCode::BAD_REQUEST,
                format!("Payment verification failed: {}", err),
            )
                .into_response();
        }
    }

    if !state.issuer_key_store_configured() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "reputation issuer key store is not available on this server",
        )
            .into_response();
    }

    let (issuer_wallet, verdict_signature) =
        match verify_transfer_verdict(&state, &req, &downloader_wallet, amount_wei).await {
            Ok(v) => v,
            Err(err) => return (StatusCode::BAD_REQUEST, err).into_response(),
        };
    let tx_hash = req
        .tx_hash
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);

    let now = match rating_storage::now_secs() {
        Ok(value) => value,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    };
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
        existing.tx_hash = tx_hash.clone();
        existing.issuer_wallet = Some(issuer_wallet);
        existing.verdict_signature = Some(verdict_signature);
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
        tx_hash,
        rating_score: None,
        rating_comment: None,
        issuer_wallet: Some(issuer_wallet),
        verdict_signature: Some(verdict_signature),
        created_at: now,
        updated_at: now,
    };

    m.events.push(event.clone());
    drop(m);
    state.persist().await;
    (StatusCode::CREATED, Json(event)).into_response()
}

/// GET /api/ratings/:wallet — wallet reputation summary + events.
async fn get_reputation(
    Extension(state): Extension<Arc<RatingState>>,
    Path(wallet): Path<String>,
) -> Response {
    let wallet = match validate_reputation_wallet(&wallet) {
        Ok(wallet) => wallet,
        Err(err) => return (StatusCode::BAD_REQUEST, err).into_response(),
    };
    let now = match rating_storage::now_secs() {
        Ok(value) => value,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    };
    let m = state.manifest.read().await;
    let snapshot = compute_reputation_for_wallet(&m.events, &wallet, now);

    let mut events: Vec<ReputationEvent> = m
        .events
        .iter()
        .filter(|e| {
            e.seeder_wallet.eq_ignore_ascii_case(&wallet)
                || e.downloader_wallet.eq_ignore_ascii_case(&wallet)
        })
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
    let wallets = match validate_reputation_batch_wallets(&req.wallets) {
        Ok(wallets) => wallets,
        Err(err) => return (StatusCode::BAD_REQUEST, err).into_response(),
    };
    let now = match rating_storage::now_secs() {
        Ok(value) => value,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    };
    let m = state.manifest.read().await;
    let mut out = HashMap::new();

    for wallet in &wallets {
        let snap = compute_reputation_for_wallet(&m.events, wallet, now);
        out.insert(
            wallet.clone(),
            BatchEntry {
                elo: snap.elo,
                completed_count: snap.completed_count,
                failed_count: snap.failed_count,
                transaction_count: snap.transaction_count,
                total_earned_wei: snap.total_earned_wei,
            },
        );
    }

    Json(BatchResponse { reputations: out }).into_response()
}

pub fn rating_routes(state: Arc<RatingState>) -> Router {
    // POST /api/ratings/transfer is gated by the owner-proof middleware
    // (FM-A03 + FM-A12): the downloader's wallet must prove control of
    // the X-Owner address, otherwise anyone could submit a "Failed"
    // outcome against an arbitrary seeder and tank their Elo.
    let protected = Router::new()
        .route("/api/ratings/issuer-key", post(publish_issuer_key))
        .route("/api/ratings/transfer", post(submit_transfer))
        .layer(axum::middleware::from_fn(
            crate::auth::owner_proof_middleware,
        ));

    // Read-only routes don't need authentication — Elo scores are
    // public anyway.
    let public = Router::new()
        .route("/api/ratings/issuer-key/:wallet", get(get_issuer_key))
        .route("/api/ratings/batch", post(batch_reputation))
        .route("/api/ratings/:wallet", get(get_reputation));

    protected.merge(public).layer(Extension(state))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALLET_A: &str = "0x1111111111111111111111111111111111111111";
    const WALLET_B: &str = "0x2222222222222222222222222222222222222222";

    fn test_state() -> (tempfile::TempDir, Arc<RatingState>) {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(RatingState::new(dir.path().to_path_buf()));
        (dir, state)
    }

    #[test]
    fn reputation_wallet_validation_accepts_valid_wallet() {
        assert_eq!(
            validate_reputation_wallet(&format!("  {WALLET_A}  ")).unwrap(),
            WALLET_A
        );
    }

    #[test]
    fn reputation_wallet_validation_rejects_invalid_wallet() {
        let err =
            validate_reputation_wallet("0x123").expect_err("short wallet address must be rejected");

        assert_eq!(err, "Invalid wallet address");
    }

    #[test]
    fn reputation_batch_validation_accepts_valid_wallets() {
        let wallets = vec![WALLET_A.to_string(), WALLET_B.to_string()];

        let validated =
            validate_reputation_batch_wallets(&wallets).expect("valid batch should be accepted");

        assert_eq!(validated, wallets);
    }

    #[test]
    fn reputation_batch_validation_rejects_empty_batch() {
        let err =
            validate_reputation_batch_wallets(&[]).expect_err("empty batches should be rejected");

        assert!(err.contains("at least one wallet address"));
    }

    #[test]
    fn reputation_batch_validation_rejects_over_limit_batch() {
        let wallets = vec![WALLET_A.to_string(); MAX_REPUTATION_BATCH_WALLETS + 1];

        let err = validate_reputation_batch_wallets(&wallets)
            .expect_err("over-limit batches should be rejected");

        assert!(err.contains("cannot contain more than"));
        assert!(err.contains(&MAX_REPUTATION_BATCH_WALLETS.to_string()));
    }

    #[tokio::test]
    async fn get_reputation_accepts_valid_wallet() {
        let (_dir, state) = test_state();
        let response = get_reputation(Extension(state), Path(WALLET_A.to_string())).await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_reputation_rejects_invalid_wallet() {
        let (_dir, state) = test_state();
        let response = get_reputation(Extension(state), Path("not-a-wallet".to_string())).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn batch_reputation_accepts_valid_batch() {
        let (_dir, state) = test_state();
        let response = batch_reputation(
            Extension(state),
            Json(BatchRequest {
                wallets: vec![WALLET_A.to_string(), WALLET_B.to_string()],
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn batch_reputation_rejects_empty_batch() {
        let (_dir, state) = test_state();
        let response = batch_reputation(
            Extension(state),
            Json(BatchRequest {
                wallets: Vec::new(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn batch_reputation_rejects_over_limit_batch() {
        let (_dir, state) = test_state();
        let response = batch_reputation(
            Extension(state),
            Json(BatchRequest {
                wallets: vec![WALLET_A.to_string(); MAX_REPUTATION_BATCH_WALLETS + 1],
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
