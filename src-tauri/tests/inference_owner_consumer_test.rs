//! Native owner-proof acceptance against the production control router on isolated loopback.
//! Adversarial middleware changes challenge replies or loses one committed opening response; no funds.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::Response,
    Router,
};
use chiral_network::{
    auth::{self, AuthPayload},
    billing_store::BillingStore,
    chain_client::ChainClient,
    consumer::{Consumer, ConsumerConfig},
    contracts,
    protocol::{self, Quote, Signed, AUTH_DOMAIN, QUOTE_DOMAIN},
    wallet,
};
use proposal::PreparedProvider;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{path::Path, sync::{Arc, Mutex}};
use tiny_keccak::{Hasher, Keccak};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

const CHALLENGE: &str = "/v1/auth/challenge";
const OPEN: &str = "/v1/contracts/open";

#[derive(Clone, Copy, Debug)]
enum Reply {
    Honest,
    LoseFirstOpeningResponse,
    UnsignedExtrasAndUppercaseHex,
    ChainId,
    Genesis,
    Deployment,
    Provider,
    Owner,
    Method,
    Path,
    Query,
    BodyHash,
    PayloadNonce,
    PayloadExpiry,
    ResponseNonce,
    ResponseExpiry,
    WrongDomain,
    ExtraSignedField,
    MissingHexPrefix,
    InvalidHex,
    OddHex,
    TruncatedBytes,
    TrailingBytes,
    MissingNonce,
    MistypedExpiry,
    EmptyNonce,
    OversizedNonce,
    Nonce { unit: &'static str, count: usize, suffix: &'static str },
    Expired,
    FarFuture,
}

fn change_hex(value: &mut String) {
    let replacement = if value.as_bytes()[2] == b'0' { "1" } else { "0" };
    value.replace_range(2..3, replacement);
}

fn transform(mode: Reply, mut payload: AuthPayload, mut reply: Value) -> Value {
    match mode {
        Reply::Honest | Reply::LoseFirstOpeningResponse => return reply,
        Reply::UnsignedExtrasAndUppercaseHex => {
            let encoded = reply["payload_hex"].as_str().unwrap();
            reply["payload_hex"] = json!(format!("0x{}", encoded[2..].to_ascii_uppercase()));
            // These unsigned additions must not replace the selected signing context.
            reply["network"] = json!({"chain_id": 999, "genesis_hash": "not-the-network"});
            reply["provider_wallet"] = json!("not-the-provider");
            reply["wallet"] = json!("not-the-owner");
            reply["path"] = json!("/v1/contracts/not-requested");
            return reply;
        }
        Reply::ChainId => payload.network.chain_id += 1,
        Reply::Genesis => change_hex(&mut payload.network.genesis_hash),
        Reply::Deployment => payload.network.deployment_id.push_str("-other"),
        Reply::Provider => change_hex(&mut payload.provider_wallet),
        Reply::Owner => change_hex(&mut payload.wallet),
        Reply::Method => payload.method = "DELETE".into(),
        Reply::Path => payload.path = "/v1/contracts/other".into(),
        Reply::Query => payload.path.push_str("?before=7"),
        Reply::BodyHash => change_hex(&mut payload.body_hash),
        Reply::PayloadNonce => change_hex(&mut payload.nonce),
        Reply::PayloadExpiry => payload.expires_at += 1,
        Reply::ResponseNonce => {
            let mut nonce = payload.nonce.clone();
            change_hex(&mut nonce);
            reply["nonce"] = json!(nonce);
        }
        Reply::ResponseExpiry => reply["expires_at"] = json!(payload.expires_at + 1),
        Reply::WrongDomain => {
            reply["payload_hex"] = json!(format!("0x{}", hex::encode(
                protocol::signing_payload(QUOTE_DOMAIN, &payload).unwrap())));
            return reply;
        }
        Reply::ExtraSignedField => {
            let mut signed = serde_json::to_value(payload).unwrap();
            signed["unexpected"] = json!(true);
            reply["payload_hex"] = json!(format!("0x{}", hex::encode(
                protocol::signing_payload(AUTH_DOMAIN, &signed).unwrap())));
            return reply;
        }
        Reply::MissingHexPrefix => {
            reply["payload_hex"] = json!(&reply["payload_hex"].as_str().unwrap()[2..]);
            return reply;
        }
        Reply::InvalidHex => {
            reply["payload_hex"] = json!("0xzz");
            return reply;
        }
        Reply::OddHex | Reply::TruncatedBytes => {
            let mut encoded = reply["payload_hex"].as_str().unwrap().to_owned();
            encoded.truncate(encoded.len() - if matches!(mode, Reply::OddHex) { 1 } else { 2 });
            reply["payload_hex"] = json!(encoded);
            return reply;
        }
        Reply::TrailingBytes => {
            reply["payload_hex"] = json!(format!("{}00", reply["payload_hex"].as_str().unwrap()));
            return reply;
        }
        Reply::MissingNonce => {
            reply.as_object_mut().unwrap().remove("nonce");
            return reply;
        }
        Reply::MistypedExpiry => {
            reply["expires_at"] = json!(payload.expires_at.to_string());
            return reply;
        }
        Reply::EmptyNonce | Reply::OversizedNonce => {
            payload.nonce = if matches!(mode, Reply::EmptyNonce) { String::new() } else { "a".repeat(257) };
            reply["nonce"] = json!(payload.nonce);
        }
        Reply::Nonce { unit, count, suffix } => {
            let mut nonce = String::with_capacity(unit.len() * count + suffix.len());
            for _ in 0..count { nonce.push_str(unit); }
            nonce.push_str(suffix);
            payload.nonce = nonce;
            reply["nonce"] = json!(payload.nonce);
        }
        Reply::Expired | Reply::FarFuture => {
            payload.expires_at = if matches!(mode, Reply::Expired) { 0 } else { u64::MAX };
            reply["expires_at"] = json!(payload.expires_at);
        }
    }
    // Nonce/lifetime rejection cases still supply exactly matching reconstructed bytes.
    // Only the receiver's validity guard, not an unrelated binding mismatch, can reject them.
    reply["payload_hex"] = json!(format!("0x{}", hex::encode(auth::challenge_payload(&payload).unwrap())));
    reply
}

