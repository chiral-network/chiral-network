//! Production control and native consumer coverage for commercial terms and proposal quotas.
//! Pre-admitted loopback development offers and real disk-backed ledgers; no engine,
//! chain simulation, actual funding, readiness-probe or performance-qualification claims.
use axum::Json;
use chiral_network::{
    app::ProviderState,
    billing_store::BillingStore,
    consumer::{Consumer, ConsumerConfig},
    contracts,
    protocol::{self, InferenceOffer, Quote, Signed, OFFER_DOMAIN, QUOTE_DOMAIN},
    wallet,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, RwLock},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tower::Service;

#[path = "support/proposal.rs"]
mod proposal;
use proposal::{quote_work_evidence, restore_quote_work, PreparedProvider};

const PAYER: &str = "0x2222222222222222222222222222222222222222";
const PROPOSE: &str = "/v1/contracts/propose";

struct Fixture {
    state: Arc<ProviderState>,
    offer: Signed<InferenceOffer>,
    offer_id: String,
    ledger_path: PathBuf,
    base: String,
    client: reqwest::Client,
    submitted: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    shutdown: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() { let _ = shutdown.send(()); }
        if let Some(server) = self.server.take() { server.abort(); }
    }
}

impl Fixture {
    async fn new(directory: &Path, case: u64, input: &str, output: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let public_bind = listener.local_addr().unwrap();
        let admin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let PreparedProvider { state, offer, offer_id, ledger_path } = PreparedProvider::new(
            directory, case, input, output, public_bind, admin.local_addr().unwrap());
        let base = offer.payload.endpoint.clone();
        let submitted = Arc::new(AtomicBool::new(false));
        let observed = submitted.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let observed_requests = requests.clone();
        let router = contracts::router(state.clone()).fallback(move |Json(value): Json<Value>| {
            let observed = observed.clone();
            async move {
                let is_submission = |call: &Value| matches!(call["method"].as_str(),
                    Some("eth_sendRawTransaction" | "eth_sendTransaction"));
                if is_submission(&value) || value.as_array().is_some_and(|batch| batch.iter().any(is_submission)) {
                    observed.store(true, Ordering::Release);
                }
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            }
        }).layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                observed_requests.fetch_add(1, Ordering::AcqRel);
                async move { next.run(request).await }
            },
        ));
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10)).build().unwrap();
        Self { state, offer, offer_id, ledger_path, base, client, submitted, requests,
            shutdown: Some(shutdown), server: Some(server) }
    }

    fn request(&self, funding: &str) -> Value {
        json!({"offer_id": self.offer_id, "consumer_wallet": PAYER, "funding_wei": funding})
    }
    async fn post(&self, path: &str, body: &Value) -> reqwest::Response {
        self.client.post(format!("{}{path}", self.base)).json(body).send().await.unwrap()
    }
    async fn raw(&self, body: Vec<u8>) -> reqwest::Response {
        self.client.post(format!("{}{PROPOSE}", self.base)).header("content-type", "application/json")
            .body(body).send().await.unwrap()
    }
    async fn propose(&self, funding: &str) -> Signed<Quote> {
        let response = self.post(PROPOSE, &self.request(funding)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let signed: Signed<Quote> = response.json().await.unwrap();
        self.verify_quote(&signed, PAYER, funding);
        signed
    }
    fn verify_quote(&self, signed: &Signed<Quote>, payer: &str, funding: &str) {
        protocol::verify(QUOTE_DOMAIN, signed, &self.state.wallet).unwrap();
        let q = &signed.payload;
        let o = &self.offer.payload;
        assert_eq!(q.network, o.network);
        assert_eq!(q.offer_id, self.offer_id);
        assert_eq!(q.provider_wallet, o.provider_wallet);
        assert_eq!(q.consumer_wallet, payer);
        assert_eq!(q.model, o.model);
        assert_eq!(q.price, o.price);
        assert_eq!(q.funding_wei, funding);
        assert_eq!(q.platform_wallet, o.platform_wallet);
        assert_eq!(q.fee_bps, o.fee_bps);
        assert_eq!(q.max_concurrency, o.max_concurrency);
        assert_eq!(q.expires_at.checked_sub(q.issued_at), Some(300));
        for identifier in [&q.id, &q.nonce] {
            let hex = identifier.strip_prefix("0x").unwrap();
            assert_eq!(hex.len(), 64);
            assert!(hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        }
    }
    // Deterministic ledger-admission prerequisite, not a public HTTP issuance claim.
    // A later sampled minute pins the committed clock while the real route samples wall time.
    fn commit_prerequisite(&self, payer: &str, funding: &str, now: u64) -> Signed<Quote> {
        let offer = &self.offer.payload;
        let quote = protocol::sign(QUOTE_DOMAIN, Quote {
            id: protocol::random_id(), network: offer.network.clone(),
            offer_id: self.offer_id.clone(), provider_wallet: offer.provider_wallet.clone(),
            consumer_wallet: payer.into(), model: offer.model.clone(), price: offer.price.clone(),
            funding_wei: funding.into(), platform_wallet: offer.platform_wallet.clone(),
            fee_bps: offer.fee_bps, max_concurrency: offer.max_concurrency,
            nonce: protocol::random_id(), issued_at: now, expires_at: now + 300,
        }, &self.state.private_key).unwrap();
        self.state.ledger.store_quote(&quote, now).unwrap();
        quote
    }
    async fn stop(mut self) {
        self.shutdown.take().unwrap().send(()).unwrap();
        self.server.take().unwrap().await.unwrap();
    }
}

async fn reject(response: reqwest::Response, status: StatusCode, code: &str) {
    assert_eq!(response.status(), status);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], code);
}

