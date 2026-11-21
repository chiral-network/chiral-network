//! inference-account-snapshot tasks 3.3 and 3.4 through the production provider ledger, control
//! router and native consumer files on isolated loopback, in development mode.
//!
//! The free account is a genuine native opening through the public proposal and owner-authorized
//! opening routes. The two paid, profiled accounts restore post-verification prerequisites the
//! way the request-accounting fixtures do: signed synthetic serving terms, an independently
//! signed EIP-155 opening payment, its acknowledgement and credit, and the native saved account
//! and journal row. Supplied token usage, performance outcomes and a funding-orphan recheck
//! observation then drive the real ledger writers. None of this is engine, hardware or
//! live-chain evidence. Ledger operation times are explicit; nothing sleeps.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{header, StatusCode},
    middleware::{from_fn, Next},
    response::Response,
};
use chiral_network::{
    app::ProviderState,
    billing_store::{BillingStore, RequestView},
    chain_client::{PaymentState, VerifiedPayment},
    consumer::{Consumer, ConsumerConfig},
    contracts, performance,
    protocol::{
        self, AccountView, ModelSpec, OpenResult, PriceSchedule, Quote, Signed, TokenUsage,
        UsageReceipt, OFFER_DOMAIN, QUOTE_DOMAIN, RECEIPT_DOMAIN,
    },
    wallet,
};
use proposal::PreparedProvider;
use rlp::RlpStream;
use rusqlite::{params, Connection};
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc, Mutex, RwLock},
};
use tiny_keccak::{Hasher, Keccak};
use tokio::net::TcpListener;
use tower::Service;

const ACCOUNT_FIELDS: [&str; 17] = [
    "contract_id", "network", "provider_wallet", "consumer_wallet", "model", "price", "status",
    "funded_wei", "fee_wei", "spent_wei", "reserved_wei", "available_wei", "reversed_wei",
    "provider_loss_wei", "usage", "waived_wei", "max_concurrency",
];
const MONEY: [&str; 8] = [
    "funded_wei", "fee_wei", "spent_wei", "reserved_wei", "available_wei", "reversed_wei",
    "provider_loss_wei", "waived_wei",
];

/// Response-only receipt claims that conflate distinct accounting quantities. Each is applied
/// to a successfully authorized genuine receipt and re-signed with the fixture provider key, so
/// a valid signature cannot be what refuses it.
#[derive(Clone, Debug)]
enum Conflation {
    /// Evidence renewal rewriting the agreement: the account claims the newer report.
    RenewedAccountModel(ModelSpec),
    /// The foregone waiver counted as spendable credit.
    WaiverCredited,
    /// Request unknown-usage exposure accumulated into account loss compensation.
    ExposureAccumulated(u128),
    /// Funding-loss compensation erased while the funding reversal remains.
    LossErased,
    /// A wide reversal/loss accounting claim: the named money fields replaced, nothing else.
    Wide([(&'static str, String); 4]),
}

impl Conflation {
    fn apply(&self, account: &mut AccountView) {
        let wei = |value: &str| value.parse::<u128>().unwrap();
        match self {
            Self::RenewedAccountModel(model) => account.model = model.clone(),
            Self::WaiverCredited => account.available_wei =
                (wei(&account.available_wei) + wei(&account.waived_wei)).to_string(),
            Self::ExposureAccumulated(exposure) => account.provider_loss_wei =
                (wei(&account.provider_loss_wei) + exposure).to_string(),
            Self::LossErased => account.provider_loss_wei = "0".into(),
            Self::Wide(fields) => {
                let mut value = serde_json::to_value(&*account).unwrap();
                for (field, amount) in fields {
                    value[*field] = json!(amount);
                }
                *account = serde_json::from_value(value).unwrap();
            }
        }
    }
}

/// Funded, loss, reversed and spent amounts for a wide reversal/loss claim.
fn wide_money(funded: u128, loss: u128, reversed: u128, spent: u128) -> [(&'static str, String); 4] {
    [("funded_wei", funded.to_string()), ("provider_loss_wei", loss.to_string()),
        ("reversed_wei", reversed.to_string()), ("spent_wei", spent.to_string())]
}

struct Paid {
    quote: Signed<Quote>,
    opened: OpenResult,
    payment: VerifiedPayment,
}

impl Paid {
    fn id(&self) -> &str {
        &self.quote.payload.id
    }
    fn bearer(&self) -> &str {
        &self.opened.credential.as_ref().unwrap().bearer
    }
}

struct Records {
    provider: PreparedProvider,
    _admin: TcpListener,
    config: ConsumerConfig,
    owner: String,
    free: AccountView,
    frozen: Paid,
    active: Paid,
    original: ModelSpec,
    renewed: ModelSpec,
    tamper: Arc<Mutex<Option<Conflation>>>,
    /// Every request that reached the provider router, with its response status.
    traffic: Arc<Mutex<Vec<(String, u16)>>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<tokio::task::JoinHandle<()>>,
}

impl Records {
    fn account_path(&self, id: &str) -> PathBuf {
        self.config.network.scoped_dir(&self.config.data_dir).join("consumer").join(&self.owner)
            .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))))
    }
    fn journal(&self) -> [Vec<String>; 2] {
        let journal = Connection::open(self.config.network.scoped_dir(&self.config.data_dir)
            .join("wallet-journal/transactions.sqlite3")).unwrap();
        [
            "SELECT json_array(sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at) FROM transactions ORDER BY sender,operation",
            "SELECT json_array(sender,operation,raw,status,error,evidence) FROM cancellations ORDER BY sender,operation",
        ].map(|query| strings(&journal, query))
    }
    fn traffic(&self) -> usize {
        self.traffic.lock().unwrap().len()
    }
    async fn stop(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        self.server.take().unwrap().await.unwrap();
    }
}

