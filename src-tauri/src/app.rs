//! Provider orchestration. Only public inference/control routers reach the public listener.
use crate::file_lock::ExclusiveFileLock;
use crate::{
    billing_store::BillingStore,
    chain_client::{ChainClient, ChainError},
    contracts,
    dht::DhtService,
    error::ApiError,
    inference_backend::{self, ModelHealth},
    inference_gateway,
    offer_registry::OfferRegistry,
    protocol::{self, InferenceOffer, Signed},
    provider_config::{self, ProviderConfig},
    version, wallet,
};
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs::File,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex, RwLock};

pub struct ProviderState {
    pub config: Arc<ProviderConfig>,
    pub chain: Arc<ChainClient>,
    pub ledger: Arc<BillingStore>,
    pub private_key: Arc<String>,
    pub wallet: String,
    pub offers: Arc<OfferRegistry>,
    pub accepting: AtomicBool,
    pub funding_ready: AtomicBool,
    pub qualified_models: std::sync::RwLock<HashSet<String>>,
    /// Shared by all public routers and readiness probes; no hidden engine concurrency.
    pub model_slots: std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>,
    pub managed_engines: std::collections::HashMap<String, Arc<crate::engine_runtime::ManagedEngine>>,
}

impl ProviderState {
    pub fn engine_alive(&self, model_id: &str) -> bool {
        self.managed_engines.get(model_id).is_none_or(|engine| engine.is_alive())
    }
    pub fn slots(config: &ProviderConfig) -> std::collections::HashMap<String, Arc<tokio::sync::Semaphore>> {
        config.models.iter().map(|model| (
            model.model.id.clone(), Arc::new(tokio::sync::Semaphore::new(model.max_concurrency as usize)),
        )).collect()
    }
    pub fn qualified_model(&self, agreement: &protocol::ModelSpec) -> Option<&protocol::ModelSpec> {
        let current = &self.config.models.iter()
            .find(|configured| agreement.accepts_execution(&configured.model))?.model;
        (self.engine_alive(&current.id)
            && self.qualified_models.read().is_ok_and(|models| models.contains(&current.id))
            && self.ledger.performance_withdrawn(current).is_ok_and(|withdrawn| !withdrawn))
            .then_some(current)
    }
}

pub struct ProviderRuntime {
    pub state: Arc<ProviderState>,
    pub dht: Arc<DhtService>,
    health: RwLock<Vec<ModelHealth>>,
    chain_rejected: AtomicBool,
    chain_error: RwLock<Option<&'static str>>,
    maintenance_status: RwLock<contracts::MaintenanceReport>,
    refresh_error: RwLock<Option<String>>,
    draining: AtomicBool,
    admin_token: String,
    published: Mutex<HashSet<String>>,
    lifecycle: Mutex<()>,
    revision_file: PathBuf,
    revision: Mutex<u64>,
    _instance_lock: ExclusiveFileLock,
}