#[derive(Debug)]
struct ControlRequest {
    method: String,
    target: String,
    body: Bytes,
    owner: Option<HeaderValue>,
    nonce: Option<HeaderValue>,
    signature: Option<HeaderValue>,
    status: Option<StatusCode>,
}

struct DeliveredChallenge {
    request: Value,
    reply: Value,
}

#[derive(Default)]
struct Observations {
    challenges: Vec<DeliveredChallenge>,
    controls: Vec<ControlRequest>,
    lost_opening: Option<protocol::OpenResult>,
}

struct Counterparty {
    mode: Reply,
    ledger: Arc<BillingStore>,
    observations: Arc<Mutex<Observations>>,
}

async fn intercept(State(peer): State<Arc<Counterparty>>, request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 96 * 1024).await.unwrap();
    let is_challenge = parts.uri.path() == CHALLENGE;
    let is_open = parts.uri.path() == OPEN;
    let challenge_request = is_challenge.then(|| serde_json::from_slice(&body).unwrap());
    let control_index = if !is_challenge && parts.uri.path() != "/v1/contracts/propose" {
        let mut observed = peer.observations.lock().unwrap();
        let index = observed.controls.len();
        observed.controls.push(ControlRequest {
            method: parts.method.to_string(),
            target: parts.uri.path_and_query().unwrap().as_str().into(),
            body: body.clone(),
            owner: parts.headers.get("x-chiral-owner").cloned(),
            nonce: parts.headers.get("x-chiral-nonce").cloned(),
            signature: parts.headers.get("x-chiral-signature").cloned(),
            status: None,
        });
        Some(index)
    } else { None };
    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
    if let Some(index) = control_index {
        peer.observations.lock().unwrap().controls[index].status = Some(response.status());
    }
    if is_open && response.status() == StatusCode::OK
        && matches!(peer.mode, Reply::LoseFirstOpeningResponse)
        && peer.observations.lock().unwrap().lost_opening.is_none() {
        // Run the real handler through its durable commit before losing its reply.
        // Retain the real secret only as an isolated authentication control, never
        // install it into the Consumer or fabricate a replacement account.
        let (mut parts, body) = response.into_parts();
        let opened = serde_json::from_slice(&to_bytes(body, 96 * 1024).await.unwrap()).unwrap();
        peer.observations.lock().unwrap().lost_opening = Some(opened);
        parts.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::from_stream(futures_util::stream::once(async {
            Err::<Bytes, _>(std::io::Error::new(std::io::ErrorKind::ConnectionReset,
                "isolated opening response loss"))
        })));
    }
    if !is_challenge { return response; }
    assert_eq!(response.status(), StatusCode::OK, "the real issuer must succeed before corruption");
    let (mut parts, body) = response.into_parts();
    let original: Value = serde_json::from_slice(&to_bytes(body, 96 * 1024).await.unwrap()).unwrap();
    let payload = peer.ledger.challenge(original["nonce"].as_str().unwrap()).unwrap();
    let reply = transform(peer.mode, payload, original);
    let bytes = serde_json::to_vec(&reply).unwrap();
    peer.observations.lock().unwrap().challenges.push(DeliveredChallenge {
        request: challenge_request.unwrap(), reply,
    });
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(bytes))
}

struct Fixture {
    consumer: Consumer,
    provider: PreparedProvider,
    quote: Signed<Quote>,
    observations: Arc<Mutex<Observations>>,
    shutdown: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
    _admin: TcpListener,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() { let _ = shutdown.send(()); }
        if let Some(server) = self.server.take() { server.abort(); }
    }
}

impl Fixture {
    async fn new(directory: &Path, case: u64, mode: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let admin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = PreparedProvider::new(directory, case, "0", "0",
            listener.local_addr().unwrap(), admin.local_addr().unwrap());
        let observations = Arc::new(Mutex::new(Observations::default()));
        let peer = Arc::new(Counterparty { mode, ledger: provider.state.ledger.clone(),
            observations: observations.clone() });
        let app = contracts::router(provider.state.clone()).layer(from_fn_with_state(peer, intercept));
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async { let _ = stopped.await; })
                .await.unwrap();
        });
        let config = ConsumerConfig { network: provider.state.config.network.clone(),
            data_dir: directory.join(format!("consumer-{case}")), catalog_url: None,
            development: true, dht: None };
        let consumer = Consumer::new(config.clone()).unwrap();
        // Known public scalars in owner-private temporary files; never funded or deployed keys.
        let import = config.data_dir.join("public-test-scalar.key");
        wallet::save_private_key(&import, &format!("{:064x}", 70_000 + case)).unwrap();
        consumer.wallet_import_file(&import).unwrap();
        let quote = consumer.quote(&provider.offer, "0").await.unwrap();
        assert_eq!(quote.payload.offer_id, provider.offer_id);
        Self { consumer, provider, quote, observations, shutdown: Some(shutdown),
            server: Some(server), _admin: admin }
    }

    async fn stop(mut self) {
        self.shutdown.take().unwrap().send(()).unwrap();
        self.server.take().unwrap().await.unwrap();
    }
}

