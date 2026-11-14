//! Fresh funding admission through the production HTTP router and isolated disk ledgers.
//! RPC observations are synthetic, not live-chain qualification, execution or paid credit.
//! All envelopes are independently signed with public test scalars; nothing is broadcast.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use chiral_network::{
    auth::{self, AuthPayload},
    billing_store::BillingStore,
    chain_client::{ChainClient, ValidatedPaymentEnvelope},
    contracts,
    protocol::{self, Challenge, FundingIntent, NetworkIdentity, Quote, Signed},
    wallet,
};
use proposal::PreparedProvider;
use rlp::RlpStream;
use rusqlite::Connection;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{net::TcpListener, path::Path, sync::{Arc, Mutex}};
use tiny_keccak::{Hasher, Keccak};
use tokio::task::JoinHandle;
use tower::Service;

const GAS: u64 = 1_000_000;
const ENVELOPES: [(u8, &str); 3] = [(0, "legacy"), (1, "EIP-2930"), (2, "EIP-1559")];

#[derive(Clone, Copy)]
struct ChainView {
    latest_nonce: u64,
    pending_nonce: u64,
    head_height: u64,
    head_hash: u64,
}

impl Default for ChainView {
    fn default() -> Self {
        Self { latest_nonce: 0, pending_nonce: 0, head_height: 10, head_hash: 10 }
    }
}

#[derive(Default)]
struct RpcControl {
    balance: u128,
    view: ChainView,
    after_balance: Option<ChainView>,
}

struct SpendabilityRpc {
    network: NetworkIdentity,
    payer: String,
    observed_at: u64,
    control: Mutex<RpcControl>,
    requests: Mutex<Vec<Value>>,
}