fn strings(db: &Connection, query: &str) -> Vec<String> {
    let mut statement = db.prepare(query).unwrap();
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

fn profiled(model: &ModelSpec, measured_at: u64) -> ModelSpec {
    // Signed synthetic terms, not observed model or hardware qualification.
    let mut model = model.clone();
    model.serving = Some(protocol::ServingProfile {
        hf_repo: "test/proposal".into(), tokenizer_repo: "test/tokenizer".into(),
        artifact_manifest_sha256: "a".repeat(64),
        engine: "llama.cpp".into(), engine_version: "test-only".into(),
        runtime_sha256: "b".repeat(64), platform: "linux".into(),
        backend: "cuda".into(), memory_kind: "dedicated".into(),
        devices: vec![protocol::EngineDevice {
            id: "GPU-records-test".into(), name: "synthetic-records-test-device".into(),
            memory_mib: 8192, driver_version: "test".into(), engine_device: "CUDA0".into(),
        }],
        memory_reserve_mib: 256, gpu_layers: 999, split_mode: "none".into(),
        tensor_split: vec![], cache_type_k: "f16".into(), cache_type_v: "f16".into(),
        flash_attention: true, batch_size: 512, ubatch_size: 128,
        capacity_tier: "8gb".into(), region: "test".into(), max_concurrency: 2,
        arrival_milli_rps: 1000, output_tokens_per_second: 20,
        policy: performance::POLICY.into(),
        qualification: protocol::QualificationIdentity {
            evidence_sha256: "c".repeat(64), measured_at,
        },
    });
    performance::validate_profile(&model, model.serving.as_ref().unwrap()).unwrap();
    model
}

/// An independently constructed EIP-155 opening payment, verified after the fact as a
/// prerequisite. The runtime's own signer is never used for the payer.
fn signed_payment(provider: &PreparedProvider, owner_key: &str, quote: &Quote, nonce: u64) -> VerifiedPayment {
    let data = protocol::opening_data(quote).unwrap();
    let value: u64 = quote.funding_wei.parse().unwrap();
    let chain_id = quote.network.chain_id;
    let to = hex::decode(provider.state.wallet.strip_prefix("0x").unwrap()).unwrap();
    let fields = |stream: &mut RlpStream| {
        stream.append(&nonce).append(&1_u64).append(&1_000_000_u64)
            .append(&to.as_slice()).append(&value).append(&data);
    };
    let mut unsigned = RlpStream::new_list(9);
    fields(&mut unsigned);
    unsigned.append(&chain_id).append(&0_u8).append(&0_u8);
    let mut hash = [0_u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(&unsigned.out());
    keccak.finalize(&mut hash);
    let key = SecretKey::from_slice(&hex::decode(owner_key).unwrap()).unwrap();
    let (recovery, signature) = Secp256k1::signing_only()
        .sign_ecdsa_recoverable(&Message::from_digest_slice(&hash).unwrap(), &key).serialize_compact();
    let mut signed = RlpStream::new_list(9);
    fields(&mut signed);
    signed.append(&(chain_id * 2 + 35 + recovery.to_i32() as u64));
    for limb in [&signature[..32], &signature[32..]] {
        let first = limb.iter().position(|byte| *byte != 0).unwrap_or(limb.len());
        signed.append(&&limb[first..]);
    }
    let raw = format!("0x{}", hex::encode(signed.out()));
    let envelope = provider.state.chain.validate_payment_envelope(
        &raw, &quote.consumer_wallet, &provider.state.wallet, &quote.funding_wei, &data).unwrap();
    assert_eq!(envelope.nonce, nonce);
    VerifiedPayment {
        network: quote.network.clone(), tx_hash: envelope.tx_hash, from: envelope.from,
        to: envelope.to, value_wei: envelope.value_wei, data: envelope.data, raw_transaction: raw,
        block_hash: protocol::hash_hex(b"isolated account-records accepted-block prerequisite"),
        block_number: 1,
    }
}

/// A paid profiled account: provider-signed terms, acknowledged payment and accepted credit.
fn paid_account(provider: &PreparedProvider, owner_key: &str, nonce: u64) -> Paid {
    let now = protocol::now().unwrap();
    let offer = &provider.offer.payload;
    let quote = protocol::sign(QUOTE_DOMAIN, Quote {
        id: protocol::random_id(), network: offer.network.clone(),
        offer_id: protocol::offer_id(offer).unwrap(), provider_wallet: offer.provider_wallet.clone(),
        consumer_wallet: wallet::address_from_private_key(owner_key).unwrap(),
        model: offer.model.clone(), price: offer.price.clone(), funding_wei: "1000".into(),
        platform_wallet: offer.platform_wallet.clone(), fee_bps: offer.fee_bps,
        max_concurrency: offer.max_concurrency, nonce: protocol::random_id(),
        issued_at: now, expires_at: now + 600,
    }, &provider.state.private_key).unwrap();
    let ledger = &provider.state.ledger;
    ledger.store_quote(&quote, now).unwrap();
    let payment = signed_payment(provider, owner_key, &quote.payload, nonce);
    ledger.register_funding_intent(&quote, &quote.payload.consumer_wallet, &payment.raw_transaction,
        &payment.tx_hash, nonce, &provider.state.private_key, protocol::now, true, &quote.payload.model).unwrap();
    let opened = ledger.open_account(&quote, Some(&payment), protocol::now().unwrap()).unwrap();
    assert_eq!((opened.account.status.as_str(), opened.account.available_wei.as_str()), ("active", "995"));
    Paid { quote, opened, payment }
}

/// Restores the native saved account and original journal row of an accepted paid opening.
fn restore_native(records: &Records, paid: &Paid, nonce: u64) {
    let quote = &paid.quote;
    let credential = paid.opened.credential.as_ref().unwrap();
    let ack = records.provider.state.ledger
        .funding_intent(&quote.payload, &paid.payment.raw_transaction).unwrap().unwrap();
    let path = records.account_path(paid.id());
    wallet::secure_directory(path.parent().unwrap()).unwrap();
    let saved = json!({
        "offer": records.provider.offer, "quote": quote, "account": paid.opened.account,
        "raw_transaction": paid.payment.raw_transaction, "tx_hash": ack.payload.tx_hash,
        "funding_intent": ack, "funding_broadcast_started": true,
        "funding_cancellation_requested": false, "funding_cancellation": null,
        "credentials": [credential], "active_credential": credential.id,
        "receipt_sequence": 0, "topup": null
    });
    let mut file = wallet::create_secret_file(&path).unwrap();
    file.write_all(&serde_json::to_vec(&saved).unwrap()).unwrap();
    file.sync_all().unwrap();
    let journal = Connection::open(records.config.network.scoped_dir(&records.config.data_dir)
        .join("wallet-journal/transactions.sqlite3")).unwrap();
    journal.execute(
        "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,evidence,created_at)
         VALUES(?1,?2,?3,?4,?5,'accepted',?6,?7)",
        params![records.owner, format!("open:{}", paid.id()), nonce.to_string(), paid.payment.tx_hash,
            paid.payment.raw_transaction, serde_json::to_string(&paid.payment).unwrap(),
            quote.payload.issued_at]).unwrap();
}

/// Builds one isolated provider ledger and native consumer directory:
/// - `free`: a genuine native free opening;
/// - `frozen`: original-report and renewed-report known charges, a renewed-report waiver, a
///   restart-recovered unknown request, a hold released as unknown by a funding orphan, and
///   the resulting reversal and funding-loss compensation;
/// - `active`: a known charge and a live hold.
async fn records(directory: &Path, case: u64) -> Records {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut provider = PreparedProvider::new(directory, case, "0", "0",
        listener.local_addr().unwrap(), admin.local_addr().unwrap());
    let tamper = Arc::new(Mutex::new(None::<Conflation>));
    let traffic = Arc::new(Mutex::new(Vec::new()));
    let app = contracts::router(provider.state.clone()).layer(from_fn({
        let tamper = tamper.clone();
        let traffic = traffic.clone();
        let signer = provider.state.private_key.clone();
        move |request: Request, next: Next| {
            let tamper = tamper.clone();
            let traffic = traffic.clone();
            let signer = signer.clone();
            async move {
                let line = format!("{} {}", request.method(), request.uri().path());
                let receipt = line.ends_with("/receipt");
                let response = next.run(request).await;
                traffic.lock().unwrap().push((line, response.status().as_u16()));
                let conflation = tamper.lock().unwrap().clone();
                let Some(conflation) = conflation.filter(|_| receipt && response.status() == StatusCode::OK)
                else { return response };
                let (mut parts, body) = response.into_parts();
                let bytes = to_bytes(body, 1 << 20).await.unwrap();
                let mut signed: Signed<UsageReceipt> = serde_json::from_slice(&bytes).unwrap();
                conflation.apply(&mut signed.payload.account);
                let signed = protocol::sign(RECEIPT_DOMAIN, signed.payload, &signer).unwrap();
                parts.headers.remove(header::CONTENT_LENGTH);
                Response::from_parts(parts, Body::from(serde_json::to_vec(&signed).unwrap()))
            }
        }
    }));
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.ok(); })
            .await.unwrap()
    });

    let owner_key = format!("{:064x}", 71_030 + case);
    let owner = wallet::address_from_private_key(&owner_key).unwrap();
    let config = ConsumerConfig {
        network: provider.state.config.network.clone(), data_dir: directory.join("consumer"),
        catalog_url: None, development: true, dht: None,
    };
    let free = {
        let consumer = Consumer::new(config.clone()).unwrap();
        let import = config.data_dir.join("public-test-scalar.key");
        wallet::save_private_key(&import, &owner_key).unwrap();
        assert_eq!(consumer.wallet_import_file(&import).unwrap(), owner);
        let quote = consumer.quote(&provider.offer, "0").await.unwrap();
        consumer.fund(&provider.offer, &quote).await.unwrap()
    };
    assert_eq!(free.status, "free");

    // Paid serving terms for the same deployment, then an otherwise identical renewal.
    let original = profiled(&provider.offer.payload.model, protocol::now().unwrap() - 10);
    let mut terms = provider.offer.payload.clone();
    terms.revision += 1;
    terms.model = original.clone();
    terms.price = PriceSchedule { input_wei_per_1k: "1000".into(), output_wei_per_1k: "2000".into() };
    provider.offer = protocol::sign(OFFER_DOMAIN, terms, &provider.state.private_key).unwrap();
    provider.offer_id = provider.state.offers.insert(provider.offer.clone()).unwrap();
    let mut renewed = original.clone();
    let qualification = &mut renewed.serving.as_mut().unwrap().qualification;
    qualification.evidence_sha256 = "d".repeat(64);
    qualification.measured_at += 1;
    assert!(renewed != original && original.accepts_execution(&renewed));

    let frozen = paid_account(&provider, &owner_key, 0);
    let active = paid_account(&provider, &owner_key, 1);
    let records = Records {
        provider, _admin: admin, config, owner, free, frozen, active, original, renewed,
        tamper, traffic, stop: Some(stop), server: Some(server),
    };
    restore_native(&records, &records.frozen, 0);
    restore_native(&records, &records.active, 1);

    // Supplied authoritative usage and outcomes at one explicit logical time. Price is 1000 and
    // 2000 wei per 1k tokens, so a request costs prompt + 2 * completion wei.
    let ledger = &records.provider.state.ledger;
    let now = protocol::now().unwrap();
    let outcome = |model: &ModelSpec, usage: &TokenUsage, miss: bool| {
        let profile = model.serving.as_ref().unwrap();
        performance::outcome(profile, true, Some(if miss { 2_000_001 } else { 1 }), Some(50_000),
            Some(usage), "completed", false, Some(2_050_001), &profile.qualification.evidence_sha256)
    };
    let known = TokenUsage { prompt_tokens: 1, completion_tokens: 2 };
    let (id, root) = (records.frozen.id(), &records.frozen.opened.credential.as_ref().unwrap().id);
    for (key, model, miss) in [("original-charged", &records.original, false),
        ("renewed-charged", &records.renewed, false), ("renewed-waived", &records.renewed, true)] {
        let reservation = ledger.reserve(id, root, key, 1, 2, now, model).unwrap();
        ledger.settle_measured(&reservation.id, &known, "completed", Some(&outcome(model, &known, miss)), now).unwrap();
    }
    ledger.reserve(id, root, "renewed-unknown", 7, 11, now, &records.renewed).unwrap();
    assert_eq!(ledger.recover_requests(now).unwrap(), 1);
    let (other, other_root) = (records.active.id(), &records.active.opened.credential.as_ref().unwrap().id);
    let charged = TokenUsage { prompt_tokens: 2, completion_tokens: 3 };
    let reservation = ledger.reserve(other, other_root, "other-charged", 2, 3, now, &records.original).unwrap();
    ledger.settle_measured(&reservation.id, &charged, "completed",
        Some(&outcome(&records.original, &charged, false)), now).unwrap();
    ledger.reserve(other, other_root, "other-held", 1, 1, now, &records.original).unwrap();
    ledger.reserve(id, root, "held-at-orphan", 3, 4, now, &records.original).unwrap();
    // A supplied post-verification observation drives the real reconciliation writer.
    ledger.apply_funding_recheck(&records.frozen.payment.tx_hash, &Ok(PaymentState::Orphaned), now).unwrap();
    records
}