async fn rejected(directory: &Path, case: u64, mode: Reply) {
    let fixture = Fixture::new(directory, case, mode).await;
    assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err(), "{mode:?}");
    {
        let observed = fixture.observations.lock().unwrap();
        // A transport/setup failure must not masquerade as rejection of the delivered challenge.
        assert_eq!(observed.challenges.len(), 1, "{mode:?}");
        let request = &observed.challenges[0].request;
        assert_eq!(request["wallet"], fixture.consumer.address().unwrap());
        assert_eq!(request["method"], "POST");
        assert_eq!(request["path"], OPEN);
        assert!(observed.controls.is_empty(), "{mode:?} leaked a control request: {:?}", observed.controls);
    }
    let persisted = BillingStore::open(&fixture.provider.ledger_path, &fixture.quote.payload.network).unwrap();
    assert!(persisted.account(&fixture.quote.payload.id).is_err(), "{mode:?} created an account");
    assert!(fixture.consumer.accounts().unwrap().is_empty(), "{mode:?} accepted a local account");
    fixture.stop().await;
}

#[tokio::test]
async fn native_rejects_substituted_signing_context_without_transmitting_owner_proofs() {
    let directory = tempfile::tempdir().unwrap();
    for (index, mode) in [Reply::ChainId, Reply::Genesis, Reply::Deployment, Reply::Provider,
        Reply::Owner, Reply::Method, Reply::Path, Reply::Query, Reply::BodyHash,
        Reply::PayloadNonce, Reply::PayloadExpiry, Reply::ResponseNonce, Reply::ResponseExpiry,
        Reply::WrongDomain, Reply::ExtraSignedField].into_iter().enumerate() {
        rejected(directory.path(), 100 + index as u64, mode).await;
    }
}

#[tokio::test]
async fn native_rejects_malformed_challenge_encodings_without_transmitting_owner_proofs() {
    let directory = tempfile::tempdir().unwrap();
    for (index, mode) in [Reply::MissingHexPrefix, Reply::InvalidHex, Reply::OddHex,
        Reply::TruncatedBytes, Reply::TrailingBytes, Reply::MissingNonce, Reply::MistypedExpiry]
        .into_iter().enumerate() {
        rejected(directory.path(), 200 + index as u64, mode).await;
    }
}

#[tokio::test]
async fn native_rejects_invalid_nonce_and_expiry_even_when_signing_bytes_match() {
    let directory = tempfile::tempdir().unwrap();
    // Timeless rejection values avoid sleeps or a race against the private receiver clock.
    for (index, mode) in [Reply::EmptyNonce, Reply::OversizedNonce, Reply::Expired, Reply::FarFuture]
        .into_iter().enumerate() {
        rejected(directory.path(), 300 + index as u64, mode).await;
    }
}

#[tokio::test]
async fn native_nonce_byte_boundaries_preserve_proofs_without_granting_provider_authority() {
    let directory = tempfile::tempdir().unwrap();
    for (index, (mode, bytes)) in [
        (Reply::Nonce { unit: "a", count: 1, suffix: "" }, 1),
        (Reply::Nonce { unit: "a", count: 255, suffix: "" }, 255),
        (Reply::Nonce { unit: "a", count: 256, suffix: "" }, 256),
        (Reply::Nonce { unit: "é", count: 127, suffix: "a" }, 255),
        (Reply::Nonce { unit: "é", count: 128, suffix: "" }, 256),
        (Reply::Nonce { unit: "界", count: 85, suffix: "a" }, 256),
    ].into_iter().enumerate() {
        let fixture = Fixture::new(directory.path(), 500 + index as u64, mode).await;
        let owner = fixture.consumer.address().unwrap();
        // Receiver acceptance is not provider authority: these substituted nonces were not issued.
        assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err());
        {
            let observed = fixture.observations.lock().unwrap();
            assert_eq!(observed.challenges.len(), 1, "{mode:?}");
            assert_eq!(observed.controls.len(), 1, "{mode:?} must pass native nonce validation");
            let challenge = &observed.challenges[0];
            let control = &observed.controls[0];
            let nonce = challenge.reply["nonce"].as_str().unwrap();
            assert_eq!(nonce.len(), bytes);
            assert_eq!(control.method, "POST");
            assert_eq!(control.target, OPEN);
            assert_eq!(control.owner.as_ref().unwrap().to_str().unwrap(), owner);
            // HeaderValue::to_str rejects non-ASCII; compare the actual transported UTF-8 bytes.
            assert_eq!(control.nonce.as_ref().unwrap().as_bytes(), nonce.as_bytes());
            assert_eq!(serde_json::from_slice::<Value>(&control.body).unwrap(),
                json!({"quote_id": fixture.quote.payload.id, "raw_transaction": null}));
            let digest = protocol::hash_hex(&control.body);
            assert_eq!(challenge.request, json!({"wallet": owner, "method": "POST",
                "path": OPEN, "body_hash": digest}));
            let expected = AuthPayload {
                network: fixture.quote.payload.network.clone(),
                provider_wallet: fixture.quote.payload.provider_wallet.clone(),
                wallet: owner.clone(), method: "POST".into(), path: OPEN.into(),
                body_hash: digest, nonce: nonce.into(),
                expires_at: challenge.reply["expires_at"].as_u64().unwrap(),
            };
            let signature = control.signature.as_ref().unwrap().to_str().unwrap();
            assert_eq!(wallet::recover_signer(&auth::challenge_payload(&expected).unwrap(), signature)
                .unwrap(), owner);
            assert_eq!(control.status, Some(StatusCode::UNAUTHORIZED));
        }
        let persisted = BillingStore::open(&fixture.provider.ledger_path, &fixture.quote.payload.network).unwrap();
        assert!(persisted.account(&fixture.quote.payload.id).is_err());
        assert!(fixture.consumer.accounts().unwrap().is_empty());
        fixture.stop().await;
    }
}