async fn rpc_reply(State(state): State<Arc<SpendabilityRpc>>, Json(request): Json<Value>) -> Json<Value> {
    state.requests.lock().unwrap().push(request.clone());
    let args = &request["params"];
    let mut control = state.control.lock().unwrap();
    let result = match request["method"].as_str().unwrap() {
        "eth_chainId" => json!(format!("0x{:x}", state.network.chain_id)),
        "eth_syncing" => json!(false),
        "eth_getBlockByNumber" => {
            assert_eq!(args[1], false);
            let (number, hash, timestamp) = match args[0].as_str().unwrap() {
                "0x0" => (0_u64, state.network.genesis_hash.clone(), 0),
                "safe" => (9, format!("0x{:064x}", 9), state.observed_at),
                "latest" => (control.view.head_height, format!("0x{:064x}", control.view.head_hash), state.observed_at),
                tag => panic!("unexpected fixture block selector: {tag}"),
            };
            json!({"number":format!("0x{number:x}"),"hash":hash,"timestamp":format!("0x{timestamp:x}")})
        }
        "eth_getTransactionCount" => {
            assert_eq!(args[0], state.payer);
            assert!(matches!(args[1].as_str(), Some("latest" | "pending")));
            let nonce = if args[1] == "latest" { control.view.latest_nonce } else { control.view.pending_nonce };
            json!(format!("0x{nonce:x}"))
        }
        "eth_getBalance" => {
            assert_eq!(args, &json!([state.payer, "latest"]));
            let balance = control.balance;
            // Deterministic transition at the balance RPC, before its reply reaches admission.
            if let Some(next) = control.after_balance.take() { control.view = next; }
            json!(format!("0x{balance:x}"))
        }
        _ => return Json(json!({"jsonrpc":"2.0","id":request["id"],
            "error":{"code":-32601,"message":"unexpected method at isolated admission RPC"}})),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

fn append_uint(stream: &mut RlpStream, value: u128) {
    let bytes = value.to_be_bytes();
    let start = bytes.iter().position(|byte| *byte != 0).unwrap_or(bytes.len());
    stream.append(&&bytes[start..]);
}

struct PaymentFields {
    chain_id: u64,
    nonce: u64,
    recipient: Vec<u8>,
    value: u128,
    data: Vec<u8>,
    gas: u64,
    max_fee: u128,
    priority_fee: u128,
}

impl PaymentFields {
    fn opening(quote: &Quote, nonce: u64, max_fee: u128) -> Self {
        Self {
            chain_id: quote.network.chain_id,
            nonce,
            recipient: hex::decode(quote.provider_wallet.strip_prefix("0x").unwrap()).unwrap(),
            value: quote.funding_wei.parse().unwrap(),
            data: protocol::opening_data(quote).unwrap(),
            gas: GAS,
            max_fee,
            // Valid controls deliberately distinguish priority from maximum price.
            priority_fee: 2,
        }
    }
}

fn signed_payment(payment: &PaymentFields, key: &str, kind: u8) -> String {
    let fields = |stream: &mut RlpStream| {
        if kind != 0 { stream.append(&payment.chain_id); }
        stream.append(&payment.nonce);
        if kind == 2 { append_uint(stream, payment.priority_fee); }
        append_uint(stream, payment.max_fee);
        stream.append(&payment.gas).append(&payment.recipient.as_slice());
        append_uint(stream, payment.value);
        stream.append(&payment.data.as_slice());
        if kind != 0 { stream.begin_list(0); }
    };
    let mut unsigned = RlpStream::new_list(match kind { 0 | 2 => 9, 1 => 8, _ => unreachable!() });
    fields(&mut unsigned);
    if kind == 0 { unsigned.append(&payment.chain_id).append(&0_u8).append(&0_u8); }
    let mut digest = [0_u8; 32];
    let mut keccak = Keccak::v256();
    if kind != 0 { keccak.update(&[kind]); }
    keccak.update(&unsigned.out());
    keccak.finalize(&mut digest);
    let key = SecretKey::from_slice(&hex::decode(key).unwrap()).unwrap();
    let (recovery, signature) = Secp256k1::signing_only()
        .sign_ecdsa_recoverable(&Message::from_digest_slice(&digest).unwrap(), &key).serialize_compact();
    let mut signed = RlpStream::new_list(match kind { 0 => 9, 1 => 11, 2 => 12, _ => unreachable!() });
    fields(&mut signed);
    let parity = recovery.to_i32() as u64;
    signed.append(&if kind == 0 { payment.chain_id * 2 + 35 + parity } else { parity });
    for limb in [&signature[..32], &signature[32..]] {
        let start = limb.iter().position(|byte| *byte != 0).unwrap_or(limb.len());
        signed.append(&&limb[start..]);
    }
    let encoded = hex::encode(signed.out());
    if kind == 0 { format!("0x{encoded}") } else { format!("0x{kind:02x}{encoded}") }
}

async fn request(app: &mut Router, path: &str, headers: &HeaderMap, body: Vec<u8>) -> (StatusCode, Value) {
    let mut request = Request::builder().method("POST").uri(path)
        .header(header::CONTENT_TYPE, "application/json").body(Body::from(body)).unwrap();
    request.headers_mut().extend(headers.clone());
    let response = app.call(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 96 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

struct Fixture {
    provider: PreparedProvider,
    app: Router,
    owner_key: String,
    quote: Signed<Quote>,
    rpc: Arc<SpendabilityRpc>,
    server: JoinHandle<()>,
    _listeners: [TcpListener; 2],
}

impl Drop for Fixture {
    fn drop(&mut self) { self.server.abort(); }
}

impl Fixture {
    async fn new(directory: &Path, case: u64, funding: u128) -> Self {
        let public = TcpListener::bind("127.0.0.1:0").unwrap();
        let admin = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut provider = PreparedProvider::new(directory, case, "1", "2",
            public.local_addr().unwrap(), admin.local_addr().unwrap());
        let owner_key = format!("{:064x}", 70_000 + case);
        let payer = wallet::address_from_private_key(&owner_key).unwrap();
        let rpc = Arc::new(SpendabilityRpc { network: provider.offer.payload.network.clone(),
            payer: payer.clone(), observed_at: protocol::now().unwrap(), control: Mutex::new(RpcControl::default()),
            requests: Mutex::new(Vec::new()) });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        // Override only the unsigned RPC transport before sharing the provider state.
        // Its public/model endpoints do not simulate RPC or engine responses.
        let state = Arc::get_mut(&mut provider.state).unwrap();
        let config = Arc::get_mut(&mut state.config).unwrap();
        config.network.rpc_url = format!("http://{}", listener.local_addr().unwrap());
        state.chain = Arc::new(ChainClient::new(config.network.clone(), config.data_dir.clone()).unwrap());
        let rpc_app = Router::new().route("/", post(rpc_reply)).with_state(rpc.clone());
        let server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });
        let mut app = contracts::router(provider.state.clone());
        let (status, document) = request(&mut app, "/v1/contracts/propose", &HeaderMap::new(),
            serde_json::to_vec(&json!({"offer_id":provider.offer_id,"consumer_wallet":payer,
                "funding_wei":funding.to_string()})).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{document}");
        let quote = serde_json::from_value(document).unwrap();
        Self { provider, app, owner_key, quote, rpc, server, _listeners: [public, admin] }
    }

    fn original(&self, kind: u8, fields: &PaymentFields) -> (String, ValidatedPaymentEnvelope) {
        let raw = signed_payment(fields, &self.owner_key, kind);
        // Independently validate the control before exercising an admission refusal.
        let envelope = self.provider.state.chain.validate_payment_envelope(&raw,
            &self.quote.payload.consumer_wallet, &self.quote.payload.provider_wallet,
            &self.quote.payload.funding_wei, &protocol::opening_data(&self.quote.payload).unwrap()).unwrap();
        (raw, envelope)
    }

    async fn submit_intent(&mut self, raw: &str) -> (StatusCode, Value) {
        let path = format!("/v1/contracts/{}/funding-intent", self.quote.payload.id);
        let body = serde_json::to_vec(&json!({"raw_transaction":raw})).unwrap();
        let body_hash = protocol::hash_hex(&body);
        let (status, document) = request(&mut self.app, "/v1/auth/challenge", &HeaderMap::new(),
            serde_json::to_vec(&json!({"wallet":self.quote.payload.consumer_wallet,
                "method":"POST","path":path,"body_hash":body_hash})).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{document}");
        let challenge: Challenge = serde_json::from_value(document).unwrap();
        let proof = AuthPayload { network: self.quote.payload.network.clone(),
            provider_wallet: self.quote.payload.provider_wallet.clone(), wallet: self.quote.payload.consumer_wallet.clone(),
            method: "POST".into(), path: path.clone(), body_hash,
            nonce: challenge.nonce.clone(), expires_at: challenge.expires_at };
        let payload = auth::challenge_payload(&proof).unwrap();
        assert_eq!(hex::decode(challenge.payload_hex.strip_prefix("0x").unwrap()).unwrap(), payload);
        let signature = wallet::sign_message(&self.owner_key, &payload).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-chiral-owner", proof.wallet.parse().unwrap());
        headers.insert("x-chiral-nonce", proof.nonce.parse().unwrap());
        headers.insert("x-chiral-signature", signature.parse().unwrap());
        request(&mut self.app, &path, &headers, body).await
    }

    fn observed_without_submission(&self, method: Option<&str>) {
        let requests = self.rpc.requests.lock().unwrap();
        if let Some(method) = method {
            assert!(requests.iter().any(|request| request["method"] == method),
                "the request must reach {method} rather than fail earlier");
        }
        assert!(requests.iter().all(|request| request["method"] != "eth_sendRawTransaction"),
            "admission must not broadcast");
    }

    fn persisted_without_credit(&self) -> BillingStore {
        let ledger = BillingStore::open(&self.provider.ledger_path, &self.quote.payload.network).unwrap();
        assert_eq!(serde_json::to_value(ledger.quote(&self.quote.payload.id).unwrap()).unwrap(),
            serde_json::to_value(&self.quote).unwrap());
        assert!(ledger.account(&self.quote.payload.id).unwrap_err().starts_with("not_found:"));
        let rows: (u64, u64) = Connection::open(&self.provider.ledger_path).unwrap().query_row(
            "SELECT (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations)",
            [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(rows, (0, 0));
        ledger
    }

    async fn refuses(&mut self, raw: &str, name: &str, expected: (StatusCode, &str), observation: Option<&str>) {
        let (status, document) = self.submit_intent(raw).await;
        assert_eq!(status, expected.0, "{name}: {document}");
        assert_eq!(document["error"]["code"], expected.1, "{name}");
        self.observed_without_submission(observation);
        assert!(self.persisted_without_credit().funding_intent(&self.quote.payload, raw).unwrap().is_none(), "{name}");
    }

    async fn acknowledges(&mut self, raw: &str, envelope: &ValidatedPaymentEnvelope, name: &str) {
        let (status, document) = self.submit_intent(raw).await;
        assert_eq!(status, StatusCode::OK, "{name}: {document}");
        let ack: Signed<FundingIntent> = serde_json::from_value(document).unwrap();
        protocol::verify(protocol::INTENT_DOMAIN, &ack, &self.quote.payload.provider_wallet).unwrap();
        assert_eq!(ack.payload.network, self.quote.payload.network, "{name}");
        assert_eq!(ack.payload.provider_wallet, self.quote.payload.provider_wallet, "{name}");
        assert_eq!(ack.payload.consumer_wallet, self.quote.payload.consumer_wallet, "{name}");
        assert_eq!(ack.payload.quote_id, self.quote.payload.id, "{name}");
        assert_eq!(ack.payload.tx_hash, envelope.tx_hash, "{name}");
        assert!((self.quote.payload.issued_at..self.quote.payload.expires_at).contains(&ack.payload.registered_at), "{name}");
        self.observed_without_submission(Some("eth_getBalance"));
        assert_eq!(serde_json::to_value(self.persisted_without_credit()
            .funding_intent(&self.quote.payload, raw).unwrap()).unwrap(), serde_json::to_value(&ack).unwrap(), "{name}");
    }
}

#[tokio::test]
async fn fresh_intent_requires_exact_full_signed_cost_for_all_envelopes() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, name) in ENVELOPES {
        let mut fixture = Fixture::new(directory.path(), 50_000 + u64::from(kind), 100).await;
        let (raw, envelope) = fixture.original(kind, &PaymentFields::opening(&fixture.quote.payload, 0, 11));
        let needed = 100 + u128::from(GAS) * 11;
        fixture.rpc.control.lock().unwrap().balance = needed - 1;
        fixture.refuses(&raw, name, (StatusCode::BAD_REQUEST, "invalid_payment"), Some("eth_getBalance")).await;
        // Same quote and original bytes, now with exactly enough observed balance and fresh authority.
        fixture.rpc.control.lock().unwrap().balance = needed;
        fixture.rpc.requests.lock().unwrap().clear();
        fixture.acknowledges(&raw, &envelope, name).await;
    }
}

#[tokio::test]
async fn fresh_intent_rejects_gas_product_above_u128_without_wrapping() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, name) in ENVELOPES {
        let mut fixture = Fixture::new(directory.path(), 51_000 + u64::from(kind), 100).await;
        let (raw, _) = fixture.original(kind, &PaymentFields::opening(&fixture.quote.payload, 0, u128::MAX));
        fixture.rpc.control.lock().unwrap().balance = u128::MAX;
        fixture.refuses(&raw, name, (StatusCode::BAD_REQUEST, "invalid_payment"), Some("eth_getBalance")).await;
    }
}

#[tokio::test]
async fn fresh_intent_rejects_value_plus_gas_one_above_u128_without_wrapping() {
    let directory = tempfile::tempdir().unwrap();
    // The gas product fits u128; adding this funding value exceeds u128::MAX by exactly one.
    let max_fee = u128::MAX / u128::from(GAS);
    let funding = u128::MAX % u128::from(GAS) + 1;
    for (kind, name) in ENVELOPES {
        let mut fixture = Fixture::new(directory.path(), 52_000 + u64::from(kind), funding).await;
        let (raw, _) = fixture.original(kind, &PaymentFields::opening(&fixture.quote.payload, 0, max_fee));
        fixture.rpc.control.lock().unwrap().balance = u128::MAX;
        fixture.refuses(&raw, name, (StatusCode::BAD_REQUEST, "invalid_payment"), Some("eth_getBalance")).await;
    }
}

#[tokio::test]
async fn fresh_intent_refuses_noncurrent_exhausted_and_competing_nonces() {
    let directory = tempfile::tempdir().unwrap();
    let cases = [
        ("future", 1, 0, 0, (StatusCode::BAD_REQUEST, "invalid_payment")),
        ("spent", 0, 1, 1, (StatusCode::BAD_REQUEST, "invalid_payment")),
        ("exhausted", u64::MAX, u64::MAX, u64::MAX, (StatusCode::BAD_REQUEST, "invalid_payment")),
        ("competing pending", 0, 0, 1, (StatusCode::CONFLICT, "funding_pending")),
        ("inconsistent pending", 1, 1, 0, (StatusCode::SERVICE_UNAVAILABLE, "settlement_unavailable")),
    ];
    for (kind, envelope_name) in ENVELOPES {
        for (index, (case, nonce, latest, pending, expected)) in cases.into_iter().enumerate() {
            let name = format!("{envelope_name}: {case}");
            let mut fixture = Fixture::new(directory.path(), 53_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let (raw, envelope) = fixture.original(kind, &PaymentFields::opening(&fixture.quote.payload, nonce, 11));
            assert_eq!(envelope.nonce, nonce, "{name}");
            {
                let mut control = fixture.rpc.control.lock().unwrap();
                control.balance = u128::MAX;
                control.view.latest_nonce = latest;
                control.view.pending_nonce = pending;
            }
            fixture.refuses(&raw, &name, expected, Some("eth_getTransactionCount")).await;
            // Exhaustion cannot become admissible without changing the original, which is forbidden here.
            if nonce != u64::MAX {
                {
                    let mut control = fixture.rpc.control.lock().unwrap();
                    control.view.latest_nonce = nonce;
                    control.view.pending_nonce = nonce;
                }
                fixture.rpc.requests.lock().unwrap().clear();
                fixture.acknowledges(&raw, &envelope, &name).await;
            }
        }
    }
}

#[tokio::test]
async fn fresh_intent_rechecks_nonce_and_head_after_balance_then_recovers_the_original() {
    let directory = tempfile::tempdir().unwrap();
    let cases = [
        ("latest nonce changes", ChainView { latest_nonce: 1, pending_nonce: 1, ..ChainView::default() }),
        ("pending nonce changes", ChainView { pending_nonce: 1, ..ChainView::default() }),
        // Safe remains height/hash 9, so the regressed second view is internally healthy.
        ("head regresses", ChainView { head_height: 9, head_hash: 9, ..ChainView::default() }),
        ("same-height hash changes", ChainView { head_hash: 11, ..ChainView::default() }),
    ];
    for (kind, envelope_name) in ENVELOPES {
        for (index, (case, changed)) in cases.into_iter().enumerate() {
            let name = format!("{envelope_name}: {case}");
            let mut fixture = Fixture::new(directory.path(), 54_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let (raw, envelope) = fixture.original(kind, &PaymentFields::opening(&fixture.quote.payload, 0, 11));
            {
                let mut control = fixture.rpc.control.lock().unwrap();
                control.balance = u128::MAX;
                control.after_balance = Some(changed);
            }
            fixture.refuses(&raw, &name, (StatusCode::CONFLICT, "funding_pending"), Some("eth_getBalance")).await;
            fixture.rpc.control.lock().unwrap().view = ChainView::default();
            fixture.rpc.requests.lock().unwrap().clear();
            fixture.acknowledges(&raw, &envelope, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_allows_healthy_head_advancement_after_balance() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, name) in ENVELOPES {
        let mut fixture = Fixture::new(directory.path(), 55_000 + u64::from(kind), 100).await;
        let (raw, envelope) = fixture.original(kind, &PaymentFields::opening(&fixture.quote.payload, 0, 11));
        {
            let mut control = fixture.rpc.control.lock().unwrap();
            control.balance = u128::MAX;
            control.after_balance = Some(ChainView { head_height: 11, head_hash: 11, ..ChainView::default() });
        }
        fixture.acknowledges(&raw, &envelope, name).await;
    }
}

#[tokio::test]
async fn fresh_intent_rejects_signed_payment_substitutions() {
    let directory = tempfile::tempdir().unwrap();
    let other_key = format!("{:064x}", 7);
    for (kind, envelope_name) in ENVELOPES {
        for (index, field) in ["chain", "payer", "recipient", "value", "commitment"].into_iter().enumerate() {
            let name = format!("{envelope_name}: substituted {field}");
            let mut fixture = Fixture::new(directory.path(), 56_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let mut fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let (original, envelope) = fixture.original(kind, &fields);
            let mut signing_key = fixture.owner_key.as_str();
            match field {
                "chain" => fields.chain_id += 1,
                "payer" => signing_key = &other_key,
                "recipient" => fields.recipient[0] ^= 1,
                "value" => fields.value += 1,
                "commitment" => *fields.data.last_mut().unwrap() ^= 1,
                _ => unreachable!(),
            }
            // Sign the substituted payload itself, not a mutation with a stale signature.
            let candidate = signed_payment(&fields, signing_key, kind);
            fixture.rpc.control.lock().unwrap().balance = u128::MAX;
            fixture.refuses(&candidate, &name, (StatusCode::BAD_REQUEST, "invalid_payment"), None).await;
            fixture.rpc.requests.lock().unwrap().clear();
            // A rejected candidate must not consume the quote or bind its sender/nonce.
            fixture.acknowledges(&original, &envelope, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_rejects_insufficient_signed_gas_and_zero_maximum_fee() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, violation) in ["insufficient structural gas", "zero maximum fee"].into_iter().enumerate() {
            let name = format!("{envelope_name}: {violation}");
            let mut fixture = Fixture::new(directory.path(), 57_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let mut fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let (original, envelope) = fixture.original(kind, &fields);
            match violation {
                // Opening calldata is nonempty, so transfer-only gas is insufficient.
                "insufficient structural gas" => fields.gas = 21_000,
                "zero maximum fee" => {
                    fields.max_fee = 0;
                    // Isolate the zero-price rule from inconsistent type-2 priority.
                    fields.priority_fee = 0;
                }
                _ => unreachable!(),
            }
            let candidate = signed_payment(&fields, &fixture.owner_key, kind);
            fixture.rpc.control.lock().unwrap().balance = u128::MAX;
            fixture.refuses(&candidate, &name, (StatusCode::BAD_REQUEST, "invalid_payment"), None).await;
            fixture.rpc.requests.lock().unwrap().clear();
            fixture.acknowledges(&original, &envelope, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_rejects_priority_fee_above_signed_maximum() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 58_000, 100).await;
    let mut fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
    let (original, envelope) = fixture.original(2, &fields);
    fields.priority_fee = fields.max_fee + 1;
    let candidate = signed_payment(&fields, &fixture.owner_key, 2);
    fixture.rpc.control.lock().unwrap().balance = u128::MAX;
    fixture.refuses(&candidate, "EIP-1559 priority above maximum",
        (StatusCode::BAD_REQUEST, "invalid_payment"), None).await;
    fixture.rpc.requests.lock().unwrap().clear();
    fixture.acknowledges(&original, &envelope, "EIP-1559 valid priority control").await;
}
