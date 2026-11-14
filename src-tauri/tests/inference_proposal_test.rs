//! Production contracts::router coverage for specify-inference-commercial-terms task 2.4.
//! Pre-admitted loopback development offers and real disk-backed ledgers; no engine,
//! chain simulation, actual funding, readiness-probe or performance-qualification claims.
use axum::Json;
use chiral_network::{
    app::ProviderState,
    billing_store::BillingStore,
    contracts,
    protocol::{self, InferenceOffer, Quote, Signed, OFFER_DOMAIN, QUOTE_DOMAIN},
    wallet,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

#[path = "support/proposal.rs"]
mod proposal;
use proposal::PreparedProvider;

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
        });
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10)).build().unwrap();
        Self { state, offer, offer_id, ledger_path, base, client, submitted,
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
        protocol::verify(QUOTE_DOMAIN, &signed, &self.state.wallet).unwrap();
        let q = &signed.payload;
        let o = &self.offer.payload;
        assert_eq!(q.network, o.network);
        assert_eq!(q.offer_id, self.offer_id);
        assert_eq!(q.provider_wallet, o.provider_wallet);
        assert_eq!(q.consumer_wallet, PAYER);
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
        signed
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
    let original = fixture.request("100");
    let mut malformed = vec![b"{".to_vec()];
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
