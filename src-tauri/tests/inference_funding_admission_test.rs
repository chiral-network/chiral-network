//! Local payment-envelope validation and fresh funding admission through production APIs.
//! HTTP fixtures use isolated ledgers and synthetic RPC, not live-chain or engine qualification.
//! All envelopes are independently signed with public test scalars; nothing is broadcast.
#[path = "support/proposal.rs"]
mod proposal;
#[path = "support/funding.rs"]
mod funding;

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{from_fn, Next},
    response::Response,
    routing::post,
    Json, Router,
};
use chiral_network::{
    app::ProviderState,
    auth::{self, AuthPayload},
    billing_store::{BillingStore, CredentialIssueRequest},
    chain_client::{ChainClient, ChainError, ValidatedPaymentEnvelope, VerifiedPayment},
    consumer::{Consumer, ConsumerConfig},
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
use std::{net::TcpListener, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex}};
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

struct TopupCapacityRpc {
    admission: Arc<SpendabilityRpc>,
    envelope: ValidatedPaymentEnvelope,
    phase: AtomicU64,
}

async fn topup_capacity_rpc(State(state): State<Arc<TopupCapacityRpc>>, Json(request): Json<Value>) -> Json<Value> {
    let phase = state.phase.load(Ordering::Acquire);
    let result = match request["method"].as_str().unwrap() {
        "eth_getTransactionReceipt" => {
            assert_eq!(request["params"], json!([state.envelope.tx_hash]));
            if phase == 0 { Value::Null } else {
                let height = if phase == 1 { 10_u64 } else { 8 };
                json!({"transactionHash":state.envelope.tx_hash,"from":state.envelope.from,
                    "to":state.envelope.to,"type":"0x0","gasUsed":"0x10000","status":"0x1",
                    "blockNumber":format!("0x{height:x}"),"blockHash":format!("0x{height:064x}")})
            }
        }
        "eth_getBlockByNumber" if request["params"][0] == "0x8" => {
            assert_eq!(request["params"], json!(["0x8", false]));
            json!({"number":"0x8","hash":format!("0x{:064x}", 8),
                "timestamp":format!("0x{:x}", state.admission.observed_at.load(Ordering::Acquire))})
        }
        "eth_chainId" | "eth_syncing" | "eth_getBlockByNumber" =>
            return rpc_reply(State(state.admission.clone()), Json(request)).await,
        other => panic!("top-up recovery must not perform admission or submission RPC: {other}"),
    };
    state.admission.requests.lock().unwrap().push(request.clone());
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

// Each payload byte is itself a complete canonical RLP item. Reinterpreting the
// enclosing bytes as a list therefore isolates field type, not malformed framing.
fn list_of_byte_items(bytes: &[u8]) -> RlpStream {
    let mut list = RlpStream::new_list(bytes.len());
    for byte in bytes {
        assert!(*byte < 0x80);
        list.append_raw(std::slice::from_ref(byte), 1);
    }
    list
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

async fn owner_headers(app: &mut Router, quote: &Quote, key: &str, path: &str, body: &[u8]) -> HeaderMap {
    let body_hash = protocol::hash_hex(body);
    let (status, document) = request(app, "/v1/auth/challenge", &HeaderMap::new(),
        serde_json::to_vec(&json!({"wallet":quote.consumer_wallet,
            "method":"POST","path":path,"body_hash":body_hash})).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let challenge: Challenge = serde_json::from_value(document).unwrap();
    let proof = AuthPayload { network: quote.network.clone(),
        provider_wallet: quote.provider_wallet.clone(), wallet: quote.consumer_wallet.clone(),
        method: "POST".into(), path: path.into(), body_hash,
        nonce: challenge.nonce.clone(), expires_at: challenge.expires_at };
    let payload = auth::challenge_payload(&proof).unwrap();
    assert_eq!(hex::decode(challenge.payload_hex.strip_prefix("0x").unwrap()).unwrap(), payload);
    let signature = wallet::sign_message(key, &payload).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("x-chiral-owner", proof.wallet.parse().unwrap());
    headers.insert("x-chiral-nonce", proof.nonce.parse().unwrap());
    headers.insert("x-chiral-signature", signature.parse().unwrap());
    headers
}

async fn authorized_intent(app: &mut Router, quote: &Signed<Quote>, key: &str, raw: &str) -> (StatusCode, Value) {
    let path = format!("/v1/contracts/{}/funding-intent", quote.payload.id);
    let body = serde_json::to_vec(&json!({"raw_transaction":raw})).unwrap();
    let headers = owner_headers(app, &quote.payload, key, &path, &body).await;
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
async fn fresh_intent_rejects_resigned_list_valued_integers() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, envelope_name) in ENVELOPES {
        let fields: &[(&str, usize)] = match kind {
            0 => &[("nonce", 0), ("gas-price", 1), ("gas-limit", 2), ("value", 4)],
            1 => &[("chain-id", 0), ("nonce", 1), ("gas-price", 2), ("gas-limit", 3), ("value", 5)],
            2 => &[("chain-id", 0), ("nonce", 1), ("priority-fee", 2), ("maximum-fee", 3),
                ("gas-limit", 4), ("value", 6)],
            _ => unreachable!(),
        };
        for (case, &(name, index)) in fields.iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(), 18_000 + u64::from(kind) * 100 + case as u64, 100).await;
            let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
            let original = signed_payment(&fields, &fixture.owner_key, kind, &[0xc0]);
            let bytes = hex::decode(&original[2..]).unwrap();
            let rlp = Rlp::new(&bytes[usize::from(kind != 0)..]);
            let field = rlp.at(index).unwrap();
            let replacement = list_of_byte_items(field.data().unwrap()).out();
            // Preserve each numerical payload, but sign the actual list-valued field.
            // In particular, legacy gas price 11 is the retained c1 0b reproduction.
            let candidate = resign_field(&original, kind, fields.chain_id, &fixture.owner_key, index, &replacement);
            fixture.rejects_candidate_preserving_original(
                &original, &candidate, &format!("{envelope_name}: list-valued {name}"),
            ).await;
        }
    }
}

#[tokio::test]
async fn fresh_intent_rejects_list_encoded_typed_signature_parity() {
    let directory = tempfile::tempdir().unwrap();
    for kind in [1, 2] {
        let mut fixture = Fixture::new(directory.path(), 18_500 + u64::from(kind), 100).await;
        let fields = PaymentFields::opening(&fixture.quote.payload, 0, 11);
        let original = signed_payment(&fields, &fixture.owner_key, kind, &[0xc0]);
        let bytes = hex::decode(&original[2..]).unwrap();
        let rlp = Rlp::new(&bytes[1..]);
        let count = rlp.item_count().unwrap();
        let parity_index = count - 3;
        let parity = rlp.at(parity_index).unwrap();
        let replacement = list_of_byte_items(parity.data().unwrap()).out();
        let mut candidate = RlpStream::new_list(count);
        for index in 0..count {
            candidate.append_raw(
                if index == parity_index { &replacement } else { rlp.at(index).unwrap().as_raw() }, 1,
            );
        }
        // Parity is outside the signing preimage. Keep its numerical value and
        // the original r/s, rather than masking field type with an invalid signature.
        let candidate = format!("0x{kind:02x}{}", hex::encode(candidate.out()));
        fixture.rejects_candidate_preserving_original(
            &original, &candidate, &format!("type-{kind}: list-valued parity"),
        ).await;
    }
}

#[test]
fn local_envelope_rejects_list_valued_recipient_and_calldata() {
    let directory = tempfile::tempdir().unwrap();
    let client = ChainClient::new(NetworkConfig {
        identity: NetworkIdentity {
            chain_id: ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: ESPACE_TESTNET_GENESIS.into(),
            deployment_id: "opaque-field-shape-test".into(),
        },
        rpc_url: "http://127.0.0.1:1".into(),
        rpc_bearer_file: None, faucet_url: None, explorer_url: None, policy_url: None,
    }, directory.path().into()).unwrap();
    let key = format!("{:064x}", 68_001);
    let payer = wallet::address_from_private_key(&key).unwrap();
    for (kind, envelope_name) in ENVELOPES {
        for shape in ["recipient", "empty-calldata", "calldata"] {
            let fields = PaymentFields {
                chain_id: ESPACE_TESTNET_CHAIN_ID, nonce: 0, recipient: vec![0; 20], value: 100,
                data: if shape == "empty-calldata" { Vec::new() } else { vec![0, 1, 0x7f] },
                gas: GAS, max_fee: 11, priority_fee: 2,
            };
            let payee = format!("0x{}", hex::encode(&fields.recipient));
            let validate = |raw: &str| client.validate_payment_envelope(raw, &payer, &payee, "100", &fields.data);
            let original = signed_payment(&fields, &key, kind, &[0xc0]);
            let expected = validate(&original).unwrap();
            assert_eq!(expected.to, payee);
            assert_eq!(expected.data, fields.data);
            assert_eq!(expected.tx_hash, protocol::hash_hex(&hex::decode(&original[2..]).unwrap()));
            let recipient_index = usize::from(kind) + 3;
            let (index, payload) = if shape == "recipient" {
                (recipient_index, &fields.recipient)
            } else {
                (recipient_index + 2, &fields.data)
            };
            let candidate = resign_field(&original, kind, fields.chain_id, &key, index,
                &list_of_byte_items(payload).out());
            assert!(matches!(validate(&candidate), Err(ChainError::Rejected(_))), "{envelope_name}: {shape}");
            assert_eq!(validate(&original).unwrap(), expected, "{envelope_name}: {shape}");
        }
    }
}