#[tokio::test]
async fn native_nonce_limit_counts_utf8_bytes_rather_than_characters() {
    let directory = tempfile::tempdir().unwrap();
    // Each nonce is 257 bytes but far fewer than 256 characters. A character-count guard leaks proof.
    for (index, mode) in [
        Reply::Nonce { unit: "é", count: 128, suffix: "a" },
        Reply::Nonce { unit: "界", count: 85, suffix: "aa" },
    ].into_iter().enumerate() {
        rejected(directory.path(), 600 + index as u64, mode).await;
    }
}

#[tokio::test]
async fn native_owner_proofs_bind_transmitted_post_get_and_delete_operations() {
    let directory = tempfile::tempdir().unwrap();
    for (index, mode) in [Reply::Honest, Reply::UnsignedExtrasAndUppercaseHex].into_iter().enumerate() {
        let fixture = Fixture::new(directory.path(), 400 + index as u64, mode).await;
        let id = &fixture.quote.payload.id;
        let owner = fixture.consumer.address().unwrap();
        let opened = fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.unwrap();
        assert_eq!(opened.contract_id, *id);
        assert_eq!(opened.status, "free");
        assert_eq!(opened.funded_wei, "0");
        let read = fixture.consumer.account(id).await.unwrap();
        assert_eq!(read.contract_id, *id);
        assert_eq!(read.consumer_wallet, owner);
        let credential = fixture.provider.state.ledger.credentials(id).unwrap().into_iter()
            .find(|credential| credential.root && !credential.revoked).unwrap();
        let revoked = fixture.consumer.credential_revoke(id, &credential.id).await.unwrap();
        assert_eq!(revoked, json!({"revoked": true}));
        assert!(fixture.provider.state.ledger.credentials(id).unwrap().into_iter()
            .find(|current| current.id == credential.id).unwrap().revoked);
        {
            let observed = fixture.observations.lock().unwrap();
            assert_eq!(observed.challenges.len(), 3);
            assert_eq!(observed.controls.len(), 3);
            let targets = [("POST", OPEN.to_owned()), ("GET", format!("/v1/contracts/{id}")),
                ("DELETE", format!("/v1/contracts/{id}/credentials/{}", credential.id))];
            for ((control, challenge), (method, target)) in observed.controls.iter()
                .zip(&observed.challenges).zip(targets) {
                assert_eq!(control.method, method);
                assert_eq!(control.target, target);
                if method == "POST" {
                    assert_eq!(serde_json::from_slice::<Value>(&control.body).unwrap(),
                        json!({"quote_id": id, "raw_transaction": null}));
                } else {
                    assert!(control.body.is_empty(), "{method} must send zero bytes, not JSON");
                }
                let digest = protocol::hash_hex(&control.body);
                assert_eq!(challenge.request, json!({"wallet": owner, "method": method,
                    "path": target, "body_hash": digest}));
                let expected = AuthPayload { network: fixture.quote.payload.network.clone(),
                    provider_wallet: fixture.quote.payload.provider_wallet.clone(), wallet: owner.clone(),
                    method: method.into(), path: target, body_hash: digest,
                    nonce: challenge.reply["nonce"].as_str().unwrap().into(),
                    expires_at: challenge.reply["expires_at"].as_u64().unwrap() };
                assert_eq!(control.owner.as_ref().unwrap().to_str().unwrap(), owner);
                assert_eq!(control.nonce.as_ref().unwrap().to_str().unwrap(), expected.nonce);
                let signature = control.signature.as_ref().unwrap().to_str().unwrap();
                assert_eq!(wallet::recover_signer(&auth::challenge_payload(&expected).unwrap(), signature)
                    .unwrap(), owner);
            }
        }
        fixture.stop().await;
    }
}

fn assert_free_control_exchange(fixture: &Fixture, quote: &Signed<Quote>, method: &str, status: StatusCode) {
    let observed = fixture.observations.lock().unwrap();
    assert_eq!(observed.challenges.len(), 1, "one fresh owner proof per explicit operation");
    assert_eq!(observed.controls.len(), 1, "no intent, RPC, engine call or automatic control retry");
    let challenge = &observed.challenges[0];
    let control = &observed.controls[0];
    let target = if method == "POST" { OPEN.to_owned() }
        else { assert_eq!(method, "GET"); format!("/v1/contracts/{}", quote.payload.id) };
    assert_eq!(control.method, method);
    assert_eq!(control.target, target);
    assert_eq!(control.status, Some(status));
    if method == "POST" {
        assert_eq!(serde_json::from_slice::<Value>(&control.body).unwrap(),
            json!({"quote_id":quote.payload.id,"raw_transaction":null}));
    } else {
        assert!(control.body.is_empty());
    }
    assert_eq!(challenge.request, json!({"wallet":quote.payload.consumer_wallet,"method":method,
        "path":target,"body_hash":protocol::hash_hex(&control.body)}));
    assert_eq!(control.owner.as_ref().unwrap().to_str().unwrap(), quote.payload.consumer_wallet);
    let nonce = challenge.reply["nonce"].as_str().unwrap();
    assert_eq!(control.nonce.as_ref().unwrap().to_str().unwrap(), nonce);
    assert!(fixture.provider.state.ledger.challenge(nonce).is_err(),
        "successful and capacity-refused openings consume their owner authority");
}