/// The genuine frozen account: 10 wei of known charges were consumed before the 1000-wei
/// opening was reversed. Loss compensation is that consumed shortfall, not request exposure.
fn frozen_expected(records: &Records) -> Value {
    let mut expected = serde_json::to_value(&records.frozen.opened.account).unwrap();
    for (field, value) in [("status", "frozen"), ("funded_wei", "1000"), ("fee_wei", "0"),
        ("spent_wei", "10"), ("reserved_wei", "0"), ("available_wei", "0"), ("reversed_wei", "1000"),
        ("provider_loss_wei", "10"), ("waived_wei", "5")] {
        expected[field] = json!(value);
    }
    expected["usage"] = json!({"prompt_tokens": 3, "completion_tokens": 6});
    expected
}

fn active_expected(records: &Records) -> Value {
    let mut expected = serde_json::to_value(&records.active.opened.account).unwrap();
    for (field, value) in [("spent_wei", "8"), ("reserved_wei", "3"), ("available_wei", "984")] {
        expected[field] = json!(value);
    }
    expected["usage"] = json!({"prompt_tokens": 2, "completion_tokens": 3});
    expected
}

fn wei(account: &Value, field: &str) -> u128 {
    account[field].as_str().unwrap().parse().unwrap()
}

/// The extended balance identity, with an optional extra term on the right-hand side.
fn balances(account: &Value, extra: u128) -> bool {
    wei(account, "funded_wei") + wei(account, "provider_loss_wei")
        == ["fee_wei", "reversed_wei", "spent_wei", "reserved_wei", "available_wei"].iter()
            .map(|field| wei(account, field)).sum::<u128>() + extra
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_receipts_keep_original_terms_waived_usage_unknown_exposure_and_funding_loss_distinct() {
    let directory = tempfile::tempdir().unwrap();
    let mut records = records(directory.path(), 33).await;
    let id = records.frozen.id().to_owned();
    let expected = frozen_expected(&records);
    assert_eq!(serde_json::to_value(records.provider.state.ledger.account(&id).unwrap()).unwrap(), expected);
    let path = records.account_path(&id);
    let restored = std::fs::read(&path).unwrap();
    let journal = records.journal();
    let consumer = Consumer::new(records.config.clone()).unwrap();
    let exported = consumer.credential_export(&id, None).unwrap();

    // Native acceptance of the genuine provider-signed receipt.
    let receipt = consumer.receipt(&id).await.unwrap();
    protocol::verify(RECEIPT_DOMAIN, &receipt, &records.provider.state.wallet).unwrap();
    assert_eq!(receipt.payload.sequence, 1);
    let account = serde_json::to_value(&receipt.payload.account).unwrap();
    assert_eq!(account, expected);
    let requests: BTreeMap<String, RequestView> = receipt.payload.requests.iter()
        .map(|request| (request.request_id.clone(), request.clone())).collect();
    assert_eq!(requests.keys().map(String::as_str).collect::<Vec<_>>(), ["held-at-orphan",
        "original-charged", "renewed-charged", "renewed-unknown", "renewed-waived"]);
    let evidence = |key: &str| requests[key].performance.as_ref().unwrap().evidence_sha256.clone();
    let counters = |key: &str| (requests[key].usage.prompt_tokens, requests[key].usage.completion_tokens);

    // 1. Original account terms: renewal binds the newer report per request only.
    assert_eq!(receipt.payload.account.model, records.original);
    assert_ne!(receipt.payload.account.model, records.renewed);
    assert_eq!(receipt.payload.account.price, records.frozen.quote.payload.price);
    assert_eq!(receipt.payload.account.max_concurrency, records.frozen.quote.payload.max_concurrency);
    for key in ["renewed-charged", "renewed-waived", "renewed-unknown"] {
        assert_eq!(evidence(key), "d".repeat(64), "{key} keeps its admitted renewed report");
    }
    for key in ["original-charged", "held-at-orphan"] {
        assert_eq!(evidence(key), "c".repeat(64), "{key} keeps the original report");
    }

    // 2. Known waived usage: counted in usage and as foregone money, never as balance credit.
    let waived = &requests["renewed-waived"];
    assert_eq!((waived.status.as_str(), counters("renewed-waived")), ("completed", (1, 2)));
    assert_eq!((waived.cost_wei.as_str(), waived.waived_wei.as_str()), ("0", "5"));
    assert_eq!(waived.performance.as_ref().unwrap().remedy, "inference_waiver");
    assert_eq!(waived.performance.as_ref().unwrap().output_tokens, Some(2));
    assert_eq!(account["waived_wei"], "5");
    assert_eq!(account["spent_wei"], "10");
    assert_eq!(account["usage"], json!({"prompt_tokens": 3, "completion_tokens": 6}),
        "settled known usage, including the waived request's, without unknown requests");
    assert!(balances(&account, 0), "the identity holds without the waiver");
    assert!(!balances(&account, wei(&account, "waived_wei")), "a waiver is not a balance term");

    // 3. Unknown request exposure: holds released at provider expense, no invented counters.
    for (key, status, exposure, maximum) in [
        ("renewed-unknown", "restart_unknown_usage", "29", "29"),
        ("held-at-orphan", "funding_orphan_unknown_usage", "11", "11"),
    ] {
        let request = &requests[key];
        assert_eq!((request.status.as_str(), counters(key)), (status, (0, 0)), "{key}");
        assert_eq!((request.cost_wei.as_str(), request.waived_wei.as_str()), ("0", "0"), "{key}");
        assert_eq!((request.provider_loss_wei.as_str(), request.max_cost_wei.as_str()), (exposure, maximum), "{key}");
        // A zero counter is not a measured zero-token execution.
        assert_eq!(request.performance.as_ref().unwrap().output_tokens, None, "{key}");
    }
    assert_eq!(account["reserved_wei"], "0");
    let exposure: u128 = requests.values().map(|request| request.provider_loss_wei.parse::<u128>().unwrap()).sum();
    assert_eq!(exposure, 40);

    // 4. Account funding-loss compensation: the current consumed shortfall after the reversal.
    assert_eq!((account["status"].as_str(), wei(&account, "reversed_wei"), wei(&account, "funded_wei")),
        (Some("frozen"), 1000, 1000));
    assert_eq!((wei(&account, "fee_wei"), wei(&account, "available_wei")), (0, 0));
    assert_eq!(wei(&account, "provider_loss_wei"), wei(&account, "spent_wei"));
    assert_ne!(wei(&account, "provider_loss_wei"), exposure, "request exposure is not accumulated");

    // Native acceptance changed only the receipt watermark.
    let mut saved: Value = serde_json::from_slice(&restored).unwrap();
    saved["receipt_sequence"] = json!(1);
    let accepted = std::fs::read(&path).unwrap();
    assert!(serde_json::from_slice::<Value>(&accepted).unwrap() == saved, "accepted receipt changed more than the watermark");

    // Re-signed conflations reach native receipt validation and are refused without state change.
    let stored = || Connection::open(&records.provider.ledger_path).unwrap().query_row(
        "SELECT account FROM accounts WHERE id=?1", [&id], |row| row.get::<_, String>(0)).unwrap();
    let ledger_account = stored();
    let route = format!("GET /v1/contracts/{id}/receipt");
    for (conflation, refusal) in [
        (Conflation::RenewedAccountModel(records.renewed.clone()), "does not match signed terms"),
        (Conflation::WaiverCredited, "monetary invariant"),
        (Conflation::ExposureAccumulated(exposure), "monetary invariant"),
        (Conflation::LossErased, "monetary invariant"),
    ] {
        *records.tamper.lock().unwrap() = Some(conflation.clone());
        let before = records.traffic();
        let error = consumer.receipt(&id).await.expect_err(&format!("{conflation:?}"));
        assert!(error.contains(refusal), "{conflation:?}: {error}");
        assert!(records.traffic.lock().unwrap()[before..].contains(&(route.clone(), 200)),
            "{conflation:?} must reach a successfully authorized receipt");
        assert!(std::fs::read(&path).unwrap() == accepted, "{conflation:?}: saved account or watermark changed");
        assert!(consumer.credential_export(&id, None).unwrap() == exported, "{conflation:?}: credential changed");
        assert_eq!(records.journal(), journal, "{conflation:?}");
        assert_eq!(stored(), ledger_account, "{conflation:?}");
    }
    *records.tamper.lock().unwrap() = None;
    let corrected = consumer.receipt(&id).await.unwrap();
    assert_eq!(corrected.payload.sequence, 6, "refusals consumed provider sequences, never the watermark");
    assert_eq!(serde_json::to_value(&corrected.payload.account).unwrap(), expected);
    assert_eq!(serde_json::to_value(&corrected.payload.requests).unwrap(),
        serde_json::to_value(&receipt.payload.requests).unwrap());

    // A later account refresh installs the frozen statement; nothing else changes.
    assert_eq!(serde_json::to_value(consumer.account(&id).await.unwrap()).unwrap(), expected);
    drop(consumer);
    let consumer = Consumer::new(records.config.clone()).unwrap();
    let mut saved: Value = serde_json::from_slice(&restored).unwrap();
    saved["receipt_sequence"] = json!(6);
    saved["account"] = expected.clone();
    assert!(serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap() == saved,
        "refresh changed more than the cached account");
    assert!(consumer.accounts().unwrap().iter().any(|account| serde_json::to_value(account).unwrap() == expected));
    assert!(consumer.credential_export(&id, None).unwrap() == exported, "credential rotated");
    assert_eq!(records.journal(), journal);
    assert_eq!(serde_json::to_value(records.provider.state.ledger.quote(&id).unwrap()).unwrap(),
        serde_json::to_value(&records.frozen.quote).unwrap());

    // Reversal and loss balances use wide sums. Response-only claims, re-signed after a
    // successfully authorized genuine receipt: conserving 2 * u128::MAX is accepted (a checked
    // u128 sum overflows), a one-wei shortfall is refused (equal in f64), and a 2^128 shortfall is
    // refused (equal in wrapping u128). Acceptance advances only the watermark; it is not funding.
    let max = u128::MAX;
    let refreshed = std::fs::read(&path).unwrap();
    *records.tamper.lock().unwrap() = Some(Conflation::Wide(wide_money(max, max, max, max)));
    let before = records.traffic();
    let claimed = consumer.receipt(&id).await.unwrap();
    assert!(records.traffic.lock().unwrap()[before..].contains(&(route.clone(), 200)));
    assert_eq!(claimed.payload.sequence, 7);
    let account = serde_json::to_value(&claimed.payload.account).unwrap();
    for (field, amount) in wide_money(max, max, max, max) {
        assert_eq!(account[field], json!(amount), "wide {field}");
    }
    assert_eq!((account["fee_wei"].as_str(), account["available_wei"].as_str(), account["waived_wei"].as_str()),
        (Some("0"), Some("0"), Some("5")));
    let mut saved: Value = serde_json::from_slice(&refreshed).unwrap();
    saved["receipt_sequence"] = json!(7);
    let widened = std::fs::read(&path).unwrap();
    assert!(serde_json::from_slice::<Value>(&widened).unwrap() == saved, "a wide receipt changed more than the watermark");
    for conflation in [
        // One wei short.
        Conflation::Wide(wide_money(max, max - 1, max, max)),
        // 2^128 short.
        Conflation::Wide(wide_money(max, max, max - 1, 0)),
    ] {
        *records.tamper.lock().unwrap() = Some(conflation.clone());
        let before = records.traffic();
        let error = consumer.receipt(&id).await.expect_err(&format!("{conflation:?}"));
        assert!(error.contains("monetary invariant"), "{conflation:?}: {error}");
        assert!(records.traffic.lock().unwrap()[before..].contains(&(route.clone(), 200)),
            "{conflation:?} must reach a successfully authorized receipt");
        assert!(std::fs::read(&path).unwrap() == widened, "{conflation:?}: saved account or watermark changed");
        assert!(consumer.credential_export(&id, None).unwrap() == exported, "{conflation:?}: credential changed");
        assert_eq!(records.journal(), journal, "{conflation:?}");
        assert_eq!(stored(), ledger_account, "{conflation:?}");
    }
    *records.tamper.lock().unwrap() = None;
    let genuine = consumer.receipt(&id).await.unwrap();
    assert_eq!(genuine.payload.sequence, 10);
    assert_eq!(serde_json::to_value(&genuine.payload.account).unwrap(), expected);
    saved["receipt_sequence"] = json!(10);
    assert!(serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap() == saved);
    records.stop().await;
}

/// Complete ledger rows as stored text, with each account's receipt sequence kept apart.
fn ledger_rows(path: &Path) -> (Vec<Vec<String>>, BTreeMap<String, u64>) {
    let db = Connection::open(path).unwrap();
    let rows = [
        "SELECT json_array(id,owner,quote,account,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,request_key,credential,status,created,record) FROM requests ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash",
        "SELECT json_array(id,status,record) FROM fee_batches ORDER BY id",
        "SELECT json_array(sequence,request,model,evidence,miss) FROM performance_observations ORDER BY sequence",
        "SELECT json_array(model,evidence) FROM performance_withdrawals ORDER BY model,evidence",
        "SELECT json_array(key,value) FROM metadata ORDER BY key",
        "SELECT json_array(type,name,sql) FROM sqlite_master ORDER BY type,name",
    ].map(|query| strings(&db, query)).to_vec();
    let mut statement = db.prepare("SELECT id,sequence FROM accounts").unwrap();
    let sequences = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
        .collect::<rusqlite::Result<BTreeMap<_, _>>>().unwrap();
    (rows, sequences)
}

fn stored_text(path: &Path, query: &str, key: &str) -> String {
    Connection::open(path).unwrap().query_row(query, [key], |row| row.get(0)).unwrap()
}

/// Makes the database file self-contained so it can be copied byte for byte.
fn checkpoint(path: &Path) {
    let busy: i64 = Connection::open(path).unwrap()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0)).unwrap();
    assert_eq!(busy, 0, "ledger checkpoint was blocked");
}

