use super::*;
use axum::{extract::State, routing::post, Json, Router};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
const CHAIN: u64 = crate::network::ESPACE_TESTNET_CHAIN_ID;
fn key() -> String {
    "01".repeat(32)
}
fn payee() -> String {
    format!("0x{}", "22".repeat(20))
}
// Matches the public testnet native-transfer estimates observed during the cutover.
// Production uses eth_estimateGas, not this fixture's calldata pricing.
fn native_gas(data: &[u8]) -> u64 {
    21000 + data.iter().map(|byte| if *byte == 0 { 10 } else { 40 }).sum::<u64>()
}
fn sign_transaction(
    key: &str, chain_id: u64, nonce: u64, to: &str, value: u128,
    data: &[u8], gas_price: u128,
) -> Result<SubmittedTransaction, String> {
    super::sign_transaction(key, chain_id, nonce, to, value, data, gas_price, native_gas(data))
}
fn config(url: String) -> NetworkConfig {
    NetworkConfig {
        identity: NetworkIdentity {
            chain_id: CHAIN,
            genesis_hash: crate::network::ESPACE_TESTNET_GENESIS.into(),
            deployment_id: "unit-test".into(),
        },
        rpc_url: url,
        rpc_bearer_file: None,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    }
}
fn legacy() -> SubmittedTransaction {
    sign_transaction(&key(), CHAIN, 0, &payee(), u128::MAX, b"terms\0", 1001).unwrap()
}
fn typed(kind: u8) -> String {
    let count = if kind == 1 { 8 } else { 9 };
    let fields = |s: &mut RlpStream| {
        s.append(&CHAIN);
        s.append(&0u8);
        if kind == 2 {
            s.append(&1u8);
        }
        s.append(&1001u64);
        s.append(&25320u64);
        s.append(&vec![0x22u8; 20]);
        append_uint(s, U256::from(u128::MAX));
        s.append(&b"x\0".as_slice());
        s.begin_list(1);
        s.begin_list(2);
        s.append(&vec![0x44u8; 20]);
        s.begin_list(1);
        s.append(&vec![0x55u8; 32]);
    };
    let mut unsigned = RlpStream::new_list(count);
    fields(&mut unsigned);
    let mut message = vec![kind];
    message.extend_from_slice(&unsigned.out());
    let (id, sig) = secp256k1::Secp256k1::new()
        .sign_ecdsa_recoverable(
            &secp256k1::Message::from_digest(protocol::digest(&message)),
            &wallet::secret(&key()).unwrap(),
        )
        .serialize_compact();
    let mut signed = RlpStream::new_list(count + 3);
    fields(&mut signed);
    signed.append(&(id.to_i32() as u8));
    append_uint(&mut signed, U256::from_big_endian(&sig[..32]));
    append_uint(&mut signed, U256::from_big_endian(&sig[32..]));
    let mut raw = vec![kind];
    raw.extend_from_slice(&signed.out());
    format!("0x{}", hex::encode(raw))
}
fn replace_field(raw: &str, index: usize, replacement: &[u8]) -> String {
    let bytes = hex::decode(&raw[2..]).unwrap();
    let typed = bytes[0] < 0x80;
    let rlp = Rlp::new(if typed { &bytes[1..] } else { &bytes });
    let count = rlp.item_count().unwrap();
    let mut stream = RlpStream::new_list(count);
    for i in 0..count {
        if i == index {
            stream.append_raw(replacement, 1);
        } else {
            stream.append_raw(rlp.at(i).unwrap().as_raw(), 1);
        }
    }
    let mut raw = Vec::new();
    if typed {
        raw.push(bytes[0]);
    }
    raw.extend_from_slice(&stream.out());
    format!("0x{}", hex::encode(raw))
}
#[test]
fn signed_envelopes_recover_exact_money_and_terms() {
    let raw = legacy();
    let decoded = decode(&raw.raw_transaction, CHAIN).unwrap();
    assert_eq!(
        decoded.from,
        wallet::address_from_private_key(&key()).unwrap()
    );
    assert_eq!(decoded.value, u128::MAX);
    assert_eq!(decoded.data, b"terms\0");
    assert_eq!(decoded.tx_hash, raw.tx_hash);
    assert_eq!(decoded.gas, 21210);
    for kind in [1, 2] {
        let tx = decode(&typed(kind), CHAIN).unwrap();
        assert_eq!(tx.kind, kind);
        assert_eq!(tx.from, decoded.from);
        assert_eq!(tx.value, u128::MAX);
        assert_eq!(tx.data, b"x\0");
    }
}
#[test]
fn envelope_validation_is_local_and_binds_every_payment_field() {
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config("http://127.0.0.1:1".into()), dir.path().into()).unwrap();
    let from = wallet::address_from_private_key(&key()).unwrap();
    let tx = legacy();
    let envelope = client
        .validate_payment_envelope(
            &tx.raw_transaction,
            &from,
            &payee(),
            &u128::MAX.to_string(),
            b"terms\0",
        )
        .unwrap();
    assert_eq!(envelope.tx_hash, tx.tx_hash);
    assert_eq!(envelope.from, from);
    assert_eq!(envelope.to, payee());
    assert_eq!(envelope.value_wei, u128::MAX.to_string());
    assert_eq!(envelope.nonce, 0);
    assert_eq!(envelope.data, b"terms\0");
    assert!(client
        .validate_payment_envelope(
            &tx.raw_transaction,
            &payee(),
            &payee(),
            &u128::MAX.to_string(),
            b"terms\0"
        )
        .is_err());
    assert!(client
        .validate_payment_envelope(
            &tx.raw_transaction,
            &from,
            &from,
            &u128::MAX.to_string(),
            b"terms\0"
        )
        .is_err());
    assert!(client
        .validate_payment_envelope(&tx.raw_transaction, &from, &payee(), "1", b"terms\0")
        .is_err());
    assert!(client
        .validate_payment_envelope(
            &tx.raw_transaction,
            &from,
            &payee(),
            &u128::MAX.to_string(),
            b"other"
        )
        .is_err());
    let wrong_chain = sign_transaction(&key(), CHAIN + 1, 0, &payee(), 1, b"terms", 1001).unwrap();
    assert!(client
        .validate_payment_envelope(&wrong_chain.raw_transaction, &from, &payee(), "1", b"terms")
        .is_err());
    for kind in [1, 2] {
        assert!(client
            .validate_payment_envelope(
                &typed(kind),
                &from,
                &payee(),
                &u128::MAX.to_string(),
                b"x\0"
            )
            .is_ok());
    }
}
#[test]
fn rejects_cross_chain_malleable_noncanonical_and_trailing_evidence() {
    for raw in [legacy().raw_transaction, typed(1), typed(2)] {
        assert!(decode(&raw, CHAIN + 1).is_err());
        assert!(decode(&format!("{raw}00"), CHAIN).is_err());
        let nonce_i = if raw.starts_with("0x01") || raw.starts_with("0x02") {
            1
        } else {
            0
        };
        assert!(decode(&replace_field(&raw, nonce_i, &[0]), CHAIN).is_err());
        assert!(decode(&replace_field(&raw, nonce_i, &[0x81, 1]), CHAIN).is_err());
        let bytes = hex::decode(&raw[2..]).unwrap();
        let kind = if bytes[0] < 0x80 { bytes[0] } else { 0 };
        let rlp = Rlp::new(if kind == 0 { &bytes } else { &bytes[1..] });
        let s_i = rlp.item_count().unwrap() - 1;
        let s = uint(&rlp, s_i, 32).unwrap();
        let order = U256::from_big_endian(
            &hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                .unwrap(),
        );
        let mut high = RlpStream::new();
        append_uint(&mut high, order - s);
        assert!(decode(&replace_field(&raw, s_i, &high.out()), CHAIN).is_err());
    }
    let raw = legacy().raw_transaction;
    assert!(decode(&replace_field(&raw, 3, &[0x80]), CHAIN).is_err());
    assert!(decode(&replace_field(&raw, 6, &[27]), CHAIN).is_err());
    let mut huge = RlpStream::new();
    huge.append(&vec![1u8; 17]);
    assert!(decode(&replace_field(&raw, 4, &huge.out()), CHAIN).is_err());
    let mut lowgas = RlpStream::new();
    lowgas.append(&21000u64);
    assert!(decode(&replace_field(&raw, 2, &lowgas.out()), CHAIN).is_err());
}
#[test]
fn exact_intrinsic_gas_and_strict_wei() {
    assert_eq!(
        intrinsic(&[0, 1, 0xff], 2, 3).unwrap(),
        21000 + 4 + 32 + 4800 + 5700
    );
    assert!(intrinsic(&[], u64::MAX, 0).is_err());
    assert_eq!(wei(&u128::MAX.to_string()).unwrap(), u128::MAX);
    for bad in ["01", "-1", "1.0", "1e18", " 1", ""] {
        assert!(wei(bad).is_err());
    }
    let total = U256::from(u128::MAX) * U256::from(21000u64) + U256::from(u128::MAX);
    assert!(total > U256::from(u128::MAX));
}
struct Fixture {
    receipt: Mutex<Value>,
    submissions: Mutex<Vec<String>>,
    nonce: AtomicU64,
    syncing: AtomicBool,
    frontier: AtomicU64,
    reads: AtomicU64,
    displace_second: AtomicBool,
    journal: Mutex<Option<PathBuf>>,
    persisted_before_broadcast: AtomicBool,
    ambiguous_submit: AtomicBool,
    balance: Mutex<u128>,
    gas_price: Mutex<u128>,
    regress_on_nonce_read: AtomicBool,
    pending_nonce: Mutex<Option<u64>>,
    chain_id: AtomicU64,
    genesis: Mutex<String>,
    genesis_reads: AtomicU64,
    malformed_frontier: AtomicBool,
    syncing_unavailable: AtomicBool,
    canonical_hash: Mutex<Option<String>>,
    observed_at: u64,
    head_age: AtomicU64,
    estimate_override: Mutex<Option<Value>>,
    block_transactions: AtomicU64,
    response_override: Mutex<Option<(String, Value)>>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            receipt: Mutex::new(Value::Null),
            submissions: Mutex::new(vec![]),
            nonce: AtomicU64::new(0),
            syncing: AtomicBool::new(false),
            frontier: AtomicU64::new(10),
            reads: AtomicU64::new(0),
            displace_second: AtomicBool::new(false),
            journal: Mutex::new(None),
            persisted_before_broadcast: AtomicBool::new(false),
            ambiguous_submit: AtomicBool::new(false),
            balance: Mutex::new(u128::MAX),
            gas_price: Mutex::new(1001),
            regress_on_nonce_read: AtomicBool::new(false),
            pending_nonce: Mutex::new(None),
            chain_id: AtomicU64::new(CHAIN),
            genesis: Mutex::new(crate::network::ESPACE_TESTNET_GENESIS.into()),
            genesis_reads: AtomicU64::new(0),
            malformed_frontier: AtomicBool::new(false),
            syncing_unavailable: AtomicBool::new(false),
            canonical_hash: Mutex::new(None),
            observed_at: protocol::now().unwrap(),
            head_age: AtomicU64::new(0),
            estimate_override: Mutex::new(None),
            block_transactions: AtomicU64::new(0),
            response_override: Mutex::new(None),
        }
    }
}
async fn fixture_rpc(State(f): State<Arc<Fixture>>, Json(request): Json<Value>) -> Json<Value> {
    if let Some((method, response)) = f.response_override.lock().unwrap().as_ref() {
        if request["method"] == method.as_str() {
            let mut response = response.clone();
            response["id"] = request["id"].clone();
            return Json(response);
        }
    }
    let result = match request["method"].as_str().unwrap() {
        "eth_chainId" => json!(format!("0x{:x}", f.chain_id.load(Ordering::SeqCst))),
        "eth_syncing" => {
            if f.syncing_unavailable.load(Ordering::SeqCst) {
                return Json(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"sync health unavailable"}}));
            }
            json!(f.syncing.load(Ordering::SeqCst))
        }
        "eth_getBlockByNumber" => {
            assert_eq!(request["params"][1], false);
            let tag = request["params"][0].as_str().unwrap();
            if tag == "safe" && f.malformed_frontier.load(Ordering::SeqCst) {
                return Json(json!({"jsonrpc":"2.0","id":request["id"],"result":null}));
            }
            let height = match tag {
                "latest" => 12,
                "safe" => f.frontier.load(Ordering::SeqCst),
                "finalized" => 6,
                _ => rpc_client::hex_to_u64(tag).unwrap(),
            };
            let hash = if height == 0 {
                f.genesis_reads.fetch_add(1, Ordering::SeqCst);
                f.genesis.lock().unwrap().clone()
            } else if height == 12 {
                format!("0x{}", "11".repeat(32))
            } else if height == 10 {
                format!("0x{}", "33".repeat(32))
            } else {
                f.canonical_hash.lock().unwrap().clone().unwrap_or_else(|| {
                    format!("0x{}", (if height == 9 { "77" } else if height == 8 { "88" } else { "55" }).repeat(32))
                })
            };
            let timestamp = f.observed_at.saturating_sub(f.head_age.load(Ordering::SeqCst)).saturating_sub(12u64.saturating_sub(height));
            let transactions: Vec<String> = (0..f.block_transactions.load(Ordering::SeqCst))
                .map(|index| format!("0x{index:064x}"))
                .collect();
            json!({"number":format!("0x{height:x}"),"hash":hash,"timestamp":format!("0x{timestamp:x}"),"transactions":transactions})
        }
        "eth_getTransactionReceipt" => {
            let count = f.reads.fetch_add(1, Ordering::SeqCst);
            let receipt = f.receipt.lock().unwrap();
            if (f.displace_second.load(Ordering::SeqCst) && count > 0)
                || receipt["transactionHash"] != request["params"][0]
            {
                Value::Null
            } else {
                receipt.clone()
            }
        }
        "eth_getTransactionCount" => {
            let tag = request["params"][1].as_str().unwrap();
            assert!(matches!(tag, "latest" | "pending"));
            if f.regress_on_nonce_read.swap(false, Ordering::SeqCst) {
                f.nonce.store(0, Ordering::SeqCst);
                *f.receipt.lock().unwrap() = Value::Null;
                *f.canonical_hash.lock().unwrap() = Some(format!("0x{}", "ff".repeat(32)));
            }
            let latest = f.nonce.load(Ordering::SeqCst);
            let nonce = if tag == "pending" { f.pending_nonce.lock().unwrap().unwrap_or(latest) } else { latest };
            json!(format!("0x{nonce:x}"))
        }
        "eth_getBalance" => {
            assert_eq!(request["params"][1], "latest");
            json!(format!("0x{:x}", *f.balance.lock().unwrap()))
        }
        "eth_gasPrice" => json!(format!("0x{:x}", *f.gas_price.lock().unwrap())),
        "eth_estimateGas" => {
            let transaction = &request["params"][0];
            assert_eq!(transaction["gas"], format!("0x{MAX_ESTIMATE_GAS:x}"));
            f.estimate_override.lock().unwrap().clone().unwrap_or_else(|| {
                let data = hex::decode(transaction["data"].as_str().unwrap().strip_prefix("0x").unwrap()).unwrap();
                json!(format!("0x{:x}", native_gas(&data)))
            })
        }
        "eth_sendRawTransaction" => {
            let raw = request["params"][0].as_str().unwrap();
            if let Some(path) = f.journal.lock().unwrap().as_ref() {
                let connection = Connection::open(path).unwrap();
                let count:u64=connection.query_row("SELECT (SELECT COUNT(*) FROM transactions WHERE raw=?1)+(SELECT COUNT(*) FROM cancellations WHERE raw=?1)",[raw],|r|r.get(0)).unwrap();
                f.persisted_before_broadcast
                    .store(count == 1, Ordering::SeqCst);
            }
            f.submissions.lock().unwrap().push(raw.to_owned());
            if f.ambiguous_submit.load(Ordering::SeqCst) {
                return Json(
                    json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"already admitted or pool full"}}),
                );
            }
            json!(protocol::hash_hex(&hex::decode(&raw[2..]).unwrap()))
        }
        other => panic!("unsupported RPC assumption: {other}"),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}
