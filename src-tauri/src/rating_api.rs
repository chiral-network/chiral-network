//! Public feedback admission and canonicality revalidation. No relay signing wallet.
use crate::{
    chain_client::{ChainClient, ChainError},
    error::ApiError,
    offer_registry::validate_wallet,
    protocol,
    rating_storage::{RatingStore, Refusal, UpsertError},
    reputation::{self, FeedbackSubmission, ReputationEvent},
};
use axum::{
    body::Bytes,
    extract::{rejection::BytesRejection, DefaultBodyLimit, Path, State},
    http::{header::CONTENT_TYPE, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct RatingState {
    pub chain: Arc<ChainClient>,
    pub store: Arc<RatingStore>,
    gate: Semaphore,
}
impl RatingState {
    pub fn new(chain: Arc<ChainClient>, store: Arc<RatingStore>) -> Self {
        Self {
            chain,
            store,
            gate: Semaphore::new(4),
        }
    }
    async fn verify(&self, submission: &FeedbackSubmission) -> Result<String, ChainError> {
        let f = &submission.feedback.payload;
        let q = &submission.quote.payload;
        let data = protocol::opening_data(q).map_err(ChainError::Rejected)?;
        let payment = self
            .chain
            .verify_payment(
                &submission.raw_transaction,
                &f.consumer_wallet,
                &f.provider_wallet,
                Some(&q.funding_wei),
                &data,
            )
            .await?;
        if payment.tx_hash != f.tx_hash || payment.network != f.network {
            return Err(ChainError::Rejected(
                "funding hash or network does not match feedback".into(),
            ));
        }
        Ok(payment.value_wei)
    }
    async fn refresh(&self, wallet: &str) -> Result<Vec<ReputationEvent>, ApiError> {
        let mut events = self.store.events(wallet).map_err(ApiError::internal)?;
        for event in &mut events {
            // Stored evidence is reverified, including its provider and payer signatures.
            if reputation::validate_submission(
                &event.submission,
                self.chain.identity(),
                protocol::now().map_err(ApiError::internal)?,
            )
            .is_err()
            {
                event.eligible = false;
            } else {
                match self.verify(&event.submission).await {
                    Ok(amount) => event.eligible = amount == event.amount_wei,
                    Err(ChainError::Rejected(_) | ChainError::Pending(_)) => event.eligible = false,
                    Err(ChainError::Unavailable(_)) => {
                        return Err(ApiError::new(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "chain_unavailable",
                            "reputation canonicality cannot currently be verified",
                        ))
                    }
                }
            }
            self.store
                .set_eligible(&event.submission.feedback.payload.quote_id, event.eligible)
                .map_err(ApiError::internal)?;
        }
        Ok(events)
    }
}
pub fn router(state: Arc<RatingState>) -> Router {
    Router::new()
        .route("/api/ratings/feedback", post(submit))
        .route("/api/ratings/:wallet", get(snapshot))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(state)
}
/// A JSON media type as axum's `Json` accepts it: `application/json` or `application/*+json`.
fn json_content_type(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let essence = value.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match essence.split_once('/') {
        Some(("application", subtype)) => {
            subtype == "json" || subtype.rsplit_once('+').is_some_and(|(_, suffix)| suffix == "json")
        }
        _ => false,
    }
}
fn malformed_submission() -> ApiError {
    ApiError::bad_request(
        "invalid_feedback",
        "body is not exactly one valid FeedbackSubmission",
    )
}
/// Every content-type, syntax, trailing-content and schema failure is `invalid_feedback`.
/// `serde_json::from_slice` refuses trailing content, which axum's `Json` accepts.
/// The schema includes `score` 0..100 and a positive `revision`.
/// The body is never echoed.
fn decode_submission(headers: &HeaderMap, body: &[u8]) -> Result<FeedbackSubmission, ApiError> {
    if !json_content_type(headers) {
        return Err(ApiError::bad_request(
            "invalid_feedback",
            "feedback must be sent as application/json",
        ));
    }
    let submission: FeedbackSubmission =
        serde_json::from_slice(body).map_err(|_| malformed_submission())?;
    let payload = &submission.feedback.payload;
    if payload.score > 100 || payload.revision == 0 {
        return Err(malformed_submission());
    }
    Ok(submission)
}
/// The body limit answers HTTP 413 before decoding; any other unreadable body is
/// `invalid_feedback`. Decoding happens before a verification slot is taken, so a
/// malformed body takes no slot and makes no RPC request.
async fn submit(
    State(state): State<Arc<RatingState>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let body = match body {
        Ok(body) => body,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return rejection.into_response()
        }
        Err(_) => return malformed_submission().into_response(),
    };
    match decode_submission(&headers, &body) {
        Ok(submission) => admit(&state, submission).await.into_response(),
        Err(error) => error.into_response(),
    }
}
async fn admit(
    state: &RatingState,
    submission: FeedbackSubmission,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _permit = state.gate.try_acquire().map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "feedback_busy",
            "feedback verifier is busy",
        )
    })?;
    let now = protocol::now().map_err(ApiError::internal)?;
    reputation::validate_submission(&submission, state.chain.identity(), now)
        .map_err(|e| ApiError::bad_request("invalid_feedback", e))?;
    if now.saturating_sub(submission.feedback.payload.issued_at) > 300 {
        return Err(ApiError::bad_request(
            "expired_feedback",
            "sign feedback with a current timestamp and increasing revision",
        ));
    }
    let amount = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        state.verify(&submission),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "chain_timeout",
            "funding verification timed out",
        )
    })?
    .map_err(|e| match e {
        ChainError::Pending(_) => ApiError::new(
            StatusCode::CONFLICT,
            "payment_pending",
            "funding has not reached the settlement frontier",
        ),
        ChainError::Rejected(_) => ApiError::bad_request(
            "payment_rejected",
            "funding evidence is not an eligible canonical payment",
        ),
        ChainError::Unavailable(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "chain_unavailable",
            "funding cannot currently be verified",
        ),
    })?;
    state
        .store
        .upsert(&submission, &amount, now)
        .map_err(|error| match error {
            UpsertError::Refused(Refusal::Invalid) => {
                ApiError::bad_request("invalid_feedback", Refusal::Invalid.message())
            }
            UpsertError::Refused(refusal) => {
                ApiError::bad_request("feedback_conflict", refusal.message())
            }
            // A store fault is the catalog's failure, never a conflict, and its detail
            // (SQLite or file-system text) never enters the response.
            UpsertError::Fault(detail) => {
                eprintln!("feedback store fault: {detail}");
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the reputation catalog could not record this feedback; retry later",
                )
            }
        })?;
    Ok(Json(
        serde_json::json!({"accepted":true,"quote_id":submission.feedback.payload.quote_id,"revision":submission.feedback.payload.revision,"experimental_testnet":true}),
    ))
}
async fn snapshot(
    State(state): State<Arc<RatingState>>,
    Path(wallet): Path<String>,
) -> Result<Json<reputation::ReputationSnapshot>, ApiError> {
    validate_wallet(&wallet).map_err(|e| ApiError::bad_request("invalid_wallet", e))?;
    let _permit = state.gate.try_acquire().map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "feedback_busy",
            "feedback verifier is busy",
        )
    })?;
    let events = tokio::time::timeout(std::time::Duration::from_secs(30), state.refresh(&wallet))
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "chain_timeout",
                "reputation revalidation timed out",
            )
        })??;
    Ok(Json(reputation::compute_reputation(
        &events,
        state.chain.identity(),
        &wallet,
        protocol::now().map_err(ApiError::internal)?,
    )))
}
/// Revalidate incrementally even without HTTP readers; no stale vote is served on
/// RPC failure, and orphaned evidence remains available for future canonical recovery.
pub async fn reconcile(state: Arc<RatingState>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut cursor = String::new();
    loop {
        tokio::select! { _=shutdown.changed()=>{if *shutdown.borrow(){break;}}, _=tokio::time::sleep(std::time::Duration::from_secs(30))=>{} }
        if *shutdown.borrow() {
            break;
        }
        let Ok(providers) = state.store.providers(&cursor) else {
            continue;
        };
        if providers.is_empty() {
            cursor.clear();
            continue;
        }
        for wallet in providers {
            if *shutdown.borrow() {
                return;
            }
            if let Ok(_permit) = state.gate.try_acquire() {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    state.refresh(&wallet),
                )
                .await;
            }
            cursor = wallet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rating_storage::tests::{exclusive_lock, submission, PAYER};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU64, Ordering};
    use tower::ServiceExt;
    const FUNDING: &str = "1000000000000000000";
    fn state_at(
        directory: &std::path::Path,
        network: crate::protocol::NetworkIdentity,
        rpc_url: String,
    ) -> Arc<RatingState> {
        let config = crate::network::NetworkConfig {
            identity: network.clone(),
            rpc_url,
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        };
        Arc::new(RatingState::new(
            Arc::new(ChainClient::new(config, directory.join("chain")).unwrap()),
            Arc::new(RatingStore::open(&directory.join("ratings.sqlite"), &network).unwrap()),
        ))
    }
    fn state(
        directory: &std::path::Path,
        network: crate::protocol::NetworkIdentity,
    ) -> Arc<RatingState> {
        state_at(directory, network, "http://127.0.0.1:1".into())
    }
    /// Loopback eSpace RPC that counts every request and reports `receipt` canonical at
    /// height 9, below the safe frontier at 10.
    struct Rpc {
        requests: AtomicU64,
        receipt: std::sync::Mutex<Value>,
        observed_at: u64,
    }
    async fn rpc(
        State(rpc): State<Arc<Rpc>>,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        rpc.requests.fetch_add(1, Ordering::SeqCst);
        let result = match request["method"].as_str().unwrap() {
            "eth_chainId" => json!(format!("0x{:x}", crate::network::ESPACE_TESTNET_CHAIN_ID)),
            "eth_syncing" => json!(false),
            "eth_getBlockByNumber" => {
                let height = match request["params"][0].as_str().unwrap() {
                    "latest" => 12,
                    "safe" => 10,
                    tag => crate::rpc_client::hex_to_u64(tag).unwrap(),
                };
                let hash = match height {
                    0 => crate::network::ESPACE_TESTNET_GENESIS.to_owned(),
                    9 => format!("0x{}", "77".repeat(32)),
                    10 => format!("0x{}", "33".repeat(32)),
                    12 => format!("0x{}", "11".repeat(32)),
                    _ => format!("0x{}", "55".repeat(32)),
                };
                let timestamp = rpc.observed_at - (12 - height.min(12));
                json!({"number":format!("0x{height:x}"),"hash":hash,"timestamp":format!("0x{timestamp:x}")})
            }
            "eth_getTransactionReceipt" => {
                let receipt = rpc.receipt.lock().unwrap();
                if receipt["transactionHash"] == request["params"][0] {
                    receipt.clone()
                } else {
                    Value::Null
                }
            }
            other => panic!("unexpected RPC {other}"),
        };
        Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
    }
    async fn rpc_fixture() -> (Arc<Rpc>, String, tokio::task::JoinHandle<()>) {
        let fixture = Arc::new(Rpc {
            requests: AtomicU64::new(0),
            receipt: std::sync::Mutex::new(Value::Null),
            observed_at: protocol::now().unwrap(),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/", post(rpc))
            .with_state(fixture.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (fixture, url, task)
    }
    /// A correctly signed submission whose raw transaction is the exact opening payment
    /// of its quote, and the receipt that makes it canonical at the fixture's `safe`.
    fn paid_submission(now: u64) -> (FeedbackSubmission, Value) {
        let mut s = submission(now);
        let q = &s.quote.payload;
        let tx = crate::chain_client::sign_transaction(
            PAYER,
            crate::network::ESPACE_TESTNET_CHAIN_ID,
            0,
            &q.provider_wallet,
            q.funding_wei.parse().unwrap(),
            &protocol::opening_data(q).unwrap(),
            1,
            30_000,
        )
        .unwrap();
        let receipt = json!({"transactionHash":tx.tx_hash,"from":q.consumer_wallet,"to":q.provider_wallet,
            "type":"0x0","gasUsed":"0x5208","status":"0x1","blockHash":format!("0x{}","77".repeat(32)),"blockNumber":"0x9"});
        s.raw_transaction = tx.raw_transaction;
        s.feedback.payload.tx_hash = tx.tx_hash;
        s.feedback = protocol::sign(protocol::FEEDBACK_DOMAIN, s.feedback.payload, PAYER).unwrap();
        (s, receipt)
    }
    async fn post_feedback(
        state: &Arc<RatingState>,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> (StatusCode, Value) {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/ratings/feedback");
        if let Some(content_type) = content_type {
            request = request.header(CONTENT_TYPE, content_type);
        }
        let response = router(state.clone())
            .oneshot(request.body(axum::body::Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        // A plain-text framework rejection decodes as Null, so its code assertion fails.
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
    async fn post_json(state: &Arc<RatingState>, body: &Value) -> (StatusCode, Value) {
        post_feedback(state, Some("application/json"), serde_json::to_vec(body).unwrap()).await
    }
    #[tokio::test]
    async fn handler_rejects_forgery_without_creating_vote() {
        let directory = tempfile::tempdir().unwrap();
        let mut submission = submission(protocol::now().unwrap());
        let state = state(
            directory.path(),
            submission.feedback.payload.network.clone(),
        );
        let wallet = submission.feedback.payload.provider_wallet.clone();
        submission.feedback.payload.score = 1;
        let (status, _) = post_json(&state, &serde_json::to_value(&submission).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(state.store.events(&wallet).unwrap().is_empty());
    }
    #[tokio::test]
    async fn handler_rejects_invalid_raw_payment_without_fake_eligibility() {
        let directory = tempfile::tempdir().unwrap();
        let submission = submission(protocol::now().unwrap());
        let state = state(
            directory.path(),
            submission.feedback.payload.network.clone(),
        );
        let wallet = submission.feedback.payload.provider_wallet.clone();
        let (status, _) = post_json(&state, &serde_json::to_value(&submission).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(state.store.events(&wallet).unwrap().is_empty());
    }
    #[tokio::test]
    async fn malformed_feedback_is_invalid_feedback_without_rpc_or_vote() {
        // payment-gated-reputation requirement 2, task 2.1 (comparison item 6).
        let directory = tempfile::tempdir().unwrap();
        let (rpc, url, task) = rpc_fixture().await;
        let (paid, receipt) = paid_submission(protocol::now().unwrap());
        *rpc.receipt.lock().unwrap() = receipt;
        let wallet = paid.feedback.payload.provider_wallet.clone();
        let state = state_at(directory.path(), paid.feedback.payload.network.clone(), url);
        let valid = serde_json::to_value(&paid).unwrap();
        let with = |change: &dyn Fn(&mut Value)| {
            let mut body = valid.clone();
            change(&mut body);
            serde_json::to_vec(&body).unwrap()
        };
        let mut trailing = serde_json::to_vec(&valid).unwrap();
        trailing.extend_from_slice(b" x");
        let json = Some("application/json");
        let cases: Vec<(&str, Option<&str>, Vec<u8>)> = vec![
            ("unknown member", json, with(&|b| b["feedback"]["payload"]["comment"] = json!("slow"))),
            ("score 300", json, with(&|b| b["feedback"]["payload"]["score"] = json!(300))),
            ("score 50.5", json, with(&|b| b["feedback"]["payload"]["score"] = json!(50.5))),
            ("missing quote", json, with(&|b| {
                b.as_object_mut().unwrap().remove("quote");
            })),
            ("malformed JSON", json, b"{\"feedback\":".to_vec()),
            ("no Content-Type", None, serde_json::to_vec(&valid).unwrap()),
            ("non-JSON Content-Type", Some("text/plain"), serde_json::to_vec(&valid).unwrap()),
            ("trailing content", json, trailing),
        ];
        // Every case is checked, and every failing one reported.
        let mut failures = Vec::new();
        for (name, content_type, body) in cases {
            let before = rpc.requests.load(Ordering::SeqCst);
            let (status, error) = post_feedback(&state, content_type, body).await;
            let requests = rpc.requests.load(Ordering::SeqCst) - before;
            if status != StatusCode::BAD_REQUEST
                || error["error"]["code"] != "invalid_feedback"
                || requests != 0
                || !state.store.events(&wallet).unwrap().is_empty()
            {
                failures.push(format!("{name}: HTTP {status}, body {error}, {requests} RPC requests"));
            }
        }
        task.abort();
        assert!(failures.is_empty(), "{failures:#?}");
    }
    #[tokio::test]
    async fn malformed_feedback_takes_no_verification_slot() {
        // payment-gated-reputation requirement 2, task 2.1: decoding precedes the slot.
        let directory = tempfile::tempdir().unwrap();
        let (rpc, url, task) = rpc_fixture().await;
        let (paid, _) = paid_submission(protocol::now().unwrap());
        let state = state_at(directory.path(), paid.feedback.payload.network.clone(), url);
        let held = state.gate.try_acquire_many(4).unwrap();
        let mut unknown = serde_json::to_value(&paid).unwrap();
        unknown["feedback"]["payload"]["comment"] = json!("slow");
        let (status, error) = post_json(&state, &unknown).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"]["code"], "invalid_feedback", "{error}");
        // The slots are really held: a well-formed submission is busy.
        let (status, error) = post_json(&state, &serde_json::to_value(&paid).unwrap()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error["error"]["code"], "feedback_busy", "{error}");
        drop(held);
        assert_eq!(rpc.requests.load(Ordering::SeqCst), 0);
        task.abort();
    }
    #[tokio::test]
    async fn out_of_range_score_or_revision_takes_no_verification_slot() {
        // payment-gated-reputation requirement 2: `score` 0..100 and a positive `revision`
        // are part of the submission's shape, so `score` 101 and `revision` 0 are refused
        // as invalid_feedback even when every verification slot is held.
        let directory = tempfile::tempdir().unwrap();
        let (rpc, url, task) = rpc_fixture().await;
        let (paid, _) = paid_submission(protocol::now().unwrap());
        let state = state_at(directory.path(), paid.feedback.payload.network.clone(), url);
        let held = state.gate.try_acquire_many(4).unwrap();
        for (field, value) in [("score", json!(101)), ("revision", json!(0))] {
            let mut body = serde_json::to_value(&paid).unwrap();
            body["feedback"]["payload"][field] = value;
            let (status, error) = post_json(&state, &body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}: {error}");
            assert_eq!(
                error["error"]["code"], "invalid_feedback",
                "{field}: {error}"
            );
        }
        drop(held);
        assert_eq!(rpc.requests.load(Ordering::SeqCst), 0);
        task.abort();
    }
    #[tokio::test]
    async fn unreadable_feedback_body_is_invalid_feedback_json() {
        // payment-gated-reputation requirement 2: a body that cannot be read (broken
        // transfer framing) is invalid_feedback in the catalog's JSON error format, not
        // axum's plain-text 400, while the 64 KiB limit keeps its HTTP 413.
        let directory = tempfile::tempdir().unwrap();
        let (rpc, url, task) = rpc_fixture().await;
        let (paid, _) = paid_submission(protocol::now().unwrap());
        let wallet = paid.feedback.payload.provider_wallet.clone();
        let state = state_at(directory.path(), paid.feedback.payload.network.clone(), url);
        let send = |chunks: Vec<Result<Vec<u8>, std::io::Error>>| {
            let request = axum::http::Request::builder()
                .method("POST")
                .uri("/api/ratings/feedback")
                .header(CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from_stream(futures_util::stream::iter(
                    chunks,
                )))
                .unwrap();
            router(state.clone()).oneshot(request)
        };
        let response = send(vec![
            Ok(b"{\"feedback\":".to_vec()),
            Err(std::io::Error::other("connection reset by peer")),
        ])
        .await
        .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let error: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(
            error["error"]["code"],
            "invalid_feedback",
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert!(!error.to_string().contains("connection reset"), "{error}");
        // A streamed body over the limit, with no length declared, is still HTTP 413.
        let response = send((0..65).map(|_| Ok(vec![b' '; 1024])).collect())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(state.store.events(&wallet).unwrap().is_empty());
        assert_eq!(rpc.requests.load(Ordering::SeqCst), 0);
        task.abort();
    }
    #[tokio::test]
    async fn oversized_feedback_body_is_413_before_decoding() {
        // payment-gated-reputation requirement 2, task 2.1: the 64 KiB limit stays HTTP 413.
        let directory = tempfile::tempdir().unwrap();
        let (rpc, url, task) = rpc_fixture().await;
        let (paid, _) = paid_submission(protocol::now().unwrap());
        let wallet = paid.feedback.payload.provider_wallet.clone();
        let state = state_at(directory.path(), paid.feedback.payload.network.clone(), url);
        let mut at_limit = b"{\"feedback\":".to_vec();
        at_limit.resize(64 * 1024, b' ');
        let (status, error) = post_feedback(&state, Some("application/json"), at_limit).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"]["code"], "invalid_feedback", "{error}");
        let mut over = serde_json::to_vec(&paid).unwrap();
        over.resize(64 * 1024 + 1, b' ');
        let (status, _) = post_feedback(&state, Some("application/json"), over).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(state.store.events(&wallet).unwrap().is_empty());
        assert_eq!(rpc.requests.load(Ordering::SeqCst), 0);
        task.abort();
    }
    #[tokio::test]
    async fn store_fault_is_a_catalog_failure_and_conflicts_stay_conflicts() {
        // payment-gated-reputation requirements 6 and 7, task 2.2.
        let directory = tempfile::tempdir().unwrap();
        let (rpc, url, task) = rpc_fixture().await;
        let (paid, receipt) = paid_submission(protocol::now().unwrap());
        *rpc.receipt.lock().unwrap() = receipt;
        let wallet = paid.feedback.payload.provider_wallet.clone();
        let state = state_at(directory.path(), paid.feedback.payload.network.clone(), url);
        let body = serde_json::to_value(&paid).unwrap();
        // Another connection holds the write lock beyond the store's 5-second busy timeout.
        let holder = exclusive_lock(&directory.path().join("ratings.sqlite"));
        let (status, error) = post_json(&state, &body).await;
        drop(holder);
        assert!(rpc.requests.load(Ordering::SeqCst) > 0, "the payment was verified first");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
        assert_eq!(error["error"]["code"], "internal_error", "{error}");
        let text = error.to_string().to_ascii_lowercase();
        for database_text in ["locked", "sqlite", "database", "busy"] {
            assert!(!text.contains(database_text), "{text}");
        }
        assert!(state.store.events(&wallet).unwrap().is_empty());
        // The released store admits the same submission.
        let (status, accepted) = post_json(&state, &body).await;
        assert_eq!(status, StatusCode::OK, "{accepted}");
        assert_eq!(
            accepted,
            json!({"accepted":true,"quote_id":paid.feedback.payload.quote_id,"revision":1,"experimental_testnet":true})
        );
        // Equivocation at the stored revision is still a conflict, with fixed text.
        let mut equivocal = paid.clone();
        equivocal.feedback.payload.score = 0;
        equivocal.feedback =
            protocol::sign(protocol::FEEDBACK_DOMAIN, equivocal.feedback.payload, PAYER).unwrap();
        let (status, error) = post_json(&state, &serde_json::to_value(&equivocal).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"]["code"], "feedback_conflict", "{error}");
        assert_eq!(
            error["error"]["message"],
            crate::rating_storage::Refusal::Stale.message()
        );
        let events = state.store.events(&wallet).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].submission.feedback.payload.score, 100);
        assert_eq!(events[0].amount_wei, FUNDING);
        task.abort();
    }
}
