//! Local payment-envelope validation and fresh funding admission through production APIs.
//! HTTP fixtures use isolated ledgers and synthetic RPC, not live execution or paid credit.
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
    app::ProviderState,
    auth::{self, AuthPayload},
    billing_store::BillingStore,
    chain_client::{ChainClient, ChainError, ValidatedPaymentEnvelope},
    contracts,
    network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol::{self, Challenge, FundingIntent, NetworkIdentity, Quote, Signed},
    wallet,
};
use proposal::PreparedProvider;
use primitive_types::U256;
use rlp::{Rlp, RlpStream};
use rusqlite::{params, Connection};
use secp256k1::{ecdsa::{RecoverableSignature, RecoveryId}, Message, PublicKey, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{net::TcpListener, path::Path, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex}};
use tiny_keccak::{Hasher, Keccak};
use tokio::{sync::Barrier, task::JoinHandle};
use tower::Service;

const GAS: u64 = 1_000_000;
const ENVELOPES: [(u8, &str); 3] = [(0, "legacy"), (1, "EIP-2930"), (2, "EIP-1559")];
const MALFORMED_RLP_CHILDREN: [(&str, &[u8]); 6] = [
    ("incomplete-string-length", &[0xbf, 1, 0]),
    ("incomplete-list-length", &[0xff, 1, 0]),
    ("truncated-string-payload", &[0xb9, 1, 0]),
    ("truncated-list-payload", &[0xf9, 1, 0]),
    ("overflowing-string-length", &[0xbf, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
    ("overflowing-list-length", &[0xff; 9]),
];

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

#[derive(Clone, Copy)]
enum FundingGate {
    Accepting,
    Ready,
}

impl FundingGate {
    fn set(self, provider: &ProviderState, enabled: bool) {
        let gate = match self {
            Self::Accepting => &provider.accepting,
            Self::Ready => &provider.funding_ready,
        };
        gate.store(enabled, Ordering::Release);
    }
}

#[derive(Default)]
struct RpcControl {
    balance: u128,
    view: ChainView,
    after_balance: Option<ChainView>,
    after_balance_gate: Option<(Arc<ProviderState>, FundingGate)>,
    balance_barrier: Option<Arc<Barrier>>,
}

struct SpendabilityRpc {
    network: NetworkIdentity,
    payer: String,
    observed_at: AtomicU64,
    control: Mutex<RpcControl>,
    requests: Mutex<Vec<Value>>,
}

async fn rpc_reply(State(state): State<Arc<SpendabilityRpc>>, Json(request): Json<Value>) -> Json<Value> {
    state.requests.lock().unwrap().push(request.clone());
    let args = &request["params"];
    let barrier = if request["method"] == "eth_getBalance" {
        state.control.lock().unwrap().balance_barrier.take()
    } else { None };
    if let Some(barrier) = barrier { barrier.wait().await; }
    let mut control = state.control.lock().unwrap();
    let result = match request["method"].as_str().unwrap() {
        "eth_chainId" => json!(format!("0x{:x}", state.network.chain_id)),
        "eth_syncing" => json!(false),
        "eth_getBlockByNumber" => {
            assert_eq!(args[1], false);
            let (number, hash, timestamp) = match args[0].as_str().unwrap() {
                "0x0" => (0_u64, state.network.genesis_hash.clone(), 0),
                "safe" => (9, format!("0x{:064x}", 9), state.observed_at.load(Ordering::Acquire)),
                "latest" => (control.view.head_height, format!("0x{:064x}", control.view.head_hash), state.observed_at.load(Ordering::Acquire)),
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
            if let Some((provider, gate)) = control.after_balance_gate.take() { gate.set(&provider, false); }
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

fn encoded_access_list(addresses: usize, keys: usize, duplicates: bool) -> RlpStream {
    assert!(addresses > 0);
    let mut list = RlpStream::new_list(addresses);
    for index in 0..addresses {
        list.begin_list(2);
        let mut address = [0_u8; 20];
        if !duplicates { address[18..].copy_from_slice(&u16::try_from(index).unwrap().to_be_bytes()); }
        list.append(&address.as_slice());
        // Spread keys so the aggregate-overflow case has no oversized individual entry.
        let count = keys / addresses + usize::from(index < keys % addresses);
        list.begin_list(count);
        for slot in 0..count {
            let mut key = [0_u8; 32];
            if !duplicates {
                key[28..30].copy_from_slice(&u16::try_from(index).unwrap().to_be_bytes());
                key[30..].copy_from_slice(&u16::try_from(slot).unwrap().to_be_bytes());
            }
            list.append(&key.as_slice());
        }
    }
    list
}

fn single_access_entry(encoded_address: &[u8], encoded_keys: &[u8]) -> RlpStream {
    let mut list = RlpStream::new_list(1);
    list.begin_list(2).append_raw(encoded_address, 1).append_raw(encoded_keys, 1);
    list
}

fn append_list_with_malformed_tail(stream: &mut RlpStream, valid_list: &Rlp<'_>, tail: &[u8]) {
    let count = valid_list.item_count().unwrap();
    stream.begin_list(count + 1);
    for index in 0..count {
        stream.append_raw(valid_list.at(index).unwrap().as_raw(), 1);
    }
    // Include every actual byte in the enclosing length; only the appended child is malformed.
    stream.append_raw(tail, 1);
}

fn payment_digest(kind: u8, payload: &[u8]) -> [u8; 32] {
    let mut digest = [0_u8; 32];
    let mut keccak = Keccak::v256();
    if kind != 0 { keccak.update(&[kind]); }
    keccak.update(payload);
    keccak.finalize(&mut digest);
    digest
}

fn signed_payment(payment: &PaymentFields, key: &str, kind: u8, access_list: &[u8]) -> String {
    let fields = |stream: &mut RlpStream| {
        if kind != 0 { stream.append(&payment.chain_id); }
        stream.append(&payment.nonce);
        if kind == 2 { append_uint(stream, payment.priority_fee); }
        append_uint(stream, payment.max_fee);
        stream.append(&payment.gas).append(&payment.recipient.as_slice());
        append_uint(stream, payment.value);
        stream.append(&payment.data.as_slice());
        if kind != 0 { stream.append_raw(access_list, 1); }
    };
    let unsigned_fields = match kind { 0 => 6, 1 => 8, 2 => 9, _ => unreachable!() };
    sign_payment_fields(kind, payment.chain_id, key, unsigned_fields, fields)
}

fn sign_payment_fields(kind: u8, chain_id: u64, key: &str, unsigned_fields: usize, fields: impl Fn(&mut RlpStream)) -> String {
    let mut unsigned = RlpStream::new_list(unsigned_fields + if kind == 0 { 3 } else { 0 });
    fields(&mut unsigned);
    if kind == 0 { unsigned.append(&chain_id).append(&0_u8).append(&0_u8); }
    let digest = payment_digest(kind, &unsigned.out());
    let key = SecretKey::from_slice(&hex::decode(key).unwrap()).unwrap();
    let (recovery, signature) = Secp256k1::signing_only()
        .sign_ecdsa_recoverable(&Message::from_digest_slice(&digest).unwrap(), &key).serialize_compact();
    let mut signed = RlpStream::new_list(unsigned_fields + 3);
    fields(&mut signed);
    let parity = recovery.to_i32() as u64;
    signed.append(&if kind == 0 { chain_id * 2 + 35 + parity } else { parity });
    for limb in [&signature[..32], &signature[32..]] {
        let start = limb.iter().position(|byte| *byte != 0).unwrap_or(limb.len());
        signed.append(&&limb[start..]);
    }
    let encoded = hex::encode(signed.out());
    if kind == 0 { format!("0x{encoded}") } else { format!("0x{kind:02x}{encoded}") }
}

fn resign_field(raw: &str, kind: u8, chain_id: u64, key: &str, index: usize, encoded: &[u8]) -> String {
    let bytes = hex::decode(&raw[2..]).unwrap();
    let rlp = Rlp::new(&bytes[usize::from(kind != 0)..]);
    let unsigned_fields = rlp.item_count().unwrap() - 3;
    assert!(index < unsigned_fields);
    sign_payment_fields(kind, chain_id, key, unsigned_fields, |stream| {
        for field in 0..unsigned_fields {
            stream.append_raw(if field == index { encoded } else { rlp.at(field).unwrap().as_raw() }, 1);
        }
    })
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

async fn authorized_intent(app: &mut Router, quote: &Signed<Quote>, key: &str, raw: &str) -> (StatusCode, Value) {
    let path = format!("/v1/contracts/{}/funding-intent", quote.payload.id);
    let body = serde_json::to_vec(&json!({"raw_transaction":raw})).unwrap();
    let body_hash = protocol::hash_hex(&body);
    let (status, document) = request(app, "/v1/auth/challenge", &HeaderMap::new(),
        serde_json::to_vec(&json!({"wallet":quote.payload.consumer_wallet,
            "method":"POST","path":path,"body_hash":body_hash})).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let challenge: Challenge = serde_json::from_value(document).unwrap();
    let proof = AuthPayload { network: quote.payload.network.clone(),
        provider_wallet: quote.payload.provider_wallet.clone(), wallet: quote.payload.consumer_wallet.clone(),
        method: "POST".into(), path: path.clone(), body_hash,
        nonce: challenge.nonce.clone(), expires_at: challenge.expires_at };
    let payload = auth::challenge_payload(&proof).unwrap();
    assert_eq!(hex::decode(challenge.payload_hex.strip_prefix("0x").unwrap()).unwrap(), payload);
    let signature = wallet::sign_message(key, &payload).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("x-chiral-owner", proof.wallet.parse().unwrap());
    headers.insert("x-chiral-nonce", proof.nonce.parse().unwrap());
    headers.insert("x-chiral-signature", signature.parse().unwrap());
    request(app, &path, &headers, body).await
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
            payer: payer.clone(), observed_at: AtomicU64::new(protocol::now().unwrap()), control: Mutex::new(RpcControl::default()),
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
        let raw = signed_payment(fields, &self.owner_key, kind, &[0xc0]);
        // Independently validate the control before exercising an admission refusal.
        let envelope = self.envelope(&raw).unwrap();
        (raw, envelope)
    }

    fn envelope(&self, raw: &str) -> Result<ValidatedPaymentEnvelope, ChainError> {
        self.provider.state.chain.validate_payment_envelope(raw,
            &self.quote.payload.consumer_wallet, &self.quote.payload.provider_wallet,
            &self.quote.payload.funding_wei, &protocol::opening_data(&self.quote.payload).unwrap())
    }

    async fn submit_intent(&mut self, raw: &str) -> (StatusCode, Value) {
        authorized_intent(&mut self.app, &self.quote, &self.owner_key, raw).await
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

    async fn access_list_boundary(&mut self, kind: u8, accepted: &[u8], rejected: &[u8], name: &str) {
        let fields = PaymentFields::opening(&self.quote.payload, 0, 11);
        let original = signed_payment(&fields, &self.owner_key, kind, accepted);
        let candidate = signed_payment(&fields, &self.owner_key, kind, rejected);
        self.rejects_candidate_preserving_original(&original, &candidate, name).await;
    }

    async fn exact_gas_boundary(&mut self, kind: u8, access_list: &[u8], access_gas: u64, name: &str) {
        let mut fields = PaymentFields::opening(&self.quote.payload, 0, 11);
        // Independent oracle: charge every byte 16, then rebate 12 for each zero byte.
        let zero_bytes = fields.data.iter().filter(|byte| **byte == 0).count() as u64;
        fields.gas = 21_000 + 16 * fields.data.len() as u64 - 12 * zero_bytes + access_gas;
        let original = signed_payment(&fields, &self.owner_key, kind, access_list);
        fields.gas -= 1;
        let candidate = signed_payment(&fields, &self.owner_key, kind, access_list);
        self.rejects_candidate_preserving_original(&original, &candidate, name).await;
    }

    async fn rejects_candidate_preserving_original(&mut self, original: &str, candidate: &str, name: &str) {
        let envelope = self.envelope(original).unwrap();
        assert!(matches!(self.envelope(candidate), Err(ChainError::Rejected(_))), "{name}");
        self.rpc.control.lock().unwrap().balance = u128::MAX;
        self.refuses(candidate, name, (StatusCode::BAD_REQUEST, "invalid_payment"), None).await;
        self.rpc.requests.lock().unwrap().clear();
        self.acknowledges(original, &envelope, name).await;
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
            let candidate = signed_payment(&fields, signing_key, kind, &[0xc0]);
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
            let candidate = signed_payment(&fields, &fixture.owner_key, kind, &[0xc0]);
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
    let candidate = signed_payment(&fields, &fixture.owner_key, 2, &[0xc0]);
    fixture.rpc.control.lock().unwrap().balance = u128::MAX;
    fixture.refuses(&candidate, "EIP-1559 priority above maximum",
        (StatusCode::BAD_REQUEST, "invalid_payment"), None).await;
    fixture.rpc.requests.lock().unwrap().clear();
    fixture.acknowledges(&original, &envelope, "EIP-1559 valid priority control").await;
}

#[tokio::test]
async fn fresh_intent_rejects_resigned_noncanonical_and_oversized_integers() {
    let directory = tempfile::tempdir().unwrap();
    let cases = ["zero nonce byte", "leading-zero nonce", "oversized nonce",
        "oversized gas", "oversized value", "oversized maximum price"];
    for (kind, envelope_name) in ENVELOPES {
        for (index, case) in cases.into_iter().enumerate() {
            let name = format!("{envelope_name}: {case}");
            let mut fixture = Fixture::new(directory.path(), 59_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let nonce = u64::from(case == "leading-zero nonce");
            let fields = PaymentFields::opening(&fixture.quote.payload, nonce, 11);
            let (original, envelope) = fixture.original(kind, &fields);
            let nonce_field = usize::from(kind != 0);
            let fee_field = usize::from(kind) + 1;
            let mut encoded = [0_u8; 18];
            let (field, length) = match case {
                // Canonical integer zero is empty bytes, not a zero byte.
                "zero nonce byte" => (nonce_field, 1),
                "leading-zero nonce" => {
                    encoded[0] = 0x82;
                    encoded[2] = 1;
                    (nonce_field, 3)
                }
                _ => {
                    let (field, width, value) = match case {
                        "oversized nonce" => (nonce_field, 8, u128::from(fields.nonce)),
                        "oversized gas" => (fee_field + 1, 8, u128::from(fields.gas)),
                        "oversized value" => (fee_field + 3, 16, fields.value),
                        "oversized maximum price" => (fee_field, 16, fields.max_fee),
                        _ => unreachable!(),
                    };
                    // A high byte beyond the accepted width, with otherwise admissible low bits.
                    encoded[0] = 0x80 + (width + 1) as u8;
                    encoded[1] = 1;
                    encoded[2..width + 2].copy_from_slice(&value.to_be_bytes()[16 - width..]);
                    (field, width + 2)
                }
            };
            let candidate = resign_field(&original, kind, fields.chain_id, &fixture.owner_key,
                field, &encoded[..length]);
            {
                let mut control = fixture.rpc.control.lock().unwrap();
                control.balance = u128::MAX;
                control.view.latest_nonce = nonce;
                control.view.pending_nonce = nonce;
            }
            fixture.refuses(&candidate, &name, (StatusCode::BAD_REQUEST, "invalid_payment"), None).await;
            fixture.rpc.requests.lock().unwrap().clear();
            fixture.acknowledges(&original, &envelope, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_rejects_high_s_even_when_the_same_payer_is_recovered() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, name) in ENVELOPES {
        let mut fixture = Fixture::new(directory.path(), 60_000 + u64::from(kind), 100).await;
        let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
        let (original, envelope) = fixture.original(kind, &fields);
        let bytes = hex::decode(&original[2..]).unwrap();
        let rlp = Rlp::new(&bytes[usize::from(kind != 0)..]);
        let unsigned_fields = rlp.item_count().unwrap() - 3;
        let mut unsigned = RlpStream::new_list(if kind == 0 { 9 } else { unsigned_fields });
        for field in 0..unsigned_fields { unsigned.append_raw(rlp.at(field).unwrap().as_raw(), 1); }
        if kind == 0 { unsigned.append(&fields.chain_id).append(&0_u8).append(&0_u8); }
        let digest = payment_digest(kind, &unsigned.out());
        let base = if kind == 0 { fields.chain_id * 2 + 35 } else { 0 };
        let parity = ((rlp.val_at::<u64>(unsigned_fields).unwrap() - base) as u8) ^ 1;
        let order = U256::from_big_endian(&secp256k1::constants::CURVE_ORDER);
        let mut signature = [0_u8; 64];
        U256::from_big_endian(rlp.at(unsigned_fields + 1).unwrap().data().unwrap())
            .to_big_endian(&mut signature[..32]);
        let high_s = order - U256::from_big_endian(rlp.at(unsigned_fields + 2).unwrap().data().unwrap());
        high_s.to_big_endian(&mut signature[32..]);
        let malleated = RecoverableSignature::from_compact(&signature, RecoveryId::from_i32(i32::from(parity)).unwrap()).unwrap();
        let secp = Secp256k1::new();
        let key = SecretKey::from_slice(&hex::decode(&fixture.owner_key).unwrap()).unwrap();
        // This is a mathematically valid alternate signature, not a payer-mismatch test.
        assert_eq!(secp.recover_ecdsa(&Message::from_digest_slice(&digest).unwrap(), &malleated).unwrap(),
            PublicKey::from_secret_key(&secp, &key), "{name}");
        let mut signed = RlpStream::new_list(unsigned_fields + 3);
        for field in 0..unsigned_fields { signed.append_raw(rlp.at(field).unwrap().as_raw(), 1); }
        signed.append(&(base + u64::from(parity)));
        signed.append_raw(rlp.at(unsigned_fields + 1).unwrap().as_raw(), 1);
        signed.append(&&signature[32..]);
        let encoded = hex::encode(signed.out());
        let candidate = if kind == 0 { format!("0x{encoded}") } else { format!("0x{kind:02x}{encoded}") };
        fixture.rpc.control.lock().unwrap().balance = u128::MAX;
        fixture.refuses(&candidate, name, (StatusCode::BAD_REQUEST, "invalid_payment"), None).await;
        fixture.rpc.requests.lock().unwrap().clear();
        fixture.acknowledges(&original, &envelope, name).await;
    }
}

#[tokio::test]
async fn fresh_intent_enforces_access_list_address_occurrence_limit() {
    let directory = tempfile::tempdir().unwrap();
    for duplicates in [false, true] {
        let accepted = encoded_access_list(64, 0, duplicates).out();
        let rejected = encoded_access_list(65, 0, duplicates).out();
        for &(kind, envelope_name) in &ENVELOPES[1..] {
            let name = format!("{envelope_name}: 64/65 addresses, duplicate={duplicates}");
            let mut fixture = Fixture::new(directory.path(),
                61_000 + u64::from(duplicates) * 10 + u64::from(kind), 100).await;
            fixture.access_list_boundary(kind, &accepted, &rejected, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_enforces_aggregate_storage_key_occurrence_limit() {
    let directory = tempfile::tempdir().unwrap();
    for (index, addresses) in [1, 64].into_iter().enumerate() {
        for duplicates in [false, true] {
            let accepted = encoded_access_list(addresses, 256, duplicates).out();
            let rejected = encoded_access_list(addresses, 257, duplicates).out();
            for &(kind, envelope_name) in &ENVELOPES[1..] {
                let name = format!("{envelope_name}: 256/257 keys over {addresses} addresses, duplicate={duplicates}");
                let mut fixture = Fixture::new(directory.path(),
                    62_000 + index as u64 * 100 + u64::from(duplicates) * 10 + u64::from(kind), 100).await;
                fixture.access_list_boundary(kind, &accepted, &rejected, &name).await;
            }
        }
    }
}

#[tokio::test]
async fn fresh_intent_rejects_malformed_access_list_shapes() {
    let directory = tempfile::tempdir().unwrap();
    let accepted = encoded_access_list(1, 1, true).out();
    let address = rlp::encode(&[0_u8; 20].as_slice());
    let key = rlp::encode(&[0_u8; 32].as_slice());
    let mut keys = RlpStream::new_list(1);
    keys.append_raw(&key, 1);
    let keys = keys.out();
    for (index, shape) in [
        "list-empty-bytes", "list-encoded-as-bytes", "entry-bytes", "entry-empty",
        "entry-address-only", "entry-extra-field", "address-list", "keys-empty-bytes",
        "keys-flat-bytes", "key-list",
    ].into_iter().enumerate() {
        let mut malformed = RlpStream::new();
        match shape {
            "list-empty-bytes" => { malformed.append_raw(&[0x80], 1); }
            "list-encoded-as-bytes" => { malformed.append(&&accepted[..]); }
            "entry-bytes" => { malformed.begin_list(1).append_raw(&address, 1); }
            "entry-empty" => { malformed.begin_list(1).begin_list(0); }
            "entry-address-only" => { malformed.begin_list(1).begin_list(1).append_raw(&address, 1); }
            "entry-extra-field" => {
                malformed.begin_list(1).begin_list(3).append_raw(&address, 1)
                    .append_raw(&keys, 1).append_raw(&[0x80], 1);
            }
            "address-list" => { malformed = single_access_entry(&[0xc0], &keys); }
            "keys-empty-bytes" => { malformed = single_access_entry(&address, &[0x80]); }
            "keys-flat-bytes" => { malformed = single_access_entry(&address, &key); }
            "key-list" => {
                let mut nested = RlpStream::new_list(1);
                // An empty key-list is at depth four: the depth guard cannot mask its wrong type.
                nested.begin_list(0);
                malformed = single_access_entry(&address, &nested.out());
            }
            _ => unreachable!(),
        }
        let rejected = malformed.out();
        for &(kind, envelope_name) in &ENVELOPES[1..] {
            let name = format!("{envelope_name}: {shape}");
            let mut fixture = Fixture::new(directory.path(),
                63_000 + index as u64 * 10 + u64::from(kind), 100).await;
            fixture.access_list_boundary(kind, &accepted, &rejected, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_requires_exact_access_list_address_width() {
    let directory = tempfile::tempdir().unwrap();
    let accepted = encoded_access_list(1, 1, true).out();
    let mut keys = RlpStream::new_list(1);
    keys.append(&[0_u8; 32].as_slice());
    let keys = keys.out();
    for width in [0, 19, 21] {
        let address = rlp::encode(&&[0_u8; 21][..width]);
        let rejected = single_access_entry(&address, &keys).out();
        for &(kind, envelope_name) in &ENVELOPES[1..] {
            let name = format!("{envelope_name}: {width}-byte access-list address");
            let mut fixture = Fixture::new(directory.path(),
                64_000 + width as u64 * 10 + u64::from(kind), 100).await;
            fixture.access_list_boundary(kind, &accepted, &rejected, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_requires_exact_storage_key_width() {
    let directory = tempfile::tempdir().unwrap();
    let accepted = encoded_access_list(1, 1, true).out();
    let address = rlp::encode(&[0_u8; 20].as_slice());
    for width in [0, 31, 33] {
        let mut keys = RlpStream::new_list(1);
        keys.append(&&[0_u8; 33][..width]);
        let rejected = single_access_entry(&address, &keys.out()).out();
        for &(kind, envelope_name) in &ENVELOPES[1..] {
            let name = format!("{envelope_name}: {width}-byte storage key");
            let mut fixture = Fixture::new(directory.path(),
                65_000 + width as u64 * 10 + u64::from(kind), 100).await;
            fixture.access_list_boundary(kind, &accepted, &rejected, &name).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_enforces_exact_calldata_gas_for_all_envelopes() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, name) in ENVELOPES {
        let mut fixture = Fixture::new(directory.path(), 66_000 + u64::from(kind), 100).await;
        fixture.exact_gas_boundary(kind, &[0xc0], 0, name).await;
    }
}

#[tokio::test]
async fn fresh_intent_charges_every_access_list_occurrence() {
    let directory = tempfile::tempdir().unwrap();
    // Literal expected access costs keep the oracle separate from the production gas helper.
    let cases = [
        (1, 0, false, 2_400),
        (1, 1, false, 4_300),
        (2, 0, true, 4_800),
        (1, 2, true, 6_200),
        (2, 2, true, 8_600),
        (2, 4, true, 12_400),
        (64, 256, false, 640_000),
        (64, 256, true, 640_000),
    ];
    for (index, (addresses, keys, duplicates, access_gas)) in cases.into_iter().enumerate() {
        let access_list = encoded_access_list(addresses, keys, duplicates).out();
        for &(kind, envelope_name) in &ENVELOPES[1..] {
            let name = format!("{envelope_name}: {addresses} addresses, {keys} keys, duplicate={duplicates}");
            let mut fixture = Fixture::new(directory.path(),
                67_000 + index as u64 * 10 + u64::from(kind), 100).await;
            fixture.exact_gas_boundary(kind, &access_list, access_gas, &name).await;
        }
    }
}

#[test]
fn local_envelope_enforces_calldata_limit_without_truncation() {
    let directory = tempfile::tempdir().unwrap();
    let client = ChainClient::new(NetworkConfig {
        identity: NetworkIdentity {
            chain_id: ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: ESPACE_TESTNET_GENESIS.into(),
            deployment_id: "calldata-boundary-test".into(),
        },
        // The synchronous local API needs no RPC server or funding-route fixture.
        rpc_url: "http://127.0.0.1:1".into(),
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    }, directory.path().into()).unwrap();
    let key = format!("{:064x}", 68_000);
    let payer = wallet::address_from_private_key(&key).unwrap();
    let mut data = Vec::with_capacity(4_097);
    data.extend((0..=u8::MAX).cycle().take(4_096));
    let mut fields = PaymentFields {
        chain_id: ESPACE_TESTNET_CHAIN_ID, nonce: 0, recipient: vec![0x22; 20], value: 100,
        data, gas: GAS, max_fee: 11, priority_fee: 2,
    };
    let payee = format!("0x{}", hex::encode(&fields.recipient));
    let validate = |raw: &str, data: &[u8]| {
        client.validate_payment_envelope(raw, &payer, &payee, "100", data)
    };
    for (kind, name) in ENVELOPES {
        fields.data.truncate(4_096);
        let original = signed_payment(&fields, &key, kind, &[0xc0]);
        let envelope = validate(&original, &fields.data).unwrap();
        assert_eq!(envelope.data, fields.data, "{name}: preserve every opaque byte");
        assert_eq!(envelope.tx_hash, protocol::hash_hex(&hex::decode(&original[2..]).unwrap()), "{name}");

        fields.data.push(0);
        let candidate = signed_payment(&fields, &key, kind, &[0xc0]);
        assert!(matches!(validate(&candidate, &fields.data), Err(ChainError::Rejected(_))), "{name}");
        // Matching only the permitted prefix must not turn clipping into valid payment evidence.
        assert!(matches!(validate(&candidate, &fields.data[..4_096]), Err(ChainError::Rejected(_))), "{name}");
        assert_eq!(validate(&original, &fields.data[..4_096]).unwrap(), envelope, "{name}");
    }
    // Both sides of RLP's short/long byte-string boundary remain valid calldata.
    for length in [56, 55] {
        fields.data.truncate(length);
        for (kind, name) in ENVELOPES {
            let raw = signed_payment(&fields, &key, kind, &[0xc0]);
            let envelope = validate(&raw, &fields.data).unwrap();
            assert_eq!(envelope.data, fields.data, "{name}: {length}-byte calldata");
            assert_eq!(envelope.tx_hash, protocol::hash_hex(&hex::decode(&raw[2..]).unwrap()), "{name}");
        }
    }
}

#[tokio::test]
async fn malformed_raw_envelopes_preserve_fresh_funding_admission() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, malformed) in [
            "empty-string", "missing-prefix", "uppercase-prefix", "empty-payload",
            "odd-payload", "non-hex", "leading-whitespace", "trailing-whitespace",
            "trailing-byte", "concatenated-envelope", "truncated-byte", "zero-tag",
        ].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 9_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let original = signed_payment(&fields, &fixture.owner_key, kind, &[0xc0]);
            let candidate = match malformed {
                "empty-string" => String::new(),
                "missing-prefix" => original[2..].to_owned(),
                "uppercase-prefix" => format!("0X{}", &original[2..]),
                "empty-payload" => "0x".to_owned(),
                "odd-payload" => format!("{original}0"),
                "non-hex" => format!("0xg{}", &original[3..]),
                "leading-whitespace" => format!("\t{original}"),
                "trailing-whitespace" => format!("{original} \n"),
                "trailing-byte" => format!("{original}00"),
                "concatenated-envelope" => format!("{original}{}", &original[2..]),
                "truncated-byte" => original[..original.len() - 2].to_owned(),
                "zero-tag" => format!("0x00{}", &original[2..]),
                _ => unreachable!(),
            };
            fixture.rejects_candidate_preserving_original(
                &original, &candidate, &format!("{envelope_name}: {malformed}"),
            ).await;
        }
    }
}

#[tokio::test]
async fn unsupported_typed_prefixes_do_not_admit_type_two_shaped_payments() {
    let directory = tempfile::tempdir().unwrap();
    for tag in [3_u8, 4, 0x7f] {
        let mut fixture = Fixture::new(directory.path(), 10_000 + u64::from(tag), 100).await;
        let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
        let original = signed_payment(&fields, &fixture.owner_key, 2, &[0xc0]);
        let bytes = hex::decode(&original[2..]).unwrap();
        let rlp = Rlp::new(&bytes[1..]);
        // Sign the unsupported prefix itself, not a stale type-2 digest. These
        // type-2-shaped payloads are not valid blob or authorization transactions.
        let candidate = sign_payment_fields(tag, fields.chain_id, &fixture.owner_key, 9, |stream| {
            for index in 0..9 {
                stream.append_raw(rlp.at(index).unwrap().as_raw(), 1);
            }
        });
        fixture.rejects_candidate_preserving_original(
            &original, &candidate, &format!("unsupported typed prefix 0x{tag:02x}"),
        ).await;
    }
}

#[tokio::test]
async fn hex_letter_case_preserves_payment_identity_and_retained_spelling() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, spelling_name) in ["lowercase", "uppercase", "mixed-case"].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 11_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let (original, envelope) = fixture.original(kind, &fields);
            let mut uppercase = original.clone();
            uppercase[2..].make_ascii_uppercase();
            let mut mixed = uppercase.clone();
            let first_letter = mixed[2..].find(|ch: char| ch.is_ascii_uppercase()).unwrap() + 2;
            mixed[first_letter..first_letter + 1].make_ascii_lowercase();
            let spellings = [original, uppercase, mixed];
            let accepted = &spellings[index];
            let name = format!("{envelope_name}: {spelling_name}");
            for spelling in &spellings {
                assert_eq!(fixture.envelope(spelling).unwrap(), envelope, "{name}");
            }
            fixture.rpc.control.lock().unwrap().balance = u128::MAX;
            fixture.acknowledges(accepted, &envelope, &name).await;
            let acknowledgement = serde_json::to_value(fixture.provider.state.ledger
                .funding_intent(&fixture.quote.payload, accepted).unwrap().unwrap()).unwrap();

            // Recovery must not delete an unrelated standalone quote or treat it
            // as another admission. No superseded named-wallet quota is an oracle.
            let (status, document) = request(&mut fixture.app, "/v1/contracts/propose", &HeaderMap::new(),
                serde_json::to_vec(&json!({"offer_id":fixture.provider.offer_id,
                    "consumer_wallet":fixture.quote.payload.consumer_wallet,"funding_wei":"100"})).unwrap()).await;
            assert_eq!(status, StatusCode::OK, "{name}: {document}");
            let pending: Signed<Quote> = serde_json::from_value(document).unwrap();
            assert_ne!(pending.payload.id, fixture.quote.payload.id, "{name}");
            let db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let retained_records = || {
                let mut statement = db.prepare(
                    "SELECT contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until
                     FROM funding_intents ORDER BY contract").unwrap();
                let records = statement.query_map([], |row| Ok((
                    row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?, row.get::<_, u64>(7)?,
                ))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
                records
            };
            let pending_record = || db.query_row("SELECT record FROM quotes WHERE id=?1",
                [&pending.payload.id], |row| row.get::<_, String>(0)).unwrap();
            let retained_before = retained_records();
            let pending_before = pending_record();
            fixture.rpc.requests.lock().unwrap().clear();

            for reopened in [false, true] {
                if reopened {
                    fixture.server.abort();
                    assert!((&mut fixture.server).await.unwrap_err().is_cancelled());
                    fixture.app = Router::new();
                    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                    state.ledger = Arc::new(BillingStore::open(
                        &fixture.provider.ledger_path, &fixture.quote.payload.network).unwrap());
                    state.chain = Arc::new(ChainClient::new(
                        state.config.network.clone(), state.config.data_dir.clone()).unwrap());
                    state.accepting.store(false, Ordering::Release);
                    state.funding_ready.store(false, Ordering::Release);
                    state.qualified_models.write().unwrap().clear();
                    fixture.app = contracts::router(fixture.provider.state.clone());
                }
                for (alternate_index, alternate) in spellings.iter().enumerate() {
                    if alternate_index == index { continue; }
                    assert_ne!(alternate, accepted, "{name}");
                    let label = format!("{name}: alternate {alternate_index}, reopened {reopened}");
                    // Every attempt uses a fresh owner proof bound to its actual
                    // body. Equivalent bytes/hash do not authorize another string.
                    let (status, document) = fixture.submit_intent(alternate).await;
                    assert_eq!(status, StatusCode::CONFLICT, "{label}: {document}");
                    assert_eq!(document["error"]["code"], "conflict", "{label}");
                    assert_eq!(retained_records(), retained_before, "{label}");
                    assert_eq!(pending_record(), pending_before, "{label}");
                    assert!(fixture.rpc.requests.lock().unwrap().is_empty(), "{label}");

                    let (status, document) = fixture.submit_intent(accepted).await;
                    assert_eq!(status, StatusCode::OK, "{label}: {document}");
                    assert_eq!(document, acknowledgement, "{label}");
                    assert_eq!(retained_records(), retained_before, "{label}");
                    assert_eq!(pending_record(), pending_before, "{label}");
                    let ledger = fixture.persisted_without_credit();
                    assert_eq!(serde_json::to_value(ledger.quote(&pending.payload.id).unwrap()).unwrap(),
                        serde_json::to_value(&pending).unwrap(), "{label}");
                    assert!(fixture.rpc.requests.lock().unwrap().is_empty(), "{label}");
                }
            }
        }
    }
}

#[tokio::test]
async fn malformed_rlp_roots_preserve_fresh_funding_admission() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, shape) in [
            "byte-string-root", "missing-field", "extra-field",
            "zero-prefixed-root-length", "truncated-root-length", "overflowing-root-length",
        ].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 12_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let original = signed_payment(&fields, &fixture.owner_key, kind, &[0xc0]);
            let bytes = hex::decode(&original[2..]).unwrap();
            let payload = &bytes[usize::from(kind != 0)..];
            // Recipient plus commitment already exceed the short-list budget.
            assert!((0xf8..=0xfe).contains(&payload[0]));
            let rlp = Rlp::new(payload);
            let count = rlp.item_count().unwrap();
            let mut malformed = RlpStream::new();
            match shape {
                "byte-string-root" => { malformed.append(&payload); }
                "missing-field" => {
                    malformed.begin_list(count - 1);
                    for field in 0..count - 1 {
                        malformed.append_raw(rlp.at(field).unwrap().as_raw(), 1);
                    }
                }
                "extra-field" => {
                    malformed.begin_list(count + 1);
                    for field in 0..count {
                        malformed.append_raw(rlp.at(field).unwrap().as_raw(), 1);
                    }
                    // The valid signed prefix must not authorize an ignored trailer.
                    malformed.append(&0_u8);
                }
                "zero-prefixed-root-length" => {
                    malformed.append_raw(&[payload[0] + 1, 0], 0);
                    malformed.append_raw(&payload[1..], 1);
                }
                "truncated-root-length" => { malformed.append_raw(&[0xff, 1, 0], 1); }
                "overflowing-root-length" => {
                    let header = 1 + usize::from(payload[0] - 0xf7);
                    // Eight maximal length bytes overflow either decoding or header addition.
                    malformed.append_raw(&[0xff; 9], 0);
                    malformed.append_raw(&payload[header..], 1);
                }
                _ => unreachable!(),
            }
            let encoded = hex::encode(malformed.out());
            let candidate = if kind == 0 { format!("0x{encoded}") } else { format!("0x{kind:02x}{encoded}") };
            fixture.rejects_candidate_preserving_original(
                &original, &candidate, &format!("{envelope_name}: {shape}"),
            ).await;
        }
    }
}

#[tokio::test]
async fn nonminimal_nested_access_list_headers_preserve_original() {
    let directory = tempfile::tempdir().unwrap();
    let accepted = encoded_access_list(1, 1, false).out();
    let rlp = Rlp::new(&accepted);
    let entry = rlp.at(0).unwrap();
    let address = entry.at(0).unwrap();
    let key = entry.at(1).unwrap().at(0).unwrap();
    // The entry has 55 payload bytes; the outer list has 56, exercising both canonical headers.
    assert_eq!(entry.as_raw()[0], 0xf7);
    assert_eq!(&accepted[..2], &[0xf8, 56]);
    for (index, encoding) in [
        "zero-prefixed-list-length", "long-form-short-entry", "long-form-short-key",
    ].into_iter().enumerate() {
        let mut malformed = RlpStream::new();
        match encoding {
            "zero-prefixed-list-length" => {
                malformed.append_raw(&[0xf9, 0], 0);
                malformed.append_raw(&accepted[1..], 1);
            }
            "long-form-short-entry" => {
                malformed.begin_list(1);
                // Count the split header and body as one entry, without repairing its header.
                malformed.append_raw(&[0xf8, 55], 0);
                malformed.append_raw(&entry.as_raw()[1..], 1);
            }
            "long-form-short-key" => {
                let mut keys = RlpStream::new_list(1);
                keys.append_raw(&[0xb8, 32], 0);
                keys.append_raw(&key.as_raw()[1..], 1);
                malformed = single_access_entry(address.as_raw(), &keys.out());
            }
            _ => unreachable!(),
        }
        let rejected = malformed.out();
        for &(kind, envelope_name) in &ENVELOPES[1..] {
            let mut fixture = Fixture::new(directory.path(), 14_000 + u64::from(kind) * 100 + index as u64, 100).await;
            fixture.access_list_boundary(kind, &accepted, &rejected, &format!("{envelope_name}: {encoding}")).await;
        }
    }
}

#[tokio::test]
async fn malformed_root_child_lengths_cannot_authorize_a_valid_prefix() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, (encoding, tail)) in MALFORMED_RLP_CHILDREN.into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 15_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let original = signed_payment(&fields, &fixture.owner_key, kind, &[0xc0]);
            let bytes = hex::decode(&original[2..]).unwrap();
            let valid_list = Rlp::new(&bytes[usize::from(kind != 0)..]);
            let mut malformed = RlpStream::new();
            // The original fields and signature remain intact before the invalid extra child.
            append_list_with_malformed_tail(&mut malformed, &valid_list, tail);
            let encoded = hex::encode(malformed.out());
            let candidate = if kind == 0 { format!("0x{encoded}") } else { format!("0x{kind:02x}{encoded}") };
            fixture.rejects_candidate_preserving_original(
                &original, &candidate, &format!("{envelope_name}: root child {encoding}"),
            ).await;
        }
    }
}

#[tokio::test]
async fn malformed_nested_child_lengths_preserve_original_admission() {
    let directory = tempfile::tempdir().unwrap();
    let accepted = encoded_access_list(1, 1, false).out();
    let access_list = Rlp::new(&accepted);
    let entry = access_list.at(0).unwrap();
    let address = entry.at(0).unwrap();
    let keys = entry.at(1).unwrap();
    for (level, parent) in ["access-list", "entry", "key-list"].into_iter().enumerate() {
        for (index, (encoding, tail)) in MALFORMED_RLP_CHILDREN.into_iter().enumerate() {
            let mut malformed = RlpStream::new();
            match parent {
                "access-list" => append_list_with_malformed_tail(&mut malformed, &access_list, tail),
                "entry" => {
                    malformed.begin_list(1);
                    append_list_with_malformed_tail(&mut malformed, &entry, tail);
                }
                "key-list" => {
                    malformed.begin_list(1).begin_list(2).append_raw(address.as_raw(), 1);
                    append_list_with_malformed_tail(&mut malformed, &keys, tail);
                }
                _ => unreachable!(),
            }
            let rejected = malformed.out();
            for &(kind, envelope_name) in &ENVELOPES[1..] {
                let mut fixture = Fixture::new(directory.path(),
                    16_000 + u64::from(kind) * 100 + level as u64 * 10 + index as u64, 100).await;
                // Re-sign the full malformed access list, not merely its valid prefix.
                fixture.access_list_boundary(
                    kind, &accepted, &rejected, &format!("{envelope_name}: {parent} child {encoding}"),
                ).await;
            }
        }
    }
}

#[tokio::test]
async fn fresh_intent_rejects_resigned_nonminimal_byte_strings() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, encoding) in [
            "wrapped-single-byte", "long-form-single-byte",
            "long-form-short-recipient", "zero-prefixed-recipient-length",
        ].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 17_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let max_fee = match encoding {
                "wrapped-single-byte" => 127,
                "long-form-single-byte" => 128,
                _ => 11,
            };
            let fields = PaymentFields::opening(&fixture.quote.payload, 0, max_fee);
            let original = signed_payment(&fields, &fixture.owner_key, kind, &[0xc0]);
            let fee_field = usize::from(kind) + 1;
            let mut replacement = RlpStream::new();
            let field = match encoding {
                "wrapped-single-byte" => {
                    replacement.append_raw(&[0x81, u8::try_from(fields.max_fee).unwrap()], 1);
                    fee_field
                }
                "long-form-single-byte" => {
                    replacement.append_raw(&[0xb8, 1, u8::try_from(fields.max_fee).unwrap()], 1);
                    fee_field
                }
                _ => {
                    let length = u8::try_from(fields.recipient.len()).unwrap();
                    match encoding {
                        "long-form-short-recipient" => { replacement.append_raw(&[0xb8, length], 0); }
                        "zero-prefixed-recipient-length" => { replacement.append_raw(&[0xb9, 0, length], 0); }
                        _ => unreachable!(),
                    }
                    replacement.append_raw(&fields.recipient, 1);
                    fee_field + 2
                }
            };
            // Preserve the value/recipient, but sign the actual nonminimal field bytes.
            let candidate = resign_field(&original, kind, fields.chain_id, &fixture.owner_key, field, &replacement.out());
            fixture.rejects_candidate_preserving_original(
                &original, &candidate, &format!("{envelope_name}: {encoding}"),
            ).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_rechecks_provider_funding_gates_after_balance() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, (gate, gate_name)) in [
            (FundingGate::Accepting, "draining"),
            (FundingGate::Ready, "funding-readiness-lost"),
        ].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 18_000 + u64::from(kind) * 100 + index as u64, 100).await;
            let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let (raw, envelope) = fixture.original(kind, &fields);
            {
                let mut control = fixture.rpc.control.lock().unwrap();
                control.balance = u128::MAX;
                control.after_balance_gate = Some((fixture.provider.state.clone(), gate));
            }
            let name = format!("{envelope_name}: {gate_name}");
            // Both gates passed initially; close exactly one before the balance reply returns.
            fixture.refuses(&raw, &name, (StatusCode::SERVICE_UNAVAILABLE, "provider_draining"),
                Some("eth_getBalance")).await;
            gate.set(&fixture.provider.state, true);
            fixture.rpc.requests.lock().unwrap().clear();
            fixture.acknowledges(&raw, &envelope, &name).await;
        }
    }
}

#[tokio::test]
async fn failed_intent_persistence_rolls_back_the_entire_retention_handoff() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for (index, (point, event, contract_field)) in [
            ("intent-insert", "AFTER INSERT ON funding_intents", "NEW.contract"),
            ("proposal-delete", "AFTER DELETE ON quotes", "OLD.id"),
        ].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 19_000 + u64::from(kind) * 100 + index as u64, 100).await;
            fixture.rpc.control.lock().unwrap().balance = u128::MAX;
            let accepted_quote = fixture.quote.clone();
            let fields = PaymentFields::opening(&accepted_quote.payload, 0, 11);
            let (accepted_raw, accepted_envelope) = fixture.original(kind, &fields);
            fixture.acknowledges(&accepted_raw, &accepted_envelope, "earlier acceptance").await;
            let accepted_ack = fixture.provider.state.ledger
                .funding_intent(&accepted_quote.payload, &accepted_raw).unwrap().unwrap();

            let (status, document) = request(&mut fixture.app, "/v1/contracts/propose", &HeaderMap::new(),
                serde_json::to_vec(&json!({"offer_id":fixture.provider.offer_id,
                    "consumer_wallet":accepted_quote.payload.consumer_wallet,"funding_wei":"100"})).unwrap()).await;
            assert_eq!(status, StatusCode::OK, "{document}");
            fixture.quote = serde_json::from_value(document).unwrap();
            // Synthetic nonce advancement is not execution or credit of the earlier intent.
            fixture.rpc.control.lock().unwrap().view = ChainView {
                latest_nonce: 1, pending_nonce: 1, ..ChainView::default()
            };
            let fields = PaymentFields::opening(&fixture.quote.payload, 1, 11);
            let (raw, envelope) = fixture.original(kind, &fields);
            let db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let pending_before = db.query_row("SELECT record FROM quotes WHERE id=?1",
                [&fixture.quote.payload.id], |row| row.get::<_, String>(0)).unwrap();
            let retained_records = || {
                let mut statement = db.prepare(
                    "SELECT contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until
                     FROM funding_intents ORDER BY contract").unwrap();
                let records = statement.query_map([], |row| Ok((
                    row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?, row.get::<_, u64>(7)?,
                ))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
                records
            };
            let retained_before = retained_records();
            // FAIL leaves this statement's writes in the transaction. The production
            // transaction must roll them back, not rely on SQLite undoing the statement.
            db.execute_batch(&format!(
                "CREATE TRIGGER fail_registration {event} WHEN {contract_field}='{}'
                 BEGIN SELECT RAISE(FAIL,'injected registration persistence failure'); END;",
                fixture.quote.payload.id)).unwrap();
            fixture.rpc.requests.lock().unwrap().clear();
            let name = format!("{envelope_name}: {point}");
            fixture.refuses(&raw, &name, (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
                Some("eth_getBalance")).await;
            let pending_after = db.query_row("SELECT record FROM quotes WHERE id=?1",
                [&fixture.quote.payload.id], |row| row.get::<_, String>(0)).unwrap();
            assert_eq!(pending_after, pending_before, "{name}");
            assert_eq!(retained_records(), retained_before, "{name}");
            let reopened = fixture.persisted_without_credit();
            let recovered = reopened.funding_intent(&accepted_quote.payload, &accepted_raw).unwrap().unwrap();
            assert_eq!(recovered.payload, accepted_ack.payload, "{name}");
            assert_eq!(recovered.signature, accepted_ack.signature, "{name}");

            db.execute_batch("DROP TRIGGER fail_registration").unwrap();
            fixture.rpc.requests.lock().unwrap().clear();
            fixture.acknowledges(&raw, &envelope, &name).await;
            assert!(retained_records().contains(&retained_before[0]), "{name}");
            let pending_count: u64 = db.query_row("SELECT COUNT(*) FROM quotes WHERE id=?1",
                [&fixture.quote.payload.id], |row| row.get(0)).unwrap();
            assert_eq!(pending_count, 0, "{name}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_authenticated_writers_compete_for_the_final_paid_lease() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 50_000, 100).await;
    let unrelated_quote = fixture.quote.clone();
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();

    // Restore 9,999 complete, internally consistent accepted originals in one fixture
    // transaction. Their historical spendability/owner admission is assumed, not replayed.
    // Unlike capacity-only placeholder rows, every quote, payment and acknowledgement is signed.
    let retained_key = format!("{:064x}", 123_456);
    let retained_payer = wallet::address_from_private_key(&retained_key).unwrap();
    {
        let tx = db.transaction().unwrap();
        {
            let mut insert = tx.prepare(
                "INSERT INTO funding_intents(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)").unwrap();
            for sequence in 1..=9_999_u64 {
                let mut payload = unrelated_quote.payload.clone();
                payload.id = format!("0x{:064x}", 0xfeed_0000 + sequence);
                payload.nonce = format!("0x{:064x}", 0xcafe_0000 + sequence);
                payload.consumer_wallet = retained_payer.clone();
                let quote = protocol::sign(protocol::QUOTE_DOMAIN, payload, &fixture.provider.state.private_key).unwrap();
                let raw = signed_payment(&PaymentFields::opening(&quote.payload, sequence, 11),
                    &retained_key, 0, &[0xc0]);
                let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw,
                    &retained_payer, &quote.payload.provider_wallet, &quote.payload.funding_wei,
                    &protocol::opening_data(&quote.payload).unwrap()).unwrap();
                let ack = protocol::sign(protocol::INTENT_DOMAIN, FundingIntent {
                    network: quote.payload.network.clone(), provider_wallet: quote.payload.provider_wallet.clone(),
                    consumer_wallet: retained_payer.clone(), quote_id: quote.payload.id.clone(),
                    tx_hash: envelope.tx_hash.clone(), registered_at: quote.payload.issued_at,
                }, &fixture.provider.state.private_key).unwrap();
                insert.execute(params![quote.payload.id, envelope.tx_hash, raw,
                    serde_json::to_string(&ack).unwrap(), serde_json::to_string(&quote).unwrap(),
                    retained_payer, sequence.to_string(), quote.payload.expires_at]).unwrap();
            }
        }
        tx.commit().unwrap();
    }

    // Issue both competing quotes after fixture restoration. Keep the unrelated signed
    // proposal intact, and do not mistake seed generation time for a fresh chain observation.
    fixture.rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
    let (status, document) = request(&mut fixture.app, "/v1/contracts/propose", &HeaderMap::new(),
        serde_json::to_vec(&json!({"offer_id":fixture.provider.offer_id,
            "consumer_wallet":fixture.rpc.payer,"funding_wei":"100"})).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    fixture.quote = serde_json::from_value(document).unwrap();
    let second_key = format!("{:064x}", 120_001);
    let second_payer = wallet::address_from_private_key(&second_key).unwrap();
    assert_ne!(second_payer, fixture.rpc.payer);
    let second_rpc = Arc::new(SpendabilityRpc {
        network: fixture.quote.payload.network.clone(), payer: second_payer.clone(),
        observed_at: AtomicU64::new(protocol::now().unwrap()),
        control: Mutex::new(RpcControl::default()), requests: Mutex::new(Vec::new()),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = &fixture.provider.state;
    let mut config = (*source.config).clone();
    config.network.rpc_url = format!("http://{}", listener.local_addr().unwrap());
    let mut second_state = Arc::new(ProviderState {
        chain: Arc::new(ChainClient::new(config.network.clone(), config.data_dir.clone()).unwrap()),
        ledger: Arc::new(BillingStore::open(&fixture.provider.ledger_path, &config.network.identity).unwrap()),
        config: Arc::new(config), private_key: source.private_key.clone(), wallet: source.wallet.clone(),
        offers: source.offers.clone(), accepting: AtomicBool::new(true), funding_ready: AtomicBool::new(true),
        qualified_models: std::sync::RwLock::new(source.qualified_models.read().unwrap().clone()),
        model_slots: source.model_slots.clone(), managed_engines: source.managed_engines.clone(),
    });
    let rpc_app = Router::new().route("/", post(rpc_reply)).with_state(second_rpc.clone());
    let second_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });
    let mut second_app = contracts::router(second_state.clone());
    let (status, document) = request(&mut second_app, "/v1/contracts/propose", &HeaderMap::new(),
        serde_json::to_vec(&json!({"offer_id":fixture.provider.offer_id,
            "consumer_wallet":second_payer,"funding_wei":"100"})).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let second_quote: Signed<Quote> = serde_json::from_value(document).unwrap();
    let (first_raw, first_envelope) = fixture.original(0, &PaymentFields::opening(&fixture.quote.payload, 0, 11));
    let second_raw = signed_payment(&PaymentFields::opening(&second_quote.payload, 0, 11), &second_key, 2, &[0xc0]);
    let second_envelope = second_state.chain.validate_payment_envelope(&second_raw, &second_payer,
        &second_quote.payload.provider_wallet, &second_quote.payload.funding_wei,
        &protocol::opening_data(&second_quote.payload).unwrap()).unwrap();

    // Hash every retained column in sorted order without keeping 10,000 large records
    // in Rust memory. Candidate rows are checked individually below, not excluded from proof.
    let retained_digest = || {
        let mut hash = Keccak::v256();
        let mut count = 0;
        let mut statement = db.prepare(
            "SELECT contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until
             FROM funding_intents WHERE contract NOT IN (?1,?2) ORDER BY contract").unwrap();
        let mut rows = statement.query(params![fixture.quote.payload.id, second_quote.payload.id]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let record = (row.get::<_, String>(0).unwrap(), row.get::<_, String>(1).unwrap(),
                row.get::<_, String>(2).unwrap(), row.get::<_, String>(3).unwrap(),
                row.get::<_, String>(4).unwrap(), row.get::<_, String>(5).unwrap(),
                row.get::<_, String>(6).unwrap(), row.get::<_, u64>(7).unwrap());
            hash.update(&serde_json::to_vec(&record).unwrap());
            count += 1;
        }
        let mut digest = [0; 32];
        hash.finalize(&mut digest);
        (count, digest)
    };
    let retained_before = retained_digest();
    assert_eq!(retained_before.0, 9_999);
    assert_eq!(fixture.provider.state.ledger.admin_summary().unwrap()["active_funding_intents"], 9_999);
    let pending = |id: &str| db.query_row("SELECT record FROM quotes WHERE id=?1", [id],
        |row| row.get::<_, String>(0)).unwrap();
    let pending_before = [pending(&fixture.quote.payload.id), pending(&second_quote.payload.id)];

    // Independent Tokio tasks and independent SQLite/chain clients; neither request
    // can finish spendability before the other reaches its balance observation.
    let barrier = Arc::new(Barrier::new(2));
    for rpc in [&fixture.rpc, &second_rpc] {
        let mut control = rpc.control.lock().unwrap();
        control.balance = u128::MAX;
        control.balance_barrier = Some(barrier.clone());
    }
    let first = tokio::spawn({
        let mut app = fixture.app.clone();
        let quote = fixture.quote.clone();
        let key = fixture.owner_key.clone();
        let raw = first_raw.clone();
        async move { authorized_intent(&mut app, &quote, &key, &raw).await }
    });
    let second = tokio::spawn({
        let mut app = second_app.clone();
        let quote = second_quote.clone();
        let key = second_key.clone();
        let raw = second_raw.clone();
        async move { authorized_intent(&mut app, &quote, &key, &raw).await }
    });
    let (first, second) = tokio::join!(first, second);
    let results = [first.unwrap(), second.unwrap()];
    let winner = match (results[0].0, results[1].0) {
        (StatusCode::OK, StatusCode::TOO_MANY_REQUESTS) => 0,
        (StatusCode::TOO_MANY_REQUESTS, StatusCode::OK) => 1,
        other => panic!("exactly one final lease must commit: {other:?}, {results:?}"),
    };
    let loser = 1 - winner;
    assert_eq!(results[loser].1["error"]["code"], "rate_limit");
    for rpc in [&fixture.rpc, &second_rpc] {
        let requests = rpc.requests.lock().unwrap();
        assert_eq!(requests.iter().filter(|call| call["method"] == "eth_getBalance").count(), 1);
        assert_eq!(requests.iter().filter(|call| call["method"] == "eth_getTransactionCount").count(), 4,
            "both originals must pass initial and post-balance nonce observations");
        assert!(requests.iter().all(|call| call["method"] != "eth_sendRawTransaction"));
    }
    let quotes = [&fixture.quote, &second_quote];
    let raws = [&first_raw, &second_raw];
    let envelopes = [&first_envelope, &second_envelope];
    let ack: Signed<FundingIntent> = serde_json::from_value(results[winner].1.clone()).unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &quotes[winner].payload.provider_wallet).unwrap();
    assert_eq!(ack.payload.network, quotes[winner].payload.network);
    assert_eq!(ack.payload.provider_wallet, quotes[winner].payload.provider_wallet);
    assert_eq!(ack.payload.consumer_wallet, quotes[winner].payload.consumer_wallet);
    assert_eq!(ack.payload.quote_id, quotes[winner].payload.id);
    assert_eq!(ack.payload.tx_hash, envelopes[winner].tx_hash);
    assert!((quotes[winner].payload.issued_at..quotes[winner].payload.expires_at).contains(&ack.payload.registered_at));
    let read_winner = || db.query_row(
        "SELECT tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until FROM funding_intents WHERE contract=?1",
        [&quotes[winner].payload.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?,
            row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)));
    let winner_record: (String, String, String, String, String, String, u64) = read_winner().unwrap();
    assert_eq!(winner_record, (envelopes[winner].tx_hash.clone(), raws[winner].clone(),
        serde_json::to_string(&ack).unwrap(), serde_json::to_string(quotes[winner]).unwrap(),
        quotes[winner].payload.consumer_wallet.clone(), "0".into(), quotes[winner].payload.expires_at));
    assert_eq!(pending(&quotes[loser].payload.id), pending_before[loser]);
    let loser_rows: u64 = db.query_row(
        "SELECT COUNT(*) FROM funding_intents WHERE contract=?1 OR tx_hash=?2 OR (sender=?3 AND nonce='0')",
        params![quotes[loser].payload.id, envelopes[loser].tx_hash, quotes[loser].payload.consumer_wallet],
        |row| row.get(0)).unwrap();
    assert_eq!(loser_rows, 0, "full refusal must not assign any part of the losing original");
    let winner_pending: u64 = db.query_row("SELECT COUNT(*) FROM quotes WHERE id=?1",
        [&quotes[winner].payload.id], |row| row.get(0)).unwrap();
    assert_eq!(winner_pending, 0, "only the winner's standalone proposal transfers to retained acceptance");
    assert_eq!(retained_digest(), retained_before);

    // A fresh proof cannot turn the losing quote into acceptance while capacity is full.
    let refused = if loser == 0 {
        authorized_intent(&mut fixture.app, &fixture.quote, &fixture.owner_key, &first_raw).await
    } else {
        authorized_intent(&mut second_app, &second_quote, &second_key, &second_raw).await
    };
    assert_eq!(refused.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.1["error"]["code"], "rate_limit");
    assert_eq!(pending(&quotes[loser].payload.id), pending_before[loser]);

    // Reopen both ledger/chain objects and routers with all fresh-admission gates closed.
    fixture.app = Router::new();
    drop(second_app);
    for state in [&mut fixture.provider.state, &mut second_state] {
        let state = Arc::get_mut(state).unwrap();
        state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
        state.accepting.store(false, Ordering::Release);
        state.funding_ready.store(false, Ordering::Release);
        state.qualified_models.write().unwrap().clear();
    }
    fixture.app = contracts::router(fixture.provider.state.clone());
    let mut second_app = contracts::router(second_state.clone());
    fixture.server.abort();
    second_server.abort();
    let rpc_counts = [fixture.rpc.requests.lock().unwrap().len(), second_rpc.requests.lock().unwrap().len()];
    let recovered = if winner == 0 {
        authorized_intent(&mut fixture.app, &fixture.quote, &fixture.owner_key, &first_raw).await
    } else {
        authorized_intent(&mut second_app, &second_quote, &second_key, &second_raw).await
    };
    assert_eq!(recovered, (StatusCode::OK, results[winner].1.clone()));
    assert_eq!(read_winner().unwrap(), winner_record, "reopening and cached recovery preserve every accepted field");
    assert_eq!([fixture.rpc.requests.lock().unwrap().len(), second_rpc.requests.lock().unwrap().len()], rpc_counts);
    assert_eq!(retained_digest(), retained_before);
    let ledger = BillingStore::open(&fixture.provider.ledger_path, &fixture.quote.payload.network).unwrap();
    assert_eq!(ledger.admin_summary().unwrap()["active_funding_intents"], 10_000);
    assert_eq!(serde_json::to_value(ledger.quote(&unrelated_quote.payload.id).unwrap()).unwrap(),
        serde_json::to_value(&unrelated_quote).unwrap());
    for index in 0..2 {
        assert_eq!(serde_json::to_value(ledger.quote(&quotes[index].payload.id).unwrap()).unwrap(),
            serde_json::to_value(quotes[index]).unwrap());
        let stored = ledger.funding_intent(&quotes[index].payload, raws[index]).unwrap();
        assert_eq!(serde_json::to_value(stored).unwrap(),
            if index == winner { results[winner].1.clone() } else { Value::Null });
    }
    let accounts: (u64, u64, u64, u64) = db.query_row(
        "SELECT (SELECT COUNT(*) FROM accounts),(SELECT COUNT(*) FROM credentials),
                (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations)", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
    assert_eq!(accounts, (0, 0, 0, 0));
    for rpc in [&fixture.rpc, &second_rpc] {
        assert!(rpc.requests.lock().unwrap().iter().all(|call| call["method"] != "eth_sendRawTransaction"));
    }
}