#[tokio::test]
async fn fresh_intent_rejects_list_valued_access_list_addresses() {
    let directory = tempfile::tempdir().unwrap();
    let original = encoded_access_list(1, 0, true).out();
    // Twenty zero-byte children preserve the exact address payload and fit depth four.
    // Neither an address-width failure nor the nesting guard explains rejection.
    let candidate = single_access_entry(&list_of_byte_items(&[0; 20]).out(), &[0xc0]).out();
    for kind in [1, 2] {
        let mut fixture = Fixture::new(directory.path(), 18_600 + u64::from(kind), 100).await;
        fixture.access_list_boundary(kind, &original, &candidate, "list-valued access address").await;
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

    funding::restore_retained_intents(&mut db, &fixture.provider, &fixture.quote, 9_999, |quote, key, sequence| {
        let raw = signed_payment(&PaymentFields::opening(quote, sequence, 11), key, 0, &[0xc0]);
        let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw,
            &quote.consumer_wallet, &quote.provider_wallet, &quote.funding_wei,
            &protocol::opening_data(quote).unwrap()).unwrap();
        (raw, envelope)
    });

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
        offers: source.offers.clone(), accepting: AtomicBool::new(true), settlement_fault: Default::default(), funding_ready: AtomicBool::new(true),
        qualified_models: std::sync::RwLock::new(source.qualified_models.read().unwrap().clone()),
        embedding_dimensions: std::sync::RwLock::new(source.embedding_dimensions.read().unwrap().clone()),
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
    let read_loser_count = || db.query_row(
        "SELECT COUNT(*) FROM funding_intents WHERE contract=?1 OR tx_hash=?2 OR (sender=?3 AND nonce='0')",
        params![quotes[loser].payload.id, envelopes[loser].tx_hash, quotes[loser].payload.consumer_wallet],
        |row| row.get::<_, u64>(0)).unwrap();
    assert_eq!(read_loser_count(), 0, "full refusal must not assign any part of the losing original");
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

    // Drive the same losing original through the native client, not another manually
    // authorized router call. Observe unmodified HTTP replies and both RPC boundaries.
    {
        let exchanges = Arc::new(Mutex::new(Vec::new()));
        let observed = exchanges.clone();
        let native_app = if loser == 0 { fixture.app.clone() } else { second_app.clone() }
            .layer(from_fn(move |request: Request, next: Next| {
                let observed = observed.clone();
                async move {
                    let path = request.uri().path().to_owned();
                    let (parts, body) = request.into_parts();
                    let body = to_bytes(body, 96 * 1024).await.unwrap();
                    let requested = serde_json::from_slice::<Value>(&body).unwrap();
                    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
                    let (parts, body) = response.into_parts();
                    let body = to_bytes(body, 96 * 1024).await.unwrap();
                    let replied = serde_json::from_slice::<Value>(&body).unwrap();
                    observed.lock().unwrap().push((path, requested, parts.status, replied));
                    Response::from_parts(parts, Body::from(body))
                }
            }));
        fixture._listeners[0].set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let native_server = tokio::spawn(async move {
            axum::serve(listener, native_app).with_graceful_shutdown(async move {
                stopped.await.unwrap();
            }).await.unwrap();
        });
        let client_calls = Arc::new(Mutex::new(Vec::new()));
        let observed_calls = client_calls.clone();
        let rpc_app = Router::new().route("/", post(move |Json(request): Json<Value>| {
            let observed_calls = observed_calls.clone();
            async move {
                observed_calls.lock().unwrap().push(request);
                StatusCode::SERVICE_UNAVAILABLE
            }
        }));
        let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut network = fixture.provider.state.config.network.clone();
        network.rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
        let client_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });
        let config = ConsumerConfig { network, data_dir: directory.path().join("native-losing-payer"),
            catalog_url: None, development: true, dht: None };
        let quote = quotes[loser];
        {
            let consumer = Consumer::new(config.clone()).unwrap();
            let key = if loser == 0 { &fixture.owner_key } else { &second_key };
            let import = config.data_dir.join("public-test-scalar.key");
            wallet::save_private_key(&import, key).unwrap();
            assert_eq!(consumer.wallet_import_file(&import).unwrap(), quote.payload.consumer_wallet);
        }
        // Restore the already prepared original at the journal boundary. This is not
        // live preparation or chain credit, and the client RPC cannot manufacture either.
        let scoped = config.network.scoped_dir(&config.data_dir);
        let journal = Connection::open(scoped.join("wallet-journal/transactions.sqlite3")).unwrap();
        journal.execute(
            "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at)
             VALUES (?1,?2,?3,?4,?5,'prepared',?6)",
            params![quote.payload.consumer_wallet, format!("open:{}", quote.payload.id),
                envelopes[loser].nonce.to_string(), envelopes[loser].tx_hash, raws[loser],
                quote.payload.issued_at]).unwrap();
        let journal_rows = || {
            let mut statement = journal.prepare("SELECT * FROM transactions ORDER BY sender,operation").unwrap();
            let columns = statement.column_count();
            statement.query_map([], |row| (0..columns)
                .map(|index| row.get::<_, rusqlite::types::Value>(index))
                .collect::<rusqlite::Result<Vec<_>>>()).unwrap()
                .collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        let journal_before = journal_rows();
        assert_eq!(journal_before.len(), 1);
        let account_path = scoped.join("consumer").join(&quote.payload.consumer_wallet)
            .join(format!("{}.json", hex::encode(protocol::digest(quote.payload.id.as_bytes()))));
        assert!(!account_path.exists());
        let mut saved_before = None;
        let provider_rpc = if loser == 0 { &fixture.rpc } else { &second_rpc };
        let intent_path = format!("/v1/contracts/{}/funding-intent", quote.payload.id);
        for resume in [false, true, false] {
            // Initial fund, then reopened resume and repeated fund must preserve the
            // original operation. A refusal must never become an implicit paid retry.
            let consumer = Consumer::new(config.clone()).unwrap();
            exchanges.lock().unwrap().clear();
            let requests_before = provider_rpc.requests.lock().unwrap().len();
            let error = if resume {
                consumer.resume(&quote.payload.id).await.unwrap_err()
            } else {
                consumer.fund(&fixture.provider.offer, quote).await.unwrap_err()
            };
            assert!(error.starts_with("rate_limit:") && error.ends_with("(HTTP 429)"), "{error}");
            let observed = exchanges.lock().unwrap();
            assert_eq!(observed.len(), 2, "one owner proof and one intent, no automatic retry or replacement quote");
            assert_eq!(observed[0].0, "/v1/auth/challenge");
            assert_eq!(observed[0].1["wallet"], quote.payload.consumer_wallet);
            assert_eq!(observed[0].1["path"], intent_path);
            assert_eq!(observed[0].2, StatusCode::OK);
            assert_eq!(observed[1].0, intent_path);
            assert_eq!(observed[1].1, json!({"raw_transaction":raws[loser]}));
            assert_eq!(observed[1].2, StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(observed[1].3["error"]["code"], "rate_limit");
            drop(observed);
            let calls = provider_rpc.requests.lock().unwrap();
            let admitted_calls = &calls[requests_before..];
            assert_eq!(admitted_calls.iter().filter(|call| call["method"] == "eth_getBalance").count(), 1);
            assert_eq!(admitted_calls.iter().filter(|call| call["method"] == "eth_getTransactionCount").count(), 4);
            assert!(calls.iter().all(|call| call["method"] != "eth_sendRawTransaction"));
            drop(calls);
            assert!(client_calls.lock().unwrap().is_empty(), "refused native funding must not reach chain RPC");
            let bytes = std::fs::read(&account_path).unwrap();
            let saved: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(saved["quote"], serde_json::to_value(quote).unwrap());
            assert_eq!(saved["raw_transaction"], *raws[loser]);
            assert_eq!(saved["tx_hash"], envelopes[loser].tx_hash);
            assert!(saved["funding_intent"].is_null());
            assert_eq!(saved["funding_broadcast_started"], false);
            assert!(saved["account"].is_null());
            assert_eq!(saved["credentials"], json!([]));
            assert!(saved["active_credential"].is_null());
            if let Some(before) = &saved_before { assert_eq!(&bytes, before); }
            else { saved_before = Some(bytes); }
            let pending = consumer.pending_accounts().unwrap();
            assert_eq!(pending["pending"][0]["contract_id"], quote.payload.id);
            assert_eq!(pending["pending"][0]["tx_hash"], envelopes[loser].tx_hash);
            assert_eq!(pending["pending"][0]["funding_stage"], "registration_pending");
            assert_eq!(journal_rows(), journal_before, "no replacement operation, nonce or submission state");
            assert_eq!(read_loser_count(), 0);
            assert_eq!(read_winner().unwrap(), winner_record);
            assert_eq!(retained_digest(), retained_before);
        }
        assert_eq!(pending(&quote.payload.id), pending_before[loser]);
        assert_eq!(fixture.provider.state.ledger.admin_summary().unwrap()["active_funding_intents"], 10_000);
        stop.send(()).unwrap();
        native_server.await.unwrap();
        client_server.abort();
        assert!(client_server.await.unwrap_err().is_cancelled());
    }

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

#[tokio::test]
async fn native_lost_fresh_acknowledgement_recovers_after_provider_and_client_reopen() {
    let directory = tempfile::tempdir().unwrap();
    for (index, repeat_fund) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 51_000 + index as u64, 100).await;
        fixture.rpc.control.lock().unwrap().balance = u128::MAX;
        let (status, unrelated) = request(&mut fixture.app, "/v1/contracts/propose", &HeaderMap::new(),
            serde_json::to_vec(&json!({"offer_id":fixture.provider.offer_id,
                "consumer_wallet":fixture.quote.payload.consumer_wallet,"funding_wei":"101"})).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let unrelated: Signed<Quote> = serde_json::from_value(unrelated).unwrap();
        let quote = &fixture.quote;
        let (raw, envelope) = fixture.original(if repeat_fund { 2 } else { 0 },
            &PaymentFields::opening(&quote.payload, 0, 11));
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let retained = || db.query_row(
            "SELECT contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until
             FROM funding_intents WHERE contract=?1", [&quote.payload.id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?,
                row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, u64>(7)?))).unwrap();
        let pending = |id: &str| db.query_row("SELECT record FROM quotes WHERE id=?1", [id],
            |row| row.get::<_, String>(0)).unwrap();
        let unrelated_before = pending(&unrelated.payload.id);
        assert!(fixture.provider.state.ledger.funding_intent(&quote.payload, &raw).unwrap().is_none());
        assert_eq!(fixture.provider.state.ledger.admin_summary().unwrap()["active_funding_intents"], 0);

        let exchanges = Arc::new(Mutex::new(Vec::new()));
        let lose_next = Arc::new(AtomicBool::new(true));
        let listener = fixture._listeners[0].try_clone().unwrap();
        listener.set_nonblocking(true).unwrap();
        let serve = |app: Router| {
            let observed = exchanges.clone();
            let lose_next = lose_next.clone();
            let app = app.layer(from_fn(move |request: Request, next: Next| {
                let observed = observed.clone();
                let lose_next = lose_next.clone();
                async move {
                    let path = request.uri().path().to_owned();
                    let (parts, body) = request.into_parts();
                    let body = to_bytes(body, 96 * 1024).await.unwrap();
                    let requested = serde_json::from_slice::<Value>(&body).unwrap();
                    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
                    let (mut parts, body) = response.into_parts();
                    let body = to_bytes(body, 96 * 1024).await.unwrap();
                    let replied = serde_json::from_slice::<Value>(&body).unwrap();
                    let intent = path.ends_with("/funding-intent");
                    if intent { assert_eq!(parts.status, StatusCode::OK, "{replied}"); }
                    observed.lock().unwrap().push((path, requested, parts.status, replied));
                    if intent && lose_next.swap(false, Ordering::AcqRel) {
                        // The production route has committed and serialized the genuine
                        // acknowledgement. Disconnect its body, not an invented refusal.
                        parts.headers.remove(header::CONTENT_LENGTH);
                        return Response::from_parts(parts, Body::from_stream(
                            futures_util::stream::once(async {
                                Err::<axum::body::Bytes, _>(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
                            })));
                    }
                    Response::from_parts(parts, Body::from(body))
                }
            }));
            let listener = tokio::net::TcpListener::from_std(listener.try_clone().unwrap()).unwrap();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).with_graceful_shutdown(async move {
                    stopped.await.unwrap();
                }).await.unwrap();
            });
            (stop, server)
        };
        let (stop, server) = serve(fixture.app.clone());
        let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut network = fixture.provider.state.config.network.clone();
        network.rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
        let config = ConsumerConfig { network, data_dir: directory.path().join(format!("lost-reply-{index}")),
            catalog_url: None, development: true, dht: None };
        let consumer = Consumer::new(config.clone()).unwrap();
        let import = config.data_dir.join("public-test-scalar.key");
        wallet::save_private_key(&import, &fixture.owner_key).unwrap();
        assert_eq!(consumer.wallet_import_file(&import).unwrap(), quote.payload.consumer_wallet);
        let scoped = config.network.scoped_dir(&config.data_dir);
        let account_path = scoped.join("consumer").join(&quote.payload.consumer_wallet)
            .join(format!("{}.json", hex::encode(protocol::digest(quote.payload.id.as_bytes()))));
        let journal = Connection::open(scoped.join("wallet-journal/transactions.sqlite3")).unwrap();
        // Preparation is a restored prerequisite, but provider acceptance below is fresh:
        // native owner proof, synthetic spendability, real ledger commit and real reply loss.
        journal.execute(
            "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at)
             VALUES (?1,?2,?3,?4,?5,'prepared',?6)",
            params![quote.payload.consumer_wallet, format!("open:{}", quote.payload.id),
                envelope.nonce.to_string(), envelope.tx_hash, raw, quote.payload.issued_at]).unwrap();
        let journal_rows = || {
            let mut statement = journal.prepare("SELECT * FROM transactions ORDER BY sender,operation").unwrap();
            let columns = statement.column_count();
            statement.query_map([], |row| (0..columns)
                .map(|column| row.get::<_, rusqlite::types::Value>(column))
                .collect::<rusqlite::Result<Vec<_>>>()).unwrap()
                .collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        let journal_before = journal_rows();
        assert_eq!(journal_before.len(), 1);
        assert!(!account_path.exists());
        let client_calls = Arc::new(Mutex::new(Vec::new()));
        let observed = client_calls.clone();
        let saved_path = account_path.clone();
        let rpc_app = Router::new().route("/", post(move |Json(request): Json<Value>| {
            let observed = observed.clone();
            let saved_path = saved_path.clone();
            async move {
                let saved = std::fs::read(saved_path).ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
                observed.lock().unwrap().push((request, saved));
                StatusCode::SERVICE_UNAVAILABLE
            }
        }));
        let client_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });

        consumer.fund(&fixture.provider.offer, quote).await
            .expect_err("the fresh acknowledgement response is disconnected");
        assert!(!lose_next.load(Ordering::Acquire), "the post-commit response fault must fire");
        let intent_path = format!("/v1/contracts/{}/funding-intent", quote.payload.id);
        let observed = exchanges.lock().unwrap();
        assert_eq!(observed.len(), 2, "no automatic retry, replacement quote or account opening");
        assert_eq!(observed[0].0, "/v1/auth/challenge");
        assert_eq!(observed[0].1["path"], intent_path);
        assert_eq!(observed[0].1["wallet"], quote.payload.consumer_wallet);
        assert_eq!(observed[0].2, StatusCode::OK);
        assert_eq!(observed[1].0, intent_path);
        assert_eq!(observed[1].1, json!({"raw_transaction":raw}));
        let acknowledgement = observed[1].3.clone();
        let first_challenge = observed[0].3["nonce"].clone();
        drop(observed);
        let ack: Signed<FundingIntent> = serde_json::from_value(acknowledgement.clone()).unwrap();
        protocol::verify(protocol::INTENT_DOMAIN, &ack, &quote.payload.provider_wallet).unwrap();
        assert_eq!(ack.payload.network, quote.payload.network);
        assert_eq!(ack.payload.provider_wallet, quote.payload.provider_wallet);
        assert_eq!(ack.payload.consumer_wallet, quote.payload.consumer_wallet);
        assert_eq!(ack.payload.quote_id, quote.payload.id);
        assert_eq!(ack.payload.tx_hash, envelope.tx_hash);
        assert!((quote.payload.issued_at..quote.payload.expires_at).contains(&ack.payload.registered_at));
        let retained_before = retained();
        assert_eq!(retained_before, (quote.payload.id.clone(), envelope.tx_hash.clone(), raw.clone(),
            serde_json::to_string(&ack).unwrap(), serde_json::to_string(quote).unwrap(),
            quote.payload.consumer_wallet.clone(), envelope.nonce.to_string(), quote.payload.expires_at));
        assert_eq!(db.query_row("SELECT COUNT(*) FROM quotes WHERE id=?1", [&quote.payload.id],
            |row| row.get::<_, u64>(0)).unwrap(), 0);
        assert_eq!(pending(&unrelated.payload.id), unrelated_before);
        let calls = fixture.rpc.requests.lock().unwrap();
        assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBalance").count(), 1);
        assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getTransactionCount").count(), 4);
        assert!(calls.iter().all(|call| call["method"] != "eth_sendRawTransaction"));
        let provider_calls = calls.len();
        drop(calls);
        assert!(client_calls.lock().unwrap().is_empty(), "lost acknowledgement must stop client RPC");
        let prepared_bytes = std::fs::read(&account_path).unwrap();
        let prepared: Value = serde_json::from_slice(&prepared_bytes).unwrap();
        assert_eq!(prepared["quote"], serde_json::to_value(quote).unwrap());
        assert_eq!(prepared["raw_transaction"], raw);
        assert_eq!(prepared["tx_hash"], envelope.tx_hash);
        assert!(prepared["funding_intent"].is_null());
        assert_eq!(prepared["funding_broadcast_started"], false);
        assert!(prepared["account"].is_null());
        assert_eq!(prepared["credentials"], json!([]));
        assert!(prepared["active_credential"].is_null());
        assert_eq!(consumer.pending_accounts().unwrap()["pending"][0]["funding_stage"], "registration_pending");
        assert_eq!(journal_rows(), journal_before);

        drop(consumer);
        stop.send(()).unwrap();
        server.await.unwrap();
        // Restore later resource pressure only after the real acknowledgement was
        // committed and lost. These are complete signed fixture prerequisites, not
        // 9,999 observed admissions or public issuances within one rate window.
        {
            let mut pressure_db = Connection::open(&fixture.provider.ledger_path).unwrap();
            funding::restore_retained_intents(&mut pressure_db, &fixture.provider, &fixture.quote, 9_999, |quote, key, sequence| {
                let raw = signed_payment(&PaymentFields::opening(quote, sequence, 11), key, 0, &[0xc0]);
                let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw,
                    &quote.consumer_wallet, &quote.provider_wallet, &quote.funding_wei,
                    &protocol::opening_data(quote).unwrap()).unwrap();
                (raw, envelope)
            });
            let tx = pressure_db.transaction().unwrap();
            {
                let mut insert = tx.prepare(
                    "INSERT INTO quotes(id,owner,expires,record) VALUES(?1,?2,?3,?4)").unwrap();
                for sequence in 1..=9_999_u64 {
                    let mut payload = unrelated.payload.clone();
                    payload.id = format!("0x{:064x}", 0xbeef_0000 + sequence);
                    payload.nonce = format!("0x{:064x}", 0xface_0000 + sequence);
                    payload.consumer_wallet = format!("0x{:040x}", 0x90000 + sequence);
                    let pending = protocol::sign(protocol::QUOTE_DOMAIN, payload,
                        &fixture.provider.state.private_key).unwrap();
                    insert.execute(params![pending.payload.id, pending.payload.consumer_wallet,
                        pending.payload.expires_at, serde_json::to_string(&pending).unwrap()]).unwrap();
                }
            }
            tx.commit().unwrap();
        }
        // Stream every persisted column, including exact signed JSON/raw strings,
        // without retaining all 20,000 records in Rust memory.
        let resource_snapshot = || [
            "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until)
             FROM funding_intents ORDER BY contract",
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
        let assert_full_resources = || {
            let counts: (u64, u64) = db.query_row(
                "SELECT (SELECT COUNT(*) FROM funding_intents WHERE admission_until>?1
                         AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)),
                        (SELECT COUNT(*) FROM quotes WHERE expires>?1)",
                [protocol::now().unwrap()], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
            assert_eq!(counts, (10_000, 10_000), "both fresh resources must still be full at recovery");
        };
        let pressure_before = resource_snapshot();
        assert_eq!((pressure_before[0].0, pressure_before[1].0), (10_000, 10_000));
        assert_full_resources();
        assert_eq!(retained(), retained_before);
        assert_eq!(pending(&unrelated.payload.id), unrelated_before);

        // Both fresh gates remain open here. Real refusals must be caused by the
        // independent storage/admission bounds, not later draining or stopped RPC.
        let (status, refused) = request(&mut fixture.app, "/v1/contracts/propose", &HeaderMap::new(),
            serde_json::to_vec(&json!({"offer_id":fixture.provider.offer_id,
                "consumer_wallet":quote.payload.consumer_wallet,"funding_wei":"102"})).unwrap()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
        assert_eq!(refused["error"]["code"], "rate_limit");
        let candidate_raw = signed_payment(&PaymentFields::opening(&unrelated.payload, 1, 11),
            &fixture.owner_key, 0, &[0xc0]);
        let candidate = fixture.provider.state.chain.validate_payment_envelope(&candidate_raw,
            &unrelated.payload.consumer_wallet, &unrelated.payload.provider_wallet,
            &unrelated.payload.funding_wei, &protocol::opening_data(&unrelated.payload).unwrap()).unwrap();
        // Synthetic next-nonce observations are not proof the original executed.
        fixture.rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
        fixture.rpc.control.lock().unwrap().view = ChainView {
            latest_nonce: 1, pending_nonce: 1, head_height: 11, head_hash: 11,
        };
        let (status, refused) = authorized_intent(&mut fixture.app, &unrelated,
            &fixture.owner_key, &candidate_raw).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
        assert_eq!(refused["error"]["code"], "rate_limit");
        let calls = fixture.rpc.requests.lock().unwrap();
        let fresh_calls = &calls[provider_calls..];
        assert_eq!(fresh_calls.iter().filter(|call| call["method"] == "eth_getBalance").count(), 1);
        assert_eq!(fresh_calls.iter().filter(|call| call["method"] == "eth_getTransactionCount").count(), 4);
        assert!(calls.iter().all(|call| call["method"] != "eth_sendRawTransaction"));
        let provider_calls = calls.len();
        drop(calls);
        assert_eq!(db.query_row(
            "SELECT COUNT(*) FROM funding_intents WHERE contract=?1 OR tx_hash=?2 OR (sender=?3 AND nonce='1')",
            params![unrelated.payload.id, candidate.tx_hash, unrelated.payload.consumer_wallet],
            |row| row.get::<_, u64>(0)).unwrap(), 0);
        assert_eq!(resource_snapshot(), pressure_before, "fresh refusals cannot evict or partially accept records");
        assert_full_resources();
        assert!(client_calls.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&account_path).unwrap(), prepared_bytes);
        assert_eq!(journal_rows(), journal_before);

        fixture.app = Router::new();
        {
            let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
            state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
            state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
            state.accepting.store(false, Ordering::Release);
            state.funding_ready.store(false, Ordering::Release);
            state.qualified_models.write().unwrap().clear();
        }
        fixture.app = contracts::router(fixture.provider.state.clone());
        fixture.server.abort();
        assert!((&mut fixture.server).await.unwrap_err().is_cancelled());
        let (stop, server) = serve(fixture.app.clone());
        let consumer = Consumer::new(config.clone()).unwrap();
        assert_eq!(std::fs::read(&account_path).unwrap(), prepared_bytes);
        assert_eq!(retained(), retained_before);
        assert_full_resources();
        assert_eq!(resource_snapshot(), pressure_before);
        if repeat_fund {
            consumer.fund(&fixture.provider.offer, quote).await
                .expect_err("recovered acceptance cannot fabricate healthy chain evidence");
        } else {
            consumer.resume(&quote.payload.id).await
                .expect_err("recovered acceptance cannot fabricate healthy chain evidence");
        }
        let observed = exchanges.lock().unwrap();
        assert_eq!(observed.len(), 4, "recovery uses one new proof and the exact original intent only");
        assert_eq!(observed[2].0, "/v1/auth/challenge");
        assert_eq!(observed[2].1, observed[0].1);
        assert_eq!(observed[2].2, StatusCode::OK);
        assert_ne!(observed[2].3["nonce"], first_challenge);
        assert_eq!(observed[3], observed[1], "the cached response must preserve the entire acknowledgement");
        drop(observed);
        assert_eq!(fixture.rpc.requests.lock().unwrap().len(), provider_calls);
        let mut accepted = prepared;
        accepted["funding_intent"] = acknowledgement;
        accepted["funding_broadcast_started"] = json!(true);
        let calls = client_calls.lock().unwrap();
        assert!(!calls.is_empty(), "valid recovered acknowledgement must permit progress to chain verification");
        for (request, saved) in calls.iter() {
            assert_ne!(request["method"], "eth_sendRawTransaction");
            assert_eq!(saved.as_ref().unwrap(), &accepted, "both acknowledgement and start intent precede chain RPC");
        }
        drop(calls);
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&account_path).unwrap()).unwrap(), accepted);
        assert_eq!(journal_rows(), journal_before, "original ownership survives without another operation or submission");
        assert_eq!(retained(), retained_before, "no renewed lease or rewritten acknowledgement");
        assert_eq!(pending(&unrelated.payload.id), unrelated_before);
        let ledger = BillingStore::open(&fixture.provider.ledger_path, &quote.payload.network).unwrap();
        assert_eq!(ledger.admin_summary().unwrap()["active_funding_intents"], 10_000);
        assert_full_resources();
        assert_eq!(resource_snapshot(), pressure_before, "cached recovery cannot consume or rewrite either resource");
        assert_eq!(serde_json::to_value(ledger.quote(&quote.payload.id).unwrap()).unwrap(), serde_json::to_value(quote).unwrap());
        assert_eq!(serde_json::to_value(ledger.funding_intent(&quote.payload, &raw).unwrap()).unwrap(), accepted["funding_intent"]);
        let accounts: (u64, u64, u64, u64) = db.query_row(
            "SELECT (SELECT COUNT(*) FROM accounts),(SELECT COUNT(*) FROM credentials),
                    (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations)", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
        assert_eq!(accounts, (0, 0, 0, 0));
        drop(consumer);
        stop.send(()).unwrap();
        server.await.unwrap();
        client_server.abort();
        assert!(client_server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn paid_topup_recovers_at_full_paid_intent_capacity_without_fresh_admission() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 75_000, 100).await;
    let original = fixture.quote.clone();
    let id = &original.payload.id;
    let (opening_raw, opening) = fixture.original(0, &PaymentFields::opening(&original.payload, 0, 11));
    fixture.rpc.control.lock().unwrap().balance = u128::MAX;
    let (status, document) = fixture.submit_intent(&opening_raw).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let ack: Signed<FundingIntent> = serde_json::from_value(document).unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &original.payload.provider_wallet).unwrap();
    // The acknowledgement above is real HTTP admission; opening credit and supplied
    // prior usage below are ledger prerequisites, not chain/engine observations.
    let opening_payment = VerifiedPayment {
        network: original.payload.network.clone(), tx_hash: opening.tx_hash.clone(),
        from: opening.from, to: opening.to, value_wei: opening.value_wei, data: opening.data,
        block_hash: protocol::hash_hex(b"paid-capacity opening prerequisite"),
        block_number: 1, raw_transaction: opening_raw.clone(),
    };
    let now = protocol::now().unwrap();
    let opened = fixture.provider.state.ledger.open_account(&original, Some(&opening_payment), now).unwrap();
    let root = opened.credential.as_ref().unwrap();
    let ledger = &fixture.provider.state.ledger;
    let read = ledger.issue_credential(id, &CredentialIssueRequest {
        expires_at: now + 3600, spend_cap_wei: None, max_concurrency: 1, scopes: vec!["read".into()],
    }, now).unwrap();
    let spent = ledger.reserve(id, &root.id, "spent-before-paid-capacity", 1, 1, now, &opened.account.model).unwrap();
    ledger.settle(&spent.id, &protocol::TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", now).unwrap();
    let held = ledger.reserve(id, &root.id, "held-through-paid-capacity", 1000, 512, now, &opened.account.model).unwrap();
    let mut expected_account = serde_json::to_value(ledger.account(id).unwrap()).unwrap();
    assert_eq!(expected_account["funded_wei"], "100");
    assert_eq!(expected_account["fee_wei"], "0");
    assert_eq!(expected_account["spent_wei"], "1");
    assert_eq!(expected_account["reserved_wei"], "3");
    assert_eq!(expected_account["available_wei"], "96");

    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    // Reuse complete signed originals, acknowledgements and independent envelope
    // validation. This is restored history, not 10,000 observed admissions.
    funding::restore_retained_intents(&mut db, &fixture.provider, &fixture.quote, 10_000, |quote, key, sequence| {
        let raw = signed_payment(&PaymentFields::opening(quote, sequence, 11), key, 0, &[0xc0]);
        let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw,
            &quote.consumer_wallet, &quote.provider_wallet, &quote.funding_wei,
            &protocol::opening_data(quote).unwrap()).unwrap();
        (raw, envelope)
    });
    fixture.rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
    let (status, document) = request(&mut fixture.app, "/v1/contracts/propose", &HeaderMap::new(),
        serde_json::to_vec(&json!({"offer_id":fixture.provider.offer_id,
            "consumer_wallet":fixture.rpc.payer,"funding_wei":"100"})).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    fixture.quote = serde_json::from_value(document).unwrap();
    let (refused_raw, refused_envelope) = fixture.original(0, &PaymentFields::opening(&fixture.quote.payload, 1, 11));
    {
        let mut control = fixture.rpc.control.lock().unwrap();
        control.view.latest_nonce = 1;
        control.view.pending_nonce = 1;
    }
    let mut fields = PaymentFields::opening(&original.payload, 1, 11);
    fields.value = 1000;
    fields.data = protocol::topup_data(id).unwrap();
    let raw = signed_payment(&fields, &fixture.owner_key, 0, &[0xc0]);
    let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw,
        &original.payload.consumer_wallet, &original.payload.provider_wallet, "1000", &fields.data).unwrap();
    let topup_hash = envelope.tx_hash.clone();
    let path = format!("/v1/contracts/{id}/topup");
    let body = serde_json::to_vec(&protocol::TopupRequest { raw_transaction: raw.clone() }).unwrap();

    let fingerprint = |query: &str, values: &[&dyn rusqlite::ToSql]| {
        let mut hash = Keccak::v256();
        let mut count = 0_u64;
        let mut statement = db.prepare(query).unwrap();
        let mut rows = statement.query(values).unwrap();
        while let Some(row) = rows.next().unwrap() {
            hash.update(row.get::<_, String>(0).unwrap().as_bytes());
            count += 1;
        }
        let mut digest = [0; 32];
        hash.finalize(&mut digest);
        (count, digest)
    };
    let protected_state = || ([
        "SELECT json_array(id,owner,quote,sequence,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
        "SELECT json_array(id,contract,request_key,credential,status,created,record) FROM requests ORDER BY id",
        "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(id,status,record) FROM fee_batches ORDER BY id",
        "SELECT json_array(key,value) FROM metadata ORDER BY key",
        "SELECT json_array(sequence,request,model,evidence,miss) FROM performance_observations ORDER BY sequence",
        "SELECT json_array(model,evidence) FROM performance_withdrawals ORDER BY model,evidence",
    ].map(|query| fingerprint(query, &[])),
        fingerprint("SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding WHERE tx_hash<>?1 ORDER BY tx_hash", &[&topup_hash]));
    let monetary_state = || [
        "SELECT json_array(id,account) FROM accounts ORDER BY id",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash",
        "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash",
    ].map(|query| fingerprint(query, &[]));
    let attempts = || fingerprint(
        "SELECT json_array(tx_hash,contract,record,raw_transaction,updated) FROM funding_attempts ORDER BY tx_hash", &[]);
    let rates = || fingerprint("SELECT json_array(bucket,window,count) FROM rate_limits ORDER BY bucket", &[]);
    let challenges = |except: &str| fingerprint(
        "SELECT json_array(nonce,wallet,expires,record) FROM challenges WHERE nonce<>?1 ORDER BY nonce", &[&except]);
    let assert_capacity = || {
        let counts: (u64, u64, u64, u64) = db.query_row(
            "SELECT (SELECT COUNT(*) FROM funding_intents WHERE admission_until>?1
                     AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)),
                    (SELECT COUNT(*) FROM funding_intents),(SELECT COUNT(*) FROM quotes),
                    (SELECT COUNT(*) FROM accounts)",
            [protocol::now().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
        assert_eq!(counts, (10_000, 10_001, 1, 1), "only the active paid-intent bound is full");
        let budgets: Vec<(String, u64)> = db.prepare(
            "SELECT bucket,count FROM rate_limits WHERE bucket IN ('quote:global','quote-work:global') ORDER BY bucket")
            .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(budgets.len(), 2);
        assert!(budgets.iter().all(|(_, count)| (1..600).contains(count)));
    };
    assert_capacity();
    let protected = protected_state();
    let before_credit = monetary_state();
    let no_attempts = attempts();
    let [accounts, credentials, requests, quotes, intents, batches, _, observations, withdrawals] = protected.0.map(|row| row.0);
    assert_eq!((accounts, credentials, requests, quotes, intents, batches, observations, withdrawals),
        (1, 2, 2, 1, 10_001, 0, 0, 0));
    assert_eq!(protected.1.0, 1);
    assert_eq!(before_credit.map(|row| row.0), [1, 1, 0]);
    assert_eq!(no_attempts.0, 0);

    // Real fresh admission gets through initial/rechecked spendability and fails
    // only at lease capacity, before any acknowledgement or quote handoff.
    let intent_path = format!("/v1/contracts/{}/funding-intent", fixture.quote.payload.id);
    let intent_body = serde_json::to_vec(&json!({"raw_transaction":refused_raw})).unwrap();
    let headers = owner_headers(&mut fixture.app, &fixture.quote.payload, &fixture.owner_key, &intent_path, &intent_body).await;
    let nonce = headers["x-chiral-nonce"].to_str().unwrap();
    let remaining = challenges(nonce);
    let before_rates = rates();
    let start = fixture.rpc.requests.lock().unwrap().len();
    let (status, document) = request(&mut fixture.app, &intent_path, &headers, intent_body).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{document}");
    assert_eq!(document["error"]["code"], "rate_limit");
    assert_eq!(challenges(""), remaining);
    assert!(fixture.provider.state.ledger.challenge(nonce).is_err());
    {
        let calls = fixture.rpc.requests.lock().unwrap();
        let calls = &calls[start..];
        assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBalance").count(), 1);
        assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getTransactionCount").count(), 4);
        assert!(calls.iter().all(|call| call["method"] != "eth_sendRawTransaction" && call["method"] != "eth_getTransactionReceipt"));
    }
    assert_eq!(db.query_row(
        "SELECT COUNT(*) FROM funding_intents WHERE contract=?1 OR tx_hash=?2 OR (sender=?3 AND nonce='1')",
        params![fixture.quote.payload.id, refused_envelope.tx_hash, fixture.rpc.payer],
        |row| row.get::<_, u64>(0)).unwrap(), 0);
    assert_eq!(protected_state(), protected);
    assert_eq!(monetary_state(), before_credit);
    assert_eq!(attempts(), no_attempts);
    assert_eq!(rates(), before_rates);

    // Settlement has a separate strict RPC surface: fresh nonce/balance admission
    // or submission is an error, not an interchangeable verification shortcut.
    let rpc = Arc::new(TopupCapacityRpc { admission: fixture.rpc.clone(), envelope, phase: AtomicU64::new(0) });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rpc_url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route("/", post(topup_capacity_rpc)).with_state(rpc.clone());
    fixture.server.abort();
    fixture.server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let mut credited = None;
    let mut pending_record = None;
    for (attempt, phase) in [0, 1, 2, 2].into_iter().enumerate() {
        fixture.app = Router::new();
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        Arc::get_mut(&mut state.config).unwrap().network.rpc_url = rpc_url.clone();
        state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
        state.accepting.store(false, Ordering::Release);
        state.funding_ready.store(false, Ordering::Release);
        state.qualified_models.write().unwrap().clear();
        fixture.app = contracts::router(fixture.provider.state.clone());
        rpc.phase.store(phase, Ordering::Release);
        fixture.rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
        assert_capacity();
        if attempt == 0 {
            let start = fixture.rpc.requests.lock().unwrap().len();
            let before_challenges = challenges("");
            let before_rates = rates();
            for credential in [None, Some(root), Some(&read)] {
                let mut headers = HeaderMap::new();
                if let Some(credential) = credential {
                    headers.insert(header::AUTHORIZATION, format!("Bearer {}", credential.bearer).parse().unwrap());
                }
                let (status, document) = request(&mut fixture.app, &path, &headers, body.clone()).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{document}");
            }
            assert_eq!(challenges(""), before_challenges);
            assert_eq!(rates(), before_rates);
            let other_key = format!("{:064x}", 7);
            let mut other_owner = original.payload.clone();
            other_owner.consumer_wallet = wallet::address_from_private_key(&other_key).unwrap();
            assert_ne!(other_owner.consumer_wallet, original.payload.consumer_wallet);
            let headers = owner_headers(&mut fixture.app, &other_owner, &other_key, &path, &body).await;
            let remaining = challenges(headers["x-chiral-nonce"].to_str().unwrap());
            let before_rates = rates();
            let (status, document) = request(&mut fixture.app, &path, &headers, body.clone()).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{document}");
            assert_eq!(challenges(""), remaining);
            assert_eq!(rates(), before_rates);
            assert_eq!(protected_state(), protected);
            assert_eq!(monetary_state(), before_credit);
            assert_eq!(attempts(), no_attempts);
            assert_eq!(fixture.rpc.requests.lock().unwrap().len(), start);
        }
        let headers = owner_headers(&mut fixture.app, &original.payload, &fixture.owner_key, &path, &body).await;
        let nonce = headers["x-chiral-nonce"].to_str().unwrap();
        let remaining = challenges(nonce);
        let before_rates = rates();
        let start = fixture.rpc.requests.lock().unwrap().len();
        let (status, document) = request(&mut fixture.app, &path, &headers, body.clone()).await;
        assert!(fixture.provider.state.ledger.challenge(nonce).is_err());
        assert_eq!(challenges(""), remaining);
        {
            let calls = fixture.rpc.requests.lock().unwrap();
            let calls = &calls[start..];
            let observations = if phase < 2 { 1 } else { 2 };
            for method in ["eth_getTransactionReceipt", "eth_syncing"] {
                assert_eq!(calls.iter().filter(|call| call["method"] == method).count(), observations);
            }
            for tag in ["safe", "latest", "0x8"] {
                assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBlockByNumber"
                    && call["params"][0] == tag).count(), if phase < 2 && tag == "0x8" { 0 } else { observations });
            }
            assert!(calls.iter().all(|call| matches!(call["method"].as_str(),
                Some("eth_chainId" | "eth_getBlockByNumber" | "eth_syncing" | "eth_getTransactionReceipt"))));
        }
        if phase < 2 {
            assert_eq!(status, StatusCode::CONFLICT, "{document}");
            assert_eq!(document["error"]["code"], "funding_pending");
            assert_eq!(monetary_state(), before_credit);
            assert_eq!(attempts().0, 1);
            let (hash, contract, stored_raw, record): (String, String, String, String) = db.query_row(
                "SELECT tx_hash,contract,raw_transaction,record FROM funding_attempts", [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
            assert_eq!((hash, contract, stored_raw), (topup_hash.clone(), id.clone(), raw.clone()));
            let record: Value = serde_json::from_str(&record).unwrap();
            assert_eq!(record["status"], "pending");
            assert_eq!(record["value_wei"], Value::Null);
            assert_eq!(record["fee_wei"], Value::Null);
            if let Some(previous) = &pending_record { assert_eq!(&record, previous); }
            else { pending_record = Some(record); }
        } else {
            assert_eq!(status, StatusCode::OK, "{document}");
            expected_account["funded_wei"] = json!("1100");
            expected_account["fee_wei"] = json!("5");
            expected_account["available_wei"] = json!("1091");
            assert_eq!(document, expected_account);
            let current = monetary_state();
            assert_eq!(current.map(|row| row.0), [1, 2, 1]);
            if let Some(previous) = credited { assert_eq!(current, previous); }
            else { credited = Some(current); }
            assert_eq!(attempts(), no_attempts);
            let (record, evidence, status, checked): (String, String, String, u64) = db.query_row(
                "SELECT record,evidence,status,checked FROM funding WHERE tx_hash=?1",
                [&topup_hash], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
            let record: Value = serde_json::from_str(&record).unwrap();
            assert_eq!(record["tx_hash"], topup_hash);
            assert_eq!(record["contract_id"], *id);
            assert_eq!(record["status"], "accepted");
            assert_eq!(record["value_wei"], "1000");
            assert_eq!(record["fee_wei"], "5");
            assert_eq!(record["created_at"], checked);
            assert_eq!(status, "accepted");
            let expected = VerifiedPayment {
                network: original.payload.network.clone(), tx_hash: topup_hash.clone(),
                from: rpc.envelope.from.clone(), to: rpc.envelope.to.clone(), value_wei: "1000".into(),
                data: fields.data.clone(), block_hash: format!("0x{:064x}", 8),
                block_number: 8, raw_transaction: raw.clone(),
            };
            assert_eq!(serde_json::from_str::<Value>(&evidence).unwrap(), serde_json::to_value(expected).unwrap());
            let fee: String = db.query_row(
                "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations",
                [], |row| row.get(0)).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&fee).unwrap(),
                json!([topup_hash, original.payload.platform_wallet, "5", checked, "pending", null]));
        }
        assert_eq!(protected_state(), protected);
        assert_eq!(rates(), before_rates);
        assert_capacity();
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.account(id).unwrap()).unwrap(), expected_account);
        assert_eq!(fixture.provider.state.ledger.request_view(id, &held.id).unwrap().status, "reserved");
        assert!(!fixture.provider.state.accepting.load(Ordering::Acquire));
        assert!(!fixture.provider.state.funding_ready.load(Ordering::Acquire));
        assert!(fixture.provider.state.qualified_models.read().unwrap().is_empty());
        let before_attempts = attempts();
        let calls = fixture.rpc.requests.lock().unwrap().len();
        let (status, document) = request(&mut fixture.app, &path, &headers, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{document}");
        assert_eq!(fixture.rpc.requests.lock().unwrap().len(), calls);
        for credential in [root, &read] {
            let response = fixture.app.call(Request::builder().method("GET").uri(format!("/v1/contracts/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {}", credential.bearer)).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), 96 * 1024).await.unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), expected_account);
        }
        assert_eq!(protected_state(), protected);
        assert_eq!(monetary_state(), credited.unwrap_or(before_credit));
        assert_eq!(attempts(), before_attempts);
        assert_eq!(rates(), before_rates);
        assert_eq!(challenges(""), remaining);
    }
    let journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let operations: (u64, u64) = journal.query_row(
        "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)", [],
        |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(operations, (0, 0));
}