// Reconstruct the router with an independently opened durable ledger, not a
// clone of BillingStore's connection or a replacement signing callback.
fn reopened_router(fixture: &Fixture, private_key: &str) -> axum::Router {
    let original = &fixture.state;
    let state = Arc::new(ProviderState {
        config: original.config.clone(), chain: original.chain.clone(),
        ledger: Arc::new(BillingStore::open(&fixture.ledger_path,
            &original.config.network.identity).unwrap()),
        private_key: Arc::new(private_key.to_owned()), wallet: original.wallet.clone(),
        offers: original.offers.clone(),
        accepting: AtomicBool::new(original.accepting.load(Ordering::Acquire)),
        funding_ready: AtomicBool::new(original.funding_ready.load(Ordering::Acquire)),
        qualified_models: RwLock::new(original.qualified_models.read().unwrap().clone()),
        embedding_dimensions: RwLock::new(original.embedding_dimensions.read().unwrap().clone()),
        model_slots: original.model_slots.clone(), managed_engines: original.managed_engines.clone(),
    });
    contracts::router(state)
}

async fn route_proposal(mut router: axum::Router, body: &Value) -> axum::response::Response {
    router.call(axum::http::Request::builder().method("POST").uri(PROPOSE)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(serde_json::to_vec(body).unwrap())).unwrap())
        .await.unwrap()
}

async fn route_error(response: axum::response::Response, status: StatusCode, code: &str) {
    assert_eq!(response.status().as_u16(), status.as_u16());
    let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], code);
}