async fn fixture() -> (Arc<Fixture>, NetworkConfig, tokio::task::JoinHandle<()>) {
    let fixture = Arc::new(Fixture::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = config(format!("http://{}", listener.local_addr().unwrap()));
    let app = Router::new()
        .route("/", post(fixture_rpc))
        .with_state(fixture.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (fixture, config, task)
}
fn receipt(tx: &SubmittedTransaction) -> Value {
    let decoded = decode(&tx.raw_transaction, CHAIN).unwrap();
    json!({"transactionHash":tx.tx_hash,"from":decoded.from,"to":decoded.to,"type":format!("0x{:x}",decoded.kind),"gasUsed":format!("0x{:x}",decoded.gas),"status":"0x1","blockHash":format!("0x{}","77".repeat(32)),"blockNumber":"0x9"})
}
#[tokio::test]
async fn canonical_receipts_safe_and_reorg_boundaries() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let tx = legacy();
    *fixture.receipt.lock().unwrap() = receipt(&tx);
    let from = wallet::address_from_private_key(&key()).unwrap();
    let payment = client
        .verify_payment(
            &tx.raw_transaction,
            &from,
            &payee(),
            Some(&u128::MAX.to_string()),
            b"terms\0",
        )
        .await
        .unwrap();
    assert!(fixture.reads.load(Ordering::SeqCst) >= 2);
    assert!(client
        .verify_payment(&tx.raw_transaction, &from, &payee(), Some("1"), b"terms\0")
        .await
        .is_err());
    fixture.frontier.store(8, Ordering::SeqCst);
    assert_eq!(
        client.recheck_payment(&payment).await.unwrap(),
        PaymentState::Pending
    );
    fixture.frontier.store(10, Ordering::SeqCst);
    fixture.syncing.store(true, Ordering::SeqCst);
    assert!(matches!(
        client.recheck_payment(&payment).await,
        Err(ChainError::Unavailable(_))
    ));
    fixture.syncing.store(false, Ordering::SeqCst);
    *fixture.receipt.lock().unwrap() = Value::Null;
    assert!(matches!(client.recheck_payment(&payment).await, Err(ChainError::Unavailable(_))));
    *fixture.canonical_hash.lock().unwrap() = Some(format!("0x{}", "ff".repeat(32)));
    assert_eq!(
        client.recheck_payment(&payment).await.unwrap(),
        PaymentState::Orphaned
    );
    *fixture.receipt.lock().unwrap() = receipt(&tx);
    *fixture.canonical_hash.lock().unwrap() = None;
    fixture.reads.store(0, Ordering::SeqCst);
    fixture.displace_second.store(true, Ordering::SeqCst);
    assert!(matches!(
        client
            .verify_payment(&tx.raw_transaction, &from, &payee(), None, b"terms\0")
            .await,
        Err(ChainError::Pending(_))
    ));
    task.abort();
}
#[tokio::test]
async fn durable_intent_restart_and_nonce_serialization() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    *fixture.journal.lock().unwrap() = Some(client.dir.join("transactions.sqlite3"));
    let tx = client
        .send_payment_once("fund-1", &key(), &payee(), "1", b"commitment")
        .await
        .unwrap();
    assert_eq!(tx.status, "submitted");
    assert!(fixture.persisted_before_broadcast.load(Ordering::SeqCst));
    assert!(matches!(
        client
            .send_payment_once("fund-2", &key(), &payee(), "2", b"other")
            .await,
        Err(ChainError::Pending(_))
    ));
    assert!(matches!(
        client
            .send_payment_once("fund-1", &key(), &payee(), "2", b"commitment")
            .await,
        Err(ChainError::Rejected(_))
    ));
    drop(client);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let resumed = client
        .send_payment_once("fund-1", &key(), &payee(), "1", b"commitment")
        .await
        .unwrap();
    assert_eq!(resumed.raw_transaction, tx.raw_transaction);
    assert_eq!(fixture.submissions.lock().unwrap().len(), 2);
    *fixture.receipt.lock().unwrap() = receipt(&tx);
    fixture.nonce.store(1, Ordering::SeqCst);
    let accepted = client
        .send_payment_once("fund-1", &key(), &payee(), "1", b"commitment")
        .await
        .unwrap();
    assert_eq!(accepted.status, "accepted");
    assert_eq!(fixture.submissions.lock().unwrap().len(), 2);
    let history = client
        .history(&wallet::address_from_private_key(&key()).unwrap())
        .await
        .unwrap();
    assert_eq!(history["complete_chain_history"], false);
    assert_eq!(history["transactions"].as_array().unwrap().len(), 1);
    *fixture.receipt.lock().unwrap() = Value::Null;
    *fixture.canonical_hash.lock().unwrap() = Some(format!("0x{}", "ff".repeat(32)));
    assert!(matches!(
        client
            .send_payment_once("fund-1", &key(), &payee(), "1", b"commitment")
            .await,
        Err(ChainError::Rejected(_))
    ));
    assert_eq!(fixture.submissions.lock().unwrap().len(), 2);
    task.abort();
}
#[tokio::test]
async fn prepare_is_durable_never_broadcasts_and_reserves_original_nonce() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    *fixture.journal.lock().unwrap() = Some(client.dir.join("transactions.sqlite3"));
    let original = client
        .prepare_payment_once("open:prepared", &key(), &payee(), "1", b"commitment")
        .await
        .unwrap();
    assert_eq!(original.status, "prepared");
    assert!(fixture.submissions.lock().unwrap().is_empty());
    let sender = wallet::address_from_private_key(&key()).unwrap();
    assert_eq!(
        client.records(&sender).unwrap()[0].raw,
        original.raw_transaction
    );
    assert!(matches!(
        client
            .prepare_payment_once("open:prepared", &key(), &payee(), "2", b"commitment")
            .await,
        Err(ChainError::Rejected(_))
    ));
    assert!(matches!(
        client
            .send_payment_once("another", &key(), &payee(), "1", b"commitment")
            .await,
        Err(ChainError::Pending(_))
    ));
    assert_eq!(client.records(&sender).unwrap()[0].status, "prepared");
    assert!(fixture.submissions.lock().unwrap().is_empty());
    drop(client);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    // Original preparation can be recovered locally even when node settlement is unavailable.
    fixture.syncing.store(true, Ordering::SeqCst);
    let resumed = client
        .prepare_payment_once("open:prepared", &key(), &payee(), "1", b"commitment")
        .await
        .unwrap();
    assert_eq!(resumed.raw_transaction, original.raw_transaction);
    assert_eq!(resumed.status, "prepared");
    assert!(fixture.submissions.lock().unwrap().is_empty());
    fixture.syncing.store(false, Ordering::SeqCst);
    let sent = client
        .send_payment_once("open:prepared", &key(), &payee(), "1", b"commitment")
        .await
        .unwrap();
    assert_eq!(sent.raw_transaction, original.raw_transaction);
    assert_eq!(sent.status, "submitted");
    assert_eq!(
        fixture.submissions.lock().unwrap().as_slice(),
        [original.raw_transaction.clone()]
    );
    assert!(fixture.persisted_before_broadcast.load(Ordering::SeqCst));
    let recovered = client
        .prepare_payment_once("open:prepared", &key(), &payee(), "1", b"commitment")
        .await
        .unwrap();
    assert_eq!(recovered.raw_transaction, original.raw_transaction);
    assert_eq!(fixture.submissions.lock().unwrap().len(), 1);
    task.abort();
}
/// Opening-intent parent 2.6 (inference-opening-funding-intent, requirement "Native preparation
/// durably owns one exact opening transaction": scenario "Preparation is durable without
/// submission", "reopening the journal returns that original for the same operation and rejects
/// changed recipient, value or commitment"; "Retrieving a prepared original SHALL NOT sign
/// another transaction merely because time, fee estimates or the node's nonce changed"). After
/// the journal is reopened, the node's gas price and gas estimate rise and its latest and
/// pending nonces move past the original's. Preparing the same operation again, twice, returns
/// the identical bytes and hash, still `prepared`, with the one journal record unchanged. The
/// same operation with a changed recipient, value or commitment is refused by both preparation
/// and sending, leaving that record unchanged and nothing submitted. Would catch re-signing a
/// prepared original at a new fee or nonce, or binding an operation to changed terms.
#[tokio::test]
async fn reopened_original_survives_fee_and_nonce_changes_and_refuses_other_terms() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let operation = "open:reopened";
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let original = client
        .prepare_payment_once(operation, &key(), &payee(), "100", b"commitment")
        .await
        .unwrap();
    assert_eq!(original.status, "prepared");
    let signed = decode(&original.raw_transaction, CHAIN).unwrap();
    assert_eq!((signed.nonce, signed.max_fee), (0, 1001));
    drop(client);
    *fixture.gas_price.lock().unwrap() = 5_000;
    *fixture.estimate_override.lock().unwrap() = Some(json!(format!("0x{:x}", signed.gas + 7_000)));
    fixture.nonce.store(3, Ordering::SeqCst);
    *fixture.pending_nonce.lock().unwrap() = Some(4);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let record = || {
        let records = client.records(&sender).unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        (record.operation.clone(), record.raw.clone(), record.status.clone(),
            record.error.clone(), record.evidence.clone(), record.created_at)
    };
    let prepared = record();
    assert_eq!((prepared.0.as_str(), prepared.1.as_str(), prepared.2.as_str()),
        (operation, original.raw_transaction.as_str(), "prepared"));
    for _ in 0..2 {
        let again = client
            .prepare_payment_once(operation, &key(), &payee(), "100", b"commitment")
            .await
            .unwrap();
        assert_eq!((&again.raw_transaction, &again.tx_hash, again.status.as_str()),
            (&original.raw_transaction, &original.tx_hash, "prepared"));
        assert_eq!(record(), prepared);
    }
    let other = format!("0x{}", "44".repeat(20));
    let payee = payee();
    let changed: [(&str, &str, &str, &[u8]); 3] = [
        ("recipient", &other, "100", b"commitment"),
        ("value", &payee, "101", b"commitment"),
        ("commitment", &payee, "100", b"other commitment"),
    ];
    for (name, to, value, data) in changed {
        for send in [false, true] {
            let result = if send {
                client.send_payment_once(operation, &key(), to, value, data).await
            } else {
                client.prepare_payment_once(operation, &key(), to, value, data).await
            };
            assert_eq!(result.unwrap_err(),
                ChainError::Rejected("operation id is already bound to different payment terms".into()),
                "{name}, send={send}");
            assert_eq!(record(), prepared, "{name}, send={send}");
        }
    }
    assert!(fixture.submissions.lock().unwrap().is_empty());
    task.abort();
}
#[tokio::test]
async fn preparation_checks_chain_balance_and_nonce_before_persisting() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    fixture.chain_id.store(1030, Ordering::SeqCst);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    assert!(matches!(
        client.prepare_payment_once("wrong-chain", &key(), &payee(), "1", b"terms").await,
        Err(ChainError::Rejected(_))
    ));
    fixture.chain_id.store(CHAIN, Ordering::SeqCst);
    assert!(matches!(
        client
            .prepare_payment_once(
                "insufficient",
                &key(),
                &payee(),
                &u128::MAX.to_string(),
                b"terms"
            )
            .await,
        Err(ChainError::Rejected(_))
    ));
    fixture.nonce.store(u64::MAX, Ordering::SeqCst);
    assert!(matches!(
        client
            .prepare_payment_once("exhausted", &key(), &payee(), "1", b"terms")
            .await,
        Err(ChainError::Rejected(_))
    ));
    assert!(client
        .records(&wallet::address_from_private_key(&key()).unwrap())
        .unwrap()
        .is_empty());
    assert!(fixture.submissions.lock().unwrap().is_empty());
    task.abort();
}
#[test]
fn cross_process_wallet_lock_is_exclusive() {
    let dir = tempfile::tempdir().unwrap();
    let config = config("http://127.0.0.1:1".into());
    let a = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let b = ChainClient::new(config, dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let held = a.process_lock(&sender).unwrap();
    assert!(b.process_lock(&sender).is_err());
    drop(held);
    assert!(b.process_lock(&sender).is_ok());
}

#[tokio::test]
async fn ambiguous_broadcast_retains_original_evidence() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    fixture.ambiguous_submit.store(true, Ordering::SeqCst);
    let pending = client
        .send_payment_once("ambiguous", &key(), &payee(), "123", b"terms")
        .await
        .unwrap();
    assert_eq!(pending.status, "pending");
    let retry = client
        .send_payment_once("ambiguous", &key(), &payee(), "123", b"terms")
        .await
        .unwrap();
    assert_eq!(retry.status, "pending");
    assert_eq!(retry.raw_transaction, pending.raw_transaction);
    assert_eq!(
        client
            .records(&wallet::address_from_private_key(&key()).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        client
            .send_payment_once("new", &key(), &payee(), "123", b"terms")
            .await,
        Err(ChainError::Pending(_))
    ));
    task.abort();
}