impl ProviderRuntime {
    pub async fn start(config: ProviderConfig) -> Result<Arc<Self>, String> {
        config.validate()?;
        // Before anything, and so before any engine, starts.
        config.verify_separate_engine_keys()?;
        let private_key = wallet::load_private_key(&config.signing_key_file)?;
        let wallet = wallet::address_from_private_key(&private_key)?;
        let offers = Arc::new(OfferRegistry::new(
            config.network.identity.clone(),
            config.development,
        ));
        let dht = Arc::new(
            DhtService::start(
                config.dht.clone(),
                config.network.identity.clone(),
                offers.clone(),
            )
            .await?,
        );
        // Deployments share the wallet nonce journal, not customer balances or locks.
        let data_dir = config.runtime_data_dir()?.join(&wallet).join(dht.peer_id());
        wallet::secure_directory(&data_dir)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let instance_lock = options
            .open(data_dir.join("provider.lock"))
            .map_err(|e| e.to_string())?;
        let instance_lock = ExclusiveFileLock::try_acquire(instance_lock)
            .map_err(|_| "another provider owns this deployment data directory")?;
        let admin_token = provider_config::read_secret(&config.admin_token_file)?;
        if admin_token.len() < 32 {
            return Err("administration token must contain at least 32 characters of cryptographic random data".into());
        }
        // Shared with consumer operations for the same network and signing wallet.
        let chain = Arc::new(ChainClient::new(
            config.network.clone(),
            config.data_dir.clone(),
        )?);
        match chain.identity_check().await {
            Ok(()) | Err(ChainError::Unavailable(_)) => {}
            Err(_) => return Err("configured RPC identity was rejected".into()),
        }
        let ledger = Arc::new(BillingStore::open(
            &data_dir.join("ledger.sqlite3"),
            &config.network.identity,
        )?);
        ledger.bind_provider(&wallet)?;
        ledger.recover_requests(protocol::now()?)?;
        let revision_file = data_dir.join("offer-revision");
        let revision = match std::fs::read_to_string(&revision_file) {
            Ok(value) => value
                .trim()
                .parse::<u64>()
                .map_err(|_| "invalid persisted offer revision")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(format!("cannot read offer revision: {e}")),
        };
        let mut managed_engines = std::collections::HashMap::new();
        for model in &config.models {
            if model.runtime.is_some() {
                let engine = crate::engine_runtime::ManagedEngine::start(model).await?;
                managed_engines.insert(model.model.id.clone(), engine);
            }
        }
        let state = Arc::new(ProviderState {
            model_slots: ProviderState::slots(&config),
            managed_engines,
            config: Arc::new(config),
            chain,
            ledger,
            private_key: Arc::new(private_key),
            wallet,
            offers,
            accepting: AtomicBool::new(false),
            funding_ready: AtomicBool::new(false),
            qualified_models: std::sync::RwLock::new(HashSet::new()),
        });
        let runtime = Arc::new(Self {
            state,
            dht,
            health: RwLock::new(Vec::new()),
            chain_rejected: AtomicBool::new(false),
            chain_error: RwLock::new(None),
            maintenance_status: RwLock::new(contracts::MaintenanceReport::default()),
            refresh_error: RwLock::new(None),
            draining: AtomicBool::new(false),
            admin_token,
            published: Mutex::new(HashSet::new()),
            lifecycle: Mutex::new(()),
            revision_file,
            revision: Mutex::new(revision),
            _instance_lock: instance_lock,
        });
        // A stopped engine leaves administration available, but never publishes a usable offer.
        runtime.refresh().await?;
        // Weak ownership avoids retaining child processes after the provider is dropped.
        let weak = Arc::downgrade(&runtime);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(runtime) = weak.upgrade() else { break };
                let failed: Vec<_> = runtime.state.managed_engines.iter()
                    .filter(|(_, engine)| !engine.is_alive()).map(|(id, _)| id.clone()).collect();
                if failed.is_empty() { continue; }
                if let Ok(mut qualified) = runtime.state.qualified_models.write() {
                    for id in &failed { qualified.remove(id); }
                }
                runtime.state.accepting.store(false, Ordering::Release);
                runtime.state.funding_ready.store(false, Ordering::Release);
                // Serialize against a refresh publishing stale readiness; no restart/retry.
                let _lifecycle = runtime.lifecycle.lock().await;
                runtime.state.accepting.store(false, Ordering::Release);
                runtime.state.funding_ready.store(false, Ordering::Release);
                for health in runtime.health.write().await.iter_mut() {
                    if failed.contains(&health.model_id) { health.ready = false; }
                }
                runtime.withdraw_published().await;
            }
        });
        Ok(runtime)
    }

    async fn next_revision(&self) -> Result<u64, String> {
        let mut current = self.revision.lock().await;
        let next = current.checked_add(1).ok_or("offer revision exhausted")?;
        let temporary = self.revision_file.with_extension("next");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        writeln!(file, "{next}").map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &self.revision_file).map_err(|e| e.to_string())?;
        if let Some(parent) = self.revision_file.parent() {
            File::open(parent)
                .and_then(|dir| dir.sync_all())
                .map_err(|e| e.to_string())?;
        }
        *current = next;
        Ok(next)
    }

    pub async fn refresh(&self) -> Result<usize, String> {
        let _lifecycle = self.lifecycle.lock().await;
        let result = self.refresh_inner().await;
        *self.refresh_error.write().await = result.as_ref().err().cloned();
        if result.is_err() {
            self.state.funding_ready.store(false, Ordering::Release);
            // Includes every offer the failed pass published before it failed.
            self.withdraw_published().await;
        }
        result
    }

    async fn refresh_inner(&self) -> Result<usize, String> {
        self.state.funding_ready.store(false, Ordering::Release);
        let health = inference_backend::model_health(&self.state).await;
        {
            let mut qualified = self
                .state
                .qualified_models
                .write()
                .map_err(|_| "model qualification lock unavailable")?;
            qualified.clear();
            qualified.extend(
                health
                    .iter()
                    .filter(|model| model.ready)
                    .map(|model| model.model_id.clone()),
            );
        }
        let (chain_ready, chain_error) = match self.state.chain.status().await {
            Ok(status)
                if status
                    .get("genesis")
                    .and_then(Value::as_str)
                    .is_some_and(|hash| {
                        !hash.eq_ignore_ascii_case(&self.state.config.network.identity.genesis_hash)
                    }) =>
            {
                self.chain_rejected.store(true, Ordering::Release);
                (
                    false,
                    Some("Configured RPC node-status genesis was rejected"),
                )
            }
            Ok(status) => {
                self.chain_rejected.store(false, Ordering::Release);
                let ready = status.get("syncing").and_then(Value::as_bool) == Some(false)
                    && status.get("acceptance").and_then(Value::as_str) == Some("safe")
                    && status.get("safe").is_some_and(Value::is_object);
                (
                    ready,
                    if ready {
                        None
                    } else {
                        Some(
                            "Canonical eSpace safe block is unavailable",
                        )
                    },
                )
            }
            Err(ChainError::Rejected(_)) => {
                self.chain_rejected.store(true, Ordering::Release);
                (false, Some("Configured RPC network identity was rejected"))
            }
            Err(_) => (
                false,
                Some("Configured RPC unavailable; retaining last observed funding states"),
            ),
        };
        *self.chain_error.write().await = chain_error;
        // Always revisit canonical evidence on recovery, before publishing offers.
        let report = match contracts::reconcile(self.state.clone()).await {
            Ok(report) => report,
            Err(_) => {
                self.state.accepting.store(false, Ordering::Release);
                return Err("Funding ledger reconciliation failed".into());
            }
        };
        let funding_safe = report.funding_error.is_none();
        if report.funding_rejected {
            self.chain_rejected.store(true, Ordering::Release);
        }
        *self.maintenance_status.write().await = report;
        let policy = version::effective_policy();
        let now = protocol::now().map_err(|error| {
            self.state.accepting.store(false, Ordering::Release);
            error
        })?;
        let version_allowed = (policy.valid_until == 0 || policy.valid_until > now)
            && version::compare_to_policy(version::CURRENT_VERSION, &policy)
                .is_ok_and(|status| status != "required");
        let accepting = !self.chain_rejected.load(Ordering::Acquire)
            && version_allowed
            && !self.draining.load(Ordering::Acquire)
            && self.state.managed_engines.values().all(|engine| engine.is_alive())
            && health.iter().any(|model| model.ready);
        *self.health.write().await = health;
        self.state.accepting.store(accepting, Ordering::Release);
        let funding_ready = accepting && chain_ready && funding_safe;
        self.state
            .funding_ready
            .store(funding_ready, Ordering::Release);
        let mut next = HashSet::new();
        if funding_ready {
            let revision = self
                .next_revision()
                .await
                .map_err(|_| "Persisting offer revision failed")?;
            for model in &self.state.config.models {
                if self.state.qualified_model(&model.model).is_none() {
                    continue;
                }
                let free =
                    model.price.input_wei_per_1k == "0" && model.price.output_wei_per_1k == "0";
                let offer = InferenceOffer {
                    network: self.state.config.network.identity.clone(),
                    provider_wallet: self.state.wallet.clone(),
                    peer_id: self.dht.peer_id(),
                    deployment_id: self.dht.peer_id(),
                    endpoint: self.state.config.public_endpoint.clone(),
                    model: model.model.clone(),
                    price: model.price.clone(),
                    min_funding_wei: if free {
                        "0".into()
                    } else {
                        self.state.config.min_funding_wei.clone()
                    },
                    platform_wallet: self.state.config.platform_wallet.clone(),
                    fee_bps: self.state.config.fee_bps,
                    max_concurrency: model.max_concurrency,
                    revision,
                    issued_at: now,
                    expires_at: now.checked_add(120).ok_or("offer expiry overflow")?,
                };
                let id =
                    protocol::offer_id(&offer).map_err(|_| "Computing offer identity failed")?;
                let signed = protocol::sign(protocol::OFFER_DOMAIN, offer, &self.state.private_key)
                    .map_err(|_| "Signing resource offer failed")?;
                if self.state.config.development && model.model.serving.is_none() {
                    // Local calibration only: do not announce an unqualified deployment.
                    self.state.offers.insert(signed)?;
                } else {
                    self.dht.publish(&signed).await.map_err(|_| "DHT offer publication failed")?;
                }
                // Published from here on: if a later model fails this pass, `refresh`
                // withdraws this offer with the earlier set, so none stays served.
                self.published.lock().await.insert(id.clone());
                next.insert(id);
            }
        }
        let mut published = self.published.lock().await;
        for id in published.difference(&next) {
            self.dht.withdraw(id).await;
        }
        *published = next;
        Ok(published.len())
    }

    pub async fn stop_engines(&self) {
        for engine in self.state.managed_engines.values() { engine.stop().await; }
    }

    pub async fn drain(&self) {
        self.draining.store(true, Ordering::Release);
        self.state.accepting.store(false, Ordering::Release);
        self.state.funding_ready.store(false, Ordering::Release);
        let _lifecycle = self.lifecycle.lock().await;
        // A refresh already in progress may have computed admission before the
        // drain flag changed. Reassert it after serializing with that refresh.
        self.state.accepting.store(false, Ordering::Release);
        self.state.funding_ready.store(false, Ordering::Release);
        let mut published = self.published.lock().await;
        for id in published.drain() {
            self.dht.withdraw(&id).await;
        }
    }

    pub fn maintenance(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let runtime = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                // Refresh reconciles funding and fees independently, and retains
                // concrete, sanitized maintenance failures for administration.
                let _ = runtime.refresh().await;
            }
        })
    }

    async fn withdraw_published(&self) {
        let mut published = self.published.lock().await;
        for id in published.drain() {
            self.dht.withdraw(&id).await;
        }
    }

    pub fn public_router(self: &Arc<Self>) -> Router {
        contracts::router(self.state.clone())
            .merge(inference_gateway::router(self.state.clone()))
            .merge(
                Router::new()
                    .route("/healthz", get(live))
                    .route("/readyz", get(ready))
                    .route("/api/version-policy", get(policy))
                    .with_state(self.clone()),
            )
            .fallback(not_found)
            .method_not_allowed_fallback(method_not_allowed)
            .layer(middleware::from_fn(public_headers))
    }

    pub fn admin_router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/admin/status", get(admin_status))
            .route("/admin/runtime", get(admin_runtime))
            .route("/admin/offers", get(admin_offers))
            .route("/admin/accounts", get(admin_accounts))
            .route("/admin/requests", get(admin_requests))
            .route("/admin/fees", get(admin_fees))
            .route("/admin/drain", post(admin_drain))
            .route("/admin/resume", post(admin_resume))
            .route("/admin/offers/refresh", post(admin_refresh))
            .route("/admin/fees/remit", post(admin_remit))
            .fallback(not_found)
            .method_not_allowed_fallback(method_not_allowed)
            .layer(middleware::from_fn_with_state(self.clone(), admin_auth))
            .with_state(self.clone())
    }
}

