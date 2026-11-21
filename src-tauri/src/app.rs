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

/// The period of `ProviderRuntime::maintenance`, the periodic pass (`inference-offer-discovery`'s
/// 30 seconds). Unit tests shorten it, so that a test can observe periodic passes
/// (provider-daemon-lifecycle task 3.5); every other build uses 30 seconds.
#[cfg(not(test))]
const MAINTENANCE_PERIOD: Duration = Duration::from_secs(30);
#[cfg(test)]
const MAINTENANCE_PERIOD: Duration = Duration::from_millis(100);

/// The test-only pause points of a maintenance pass (provider-daemon-lifecycle task 3.5), both
/// after the pass has decided admission and before it publishes.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassPoint {
    /// The pass has decided admission and new-funding readiness and has not yet stored them.
    Decided,
    /// The pass has stored its decision and has not yet published.
    Stored,
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
    /// Test-only pause point of `refresh_inner` (provider-daemon-lifecycle task 3.5): when armed,
    /// the next pass that reaches the armed `PassPoint` reports on the sender and continues once
    /// the receiver resolves.
    #[cfg(test)]
    pass_pause: std::sync::Mutex<Option<(PassPoint, tokio::sync::oneshot::Sender<()>, tokio::sync::oneshot::Receiver<()>)>>,
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
            #[cfg(test)]
            pass_pause: std::sync::Mutex::new(None),
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
        #[cfg(test)]
        self.pause_for_test(PassPoint::Decided).await;
        // A durable settlement failure keeps admission, and so new funding and publication,
        // closed until the operator resumes (provider-daemon-lifecycle requirement 6).
        let funding_ready = self.state.store_admission(accepting, chain_ready && funding_safe);
        #[cfg(test)]
        self.pause_for_test(PassPoint::Stored).await;
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

    /// The test-only pause point `point` of a pass, when it is the armed one: reports that the
    /// pass has reached it and waits until the test lets the pass continue.
    #[cfg(test)]
    async fn pause_for_test(&self, point: PassPoint) {
        let armed = {
            let mut pause = self.pass_pause.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if pause.as_ref().is_some_and(|(armed, _, _)| *armed == point) { pause.take() } else { None }
        };
        if let Some((_, paused, resume)) = armed {
            let _ = paused.send(());
            let _ = resume.await;
        }
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
            let mut interval = tokio::time::interval(MAINTENANCE_PERIOD);
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
    /// An exited managed engine is read here too, as `/admin/runtime` and `/readyz` read it:
    /// while a pass holds the lifecycle lock, the engine watcher cannot yet replace the latest
    /// pass's entry (provider-daemon-lifecycle requirement 1, design code difference 10).
    async fn model_status(&self) -> Vec<Value> {
        let health = self.health.read().await;
        health.iter().map(|model| {
            let (ready, reason) = if self.state.engine_alive(&model.model_id) {
                (model.ready, model.detail.as_str())
            } else {
                (false, inference_backend::ENGINE_EXITED)
            };
            let withdrawn = self.state.config.models.iter()
                .find(|configured| configured.model.id == model.model_id)
                .and_then(|configured| self.state.ledger.performance_withdrawn(&configured.model).ok());
            let detail = if withdrawn == Some(true) {
                format!("{PERFORMANCE_WITHDRAWN} Latest readiness result: {reason}")
            } else {
                reason.to_owned()
            };
            json!({"model_id": model.model_id, "ready": ready, "detail": detail,
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
        // provider-daemon-lifecycle task 3.8: refused by the deployment lock, not by another fault,
        // and before it opens the deployment's ledger. Another connection holds the ledger's write
        // lock meanwhile, so a start that opened the ledger first would wait for it and fail with
        // "database is locked" instead.
        let writer = ledger_connection(&first);
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert_eq!(ProviderRuntime::start(config.clone()).await.err().as_deref(),
            Some("another provider owns this deployment data directory"));
        writer.execute_batch("ROLLBACK").unwrap();
        drop(writer);
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
        let opened = serde_json::to_value(first.state.ledger.account(&quote.payload.id).unwrap()).unwrap();
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
        // Task 3.8, scenario "A restarted daemon serves the same deployment": the whole account,
        // its balance and status included, is unchanged.
        assert_eq!(serde_json::to_value(restarted.state.ledger.account(&quote.payload.id).unwrap()).unwrap(), opened);
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
        chat_engine_holding(model, template, None)
    }

    /// Holds one streamed chat request of `chat_engine_holding` open after its first token.
    #[derive(Clone)]
    struct HeldStream {
        entered: Arc<tokio::sync::watch::Sender<bool>>,
        open: Arc<tokio::sync::watch::Sender<bool>>,
    }

    impl HeldStream {
        fn new() -> Self {
            Self {
                entered: Arc::new(tokio::sync::watch::channel(false).0),
                open: Arc::new(tokio::sync::watch::channel(false).0),
            }
        }
        /// Returns once the held stream has sent its first token, within 30 seconds.
        async fn entered(&self) {
            within(self.entered.subscribe().wait_for(|entered| *entered)).await.unwrap();
        }
        /// Lets the held stream finish.
        fn release(&self) {
            self.open.send_replace(true);
        }
        /// The engine's answer to the held request: its token `OK` with its sampled ID, then,
        /// once released, a `stop` finish, exact usage (two prompt tokens, one completion
        /// token) and `[DONE]`.
        fn answer(&self, model: Value) -> Response {
            use futures_util::StreamExt;
            let chunk = |choices: Value| json!({"id": "held", "object": "chat.completion.chunk",
                "model": model, "created": 1, "choices": choices});
            let token = chunk(json!([{"index": 0, "delta": {"content": "OK"}, "finish_reason": null,
                "logprobs": {"content": [{"id": 42, "token": "OK", "bytes": [79, 75], "logprob": -0.5,
                    "top_logprobs": []}]}}]));
            let finish = chunk(json!([{"index": 0, "delta": {}, "finish_reason": "stop"}]));
            let mut usage = chunk(json!([]));
            usage["usage"] = json!({"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3});
            let first = axum::body::Bytes::from(format!("data: {token}\n\n"));
            let rest = axum::body::Bytes::from(format!("data: {finish}\n\ndata: {usage}\n\ndata: [DONE]\n\n"));
            let held = self.clone();
            let body = futures_util::stream::iter([Ok::<_, std::convert::Infallible>(first)])
                .chain(futures_util::stream::once(async move {
                    held.entered.send_replace(true);
                    let _ = held.open.subscribe().wait_for(|open| *open).await;
                    Ok(rest)
                }));
            ([(header::CONTENT_TYPE, "text/event-stream")], axum::body::Body::from_stream(body)).into_response()
        }
    }

    /// `chat_engine`, which, given `held`, answers a chat request whose user message is `held`
    /// with `HeldStream::answer`: an admitted stream that the test holds open.
    fn chat_engine_holding(model: protocol::ModelSpec, template: &'static str, held: Option<HeldStream>) -> Router {
        let id = model.id.clone();
        let props = json!({"build_info": "b1-cd959ba", "total_slots": 1, "model_alias": model.id,
            "default_generation_settings": {"n_ctx": model.context_length}, "chat_template": template});
        Router::new()
            .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
            .route("/v1/models", get(move || async move { Json(json!({"data": [{"id": id}]})) }))
            .route("/props", get(move || async move { Json(props) }))
            .route("/apply-template", post(|| async { Json(json!({"prompt": "rendered"})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens": [1, 2]})) }))
            .route("/v1/chat/completions", post(move |request: Json<Value>| {
                let held = held.clone().filter(|_| request["messages"][0]["content"] == "held");
                async move {
                    if let Some(held) = held {
                        return held.answer(request["model"].clone());
                    }
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
                }
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
        // `/readyz` and the status see the exit once the process is gone (design code difference
        // 10); the engine watcher then closes admission, records the exit and withdraws.
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
        watcher_recorded_exit(&runtime).await;
        let model = admin(&runtime, "GET", "/admin/status").await.1["models"][0].clone();
        assert_eq!((&model["ready"], &model["detail"]), (&json!(false), &json!(inference_backend::ENGINE_EXITED)), "{model}");
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

    /// Waits, within 30 seconds, until the engine watcher has replaced the first model's entry of
    /// the latest pass with the engine exit. It does so under the lifecycle lock, after it has
    /// closed admission and new funding again, and just before it withdraws the published offers.
    /// `/admin/status` reports the exit as soon as the process is gone, before the watcher acts
    /// (design code difference 10), so it is no signal that the watcher has acted.
    async fn watcher_recorded_exit(runtime: &ProviderRuntime) {
        within(async {
            while runtime.health.read().await.first().is_none_or(|model| model.detail != inference_backend::ENGINE_EXITED) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
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

    /// public-inference-api requirement 2 (task 3.2), scenarios "A stock client uses both modes
    /// with only its bearer" and "The base URL is the origin plus /v1", short of a run of the
    /// OpenAI SDKs themselves: a started development provider whose chat and completions model
    /// passed its real readiness probes against `ProbeEngine` serves its merged public router on a
    /// real HTTP/1 listener. Requests shaped as the OpenAI Python SDK sends them (its base URL is
    /// the origin plus `/v1`, joined with `/models`, `/chat/completions`, `/completions` and
    /// `/requests/:id`; its own `Accept`, `User-Agent` and `X-Stainless-*` headers; the exported
    /// bearer; no `X-Chiral-Client-Version` and no `Idempotency-Key`) reach the listing, both
    /// generation routes in both modes, and request status. Non-streaming answers are one JSON
    /// object; streams are `text/event-stream` ending with `[DONE]`, with exactly one usage
    /// event when `stream_options.include_usage` asks and none otherwise. No answer carries
    /// engine instrumentation.
    #[tokio::test]
    async fn sdk_shaped_requests_under_the_origin_plus_v1_reach_the_listing_and_both_generation_routes() {
        use crate::inference_backend::tests::{ProbeEngine, PROBE_TEMPLATE};
        use sha2::{Digest, Sha256};
        let directory = tempfile::tempdir().unwrap();
        let (mut config, unavailable) = unavailable_provider(directory.path()).await;
        let model = &mut config.models[0];
        model.model.capabilities = vec!["chat".into(), "completions".into()];
        model.model.chat_template_sha256 = hex::encode(Sha256::digest(PROBE_TEMPLATE));
        model.request_timeout_secs = 30;
        let engine = ProbeEngine::start(&model.model, 1).await;
        model.engine_url = engine.base.clone();
        let runtime = ProviderRuntime::start(config).await.unwrap();
        assert!(runtime.health.read().await[0].ready, "{:?}", runtime.health.read().await);
        let (_, bearer) = free_account(&runtime.state, "22");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = runtime.public_router();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let base_url = format!("{origin}/v1");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let sdk = |request: reqwest::RequestBuilder| request
            .bearer_auth(&bearer)
            .header("accept", "application/json")
            .header("user-agent", "OpenAI/Python 1.99.9")
            .header("x-stainless-lang", "python")
            .header("x-stainless-package-version", "1.99.9")
            .header("x-stainless-os", "MacOS")
            .header("x-stainless-arch", "arm64")
            .header("x-stainless-runtime", "CPython")
            .header("x-stainless-runtime-version", "3.12.7")
            .header("x-stainless-async", "false")
            .header("x-stainless-retry-count", "0")
            .header("x-stainless-read-timeout", "120.0");
        // The answer's status, content type, request identifier and text.
        let send = |request: reqwest::RequestBuilder| async move {
            let response = request.send().await.unwrap();
            let status = response.status().as_u16();
            let header = |name: &str| response.headers().get(name).map(|value| value.to_str().unwrap().to_owned());
            let (content_type, id) = (header("content-type"), header("x-request-id"));
            let text = response.text().await.unwrap();
            for private in ["token_ids", "timings", "__verbose", "chiral_echo", "n_probs"] {
                assert!(!text.contains(private), "{private} in an answer: {text}");
            }
            (status, content_type, id, text)
        };
        let events = |text: &str| -> (Vec<Value>, bool) {
            let data: Vec<&str> = text.split("\n\n").filter_map(|event| event.strip_prefix("data: ")).collect();
            (data.iter().filter(|data| **data != "[DONE]").map(|data| serde_json::from_str(data).unwrap()).collect(),
                data.last() == Some(&"[DONE]"))
        };

        let (status, _, _, text) = send(sdk(client.get(format!("{base_url}/models")))).await;
        assert_eq!(status, 200, "{text}");
        let listing: Value = serde_json::from_str(&text).unwrap();
        assert_eq!((listing["object"].as_str(), listing["data"][0]["id"].as_str()), (Some("list"), Some("model")), "{text}");
        let mut identifiers = Vec::new();
        for (route, output) in [("chat/completions", "OK"), ("completions", "B")] {
            let chat = route == "chat/completions";
            let mut body = if chat {
                json!({"model":"model","messages":[{"role":"user","content":"Say OK"}],"max_tokens":4})
            } else {
                json!({"model":"model","prompt":"A","max_tokens":4})
            };
            let (status, content_type, id, text) = send(sdk(client.post(format!("{base_url}/{route}"))).json(&body)).await;
            assert_eq!((status, content_type.as_deref()), (200, Some("application/json")), "{route}: {text}");
            let answer: Value = serde_json::from_str(&text).unwrap();
            let object = if chat { "chat.completion" } else { "text_completion" };
            let generated = if chat { &answer["choices"][0]["message"]["content"] } else { &answer["choices"][0]["text"] };
            assert_eq!((answer["object"].as_str(), generated.as_str()), (Some(object), Some(output)), "{route}: {text}");
            assert_eq!(answer["usage"], json!({"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}), "{route}");
            identifiers.push(id.unwrap());
            for include_usage in [true, false] {
                body["stream"] = json!(true);
                body["stream_options"] = json!({"include_usage":include_usage});
                let (status, content_type, id, text) = send(sdk(client.post(format!("{base_url}/{route}"))).json(&body)).await;
                assert_eq!((status, content_type.as_deref()), (200, Some("text/event-stream")), "{route}: {text}");
                let (events, done) = events(&text);
                assert!(done, "{route}: {text}");
                let streamed: String = events.iter().filter_map(|event| {
                    let choice = event["choices"].get(0)?;
                    if chat { choice["delta"]["content"].as_str() } else { choice["text"].as_str() }
                }).collect();
                assert_eq!(streamed, output, "{route}: {text}");
                let usage: Vec<&Value> = events.iter().filter(|event| !event["usage"].is_null()).collect();
                assert_eq!(usage.len(), usize::from(include_usage), "{route}, include_usage {include_usage}: {text}");
                if include_usage {
                    assert_eq!(events.last().unwrap()["choices"], json!([]), "{route}: {text}");
                }
                identifiers.push(id.unwrap());
            }
        }
        for id in identifiers {
            let (status, _, _, text) = send(sdk(client.get(format!("{base_url}/requests/{id}")))).await;
            assert_eq!(status, 200, "{text}");
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["status"], "completed", "{text}");
        }
        server.abort();
        runtime.dht.stop().await;
        unavailable.abort();
    }

    /// One chat request with `Idempotency-Key: key` through the public router.
    async fn chat_request(runtime: &Arc<ProviderRuntime>, bearer: &str, key: &str) -> (StatusCode, Value) {
        use tower::ServiceExt;
        let response = runtime.public_router().oneshot(Request::builder().method("POST")
            .uri("/v1/chat/completions")
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", key)
            .body(axum::body::Body::from(json!({"model":"model","messages":[{"role":"user","content":"hello"}],
                "max_tokens":4}).to_string()))
            .unwrap()).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// Asserts that the only model of `runtime` is not ready: its health entry is not ready, it is
    /// not qualified and the provider is not new-funding ready; no offer is published (the DHT
    /// reports none, and `/admin/offers` and the bearer's `GET /v1/models` list none); and a chat
    /// request is refused before it reaches the engine or is recorded.
    async fn assert_not_ready_admitted_or_published(runtime: &Arc<ProviderRuntime>,
        engine: &crate::inference_backend::tests::ProbeEngine, bearer: &str, case: &str) {
        let key = &case.replace(|c: char| !c.is_ascii_alphanumeric(), "-");
        let health = runtime.health.read().await.clone();
        assert!(!health[0].ready && health[0].detail.starts_with("Not ready"), "{key}: {health:?}");
        assert!(runtime.state.qualified_model(&runtime.state.config.models[0].model).is_none(), "{key}");
        assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{key}");
        assert_eq!(runtime.dht.status().await["published_offers"], 0, "{key}");
        assert_eq!(offers_and_listing(runtime, bearer).await, (vec![], vec![]), "{key}");
        let seen = engine.requests();
        let (status, answer) = chat_request(runtime, bearer, key).await;
        assert_eq!((status, answer["error"]["code"].as_str()), (StatusCode::SERVICE_UNAVAILABLE, Some("provider_paused")), "{key}: {answer}");
        assert_eq!(engine.requests(), seen, "{key}: the refused request reached the engine");
        assert!(runtime.state.ledger.list_requests().unwrap().is_empty(), "{key}: a request was recorded");
    }

    /// public-inference-api requirement 8 (task 3.8) through the provider, with scenarios "A
    /// different template is not ready", "A healthy engine whose probe fails is not ready", "A
    /// tool probe that produces no call" and "A structured-output probe that does not conform".
    /// A development provider with a ready chain and one qualified (profiled, so DHT-published)
    /// model advertising `chat`, `tools`, `structured_output` and `streaming`, on `ProbeEngine`:
    /// - with the fault from the start (a served template of another digest, on an
    ///   operator-owned and on a managed engine; a tool probe answered with text; a schema probe
    ///   answered with non-conforming JSON; streams without final usage), the model is not ready,
    ///   not admitted and not published;
    /// - once the fault is gone, a refresh makes it ready and publishes one offer advertising
    ///   those capabilities;
    /// - when the fault returns while the model is idle, the next refresh withdraws it: it is
    ///   no longer ready, admitted or listed, and its offer is withdrawn.
    #[tokio::test]
    async fn a_failed_readiness_probe_keeps_a_model_unadmitted_and_unpublished_and_withdraws_a_ready_one() {
        use crate::inference_backend::tests::{ProbeEngine, PROBE_TEMPLATE};
        use sha2::{Digest, Sha256};
        for (fault, managed) in [("template", false), ("template", true), ("tool-text", false),
            ("schema-extra", false), ("no-final-usage", false)] {
            let case = format!("{fault} on {} engine", if managed { "a managed" } else { "an operator-owned" });
            let directory = tempfile::tempdir().unwrap();
            let (mut config, unavailable) = unavailable_provider(directory.path()).await;
            let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
            config.network.rpc_url = rpc;
            let (mut model, mut report) = crate::performance::evidence_tests::fixture();
            model.capabilities = ["chat", "tools", "structured_output", "streaming"].map(String::from).to_vec();
            model.chat_template_sha256 = hex::encode(Sha256::digest(PROBE_TEMPLATE));
            let version = crate::engine_runtime::ENGINE_VERSION;
            model.serving.as_mut().unwrap().engine_version = version.into();
            report["runtime_observation"]["engine_version"] = json!(version);
            let evidence = directory.path().join("evidence.json");
            crate::performance::evidence_tests::persist(&mut model, &mut report, &evidence);
            let engine = ProbeEngine::start(&model, 1).await;
            engine.set_fault(fault);
            let deployment = &mut config.models[0];
            deployment.model = model.clone();
            deployment.engine_url = engine.base.clone();
            deployment.qualification_evidence_file = Some(evidence);
            deployment.request_timeout_secs = 30;
            let mut engines = std::collections::HashMap::new();
            if managed {
                let helper = crate::engine_runtime::ManagedEngine::helper_for_test(&model.id, &engine.base);
                engines.insert(model.id.clone(), Arc::new(helper));
            }
            let runtime = ProviderRuntime::start_with(config, engines).await.unwrap();
            let (_, bearer) = free_account(&runtime.state, "22");
            assert_not_ready_admitted_or_published(&runtime, &engine, &bearer, &format!("{case}: at start")).await;

            engine.set_fault("");
            assert_eq!(runtime.refresh().await, Ok(1), "{case}");
            assert!(runtime.health.read().await[0].ready, "{case}");
            assert!(runtime.state.qualified_model(&model).is_some(), "{case}");
            assert_eq!(runtime.dht.status().await["published_offers"], 1, "{case}");
            assert_eq!(offers_and_listing(&runtime, &bearer).await,
                (vec![json!(["chat", "tools", "structured_output", "streaming"])], vec![json!("model")]), "{case}");

            engine.set_fault(fault);
            assert_eq!(runtime.refresh().await, Ok(0), "{case}");
            assert_not_ready_admitted_or_published(&runtime, &engine, &bearer, &format!("{case}: withdrawn")).await;
            runtime.dht.stop().await;
            runtime.stop_engines().await;
            for server in [rpc_server, unavailable] {
                server.abort();
            }
        }
    }

    // provider-daemon-lifecycle acceptance evidence, tasks 3.1 to 3.6.

    /// The hash of block 10, the `safe` block of `ready_chain` and `settled_chain`.
    fn safe_block_hash() -> String {
        format!("0x{}", "56".repeat(32))
    }

    /// The receipts that `settled_chain` answers, by lower-case transaction hash.
    type Receipts = Arc<std::sync::Mutex<std::collections::HashMap<String, Value>>>;

    /// `ready_chain`, which also answers the receipt of each transaction in `receipts`, executed
    /// successfully in block 10, the `safe` block, and that block by its number, with a timestamp
    /// fixed when the stub is made, so that the two observations of one payment agree. A credited
    /// payment then rechecks as accepted, and a signed payment verifies as canonical.
    fn settled_chain(receipts: Receipts) -> Router {
        let fixed = protocol::now().unwrap() - 2;
        Router::new().fallback(move |request: Json<Value>| {
            let receipts = receipts.clone();
            async move {
                let method = request["method"].as_str().unwrap_or("").to_owned();
                let result = match method.as_str() {
                    "eth_getTransactionReceipt" => receipts.lock().unwrap()
                        .get(&request["params"][0].as_str().unwrap_or("").to_ascii_lowercase())
                        .cloned().unwrap_or(Value::Null),
                    "eth_getBlockByNumber" if request["params"][0] == "0xa" =>
                        json!({"number": "0xa", "hash": safe_block_hash(), "timestamp": format!("0x{fixed:x}")}),
                    _ => return ready_chain(request).await,
                };
                Json(json!({"jsonrpc": "2.0", "id": request["id"], "result": result})).into_response()
            }
        })
    }

    /// Signs a legacy payment of `value` wei carrying `data`, from the wallet of signing key
    /// `consumer` repeated with sender nonce `nonce`, to the provider, and adds its successful
    /// receipt in the `safe` block to `receipts`.
    fn signed_payment(state: &ProviderState, receipts: &Receipts, consumer: &str, nonce: u64, value: u128,
        data: &[u8]) -> crate::chain_client::SubmittedTransaction {
        let key = consumer.repeat(32);
        let payment = crate::chain_client::sign_transaction(&key, state.config.network.identity.chain_id, nonce,
            &state.wallet, value, data, 1, 21_000 + 40 * data.len() as u64).unwrap();
        receipts.lock().unwrap().insert(payment.tx_hash.to_ascii_lowercase(), json!({
            "transactionHash": payment.tx_hash, "from": wallet::address_from_private_key(&key).unwrap(),
            "to": state.wallet, "type": "0x0", "gasUsed": "0x5208", "status": "0x1",
            "blockNumber": "0xa", "blockHash": safe_block_hash()}));
        payment
    }

    /// Stores a quote of the first model for the wallet of signing key `consumer` repeated, free
    /// when `funding` is zero and otherwise at the configured price.
    fn stored_quote(state: &ProviderState, consumer: &str, funding: u128) -> Signed<Quote> {
        stored_quote_for(state, consumer, funding, &state.config.models[0].model)
    }

    /// `stored_quote` with `model` as the agreement's terms, such as an earlier report of the
    /// configured model under which the account was opened.
    fn stored_quote_for(state: &ProviderState, consumer: &str, funding: u128, model: &protocol::ModelSpec)
        -> Signed<Quote> {
        let now = protocol::now().unwrap();
        let configured = &state.config.models[0];
        let price = if funding == 0 {
            PriceSchedule { input_wei_per_1k: "0".into(), output_wei_per_1k: "0".into() }
        } else {
            configured.price.clone()
        };
        let quote = protocol::sign(protocol::QUOTE_DOMAIN, Quote {
            id: protocol::random_id(), network: state.config.network.identity.clone(),
            offer_id: protocol::random_id(), provider_wallet: state.wallet.clone(),
            consumer_wallet: wallet::address_from_private_key(&consumer.repeat(32)).unwrap(),
            model: model.clone(), price, funding_wei: funding.to_string(),
            platform_wallet: state.config.platform_wallet.clone(), fee_bps: 50, max_concurrency: 1,
            nonce: protocol::random_id(), issued_at: now, expires_at: now + 300,
        }, &state.private_key).unwrap();
        state.ledger.store_quote(&quote, now).unwrap();
        quote
    }

    /// Acknowledges the funding intent of the paid `quote` for its signed opening payment, with
    /// sender nonce 0, as an admitted `POST /v1/contracts/<id>/funding-intent` does: the payment
    /// and the acknowledgement.
    fn acknowledged_opening(state: &ProviderState, receipts: &Receipts, consumer: &str, quote: &Signed<Quote>)
        -> (crate::chain_client::SubmittedTransaction, Value) {
        let opening = protocol::opening_data(&quote.payload).unwrap();
        let payment = signed_payment(state, receipts, consumer, 0, quote.payload.funding_wei.parse().unwrap(), &opening);
        let acknowledgement = state.ledger.register_funding_intent(quote, &quote.payload.consumer_wallet,
            &payment.raw_transaction, &payment.tx_hash, 0, &state.private_key, protocol::now, true,
            &quote.payload.model).unwrap();
        (payment, serde_json::to_value(acknowledgement).unwrap())
    }

    /// Opens a paid account through an acknowledged opening of `funding` wei, whose canonical
    /// evidence (block 10 of `settled_chain`) is a ledger input here: its contract, its default
    /// credential and the opening's transaction hash.
    fn funded_account(state: &ProviderState, receipts: &Receipts, consumer: &str, funding: u128)
        -> (String, protocol::Credential, String) {
        funded_account_for(state, receipts, consumer, funding, &state.config.models[0].model)
    }

    /// `funded_account` with `model` as the agreement's terms.
    fn funded_account_for(state: &ProviderState, receipts: &Receipts, consumer: &str, funding: u128,
        model: &protocol::ModelSpec) -> (String, protocol::Credential, String) {
        let quote = stored_quote_for(state, consumer, funding, model);
        let (payment, _) = acknowledged_opening(state, receipts, consumer, &quote);
        let evidence = crate::chain_client::VerifiedPayment {
            network: state.config.network.identity.clone(), tx_hash: payment.tx_hash.clone(),
            from: quote.payload.consumer_wallet.clone(), to: state.wallet.clone(), value_wei: funding.to_string(),
            data: protocol::opening_data(&quote.payload).unwrap(), block_hash: safe_block_hash(), block_number: 10,
            raw_transaction: payment.raw_transaction,
        };
        let opened = state.ledger.open_account(&quote, Some(&evidence), protocol::now().unwrap()).unwrap();
        (quote.payload.id.clone(), opened.credential.unwrap(), payment.tx_hash)
    }

    /// When the ledger last checked the credited funding `hash`.
    fn funding_checked(runtime: &ProviderRuntime, hash: &str) -> u64 {
        ledger_connection(runtime).query_row("SELECT checked FROM funding WHERE tx_hash=?1", [hash],
            |row| row.get::<_, i64>(0)).unwrap() as u64
    }

    /// Marks the credited funding `hash` as never checked, so that a pass that rechecks it moves its
    /// checked time forward, even within the same second: the time before that pass.
    fn unchecked(runtime: &ProviderRuntime, hash: &str) -> u64 {
        ledger_connection(runtime).execute("UPDATE funding SET checked=0 WHERE tx_hash=?1", [hash]).unwrap();
        protocol::now().unwrap()
    }

    /// `method path` on `router` with `headers` and, when given, a JSON `body`: the status and the
    /// body.
    async fn send(router: &Router, method: &str, path: &str, headers: &[(String, String)], body: Option<&Value>)
        -> (StatusCode, Vec<u8>) {
        use tower::ServiceExt;
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                axum::body::Body::from(body.to_string())
            }
            None => axum::body::Body::empty(),
        };
        let response = router.clone().oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        (status, axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap().to_vec())
    }

    /// `send`, with the answer as JSON (`null` when it is not JSON).
    async fn call(router: &Router, method: &str, path: &str, headers: &[(String, String)], body: Option<&Value>)
        -> (StatusCode, Value) {
        let (status, bytes) = send(router, method, path, headers, body).await;
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// The `Authorization` header for `bearer`.
    fn bearer_header(bearer: &str) -> Vec<(String, String)> {
        vec![("authorization".into(), format!("Bearer {bearer}"))]
    }

    /// The `Authorization` header for `bearer` with `Idempotency-Key: key`.
    fn keyed(bearer: &str, key: &str) -> Vec<(String, String)> {
        let mut headers = bearer_header(bearer);
        headers.push(("idempotency-key".into(), key.into()));
        headers
    }

    /// `POST path` with `body` on `public`, proved by the owner, the wallet of signing key
    /// `consumer` repeated, with a one-use challenge of `public` signed by that key.
    async fn owner_post(public: &Router, state: &ProviderState, consumer: &str, path: &str, body: Value)
        -> (StatusCode, Value) {
        owner_call(public, state, consumer, "POST", path, Some(body)).await
    }

    /// `method path` on `public`, with the JSON `body` or none, proved by the owner as `owner_post`
    /// proves it.
    async fn owner_call(public: &Router, state: &ProviderState, consumer: &str, method: &str, path: &str,
        body: Option<Value>) -> (StatusCode, Value) {
        let key = consumer.repeat(32);
        let owner = wallet::address_from_private_key(&key).unwrap();
        let body_hash = protocol::hash_hex(body.as_ref().map(Value::to_string).unwrap_or_default().as_bytes());
        let (status, challenge) = call(public, "POST", "/v1/auth/challenge", &[], Some(&json!({
            "wallet": owner, "method": method, "path": path, "body_hash": body_hash}))).await;
        assert_eq!(status, StatusCode::OK, "{challenge}");
        let challenge: protocol::Challenge = serde_json::from_value(challenge).unwrap();
        let payload = crate::auth::challenge_payload(&crate::auth::AuthPayload {
            network: state.config.network.identity.clone(), provider_wallet: state.wallet.clone(),
            wallet: owner.clone(), method: method.into(), path: path.into(), body_hash,
            nonce: challenge.nonce.clone(), expires_at: challenge.expires_at,
        }).unwrap();
        let signature = wallet::sign_message(&key, &payload).unwrap();
        let proof = [("x-chiral-owner".to_owned(), owner), ("x-chiral-nonce".to_owned(), challenge.nonce),
            ("x-chiral-signature".to_owned(), signature)];
        call(public, method, path, &proof, body.as_ref()).await
    }

    /// Every row of every table of `connection`, by table name.
    fn table_rows(connection: &rusqlite::Connection) -> Value {
        let tables: Vec<String> = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").unwrap()
            .query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        tables.into_iter().map(|table| {
            let mut statement = connection.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid")).unwrap();
            let columns = statement.column_count();
            let rows: Vec<String> = statement
                .query_map([], |row| (0..columns).map(|column| row.get::<_, rusqlite::types::Value>(column))
                    .collect::<Result<Vec<_>, _>>()).unwrap()
                .map(|row| format!("{:?}", row.unwrap())).collect();
            (table, json!(rows))
        }).collect::<serde_json::Map<_, _>>().into()
    }

    /// The deployment's ledger and wallet journal, every table and row, with the operator's
    /// account, request and fee views: the durable state that a drain must leave unchanged.
    fn durable_state(runtime: &ProviderRuntime) -> Value {
        let journals = files_named(&runtime.state.config.data_dir, "transactions.sqlite3");
        assert_eq!(journals.len(), 1, "{journals:?}");
        json!({
            "ledger": table_rows(&ledger_connection(runtime)),
            "journal": table_rows(&rusqlite::Connection::open(&journals[0]).unwrap()),
            "accounts": runtime.state.ledger.list_accounts().unwrap(),
            "requests": runtime.state.ledger.list_requests().unwrap(),
            "fees": runtime.state.ledger.fee_summary().unwrap(),
        })
    }

    /// Asserts that the `durable_state` `after` equals `before`, naming only the tables and views
    /// that differ, so that no stored credential digest reaches the failure message.
    fn assert_unchanged(before: &Value, after: &Value, context: &str) {
        let mut differing = Vec::new();
        for part in ["ledger", "journal"] {
            let tables: std::collections::BTreeSet<&String> = before[part].as_object().unwrap().keys()
                .chain(after[part].as_object().unwrap().keys()).collect();
            differing.extend(tables.into_iter().filter(|table| before[part][table.as_str()] != after[part][table.as_str()])
                .map(|table| format!("{part}.{table}")));
        }
        differing.extend(["accounts", "requests", "fees"].into_iter().filter(|view| before[*view] != after[*view])
            .map(str::to_owned));
        assert!(differing.is_empty(), "{context}: changed {differing:?}");
    }

    /// The signed offers that `GET /admin/offers` lists, and the number that the DHT publishes.
    async fn published(runtime: &Arc<ProviderRuntime>) -> (Vec<Value>, Value) {
        let (status, offers) = admin(runtime, "GET", "/admin/offers").await;
        assert_eq!(status, StatusCode::OK);
        (offers["offers"].as_array().unwrap().clone(), runtime.dht.status().await["published_offers"].clone())
    }

    /// `/admin/status`'s admission, drain and new-funding readiness.
    async fn admission(runtime: &Arc<ProviderRuntime>) -> (Value, Value, Value) {
        let (status, view) = admin(runtime, "GET", "/admin/status").await;
        assert_eq!(status, StatusCode::OK);
        (view["accepting"].clone(), view["draining"].clone(), view["funding_ready"].clone())
    }

    /// inference-generation-performance's synthetic evidence for a chat model whose served
    /// template is `template`, for the pinned engine version, persisted in `directory`: the
    /// profiled model and its evidence file.
    fn profiled_chat(directory: &std::path::Path, template: &str) -> (protocol::ModelSpec, PathBuf) {
        use sha2::{Digest, Sha256};
        let (mut model, mut report) = crate::performance::evidence_tests::fixture();
        model.chat_template_sha256 = hex::encode(Sha256::digest(template));
        let version = crate::engine_runtime::ENGINE_VERSION;
        model.serving.as_mut().unwrap().engine_version = version.into();
        report["runtime_observation"]["engine_version"] = json!(version);
        let evidence = directory.join("evidence.json");
        crate::performance::evidence_tests::persist(&mut model, &mut report, &evidence);
        (model, evidence)
    }

    /// `unavailable_provider` with chain RPC `rpc`, whose only model is the profiled `model` with
    /// its `evidence`, served by the engine at `engine_url`: a development provider that publishes
    /// the model's offer through the DHT.
    async fn profiled_chat_config(directory: &std::path::Path, rpc: String, model: &protocol::ModelSpec,
        evidence: PathBuf, engine_url: &str) -> (ProviderConfig, tokio::task::JoinHandle<()>) {
        let (mut config, unavailable) = unavailable_provider(directory).await;
        config.network.rpc_url = rpc;
        let deployment = &mut config.models[0];
        deployment.model = model.clone();
        deployment.engine_url = engine_url.into();
        deployment.qualification_evidence_file = Some(evidence);
        deployment.request_timeout_secs = 30;
        (config, unavailable)
    }

    /// Arms the test-only pause point `point` of `runtime`'s next maintenance pass: the receiver
    /// resolves once that pass has reached it, after the pass has decided admission and before
    /// it publishes, and the pass continues once the sender is used.
    fn pause_next_pass(runtime: &ProviderRuntime, point: PassPoint)
        -> (tokio::sync::oneshot::Receiver<()>, tokio::sync::oneshot::Sender<()>) {
        let (paused, reached) = tokio::sync::oneshot::channel();
        let (release, resume) = tokio::sync::oneshot::channel();
        *runtime.pass_pause.lock().unwrap() = Some((point, paused, resume));
        (reached, release)
    }

    /// provider-daemon-lifecycle requirement 1 (task 3.1): the status is an unsigned operator
    /// observation. Reading `GET /admin/status`, with the other read-only administration routes
    /// and `GET /readyz`, again and again while the provider serves, after a settlement failure
    /// and while it drains, changes no admission, new-funding readiness, drain or settlement
    /// failure, no publication (the signed offers with their revisions, the DHT and the persisted
    /// revision) and no durable ledger or wallet-journal state. The provider holds a credited
    /// funding, marked as never checked, and its pending platform fee, so that a read that
    /// rechecked funding or remitted fees would change the ledger.
    #[tokio::test]
    async fn reading_the_operator_status_changes_no_admission_publication_or_ledger_state() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (runtime, bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            Arc::new(AtomicBool::new(true)), settled_chain(receipts.clone()), false).await;
        assert_eq!(embed(&runtime, &bearer, "before-the-reads").await.0, StatusCode::OK);
        let (_, _, opening) = funded_account(&runtime.state, &receipts, "44", 1000);
        assert_eq!(runtime.refresh().await, Ok(1));
        unchecked(&runtime, &opening);
        assert_eq!(runtime.state.ledger.fee_summary().unwrap()["amounts_wei"]["pending"], "5");
        let observed = || {
            let runtime = runtime.clone();
            async move {
                let (offers, dht) = published(&runtime).await;
                (json!({
                    "accepting": runtime.state.accepting.load(Ordering::Acquire),
                    "funding_ready": runtime.state.funding_ready.load(Ordering::Acquire),
                    "drain_state": runtime.drain_state.load(Ordering::Acquire),
                    "settlement_error": runtime.state.settlement_fault(),
                    "refresh_error": runtime.refresh_error().await,
                    "offers": offers, "dht": dht,
                    "revision": std::fs::read_to_string(&runtime.revision_file).unwrap(),
                }), durable_state(&runtime))
            }
        };
        for case in ["serving", "after a settlement failure", "drained"] {
            match case {
                "after a settlement failure" =>
                    runtime.state.record_settlement_fault(inference_gateway::DURABLE_SETTLEMENT_FAILED),
                "drained" => assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK),
                _ => {}
            }
            let (facts, durable) = observed().await;
            let expected = match case {
                "serving" => (true, true, OPEN, Value::Null),
                "after a settlement failure" => (false, false, OPEN, json!(inference_gateway::DURABLE_SETTLEMENT_FAILED)),
                _ => (false, false, DRAINING, json!(inference_gateway::DURABLE_SETTLEMENT_FAILED)),
            };
            assert_eq!((&facts["accepting"], &facts["funding_ready"], &facts["drain_state"], &facts["settlement_error"]),
                (&json!(expected.0), &json!(expected.1), &json!(expected.2), &expected.3), "{case}");
            // A settlement failure closes admission and new funding at once; its offer stays
            // published until the next pass withdraws it, which reading the status never runs.
            assert_eq!(facts["offers"].as_array().unwrap().len(), usize::from(case != "drained"), "{case}");
            for _ in 0..3 {
                for path in ["/admin/status", "/admin/runtime", "/admin/offers", "/admin/accounts", "/admin/requests",
                    "/admin/fees"] {
                    assert_eq!(admin(&runtime, "GET", path).await.0, StatusCode::OK, "{case}: {path}");
                }
                readyz(&runtime).await;
            }
            let (after, after_durable) = observed().await;
            assert_eq!(after, facts, "{case}");
            assert_unchanged(&durable, &after_durable, case);
        }
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 3 (task 3.3), scenarios "Admitted work settles while
    /// new work is refused", "Recovery and maintenance continue while draining" and "Drain changes
    /// no money", with requirement 1's scenario "The status shows when in-flight work has settled"
    /// (task 3.1). A development provider publishes the offer of its profiled chat model, served
    /// by a stub engine. It holds a paid account funded through an acknowledged opening (large
    /// enough that its platform fee is due at once), another acknowledged opening, a paid quote
    /// without an intent and a free quote. One streamed chat request of the funded account is
    /// admitted, and the engine holds it open after its first token. Then `POST /admin/drain`:
    /// - changes no durable state (the admitted request's reservation included) and withdraws the
    ///   offer before it answers; the status reports the drain and one admitted request not yet
    ///   settled;
    /// - a new request on the same account is refused as `provider_paused` before any record or
    ///   engine contact; a quote proposal for the offer, a fresh funding intent and a free opening
    ///   are refused as `provider_draining`; the acknowledged intent is recovered unchanged;
    /// - the held stream completes and settles as `completed` with the engine's usage, and the
    ///   status then reports no unsettled request;
    /// - the owner reads the request's status, by the gateway and the control plane, and a
    ///   receipt; a cancellation finds nothing in flight;
    /// - the next maintenance pass rechecks the credited funding and attempts the due fee
    ///   remittance (the stub refuses the submission, which the status reports), and reopens
    ///   nothing;
    /// - the owner settles a top-up of the existing agreement, credited with its own fee and
    ///   recovered without a second credit, opens the acknowledged account and recovers it, and
    ///   rotates a credential, whose new bearer reads the account;
    /// - a second drain changes no durable state.
    #[tokio::test]
    async fn a_drain_refuses_new_work_while_admitted_work_settles_and_recovery_continues() {
        use std::sync::atomic::AtomicUsize;
        const TEMPLATE: &str = "{{ messages }}";
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (rpc, rpc_server) = serve(settled_chain(receipts.clone())).await;
        let (model, evidence) = profiled_chat(directory.path(), TEMPLATE);
        let held = HeldStream::new();
        let contacted = Arc::new(AtomicUsize::new(0));
        let engine = chat_engine_holding(model.clone(), TEMPLATE, Some(held.clone())).layer(middleware::from_fn({
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
        let (config, unavailable) = profiled_chat_config(directory.path(), rpc, &model, evidence, &engine_url).await;
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let state = runtime.state.clone();
        let public = runtime.public_router();
        let (offers, dht) = published(&runtime).await;
        assert_eq!((offers.len(), dht), (1, json!(1)));
        let offer_id = runtime.published.lock().await.iter().next().unwrap().clone();
        // 2 × 10^17 wei: its 50-basis-point platform fee, 10^15 wei, is the minimum remittance.
        let (contract, credential, opening) = funded_account(&state, &receipts, "44", 200_000_000_000_000_000);
        let acknowledged = stored_quote(&state, "55", 1000);
        let (acknowledged_payment, acknowledgement) = acknowledged_opening(&state, &receipts, "55", &acknowledged);
        let unacknowledged = stored_quote(&state, "66", 1000);
        let fresh = signed_payment(&state, &receipts, "66", 0, 1000, &protocol::opening_data(&unacknowledged.payload).unwrap());
        let free = stored_quote(&state, "77", 0);
        // 300 output tokens is beyond the qualified workload envelope (256), so that holding the
        // stream is not judged against the qualified speed, which would latch the model
        // (inference-generation-performance).
        let chat = |content: &str| json!({"model": "model", "messages": [{"role": "user", "content": content}],
            "max_tokens": 300, "stream": true});

        let admitted = tokio::spawn({
            let (public, headers, body) = (public.clone(), keyed(&credential.bearer, "admitted-before-the-drain"), chat("held"));
            async move { send(&public, "POST", "/v1/chat/completions", &headers, Some(&body)).await }
        });
        held.entered().await;
        assert_eq!(state.ledger.request_view(&contract, "admitted-before-the-drain").unwrap().status, "reserved");
        let before = durable_state(&runtime);
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        // The drain changed no durable state, the admitted request's reservation included, and
        // withdrew the offer before it answered.
        assert_unchanged(&before, &durable_state(&runtime), "the drain");
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["draining"], &view["funding_ready"], &view["ledger"]["active_requests"]),
            (&json!(false), &json!(true), &json!(false), &json!(1)), "{view}");
        // Each fact stays its own: the drain is not reported as a model that is not ready, a
        // latch, a settlement failure, a failed pass or a chain fault.
        assert_eq!((&view["models"][0]["ready"], &view["models"][0]["performance_withdrawn"], &view["settlement_error"],
            &view["refresh_error"], &view["chain_error"]),
            (&json!(true), &json!(false), &Value::Null, &Value::Null, &Value::Null), "{view}");
        assert!(view["models"][0]["detail"].as_str().unwrap().starts_with("Ready: "), "{view}");
        assert_eq!(admin(&runtime, "GET", "/admin/runtime").await.1["models"][0]["ready"], true);
        // A new request is refused, also on the account whose stream is in progress, before any
        // record or engine contact.
        let reached = contacted.load(Ordering::SeqCst);
        let (status, answer) = call(&public, "POST", "/v1/chat/completions", &keyed(&credential.bearer, "after-the-drain"),
            Some(&chat("hello"))).await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")), "{answer}");
        assert!(state.ledger.request_view(&contract, "after-the-drain").is_err());
        assert_eq!(contacted.load(Ordering::SeqCst), reached, "the refused request reached the engine");

        // The admitted stream completes and settles with the engine's usage.
        held.release();
        let (status, text) = within(admitted).await.unwrap();
        let text = String::from_utf8(text).unwrap();
        assert_eq!(status, StatusCode::OK, "{text}");
        assert!(text.contains("\"OK\"") && text.contains("[DONE]"), "{text}");
        let settled = within(async {
            loop {
                let view = state.ledger.request_view(&contract, "admitted-before-the-drain").unwrap();
                if view.status != "reserved" {
                    break view;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        assert_eq!(settled.status, "completed");
        assert_eq!((settled.usage.prompt_tokens, settled.usage.completion_tokens), (2, 1));
        // One ceiling-rounded charge at the agreement's rates: (2 × 1 + 1 × 2) / 1,000 wei.
        assert_eq!(settled.cost_wei, "1");
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["draining"], &view["ledger"]["active_requests"]), (&json!(true), &json!(0)), "{view}");

        // New agreements are refused: a quote proposal for the offer, a fresh funding intent and a
        // free opening. The acknowledged intent is recovered as it was acknowledged.
        let (status, answer) = call(&public, "POST", "/v1/contracts/propose", &[], Some(&json!({"offer_id": offer_id,
            "consumer_wallet": wallet::address_from_private_key(&"88".repeat(32)).unwrap(), "funding_wei": "1000"}))).await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_draining")), "{answer}");
        let path = format!("/v1/contracts/{}/funding-intent", unacknowledged.payload.id);
        let (status, answer) = owner_post(&public, &state, "66", &path, json!({"raw_transaction": fresh.raw_transaction})).await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_draining")), "{answer}");
        assert!(state.ledger.funding_intent(&unacknowledged.payload, &fresh.raw_transaction).unwrap().is_none());
        let (status, answer) = owner_post(&public, &state, "77", "/v1/contracts/open", json!({"quote_id": free.payload.id})).await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_draining")), "{answer}");
        assert!(state.ledger.account(&free.payload.id).is_err());
        let path = format!("/v1/contracts/{}/funding-intent", acknowledged.payload.id);
        assert_eq!(owner_post(&public, &state, "55", &path, json!({"raw_transaction": acknowledged_payment.raw_transaction})).await,
            (StatusCode::OK, acknowledgement));

        // The owner reads the request's status and a receipt; nothing is in flight to cancel.
        let read = bearer_header(&credential.bearer);
        for path in ["/v1/requests/admitted-before-the-drain".to_owned(),
            format!("/v1/contracts/{contract}/requests/admitted-before-the-drain")] {
            let (status, view) = call(&public, "GET", &path, &read, None).await;
            assert_eq!((status, &view["status"], &view["id"]), (StatusCode::OK, &json!("completed"), &json!(settled.id)), "{path}: {view}");
        }
        let (status, receipt) = call(&public, "GET", &format!("/v1/contracts/{contract}/receipt"), &read, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(receipt["payload"]["account"]["contract_id"], json!(contract));
        assert!(receipt["payload"]["requests"].as_array().unwrap().iter()
            .any(|request| request["id"] == json!(settled.id) && request["status"] == "completed"));
        let (status, view) = call(&public, "POST", "/v1/requests/admitted-before-the-drain/cancel", &read, None).await;
        assert_eq!((status, &view["status"]), (StatusCode::OK, &json!("completed")), "{view}");

        // The next maintenance pass rechecks the credited funding and attempts the due platform-fee
        // remittance, whose submission the stub chain refuses, and reopens nothing.
        let since = unchecked(&runtime, &opening);
        assert_eq!(runtime.refresh().await, Ok(0));
        assert!(funding_checked(&runtime, &opening) >= since, "the pass did not recheck the credited funding");
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["draining"], &view["funding_ready"], &view["models"][0]["ready"],
            &view["settlement_error"], &view["refresh_error"]),
            (&json!(false), &json!(true), &json!(false), &json!(true), &Value::Null, &Value::Null), "{view}");
        assert_eq!((&view["maintenance"]["funding_error"], &view["maintenance"]["funding_rechecks_quarantined"]),
            (&Value::Null, &json!(0)), "{view}");
        assert!(view["maintenance"]["fee_error"].as_str().is_some_and(|error| error.starts_with("Fee submission: ")), "{view}");
        assert_eq!(state.ledger.fee_summary().unwrap()["amounts_wei"]["batched"], "1000000000000000");
        assert_eq!(published(&runtime).await, (vec![], json!(0)));

        // A top-up of the existing agreement settles, credited with its own fee only, and its
        // recovery credits nothing more.
        let account = serde_json::to_value(state.ledger.account(&contract).unwrap()).unwrap();
        let fees = state.ledger.fee_summary().unwrap();
        let topup = signed_payment(&state, &receipts, "44", 1, 1000, &protocol::topup_data(&contract).unwrap());
        let path = format!("/v1/contracts/{contract}/topup");
        let (status, credited) = owner_post(&public, &state, "44", &path, json!({"raw_transaction": topup.raw_transaction})).await;
        assert_eq!(status, StatusCode::OK, "{credited}");
        let plus = |value: &Value, amount: u128| json!((value.as_str().unwrap().parse::<u128>().unwrap() + amount).to_string());
        let mut expected = account.clone();
        expected["funded_wei"] = plus(&account["funded_wei"], 1000);
        expected["fee_wei"] = plus(&account["fee_wei"], 5);
        expected["available_wei"] = plus(&account["available_wei"], 995);
        assert_eq!(credited, expected);
        let mut expected = fees.clone();
        expected["amounts_wei"]["pending"] = plus(&fees["amounts_wei"]["pending"], 5);
        assert_eq!(state.ledger.fee_summary().unwrap(), expected);
        assert_eq!(owner_post(&public, &state, "44", &path, json!({"raw_transaction": topup.raw_transaction})).await,
            (StatusCode::OK, credited));
        // The acknowledged opening completes, with a credential, and its recovery issues none.
        let body = json!({"quote_id": acknowledged.payload.id, "raw_transaction": acknowledged_payment.raw_transaction});
        let (status, opened) = owner_post(&public, &state, "55", "/v1/contracts/open", body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((&opened["account"]["status"], &opened["account"]["funded_wei"]), (&json!("active"), &json!("1000")));
        assert!(opened["credential"]["bearer"].is_string());
        let (status, recovered) = owner_post(&public, &state, "55", "/v1/contracts/open", body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(recovered["account"], opened["account"]);
        assert!(recovered["credential"].is_null());
        // The owner rotates the account's credential, and the new bearer reads the account.
        let path = format!("/v1/contracts/{contract}/credentials/{}/rotate", credential.id);
        let (status, rotated) = owner_post(&public, &state, "44", &path, json!({"expires_at": protocol::now().unwrap() + 86_400,
            "spend_cap_wei": null, "max_concurrency": 1, "scopes": ["infer", "read"]})).await;
        assert_eq!(status, StatusCode::OK, "credential rotation failed: {}", rotated["error"]);
        let rotated = rotated["bearer"].as_str().unwrap().to_owned();
        assert!(rotated != credential.bearer, "the rotation kept the bearer");
        let (status, view) = call(&public, "GET", &format!("/v1/contracts/{contract}"), &bearer_header(&rotated), None).await;
        assert_eq!((status, &view["contract_id"]), (StatusCode::OK, &json!(contract)));

        // A second drain has the effect of one: nothing durable changes.
        let before = durable_state(&runtime);
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        assert_unchanged(&before, &durable_state(&runtime), "the second drain");
        assert_eq!(admission(&runtime).await, (json!(false), json!(true), json!(false)));
        drop(public);
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 3 (task 3.3): cancellation continues while draining.
    /// An embedding request admitted before the drain waits in the engine; after `POST
    /// /admin/drain`, its owner cancels it (202 `cancelling`), it ends as cancelled with unknown
    /// usage and its status reads so, the status counts it as unsettled until then, and a new
    /// request is still refused. The drain lasts until the daemon exits: a restart serves again.
    #[tokio::test]
    async fn a_request_admitted_before_a_drain_can_be_cancelled_while_draining() {
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
        model.request_timeout_secs = 60;
        let engine = Gate::new(Some("/v1/embeddings"));
        let (engine_url, engine_server) = serve(engine.hold(embeddings_engine(model.model.clone()))).await;
        model.engine_url = engine_url;
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let (contract, bearer) = free_account(&runtime.state, "22");
        let public = runtime.public_router();
        let body = json!({"model": "model", "input": "hello"});
        engine.arm();
        let admitted = tokio::spawn({
            let (public, headers, body) = (public.clone(), keyed(&bearer, "admitted-before-the-drain"), body.clone());
            async move { call(&public, "POST", "/v1/embeddings", &headers, Some(&body)).await }
        });
        engine.entered().await;
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        assert_eq!(admin(&runtime, "GET", "/admin/status").await.1["ledger"]["active_requests"], 1);
        let (status, answer) = call(&public, "POST", "/v1/requests/admitted-before-the-drain/cancel",
            &bearer_header(&bearer), None).await;
        assert_eq!((status, &answer["status"]), (StatusCode::ACCEPTED, &json!("cancelling")), "{answer}");
        let (status, answer) = within(admitted).await.unwrap();
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::CONFLICT, &json!("request_cancelled")), "{answer}");
        let view = runtime.state.ledger.request_view(&contract, "admitted-before-the-drain").unwrap();
        assert_eq!(view.status, "cancelled_unknown_usage");
        let (status, read) = call(&public, "GET", "/v1/requests/admitted-before-the-drain", &bearer_header(&bearer), None).await;
        assert_eq!((status, &read["status"]), (StatusCode::OK, &json!("cancelled_unknown_usage")), "{read}");
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["draining"], &view["ledger"]["active_requests"]), (&json!(true), &json!(0)), "{view}");
        let (status, answer) = call(&public, "POST", "/v1/embeddings", &keyed(&bearer, "after-the-drain"), Some(&body)).await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")), "{answer}");
        assert!(runtime.state.ledger.request_view(&contract, "after-the-drain").is_err());
        engine.release();
        // A drain is a state of the running daemon (design decision 4): once the daemon exits,
        // the next start serves the account again.
        drop(public);
        let config = (*runtime.state.config).clone();
        runtime.dht.stop().await;
        drop(runtime);
        let runtime = ProviderRuntime::start(config).await.unwrap();
        assert_eq!(admission(&runtime).await, (json!(true), json!(false), json!(true)));
        let (status, answer) = embed(&runtime, &bearer, "after-the-restart").await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(runtime.state.ledger.request_view(&contract, "after-the-restart").unwrap().status, "completed");
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 2 (task 3.2), scenario "A live engine whose model is
    /// not ready". A managed engine (the helper child) whose process is alive while its model fails
    /// the template probe (the stub serves a template of another digest) is reported alive, with
    /// the launcher's observation, and not ready. The row has no other member, and the observation
    /// carries no signature, serving profile or qualification evidence and is no permission to
    /// publish: nothing is published. Once the probe passes, the model is ready with the same
    /// process; once that process exits, it is reported not alive and not ready.
    #[tokio::test]
    async fn admin_runtime_reports_a_live_engine_failing_its_template_probe_as_alive_and_not_ready() {
        use crate::inference_backend::tests::{ProbeEngine, PROBE_TEMPLATE};
        let directory = tempfile::tempdir().unwrap();
        let (rpc, rpc_server) = serve(Router::new().fallback(ready_chain)).await;
        let (model, evidence) = profiled_chat(directory.path(), PROBE_TEMPLATE);
        let engine = ProbeEngine::start(&model, 1).await;
        engine.set_fault("template");
        let (config, unavailable) = profiled_chat_config(directory.path(), rpc, &model, evidence, &engine.base).await;
        let helper = crate::engine_runtime::ManagedEngine::helper_for_test(&model.id, &engine.base);
        let pid = helper.observation.pid;
        let _cleanup = KillOnPanic(pid);
        let runtime = ProviderRuntime::start_with(config,
            std::collections::HashMap::from([(model.id.clone(), Arc::new(helper))])).await.unwrap();
        let observation = serde_json::to_value(&runtime.state.managed_engines["model"].observation).unwrap();
        let row = |alive: bool, ready: bool| json!({"models": [{"model_id": "model", "managed": true, "alive": alive,
            "ready": ready, "observation": observation}]});
        assert_eq!(admin(&runtime, "GET", "/admin/runtime").await, (StatusCode::OK, row(true, false)));
        assert_eq!(observation["pid"], pid);
        for member in ["signature", "serving", "qualification", "evidence_sha256", "profile_sha256"] {
            assert!(observation.get(member).is_none(), "{member}");
        }
        let status = admin(&runtime, "GET", "/admin/status").await.1["models"][0].clone();
        assert_eq!(status["ready"], false, "{status}");
        assert!(status["detail"].as_str().unwrap().starts_with("Not ready"), "{status}");
        assert!(!runtime.state.funding_ready.load(Ordering::Acquire));
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        // The probes alone decide: the same live process, once the template matches.
        engine.set_fault("");
        assert_eq!(runtime.refresh().await, Ok(1));
        assert_eq!(admin(&runtime, "GET", "/admin/runtime").await, (StatusCode::OK, row(true, true)));
        runtime.state.managed_engines["model"].kill_for_test();
        _cleanup.disarm();
        let exited = within(async {
            loop {
                let answer = admin(&runtime, "GET", "/admin/runtime").await.1;
                if answer["models"][0]["alive"] == false {
                    break answer;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        assert_eq!(exited, row(false, false));
        runtime.dht.stop().await;
        runtime.stop_engines().await;
        for server in [rpc_server, unavailable] {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirements 1 and 2 (tasks 3.1 and 3.2), scenarios "An engine
    /// exit is reported with its own reason" and "An exited engine is reported as not alive",
    /// while a maintenance pass holds the lifecycle lock (the test holds it here), so that the
    /// engine watcher, which has observed the exit and closed admission, cannot yet replace the
    /// model's entry of the latest pass, which still reads ready. `/admin/status` reports the
    /// model not ready with the engine exit as its reason, and `/admin/runtime` reports the
    /// engine not alive and the model not ready: neither repeats the passed probes of the pass
    /// before the exit. Once the lock is free, the watcher's entry agrees.
    #[tokio::test]
    async fn an_engine_exit_is_reported_while_a_pass_holds_the_lifecycle_lock() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, _bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            Arc::new(AtomicBool::new(true)), Router::new().fallback(ready_chain), true).await;
        let facts = || {
            let runtime = runtime.clone();
            async move {
                let status = admin(&runtime, "GET", "/admin/status").await.1["models"][0].clone();
                let row = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
                (status["ready"].clone(), status["detail"].clone(), row["alive"].clone(), row["ready"].clone())
            }
        };
        let (ready, detail, alive, row_ready) = facts().await;
        assert_eq!((&ready, &alive, &row_ready), (&json!(true), &json!(true), &json!(true)));
        assert!(detail.as_str().unwrap().starts_with("Ready: "), "{detail}");
        let lifecycle = runtime.lifecycle.lock().await;
        runtime.state.managed_engines["model"].kill_for_test();
        // The engine watcher has observed the exit and closed admission; it waits for the lock.
        within(async {
            while runtime.state.qualified_models.read().unwrap().contains("model") {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await;
        assert!(!runtime.state.accepting.load(Ordering::Acquire));
        assert_eq!(runtime.health.read().await[0].ready, true, "the latest pass's entry was replaced under the lock");
        assert_eq!(facts().await, (json!(false), json!(inference_backend::ENGINE_EXITED), json!(false), json!(false)),
            "while a pass holds the lifecycle lock");
        drop(lifecycle);
        within(async {
            while runtime.health.read().await[0].ready {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        assert_eq!(facts().await, (json!(false), json!(inference_backend::ENGINE_EXITED), json!(false), json!(false)),
            "once the watcher holds the lock");
        runtime.dht.stop().await;
        runtime.stop_engines().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 4 (task 3.4), scenarios "Resume reopens a healthy
    /// provider" and "Offer refresh does not end a drain", and a failed pass. A healthy provider
    /// with a published offer and a credited funding is drained.
    /// - `POST /admin/offers/refresh` runs a pass that rechecks the funding; the provider is still
    ///   draining, admits nothing, is not new-funding ready and publishes nothing, and the answer
    ///   says so.
    /// - `POST /admin/resume` ends the drain, and its pass reopens admission and new funding and
    ///   publishes the offer under a revision higher than any earlier one; the answer says so.
    /// - Neither changes the funded account or its credentials.
    /// - When the resume's pass fails (the offer revision cannot advance), the answer reports a
    ///   failure, and the status shows the result: the drain has ended, the failed pass is named,
    ///   admission is open for existing accounts (design observation 2), and there is no new
    ///   funding and no offer. An offer refresh then fails the same way.
    #[tokio::test]
    async fn resume_reopens_only_through_its_pass_and_offer_refresh_does_not_end_a_drain() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (runtime, bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            Arc::new(AtomicBool::new(true)), settled_chain(receipts.clone()), false).await;
        let contract = runtime.state.ledger.list_accounts().unwrap()[0].contract_id.clone();
        let (funded, _, opening) = funded_account(&runtime.state, &receipts, "44", 1000);
        // The credited funding rechecks as accepted, and the offer stays published.
        assert_eq!(runtime.refresh().await, Ok(1));
        let revision = |offers: &[Value]| offers[0]["payload"]["revision"].as_u64().unwrap();
        let first = revision(&published(&runtime).await.0);
        let agreement = || json!([runtime.state.ledger.account(&funded).unwrap(), runtime.state.ledger.credentials(&funded).unwrap()]);
        let before = agreement();

        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        let since = unchecked(&runtime, &opening);
        assert_eq!(admin(&runtime, "POST", "/admin/offers/refresh").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        assert!(funding_checked(&runtime, &opening) >= since, "the offer refresh did not recheck the credited funding");
        assert_eq!(admission(&runtime).await, (json!(false), json!(true), json!(false)));
        assert_eq!(admin(&runtime, "GET", "/admin/status").await.1["refresh_error"], Value::Null);
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        let (status, answer) = embed(&runtime, &bearer, "during-the-drain").await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")));

        assert_eq!(admin(&runtime, "POST", "/admin/resume").await,
            (StatusCode::OK, json!({"accepting": true, "offers_published": 1})));
        assert_eq!(admission(&runtime).await, (json!(true), json!(false), json!(true)));
        let (offers, dht) = published(&runtime).await;
        assert_eq!((offers.len(), dht), (1, json!(1)));
        let resumed = revision(&offers);
        assert!(resumed > first, "revision {resumed} after {first}");
        assert_eq!(std::fs::read_to_string(&runtime.revision_file).unwrap().trim(), resumed.to_string());
        assert_eq!(embed(&runtime, &bearer, "after-the-resume").await.0, StatusCode::OK);
        assert_eq!(runtime.state.ledger.request_view(&contract, "after-the-resume").unwrap().status, "completed");
        assert!(agreement() == before, "resume or offer refresh changed the funded account or its credentials");

        // A resume whose pass fails at publication.
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        *runtime.revision.lock().await = u64::MAX;
        for action in ["/admin/resume", "/admin/offers/refresh"] {
            let (status, answer) = admin(&runtime, "POST", action).await;
            assert_eq!((status, &answer["error"]["code"]), (StatusCode::INTERNAL_SERVER_ERROR, &json!("internal_error")), "{action}");
            let view = admin(&runtime, "GET", "/admin/status").await.1;
            assert_eq!((&view["accepting"], &view["draining"], &view["funding_ready"], &view["refresh_error"]),
                (&json!(true), &json!(false), &json!(false), &json!("Persisting offer revision failed")), "{action}: {view}");
            assert_eq!(published(&runtime).await, (vec![], json!(0)), "{action}");
        }
        assert!(agreement() == before, "a failed pass changed the funded account or its credentials");
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 4 (task 3.4), scenario "Resume does not pass a failed
    /// gate", shared with managed-engine-runtime task 3.10. A drained provider whose only model
    /// runs on a managed engine (the recording helper child) loses that engine. `POST
    /// /admin/resume` ends the drain, and its pass keeps admission and new funding closed and
    /// publishes nothing; a request is refused before any record; no engine process starts and no
    /// launch is attempted, and the provider keeps the exited engine's handle.
    #[tokio::test]
    async fn resume_of_a_drained_provider_whose_engine_exited_ends_the_drain_and_reopens_nothing() {
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
        let (engine_url, engine_server) = serve(embeddings_engine(model.model.clone())).await;
        model.engine_url = engine_url.clone();
        let spawn_log = directory.path().join("engine-spawns");
        let helper = crate::engine_runtime::ManagedEngine::recording_helper_for_test("model", &engine_url, &spawn_log);
        let pid = helper.observation.pid;
        let _cleanup = KillOnPanic(pid);
        let runtime = ProviderRuntime::start_with(config,
            std::collections::HashMap::from([("model".to_owned(), Arc::new(helper))])).await.unwrap();
        within(async { while engine_starts(&spawn_log).is_empty() { tokio::time::sleep(Duration::from_millis(20)).await; } }).await;
        let (contract, bearer) = free_account(&runtime.state, "22");
        assert_eq!(published(&runtime).await.1, json!(1));

        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        runtime.state.managed_engines["model"].kill_for_test();
        _cleanup.disarm();
        watcher_recorded_exit(&runtime).await;
        let attempts = crate::engine_runtime::ManagedEngine::launch_attempts_for_test(&engine_url);
        assert_eq!(admin(&runtime, "POST", "/admin/resume").await,
            (StatusCode::OK, json!({"accepting": false, "offers_published": 0})));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["draining"], &view["funding_ready"], &view["settlement_error"]),
            (&json!(false), &json!(false), &json!(false), &Value::Null), "{view}");
        assert_eq!((&view["models"][0]["ready"], &view["models"][0]["detail"]),
            (&json!(false), &json!(inference_backend::ENGINE_EXITED)), "{view}");
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        let (status, answer) = embed(&runtime, &bearer, "after-the-resume").await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")), "{answer}");
        assert!(runtime.state.ledger.request_view(&contract, "after-the-resume").is_err());
        let engine = &runtime.state.managed_engines["model"];
        assert_eq!((engine.observation.pid, engine.is_alive()), (pid, false));
        assert_eq!(engine_starts(&spawn_log), [pid.to_string()], "an engine process started again");
        assert_eq!(crate::engine_runtime::ManagedEngine::launch_attempts_for_test(&engine_url), attempts,
            "an engine launch was attempted");
        let row = admin(&runtime, "GET", "/admin/runtime").await.1["models"][0].clone();
        assert_eq!((&row["alive"], &row["ready"]), (&json!(false), &json!(false)), "{row}");
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 4 (task 3.4), shared with
    /// inference-generation-performance: a latched report stays latched through `POST
    /// /admin/drain`, `POST /admin/resume` and `POST /admin/offers/refresh`. A development
    /// provider publishes its profiled chat model, which has a credited funding. Once the model's
    /// evidence digest is latched, the resume's and the refresh's passes still run and recheck the
    /// funding (requirement 5: passes continue while a model is latched) and publish nothing; the
    /// latch is unchanged, the status names the withdrawal, and a request of the funded account is
    /// refused before reservation. Requirement 1: when the latch cannot be read, the status reports
    /// it as unavailable and still reports the other facts, and the model is not admissible.
    #[tokio::test]
    async fn a_latched_report_stays_latched_through_resume_and_offer_refresh() {
        const TEMPLATE: &str = "{{ messages }}";
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (rpc, rpc_server) = serve(settled_chain(receipts.clone())).await;
        let (model, evidence) = profiled_chat(directory.path(), TEMPLATE);
        let (engine_url, engine_server) = serve(chat_engine(model.clone(), TEMPLATE)).await;
        let (config, unavailable) = profiled_chat_config(directory.path(), rpc, &model, evidence, &engine_url).await;
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let (contract, credential, opening) = funded_account(&runtime.state, &receipts, "44", 1000);
        assert_eq!(runtime.refresh().await, Ok(1));
        let evidence_sha256 = model.serving.as_ref().unwrap().qualification.evidence_sha256.clone();
        let latches = || ledger_connection(&runtime).query_row(
            "SELECT json_group_array(json_array(model,evidence)) FROM performance_withdrawals", [],
            |row| row.get::<_, String>(0)).unwrap();
        ledger_connection(&runtime).execute("INSERT INTO performance_withdrawals(model,evidence) VALUES(?1,?2)",
            rusqlite::params![model.id, evidence_sha256]).unwrap();
        let latched = latches();
        assert_eq!(latched, json!([[model.id, evidence_sha256]]).to_string());

        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        for action in ["/admin/resume", "/admin/offers/refresh"] {
            let since = unchecked(&runtime, &opening);
            assert_eq!(admin(&runtime, "POST", action).await,
                (StatusCode::OK, json!({"accepting": true, "offers_published": 0})), "{action}");
            assert!(funding_checked(&runtime, &opening) >= since, "{action}: the pass did not recheck the funding");
            assert_eq!(latches(), latched, "{action}");
            let view = admin(&runtime, "GET", "/admin/status").await.1;
            assert_eq!((&view["draining"], &view["models"][0]["performance_withdrawn"]), (&json!(false), &json!(true)),
                "{action}: {view}");
            assert!(view["models"][0]["detail"].as_str().unwrap().starts_with(PERFORMANCE_WITHDRAWN), "{action}: {view}");
            assert!(runtime.state.qualified_model(&model).is_none(), "{action}");
            assert_eq!(published(&runtime).await, (vec![], json!(0)), "{action}");
            let key = format!("after-{}", action.replace('/', "-"));
            let (status, answer) = chat_request(&runtime, &credential.bearer, &key).await;
            assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("model_unqualified")),
                "{action}: {answer}");
            assert!(runtime.state.ledger.request_view(&contract, &key).is_err(), "{action}");
        }

        // An unreadable latch is reported as unavailable, beside every other fact, and fails closed.
        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("ALTER TABLE performance_withdrawals RENAME TO performance_withdrawals_unreadable").unwrap();
        let (status, view) = admin(&runtime, "GET", "/admin/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((&view["models"][0]["performance_withdrawn"], &view["models"][0]["ready"]), (&Value::Null, &json!(true)),
            "{view}");
        assert_eq!((&view["accepting"], &view["ledger"]["accounts"], &view["ledger_error"]), (&json!(true), &json!(1), &Value::Null),
            "{view}");
        assert!(runtime.state.qualified_model(&model).is_none());
        ledger.execute_batch("ALTER TABLE performance_withdrawals_unreadable RENAME TO performance_withdrawals").unwrap();
        assert_eq!(admin(&runtime, "GET", "/admin/status").await.1["models"][0]["performance_withdrawn"], true);
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 5 (task 3.5), scenarios "A drain during a pass leaves
    /// nothing published" and "An engine exit during a pass leaves nothing published", with
    /// requirement 3's "at once". A pass is held at a test-only pause point after it has decided
    /// admission open, before it publishes: once before it stores that decision (`Decided`), and
    /// once after (`Stored`).
    /// - The operator drains: admission and new funding close at once, and a new request is
    ///   refused, while the drain waits for the pass. The pass then stores its decision (at
    ///   `Decided`) and publishes. After both have returned, admission and new funding are closed,
    ///   the drain answered so, and neither `/admin/offers` nor the DHT holds an offer: the drain
    ///   closes again and withdraws once it holds the lifecycle lock.
    /// - The model's managed engine exits instead (the helper child is killed): once the engine
    ///   watcher has observed it, admission and new funding are closed at once and a request is
    ///   refused, while the watcher waits for the pass. The pass then completes, and admission and
    ///   new funding are closed, nothing is published, the status names the exit, and a request is
    ///   refused before any record.
    #[tokio::test]
    async fn a_drain_or_an_engine_exit_during_a_pass_leaves_nothing_admitted_or_published() {
        for point in [PassPoint::Decided, PassPoint::Stored] {
            for case in ["drain", "engine exit"] {
                let context = format!("{case} at {point:?}");
                let directory = tempfile::tempdir().unwrap();
                let (runtime, bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
                    Arc::new(AtomicBool::new(true)), Router::new().fallback(ready_chain), case == "engine exit").await;
                let contract = runtime.state.ledger.list_accounts().unwrap()[0].contract_id.clone();
                assert_eq!(published(&runtime).await.1, json!(1), "{context}");
                let (paused, release) = pause_next_pass(&runtime, point);
                let pass = tokio::spawn({
                    let runtime = runtime.clone();
                    async move { runtime.refresh().await }
                });
                within(paused).await.unwrap();
                // The pass has decided admission open and has not yet published. At `Decided`, the
                // previous pass's admission still stands, and new funding closed when the pass
                // began; at `Stored`, the pass has opened both.
                assert!(runtime.state.accepting.load(Ordering::Acquire), "{context}");
                assert_eq!(runtime.state.funding_ready.load(Ordering::Acquire), point == PassPoint::Stored, "{context}");
                let drain = if case == "drain" {
                    let drain = tokio::spawn({
                        let runtime = runtime.clone();
                        async move { admin(&runtime, "POST", "/admin/drain").await }
                    });
                    drain_state_reaches(&runtime, |state| state == DRAINING).await;
                    // The drain waits for the paused pass, which holds the lifecycle lock.
                    assert!(!drain.is_finished(), "{context}: the drain did not wait for the pass");
                    Some(drain)
                } else {
                    runtime.state.managed_engines["model"].kill_for_test();
                    // The engine watcher has observed the exit; it then waits for the paused pass.
                    within(async {
                        while runtime.state.qualified_models.read().unwrap().contains("model") {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }).await;
                    None
                };
                // At once, before the drain or the watcher holds the lifecycle lock: on this
                // current-thread runtime, each closes admission in the same poll in which it
                // records the drain or removes the model, before it waits for the lock.
                assert!(!runtime.state.accepting.load(Ordering::Acquire), "{context}: admission still open while the pass runs");
                assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{context}: new funding still open while the pass runs");
                let (status, answer) = embed(&runtime, &bearer, "while-the-pass-runs").await;
                assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")),
                    "{context}: {answer}");
                assert!(runtime.state.ledger.request_view(&contract, "while-the-pass-runs").is_err(), "{context}");
                release.send(()).unwrap();
                let count = within(pass).await.unwrap().unwrap();
                assert_eq!(count, usize::from(case == "drain"), "{context}: the pass's own publication");
                if let Some(drain) = drain {
                    assert_eq!(within(drain).await.unwrap(), (StatusCode::OK, json!({"accepting": false, "offers_published": 0})),
                        "{context}");
                } else {
                    // The watcher has closed admission again under the lifecycle lock.
                    watcher_recorded_exit(&runtime).await;
                }
                assert!(!runtime.state.accepting.load(Ordering::Acquire), "{context}");
                assert!(!runtime.state.funding_ready.load(Ordering::Acquire), "{context}");
                assert_eq!(admission(&runtime).await, (json!(false), json!(case == "drain"), json!(false)), "{context}");
                assert_eq!(published(&runtime).await, (vec![], json!(0)), "{context}");
                let (status, answer) = embed(&runtime, &bearer, "after-the-pass").await;
                assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")),
                    "{context}: {answer}");
                assert!(runtime.state.ledger.request_view(&contract, "after-the-pass").is_err(), "{context}");
                runtime.dht.stop().await;
                runtime.stop_engines().await;
                for server in servers {
                    server.abort();
                }
            }
        }
    }

    /// provider-daemon-lifecycle requirement 5 (task 3.5), scenario "Passes continue while drained
    /// and after an engine failure", and a chain outage. A provider whose only model runs on a
    /// managed engine has a credited funding, which a pass rechecks as accepted. The operator
    /// drains it; then its engine exits; then its chain RPC fails. After each step, the next pass
    /// (the periodic pass's body) still rechecks the credited funding, and the provider admits
    /// nothing and publishes nothing; the status reports the outage's funding recheck result.
    #[tokio::test]
    async fn passes_keep_rechecking_credited_funding_while_drained_after_an_engine_exit_and_in_an_outage() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let outage = Arc::new(AtomicBool::new(false));
        let chain = settled_chain(receipts.clone()).layer(middleware::from_fn({
            let outage = outage.clone();
            move |request: Request, next: Next| {
                let outage = outage.clone();
                async move {
                    if outage.load(Ordering::Acquire) {
                        StatusCode::SERVICE_UNAVAILABLE.into_response()
                    } else {
                        next.run(request).await
                    }
                }
            }
        }));
        let (runtime, _bearer, servers) =
            embedding_provider_on(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true)), chain, true).await;
        let (_, _, opening) = funded_account(&runtime.state, &receipts, "44", 1000);
        assert_eq!(runtime.refresh().await, Ok(1));
        for step in ["drained", "drained after an engine exit", "drained after an engine exit, in a chain outage"] {
            match step {
                "drained" => assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK),
                "drained after an engine exit" => {
                    runtime.state.managed_engines["model"].kill_for_test();
                    watcher_recorded_exit(&runtime).await;
                }
                _ => outage.store(true, Ordering::Release),
            }
            let since = unchecked(&runtime, &opening);
            assert_eq!(runtime.refresh().await, Ok(0), "{step}");
            assert!(funding_checked(&runtime, &opening) >= since, "{step}: the pass did not recheck the credited funding");
            let view = admin(&runtime, "GET", "/admin/status").await.1;
            assert_eq!((&view["accepting"], &view["draining"], &view["funding_ready"]),
                (&json!(false), &json!(true), &json!(false)), "{step}: {view}");
            let funding_error = if step.ends_with("outage") {
                json!("Funding recheck: configured RPC unavailable or canonical status untrusted")
            } else {
                Value::Null
            };
            assert_eq!(view["maintenance"]["funding_error"], funding_error, "{step}: {view}");
            assert_eq!(published(&runtime).await, (vec![], json!(0)), "{step}");
        }
        runtime.dht.stop().await;
        runtime.stop_engines().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 5 (task 3.5), the order of a pass: a pass publishes
    /// only after its own reconciliation. A published provider has a credited funding whose
    /// recheck the ledger refuses to apply (a trigger refuses the funding evidence update), so the
    /// pass quarantines it. That same pass publishes nothing and is not new-funding ready, while
    /// existing credit keeps serving; the status reports the quarantine as the funding result, and
    /// the pass itself did not fail. Once the ledger accepts the observation, a pass releases the
    /// quarantine and publishes again.
    #[tokio::test]
    async fn a_funding_recheck_the_ledger_quarantines_keeps_the_same_pass_from_publishing() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (runtime, _bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            Arc::new(AtomicBool::new(true)), settled_chain(receipts.clone()), false).await;
        let (contract, credential, _) = funded_account(&runtime.state, &receipts, "44", 1000);
        assert_eq!(runtime.refresh().await, Ok(1));
        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("CREATE TRIGGER refuse_funding_evidence BEFORE UPDATE OF evidence ON funding
            BEGIN SELECT RAISE(ABORT,'injected funding evidence refusal'); END;").unwrap();
        assert_eq!(runtime.refresh().await, Ok(0));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["refresh_error"]),
            (&json!(true), &json!(false), &Value::Null), "{view}");
        assert_eq!(view["maintenance"]["funding_rechecks_quarantined"], 1, "{view}");
        assert_eq!(view["maintenance"]["funding_error"],
            "Funding recheck: ledger refused a chain observation; payment held for operator review", "{view}");
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        // Existing credit keeps serving.
        let (status, answer) = embed(&runtime, &credential.bearer, "during-the-quarantine").await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(runtime.state.ledger.request_view(&contract, "during-the-quarantine").unwrap().status, "completed");
        ledger.execute_batch("DROP TRIGGER refuse_funding_evidence").unwrap();
        assert_eq!(runtime.refresh().await, Ok(1));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["funding_ready"], &view["maintenance"]["funding_rechecks_quarantined"],
            &view["maintenance"]["funding_error"]), (&json!(true), &json!(0), &Value::Null), "{view}");
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 6 (task 3.6), scenario "Resume or restart reopens
    /// admission", restart case, shared with inference-request-accounting task 3.1. A request's
    /// settlement cannot be written, so admission closes and the request stays `reserved`. The
    /// ledger accepts writes again, and the operator restarts the daemon instead of resuming. When
    /// the start returns, before any new request, startup recovery has settled the request as
    /// `restart_unknown_usage` with no usage and no charge; admission and new funding are open
    /// through the starting pass, with no settlement failure reported and the offer published.
    /// Re-sending the request returns its record and never generates it again, and a new request
    /// on the same account, whose only concurrency slot the unsettled request held, completes.
    #[tokio::test]
    async fn a_restart_after_a_settlement_failure_recovers_the_request_before_admitting_again() {
        use std::sync::atomic::AtomicUsize;
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
        let generated = Arc::new(AtomicUsize::new(0));
        let engine = embeddings_engine(model.model.clone()).layer(middleware::from_fn({
            let generated = generated.clone();
            move |request: Request, next: Next| {
                let generated = generated.clone();
                async move {
                    if request.uri().path() == "/v1/embeddings" {
                        generated.fetch_add(1, Ordering::SeqCst);
                    }
                    next.run(request).await
                }
            }
        }));
        let (engine_url, engine_server) = serve(engine).await;
        model.engine_url = engine_url;
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        let (contract, bearer) = free_account(&runtime.state, "22");
        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("CREATE TRIGGER refuse_settlement BEFORE UPDATE ON requests
            WHEN OLD.status='reserved' AND NEW.status<>'reserved'
            BEGIN SELECT RAISE(ABORT,'injected settlement write failure'); END;").unwrap();
        let (status, answer) = embed(&runtime, &bearer, "unsettled").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
        assert_eq!(runtime.state.settlement_fault(), Some(inference_gateway::DURABLE_SETTLEMENT_FAILED));
        assert_eq!(runtime.state.ledger.request_view(&contract, "unsettled").unwrap().status, "reserved");
        // The answer arrives after the settlement's drop retried its write once.
        ledger.execute_batch("DROP TRIGGER refuse_settlement").unwrap();
        drop(ledger);
        runtime.dht.stop().await;
        drop(runtime);

        let runtime = ProviderRuntime::start(config).await.unwrap();
        // Recovered by the start, before any new request.
        let recovered = runtime.state.ledger.request_view(&contract, "unsettled").unwrap();
        assert_eq!(recovered.status, "restart_unknown_usage");
        assert_eq!((recovered.usage.prompt_tokens, recovered.usage.completion_tokens, recovered.cost_wei.as_str()),
            (0, 0, "0"));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["settlement_error"], &view["ledger"]["active_requests"]),
            (&json!(true), &json!(true), &Value::Null, &json!(0)), "{view}");
        assert_eq!(published(&runtime).await.1, json!(1));
        let (status, requests) = admin(&runtime, "GET", "/admin/requests").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(requests["requests"].as_array().unwrap().iter().map(|request| request["status"].clone()).collect::<Vec<_>>(),
            [json!("restart_unknown_usage")]);
        // The recovered request is never generated again.
        let before = generated.load(Ordering::SeqCst);
        let (status, answer) = embed(&runtime, &bearer, "unsettled").await;
        assert_eq!((status, &answer["error"]["code"], &answer["error"]["request_status"]),
            (StatusCode::CONFLICT, &json!("duplicate_request"), &json!("restart_unknown_usage")), "{answer}");
        assert_eq!(generated.load(Ordering::SeqCst), before, "the recovered request was generated again");
        let (status, answer) = embed(&runtime, &bearer, "after-the-restart").await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(generated.load(Ordering::SeqCst), before + 1);
        assert_eq!(runtime.state.ledger.request_view(&contract, "after-the-restart").unwrap().status, "completed");
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// inference-request-accounting task 3.1 (d), restart case for a paid account, with
    /// provider-daemon-lifecycle requirement 6: "the gateway SHALL stop new admission and
    /// retain the original request for recovery", and *Restart recovers orphaned reservations
    /// before new admission*. A development chat provider holds a funded account (1,000,000 wei,
    /// 995,000 available after the fee); its chat request reserves 10 wei and costs 4. An
    /// `AFTER UPDATE` trigger with `RAISE(FAIL)` refuses the request's terminal write, so the
    /// settlement transaction must roll back the account and credential writes before it. The
    /// request answers 500 `internal_error`, stays `reserved` with its whole 10-wei hold in the
    /// account and credential, and admission closes. While closed, a new request is refused 503
    /// `provider_paused` with nothing recorded, the request reads as `reserved` from the gateway
    /// and the control plane, and its key is the 409 naming it. The ledger then accepts writes and
    /// the daemon restarts. When the start returns, before any request, startup recovery has
    /// released the hold as `restart_unknown_usage`: no charge or usage, 10 wei of request
    /// exposure, the account back at 995,000 available with nothing reserved or spent and its own
    /// funding-loss compensation untouched, the credential's hold gone. Admission is open with no
    /// failure reported, the key still answers 409 with the recovered status, and a new request is
    /// reserved and charged at the agreement's rates.
    #[tokio::test]
    async fn a_paid_settlement_failure_keeps_its_hold_until_a_restart_releases_it_before_admission() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (config, servers) = development_chat_provider(directory.path(), settled_chain(receipts.clone())).await;
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        let (contract, credential, _) = funded_account(&runtime.state, &receipts, "44", 1_000_000);
        let balances = |runtime: &ProviderRuntime| {
            let account = runtime.state.ledger.account(&contract).unwrap();
            let held = runtime.state.ledger.credentials(&contract).unwrap().into_iter()
                .find(|entry| entry.id == credential.id).unwrap();
            (account.available_wei, account.reserved_wei, account.spent_wei, account.provider_loss_wei,
                (account.usage.prompt_tokens, account.usage.completion_tokens), held.reserved_wei, held.spent_wei)
        };
        let opened = || ("995000".to_owned(), "0".to_owned(), "0".to_owned(), "0".to_owned(), (0, 0), "0".to_owned(), "0".to_owned());
        assert_eq!(balances(&runtime), opened());
        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("CREATE TRIGGER refuse_paid_settlement AFTER UPDATE ON requests
            WHEN OLD.request_key='unsettled' AND NEW.status<>'reserved'
            BEGIN SELECT RAISE(FAIL,'injected settlement write failure'); END;").unwrap();
        let (status, answer) = chat_request(&runtime, &credential.bearer, "unsettled").await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::INTERNAL_SERVER_ERROR, &json!("internal_error")), "{answer}");
        assert_eq!(runtime.state.settlement_fault(), Some(inference_gateway::DURABLE_SETTLEMENT_FAILED));
        assert!(!runtime.state.accepting.load(Ordering::Acquire));
        let held = runtime.state.ledger.request_view(&contract, "unsettled").unwrap();
        assert_eq!((held.status.as_str(), held.max_cost_wei.as_str(), held.cost_wei.as_str(), held.provider_loss_wei.as_str(),
            held.prompt_tokens_reserved, held.max_output_tokens), ("reserved", "10", "0", "0", 2, 4));
        let holding = ("994990".to_owned(), "10".to_owned(), "0".to_owned(), "0".to_owned(), (0, 0), "10".to_owned(), "0".to_owned());
        assert_eq!(balances(&runtime), holding, "a failed settlement left a partial or acknowledged debit");

        // While admission is closed: nothing new is admitted, the request stays readable.
        let (status, answer) = chat_request(&runtime, &credential.bearer, "while-closed").await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")), "{answer}");
        assert!(runtime.state.ledger.request_view(&contract, "while-closed").is_err());
        let record = serde_json::to_value(&held).unwrap();
        for path in ["/v1/requests/unsettled".to_owned(), format!("/v1/contracts/{contract}/requests/unsettled")] {
            assert_eq!(call(&runtime.public_router(), "GET", &path, &bearer_header(&credential.bearer), None).await,
                (StatusCode::OK, record.clone()), "{path}");
        }
        let (status, answer) = chat_request(&runtime, &credential.bearer, "unsettled").await;
        assert_eq!((status, &answer["error"]["code"], &answer["error"]["request_id"], &answer["error"]["request_status"]),
            (StatusCode::CONFLICT, &json!("duplicate_request"), &json!(held.id), &json!("reserved")), "{answer}");
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["settlement_error"], &view["ledger"]["active_requests"]),
            (&json!(false), &json!(inference_gateway::DURABLE_SETTLEMENT_FAILED), &json!(1)), "{view}");
        assert_eq!(balances(&runtime), holding);
        ledger.execute_batch("DROP TRIGGER refuse_paid_settlement").unwrap();
        drop(ledger);
        runtime.dht.stop().await;
        drop(runtime);

        // Recovered by the start, before any request.
        let runtime = ProviderRuntime::start(config).await.unwrap();
        let recovered = runtime.state.ledger.request_view(&contract, "unsettled").unwrap();
        assert_eq!((recovered.id.as_str(), recovered.status.as_str(), recovered.usage.prompt_tokens, recovered.usage.completion_tokens,
            recovered.cost_wei.as_str(), recovered.provider_loss_wei.as_str(), recovered.max_cost_wei.as_str()),
            (held.id.as_str(), "restart_unknown_usage", 0, 0, "0", "10", "10"));
        assert_eq!(balances(&runtime), opened(), "recovery did not return exactly the hold");
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["settlement_error"], &view["ledger"]["active_requests"]),
            (&json!(true), &Value::Null, &json!(0)), "{view}");
        let (status, answer) = chat_request(&runtime, &credential.bearer, "unsettled").await;
        assert_eq!((status, &answer["error"]["code"], &answer["error"]["request_status"]),
            (StatusCode::CONFLICT, &json!("duplicate_request"), &json!("restart_unknown_usage")), "{answer}");
        assert_eq!(balances(&runtime), opened());
        completes_at_agreement_rates(&runtime, &contract, &credential.bearer, "after-the-restart").await;
        assert_eq!(balances(&runtime), ("994996".to_owned(), "0".to_owned(), "4".to_owned(), "0".to_owned(), (2, 1),
            "0".to_owned(), "4".to_owned()));
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// inference-request-accounting task 3.2, *Performance remedies preserve usage and admitted
    /// evidence* and *Restart recovers orphaned reservations before new admission*. A daemon's
    /// served report comes from its configuration, which does not change while it runs, and one
    /// daemon owns a deployment's ledger; a renewal therefore reaches the gateway through a
    /// restart, and startup recovery is where a request admitted under one report meets the next.
    ///
    /// Three synthetic reports of identical terms (parser inputs, not qualification) are bound by
    /// their files' digests: R0 (measured at 100), R1 (150) and R2 (200). Two accounts were opened
    /// under R0. The daemon serves R1. A chat request admitted through the gateway binds R1, not
    /// the agreement's R0, and its settlement write fails, so it stays reserved; three ledger
    /// misses of the other account then latch R1. The daemon restarts with R2. When the start
    /// returns, before any request, the held request is released as `restart_unknown_usage`
    /// still bound to R1 (no usage, no waived amount, its whole 10-wei hold as exposure), and its
    /// provider-incomplete observation joins R1's window, not R2's. R1 stays withdrawn; R2
    /// admits. A new request is charged 4 wei at the agreement's rates and binds R2. A request
    /// whose first token arrives after 2.1 s settles as a known-usage inference waiver bound to
    /// R2: no charge, its two input and one output token recorded, 4 wei waived once, only its
    /// hold returned; funding and the platform fee are unchanged. (The charged request is outside
    /// the qualified workload envelope, so its accounting does not depend on the stub's speed.)
    /// The gateway's status read and a signed receipt show each request's admitted digest beside
    /// the unchanged agreement (R0). Finally, two more ledger misses latch R2 while a streamed
    /// request admitted under R2 is held in the engine: a new request is refused before
    /// reservation, and the held request settles at its known usage (4 wei) with the gateway's
    /// own outcome bound to R2, admission and settlement health unchanged.
    #[tokio::test]
    async fn a_restart_under_a_renewed_report_recovers_held_requests_with_their_admitted_report() {
        use sha2::{Digest, Sha256};
        const TEMPLATE: &str = "{{ messages }}";
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (rpc, rpc_server) = serve(settled_chain(receipts.clone())).await;
        let report = |name: &str, measured_at: u64| {
            let (mut model, mut report) = crate::performance::evidence_tests::fixture();
            model.chat_template_sha256 = hex::encode(Sha256::digest(TEMPLATE));
            let version = crate::engine_runtime::ENGINE_VERSION;
            let profile = model.serving.as_mut().unwrap();
            profile.engine_version = version.into();
            profile.qualification.measured_at = measured_at;
            report["runtime_observation"]["engine_version"] = json!(version);
            report["measured_at"] = json!(measured_at);
            let evidence = directory.path().join(name);
            crate::performance::evidence_tests::persist(&mut model, &mut report, &evidence);
            (model, evidence)
        };
        let (r0, _) = report("r0.json", 100);
        let (r1, r1_evidence) = report("r1.json", 150);
        let (r2, r2_evidence) = report("r2.json", 200);
        let digest = |model: &protocol::ModelSpec| model.serving.as_ref().unwrap().qualification.evidence_sha256.clone();
        let (d0, d1, d2) = (digest(&r0), digest(&r1), digest(&r2));
        assert!(d0 != d1 && d1 != d2 && d0 != d2);
        assert!(r0.accepts_execution(&r1) && r0.accepts_execution(&r2));
        // The stub engine delays its whole answer to the chat message `slow-first-token` by 2.1 s,
        // past the 2 s TTFT, holds a streamed chat message `held` open after its first token
        // until released, and answers everything else at once.
        let stream = HeldStream::new();
        let engine = chat_engine_holding(r1.clone(), TEMPLATE, Some(stream.clone()))
            .layer(middleware::from_fn(|request: Request, next: Next| async move {
            let (parts, body) = request.into_parts();
            let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
            if serde_json::from_slice::<Value>(&bytes).is_ok_and(|body| body["messages"][0]["content"] == "slow-first-token") {
                tokio::time::sleep(Duration::from_millis(2_100)).await;
            }
            next.run(Request::from_parts(parts, axum::body::Body::from(bytes))).await
        }));
        let (engine_url, engine_server) = serve(engine).await;
        let (mut config, unavailable) = profiled_chat_config(directory.path(), rpc, &r1, r1_evidence, &engine_url).await;
        config.models[0].price = PriceSchedule { input_wei_per_1k: "1000".into(), output_wei_per_1k: "2000".into() };
        let mut renewed = config.clone();
        renewed.models[0].model = r2.clone();
        renewed.models[0].qualification_evidence_file = Some(r2_evidence);

        let runtime = ProviderRuntime::start(config).await.unwrap();
        let (contract, credential, _) = funded_account_for(&runtime.state, &receipts, "44", 1_000_000, &r0);
        let (other, other_credential, _) = funded_account_for(&runtime.state, &receipts, "55", 1_000_000, &r0);
        assert_eq!(runtime.state.qualified_model(&r0), Some(&r1));
        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("CREATE TRIGGER refuse_held_settlement AFTER UPDATE ON requests
            WHEN OLD.request_key='held-under-r1' AND NEW.status<>'reserved'
            BEGIN SELECT RAISE(FAIL,'injected settlement write failure'); END;").unwrap();
        let (status, answer) = chat_request(&runtime, &credential.bearer, "held-under-r1").await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::INTERNAL_SERVER_ERROR, &json!("internal_error")), "{answer}");
        assert_eq!(runtime.state.settlement_fault(), Some(inference_gateway::DURABLE_SETTLEMENT_FAILED));
        let held = runtime.state.ledger.request_view(&contract, "held-under-r1").unwrap();
        assert_eq!((held.status.as_str(), held.max_cost_wei.as_str(), held.prompt_tokens_reserved, held.max_output_tokens),
            ("reserved", "10", 2, 4));
        assert_eq!(serde_json::to_value(&held.performance).unwrap(), json!({"evidence_sha256": d1,
            "policy": crate::performance::POLICY, "eligibility": "pending", "ttft_us": null, "decode_us": null,
            "elapsed_us": null, "output_tokens": null, "attained": null, "remedy": "none"}),
            "the gateway's reservation binds the served report, not the agreement's");
        // Three ledger misses of the other account (supplied usage, a prerequisite) latch R1.
        let usage = protocol::TokenUsage { prompt_tokens: 2, completion_tokens: 1 };
        for index in 0..3 {
            let now = protocol::now().unwrap();
            let reservation = runtime.state.ledger.reserve(&other, &other_credential.id, &format!("r1-miss-{index}"),
                2, 4, now, &r1).unwrap();
            let miss = crate::performance::outcome(r1.serving.as_ref().unwrap(), true, Some(2_000_001), None, Some(&usage),
                "completed", false, Some(2_000_001), &d1);
            runtime.state.ledger.settle_measured(&reservation.id, &usage, "completed", Some(&miss), now).unwrap();
        }
        assert!(runtime.state.ledger.performance_withdrawn(&r1).unwrap());
        assert!(runtime.state.qualified_model(&r0).is_none());
        ledger.execute_batch("DROP TRIGGER refuse_held_settlement").unwrap();
        drop(ledger);
        runtime.dht.stop().await;
        drop(runtime);

        // Restarted with the renewal R2: recovered before any request, still bound to R1.
        let runtime = ProviderRuntime::start(renewed).await.unwrap();
        let recovered = runtime.state.ledger.request_view(&contract, "held-under-r1").unwrap();
        assert_eq!((recovered.id.as_str(), recovered.status.as_str(), recovered.usage.prompt_tokens, recovered.usage.completion_tokens,
            recovered.cost_wei.as_str(), recovered.provider_loss_wei.as_str(), recovered.waived_wei.as_str()),
            (held.id.as_str(), "restart_unknown_usage", 0, 0, "0", "10", "0"));
        let recovered_performance = json!({"evidence_sha256": d1, "policy": crate::performance::POLICY,
            "eligibility": "provider_incomplete", "ttft_us": null, "decode_us": null, "elapsed_us": null,
            "output_tokens": null, "attained": false, "remedy": "inference_waiver"});
        assert_eq!(serde_json::to_value(&recovered.performance).unwrap(), recovered_performance);
        let observations = || ledger_connection(&runtime).query_row(
            "SELECT json_group_array(json_array(request,evidence,miss)) FROM
                (SELECT request,evidence,miss FROM performance_observations ORDER BY sequence)", [],
            |row| row.get::<_, String>(0)).map(|text| serde_json::from_str::<Value>(&text).unwrap()).unwrap();
        let latches = || ledger_connection(&runtime).query_row(
            "SELECT json_group_array(json_array(model,evidence)) FROM performance_withdrawals", [],
            |row| row.get::<_, String>(0)).map(|text| serde_json::from_str::<Value>(&text).unwrap()).unwrap();
        let misses: Vec<Value> = (0..3).map(|index| {
            let id = runtime.state.ledger.request_view(&other, &format!("r1-miss-{index}")).unwrap().id;
            json!([id, d1, 1])
        }).collect();
        let mut expected_observations = misses.clone();
        expected_observations.push(json!([held.id, d1, 1]));
        assert_eq!(observations(), json!(expected_observations), "the recovered request joins its admitted report's window");
        assert_eq!(latches(), json!([["model", d1]]));
        assert_eq!((runtime.state.accepting.load(Ordering::Acquire), runtime.state.settlement_fault()), (true, None));
        assert_eq!(runtime.state.qualified_model(&r0), Some(&r2));
        let balances = |runtime: &ProviderRuntime| {
            let account = runtime.state.ledger.account(&contract).unwrap();
            let held = runtime.state.ledger.credentials(&contract).unwrap().into_iter()
                .find(|entry| entry.id == credential.id).unwrap();
            json!([account.funded_wei, account.fee_wei, account.available_wei, account.reserved_wei, account.spent_wei,
                account.waived_wei, account.provider_loss_wei, [account.usage.prompt_tokens, account.usage.completion_tokens],
                held.reserved_wei, held.spent_wei])
        };
        assert_eq!(balances(&runtime), json!(["1000000", "5000", "995000", "0", "0", "0", "0", [0, 0], "0", "0"]));

        // New requests bind R2: one charged at the agreement's rates, one a known-usage waiver.
        // The charged request allows 300 output tokens, beyond the qualified workload envelope
        // (256), so that its accounting does not depend on how fast the stub answers.
        let (status, answer) = call(&runtime.public_router(), "POST", "/v1/chat/completions",
            &keyed(&credential.bearer, "after-the-renewal"), Some(&json!({"model": "model",
            "messages": [{"role": "user", "content": "hello"}], "max_tokens": 300}))).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let charged = runtime.state.ledger.request_view(&contract, "after-the-renewal").unwrap();
        assert_eq!((charged.status.as_str(), charged.usage.prompt_tokens, charged.usage.completion_tokens,
            charged.max_cost_wei.as_str(), charged.cost_wei.as_str(), charged.waived_wei.as_str()),
            ("completed", 2, 1, "602", "4", "0"));
        let performance = charged.performance.as_ref().unwrap();
        assert_eq!((performance.evidence_sha256.as_str(), performance.eligibility.as_str(), performance.attained,
            performance.remedy.as_str()), (d2.as_str(), "outside_workload", None, "none"));
        let (status, answer) = call(&runtime.public_router(), "POST", "/v1/chat/completions",
            &keyed(&credential.bearer, "waived-after-the-renewal"), Some(&json!({"model": "model",
            "messages": [{"role": "user", "content": "slow-first-token"}], "max_tokens": 4}))).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let waived = runtime.state.ledger.request_view(&contract, "waived-after-the-renewal").unwrap();
        assert_eq!((waived.status.as_str(), waived.usage.prompt_tokens, waived.usage.completion_tokens,
            waived.max_cost_wei.as_str(), waived.cost_wei.as_str(), waived.waived_wei.as_str(), waived.provider_loss_wei.as_str()),
            ("completed", 2, 1, "10", "0", "4", "0"));
        let performance = waived.performance.as_ref().unwrap();
        assert_eq!((performance.evidence_sha256.as_str(), performance.attained, performance.remedy.as_str(), performance.output_tokens),
            (d2.as_str(), Some(false), "inference_waiver", Some(1)));
        assert!(performance.ttft_us.unwrap() > crate::performance::TTFT_US, "{performance:?}");
        expected_observations.push(json!([waived.id, d2, 1]));
        assert_eq!(observations(), json!(expected_observations));
        assert_eq!(latches(), json!([["model", d1]]), "R2 keeps its own window");
        assert_eq!(balances(&runtime), json!(["1000000", "5000", "994996", "0", "4", "4", "0", [4, 2], "0", "4"]));

        // Reads: the held request by key through the gateway, and a signed receipt binding every
        // request's admitted digest beside the agreement's terms.
        let read = bearer_header(&credential.bearer);
        let (status, view) = call(&runtime.public_router(), "GET", "/v1/requests/held-under-r1", &read, None).await;
        assert_eq!((status, &view["performance"]), (StatusCode::OK, &recovered_performance));
        let (status, receipt) = call(&runtime.public_router(), "GET", &format!("/v1/contracts/{contract}/receipt"), &read, None).await;
        assert_eq!(status, StatusCode::OK, "{receipt}");
        let receipt: Signed<protocol::UsageReceipt> = serde_json::from_value(receipt).unwrap();
        protocol::verify(protocol::RECEIPT_DOMAIN, &receipt, &runtime.state.wallet).unwrap();
        assert_eq!(receipt.payload.account.model, r0, "the agreement keeps R0's digest and measurement time");
        let bound: std::collections::BTreeMap<String, String> = receipt.payload.requests.iter()
            .map(|request| (request.request_id.clone(), request.performance.as_ref().unwrap().evidence_sha256.clone()))
            .collect();
        assert_eq!(bound, [("after-the-renewal", &d2), ("held-under-r1", &d1), ("waived-after-the-renewal", &d2)]
            .into_iter().map(|(key, evidence)| (key.to_owned(), evidence.clone())).collect());
        let (status, answer) = chat_request(&runtime, &credential.bearer, "held-under-r1").await;
        assert_eq!((status, &answer["error"]["code"], &answer["error"]["request_status"]),
            (StatusCode::CONFLICT, &json!("duplicate_request"), &json!("restart_unknown_usage")), "{answer}");

        // R2 is latched while a request admitted under it is in the engine (review addition).
        // New requests are refused before reservation, and the held request still settles at its
        // known usage with the gateway's own outcome bound to R2: settlement classifies with the
        // report admitted at reservation and never re-reads the current qualification state. It
        // allows 300 output tokens, outside the qualified workload envelope, so that its
        // accounting does not depend on timing.
        let admitted = tokio::spawn({
            let (public, headers) = (runtime.public_router(), keyed(&credential.bearer, "held-while-r2-latches"));
            let body = json!({"model": "model", "messages": [{"role": "user", "content": "held"}],
                "max_tokens": 300, "stream": true});
            async move { send(&public, "POST", "/v1/chat/completions", &headers, Some(&body)).await }
        });
        stream.entered().await;
        let pending = runtime.state.ledger.request_view(&contract, "held-while-r2-latches").unwrap();
        assert_eq!((pending.status.as_str(), pending.max_cost_wei.as_str()), ("reserved", "602"));
        assert_eq!(pending.performance.as_ref().unwrap().evidence_sha256, d2);
        // Two ledger misses of the other account join the waived request's miss and latch R2.
        for index in 0..2 {
            let now = protocol::now().unwrap();
            let reservation = runtime.state.ledger.reserve(&other, &other_credential.id, &format!("r2-miss-{index}"),
                2, 4, now, &r2).unwrap();
            let miss = crate::performance::outcome(r2.serving.as_ref().unwrap(), true, Some(2_000_001), None, Some(&usage),
                "completed", false, Some(2_000_001), &d2);
            runtime.state.ledger.settle_measured(&reservation.id, &usage, "completed", Some(&miss), now).unwrap();
            let id = runtime.state.ledger.request_view(&other, &format!("r2-miss-{index}")).unwrap().id;
            expected_observations.push(json!([id, d2, 1]));
        }
        assert_eq!(latches(), json!([["model", d1], ["model", d2]]));
        assert!(runtime.state.qualified_model(&r0).is_none());
        let (status, answer) = chat_request(&runtime, &credential.bearer, "after-r2-latched").await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("model_unqualified")),
            "{answer}");
        assert!(runtime.state.ledger.request_view(&contract, "after-r2-latched").is_err());
        stream.release();
        let (status, text) = within(admitted).await.unwrap();
        let text = String::from_utf8(text).unwrap();
        assert_eq!(status, StatusCode::OK, "{text}");
        assert!(text.contains("\"OK\"") && text.ends_with("data: [DONE]\n\n") && !text.contains("\"error\""), "{text}");
        let settled = within(async {
            loop {
                let view = runtime.state.ledger.request_view(&contract, "held-while-r2-latches").unwrap();
                if view.status != "reserved" {
                    break view;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        assert_eq!((settled.id.as_str(), settled.status.as_str(), settled.usage.prompt_tokens, settled.usage.completion_tokens,
            settled.cost_wei.as_str(), settled.waived_wei.as_str(), settled.provider_loss_wei.as_str()),
            (pending.id.as_str(), "completed", 2, 1, "4", "0", "0"));
        let performance = settled.performance.as_ref().unwrap();
        assert_eq!((performance.evidence_sha256.as_str(), performance.policy.as_str(), performance.eligibility.as_str(),
            performance.attained, performance.remedy.as_str(), performance.output_tokens),
            (d2.as_str(), crate::performance::POLICY, "outside_workload", None, "none", Some(1)));
        assert!(performance.ttft_us.is_some() && performance.elapsed_us.is_some(),
            "the gateway's own measured outcome: {performance:?}");
        assert_eq!((runtime.state.accepting.load(Ordering::Acquire), runtime.state.settlement_fault()), (true, None));
        assert_eq!(observations(), json!(expected_observations), "an outside-workload request adds no observation");
        assert_eq!(latches(), json!([["model", d1], ["model", d2]]));
        assert_eq!(balances(&runtime), json!(["1000000", "5000", "994992", "0", "8", "4", "0", [6, 3], "0", "8"]));
        runtime.dht.stop().await;
        for server in [engine_server, rpc_server, unavailable] {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 5 (task 3.5): the daemon's periodic pass
    /// (`ProviderRuntime::maintenance`, whose period unit tests shorten) keeps running after a
    /// failed pass and while the provider drains. A published provider with a credited funding can
    /// no longer advance its offer revision, so each pass fails at publication: the status names
    /// the failure, new funding is closed and nothing is published, while the existing account is
    /// served. A periodic pass after such a failure still rechecks the credited funding; so does
    /// one after the operator drains, which publishes nothing and keeps the drain.
    #[tokio::test]
    async fn the_periodic_pass_keeps_running_after_a_failed_pass_and_while_drained() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (runtime, bearer, servers) = embedding_provider_on(directory.path(), &["embeddings"],
            Arc::new(AtomicBool::new(true)), settled_chain(receipts.clone()), false).await;
        let (_, _, opening) = funded_account(&runtime.state, &receipts, "44", 1000);
        let rechecked_since = |since: u64| {
            let (runtime, opening) = (runtime.clone(), opening.clone());
            async move {
                within(async {
                    while funding_checked(&runtime, &opening) < since {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }).await
            }
        };
        *runtime.revision.lock().await = u64::MAX;
        let periodic = runtime.maintenance();
        // A periodic pass fails at publication.
        within(async {
            while runtime.refresh_error().await.is_none() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await;
        // A later periodic pass runs all the same: it rechecks the credited funding.
        rechecked_since(unchecked(&runtime, &opening)).await;
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["refresh_error"]),
            (&json!(true), &json!(false), &json!("Persisting offer revision failed")), "{view}");
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        {
            // The existing account is served between failed passes. A pass's readiness probe
            // takes the model's only slot, which requests share, so the request is sent while
            // the test holds the lifecycle lock: no pass is probing then.
            let _no_pass = runtime.lifecycle.lock().await;
            assert_eq!(embed(&runtime, &bearer, "between-failed-passes").await.0, StatusCode::OK);
        }
        // Drained, the periodic pass continues: it rechecks, succeeds without publishing, and the
        // drain holds.
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        rechecked_since(unchecked(&runtime, &opening)).await;
        within(async {
            while runtime.refresh_error().await.is_some() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await;
        assert_eq!(admission(&runtime).await, (json!(false), json!(true), json!(false)));
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        periodic.abort();
        let _ = periodic.await;
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 5 (task 3.5), the ledger gate: admission is open only
    /// when the pass's reconciliation could read and update the ledger. A published provider whose
    /// credited-funding records cannot be read (the table is renamed through a second connection)
    /// runs a pass that fails at reconciliation: admission and new funding close, the offer is
    /// withdrawn, the status names the failed pass, and a request is refused before any record.
    /// Once the ledger can be read again, a later pass reopens admission and publishes.
    #[tokio::test]
    async fn a_failed_reconciliation_closes_admission_and_publication_until_a_later_pass_succeeds() {
        let directory = tempfile::tempdir().unwrap();
        let (runtime, bearer, servers) =
            embedding_provider(directory.path(), &["embeddings"], Arc::new(AtomicBool::new(true))).await;
        let contract = runtime.state.ledger.list_accounts().unwrap()[0].contract_id.clone();
        assert_eq!(admission(&runtime).await, (json!(true), json!(false), json!(true)));
        let ledger = ledger_connection(&runtime);
        ledger.execute_batch("ALTER TABLE funding RENAME TO funding_unreadable").unwrap();
        assert_eq!(runtime.refresh().await, Err("Funding ledger reconciliation failed".into()));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["refresh_error"]),
            (&json!(false), &json!(false), &json!("Funding ledger reconciliation failed")), "{view}");
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        let (status, answer) = embed(&runtime, &bearer, "unreadable-ledger").await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("provider_paused")), "{answer}");
        assert!(runtime.state.ledger.request_view(&contract, "unreadable-ledger").is_err());
        ledger.execute_batch("ALTER TABLE funding_unreadable RENAME TO funding").unwrap();
        assert_eq!(runtime.refresh().await, Ok(1));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["refresh_error"]),
            (&json!(true), &json!(true), &Value::Null), "{view}");
        assert_eq!(embed(&runtime, &bearer, "readable-ledger").await.0, StatusCode::OK);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    // provider-daemon-lifecycle acceptance evidence, tasks 3.7 to 3.10.

    /// The served chat template of `development_chat_provider`'s model.
    const DEVELOPMENT_TEMPLATE: &str = "{{ messages }}";

    /// `unavailable_provider` whose chain RPC is `chain` and whose only model, the development chat
    /// model of the offer fixture, runs on `chat_engine`, priced at 1,000 wei per 1,000 input tokens
    /// and 2,000 per 1,000 output tokens: `chat_request` (two prompt tokens, at most four output
    /// tokens, one generated) then reserves 10 wei and costs 4. A development provider that
    /// publishes its offer in its own registry. Returns the configuration and the servers to abort.
    async fn development_chat_provider(directory: &std::path::Path, chain: Router)
        -> (ProviderConfig, Vec<tokio::task::JoinHandle<()>>) {
        use sha2::{Digest, Sha256};
        let (mut config, unavailable) = unavailable_provider(directory).await;
        let (rpc, rpc_server) = serve(chain).await;
        config.network.rpc_url = rpc;
        let deployment = &mut config.models[0];
        deployment.model.chat_template_sha256 = hex::encode(Sha256::digest(DEVELOPMENT_TEMPLATE));
        deployment.price = PriceSchedule { input_wei_per_1k: "1000".into(), output_wei_per_1k: "2000".into() };
        deployment.request_timeout_secs = 30;
        let (engine, engine_server) = serve(chat_engine(deployment.model.clone(), DEVELOPMENT_TEMPLATE)).await;
        deployment.engine_url = engine;
        (config, vec![engine_server, rpc_server, unavailable])
    }

    /// Sends `chat_request` under `key` with `bearer` of the funded `contract` of a
    /// `development_chat_provider`, and asserts that it completes at that agreement's rates of 1,000
    /// and 2,000 wei per 1,000 tokens: 10 wei reserved and 4 charged.
    async fn completes_at_agreement_rates(runtime: &Arc<ProviderRuntime>, contract: &str, bearer: &str, key: &str) {
        let (status, answer) = chat_request(runtime, bearer, key).await;
        assert_eq!(status, StatusCode::OK, "{key}: {answer}");
        let view = runtime.state.ledger.request_view(contract, key).unwrap();
        assert_eq!((view.status.as_str(), view.max_cost_wei.as_str(), view.cost_wei.as_str()), ("completed", "10", "4"), "{key}");
    }

    /// The chain RPC of `ready_chain`, except that it reports chain ID `chain_id` and block 0 with
    /// hash `genesis`.
    fn chain_reporting(chain_id: &'static str, genesis: String) -> Router {
        Router::new().fallback(move |request: Json<Value>| {
            let genesis = genesis.clone();
            async move {
                let method = request["method"].as_str().unwrap_or("").to_owned();
                let result = match method.as_str() {
                    "eth_chainId" => json!(chain_id),
                    "eth_getBlockByNumber" if request["params"][0] == "0x0" => json!({"number": "0x0", "hash": genesis,
                        "timestamp": format!("0x{:x}", protocol::now().unwrap() - 12)}),
                    _ => return ready_chain(request).await,
                };
                Json(json!({"jsonrpc": "2.0", "id": request["id"], "result": result})).into_response()
            }
        })
    }

    /// provider-daemon-lifecycle requirement 7 (task 3.7). A development chat provider whose chain
    /// RPC answers as `settled_chain` publishes its offer and holds a funded account. Restarted:
    /// - with a chain RPC that reports chain ID 72, and with one that reports another genesis, the
    ///   start is refused with "configured RPC identity was rejected" (scenario "A rejected chain
    ///   identity stops the start");
    /// - while the RPC answers 503, it starts: `/readyz` answers 200 with `ready` true and
    ///   `funding_ready` false, `/admin/offers` and the DHT hold no offer, the status names the
    ///   outage and no failed pass, the funded account's request is admitted and charged at its
    ///   agreement's rates, and a quote proposal is refused for new funding (scenario "An
    ///   unavailable chain RPC does not stop the start"); once the RPC answers again, a pass
    ///   publishes;
    /// - over an exhausted offer revision, it starts, reports the failed pass, is not new-funding
    ///   ready, publishes nothing and admits the funded account's request (scenario "A
    ///   publication failure at start leaves the daemon running", with a funded account);
    /// - with a reserved request whose recovery the ledger refuses, the start is refused and the
    ///   request stays reserved (a startup-recovery failure is fatal); once the ledger accepts the
    ///   write, the start recovers it.
    #[tokio::test]
    async fn startup_refuses_a_rejected_chain_identity_or_failed_recovery_and_runs_through_an_outage_or_publication_failure() {
        use tower::ServiceExt;
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let outage = Arc::new(AtomicBool::new(false));
        let chain = settled_chain(receipts.clone()).layer(middleware::from_fn({
            let outage = outage.clone();
            move |request: Request, next: Next| {
                let outage = outage.clone();
                async move {
                    if outage.load(Ordering::Acquire) {
                        StatusCode::SERVICE_UNAVAILABLE.into_response()
                    } else {
                        next.run(request).await
                    }
                }
            }
        }));
        let (config, servers) = development_chat_provider(directory.path(), chain).await;
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        assert_eq!(published(&runtime).await.0.len(), 1);
        let (contract, credential, _) = funded_account(&runtime.state, &receipts, "44", 1_000_000);
        let ledger_path = runtime.state.config.runtime_data_dir().unwrap().join(&runtime.state.wallet)
            .join(runtime.dht.peer_id()).join("ledger.sqlite3");
        let revision_file = runtime.revision_file.clone();
        runtime.dht.stop().await;
        drop(runtime);

        // A rejected chain identity stops the start.
        for (case, chain) in [
            ("another chain ID", chain_reporting("0x48", crate::network::ESPACE_TESTNET_GENESIS.into())),
            ("another genesis", chain_reporting("0x47", format!("0x{}", "99".repeat(32)))),
        ] {
            let (rpc, server) = serve(chain).await;
            let mut rejected = config.clone();
            rejected.network.rpc_url = rpc;
            assert_eq!(ProviderRuntime::start(rejected).await.err().as_deref(), Some("configured RPC identity was rejected"), "{case}");
            server.abort();
        }

        // An unavailable chain RPC does not stop the start.
        outage.store(true, Ordering::Release);
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        let response = runtime.public_router().oneshot(Request::builder().uri("/readyz")
            .body(axum::body::Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), json!({"ready": true, "funding_ready": false}));
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["draining"], &view["refresh_error"], &view["models"][0]["ready"]),
            (&json!(true), &json!(false), &json!(false), &Value::Null, &json!(true)), "{view}");
        assert_eq!(view["chain_error"], "Configured RPC unavailable; retaining last observed funding states", "{view}");
        completes_at_agreement_rates(&runtime, &contract, &credential.bearer, "during-the-outage").await;
        let (status, answer) = call(&runtime.public_router(), "POST", "/v1/contracts/propose", &[], Some(&json!({
            "offer_id": protocol::random_id(), "consumer_wallet": wallet::address_from_private_key(&"88".repeat(32)).unwrap(),
            "funding_wei": "1000"}))).await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("settlement_unavailable")), "{answer}");
        // Once the RPC answers again, a pass publishes.
        outage.store(false, Ordering::Release);
        assert_eq!(runtime.refresh().await, Ok(1));
        assert_eq!(admission(&runtime).await, (json!(true), json!(false), json!(true)));
        assert_eq!(published(&runtime).await.0.len(), 1);
        let revision = std::fs::read_to_string(&revision_file).unwrap();
        runtime.dht.stop().await;
        drop(runtime);

        // A publication failure at start leaves the provider running and serving the funded account.
        std::fs::write(&revision_file, format!("{}\n", u64::MAX)).unwrap();
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        let view = admin(&runtime, "GET", "/admin/status").await.1;
        assert_eq!((&view["accepting"], &view["funding_ready"], &view["refresh_error"], &view["chain_error"]),
            (&json!(true), &json!(false), &json!("Persisting offer revision failed"), &Value::Null), "{view}");
        assert_eq!(published(&runtime).await, (vec![], json!(0)));
        completes_at_agreement_rates(&runtime, &contract, &credential.bearer, "after-a-failed-starting-pass").await;
        runtime.dht.stop().await;
        drop(runtime);
        std::fs::write(&revision_file, revision).unwrap();

        // A failed startup recovery stops the start.
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        runtime.state.ledger.reserve(&contract, &credential.id, "left-reserved", 2, 4, protocol::now().unwrap(),
            &config.models[0].model).unwrap();
        runtime.dht.stop().await;
        drop(runtime);
        let ledger = rusqlite::Connection::open(&ledger_path).unwrap();
        ledger.busy_timeout(Duration::from_secs(5)).unwrap();
        ledger.execute_batch("CREATE TRIGGER refuse_recovery BEFORE UPDATE ON requests
            WHEN OLD.status='reserved' AND NEW.status<>'reserved'
            BEGIN SELECT RAISE(ABORT,'injected recovery write failure'); END;").unwrap();
        let refused = ProviderRuntime::start(config.clone()).await.err().unwrap_or_default();
        assert!(refused.contains("injected recovery write failure"), "{refused}");
        let status: String = ledger.query_row("SELECT status FROM requests WHERE request_key='left-reserved'", [],
            |row| row.get(0)).unwrap();
        assert_eq!(status, "reserved");
        ledger.execute_batch("DROP TRIGGER refuse_recovery").unwrap();
        drop(ledger);
        let runtime = ProviderRuntime::start(config).await.unwrap();
        assert_eq!(runtime.state.ledger.request_view(&contract, "left-reserved").unwrap().status, "restart_unknown_usage");
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// Every row of every table, and every schema object, of the SQLite database at `path`.
    fn database_snapshot(path: &std::path::Path) -> Value {
        let connection = rusqlite::Connection::open(path).unwrap();
        let objects: Vec<String> = connection
            .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name").unwrap()
            .query_map([], |row| Ok(format!("{:?}", (row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)))).unwrap()
            .collect::<Result<_, _>>().unwrap();
        json!({"rows": table_rows(&connection), "objects": objects})
    }

    /// Asserts that two `database_snapshot`s are equal, naming only the tables, or the schema, that
    /// differ, so that no stored credential digest reaches the failure message.
    fn assert_same_database(before: &Value, after: &Value, context: &str) {
        let tables: std::collections::BTreeSet<&String> = before["rows"].as_object().unwrap().keys()
            .chain(after["rows"].as_object().unwrap().keys()).collect();
        let mut differing: Vec<String> = tables.into_iter()
            .filter(|table| before["rows"][table.as_str()] != after["rows"][table.as_str()]).cloned().collect();
        if before["objects"] != after["objects"] {
            differing.push("schema objects".into());
        }
        assert!(differing.is_empty(), "{context}: changed {differing:?}");
    }

    /// provider-daemon-lifecycle requirement 9 (task 3.9), with requirement 10's rule that no
    /// startup refusal rewrites state. A development chat provider publishes its offer and opens a
    /// free account, then stops. Each refused start leaves the ledger it refused, every row and
    /// every schema object, unchanged:
    /// - its own ledger, recording schema 2, and then schema 4: each refused ("performance
    ///   protocol requires a fresh network-scoped ledger …"; scenario "An older schema is refused
    ///   without conversion", at the start, and the requirement's "different schema");
    /// - a copy in the deployment directory of the same network and peer identity under another
    ///   signing wallet: refused with `wrong_provider` (scenario "A ledger of another network or
    ///   provider is refused");
    /// - a copy in the deployment directory of another network identity (another deployment ID of
    ///   the same chain): `BillingStore::open` and the start each refuse it with `wrong_network`.
    /// The ledger holds a funded account, with its funding acknowledgement, credited funding and
    /// pending platform fee, and records schema 3, the configured network identity and the signing
    /// wallet. The signing key, the peer identity, the offer revision and the wallet journal are
    /// unchanged after all three refusals, and the deployment's own ledger then opens with its
    /// account and its recorded identity unchanged.
    #[tokio::test]
    async fn a_ledger_of_another_schema_network_or_provider_refuses_the_start_and_stays_unchanged() {
        use sha2::{Digest, Sha256};
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (config, servers) = development_chat_provider(directory.path(), settled_chain(receipts.clone())).await;
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        assert_eq!(published(&runtime).await.0.len(), 1);
        let (contract, _, _) = funded_account(&runtime.state, &receipts, "44", 1_000_000);
        let account = serde_json::to_value(runtime.state.ledger.account(&contract).unwrap()).unwrap();
        let (provider_wallet, peer) = (runtime.state.wallet.clone(), runtime.dht.peer_id());
        runtime.dht.stop().await;
        drop(runtime);
        let deployment = config.runtime_data_dir().unwrap().join(&provider_wallet).join(&peer);
        let ledger = deployment.join("ledger.sqlite3");
        let journals = files_named(&config.data_dir, "transactions.sqlite3");
        assert_eq!(journals.len(), 1, "{journals:?}");
        let digest = |path: &std::path::Path| hex::encode(Sha256::digest(std::fs::read(path).unwrap()));
        let files = || json!({"signing key": digest(&config.signing_key_file), "peer identity": digest(&config.dht.identity_file),
            "offer revision": std::fs::read_to_string(deployment.join("offer-revision")).unwrap()});
        let (files_before, journal_before, original) = (files(), database_snapshot(&journals[0]), database_snapshot(&ledger));
        for table in ["funding", "funding_intents", "fee_obligations"] {
            assert_eq!(original["rows"][table].as_array().map(Vec::len), Some(1), "{table}");
        }
        // The recorded schema, network identity and provider.
        let recorded = |path: &std::path::Path| -> Vec<(String, String)> {
            rusqlite::Connection::open(path).unwrap()
                .prepare("SELECT key, value FROM metadata WHERE key IN ('schema','network','provider') ORDER BY key").unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
        };
        let identity = recorded(&ledger);
        assert_eq!(identity.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(), ["network", "provider", "schema"]);
        assert_eq!(serde_json::from_str::<Value>(&identity[0].1).unwrap(), serde_json::to_value(&config.network.identity).unwrap());
        assert_eq!((identity[1].1.as_str(), identity[2].1.as_str()), (provider_wallet.as_str(), "3"));
        // Copies the ledger into the deployment directory `into`, as a restore by the operator would.
        let copy = |into: &std::path::Path| {
            wallet::secure_directory(into).unwrap();
            std::fs::copy(&ledger, into.join("ledger.sqlite3")).unwrap();
            into.join("ledger.sqlite3")
        };

        // An older schema, and a different (newer) one.
        for schema in ["2", "4"] {
            rusqlite::Connection::open(&ledger).unwrap().execute("UPDATE metadata SET value=?1 WHERE key='schema'", [schema]).unwrap();
            let refused_ledger = database_snapshot(&ledger);
            let refused = ProviderRuntime::start(config.clone()).await.err().unwrap_or_default();
            assert!(refused.starts_with("performance protocol requires a fresh network-scoped ledger"), "schema {schema}: {refused}");
            assert_same_database(&refused_ledger, &database_snapshot(&ledger), &format!("the refused ledger of schema {schema}"));
        }
        rusqlite::Connection::open(&ledger).unwrap().execute("UPDATE metadata SET value='3' WHERE key='schema'", []).unwrap();
        assert_same_database(&original, &database_snapshot(&ledger), "the ledger with its schema restored");

        // The ledger of another provider wallet.
        let mut other_provider = config.clone();
        other_provider.signing_key_file = directory.path().join("other-wallet.key");
        wallet::save_private_key(&other_provider.signing_key_file, &"33".repeat(32)).unwrap();
        let other_wallet = wallet::address_from_private_key(&"33".repeat(32)).unwrap();
        let copied = copy(&config.runtime_data_dir().unwrap().join(&other_wallet).join(&peer));
        assert_eq!(ProviderRuntime::start(other_provider).await.err().as_deref(),
            Some("wrong_provider: ledger belongs to another provider"));
        assert_same_database(&original, &database_snapshot(&copied), "the refused ledger of another provider");

        // The ledger of another network identity.
        let mut other_network = config.clone();
        other_network.network.identity.deployment_id = "another-deployment".into();
        let copied = copy(&other_network.runtime_data_dir().unwrap().join(&provider_wallet).join(&peer));
        let opened = BillingStore::open(&copied, &other_network.network.identity).err().unwrap_or_default();
        assert!(opened.starts_with("wrong_network: "), "{opened}");
        assert_same_database(&original, &database_snapshot(&copied), "the ledger that BillingStore::open refused");
        assert_eq!(ProviderRuntime::start(other_network).await.err().as_deref(),
            Some("wrong_network: ledger belongs to another chain/deployment"));
        assert_same_database(&original, &database_snapshot(&copied), "the refused ledger of another network");

        // Nothing else was rewritten, and the deployment's own ledger opens with its account.
        assert_eq!(files(), files_before);
        assert_same_database(&journal_before, &database_snapshot(&journals[0]), "the wallet journal");
        assert_same_database(&original, &database_snapshot(&ledger), "the deployment's own ledger");
        let runtime = ProviderRuntime::start(config).await.unwrap();
        assert_eq!(serde_json::to_value(runtime.state.ledger.account(&contract).unwrap()).unwrap(), account);
        assert_eq!(recorded(&ledger), identity, "the start changed the recorded identity");
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 10 (task 3.10), scenarios "Editing the configuration of
    /// a running daemon changes nothing" and "A price change does not re-price an existing
    /// account". A development chat provider, started from its configuration file, publishes its
    /// offer at rates R (1,000 and 2,000 wei per 1,000 input and output tokens) and holds a funded
    /// account under R.
    /// - The file is edited to price the input at 2R and to another model revision, and a pass
    ///   runs: the provider's configuration is still the one it loaded, its offer keeps the model
    ///   and the price under a higher revision, and the account is served at R.
    /// - The operator drains, prices the input at 2R and restarts from the file: the account is
    ///   unchanged; the new offer advertises 2R under a revision higher than any earlier one; the
    ///   account's next request reserves 10 wei and is charged 4 at R, not 12 and 6 at 2R; and a
    ///   quote proposal for the new offer takes 2R.
    #[tokio::test]
    async fn configuration_changes_reach_new_offers_only_through_a_restart_and_never_reprice_an_agreement() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (config, servers) = development_chat_provider(directory.path(), settled_chain(receipts.clone())).await;
        let file = directory.path().join("provider.json");
        let write = |config: &ProviderConfig| std::fs::write(&file, serde_json::to_vec_pretty(config).unwrap()).unwrap();
        write(&config);
        let loaded = ProviderConfig::load(&file).unwrap();
        let runtime = ProviderRuntime::start(loaded.clone()).await.unwrap();
        let only = |offers: Vec<Value>| {
            assert_eq!(offers.len(), 1);
            offers[0]["payload"].clone()
        };
        let first = only(published(&runtime).await.0);
        assert_eq!(first["price"], json!({"input_wei_per_1k": "1000", "output_wei_per_1k": "2000"}));
        let (contract, credential, _) = funded_account(&runtime.state, &receipts, "44", 1_000_000);

        // Edited under the running provider: nothing changes.
        let mut edited = loaded.clone();
        edited.models[0].price.input_wei_per_1k = "2000".into();
        edited.models[0].model.revision = "cd".repeat(20);
        write(&edited);
        assert_eq!(runtime.refresh().await, Ok(1));
        assert_eq!(serde_json::to_value(&*runtime.state.config).unwrap(), serde_json::to_value(&loaded).unwrap());
        let second = only(published(&runtime).await.0);
        assert_eq!((&second["model"], &second["price"]), (&first["model"], &first["price"]));
        assert!(second["revision"].as_u64().unwrap() > first["revision"].as_u64().unwrap());
        completes_at_agreement_rates(&runtime, &contract, &credential.bearer, "after-the-edit").await;

        // Drained, repriced and restarted: the new price reaches new offers only.
        assert_eq!(admin(&runtime, "POST", "/admin/drain").await.0, StatusCode::OK);
        let account = serde_json::to_value(runtime.state.ledger.account(&contract).unwrap()).unwrap();
        runtime.dht.stop().await;
        drop(runtime);
        let mut repriced = loaded.clone();
        repriced.models[0].price.input_wei_per_1k = "2000".into();
        write(&repriced);
        let runtime = ProviderRuntime::start(ProviderConfig::load(&file).unwrap()).await.unwrap();
        assert_eq!(serde_json::to_value(runtime.state.ledger.account(&contract).unwrap()).unwrap(), account);
        let third = only(published(&runtime).await.0);
        assert_eq!(third["price"], json!({"input_wei_per_1k": "2000", "output_wei_per_1k": "2000"}));
        assert_eq!(third["model"], first["model"]);
        assert!(third["revision"].as_u64().unwrap() > second["revision"].as_u64().unwrap());
        completes_at_agreement_rates(&runtime, &contract, &credential.bearer, "after-the-restart").await;
        let after = runtime.state.ledger.account(&contract).unwrap();
        assert_eq!((after.price.input_wei_per_1k.as_str(), after.spent_wei.as_str()), ("1000", "8"));
        // A new agreement takes the new price. The ledger signs no quote in its first minute, so
        // it is given an earlier minute of quote signing.
        let minute = protocol::now().unwrap() / 60 - 1;
        ledger_connection(&runtime).execute_batch(&format!(
            "INSERT INTO metadata(key,value) VALUES('quote_work_minute','{minute}');
             INSERT INTO rate_limits(bucket,window,count) VALUES('quote-work:global',{minute},1);")).unwrap();
        let offer_id = runtime.published.lock().await.iter().next().unwrap().clone();
        let (status, quote) = call(&runtime.public_router(), "POST", "/v1/contracts/propose", &[], Some(&json!({
            "offer_id": offer_id, "consumer_wallet": wallet::address_from_private_key(&"88".repeat(32)).unwrap(),
            "funding_wei": "1000"}))).await;
        assert_eq!(status, StatusCode::OK, "{quote}");
        assert_eq!(quote["payload"]["price"], third["price"]);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }

    /// provider-daemon-lifecycle requirement 10 (task 3.10), scenario "A replaced model does not
    /// spend an old balance". A development chat provider opens a funded account through its
    /// acknowledged opening, serves it one request, and restarts configured only with another
    /// revision of the model, which it serves and publishes. The account's request is refused
    /// (409 `model_revision_unavailable`) before reservation: no record, and the account, its
    /// balance and holds included, is unchanged. Its owner still reads the account, the earlier
    /// request, the receipt, the funding history and the credentials, recovers the funding
    /// acknowledgement as it was given and the opening (the same account and no new bearer), and
    /// revokes the credential, whose bearer is then refused.
    #[tokio::test]
    async fn a_restart_with_another_model_revision_refuses_old_agreements_before_reservation_and_keeps_their_control() {
        let directory = tempfile::tempdir().unwrap();
        let receipts = Receipts::default();
        let (config, servers) = development_chat_provider(directory.path(), settled_chain(receipts.clone())).await;
        let runtime = ProviderRuntime::start(config.clone()).await.unwrap();
        let quote = stored_quote(&runtime.state, "44", 1_000_000);
        let (payment, acknowledgement) = acknowledged_opening(&runtime.state, &receipts, "44", &quote);
        let evidence = crate::chain_client::VerifiedPayment {
            network: config.network.identity.clone(), tx_hash: payment.tx_hash.clone(),
            from: quote.payload.consumer_wallet.clone(), to: runtime.state.wallet.clone(), value_wei: "1000000".into(),
            data: protocol::opening_data(&quote.payload).unwrap(), block_hash: safe_block_hash(), block_number: 10,
            raw_transaction: payment.raw_transaction.clone(),
        };
        let credential = runtime.state.ledger.open_account(&quote, Some(&evidence), protocol::now().unwrap()).unwrap()
            .credential.unwrap();
        let contract = quote.payload.id.clone();
        completes_at_agreement_rates(&runtime, &contract, &credential.bearer, "before-the-replacement").await;
        let earlier = runtime.state.ledger.request_view(&contract, "before-the-replacement").unwrap();
        let account = serde_json::to_value(runtime.state.ledger.account(&contract).unwrap()).unwrap();
        runtime.dht.stop().await;
        drop(runtime);

        let mut replaced = config.clone();
        replaced.models[0].model.revision = "cd".repeat(20);
        let runtime = ProviderRuntime::start(replaced).await.unwrap();
        let (offers, _) = published(&runtime).await;
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0]["payload"]["model"]["revision"], json!("cd".repeat(20)));
        let public = runtime.public_router();
        let (status, answer) = call(&public, "POST", "/v1/chat/completions", &keyed(&credential.bearer, "after-the-replacement"),
            Some(&json!({"model": "model", "messages": [{"role": "user", "content": "hello"}], "max_tokens": 4}))).await;
        assert_eq!((status, &answer["error"]["code"]), (StatusCode::CONFLICT, &json!("model_revision_unavailable")), "{answer}");
        assert!(runtime.state.ledger.request_view(&contract, "after-the-replacement").is_err());
        assert_eq!(serde_json::to_value(runtime.state.ledger.account(&contract).unwrap()).unwrap(), account);
        // The account's status, its request, receipt, funding and credentials stay readable.
        let read = bearer_header(&credential.bearer);
        assert_eq!(call(&public, "GET", &format!("/v1/contracts/{contract}"), &read, None).await, (StatusCode::OK, account.clone()));
        let (status, view) = call(&public, "GET", "/v1/requests/before-the-replacement", &read, None).await;
        assert_eq!((status, &view["status"], &view["id"]), (StatusCode::OK, &json!("completed"), &json!(earlier.id)), "{view}");
        let (status, receipt) = call(&public, "GET", &format!("/v1/contracts/{contract}/receipt"), &read, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(receipt["payload"]["requests"].as_array().unwrap().iter().any(|request| request["id"] == json!(earlier.id)));
        let (status, funding) = call(&public, "GET", &format!("/v1/contracts/{contract}/funding"), &read, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(funding.to_string().to_ascii_lowercase().contains(&payment.tx_hash.to_ascii_lowercase()),
            "the funding history does not list the opening");
        let (status, credentials) = call(&public, "GET", &format!("/v1/contracts/{contract}/credentials"), &read, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(credentials.to_string().contains(&credential.id), "the credential list does not name the credential");
        // Its owner recovers the funding acknowledgement as it was given, and the opening, and
        // revokes the credential.
        let path = format!("/v1/contracts/{contract}/funding-intent");
        assert_eq!(owner_post(&public, &runtime.state, "44", &path, json!({"raw_transaction": payment.raw_transaction})).await,
            (StatusCode::OK, acknowledgement));
        let (status, recovered) = owner_post(&public, &runtime.state, "44", "/v1/contracts/open",
            json!({"quote_id": contract, "raw_transaction": payment.raw_transaction})).await;
        assert_eq!(status, StatusCode::OK, "{}", recovered["error"]);
        assert_eq!((&recovered["account"], &recovered["credential"]), (&account, &Value::Null));
        let path = format!("/v1/contracts/{contract}/credentials/{}", credential.id);
        assert_eq!(owner_call(&public, &runtime.state, "44", "DELETE", &path, None).await,
            (StatusCode::OK, json!({"revoked": true})));
        assert_eq!(call(&public, "GET", &format!("/v1/contracts/{contract}"), &read, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(serde_json::to_value(runtime.state.ledger.account(&contract).unwrap()).unwrap(), account);
        runtime.dht.stop().await;
        for server in servers {
            server.abort();
        }
    }
}