#[test]
fn published_eip155_signing_vector() {
    // Independent vector: https://eips.ethereum.org/EIPS/eip-155#example
    let raw="0xf86c098504a817c800825208943535353535353535353535353535353535353535880de0b6b3a76400008025a028ef61340bd939bc2195fe537567866003e1a15d3c71ff63e1590620aa636276a067cbe9d8997f761aecb703304b3800ccf555c9f3dc64214b297fb1966a3b6d83";
    let decoded = decode(raw, 1).unwrap();
    assert_eq!(decoded.nonce, 9);
    assert_eq!(decoded.value, 1_000_000_000_000_000_000);
    assert_eq!(decoded.from, "0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f");
    let signed = sign_transaction(
        &"46".repeat(32),
        1,
        9,
        "0x3535353535353535353535353535353535353535",
        1_000_000_000_000_000_000,
        &[],
        20_000_000_000,
    )
    .unwrap();
    assert_eq!(signed.raw_transaction, raw);
}

#[tokio::test]
async fn explicit_cancellation_restarts_exact_bytes_and_unlocks_next_provider() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    *fixture.journal.lock().unwrap() = Some(client.dir.join("transactions.sqlite3"));
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let original = client
        .prepare_payment_once("open:expired", &key(), &payee(), "100", b"expired terms")
        .await
        .unwrap();
    assert!(matches!(
        client
            .send_payment_once("other-provider", &key(), &payee(), "1", b"other")
            .await,
        Err(ChainError::Pending(_))
    ));
    assert!(fixture.submissions.lock().unwrap().is_empty());
    assert!(client
        .cancellation_record(&sender, "open:expired")
        .unwrap()
        .is_none());
    fixture.ambiguous_submit.store(true, Ordering::SeqCst);
    let pending = client
        .cancel_payment_once("open:expired", &key())
        .await
        .unwrap();
    assert_eq!(pending.status, "cancellation_pending");
    assert!(pending.error.is_some());
    let replacement = client
        .cancellation_record(&sender, "open:expired")
        .unwrap()
        .unwrap()
        .raw;
    let tx = decode(&replacement, CHAIN).unwrap();
    let old = decode(&original.raw_transaction, CHAIN).unwrap();
    assert_eq!(tx.nonce, old.nonce);
    assert_eq!(tx.to, sender);
    assert_eq!(tx.value, 0);
    assert!(tx.data.is_empty());
    assert_eq!(tx.gas, 21000);
    assert_eq!(tx.max_fee, old.max_fee + old.max_fee / 8 + 1);
    assert_eq!(pending.gas_price_wei, Some(tx.max_fee.to_string()));
    assert!(fixture.persisted_before_broadcast.load(Ordering::SeqCst));
    assert_eq!(
        client
            .original_payment_raw("open:expired", &sender)
            .unwrap()
            .as_deref(),
        Some(original.raw_transaction.as_str())
    );
    drop(client);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    client
        .cancel_payment_once("open:expired", &key())
        .await
        .unwrap();
    assert_eq!(
        fixture.submissions.lock().unwrap().as_slice(),
        [replacement.clone(), replacement.clone()]
    );
    assert!(client
        .send_payment_once("open:expired", &key(), &payee(), "100", b"expired terms")
        .await
        .is_err());
    let replacement_tx = SubmittedTransaction {
        tx_hash: tx.tx_hash,
        raw_transaction: replacement.clone(),
        status: "submitted".into(),
    };
    *fixture.receipt.lock().unwrap() = receipt(&replacement_tx);
    fixture.nonce.store(1, Ordering::SeqCst);
    fixture.frontier.store(8, Ordering::SeqCst);
    assert_eq!(
        client
            .cancel_payment_once("open:expired", &key())
            .await
            .unwrap()
            .status,
        "cancellation_pending"
    );
    assert_eq!(fixture.submissions.lock().unwrap().len(), 2);
    fixture.frontier.store(10, Ordering::SeqCst);
    assert_eq!(
        client
            .cancel_payment_once("open:expired", &key())
            .await
            .unwrap()
            .status,
        "cancelled"
    );
    let next = client
        .prepare_payment_once("next-provider", &key(), &payee(), "100", b"new terms")
        .await
        .unwrap();
    assert_eq!(decode(&next.raw_transaction, CHAIN).unwrap().nonce, 1);
    let history = client.history(&sender).await.unwrap();
    assert_eq!(
        history["transactions"][0]["raw_transaction"],
        original.raw_transaction
    );
    assert_eq!(
        history["transactions"][0]["cancellation"]["raw_transaction"],
        replacement
    );
    // Reorg evidence must revoke the safe result and block new spending, never rebroadcast by itself.
    *fixture.receipt.lock().unwrap() = Value::Null;
    assert_eq!(
        client
            .cancellation_status("open:expired", &sender)
            .await
            .unwrap()
            .unwrap()
            .status,
        "cancellation_pending"
    );
    assert!(client
        .prepare_payment_once("third-provider", &key(), &payee(), "100", b"third")
        .await
        .is_err());
    assert_eq!(fixture.submissions.lock().unwrap().len(), 2);
    task.abort();
}