async fn live() -> Json<Value> {
    Json(json!({"status":"live"}))
}
async fn ready(State(runtime): State<Arc<ProviderRuntime>>) -> Response {
    let ready = runtime.state.accepting.load(Ordering::Acquire)
        && runtime.state.managed_engines.values().all(|engine| engine.is_alive())
        // Serving readiness deliberately does not imply new-funding readiness.
        && runtime.state.ledger.admin_summary().is_ok();
    (if ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE }, Json(json!({"ready":ready,"funding_ready":runtime.state.funding_ready.load(Ordering::Acquire)}))).into_response()
}
async fn admin_runtime(State(runtime): State<Arc<ProviderRuntime>>) -> Json<Value> {
    Json(json!({"models": runtime.state.config.models.iter().map(|model| {
        let engine = runtime.state.managed_engines.get(&model.model.id);
        json!({"model_id": model.model.id, "managed": engine.is_some(),
            "ready": engine.is_some_and(|engine| engine.is_alive()),
            "observation": engine.map(|engine| &engine.observation)})
    }).collect::<Vec<_>>()}))
}
async fn policy() -> Json<version::VersionPolicy> {
    Json(version::effective_policy())
}
async fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "Unknown API endpoint")
}
async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "HTTP method is not supported for this endpoint",
    )
}