#[tokio::test]
async fn public_quote_work_initialization_refuses_durably_without_erasing_opened_terms() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 32, "0", "0").await;
    let config = ConsumerConfig {
        network: fixture.state.config.network.clone(),
        data_dir: directory.path().join("consumer"),
        catalog_url: None, development: true, dht: None,
    };
    let consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, &format!("{:064x}", 50_032)).unwrap();
    consumer.wallet_import_file(&import).unwrap();
    let original = consumer.quote(&fixture.offer, "0").await.unwrap();
    let account = serde_json::to_value(consumer.fund(&fixture.offer, &original).await.unwrap()).unwrap();
    let credential = consumer.credential_export(&original.payload.id, None).unwrap();
    let db = rusqlite::Connection::open(&fixture.ledger_path).unwrap();
    // Remove BOTH restored fixture rows: this is genuine production initialization
    // of missing work evidence, not a pre-populated exhausted-budget assertion.
    db.execute_batch("BEGIN IMMEDIATE;
        DELETE FROM metadata WHERE key='quote_work_minute';
        DELETE FROM rate_limits WHERE bucket='quote-work:global';
        COMMIT;").unwrap();
    let before = protocol::now().unwrap() / 60;
    reject(fixture.post(PROPOSE, &fixture.request("0")).await,
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    let after = protocol::now().unwrap() / 60;
    let (watermark, minute, count) = quote_work_evidence(&fixture.ledger_path);
    assert!((before..=after).contains(&minute));
    assert_eq!(watermark, minute.to_string());
    assert_eq!(count, 600);
    let quotes: u64 = db.query_row("SELECT COUNT(*) FROM quotes", [], |row| row.get(0)).unwrap();
    assert_eq!(quotes, 0, "the opened agreement is account-retained; initialization must not create a standalone quote");

    // Restore that exhausted shape in a future effective window solely to avoid a
    // wall-clock minute race in the restart assertions; this is not observed work.
    let stable_minute = after + 10;
    restore_quote_work(&fixture.ledger_path, stable_minute, 600);
    let request = fixture.request("0");
    let restarted = reopened_router(&fixture, &fixture.state.private_key);
    route_error(route_proposal(restarted.clone(), &request).await,
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    let resumed = consumer.fund(&fixture.offer, &original).await.unwrap();
    assert_eq!(serde_json::to_value(resumed).unwrap(), account);
    assert_eq!(consumer.credential_export(&original.payload.id, None).unwrap(), credential);
    let saved = fixture.state.ledger.quote(&original.payload.id).unwrap();
    assert_eq!(saved.payload, original.payload);
    assert_eq!(saved.signature, original.signature);
    assert!(fixture.state.ledger.funding_history(&original.payload.id).unwrap().is_empty());
    assert!(!fixture.submitted.load(Ordering::Acquire));
    let ledger_path = fixture.ledger_path.clone();
    fixture.stop().await;
    route_error(route_proposal(restarted, &request).await,
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    assert_eq!(quote_work_evidence(&ledger_path),
        (stable_minute.to_string(), stable_minute, 600));
}

#[tokio::test]
async fn public_quote_signing_failures_spend_global_work_across_wallets_and_reopened_routers() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 33, "1", "2").await;
    let original = fixture.propose("100").await;
    let minute = protocol::now().unwrap() / 60 + 10;
    // Three remaining attempts are restored evidence, not 597 observed signatures.
    restore_quote_work(&fixture.ledger_path, minute, 597);
    let payer = wallet::address_from_private_key(&format!("{:064x}", 50_033)).unwrap();
    let alias = format!("0x{}", payer[2..].to_ascii_uppercase());
    assert_ne!(payer, alias);
    let invalid = reopened_router(&fixture, "not-a-private-key");
    let independently_reopened = reopened_router(&fixture, "not-a-private-key");
    let db = rusqlite::Connection::open(&fixture.ledger_path).unwrap();
    let committed = || db.query_row(
        "SELECT window,count FROM rate_limits WHERE bucket='quote:global'", [],
        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))).unwrap();
    let before = committed();
    for (index, (router, named)) in [(invalid, &payer), (independently_reopened, &alias)]
        .into_iter().enumerate()
    {
        let mut request = fixture.request("100");
        request["consumer_wallet"] = json!(named);
        // The real protocol signer rejects this key. A permitted attempt is 500,
        // whereas denial before signing below is 429; no callback mock is involved.
        route_error(route_proposal(router, &request).await,
            StatusCode::INTERNAL_SERVER_ERROR, "internal_error").await;
        assert_eq!(quote_work_evidence(&fixture.ledger_path),
            (minute.to_string(), minute, 598 + index as u64));
        assert_eq!(committed(), before, "signing failure changed committed accounting");
    }
    let sampled_before = protocol::now().unwrap();
    let last = fixture.propose("100").await; // Another named wallet shares the final permit.
    let sampled_after = protocol::now().unwrap();
    assert!((sampled_before..=sampled_after).contains(&last.payload.issued_at));
    assert!(last.payload.issued_at / 60 < minute, "budget time replaced signed wall time");
    assert_eq!(quote_work_evidence(&fixture.ledger_path), (minute.to_string(), minute, 600));
    let restarted = reopened_router(&fixture, "not-a-private-key");
    route_error(route_proposal(restarted, &fixture.request("100")).await,
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    // A new actual HTTP client/connection and a different canonical owner cannot
    // acquire another budget or undo failed signatures.
    let mut request = fixture.request("100");
    request["consumer_wallet"] = json!(payer);
    let caller = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap();
    reject(caller.post(format!("{}{PROPOSE}", fixture.base)).json(&request).send().await.unwrap(),
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    assert_eq!(quote_work_evidence(&fixture.ledger_path), (minute.to_string(), minute, 600));
    let work_buckets: Vec<String> = db.prepare(
        "SELECT bucket FROM rate_limits WHERE bucket LIKE 'quote-work:%' ORDER BY bucket"
    ).unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(work_buckets, vec!["quote-work:global".to_string()]);
    let count: u64 = db.query_row("SELECT COUNT(*) FROM quotes", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 2);
    for signed in [&original, &last] {
        let saved = fixture.state.ledger.quote(&signed.payload.id).unwrap();
        assert_eq!(saved.payload, signed.payload);
        assert_eq!(saved.signature, signed.signature);
        assert!(fixture.state.ledger.account(&signed.payload.id).is_err());
    }
    assert!(!fixture.submitted.load(Ordering::Acquire));
}

#[tokio::test]
async fn failed_quote_response_body_does_not_refund_work_or_committed_quote() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 34, "1", "2").await;
    let original = fixture.propose("100").await;
    let minute = protocol::now().unwrap() / 60 + 10;
    restore_quote_work(&fixture.ledger_path, minute, 599);
    let router = reopened_router(&fixture, &fixture.state.private_key)
        .layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                let response = next.run(request).await;
                assert_eq!(response.status(), axum::http::StatusCode::OK);
                // Lose the reply only AFTER the real handler has signed and
                // durably inserted it, following the owner-response-loss fixture.
                let (mut parts, body) = response.into_parts();
                drop(body);
                parts.headers.remove(axum::http::header::CONTENT_LENGTH);
                axum::response::Response::from_parts(parts, axum::body::Body::from_stream(
                    futures_util::stream::once(async {
                        Err::<axum::body::Bytes, _>(std::io::Error::new(
                            std::io::ErrorKind::ConnectionReset, "isolated quote response loss"))
                    })))
            },
        ));
    let response = route_proposal(router, &fixture.request("100")).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert!(axum::body::to_bytes(response.into_body(), 96 * 1024).await.is_err());
    assert_eq!(quote_work_evidence(&fixture.ledger_path), (minute.to_string(), minute, 600));
    let db = rusqlite::Connection::open(&fixture.ledger_path).unwrap();
    let retained = || db.prepare("SELECT record FROM quotes ORDER BY id").unwrap()
        .query_map([], |row| row.get::<_, String>(0)).unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap();
    let before = retained();
    assert_eq!(before.len(), 2);
    let independently_opened = BillingStore::open(&fixture.ledger_path,
        &fixture.state.config.network.identity).unwrap();
    let mut latest_minute = original.payload.issued_at / 60;
    let mut expected_count = 2;
    for record in &before {
        let signed: Signed<Quote> = serde_json::from_str(record).unwrap();
        protocol::verify(QUOTE_DOMAIN, &signed, &fixture.state.wallet).unwrap();
        if signed.payload.id != original.payload.id {
            let lost_minute = signed.payload.issued_at / 60;
            if lost_minute > latest_minute {
                latest_minute = lost_minute;
                expected_count = 1;
            }
        }
        let saved = independently_opened.quote(&signed.payload.id).unwrap();
        assert_eq!(saved.payload, signed.payload);
        assert_eq!(saved.signature, signed.signature);
        assert!(independently_opened.account(&signed.payload.id).is_err());
    }
    assert_eq!(independently_opened.quote(&original.payload.id).unwrap().signature, original.signature);
    let committed = || db.query_row(
        "SELECT value,window,count FROM metadata,rate_limits
         WHERE key='quote_committed_minute' AND bucket='quote:global'", [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?))).unwrap();
    let charged = (latest_minute.to_string(), latest_minute, expected_count);
    assert_eq!(committed(), charged);
    route_error(route_proposal(reopened_router(&fixture, &fixture.state.private_key),
        &fixture.request("100")).await, StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    assert_eq!(retained(), before);
    assert_eq!(quote_work_evidence(&fixture.ledger_path), (minute.to_string(), minute, 600));
    assert_eq!(committed(), charged);
}

#[tokio::test]
async fn native_proposals_bind_complete_terms_and_survive_provider_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 1, "1", "2").await;
    let first = fixture.propose("100").await;
    let second = fixture.propose("101").await;
    assert_ne!(first.payload.id, second.payload.id);
    assert_ne!(first.payload.nonce, second.payload.nonce);
    let ledger_path = fixture.ledger_path.clone();
    let network = fixture.state.config.network.identity.clone();
    // A separate connection sees the complete signed record immediately after each response.
    let independent = BillingStore::open(&ledger_path, &network).unwrap();
    for quote in [&first, &second] {
        let saved = independent.quote(&quote.payload.id).unwrap();
        assert_eq!(saved.payload, quote.payload);
        assert_eq!(saved.signature, quote.signature);
        assert!(independent.account(&quote.payload.id).is_err());
    }
    assert!(!fixture.submitted.load(Ordering::Acquire));
    drop(independent);
    fixture.stop().await;
    let reopened = BillingStore::open(&ledger_path, &network).unwrap();
    for quote in [&first, &second] {
        let saved = reopened.quote(&quote.payload.id).unwrap();
        assert_eq!(saved.payload, quote.payload);
        assert_eq!(saved.signature, quote.signature);
        assert!(reopened.account(&quote.payload.id).is_err());
    }
}