#[tokio::test]
async fn original_winning_cancellation_race_is_never_reported_as_refunded() {
    for already_mined in [false, true] {
        let (fixture, config, task) = fixture().await;
        let dir = tempfile::tempdir().unwrap();
        let client = ChainClient::new(config, dir.path().into()).unwrap();
        let original = client
            .prepare_payment_once("race", &key(), &payee(), "100", b"terms")
            .await
            .unwrap();
        if !already_mined {
            assert_eq!(
                client
                    .cancel_payment_once("race", &key())
                    .await
                    .unwrap()
                    .status,
                "cancellation_pending"
            );
            assert!(client.prepare_payment_once("race", &key(), &payee(), "100", b"terms").await.is_err());
        }
        *fixture.receipt.lock().unwrap() = receipt(&original);
        fixture.nonce.store(1, Ordering::SeqCst);
        if !already_mined {
            let resumed = client.prepare_payment_once("race", &key(), &payee(), "100", b"terms")
                .await.unwrap();
            assert_eq!(resumed.status, "accepted");
            assert_eq!(resumed.raw_transaction, original.raw_transaction);
        }
        let result = client.cancel_payment_once("race", &key()).await.unwrap();
        assert_eq!(result.status, "original_accepted");
        assert_eq!(
            fixture.submissions.lock().unwrap().len(),
            usize::from(!already_mined)
        );
        assert_eq!(
            client
                .send_payment_once("race", &key(), &payee(), "100", b"terms")
                .await
                .unwrap()
                .status,
            "accepted"
        );
        assert_eq!(
            client
                .original_payment_raw("race", &wallet::address_from_private_key(&key()).unwrap())
                .unwrap(),
            Some(original.raw_transaction)
        );
        task.abort();
    }
}

