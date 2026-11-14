//! Native funding recovery against real cached provider acknowledgements on isolated loopback.
//! The original transaction/acknowledgement are fixture prerequisites, not observed spendability.
//! The separate RPC endpoint only refuses requests: no synthetic chain success or live payment.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use chiral_network::{
    billing_store::BillingStore,
    consumer::{Consumer, ConsumerConfig},
    contracts,
    protocol::{self, FundingIntent, Quote, Signed},
    wallet,
};
use proposal::PreparedProvider;
use rlp::RlpStream;
use rusqlite::{params, Connection};
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{path::{Path, PathBuf}, sync::{atomic::Ordering, Arc, Mutex}};
use tiny_keccak::{Hasher, Keccak};
use tokio::{net::TcpListener, task::JoinHandle};

struct Exchange {
    request: Value,
    original: Value,
    delivered: Vec<u8>,
}

#[derive(Default)]
struct Counterparty {
    replacement: Mutex<Option<Vec<u8>>>,
    exchanges: Mutex<Vec<Exchange>>,
    unexpected: Mutex<Vec<String>>,
}

async fn intercept(State(peer): State<Arc<Counterparty>>, request: Request, next: Next) -> Response {
    if !request.uri().path().ends_with("/funding-intent") {
        if !matches!(request.uri().path(), "/v1/auth/challenge" | "/v1/contracts/propose") {
            peer.unexpected.lock().unwrap().push(request.uri().path().into());
        }
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 96 * 1024).await.unwrap();
    let requested = serde_json::from_slice(&body).unwrap();
    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
    assert_eq!(response.status(), StatusCode::OK, "real owner authentication and cached recovery must succeed");
    let (mut parts, body) = response.into_parts();
    let body = to_bytes(body, 96 * 1024).await.unwrap();
    let original = serde_json::from_slice(&body).unwrap();
    let delivered = peer.replacement.lock().unwrap().clone().unwrap_or_else(|| body.to_vec());
    peer.exchanges.lock().unwrap().push(Exchange { request: requested, original, delivered: delivered.clone() });
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(delivered))
}

struct RpcAttempt {
    method: String,
    saved: Option<Value>,
}

#[derive(Default)]
struct RefusingRpc {
    account_path: Mutex<Option<PathBuf>>,
    attempts: Mutex<Vec<RpcAttempt>>,
}

async fn refuse_rpc(State(rpc): State<Arc<RefusingRpc>>, body: Bytes) -> Response {
    let request: Value = serde_json::from_slice(&body).unwrap();
    let saved = rpc.account_path.lock().unwrap().as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    rpc.attempts.lock().unwrap().push(RpcAttempt {
        method: request["method"].as_str().unwrap().into(), saved,
    });
    (StatusCode::SERVICE_UNAVAILABLE, "deliberately unavailable fixture RPC").into_response()
}

fn signed_opening(quote: &Signed<Quote>, key: &str) -> String {
    // Independent EIP-155 construction, following the existing router fixture convention.
    let data = protocol::opening_data(&quote.payload).unwrap();
    let to = hex::decode(quote.payload.provider_wallet.strip_prefix("0x").unwrap()).unwrap();
    let value: u64 = quote.payload.funding_wei.parse().unwrap();
    let fields = |stream: &mut RlpStream| {
        stream.append(&0_u64).append(&1_u64).append(&1_000_000_u64)
            .append(&to.as_slice()).append(&value).append(&data.as_slice());
    };
    let chain_id = quote.payload.network.chain_id;
    let mut unsigned = RlpStream::new_list(9);
    fields(&mut unsigned);
    unsigned.append(&chain_id).append(&0_u8).append(&0_u8);
    let mut hash = [0_u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(&unsigned.out());
    keccak.finalize(&mut hash);
    let key = SecretKey::from_slice(&hex::decode(key).unwrap()).unwrap();
    let (recovery, signature) = Secp256k1::signing_only()
        .sign_ecdsa_recoverable(&Message::from_digest_slice(&hash).unwrap(), &key).serialize_compact();
    let mut signed = RlpStream::new_list(9);
    fields(&mut signed);
    signed.append(&(chain_id * 2 + 35 + recovery.to_i32() as u64));
    for limb in [&signature[..32], &signature[32..]] {
        let first = limb.iter().position(|byte| *byte != 0).unwrap_or(limb.len());
        signed.append(&&limb[first..]);
    }
    format!("0x{}", hex::encode(signed.out()))
}

struct Fixture {
    consumer: Consumer,
    config: ConsumerConfig,
    provider: PreparedProvider,
    quote: Signed<Quote>,
    acknowledgement: Signed<FundingIntent>,
    raw: String,
    account_path: PathBuf,
    journal_path: PathBuf,
    peer: Arc<Counterparty>,
    rpc: Arc<RefusingRpc>,
    servers: [JoinHandle<()>; 2],
    _admin: TcpListener,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for server in &self.servers { server.abort(); }
    }
}