#[tokio::test]
async fn a_public_free_quote_does_not_authorize_account_opening_or_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 2, "0", "0").await;
    let quote = fixture.propose("0").await;
    reject(fixture.post("/v1/contracts/open", &json!({
        "quote_id": quote.payload.id, "raw_transaction": null
    })).await, StatusCode::UNAUTHORIZED, "invalid_api_key").await;
    let path = format!("/v1/contracts/{}/credentials", quote.payload.id);
    reject(fixture.post(&path, &json!({
        "expires_at": protocol::now().unwrap() + 3600, "spend_cap_wei": null,
        "max_concurrency": 1, "scopes": ["read"]
    })).await, StatusCode::UNAUTHORIZED, "invalid_api_key").await;
    reject(fixture.client.get(format!("{}{path}", fixture.base)).send().await.unwrap(),
        StatusCode::UNAUTHORIZED, "unauthorized").await;
    assert!(fixture.state.ledger.account(&quote.payload.id).is_err());
    assert!(!fixture.submitted.load(Ordering::Acquire));
}

#[tokio::test]
async fn production_proposal_schema_and_canonical_money_are_not_normalized() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 3, "1", "2").await;
    let work_before = quote_work_evidence(&fixture.ledger_path);
    let original = fixture.request("100");
    let mut malformed = vec![
        b"{".to_vec(),
        serde_json::to_vec(&json!([
            original["offer_id"], original["consumer_wallet"], original["funding_wei"],
        ])).unwrap(),
    ];
    for field in ["offer_id", "consumer_wallet", "funding_wei"] {
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove(field);
        malformed.push(serde_json::to_vec(&missing).unwrap());
        let mut wrong_type = original.clone();
        wrong_type[field] = json!(100);
        malformed.push(serde_json::to_vec(&wrong_type).unwrap());
    }
    let mut extended = original.clone();
    extended["max_concurrency"] = json!(1);
    malformed.push(serde_json::to_vec(&extended).unwrap());
    malformed.push(format!(r#"{{"offer_id":"{}","consumer_wallet":"{PAYER}","funding_wei":"100","funding_wei":"100"}}"#,
        fixture.offer_id).into_bytes());
    for body in malformed {
        reject(fixture.raw(body).await, StatusCode::BAD_REQUEST, "invalid_request").await;
    }
    for funding in ["", "0100", "+100", "100.0", " 100", "340282366920938463463374607431768211456"] {
        reject(fixture.post(PROPOSE, &fixture.request(funding)).await,
            StatusCode::BAD_REQUEST, "invalid_amount").await;
    }
    assert_eq!(quote_work_evidence(&fixture.ledger_path), work_before,
        "malformed input must not spend or advance signing-work allowance");
    let db = rusqlite::Connection::open(&fixture.ledger_path).unwrap();
    let quotes: u64 = db.query_row("SELECT COUNT(*) FROM quotes", [], |row| row.get(0)).unwrap();
    assert_eq!(quotes, 0, "malformed input must not persist issued quotes");
    fixture.propose("100").await;
}

#[tokio::test]
async fn native_issuer_distinguishes_free_and_one_sided_paid_prices() {
    let directory = tempfile::tempdir().unwrap();
    for (case, input, output, accepted, rejected) in [
        (4, "1", "0", "100", ["0", "99"]),
        (5, "0", "2", "100", ["0", "99"]),
        (6, "0", "0", "0", ["1", "100"]),
    ] {
        let fixture = Fixture::new(directory.path(), case, input, output).await;
        for funding in rejected {
            reject(fixture.post(PROPOSE, &fixture.request(funding)).await,
                StatusCode::BAD_REQUEST, "invalid_funding").await;
        }
        let quote = fixture.propose(accepted).await;
        assert!(fixture.state.ledger.account(&quote.payload.id).is_err());
        assert!(!fixture.submitted.load(Ordering::Acquire));
    }
}

#[tokio::test]
async fn new_funding_gates_refuse_proposals_without_erasing_issued_terms() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 7, "1", "2").await;
    let issued = fixture.propose("100").await;
    for (accepting, funding_ready) in [(false, true), (true, false), (false, false)] {
        fixture.state.accepting.store(accepting, Ordering::Release);
        fixture.state.funding_ready.store(funding_ready, Ordering::Release);
        let response = fixture.post(PROPOSE, &fixture.request("100")).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = response.json().await.unwrap();
        let code = body["error"]["code"].as_str().unwrap();
        if accepting { assert_eq!(code, "settlement_unavailable"); }
        else if funding_ready { assert_eq!(code, "provider_draining"); }
        else { assert!(["settlement_unavailable", "provider_draining"].contains(&code)); }
        let saved = fixture.state.ledger.quote(&issued.payload.id).unwrap();
        assert_eq!(saved.payload, issued.payload);
        assert_eq!(saved.signature, issued.signature);
    }
    fixture.state.accepting.store(true, Ordering::Release);
    fixture.state.funding_ready.store(true, Ordering::Release);
    assert_ne!(fixture.propose("100").await.payload.id, issued.payload.id);
}

#[tokio::test]
async fn missing_foreign_and_superseded_offers_are_not_silently_substituted() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 8, "1", "2").await;
    let issued = fixture.propose("100").await;
    let mut request = fixture.request("100");
    request["offer_id"] = json!(format!("0x{}", "ff".repeat(32)));
    reject(fixture.post(PROPOSE, &request).await, StatusCode::NOT_FOUND, "offer_not_found").await;
    let foreign_key = format!("{:064x}", 2);
    let mut foreign = fixture.offer.payload.clone();
    foreign.provider_wallet = wallet::address_from_private_key(&foreign_key).unwrap();
    let foreign_id = fixture.state.offers.insert(protocol::sign(OFFER_DOMAIN, foreign, &foreign_key).unwrap()).unwrap();
    request["offer_id"] = json!(foreign_id);
    reject(fixture.post(PROPOSE, &request).await, StatusCode::BAD_REQUEST, "invalid_offer").await;
    let old_id = fixture.offer_id.clone();
    let mut refreshed = fixture.offer.payload.clone();
    refreshed.revision += 1;
    fixture.offer = protocol::sign(OFFER_DOMAIN, refreshed, &fixture.state.private_key).unwrap();
    fixture.offer_id = fixture.state.offers.insert(fixture.offer.clone()).unwrap();
    request["offer_id"] = json!(old_id);
    reject(fixture.post(PROPOSE, &request).await, StatusCode::NOT_FOUND, "offer_not_found").await;
    fixture.propose("100").await;
    let saved = fixture.state.ledger.quote(&issued.payload.id).unwrap();
    assert_eq!(saved.payload, issued.payload);
    assert_eq!(saved.signature, issued.signature);
}