fn state_for(provider: &ProviderState, ledger: BillingStore) -> Arc<ProviderState> {
    Arc::new(ProviderState {
        config: provider.config.clone(), chain: provider.chain.clone(), ledger: Arc::new(ledger),
        private_key: provider.private_key.clone(), wallet: provider.wallet.clone(),
        offers: provider.offers.clone(), accepting: AtomicBool::new(true),
        settlement_fault: Default::default(), funding_ready: AtomicBool::new(true),
        qualified_models: RwLock::new(Default::default()),
        embedding_dimensions: RwLock::new(Default::default()),
        model_slots: ProviderState::slots(&provider.config), managed_engines: Default::default(),
    })
}

/// A read-bearer GET through the production control router: status and exact body bytes.
async fn bearer_get(state: &Arc<ProviderState>, path: &str, bearer: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder().method("GET").uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}")).body(Body::empty()).unwrap();
    let response = contracts::router(state.clone()).call(request).await.unwrap();
    let status = response.status();
    (status, to_bytes(response.into_body(), 1 << 20).await.unwrap().to_vec())
}

fn replace_once(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "fixture text must contain {from} exactly once");
    text.replacen(from, to, 1)
}

fn positional(text: &str) -> String {
    let value: Value = serde_json::from_str(text).unwrap();
    Value::Array(ACCOUNT_FIELDS.iter().map(|field| value[*field].clone()).collect()).to_string()
}