type Observed = Arc<Mutex<Vec<(String, String, Value, StatusCode, Value)>>>;

async fn propose(app: &mut Router, offer_id: &str, wallet: &str, funding: &str) -> (StatusCode, Value) {
    request(app, "/v1/contracts/propose", &HeaderMap::new(), serde_json::to_vec(&json!({
        "offer_id": offer_id, "consumer_wallet": wallet, "funding_wei": funding})).unwrap()).await
}

// Serve the production router to the native client, recording each exchange. When
// `lose_intent` is set, the next funding-intent reply is serialized by the real route
// and then disconnected: genuine reply loss, not an invented refusal.
fn serve_observed(app: Router, listener: &TcpListener, observed: Observed, lose_intent: Arc<AtomicBool>)
    -> (tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    let app = app.layer(from_fn(move |request: Request, next: Next| {
        let observed = observed.clone();
        let lose_intent = lose_intent.clone();
        async move {
            let method = request.method().to_string();
            let path = request.uri().path().to_owned();
            let (parts, body) = request.into_parts();
            let body = to_bytes(body, 96 * 1024).await.unwrap();
            let requested = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
            let response = next.run(Request::from_parts(parts, Body::from(body))).await;
            let (mut parts, body) = response.into_parts();
            let body = to_bytes(body, 96 * 1024).await.unwrap();
            let replied = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
            let intent = path.ends_with("/funding-intent");
            observed.lock().unwrap().push((method, path, requested, parts.status, replied));
            if intent && lose_intent.swap(false, Ordering::AcqRel) {
                parts.headers.remove(header::CONTENT_LENGTH);
                return Response::from_parts(parts, Body::from_stream(futures_util::stream::once(async {
                    Err::<axum::body::Bytes, _>(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
                })));
            }
            Response::from_parts(parts, Body::from(body))
        }
    }));
    listener.set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(listener.try_clone().unwrap()).unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).with_graceful_shutdown(async move { let _ = stopped.await; }).await.unwrap();
    });
    (stop, server)
}