#[tokio::test]
async fn proposal_body_limit_accepts_exactly_96_kib_and_rejects_one_more_byte() {
    const LIMIT: usize = 96 * 1024;
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 9, "1", "2").await;
    let mut body = serde_json::to_vec(&fixture.request("100")).unwrap();
    // JSON whitespace keeps both bodies syntactically valid, isolating the byte bound.
    body.resize(LIMIT, b' ');
    let response = fixture.raw(body.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let quote: Signed<Quote> = response.json().await.unwrap();
    protocol::verify(QUOTE_DOMAIN, &quote, &fixture.state.wallet).unwrap();
    assert_eq!(quote.payload.offer_id, fixture.offer_id);
    assert_eq!(quote.payload.funding_wei, "100");
    body.push(b' ');
    reject(fixture.raw(body).await, StatusCode::PAYLOAD_TOO_LARGE, "request_too_large").await;
    fixture.propose("100").await;
}

#[tokio::test]
async fn public_and_native_quotes_exceed_both_former_wallet_limits_without_reserving_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 30, "1", "2").await;
    let config = ConsumerConfig {
        network: fixture.state.config.network.clone(),
        data_dir: directory.path().join("consumer"),
        catalog_url: None, development: true, dht: None,
    };
    let consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, &format!("{:064x}", 50_030)).unwrap();
    consumer.wallet_import_file(&import).unwrap();
    let payer = consumer.address().unwrap();
    let alias = format!("0x{}", payer[2..].to_ascii_uppercase());
    assert_ne!(payer, alias);
    let minute_sample = protocol::now().unwrap() + 600;
    let prerequisite = fixture.commit_prerequisite(PAYER, "100", minute_sample);
    let mut accepted = vec![prerequisite];

    // Each request uses a separate anonymous client/connection, not owner proof or
    // a forwarded source header. Sixteen actual HTTP admissions name the same owner.
    for index in 0..16 {
        let caller = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10))
            .build().unwrap();
        let response = caller.post(format!("{}{PROPOSE}", fixture.base)).json(&json!({
            "offer_id": fixture.offer_id,
            "consumer_wallet": if index % 2 == 0 { &payer } else { &alias },
            "funding_wei": "100"
        })).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "public admission {}", index + 1);
        let signed: Signed<Quote> = response.json().await.unwrap();
        fixture.verify_quote(&signed, &payer, "100");
        accepted.push(signed);
    }

    // Every variant is requested after BOTH former wallet ceilings have been passed.
    // Signed development offers are prerequisites, not engine qualification evidence.
    for (model, input, output, funding) in [
        ("test/proposal", "0", "0", "0"),
        ("test/proposal", "1", "0", "101"),
        ("test/another-proposal", "0", "0", "0"),
        ("test/another-proposal", "0", "2", "102"),
    ] {
        let mut offer = fixture.offer.payload.clone();
        offer.model.id = model.into();
        offer.price.input_wei_per_1k = input.into();
        offer.price.output_wei_per_1k = output.into();
        offer.revision += 1;
        fixture.offer = protocol::sign(OFFER_DOMAIN, offer, &fixture.state.private_key).unwrap();
        fixture.offer_id = fixture.state.offers.insert(fixture.offer.clone()).unwrap();
        let caller = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10))
            .build().unwrap();
        let response = caller.post(format!("{}{PROPOSE}", fixture.base)).json(&json!({
            "offer_id": fixture.offer_id, "consumer_wallet": alias, "funding_wei": funding
        })).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let public: Signed<Quote> = response.json().await.unwrap();
        fixture.verify_quote(&public, &payer, funding);
        accepted.push(public);
        let native = consumer.quote(&fixture.offer, funding).await.unwrap();
        fixture.verify_quote(&native, &payer, funding);
        accepted.push(native);
    }

    let independent = BillingStore::open(&fixture.ledger_path,
        &fixture.state.config.network.identity).unwrap();
    for signed in &accepted {
        let saved = independent.quote(&signed.payload.id).unwrap();
        assert_eq!(saved.payload, signed.payload);
        assert_eq!(saved.signature, signed.signature);
        assert!(independent.account(&signed.payload.id).is_err());
        // Naming the payer, including through the native quote workflow, grants no
        // opening authority to a caller that lacks the owner's exact request proof.
        reject(fixture.post("/v1/contracts/open", &json!({
            "quote_id": signed.payload.id, "raw_transaction": null
        })).await, StatusCode::UNAUTHORIZED, "invalid_api_key").await;
    }
    let db = rusqlite::Connection::open(&fixture.ledger_path).unwrap();
    let count: usize = db.query_row("SELECT COUNT(*) FROM quotes", [], |row| row.get(0)).unwrap();
    assert_eq!(count, accepted.len());
    let buckets: Vec<(String, u64, usize)> = db.prepare(
        "SELECT bucket,window,count FROM rate_limits WHERE bucket LIKE 'quote:%' ORDER BY bucket"
    ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(buckets, vec![("quote:global".into(), minute_sample / 60, accepted.len())]);
    for table in ["funding_intents", "accounts", "credentials", "funding", "requests", "challenges"] {
        let count: u64 = db.query_row(&format!("SELECT COUNT(*) FROM {table}"),
            [], |row| row.get(0)).unwrap();
        assert_eq!(count, 0, "quoting must not reserve or authorize {table}");
    }
    assert!(!fixture.submitted.load(Ordering::Acquire));
}