#[tokio::test]
async fn cancellation_requires_sync_health_current_nonce_and_affordable_bounded_gas() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    client
        .prepare_payment_once("blocked", &key(), &payee(), "1", b"terms")
        .await
        .unwrap();
    fixture.syncing.store(true, Ordering::SeqCst);
    assert!(matches!(
        client.cancel_payment_once("blocked", &key()).await,
        Err(ChainError::Unavailable(_))
    ));
    fixture.syncing.store(false, Ordering::SeqCst);
    fixture.nonce.store(1, Ordering::SeqCst);
    assert!(matches!(
        client.cancel_payment_once("blocked", &key()).await,
        Err(ChainError::Pending(_))
    ));
    fixture.nonce.store(0, Ordering::SeqCst);
    *fixture.balance.lock().unwrap() = 1;
    assert!(matches!(
        client.cancel_payment_once("blocked", &key()).await,
        Err(ChainError::Rejected(_))
    ));
    assert!(client
        .cancellation_record(&sender, "blocked")
        .unwrap()
        .is_none());
    assert!(fixture.submissions.lock().unwrap().is_empty());
    *fixture.balance.lock().unwrap() = u128::MAX;
    client.cancel_payment_once("blocked", &key()).await.unwrap();
    let original_replacement = client
        .cancellation_record(&sender, "blocked")
        .unwrap()
        .unwrap()
        .raw;
    *fixture.gas_price.lock().unwrap() = u128::MAX;
    client.cancel_payment_once("blocked", &key()).await.unwrap();
    assert_eq!(
        client
            .cancellation_record(&sender, "blocked")
            .unwrap()
            .unwrap()
            .raw,
        original_replacement
    );
    assert_eq!(
        fixture.submissions.lock().unwrap().as_slice(),
        [original_replacement.clone(), original_replacement]
    );
    task.abort();
}

#[tokio::test]
async fn shared_base_serializes_concurrent_consumer_preparation_and_provider_fee() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let consumer = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let provider = ChainClient::new(config, dir.path().into()).unwrap();
    assert_eq!(consumer.dir, provider.dir);
    let key = key();
    let payee = payee();
    let (opening, fee) = tokio::join!(
        consumer.prepare_payment_once("open:consumer", &key, &payee, "100", b"opening"),
        provider.send_payment_once("platform-fee:provider", &key, &payee, "1", b"fee")
    );
    assert_ne!(opening.is_ok(), fee.is_ok());
    let sender = wallet::address_from_private_key(&key).unwrap();
    assert_eq!(consumer.records(&sender).unwrap().len(), 1);
    assert_eq!(provider.records(&sender).unwrap().len(), 1);
    assert!(fixture.submissions.lock().unwrap().len() <= 1);
    task.abort();
}

#[tokio::test]
async fn crash_after_replacement_journal_commit_resumes_before_any_broadcast() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let original = client
        .prepare_payment_once("crash", &key(), &payee(), "1", b"terms")
        .await
        .unwrap();
    let replacement = sign_transaction(&key(), CHAIN, 0, &sender, 0, &[], 1127).unwrap();
    client.journal.lock().unwrap().execute("INSERT INTO cancellations(sender,operation,raw,status) VALUES(?1,'crash',?2,'cancellation_pending')",
        params![sender,replacement.raw_transaction]).unwrap();
    drop(client);
    assert!(fixture.submissions.lock().unwrap().is_empty());
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    assert_eq!(
        client
            .cancel_payment_once("crash", &key())
            .await
            .unwrap()
            .status,
        "cancellation_pending"
    );
    assert_eq!(
        fixture.submissions.lock().unwrap().as_slice(),
        [replacement.raw_transaction]
    );
    assert_eq!(
        client.original_payment_raw("crash", &sender).unwrap(),
        Some(original.raw_transaction)
    );
    task.abort();
}

