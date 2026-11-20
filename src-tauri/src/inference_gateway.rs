//! OpenAI-compatible, contract-scoped inference. The durable ledger owns every
//! debit; an HTTP retry never starts an already-reserved request a second time.
use crate::app::ProviderState;
use crate::billing_store::{CredentialContext, RequestView};
use crate::error::ApiError;
use crate::inference_backend::{
    self as backend, Backend, Endpoint, SseDecoder, StreamNormalization, StreamSpec, StreamValidation,
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
};
use crate::protocol::{self, TokenUsage};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest, Path, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

const MAX_BODY_READERS: usize = 64;
const BODY_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The operator's reasons for a settlement-failure stop (provider-daemon-lifecycle
/// requirement 6), reported by `GET /admin/status` until `POST /admin/resume`.
pub const DURABLE_SETTLEMENT_FAILED: &str =
    "Durable settlement failed; admission stays stopped until the operator resumes the provider";
pub const SETTLEMENT_CLOCK_FAILED: &str =
    "Accounting clock unavailable at settlement; admission stays stopped until the operator resumes the provider";

struct ModelRuntime {
    backend: Result<Backend, ()>,
    slots: Arc<Semaphore>,
    arrival: Mutex<(Instant, u128)>,
    profile: Option<Arc<protocol::ServingProfile>>,
}
struct Gateway {
    provider: Arc<ProviderState>,
    models: HashMap<String, ModelRuntime>,
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
    rates: Mutex<HashMap<String, (u64, u32)>>,
    body_slots: Semaphore,
}

pub fn router(state: Arc<ProviderState>) -> Router {
    let runtime_models = state
        .config
        .models
        .iter()
        .map(|config| {
            (
                config.model.id.clone(),
                ModelRuntime {
                    backend: Backend::new(config).map_err(|_| ()),
                    slots: state.model_slots[&config.model.id].clone(),
                    arrival: Mutex::new((Instant::now(), u128::from(config.max_concurrency) * 1_000_000_000)),
                    profile: config.model.serving.clone().map(Arc::new),
                },
            )
        })
        .collect();
    let gateway = Arc::new(Gateway {
        provider: state,
        models: runtime_models,
        active: Mutex::new(HashMap::new()),
        rates: Mutex::new(HashMap::new()),
        body_slots: Semaphore::new(MAX_BODY_READERS),
    });
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/completions", post(completions))
        .route("/v1/embeddings", post(embeddings))
        .route("/v1/requests/:id", get(request_status))
        .route("/v1/requests/:id/cancel", post(cancel_request))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(gateway)
}

fn ledger_error(error: String) -> ApiError {
    let (status, code, message) = if error.starts_with("insufficient_funds") {
        (
            StatusCode::PAYMENT_REQUIRED,
            "insufficient_balance",
            "Prepaid balance cannot cover this request's maximum authorized usage",
        )
    } else if error.starts_with("spend_cap") {
        (
            StatusCode::PAYMENT_REQUIRED,
            "credential_spend_cap",
            "Credential spending cap cannot cover this request's maximum authorized usage",
        )
    } else if error.starts_with("duplicate_request") {
        (
            StatusCode::CONFLICT,
            "duplicate_request",
            "Request was already admitted; inspect its durable status rather than replaying it",
        )
    } else if error.starts_with("saturation") || error.starts_with("rate_limit") {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_exceeded",
            "Account concurrency or request rate limit exceeded",
        )
    } else if error.starts_with("unauthorized") {
        (
            StatusCode::UNAUTHORIZED,
            "invalid_api_key",
            "Bearer credential is invalid, expired or revoked",
        )
    } else if error.starts_with("forbidden")
        || error.starts_with("paused")
        || error.starts_with("account_frozen")
    {
        (
            StatusCode::FORBIDDEN,
            "account_forbidden",
            "Account is paused or credential does not authorize this operation",
        )
    } else if error.starts_with("model_unqualified") || error.starts_with("model_revision_unavailable") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "model_unqualified",
            "Contracted serving terms or current qualification are unavailable; no paid dispatch",
        )
    } else if error.starts_with("not_found") {
        (
            StatusCode::NOT_FOUND,
            "request_not_found",
            "Request was not found in this account",
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "ledger_unavailable",
            "Durable accounting is unavailable; inspect request status before retrying",
        )
    };
    ApiError::new(status, code, message)
}
fn authenticate(
    gateway: &Gateway,
    headers: &HeaderMap,
    scope: &str,
) -> Result<CredentialContext, ApiError> {
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return Err(ApiError::unauthorized(
            "Supply exactly one bearer credential",
        ));
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .ok_or_else(|| ApiError::unauthorized("A valid bearer credential is required"))?;
    let context = gateway
        .provider
        .ledger
        .authenticate(bearer, protocol::now().map_err(ApiError::from)?)
        .map_err(ledger_error)?;
    if !context.scopes.iter().any(|s| s == scope) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "credential_scope",
            "Credential does not authorize this operation",
        ));
    }
    Ok(context)
}
fn request_key(headers: &HeaderMap) -> Result<String, ApiError> {
    if headers.get_all("idempotency-key").iter().count() > 1 {
        return Err(ApiError::bad_request(
            "invalid_idempotency_key",
            "Supply at most one Idempotency-Key",
        ));
    }
    match headers.get("idempotency-key") {
        Some(value) => {
            let key = value.to_str().map_err(|_| {
                ApiError::bad_request("invalid_idempotency_key", "Idempotency-Key must be ASCII")
            })?;
            validate_id(key)?;
            Ok(key.to_owned())
        }
        None => Ok(uuid::Uuid::new_v4().to_string()),
    }
}
fn validate_id(id: &str) -> Result<(), ApiError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ApiError::bad_request("invalid_request_id", "Request identifiers allow 1 to 128 ASCII letters, digits, dots, underscores, colons or hyphens"));
    }
    Ok(())
}
/// Dimension reduction is not offered: an embedding request may name only the native
/// dimension that readiness measured for the deployment, and none while no value is
/// recorded. Refused as an unsupported option, before tokenization and reservation.
fn check_dimensions(
    provider: &ProviderState,
    request: &backend::PreparedRequest,
    model_id: &str,
) -> Result<(), ApiError> {
    let Some(requested) = request.body.get("dimensions").filter(|_| request.endpoint == Endpoint::Embeddings) else {
        return Ok(());
    };
    let native = provider
        .embedding_dimensions
        .read()
        .ok()
        .and_then(|recorded| recorded.get(model_id).copied());
    if native.is_none_or(|native| requested.as_u64() != Some(native)) {
        return Err(ApiError::bad_request(
            "unsupported_parameter",
            "This deployment does not produce the requested embedding dimensions; omit dimensions or request its native dimension",
        ));
    }
    Ok(())
}
fn with_id(mut response: Response, id: &str) -> Response {
    if let Ok(header) = HeaderValue::from_str(id) {
        response.headers_mut().insert("x-request-id", header);
    }
    response
}
fn duplicate(view: RequestView) -> Response {
    with_id((StatusCode::CONFLICT, Json(json!({"error":{"message":"Request already exists and will not be replayed. Retrieve its status; response content is not retained.","type":"invalid_request_error","param":null,"code":"duplicate_request","request_id":view.id,"request_status":view.status}}))).into_response(), &view.id)
}
impl Gateway {
    /// Runs before tokenization, including free/failed admission attempts. Durable
    /// reserve also applies its own credential limits; rotating keys cannot reset
    /// this account-level window. Memory is bounded and expired entries are purged.
    fn rate_limit(&self, account: &str, now: u64) -> Result<(), ApiError> {
        let mut rates = self
            .rates
            .lock()
            .map_err(|_| ApiError::internal("Admission lock unavailable"))?;
        let window = now / 60;
        rates.retain(|_, (old, _)| *old == window);
        if !rates.contains_key(account) && rates.len() >= 8192 {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "provider_saturated",
                "Provider admission capacity reached",
            ));
        }
        let (_, count) = rates.entry(account.into()).or_insert((window, 0));
        if *count >= 120 {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_exceeded",
                "Account is limited to 120 inference attempts per minute",
            ));
        }
        *count += 1;
        Ok(())
    }
}
async fn models(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let context = authenticate(&gateway, &headers, "infer")?;
    let current = gateway.provider.qualified_model(&context.account.model).filter(|_| {
        gateway.provider.accepting.load(Ordering::Acquire)
            && matches!(context.account.status.as_str(), "active" | "free")
    });
    let data = if let Some(model) = current {
        vec![
            json!({"id":model.id,"object":"model","created":0,"owned_by":gateway.provider.wallet,"chiral":{"model":model,"max_embedding_batch":backend::MAX_EMBEDDING_BATCH,"max_request_bytes":MAX_REQUEST_BYTES,"requests_per_account_minute":120,"tool_choice":"auto_only_when_qualified","cancellation_billing":"known_final_usage_only_otherwise_provider_loss"}}),
        ]
    } else {
        vec![]
    };
    Ok(Json(json!({"object":"list","data":data})))
}
async fn request_status(
    State(gateway): State<Arc<Gateway>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<RequestView>, ApiError> {
    validate_id(&id)?;
    let context = authenticate(&gateway, &headers, "read")?;
    Ok(Json(
        gateway
            .provider
            .ledger
            .request_view(&context.account.contract_id, &id)
            .map_err(ledger_error)?,
    ))
}
async fn cancel_request(
    State(gateway): State<Arc<Gateway>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    validate_id(&id)?;
    let context = authenticate(&gateway, &headers, "infer")?;
    let view = gateway
        .provider
        .ledger
        .request_view(&context.account.contract_id, &id)
        .map_err(ledger_error)?;
    let active = gateway
        .active
        .lock()
        .map_err(|_| ApiError::internal("Cancellation lock unavailable"))?;
    if let Some(cancel) = active.get(&view.id) {
        let _ = cancel.send(true);
        Ok(with_id(
            (
                StatusCode::ACCEPTED,
                Json(json!({"id":view.id,"status":"cancelling"})),
            )
                .into_response(),
            &view.id,
        ))
    } else {
        Ok(with_id(Json(view.clone()).into_response(), &view.id))
    }
}
async fn chat(State(gateway): State<Arc<Gateway>>, request: Request) -> Response {
    infer(gateway, request, Endpoint::Chat)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}
async fn completions(State(gateway): State<Arc<Gateway>>, request: Request) -> Response {
    infer(gateway, request, Endpoint::Completions)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}
async fn embeddings(State(gateway): State<Arc<Gateway>>, request: Request) -> Response {
    infer(gateway, request, Endpoint::Embeddings)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

/// A single owner moves from the handler to the request task (`execute`), which
/// settles cancellation, client disconnects, deadlines and engine results with
/// `finish`. Drop settles only what the gateway cannot attribute, such as a task
/// panic, runtime shutdown or a failed durable write, as `interrupted`, which is
/// provider-incomplete. Process death is handled by
/// BillingStore::recover_requests before admission on next startup.
struct Settlement {
    gateway: Arc<Gateway>,
    id: String,
    known_usage: Option<TokenUsage>,
    settled: bool,
    profile: Option<Arc<protocol::ServingProfile>>,
    normalization: StreamNormalization,
    accepted: Instant,
    first_output: Option<Instant>,
    last_output: Option<Instant>,
    in_envelope: bool,
    client_stalled: bool,
    blocked: Duration,
    observed_tokens: u64,
    token_clock_valid: bool,
    _permit: OwnedSemaphorePermit,
}
impl Settlement {
    fn finish(&mut self, status: &str) -> Result<(), ApiError> {
        if self.settled {
            return Ok(());
        }
        let usage = self.known_usage.clone().unwrap_or_default();
        let final_status = if self.known_usage.is_some() {
            status.to_owned()
        } else {
            format!("{status}_unknown_usage")
        };
        let now = match protocol::now() {
            Ok(now) => now,
            Err(_) => {
                self.gateway.provider.record_settlement_fault(SETTLEMENT_CLOCK_FAILED);
                return Err(ApiError::internal("Accounting clock unavailable"));
            }
        };
        let micros = |duration: Duration| u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        let ttft_us = self.first_output.map(|first| micros(first.duration_since(self.accepted)));
        let decode_us = self.first_output.zip(self.last_output)
            .filter(|_| self.token_clock_valid)
            .map(|(first, last)| micros(last.duration_since(first)));
        let elapsed_us = Some(micros(self.accepted.elapsed().saturating_sub(self.blocked)));
        let performance = Some(match self.profile.as_ref() {
            Some(profile) => crate::performance::outcome(profile, self.in_envelope, ttft_us, decode_us, self.known_usage.as_ref(), status, self.client_stalled, elapsed_us, &(profile).qualification.evidence_sha256),
            None => protocol::PerformanceOutcome {
                evidence_sha256: String::new(), policy: "unqualified".into(),
                eligibility: "unqualified".into(), ttft_us, decode_us, elapsed_us,
                output_tokens: self.known_usage.as_ref().map(|u| u.completion_tokens),
                attained: None, remedy: "none".into(),
            },
        });
        match self
            .gateway
            .provider
            .ledger
            .settle_measured(&self.id, &usage, &final_status, performance.as_ref(), now)
        {
            Ok(_) => {
                self.settled = true;
                Ok(())
            }
            Err(_) => {
                // Never acknowledge a successful completion with an unpersisted debit.
                // A repeated settle is idempotent; restart recovery remains authoritative.
                // Admission stays closed until the operator resumes, also when the drop's
                // single retry then records the request (design observation 13).
                self.gateway.provider.record_settlement_fault(DURABLE_SETTLEMENT_FAILED);
                Err(ApiError::internal(
                    "Durable settlement failed; provider admission has been stopped",
                ))
            }
        }
    }
}
impl Drop for Settlement {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.finish("interrupted");
        }
        if let Ok(mut active) = self.gateway.active.lock() {
            active.remove(&self.id);
        }
    }
}