#[tokio::test]
async fn public_global_committed_rate_refusal_preserves_quotes_and_an_opened_agreement() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 31, "0", "0").await;
    let config = ConsumerConfig {
        network: fixture.state.config.network.clone(),
        data_dir: directory.path().join("consumer"),
        catalog_url: None, development: true, dht: None,
    };
    let consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, &format!("{:064x}", 50_031)).unwrap();
    consumer.wallet_import_file(&import).unwrap();
    let original = consumer.quote(&fixture.offer, "0").await.unwrap();
    let opened = consumer.fund(&fixture.offer, &original).await.unwrap();
    let account = serde_json::to_value(&opened).unwrap();
    let credential = consumer.credential_export(&original.payload.id, None).unwrap();

    // These 599 complete signed quotes are real public-time ledger admissions,
    // NOT 599 live HTTP issuances. The later sample keeps route assertions away
    // from minute-boundary races and exercises the non-regressing committed clock.
    let minute_sample = protocol::now().unwrap() + 600;
    for index in 0..599 {
        fixture.commit_prerequisite(&format!("0x{:040x}", 100_000 + index), "0", minute_sample);
    }
    // Restore a stable, nearly exhausted independent work window. The 599 ledger
    // prerequisites above did not perform public-route signing work.
    restore_quote_work(&fixture.ledger_path, minute_sample / 60, 597);
    let last = fixture.propose("0").await; // Actual HTTP admission spends global unit 600.
    let db = rusqlite::Connection::open(&fixture.ledger_path).unwrap();
    let retained = || {
        db.prepare("SELECT record FROM quotes ORDER BY id").unwrap()
            .query_map([], |row| row.get::<_, String>(0)).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap()
    };
    let before = retained();
    assert_eq!(before.len(), 600);
    assert_eq!(quote_work_evidence(&fixture.ledger_path),
        ((minute_sample / 60).to_string(), minute_sample / 60, 598));
    for (index, payer) in [PAYER.to_string(), consumer.address().unwrap()].into_iter().enumerate() {
        let mut request = fixture.request("0");
        request["consumer_wallet"] = json!(payer);
        reject(fixture.post(PROPOSE, &request).await,
            StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
        assert!(retained() == before, "global refusal altered retained signed quotes");
        assert_eq!(quote_work_evidence(&fixture.ledger_path),
            ((minute_sample / 60).to_string(), minute_sample / 60, 599 + index as u64),
            "failed committed insertion must still spend its independent signing permit");
    }
    reject(fixture.post(PROPOSE, &fixture.request("0")).await,
        StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    assert_eq!(quote_work_evidence(&fixture.ledger_path),
        ((minute_sample / 60).to_string(), minute_sample / 60, 600));
    assert_eq!(retained(), before);
    let counter: (u64, u64) = db.query_row(
        "SELECT window,count FROM rate_limits WHERE bucket='quote:global'", [],
        |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(counter, (minute_sample / 60, 600));
    let watermark: String = db.query_row(
        "SELECT value FROM metadata WHERE key='quote_committed_minute'", [],
        |row| row.get(0)).unwrap();
    assert_eq!(watermark, (minute_sample / 60).to_string());
    for signed in [&original, &last] {
        let saved = fixture.state.ledger.quote(&signed.payload.id).unwrap();
        assert_eq!(saved.payload, signed.payload);
        assert_eq!(saved.signature, signed.signature);
    }
    let account_count: u64 = db.query_row("SELECT COUNT(*) FROM accounts", [],
        |row| row.get(0)).unwrap();
    assert_eq!(account_count, 1);
    let intents: u64 = db.query_row("SELECT COUNT(*) FROM funding_intents", [],
        |row| row.get(0)).unwrap();
    assert_eq!(intents, 0);
    assert!(fixture.state.ledger.account(&last.payload.id).is_err());
    let resumed = consumer.fund(&fixture.offer, &original).await.unwrap();
    assert_eq!(serde_json::to_value(resumed).unwrap(), account);
    assert!(consumer.credential_export(&original.payload.id, None).unwrap() == credential);
    assert!(fixture.state.ledger.funding_history(&original.payload.id).unwrap().is_empty());
    assert!(!fixture.submitted.load(Ordering::Acquire));
}

#[tokio::test]
async fn saved_signed_agreements_reject_conflicting_funding_calls_without_remote_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 40, "0", "0").await;
    let config = ConsumerConfig {
        network: fixture.state.config.network.clone(),
        data_dir: directory.path().join("consumer"),
        catalog_url: None, development: true, dht: None,
    };
    let mut consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, &format!("{:064x}", 50_040)).unwrap();
    consumer.wallet_import_file(&import).unwrap();
    let original = consumer.quote(&fixture.offer, "0").await.unwrap();
    let opened = consumer.fund(&fixture.offer, &original).await.unwrap();
    let id = &original.payload.id;
    assert_eq!(opened.contract_id, *id);
    assert_eq!(opened.status, "free");
    assert_eq!(opened.funded_wei, "0");
    let expected_account = serde_json::to_value(&opened).unwrap();
    let exported = consumer.credential_export(id, None).unwrap();
    // Inspect bytes produced by the native writer, never seed or edit the protected record.
    let saved_path = config.network.scoped_dir(&config.data_dir).join("consumer")
        .join(consumer.address().unwrap())
        .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
    let saved_bytes = std::fs::read(&saved_path).unwrap();
    let saved: Value = serde_json::from_slice(&saved_bytes).unwrap();
    assert_eq!(saved["offer"], serde_json::to_value(&fixture.offer).unwrap());
    assert_eq!(saved["quote"], serde_json::to_value(&original).unwrap());
    let other_provider_key = format!("{:064x}", 60_040);

    for restart in [false, true] {
        if restart {
            drop(consumer);
            consumer = Consumer::new(config.clone()).unwrap();
        }
        for change in ["nonce", "validity", "offer_revision", "model", "price",
            "fee_destination", "concurrency", "provider"] {
            let mut offer = fixture.offer.payload.clone();
            let mut quote = original.payload.clone();
            let key = if change == "provider" {
                other_provider_key.as_str()
            } else {
                fixture.state.private_key.as_str()
            };
            match change {
                "nonce" => {
                    let digit = if quote.nonce.as_bytes()[2] == b'0' { "1" } else { "0" };
                    quote.nonce.replace_range(2..3, digit);
                }
                "validity" => {
                    quote.issued_at -= 1;
                    quote.expires_at -= 1;
                }
                "offer_revision" => offer.revision += 1,
                "model" => {
                    offer.model.context_length = 8192;
                    quote.model = offer.model.clone();
                }
                "price" => {
                    offer.price.input_wei_per_1k = "1".into();
                    quote.price = offer.price.clone();
                    quote.funding_wei = "100".into();
                }
                "fee_destination" => {
                    offer.platform_wallet = format!("0x{}", "44".repeat(20));
                    quote.platform_wallet = offer.platform_wallet.clone();
                }
                "concurrency" => {
                    offer.max_concurrency = 1;
                    quote.max_concurrency = 1;
                }
                "provider" => {
                    offer.provider_wallet = wallet::address_from_private_key(key).unwrap();
                    quote.provider_wallet = offer.provider_wallet.clone();
                }
                _ => unreachable!(),
            }
            // Matching, validly signed replacement declarations must still not replace
            // an existing agreement. Keep its account ID and intended payer unchanged.
            quote.offer_id = protocol::offer_id(&offer).unwrap();
            let replacement_offer = protocol::sign(OFFER_DOMAIN, offer, key).unwrap();
            let replacement_quote = protocol::sign(QUOTE_DOMAIN, quote, key).unwrap();
            let requests = fixture.requests.load(Ordering::Acquire);
            assert!(consumer.fund(&replacement_offer, &replacement_quote).await.is_err(),
                "{change} must conflict with the saved agreement");
            assert_eq!(fixture.requests.load(Ordering::Acquire), requests,
                "{change} must be refused before contacting the provider or RPC");
            // Avoid dumping protected credentials if an equality assertion fails.
            assert!(std::fs::read(&saved_path).unwrap() == saved_bytes,
                "{change} altered the saved record");
            assert!(consumer.credential_export(id, None).unwrap() == exported,
                "{change} altered the exported credential");
        }
        // Ordinary resumption remains reachable and must not rotate the exported root.
        let resumed = consumer.fund(&fixture.offer, &original).await.unwrap();
        assert_eq!(serde_json::to_value(resumed).unwrap(), expected_account);
        assert!(consumer.credential_export(id, None).unwrap() == exported);
        assert!(std::fs::read(&saved_path).unwrap() == saved_bytes);
        let provider_saved = fixture.state.ledger.quote(id).unwrap();
        assert_eq!(provider_saved.payload, original.payload);
        assert_eq!(provider_saved.signature, original.signature);
        let authenticated = fixture.state.ledger.authenticate(
            exported["api_key"].as_str().unwrap(), protocol::now().unwrap()).unwrap();
        assert_eq!(authenticated.credential_id, exported["credential_id"].as_str().unwrap());
        assert_eq!(serde_json::to_value(authenticated.account).unwrap(), expected_account);
        assert!(fixture.state.ledger.funding_history(id).unwrap().is_empty());
        assert!(!fixture.submitted.load(Ordering::Acquire));
    }
    drop(consumer);
    fixture.stop().await;
}