#[tokio::test]
async fn failed_execution_is_not_successful_cancellation_and_requires_safe() {
    for replacement_fails in [false, true] {
        let (fixture, config, task) = fixture().await;
        let dir = tempfile::tempdir().unwrap();
        let client = ChainClient::new(config, dir.path().into()).unwrap();
        let original = client
            .prepare_payment_once("failure", &key(), &payee(), "1", b"terms")
            .await
            .unwrap();
        client.cancel_payment_once("failure", &key()).await.unwrap();
        let raw = fixture.submissions.lock().unwrap()[0].clone();
        let replacement = SubmittedTransaction {
            tx_hash: protocol::hash_hex(&hex::decode(&raw[2..]).unwrap()),
            raw_transaction: raw,
            status: "submitted".into(),
        };
        let mut failed = receipt(if replacement_fails {
            &replacement
        } else {
            &original
        });
        failed["status"] = json!("0x0");
        *fixture.receipt.lock().unwrap() = failed;
        fixture.nonce.store(1, Ordering::SeqCst);
        fixture.frontier.store(8, Ordering::SeqCst);
        assert_eq!(
            client
                .cancel_payment_once("failure", &key())
                .await
                .unwrap()
                .status,
            "cancellation_pending"
        );
        fixture.frontier.store(10, Ordering::SeqCst);
        let outcome = client.cancel_payment_once("failure", &key()).await.unwrap();
        assert_eq!(
            outcome.status,
            if replacement_fails {
                "cancellation_failed"
            } else {
                "original_failed"
            }
        );
        let next = client
            .prepare_payment_once("after-failure", &key(), &payee(), "1", b"next")
            .await
            .unwrap();
        assert_eq!(decode(&next.raw_transaction, CHAIN).unwrap().nonce, 1);
        assert_eq!(fixture.submissions.lock().unwrap().len(), 1);
        task.abort();
    }
}

#[tokio::test]
async fn nonce_regression_after_reconciliation_never_signs_a_competing_original() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let original = client
        .send_payment_once("original", &key(), &payee(), "100", b"original")
        .await
        .unwrap();
    *fixture.receipt.lock().unwrap() = receipt(&original);
    fixture.nonce.store(1, Ordering::SeqCst);
    client.reconcile(&sender).await.unwrap();
    // The reorg occurs inside the allocation's nonce RPC, strictly after the
    // preceding canonical-receipt reconciliation has returned accepted.
    fixture.regress_on_nonce_read.store(true, Ordering::SeqCst);
    assert!(matches!(
        client
            .send_payment_once("competing", &key(), &payee(), "200", b"different")
            .await,
        Err(ChainError::Pending(_))
    ));
    assert_eq!(client.records(&sender).unwrap().len(), 1);
    assert_eq!(
        fixture.submissions.lock().unwrap().as_slice(),
        [original.raw_transaction.clone()]
    );
    drop(client);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    assert_eq!(
        client.original_payment_raw("original", &sender).unwrap(),
        Some(original.raw_transaction)
    );
    // Durable protection also rejects inserts independently of the high-level
    // terminal-status check, including after process restart.
    let competing = sign_transaction(&key(), CHAIN, 0, &payee(), 200, b"different", 1001).unwrap();
    assert!(client.journal.lock().unwrap().execute(
        "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at) VALUES(?1,'other','0',?2,?3,'prepared',0)",
        params![sender,competing.tx_hash,competing.raw_transaction]).is_err());
    task.abort();
}

#[tokio::test]
async fn reincluded_identical_payment_refreshes_journal_evidence_without_broadcast() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let original = client
        .send_payment_once("funding", &key(), &payee(), "100", b"terms")
        .await
        .unwrap();
    *fixture.receipt.lock().unwrap() = receipt(&original);
    fixture.nonce.store(1, Ordering::SeqCst);
    client.reconcile(&sender).await.unwrap();
    let old: VerifiedPayment = serde_json::from_str(
        client.records(&sender).unwrap()[0]
            .evidence
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    let mut inclusion = receipt(&original);
    inclusion["blockHash"] = json!(format!("0x{}", "88".repeat(32)));
    inclusion["blockNumber"] = json!("0x8");
    *fixture.receipt.lock().unwrap() = inclusion;
    let PaymentState::Accepted(refreshed) = client.recheck_payment(&old).await.unwrap() else {
        panic!("re-inclusion is canonical");
    };
    assert_eq!(refreshed.tx_hash, old.tx_hash);
    assert_ne!(refreshed.block_hash, old.block_hash);
    assert_eq!(refreshed.block_number, 8);
    client.reconcile(&sender).await.unwrap();
    drop(client);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let stored: VerifiedPayment = serde_json::from_str(
        client.records(&sender).unwrap()[0]
            .evidence
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(stored, refreshed);
    assert_eq!(fixture.submissions.lock().unwrap().len(), 1);
    *fixture.receipt.lock().unwrap() = Value::Null;
    *fixture.canonical_hash.lock().unwrap() = Some(format!("0x{}", "ff".repeat(32)));
    fixture.frontier.store(7, Ordering::SeqCst);
    assert_eq!(
        client.recheck_payment(&stored).await.unwrap(),
        PaymentState::Pending
    );
    fixture.frontier.store(10, Ordering::SeqCst);
    assert_eq!(
        client.recheck_payment(&stored).await.unwrap(),
        PaymentState::Orphaned
    );
    fixture.syncing.store(true, Ordering::SeqCst);
    assert!(matches!(
        client.recheck_payment(&stored).await,
        Err(ChainError::Unavailable(_))
    ));
    task.abort();
}

#[tokio::test]
async fn signed_intent_admission_requires_current_nonce_and_entire_gas_cost() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let raw = sign_transaction(&key(), CHAIN, 0, &payee(), 100, b"terms\0", 1001)
        .unwrap()
        .raw_transaction;
    let minimum = 100 + u128::from(native_gas(b"terms\0")) * 1001;
    for balance in [0, 100, minimum - 1] {
        *fixture.balance.lock().unwrap() = balance;
        assert!(matches!(
            client
                .admit_payment_envelope(&raw, &sender, &payee(), "100", b"terms\0")
                .await,
            Err(ChainError::Rejected(_))
        ));
    }
    *fixture.balance.lock().unwrap() = minimum;
    assert_eq!(
        client
            .admit_payment_envelope(&raw, &sender, &payee(), "100", b"terms\0")
            .await
            .unwrap()
            .nonce,
        0
    );
    let future = sign_transaction(&key(), CHAIN, 1, &payee(), 100, b"terms\0", 1001).unwrap();
    assert!(matches!(
        client
            .admit_payment_envelope(
                &future.raw_transaction,
                &sender,
                &payee(),
                "100",
                b"terms\0"
            )
            .await,
        Err(ChainError::Rejected(_))
    ));
    fixture.nonce.store(1, Ordering::SeqCst);
    assert!(matches!(
        client
            .admit_payment_envelope(&raw, &sender, &payee(), "100", b"terms\0")
            .await,
        Err(ChainError::Rejected(_))
    ));
    assert!(fixture.submissions.lock().unwrap().is_empty());
    assert!(client.records(&sender).unwrap().is_empty());
    task.abort();
}