/// The frozen account's stored statement with its funded, loss, reversed and spent amounts
/// replaced (originally 1000, 10, 1000 and 10 wei; fee, hold and available stay 0).
fn wide_account(account: &str, funded: u128, loss: u128, reversed: u128, spent: u128) -> String {
    [("funded_wei", "1000", funded), ("provider_loss_wei", "10", loss),
        ("reversed_wei", "1000", reversed), ("spent_wei", "10", spent)]
        .iter().fold(account.to_owned(), |text, (field, from, to)|
            replace_once(&text, &format!("\"{field}\":\"{from}\""), &format!("\"{field}\":\"{to}\"")))
}

/// Corruptions of the frozen account's stored statement. Each is a complete JSON text.
fn account_corruptions(account: &str) -> Vec<(&'static str, String)> {
    let max = u128::MAX;
    vec![
        ("extra member", replace_once(account, "{\"contract_id\"", "{\"unverified_label\":\"x\",\"contract_id\"")),
        ("positional account", positional(account)),
        ("missing member", replace_once(account, "\"waived_wei\":\"5\",", "")),
        // Same value twice: only raw duplicate detection refuses it, not the money checks.
        ("duplicate member", replace_once(account, "\"spent_wei\":\"10\"", "\"spent_wei\":\"10\",\"spent_wei\":\"10\"")),
        ("oversized counter", replace_once(account, "\"usage\":{\"prompt_tokens\":3,",
            "\"usage\":{\"prompt_tokens\":18446744073709551616,")),
        ("non-canonical wei", replace_once(account, "\"spent_wei\":\"10\"", "\"spent_wei\":\"010\"")),
        // The waiver is not a balance term, but it is still validated money.
        ("non-canonical waiver", replace_once(account, "\"waived_wei\":\"5\"", "\"waived_wei\":\"05\"")),
        ("unbalanced money", replace_once(account, "\"available_wei\":\"0\"", "\"available_wei\":\"1\"")),
        // Wide sums: one wei short is equal in f64; 2^128 short is equal in wrapping u128.
        ("wide one-wei shortfall", wide_account(account, max, max - 1, max, max)),
        ("wide 2^128 shortfall", wide_account(account, max, max, max - 11, 10)),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persisted_schema3_and_native_records_read_exactly_and_corruption_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let mut records = records(directory.path(), 34).await;
    let (free, frozen, active) = (records.free.contract_id.clone(), records.frozen.id().to_owned(),
        records.active.id().to_owned());
    let ids = [free.clone(), frozen.clone(), active.clone()];
    // Genuine native writer output for the restored paid files: an accepted receipt, then refreshes.
    let consumer = Consumer::new(records.config.clone()).unwrap();
    assert_eq!(consumer.receipt(&frozen).await.unwrap().payload.sequence, 1);
    assert_eq!(serde_json::to_value(consumer.account(&frozen).await.unwrap()).unwrap(), frozen_expected(&records));
    assert_eq!(serde_json::to_value(consumer.account(&active).await.unwrap()).unwrap(), active_expected(&records));
    let free_bearer = consumer.credential_export(&free, None).unwrap()["api_key"].as_str().unwrap().to_owned();
    drop(consumer);
    let ledger_path = records.provider.ledger_path.clone();
    let account_query = "SELECT account FROM accounts WHERE id=?1";

    // Provider: current genuine schema-3 rows decode through the strict visitors and re-encode to
    // the identical stored bytes, with the money strings exactly as the writer produced them.
    assert_eq!(stored_text(&ledger_path, "SELECT value FROM metadata WHERE key=?1", "schema"), "3");
    let accounts: BTreeMap<String, String> = ids.iter()
        .map(|id| (id.clone(), stored_text(&ledger_path, account_query, id))).collect();
    for (id, money) in [
        (&free, ["0", "0", "0", "0", "0", "0", "0", "0"]),
        (&frozen, ["1000", "0", "10", "0", "0", "1000", "10", "5"]),
        (&active, ["1000", "5", "8", "3", "984", "0", "0", "0"]),
    ] {
        let text = &accounts[id];
        let decoded: AccountView = serde_json::from_str(text).unwrap();
        assert_eq!(&serde_json::to_string(&decoded).unwrap(), text);
        let value: Value = serde_json::from_str(text).unwrap();
        assert_eq!(value.as_object().unwrap().keys().len(), 17);
        for (field, amount) in MONEY.iter().zip(money) {
            assert!(text.contains(&format!("\"{field}\":\"{amount}\"")), "{id} {field}");
        }
        let usage: TokenUsage = serde_json::from_value(value["usage"].clone()).unwrap();
        assert!(text.contains(&format!("\"usage\":{}", serde_json::to_string(&usage).unwrap())));
    }
    let db = Connection::open(&ledger_path).unwrap();
    let request_rows = strings(&db, "SELECT record FROM requests ORDER BY created DESC,rowid DESC");
    assert_eq!(request_rows.len(), 7);
    for text in &request_rows {
        let decoded: RequestView = serde_json::from_str(text).unwrap();
        assert_eq!(&serde_json::to_string(&decoded).unwrap(), text);
    }
    drop(db);
    let (rows, sequences) = ledger_rows(&ledger_path);

    // Reopened public read paths return those exact bytes; only receipt issuance moves a sequence.
    let reopened = state_for(&records.provider.state,
        BillingStore::open(&ledger_path, &records.provider.state.config.network.identity).unwrap());
    let ledger = &reopened.ledger;
    for (id, bearer) in [(&free, free_bearer.as_str()), (&frozen, records.frozen.bearer()),
        (&active, records.active.bearer())] {
        assert_eq!(&serde_json::to_string(&ledger.account(id).unwrap()).unwrap(), &accounts[id]);
        let (status, body) = bearer_get(&reopened, &format!("/v1/contracts/{id}"), bearer).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(String::from_utf8(body).unwrap(), accounts[id], "served bytes are the stored bytes");
        for request in ledger.requests(id, None).unwrap() {
            let text = serde_json::to_string(&request).unwrap();
            assert!(request_rows.contains(&text), "{id}: request record changed");
        }
    }
    let mut operator = ledger.list_accounts().unwrap().iter()
        .map(|account| serde_json::to_string(account).unwrap()).collect::<Vec<_>>();
    operator.sort();
    let mut genuine = accounts.values().cloned().collect::<Vec<_>>();
    genuine.sort();
    assert_eq!(operator, genuine, "operator listing");
    let receipt = ledger.receipt(&frozen, &records.provider.state.private_key, protocol::now().unwrap()).unwrap();
    protocol::verify(RECEIPT_DOMAIN, &receipt, &records.provider.state.wallet).unwrap();
    assert_eq!(serde_json::to_string(&receipt.payload.account).unwrap(), accounts[&frozen]);
    let mut issued = sequences.clone();
    *issued.get_mut(&frozen).unwrap() += 1;
    assert_eq!(ledger_rows(&ledger_path), (rows.clone(), issued.clone()), "reads rewrote a stored record");

    // Native: each saved file holds the provider's stored statement verbatim.
    let journal = records.journal();
    let saved: BTreeMap<String, Vec<u8>> = ids.iter()
        .map(|id| (id.clone(), std::fs::read(records.account_path(id)).unwrap())).collect();
    for id in &ids {
        let text = String::from_utf8(saved[id].clone()).unwrap();
        assert!(text.contains(&format!("\"account\":{}", accounts[id])), "{id}: native account bytes differ");
        let value: Value = serde_json::from_str(&text).unwrap();
        let decoded: AccountView = serde_json::from_value(value["account"].clone()).unwrap();
        assert_eq!(&serde_json::to_string(&decoded).unwrap(), &accounts[id]);
    }
    assert!(String::from_utf8(saved[&frozen].clone()).unwrap().contains("\"receipt_sequence\":1"));
    let consumer = Consumer::new(records.config.clone()).unwrap();
    let mut listed = consumer.accounts().unwrap().iter()
        .map(|account| serde_json::to_string(account).unwrap()).collect::<Vec<_>>();
    listed.sort();
    let mut stored = accounts.values().cloned().collect::<Vec<_>>();
    stored.sort();
    assert_eq!(listed, stored);
    let exports = ids.iter().map(|id| consumer.credential_export(id, None).unwrap()).collect::<Vec<_>>();
    for id in &ids {
        assert!(std::fs::read(records.account_path(id)).unwrap() == saved[id], "{id}: a read rewrote the file");
    }
    assert_eq!(records.journal(), journal);
    drop(consumer);

    // Native corruption fails closed before any provider request, without rewriting the file.
    let target = records.account_path(&frozen);
    let original = String::from_utf8(saved[&frozen].clone()).unwrap();
    let account_member = format!("\"account\":{}", accounts[&frozen]);
    let mut native_cases = account_corruptions(&accounts[&frozen]).into_iter()
        .map(|(name, account)| (name, replace_once(&original, &account_member, &format!("\"account\":{account}"))))
        .collect::<Vec<_>>();
    native_cases.push(("renewed account terms", replace_once(&original, &account_member,
        &format!("\"account\":{}", replace_once(&accounts[&frozen],
            &format!("\"evidence_sha256\":\"{}\"", "c".repeat(64)),
            &format!("\"evidence_sha256\":\"{}\"", "d".repeat(64)))))));
    native_cases.push(("unsupported saved member", replace_once(&original, "{\"offer\":", "{\"schema\":2,\"offer\":")));
    let listing = || {
        let mut names = std::fs::read_dir(target.parent().unwrap()).unwrap()
            .map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
        names.sort();
        names
    };
    let files = listing();
    for (index, (name, corrupted)) in native_cases.iter().enumerate() {
        std::fs::write(&target, corrupted).unwrap();
        let consumer = Consumer::new(records.config.clone()).unwrap();
        let before = records.traffic();
        assert!(consumer.account(&frozen).await.is_err(), "native {name}: account refresh accepted");
        assert!(consumer.receipt(&frozen).await.is_err(), "native {name}: receipt accepted");
        assert!(consumer.credential_export(&frozen, None).is_err(), "native {name}: credential exported");
        // The aggregate listings refuse as a whole rather than silently omitting an account.
        assert!(consumer.accounts().is_err(), "native {name}: listing accepted");
        assert!(consumer.pending_accounts().is_err(), "native {name}: pending listing accepted");
        assert_eq!(records.traffic(), before, "native {name}: contacted the provider");
        assert!(std::fs::read(&target).unwrap() == corrupted.as_bytes(), "native {name}: file rewritten");
        assert_eq!(listing(), files, "native {name}: a file was created or removed");
        assert_eq!(records.journal(), journal, "native {name}");
        // Other accounts remain readable by id, locally and (once) from the provider.
        for (id, export) in ids.iter().zip(&exports).filter(|(id, _)| **id != frozen) {
            assert!(consumer.credential_export(id, None).unwrap() == *export, "native {name}: {id} export");
        }
        if index == 0 {
            assert_eq!(serde_json::to_value(consumer.account(&active).await.unwrap()).unwrap(), active_expected(&records));
            assert!(std::fs::read(records.account_path(&active)).unwrap() == saved[&active], "refresh rewrote equal state");
        }
    }
    std::fs::write(&target, &original).unwrap();
    let consumer = Consumer::new(records.config.clone()).unwrap();
    assert_eq!(serde_json::to_value(consumer.account(&frozen).await.unwrap()).unwrap(), frozen_expected(&records));
    assert!(std::fs::read(&target).unwrap() == saved[&frozen], "restored file differs after refresh");
    assert_eq!(consumer.accounts().unwrap().len(), 3);
    drop(consumer);
    records.stop().await;

    // Provider corruption: copies of the self-contained ledger file, one fault each.
    checkpoint(&ledger_path);
    let frozen_bearer = records.frozen.bearer().to_owned();
    let waived_query = "SELECT record FROM requests WHERE request_key=?1";
    let waived_record = stored_text(&ledger_path, waived_query, "renewed-waived");
    let mut provider_cases = account_corruptions(&accounts[&frozen]).into_iter()
        .map(|(name, text)| (name, "UPDATE accounts SET account=?2 WHERE id=?1", frozen.clone(), text))
        .collect::<Vec<_>>();
    provider_cases.push(("positional request usage", "UPDATE requests SET record=?2 WHERE request_key=?1",
        "renewed-waived".into(), replace_once(&waived_record,
            "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2}", "\"usage\":[1,2]")));
    for (index, (name, update, key, text)) in provider_cases.into_iter().enumerate() {
        let copy = directory.path().join(format!("corrupt-ledger-{index}/billing.sqlite"));
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::copy(&ledger_path, &copy).unwrap();
        assert_eq!(Connection::open(&copy).unwrap().execute(update, params![key, text]).unwrap(), 1);
        checkpoint(&copy);
        let (rows, sequences) = ledger_rows(&copy);
        let state = state_for(&records.provider.state,
            BillingStore::open(&copy, &records.provider.state.config.network.identity).unwrap());
        let ledger = &state.ledger;
        let request_fault = name == "positional request usage";
        let now = protocol::now().unwrap();
        // The faulty record fails closed on every read that needs it, mapped to an internal error.
        assert!(ledger.receipt(&frozen, &records.provider.state.private_key, now).is_err(), "{name}: receipt signed");
        let (status, _) = bearer_get(&state, &format!("/v1/contracts/{frozen}/receipt"), &frozen_bearer).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{name}: receipt route");
        let (status, body) = bearer_get(&state, &format!("/v1/contracts/{frozen}"), &frozen_bearer).await;
        if request_fault {
            assert_eq!(serde_json::to_string(&ledger.account(&frozen).unwrap()).unwrap(), accounts[&frozen]);
            assert_eq!((status, String::from_utf8(body).unwrap()), (StatusCode::OK, accounts[&frozen].clone()));
            assert!(ledger.requests(&frozen, None).is_err(), "{name}: request page decoded");
            assert!(ledger.request_view(&frozen, "renewed-waived").is_err(), "{name}: request decoded");
            assert_eq!(ledger.request_view(&frozen, "renewed-charged").unwrap().cost_wei, "5");
            assert_eq!(ledger.list_accounts().unwrap().len(), 3, "{name}: operator listing");
        } else {
            let error = ledger.account(&frozen).unwrap_err();
            assert!(error.starts_with("ledger corruption:"), "{name}: {error}");
            assert!(ledger.authenticate(&frozen_bearer, now).is_err(), "{name}: bearer authenticated");
            // The operator listing refuses as a whole, as it already did for malformed JSON.
            let error = ledger.list_accounts().unwrap_err();
            assert!(error.starts_with("ledger corruption:"), "{name}: operator listing: {error}");
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{name}: account route");
            assert!(!String::from_utf8(body).unwrap().contains("010"), "{name}: corrupt money served");
            assert_eq!(ledger.requests(&frozen, None).unwrap().len(), 5, "{name}: requests stay readable");
        }
        // Other accounts remain exactly readable, served and signable.
        for (id, bearer) in [(&free, free_bearer.as_str()), (&active, records.active.bearer())] {
            assert_eq!(&serde_json::to_string(&ledger.account(id).unwrap()).unwrap(), &accounts[id], "{name}: {id}");
            let (status, body) = bearer_get(&state, &format!("/v1/contracts/{id}"), bearer).await;
            assert_eq!((status, String::from_utf8(body).unwrap()), (StatusCode::OK, accounts[id].clone()), "{name}: {id}");
        }
        let receipt = ledger.receipt(&active, &records.provider.state.private_key, now).unwrap();
        protocol::verify(RECEIPT_DOMAIN, &receipt, &records.provider.state.wallet).unwrap();
        assert_eq!(serde_json::to_string(&receipt.payload.account).unwrap(), accounts[&active]);
        drop(state);
        // No reset, repair or migration: only the other account's receipt sequence moved.
        let mut issued = sequences.clone();
        *issued.get_mut(&active).unwrap() += 1;
        assert_eq!(ledger_rows(&copy), (rows, issued), "{name}: ledger rows changed");
        assert_eq!(stored_text(&copy, if request_fault { waived_query } else { account_query },
            if request_fault { "renewed-waived" } else { frozen.as_str() }), text, "{name}: faulty record rewritten");
    }

    // A hand-written wide but balanced statement (2 * u128::MAX on each side) is not corruption:
    // the read check uses exact wide sums, so it is served, authenticated, listed and signed
    // exactly as stored. This is an acceptance control for the check, not a genuine record.
    {
        let max = u128::MAX;
        let wide = wide_account(&accounts[&frozen], max, max, max, max);
        let copy = directory.path().join("wide-ledger/billing.sqlite");
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::copy(&ledger_path, &copy).unwrap();
        assert_eq!(Connection::open(&copy).unwrap()
            .execute("UPDATE accounts SET account=?2 WHERE id=?1", params![frozen, wide]).unwrap(), 1);
        checkpoint(&copy);
        let (rows, sequences) = ledger_rows(&copy);
        let state = state_for(&records.provider.state,
            BillingStore::open(&copy, &records.provider.state.config.network.identity).unwrap());
        let ledger = &state.ledger;
        assert_eq!(serde_json::to_string(&ledger.account(&frozen).unwrap()).unwrap(), wide);
        let (status, body) = bearer_get(&state, &format!("/v1/contracts/{frozen}"), &frozen_bearer).await;
        assert_eq!((status, String::from_utf8(body).unwrap()), (StatusCode::OK, wide.clone()));
        assert!(ledger.list_accounts().unwrap().iter().any(|account| serde_json::to_string(account).unwrap() == wide));
        let receipt = ledger.receipt(&frozen, &records.provider.state.private_key, protocol::now().unwrap()).unwrap();
        protocol::verify(RECEIPT_DOMAIN, &receipt, &records.provider.state.wallet).unwrap();
        assert_eq!(serde_json::to_string(&receipt.payload.account).unwrap(), wide);
        drop(state);
        let mut issued = sequences.clone();
        *issued.get_mut(&frozen).unwrap() += 1;
        assert_eq!(ledger_rows(&copy), (rows, issued), "wide control: ledger rows changed");
    }

    // An unsupported ledger schema is refused before any table is created or read for money.
    for schema in ["2", "4"] {
        let copy = directory.path().join(format!("schema-{schema}/billing.sqlite"));
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::copy(&ledger_path, &copy).unwrap();
        Connection::open(&copy).unwrap()
            .execute("UPDATE metadata SET value=?1 WHERE key='schema'", [schema]).unwrap();
        checkpoint(&copy);
        let (rows, sequences) = ledger_rows(&copy);
        let bytes = std::fs::read(&copy).unwrap();
        let error = BillingStore::open(&copy, &records.provider.state.config.network.identity).err().unwrap();
        assert!(error.contains("preserve the old ledger"), "schema {schema}: {error}");
        assert!(std::fs::read(&copy).unwrap() == bytes, "schema {schema}: ledger file rewritten");
        assert_eq!(ledger_rows(&copy), (rows, sequences), "schema {schema}");
        assert_eq!(stored_text(&copy, account_query, &frozen), accounts[&frozen]);
    }
}