fn journal_rows(journal: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    let mut statement = journal.prepare("SELECT * FROM transactions ORDER BY sender,operation").unwrap();
    let columns = statement.column_count();
    statement.query_map([], |row| (0..columns)
        .map(|column| row.get::<_, rusqlite::types::Value>(column))
        .collect::<rusqlite::Result<Vec<_>>>()).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

// Restore one already prepared original at the wallet-journal boundary. This is not
// observed preparation, spendability or chain execution.
fn restore_prepared_original(journal: &Connection, quote: &Quote, envelope: &ValidatedPaymentEnvelope, raw: &str) {
    journal.execute(
        "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at)
         VALUES (?1,?2,?3,?4,?5,'prepared',?6)",
        params![quote.consumer_wallet, format!("open:{}", quote.id), envelope.nonce.to_string(),
            envelope.tx_hash, raw, quote.issued_at]).unwrap();
}

fn saved_account_path(config: &ConsumerConfig, quote: &Quote) -> PathBuf {
    config.network.scoped_dir(&config.data_dir).join("consumer").join(&quote.consumer_wallet)
        .join(format!("{}.json", hex::encode(protocol::digest(quote.id.as_bytes()))))
}

/// specify-non-reserving-quotes 3.3/4.3, requirement "Public quotes do not reserve opening
/// capacity", scenario "Quotes overbook paid admission". With all 10,000 paid leases and
/// the owner's eight free-account slots occupied, real public paid and free proposals for
/// that owner and unrelated wallets still return provider-signed terms and change no lease,
/// intent, account or credential. Each holder still needs authenticated admission, which
/// checks authentication and the exact payment before refusing for capacity with 429
/// `rate_limit`, and creates no acknowledgement, intent, account, credit or submission.
/// The native client refuses to submit the overbooked payment. Restored leases, free
/// accounts and the prepared journal original are prerequisites, not observed admissions;
/// RPC observations are synthetic. Would catch quote issuance gated by (or allocating)
/// opening capacity, or fresh admission that lets an overbooked quote past a full bound.
#[tokio::test]
async fn overbooked_quotes_change_no_opening_capacity_and_still_need_admission() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 76_000, 100).await;
    fixture.rpc.control.lock().unwrap().balance = u128::MAX;
    let payer = fixture.rpc.payer.clone();
    let provider_key = fixture.provider.state.private_key.to_string();
    let paid_offer = fixture.provider.offer.clone();
    let paid_offer_id = fixture.provider.offer_id.clone();
    let mut free_terms = paid_offer.payload.clone();
    free_terms.deployment_id = "test-provider-free".into();
    free_terms.price.input_wei_per_1k = "0".into();
    free_terms.price.output_wei_per_1k = "0".into();
    let free_offer = protocol::sign(protocol::OFFER_DOMAIN, free_terms, &provider_key).unwrap();
    let free_offer_id = fixture.provider.state.offers.insert(free_offer.clone()).unwrap();

    // Owner's free-account allowance: eight complete signed openings at the ledger
    // boundary (prerequisites), using genuine provider-signed free terms.
    let (status, template) = propose(&mut fixture.app, &free_offer_id, &payer, "0").await;
    assert_eq!(status, StatusCode::OK, "{template}");
    let template: Signed<Quote> = serde_json::from_value(template).unwrap();
    let now = protocol::now().unwrap();
    for index in 0..8_u64 {
        let mut terms = template.payload.clone();
        terms.id = format!("0x{:064x}", 0xf4ee_0000 + index);
        terms.nonce = format!("0x{:064x}", 0xf4ee_1000 + index);
        let signed = protocol::sign(protocol::QUOTE_DOMAIN, terms, &provider_key).unwrap();
        fixture.provider.state.ledger.store_quote(&signed, now).unwrap();
        assert!(fixture.provider.state.ledger.open_account(&signed, None, now).unwrap().credential.is_some());
    }
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    funding::restore_retained_intents(&mut db, &fixture.provider, &fixture.quote, 10_000, |quote, key, sequence| {
        let raw = signed_payment(&PaymentFields::opening(quote, sequence, 11), key, 0, &[0xc0]);
        let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw,
            &quote.consumer_wallet, &quote.provider_wallet, &quote.funding_wei,
            &protocol::opening_data(quote).unwrap()).unwrap();
        (raw, envelope)
    });
    fixture.rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
    let fingerprint = |query: &str| {
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
    };
    let capacity = || {
        let counts: [u64; 6] = db.query_row(
            "SELECT (SELECT COUNT(*) FROM funding_intents WHERE admission_until>?1
                     AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)),
                    (SELECT COUNT(*) FROM accounts WHERE owner=?2),(SELECT COUNT(*) FROM accounts),
                    (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations),
                    (SELECT COUNT(*) FROM funding_attempts)",
            params![protocol::now().unwrap(), payer],
            |row| Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?])).unwrap();
        (counts, [
            "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
            "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts ORDER BY id",
            "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
        ].map(fingerprint))
    };
    let full = capacity();
    assert_eq!(full.0, [10_000, 8, 8, 0, 0, 0], "paid leases and the owner's free slots are full");

    // Overbooking: unrelated callers may name the owner or other wallets.
    let other = |n: u64| wallet::address_from_private_key(&format!("{:064x}", 130_000 + n)).unwrap();
    let mut overbooked = Vec::new();
    for (offer, offer_id, wallet, funding) in [
        (&paid_offer, &paid_offer_id, payer.clone(), "100"), (&free_offer, &free_offer_id, payer.clone(), "0"),
        (&paid_offer, &paid_offer_id, other(1), "150"), (&free_offer, &free_offer_id, other(2), "0"),
    ] {
        let (status, document) = propose(&mut fixture.app, offer_id, &wallet, funding).await;
        assert_eq!(status, StatusCode::OK, "overbooked issuance for {wallet}/{funding}: {document}");
        let quote: Signed<Quote> = serde_json::from_value(document).unwrap();
        protocol::verify(protocol::QUOTE_DOMAIN, &quote, &fixture.provider.state.wallet).unwrap();
        let terms = &quote.payload;
        assert_eq!((&terms.offer_id, &terms.consumer_wallet, terms.funding_wei.as_str()), (offer_id, &wallet, funding));
        assert_eq!((&terms.price, &terms.model, &terms.network), (&offer.payload.price, &offer.payload.model, &offer.payload.network));
        assert_eq!(terms.expires_at - terms.issued_at, 300);
        assert_eq!(capacity(), full, "a quote must not allocate or consume opening capacity");
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.quote(&terms.id).unwrap()).unwrap(),
            serde_json::to_value(&quote).unwrap());
        overbooked.push(quote);
    }
    let retained = |quotes: &[Signed<Quote>]| {
        for quote in quotes {
            let record: String = db.query_row("SELECT record FROM quotes WHERE id=?1", [&quote.payload.id],
                |row| row.get(0)).unwrap();
            assert_eq!(record, serde_json::to_string(quote).unwrap(), "refusal must keep the signed standalone quote");
        }
    };

    // The paid holder must still pass authenticated admission; authentication and the
    // exact payment are checked before the capacity decision, which refuses honestly.
    let paid_quote = overbooked[0].clone();
    let intent_path = format!("/v1/contracts/{}/funding-intent", paid_quote.payload.id);
    let fields = PaymentFields::opening(&paid_quote.payload, 0, 11);
    let raw = signed_payment(&fields, &fixture.owner_key, 2, &[0xc0]);
    let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw, &payer,
        &paid_quote.payload.provider_wallet, &paid_quote.payload.funding_wei,
        &protocol::opening_data(&paid_quote.payload).unwrap()).unwrap();
    let (status, _) = request(&mut fixture.app, &intent_path, &HeaderMap::new(),
        serde_json::to_vec(&json!({"raw_transaction":raw})).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut underpaid = PaymentFields::opening(&paid_quote.payload, 0, 11);
    underpaid.value -= 1;
    let underpaid = signed_payment(&underpaid, &fixture.owner_key, 2, &[0xc0]);
    let (status, document) = authorized_intent(&mut fixture.app, &paid_quote, &fixture.owner_key, &underpaid).await;
    assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_payment")), "{document}");
    let start = fixture.rpc.requests.lock().unwrap().len();
    let (status, document) = authorized_intent(&mut fixture.app, &paid_quote, &fixture.owner_key, &raw).await;
    assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("rate_limit")), "{document}");
    {
        let calls = fixture.rpc.requests.lock().unwrap();
        let fresh = &calls[start..];
        assert_eq!(fresh.iter().filter(|call| call["method"] == "eth_getBalance").count(), 1,
            "capacity is decided after the exact payment passes spendability");
        assert_eq!(fresh.iter().filter(|call| call["method"] == "eth_getTransactionCount").count(), 4);
    }
    let unassigned = || db.query_row(
        "SELECT COUNT(*) FROM funding_intents WHERE contract=?1 OR tx_hash=?2 OR (sender=?3 AND nonce='0')",
        params![paid_quote.payload.id, envelope.tx_hash, payer], |row| row.get::<_, u64>(0)).unwrap();
    assert_eq!(unassigned(), 0, "full refusal creates no acknowledgement, intent or nonce ownership");
    assert_eq!(capacity(), full);
    retained(&overbooked);

    // The free holder's authenticated first opening refuses at the owner's bound.
    let open_body = serde_json::to_vec(&json!({"quote_id":overbooked[1].payload.id,"raw_transaction":null})).unwrap();
    let headers = owner_headers(&mut fixture.app, &overbooked[1].payload, &fixture.owner_key, "/v1/contracts/open", &open_body).await;
    let (status, document) = request(&mut fixture.app, "/v1/contracts/open", &headers, open_body).await;
    assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("rate_limit")), "{document}");
    assert!(fixture.provider.state.ledger.account(&overbooked[1].payload.id).unwrap_err().starts_with("not_found:"));
    assert_eq!(capacity(), full);
    retained(&overbooked);

    // The native client, holding the overbooked quote and its prepared original, obtains
    // a refusal and never reaches chain RPC: no acknowledgement, start marker or broadcast.
    let observed: Observed = Arc::new(Mutex::new(Vec::new()));
    let (stop, server) = serve_observed(fixture.app.clone(), &fixture._listeners[0], observed.clone(),
        Arc::new(AtomicBool::new(false)));
    let client_calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = client_calls.clone();
    let rpc_app = Router::new().route("/", post(move |Json(request): Json<Value>| {
        let recorded = recorded.clone();
        async move { recorded.lock().unwrap().push(request); StatusCode::SERVICE_UNAVAILABLE }
    }));
    let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut network = fixture.provider.state.config.network.clone();
    network.rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
    let client_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });
    let config = ConsumerConfig { network, data_dir: directory.path().join("overbooked-payer"),
        catalog_url: None, development: true, dht: None };
    let consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, &fixture.owner_key).unwrap();
    assert_eq!(consumer.wallet_import_file(&import).unwrap(), payer);
    let journal = Connection::open(config.network.scoped_dir(&config.data_dir)
        .join("wallet-journal/transactions.sqlite3")).unwrap();
    restore_prepared_original(&journal, &paid_quote.payload, &envelope, &raw);
    let journal_before = journal_rows(&journal);
    let start = fixture.rpc.requests.lock().unwrap().len();
    let error = consumer.fund(&paid_offer, &paid_quote).await.unwrap_err();
    assert!(error.starts_with("rate_limit:") && error.ends_with("(HTTP 429)"), "{error}");
    {
        let exchanges = observed.lock().unwrap();
        assert_eq!(exchanges.len(), 2, "one owner proof and one intent; no retry or replacement quote");
        assert_eq!((exchanges[0].1.as_str(), exchanges[0].3), ("/v1/auth/challenge", StatusCode::OK));
        assert_eq!((&exchanges[1].1, &exchanges[1].2, exchanges[1].3), (&intent_path, &json!({"raw_transaction":raw}), StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(exchanges[1].4["error"]["code"], "rate_limit");
    }
    assert!(client_calls.lock().unwrap().is_empty(), "a refused native payment must not reach chain RPC");
    let saved: Value = serde_json::from_slice(&std::fs::read(saved_account_path(&config, &paid_quote.payload)).unwrap()).unwrap();
    assert_eq!((&saved["raw_transaction"], &saved["tx_hash"]), (&json!(raw), &json!(envelope.tx_hash)));
    assert!(saved["funding_intent"].is_null() && saved["account"].is_null());
    assert_eq!(saved["funding_broadcast_started"], false);
    assert_eq!(consumer.pending_accounts().unwrap()["pending"][0]["funding_stage"], "registration_pending");
    assert_eq!(journal_rows(&journal), journal_before, "no replacement operation or submission state");
    assert_eq!(fixture.rpc.requests.lock().unwrap()[start..].iter()
        .filter(|call| call["method"] == "eth_getBalance").count(), 1);
    assert_eq!(unassigned(), 0);
    assert_eq!(capacity(), full);
    retained(&overbooked);
    assert!(fixture.rpc.requests.lock().unwrap().iter().all(|call| call["method"] != "eth_sendRawTransaction"));
    drop(consumer);
    stop.send(()).unwrap();
    server.await.unwrap();
    client_server.abort();
    assert!(client_server.await.unwrap_err().is_cancelled());
}

// One isolated RPC for the native client and the provider. Scripted observations, not
// live-chain evidence: phase 0 has no receipt, phase 1 a successful receipt above the
// safe frontier (block 10 > safe 9), phase 2 the same receipt at canonical block 8.
struct RecoveryRpc {
    network: NetworkIdentity,
    original: ValidatedPaymentEnvelope,
    raw: String,
    account_path: PathBuf,
    observed_at: AtomicU64,
    phase: AtomicU64,
    calls: Mutex<Vec<(Value, Option<Value>)>>,
    unexpected: Mutex<Vec<Value>>,
}

async fn recovery_rpc(State(rpc): State<Arc<RecoveryRpc>>, Json(request): Json<Value>) -> Json<Value> {
    let saved = std::fs::read(&rpc.account_path).ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    rpc.calls.lock().unwrap().push((request.clone(), saved));
    let at = rpc.observed_at.load(Ordering::Acquire);
    let phase = rpc.phase.load(Ordering::Acquire);
    let block = |height: u64| json!({"number":format!("0x{height:x}"),
        "hash":format!("0x{height:064x}"),"timestamp":format!("0x{at:x}")});
    let params = &request["params"];
    let hashes_only = params[1] == false;
    let result = match (request["method"].as_str().unwrap_or_default(), params[0].as_str()) {
        ("eth_chainId", _) => json!(format!("0x{:x}", rpc.network.chain_id)),
        ("eth_syncing", _) => json!(false),
        ("eth_getBlockByNumber", Some("0x0")) if hashes_only =>
            json!({"number":"0x0","hash":rpc.network.genesis_hash,"timestamp":"0x0"}),
        ("eth_getBlockByNumber", Some("safe")) if hashes_only => block(9),
        ("eth_getBlockByNumber", Some("latest")) if hashes_only => block(11),
        ("eth_getBlockByNumber", Some("0x8")) if hashes_only => block(8),
        // Served consistently so that only the safe-frontier rule, not a missing
        // block, keeps the phase-1 receipt from being credited.
        ("eth_getBlockByNumber", Some("0xa")) if hashes_only => block(10),
        // Only an observed receipt advances the original sender's executed nonce.
        ("eth_getTransactionCount", Some(sender)) =>
            json!(if phase > 0 && sender.eq_ignore_ascii_case(&rpc.original.from) { "0x1" } else { "0x0" }),
        ("eth_getBalance", Some(_)) => json!(format!("0x{:x}", u128::MAX)),
        ("eth_getTransactionReceipt", Some(hash)) if hash == rpc.original.tx_hash => match phase {
            0 => Value::Null,
            _ => {
                let height = if phase == 1 { 10_u64 } else { 8 };
                json!({"transactionHash":rpc.original.tx_hash,"from":rpc.original.from,
                    "to":rpc.original.to,"type":"0x0","gasUsed":"0x10000","status":"0x1",
                    "blockNumber":format!("0x{height:x}"),"blockHash":format!("0x{height:064x}")})
            }
        },
        ("eth_sendRawTransaction", Some(raw)) if raw == rpc.raw => json!(rpc.original.tx_hash),
        _ => {
            rpc.unexpected.lock().unwrap().push(request.clone());
            return Json(json!({"jsonrpc":"2.0","id":request["id"],
                "error":{"code":-32601,"message":"unexpected recovery RPC"}}));
        }
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

// Restart the provider: independent ledger, chain client and router over the same
// durable state, with every fresh-admission gate left open.
fn restart_provider(fixture: &mut Fixture, rpc_url: &str) {
    fixture.app = Router::new();
    let state = Arc::get_mut(&mut fixture.provider.state).expect("no provider state survives a restart");
    Arc::get_mut(&mut state.config).unwrap().network.rpc_url = rpc_url.to_owned();
    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
    state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
    fixture.app = contracts::router(fixture.provider.state.clone());
}

/// specify-non-reserving-quotes 3.3/4.3, requirement "Authenticated paid admission preserves
/// accepted commitments", scenario "Accepted recovery outlives reply loss and capacity
/// changes", with deterministic expiry. The provider ledger acknowledges a genuinely signed
/// original at an explicit past time through its transaction-sampled clock; the client never
/// received that acknowledgement (its restored state is the prepared original without one).
/// At real time the quote and lease have expired, the provider restarts and all 10,000 paid
/// leases are refilled. Fresh admission then refuses (409 for the owned sender nonce, 429 for
/// capacity); standalone quote storage is then filled and signing work and committed issuance
/// exhausted, so fresh quoting refuses too, while the native client recovers the identical
/// acknowledgement through the
/// production route -- first losing that reply for real, then after provider and client
/// restart -- with no new lease, renewal, replacement quote or payment. Only after durable
/// acknowledgement and start marker does the client submit the exact original bytes. The
/// provider credits nothing for a missing or above-safe receipt and credits once, with the
/// exact fee, only after the scripted safe-frontier evidence passes the real verification
/// path; a later fund refreshes without rotating the credential. Synthetic RPC and restored
/// leases/preparation are prerequisites, not live-chain or power-loss evidence. Would catch
/// expiry or capacity refusing cached recovery or credit, a renewed or new lease, any
/// replacement operation, submission before durable acknowledgement, or credit before safety.
#[tokio::test]
async fn native_accepted_original_recovers_after_expiry_restart_and_refill_until_safe_credit() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 77_000, 100).await;
    let unrelated = fixture.quote.clone();
    let payer = fixture.rpc.payer.clone();
    let offer = fixture.provider.offer.clone();
    let issued = protocol::now().unwrap() - 1_000;
    let mut terms = unrelated.payload.clone();
    terms.id = protocol::random_id();
    terms.nonce = protocol::random_id();
    terms.funding_wei = "1000000".into();
    terms.issued_at = issued;
    terms.expires_at = issued + 300;
    let original = protocol::sign(protocol::QUOTE_DOMAIN, terms, &fixture.provider.state.private_key).unwrap();
    let id = original.payload.id.clone();
    let raw = signed_payment(&PaymentFields::opening(&original.payload, 0, 11), &fixture.owner_key, 0, &[0xc0]);
    let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw, &payer,
        &original.payload.provider_wallet, &original.payload.funding_wei,
        &protocol::opening_data(&original.payload).unwrap()).unwrap();
    // Explicit-time issuance and acceptance at the ledger boundary, after independent
    // envelope validation; spendability at that time is a prerequisite, not observed.
    fixture.provider.state.ledger.store_quote(&original, issued).unwrap();
    let ack = fixture.provider.state.ledger.register_funding_intent(&original, &payer, &raw,
        &envelope.tx_hash, envelope.nonce, &fixture.provider.state.private_key, || Ok(issued + 5),
        true, &original.payload.model).unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &ack, &original.payload.provider_wallet).unwrap();
    assert_eq!((ack.payload.quote_id.as_str(), ack.payload.tx_hash.as_str(), ack.payload.registered_at),
        (id.as_str(), envelope.tx_hash.as_str(), issued + 5));
    let ack_json = serde_json::to_value(&ack).unwrap();

    let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
    let mut network = fixture.provider.state.config.network.clone();
    network.rpc_url = rpc_url.clone();
    let config = ConsumerConfig { network, data_dir: directory.path().join("expired-recovery"),
        catalog_url: None, development: true, dht: None };
    let account_path = saved_account_path(&config, &original.payload);
    let rpc = Arc::new(RecoveryRpc { network: original.payload.network.clone(), original: envelope.clone(),
        raw: raw.clone(), account_path: account_path.clone(), observed_at: AtomicU64::new(protocol::now().unwrap()),
        phase: AtomicU64::new(0), calls: Mutex::new(Vec::new()), unexpected: Mutex::new(Vec::new()) });
    let rpc_app = Router::new().route("/", post(recovery_rpc)).with_state(rpc.clone());
    let rpc_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });
    {
        let consumer = Consumer::new(config.clone()).unwrap();
        let import = config.data_dir.join("public-test-scalar.key");
        wallet::save_private_key(&import, &fixture.owner_key).unwrap();
        assert_eq!(consumer.wallet_import_file(&import).unwrap(), payer);
    }
    let journal = Connection::open(config.network.scoped_dir(&config.data_dir)
        .join("wallet-journal/transactions.sqlite3")).unwrap();
    restore_prepared_original(&journal, &original.payload, &envelope, &raw);
    let journal_before = journal_rows(&journal);
    // The client's durable state after a lost acknowledgement: the prepared original only.
    let prepared = serde_json::to_vec(&json!({"offer":offer,"quote":original,"account":null,
        "raw_transaction":raw,"tx_hash":envelope.tx_hash,"funding_intent":null,
        "funding_broadcast_started":false,"funding_cancellation_requested":false,
        "funding_cancellation":null,"credentials":[],"active_credential":null,"topup":null,
        "receipt_sequence":0})).unwrap();
    wallet::secure_directory(account_path.parent().unwrap()).unwrap();
    {
        use std::io::Write;
        let mut file = wallet::create_secret_file(&account_path).unwrap();
        file.write_all(&prepared).unwrap();
        file.sync_all().unwrap();
    }
    assert_eq!(Consumer::new(config.clone()).unwrap().pending_accounts().unwrap()["pending"][0]["funding_stage"],
        "registration_pending");

    // Later pressure: refill every paid lease at real time (complete signed prerequisites).
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    funding::restore_retained_intents(&mut db, &fixture.provider, &unrelated, 10_000, |quote, key, sequence| {
        let raw = signed_payment(&PaymentFields::opening(quote, sequence, 11), key, 0, &[0xc0]);
        let envelope = fixture.provider.state.chain.validate_payment_envelope(&raw,
            &quote.consumer_wallet, &quote.provider_wallet, &quote.funding_wei,
            &protocol::opening_data(quote).unwrap()).unwrap();
        (raw, envelope)
    });
    fixture.server.abort();
    fixture.server = rpc_server;
    restart_provider(&mut fixture, &rpc_url);
    let original_row = || db.query_row(
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce),admission_until
         FROM funding_intents WHERE contract=?1", [&id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))).unwrap();
    let accepted_fields = json!([id, envelope.tx_hash, raw, serde_json::to_string(&ack).unwrap(),
        serde_json::to_string(&original).unwrap(), payer, "0"]).to_string();
    assert_eq!(original_row(), (accepted_fields.clone(), issued + 300), "the accepted deadline is the quote expiry");
    let counts = || db.query_row(
        "SELECT (SELECT COUNT(*) FROM funding_intents WHERE admission_until>?1
                 AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract)),
                (SELECT COUNT(*) FROM funding_intents),(SELECT COUNT(*) FROM accounts),
                (SELECT COUNT(*) FROM credentials),(SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations)",
        [protocol::now().unwrap()],
        |row| Ok([row.get::<_, u64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?])).unwrap();
    assert_eq!(counts(), [10_000, 10_001, 0, 0, 0, 0], "expired acceptance holds no lease; the refill holds them all");
    let sends = || rpc.calls.lock().unwrap().iter()
        .filter(|(call, _)| call["method"] == "eth_sendRawTransaction")
        .map(|(call, _)| call["params"].clone()).collect::<Vec<_>>();

    // Fresh admission at real time, with every gate open: the accepted original still owns
    // its sender nonce after expiry, and a new payer is refused for capacity.
    let (status, document) = propose(&mut fixture.app, &fixture.provider.offer_id, &payer, "100").await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let reused_nonce_quote: Signed<Quote> = serde_json::from_value(document).unwrap();
    let reused_nonce = signed_payment(&PaymentFields::opening(&reused_nonce_quote.payload, 0, 11), &fixture.owner_key, 2, &[0xc0]);
    let (status, document) = authorized_intent(&mut fixture.app, &reused_nonce_quote, &fixture.owner_key, &reused_nonce).await;
    assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("conflict")), "{document}");
    let other_key = format!("{:064x}", 130_007);
    let other = wallet::address_from_private_key(&other_key).unwrap();
    let (status, document) = propose(&mut fixture.app, &fixture.provider.offer_id, &other, "100").await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let capacity_quote: Signed<Quote> = serde_json::from_value(document).unwrap();
    let capacity_raw = signed_payment(&PaymentFields::opening(&capacity_quote.payload, 0, 11), &other_key, 2, &[0xc0]);
    let (status, document) = authorized_intent(&mut fixture.app, &capacity_quote, &other_key, &capacity_raw).await;
    assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("rate_limit")), "{document}");
    assert_eq!(rpc.calls.lock().unwrap().iter().filter(|(call, _)| call["method"] == "eth_getBalance").count(), 2,
        "both refusals follow the exact payment's spendability observation");
    for quote in [&unrelated, &reused_nonce_quote, &capacity_quote] {
        assert!(fixture.provider.state.ledger.funding_intent(&quote.payload, &raw).unwrap().is_none());
        let record: String = db.query_row("SELECT record FROM quotes WHERE id=?1", [&quote.payload.id], |row| row.get(0)).unwrap();
        assert_eq!(record, serde_json::to_string(quote).unwrap());
    }
    // Real-time maintenance released the expired lease without touching the agreement.
    assert_eq!(original_row(), (accepted_fields.clone(), 0));
    assert_eq!(counts(), [10_000, 10_001, 0, 0, 0, 0]);
    assert!(sends().is_empty());

    // Later quote pressure too: standalone storage filled to 10,000 complete signed quotes
    // and both signing work and committed issuance exhausted in a later minute. These are
    // restored prerequisites, not 10,000 observed issuances or 600 observed signatures.
    let later_minute = protocol::now().unwrap() / 60 + 10;
    {
        let mut pressure = Connection::open(&fixture.provider.ledger_path).unwrap();
        let tx = pressure.transaction().unwrap();
        {
            let mut insert = tx.prepare("INSERT INTO quotes(id,owner,expires,record) VALUES(?1,?2,?3,?4)").unwrap();
            for sequence in 1..=9_997_u64 {
                let mut payload = unrelated.payload.clone();
                payload.id = format!("0x{:064x}", 0xbeef_0000 + sequence);
                payload.nonce = format!("0x{:064x}", 0xface_0000 + sequence);
                payload.consumer_wallet = format!("0x{:040x}", 0x90000 + sequence);
                let pending = protocol::sign(protocol::QUOTE_DOMAIN, payload, &fixture.provider.state.private_key).unwrap();
                insert.execute(params![pending.payload.id, pending.payload.consumer_wallet,
                    pending.payload.expires_at, serde_json::to_string(&pending).unwrap()]).unwrap();
            }
        }
        tx.execute("INSERT OR REPLACE INTO metadata(key,value) VALUES('quote_committed_minute',?1)",
            [later_minute.to_string()]).unwrap();
        tx.execute("INSERT OR REPLACE INTO rate_limits(bucket,window,count) VALUES('quote:global',?1,600)",
            [later_minute]).unwrap();
        tx.commit().unwrap();
    }
    proposal::restore_quote_work(&fixture.provider.ledger_path, later_minute, 600);
    let ledger_path = fixture.provider.ledger_path.clone();
    let quote_resources = || {
        let mut hash = Keccak::v256();
        let mut statement = db.prepare("SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id").unwrap();
        let mut rows = statement.query([]).unwrap();
        let mut count = 0_u64;
        while let Some(row) = rows.next().unwrap() {
            hash.update(row.get::<_, String>(0).unwrap().as_bytes());
            count += 1;
        }
        let mut digest = [0; 32];
        hash.finalize(&mut digest);
        let committed: (String, u64, u64) = db.query_row(
            "SELECT metadata.value,rate_limits.window,rate_limits.count FROM metadata,rate_limits
             WHERE metadata.key='quote_committed_minute' AND rate_limits.bucket='quote:global'",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        (count, digest, committed, proposal::quote_work_evidence(&ledger_path))
    };
    let (status, document) = propose(&mut fixture.app, &fixture.provider.offer_id, &payer, "100").await;
    assert_eq!((status, document["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("rate_limit")), "{document}");
    let exhausted = quote_resources();
    assert_eq!((exhausted.0, &exhausted.2, &exhausted.3), (10_000, &(later_minute.to_string(), later_minute, 600),
        &(later_minute.to_string(), later_minute, 600)), "every fresh quote resource is exhausted");

    let observed: Observed = Arc::new(Mutex::new(Vec::new()));
    let lose_intent = Arc::new(AtomicBool::new(true));
    let intent_path = format!("/v1/contracts/{id}/funding-intent");
    let assert_exchanges = |expected: &[(&str, &str, StatusCode)]| {
        let exchanges = observed.lock().unwrap();
        let summary = exchanges.iter().map(|(method, path, _, status, _)| (method.clone(), path.clone(), *status)).collect::<Vec<_>>();
        assert_eq!(summary, expected.iter().map(|(method, path, status)| (method.to_string(), path.to_string(), *status)).collect::<Vec<_>>(),
            "no replacement quote, payment or automatic paid retry");
        for (_, path, requested, _, replied) in exchanges.iter() {
            if path == &intent_path {
                assert_eq!(requested, &json!({"raw_transaction":raw}));
                assert_eq!(replied, &ack_json, "the same retained acknowledgement, never a new one");
            } else if path == "/v1/contracts/open" {
                assert_eq!(requested, &json!({"quote_id":id,"raw_transaction":raw}));
            }
        }
    };

    // 1. The first post-expiry recovery reply is genuinely lost after the route served it.
    let (stop, server) = serve_observed(fixture.app.clone(), &fixture._listeners[0], observed.clone(), lose_intent.clone());
    let calls_before = rpc.calls.lock().unwrap().len();
    let consumer = Consumer::new(config.clone()).unwrap();
    consumer.resume(&id).await.expect_err("the recovered acknowledgement reply is disconnected");
    assert!(!lose_intent.load(Ordering::Acquire), "the reply fault must fire");
    assert_exchanges(&[("POST", "/v1/auth/challenge", StatusCode::OK), ("POST", intent_path.as_str(), StatusCode::OK)]);
    assert_eq!(rpc.calls.lock().unwrap().len(), calls_before, "an ambiguous reply reaches no chain RPC and submits nothing");
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&account_path).unwrap()).unwrap(),
        serde_json::from_slice::<Value>(&prepared).unwrap(), "no acknowledgement or start marker is saved");
    assert_eq!(consumer.pending_accounts().unwrap()["pending"][0]["funding_stage"], "registration_pending");
    assert_eq!(journal_rows(&journal), journal_before);
    assert_eq!(original_row(), (accepted_fields.clone(), 0));
    assert_eq!(counts(), [10_000, 10_001, 0, 0, 0, 0]);
    drop(consumer);
    stop.send(()).unwrap();
    server.await.unwrap();

    // 2. After provider and client restart: identical acknowledgement, durable before the
    //    single exact-original submission; no credit without a receipt.
    restart_provider(&mut fixture, &rpc_url);
    observed.lock().unwrap().clear();
    let (stop, server) = serve_observed(fixture.app.clone(), &fixture._listeners[0], observed.clone(), lose_intent.clone());
    let calls_before = rpc.calls.lock().unwrap().len();
    rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
    let consumer = Consumer::new(config.clone()).unwrap();
    let error = consumer.resume(&id).await.unwrap_err();
    assert!(error.starts_with("funding_pending:"), "{error}");
    assert_exchanges(&[("POST", "/v1/auth/challenge", StatusCode::OK), ("POST", intent_path.as_str(), StatusCode::OK),
        ("POST", "/v1/auth/challenge", StatusCode::OK), ("POST", "/v1/contracts/open", StatusCode::CONFLICT)]);
    assert_eq!(observed.lock().unwrap()[3].4["error"]["code"], "funding_pending");
    {
        let calls = rpc.calls.lock().unwrap();
        assert!(calls.len() > calls_before);
        for (call, saved) in &calls[calls_before..] {
            let saved = saved.as_ref().expect("durable client state precedes chain RPC");
            assert_eq!(saved["funding_intent"], ack_json, "{}", call["method"]);
            assert_eq!(saved["funding_broadcast_started"], true, "{}", call["method"]);
            assert_eq!(saved["raw_transaction"], raw);
        }
    }
    assert_eq!(sends(), vec![json!([raw])], "one submission of the exact original bytes");
    let journal_now = journal_rows(&journal);
    assert_eq!(journal_now.len(), 1);
    assert_eq!(journal_now[0][..5], journal_before[0][..5], "sender, operation, nonce, hash and raw are unchanged");
    assert!(fixture.provider.state.ledger.account(&id).unwrap_err().starts_with("not_found:"));
    assert_eq!(original_row(), (accepted_fields.clone(), 0));
    assert_eq!(counts(), [10_000, 10_001, 0, 0, 0, 0]);

    // 3. A receipt above the safe frontier: repeated fund resubmits only the same bytes
    //    and still receives no credit or new acknowledgement.
    observed.lock().unwrap().clear();
    rpc.phase.store(1, Ordering::Release);
    rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
    let error = consumer.fund(&offer, &original).await.unwrap_err();
    assert!(error.starts_with("funding_pending:"), "{error}");
    assert_exchanges(&[("POST", "/v1/auth/challenge", StatusCode::OK), ("POST", "/v1/contracts/open", StatusCode::CONFLICT)]);
    assert!(sends().iter().all(|params| params == &json!([raw])));
    assert!(fixture.provider.state.ledger.account(&id).unwrap_err().starts_with("not_found:"));
    assert_eq!(counts(), [10_000, 10_001, 0, 0, 0, 0]);
    drop(consumer);
    stop.send(()).unwrap();
    server.await.unwrap();

    // 4. Restart again; a receipt at canonical block 8 below safe block 9 is the only
    //    evidence on which the original is credited, once, with the exact 50 bps fee.
    restart_provider(&mut fixture, &rpc_url);
    observed.lock().unwrap().clear();
    let (stop, server) = serve_observed(fixture.app.clone(), &fixture._listeners[0], observed.clone(), lose_intent.clone());
    rpc.phase.store(2, Ordering::Release);
    rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
    let submitted = sends().len();
    let consumer = Consumer::new(config.clone()).unwrap();
    let account = consumer.resume(&id).await.unwrap();
    assert_eq!((account.status.as_str(), account.funded_wei.as_str(), account.fee_wei.as_str(), account.available_wei.as_str()),
        ("active", "1000000", "5000", "995000"));
    assert_exchanges(&[("POST", "/v1/auth/challenge", StatusCode::OK), ("POST", "/v1/contracts/open", StatusCode::OK)]);
    {
        let exchanges = observed.lock().unwrap();
        assert_eq!(exchanges[1].4["account"], serde_json::to_value(&account).unwrap());
        assert!(exchanges[1].4["credential"].is_object(), "first opening issues one root credential");
    }
    assert_eq!(sends().len(), submitted, "safe acceptance needs no further submission");
    let journal_now = journal_rows(&journal);
    assert_eq!((journal_now.len(), &journal_now[0][..5]), (1, &journal_before[0][..5]));
    assert_eq!(journal_now[0][5], rusqlite::types::Value::Text("accepted".into()));
    let (evidence, status): (String, String) = db.query_row("SELECT evidence,status FROM funding WHERE tx_hash=?1",
        [&envelope.tx_hash], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(status, "accepted");
    assert_eq!(serde_json::from_str::<Value>(&evidence).unwrap(), serde_json::to_value(VerifiedPayment {
        network: original.payload.network.clone(), tx_hash: envelope.tx_hash.clone(), from: envelope.from.clone(),
        to: envelope.to.clone(), value_wei: "1000000".into(), data: protocol::opening_data(&original.payload).unwrap(),
        block_hash: format!("0x{:064x}", 8), block_number: 8, raw_transaction: raw.clone(),
    }).unwrap());
    let fee: (String, String, String) = db.query_row("SELECT platform,amount,status FROM fee_obligations WHERE tx_hash=?1",
        [&envelope.tx_hash], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(fee, (original.payload.platform_wallet.clone(), "5000".into(), "pending".into()));
    assert_eq!(original_row(), (accepted_fields.clone(), 0), "credit keeps the acknowledged agreement intact");
    assert_eq!(counts(), [10_000, 10_001, 1, 1, 1, 1], "credit allocates no lease and no second agreement");
    let exported = consumer.credential_export(&id, None).unwrap();
    drop(consumer);

    // 5. A later fund refreshes the saved account: no new acknowledgement, opening,
    //    credential, submission or credit.
    observed.lock().unwrap().clear();
    rpc.observed_at.store(protocol::now().unwrap(), Ordering::Release);
    let consumer = Consumer::new(config.clone()).unwrap();
    assert_eq!(serde_json::to_value(consumer.fund(&offer, &original).await.unwrap()).unwrap(),
        serde_json::to_value(&account).unwrap());
    assert!(observed.lock().unwrap().iter().all(|(method, path, ..)| method == "GET" || path == "/v1/auth/challenge"),
        "existing-account recovery repeats no acceptance or opening");
    assert!(consumer.credential_export(&id, None).unwrap() == exported, "the exported credential is not rotated");
    assert_eq!(sends().len(), submitted);
    assert_eq!(counts(), [10_000, 10_001, 1, 1, 1, 1]);
    let cancellations: u64 = journal.query_row("SELECT COUNT(*) FROM cancellations", [], |row| row.get(0)).unwrap();
    assert_eq!((journal_rows(&journal).len(), cancellations), (1, 0));
    assert_eq!(quote_resources(), exhausted, "cached recovery and credit spend no quote storage, work or issuance");
    assert!(rpc.unexpected.lock().unwrap().is_empty(), "{:?}", rpc.unexpected.lock().unwrap());
    drop(consumer);
    stop.send(()).unwrap();
    server.await.unwrap();
}