impl Fixture {
    async fn new(directory: &Path, case: u64) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let admin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = PreparedProvider::new(directory, case, "1", "2",
            listener.local_addr().unwrap(), admin.local_addr().unwrap());
        let peer = Arc::new(Counterparty::default());
        let app = contracts::router(provider.state.clone()).layer(from_fn_with_state(peer.clone(), intercept));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let rpc = Arc::new(RefusingRpc::default());
        let rpc_app = Router::new().route("/", post(refuse_rpc)).with_state(rpc.clone());
        let mut network = provider.state.config.network.clone();
        network.rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
        let rpc_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });
        let config = ConsumerConfig { network, data_dir: directory.join(format!("consumer-{case}")),
            catalog_url: None, development: true, dht: None };
        let consumer = Consumer::new(config.clone()).unwrap();
        // Public test scalar, never funded or deployed; import uses an owner-private file.
        let key = format!("{:064x}", 70_000 + case);
        let import = config.data_dir.join("public-test-scalar.key");
        wallet::save_private_key(&import, &key).unwrap();
        consumer.wallet_import_file(&import).unwrap();
        let quote = consumer.quote(&provider.offer, "100").await.unwrap();
        let raw = signed_opening(&quote, &key);
        let envelope = provider.state.chain.validate_payment_envelope(&raw,
            &quote.payload.consumer_wallet, &quote.payload.provider_wallet,
            &quote.payload.funding_wei, &protocol::opening_data(&quote.payload).unwrap()).unwrap();
        // Restore one already prepared original at the journal boundary. No preparation RPC,
        // balance/nonce observation or successful chain admission is claimed by this fixture.
        let journal_path = config.network.scoped_dir(&config.data_dir).join("wallet-journal/transactions.sqlite3");
        Connection::open(&journal_path).unwrap().execute(
            "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at) VALUES (?1,?2,?3,?4,?5,'prepared',?6)",
            params![quote.payload.consumer_wallet, format!("open:{}", quote.payload.id),
                envelope.nonce.to_string(), envelope.tx_hash, raw, quote.payload.issued_at]).unwrap();
        // Trusted post-spendability ledger boundary, not a manufactured canonical payment.
        let acknowledgement = provider.state.ledger.register_funding_intent(&quote,
            &quote.payload.consumer_wallet, &raw, &envelope.tx_hash, envelope.nonce,
            &provider.state.private_key, protocol::now, true, &quote.payload.model).unwrap();
        provider.state.accepting.store(false, Ordering::Release);
        let account_path = config.network.scoped_dir(&config.data_dir).join("consumer")
            .join(&quote.payload.consumer_wallet).join(format!("{}.json", hex::encode(protocol::digest(quote.payload.id.as_bytes()))));
        *rpc.account_path.lock().unwrap() = Some(account_path.clone());
        Self { consumer, config, provider, quote, acknowledgement, raw, account_path,
            journal_path, peer, rpc, servers: [server, rpc_server], _admin: admin }
    }

    fn saved(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.account_path).unwrap()).unwrap()
    }

    fn unchanged_provider(&self) {
        let ledger = BillingStore::open(&self.provider.ledger_path, &self.quote.payload.network).unwrap();
        assert_eq!(serde_json::to_value(ledger.funding_intent(&self.quote.payload, &self.raw).unwrap()).unwrap(),
            serde_json::to_value(&self.acknowledgement).unwrap());
        assert_eq!(serde_json::to_value(ledger.quote(&self.quote.payload.id).unwrap()).unwrap(),
            serde_json::to_value(&self.quote).unwrap());
        assert!(ledger.account(&self.quote.payload.id).unwrap_err().starts_with("not_found:"));
        let rows: (u64, u64) = Connection::open(&self.provider.ledger_path).unwrap().query_row(
            "SELECT (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations)",
            [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(rows, (0, 0));
        assert!(self.peer.unexpected.lock().unwrap().is_empty());
    }
}

async fn rejected_case(directory: &Path, case: u64, name: &str, malformed: impl FnOnce(Value) -> Vec<u8>) {
    let mut fixture = Fixture::new(directory, case).await;
    let expected_ack = serde_json::to_value(&fixture.acknowledgement).unwrap();
    let bad = malformed(expected_ack.clone());
    *fixture.peer.replacement.lock().unwrap() = Some(bad.clone());
    assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err(), "{name}");
    {
        let exchanges = fixture.peer.exchanges.lock().unwrap();
        assert_eq!(exchanges.len(), 1, "{name}: must reach the real acknowledgement handler");
        assert_eq!(exchanges[0].request, json!({"raw_transaction": fixture.raw}), "{name}");
        assert_eq!(exchanges[0].original, expected_ack, "{name}");
        assert_eq!(exchanges[0].delivered, bad, "{name}");
    }
    assert!(fixture.rpc.attempts.lock().unwrap().is_empty(), "{name}: invalid acknowledgement reached chain RPC");
    let saved = fixture.saved();
    assert_eq!(saved["quote"], serde_json::to_value(&fixture.quote).unwrap(), "{name}");
    assert_eq!(saved["raw_transaction"], fixture.raw, "{name}");
    assert_eq!(saved["tx_hash"], fixture.acknowledgement.payload.tx_hash, "{name}");
    assert!(saved["funding_intent"].is_null(), "{name}");
    assert_eq!(saved["funding_broadcast_started"], false, "{name}");
    assert!(saved["account"].is_null(), "{name}");
    assert_eq!(saved["credentials"], json!([]), "{name}");
    let journal = Connection::open(&fixture.journal_path).unwrap();
    let original: (String, String, String, String) = journal.query_row(
        "SELECT raw,tx_hash,nonce,status FROM transactions WHERE sender=?1 AND operation=?2",
        params![fixture.quote.payload.consumer_wallet, format!("open:{}", fixture.quote.payload.id)],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
    assert_eq!(original, (fixture.raw.clone(), fixture.acknowledgement.payload.tx_hash.clone(), "0".into(), "prepared".into()), "{name}");
    assert_eq!(journal.query_row("SELECT COUNT(*) FROM transactions", [], |row| row.get::<_, u64>(0)).unwrap(), 1, "{name}");
    fixture.unchanged_provider();

    // Reopen the native consumer, restore the genuine reply and recover the same operation.
    // Valid acceptance must reach the refusal-only RPC, not silently fail for all replies.
    fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
    assert_eq!(fixture.consumer.pending_accounts().unwrap()["pending"][0]["funding_stage"], "registration_pending", "{name}");
    *fixture.peer.replacement.lock().unwrap() = None;
    assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err(), "{name}: RPC is deliberately unavailable");
    {
        let attempts = fixture.rpc.attempts.lock().unwrap();
        assert!(!attempts.is_empty(), "{name}: valid acknowledgement did not advance to RPC");
        for attempt in attempts.iter() {
            assert_ne!(attempt.method, "eth_sendRawTransaction", "{name}: fixture must not submit a transaction");
            let saved = attempt.saved.as_ref().expect("native evidence must precede RPC");
            assert_eq!(saved["funding_intent"], expected_ack, "{name}");
            assert_eq!(saved["funding_broadcast_started"], true, "{name}");
            assert_eq!(saved["raw_transaction"], fixture.raw, "{name}");
            assert_eq!(saved["tx_hash"], fixture.acknowledgement.payload.tx_hash, "{name}");
        }
    }
    let exchanges = fixture.peer.exchanges.lock().unwrap();
    assert_eq!(exchanges.len(), 2, "{name}");
    assert_eq!(exchanges[1].request, exchanges[0].request, "{name}: recovery must retain the original bytes");
    assert_eq!(exchanges[1].original, expected_ack, "{name}");
    assert_eq!(serde_json::from_slice::<Value>(&exchanges[1].delivered).unwrap(), expected_ack, "{name}");
    fixture.unchanged_provider();
}

