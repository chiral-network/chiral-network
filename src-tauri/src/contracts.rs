//! Public inference control plane and durable settlement maintenance.
use crate::{
    app::ProviderState,
    auth::{self, AuthPayload},
    billing_store::{self, CredentialIssueRequest},
    chain_client::{ChainError, PaymentState, VerifiedPayment},
    error::ApiError,
    protocol::{
        self, Challenge, ChallengeRequest, FundingIntentRequest, OpenRequest, ProposeRequest,
        Quote, TopupRequest,
    },
};
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};

const MAX_CONTROL_BODY: usize = 96 * 1024;
static MAINTENANCE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CONTROL_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(64);

pub fn router(state: Arc<ProviderState>) -> Router {
    Router::new()
        .route("/v1/auth/challenge", post(control))
        .route("/v1/contracts/propose", post(control))
        .route("/v1/contracts/open", post(control))
        .route("/v1/contracts/:id", get(control))
        .route("/v1/contracts/:id/funding-intent", post(control))
        .route("/v1/contracts/:id/topup", post(control))
        .route("/v1/contracts/:id/receipt", get(control))
        .route("/v1/contracts/:id/funding", get(control))
        .route("/v1/contracts/:id/requests", get(control))
        .route("/v1/contracts/:id/requests/:request_id", get(control))
        .route("/v1/contracts/:id/credentials", get(control).post(control))
        .route(
            "/v1/contracts/:id/credentials/:credential_id",
            axum::routing::delete(control),
        )
        .route(
            "/v1/contracts/:id/credentials/:credential_id/rotate",
            post(control),
        )
        .with_state(state)
}

