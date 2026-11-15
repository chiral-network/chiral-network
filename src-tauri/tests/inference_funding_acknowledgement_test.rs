//! Native funding recovery against real cached provider acknowledgements on isolated loopback.
//! The original transaction/acknowledgement are fixture prerequisites, not observed spendability.
//! RPC normally refuses all requests. The ambiguity test supplies synthetic pending observations,
//! never execution/credit, and faults only submission replies after recording the original bytes.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chiral_network::{
    billing_store::BillingStore,
    chain_client::{ChainClient, ChainError},
    consumer::{Consumer, ConsumerConfig},
    contracts,
    protocol::{self, FundingIntent, NetworkIdentity, Quote, Signed},
    wallet,
};
use proposal::PreparedProvider;
use rlp::RlpStream;
use rusqlite::{params, Connection};
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{path::{Path, PathBuf}, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}};
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
    ack_commit_obstruction: Mutex<Option<(PathBuf, PathBuf)>>,
    allow_open: AtomicBool,
    exchanges: Mutex<Vec<Exchange>>,
    unexpected: Mutex<Vec<String>>,
}

async fn intercept(State(peer): State<Arc<Counterparty>>, request: Request, next: Next) -> Response {
    if !request.uri().path().ends_with("/funding-intent") {
        if !matches!(request.uri().path(), "/v1/auth/challenge" | "/v1/contracts/propose")
            && !(request.uri().path() == "/v1/contracts/open" && peer.allow_open.load(Ordering::Acquire)) {
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
    if let Some((account, preserved)) = peer.ack_commit_obstruction.lock().unwrap().take() {
        // Synchronous one-shot obstruction before the genuine reply is released.
        // Keep the prepared file intact; replacing a directory with a file fails
        // even when the test process has permission to write the parent.
        assert!(!preserved.exists());
        std::fs::rename(&account, &preserved).unwrap();
        std::fs::create_dir(&account).unwrap();
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(delivered))
}

struct RpcAttempt {
    method: String,
    params: Value,
    saved: Option<Value>,
    journal: Option<Value>,
}

#[derive(Clone, Copy, Debug)]
enum SubmissionReply {
    RemoteError,
    WrongHash,
    Disconnected,
    ExpectedHash,
}

struct PendingRpc {
    network: NetworkIdentity,
    journal_path: PathBuf,
    reply: SubmissionReply,
}

#[derive(Default)]
struct RpcPeer {
    account_path: Mutex<Option<PathBuf>>,
    attempts: Mutex<Vec<RpcAttempt>>,
    pending: Mutex<Option<PendingRpc>>,
}

fn journal_state(path: &Path) -> Value {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection.prepare(
        "SELECT sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at
         FROM transactions ORDER BY sender,operation").unwrap();
    let transactions = statement.query_map([], |row| Ok(json!({
        "sender": row.get::<_, String>(0)?, "operation": row.get::<_, String>(1)?,
        "nonce": row.get::<_, String>(2)?, "tx_hash": row.get::<_, String>(3)?,
        "raw": row.get::<_, String>(4)?, "status": row.get::<_, String>(5)?,
        "error": row.get::<_, Option<String>>(6)?, "evidence": row.get::<_, Option<String>>(7)?,
        "created_at": row.get::<_, u64>(8)?,
    }))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    let cancellations: u64 = connection.query_row(
        "SELECT COUNT(*) FROM cancellations", [], |row| row.get(0)).unwrap();
    json!({"transactions": transactions, "cancellations": cancellations})
}

async fn fixture_rpc(State(rpc): State<Arc<RpcPeer>>, body: Bytes) -> Response {
    let request: Value = serde_json::from_slice(&body).unwrap();
    let saved = rpc.account_path.lock().unwrap().as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let pending = rpc.pending.lock().unwrap();
    let method = request["method"].as_str().unwrap();
    let journal = pending.as_ref().filter(|_| method == "eth_sendRawTransaction")
        .map(|plan| journal_state(&plan.journal_path));
    rpc.attempts.lock().unwrap().push(RpcAttempt {
        method: method.into(), params: request["params"].clone(), saved, journal,
    });
    let Some(plan) = pending.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "deliberately unavailable fixture RPC").into_response();
    };
    let result = match method {
        "eth_chainId" => json!(format!("0x{:x}", plan.network.chain_id)),
        "eth_syncing" => json!(false),
        "eth_getBlockByNumber" => {
            assert_eq!(request["params"][1], false);
            let (height, hash) = match request["params"][0].as_str().unwrap() {
                "0x0" => (0, plan.network.genesis_hash.clone()),
                "safe" => (10, format!("0x{}", "33".repeat(32))),
                "latest" => (12, format!("0x{}", "11".repeat(32))),
                tag => panic!("unexpected pending-only block observation: {tag}"),
            };
            json!({"number": format!("0x{height:x}"), "hash": hash,
                "timestamp": format!("0x{:x}", protocol::now().unwrap())})
        }
        "eth_getTransactionReceipt" => Value::Null,
        "eth_getTransactionCount" => {
            assert_eq!(request["params"][1], "latest");
            json!("0x0")
        }
        "eth_sendRawTransaction" => match plan.reply {
            SubmissionReply::RemoteError => return Json(json!({
                "jsonrpc": "2.0", "id": request["id"],
                "error": {"code": -32000, "message": "submission outcome unavailable"},
            })).into_response(),
            SubmissionReply::WrongHash => json!(format!("0x{}", "ff".repeat(32))),
            SubmissionReply::Disconnected => return Response::new(Body::from_stream(
                futures_util::stream::once(async {
                    Err::<Bytes, _>(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
                }))),
            SubmissionReply::ExpectedHash => {
                let raw = request["params"][0].as_str().unwrap();
                json!(protocol::hash_hex(&hex::decode(&raw[2..]).unwrap()))
            }
        },
        other => panic!("unexpected pending-only RPC: {other}"),
    };
    Json(json!({"jsonrpc": "2.0", "id": request["id"], "result": result})).into_response()
}

fn signed_opening(quote: &Signed<Quote>, key: &str, nonce: u64) -> String {
    // Independent EIP-155 construction, following the existing router fixture convention.
    let data = protocol::opening_data(&quote.payload).unwrap();
    let to = hex::decode(quote.payload.provider_wallet.strip_prefix("0x").unwrap()).unwrap();
    let value: u64 = quote.payload.funding_wei.parse().unwrap();
    let fields = |stream: &mut RlpStream| {
        stream.append(&nonce).append(&1_u64).append(&1_000_000_u64)
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
    key: String,
    account_path: PathBuf,
    journal_path: PathBuf,
    peer: Arc<Counterparty>,
    rpc: Arc<RpcPeer>,
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
        let rpc = Arc::new(RpcPeer::default());
        let app = contracts::router(provider.state.clone())
            .layer(from_fn_with_state(peer.clone(), intercept))
            // The provider's configured RPC origin is its reserved loopback listener.
            // Add the isolated RPC fixture outside the production control middleware.
            .route("/", post(fixture_rpc).with_state(rpc.clone()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let rpc_app = Router::new().route("/", post(fixture_rpc)).with_state(rpc.clone());
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
        let raw = signed_opening(&quote, &key, 0);
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
        Self { consumer, config, provider, quote, acknowledgement, raw, key, account_path,
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

#[tokio::test]
async fn native_acknowledgement_rejects_positional_objects_before_chain_access() {
    let directory = tempfile::tempdir().unwrap();
    let objects: &[(&str, &[&str])] = &[
        ("", &["payload", "signature"]),
        ("/payload", &["network", "provider_wallet", "consumer_wallet",
            "quote_id", "tx_hash", "registered_at"]),
        ("/payload/network", &["chain_id", "genesis_hash", "deployment_id"]),
    ];
    for (index, (path, fields)) in objects.iter().enumerate() {
        rejected_case(directory.path(), 45_000 + index as u64, path, |mut document| {
            let container = document.pointer_mut(path).unwrap();
            let object = container.as_object_mut().unwrap();
            // Preserve every original field and the genuine signature. These
            // arrays used to decode into the same authenticated typed objects.
            let values = fields.iter().map(|field| object.remove(*field).unwrap()).collect();
            assert!(object.is_empty());
            *container = Value::Array(values);
            serde_json::to_vec(&document).unwrap()
        }).await;
    }
}

#[tokio::test]
async fn native_recovery_refuses_missing_or_inconsistent_original_journal() {
    let directory = tempfile::tempdir().unwrap();
    for (index, missing) in [true, false].into_iter().enumerate() {
        let case = 45_000 + index as u64;
        let mut fixture = Fixture::new(directory.path(), case).await;
        // Obtain a genuine cached acknowledgement and let the unavailable RPC
        // stop the native workflow after durable acknowledgement/start state.
        assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err());
        assert!(!fixture.rpc.attempts.lock().unwrap().is_empty());
        let saved_before = std::fs::read(&fixture.account_path).unwrap();
        let expected_saved: Value = serde_json::from_slice(&saved_before).unwrap();
        assert_eq!(expected_saved["raw_transaction"], fixture.raw);
        assert_eq!(expected_saved["tx_hash"], fixture.acknowledgement.payload.tx_hash);
        assert_eq!(expected_saved["funding_intent"], serde_json::to_value(&fixture.acknowledgement).unwrap());
        assert_eq!(expected_saved["funding_broadcast_started"], true);
        let journal = Connection::open(&fixture.journal_path).unwrap();
        let records = || {
            let mut statement = journal.prepare(
                "SELECT sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at
                 FROM transactions ORDER BY sender,operation").unwrap();
            let rows = statement.query_map([], |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?, row.get::<_, Option<String>>(7)?,
                row.get::<_, u64>(8)?,
            ))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
            rows
        };
        let original_records = records();
        assert_eq!(original_records.len(), 1);
        let original = &original_records[0];
        assert_eq!(original.0, fixture.quote.payload.consumer_wallet);
        assert_eq!(original.1, format!("open:{}", fixture.quote.payload.id));
        assert_eq!(original.2, "0");
        assert_eq!(original.3, fixture.acknowledgement.payload.tx_hash);
        assert_eq!(original.4, fixture.raw);
        if missing {
            assert_eq!(journal.execute("DELETE FROM transactions WHERE sender=?1 AND operation=?2",
                params![original.0, original.1]).unwrap(), 1);
        } else {
            // This competing journal original is correctly signed for the same
            // payer/quote and internally consistent, not malformed-byte refusal.
            let key = format!("{:064x}", 70_000 + case);
            let different = signed_opening(&fixture.quote, &key, 1);
            let envelope = fixture.provider.state.chain.validate_payment_envelope(&different,
                &fixture.quote.payload.consumer_wallet, &fixture.quote.payload.provider_wallet,
                &fixture.quote.payload.funding_wei,
                &protocol::opening_data(&fixture.quote.payload).unwrap()).unwrap();
            assert_eq!(envelope.nonce, 1);
            assert_ne!(different, fixture.raw);
            assert_ne!(envelope.tx_hash, fixture.acknowledgement.payload.tx_hash);
            assert_eq!(journal.execute(
                "UPDATE transactions SET nonce=?3,tx_hash=?4,raw=?5 WHERE sender=?1 AND operation=?2",
                params![original.0, original.1, envelope.nonce.to_string(), envelope.tx_hash, different]).unwrap(), 1);
        }
        let damaged_records = records();
        fixture.peer.exchanges.lock().unwrap().clear();
        fixture.rpc.attempts.lock().unwrap().clear();

        for repeat_fund in [false, true] {
            fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
            let result = if repeat_fund {
                fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await
            } else {
                fixture.consumer.resume(&fixture.quote.payload.id).await
            };
            let name = format!("missing={missing}, repeat_fund={repeat_fund}");
            assert!(result.is_err(), "{name}");
            assert!(fixture.rpc.attempts.lock().unwrap().is_empty(), "{name}: journal refusal must precede RPC");
            assert!(fixture.peer.exchanges.lock().unwrap().is_empty(), "{name}: no replacement acknowledgement");
            assert_eq!(std::fs::read(&fixture.account_path).unwrap(), saved_before, "{name}");
            assert_eq!(records(), damaged_records, "{name}: no recreated or overwritten journal operation");
            fixture.unchanged_provider();
        }

        // Explicit isolated-fixture repair, not an automatic recovery strategy:
        // restoring the original record must make that same payment usable again.
        if missing {
            journal.execute(
                "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![original.0, original.1, original.2, original.3, original.4,
                    original.5, original.6, original.7, original.8]).unwrap();
        } else {
            journal.execute(
                "UPDATE transactions SET nonce=?3,tx_hash=?4,raw=?5 WHERE sender=?1 AND operation=?2",
                params![original.0, original.1, original.2, original.3, original.4]).unwrap();
        }
        fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
        assert!(fixture.consumer.resume(&fixture.quote.payload.id).await.is_err(), "RPC remains unavailable");
        let attempts = fixture.rpc.attempts.lock().unwrap();
        assert!(!attempts.is_empty(), "restored original must pass local recovery and reach RPC");
        for attempt in attempts.iter() {
            assert_ne!(attempt.method, "eth_sendRawTransaction");
            assert_eq!(attempt.saved.as_ref().unwrap(), &expected_saved);
        }
        assert!(fixture.peer.exchanges.lock().unwrap().is_empty(), "saved acknowledgement must be reused");
        assert_eq!(std::fs::read(&fixture.account_path).unwrap(), saved_before);
        assert_eq!(records(), original_records);
        fixture.unchanged_provider();
    }
}

#[tokio::test]
async fn native_broadcast_started_without_acknowledgement_refuses_recovery() {
    let directory = tempfile::tempdir().unwrap();
    for (index, omitted) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 46_000 + index as u64).await;
        assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err());
        {
            let attempts = fixture.rpc.attempts.lock().unwrap();
            assert!(!attempts.is_empty(), "genuine acknowledgement must reach the refusal-only RPC");
            assert!(attempts.iter().all(|attempt| attempt.method != "eth_sendRawTransaction"));
        }
        let saved_before = std::fs::read(&fixture.account_path).unwrap();
        let expected_saved: Value = serde_json::from_slice(&saved_before).unwrap();
        assert_eq!(expected_saved["funding_intent"], serde_json::to_value(&fixture.acknowledgement).unwrap());
        assert_eq!(expected_saved["funding_broadcast_started"], true);
        assert_eq!(expected_saved["raw_transaction"], fixture.raw);
        assert_eq!(expected_saved["tx_hash"], fixture.acknowledgement.payload.tx_hash);
        let journal = Connection::open(&fixture.journal_path).unwrap();
        let records = || {
            let mut statement = journal.prepare(
                "SELECT sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at
                 FROM transactions ORDER BY sender,operation").unwrap();
            let rows = statement.query_map([], |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?, row.get::<_, Option<String>>(7)?,
                row.get::<_, u64>(8)?,
            ))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
            rows
        };
        let original_records = records();
        let mut damaged = expected_saved.clone();
        if omitted {
            damaged.as_object_mut().unwrap().remove("funding_intent").unwrap();
        } else {
            damaged["funding_intent"] = Value::Null;
        }
        let damaged_bytes = serde_json::to_vec(&damaged).unwrap();
        // Deliberately corrupt only this owner-private fixture file, retaining
        // genuine signed terms, raw/hash, journal original and the start marker.
        std::fs::write(&fixture.account_path, &damaged_bytes).unwrap();
        fixture.peer.exchanges.lock().unwrap().clear();
        fixture.rpc.attempts.lock().unwrap().clear();

        for repeat_fund in [false, true] {
            fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
            let result = if repeat_fund {
                fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await
            } else {
                fixture.consumer.resume(&fixture.quote.payload.id).await
            };
            let name = format!("omitted={omitted}, repeat_fund={repeat_fund}");
            assert!(result.is_err(), "{name}");
            assert!(fixture.rpc.attempts.lock().unwrap().is_empty(), "{name}: no payment reconciliation or submission");
            assert!(fixture.peer.exchanges.lock().unwrap().is_empty(), "{name}: invalid saved state must not re-register");
            assert_eq!(std::fs::read(&fixture.account_path).unwrap(), damaged_bytes, "{name}");
            assert_eq!(records(), original_records, "{name}: original nonce ownership must remain intact");
            fixture.unchanged_provider();
        }

        // Explicit test repair, not a product fallback: the genuine saved
        // acknowledgement must still authorize progress for this exact original.
        std::fs::write(&fixture.account_path, &saved_before).unwrap();
        fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
        assert!(fixture.consumer.resume(&fixture.quote.payload.id).await.is_err(), "RPC remains unavailable");
        let attempts = fixture.rpc.attempts.lock().unwrap();
        assert!(!attempts.is_empty(), "restored acknowledgement must permit local recovery");
        for attempt in attempts.iter() {
            assert_ne!(attempt.method, "eth_sendRawTransaction");
            assert_eq!(attempt.saved.as_ref().unwrap(), &expected_saved);
        }
        assert!(fixture.peer.exchanges.lock().unwrap().is_empty(), "reuse the saved acknowledgement");
        assert_eq!(std::fs::read(&fixture.account_path).unwrap(), saved_before);
        assert_eq!(records(), original_records);
        fixture.unchanged_provider();
    }
}

#[tokio::test]
async fn native_acknowledgement_commit_failure_preserves_original_recovery() {
    let directory = tempfile::tempdir().unwrap();
    for (index, repeat_fund) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 47_000 + index as u64).await;
        // Reach the ordinary durable prepared state without accepting a reply.
        // Malformed-response refusal is already covered by the schema tests.
        *fixture.peer.replacement.lock().unwrap() = Some(b"{}".to_vec());
        assert!(fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await.is_err());
        assert!(fixture.rpc.attempts.lock().unwrap().is_empty());
        let prepared_bytes = std::fs::read(&fixture.account_path).unwrap();
        let prepared: Value = serde_json::from_slice(&prepared_bytes).unwrap();
        assert_eq!(prepared["raw_transaction"], fixture.raw);
        assert_eq!(prepared["tx_hash"], fixture.acknowledgement.payload.tx_hash);
        assert_eq!(prepared["quote"], serde_json::to_value(&fixture.quote).unwrap());
        assert!(prepared["funding_intent"].is_null());
        assert_eq!(prepared["funding_broadcast_started"], false);
        let journal = Connection::open(&fixture.journal_path).unwrap();
        let records = || {
            let mut statement = journal.prepare(
                "SELECT sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at
                 FROM transactions ORDER BY sender,operation").unwrap();
            let rows = statement.query_map([], |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?, row.get::<_, Option<String>>(7)?,
                row.get::<_, u64>(8)?,
            ))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
            rows
        };
        let original_records = records();
        let preserved_path = fixture.account_path.with_extension("before-ack");
        *fixture.peer.replacement.lock().unwrap() = None;
        fixture.peer.exchanges.lock().unwrap().clear();
        *fixture.peer.ack_commit_obstruction.lock().unwrap() =
            Some((fixture.account_path.clone(), preserved_path.clone()));
        fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
        let result = if repeat_fund {
            fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await
        } else {
            fixture.consumer.resume(&fixture.quote.payload.id).await
        };
        assert!(result.is_err(), "repeat_fund={repeat_fund}: commit obstruction must stop funding");
        let expected_ack = serde_json::to_value(&fixture.acknowledgement).unwrap();
        {
            let exchanges = fixture.peer.exchanges.lock().unwrap();
            assert_eq!(exchanges.len(), 1, "must reach the real cached acknowledgement");
            assert_eq!(exchanges[0].request, json!({"raw_transaction": fixture.raw}));
            assert_eq!(exchanges[0].original, expected_ack);
            assert_eq!(serde_json::from_slice::<Value>(&exchanges[0].delivered).unwrap(), expected_ack);
        }
        assert!(fixture.peer.ack_commit_obstruction.lock().unwrap().is_none());
        assert!(fixture.account_path.is_dir(), "the target must still obstruct state commit");
        assert_eq!(std::fs::read(&preserved_path).unwrap(), prepared_bytes);
        assert!(fixture.rpc.attempts.lock().unwrap().is_empty(), "failed acknowledgement persistence must precede RPC");
        assert_eq!(records(), original_records);
        fixture.unchanged_provider();

        // Remove only the isolated obstruction and return the intact prepared
        // file. Recovery must fetch the same acknowledgement, never a new payment.
        std::fs::remove_dir(&fixture.account_path).unwrap();
        std::fs::rename(&preserved_path, &fixture.account_path).unwrap();
        fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
        let result = if repeat_fund {
            fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await
        } else {
            fixture.consumer.resume(&fixture.quote.payload.id).await
        };
        assert!(result.is_err(), "RPC remains deliberately unavailable");
        {
            let exchanges = fixture.peer.exchanges.lock().unwrap();
            assert_eq!(exchanges.len(), 2);
            assert_eq!(exchanges[1].request, exchanges[0].request);
            assert_eq!(exchanges[1].original, expected_ack);
            assert_eq!(serde_json::from_slice::<Value>(&exchanges[1].delivered).unwrap(), expected_ack);
        }
        let attempts = fixture.rpc.attempts.lock().unwrap();
        assert!(!attempts.is_empty(), "repair must permit same-original progress to RPC");
        let mut expected_saved = prepared;
        expected_saved["funding_intent"] = expected_ack;
        expected_saved["funding_broadcast_started"] = json!(true);
        for attempt in attempts.iter() {
            assert_ne!(attempt.method, "eth_sendRawTransaction");
            assert_eq!(attempt.saved.as_ref().unwrap(), &expected_saved);
        }
        assert_eq!(fixture.saved(), expected_saved);
        assert_eq!(records(), original_records);
        fixture.unchanged_provider();
    }
}

#[tokio::test]
async fn native_ambiguous_submission_recovers_only_the_durable_original() {
    let directory = tempfile::tempdir().unwrap();
    for (index, reply) in [SubmissionReply::RemoteError, SubmissionReply::WrongHash,
        SubmissionReply::Disconnected].into_iter().enumerate() {
        for repeat_fund in [false, true] {
            let case = 48_000 + 2 * index as u64 + u64::from(repeat_fund);
            let mut fixture = Fixture::new(directory.path(), case).await;
            let original_journal = journal_state(&fixture.journal_path);
            let assert_journal = |mut observed: Value, status: &str| {
                assert_eq!(observed["transactions"].as_array().unwrap().len(), 1);
                let record = &mut observed["transactions"][0];
                assert_eq!(record["status"], status);
                if status == "pending" {
                    assert!(!record["error"].as_str().unwrap().is_empty());
                } else {
                    assert!(record["error"].is_null());
                }
                record["status"] = json!("prepared");
                record["error"] = Value::Null;
                assert_eq!(observed, original_journal, "all original fields and nonce ownership survive");
            };
            *fixture.rpc.pending.lock().unwrap() = Some(PendingRpc {
                network: fixture.quote.payload.network.clone(),
                journal_path: fixture.journal_path.clone(), reply,
            });
            fixture.peer.allow_open.store(true, Ordering::Release);

            for attempt in 0..3 {
                if attempt > 0 {
                    fixture.consumer = Consumer::new(fixture.config.clone()).unwrap();
                }
                if attempt == 2 {
                    // A normal hash reply is still not execution or safe-frontier credit.
                    fixture.rpc.pending.lock().unwrap().as_mut().unwrap().reply = SubmissionReply::ExpectedHash;
                }
                let result = if attempt == 0 || repeat_fund {
                    fixture.consumer.fund(&fixture.provider.offer, &fixture.quote).await
                } else {
                    fixture.consumer.resume(&fixture.quote.payload.id).await
                };
                assert!(result.unwrap_err().contains("funding_pending"),
                    "{reply:?}, repeat_fund={repeat_fund}: provider must still require canonical credit evidence");
                let expected_saved = fixture.saved();
                assert_eq!(expected_saved["quote"], serde_json::to_value(&fixture.quote).unwrap());
                assert_eq!(expected_saved["funding_intent"], serde_json::to_value(&fixture.acknowledgement).unwrap());
                assert_eq!(expected_saved["funding_broadcast_started"], true);
                assert_eq!(expected_saved["raw_transaction"], fixture.raw);
                assert_eq!(expected_saved["tx_hash"], fixture.acknowledgement.payload.tx_hash);
                assert!(expected_saved["account"].is_null());
                assert_eq!(expected_saved["credentials"], json!([]));
                {
                    let attempts = fixture.rpc.attempts.lock().unwrap();
                    let submissions: Vec<_> = attempts.iter()
                        .filter(|call| call.method == "eth_sendRawTransaction").collect();
                    assert_eq!(submissions.len(), attempt + 1, "one send per explicit native attempt, never an automatic retry");
                    let submission = submissions.last().unwrap();
                    assert_eq!(submission.params, json!([fixture.raw]));
                    assert_eq!(submission.saved.as_ref().unwrap(), &expected_saved,
                        "verified acknowledgement and start marker must be durable at submission");
                    assert_journal(submission.journal.clone().unwrap(), if attempt == 0 { "prepared" } else { "pending" });
                }
                assert_journal(journal_state(&fixture.journal_path), if attempt == 2 { "submitted" } else { "pending" });
                let exchanges = fixture.peer.exchanges.lock().unwrap();
                assert_eq!(exchanges.len(), 1, "restart must reuse the saved acknowledgement, not register again");
                assert_eq!(exchanges[0].request, json!({"raw_transaction": fixture.raw}));
                assert_eq!(exchanges[0].original, expected_saved["funding_intent"]);
                drop(exchanges);
                fixture.unchanged_provider();

                if attempt == 0 {
                    let saved_bytes = std::fs::read(&fixture.account_path).unwrap();
                    let chain = ChainClient::new(fixture.config.network.clone(), fixture.config.data_dir.clone()).unwrap();
                    assert!(matches!(chain.prepare_payment_once("different-opening", &fixture.key,
                        &fixture.quote.payload.provider_wallet, &fixture.quote.payload.funding_wei,
                        &protocol::opening_data(&fixture.quote.payload).unwrap()).await,
                        Err(ChainError::Pending(_))), "an ambiguous original must block fresh wallet preparation");
                    assert_journal(journal_state(&fixture.journal_path), "pending");
                    assert_eq!(std::fs::read(&fixture.account_path).unwrap(), saved_bytes);
                    assert_eq!(fixture.rpc.attempts.lock().unwrap().iter()
                        .filter(|call| call.method == "eth_sendRawTransaction").count(), 1);
                }
            }
        }
    }
}
