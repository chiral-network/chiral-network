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
        atomic::{AtomicBool, AtomicU8, Ordering},
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
    /// Durable settlement failures that closed admission (provider-daemon-lifecycle
    /// requirement 6), each with a fixed, content-free reason. Only the operator's resume
    /// clears them, and only those recorded before it was requested; a restart starts without
    /// them. No maintenance pass or offer refresh reopens admission while one is set.
    pub settlement_fault: std::sync::Mutex<SettlementFaults>,
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

/// The durable settlement failures recorded since start (provider-daemon-lifecycle
/// requirement 6). Each has a sequence number, so that a resume clears only the failures
/// recorded before the operator asked for it (task 2.10).
#[derive(Debug, Default)]
pub struct SettlementFaults {
    /// Failures recorded since start: the sequence number of the latest.
    recorded: u64,
    /// The failure that keeps admission closed, with its sequence number: the first one that
    /// no resume has cleared or, after a resume that a later failure outlived, that later one.
    open: Option<(u64, &'static str)>,
    /// The latest failure's reason.
    latest: Option<&'static str>,
}

impl SettlementFaults {
    fn record(&mut self, reason: &'static str) {
        self.recorded = self.recorded.saturating_add(1);
        self.open.get_or_insert((self.recorded, reason));
        self.latest = Some(reason);
    }
    /// Clears the failures numbered up to `recorded`, the count when the operator asked to
    /// resume. A failure recorded later, for example by a request admitted before the first
    /// failure that settles while the resume waits for a running pass, stays open.
    fn clear_through(&mut self, recorded: u64) {
        if self.open.is_some_and(|(sequence, _)| sequence <= recorded) {
            self.open = (self.recorded > recorded).then_some(self.recorded).zip(self.latest);
        }
    }
}

impl ProviderState {
    pub fn engine_alive(&self, model_id: &str) -> bool {
        self.managed_engines.get(model_id).is_none_or(|engine| engine.is_alive())
    }
    fn fault(&self) -> std::sync::MutexGuard<'_, SettlementFaults> {
        // A poisoned lock still holds the faults; settlement never panics over it.
        self.settlement_fault.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    /// Closes admission, and with it new-funding readiness, after a durable settlement
    /// failure and keeps them closed until the operator's resume (provider-daemon-lifecycle
    /// requirement 6). The first fault is the one reported.
    pub fn record_settlement_fault(&self, reason: &'static str) {
        let mut fault = self.fault();
        fault.record(reason);
        self.accepting.store(false, Ordering::Release);
        self.funding_ready.store(false, Ordering::Release);
    }
    /// The reason of the settlement failure that keeps admission closed, if any.
    pub fn settlement_fault(&self) -> Option<&'static str> {
        self.fault().open.map(|(_, reason)| reason)
    }
    /// Stores a maintenance pass's admission decision and its new-funding readiness, open
    /// only with admission and `funding_gates`; a recorded settlement fault overrides both.
    /// Serialized with `record_settlement_fault`, so that a fault recorded while the pass ran
    /// is never overwritten by its earlier decision. Returns the new-funding readiness.
    fn store_admission(&self, accepting: bool, funding_gates: bool) -> bool {
        let fault = self.fault();
        let accepting = accepting && fault.open.is_none();
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

/// `ProviderRuntime::drain_state` values.
const OPEN: u8 = 0;
const DRAINING: u8 = 1;
const SHUTTING_DOWN: u8 = 2;

/// Why `POST /admin/resume` did not reopen the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeError {
    /// The shutdown path has drained the provider. Nothing reopens it; a restart serves again
    /// (provider-daemon-lifecycle requirement 11).
    ShuttingDown,
    /// The maintenance pass failed, with its sanitized error.
    Pass(String),
}

pub struct ProviderRuntime {
    pub state: Arc<ProviderState>,
    pub dht: Arc<DhtService>,
    health: RwLock<Vec<ModelHealth>>,
    chain_rejected: AtomicBool,
    chain_error: RwLock<Option<&'static str>>,
    maintenance_status: RwLock<contracts::MaintenanceReport>,
    refresh_error: RwLock<Option<String>>,
    /// `OPEN`, `DRAINING` (the operator's drain) or `SHUTTING_DOWN` (the shutdown path's
    /// drain, which nothing ends: provider-daemon-lifecycle requirement 11). It only rises,
    /// except that a resume ends an operator drain.
    drain_state: AtomicU8,
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
        // The deployment's peer identity, under the protected-file rules of
        // `inference-offer-discovery`. Nothing joins the network under it before this process
        // owns the deployment (provider-daemon-lifecycle requirement 8).
        let identity = crate::dht::PeerIdentity::load(&config.dht.identity_file)?;
        // Deployments share the wallet nonce journal, not customer balances or locks.
        let data_dir = config.runtime_data_dir()?.join(&wallet).join(identity.peer_id());
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
        // Only the lock owner listens, dials its bootstrap peers and answers manifests under the
        // deployment's peer ID; a refused second daemon never connects to a peer.
        let dht = Arc::new(
            DhtService::start_as(
                identity,
                config.dht.clone(),
                config.network.identity.clone(),
                offers.clone(),
            )
            .await?,
        );
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
            drain_state: AtomicU8::new(OPEN),
            admin_token,
            published: Mutex::new(HashSet::new()),
            lifecycle: Mutex::new(()),
            revision_file,
            revision: Mutex::new(revision),
            _instance_lock: instance_lock,
        });
        // A stopped engine leaves administration available, but never publishes a usable offer.
        // The starting pass fails as a later pass does: its error is kept in `refresh_error`
        // for `/admin/status`, new-funding readiness is closed and nothing stays published,
        // and admission is what the pass decided (a reconciliation or clock failure closes
        // it). A publication failure is not a startup failure; configuration, secret,
        // inventory, launch, lock, ledger-identity, chain-identity and recovery failures
        // refused the start above (provider-daemon-lifecycle requirement 7).
        let _ = runtime.refresh().await;
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

    /// The sanitized error of the latest maintenance pass, if it failed, as `/admin/status`
    /// reports it in `refresh_error`.
    pub async fn refresh_error(&self) -> Option<String> {
        self.refresh_error.read().await.clone()
    }

    /// One maintenance pass: the periodic pass and `POST /admin/offers/refresh`. It never
    /// ends a drain or a settlement-failure stop.
    pub async fn refresh(&self) -> Result<usize, String> {
        let lifecycle = self.lifecycle.lock().await;
        self.pass(&lifecycle).await
    }

    /// `POST /admin/resume`: ends an operator drain and the settlement-failure stops recorded
    /// before the request, then runs one maintenance pass, which alone may reopen admission
    /// (provider-daemon-lifecycle requirements 4 and 6). Once the shutdown path has drained,
    /// it refuses without a pass, also when it was received before (requirement 11).
    pub async fn resume(&self) -> Result<usize, ResumeError> {
        // When the operator asked: a failure recorded after this, for example by an earlier
        // admitted request while this resume waits for a running pass, is not cleared.
        let recorded = self.state.fault().recorded;
        // Ends only an operator drain, never the shutdown drain.
        if let Err(SHUTTING_DOWN) =
            self.drain_state.compare_exchange(DRAINING, OPEN, Ordering::AcqRel, Ordering::Acquire)
        {
            return Err(ResumeError::ShuttingDown);
        }
        let lifecycle = self.lifecycle.lock().await;
        // Under the lifecycle lock: a resume that waited behind the shutdown drain, or whose
        // drain ending raced it, is refused and runs no pass, so it never reopens admission.
        if self.drain_state.load(Ordering::Acquire) == SHUTTING_DOWN {
            return Err(ResumeError::ShuttingDown);
        }
        // Only a pass that starts after the operator's resume can reopen admission that a
        // settlement failure closed.
        self.state.fault().clear_through(recorded);
        self.pass(&lifecycle).await.map_err(ResumeError::Pass)
    }

    /// One maintenance pass, under the lifecycle lock that `_lifecycle` holds.
    async fn pass(&self, _lifecycle: &tokio::sync::MutexGuard<'_, ()>) -> Result<usize, String> {
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
            && self.drain_state.load(Ordering::Acquire) == OPEN
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

    /// `POST /admin/drain`: stops new admission and new funding and withdraws the published
    /// offers (provider-daemon-lifecycle requirement 3). A resume ends it.
    pub async fn drain(&self) {
        self.close(DRAINING).await;
    }

    /// The shutdown path's drain (provider-daemon-lifecycle requirement 11, design code
    /// difference 9): `drain`, which nothing ends. From here, a resume, also one received
    /// before, is refused, and no maintenance pass or offer refresh reopens admission or
    /// new-funding readiness or publishes. The state is set before the lifecycle lock is
    /// taken, so a resume that waits for the lock behind a running pass sees it too.
    pub async fn drain_for_shutdown(&self) {
        self.close(SHUTTING_DOWN).await;
    }

    async fn close(&self, state: u8) {
        self.drain_state.fetch_max(state, Ordering::AcqRel);
        self.state.accepting.store(false, Ordering::Release);
        self.state.funding_ready.store(false, Ordering::Release);
        let _lifecycle = self.lifecycle.lock().await;
        // A refresh already in progress may have computed admission before the
        // drain state changed. Reassert it after serializing with that refresh.
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
/// Process observations, not readiness (provider-daemon-lifecycle requirement 2, design code
/// difference 7). For each configured model: `managed`; `alive`, the managed engine process's
/// liveness, `null` for an operator-owned engine, which has no process here; `ready`, the
/// model's result in the latest readiness pass, as `/admin/status` reports it, and false once
/// its managed engine is not alive, never the process alone; and the launcher's
/// `observation`, which is not a signed serving profile, qualification evidence or permission
/// to publish.
async fn admin_runtime(State(runtime): State<Arc<ProviderRuntime>>) -> Json<Value> {
    let health = runtime.health.read().await;
    Json(json!({"models": runtime.state.config.models.iter().map(|model| {
        let engine = runtime.state.managed_engines.get(&model.model.id);
        let alive = engine.map(|engine| engine.is_alive());
        let ready = alive != Some(false)
            && health.iter().any(|health| health.model_id == model.model.id && health.ready);
        json!({"model_id": model.model.id, "managed": engine.is_some(), "alive": alive,
            "ready": ready, "observation": engine.map(|engine| &engine.observation)})
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
        "draining":runtime.drain_state.load(Ordering::Acquire) != OPEN,
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
    let count = runtime.resume().await.map_err(|error| match error {
        ResumeError::ShuttingDown => ApiError::new(StatusCode::CONFLICT, "shutting_down",
            "The provider is shutting down; restart the daemon to serve again"),
        ResumeError::Pass(error) => ApiError::internal(error),
    })?;
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
        embeddings_engine_with_slots(model, 1)
    }

    /// `embeddings_engine` reporting `slots` engine slots, for a model of that concurrency.
    fn embeddings_engine_with_slots(model: protocol::ModelSpec, slots: u32) -> Router {
        let props = json!({"build_info": "b1-cd959ba", "total_slots": slots, "model_alias": model.id,
            "default_generation_settings": {"n_ctx": model.context_length}});
        embeddings_engine_with_props(model, props)
    }

    /// `embeddings_engine` whose `/props` answers `props`.
    fn embeddings_engine_with_props(model: protocol::ModelSpec, props: Value) -> Router {
        let id = model.id.clone();
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

    /// `managed-engine-runtime` requirement 8 (task 3.8): before a model on a managed engine is
    /// ready, the engine's own `/props` must report the pinned build, the model's alias, exactly
    /// `max_concurrency` slots and exactly the model's per-slot context. An engine that reduced
    /// or otherwise changed one of them keeps the model not ready, and a refresh with a ready
    /// chain publishes no offer for it, while the matching engine is ready and published. The
    /// engine answers only requests carrying the engine key as bearer (requirement 6), so the
    /// matching case also shows that every readiness probe presents it.
    #[tokio::test]
    async fn an_engine_reporting_another_contract_keeps_the_model_unready_and_unpublished() {
        const ENGINE_KEY: &str = "private-test-engine-key-of-32-characters";
        let cases: [(&str, fn(&mut Value)); 10] = [
            ("the matching engine", |_| {}),
            ("per-slot context 256 for context 512", |props| props["default_generation_settings"]["n_ctx"] = json!(256)),
            ("the total context 1,024 as per-slot context", |props| props["default_generation_settings"]["n_ctx"] = json!(1024)),
            ("one slot for concurrency 2", |props| props["total_slots"] = json!(1)),
            ("three slots for concurrency 2", |props| props["total_slots"] = json!(3)),
            ("another model alias", |props| props["model_alias"] = json!("test/another-model")),
            ("a build of another commit", |props| props["build_info"] = json!("b1-0123456")),
            // A commit too short to identify the pinned revision, though a prefix of it.
            ("a build naming no commit", |props| props["build_info"] = json!("b1-")),
            ("a build naming six characters of the pinned commit", |props| props["build_info"] = json!("b1-cd959b")),
            ("no per-slot context", |props| props["default_generation_settings"] = json!({})),
        ];
        for (case, change) in cases {
            let ready = case == "the matching engine";
            let directory = tempfile::tempdir().unwrap();
            let (mut config, unavailable) = unavailable_provider(directory.path()).await;
            let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
            config.network.rpc_url = rpc;
            // A public embeddings deployment, which a non-development provider publishes
            // through its DHT manifest without a serving profile.
            config.development = false;
            config.public_endpoint = "https://inference.example.org/".into();
            let key_file = directory.path().join("engine.key");
            wallet::create_secret_file(&key_file).unwrap().write_all(ENGINE_KEY.as_bytes()).unwrap();
            let model = &mut config.models[0];
            model.model.capabilities = vec!["embeddings".into()];
            model.model.context_length = 512;
            model.model.max_output_tokens = 0;
            model.price.output_wei_per_1k = "0".into();
            model.max_concurrency = 2;
            model.request_timeout_secs = 30;
            model.engine_api_key_file = Some(key_file);
            let mut props = json!({"build_info": "b1-cd959ba", "total_slots": 2, "model_alias": model.model.id,
                "default_generation_settings": {"n_ctx": 512}});
            change(&mut props);
            let engine = embeddings_engine_with_props(model.model.clone(), props).layer(middleware::from_fn(
                |request: Request, next: Next| async move {
                    let bearer = format!("Bearer {ENGINE_KEY}");
                    if request.headers().get(header::AUTHORIZATION).is_some_and(|value| value.as_bytes() == bearer.as_bytes()) {
                        next.run(request).await
                    } else {
                        StatusCode::UNAUTHORIZED.into_response()
                    }
                },
            ));
            let (engine_url, engine_server) = serve(engine).await;
            model.engine_url = engine_url.clone();
            // The model runs on a managed engine: the helper child stands in for the process,
            // the fixture answers for it.
            let helper = crate::engine_runtime::ManagedEngine::helper_for_test(&model.model.id, &engine_url);
            let engines = std::collections::HashMap::from([(model.model.id.clone(), Arc::new(helper))]);
            let runtime = ProviderRuntime::start_with(config, engines).await.unwrap();

            let health = inference_backend::model_health(&runtime.state).await;
            assert_eq!(health.len(), 1);
            assert_eq!(health[0].ready, ready, "{case}: {}", health[0].detail);
            assert_eq!(runtime.refresh().await, Ok(usize::from(ready)), "{case}");
            assert_eq!(runtime.dht.status().await["published_offers"], usize::from(ready), "{case}");
            let (status, offers) = admin(&runtime, "GET", "/admin/offers").await;
            assert_eq!((status, offers["offers"].as_array().unwrap().len()), (StatusCode::OK, usize::from(ready)), "{case}");
            let (_, view) = admin(&runtime, "GET", "/admin/status").await;
            assert_eq!(view["models"][0]["ready"], ready, "{case}: {view}");
            assert_eq!(runtime.state.funding_ready.load(Ordering::Acquire), ready, "{case}");

            runtime.dht.stop().await;
            runtime.stop_engines().await;
            for server in [engine_server, rpc_server, unavailable] {
                server.abort();
            }
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

    /// Holds the next request that it lets through, on `path` when one is given, until
    /// `release`: a stub engine or chain RPC that is slow exactly once.
    #[derive(Clone)]
    struct Gate {
        path: Option<&'static str>,
        armed: Arc<AtomicBool>,
        entered: Arc<tokio::sync::watch::Sender<bool>>,
        open: Arc<tokio::sync::watch::Sender<bool>>,
    }

    impl Gate {
        fn new(path: Option<&'static str>) -> Self {
            Self {
                path,
                armed: Arc::new(AtomicBool::new(false)),
                entered: Arc::new(tokio::sync::watch::channel(false).0),
                open: Arc::new(tokio::sync::watch::channel(false).0),
            }
        }
        /// Holds the next request that reaches the gate.
        fn arm(&self) {
            self.open.send_replace(false);
            self.entered.send_replace(false);
            self.armed.store(true, Ordering::Release);
        }
        /// Returns once the held request has arrived, within 30 seconds.
        async fn entered(&self) {
            within(self.entered.subscribe().wait_for(|entered| *entered)).await.unwrap();
        }
        fn release(&self) {
            self.open.send_replace(true);
        }
        fn hold(&self, router: Router) -> Router {
            let gate = self.clone();
            router.layer(middleware::from_fn(move |request: Request, next: Next| {
                let gate = gate.clone();
                async move {
                    if gate.path.is_none_or(|path| request.uri().path() == path)
                        && gate.armed.swap(false, Ordering::AcqRel)
                    {
                        gate.entered.send_replace(true);
                        let _ = gate.open.subscribe().wait_for(|open| *open).await;
                    }
                    next.run(request).await
                }
            }))
        }
    }

    /// `future`, which must finish within 30 seconds.
    async fn within<F: std::future::Future>(future: F) -> F::Output {
        tokio::time::timeout(Duration::from_secs(30), future).await.expect("timed out")
    }

    /// Waits until the provider's drain state satisfies `reached`, within 30 seconds.
    async fn drain_state_reaches(runtime: &ProviderRuntime, reached: impl Fn(u8) -> bool) {
        within(async {
            while !reached(runtime.drain_state.load(Ordering::Acquire)) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
    }

    /// Nothing has reopened since the shutdown drain: admission and new funding are closed,
    /// the status reports the drain, `/admin/offers` and the DHT hold no offer, and a new
    /// embedding request under `key` is refused as `provider_paused` before any record.
    async fn assert_shut(runtime: &Arc<ProviderRuntime>, bearer: &str, contract: &str, key: &str) {
        assert!(!runtime.state.accepting.load(Ordering::Acquire), "{key}");
        assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{key}");
        let (status, view) = admin(runtime, "GET", "/admin/status").await;
        assert_eq!(status, StatusCode::OK, "{key}");
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["draining"]),
            (&json!(false), &json!(false), &json!(true)), "{key}: {view}");
        assert_eq!(admin(runtime, "GET", "/admin/offers").await, (StatusCode::OK, json!({"offers": []})), "{key}");
        assert_eq!(runtime.dht.status().await["published_offers"], 0, "{key}");
        let (status, answer) = embed(runtime, bearer, key).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{key}: {answer}");
        assert_eq!(answer["error"]["code"], "provider_paused", "{key}");
        assert!(runtime.state.ledger.request_view(contract, key).is_err(), "{key}");
    }

    /// provider-daemon-lifecycle requirement 11 (task 2.9, design code difference 9): once the
    /// shutdown path has drained, a `POST /admin/resume` and a `POST /admin/offers/refresh` reopen
    /// nothing. In the first case they arrive after the shutdown drain has returned; the resume is
    /// refused (409 `shutting_down`) without ending the drain, and the refresh runs a pass that
    /// keeps admission closed. In the second, the resume was received before: it has ended an
    /// operator drain and waits for the lifecycle lock behind a running pass when the shutdown
    /// drain begins, and is refused when it gets the lock. `POST /admin/drain` and the read-only
    /// routes keep answering.
    #[tokio::test]
    async fn a_resume_during_shutdown_does_not_reopen_the_provider() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        let contract = runtime.state.ledger.list_accounts().unwrap()[0].contract_id.clone();
        assert!(runtime.state.accepting.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        runtime.drain_for_shutdown().await;
        let (status, answer) = admin(&runtime, "POST", "/admin/resume").await;
        assert_eq!(status, StatusCode::CONFLICT, "{answer}");
        assert_eq!(answer["error"]["code"], "shutting_down", "{answer}");
        assert_eq!(admin(&runtime, "POST", "/admin/offers/refresh").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        for path in ["/admin/requests", "/admin/accounts", "/admin/fees", "/admin/runtime"] {
            assert_eq!(admin(&runtime, "GET", path).await.0, StatusCode::OK, "{path}");
        }
        assert_eq!(admin(&runtime, "POST", "/admin/resume").await.0, StatusCode::CONFLICT);
        // A periodic pass that was not yet stopped reopens nothing either.
        runtime.refresh().await.unwrap();
        assert_shut(&runtime, &bearer, &contract, "after-shutdown-drain").await;
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }

        // The operator drains for maintenance, then resumes while a periodic pass runs, and the
        // daemon is told to stop while that resume waits for the pass.
        let directory = tempfile::tempdir().unwrap();
        let chain = Gate::new(None);
        let (runtime, bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            Arc::new(AtomicBool::new(true)), chain.hold(Router::new().fallback(ready_chain)), false).await;
        let contract = runtime.state.ledger.list_accounts().unwrap()[0].contract_id.clone();
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        chain.arm();
        let pass = tokio::spawn({
            let runtime = runtime.clone();
            async move { runtime.refresh().await }
        });
        chain.entered().await;
        let resume = tokio::spawn({
            let runtime = runtime.clone();
            async move { admin(&runtime, "POST", "/admin/resume").await }
        });
        // The resume has ended the operator drain and waits for the lifecycle lock.
        drain_state_reaches(&runtime, |state| state == OPEN).await;
        let shutdown = tokio::spawn({
            let runtime = runtime.clone();
            async move { runtime.drain_for_shutdown().await }
        });
        drain_state_reaches(&runtime, |state| state != OPEN).await;
        chain.release();
        within(pass).await.unwrap().unwrap();
        within(shutdown).await.unwrap();
        let (status, answer) = within(resume).await.unwrap();
        assert_eq!(status, StatusCode::CONFLICT, "{answer}");
        assert_eq!(answer["error"]["code"], "shutting_down", "{answer}");
        assert_eq!(admin(&runtime, "POST", "/admin/offers/refresh").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        assert_shut(&runtime, &bearer, &contract, "racing-resume").await;
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 6 (task 2.10): the failures a resume clears are those
    /// numbered up to the count when it was requested. A later one stays open and is reported,
    /// also when no failure preceded the request; the first open failure is the one reported.
    #[test]
    fn a_resume_clears_only_the_settlement_failures_recorded_before_it() {
        use inference_gateway::{DURABLE_SETTLEMENT_FAILED as DURABLE, SETTLEMENT_CLOCK_FAILED as CLOCK};
        let mut faults = SettlementFaults::default();
        faults.clear_through(0);
        assert_eq!(faults.open, None);
        // None was recorded when the resume was requested; one recorded while it waited stays.
        faults.record(DURABLE);
        faults.clear_through(0);
        assert_eq!(faults.open, Some((1, DURABLE)));
        faults.record(CLOCK);
        assert_eq!(faults.open, Some((1, DURABLE)));
        faults.clear_through(2);
        assert_eq!(faults.open, None);
        // A failure after the request outlives the earlier ones that the resume clears.
        faults.record(DURABLE);
        faults.record(CLOCK);
        faults.clear_through(3);
        assert_eq!(faults.open, Some((4, CLOCK)));
        faults.clear_through(4);
        assert_eq!((faults.open, faults.recorded), (None, 4));
    }

    /// provider-daemon-lifecycle requirement 6 (task 2.10, found in the round-56 review): a
    /// resume clears only the settlement failures recorded before the operator asked for it. A
    /// request is admitted and waits in the engine; a first failure (the accounting clock at
    /// another settlement, recorded as the gateway records it) closes admission; a periodic
    /// pass holds the lifecycle lock; `POST /admin/resume` arrives and waits for it; the
    /// admitted request's settlement then fails as well. After the lock is released, the resume
    /// answers that admission is still closed, nothing is published, and the status names the
    /// second failure. A resume requested after that failure reopens admission.
    #[tokio::test]
    async fn a_resume_keeps_a_settlement_failure_recorded_while_it_waits() {
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        let chain = Gate::new(None);
        let (rpc, rpc_server) = serve(chain.hold(Router::new().fallback(ready_chain))).await;
        config.network.rpc_url = rpc;
        config.development = false;
        config.public_endpoint = "https://inference.example.org/".into();
        let model = &mut config.models[0];
        model.model.capabilities = vec!["embeddings".into()];
        model.model.max_output_tokens = 0;
        model.price.output_wei_per_1k = "0".into();
        // Two slots, so that the pass runs while a request waits in the engine, and time enough
        // for that request to wait.
        model.max_concurrency = 2;
        model.request_timeout_secs = 60;
        let engine = Gate::new(Some("/v1/embeddings"));
        let (engine_url, engine_server) =
            serve(engine.hold(embeddings_engine_with_slots(model.model.clone(), 2))).await;
        model.engine_url = engine_url;
        let runtime = ProviderRuntime::start(config).await.unwrap();
        assert!(runtime.state.accepting.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        let (contract, bearer) = free_account(&runtime.state, "22");
        let (earlier, earlier_bearer) = free_account(&runtime.state, "33");

        engine.arm();
        let admitted = tokio::spawn({
            let runtime = runtime.clone();
            async move { embed(&runtime, &earlier_bearer, "admitted-before-the-failures").await }
        });
        engine.entered().await;
        runtime.state.record_settlement_fault(inference_gateway::SETTLEMENT_CLOCK_FAILED);
        assert!(!runtime.state.accepting.load(Ordering::Acquire));
        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("CREATE TRIGGER refuse_settlement BEFORE UPDATE ON requests
            WHEN OLD.status='reserved' AND NEW.status<>'reserved'
            BEGIN SELECT RAISE(ABORT,'injected settlement write failure'); END;").unwrap();
        chain.arm();
        let pass = tokio::spawn({
            let runtime = runtime.clone();
            async move { runtime.refresh().await }
        });
        chain.entered().await;
        let resume = tokio::spawn({
            let runtime = runtime.clone();
            async move { admin(&runtime, "POST", "/admin/resume").await }
        });
        // The resume has arrived and waits for the lifecycle lock that the pass holds.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!resume.is_finished());
        // While it waits, the admitted request's settlement fails: a second failure.
        engine.release();
        let (status, answer) = within(admitted).await.unwrap();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
        // The first failure is still the one reported until the resume runs.
        assert_eq!(runtime.state.settlement_fault(), Some(inference_gateway::SETTLEMENT_CLOCK_FAILED));
        // The answer arrives after the settlement's drop retried its write once.
        ledger.execute_batch("DROP TRIGGER refuse_settlement").unwrap();
        chain.release();
        within(pass).await.unwrap().unwrap();
        let resumed = within(resume).await.unwrap();
        assert_eq!(resumed, (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!(view["settlement_error"], inference_gateway::DURABLE_SETTLEMENT_FAILED, "{view}");
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["draining"]),
            (&json!(false), &json!(false), &json!(false)), "{view}");
        assert_eq!(admin(&runtime, "GET", "/admin/offers").await, (StatusCode::OK, json!({"offers": []})));
        assert_eq!(runtime.dht.status().await["published_offers"], 0);
        let (status, answer) = embed(&runtime, &bearer, "after-the-first-resume").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{answer}");
        assert_eq!(answer["error"]["code"], "provider_paused");
        assert!(runtime.state.ledger.request_view(&contract, "after-the-first-resume").is_err());
        assert_eq!(runtime.state.ledger.request_view(&earlier, "admitted-before-the-failures").unwrap().status, "reserved");

        // A resume requested after the second failure clears it, and the provider admits again.
        assert_eq!(admin(&runtime, "POST", "/admin/resume").await,
            (StatusCode::OK, json!({"accepting": true, "offers_published": 1})));
        assert_eq!(admin(&runtime, "GET", "/admin/status").await.1["settlement_error"], Value::Null);
        let (status, answer) = embed(&runtime, &bearer, "after-the-second-resume").await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(runtime.state.ledger.request_view(&contract, "after-the-second-resume").unwrap().status, "completed");
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 7 (task 2.5, design code difference 5): a
    /// publication failure in the starting maintenance pass does not stop the start. The
    /// provider restarts over a deployment whose offer revision cannot be persisted, serves
    /// administration and the existing account's inference, is not new-funding ready, publishes
    /// nothing and reports the failed pass. In the first case the revision is exhausted, so later
    /// passes fail the same way; in the second the revision file's replacement path is blocked,
    /// and a later pass publishes once the obstacle is removed.
    #[tokio::test]
    async fn a_publication_failure_at_start_leaves_the_provider_serving_and_reports_it() {
        for exhausted in [true, false] {
            let case = if exhausted { "exhausted revision" } else { "blocked revision file" };
            let directory = tempfile::tempdir().unwrap();
            let (runtime, bearer, servers) =
                embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
            assert_eq!(runtime.dht.status().await["published_offers"], 1, "{case}");
            let contract = runtime.state.ledger.list_accounts().unwrap()[0].contract_id.clone();
            let config = (*runtime.state.config).clone();
            let revision_file = runtime.revision_file.clone();
            runtime.dht.stop().await;
            drop(runtime);
            if exhausted {
                std::fs::write(&revision_file, format!("{}\n", u64::MAX)).unwrap();
            } else {
                std::fs::create_dir(revision_file.with_extension("next")).unwrap();
            }

            let runtime = ProviderRuntime::start(config).await.unwrap();
            let (status, view) = admin(&runtime, "GET", "/admin/status").await;
            assert_eq!(status, StatusCode::OK, "{case}");
            assert_eq!(view["refresh_error"], "Persisting offer revision failed", "{case}: {view}");
            assert_eq!(runtime.refresh_error().await.as_deref(), Some("Persisting offer revision failed"));
            assert_eq!((&view["accepting"], &view["funding_ready"], &view["draining"], &view["settlement_error"]),
                (&json!(true), &json!(false), &json!(false), &Value::Null), "{case}: {view}");
            assert_eq!(view["models"][0]["ready"], true, "{case}: {view}");
            assert_eq!(readyz(&runtime).await, StatusCode::OK, "{case}");
            assert_eq!(admin(&runtime, "GET", "/admin/offers").await, (StatusCode::OK, json!({"offers": []})), "{case}");
            assert_eq!(runtime.dht.status().await["published_offers"], 0, "{case}");
            // The existing account is served.
            let (status, answer) = embed(&runtime, &bearer, "after-a-failed-starting-pass").await;
            assert_eq!(status, StatusCode::OK, "{case}: {answer}");
            assert_eq!(runtime.state.ledger.request_view(&contract, "after-a-failed-starting-pass").unwrap().status,
                "completed", "{case}");

            if exhausted {
                // Later passes fail the same way and keep reporting it; nothing is published.
                assert_eq!(runtime.refresh().await, Err("Persisting offer revision failed".into()), "{case}");
                let view = admin(&runtime, "GET", "/admin/status").await.1;
                assert_eq!(view["refresh_error"], "Persisting offer revision failed", "{case}: {view}");
                assert_eq!(runtime.dht.status().await["published_offers"], 0, "{case}");
            } else {
                // Once the failure clears, a later pass publishes and clears the report.
                std::fs::remove_dir(revision_file.with_extension("next")).unwrap();
                assert_eq!(admin(&runtime, "POST", "/admin/offers/refresh").await,
                    (StatusCode::OK, json!({"accepting": true, "offers_published": 1})), "{case}");
                let view = admin(&runtime, "GET", "/admin/status").await.1;
                assert_eq!((&view["refresh_error"], &view["funding_ready"]), (&Value::Null, &json!(true)), "{case}: {view}");
                assert_eq!(runtime.dht.status().await["published_offers"], 1, "{case}");
            }
            runtime.dht.stop().await;
            for server in servers {
                server.abort();
            }
        }
    }

    /// provider-daemon-lifecycle requirement 8 (task 2.6, design code difference 6): a second
    /// start of a running deployment, with the same data directory, signing key and DHT identity
    /// file and an ephemeral DHT listen address, is refused by the deployment lock before its DHT
    /// node listens or dials. The only bootstrap address is a plain listener that counts the
    /// connections it accepts. Once the first provider has dialed it, the first provider's DHT
    /// node is stopped, while it still holds the lock, so that only another start under this
    /// identity could connect. The refused start makes no connection. Controls: once the first
    /// provider is gone, the same configuration starts and dials; and the identity file is still
    /// loaded under the protected-file rules of `inference-offer-discovery` before any dial.
    #[tokio::test]
    async fn a_second_provider_for_a_deployment_is_refused_before_it_joins_the_network() {
        use std::sync::atomic::AtomicUsize;
        let directory = tempfile::tempdir().unwrap();
        let (mut config, server) = unavailable_provider(directory.path()).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let bootstrap = tokio::spawn({
            let accepted = accepted.clone();
            async move {
                // Held open and never answered, so that no dialer completes a handshake.
                let mut held = Vec::new();
                while let Ok((stream, _)) = listener.accept().await {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    held.push(stream);
                }
            }
        });
        let bootstrap_peer = libp2p::identity::Keypair::generate_ed25519().public().to_peer_id();
        config.dht.bootstrap = vec![format!("/ip4/127.0.0.1/tcp/{port}/p2p/{bootstrap_peer}")];
        let connections_after = |count: usize| {
            let accepted = accepted.clone();
            async move {
                within(async {
                    while accepted.load(Ordering::SeqCst) <= count {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
            }
        };

        let first = ProviderRuntime::start(config.clone()).await.unwrap();
        let peer = first.dht.peer_id();
        connections_after(0).await;
        first.dht.stop().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let before = accepted.load(Ordering::SeqCst);
        let refused = ProviderRuntime::start(config.clone()).await.err();
        assert_eq!(refused.as_deref(), Some("another provider owns this deployment data directory"));
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(accepted.load(Ordering::SeqCst), before,
            "the refused start connected to its bootstrap peer under the deployment's identity");

        // Control: once the owner has gone, the same configuration starts, under the same peer
        // identity, and dials its bootstrap peer.
        drop(first);
        let restarted = ProviderRuntime::start(config.clone()).await.unwrap();
        assert_eq!(restarted.dht.peer_id(), peer);
        connections_after(before).await;
        restarted.dht.stop().await;
        drop(restarted);

        // The identity file rules still apply before anything joins the network.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let identity = &config.dht.identity_file;
            std::fs::set_permissions(identity, std::fs::Permissions::from_mode(0o644)).unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            let before = accepted.load(Ordering::SeqCst);
            let refused = ProviderRuntime::start(config.clone()).await.err().unwrap_or_default();
            assert!(refused.contains("owner-only"), "{refused}");
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert_eq!(accepted.load(Ordering::SeqCst), before);
        }
        bootstrap.abort();
        server.abort();
    }

    /// provider-daemon-lifecycle requirement 2 (task 2.7, design code difference 7):
    /// `/admin/runtime` reports engine process liveness as `alive`, never as readiness. An
    /// operator-owned engine whose model is ready is reported unmanaged, without a liveness claim
    /// or observation, and not as not ready. A live managed engine whose model fails its readiness
    /// probe is reported alive and not ready; once it exits, not alive and not ready.
    #[tokio::test]
    async fn admin_runtime_reports_engine_liveness_without_calling_it_readiness() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, _bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        assert_eq!(admin(&runtime, "GET", "/admin/status").await.1["models"][0]["ready"], true);
        assert_eq!(admin(&runtime, "GET", "/admin/runtime").await, (StatusCode::OK, json!({"models": [
            {"model_id": "model", "managed": false, "alive": null, "ready": true, "observation": null}]})));
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }

        let directory = tempfile::tempdir().unwrap();
        let healthy = Arc::new(AtomicBool::new(true));
        let (runtime, _bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            healthy.clone(), Router::new().fallback(ready_chain), true).await;
        let pid = runtime.state.managed_engines["model"].observation.pid;
        let facts = |model: &Value| (model["managed"].clone(), model["alive"].clone(), model["ready"].clone());
        let model = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
        assert_eq!(facts(&model), (json!(true), json!(true), json!(true)), "{model}");
        assert_eq!(model["observation"]["pid"], pid, "{model}");
        // The engine process stays alive while its model fails the readiness probe.
        healthy.store(false, Ordering::Release);
        assert_eq!(runtime.refresh().await, Ok(0));
        assert_eq!(admin(&runtime, "GET", "/admin/status").await.1["models"][0]["ready"], false);
        let model = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
        assert_eq!(facts(&model), (json!(true), json!(true), json!(false)), "{model}");
        assert_eq!(model["observation"]["pid"], pid, "{model}");
        // An exited engine is not alive, and its model is not ready.
        healthy.store(true, Ordering::Release);
        assert_eq!(runtime.refresh().await, Ok(1));
        runtime.state.managed_engines["model"].kill_for_test();
        let model = within(async {
            loop {
                let model = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
                if model["alive"] == false {
                    break model;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        assert_eq!(facts(&model), (json!(true), json!(false), json!(false)), "{model}");
        assert_eq!(model["observation"]["pid"], pid, "{model}");
        runtime.stop_engines().await;
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// Files named `name` anywhere under `root`.
    fn files_named(root: &std::path::Path, name: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_owned()];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() { pending.push(path); } else if path.file_name() == Some(std::ffi::OsStr::new(name)) { found.push(path); }
            }
        }
        found
    }

    /// The process identifiers that `ManagedEngine::recording_helper_for_test` engines have
    /// logged to `log`, one per engine process started.
    fn engine_starts(log: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// Kills process `pid`, a helper engine, when the test panics before the helper is known to
    /// be gone, so that a failed assertion does not leave it running.
    struct KillOnPanic(u32);
    impl KillOnPanic {
        /// Called once the helper is killed or known to be gone, so that a panic afterwards
        /// never kills another process that reused its identifier.
        fn disarm(self) {
            std::mem::forget(self);
        }
    }
    impl Drop for KillOnPanic {
        fn drop(&mut self) {
            #[cfg(unix)]
            if std::thread::panicking() {
                unsafe { libc::kill(self.0 as i32, libc::SIGKILL) };
            }
            #[cfg(not(unix))]
            let _ = self.0;
        }
    }

    /// Whether process `pid` is gone (reaped) within five seconds.
    #[cfg(unix)]
    async fn gone_within_five_seconds(pid: u32) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(pid as i32, 0) } == 0 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        unsafe { libc::kill(pid as i32, 0) == -1 }
    }

    /// `managed-engine-runtime` requirement 9 (task 3.9), *One failed model stops the whole
    /// start*. A ready, non-development provider whose first model runs on a managed engine (the
    /// recording helper child), and whose second, managed model fails its inventory, does not
    /// start: `ProviderRuntime::start_with` returns that inventory error after one launch attempt,
    /// the second model's, which it does not retry; its handle to the first engine is dropped, and
    /// the first engine process no longer exists within five seconds. No
    /// offer was signed (the deployment's persisted offer revision, written before any offer is
    /// signed, does not exist), and the deployment lock is free again. The control: the first
    /// model alone starts with the same configuration and publishes its offer under revision 1.
    /// The daemon-level case, where the daemon itself launched and readied the first engine, is
    /// `daemon_engine_process_test`'s `check_and_start_verify_every_managed_model`.
    #[tokio::test]
    async fn a_failed_managed_launch_fails_the_start_and_stops_the_engine_already_running() {
        const ENGINE_KEY: &str = "private-test-engine-key-of-32-characters";
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
        config.network.rpc_url = rpc;
        config.development = false;
        config.public_endpoint = "https://inference.example.org/".into();
        let first = &mut config.models[0];
        first.model.capabilities = vec!["embeddings".into()];
        first.model.max_output_tokens = 0;
        first.price.output_wei_per_1k = "0".into();
        let (engine_url, engine_server) = serve(embeddings_engine(first.model.clone())).await;
        first.engine_url = engine_url.clone();
        let first_id = first.model.id.clone();
        let alone = config.clone();
        // The second model: a managed CPU runtime whose installed directories hold no inventory.
        let key_file = directory.path().join("engine-b.key");
        wallet::create_secret_file(&key_file).unwrap().write_all(ENGINE_KEY.as_bytes()).unwrap();
        let (runtime_dir, artifact_dir) = (directory.path().join("runtime-b"), directory.path().join("artifact-b"));
        std::fs::create_dir(&runtime_dir).unwrap();
        std::fs::create_dir(&artifact_dir).unwrap();
        let mut second = config.models[0].clone();
        second.model.id = "embed-b".into();
        // Held through the test, so that no other test's launch names the same endpoint.
        let second_port = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        second.engine_url = format!("http://{}", second_port.local_addr().unwrap());
        let second_url = second.engine_url.clone();
        second.engine_api_key_file = Some(key_file);
        second.runtime = Some(crate::engine_runtime::EngineRuntimeConfig {
            runtime_dir, artifact_dir, backend: "cpu".into(), device_ids: vec![], memory_reserve_mib: 0, gpu_layers: 0,
            split_mode: "none".into(), tensor_split: vec![], cache_type_k: "f16".into(), cache_type_v: "f16".into(),
            flash_attention: false, batch_size: 128, ubatch_size: 64, startup_timeout_secs: 1 });
        let inventory_error = crate::engine_runtime::validate_inventory(&second).err().unwrap();
        assert_eq!(inventory_error, "cannot inspect runtime metadata");
        config.models.push(second);
        config.validate().unwrap();

        let spawn_log = directory.path().join("engine-spawns");
        let helper = Arc::new(crate::engine_runtime::ManagedEngine::recording_helper_for_test(&first_id, &engine_url, &spawn_log));
        let pid = helper.observation.pid;
        let _cleanup = KillOnPanic(pid);
        let handle = Arc::downgrade(&helper);
        within(async { while engine_starts(&spawn_log).is_empty() { tokio::time::sleep(Duration::from_millis(20)).await; } }).await;
        assert!(helper.is_alive());
        let attempts = |url: &str| crate::engine_runtime::ManagedEngine::launch_attempts_for_test(url);
        let before = (attempts(&engine_url), attempts(&second_url));
        let started = ProviderRuntime::start_with(config, std::collections::HashMap::from([(first_id.clone(), helper)])).await;
        assert_eq!(started.err(), Some(inventory_error));
        assert!(handle.upgrade().is_none(), "the failed start kept its handle to the first engine");
        #[cfg(unix)]
        assert!(gone_within_five_seconds(pid).await, "the first engine outlived the failed start");
        _cleanup.disarm();
        assert_eq!(engine_starts(&spawn_log), [pid.to_string()]);
        // One launch attempt, the second model's failed one: no retry, and nothing launched the
        // first model, whose engine was already running.
        assert_eq!((attempts(&engine_url) - before.0, attempts(&second_url) - before.1), (0, 1));
        assert!(files_named(&directory.path().join("data"), "offer-revision").is_empty(), "the failed start signed an offer");

        // The control: the first model alone starts, which also shows the deployment lock free.
        let helper = crate::engine_runtime::ManagedEngine::helper_for_test(&first_id, &engine_url);
        let runtime = ProviderRuntime::start_with(alone, std::collections::HashMap::from([(first_id, Arc::new(helper))])).await.unwrap();
        assert!(runtime.state.funding_ready.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        let revisions = files_named(&directory.path().join("data"), "offer-revision");
        assert_eq!(revisions.len(), 1, "{revisions:?}");
        assert_eq!(std::fs::read_to_string(&revisions[0]).unwrap().trim(), "1");
        runtime.dht.stop().await;
        runtime.stop_engines().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// `managed-engine-runtime` requirement 10 (task 3.10), with requirement 1's no-fallback
    /// scenario (task 3.1) and the engine-exit cases of `inference-offer-discovery` tasks 3.11
    /// and 3.15. A ready, non-development provider serves one embeddings model on a managed
    /// engine: the recording helper child, which logs each start of an engine process, with the
    /// fixture endpoint answering for it and counting every request that reaches it. The offer is
    /// published and a request has completed. When the engine process is killed:
    /// - within one second, admission and new-funding readiness are closed, `GET /admin/runtime`
    ///   reports the model not ready, `GET /readyz` answers 503 and the model is not admissible;
    /// - `GET /readyz` answers 503 with `ready` and `funding_ready` false, and the offer is
    ///   withdrawn from the DHT, from `GET /admin/offers` and from a consumer's discovery;
    /// - a maintenance refresh, `POST /admin/offers/refresh` and `POST /admin/resume` leave all of
    ///   that so and publish nothing, and a request for the model is refused before any record;
    /// - nothing reaches the engine's endpoint after the exit, no engine process starts again,
    ///   no launch of an engine for that endpoint is even attempted (`ManagedEngine::start` is
    ///   not called, which the helper's start log alone cannot show, because the model carries no
    ///   `runtime` from which a launch could spawn), and the provider keeps the exited engine's
    ///   handle;
    /// - `GET /admin/status`, `GET /admin/accounts`, `GET /admin/fees` and `GET /admin/requests`
    ///   answer 200 with the accounts, fee summary and request records unchanged, and the
    ///   consumer's `GET /v1/requests/<id>` still recovers the completed request.
    #[tokio::test]
    async fn a_managed_engine_exit_withdraws_service_and_nothing_restarts_or_replaces_it() {
        use std::sync::atomic::AtomicUsize;
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
        config.network.rpc_url = rpc;
        config.development = false;
        config.public_endpoint = "https://inference.example.org/".into();
        let model = &mut config.models[0];
        model.model.capabilities = vec!["embeddings".into()];
        model.model.max_output_tokens = 0;
        model.price.output_wei_per_1k = "0".into();
        let contacted = Arc::new(AtomicUsize::new(0));
        let engine = embeddings_engine(model.model.clone()).layer(middleware::from_fn({
            let contacted = contacted.clone();
            move |request: Request, next: Next| {
                let contacted = contacted.clone();
                async move {
                    contacted.fetch_add(1, Ordering::SeqCst);
                    next.run(request).await
                }
            }
        }));
        let (engine_url, engine_server) = serve(engine).await;
        model.engine_url = engine_url.clone();
        let spec = model.model.clone();
        let spawn_log = directory.path().join("engine-spawns");
        let helper = crate::engine_runtime::ManagedEngine::recording_helper_for_test(&spec.id, &engine_url, &spawn_log);
        let pid = helper.observation.pid;
        let _cleanup = KillOnPanic(pid);
        let runtime = ProviderRuntime::start_with(config, std::collections::HashMap::from([(spec.id.clone(), Arc::new(helper))])).await.unwrap();
        within(async { while engine_starts(&spawn_log).is_empty() { tokio::time::sleep(Duration::from_millis(20)).await; } }).await;
        assert_eq!(engine_starts(&spawn_log), [pid.to_string()]);
        let (contract, bearer) = free_account(&runtime.state, "22");

        let readiness = || {
            let runtime = runtime.clone();
            async move {
                let response = runtime.public_router().oneshot(Request::builder().uri("/readyz")
                    .body(axum::body::Body::empty()).unwrap()).await.unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
                (status, serde_json::from_slice::<Value>(&bytes).unwrap())
            }
        };
        let address = format!("{}/p2p/{}",
            runtime.dht.status().await["listening_addresses"][0].as_str().unwrap(), runtime.dht.peer_id());
        let network = runtime.state.config.network.identity.clone();
        let discovered = |name: &'static str| {
            let (address, network, directory) = (address.clone(), network.clone(), directory.path().to_owned());
            async move {
                let consumer = DhtService::start(
                    provider_config::DhtConfig { listen: "/ip4/127.0.0.1/tcp/0".into(), bootstrap: vec![address], identity_file: directory.join(name) },
                    network.clone(),
                    Arc::new(OfferRegistry::new(network, false)),
                ).await.unwrap();
                let offers = consumer.discover(None).await.unwrap();
                consumer.stop().await;
                offers.len()
            }
        };

        // Ready, admitted and published, and one request completes.
        assert!(runtime.state.accepting.load(Ordering::Acquire));
        assert!(runtime.state.funding_ready.load(Ordering::Acquire));
        assert_eq!(readiness().await, (StatusCode::OK, json!({"ready": true, "funding_ready": true})));
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        assert_eq!(admin(&runtime, "GET", "/admin/offers").await.1["offers"].as_array().unwrap().len(), 1);
        assert_eq!(discovered("consumer-before.key").await, 1);
        let view = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
        assert_eq!((&view["alive"], &view["ready"]), (&json!(true), &json!(true)), "{view}");
        let (status, answer) = embed(&runtime, &bearer, "before-exit").await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let (status, accounts) = admin(&runtime, "GET", "/admin/accounts").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(accounts["accounts"].as_array().unwrap().len(), 1, "{accounts}");
        let (status, fees) = admin(&runtime, "GET", "/admin/fees").await;
        assert_eq!(status, StatusCode::OK);
        let (status, requests) = admin(&runtime, "GET", "/admin/requests").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(requests["requests"].as_array().unwrap().len(), 1, "{requests}");
        let request_status = || {
            let (runtime, bearer) = (runtime.clone(), bearer.clone());
            async move {
                let response = runtime.public_router().oneshot(Request::builder().uri("/v1/requests/before-exit")
                    .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                    .body(axum::body::Body::empty()).unwrap()).await.unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
                (status, serde_json::from_slice::<Value>(&bytes).unwrap())
            }
        };
        let (status, completed) = request_status().await;
        assert_eq!((status, &completed["status"]), (StatusCode::OK, &json!("completed")), "{completed}");

        // The engine process exits; nothing has reached its endpoint since.
        let reached = contacted.load(Ordering::SeqCst);
        // Launch attempts for the engine's endpoint: the helper was not launched through
        // `ManagedEngine::start`, and a restart or replacement of any kind would be one.
        let attempts = crate::engine_runtime::ManagedEngine::launch_attempts_for_test(&engine_url);
        // The helper is killed next, and the provider's handle collects its exit.
        _cleanup.disarm();
        let killed = std::time::Instant::now();
        runtime.state.managed_engines[&spec.id].kill_for_test();
        let observed = loop {
            let view = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
            if !runtime.state.accepting.load(Ordering::Acquire)
                && !runtime.state.funding_ready.load(Ordering::Acquire)
                && view["ready"] == false
                && runtime.state.qualified_model(&spec).is_none()
                && readiness().await.0 == StatusCode::SERVICE_UNAVAILABLE
            {
                break killed.elapsed();
            }
            assert!(killed.elapsed() < Duration::from_secs(1), "the engine exit was not observed within one second: {view}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert!(observed < Duration::from_secs(1), "{observed:?}");

        let withdrawn = |case: &'static str| {
            let (runtime, readiness, discovered, spawn_log, spec, contract, bearer, engine_url) =
                (runtime.clone(), &readiness, &discovered, &spawn_log, &spec, &contract, &bearer, &engine_url);
            async move {
                assert!(!runtime.state.accepting.load(Ordering::Acquire), "{case}");
                assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{case}");
                assert!(runtime.state.qualified_model(spec).is_none(), "{case}");
                assert_eq!(readiness().await, (StatusCode::SERVICE_UNAVAILABLE, json!({"ready": false, "funding_ready": false})), "{case}");
                let view = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
                assert_eq!((&view["managed"], &view["alive"], &view["ready"], &view["observation"]["pid"]),
                    (&json!(true), &json!(false), &json!(false), &json!(pid)), "{case}: {view}");
                let (status, view) = admin(&runtime, "GET", "/admin/status").await;
                assert_eq!(status, StatusCode::OK, "{case}");
                assert_eq!((&view["accepting"], &view["funding_ready"]), (&json!(false), &json!(false)), "{case}: {view}");
                assert_eq!((&view["models"][0]["ready"], &view["models"][0]["detail"]),
                    (&json!(false), &json!(inference_backend::ENGINE_EXITED)), "{case}: {view}");
                assert_eq!(runtime.dht.status().await["published_offers"], 0, "{case}");
                assert_eq!(admin(&runtime, "GET", "/admin/offers").await, (StatusCode::OK, json!({"offers": []})), "{case}");
                // No fallback: the request is refused before any record, and no other engine,
                // runtime or process serves it.
                let key = format!("after-exit-{}", case.replace(' ', "-"));
                let (status, answer) = embed(&runtime, bearer, &key).await;
                assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{case}: {answer}");
                assert_eq!(answer["error"]["code"], "provider_paused", "{case}: {answer}");
                assert!(runtime.state.ledger.request_view(contract, &key).is_err(), "{case}");
                // The handle is the exited engine's, and no engine process started again.
                let engine = &runtime.state.managed_engines[&spec.id];
                assert_eq!(engine.observation.pid, pid, "{case}");
                assert!(!engine.is_alive(), "{case}");
                assert_eq!(engine_starts(spawn_log), [pid.to_string()], "{case}: an engine process started again");
                assert_eq!(crate::engine_runtime::ManagedEngine::launch_attempts_for_test(engine_url), attempts,
                    "{case}: an engine launch was attempted again");
                assert_eq!(discovered(if case == "the exit" { "consumer-exit.key" } else { "consumer-resume.key" }).await, 0, "{case}");
            }
        };
        withdrawn("the exit").await;
        assert_eq!(runtime.refresh().await, Ok(0));
        let (status, refreshed) = admin(&runtime, "POST", "/admin/offers/refresh").await;
        assert_eq!((status, refreshed), (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        let (status, resumed) = admin(&runtime, "POST", "/admin/resume").await;
        assert_eq!((status, resumed), (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        // Several engine-watcher periods, in which a restart would have logged a start.
        tokio::time::sleep(Duration::from_millis(500)).await;
        withdrawn("refresh and resume").await;
        assert_eq!(contacted.load(Ordering::SeqCst), reached, "a request reached the exited engine's endpoint");
        // Administration and recovery answer, and the accepted account, the fee summary and the
        // request records are unchanged: the completed request is still recorded, for the
        // operator and for the consumer's request status.
        assert_eq!(admin(&runtime, "GET", "/admin/accounts").await, (StatusCode::OK, accounts));
        assert_eq!(admin(&runtime, "GET", "/admin/fees").await, (StatusCode::OK, fees));
        assert_eq!(admin(&runtime, "GET", "/admin/requests").await, (StatusCode::OK, requests));
        assert_eq!(request_status().await, (StatusCode::OK, completed));

        runtime.dht.stop().await;
        runtime.stop_engines().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// outbound-http-safeguards requirement 6 scenario "The public listener serves no
    /// administration route" (task 3.6): every administration route, sent to the public
    /// router with the valid administration bearer, is an unknown endpoint, and no
    /// administrative action runs: the provider keeps accepting and its offer stays published.
    #[tokio::test]
    async fn the_public_router_serves_no_administration_route() {
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let (runtime, _, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        assert!(runtime.state.accepting.load(Ordering::Acquire));
        let funding_ready = runtime.state.funding_ready.load(Ordering::Acquire);
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        for (method, path) in [
            ("GET", "/admin/status"),
            ("GET", "/admin/offers"),
            ("POST", "/admin/drain"),
            ("GET", "/admin/runtime"),
            ("GET", "/admin/accounts"),
            ("GET", "/admin/requests"),
            ("GET", "/admin/fees"),
            ("POST", "/admin/resume"),
            ("POST", "/admin/offers/refresh"),
            ("POST", "/admin/fees/remit"),
        ] {
            let response = runtime.public_router().oneshot(Request::builder().method(method).uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {OPERATOR_TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from("{}")).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
            let answer: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(answer["error"]["code"], "not_found", "{method} {path}: {answer}");
        }
        assert!(runtime.state.accepting.load(Ordering::Acquire));
        assert_eq!(runtime.state.funding_ready.load(Ordering::Acquire), funding_ready);
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        // Positive control: the same drain on the administration router closes admission.
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        assert!(!runtime.state.accepting.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 0);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// outbound-http-safeguards requirement 6 scenario "Only the administration token
    /// authorizes administration" (task 3.6). No `Authorization`, a wrong token, an account
    /// API bearer issued by this provider, the valid bearer with `Origin: http://localhost`,
    /// the account owner's wallet control headers and signature, the bare token without the
    /// `Bearer` scheme, and the token with a suffix are each refused as unauthorized (HTTP 401,
    /// an implementation value of design decision 10) on every administration route, before
    /// any administrative action: `POST /admin/drain` leaves the provider accepting with its
    /// offer published.
    #[tokio::test]
    async fn only_the_administration_bearer_without_an_origin_authorizes_administration() {
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let (runtime, account_bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        // The account bearer is genuine: the public router accepts it.
        let listing = runtime.public_router().oneshot(Request::builder().uri("/v1/models")
            .header(header::AUTHORIZATION, format!("Bearer {account_bearer}"))
            .body(axum::body::Body::empty()).unwrap()).await.unwrap();
        assert_eq!(listing.status(), StatusCode::OK);
        // The account owner's wallet signs the request it sends.
        let owner_key = "22".repeat(32);
        let owner = wallet::address_from_private_key(&owner_key).unwrap();
        let signature = wallet::sign_message(&owner_key, b"POST /admin/drain").unwrap();
        let operator = format!("Bearer {OPERATOR_TOKEN}");
        let account = format!("Bearer {account_bearer}");
        let extended = format!("Bearer {OPERATOR_TOKEN}x");
        let attempts: [(&str, Vec<(&str, &str)>); 7] = [
            ("no Authorization", vec![]),
            ("a wrong token", vec![("authorization", "Bearer wrong-operator-token-of-32-bytes")]),
            ("an account API bearer", vec![("authorization", account.as_str())]),
            (
                "the administration bearer with a browser Origin",
                vec![("authorization", operator.as_str()), ("origin", "http://localhost")],
            ),
            (
                "wallet owner-control headers",
                vec![
                    ("x-chiral-owner", owner.as_str()),
                    ("x-chiral-nonce", "owner-nonce"),
                    ("x-chiral-signature", signature.as_str()),
                ],
            ),
            ("the token without the Bearer scheme", vec![("authorization", OPERATOR_TOKEN)]),
            ("the token followed by more characters", vec![("authorization", extended.as_str())]),
        ];
        assert!(runtime.state.accepting.load(Ordering::Acquire));
        assert_eq!(runtime.dht.status().await["published_offers"], 1);
        // Every administration route, so that a route mounted outside the authentication
        // layer is caught, not only the three routes the scenario names.
        let routes = [
            ("GET", "/admin/status"),
            ("GET", "/admin/runtime"),
            ("GET", "/admin/offers"),
            ("GET", "/admin/accounts"),
            ("GET", "/admin/requests"),
            ("GET", "/admin/fees"),
            ("POST", "/admin/drain"),
            ("POST", "/admin/resume"),
            ("POST", "/admin/offers/refresh"),
            ("POST", "/admin/fees/remit"),
        ];
        for (case, headers) in &attempts {
            for (method, path) in routes {
                let mut request = Request::builder().method(method).uri(path);
                for (name, value) in headers {
                    request = request.header(*name, *value);
                }
                let response = runtime.admin_router()
                    .oneshot(request.body(axum::body::Body::empty()).unwrap()).await.unwrap();
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{case}: {method} {path}");
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
                let answer: Value = serde_json::from_slice(&bytes).unwrap();
                assert!(answer.get("offers").is_none() && answer.get("accepting").is_none(), "{case}: {answer}");
            }
            assert!(runtime.state.accepting.load(Ordering::Acquire), "{case}");
            assert_eq!(runtime.dht.status().await["published_offers"], 1, "{case}");
        }
        // Positive control: the administration bearer alone authorizes the same drain.
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        assert!(!runtime.state.accepting.load(Ordering::Acquire));
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// outbound-http-safeguards requirement 6, task 3.6: the administration token is read
    /// only from a protected file (book lines 474 and 658). A copy readable by group or
    /// others refuses the start, and the error names no token.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_administration_token_is_read_only_from_a_protected_file() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let (config, server) = unavailable_provider(directory.path()).await;
        std::fs::set_permissions(&config.admin_token_file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error = ProviderRuntime::start(config.clone()).await.err().unwrap();
        assert_eq!(error, "secret file must be owned by this user with no group/other permissions");
        assert!(!error.contains(OPERATOR_TOKEN));
        std::fs::set_permissions(&config.admin_token_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let runtime = ProviderRuntime::start(config).await.unwrap();
        runtime.dht.stop().await;
        server.abort();
    }

    /// `method path` on the public router, with `bearer` and the JSON `body` when given: the
    /// status, and the response's headers and body as one text.
    async fn public_text(
        runtime: &Arc<ProviderRuntime>,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        use tower::ServiceExt;
        let mut request = Request::builder().method(method).uri(path);
        if let Some(bearer) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        }
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json").header("idempotency-key", protocol::random_id());
        }
        let body = body.map_or_else(axum::body::Body::empty, |body| axum::body::Body::from(body.to_string()));
        let response = runtime.public_router().oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let headers = format!("{:?}", response.headers());
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, format!("{headers}\n{}", String::from_utf8_lossy(&bytes)))
    }

    /// outbound-http-safeguards requirement 7 scenario "The engine origin is never advertised"
    /// (task 3.7). A non-development provider with `public_endpoint` `https://provider.example`
    /// serves one embeddings model from a loopback engine fixture. (A provider probes its
    /// engine when it starts, so an engine at the scenario's `http://10.0.0.5:8080` would need
    /// LAN traffic; the fixture's own origin is the private `engine_url` here, and the failing
    /// engine names `http://10.0.0.5:8080`, its own origin and a private marker in its error
    /// body and in a header.) The published signed offer advertises exactly
    /// `https://provider.example`; `/healthz`, `/readyz`, `/v1/models`,
    /// `/api/version-policy`, a signed quote for that offer, an embedding request that succeeds, one whose dispatch the engine
    /// fails (502 `engine_error`), one whose engine readiness check before tokenization the
    /// engine fails (503 `engine_unavailable`), and `/readyz` after the failing engine
    /// made the model not ready contain no part of the engine origin, no loopback address,
    /// no `10.0.0.5` and nothing of the engine's error.
    #[tokio::test]
    async fn the_engine_origin_is_never_advertised_or_echoed() {
        const MARKER: &str = "engine-private-detail";
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
        config.network.rpc_url = rpc;
        config.development = false;
        config.public_endpoint = "https://provider.example".into();
        let model = &mut config.models[0];
        model.model.capabilities = vec!["embeddings".into()];
        model.model.max_output_tokens = 0;
        model.price.output_wei_per_1k = "0".into();
        // 0 serves; 1 fails `/v1/embeddings`, the dispatch; 2 fails every request, so the
        // engine readiness check before tokenization fails.
        let mode = Arc::new(AtomicU8::new(0));
        let failures = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (engine_mode, engine_failures) = (mode.clone(), failures.clone());
        let engine = embeddings_engine(model.model.clone()).layer(middleware::from_fn(
            move |request: Request, next: Next| {
                let (mode, failures) = (engine_mode.clone(), engine_failures.clone());
                async move {
                    let mode = mode.load(Ordering::Acquire);
                    if mode == 2 || (mode == 1 && request.uri().path() == "/v1/embeddings") {
                        failures.fetch_add(1, Ordering::SeqCst);
                        let host = request.headers().get(header::HOST)
                            .and_then(|host| host.to_str().ok()).unwrap_or("").to_string();
                        let detail = format!("{MARKER}: engine http://{host} (http://10.0.0.5:8080) failed");
                        (StatusCode::INTERNAL_SERVER_ERROR, [("x-engine-detail", detail.clone())],
                            Json(json!({"error": {"code": "engine_error", "message": detail}}))).into_response()
                    } else {
                        next.run(request).await
                    }
                }
            },
        ));
        let (engine_url, engine_server) = serve(engine).await;
        model.engine_url = engine_url.clone();
        let engine_authority = engine_url.trim_start_matches("http://").to_string();
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let (_, bearer) = free_account(&runtime.state, "22");
        let leaks = |text: &str| -> Vec<String> {
            [engine_url.as_str(), engine_authority.as_str(), "127.0.0.1", "10.0.0.5", MARKER]
                .into_iter()
                .filter(|part| text.contains(part))
                .map(str::to_string)
                .collect()
        };

        // The signed offer advertises only the public endpoint.
        let (status, offers) = admin(&runtime, "GET", "/admin/offers").await;
        assert_eq!(status, StatusCode::OK);
        let mut offers = offers["offers"].as_array().unwrap().clone();
        assert_eq!(offers.len(), 1);
        // The registry the DHT announces from holds the same signed offer.
        let registry = runtime.state.offers.list();
        assert_eq!(registry.len(), 1);
        offers.push(serde_json::to_value(&registry[0]).unwrap());
        for offer in &offers {
            assert_eq!(offer["payload"]["endpoint"], "https://provider.example");
            assert!(leaks(&offer.to_string()).is_empty(), "{offer}");
        }

        let mut answers = Vec::new();
        for (method, path, credential) in [
            ("GET", "/healthz", None),
            ("GET", "/readyz", None),
            ("GET", "/v1/models", Some(bearer.as_str())),
            ("GET", "/api/version-policy", None),
        ] {
            answers.push((format!("{method} {path}"), public_text(&runtime, method, path, credential, None).await));
        }
        // A quote for the published offer is a signed public answer too. A new ledger spends
        // its first minute of quote-signing work when it initializes, so the first proposal
        // is refused (429); the test then moves that spent minute into the past.
        let proposal = json!({"offer_id": protocol::offer_id(&registry[0].payload).unwrap(),
            "consumer_wallet": wallet::address_from_private_key(&"22".repeat(32)).unwrap(), "funding_wei": "1000"});
        let refused = public_text(&runtime, "POST", "/v1/contracts/propose", None, Some(proposal.clone())).await;
        answers.push(("POST /v1/contracts/propose on a new ledger".into(), refused));
        let ledger = ledger_connection(&runtime);
        let minute: u64 = ledger.query_row("SELECT window FROM rate_limits WHERE bucket='quote-work:global'",
            [], |row| row.get(0)).unwrap();
        ledger.execute("UPDATE rate_limits SET window=?1 WHERE bucket='quote-work:global'", [minute - 1]).unwrap();
        ledger.execute("UPDATE metadata SET value=?1 WHERE key='quote_work_minute'", [(minute - 1).to_string()]).unwrap();
        let quote = public_text(&runtime, "POST", "/v1/contracts/propose", None, Some(proposal)).await;
        assert!(quote.1.contains(r#""signature""#), "{}", quote.1);
        answers.push(("POST /v1/contracts/propose".into(), quote));
        let embedding = json!({"model": "model", "input": "hello"});
        for engine_mode in [0, 1, 2] {
            mode.store(engine_mode, Ordering::Release);
            let answer = public_text(&runtime, "POST", "/v1/embeddings", Some(&bearer), Some(embedding.clone())).await;
            answers.push((format!("POST /v1/embeddings in engine mode {engine_mode}"), answer));
        }
        assert_eq!(failures.load(Ordering::SeqCst), 2, "the engine failed the dispatch and the readiness check");
        // The failing engine makes the model not ready at the next pass.
        let _ = runtime.refresh().await;
        answers.push(("GET /readyz after the engine failed".into(), public_text(&runtime, "GET", "/readyz", None, None).await));
        let statuses: Vec<_> = answers.iter().map(|(request, (status, _))| (request.as_str(), status.as_u16())).collect();
        assert_eq!(statuses, [
            ("GET /healthz", 200),
            ("GET /readyz", 200),
            ("GET /v1/models", 200),
            ("GET /api/version-policy", 200),
            ("POST /v1/contracts/propose on a new ledger", 429),
            ("POST /v1/contracts/propose", 200),
            ("POST /v1/embeddings in engine mode 0", 200),
            ("POST /v1/embeddings in engine mode 1", 502),
            ("POST /v1/embeddings in engine mode 2", 503),
            ("GET /readyz after the engine failed", 503),
        ]);
        // The provider's own fixed classification, not the engine's text.
        assert!(answers[7].1 .1.contains(r#""code":"engine_error""#), "{}", answers[7].1 .1);
        assert!(answers[8].1 .1.contains(r#""code":"engine_unavailable""#), "{}", answers[8].1 .1);
        for (request, (_, text)) in &answers {
            assert!(leaks(text).is_empty(), "{request} exposes {:?}: {text}", leaks(text));
        }
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// public-inference-api requirement 1 (task 3.1), through the merged public router
    /// (`ProviderRuntime::public_router`) of a ready, non-development provider with one external
    /// embedding deployment and one free account. Its private engine records every request it
    /// receives; besides `/tokenize`, `/props`, `/health`, `/v1/models` and `/v1/embeddings` it
    /// answers every other path, `/apply-template`, `/metrics`, `/slots` and `/completion`
    /// included, with a private marker, so a forwarded path would reach it.
    /// - Engine paths (tokenization, template rendering, properties, metrics, slots and a slot
    ///   action, the native completion, infill, embedding and rerank routes and their aliases,
    ///   adapter loading, health, model loading and management) and arbitrary paths, each with
    ///   `GET` and `POST`, with no bearer, an invalid one and the account's valid bearer, are
    ///   answered as an unknown endpoint (404 `not_found`, design decision 4) without the
    ///   marker.
    /// - The out-of-scope OpenAI surfaces (Responses, images, audio, video, files and uploads,
    ///   batches, fine-tuning), with `GET` and `POST` and the valid bearer, are answered the same
    ///   way.
    ///
    /// Throughout, the engine receives no request, no request is recorded and the account is
    /// unchanged. Positive control: the same router serves the six implemented endpoints:
    /// `GET /v1/models` lists the model, `POST /v1/embeddings` completes through the engine,
    /// `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel` answer for that request, and
    /// `POST /v1/chat/completions` and `POST /v1/completions` are routed, and refused before
    /// reservation because the model advertises only `embeddings`.
    #[tokio::test]
    async fn the_public_router_serves_no_engine_or_out_of_scope_route() {
        use tower::ServiceExt;
        const MARKER: &str = "private-engine-route";
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
        config.network.rpc_url = rpc;
        config.development = false;
        config.public_endpoint = "https://inference.example.org/".into();
        let model = &mut config.models[0];
        model.model.capabilities = vec!["embeddings".into()];
        model.model.max_output_tokens = 0;
        model.price.output_wei_per_1k = "0".into();
        let reached = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let engine = embeddings_engine(model.model.clone())
            .fallback(|| async { Json(json!({ "route": MARKER })) })
            .layer(middleware::from_fn({
                let reached = reached.clone();
                move |request: Request, next: Next| {
                    let reached = reached.clone();
                    async move {
                        reached.lock().unwrap().push(format!("{} {}", request.method(), request.uri()));
                        next.run(request).await
                    }
                }
            }));
        let (engine_url, engine_server) = serve(engine).await;
        model.engine_url = engine_url;
        let runtime = ProviderRuntime::start(config).await.unwrap();
        assert!(runtime.health.read().await[0].ready);
        let (contract, bearer) = free_account(&runtime.state, "22");
        let app = runtime.public_router();
        let observed = || json!({
            "engine": reached.lock().unwrap().clone(),
            "account": runtime.state.ledger.account(&contract).unwrap(),
            "requests": runtime.state.ledger.list_requests().unwrap(),
        });
        let call = |method: &str, path: &str, credential: Option<&str>| {
            let mut request = Request::builder().method(method).uri(path);
            if let Some(credential) = credential {
                request = request.header(header::AUTHORIZATION, format!("Bearer {credential}"));
            }
            let body = if method == "POST" {
                request = request.header(header::CONTENT_TYPE, "application/json")
                    .header("idempotency-key", protocol::random_id());
                axum::body::Body::from(json!({"model": "model", "input": "hello"}).to_string())
            } else {
                axum::body::Body::empty()
            };
            let app = app.clone();
            let request = request.body(body).unwrap();
            let credential = credential.map(str::to_owned);
            async move {
                let response = app.oneshot(request).await.unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
                let text = String::from_utf8_lossy(&bytes).into_owned();
                // Checked before any failure message can show the answer.
                assert!(credential.is_none_or(|credential| !text.contains(&credential)), "the answer contains the bearer");
                (status, text)
            }
        };
        let before = observed();
        let engine_paths = [
            "/tokenize", "/detokenize", "/apply-template", "/props", "/metrics", "/slots",
            "/slots/0?action=erase", "/completion", "/completions", "/chat/completions", "/infill",
            "/embedding", "/embeddings", "/rerank", "/v1/rerank", "/lora-adapters", "/health",
            "/models", "/models/load", "/v1/engine/props", "/engine/v1/chat/completions",
        ];
        let out_of_scope = [
            "/v1/responses", "/v1/images/generations", "/v1/images/edits", "/v1/audio/transcriptions",
            "/v1/audio/translations", "/v1/audio/speech", "/v1/videos", "/v1/files", "/v1/uploads",
            "/v1/batches", "/v1/fine_tuning/jobs",
        ];
        let credentials = [None, Some("invalid-account-bearer"), Some(bearer.as_str())];
        let cases = engine_paths.iter().flat_map(|path| credentials.iter().map(move |credential| (*path, *credential)))
            .chain(out_of_scope.iter().map(|path| (*path, Some(bearer.as_str()))));
        for (path, credential) in cases {
            for method in ["GET", "POST"] {
                let case = format!("{method} {path} with {}", match credential {
                    None => "no bearer",
                    Some(credential) if credential == bearer => "the valid bearer",
                    Some(_) => "an invalid bearer",
                });
                let (status, text) = call(method, path, credential).await;
                assert!(!text.contains(&bearer), "{case}: the answer contains the bearer");
                assert_eq!(status, StatusCode::NOT_FOUND, "{case}: {text}");
                let answer: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(answer, json!({"error": {"message": "Unknown API endpoint",
                    "type": "invalid_request_error", "param": null, "code": "not_found"}}), "{case}");
                assert!(!text.contains(MARKER), "{case}: forwarded to the engine");
                assert_eq!(observed(), before, "{case}: the engine, the account or the requests changed");
            }
        }

        // Positive control: the implemented endpoints are served by the same router.
        let (status, text) = call("GET", "/v1/models", Some(&bearer)).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["data"][0]["id"], "model");
        let response = app.clone().oneshot(Request::builder().method("POST").uri("/v1/embeddings")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "served")
            .body(axum::body::Body::from(json!({"model": "model", "input": "hello"}).to_string()))
            .unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let engine = reached.lock().unwrap().clone();
        assert_eq!(engine[before["engine"].as_array().unwrap().len()..], ["GET /health", "POST /tokenize", "POST /v1/embeddings"]);
        for (method, path) in [("GET", "/v1/requests/served"), ("POST", "/v1/requests/served/cancel")] {
            let (status, text) = call(method, path, Some(&bearer)).await;
            assert_eq!(status, StatusCode::OK, "{method} {path}: {text}");
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["status"], "completed", "{method} {path}");
        }
        for path in ["/v1/chat/completions", "/v1/completions"] {
            let (status, text) = call("POST", path, Some(&bearer)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {text}");
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["error"]["code"], "unsupported_parameter");
        }
        assert_eq!(*reached.lock().unwrap(), engine);
        assert_eq!(runtime.state.ledger.list_requests().unwrap().len(), 1);
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }
}