fn error(message: String) -> ApiError {
    let code = message.split(':').next().unwrap_or("internal_error");
    let status = match code {
        "unauthorized" | "invalid_wallet" => StatusCode::UNAUTHORIZED,
        "forbidden" | "wrong_network" | "wrong_provider" => StatusCode::FORBIDDEN,
        "not_found" => StatusCode::NOT_FOUND,
        "rate_limit" | "saturation" => StatusCode::TOO_MANY_REQUESTS,
        "funding_replay"
        | "funding_orphaned"
        | "funding_intent_required"
        | "account_frozen"
        | "conflict" => StatusCode::CONFLICT,
        "provider_draining" | "model_unqualified" => StatusCode::SERVICE_UNAVAILABLE,
        "invalid_amount" | "invalid_quote" | "invalid_payment" | "expired_quote"
        | "invalid_challenge" | "invalid_credential" | "invalid_fee" | "amount_overflow" => {
            StatusCode::BAD_REQUEST
        }
        _ => return ApiError::internal("Control operation could not be completed"),
    };
    ApiError::new(
        status,
        code.to_owned(),
        message.chars().take(256).collect::<String>(),
    )
}
fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|_| {
        ApiError::bad_request(
            "invalid_request",
            "Malformed JSON or unsupported request fields",
        )
    })
}
fn owner(
    state: &ProviderState,
    headers: &HeaderMap,
    method: &str,
    path: &str,
    body: &[u8],
    now: u64,
) -> Result<String, ApiError> {
    auth::authenticate_owner(
        &state.ledger,
        headers,
        &state.config.network.identity,
        &state.wallet,
        method,
        path,
        body,
        now,
    )
    .map_err(|_| {
        ApiError::unauthorized("A valid one-use owner proof for this exact request is required")
    })
}
fn authorize_read(
    state: &ProviderState,
    headers: &HeaderMap,
    id: &str,
    path: &str,
    body: &[u8],
    now: u64,
) -> Result<(), ApiError> {
    if headers.contains_key("x-chiral-owner") {
        let wallet = owner(state, headers, "GET", path, body, now)?;
        let account = state.ledger.account(id).map_err(error)?;
        if wallet != account.consumer_wallet {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden",
                "Account belongs to another wallet",
            ));
        }
    } else {
        let context = state
            .ledger
            .authenticate(auth::bearer(headers).map_err(error)?, now)
            .map_err(error)?;
        if context.account.contract_id != id || !context.scopes.iter().any(|s| s == "read") {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden",
                "Credential does not authorize this account read",
            ));
        }
    }
    Ok(())
}
fn authorize_owner(
    state: &ProviderState,
    headers: &HeaderMap,
    id: &str,
    method: &str,
    path: &str,
    body: &[u8],
    now: u64,
) -> Result<(), ApiError> {
    let wallet = owner(state, headers, method, path, body, now)?;
    if state.ledger.account(id).map_err(error)?.consumer_wallet != wallet {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Account belongs to another wallet",
        ));
    }
    Ok(())
}
fn accepting(state: &ProviderState) -> Result<(), ApiError> {
    if !state.accepting.load(Ordering::Acquire) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_draining",
            "Provider is not accepting new funding",
        ));
    }
    if !state.funding_ready.load(Ordering::Acquire) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "settlement_unavailable",
            "New funding is paused until canonical settlement verification recovers",
        ));
    }
    Ok(())
}
async fn control(
    State(state): State<Arc<ProviderState>>,
    request: Request,
) -> Result<Response, ApiError> {
    let _slot = CONTROL_SLOTS.try_acquire().map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit",
            "Control request concurrency limit reached",
        )
    })?;
    let (parts, body) = request.into_parts();
    let body = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        to_bytes(body, MAX_CONTROL_BODY),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            "Control request body deadline exceeded",
        )
    })?
    .map_err(|_| {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request_too_large",
            "Control request body exceeds the limit",
        )
    })?;
    let now = protocol::now().map_err(error)?;
    let method = parts.method.as_str();
    let path = parts.uri.path();
    let signed_path = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or(path);
    let headers = &parts.headers;
    state.ledger.bind_provider(&state.wallet).map_err(error)?;
    if path == "/v1/auth/challenge" {
        let request: ChallengeRequest = parse(&body)?;
        auth::validate_challenge_request(&request).map_err(error)?;
        let payload = AuthPayload {
            network: state.config.network.identity.clone(),
            provider_wallet: state.wallet.clone(),
            wallet: auth::canonical_wallet(&request.wallet).map_err(error)?,
            method: request.method,
            path: request.path,
            body_hash: request.body_hash.to_ascii_lowercase(),
            nonce: protocol::random_id(),
            expires_at: now.saturating_add(auth::CHALLENGE_TTL_SECS),
        };
        state
            .ledger
            .create_challenge(&payload, now)
            .map_err(error)?;
        let challenge = Challenge {
            nonce: payload.nonce.clone(),
            payload_hex: format!(
                "0x{}",
                hex::encode(auth::challenge_payload(&payload).map_err(error)?)
            ),
            expires_at: payload.expires_at,
        };
        return Ok(Json(challenge).into_response());
    }
    if path == "/v1/contracts/propose" {
        accepting(&state)?;
        let request: ProposeRequest = parse(&body)?;
        let offer = state.offers.get(&request.offer_id).ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "offer_not_found",
                "Offer is absent or expired",
            )
        })?;
        let offer = offer.payload;
        if offer.network != state.config.network.identity
            || offer.provider_wallet != state.wallet
            || offer.expires_at <= now
        {
            return Err(ApiError::bad_request(
                "invalid_offer",
                "Offer does not belong to this provider/network or has expired",
            ));
        }
        let funding = billing_store::money(&request.funding_wei).map_err(error)?;
        let free = billing_store::money(&offer.price.input_wei_per_1k).map_err(error)? == 0
            && billing_store::money(&offer.price.output_wei_per_1k).map_err(error)? == 0;
        if (free && funding != 0)
            || (!free
                && (funding == 0
                    || funding < billing_store::money(&offer.min_funding_wei).map_err(error)?))
        {
            return Err(ApiError::bad_request(
                "invalid_funding",
                "Funding does not match the offer minimum or free-session policy",
            ));
        }
        let quote = Quote {
            id: protocol::random_id(),
            network: offer.network,
            offer_id: request.offer_id,
            provider_wallet: state.wallet.clone(),
            consumer_wallet: auth::canonical_wallet(&request.consumer_wallet).map_err(error)?,
            model: offer.model,
            price: offer.price,
            funding_wei: funding.to_string(),
            platform_wallet: offer.platform_wallet,
            fee_bps: offer.fee_bps,
            max_concurrency: offer.max_concurrency,
            nonce: protocol::random_id(),
            issued_at: now,
            expires_at: now.saturating_add(300),
        };
        let signed =
            protocol::sign(protocol::QUOTE_DOMAIN, quote, &state.private_key).map_err(error)?;
        state.ledger.store_quote(&signed, now).map_err(error)?;
        return Ok(Json(signed).into_response());
    }
    if path == "/v1/contracts/open" {
        let request: OpenRequest = parse(&body)?;
        let wallet = owner(&state, headers, method, signed_path, &body, now)?;
        let quote = state.ledger.quote(&request.quote_id).map_err(error)?;
        if quote.payload.consumer_wallet != wallet {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden",
                "Quote belongs to another wallet",
            ));
        }
        state
            .ledger
            .check_open_funding(&quote.payload, request.raw_transaction.as_deref(), now)
            .map_err(error)?;
        // Existing-account recovery is independent of new funding availability.
        let existing = match state.ledger.account(&request.quote_id) {
            Ok(_) => true,
            Err(e) if e.starts_with("not_found:") => false,
            Err(e) => return Err(error(e)),
        };
        let payment = if existing {
            None
        } else {
            if quote.payload.funding_wei == "0" {
                accepting(&state)?;
                None
            } else {
                let raw = request.raw_transaction.as_deref().ok_or_else(|| {
                    ApiError::bad_request(
                        "invalid_payment",
                        "Original signed funding transaction bytes are required",
                    )
                })?;
                Some(
                    verify_funding(
                        &state,
                        &request.quote_id,
                        raw,
                        &wallet,
                        Some(&quote.payload.funding_wei),
                        &protocol::opening_data(&quote.payload).map_err(error)?,
                        now,
                    )
                    .await?,
                )
            }
        };
        return Ok(Json(
            state
                .ledger
                .open_account(&quote, payment.as_ref(), now)
                .map_err(error)?,
        )
        .into_response());
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let id = segments
        .get(2)
        .copied()
        .ok_or_else(|| ApiError::bad_request("invalid_path", "Missing contract identifier"))?;
    if id.len() != 66 || !id.starts_with("0x") || !id[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request(
            "invalid_contract_id",
            "Invalid contract identifier",
        ));
    }
    if method == "POST" && segments.get(3).copied() == Some("funding-intent") {
        let request: FundingIntentRequest = parse(&body)?;
        let wallet = owner(&state, headers, method, signed_path, &body, now)?;
        let quote = state.ledger.quote(id).map_err(error)?;
        if quote.payload.consumer_wallet != wallet {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden",
                "Quote belongs to another wallet",
            ));
        }
        if let Some(ack) = state
            .ledger
            .funding_intent(&quote.payload, &request.raw_transaction)
            .map_err(error)?
        {
            return Ok(Json(ack).into_response());
        }
        accepting(&state)?;
        if state.qualified_model(&quote.payload.model).is_none() {
            return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "model_unqualified", "Quoted model is not currently qualified for new funding"));
        }
        let envelope=state.chain.admit_payment_envelope(&request.raw_transaction,&wallet,&state.wallet,&quote.payload.funding_wei,&protocol::opening_data(&quote.payload).map_err(error)?).await
            .map_err(|failure|match failure {
                ChainError::Rejected(_)=>ApiError::bad_request("invalid_payment","Funding intent must match the agreement, use the current sender nonce, and have balance for value plus signed gas"),
                ChainError::Pending(_)=>ApiError::new(StatusCode::CONFLICT,"funding_pending","Sender state changed during admission; retry the identical signed intent"),
                ChainError::Unavailable(_)=>ApiError::new(StatusCode::SERVICE_UNAVAILABLE,"settlement_unavailable","Current sender spendability cannot be established; no acknowledgement issued"),
            })?;
        let execution_model = state.qualified_model(&quote.payload.model).ok_or_else(||
            ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "model_unqualified", "Quoted model is no longer qualified for new funding")
        )?;
        let ack = state
            .ledger
            .register_funding_intent(
                &quote,
                &wallet,
                &request.raw_transaction,
                &envelope.tx_hash,
                envelope.nonce,
                &state.private_key,
                protocol::now,
                state.accepting.load(Ordering::Acquire)
                    && state.funding_ready.load(Ordering::Acquire),
                execution_model,
            )
            .map_err(error)?;
        return Ok(Json(ack).into_response());
    }
    if method == "GET" {
        if !body.is_empty() {
            return Err(ApiError::bad_request(
                "invalid_request",
                "GET requests must have an empty body",
            ));
        }
        authorize_read(&state, headers, id, signed_path, &body, now)?;
        return match segments.get(3).copied() {
            None => Ok(Json(state.ledger.account(id).map_err(error)?).into_response()),
            Some("receipt") => Ok(Json(
                state
                    .ledger
                    .receipt(id, &state.private_key, now)
                    .map_err(error)?,
            )
            .into_response()),
            Some("funding") => {
                Ok(Json(state.ledger.funding_history(id).map_err(error)?).into_response())
            }
            Some("credentials") => {
                Ok(Json(state.ledger.credentials(id).map_err(error)?).into_response())
            }
            Some("requests") if segments.len() == 5 => Ok(Json(
                state.ledger.request_view(id, segments[4]).map_err(error)?,
            )
            .into_response()),
            Some("requests") => {
                let before = match parts.uri.query() {
                    None => None,
                    Some(query) => Some(
                        query
                            .strip_prefix("before=")
                            .ok_or_else(|| {
                                ApiError::bad_request(
                                    "invalid_query",
                                    "Only before=<unix_seconds> is supported",
                                )
                            })?
                            .parse::<u64>()
                            .map_err(|_| {
                                ApiError::bad_request("invalid_query", "Invalid before timestamp")
                            })?,
                    ),
                };
                if before.is_some_and(|value| value > i64::MAX as u64) {
                    return Err(ApiError::bad_request(
                        "invalid_query",
                        "Timestamp exceeds the supported range",
                    ));
                }
                Ok(Json(state.ledger.requests(id, before).map_err(error)?).into_response())
            }
            _ => Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "Unknown control route",
            )),
        };
    }
    authorize_owner(&state, headers, id, method, signed_path, &body, now)?;
    match segments.get(3).copied() {
        Some("topup") => {
            // This settles a payment to an existing immutable agreement; it does not
            // admit a new session. Draining or an offline engine must not strand credit.
            let request: TopupRequest = parse(&body)?;
            let account = state.ledger.account(id).map_err(error)?;
            if account.status != "active" {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "account_frozen",
                    "Only active paid accounts accept funding",
                ));
            }
            let payment = verify_funding(
                &state,
                id,
                &request.raw_transaction,
                &account.consumer_wallet,
                None,
                &protocol::topup_data(id).map_err(error)?,
                now,
            )
            .await?;
            Ok(Json(
                state
                    .ledger
                    .credit_topup(id, &payment, now)
                    .map_err(error)?,
            )
            .into_response())
        }
        Some("credentials") if method == "DELETE" => {
            if !body.is_empty() {
                return Err(ApiError::bad_request(
                    "invalid_request",
                    "Revocation requires an empty body",
                ));
            }
            state
                .ledger
                .revoke_credential(id, segments[4])
                .map_err(error)?;
            Ok(Json(json!({"revoked":true})).into_response())
        }
        Some("credentials") => {
            let request: CredentialIssueRequest = parse(&body)?;
            let credential = if segments.len() == 6 {
                state
                    .ledger
                    .rotate_credential(id, segments[4], &request, now)
            } else {
                state.ledger.issue_credential(id, &request, now)
            }
            .map_err(error)?;
            Ok(Json(credential).into_response())
        }
        _ => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "Unknown control route",
        )),
    }
}