const FIELDS: &[(&str, &str)] = &[
    ("", "payload"), ("", "signature"),
    ("/payload", "network"), ("/payload", "provider_wallet"), ("/payload", "consumer_wallet"),
    ("/payload", "quote_id"), ("/payload", "tx_hash"), ("/payload", "registered_at"),
    ("/payload/network", "chain_id"), ("/payload/network", "genesis_hash"), ("/payload/network", "deployment_id"),
];

#[tokio::test]
async fn native_acknowledgement_rejects_unknown_object_members_before_chain_access() {
    let directory = tempfile::tempdir().unwrap();
    for (index, path) in ["", "/payload", "/payload/network"].into_iter().enumerate() {
        rejected_case(directory.path(), 40_000 + index as u64, path, |mut document| {
            document.pointer_mut(path).unwrap().as_object_mut().unwrap().insert("unsupported".into(), json!(true));
            serde_json::to_vec(&document).unwrap()
        }).await;
    }
}

#[tokio::test]
async fn native_acknowledgement_rejects_missing_object_members_before_chain_access() {
    let directory = tempfile::tempdir().unwrap();
    for (index, (path, field)) in FIELDS.iter().enumerate() {
        rejected_case(directory.path(), 41_000 + index as u64, &format!("{path}/{field}"), |mut document| {
            document.pointer_mut(path).unwrap().as_object_mut().unwrap().remove(*field).unwrap();
            serde_json::to_vec(&document).unwrap()
        }).await;
    }
}