async fn infer(
    gateway: Arc<Gateway>,
    request: Request,
    endpoint: Endpoint,
) -> Result<Response, ApiError> {
    let headers = request.headers();
    let context = authenticate(&gateway, headers, "infer")?;
    let key = request_key(headers)?;
    if let Ok(view) = gateway
        .provider
        .ledger
        .request_view(&context.account.contract_id, &key)
    {
        return Ok(duplicate(view));
    }
    if !gateway.provider.accepting.load(Ordering::Acquire) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_paused",
            "Provider is not accepting new inference",
        ));
    }
    if !matches!(context.account.status.as_str(), "active" | "free") {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_paused",
            "Account is not active",
        ));
    }
    gateway.rate_limit(
        &context.account.contract_id,
        protocol::now().map_err(ApiError::from)?,
    )?;
    if !headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next() == Some("application/json"))
    {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
            "Content-Type must be application/json",
        ));
    }
    // Unlike a handler Bytes extractor, collection here cannot precede bearer
    // authentication. Readers never queue, and their deadline is independent of
    // model tokenization/generation/SSE. Drop releases the slot on every exit,
    // including a body error or cancellation of the HTTP handler.
    let bytes = {
        let _slot = gateway.body_slots.try_acquire().map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "provider_saturated",
                "Inference body reader capacity reached",
            )
        })?;
        tokio::time::timeout(BODY_READ_TIMEOUT, Bytes::from_request(request, &()))
            .await
            .map_err(|_| {
                ApiError::new(
                    StatusCode::REQUEST_TIMEOUT,
                    "request_timeout",
                    "Inference request body deadline exceeded",
                )
            })?
            .map_err(|rejection| {
                ApiError::new(
                    rejection.status(),
                    "invalid_request_body",
                    "Request body is unreadable or exceeds 256 KiB",
                )
            })?
    };
    // Acceptance excludes slow request uploads, but includes parsing, admission and tokenization.
    let accepted = Instant::now();
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| {
        ApiError::bad_request("invalid_json", "Request must be a bounded JSON object")
    })?;
    let mut request = backend::prepare(body, endpoint, &context.account.model)?;
    let client_stream = request.stream;
    // Both public generation modes use the same private stream and output clock.
    if endpoint != Endpoint::Embeddings {
        request.stream = true;
        request.body["stream"] = json!(true);
        request.body["stream_options"] = json!({"include_usage":true});
    }
    let runtime = gateway
        .models
        .get(&context.account.model.id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "model_unavailable",
                "Contracted model is not deployed",
            )
        })?;
    let engine = runtime
        .backend
        .as_ref()
        .map_err(|_| backend::upstream_error())?;
    if !context.account.model.accepts_execution(&engine.config.model) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "model_revision_unavailable",
            "The exact contracted model revision is no longer deployed",
        ));
    }
    if gateway.provider.qualified_model(&context.account.model).is_none() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "model_unqualified",
            "Contracted model has not passed current readiness and inference qualification",
        ));
    }
    check_dimensions(&gateway.provider, &request, &engine.config.model.id)?;
    // Queue length is deliberately zero: every operation, including private
    // tokenization, must acquire a model slot or fail before making a reservation.
    let permit = runtime.slots.clone().try_acquire_owned().map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "model_saturated",
            "Model concurrency limit reached; no request was dispatched",
        )
    })?;
    if let Some(profile) = &context.account.model.serving {
        let mut bucket = runtime.arrival.lock().map_err(|_| ApiError::internal("Admission clock unavailable"))?;
        let now = Instant::now();
        bucket.1 = (bucket.1 + now.duration_since(bucket.0).as_nanos()
            .saturating_mul(u128::from(profile.arrival_milli_rps)) / 1000)
            .min(u128::from(profile.max_concurrency) * 1_000_000_000);
        bucket.0 = now;
        if bucket.1 < 1_000_000_000 {
            return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limit",
                "Qualified global arrival envelope exceeded; no paid dispatch"));
        }
        bucket.1 -= 1_000_000_000;
    }
    let deadline = Instant::now() + Duration::from_secs(engine.config.request_timeout_secs);
    tokio::time::timeout_at(deadline, async {
        engine.ready().await?;
        engine.tokenize(&mut request).await
    })
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "tokenization_timeout",
            "Tokenization timed out; no inference was dispatched",
        )
    })??;
    if !gateway.provider.accepting.load(Ordering::Acquire) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_paused",
            "Provider stopped accepting inference before reservation",
        ));
    }
    if gateway.provider.qualified_model(&context.account.model).is_none() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "model_unqualified",
            "Model qualification was withdrawn before reservation",
        ));
    }
    // Readiness may have measured again while the input was tokenized.
    check_dimensions(&gateway.provider, &request, &engine.config.model.id)?;
    let reservation = match gateway.provider.ledger.reserve(
        &context.account.contract_id,
        &context.credential_id,
        &key,
        request.prompt_tokens,
        request.max_output_tokens,
        protocol::now().map_err(ApiError::from)?,
        &engine.config.model,
    ) {
        Ok(reservation) => reservation,
        Err(error) if error.starts_with("duplicate_request") => {
            return Ok(duplicate(
                gateway
                    .provider
                    .ledger
                    .request_view(&context.account.contract_id, &key)
                    .map_err(ledger_error)?,
            ))
        }
        Err(error) => return Err(ledger_error(error)),
    };
    let id = reservation.id;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let settlement = Settlement {
        gateway: gateway.clone(),
        id: id.clone(),
        known_usage: None,
        settled: false,
        profile: runtime.profile.clone(),
        normalization: StreamNormalization::new(request.logprobs, request.echo_prompt.take(), context.account.model.context_length),
        accepted, first_output: None, last_output: None,
        in_envelope: endpoint != Endpoint::Embeddings && request.prompt_tokens <= 2048
            && request.max_output_tokens <= 256
            && request.prompt_tokens.saturating_add(request.max_output_tokens) <= 4096,
        client_stalled: false,
        blocked: Duration::ZERO,
        observed_tokens: 0, token_clock_valid: true,
        _permit: permit,
    };
    gateway
        .active
        .lock()
        .map_err(|_| ApiError::internal("Cancellation lock unavailable"))?
        .insert(id.clone(), cancel_tx);
    // Dispatch and collection run in a task that owns the settlement. When the
    // client closes its connection, hyper drops this handler future; the task
    // sees the closed reply and settles a client disconnect, not `interrupted`.
    let (reply, answer) = oneshot::channel();
    tokio::spawn(execute(
        engine.clone(), request, context.account.model.id, client_stream,
        settlement, cancel_rx, reply, deadline,
    ));
    Ok(answer.await.unwrap_or_else(|_| {
        // Only a task panic or runtime shutdown ends the task without an answer;
        // Settlement::drop has then recorded a provider-incomplete interruption.
        with_id(ApiError::internal("Inference task ended without a response").into_response(), &id)
    }))
}

/// Owns one reserved request from engine dispatch to settlement, outside the
/// HTTP handler. A closed `reply` (the handler was dropped, e.g. on client EOF)
/// or a closed SSE body is a client disconnect. Every return drops any pending
/// engine request or response stream, which cancels upstream generation.
#[allow(clippy::too_many_arguments)]
async fn execute(
    engine: Backend,
    request: backend::PreparedRequest,
    model: String,
    client_stream: bool,
    mut settlement: Settlement,
    mut cancel: watch::Receiver<bool>,
    mut reply: oneshot::Sender<Response>,
    deadline: Instant,
) {
    let endpoint = request.endpoint;
    let id = settlement.id.clone();
    let response = tokio::select! {
        biased;
        _ = cancelled(&mut cancel) => Err(ApiError::new(StatusCode::CONFLICT, "request_cancelled", "Inference request was cancelled")),
        _ = reply.closed() => Err(client_disconnected()),
        _ = tokio::time::sleep_until(deadline) => Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "engine_timeout", "Inference timed out; it will not be replayed")),
        response = engine.dispatch(&request, &id) => response,
    };
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let error = match settlement.finish(terminal_status(Some(&error))) {
                Ok(()) => error,
                Err(settlement_error) => settlement_error,
            };
            return answer(settlement, reply, error.into_response());
        }
    };
    if client_stream {
        let (sender, receiver) = mpsc::channel::<Bytes>(4);
        let failed = Arc::new(AtomicBool::new(false));
        let spec = StreamSpec {
            endpoint,
            model,
            prompt: request.prompt_tokens,
            output: request.max_output_tokens,
            include_usage: request.include_usage,
        };
        if reply.send(with_id(event_stream(receiver, failed.clone()), &id)).is_err() {
            // The client left before its event stream could start.
            let _ = settlement.finish("disconnected");
            return;
        }
        // Only bounded chunks and, for echo, the bounded prompt reach the response body.
        stream_worker(response, spec, settlement, cancel, sender, failed, deadline).await;
    } else if endpoint != Endpoint::Embeddings {
        let spec = StreamSpec {
            endpoint, model, prompt: request.prompt_tokens,
            output: request.max_output_tokens, include_usage: true,
        };
        let result = stream_inner(response, &spec, &mut settlement, &mut cancel,
            &mut Client::Collect(&mut reply), deadline).await;
        let status = terminal_status(result.as_ref().err());
        let response = match settlement.finish(status) {
            Err(error) => error.into_response(),
            Ok(()) => match result {
                Ok(value) => Json(value).into_response(),
                Err(error) => error.into_response(),
            },
        };
        answer(settlement, reply, response);
    } else {
        let result = tokio::select! {
            biased;
            _ = cancelled(&mut cancel) => Err(ApiError::new(StatusCode::CONFLICT, "request_cancelled", "Inference request was cancelled")),
            _ = reply.closed() => Err(client_disconnected()),
            _ = tokio::time::sleep_until(deadline) => Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "engine_timeout", "Inference timed out; it will not be replayed")),
            result = backend::read_json(response, MAX_RESPONSE_BYTES) => result,
        };
        let result = result.and_then(|mut value| {
            backend::normalize_embedding_usage(&mut value, endpoint)?;
            let usage = backend::validate_response(&value, &request)?;
            backend::validate_authorized_usage(
                &usage,
                request.prompt_tokens,
                request.max_output_tokens,
            )?;
            settlement.known_usage = Some(usage);
            Ok(value)
        });
        let status = terminal_status(result.as_ref().err());
        let response = match settlement.finish(status) {
            Err(error) => error.into_response(),
            Ok(()) => match result {
                Ok(value) => Json(value).into_response(),
                Err(error) => error.into_response(),
            },
        };
        answer(settlement, reply, response);
    }
}

