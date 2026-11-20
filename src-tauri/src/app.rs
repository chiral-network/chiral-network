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
    /// A durable settlement failure that closed admission (provider-daemon-lifecycle
    /// requirement 6): a fixed, content-free reason. Only the operator's resume clears it,
    /// and a restart starts without it; no maintenance pass or offer refresh reopens
    /// admission while it is set.
    pub settlement_fault: std::sync::Mutex<Option<&'static str>>,
    pub funding_ready: AtomicBool,
    pub qualified_models: std::sync::RwLock<HashSet<String>>,
    /// The native embedding dimension of each ready embedding model, measured by its
    /// last readiness embedding probe and removed when the model stops being ready.
    /// A request naming other `dimensions` is refused before reservation; never
    /// signed or listed.
    pub embedding_dimensions: std::sync::RwLock<std::collections::HashMap<String, u64>>,
    /// Shared by all public routers and readiness probes; no hidden engine concurrency.
    pub model_slots: std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>,
    pub managed_engines: std::collections::HashMap<String, Arc<crate::engine_runtime::ManagedEngine>>,
}

impl ProviderState {
    pub fn engine_alive(&self, model_id: &str) -> bool {
        self.managed_engines.get(model_id).is_none_or(|engine| engine.is_alive())
    }
    fn fault(&self) -> std::sync::MutexGuard<'_, Option<&'static str>> {
        // A poisoned lock still holds the fault; settlement never panics over it.
        self.settlement_fault.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    /// Closes admission, and with it new-funding readiness, after a durable settlement
    /// failure and keeps them closed until the operator's resume (provider-daemon-lifecycle
    /// requirement 6). The first fault is kept.
    pub fn record_settlement_fault(&self, reason: &'static str) {
        let mut fault = self.fault();
        fault.get_or_insert(reason);
        self.accepting.store(false, Ordering::Release);
        self.funding_ready.store(false, Ordering::Release);
    }
    pub fn settlement_fault(&self) -> Option<&'static str> {
        *self.fault()
    }
    /// Stores a maintenance pass's admission decision and its new-funding readiness, open
    /// only with admission and `funding_gates`; a recorded settlement fault overrides both.
    /// Serialized with `record_settlement_fault`, so that a fault recorded while the pass ran
    /// is never overwritten by its earlier decision. Returns the new-funding readiness.
    fn store_admission(&self, accepting: bool, funding_gates: bool) -> bool {
        let fault = self.fault();
        let accepting = accepting && fault.is_none();
        let funding_ready = accepting && funding_gates;
        self.accepting.store(accepting, Ordering::Release);
        self.funding_ready.store(funding_ready, Ordering::Release);
        funding_ready
    }
    pub fn slots(config: &ProviderConfig) -> std::collections::HashMap<String, Arc<tokio::sync::Semaphore>> {
        config.models.iter().map(|model| (
            model.model.id.clone(), Arc::new(tokio::sync::Semaphore::new(model.max_concurrency as usize)),
        )).collect()
    }
    /// Applies one readiness pass: the ready set, and each ready embedding model's
    /// measured native dimension. A model that is not ready keeps no dimension; a
    /// busy, already ready model whose pass repeated no inference keeps its value.
    pub fn record_readiness(&self, health: &[ModelHealth]) -> Result<(), &'static str> {
        let mut qualified = self
            .qualified_models
            .write()
            .map_err(|_| "model qualification lock unavailable")?;
        let mut dimensions = self
            .embedding_dimensions
            .write()
            .map_err(|_| "model qualification lock unavailable")?;
        qualified.clear();
        for model in health.iter().filter(|model| model.ready) {
            qualified.insert(model.model_id.clone());
            if let Some(native) = model.embedding_dimensions {
                dimensions.insert(model.model_id.clone(), native);
            }
        }
        dimensions.retain(|id, _| qualified.contains(id));
        Ok(())
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
        Self::start_with(config, std::collections::HashMap::new()).await
    }

    /// `start`, holding `managed_engines` for the models they name besides the runtimes it
    /// launches. Only tests pass engines here (`managed-engine-runtime` task 3.10's helper
    /// child); the daemon starts with none.
    async fn start_with(
        config: ProviderConfig,
        mut managed_engines: std::collections::HashMap<String, Arc<crate::engine_runtime::ManagedEngine>>,
    ) -> Result<Arc<Self>, String> {
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
            settlement_fault: Default::default(),
            funding_ready: AtomicBool::new(false),
            qualified_models: std::sync::RwLock::new(HashSet::new()),
            embedding_dimensions: std::sync::RwLock::new(Default::default()),
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
                if let Ok(mut dimensions) = runtime.state.embedding_dimensions.write() {
                    for id in &failed { dimensions.remove(id); }
                }
                runtime.state.accepting.store(false, Ordering::Release);
                runtime.state.funding_ready.store(false, Ordering::Release);
                // Serialize against a refresh publishing stale readiness; no restart/retry.
                let _lifecycle = runtime.lifecycle.lock().await;
                runtime.state.accepting.store(false, Ordering::Release);
                runtime.state.funding_ready.store(false, Ordering::Release);
                // The reason agrees with the readiness until the next pass replaces the entry
                // (provider-daemon-lifecycle requirement 1, design code difference 8).
                for health in runtime.health.write().await.iter_mut() {
                    if failed.contains(&health.model_id) {
                        health.ready = false;
                        health.detail = inference_backend::ENGINE_EXITED.into();
                    }
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

    /// One maintenance pass: the periodic pass and `POST /admin/offers/refresh`. It never
    /// ends a drain or a settlement-failure stop.
    pub async fn refresh(&self) -> Result<usize, String> {
        self.pass(false).await
    }

    /// `POST /admin/resume`: ends a drain and a settlement-failure stop, then runs one
    /// maintenance pass, which alone may reopen admission (provider-daemon-lifecycle
    /// requirements 4 and 6).
    pub async fn resume(&self) -> Result<usize, String> {
        self.draining.store(false, Ordering::Release);
        self.pass(true).await
    }

    async fn pass(&self, resume: bool) -> Result<usize, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if resume {
            // Under the lifecycle lock: only a pass that starts after the operator's
            // resume can reopen admission that a settlement failure closed.
            *self.state.fault() = None;
        }
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
        self.state.record_readiness(&health)?;
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
        // A durable settlement failure keeps admission, and so new funding and publication,
        // closed until the operator resumes (provider-daemon-lifecycle requirement 6).
        let funding_ready = self.state.store_admission(accepting, chain_ready && funding_safe);
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

/// The status entry of a model whose report is latched (provider-daemon-lifecycle
/// requirement 1); its latest readiness result follows.
pub(crate) const PERFORMANCE_WITHDRAWN: &str = "Performance-withdrawn: a durable withdrawal latch for this model's qualification evidence blocks its admission; resume and offer refresh do not clear it.";
/// `ledger_error` when the ledger summary cannot be read; the other facts are still reported.
pub(crate) const LEDGER_UNAVAILABLE: &str = "Ledger summary unavailable";

impl ProviderRuntime {
    /// Each model's latest readiness, or a later observed engine exit, and whether a durable
    /// performance-withdrawal latch blocks its admission: read from the ledger now, because a
    /// settlement can latch a report between passes; `null` when the ledger cannot answer.
    async fn model_status(&self) -> Vec<Value> {
        let health = self.health.read().await;
        health.iter().map(|model| {
            let withdrawn = self.state.config.models.iter()
                .find(|configured| configured.model.id == model.model_id)
                .and_then(|configured| self.state.ledger.performance_withdrawn(&configured.model).ok());
            let detail = if withdrawn == Some(true) {
                format!("{PERFORMANCE_WITHDRAWN} Latest readiness result: {}", model.detail)
            } else {
                model.detail.clone()
            };
            json!({"model_id": model.model_id, "ready": model.ready, "detail": detail,
                "performance_withdrawn": withdrawn})
        }).collect()
    }
}

/// Reports each lifecycle fact separately; one that cannot be read is reported as
/// unavailable while the others are still reported (provider-daemon-lifecycle requirement 1).
async fn admin_status(State(runtime): State<Arc<ProviderRuntime>>) -> Json<Value> {
    let (ledger, ledger_error) = match runtime.state.ledger.admin_summary() {
        Ok(summary) => (summary, None),
        Err(_) => (Value::Null, Some(LEDGER_UNAVAILABLE)),
    };
    Json(
        json!({"accepting":runtime.state.accepting.load(Ordering::Acquire),
        "draining":runtime.draining.load(Ordering::Acquire),
        "settlement_error":runtime.state.settlement_fault(),
        "funding_ready":runtime.state.funding_ready.load(Ordering::Acquire),
        "chain_error":&*runtime.chain_error.read().await,
        "maintenance":&*runtime.maintenance_status.read().await,
        "refresh_error":&*runtime.refresh_error.read().await,
        "network":runtime.state.config.network.identity,"provider_wallet":runtime.state.wallet,
        "models":runtime.model_status().await,"dht":runtime.dht.status().await,
        "ledger":ledger,"ledger_error":ledger_error}),
    )
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
    let count = runtime.resume().await.map_err(ApiError::internal)?;
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

    /// A non-development provider whose only model, `model`, is an external embedding
    /// deployment with `capabilities`, served by `embeddings_engine` while `healthy`
    /// (503 otherwise); a free account for it; and the servers to abort.
    async fn embedding_provider(
        directory: &std::path::Path,
        capabilities: &[&str],
        healthy: Arc<AtomicBool>,
    ) -> (Arc<ProviderRuntime>, String, Vec<tokio::task::JoinHandle<()>>) {
        embedding_provider_on(directory, capabilities, healthy, Router::new().fallback(ready_chain), false).await
    }

    /// `embedding_provider` whose chain RPC is `chain`. With `managed`, its model also runs on
    /// a managed engine: `managed-engine-runtime` task 3.10's helper child, whose liveness
    /// gates the model while `embeddings_engine` answers its readiness probes.
    async fn embedding_provider_on(
        directory: &std::path::Path,
        capabilities: &[&str],
        healthy: Arc<AtomicBool>,
        chain: Router,
        managed: bool,
    ) -> (Arc<ProviderRuntime>, String, Vec<tokio::task::JoinHandle<()>>) {
        let (mut config, unavailable) = unavailable_provider(directory).await;
        let (rpc, rpc_server) = serve(chain).await;
        config.network.rpc_url = rpc;
        config.development = false;
        config.public_endpoint = "https://inference.example.org/".into();
        let model = &mut config.models[0];
        model.model.capabilities = capabilities.iter().map(|c| c.to_string()).collect();
        model.model.max_output_tokens = 0;
        model.price.output_wei_per_1k = "0".into();
        let engine = embeddings_engine(model.model.clone()).layer(middleware::from_fn(
            move |request: Request, next: Next| {
                let healthy = healthy.clone();
                async move {
                    if healthy.load(Ordering::Acquire) {
                        next.run(request).await
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE.into_response()
                    }
                }
            },
        ));
        let (engine, engine_server) = serve(engine).await;
        model.engine_url = engine;
        let mut engines = std::collections::HashMap::new();
        if managed {
            let helper = crate::engine_runtime::ManagedEngine::helper_for_test(&model.model.id, &model.engine_url);
            engines.insert(model.model.id.clone(), Arc::new(helper));
        }
        let runtime = ProviderRuntime::start_with(config, engines).await.unwrap();
        let (_, bearer) = free_account(&runtime.state, "22");
        (runtime, bearer, vec![engine_server, rpc_server, unavailable])
    }

    /// Opens a free, one-request-at-a-time account for the first model, owned by the wallet of
    /// signing key `consumer` repeated: its contract identifier and bearer.
    fn free_account(state: &ProviderState, consumer: &str) -> (String, String) {
        let now = protocol::now().unwrap();
        let quote = protocol::sign(protocol::QUOTE_DOMAIN, Quote {
            id: protocol::random_id(), network: state.config.network.identity.clone(),
            offer_id: protocol::random_id(), provider_wallet: state.wallet.clone(),
            consumer_wallet: wallet::address_from_private_key(&consumer.repeat(32)).unwrap(),
            model: state.config.models[0].model.clone(),
            price: PriceSchedule { input_wei_per_1k: "0".into(), output_wei_per_1k: "0".into() },
            funding_wei: "0".into(), platform_wallet: state.config.platform_wallet.clone(),
            fee_bps: 50, max_concurrency: 1, nonce: protocol::random_id(),
            issued_at: now, expires_at: now + 300,
        }, &state.private_key).unwrap();
        state.ledger.store_quote(&quote, now).unwrap();
        let bearer = state.ledger.open_account(&quote, None, now).unwrap().credential.unwrap().bearer;
        (quote.payload.id, bearer)
    }

    /// `GET /admin/offers` and `GET /v1/models` with `bearer`: the published offers'
    /// advertised capabilities, and the listed model identifiers.
    async fn offers_and_listing(runtime: &Arc<ProviderRuntime>, bearer: &str) -> (Vec<Value>, Vec<Value>) {
        use tower::ServiceExt;
        let read = |response: Response| async move {
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        };
        let offers = read(runtime.admin_router().oneshot(Request::builder().uri("/admin/offers")
            .header(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))
            .body(axum::body::Body::empty()).unwrap()).await.unwrap()).await;
        let listing = read(runtime.public_router().oneshot(Request::builder().uri("/v1/models")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .body(axum::body::Body::empty()).unwrap()).await.unwrap()).await;
        (
            offers["offers"].as_array().unwrap().iter()
                .map(|offer| offer["payload"]["model"]["capabilities"].clone()).collect(),
            listing["data"].as_array().unwrap().iter().map(|model| model["id"].clone()).collect(),
        )
    }

    /// public-inference-api requirement 8 (task 2.1, design code difference 1): a model whose
    /// advertised capability has no probe on its endpoints is not ready, so it is neither
    /// admitted, listed nor published, although its embedding probes succeed. `streaming`
    /// without `chat` or `completions` is the case that was ready before; `tools` and
    /// `structured_output` without `chat` are refused by the same rule.
    #[tokio::test]
    async fn a_capability_without_a_probe_on_the_model_endpoints_is_not_ready_or_published() {
        for capabilities in [
            &["embeddings", "streaming"][..],
            &["embeddings", "tools"],
            &["embeddings", "structured_output"],
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (runtime, bearer, servers) =
                embedding_provider(directory.path(), capabilities, Arc::new(AtomicBool::new(true))).await;
            let health = runtime.health.read().await.clone();
            assert_eq!(health.len(), 1);
            assert!(!health[0].ready, "{capabilities:?}: {health:?}");
            // Task 2.5: the operator's health detail names the capability without a probe.
            assert!(health[0].detail.contains("without a probe"), "{capabilities:?}: {health:?}");
            assert!(health[0].detail.contains(capabilities[1]), "{capabilities:?}: {health:?}");
            assert!(runtime.state.qualified_model(&runtime.state.config.models[0].model).is_none());
            assert!(runtime.state.embedding_dimensions.read().unwrap().is_empty());
            assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{capabilities:?}");
            assert_eq!(runtime.dht.status().await["published_offers"], 0, "{capabilities:?}");
            assert_eq!(offers_and_listing(&runtime, &bearer).await, (vec![], vec![]), "{capabilities:?}");
            runtime.dht.stop().await;
            for server in servers {
                server.abort();
            }
        }
        // Control: the same deployment advertising only `embeddings` is ready and published.
        let directory = tempfile::tempdir().unwrap();
        let (runtime, bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        assert!(runtime.health.read().await[0].ready);
        assert!(runtime.state.funding_ready.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        assert_eq!(offers_and_listing(&runtime, &bearer).await, (vec![json!(["embeddings"])], vec![json!("model")]));
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// public-inference-api requirement 10 (task 2.2): readiness records the native
    /// embedding dimension that its embedding probes measured, a busy re-check that runs no
    /// inference keeps it, a pass in which the model is not ready removes it, and the next
    /// ready pass records it again. The value is
    /// private: it is not in the status or the signed offer.
    #[tokio::test]
    async fn readiness_records_the_native_embedding_dimension_only_while_the_model_is_ready() {
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let healthy = Arc::new(AtomicBool::new(true));
        let (runtime, bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], healthy.clone()).await;
        let recorded = || runtime.state.embedding_dimensions.read().unwrap().clone();
        // `embeddings_engine` answers one-dimensional vectors.
        let native = std::collections::HashMap::from([("model".to_owned(), 1)]);
        assert_eq!(recorded(), native);
        let status = runtime.admin_router().oneshot(Request::builder().uri("/admin/status")
            .header(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))
            .body(axum::body::Body::empty()).unwrap()).await.unwrap();
        let status = axum::body::to_bytes(status.into_body(), 1 << 20).await.unwrap();
        assert!(!std::str::from_utf8(&status).unwrap().contains("dimension"));

        // A busy re-check (every slot held) repeats no inference and measures nothing:
        // the model stays ready and keeps the value its last probe measured.
        let busy = runtime.state.model_slots["model"].clone()
            .try_acquire_many_owned(runtime.state.config.models[0].max_concurrency).unwrap();
        runtime.refresh().await.unwrap();
        assert!(runtime.health.read().await[0].ready);
        assert_eq!(recorded(), native);
        drop(busy);

        healthy.store(false, Ordering::Release);
        runtime.refresh().await.unwrap();
        assert!(!runtime.health.read().await[0].ready);
        assert!(runtime.state.qualified_models.read().unwrap().is_empty());
        assert!(recorded().is_empty());
        assert_eq!(offers_and_listing(&runtime, &bearer).await, (vec![], vec![]));

        healthy.store(true, Ordering::Release);
        runtime.refresh().await.unwrap();
        assert!(runtime.health.read().await[0].ready);
        assert_eq!(recorded(), native);
        let (offers, listing) = offers_and_listing(&runtime, &bearer).await;
        assert_eq!(listing, vec![json!("model")]);
        assert_eq!(offers.len(), 1);
        let published = runtime.state.offers.get(runtime.published.lock().await.iter().next().unwrap()).unwrap();
        assert!(!serde_json::to_string(&published).unwrap().contains("dimension"));
        runtime.dht.stop().await;
        for server in servers {
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

    /// `method path` on the administration router with the operator bearer: the status and
    /// the JSON answer.
    async fn admin(runtime: &Arc<ProviderRuntime>, method: &str, path: &str) -> (StatusCode, Value) {
        use tower::ServiceExt;
        let response = runtime.admin_router().oneshot(Request::builder().method(method).uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))
            .body(axum::body::Body::empty()).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// `GET /readyz` on the public router.
    async fn readyz(runtime: &Arc<ProviderRuntime>) -> StatusCode {
        use tower::ServiceExt;
        runtime.public_router().oneshot(Request::builder().uri("/readyz")
            .body(axum::body::Body::empty()).unwrap()).await.unwrap().status()
    }

    /// One embedding request with `Idempotency-Key: key` through the public router.
    async fn embed(runtime: &Arc<ProviderRuntime>, bearer: &str, key: &str) -> (StatusCode, Value) {
        use tower::ServiceExt;
        let response = runtime.public_router().oneshot(Request::builder().method("POST")
            .uri("/v1/embeddings")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", key)
            .body(axum::body::Body::from(json!({"model": "model", "input": "hello"}).to_string()))
            .unwrap()).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// The deployment's ledger, for a second connection that injects faults.
    fn ledger_connection(runtime: &ProviderRuntime) -> rusqlite::Connection {
        let path = runtime.state.config.runtime_data_dir().unwrap()
            .join(&runtime.state.wallet).join(runtime.dht.peer_id()).join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(path).unwrap();
        connection.busy_timeout(Duration::from_secs(5)).unwrap();
        connection
    }

    /// provider-daemon-lifecycle requirements 1 and 6 (task 2.1, design code difference 1):
    /// admission closed by a durable settlement failure stays closed through a maintenance
    /// pass and `POST /admin/offers/refresh` after the ledger accepts writes again, the status
    /// names the failure, and only `POST /admin/resume` reopens admission, through its pass.
    /// In the second case the ledger refuses only the `completed` write, so that the
    /// settlement's single retry on drop records the request as `interrupted` (design
    /// observation 13); admission stays closed all the same.
    #[tokio::test]
    async fn a_settlement_failure_keeps_admission_closed_until_the_operator_resumes() {
        for (refused, recorded) in [("NEW.status<>'reserved'", "reserved"), ("NEW.status='completed'", "interrupted")] {
            let directory = tempfile::tempdir().unwrap();
            let (runtime, bearer, servers) =
                embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
            assert!(runtime.state.accepting.load(Ordering::Acquire));
            assert_eq!(runtime.dht.status().await["published_offers"], 1);
            let contract = runtime.state.ledger.list_accounts().unwrap()[0].contract_id.clone();
            let ledger = ledger_connection(&runtime);
            ledger.execute_batch(&format!("CREATE TRIGGER refuse_settlement BEFORE UPDATE ON requests
                WHEN OLD.status='reserved' AND {refused}
                BEGIN SELECT RAISE(ABORT,'injected settlement write failure'); END;")).unwrap();
            let (status, answer) = embed(&runtime, &bearer, "settlement-fault-1").await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{refused}: {answer}");
            assert!(!runtime.state.accepting.load(Ordering::Acquire), "{refused}");
            // New funding closes with admission at once, not at the next pass: the status
            // and `/readyz` never report new-funding readiness while admission is closed.
            assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{refused}");
            let view = admin(&runtime, "GET", "/admin/status").await.1;
            assert_eq!((&view["accepting"], &view["funding_ready"], &view["settlement_error"]),
                (&json!(false), &json!(false), &json!(inference_gateway::DURABLE_SETTLEMENT_FAILED)),
                "{refused}: {view}");
            // The answer arrives after the settlement's drop retried its write once.
            ledger.execute_batch("DROP TRIGGER refuse_settlement").unwrap();

            // Neither the periodic pass nor an offer refresh reopens admission.
            runtime.refresh().await.unwrap();
            assert!(!runtime.state.accepting.load(Ordering::Acquire), "{refused}");
            let (status, refreshed) = admin(&runtime, "POST", "/admin/offers/refresh").await;
            assert_eq!(status, StatusCode::OK, "{refused}");
            assert_eq!(refreshed, json!({"accepting": false, "offers_published": 0}), "{refused}");
            assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{refused}");
            assert_eq!(runtime.dht.status().await["published_offers"], 0, "{refused}");
            let (status, answer) = embed(&runtime, &bearer, "settlement-fault-2").await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refused}: {answer}");
            assert_eq!(answer["error"]["code"], "provider_paused", "{refused}");
            assert!(runtime.state.ledger.request_view(&contract, "settlement-fault-2").is_err(), "{refused}");
            let (status, view) = admin(&runtime, "GET", "/admin/status").await;
            assert_eq!(status, StatusCode::OK, "{refused}");
            assert_eq!(view["settlement_error"], inference_gateway::DURABLE_SETTLEMENT_FAILED, "{refused}: {view}");
            assert_eq!((&view["accepting"], &view["draining"]), (&json!(false), &json!(false)), "{refused}");
            let (_, requests) = admin(&runtime, "GET", "/admin/requests").await;
            let requests = requests["requests"].as_array().unwrap();
            assert_eq!(requests.len(), 1, "{refused}: {requests:?}");
            assert_eq!(requests[0]["status"], recorded, "{refused}: {requests:?}");

            // The operator's resume clears the failure and reopens admission through its pass.
            let (status, resumed) = admin(&runtime, "POST", "/admin/resume").await;
            assert_eq!(status, StatusCode::OK, "{refused}");
            assert_eq!(resumed, json!({"accepting": true, "offers_published": 1}), "{refused}");
            assert_eq!(admin(&runtime, "GET", "/admin/status").await.1["settlement_error"], Value::Null);
            let (status, answer) = embed(&runtime, &bearer, "settlement-fault-3").await;
            let completed = if recorded == "reserved" {
                // The unsettled request, never generated again, still holds its account's only
                // concurrency slot until restart recovery settles it; another account is admitted.
                assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}: {answer}");
                assert_eq!(answer["error"]["code"], "rate_limit_exceeded", "{refused}");
                let (other, other_bearer) = free_account(&runtime.state, "33");
                let (status, answer) = embed(&runtime, &other_bearer, "settlement-fault-3").await;
                assert_eq!(status, StatusCode::OK, "{refused}: {answer}");
                other
            } else {
                assert_eq!(status, StatusCode::OK, "{refused}: {answer}");
                contract.clone()
            };
            assert_eq!(runtime.state.ledger.request_view(&completed, "settlement-fault-3").unwrap().status, "completed");
            assert_eq!(runtime.state.ledger.request_view(&contract, "settlement-fault-1").unwrap().status, recorded);
            runtime.dht.stop().await;
            for server in servers {
                server.abort();
            }
        }
    }

    /// provider-daemon-lifecycle requirement 1 (task 2.3, design code difference 3): the status
    /// tells a drain from a rejected chain identity, which also closes admission.
    #[tokio::test]
    async fn admin_status_distinguishes_a_drain_from_a_rejected_chain_identity() {
        let facts = |view: &Value| (view["accepting"].clone(), view["draining"].clone());
        let directory = tempfile::tempdir().unwrap();
        let (runtime, _bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        assert_eq!(facts(&admin(&runtime, "GET", "/admin/status").await.1), (json!(true), json!(false)));
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!(facts(&view), (json!(false), json!(true)), "{view}");
        assert_eq!(view["funding_ready"], false);
        assert_eq!(view["chain_error"], Value::Null);
        assert_eq!(admin(&runtime, "POST", "/admin/resume").await.0, StatusCode::OK);
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!(facts(&view), (json!(true), json!(false)), "{view}");
        assert_eq!(view["funding_ready"], true);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }

        // The configured RPC is unavailable at start, then reports another genesis. No drain.
        let rejected = Arc::new(AtomicBool::new(false));
        let chain = {
            let rejected = rejected.clone();
            Router::new().fallback(move |request: Json<Value>| {
                let rejected = rejected.clone();
                async move {
                    if !rejected.load(Ordering::Acquire) {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    if request["method"] == "eth_getBlockByNumber" && request["params"][0] == "0x0" {
                        let genesis = json!({"number": "0x0", "hash": format!("0x{}", "99".repeat(32)), "timestamp": "0x0"});
                        return Json(json!({"jsonrpc": "2.0", "id": request["id"], "result": genesis})).into_response();
                    }
                    ready_chain(request).await
                }
            })
        };
        let directory = tempfile::tempdir().unwrap();
        let (runtime, _bearer, servers) = embedding_provider_on(
            directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true)), chain, false).await;
        // An outage withholds only new funding.
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!(facts(&view), (json!(true), json!(false)), "{view}");
        rejected.store(true, Ordering::Release);
        let _ = runtime.refresh().await;
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!(facts(&view), (json!(false), json!(false)), "{view}");
        assert_eq!(view["chain_error"], "Configured RPC network identity was rejected", "{view}");
        assert_eq!(view["funding_ready"], false);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// A private engine that passes the chat readiness probes of one-slot `model`, whose
    /// served chat template is `template`.
    fn chat_engine(model: protocol::ModelSpec, template: &'static str) -> Router {
        let id = model.id.clone();
        let props = json!({"build_info": "b1-cd959ba", "total_slots": 1, "model_alias": model.id,
            "default_generation_settings": {"n_ctx": model.context_length}, "chat_template": template});
        Router::new()
            .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
            .route("/v1/models", get(move || async move { Json(json!({"data": [{"id": id}]})) }))
            .route("/props", get(move || async move { Json(props) }))
            .route("/apply-template", post(|| async { Json(json!({"prompt": "rendered"})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens": [1, 2]})) }))
            .route("/v1/chat/completions", post(|request: Json<Value>| async move {
                let usage = json!({"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3});
                if request["stream"] != true {
                    return Json(json!({"id": "probe", "object": "chat.completion", "model": request["model"],
                        "choices": [{"index": 0, "message": {"role": "assistant", "content": "OK"},
                            "finish_reason": "length"}], "usage": usage})).into_response();
                }
                let token = json!({"id": 7, "token": "OK", "logprob": -0.1,
                    "top_logprobs": [{"token": "OK", "logprob": -0.1}]});
                let chunk = json!({"id": "probe", "object": "chat.completion.chunk", "model": request["model"],
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": "OK"},
                        "logprobs": {"content": [token]}, "finish_reason": "length"}]});
                let last = json!({"id": "probe", "object": "chat.completion.chunk", "model": request["model"],
                    "choices": [], "usage": usage});
                ([(header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {chunk}\n\ndata: {last}\n\ndata: [DONE]\n\n")).into_response()
            }))
    }

    /// provider-daemon-lifecycle requirement 1 (task 2.3, design code difference 3): a profiled
    /// model whose readiness probes pass and whose report is latched is reported as
    /// performance-withdrawn, and its probe result is still reported, also when every slot is
    /// busy and the pass ran no probe.
    #[tokio::test]
    async fn admin_status_names_a_performance_withdrawn_model_and_keeps_its_readiness() {
        use sha2::{Digest, Sha256};
        const TEMPLATE: &str = "{{ messages }}";
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        // inference-generation-performance's synthetic evidence, for the pinned engine
        // version and the stub engine's chat template.
        let (mut model, mut report) = crate::performance::evidence_tests::fixture();
        model.chat_template_sha256 = hex::encode(Sha256::digest(TEMPLATE));
        let version = crate::engine_runtime::ENGINE_VERSION;
        model.serving.as_mut().unwrap().engine_version = version.into();
        report["runtime_observation"]["engine_version"] = json!(version);
        let evidence = directory.path().join("evidence.json");
        crate::performance::evidence_tests::persist(&mut model, &mut report, &evidence);
        let (engine, engine_server) = serve(chat_engine(model.clone(), TEMPLATE)).await;
        let deployment = &mut config.models[0];
        deployment.model = model.clone();
        deployment.engine_url = engine;
        deployment.qualification_evidence_file = Some(evidence);
        deployment.request_timeout_secs = 30;
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let entry = |view: Value| view["models"][0].clone();
        let detail = |entry: &Value| entry["detail"].as_str().unwrap().to_owned();
        // Control: the probes pass, and no report is latched.
        let control = entry(admin(&runtime, "GET", "/admin/status").await.1);
        assert_eq!(control["ready"], true, "{control}");
        assert_eq!(control["performance_withdrawn"], false, "{control}");
        assert!(detail(&control).starts_with("Ready: "), "{control}");
        assert!(runtime.state.qualified_model(&model).is_some());

        // Latch the admitted report's evidence digest through a second connection.
        let evidence_sha256 = model.serving.as_ref().unwrap().qualification.evidence_sha256.clone();
        ledger_connection(&runtime).execute(
            "INSERT INTO performance_withdrawals(model,evidence) VALUES(?1,?2)",
            rusqlite::params![model.id, evidence_sha256]).unwrap();
        assert!(runtime.state.qualified_model(&model).is_none());
        // Read at once, before any pass; then after passes that probe and that find every slot busy.
        let latched = entry(admin(&runtime, "GET", "/admin/status").await.1);
        assert_eq!(latched["performance_withdrawn"], true, "{latched}");
        assert_eq!(latched["ready"], true, "{latched}");
        assert!(detail(&latched).starts_with(PERFORMANCE_WITHDRAWN), "{latched}");
        assert!(detail(&latched).ends_with(&format!("Latest readiness result: {}", detail(&control))), "{latched}");
        for busy in [false, true, false] {
            let held = busy.then(|| runtime.state.model_slots[&model.id].clone().try_acquire_owned().unwrap());
            runtime.refresh().await.unwrap();
            drop(held);
            let latched = entry(admin(&runtime, "GET", "/admin/status").await.1);
            assert_eq!(latched["performance_withdrawn"], true, "{latched}");
            // The pass's own result: its probes passed, or every slot was busy.
            assert_eq!(latched["ready"], !busy, "{latched}");
            let text = detail(&latched);
            assert!(text.starts_with(PERFORMANCE_WITHDRAWN), "{text}");
            let result = if busy { "Initial qualification awaits idle engine capacity" } else { "Ready: " };
            assert!(text.contains(&format!("Latest readiness result: {result}")), "{text}");
            assert!(runtime.state.qualified_model(&model).is_none());
        }
        runtime.dht.stop().await;
        engine_server.abort();
        unavailable.abort();
    }

    /// provider-daemon-lifecycle requirement 1 (task 2.4, design code difference 4): when the
    /// ledger summary cannot be read, the status answers, reports the ledger as unavailable and
    /// still reports every other fact; `/readyz` answers 503 (`inference-offer-discovery`).
    #[tokio::test]
    async fn admin_status_reports_an_unreadable_ledger_and_every_other_fact() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, _bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        let (status, view) = admin(&runtime, "GET", "/admin/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((&view["ledger"]["accounts"], &view["ledger_error"]), (&json!(1), &Value::Null), "{view}");
        assert_eq!(readyz(&runtime).await, StatusCode::OK);

        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("ALTER TABLE accounts RENAME TO accounts_unreadable").unwrap();
        assert!(runtime.state.ledger.admin_summary().is_err());
        let (status, view) = admin(&runtime, "GET", "/admin/status").await;
        assert_eq!(status, StatusCode::OK, "{view}");
        assert_eq!(view["ledger"], Value::Null, "{view}");
        assert_eq!(view["ledger_error"], LEDGER_UNAVAILABLE, "{view}");
        for member in ["models", "accepting", "draining", "settlement_error", "funding_ready", "chain_error",
            "maintenance", "refresh_error", "network", "provider_wallet", "dht"] {
            assert!(view.get(member).is_some(), "{member}: {view}");
        }
        assert_eq!(view["models"][0]["ready"], true, "{view}");
        assert_eq!((&view["accepting"], &view["funding_ready"]), (&json!(true), &json!(true)), "{view}");
        assert_eq!(view["dht"]["published_offers"], 1, "{view}");
        assert_eq!(readyz(&runtime).await, StatusCode::SERVICE_UNAVAILABLE);

        ledger.execute_batch("ALTER TABLE accounts_unreadable RENAME TO accounts").unwrap();
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["ledger"]["accounts"], &view["ledger_error"]), (&json!(1), &Value::Null), "{view}");
        assert_eq!(readyz(&runtime).await, StatusCode::OK);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 1 (task 2.8, design code difference 8): after its
    /// managed engine exits, and before any further pass, the status reports the model not
    /// ready with the engine exit as its reason, not the passed probes of the last pass.
    #[tokio::test]
    async fn admin_status_reports_an_engine_exit_with_its_own_reason() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, _bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            Arc::new(AtomicBool::new(true)), Router::new().fallback(ready_chain), true).await;
        let model = admin(&runtime, "GET", "/admin/status").await.1["models"][0].clone();
        assert_eq!(model["ready"], true, "{model}");
        assert!(model["detail"].as_str().unwrap().starts_with("Ready: "), "{model}");
        assert_eq!(readyz(&runtime).await, StatusCode::OK);

        runtime.state.managed_engines["model"].kill_for_test();
        // `/readyz` sees the exit at once; the status once the engine watcher has observed it.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let model = loop {
            let model = admin(&runtime, "GET", "/admin/status").await.1["models"][0].clone();
            if readyz(&runtime).await == StatusCode::SERVICE_UNAVAILABLE && model["ready"] == false {
                break model;
            }
            assert!(tokio::time::Instant::now() < deadline, "the engine exit was not observed: {model}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(model["detail"], inference_backend::ENGINE_EXITED, "{model}");
        assert!(!runtime.state.accepting.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 0);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }
}