#[test]
fn legacy_duplicate_nonce_journal_is_preserved_but_cannot_allocate_more() {
    let dir = tempfile::tempdir().unwrap();
    let config = config("http://127.0.0.1:1".into());
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    {
        let db = client.journal.lock().unwrap();
        db.execute_batch("DROP TRIGGER transactions_unique_original_nonce")
            .unwrap();
        for (operation, value) in [("first", 1), ("second", 2)] {
            let tx = sign_transaction(&key(), CHAIN, 0, &payee(), value, b"terms", 1001).unwrap();
            db.execute("INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at) VALUES(?1,?2,'0',?3,?4,'accepted',0)",
                params![sender,operation,tx.tx_hash,tx.raw_transaction]).unwrap();
        }
    }
    drop(client);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    assert_eq!(client.records(&sender).unwrap().len(), 2);
    let tx = sign_transaction(&key(), CHAIN, 0, &payee(), 3, b"terms", 1001).unwrap();
    assert!(client.journal.lock().unwrap().execute("INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at) VALUES(?1,'third','0',?2,?3,'prepared',0)",
        params![sender,tx.tx_hash,tx.raw_transaction]).is_err());
}

#[tokio::test]
async fn public_identity_is_pinned_and_genesis_is_not_refetched_per_operation() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    *fixture.genesis.lock().unwrap() = format!("0x{}", "aa".repeat(32));
    assert!(matches!(client.identity_check().await, Err(ChainError::Rejected(_))));
    *fixture.genesis.lock().unwrap() = crate::network::ESPACE_TESTNET_GENESIS.into();
    client.identity_check().await.unwrap();
    client.identity_check().await.unwrap();
    assert_eq!(fixture.genesis_reads.load(Ordering::SeqCst), 2);
    fixture.chain_id.store(1030, Ordering::SeqCst);
    assert!(matches!(client.identity_check().await, Err(ChainError::Rejected(_))));
    assert_eq!(fixture.genesis_reads.load(Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn legacy_and_typed_execution_waits_for_safe_not_first_receipt_or_finalized() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    for raw in [legacy().raw_transaction, typed(1), typed(2)] {
        let decoded = decode(&raw, CHAIN).unwrap();
        let tx = SubmittedTransaction { tx_hash: decoded.tx_hash, raw_transaction: raw, status: "submitted".into() };
        *fixture.receipt.lock().unwrap() = receipt(&tx);
        fixture.frontier.store(8, Ordering::SeqCst);
        assert!(matches!(client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, &decoded.data).await,
            Err(ChainError::Pending(_))));
        fixture.frontier.store(10, Ordering::SeqCst);
        let payment = client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, &decoded.data).await.unwrap();
        assert_eq!(payment.block_number, 9);
    }
    let status = client.status().await.unwrap();
    assert_eq!(status["safe"]["height"], 10);
    assert_eq!(status["finalized"]["height"], 6);
    assert_eq!(status["acceptance"], "safe");
    assert!(status.get("trusted").is_none());
    task.abort();
}

#[tokio::test]
async fn exact_canonical_hash_failure_and_orphan_have_distinct_outcomes() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let tx = legacy();
    *fixture.receipt.lock().unwrap() = receipt(&tx);
    let accepted = client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, b"terms\0").await.unwrap();
    *fixture.canonical_hash.lock().unwrap() = Some(format!("0x{}", "aa".repeat(32)));
    assert!(matches!(client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, b"terms\0").await,
        Err(ChainError::Pending(_))));
    assert_eq!(client.recheck_payment(&accepted).await.unwrap(), PaymentState::Orphaned);
    *fixture.canonical_hash.lock().unwrap() = None;
    fixture.receipt.lock().unwrap()["status"] = json!("0x0");
    fixture.frontier.store(8, Ordering::SeqCst);
    assert!(matches!(client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, b"terms\0").await,
        Err(ChainError::Pending(_))));
    fixture.frontier.store(10, Ordering::SeqCst);
    assert!(matches!(client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, b"terms\0").await,
        Err(ChainError::Rejected(reason)) if reason == "transaction execution failed"));
    assert!(matches!(client.recheck_payment(&accepted).await, Err(ChainError::Unavailable(_))));
    *fixture.canonical_hash.lock().unwrap() = Some(format!("0x{}", "aa".repeat(32)));
    fixture.receipt.lock().unwrap()["blockHash"] = json!(format!("0x{}", "aa".repeat(32)));
    assert_eq!(client.recheck_payment(&accepted).await.unwrap(), PaymentState::Orphaned);
    task.abort();
}

#[tokio::test]
async fn malformed_stale_or_unavailable_settlement_data_never_accepts() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    fixture.syncing_unavailable.store(true, Ordering::SeqCst);
    assert!(matches!(client.status().await, Err(ChainError::Unavailable(_))));
    fixture.syncing_unavailable.store(false, Ordering::SeqCst);
    fixture.malformed_frontier.store(true, Ordering::SeqCst);
    assert!(matches!(client.status().await, Err(ChainError::Unavailable(_))));
    fixture.malformed_frontier.store(false, Ordering::SeqCst);
    fixture.frontier.store(13, Ordering::SeqCst);
    assert!(matches!(client.status().await, Err(ChainError::Unavailable(_))));
    fixture.frontier.store(10, Ordering::SeqCst);
    fixture.head_age.store(121, Ordering::SeqCst);
    assert!(matches!(client.status().await, Err(ChainError::Unavailable(_))));
    fixture.head_age.store(0, Ordering::SeqCst);
    let tx = legacy();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    *fixture.receipt.lock().unwrap() = receipt(&tx);
    fixture.receipt.lock().unwrap()["status"] = json!("0x2");
    assert!(matches!(client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, b"terms\0").await,
        Err(ChainError::Unavailable(_))));
    task.abort();
}

#[tokio::test]
async fn pending_nonce_blocks_new_original_but_not_same_nonce_cancellation() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    *fixture.pending_nonce.lock().unwrap() = Some(1);
    assert!(matches!(client.prepare_payment_once("new", &key(), &payee(), "1", b"terms").await,
        Err(ChainError::Pending(_))));
    assert!(client.records(&sender).unwrap().is_empty());
    let raw = sign_transaction(&key(), CHAIN, 0, &payee(), 1, b"terms", 1001).unwrap();
    assert!(matches!(client.admit_payment_envelope(&raw.raw_transaction, &sender, &payee(), "1", b"terms").await,
        Err(ChainError::Pending(_))));
    *fixture.pending_nonce.lock().unwrap() = None;
    let original = client.prepare_payment_once("new", &key(), &payee(), "1", b"terms").await.unwrap();
    *fixture.pending_nonce.lock().unwrap() = Some(1);
    client.cancel_payment_once("new", &key()).await.unwrap();
    let replacement = client.cancellation_record(&sender, "new").unwrap().unwrap();
    assert_eq!(decode(&replacement.raw, CHAIN).unwrap().nonce, decode(&original.raw_transaction, CHAIN).unwrap().nonce);
    fixture.nonce.store(2, Ordering::SeqCst);
    assert!(matches!(client.nonce_state(&sender).await, Err(ChainError::Unavailable(_))));
    task.abort();
}