/// Answers the handler once the request has settled. Dropping the settlement
/// first releases the model slot and cancellation entry before the client can
/// see the answer, as when the handler itself returned the response.
fn answer(settlement: Settlement, reply: oneshot::Sender<Response>, response: Response) {
    let id = settlement.id.clone();
    drop(settlement);
    let _ = reply.send(with_id(response, &id));
}

/// The public SSE body drains the bounded queue. When the worker failed without
/// queueing its error event (`failed`, set before its sender is dropped), the body
/// ends with an error instead of cleanly, so that an HTTP/1 chunked response is
/// not terminated like a completed stream.
fn event_stream(receiver: mpsc::Receiver<Bytes>, failed: Arc<AtomicBool>) -> Response {
    let body = futures_util::stream::unfold(Some(receiver), move |receiver| {
        let failed = failed.clone();
        async move {
            let mut receiver = receiver?;
            match receiver.recv().await {
                Some(bytes) => Some((Ok(bytes), Some(receiver))),
                None if failed.load(Ordering::Acquire) => Some((
                    Err(std::io::Error::other("inference stream failed after its output began")),
                    None,
                )),
                None => None,
            }
        }
    });
    let mut response = Body::from_stream(body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

/// Where generated output goes, and how the client's departure is observed.
enum Client<'a> {
    /// Public SSE: chunks cross a bounded queue whose receiver is the response body.
    Stream(&'a mpsc::Sender<Bytes>),
    /// Non-streaming: output is collected for the handler's single reply.
    Collect(&'a mut oneshot::Sender<Response>),
}
impl Client<'_> {
    /// Resolves once nothing can reach the client any more.
    async fn closed(&mut self) {
        match self {
            Client::Stream(sender) => sender.closed().await,
            Client::Collect(reply) => reply.closed().await,
        }
    }
    fn sender(&self) -> Option<&mpsc::Sender<Bytes>> {
        match self {
            Client::Stream(sender) => Some(*sender),
            Client::Collect(_) => None,
        }
    }
}

fn client_disconnected() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "client_disconnected", "Client disconnected")
}
async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

fn terminal_status(error: Option<&ApiError>) -> &'static str {
    match error.map(|error| error.code.as_str()) {
        None => "completed",
        Some("request_cancelled") => "cancelled",
        Some("client_disconnected") => "disconnected",
        Some("engine_timeout") => "timed_out",
        _ => "failed",
    }
}

async fn send_chunk(
    sender: &mpsc::Sender<Bytes>,
    cancel: &mut watch::Receiver<bool>,
    deadline: Instant,
    bytes: Bytes,
) -> Result<(), ApiError> {
    tokio::select! {
        biased;
        _ = cancelled(cancel) => Err(ApiError::new(StatusCode::CONFLICT, "request_cancelled", "Inference request was cancelled")),
        _ = tokio::time::sleep_until(deadline) => Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "engine_timeout", "Inference stream timed out")),
        result = sender.send(bytes) => result.map_err(|_| client_disconnected()),
    }
}
async fn stream_worker(
    response: reqwest::Response,
    spec: StreamSpec,
    mut settlement: Settlement,
    mut cancel: watch::Receiver<bool>,
    sender: mpsc::Sender<Bytes>,
    failed: Arc<AtomicBool>,
    deadline: Instant,
) {
    let result = stream_inner(
        response,
        &spec,
        &mut settlement,
        &mut cancel,
        &mut Client::Stream(&sender),
        deadline,
    )
    .await;
    let status = terminal_status(result.as_ref().err());
    let settled = settlement.finish(status);
    if let Err(error) = result.and(settled) {
        let bytes = Bytes::from(format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"error":{"message":error.message,"type":"server_error","param":null,"code":error.code}})
        ));
        // Never retain engine resources waiting for a slow/disconnected caller. When
        // the error event cannot be queued, the body ends abnormally instead, never
        // like a completed stream; the flag is set before the sender is dropped.
        if sender.try_send(bytes).is_err() {
            failed.store(true, Ordering::Release);
        }
    }
}
fn output_event(value: &Value) -> bool {
    let choice = &value["choices"][0];
    choice["text"].as_str().is_some_and(|s| !s.is_empty())
        || choice["delta"]["content"].as_str().is_some_and(|s| !s.is_empty())
        || choice["delta"]["refusal"].as_str().is_some_and(|s| !s.is_empty())
        || choice["delta"]["tool_calls"].as_array().is_some_and(|a| !a.is_empty())
}

fn append_text(target: &mut Value, fragment: &mut Value) -> Result<(), ApiError> {
    if fragment.is_null() { return Ok(()); }
    if !fragment.is_string() { return Err(backend::upstream_error()); }
    if target.is_null() || target.as_str() == Some("") {
        *target = fragment.take();
    } else if let Value::String(buffer) = target {
        buffer.push_str(fragment.as_str().ok_or_else(backend::upstream_error)?);
    } else { return Err(backend::upstream_error()); }
    Ok(())
}

fn append_logprobs(target: &mut Value, fragment: &mut Value) -> Result<(), ApiError> {
    if fragment.is_null() { return Ok(()); }
    let fields = fragment.as_object_mut().ok_or_else(backend::upstream_error)?;
    if target.is_null() { *target = json!({}); }
    let output = target.as_object_mut().ok_or_else(backend::upstream_error)?;
    for (key, value) in std::mem::take(fields) {
        if value.is_null() { output.entry(key).or_insert(Value::Null); continue; }
        let Value::Array(mut values) = value else { return Err(backend::upstream_error()); };
        let destination = output.entry(key).or_insert(Value::Null);
        if destination.is_null() { *destination = Value::Array(values); }
        else { destination.as_array_mut().ok_or_else(backend::upstream_error)?.append(&mut values); }
    }
    Ok(())
}

/// Bounded by the same total upstream byte limit as streaming; never retokenizes output.
fn collect_event(output: &mut Value, event: &mut Value, endpoint: Endpoint) -> Result<(), ApiError> {
    if output.get("id").is_none() {
        for field in ["id", "model", "created", "system_fingerprint", "service_tier"] {
            if let Some(value) = event.get_mut(field) { output[field] = value.take(); }
        }
        output["object"] = json!(if endpoint == Endpoint::Chat { "chat.completion" } else { "text_completion" });
        if endpoint == Endpoint::Chat {
            output["choices"][0]["message"] = json!({"role":"assistant","content":""});
        } else {
            output["choices"][0]["text"] = json!("");
        }
    }
    if event["choices"].as_array().is_some_and(|choices| choices.is_empty()) {
        output["usage"] = event["usage"].take();
        return Ok(());
    }
    let choice = &mut event["choices"][0];
    for field in ["finish_reason", "stop_reason"] {
        if !choice[field].is_null() { output["choices"][0][field] = choice[field].take(); }
    }
    append_logprobs(&mut output["choices"][0]["logprobs"], &mut choice["logprobs"])?;
    if endpoint != Endpoint::Chat {
        return append_text(&mut output["choices"][0]["text"], &mut choice["text"]);
    }
    let message = &mut output["choices"][0]["message"];
    for field in ["content", "refusal", "reasoning_content"] {
        if !choice["delta"][field].is_null() {
            append_text(&mut message[field], &mut choice["delta"][field])?;
        }
    }
    if let Some(calls) = choice["delta"]["tool_calls"].as_array_mut() {
        if message.get("tool_calls").is_none() { message["tool_calls"] = json!([]); }
        let target = message["tool_calls"].as_array_mut().ok_or_else(backend::upstream_error)?;
        for call in calls {
            let index = call["index"].as_u64().filter(|i| *i < 128).ok_or_else(backend::upstream_error)? as usize;
            if index > target.len() { return Err(backend::upstream_error()); }
            if index == target.len() {
                target.push(json!({"id":"","type":"function","function":{"name":"","arguments":""}}));
            }
            let entry = &mut target[index];
            append_text(&mut entry["id"], &mut call["id"])?;
            append_text(&mut entry["function"]["name"], &mut call["function"]["name"])?;
            append_text(&mut entry["function"]["arguments"], &mut call["function"]["arguments"])?;
        }
    }
    Ok(())
}