async fn public_headers(request: Request, next: Next) -> Response {
    if let Some(raw) = request.headers().get("x-chiral-client-version") {
        let supported = raw
            .to_str()
            .ok()
            .and_then(|value| version::compare_to_policy(value, &version::effective_policy()).ok());
        match supported {
            Some("required") => {
                return ApiError::new(
                    StatusCode::UPGRADE_REQUIRED,
                    "upgrade_required",
                    "Upgrade the Chiral client before using this provider",
                )
                .into_response()
            }
            None => {
                return ApiError::bad_request("invalid_version", "Invalid Chiral client version")
                    .into_response()
            }
            _ => {}
        }
    }
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    response
}

async fn admin_auth(
    State(runtime): State<Arc<ProviderRuntime>>,
    request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    if request.headers().contains_key(header::ORIGIN)
        || !bool::from(token.as_bytes().ct_eq(runtime.admin_token.as_bytes()))
    {
        return ApiError::unauthorized("Local operator authentication is required").into_response();
    }
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

async fn admin_status(
    State(runtime): State<Arc<ProviderRuntime>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"accepting":runtime.state.accepting.load(Ordering::Acquire),
        "funding_ready":runtime.state.funding_ready.load(Ordering::Acquire),
        "chain_error":&*runtime.chain_error.read().await,
        "maintenance":&*runtime.maintenance_status.read().await,
        "refresh_error":&*runtime.refresh_error.read().await,
        "network":runtime.state.config.network.identity,"provider_wallet":runtime.state.wallet,
        "models":&*runtime.health.read().await,"dht":runtime.dht.status().await,
        "ledger":runtime.state.ledger.admin_summary()?}),
    ))
}
async fn admin_offers(State(runtime): State<Arc<ProviderRuntime>>) -> Json<Value> {
    let ids = runtime.published.lock().await;
    let offers: Vec<Signed<InferenceOffer>> = ids
        .iter()
        .filter_map(|id| runtime.state.offers.get(id))
        .collect();
    Json(json!({"offers":offers}))
}
async fn admin_accounts(
    State(runtime): State<Arc<ProviderRuntime>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"accounts":runtime.state.ledger.list_accounts()?}),
    ))
}
async fn admin_requests(
    State(runtime): State<Arc<ProviderRuntime>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"requests":runtime.state.ledger.list_requests()?}),
    ))
}
async fn admin_fees(State(runtime): State<Arc<ProviderRuntime>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(runtime.state.ledger.fee_summary()?))
}
async fn admin_drain(State(runtime): State<Arc<ProviderRuntime>>) -> Json<Value> {
    runtime.drain().await;
    Json(json!({"accepting":false,"offers_published":0}))
}
async fn admin_resume(
    State(runtime): State<Arc<ProviderRuntime>>,
) -> Result<Json<Value>, ApiError> {
    runtime.draining.store(false, Ordering::Release);
    let count = runtime.refresh().await.map_err(ApiError::internal)?;
    Ok(Json(
        json!({"accepting":runtime.state.accepting.load(Ordering::Acquire),"offers_published":count}),
    ))
}
async fn admin_refresh(
    State(runtime): State<Arc<ProviderRuntime>>,
) -> Result<Json<Value>, ApiError> {
    let count = runtime.refresh().await.map_err(ApiError::internal)?;
    Ok(Json(
        json!({"accepting":runtime.state.accepting.load(Ordering::Acquire),"offers_published":count}),
    ))
}
async fn admin_remit(State(runtime): State<Arc<ProviderRuntime>>) -> Result<Json<Value>, ApiError> {
    let result = contracts::remit_fees(runtime.state.clone(), true).await;
    runtime.maintenance_status.write().await.fee_error = result.as_ref().err().cloned();
    result.map_err(ApiError::internal)?;
    Ok(Json(runtime.state.ledger.fee_summary()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        network::NetworkConfig,
        protocol::{PriceSchedule, Quote},
        provider_config::{DhtConfig, ModelConfig},
    };

    const OPERATOR_TOKEN: &str = "private-test-operator-token-32-bytes";

    /// A development provider with signing key `11…11` whose RPC and engine
    /// both answer 503, with the server that answers for them.
    async fn unavailable_provider(
        directory: &std::path::Path,
    ) -> (ProviderConfig, tokio::task::JoinHandle<()>) {
        let offer = crate::offer_registry::tests::offer().payload;
        let key_file = directory.join("wallet.key");
        wallet::save_private_key(&key_file, &"11".repeat(32)).unwrap();
        let admin_file = directory.join("admin.token");
        wallet::create_secret_file(&admin_file)
            .unwrap()
            .write_all(OPERATOR_TOKEN.as_bytes())
            .unwrap();
        // Engine/RPC outage is deliberate: restoring a provider must not require either.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unavailable = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(|| async { StatusCode::SERVICE_UNAVAILABLE }),
            )
            .await
            .unwrap();
        });
        let config = ProviderConfig {
            network: NetworkConfig {
                identity: offer.network.clone(),
                rpc_url: unavailable.clone(),
                rpc_bearer_file: None,
                faucet_url: None,
                explorer_url: None,
                policy_url: None,
            },
            data_dir: directory.join("data"),
            signing_key_file: key_file,
            admin_token_file: admin_file,
            public_endpoint: "http://127.0.0.1:9419".into(),
            public_bind: "127.0.0.1:9419".parse().unwrap(),
            admin_bind: "127.0.0.1:9421".parse().unwrap(),
            platform_wallet: offer.platform_wallet.clone(),
            fee_bps: 50,
            min_funding_wei: "0".into(),
            models: vec![ModelConfig {
                model: offer.model.clone(),
                price: offer.price.clone(),
                engine_url: unavailable,
                engine_api_key_file: None,
                runtime: None,
                max_concurrency: 1,
                request_timeout_secs: 1,
                qualification_evidence_file: None,
            }],
            dht: DhtConfig {
                listen: "/ip4/127.0.0.1/tcp/0".into(),
                bootstrap: vec![],
                identity_file: directory.join("peer-a.key"),
            },
            development: true,
        };
        (config, server)
    }

    /// A chain RPC whose eSpace `safe` frontier is current, so a refresh finds the
    /// provider new-funding ready. Other methods answer 503.
    async fn ready_chain(
        request: Json<Value>,
    ) -> Response {
        let now = protocol::now().unwrap();
        let block = |height: u64, hash: String| {
            json!({"number": format!("0x{height:x}"), "hash": hash,
                "timestamp": format!("0x{:x}", now - (12 - height))})
        };
        let result = match request["method"].as_str().unwrap_or("") {
            "eth_chainId" => json!("0x47"),
            "eth_syncing" => json!(false),
            "eth_getBlockByNumber" => match request["params"][0].as_str().unwrap_or("") {
                "0x0" => block(0, crate::network::ESPACE_TESTNET_GENESIS.into()),
                "latest" => block(12, format!("0x{}", "55".repeat(32))),
                "safe" => block(10, format!("0x{}", "56".repeat(32))),
                "finalized" => block(6, format!("0x{}", "57".repeat(32))),
                _ => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
            },
            _ => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
        Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
    }

    /// A private engine serving one embeddings model well enough to pass its readiness
    /// probe: two tokens per input and one-dimensional embeddings.
    fn embeddings_engine(model: protocol::ModelSpec) -> Router {
        let id = model.id.clone();
        let props = json!({"build_info": "b1-cd959ba", "total_slots": 1, "model_alias": model.id,
            "default_generation_settings": {"n_ctx": model.context_length}});
        Router::new()
            .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
            .route("/v1/models", get(move || async move { Json(json!({"data": [{"id": id}]})) }))
            .route("/props", get(move || async move { Json(props) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens": [1, 2]})) }))
            .route("/v1/embeddings", post(|request: Json<Value>| async move {
                let inputs = request["input"].as_array().cloned().unwrap_or_default();
                let tokens: usize = inputs.iter().map(|i| i.as_array().map_or(0, Vec::len)).sum();
                // 0.5 as one little-endian float32.
                let embedding = if request["encoding_format"] == "base64" { json!("AAAAPw==") } else { json!([0.5]) };
                let data: Vec<_> = (0..inputs.len()).map(|index| json!({"index": index,
                    "object": "embedding", "embedding": embedding})).collect();
                Json(json!({"model": request["model"], "object": "list", "data": data,
                    "usage": {"prompt_tokens": tokens, "total_tokens": tokens}}))
            }))
    }

    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        (origin, tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }))
    }

    #[tokio::test]
    async fn failed_refresh_withdraws_the_offers_it_published() {
        // inference-offer-discovery requirement 11, task 2.2: a refresh that publishes
        // one model's offer and then fails on the next leaves no offer served.
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
        config.network.rpc_url = rpc;
        // Public embedding deployments, which a non-development provider publishes
        // through its DHT manifest without a serving profile.
        config.development = false;
        config.public_endpoint = "https://inference.example.org/".into();
        let template = config.models.remove(0);
        let mut engines = Vec::new();
        for id in ["embed-a", "embed-b"] {
            let mut model = template.clone();
            model.model.id = id.into();
            model.model.capabilities = vec!["embeddings".into()];
            model.model.max_output_tokens = 0;
            model.price.output_wei_per_1k = "0".into();
            let (engine, server) = serve(embeddings_engine(model.model.clone())).await;
            model.engine_url = engine;
            engines.push(server);
            config.models.push(model);
        }
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let offers = |runtime: Arc<ProviderRuntime>| async move {
            let response = runtime.admin_router().oneshot(Request::builder().uri("/admin/offers")
                .header(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))
                .body(axum::body::Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()["offers"].as_array().unwrap().len()
        };
        // The starting refresh found the provider ready and published both offers.
        assert!(runtime.state.funding_ready.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 2);
        assert_eq!(offers(runtime.clone()).await, 2);
        let status = runtime.dht.status().await;
        let address = format!(
            "{}/p2p/{}",
            status["listening_addresses"][0].as_str().unwrap(),
            runtime.dht.peer_id()
        );
        let network = runtime.state.config.network.identity.clone();
        let discover = |name: &'static str| {
            let (address, network, directory) = (address.clone(), network.clone(), directory.path().to_owned());
            async move {
                let consumer = DhtService::start(
                    provider_config::DhtConfig {
                        listen: "/ip4/127.0.0.1/tcp/0".into(),
                        bootstrap: vec![address],
                        identity_file: directory.join(name),
                    },
                    network.clone(),
                    Arc::new(OfferRegistry::new(network, false)),
                )
                .await
                .unwrap();
                let offers = consumer.discover(None).await.unwrap();
                consumer.stop().await;
                offers
            }
        };
        assert_eq!(discover("consumer-a.key").await.len(), 2);

        // `embed-b`'s next offer cannot be published: the provider's registry already
        // holds a higher signed revision of its namespace, as after a lowered counter.
        let mut blocking = runtime.state.offers.get(&runtime.published.lock().await.iter()
            .find(|id| runtime.state.offers.get(id).is_some_and(|o| o.payload.model.id == "embed-b"))
            .unwrap().clone()).unwrap().payload;
        blocking.revision = 1_000_000;
        runtime.state.offers.insert(protocol::sign(protocol::OFFER_DOMAIN, blocking,
            &runtime.state.private_key).unwrap()).unwrap();
        assert!(runtime.refresh().await.is_err());
        assert!(runtime.refresh_error.read().await.is_some());
        assert!(!runtime.state.funding_ready.load(Ordering::Acquire));
        // `embed-a`'s new offer, published earlier in the failed pass, is withdrawn too.
        assert_eq!(runtime.dht.status().await["published_offers"], 0);
        assert_eq!(offers(runtime.clone()).await, 0);
        assert!(discover("consumer-b.key").await.is_empty());

        runtime.dht.stop().await;
        for server in engines.into_iter().chain([rpc_server, unavailable]) {
            server.abort();
        }
    }

    #[tokio::test]
    async fn fee_view_lists_confirmed_remittances_and_the_receipt_statement() {
        // inference-fee-remittance requirements 11 and 12 (task 2.2). Both fee
        // routes list actual remittances with their transaction and inclusion
        // evidence, never their signed bytes, and state that a recorded
        // obligation is not proof that the platform received the fee.
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let (config, server) = unavailable_provider(directory.path()).await;
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let state = &runtime.state;
        let identity = state.config.network.identity.clone();
        let consumer_key = "22".repeat(32);
        let now = protocol::now().unwrap();
        let quote = protocol::sign(
            protocol::QUOTE_DOMAIN,
            Quote {
                id: protocol::random_id(),
                network: identity.clone(),
                offer_id: protocol::random_id(),
                provider_wallet: state.wallet.clone(),
                consumer_wallet: wallet::address_from_private_key(&consumer_key).unwrap(),
                model: state.config.models[0].model.clone(),
                price: state.config.models[0].price.clone(),
                funding_wei: "1000".into(),
                platform_wallet: state.config.platform_wallet.clone(),
                fee_bps: 50,
                max_concurrency: 1,
                nonce: protocol::random_id(),
                issued_at: now,
                expires_at: now + 300,
            },
            &state.private_key,
        )
        .unwrap();
        state.ledger.store_quote(&quote, now).unwrap();
        // A 1,000-wei opening owes a 5-wei fee; its funding evidence is a ledger
        // input here, past the chain verification boundary.
        let opening = protocol::opening_data(&quote.payload).unwrap();
        let funding = crate::chain_client::sign_transaction(
            &consumer_key,
            identity.chain_id,
            0,
            &state.wallet,
            1000,
            &opening,
            1,
            21_000 + 40 * opening.len() as u64,
        )
        .unwrap();
        state
            .ledger
            .register_funding_intent(
                &quote,
                &quote.payload.consumer_wallet,
                &funding.raw_transaction,
                &funding.tx_hash,
                0,
                &state.private_key,
                || Ok(now),
                true,
                &quote.payload.model,
            )
            .unwrap();
        let payment = crate::chain_client::VerifiedPayment {
            network: identity.clone(),
            tx_hash: funding.tx_hash,
            from: quote.payload.consumer_wallet.clone(),
            to: state.wallet.clone(),
            value_wei: "1000".into(),
            data: opening,
            block_hash: format!("0x{}", "44".repeat(32)),
            block_number: 110,
            raw_transaction: funding.raw_transaction,
        };
        state.ledger.open_account(&quote, Some(&payment), now).unwrap();
        // Seal, record and confirm one 5-wei remittance in block 120.
        let mut batch = state.ledger.prepare_fee_batches(now, true).unwrap().remove(0);
        assert_eq!(batch.amount_wei, "5");
        let data = batch.payment_data().unwrap();
        let remittance = crate::chain_client::sign_transaction(
            &state.private_key,
            identity.chain_id,
            0,
            &batch.platform_wallet,
            5,
            &data,
            1,
            21_000 + 40 * data.len() as u64,
        )
        .unwrap();
        batch.tx_hash = Some(remittance.tx_hash.clone());
        batch.raw_transaction = Some(remittance.raw_transaction.clone());
        batch.status = "submitted".into();
        state.ledger.update_fee_batch(&batch).unwrap();
        let block_hash = format!("0x{}", "55".repeat(32));
        batch.status = "accepted".into();
        batch.evidence = Some(crate::chain_client::VerifiedPayment {
            network: identity,
            tx_hash: remittance.tx_hash.clone(),
            from: state.wallet.clone(),
            to: batch.platform_wallet.clone(),
            value_wei: "5".into(),
            data,
            block_hash: block_hash.clone(),
            block_number: 120,
            raw_transaction: remittance.raw_transaction.clone(),
        });
        state.ledger.update_fee_batch(&batch).unwrap();

        for (method, path) in [("GET", "/admin/fees"), ("POST", "/admin/fees/remit")] {
            // The operator bearer, without a browser `Origin`.
            let response = runtime
                .admin_router()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .unwrap();
            let text = std::str::from_utf8(&bytes).unwrap();
            // The signed bytes stay in the ledger and the journal.
            assert!(!text.contains("raw_transaction"), "{path}: {text}");
            assert!(
                !text.contains(remittance.raw_transaction.trim_start_matches("0x")),
                "{path}: {text}"
            );
            let view: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(view["amounts_wei"]["confirmed"], "5", "{path}");
            assert_eq!(
                view["batches"],
                json!([{
                    "id": batch.id,
                    "platform_wallet": state.config.platform_wallet,
                    "amount_wei": "5",
                    "state": "confirmed",
                    "tx_hash": remittance.tx_hash,
                    "inclusion_block_hash": block_hash,
                    "inclusion_block_number": 120,
                    "created_at": now,
                }]),
                "{path}"
            );
            assert_eq!(view["collection"], "provider-remitted-not-trustless", "{path}");
            assert_eq!(
                view["platform_receipt"],
                "A recorded fee obligation is not proof that the platform received the fee.",
                "{path}"
            );
        }
        runtime.dht.stop().await;
        server.abort();
    }

    /// Book line 474 (`managed-engine-runtime` requirement 6, task 2.1): an engine key that
    /// repeats the signing key in any spelling (`0x`, letter case, leading zeros), the
    /// administration token or the RPC bearer is refused before anything starts, without printing
    /// it; a distinct key is accepted.
    #[tokio::test]
    async fn an_engine_key_that_repeats_another_credential_is_refused_before_anything_starts() {
        let directory = tempfile::tempdir().unwrap();
        let (mut config, server) = unavailable_provider(directory.path()).await;
        let protected = |name: &str, token: &str| {
            let path = directory.path().join(name);
            wallet::create_secret_file(&path).unwrap().write_all(token.as_bytes()).unwrap();
            path
        };
        // Test-only values; the signing key has letters, so that letter case matters.
        let signing = "0123456789abcdef".repeat(4);
        config.signing_key_file = directory.path().join("lettered-wallet.key");
        wallet::save_private_key(&config.signing_key_file, &signing).unwrap();
        let bearer = "private-test-rpc-bearer-token-of-40-chars";
        config.network.rpc_bearer_file = Some(protected("rpc.token", bearer));
        let cases = [
            (config.signing_key_file.clone(), "the provider signing key"),
            (protected("upper.key", &format!("0X{}", signing.to_ascii_uppercase())), "the provider signing key"),
            (protected("prefixed.key", &format!("0x{signing}\n")), "the provider signing key"),
            // The same 256-bit value without its leading zero, or with more of them.
            (protected("unpadded.key", signing.trim_start_matches('0')), "the provider signing key"),
            (protected("padded.key", &format!("0X000{}", signing.to_ascii_uppercase())), "the provider signing key"),
            (protected("admin-copy.token", OPERATOR_TOKEN), "the administration token"),
            (config.admin_token_file.clone(), "the administration token"),
            (protected("bearer-copy.token", bearer), "the RPC bearer"),
        ];
        for (engine_key, credential) in cases {
            let mut refused = config.clone();
            refused.models[0].engine_api_key_file = Some(engine_key);
            let error = ProviderRuntime::start(refused).await.err().unwrap();
            assert!(error.contains("must hold a separate credential") && error.contains(credential), "{error}");
            assert!(error.contains(&config.models[0].model.id), "{error}");
            for value in [signing.trim_start_matches('0'), OPERATOR_TOKEN, bearer] {
                assert!(!error.to_ascii_lowercase().contains(&value.to_ascii_lowercase()), "{error}");
            }
            // Refused before the DHT, the deployment lock or any engine.
            assert!(!config.data_dir.exists());
        }
        config.models[0].engine_api_key_file = Some(protected("engine.token", &"e7".repeat(32)));
        let runtime = ProviderRuntime::start(config).await.unwrap();
        runtime.dht.stop().await;
        server.abort();
    }

    #[tokio::test]
    async fn deployments_share_a_wallet_without_sharing_accounts_or_process_locks() {
        let directory = tempfile::tempdir().unwrap();
        let offer = crate::offer_registry::tests::offer().payload;
        let key = "11".repeat(32);
        let (config, server) = unavailable_provider(directory.path()).await;
        let first = ProviderRuntime::start(config.clone()).await.unwrap();
        assert!(ProviderRuntime::start(config.clone()).await.is_err());
        let mut second_config = config.clone();
        second_config.dht.identity_file = directory.path().join("peer-b.key");
        let second = ProviderRuntime::start(second_config).await.unwrap();
        let now = protocol::now().unwrap();
        let quote = protocol::sign(
            protocol::QUOTE_DOMAIN,
            Quote {
                id: protocol::random_id(),
                network: offer.network,
                offer_id: protocol::random_id(),
                provider_wallet: offer.provider_wallet,
                consumer_wallet: wallet::address_from_private_key(&"22".repeat(32)).unwrap(),
                model: offer.model,
                price: PriceSchedule {
                    input_wei_per_1k: "0".into(),
                    output_wei_per_1k: "0".into(),
                },
                funding_wei: "0".into(),
                platform_wallet: offer.platform_wallet,
                fee_bps: 50,
                max_concurrency: 1,
                nonce: protocol::random_id(),
                issued_at: now,
                expires_at: now + 300,
            },
            &key,
        )
        .unwrap();
        first.state.ledger.store_quote(&quote, now).unwrap();
        first.state.ledger.open_account(&quote, None, now).unwrap();
        assert_eq!(
            first
                .state
                .ledger
                .account(&quote.payload.id)
                .unwrap()
                .status,
            "free"
        );
        assert!(second.state.ledger.account(&quote.payload.id).is_err());
        first.dht.stop().await;
        drop(first);
        let restarted = ProviderRuntime::start(config).await.unwrap();
        assert_eq!(
            restarted
                .state
                .ledger
                .account(&quote.payload.id)
                .unwrap()
                .status,
            "free"
        );
        assert!(second.state.ledger.account(&quote.payload.id).is_err());
        restarted.dht.stop().await;
        second.dht.stop().await;
        server.abort();
    }
}
