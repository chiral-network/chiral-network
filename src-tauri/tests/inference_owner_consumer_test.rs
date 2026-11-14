//! Native owner-proof acceptance against the production control router on isolated loopback.
//! Adversarial middleware changes only challenge replies; no chain/engine simulation or funds.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::Response,
};
use chiral_network::{
    auth::{self, AuthPayload},
    billing_store::BillingStore,
    consumer::{Consumer, ConsumerConfig},
    contracts,
    protocol::{self, Quote, Signed, AUTH_DOMAIN, QUOTE_DOMAIN},
    wallet,
};
use proposal::PreparedProvider;
use serde_json::{json, Value};
use std::{path::Path, sync::{Arc, Mutex}};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

const CHALLENGE: &str = "/v1/auth/challenge";
const OPEN: &str = "/v1/contracts/open";

#[derive(Clone, Copy, Debug)]
enum Reply {
    Honest,
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
    Expired,
    FarFuture,
}

fn change_hex(value: &mut String) {
    let replacement = if value.as_bytes()[2] == b'0' { "1" } else { "0" };
    value.replace_range(2..3, replacement);
}

fn transform(mode: Reply, mut payload: AuthPayload, mut reply: Value) -> Value {
    match mode {
        Reply::Honest => return reply,
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
}

struct DeliveredChallenge {
    request: Value,
    reply: Value,
}

#[derive(Default)]
struct Observations {
    challenges: Vec<DeliveredChallenge>,
    controls: Vec<ControlRequest>,
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
    let challenge_request = is_challenge.then(|| serde_json::from_slice(&body).unwrap());
    if !is_challenge && parts.uri.path() != "/v1/contracts/propose" {
        peer.observations.lock().unwrap().controls.push(ControlRequest {
            method: parts.method.to_string(),
            target: parts.uri.path_and_query().unwrap().as_str().into(),
            body: body.clone(),
            owner: parts.headers.get("x-chiral-owner").cloned(),
            nonce: parts.headers.get("x-chiral-nonce").cloned(),
            signature: parts.headers.get("x-chiral-signature").cloned(),
        });
    }
    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
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