#[tokio::test]
async fn historical_saved_agreements_recheck_authenticity_before_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 41, "0", "0").await;
    let config = ConsumerConfig {
        network: fixture.state.config.network.clone(),
        data_dir: directory.path().join("consumer"),
        catalog_url: None, development: true, dht: None,
    };
    let mut consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, &format!("{:064x}", 50_041)).unwrap();
    consumer.wallet_import_file(&import).unwrap();
    let original = consumer.quote(&fixture.offer, "0").await.unwrap();
    let account = consumer.fund(&fixture.offer, &original).await.unwrap();
    let expected_account = serde_json::to_value(&account).unwrap();
    let id = &original.payload.id;
    let exported = consumer.credential_export(id, None).unwrap();
    let saved_path = config.network.scoped_dir(&config.data_dir).join("consumer")
        .join(consumer.address().unwrap())
        .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
    let native_bytes = std::fs::read(&saved_path).unwrap();
    let mut record: Value = serde_json::from_slice(&native_bytes).unwrap();
    let provider_key = fixture.state.private_key.as_str();
    let other_key = format!("{:064x}", 60_041);
    let other_wallet = wallet::address_from_private_key(&other_key).unwrap();
    let now = protocol::now().unwrap();

    for expired in ["neither", "offer", "quote", "both"] {
        let mut offer = fixture.offer.payload.clone();
        let mut quote = original.payload.clone();
        // Controlled ageing of native-written fixture envelopes, not elapsed-time,
        // provider-retention or paid-settlement evidence. Keep the account/credential
        // from actual free onboarding; no invented transfer or acknowledgement.
        match expired {
            "offer" => {
                offer.issued_at = now - 3600;
                offer.expires_at = now - 60;
                quote.issued_at = now - 120;
                quote.expires_at = now + 180;
            }
            "quote" => {
                offer.issued_at = now - 1800;
                offer.expires_at = now + 1800;
                quote.issued_at = now - 1200;
                quote.expires_at = now - 900;
            }
            "both" => {
                offer.issued_at = now - 7200;
                offer.expires_at = now - 3600;
                quote.issued_at = now - 7000;
                quote.expires_at = now - 6700;
            }
            _ => {}
        }
        quote.offer_id = protocol::offer_id(&offer).unwrap();
        let accepted_offer = protocol::sign(OFFER_DOMAIN, offer, provider_key).unwrap();
        let accepted_quote = protocol::sign(QUOTE_DOMAIN, quote, provider_key).unwrap();
        record["offer"] = serde_json::to_value(&accepted_offer).unwrap();
        record["quote"] = serde_json::to_value(&accepted_quote).unwrap();
        let historical_bytes = serde_json::to_vec(&record).unwrap();
        std::fs::write(&saved_path, &historical_bytes).unwrap();
        drop(consumer);
        consumer = Consumer::new(config.clone()).unwrap();
        assert_eq!(serde_json::to_value(consumer.accounts().unwrap()).unwrap(),
            json!([expected_account.clone()]));
        assert!(consumer.credential_export(id, None).unwrap() == exported);

        if expired != "neither" {
            // A distinct ID has no saved agreement: the historical exception must
            // not leak into fresh onboarding, even for the same payer/provider.
            let mut fresh = accepted_quote.payload.clone();
            let digit = if fresh.id.as_bytes()[2] == b'0' { "1" } else { "0" };
            fresh.id.replace_range(2..3, digit);
            let fresh = protocol::sign(QUOTE_DOMAIN, fresh, provider_key).unwrap();
            let requests = fixture.requests.load(Ordering::Acquire);
            assert!(consumer.fund(&accepted_offer, &fresh).await.is_err(), "{expired}");
            assert_eq!(fixture.requests.load(Ordering::Acquire), requests);
            assert!(std::fs::read(&saved_path).unwrap() == historical_bytes);
            assert!(consumer.pending_accounts().unwrap()["pending"].as_array().unwrap().is_empty());
        }

        for change in ["offer_signer", "offer_domain", "quote_signer", "quote_domain",
            "chain", "genesis", "deployment", "account_id", "payer", "provider",
            "offer_commitment", "model", "price", "funding", "fee_destination",
            "fee_rate", "concurrency", "validity", "future_issuance", "origin"] {
            let mut offer = accepted_offer.payload.clone();
            let mut quote = accepted_quote.payload.clone();
            match change {
                "chain" => { offer.network.chain_id += 1; quote.network = offer.network.clone(); }
                "genesis" => {
                    offer.network.genesis_hash = format!("0x{}", "55".repeat(32));
                    quote.network = offer.network.clone();
                }
                "deployment" => {
                    offer.network.deployment_id.push_str("-other");
                    quote.network = offer.network.clone();
                }
                "account_id" => {
                    let digit = if quote.id.as_bytes()[2] == b'0' { "1" } else { "0" };
                    quote.id.replace_range(2..3, digit);
                }
                "payer" => quote.consumer_wallet = other_wallet.clone(),
                "provider" => quote.provider_wallet = other_wallet.clone(),
                "model" => quote.model.context_length += 1,
                "price" => quote.price.output_wei_per_1k = "1".into(),
                "funding" => quote.funding_wei = "100".into(),
                "fee_destination" => quote.platform_wallet = other_wallet.clone(),
                "fee_rate" => quote.fee_bps += 1,
                "concurrency" => quote.max_concurrency = 1,
                "validity" => quote.expires_at = quote.issued_at,
                "future_issuance" => {
                    quote.issued_at = now + 86_400;
                    quote.expires_at = quote.issued_at + 300;
                }
                "origin" => offer.endpoint.push_str("/unexpected"),
                _ => {}
            }
            quote.offer_id = protocol::offer_id(&offer).unwrap();
            if change == "offer_commitment" {
                let digit = if quote.offer_id.as_bytes()[2] == b'0' { "1" } else { "0" };
                quote.offer_id.replace_range(2..3, digit);
            }
            let offer = protocol::sign(
                if change == "offer_domain" { QUOTE_DOMAIN } else { OFFER_DOMAIN },
                offer, if change == "offer_signer" { &other_key } else { provider_key }).unwrap();
            let quote = protocol::sign(
                if change == "quote_domain" { OFFER_DOMAIN } else { QUOTE_DOMAIN },
                quote, if change == "quote_signer" { &other_key } else { provider_key }).unwrap();
            record["offer"] = serde_json::to_value(offer).unwrap();
            record["quote"] = serde_json::to_value(quote).unwrap();
            let invalid_bytes = serde_json::to_vec(&record).unwrap();
            std::fs::write(&saved_path, &invalid_bytes).unwrap();
            drop(consumer);
            consumer = Consumer::new(config.clone()).unwrap();
            let requests = fixture.requests.load(Ordering::Acquire);
            assert!(consumer.accounts().is_err(), "{expired}/{change}: account listing");
            assert!(consumer.credential_export(id, None).is_err(),
                "{expired}/{change}: credential export");
            assert!(consumer.resume(id).await.is_err(), "{expired}/{change}: recovery");
            assert_eq!(fixture.requests.load(Ordering::Acquire), requests,
                "{expired}/{change}: must reject before contacting provider or RPC");
            // A refused read/recovery must not erase or silently repair evidence.
            // Do not print protected bytes or credentials on assertion failure.
            assert!(std::fs::read(&saved_path).unwrap() == invalid_bytes,
                "{expired}/{change}: persisted evidence changed");
        }
        std::fs::write(&saved_path, &historical_bytes).unwrap();
        drop(consumer);
        consumer = Consumer::new(config.clone()).unwrap();
        assert_eq!(serde_json::to_value(consumer.resume(id).await.unwrap()).unwrap(), expected_account);
        assert!(consumer.credential_export(id, None).unwrap() == exported);
        let recovered: Value = serde_json::from_slice(&std::fs::read(&saved_path).unwrap()).unwrap();
        assert!(recovered == serde_json::from_slice::<Value>(&historical_bytes).unwrap());
    }
    std::fs::write(&saved_path, &native_bytes).unwrap();
    assert_eq!(serde_json::to_value(consumer.resume(id).await.unwrap()).unwrap(), expected_account);
    assert!(consumer.credential_export(id, None).unwrap() == exported);
    let stored = fixture.state.ledger.quote(id).unwrap();
    assert_eq!(stored.payload, original.payload);
    assert_eq!(stored.signature, original.signature);
    let authenticated = fixture.state.ledger.authenticate(
        exported["api_key"].as_str().unwrap(), protocol::now().unwrap()).unwrap();
    assert_eq!(serde_json::to_value(authenticated.account).unwrap(), expected_account);
    assert!(fixture.state.ledger.funding_history(id).unwrap().is_empty());
    assert!(!fixture.submitted.load(Ordering::Acquire));
    drop(consumer);
    fixture.stop().await;
}