#[tokio::test]
async fn native_gas_estimate_is_signed_exactly_and_malformed_estimates_never_persist() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    for bad in [json!(null), json!("0x00"), json!("0x1"), json!("0xf4241")] {
        *fixture.estimate_override.lock().unwrap() = Some(bad);
        assert!(matches!(client.prepare_payment_once("gas", &key(), &payee(), "1", b"terms").await,
            Err(ChainError::Unavailable(_))));
        assert!(client.records(&sender).unwrap().is_empty());
    }
    *fixture.estimate_override.lock().unwrap() = Some(json!("0x7530"));
    let signed = client.prepare_payment_once("gas", &key(), &payee(), "1", b"terms").await.unwrap();
    assert_eq!(decode(&signed.raw_transaction, CHAIN).unwrap().gas, 30000);
    assert!(fixture.submissions.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn large_hash_only_blocks_verify_canonical_funding_through_gateway() {
    let (fixture, upstream, task) = fixture().await;
    fixture.block_transactions.store(2048, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let gateway_chain = Arc::new(ChainClient::new(upstream, dir.path().join("gateway")).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway = config(format!("http://{}/api/chain/rpc", listener.local_addr().unwrap()));
    let app = crate::chain_rpc_api::router(gateway_chain);
    let gateway_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let block = rpc_client::call(&gateway, "eth_getBlockByNumber", json!(["safe", false])).await.unwrap();
    assert_eq!(block["transactions"].as_array().unwrap().len(), 2048);
    assert!(serde_json::to_vec(&block).unwrap().len() > rpc_client::MAX_RESPONSE_BYTES);
    let client = ChainClient::new(gateway, dir.path().join("consumer")).unwrap();
    let tx = legacy();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    *fixture.receipt.lock().unwrap() = receipt(&tx);
    let payment = client.verify_payment(&tx.raw_transaction, &sender, &payee(), None, b"terms\0")
        .await.unwrap();
    assert_eq!(payment.block_number, 9);
    assert_eq!(client.recheck_payment(&payment).await.unwrap(), PaymentState::Accepted(payment.clone()));
    fixture.frontier.store(8, Ordering::SeqCst);
    assert_eq!(client.recheck_payment(&payment).await.unwrap(), PaymentState::Pending);
    fixture.frontier.store(10, Ordering::SeqCst);
    *fixture.canonical_hash.lock().unwrap() = Some(format!("0x{}", "ff".repeat(32)));
    assert_eq!(client.recheck_payment(&payment).await.unwrap(), PaymentState::Orphaned);
    gateway_task.abort();
    task.abort();
}

#[tokio::test]
async fn block_transport_and_gateway_enforce_separate_request_response_and_batch_caps() {
    let (fixture, upstream, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let gateway_chain = Arc::new(ChainClient::new(upstream.clone(), dir.path().into()).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/chain/rpc", listener.local_addr().unwrap());
    let app = crate::chain_rpc_api::router(gateway_chain);
    let gateway_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":["safe",false]});

    // Each response fits the block allowance; eight together must not get 8x that allowance.
    fixture.block_transactions.store(20_000, Ordering::SeqCst);
    let response = http.post(&url).json(&request).send().await.unwrap();
    assert!(response.status().is_success());
    assert_eq!(response.json::<Value>().await.unwrap()["result"]["transactions"].as_array().unwrap().len(), 20_000);
    let response = http.post(&url).json(&vec![request.clone(); 8]).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], -32005);

    fixture.block_transactions.store((rpc_client::MAX_BLOCK_RESPONSE_BYTES / 69 + 1) as u64, Ordering::SeqCst);
    assert!(matches!(rpc_client::call(&upstream, "eth_getBlockByNumber", json!(["safe",false])).await,
        Err(rpc_client::RpcError::Protocol)));
    let response = http.post(&url).json(&request).send().await.unwrap();
    let response = response.json::<Value>().await.unwrap();
    assert_eq!(response["error"]["code"], -32002);
    assert!(response.get("result").is_none());

    // A large block budget must not widen normal methods or RPC errors.
    fixture.block_transactions.store(0, Ordering::SeqCst);
    *fixture.response_override.lock().unwrap() = Some(("eth_gasPrice".into(),
        json!({"jsonrpc":"2.0","result":"x".repeat(rpc_client::MAX_RESPONSE_BYTES)})));
    assert!(matches!(rpc_client::call(&upstream, "eth_gasPrice", json!([])).await,
        Err(rpc_client::RpcError::Protocol)));
    *fixture.response_override.lock().unwrap() = Some(("eth_getBlockByNumber".into(),
        json!({"jsonrpc":"2.0","error":{"code":-32000,"message":"x".repeat(rpc_client::MAX_RESPONSE_BYTES)}})));
    assert!(matches!(rpc_client::call(&upstream, "eth_getBlockByNumber", json!(["safe",false])).await,
        Err(rpc_client::RpcError::Protocol)));
    let response = http.post(&url).json(&request).send().await.unwrap().json::<Value>().await.unwrap();
    assert_eq!(response["error"]["code"], -32002);
    assert!(response.to_string().len() < rpc_client::MAX_RESPONSE_BYTES);

    let response = http.post(&url).body(vec![b' '; rpc_client::MAX_REQUEST_BYTES + 1]).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    assert!(matches!(rpc_client::call(&upstream, "eth_getBlockByNumber",
        json!(["x".repeat(rpc_client::MAX_REQUEST_BYTES),false])).await,
        Err(rpc_client::RpcError::Protocol)));
    gateway_task.abort();
    task.abort();
}

#[tokio::test]
async fn opening_prepare_recovers_nonce_conflict_after_canonical_receipt_returns() {
    let (fixture, config, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let client = ChainClient::new(config.clone(), dir.path().into()).unwrap();
    let sender = wallet::address_from_private_key(&key()).unwrap();
    let original = client.send_payment_once("open:receipt-gap", &key(), &payee(), "100", b"terms")
        .await.unwrap();
    fixture.nonce.store(1, Ordering::SeqCst);
    // The normal send/resume path observes a transient missing receipt after nonce advancement.
    assert!(matches!(client.send_payment_once("open:receipt-gap", &key(), &payee(), "100", b"terms").await,
        Err(ChainError::Rejected(_))));
    assert_eq!(client.records(&sender).unwrap()[0].status, "nonce_conflict");
    drop(client);
    let client = ChainClient::new(config, dir.path().into()).unwrap();
    assert!(client.prepare_payment_once("open:receipt-gap", &key(), &payee(), "100", b"terms").await.is_err());
    assert_eq!(fixture.submissions.lock().unwrap().len(), 1);
    *fixture.receipt.lock().unwrap() = receipt(&original);
    // Still no safe canonical credit while syncing, and no replacement is signed.
    fixture.syncing.store(true, Ordering::SeqCst);
    assert!(matches!(client.prepare_payment_once("open:receipt-gap", &key(), &payee(), "100", b"terms").await,
        Err(ChainError::Unavailable(_))));
    fixture.syncing.store(false, Ordering::SeqCst);
    // Opening resume starts with preparation, not a separate reconciliation command.
    let resumed = client.prepare_payment_once("open:receipt-gap", &key(), &payee(), "100", b"terms")
        .await.unwrap();
    assert_eq!(resumed.raw_transaction, original.raw_transaction);
    assert_eq!(resumed.tx_hash, original.tx_hash);
    assert_eq!(resumed.status, "accepted");
    let sent = client.send_payment_once("open:receipt-gap", &key(), &payee(), "100", b"terms")
        .await.unwrap();
    assert_eq!(sent.raw_transaction, original.raw_transaction);
    assert_eq!(sent.status, "accepted");
    assert_eq!(client.records(&sender).unwrap().len(), 1);
    assert_eq!(fixture.submissions.lock().unwrap().as_slice(), [original.raw_transaction]);
    task.abort();
}

#[tokio::test]
async fn chunked_rpc_responses_cannot_bypass_method_byte_limits() {
    use axum::body::{Body, Bytes};
    use axum::response::Response;
    use std::convert::Infallible;
    async fn oversized(Json(request): Json<Value>) -> Response {
        let limit = rpc_client::response_limit(request["method"].as_str().unwrap(), &request["params"]);
        let mut chunks = vec![Ok::<_, Infallible>(Bytes::from(format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":\"", request["id"]
        )))];
        let chunk = Bytes::from(vec![b'x'; 4096]);
        chunks.extend((0..=limit / chunk.len()).map(|_| Ok(chunk.clone())));
        chunks.push(Ok(Bytes::from_static(b"\"}")));
        Response::new(Body::from_stream(futures_util::stream::iter(chunks)))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = config(format!("http://{}", listener.local_addr().unwrap()));
    let app = Router::new().route("/", post(oversized));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for (method, params) in [
        ("eth_gasPrice", json!([])),
        ("eth_getBlockByNumber", json!(["safe", false])),
    ] {
        assert!(matches!(rpc_client::call(&config, method, params).await,
            Err(rpc_client::RpcError::Protocol)));
    }
    task.abort();
}