fn assert_no_free_payment(saved: &Value) {
    for field in ["raw_transaction", "tx_hash", "funding_intent", "funding_cancellation", "topup"] {
        assert_eq!(saved.get(field), Some(&Value::Null), "{field}");
    }
    assert_eq!(saved["funding_broadcast_started"], false);
    assert_eq!(saved["funding_cancellation_requested"], false);
}

#[tokio::test]
async fn native_free_opening_and_full_refusal_never_prepare_or_submit_payment() {
    let directory = tempfile::tempdir().unwrap();
    // Separate fixtures keep each case within the existing twelve-challenge
    // wallet window; no clock manipulation or synthetic account admission.
    for (index, refresh_with_resume) in [true, false].into_iter().enumerate() {
        let case = 700 + index as u64;
        let mut fixture = Fixture::new(directory.path(), case, Reply::Honest).await;
        assert!(fixture.observations.lock().unwrap().controls.is_empty());
        let client_calls = Arc::new(Mutex::new(Vec::new()));
        let observed = client_calls.clone();
        let rpc_app = Router::new().fallback(move |request: Request| {
            let observed = observed.clone();
            async move {
                observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
                StatusCode::SERVICE_UNAVAILABLE
            }
        });
        let rpc_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut network = fixture.provider.state.config.network.clone();
        network.rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
        let rpc_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });
        let config = ConsumerConfig { network, data_dir: directory.path().join(format!("consumer-{case}")),
            catalog_url: None, development: true, dht: None };
        // Preserve the imported wallet and its base journal directory; only isolate
        // native chain access from provider RPC/engine access at the control endpoint.
        fixture.consumer = Consumer::new(config.clone()).unwrap();
        let owner = fixture.consumer.address().unwrap();
        assert_eq!(owner, fixture.quote.payload.consumer_wallet);
        let scoped = config.network.scoped_dir(&config.data_dir);
        let account_path = |id: &str| scoped.join("consumer").join(&owner)
            .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
        let client_journal = Connection::open(scoped.join("wallet-journal/transactions.sqlite3")).unwrap();
        let provider_journal = Connection::open(fixture.provider.state.config.network
            .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let no_payments = || {
            assert!(client_calls.lock().unwrap().is_empty(), "free native workflows must not reach chain RPC");
            for journal in [&client_journal, &provider_journal] {
                let rows: (u64, u64) = journal.query_row(
                    "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)", [],
                    |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
                assert_eq!(rows, (0, 0), "neither signing wallet prepares an original or cancellation");
            }
            let rows: (u64, u64, u64, u64, u64) = db.query_row(
                "SELECT (SELECT COUNT(*) FROM funding_intents),(SELECT COUNT(*) FROM funding_attempts),
                        (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations),
                        (SELECT COUNT(*) FROM fee_batches)", [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
            assert_eq!(rows, (0, 0, 0, 0, 0));
        };
        no_payments();
        let mut accepted = Vec::with_capacity(8);
        for opening in 0..8 {
            {
                let mut observed = fixture.observations.lock().unwrap();
                observed.challenges.clear();
                observed.controls.clear();
            }
            let quote = if opening == 0 { fixture.quote.clone() }
                else { fixture.consumer.quote(&fixture.provider.offer, "0").await.unwrap() };
            let account = fixture.consumer.fund(&fixture.provider.offer, &quote).await.unwrap();
            assert_free_control_exchange(&fixture, &quote, "POST", StatusCode::OK);
            assert_eq!(account.contract_id, quote.payload.id);
            assert_eq!(account.status, "free");
            for amount in [&account.funded_wei, &account.fee_wei, &account.spent_wei,
                &account.reserved_wei, &account.available_wei, &account.reversed_wei,
                &account.provider_loss_wei, &account.waived_wei] {
                assert_eq!(amount, "0");
            }
            let exported = fixture.consumer.credential_export(&quote.payload.id, None).unwrap();
            let bytes = std::fs::read(account_path(&quote.payload.id)).unwrap();
            let saved: Value = serde_json::from_slice(&bytes).unwrap();
            assert_no_free_payment(&saved);
            assert_eq!(saved["offer"], serde_json::to_value(&fixture.provider.offer).unwrap());
            assert_eq!(saved["quote"], serde_json::to_value(&quote).unwrap());
            assert_eq!(saved["account"], serde_json::to_value(&account).unwrap());
            assert_eq!(saved["active_credential"], exported["credential_id"]);
            assert_eq!(saved["credentials"].as_array().unwrap().len(), 1);
            assert_eq!(saved["credentials"][0]["bearer"], exported["api_key"]);
            accepted.push((quote, account, exported, bytes));
            no_payments();
        }
        let assert_accepted = |consumer: &Consumer, ledger: &BillingStore| {
            assert_eq!(consumer.accounts().unwrap().len(), 8);
            for (quote, account, exported, bytes) in &accepted {
                assert_eq!(std::fs::read(account_path(&quote.payload.id)).unwrap(), *bytes);
                assert_eq!(consumer.credential_export(&quote.payload.id, None).unwrap(), *exported);
                let context = ledger.authenticate(exported["api_key"].as_str().unwrap(), protocol::now().unwrap()).unwrap();
                assert_eq!(context.credential_id, exported["credential_id"].as_str().unwrap());
                assert_eq!(serde_json::to_value(context.account).unwrap(), serde_json::to_value(account).unwrap());
            }
        };
        assert_accepted(&fixture.consumer, &fixture.provider.state.ledger);
        assert_eq!(fixture.consumer.pending_accounts().unwrap(), json!({"pending":[]}));
        {
            let mut observed = fixture.observations.lock().unwrap();
            observed.challenges.clear();
            observed.controls.clear();
        }
        // A public quote can still be issued with all eight owned account slots full.
        // Its old named-wallet quote budgets remain unexhausted, not mitigated here.
        let refused = fixture.consumer.quote(&fixture.provider.offer, "0").await.unwrap();
        assert!(fixture.observations.lock().unwrap().controls.is_empty());
        assert!(fixture.observations.lock().unwrap().challenges.is_empty());
        assert!(!account_path(&refused.payload.id).exists());
        let records = || [
            "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts ORDER BY id",
            "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
            "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        ].map(|query| {
            let mut hash = Keccak::v256();
            let mut count = 0_u64;
            let mut statement = db.prepare(query).unwrap();
            let mut rows = statement.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                hash.update(row.get::<_, String>(0).unwrap().as_bytes());
                count += 1;
            }
            let mut digest = [0; 32];
            hash.finalize(&mut digest);
            (count, digest)
        });
        let full_records = records();
        assert_eq!(full_records.map(|(count, _)| count), [8, 8, 1]);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM accounts WHERE owner=?1", [&owner],
            |row| row.get::<_, u64>(0)).unwrap(), 8);
        let mut pending_before = None;
        for (attempt, resume) in [false, true, false].into_iter().enumerate() {
            if attempt == 1 {
                fixture.shutdown.take().unwrap().send(()).unwrap();
                fixture.server.take().unwrap().await.unwrap();
                {
                    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
                    state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
                }
                let listener = TcpListener::bind(fixture.provider.offer.payload.endpoint.strip_prefix("http://").unwrap())
                    .await.unwrap();
                let peer = Arc::new(Counterparty { mode: Reply::Honest, ledger: fixture.provider.state.ledger.clone(),
                    observations: fixture.observations.clone() });
                let app = contracts::router(fixture.provider.state.clone()).layer(from_fn_with_state(peer, intercept));
                let (shutdown, stopped) = oneshot::channel();
                fixture.shutdown = Some(shutdown);
                fixture.server = Some(tokio::spawn(async move {
                    axum::serve(listener, app).with_graceful_shutdown(async { let _ = stopped.await; })
                        .await.unwrap();
                }));
            }
            if attempt > 0 { fixture.consumer = Consumer::new(config.clone()).unwrap(); }
            {
                let mut observed = fixture.observations.lock().unwrap();
                observed.challenges.clear();
                observed.controls.clear();
            }
            let error = if resume { fixture.consumer.resume(&refused.payload.id).await.unwrap_err() }
                else { fixture.consumer.fund(&fixture.provider.offer, &refused).await.unwrap_err() };
            assert!(error.starts_with("rate_limit:") && error.ends_with("(HTTP 429)"), "{error}");
            assert_free_control_exchange(&fixture, &refused, "POST", StatusCode::TOO_MANY_REQUESTS);
            let bytes = std::fs::read(account_path(&refused.payload.id)).unwrap();
            let saved: Value = serde_json::from_slice(&bytes).unwrap();
            assert_no_free_payment(&saved);
            assert_eq!(saved["offer"], serde_json::to_value(&fixture.provider.offer).unwrap());
            assert_eq!(saved["quote"], serde_json::to_value(&refused).unwrap());
            assert!(saved["account"].is_null() && saved["active_credential"].is_null());
            assert_eq!(saved["credentials"], json!([]));
            if let Some(before) = &pending_before { assert_eq!(&bytes, before); }
            else { pending_before = Some(bytes); }
            let pending = fixture.consumer.pending_accounts().unwrap();
            assert_eq!(pending["pending"].as_array().unwrap().len(), 1);
            assert_eq!(pending["pending"][0]["contract_id"], refused.payload.id);
            assert_eq!(pending["pending"][0]["funding_stage"], "free_open_pending");
            assert_eq!(pending["pending"][0]["funding_wei"], "0");
            assert!(pending["pending"][0]["tx_hash"].is_null());
            assert!(pending["pending"][0]["funding_intent_registered_at"].is_null());
            assert!(fixture.provider.state.ledger.account(&refused.payload.id).unwrap_err().starts_with("not_found:"));
            assert!(fixture.provider.state.ledger.credentials(&refused.payload.id).unwrap().is_empty());
            assert_eq!(records(), full_records);
            assert_accepted(&fixture.consumer, &fixture.provider.state.ledger);
            no_payments();
        }

        // Saved accepted accounts take the GET refresh path at full capacity;
        // genuinely lost first responses are exercised separately below.
        fixture.consumer = Consumer::new(config.clone()).unwrap();
        {
            let mut observed = fixture.observations.lock().unwrap();
            observed.challenges.clear();
            observed.controls.clear();
        }
        let (quote, account, _, _) = &accepted[0];
        let refreshed = if refresh_with_resume { fixture.consumer.resume(&quote.payload.id).await.unwrap() }
            else { fixture.consumer.fund(&fixture.provider.offer, quote).await.unwrap() };
        assert_free_control_exchange(&fixture, quote, "GET", StatusCode::OK);
        assert_eq!(serde_json::to_value(refreshed).unwrap(), serde_json::to_value(account).unwrap());
        assert_accepted(&fixture.consumer, &fixture.provider.state.ledger);
        assert_eq!(std::fs::read(account_path(&refused.payload.id)).unwrap(), pending_before.unwrap());
        assert_eq!(records(), full_records);
        no_payments();
        fixture.stop().await;
        rpc_server.abort();
        assert!(rpc_server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn native_lost_free_opening_recovers_without_issuing_or_rotating_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let case = 800;
    let mut fixture = Fixture::new(directory.path(), case, Reply::LoseFirstOpeningResponse).await;
    let client_calls = Arc::new(Mutex::new(Vec::new()));
    let observed = client_calls.clone();
    let rpc_app = Router::new().fallback(move |request: Request| {
        let observed = observed.clone();
        async move {
            observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
            StatusCode::SERVICE_UNAVAILABLE
        }
    });
    let rpc_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut network = fixture.provider.state.config.network.clone();
    network.rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
    let rpc_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });
    let config = ConsumerConfig { network, data_dir: directory.path().join(format!("consumer-{case}")),
        catalog_url: None, development: true, dht: None };
    fixture.consumer = Consumer::new(config.clone()).unwrap();
    let id = fixture.quote.payload.id.clone();
    let owner = fixture.consumer.address().unwrap();
    let scoped = config.network.scoped_dir(&config.data_dir);
    let account_path = scoped.join("consumer").join(&owner)
        .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
    let saved = || -> Value {
        serde_json::from_slice(&std::fs::read(&account_path).unwrap()).unwrap()
    };
    let client_journal = Connection::open(scoped.join("wallet-journal/transactions.sqlite3")).unwrap();
    let provider_journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let db = Connection::open(&fixture.provider.ledger_path).unwrap();
    let no_payments = || {
        assert!(client_calls.lock().unwrap().is_empty(), "free recovery must not reach chain RPC");
        for journal in [&client_journal, &provider_journal] {
            let rows: (u64, u64) = journal.query_row(
                "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)", [],
                |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
            assert_eq!(rows, (0, 0));
        }
        let rows: (u64, u64, u64, u64, u64) = db.query_row(
            "SELECT (SELECT COUNT(*) FROM funding_intents),(SELECT COUNT(*) FROM funding_attempts),
                    (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations),
                    (SELECT COUNT(*) FROM fee_batches)", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
        assert_eq!(rows, (0, 0, 0, 0, 0));
        assert_no_free_payment(&saved());
    };
    let clear_controls = || {
        let mut observed = fixture.observations.lock().unwrap();
        observed.challenges.clear();
        observed.controls.clear();
    };

    // Native sees an actual transport/body failure, not a forged failure status
    // or a manufactured pending/account record.
    assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err());
    assert_free_control_exchange(&fixture, &fixture.quote, "POST", StatusCode::OK);
    let opening = fixture.observations.lock().unwrap().lost_opening.clone().unwrap();
    let original = opening.credential.as_ref().unwrap();
    let account = serde_json::to_value(&opening.account).unwrap();
    assert_eq!(opening.account.contract_id, id);
    assert_eq!(opening.account.consumer_wallet, owner);
    assert_eq!(opening.account.status, "free");
    for amount in [&opening.account.funded_wei, &opening.account.fee_wei,
        &opening.account.spent_wei, &opening.account.reserved_wei, &opening.account.available_wei,
        &opening.account.reversed_wei, &opening.account.provider_loss_wei, &opening.account.waived_wei] {
        assert_eq!(amount, "0");
    }
    assert_eq!(serde_json::to_value(fixture.provider.state.ledger.account(&id).unwrap()).unwrap(), account);
    let root_record = || -> (String, String, String, bool) {
        db.query_row("SELECT id,token_hash,record,revoked FROM credentials WHERE id=?1",
            [&original.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap()
    };
    let original_record = root_record();
    let assert_original = |ledger: &BillingStore, expected_count: usize| {
        assert!(root_record() == original_record, "original root and token hash must remain unchanged");
        let credentials = ledger.credentials(&id).unwrap();
        assert_eq!(credentials.len(), expected_count);
        let root = credentials.iter().find(|credential| credential.id == original.id).unwrap();
        assert!(root.root && !root.revoked);
        assert_eq!(credentials.iter().filter(|credential| credential.root).count(), 1);
        let context = ledger.authenticate(&original.bearer, protocol::now().unwrap()).unwrap();
        assert_eq!(context.credential_id, original.id);
        assert_eq!(serde_json::to_value(context.account).unwrap(), account);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get::<_, u64>(0)).unwrap(), 1);
    };
    assert_original(&fixture.provider.state.ledger, 1);
    let pending_saved = saved();
    assert_eq!(pending_saved["offer"], serde_json::to_value(&fixture.provider.offer).unwrap());
    assert_eq!(pending_saved["quote"], serde_json::to_value(&fixture.quote).unwrap());
    assert!(pending_saved["account"].is_null() && pending_saved["active_credential"].is_null());
    assert_eq!(pending_saved["credentials"], json!([]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&account_path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(fixture.consumer.accounts().unwrap().is_empty());
    let pending = fixture.consumer.pending_accounts().unwrap();
    assert_eq!(pending["pending"].as_array().unwrap().len(), 1);
    assert_eq!(pending["pending"][0]["contract_id"], id);
    assert_eq!(pending["pending"][0]["funding_stage"], "free_open_pending");
    assert_eq!(pending["pending"][0]["funding_wei"], "0");
    assert!(pending["pending"][0]["tx_hash"].is_null());
    assert!(pending["pending"][0]["funding_intent_registered_at"].is_null());
    assert!(fixture.consumer.credential_export(&id, None).is_err());
    no_payments();

    // Reopen both durable stores and the production provider router at the same
    // reserved endpoint. No sleep, clock change, synthetic account or new quote.
    fixture.shutdown.take().unwrap().send(()).unwrap();
    fixture.server.take().unwrap().await.unwrap();
    {
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
    }
    let listener = TcpListener::bind(fixture.provider.offer.payload.endpoint.strip_prefix("http://").unwrap())
        .await.unwrap();
    let peer = Arc::new(Counterparty { mode: Reply::Honest, ledger: fixture.provider.state.ledger.clone(),
        observations: fixture.observations.clone() });
    let app = contracts::router(fixture.provider.state.clone()).layer(from_fn_with_state(peer, intercept));
    let (shutdown, stopped) = oneshot::channel();
    fixture.shutdown = Some(shutdown);
    fixture.server = Some(tokio::spawn(async move {
        axum::serve(listener, app).with_graceful_shutdown(async { let _ = stopped.await; })
            .await.unwrap();
    }));
    assert_original(&fixture.provider.state.ledger, 1);
    for resume in [true, false] {
        fixture.consumer = Consumer::new(config.clone()).unwrap();
        clear_controls();
        let recovered = if resume { fixture.consumer.resume(&id).await.unwrap() }
            else { fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.unwrap() };
        assert_free_control_exchange(&fixture, &fixture.quote, if resume { "POST" } else { "GET" }, StatusCode::OK);
        assert_eq!(serde_json::to_value(recovered).unwrap(), account);
        assert_eq!(saved()["account"], account);
        assert!(saved()["active_credential"].is_null());
        assert_eq!(saved()["credentials"], json!([]));
        assert_eq!(fixture.consumer.accounts().unwrap().len(), 1);
        assert_eq!(fixture.consumer.pending_accounts().unwrap(), json!({"pending":[]}));
        assert!(fixture.consumer.credential_export(&id, None).is_err(),
            "recovered account must not pretend the undisclosed root is locally available");
        assert_original(&fixture.provider.state.ledger, 1);
        no_payments();
    }

    clear_controls();
    let issued = fixture.consumer.credential_issue(&id, json!({
        "expires_at": protocol::now().unwrap() + 600, "spend_cap_wei": null,
        "max_concurrency": 1, "scopes": ["infer", "read"]
    })).await.unwrap();
    assert!(issued.get("bearer").is_none() && issued.get("api_key").is_none());
    {
        let observed = fixture.observations.lock().unwrap();
        assert_eq!(observed.challenges.len(), 2);
        assert_eq!(observed.controls.len(), 2);
        for (control, method) in observed.controls.iter().zip(["GET", "POST"]) {
            assert_eq!(control.method, method);
            assert_eq!(control.target, format!("/v1/contracts/{id}/credentials"));
            assert_eq!(control.status, Some(StatusCode::OK));
        }
    }
    let exported = fixture.consumer.credential_export(&id, None).unwrap();
    assert_eq!(exported["credential_id"], issued["id"]);
    assert_ne!(exported["credential_id"], original.id);
    assert!(exported["api_key"].as_str().unwrap() != original.bearer);
    let context = fixture.provider.state.ledger.authenticate(exported["api_key"].as_str().unwrap(),
        protocol::now().unwrap()).unwrap();
    assert_eq!(context.credential_id, exported["credential_id"].as_str().unwrap());
    assert!(context.scopes.iter().any(|scope| scope == "infer"));
    assert!(context.scopes.iter().any(|scope| scope == "read"));
    assert_eq!(serde_json::to_value(context.account).unwrap(), account);
    assert_eq!(saved()["active_credential"], exported["credential_id"]);
    assert_eq!(saved()["credentials"].as_array().unwrap().len(), 1);
    assert!(saved()["credentials"][0]["bearer"] == exported["api_key"]);
    assert_original(&fixture.provider.state.ledger, 2);

    // Both independently issued secrets still authorize real production reads;
    // explicit issuance restores native infer/read defaults without revocation.
    let http = reqwest::Client::new();
    for bearer in [original.bearer.as_str(), exported["api_key"].as_str().unwrap()] {
        let response = http.get(format!("{}/v1/contracts/{id}", fixture.provider.offer.payload.endpoint))
            .bearer_auth(bearer).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.json::<Value>().await.unwrap(), account);
    }
    for resume in [true, false] {
        fixture.consumer = Consumer::new(config.clone()).unwrap();
        clear_controls();
        let refreshed = if resume { fixture.consumer.resume(&id).await.unwrap() }
            else { fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.unwrap() };
        assert_free_control_exchange(&fixture, &fixture.quote, "GET", StatusCode::OK);
        assert_eq!(serde_json::to_value(refreshed).unwrap(), account);
        assert!(fixture.consumer.credential_export(&id, None).unwrap() == exported,
            "native refresh must preserve the explicitly exported key");
        assert_eq!(saved()["active_credential"], exported["credential_id"]);
        assert_original(&fixture.provider.state.ledger, 2);
        no_payments();
    }
    fixture.stop().await;
    rpc_server.abort();
    assert!(rpc_server.await.unwrap_err().is_cancelled());
}