async fn verify_funding(
    state: &ProviderState,
    id: &str,
    raw: &str,
    from: &str,
    value: Option<&str>,
    data: &[u8],
    now: u64,
) -> Result<VerifiedPayment, ApiError> {
    match state
        .chain
        .verify_payment(raw, from, &state.wallet, value, data)
        .await
    {
        Ok(payment) => Ok(payment),
        Err(failure) => {
            if matches!(failure, ChainError::Unavailable(_)) {
                state.funding_ready.store(false, Ordering::Release);
            }
            let (status,code,message)=match failure {
                ChainError::Pending(_)=>(StatusCode::CONFLICT,"funding_pending","Funding has not reached the canonical safe block; retry with a fresh owner proof"),
                ChainError::Rejected(_)=>(StatusCode::BAD_REQUEST,"funding_rejected","Funding transaction does not satisfy the agreement and settlement policy"),
                ChainError::Unavailable(_)=>(StatusCode::SERVICE_UNAVAILABLE,"settlement_unavailable","Settlement verification is unavailable; no credit has been issued"),
            };
            state
                .ledger
                .record_funding_attempt(
                    id,
                    raw,
                    match code {
                        "funding_pending" => "pending",
                        "funding_rejected" => "rejected",
                        _ => "unavailable",
                    },
                    now,
                )
                .map_err(error)?;
            Err(ApiError::new(status, code, message))
        }
    }
}