#[tokio::test]
async fn native_acknowledgement_rejects_mistyped_object_members_before_chain_access() {
    let directory = tempfile::tempdir().unwrap();
    for (index, (path, field)) in FIELDS.iter().enumerate() {
        rejected_case(directory.path(), 42_000 + index as u64, &format!("{path}/{field}"), |mut document| {
            let value = &mut document.pointer_mut(path).unwrap()[*field];
            // Keep numeric text identical: permissive coercion must not make it acceptable.
            *value = if value.is_u64() { json!(value.to_string()) } else { json!(false) };
            serde_json::to_vec(&document).unwrap()
        }).await;
    }
    for (index, value) in [Value::Null, json!(-1), json!(1.5), json!([]), json!({})].into_iter().enumerate() {
        rejected_case(directory.path(), 43_000 + index as u64, &format!("registered_at={value}"), |mut document| {
            document["payload"]["registered_at"] = value;
            serde_json::to_vec(&document).unwrap()
        }).await;
    }
}

#[tokio::test]
async fn native_acknowledgement_rejects_duplicate_object_members_before_chain_access() {
    let directory = tempfile::tempdir().unwrap();
    for (index, (path, field)) in [("", "signature"), ("/payload", "tx_hash"), ("/payload/network", "chain_id")].into_iter().enumerate() {
        rejected_case(directory.path(), 44_000 + index as u64, &format!("{path}/{field}"), |document| {
            let member = format!("\"{field}\":{}", document.pointer(path).unwrap()[field]);
            serde_json::to_string(&document).unwrap().replacen(&member, &format!("{member},{member}"), 1).into_bytes()
        }).await;
    }
}