async fn stream_inner(
    response: reqwest::Response,
    spec: &StreamSpec,
    settlement: &mut Settlement,
    cancel: &mut watch::Receiver<bool>,
    client: &mut Client<'_>,
    deadline: Instant,
) -> Result<Value, ApiError> {
    let mut upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>> =
        Box::pin(response.bytes_stream());
    let mut decoder = SseDecoder::default();
    let mut validation = StreamValidation::default();
    let mut total_bytes = 0usize;
    let mut collected = json!({"choices":[{"index":0,"finish_reason":null}]});
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancelled(cancel) => return Err(ApiError::new(StatusCode::CONFLICT, "request_cancelled", "Inference request was cancelled")),
            _ = client.closed() => return Err(client_disconnected()),
            _ = tokio::time::sleep_until(deadline) => return Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "engine_timeout", "Inference stream timed out")),
            chunk = upstream.next() => chunk.ok_or_else(backend::upstream_error)?.map_err(|_| backend::upstream_error())?,
        };
        total_bytes = total_bytes
            .checked_add(chunk.len())
            .ok_or_else(backend::upstream_error)?;
        if total_bytes > MAX_RESPONSE_BYTES {
            return Err(backend::upstream_error());
        }
        for byte in chunk {
            if let Some(data) = decoder.byte(byte)? {
                if data == b"[DONE]" {
                    if !validation.finished || !validation.final_usage {
                        return Err(backend::upstream_error());
                    }
                    settlement.finish("completed")?;
                    if let Some(sender) = client.sender() {
                        send_chunk(sender, cancel, deadline, Bytes::from_static(b"data: [DONE]\n\n")).await?;
                    }
                    return Ok(collected);
                }
                let value: Value = serde_json::from_slice(&data).map_err(|_| backend::upstream_error())?;
                for mut value in settlement.normalization.events(value, spec.endpoint)?.into_iter().flatten() {
                let (forward, usage) = validation.event(&mut value, spec)?;
                let now = Instant::now() - settlement.blocked;
                let visible_output = output_event(&value) && value["chiral_echo"].as_bool() != Some(true);
                let token_ids = value["choices"][0]["token_ids"].as_array();
                let observed_output = if let Some(ids) = token_ids {
                    settlement.observed_tokens = settlement.observed_tokens.saturating_add(ids.len() as u64);
                    if ids.len() > 1 || ids.iter().any(|id| id.as_u64().is_none()) || (ids.is_empty() && visible_output) {
                        settlement.token_clock_valid = false;
                    }
                    !ids.is_empty()
                } else {
                    if visible_output { settlement.token_clock_valid = false; }
                    false
                };
                if observed_output {
                    settlement.first_output.get_or_insert(now);
                    settlement.last_output = Some(now);
                }
                // Token IDs are private instrumentation, never an OpenAI response extension.
                if let Some(object) = value.as_object_mut() {
                    object.remove("chiral_echo");
                }
                if let Some(choices) = value["choices"].as_array_mut() {
                    for choice in choices {
                        if let Some(object) = choice.as_object_mut() {
                            object.remove("token_ids");
                        }
                    }
                }
                let sender = client.sender();
                if sender.is_none() {
                    collect_event(&mut collected, &mut value, spec.endpoint)?;
                }
                if let Some(usage) = usage {
                    if settlement.observed_tokens != usage.completion_tokens {
                        settlement.token_clock_valid = false;
                    }
                    settlement.known_usage = Some(usage);
                }
                if forward && sender.is_some() {
                    let encoded =
                        serde_json::to_vec(&value).map_err(|_| backend::upstream_error())?;
                    let mut event = Vec::with_capacity(encoded.len() + 8);
                    event.extend_from_slice(b"data: ");
                    event.extend_from_slice(&encoded);
                    event.extend_from_slice(b"\n\n");
                    let sender = sender.expect("checked sender");
                    // A full bounded downstream queue identifies a consumer stall. Never
                    // attribute time spent awaiting its capacity to provider decoding.
                    let before_send = if sender.capacity() == 0 {
                        settlement.client_stalled = true;
                        Some(Instant::now())
                    } else { None };
                    let sent = send_chunk(sender, cancel, deadline, Bytes::from(event)).await;
                    if let Some(before_send) = before_send {
                        settlement.blocked += before_send.elapsed();
                    }
                    sent?;
                }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use tower::Service;

    #[test]
    fn internal_stream_preserves_nonstream_text_and_authoritative_logprobs() {
        let mut output = json!({"choices":[{"index":0,"finish_reason":null}]});
        for (text, offset, finish) in [("Hello", 0, Value::Null), (" world", 5, json!("length"))] {
            let mut event = json!({"id":"response","model":"model","created":123,
                "choices":[{"index":0,"text":text,"finish_reason":finish,
                    "logprobs":{"tokens":[text],"token_logprobs":[-0.5],"top_logprobs":[{text:-0.5}],
                        "text_offset":[offset]}}]});
            collect_event(&mut output, &mut event, Endpoint::Completions).unwrap();
        }
        let mut final_event = json!({"id":"response","model":"model","choices":[],
            "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}});
        collect_event(&mut output, &mut final_event, Endpoint::Completions).unwrap();
        assert_eq!(output["choices"][0]["text"], "Hello world");
        assert_eq!(output["choices"][0]["finish_reason"], "length");
        assert_eq!(output["choices"][0]["logprobs"]["tokens"], json!(["Hello"," world"]));
        assert_eq!(output["choices"][0]["logprobs"]["text_offset"], json!([0,5]));
        assert_eq!(output["usage"]["completion_tokens"], 2);
        assert_eq!(output["object"], "text_completion");
    }

    fn ingress_fixture() -> (tempfile::TempDir, Arc<ProviderState>, String) {
        ingress_fixture_for(|_| {})
    }

    /// `ingress_fixture` whose offer model, deployed and agreed, is first changed by `edit`.
    fn ingress_fixture_for(
        edit: impl FnOnce(&mut protocol::ModelSpec),
    ) -> (tempfile::TempDir, Arc<ProviderState>, String) {
        use crate::{
            billing_store::BillingStore,
            chain_client::ChainClient,
            network::NetworkConfig,
            offer_registry::OfferRegistry,
            provider_config::{DhtConfig, ModelConfig, ProviderConfig},
        };
        use std::sync::atomic::AtomicBool;
        let directory = tempfile::tempdir().unwrap();
        let mut offer = crate::offer_registry::tests::offer().payload;
        edit(&mut offer.model);
        let key = "11".repeat(32);
        let now = protocol::now().unwrap();
        let price = protocol::PriceSchedule {
            input_wei_per_1k: "0".into(),
            output_wei_per_1k: "0".into(),
        };
        let quote = protocol::sign(
            protocol::QUOTE_DOMAIN,
            protocol::Quote {
                id: protocol::random_id(),
                network: offer.network.clone(),
                offer_id: protocol::random_id(),
                provider_wallet: offer.provider_wallet.clone(),
                consumer_wallet: crate::wallet::address_from_private_key(&"22".repeat(32)).unwrap(),
                model: offer.model.clone(),
                price: price.clone(),
                funding_wei: "0".into(),
                platform_wallet: offer.platform_wallet.clone(),
                fee_bps: 50,
                max_concurrency: 1,
                nonce: protocol::random_id(),
                issued_at: now,
                expires_at: now + 300,
            },
            &key,
        )
        .unwrap();
        let ledger = Arc::new(
            BillingStore::open(&directory.path().join("ledger.sqlite"), &offer.network).unwrap(),
        );
        ledger.store_quote(&quote, now).unwrap();
        let bearer = ledger
            .open_account(&quote, None, now)
            .unwrap()
            .credential
            .unwrap()
            .bearer;
        let network = NetworkConfig {
            identity: offer.network.clone(),
            rpc_url: "http://127.0.0.1:1".into(),
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        };
        let chain = Arc::new(ChainClient::new(network.clone(), directory.path().into()).unwrap());
        let config = ProviderConfig {
            network,
            data_dir: directory.path().into(),
            signing_key_file: directory.path().join("key"),
            admin_token_file: directory.path().join("admin"),
            public_endpoint: "http://127.0.0.1:9419".into(),
            public_bind: "127.0.0.1:9419".parse().unwrap(),
            admin_bind: "127.0.0.1:9421".parse().unwrap(),
            platform_wallet: offer.platform_wallet,
            fee_bps: 50,
            min_funding_wei: "0".into(),
            models: vec![ModelConfig {
                model: offer.model,
                price,
                engine_url: "http://127.0.0.1:1".into(),
                engine_api_key_file: None,
                max_concurrency: 1,
                request_timeout_secs: 60,
                qualification_evidence_file: None,
                runtime: None,
            }],
            dht: DhtConfig {
                listen: "/ip4/127.0.0.1/tcp/0".into(),
                bootstrap: vec![],
                identity_file: directory.path().join("peer"),
            },
            development: true,
        };
        let state = Arc::new(ProviderState {
            model_slots: ProviderState::slots(&config),
            managed_engines: Default::default(),
            config: Arc::new(config),
            chain,
            ledger,
            private_key: Arc::new(key),
            wallet: offer.provider_wallet,
            offers: Arc::new(OfferRegistry::new(offer.network, true)),
            accepting: AtomicBool::new(true),
            settlement_fault: Default::default(),
            funding_ready: AtomicBool::new(true),
            qualified_models: std::sync::RwLock::new(Default::default()),
            embedding_dimensions: std::sync::RwLock::new(Default::default()),
        });
        (directory, state, bearer)
    }

    #[tokio::test]
    async fn renewed_qualification_keeps_existing_model_visible_without_rewriting_agreement() {
        let (_directory, mut state, _) = ingress_fixture();
        let inner = Arc::get_mut(&mut state).unwrap();
        let config = Arc::get_mut(&mut inner.config).unwrap();
        let deployed = &mut config.models[0].model;
        deployed.serving = Some(crate::performance::test_profile());
        let agreement = deployed.clone();
        deployed.serving.as_mut().unwrap().qualification.evidence_sha256 = "d".repeat(64);
        deployed.serving.as_mut().unwrap().qualification.measured_at += 1;
        inner.qualified_models.write().unwrap().insert(deployed.id.clone());
        let current = deployed.clone();
        let now = protocol::now().unwrap();
        let quote = protocol::sign(protocol::QUOTE_DOMAIN, protocol::Quote {
            id: protocol::random_id(), network: inner.config.network.identity.clone(),
            offer_id: protocol::random_id(), provider_wallet: inner.wallet.clone(),
            consumer_wallet: crate::wallet::address_from_private_key(&"22".repeat(32)).unwrap(),
            model: agreement.clone(), price: inner.config.models[0].price.clone(),
            funding_wei: "0".into(), platform_wallet: inner.config.platform_wallet.clone(),
            fee_bps: 50, max_concurrency: 1, nonce: protocol::random_id(),
            issued_at: now, expires_at: now + 300,
        }, &inner.private_key).unwrap();
        inner.ledger.store_quote(&quote, now).unwrap();
        let opened = inner.ledger.open_account(&quote, None, now).unwrap();
        let mut app = router(state.clone());
        let response = app.call(Request::builder().uri("/v1/models")
            .header(header::AUTHORIZATION, format!("Bearer {}", opened.credential.as_ref().unwrap().bearer))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let data: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 64 * 1024).await.unwrap()
        ).unwrap();
        assert_eq!(data["data"][0]["chiral"]["model"], serde_json::to_value(&current).unwrap());
        assert_eq!(state.ledger.account(&quote.payload.id).unwrap().model, agreement);
        state.qualified_models.write().unwrap().clear();
        assert!(state.qualified_model(&agreement).is_none());
    }

    fn inference_request(path: &str, bearer: Option<&str>, body: Body) -> Request {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(bearer) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        }
        request.body(body).unwrap()
    }

    fn incomplete_body(started: Option<mpsc::UnboundedSender<()>>) -> Body {
        Body::from_stream(
            futures_util::stream::once(async move {
                if let Some(started) = started {
                    started.send(()).unwrap();
                }
                Ok::<_, std::io::Error>(Bytes::from_static(b"{"))
            })
            .chain(futures_util::stream::pending()),
        )
    }

    async fn assert_api_error(response: Response, status: StatusCode, code: &str, bearer: &str) {
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error["error"]["code"], code);
        assert!(error["error"]["message"].is_string());
        assert!(error["error"]["type"].is_string());
        assert!(!std::str::from_utf8(&bytes).unwrap().contains(bearer));
    }

    #[tokio::test]
    async fn unauthenticated_incomplete_inference_bodies_are_not_polled() {
        let (_directory, state, bearer) = ingress_fixture();
        let mut app = router(state);
        for path in ["/v1/chat/completions", "/v1/completions", "/v1/embeddings"] {
            for credential in [None, Some("invalid-secret-credential")] {
                let (started, mut observed) = mpsc::unbounded_channel();
                let mut request =
                    inference_request(path, credential, incomplete_body(Some(started)));
                // Same incomplete framing as the live embeddings reproduction.
                request
                    .headers_mut()
                    .insert(header::CONTENT_LENGTH, HeaderValue::from_static("100000"));
                let response = tokio::time::timeout(Duration::from_millis(100), app.call(request))
                    .await
                    .expect("authentication must not wait for body completion")
                    .unwrap();
                assert_api_error(
                    response,
                    StatusCode::UNAUTHORIZED,
                    "invalid_api_key",
                    &bearer,
                )
                .await;
                assert!(
                    observed.try_recv().is_err(),
                    "authentication polled the body"
                );
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn authorized_body_deadline_is_independent_of_generation_timeout() {
        let (_directory, state, bearer) = ingress_fixture();
        let mut app = router(state);
        let started = Instant::now();
        let request = inference_request("/v1/embeddings", Some(&bearer), incomplete_body(None));
        let response = tokio::time::timeout(
            BODY_READ_TIMEOUT + Duration::from_secs(1),
            app.call(request),
        )
        .await
        .expect("an authorized incomplete body must have its own deadline")
        .unwrap();
        assert_eq!(Instant::now() - started, BODY_READ_TIMEOUT);
        assert_api_error(
            response,
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            &bearer,
        )
        .await;
        let response = app
            .call(inference_request(
                "/v1/embeddings",
                Some(&bearer),
                Body::from("{"),
            ))
            .await
            .unwrap();
        assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_json", &bearer).await;
    }

    #[tokio::test(start_paused = true)]
    async fn body_reader_saturation_never_queues_and_reclaims_slots_on_all_errors() {
        let (_directory, state, bearer) = ingress_fixture();
        let mut app = router(state);
        let (started, mut observed) = mpsc::unbounded_channel();
        let mut readers = Vec::new();
        for _ in 0..MAX_BODY_READERS {
            let mut app = app.clone();
            let request = inference_request(
                "/v1/embeddings",
                Some(&bearer),
                incomplete_body(Some(started.clone())),
            );
            readers.push(tokio::spawn(
                async move { app.call(request).await.unwrap() },
            ));
            observed.recv().await.unwrap();
        }
        let request = inference_request("/v1/embeddings", Some(&bearer), incomplete_body(None));
        let response = tokio::time::timeout(Duration::from_millis(100), app.call(request))
            .await
            .expect("saturated body readers must reject rather than queue")
            .unwrap();
        assert_api_error(
            response,
            StatusCode::TOO_MANY_REQUESTS,
            "provider_saturated",
            &bearer,
        )
        .await;

        // A disconnected handler frees its permit immediately, without waiting
        // for the body deadline. Keep every other slot occupied throughout.
        let reader = readers.pop().unwrap();
        reader.abort();
        assert!(reader.await.unwrap_err().is_cancelled());
        let broken = Body::from_stream(futures_util::stream::once(async {
            Err::<Bytes, _>(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "private-body-detail",
            ))
        }));
        let response = app
            .call(inference_request("/v1/embeddings", Some(&bearer), broken))
            .await
            .unwrap();
        assert_api_error(
            response,
            StatusCode::BAD_REQUEST,
            "invalid_request_body",
            "private-body-detail",
        )
        .await;

        // No Content-Length: the actual streamed bytes must still be bounded.
        let oversized = Body::from_stream(futures_util::stream::iter([
            Ok::<_, std::io::Error>(Bytes::from(vec![b' '; MAX_REQUEST_BYTES])),
            Ok(Bytes::from_static(b"x")),
        ]));
        let response = app
            .call(inference_request(
                "/v1/embeddings",
                Some(&bearer),
                oversized,
            ))
            .await
            .unwrap();
        assert_api_error(
            response,
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request_body",
            &bearer,
        )
        .await;
        let response = app
            .call(inference_request(
                "/v1/embeddings",
                Some(&bearer),
                Body::from(vec![b' '; MAX_REQUEST_BYTES]),
            ))
            .await
            .unwrap();
        assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_json", &bearer).await;

        // Refill the released slot, proving neither transport/size/JSON errors
        // nor handler cancellation leaked it.
        let mut replacement_app = app.clone();
        let request = inference_request(
            "/v1/embeddings",
            Some(&bearer),
            incomplete_body(Some(started)),
        );
        readers.push(tokio::spawn(async move {
            replacement_app.call(request).await.unwrap()
        }));
        observed.recv().await.unwrap();
        let response = app
            .call(inference_request(
                "/v1/embeddings",
                Some(&bearer),
                Body::from("{"),
            ))
            .await
            .unwrap();
        assert_api_error(
            response,
            StatusCode::TOO_MANY_REQUESTS,
            "provider_saturated",
            &bearer,
        )
        .await;
        tokio::time::advance(BODY_READ_TIMEOUT).await;
        for reader in readers {
            assert_api_error(
                reader.await.unwrap(),
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                &bearer,
            )
            .await;
        }
        let response = app
            .call(inference_request(
                "/v1/embeddings",
                Some(&bearer),
                Body::from("{"),
            ))
            .await
            .unwrap();
        assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_json", &bearer).await;
    }

    #[test]
    fn credential_cap_is_payment_error_not_ledger_failure() {
        let error = ledger_error("spend_cap: credential budget exhausted".into());
        assert_eq!(error.status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(error.code, "credential_spend_cap");
        assert!(!error.message.contains("budget exhausted"));
    }
    #[test]
    fn sse_framing_handles_network_splits_and_crlf() {
        let mut decoder = SseDecoder::default();
        let mut events = vec![];
        for byte in b":heartbeat\r\n\r\ndata: {\"a\":\r\ndata: 1}\r\n\r\ndata: [DONE]\n\n" {
            if let Some(event) = decoder.byte(*byte).unwrap() {
                events.push(event);
            }
        }
        assert_eq!(events, vec![b"{\"a\":\n1}".to_vec(), b"[DONE]".to_vec()]);
    }
    #[test]
    fn sse_rejects_oversized_and_non_sse_events() {
        let mut decoder = SseDecoder::default();
        for _ in 0..backend::MAX_SSE_EVENT_BYTES {
            assert!(decoder.byte(b'a').is_ok());
        }
        assert!(decoder.byte(b'a').is_err());
        let mut decoder = SseDecoder::default();
        for byte in b"not-an-event" {
            decoder.byte(*byte).unwrap();
        }
        assert!(decoder.byte(b'\n').is_err());
    }
    #[test]
    fn idempotency_keys_are_bounded_and_path_safe() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "idempotency-key",
            HeaderValue::from_static("retry:account-1_2.3"),
        );
        assert_eq!(request_key(&headers).unwrap(), "retry:account-1_2.3");
        assert!(validate_id("../another/path").is_err());
        assert!(validate_id(&"a".repeat(129)).is_err());
        assert!(validate_id("").is_err());
    }
    #[tokio::test]
    async fn bounded_backpressure_remains_cancellable() {
        let (sender, _receiver) = mpsc::channel(1);
        sender.send(Bytes::from_static(b"first")).await.unwrap();
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        cancel_tx.send(true).unwrap();
        assert!(send_chunk(
            &sender,
            &mut cancel_rx,
            Instant::now() + Duration::from_secs(60),
            Bytes::from_static(b"second")
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn llama_streams_preserve_public_modes_hide_metadata_and_settle_only_valid_usage() {
        use axum::routing::post;
        let (_directory, mut state, bearer) = ingress_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let engine = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/apply-template", post(|| async { Json(json!({"prompt":"rendered"})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens":[10,11]})) }))
            .route("/v1/chat/completions", post(|Json(body): Json<Value>| async move {
                let output = match body["messages"][0]["content"].as_str() {
                    Some("overshoot") => 5,
                    Some("hidden-token") => 2,
                    _ => 1,
                };
                let events = [
                    json!({"id":"engine-response","object":"chat.completion.chunk","model":"model","created":1,
                        "choices":[{"index":0,"delta":{"content":"OK"},"finish_reason":null,
                            "logprobs":{"content":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5,
                                "top_logprobs":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5}]}]}}],
                        "timings":{"predicted_n":1}}),
                    json!({"id":"engine-response","object":"chat.completion.chunk","model":"model","created":1,
                        "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                    json!({"id":"engine-response","object":"chat.completion.chunk","model":"model","created":1,
                        "choices":[],"usage":{"prompt_tokens":2,"completion_tokens":output,"total_tokens":2+output}}),
                ];
                let mut body = String::new();
                for event in events { body.push_str(&format!("data: {event}\n\n")); }
                body.push_str("data: [DONE]\n\n");
                ([(header::CONTENT_TYPE, "text/event-stream")], body)
            }));
        let engine_task = tokio::spawn(async move { axum::serve(listener, engine).await.unwrap() });
        let inner = Arc::get_mut(&mut state).unwrap();
        Arc::get_mut(&mut inner.config).unwrap().models[0].engine_url = base;
        inner.qualified_models.write().unwrap().insert("model".into());
        let mut app = router(state);
        for (stream, prompt) in [(false, "normal"), (true, "normal"), (false, "hidden-token"), (false, "overshoot")] {
            let expected_tokens = if prompt == "hidden-token" { 2 } else { 1 };
            let response = app.call(inference_request("/v1/chat/completions", Some(&bearer), Body::from(
                json!({"model":"model","messages":[{"role":"user","content":prompt}],
                    "stream":stream,"max_tokens":4}).to_string()
            ))).await.unwrap();
            let expected = if prompt == "overshoot" { StatusCode::BAD_GATEWAY } else { StatusCode::OK };
            assert_eq!(response.status(), expected);
            let id = response.headers()["x-request-id"].to_str().unwrap().to_owned();
            let bytes = axum::body::to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await.unwrap();
            let public = std::str::from_utf8(&bytes).unwrap();
            let events: Vec<Value> = if stream {
                public.lines().filter_map(|line| line.strip_prefix("data: "))
                    .filter(|data| *data != "[DONE]")
                    .map(|data| serde_json::from_str(data).unwrap()).collect()
            } else { vec![serde_json::from_slice(&bytes).unwrap()] };
            for event in &events {
                for private in ["token_ids", "timings", "__verbose"] {
                    assert!(event.get(private).is_none(), "private metadata: {private}");
                }
                if let Some(choices) = event["choices"].as_array() {
                    for choice in choices {
                        assert!(choice["logprobs"].is_null(), "unrequested sampled probabilities leaked");
                        assert!(choice.get("token_ids").is_none());
                    }
                }
            }
            if prompt != "overshoot" {
                if stream {
                    assert!(public.contains("[DONE]"));
                    assert!(!public.contains("\"usage\""));
                } else {
                    let value: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(value["choices"][0]["message"]["content"], "OK");
                    assert_eq!(value["usage"]["completion_tokens"], expected_tokens);
                }
            }
            let status = app.call(Request::builder().uri(format!("/v1/requests/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(status.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(status.into_body(), 16_384).await.unwrap();
            let settled: Value = serde_json::from_slice(&bytes).unwrap();
            if prompt == "overshoot" {
                assert_eq!(settled["status"], "failed_unknown_usage");
                assert_eq!(settled["usage"]["completion_tokens"], 0);
                assert_eq!(settled["cost_wei"], "0");
            } else {
                assert_eq!(settled["status"], "completed");
                assert_eq!(settled["usage"]["prompt_tokens"], 2);
                assert_eq!(settled["usage"]["completion_tokens"], expected_tokens);
                if prompt == "hidden-token" {
                    assert!(settled["performance"]["decode_us"].is_null());
                } else {
                    assert!(settled["performance"]["decode_us"].as_u64().is_some());
                }
            }
        }
        for path in ["/props", "/tokenize", "/apply-template", "/completion", "/slots"] {
            let response = app.call(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        engine_task.abort();
    }

    /// A qualified, profiled agreement served by the real HTTP/1 stack on a TCP
    /// listener, with the delaying engine below.
    struct DisconnectFixture {
        directory: tempfile::TempDir,
        state: Arc<ProviderState>,
        bearer: String,
        contract: String,
        model: protocol::ModelSpec,
        gateway: std::net::SocketAddr,
        engine_events: mpsc::UnboundedReceiver<&'static str>,
        tasks: [tokio::task::JoinHandle<()>; 2],
    }
    impl Drop for DisconnectFixture {
        fn drop(&mut self) {
            for task in &self.tasks {
                task.abort();
            }
        }
    }

    /// Engine whose chat prompt `headers:<µs>` delays the engine response itself
    /// and `token:<µs>` delays the first token. The stream then stays open, so
    /// only the gateway can end the engine request. Each engine request reports
    /// `first_token` when it emits one and `dropped` when the gateway abandons it.
    async fn delayed_engine(events: mpsc::UnboundedSender<&'static str>, body: Value) -> Response {
        struct InFlight(mpsc::UnboundedSender<&'static str>);
        impl Drop for InFlight {
            fn drop(&mut self) {
                let _ = self.0.send("dropped");
            }
        }
        let in_flight = InFlight(events.clone());
        let plan = body["messages"][0]["content"].as_str().unwrap().to_owned();
        let (phase, micros) = plan.split_once(':').unwrap();
        let delay = Duration::from_micros(micros.parse().unwrap());
        if phase == "headers" {
            tokio::time::sleep(delay).await;
        }
        let token_delay = if phase == "token" { delay } else { Duration::ZERO };
        let first = json!({"id":"engine-response","object":"chat.completion.chunk","model":"model","created":1,
            "choices":[{"index":0,"delta":{"content":"OK"},"finish_reason":null,
                "logprobs":{"content":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5,
                    "top_logprobs":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5}]}]}}]});
        let stream = futures_util::stream::once(async move {
            tokio::time::sleep(token_delay).await;
            let _ = events.send("first_token");
            Ok::<_, Infallible>(Bytes::from(format!("data: {first}\n\n")))
        })
        .chain(futures_util::stream::pending())
        .map(move |event| {
            // The in-flight marker lives exactly as long as the engine stream.
            std::hint::black_box(&in_flight);
            event
        });
        ([(header::CONTENT_TYPE, "text/event-stream")], Body::from_stream(stream)).into_response()
    }

    async fn disconnect_fixture() -> DisconnectFixture {
        let (directory, mut state, _) = ingress_fixture();
        let (events, engine_events) = mpsc::unbounded_channel();
        let engine_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let engine_url = format!("http://{}", engine_listener.local_addr().unwrap());
        let engine = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/apply-template", post(|| async { Json(json!({"prompt":"rendered"})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens":[10,11]})) }))
            .route("/v1/chat/completions", post(move |Json(body): Json<Value>| delayed_engine(events.clone(), body)));
        let engine_task = tokio::spawn(async move { axum::serve(engine_listener, engine).await.unwrap() });
        let inner = Arc::get_mut(&mut state).unwrap();
        let config = Arc::get_mut(&mut inner.config).unwrap();
        // Enough concurrency and arrival credit that sequential requests are never refused.
        let mut profile = crate::performance::test_profile();
        profile.max_concurrency = 4;
        config.models[0].max_concurrency = 4;
        config.models[0].engine_url = engine_url;
        config.models[0].model.serving = Some(profile);
        let model = config.models[0].model.clone();
        inner.model_slots = ProviderState::slots(&inner.config);
        inner.qualified_models.write().unwrap().insert(model.id.clone());
        let now = protocol::now().unwrap();
        let quote = protocol::sign(protocol::QUOTE_DOMAIN, protocol::Quote {
            id: protocol::random_id(), network: inner.config.network.identity.clone(),
            offer_id: protocol::random_id(), provider_wallet: inner.wallet.clone(),
            consumer_wallet: crate::wallet::address_from_private_key(&"22".repeat(32)).unwrap(),
            model: model.clone(), price: inner.config.models[0].price.clone(),
            funding_wei: "0".into(), platform_wallet: inner.config.platform_wallet.clone(),
            fee_bps: 50, max_concurrency: 1, nonce: protocol::random_id(),
            issued_at: now, expires_at: now + 300,
        }, &inner.private_key).unwrap();
        inner.ledger.store_quote(&quote, now).unwrap();
        let opened = inner.ledger.open_account(&quote, None, now).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway = listener.local_addr().unwrap();
        let app = router(state.clone());
        let gateway_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        DisconnectFixture {
            directory, state, model, gateway, engine_events,
            bearer: opened.credential.unwrap().bearer,
            contract: quote.payload.id,
            tasks: [engine_task, gateway_task],
        }
    }

    impl DisconnectFixture {
        /// Writes one complete HTTP/1.1 request on a real socket. Dropping the
        /// returned socket closes the connection, as a departing client does.
        async fn send(&self, key: &str, content: &str, stream: bool) -> tokio::net::TcpStream {
            use tokio::io::AsyncWriteExt;
            let body = json!({"model":"model","messages":[{"role":"user","content":content}],
                "stream":stream,"max_tokens":4}).to_string();
            let mut socket = tokio::net::TcpStream::connect(self.gateway).await.unwrap();
            socket.write_all(format!(
                "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\
                 Content-Type: application/json\r\nIdempotency-Key: {key}\r\nContent-Length: {}\r\n\r\n{body}",
                self.gateway, self.bearer, body.len(),
            ).as_bytes()).await.unwrap();
            socket
        }

        async fn settled(&self, key: &str) -> RequestView {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok(view) = self.state.ledger.request_view(&self.contract, key) {
                    if view.status != "reserved" {
                        return view;
                    }
                }
                assert!(Instant::now() < deadline, "request {key} never settled");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        async fn engine_event(&mut self) -> &'static str {
            tokio::time::timeout(Duration::from_secs(10), self.engine_events.recv())
                .await.expect("engine event").unwrap()
        }

        fn observations(&self) -> Vec<(String, bool)> {
            let db = rusqlite::Connection::open_with_flags(
                self.directory.path().join("ledger.sqlite"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            ).unwrap();
            let mut query = db.prepare("SELECT request,miss FROM performance_observations ORDER BY sequence").unwrap();
            let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
            rows.collect::<Result<_, _>>().unwrap()
        }

        fn assert_still_qualified(&self) {
            assert!(!self.state.ledger.performance_withdrawn(&self.model).unwrap());
            assert!(self.state.qualified_model(&self.model).is_some());
        }
    }

    /// The gateway has not answered: the request is still in progress.
    fn assert_unanswered(socket: &tokio::net::TcpStream) {
        let mut buffer = [0; 512];
        match socket.try_read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("gateway answered before the disconnect: {other:?} {}",
                String::from_utf8_lossy(&buffer)),
        }
    }

    fn assert_client_interrupted(view: &RequestView) {
        assert_eq!(view.status, "disconnected_unknown_usage");
        assert_eq!((view.usage.prompt_tokens, view.usage.completion_tokens), (0, 0));
        let performance = view.performance.as_ref().unwrap();
        assert_eq!(performance.eligibility, "client_interrupted");
        assert_eq!(performance.attained, None);
        assert_eq!(performance.remedy, "none");
        assert_eq!(performance.ttft_us, None);
        assert_eq!(view.waived_wei, "0");
        // Settled at the disconnect (about 500,000 µs), before any measured miss.
        let elapsed = performance.elapsed_us.unwrap();
        assert!((250_000..crate::performance::TTFT_US).contains(&elapsed), "elapsed {elapsed} µs");
    }

    #[tokio::test]
    async fn non_streaming_disconnects_before_a_miss_are_client_interruptions_and_never_latch() {
        let mut fixture = disconnect_fixture().await;
        // Three in a row would latch withdrawal if each counted as a provider miss.
        for attempt in 0..3 {
            let key = format!("non-streaming-disconnect-{attempt}");
            // The engine responds at once but holds its first token for 10 s.
            let socket = fixture.send(&key, "token:10000000", false).await;
            tokio::time::sleep(Duration::from_micros(500_000)).await;
            assert_unanswered(&socket);
            drop(socket);
            let view = fixture.settled(&key).await;
            assert_client_interrupted(&view);
            // The disconnect cancelled the engine request; it did not run to its deadline.
            assert_eq!(fixture.engine_event().await, "dropped");
            assert!(fixture.observations().is_empty());
            fixture.assert_still_qualified();
        }
    }

    #[tokio::test]
    async fn disconnect_before_the_engine_responds_is_a_client_interruption_in_both_modes() {
        let mut fixture = disconnect_fixture().await;
        for stream in [false, true] {
            let key = format!("dispatch-disconnect-{stream}");
            // The engine itself does not respond for 10 s.
            let socket = fixture.send(&key, "headers:10000000", stream).await;
            tokio::time::sleep(Duration::from_micros(500_000)).await;
            assert_unanswered(&socket);
            drop(socket);
            assert_client_interrupted(&fixture.settled(&key).await);
            assert_eq!(fixture.engine_event().await, "dropped");
        }
        assert!(fixture.observations().is_empty());
        fixture.assert_still_qualified();
    }

    #[tokio::test]
    async fn a_measured_miss_survives_a_later_non_streaming_disconnect() {
        let mut fixture = disconnect_fixture().await;
        let key = "late-first-token-then-disconnect";
        let socket = fixture.send(key, "token:2100000", false).await;
        assert_eq!(fixture.engine_event().await, "first_token");
        // Let the gateway record the token; the engine stream stays open.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(fixture.engine_events.try_recv().is_err(), "engine request ended early");
        assert_unanswered(&socket);
        drop(socket);
        let view = fixture.settled(key).await;
        assert_eq!(view.status, "disconnected_unknown_usage");
        let performance = view.performance.as_ref().unwrap();
        assert_eq!(performance.eligibility, "eligible");
        assert_eq!(performance.attained, Some(false));
        assert_eq!(performance.remedy, "inference_waiver");
        assert!(performance.ttft_us.unwrap() >= 2_100_000, "{performance:?}");
        assert_eq!(fixture.observations(), vec![(view.id.clone(), true)]);
        assert_eq!(fixture.engine_event().await, "dropped");
        // One miss does not latch withdrawal.
        fixture.assert_still_qualified();
    }
    async fn serve_engine(engine: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        (base, tokio::spawn(async move { axum::serve(listener, engine).await.unwrap() }))
    }

    fn keyed(path: &str, bearer: &str, key: &'static str, body: Value) -> Request {
        let mut request = inference_request(path, Some(bearer), Body::from(body.to_string()));
        request.headers_mut().insert("idempotency-key", HeaderValue::from_static(key));
        request
    }

    async fn request_status_of(app: &mut Router, bearer: &str, id: &str) -> Value {
        let response = app.call(Request::builder().uri(format!("/v1/requests/{id}"))
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 16_384).await.unwrap()).unwrap()
    }

    /// public-inference-api requirements 6 and 10 (task 2.2, design code difference 2): an
    /// embedding request naming dimensions the deployment does not produce is refused as an
    /// unsupported option before tokenization and reservation, and the engine is not asked
    /// for it. The native dimension is the one readiness measured; while none is recorded,
    /// any `dimensions` is refused. Requests without `dimensions` are unaffected.
    #[tokio::test]
    async fn unproducible_embedding_dimensions_are_refused_before_tokenization_and_reservation() {
        use axum::routing::post;
        use base64::Engine as _;
        use std::sync::atomic::AtomicUsize;
        let (_directory, mut state, bearer) = ingress_fixture_for(|model| {
            model.capabilities = vec!["embeddings".into()];
            model.max_output_tokens = 0;
        });
        let tokenize_calls = Arc::new(AtomicUsize::new(0));
        let embedding_calls = Arc::new(AtomicUsize::new(0));
        let (tokenized, embedded) = (tokenize_calls.clone(), embedding_calls.clone());
        let props = json!({"build_info":"b1-cd959ba","total_slots":1,"model_alias":"model",
            "default_generation_settings":{"n_ctx":4096}});
        // Two-dimensional vectors whatever `dimensions` asks for.
        let engine = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/v1/models", get(|| async { Json(json!({"data":[{"id":"model"}]})) }))
            .route("/props", get(move || async move { Json(props) }))
            .route("/tokenize", post(move || {
                tokenized.fetch_add(1, Ordering::SeqCst);
                async { Json(json!({"tokens":[1,2]})) }
            }))
            .route("/v1/embeddings", post(move |Json(body): Json<Value>| {
                embedded.fetch_add(1, Ordering::SeqCst);
                async move {
                    let inputs = body["input"].as_array().cloned().unwrap_or_default();
                    let tokens: usize = inputs.iter().map(|i| i.as_array().map_or(0, Vec::len)).sum();
                    let embedding = if body["encoding_format"] == "base64" {
                        json!(base64::engine::general_purpose::STANDARD
                            .encode([0.5f32.to_le_bytes(), 0.25f32.to_le_bytes()].concat()))
                    } else {
                        json!([0.5, 0.25])
                    };
                    let data: Vec<_> = (0..inputs.len()).map(|index| json!({"index":index,
                        "object":"embedding","embedding":embedding})).collect();
                    Json(json!({"model":body["model"],"object":"list","data":data,
                        "usage":{"prompt_tokens":tokens,"total_tokens":tokens}}))
                }
            }));
        let (base, engine_task) = serve_engine(engine).await;
        let inner = Arc::get_mut(&mut state).unwrap();
        Arc::get_mut(&mut inner.config).unwrap().models[0].engine_url = base;
        let mut app = router(state.clone());
        let contract = state.ledger.authenticate(&bearer, protocol::now().unwrap()).unwrap()
            .account.contract_id;
        let request = |key: &'static str, dimensions: Option<u64>| {
            let mut body = json!({"model":"model","input":["a","b"]});
            if let Some(dimensions) = dimensions {
                body["dimensions"] = json!(dimensions);
            }
            keyed("/v1/embeddings", &bearer, key, body)
        };
        let calls = || (tokenize_calls.load(Ordering::SeqCst), embedding_calls.load(Ordering::SeqCst));
        let unknown_usage = || state.ledger.list_requests().unwrap().iter()
            .filter(|r| r.contract_id == contract && r.status.ends_with("unknown_usage")).count();

        // Ready, but no dimension recorded: any `dimensions` is refused.
        state.qualified_models.write().unwrap().insert("model".into());
        let before = calls();
        let response = app.call(request("unrecorded", Some(2))).await.unwrap();
        assert_api_error(response, StatusCode::BAD_REQUEST, "unsupported_parameter", &bearer).await;
        assert_eq!(calls(), before);
        assert!(state.ledger.request_view(&contract, "unrecorded").is_err());

        // Readiness's real embedding probes measure and record the native dimension.
        let health = backend::model_health(&state).await;
        assert!(health[0].ready, "{health:?}");
        state.record_readiness(&health).unwrap();
        assert_eq!(*state.embedding_dimensions.read().unwrap(), HashMap::from([("model".to_owned(), 2)]));

        let account = serde_json::to_value(state.ledger.account(&contract).unwrap()).unwrap();
        let before = calls();
        let response = app.call(request("reduced", Some(1))).await.unwrap();
        assert_api_error(response, StatusCode::BAD_REQUEST, "unsupported_parameter", &bearer).await;
        // No `/tokenize` or `/v1/embeddings` call, no request record, no account change.
        assert_eq!(calls(), before);
        assert!(state.ledger.request_view(&contract, "reduced").is_err());
        assert_eq!(unknown_usage(), 0);
        assert_eq!(serde_json::to_value(state.ledger.account(&contract).unwrap()).unwrap(), account);

        for (key, dimensions) in [("unnamed", None), ("native", Some(2)), ("recorded-default", None)] {
            let response = app.call(request(key, dimensions)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{key}");
            let value: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await.unwrap()).unwrap();
            let data = value["data"].as_array().unwrap();
            assert_eq!(data.len(), 2, "{key}");
            assert!(data.iter().all(|item| item["embedding"].as_array().unwrap().len() == 2), "{key}");
            assert_eq!(state.ledger.request_view(&contract, key).unwrap().status, "completed", "{key}");
        }
        assert_eq!(unknown_usage(), 0);
        engine_task.abort();
    }

    /// A chat engine that streams six output events, each with one token ID. For the
    /// prompt `finish` it then finishes with exact usage and `[DONE]`; otherwise its
    /// stream stays open, so only the gateway can end the request.
    fn six_event_engine() -> Router {
        use axum::routing::post;
        Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/apply-template", post(|| async { Json(json!({"prompt":"rendered"})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens":[10,11]})) }))
            .route("/v1/chat/completions", post(|Json(body): Json<Value>| async move {
                let chunk = |choices: Value| json!({"id":"engine-response","object":"chat.completion.chunk",
                    "model":"model","created":1,"choices":choices});
                let mut events: Vec<Value> = (0..6).map(|index| chunk(json!([{"index":0,
                    "delta":{"content":format!("t{index}")},"finish_reason":null,
                    "logprobs":{"content":[{"id":40 + index,"token":format!("t{index}"),
                        "bytes":[116],"logprob":-0.5,"top_logprobs":[]}]}}]))).collect();
                let finish = body["messages"][0]["content"] == "finish";
                if finish {
                    events.push(chunk(json!([{"index":0,"delta":{},"finish_reason":"stop"}])));
                    let mut usage = chunk(json!([]));
                    usage["usage"] = json!({"prompt_tokens":2,"completion_tokens":6,"total_tokens":8});
                    events.push(usage);
                }
                let mut text: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
                if finish {
                    text.push_str("data: [DONE]\n\n");
                }
                let head = futures_util::stream::once(async move { Ok::<_, Infallible>(Bytes::from(text)) });
                let body = if finish {
                    Body::from_stream(head)
                } else {
                    Body::from_stream(head.chain(futures_util::stream::pending()))
                };
                ([(header::CONTENT_TYPE, "text/event-stream")], body)
            }))
    }

    /// Reads a response body to its end: its text, and whether it ended with an error
    /// rather than cleanly.
    async fn read_to_end(response: Response) -> (String, bool) {
        let mut body = response.into_body().into_data_stream();
        let mut text = String::new();
        while let Some(frame) = tokio::time::timeout(Duration::from_secs(10), body.next())
            .await
            .expect("the body neither ended nor failed")
        {
            match frame {
                Ok(bytes) => text.push_str(std::str::from_utf8(&bytes).unwrap()),
                Err(_) => return (text, true),
            }
        }
        (text, false)
    }

    /// public-inference-api requirement 6 (task 2.3, design code difference 3): a stream
    /// that fails after its output began does not end like a completed stream, even when
    /// the client stopped reading and the bounded queue was full when it failed. The
    /// router runs in-process, so no socket buffer drains the queue.
    #[tokio::test]
    async fn a_stream_that_fails_while_the_client_is_not_reading_does_not_end_cleanly() {
        let (base, engine_task) = serve_engine(six_event_engine()).await;
        let streaming = |content: &str| json!({"model":"model","messages":[{"role":"user","content":content}],
            "stream":true,"max_tokens":8});

        // Control: polled promptly against an engine that finishes, the stream ends with
        // `[DONE]` and a clean end, and the request is completed.
        let (_control_directory, mut state, bearer) = ingress_fixture();
        let inner = Arc::get_mut(&mut state).unwrap();
        Arc::get_mut(&mut inner.config).unwrap().models[0].engine_url = base.clone();
        inner.qualified_models.write().unwrap().insert("model".into());
        let mut app = router(state);
        let response = app.call(keyed("/v1/chat/completions", &bearer, "finishes", streaming("finish"))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let id = response.headers()["x-request-id"].to_str().unwrap().to_owned();
        let (text, ended_abnormally) = read_to_end(response).await;
        assert!(!ended_abnormally, "{text}");
        assert!(text.ends_with("data: [DONE]\n\n"), "{text}");
        assert!((0..6).all(|index| text.contains(&format!("\"t{index}\""))), "{text}");
        assert_eq!(request_status_of(&mut app, &bearer, &id).await["status"], "completed");

        // The engine streams six events and stays open; the generation deadline is 1 s.
        let (_directory, mut state, bearer) = ingress_fixture();
        let inner = Arc::get_mut(&mut state).unwrap();
        let config = &mut Arc::get_mut(&mut inner.config).unwrap().models[0];
        config.engine_url = base;
        config.request_timeout_secs = 1;
        inner.qualified_models.write().unwrap().insert("model".into());
        let mut app = router(state);
        let response = app.call(keyed("/v1/chat/completions", &bearer, "stalled", streaming("hold"))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let id = response.headers()["x-request-id"].to_str().unwrap().to_owned();
        // The body is not polled: four events fill the queue, and the fifth waits until
        // the deadline fails the request.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = request_status_of(&mut app, &bearer, &id).await["status"].clone();
            if status == "timed_out_unknown_usage" {
                break;
            }
            assert_eq!(status, "reserved");
            assert!(Instant::now() < deadline, "the request never reached its deadline");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let (text, ended_abnormally) = read_to_end(response).await;
        assert!((0..4).all(|index| text.contains(&format!("\"t{index}\""))), "{text}");
        assert!(
            ended_abnormally || text.contains("\"code\":\"engine_timeout\""),
            "the failed stream ended cleanly with only output events: {text}"
        );
        engine_task.abort();
    }

    /// public-inference-api requirement 6 (task 2.4, design code difference 4): a
    /// completion prompt that the engine prepares into no tokens is refused before
    /// reservation as invalid input, not as a context overflow.
    #[tokio::test]
    async fn a_prompt_prepared_into_no_tokens_is_refused_as_invalid_input_before_reservation() {
        use axum::routing::post;
        use std::sync::atomic::AtomicUsize;
        let (_directory, mut state, bearer) =
            ingress_fixture_for(|model| model.capabilities = vec!["completions".into()]);
        let generations = Arc::new(AtomicUsize::new(0));
        let generated = generations.clone();
        let engine = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/tokenize", post(|| async { Json(json!({"tokens":[]})) }))
            .route("/v1/completions", post(move || {
                generated.fetch_add(1, Ordering::SeqCst);
                async { StatusCode::INTERNAL_SERVER_ERROR }
            }));
        let (base, engine_task) = serve_engine(engine).await;
        let inner = Arc::get_mut(&mut state).unwrap();
        Arc::get_mut(&mut inner.config).unwrap().models[0].engine_url = base;
        inner.qualified_models.write().unwrap().insert("model".into());
        let mut app = router(state.clone());
        let contract = state.ledger.authenticate(&bearer, protocol::now().unwrap()).unwrap()
            .account.contract_id;
        for stream in [false, true] {
            let key = if stream { "empty-streamed" } else { "empty" };
            let response = app.call(keyed("/v1/completions", &bearer, key,
                json!({"model":"model","prompt":"","max_tokens":1,"stream":stream}))).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{key}");
            let error: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
            assert_eq!(error["error"]["code"], "unsupported_parameter", "{key}");
            assert!(!error["error"]["message"].as_str().unwrap().contains("context"), "{error}");
            assert!(state.ledger.request_view(&contract, key).is_err(), "{key}");
        }
        assert_eq!(generations.load(Ordering::SeqCst), 0);
        assert!(state.ledger.list_requests().unwrap().is_empty());
        engine_task.abort();
    }

    /// One request to a bearer-authorized public route: `models`, `chat`, `completions`,
    /// `embeddings`, `status` or `cancel`. `key` is the request key of an inference request
    /// and the request named by `status` and `cancel`.
    fn bearer_route(route: &str, bearer: &str, key: &str) -> Request {
        let (method, path, body) = match route {
            "models" => ("GET", "/v1/models".to_string(), None),
            "chat" => ("POST", "/v1/chat/completions".into(),
                Some(json!({"model":"model","messages":[{"role":"user","content":"complete"}],"max_tokens":4}))),
            "completions" => ("POST", "/v1/completions".into(),
                Some(json!({"model":"model","prompt":"complete","max_tokens":4}))),
            "embeddings" => ("POST", "/v1/embeddings".into(), Some(json!({"model":"model","input":"complete"}))),
            "status" => ("GET", format!("/v1/requests/{key}"), None),
            "cancel" => ("POST", format!("/v1/requests/{key}/cancel"), None),
            _ => unreachable!("{route}"),
        };
        let mut request = Request::builder().method(method).uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json").header("idempotency-key", key);
        }
        request.body(body.map_or_else(Body::empty, |body| Body::from(body.to_string()))).unwrap()
    }

    /// account-credentials requirement 3 (task 3.3) through the public inference router,
    /// with a qualified model, an engine that counts every request it receives, and a
    /// `read`-only, an `infer`-only, a revoked and an expired credential of the account:
    /// - the `read`-only bearer is refused for missing scope on `GET /v1/models`, the three
    ///   inference routes and `POST /v1/requests/:id/cancel` of an in-flight request of its
    ///   own account: nothing is reserved, the engine receives nothing, and the in-flight
    ///   request is not cancelled;
    /// - the `infer`-only bearer is refused for missing scope on `GET /v1/requests/:id`;
    /// - the revoked bearer, and a bearer whose `expires_at` has passed or equals the
    ///   provider's current time, are refused as unauthenticated on all six routes, and no
    ///   account, credential, request or engine state changes; their cancellation of the
    ///   in-flight request signals nothing.
    ///
    /// Positive controls show that the same fixture reserves, reads and cancels for the
    /// scopes that allow it. 403 `credential_scope` and 401 `invalid_api_key` are current
    /// implementation values (design decision 4). The exact-expiry case restores the
    /// record instead of sleeping: a call counts only when the clock reads `expires_at`
    /// both before and after it, so the provider's own reading was `expires_at`.
    #[tokio::test]
    async fn bearers_outside_their_scope_or_lifetime_change_nothing_on_any_route() {
        use crate::billing_store::CredentialIssueRequest;
        use axum::routing::post;
        use std::sync::atomic::AtomicUsize;
        const ROUTES: [&str; 6] = ["models", "chat", "completions", "embeddings", "status", "cancel"];
        let (directory, mut state, root) = ingress_fixture();
        let engine_requests = Arc::new(AtomicUsize::new(0));
        let counted = engine_requests.clone();
        let (events, mut engine_events) = mpsc::unbounded_channel();
        let engine = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/apply-template", post(|| async { Json(json!({"prompt":"rendered"})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens":[10,11]})) }))
            .route("/v1/chat/completions", post(move |Json(body): Json<Value>| {
                let events = events.clone();
                async move {
                    if body["messages"][0]["content"] == "token:0" {
                        // One token, then the stream stays open: the request is in flight.
                        return delayed_engine(events, body).await;
                    }
                    let chunk = |choices: Value| json!({"id":"engine-response","object":"chat.completion.chunk",
                        "model":"model","created":1,"choices":choices});
                    let mut usage = chunk(json!([]));
                    usage["usage"] = json!({"prompt_tokens":2,"completion_tokens":1,"total_tokens":3});
                    let events = [
                        chunk(json!([{"index":0,"delta":{"content":"OK"},"finish_reason":null,
                            "logprobs":{"content":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5,
                                "top_logprobs":[{"id":42,"token":"OK","bytes":[79,75],"logprob":-0.5}]}]}}])),
                        chunk(json!([{"index":0,"delta":{},"finish_reason":"stop"}])),
                        usage,
                    ];
                    let mut text: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
                    text.push_str("data: [DONE]\n\n");
                    ([(header::CONTENT_TYPE, "text/event-stream")], text).into_response()
                }
            }))
            .layer(axum::middleware::from_fn(move |request: Request, next: axum::middleware::Next| {
                let counted = counted.clone();
                async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                    next.run(request).await
                }
            }));
        let (base, engine_task) = serve_engine(engine).await;
        let inner = Arc::get_mut(&mut state).unwrap();
        Arc::get_mut(&mut inner.config).unwrap().models[0].engine_url = base;
        inner.qualified_models.write().unwrap().insert("model".into());
        let ledger = state.ledger.clone();
        let contract = ledger.authenticate(&root, protocol::now().unwrap()).unwrap().account.contract_id;
        let issue = |scopes: &[&str]| {
            let now = protocol::now().unwrap();
            ledger.issue_credential(&contract, &CredentialIssueRequest {
                expires_at: now + 3600, spend_cap_wei: None, max_concurrency: 1,
                scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
            }, now).unwrap()
        };
        let read = issue(&["read"]).bearer;
        let infer = issue(&["infer"]).bearer;
        let revoked = issue(&["infer", "read"]);
        ledger.revoke_credential(&contract, &revoked.id).unwrap();
        let revoked = revoked.bearer;
        let expired = issue(&["infer", "read"]);
        // Restores the expired credential's record with `expires_at` = `at`.
        let db = rusqlite::Connection::open(directory.path().join("ledger.sqlite")).unwrap();
        let expire = |at: u64| {
            assert_eq!(db.execute("UPDATE credentials SET record=json_set(record,'$.expires_at',?1) WHERE id=?2",
                rusqlite::params![at, expired.id]).unwrap(), 1);
        };
        let expired = expired.bearer.clone();
        let observed = || json!({
            "account": ledger.account(&contract).unwrap(),
            "credentials": ledger.credentials(&contract).unwrap(),
            "requests": ledger.requests(&contract, None).unwrap(),
            "engine_requests": engine_requests.load(Ordering::SeqCst),
        });
        let mut app = router(state.clone());

        let before = observed();
        assert_eq!(before["requests"], json!([]));
        for route in ["models", "chat", "completions", "embeddings"] {
            let response = app.call(bearer_route(route, &read, "read-only")).await.unwrap();
            assert_api_error(response, StatusCode::FORBIDDEN, "credential_scope", &read).await;
            assert_eq!(observed(), before, "read-only {route}");
        }
        expire(protocol::now().unwrap() - 60);
        let before = observed();
        for bearer in [&revoked, &expired] {
            for route in ROUTES {
                let response = app.call(bearer_route(route, bearer, "refused")).await.unwrap();
                assert_api_error(response, StatusCode::UNAUTHORIZED, "invalid_api_key", bearer).await;
                assert_eq!(observed(), before, "{route}");
            }
        }
        for route in ROUTES {
            let mut exact = false;
            for _ in 0..20 {
                let at = protocol::now().unwrap();
                expire(at);
                let before = observed();
                let response = app.call(bearer_route(route, &expired, "refused")).await.unwrap();
                let after = protocol::now().unwrap();
                assert_api_error(response, StatusCode::UNAUTHORIZED, "invalid_api_key", &expired).await;
                assert_eq!(observed(), before, "{route} at expiry");
                if after == at {
                    exact = true;
                    break;
                }
            }
            assert!(exact, "{route}: no call ran within the second of its expiry");
        }

        // Positive controls: `infer` reserves and completes; `read`, not `infer`, reads.
        let response = app.call(bearer_route("chat", &infer, "completed")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(ledger.request_view(&contract, "completed").unwrap().status, "completed");
        let engine_after_completion = engine_requests.load(Ordering::SeqCst);
        assert!(engine_after_completion > 0);
        let response = app.call(bearer_route("status", &infer, "completed")).await.unwrap();
        assert_api_error(response, StatusCode::FORBIDDEN, "credential_scope", &infer).await;
        assert_eq!(request_status_of(&mut app, &read, "completed").await["status"], "completed");

        // An in-flight request of the same account.
        let mut held_app = app.clone();
        let held_bearer = root.clone();
        let held = tokio::spawn(async move {
            held_app.call(keyed("/v1/chat/completions", &held_bearer, "held",
                json!({"model":"model","messages":[{"role":"user","content":"token:0"}],"max_tokens":4})))
                .await.unwrap()
        });
        let event = tokio::time::timeout(Duration::from_secs(10), engine_events.recv()).await;
        assert_eq!(event.unwrap().unwrap(), "first_token");
        assert_eq!(ledger.request_view(&contract, "held").unwrap().status, "reserved");
        let before = observed();
        for (bearer, status, code) in [
            (&read, StatusCode::FORBIDDEN, "credential_scope"),
            (&revoked, StatusCode::UNAUTHORIZED, "invalid_api_key"),
            (&expired, StatusCode::UNAUTHORIZED, "invalid_api_key"),
        ] {
            let response = app.call(bearer_route("cancel", bearer, "held")).await.unwrap();
            assert_api_error(response, status, code, bearer).await;
            // A signalled cancellation would settle the hold and end the engine request.
            assert!(tokio::time::timeout(Duration::from_millis(200), engine_events.recv()).await.is_err(),
                "{code}: the in-flight request was cancelled");
            assert_eq!(observed(), before, "{code}");
        }
        for (bearer, status, code) in [
            (&infer, StatusCode::FORBIDDEN, "credential_scope"),
            (&revoked, StatusCode::UNAUTHORIZED, "invalid_api_key"),
            (&expired, StatusCode::UNAUTHORIZED, "invalid_api_key"),
        ] {
            let response = app.call(bearer_route("status", bearer, "held")).await.unwrap();
            assert_api_error(response, status, code, bearer).await;
            assert_eq!(observed(), before, "{code}");
        }
        assert_eq!(request_status_of(&mut app, &read, "held").await["status"], "reserved");
        let response = app.call(bearer_route("cancel", &infer, "held")).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let event = tokio::time::timeout(Duration::from_secs(10), engine_events.recv()).await;
        assert_eq!(event.unwrap().unwrap(), "dropped");
        let held = tokio::time::timeout(Duration::from_secs(10), held).await.unwrap().unwrap();
        assert_api_error(held, StatusCode::CONFLICT, "request_cancelled", &root).await;
        assert_eq!(ledger.request_view(&contract, "held").unwrap().status, "cancelled_unknown_usage");
        engine_task.abort();
    }
}