/// Funding and fee-outbox health are independent of serving settled balances.
/// Messages are fixed categories: never expose RPC responses, URLs or secrets.
#[derive(Debug, Default, serde::Serialize)]
pub struct MaintenanceReport {
    pub funding_error: Option<String>,
    pub fee_error: Option<String>,
    pub funding_rejected: bool,
}
fn chain_failure(failure: &ChainError) -> &'static str {
    match failure {
        ChainError::Unavailable(_) => "configured RPC unavailable or canonical status untrusted",
        ChainError::Pending(_) => "canonical settlement pending",
        ChainError::Rejected(message) if message.contains("insufficient") => {
            "wallet balance insufficient for payment and gas"
        }
        ChainError::Rejected(_) => {
            "chain identity, payment evidence or transaction policy rejected"
        }
    }
}
pub async fn reconcile(state: Arc<ProviderState>) -> Result<MaintenanceReport, String> {
    let _guard = MAINTENANCE.lock().await;
    let now = protocol::now()?;
    let mut report = MaintenanceReport::default();
    for payment in state.ledger.funding_to_recheck()? {
        let observation = state.chain.recheck_payment(&payment).await;
        state
            .ledger
            .apply_funding_recheck(&payment.tx_hash, &observation, now)?;
        if let Err(failure) = observation {
            state.funding_ready.store(false, Ordering::Release);
            if matches!(failure, ChainError::Rejected(_)) {
                state.accepting.store(false, Ordering::Release);
            }
            report.funding_rejected |= matches!(failure, ChainError::Rejected(_));
            report.funding_error = Some(format!("Funding recheck: {}", chain_failure(&failure)));
        }
    }
    // A fee submission failure is operator debt, never evidence against consumer credit.
    report.fee_error = reconcile_fees(&state, now).await.err();
    Ok(report)
}
async fn reconcile_fees(state: &ProviderState, now: u64) -> Result<(), String> {
    let mut failure = None;
    for mut batch in state
        .ledger
        .fee_batches_to_recheck(now)
        .map_err(|_| "Fee outbox scan failed")?
    {
        if let Some(evidence) = &batch.evidence {
            match state.chain.recheck_payment(evidence).await {
                Ok(PaymentState::Accepted(evidence)) => {
                    batch.evidence = Some(evidence);
                    state
                        .ledger
                        .update_fee_batch(&batch)
                        .map_err(|_| "Fee inclusion refresh failed")?;
                }
                Ok(PaymentState::Pending) => {
                    batch.status = "submitted".into();
                    state
                        .ledger
                        .update_fee_batch(&batch)
                        .map_err(|_| "Fee outbox update failed")?;
                }
                Ok(PaymentState::Orphaned) => {
                    batch.status = "orphaned".into();
                    state
                        .ledger
                        .update_fee_batch(&batch)
                        .map_err(|_| "Fee outbox update failed")?;
                }
                Err(error) => {
                    failure = Some(format!("Fee evidence recheck: {}", chain_failure(&error)));
                }
            }
        } else {
            return Err("Accepted fee batch is missing canonical evidence".into());
        }
    }
    if let Err(error) = remit_inner(state, false, now).await {
        failure = Some(error);
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub async fn remit_fees(state: Arc<ProviderState>, force: bool) -> Result<(), String> {
    let _guard = MAINTENANCE.lock().await;
    remit_inner(&state, force, protocol::now()?).await
}
async fn remit_inner(state: &ProviderState, force: bool, now: u64) -> Result<(), String> {
    let batches = state
        .ledger
        .prepare_fee_batches(now, force)
        .map_err(|_| "Fee outbox preparation failed")?;
    let mut failure = None;
    for mut batch in batches {
        let data = batch
            .payment_data()
            .map_err(|_| "Fee batch commitment is invalid")?;
        match state
            .chain
            .send_payment_once(
                &format!("fee:{}", batch.id),
                &state.private_key,
                &batch.platform_wallet,
                &batch.amount_wei,
                &data,
            )
            .await
        {
            Ok(submitted) => {
                batch.tx_hash = Some(submitted.tx_hash);
                batch.raw_transaction = Some(submitted.raw_transaction);
                batch.status = "submitted".into();
                state
                    .ledger
                    .update_fee_batch(&batch)
                    .map_err(|_| "Fee submission evidence could not be persisted")?;
            }
            Err(error) => {
                failure = Some(format!("Fee submission: {}", chain_failure(&error)));
                continue;
            }
        }
        let raw = batch
            .raw_transaction
            .as_deref()
            .ok_or("fee submission did not retain signed transaction")?;
        match state
            .chain
            .verify_payment(
                raw,
                &state.wallet,
                &batch.platform_wallet,
                Some(&batch.amount_wei),
                &data,
            )
            .await
        {
            Ok(evidence) => {
                batch.status = "accepted".into();
                batch.evidence = Some(evidence);
                state
                    .ledger
                    .update_fee_batch(&batch)
                    .map_err(|_| "Accepted fee evidence could not be persisted")?;
            }
            Err(ChainError::Pending(_)) => {}
            Err(error) => {
                failure = Some(format!("Fee verification: {}", chain_failure(&error)));
            }
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
