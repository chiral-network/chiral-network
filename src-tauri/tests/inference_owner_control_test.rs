//! Owner control and challenge capacity through production routes and isolated disk ledgers.
//! Ledger prerequisites are distinct from the native/HTTP cases with isolated synthetic RPC.
//! Synthetic execution/frontier observations are not live-chain qualification; no real funds move.
#[path = "support/proposal.rs"]
mod proposal;
#[path = "support/funding.rs"]
mod funding;

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{from_fn, Next},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chiral_network::{
    app::ProviderState,
    auth::{self, AuthPayload},
    billing_store::{BillingStore, CredentialIssueRequest, CredentialView, RequestView},
    chain_client::{ChainClient, ValidatedPaymentEnvelope, VerifiedPayment},
    consumer::{Consumer, ConsumerConfig},
    contracts,
    protocol::{self, Challenge, Credential, FundingIntent, OpenResult, Quote, Signed, TokenUsage, UsageReceipt, RECEIPT_DOMAIN},
    wallet,
};
use proposal::PreparedProvider;
use rlp::RlpStream;
use rusqlite::{params, Connection};
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{net::TcpListener, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Barrier, Mutex}, thread};
use tiny_keccak::{Hasher, Keccak};
use tower::Service;

// Public fixture scalars, never funded or used outside isolated test state.
const OWNER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000007";
const OTHER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000008";
const REQUEST_KEY: &str = "read-proof-request";
const OPEN: &str = "/v1/contracts/open";

fn rate_records(path: &Path) -> Vec<(String, u64, u64)> {
    let db = Connection::open(path).unwrap();
    let mut statement = db.prepare("SELECT bucket,window,count FROM rate_limits ORDER BY bucket").unwrap();
    let records = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    records
}

#[derive(Clone, Copy, Debug)]
enum ReadRoute { Account, Receipt, Funding, Requests, Request, Credentials }
const ROUTES: [ReadRoute; 6] = [ReadRoute::Account, ReadRoute::Receipt, ReadRoute::Funding,
    ReadRoute::Requests, ReadRoute::Request, ReadRoute::Credentials];

impl ReadRoute {
    fn path(self, id: &str) -> String {
        match self {
            Self::Account => format!("/v1/contracts/{id}"),
            Self::Receipt => format!("/v1/contracts/{id}/receipt"),
            Self::Funding => format!("/v1/contracts/{id}/funding"),
            Self::Requests => format!("/v1/contracts/{id}/requests"),
            Self::Request => format!("/v1/contracts/{id}/requests/{REQUEST_KEY}"),
            Self::Credentials => format!("/v1/contracts/{id}/credentials"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CredentialMutation { Issue, Revoke, Rotate }
const MUTATIONS: [CredentialMutation; 3] = [
    CredentialMutation::Issue, CredentialMutation::Revoke, CredentialMutation::Rotate,
];

impl CredentialMutation {
    fn request(self, account: &str, credential: &str, options: &Value) -> (&'static str, String, Vec<u8>) {
        let base = format!("/v1/contracts/{account}/credentials");
        match self {
            Self::Issue => ("POST", base, serde_json::to_vec(options).unwrap()),
            Self::Revoke => ("DELETE", format!("{base}/{credential}"), Vec::new()),
            Self::Rotate => ("POST", format!("{base}/{credential}/rotate"), serde_json::to_vec(options).unwrap()),
        }
    }
}

fn credential_options() -> Value {
    json!({"expires_at": protocol::now().unwrap() + 3600, "spend_cap_wei": null,
        "max_concurrency": 1, "scopes": ["read"]})
}

struct OwnerProof { headers: HeaderMap, nonce: String }
struct Accounts {
    target: OpenResult,
    other: OpenResult,
    read: Credential,
    infer: Credential,
    target_request: RequestView,
    other_request: RequestView,
}
struct Fixture {
    provider: PreparedProvider,
    app: Router,
    _listeners: [TcpListener; 2],
}

fn bearer(credential: &Credential) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, format!("Bearer {}", credential.bearer).parse().unwrap());
    headers
}

fn ledger_snapshot(db: &Connection) -> [Vec<Value>; 14] {
    // Complete rows, including indexed identity/authority columns and retained
    // agreements. JSON records are compared structurally, not by field order.
    [
        "SELECT json_array(id,owner,json(quote),json(account),sequence,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,token_hash,json(record),revoked) FROM credentials ORDER BY id",
        "SELECT json_array(id,contract,request_key,credential,status,created,json(record)) FROM requests ORDER BY id",
        "SELECT json_array(id,owner,expires,json(record)) FROM quotes ORDER BY id",
        "SELECT json_array(tx_hash,contract,json(record),json(evidence),status,checked) FROM funding ORDER BY tx_hash",
        "SELECT json_array(tx_hash,contract,json(record),raw_transaction,updated) FROM funding_attempts ORDER BY tx_hash",
        "SELECT json_array(contract,tx_hash,raw_transaction,json(ack),json(quote),sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash",
        "SELECT json_array(id,status,json(record)) FROM fee_batches ORDER BY id",
        "SELECT json_array(bucket,window,count) FROM rate_limits ORDER BY bucket",
        "SELECT json_array(key,value) FROM metadata ORDER BY key",
        "SELECT json_array(nonce,wallet,expires,json(record)) FROM challenges ORDER BY nonce",
        "SELECT json_array(sequence,request,model,evidence,miss) FROM performance_observations ORDER BY sequence",
        "SELECT json_array(model,evidence) FROM performance_withdrawals ORDER BY model,evidence",
    ].map(|query| {
        db.prepare(query).unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap()
            .map(|row| serde_json::from_str::<Value>(&row.unwrap()).unwrap())
            .collect::<Vec<_>>()
    })
}

fn denied(response: (StatusCode, Value), status: StatusCode) {
    assert_eq!(response.0, status, "{}", response.1);
    assert!(response.1.get("error").is_some(), "{}", response.1);
}

impl Fixture {
    fn new(directory: &Path, case: u64) -> Self {
        let public = TcpListener::bind("127.0.0.1:0").unwrap();
        let admin = TcpListener::bind("127.0.0.1:0").unwrap();
        let provider = PreparedProvider::new(directory, case, "0", "0",
            public.local_addr().unwrap(), admin.local_addr().unwrap());
        let app = contracts::router(provider.state.clone());
        Self { provider, app, _listeners: [public, admin] }
    }

    async fn send(&mut self, method: &str, path: &str, headers: &HeaderMap, body: Vec<u8>)
        -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path)
            .header(header::CONTENT_TYPE, "application/json").body(Body::from(body)).unwrap();
        request.headers_mut().extend(headers.clone());
        let response = self.app.call(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 96 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn post(&mut self, path: &str, value: Value) -> (StatusCode, Value) {
        self.send("POST", path, &HeaderMap::new(), serde_json::to_vec(&value).unwrap()).await
    }

    async fn proof(&mut self, key: &str, method: &str, path: &str, body: &[u8]) -> OwnerProof {
        let owner = wallet::address_from_private_key(key).unwrap();
        let hash = protocol::hash_hex(body);
        let (status, value) = self.post("/v1/auth/challenge",
            json!({"wallet": owner, "method": method, "path": path, "body_hash": hash})).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let challenge: Challenge = serde_json::from_value(value).unwrap();
        let payload = AuthPayload {
            network: self.provider.offer.payload.network.clone(),
            provider_wallet: self.provider.offer.payload.provider_wallet.clone(),
            wallet: owner.clone(), method: method.into(), path: path.into(), body_hash: hash,
            nonce: challenge.nonce.clone(), expires_at: challenge.expires_at,
        };
        let bytes = auth::challenge_payload(&payload).unwrap();
        assert_eq!(hex::decode(challenge.payload_hex.strip_prefix("0x").unwrap()).unwrap(), bytes);
        let signature = wallet::sign_message(key, &bytes).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-chiral-owner", owner.parse().unwrap());
        headers.insert("x-chiral-nonce", challenge.nonce.parse().unwrap());
        headers.insert("x-chiral-signature", signature.parse().unwrap());
        OwnerProof { headers, nonce: challenge.nonce }
    }

    async fn quote(&mut self, funding: &str) -> Signed<Quote> {
        let owner = wallet::address_from_private_key(OWNER_KEY).unwrap();
        let (status, value) = self.post("/v1/contracts/propose", json!({
            "offer_id": self.provider.offer_id, "consumer_wallet": owner, "funding_wei": funding
        })).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        serde_json::from_value(value).unwrap()
    }

    async fn open_free(&mut self) -> OpenResult {
        let quote = self.quote("0").await;
        let body = serde_json::to_vec(&json!({"quote_id": quote.payload.id, "raw_transaction": null})).unwrap();
        let proof = self.proof(OWNER_KEY, "POST", OPEN, &body).await;
        let (status, value) = self.send("POST", OPEN, &proof.headers, body).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        serde_json::from_value(value).unwrap()
    }

    async fn accounts(&mut self) -> Accounts {
        let target = self.open_free().await;
        // Same wallet, different agreement: bearer scope must bind the account, not merely its owner.
        let other = self.open_free().await;
        let now = protocol::now().unwrap();
        let ledger = &self.provider.state.ledger;
        let read = self.scoped_credential(&target.account.contract_id, "read");
        let infer = self.scoped_credential(&target.account.contract_id, "infer");
        let cancelled = |opened: &OpenResult| {
            let reservation = ledger.reserve(&opened.account.contract_id, &opened.credential.as_ref().unwrap().id,
                REQUEST_KEY, 1, 1, now, &opened.account.model).unwrap();
            ledger.settle(&reservation.id, &TokenUsage::default(), "cancelled_unknown_usage", now).unwrap()
        };
        let target_request = cancelled(&target);
        let other_request = cancelled(&other);
        Accounts { target, other, read, infer, target_request, other_request }
    }

    fn check_read(&self, route: ReadRoute, accounts: &Accounts, value: Value) {
        let id = &accounts.target.account.contract_id;
        let check_request = |request: &RequestView| {
            assert_eq!(&request.contract_id, id);
            assert_eq!(request.id, accounts.target_request.id);
            assert_eq!(request.request_id, REQUEST_KEY);
            assert_eq!(request.status, "cancelled_unknown_usage");
            assert_eq!(request.cost_wei, "0");
        };
        match route {
            ReadRoute::Account => {
                assert_eq!(value["contract_id"], *id);
                assert_eq!(value["consumer_wallet"], accounts.target.account.consumer_wallet);
                assert_eq!(value["status"], "free");
            }
            ReadRoute::Receipt => {
                let receipt: Signed<UsageReceipt> = serde_json::from_value(value).unwrap();
                protocol::verify(RECEIPT_DOMAIN, &receipt, &self.provider.state.wallet).unwrap();
                assert_eq!(receipt.payload.network, self.provider.state.config.network.identity);
                assert_eq!(receipt.payload.account.contract_id, *id);
                assert!(!receipt.payload.requests.is_empty());
                for request in &receipt.payload.requests { check_request(request); }
            }
            ReadRoute::Funding => assert_eq!(value, json!([])),
            ReadRoute::Requests => {
                let requests: Vec<RequestView> = serde_json::from_value(value).unwrap();
                assert!(!requests.is_empty());
                for request in &requests { check_request(request); }
            }
            ReadRoute::Request => check_request(&serde_json::from_value(value).unwrap()),
            ReadRoute::Credentials => {
                let entries = value.as_array().unwrap();
                assert!(entries.iter().any(|entry| entry["id"] == accounts.read.id
                    && entry["scopes"] == json!(["read"]) && entry["revoked"] == false));
                for entry in entries {
                    assert_eq!(entry["contract_id"], *id);
                    assert!(entry.get("bearer").is_none(), "credential listing must not disclose bearer secrets");
                }
            }
        }
    }

    async fn read_ok(&mut self, route: ReadRoute, accounts: &Accounts, headers: &HeaderMap) {
        let path = route.path(&accounts.target.account.contract_id);
        let (status, value) = self.send("GET", &path, headers, Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{route:?}: {value}");
        self.check_read(route, accounts, value);
    }

    fn credential_state(&self, account: &str) -> Value {
        let ledger = &self.provider.state.ledger;
        json!({"account": ledger.account(account).unwrap(),
            "credentials": ledger.credentials(account).unwrap()})
    }

    fn credentials_usable(&self, credentials: &[&Credential]) {
        for credential in credentials {
            let context = self.provider.state.ledger.authenticate(&credential.bearer, protocol::now().unwrap()).unwrap();
            assert_eq!(context.account.contract_id, credential.contract_id);
            assert_eq!(context.credential_id, credential.id);
        }
    }

    fn check_mutation(&self, mutation: CredentialMutation, previous: &Credential, value: Value) {
        let ledger = &self.provider.state.ledger;
        let now = protocol::now().unwrap();
        match mutation {
            CredentialMutation::Revoke => assert_eq!(value["revoked"], true),
            CredentialMutation::Issue | CredentialMutation::Rotate => {
                let issued: Credential = serde_json::from_value(value).unwrap();
                assert_eq!(issued.contract_id, previous.contract_id);
                assert_ne!(issued.id, previous.id);
                assert!(issued.bearer != previous.bearer);
                let context = ledger.authenticate(&issued.bearer, now).unwrap();
                assert_eq!(context.account.contract_id, previous.contract_id);
                assert_eq!(context.credential_id, issued.id);
                assert_eq!(context.scopes, ["read"]);
            }
        }
        if matches!(mutation, CredentialMutation::Issue) {
            self.credentials_usable(&[previous]);
        } else {
            assert!(ledger.authenticate(&previous.bearer, now).is_err());
            assert!(ledger.credentials(&previous.contract_id).unwrap().iter()
                .find(|credential| credential.id == previous.id).unwrap().revoked);
        }
    }

    fn scoped_credential(&self, account: &str, scope: &str) -> Credential {
        let now = protocol::now().unwrap();
        self.provider.state.ledger.issue_credential(account,
            &CredentialIssueRequest { expires_at: now + 3600, spend_cap_wei: None,
                max_concurrency: 1, scopes: vec![scope.into()] }, now).unwrap()
    }

    async fn paid_quote(&mut self) -> Signed<Quote> {
        // New signed prices do not rewrite the previously opened free agreements or their credentials.
        let mut offer = self.provider.offer.payload.clone();
        offer.revision += 1;
        offer.price.input_wei_per_1k = "1".into();
        offer.price.output_wei_per_1k = "2".into();
        self.provider.offer = protocol::sign(protocol::OFFER_DOMAIN, offer, &self.provider.state.private_key).unwrap();
        self.provider.offer_id = self.provider.state.offers.insert(self.provider.offer.clone()).unwrap();
        self.quote("100").await
    }

    fn signed_payment(&self, data: &[u8], value: u64, nonce: u64) -> (String, ValidatedPaymentEnvelope) {
        self.signed_payment_for(OWNER_KEY, data, value, nonce)
    }

    fn signed_payment_for(&self, signer: &str, data: &[u8], value: u64, nonce: u64) -> (String, ValidatedPaymentEnvelope) {
        // Independent EIP-155 fixture construction; never expose the runtime's private signer.
        let chain_id = self.provider.offer.payload.network.chain_id;
        let to = hex::decode(self.provider.state.wallet.strip_prefix("0x").unwrap()).unwrap();
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
        let key = SecretKey::from_slice(&hex::decode(signer).unwrap()).unwrap();
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
        let owner = wallet::address_from_private_key(signer).unwrap();
        let envelope = self.provider.state.chain.validate_payment_envelope(
            &raw, &owner, &self.provider.state.wallet, &value.to_string(), data).unwrap();
        (raw, envelope)
    }

    fn acknowledge_fixture(&self, quote: &Signed<Quote>, raw: &str,
        envelope: &ValidatedPaymentEnvelope) -> Signed<FundingIntent> {
        // Admission/spendability is a fixture prerequisite, not an observed chain result.
        self.provider.state.ledger.register_funding_intent(quote, &quote.payload.consumer_wallet,
            raw, &envelope.tx_hash, envelope.nonce, &self.provider.state.private_key,
            protocol::now, true, &quote.payload.model).unwrap()
    }

    async fn paid_account(&mut self) -> (OpenResult, String) {
        let quote = self.paid_quote().await;
        let (raw, envelope) = self.signed_payment(&protocol::opening_data(&quote.payload).unwrap(), 100, 0);
        self.acknowledge_fixture(&quote, &raw, &envelope);
        // Like billing_store_tests, inject an accepted payment AFTER verification for ledger setup only.
        let payment = VerifiedPayment {
            network: quote.payload.network.clone(), tx_hash: envelope.tx_hash,
            from: envelope.from, to: envelope.to, value_wei: envelope.value_wei, data: envelope.data,
            block_hash: protocol::hash_hex(b"isolated owner-control accepted-block fixture"),
            block_number: 1, raw_transaction: raw.clone(),
        };
        let opened = self.provider.state.ledger.open_account(&quote, Some(&payment), protocol::now().unwrap()).unwrap();
        assert_eq!(opened.account.status, "active");
        assert_eq!(opened.account.funded_wei, "100");
        (opened, raw)
    }

    fn consumed(&self, proof: &OwnerProof) {
        let persisted = BillingStore::open(&self.provider.ledger_path, &self.provider.offer.payload.network).unwrap();
        assert!(persisted.challenge(&proof.nonce).is_err());
    }

    fn unopened(&self, quote: &Signed<Quote>) {
        let ledger = &self.provider.state.ledger;
        assert!(ledger.account(&quote.payload.id).unwrap_err().starts_with("not_found:"));
        assert_eq!(serde_json::to_value(ledger.quote(&quote.payload.id).unwrap()).unwrap(),
            serde_json::to_value(quote).unwrap());
    }

    async fn payment_denials(&mut self, path: &str, body: &[u8], credentials: [&Credential; 4]) {
        self.credentials_usable(&credentials);
        denied(self.send("POST", path, &HeaderMap::new(), body.to_vec()).await, StatusCode::UNAUTHORIZED);
        for credential in credentials {
            denied(self.send("POST", path, &bearer(credential), body.to_vec()).await, StatusCode::UNAUTHORIZED);
        }
        let mut incomplete = bearer(credentials[0]);
        incomplete.insert("x-chiral-owner", wallet::address_from_private_key(OWNER_KEY).unwrap().parse().unwrap());
        denied(self.send("POST", path, &incomplete, body.to_vec()).await, StatusCode::UNAUTHORIZED);
        let mut wrong_owner = self.proof(OTHER_KEY, "POST", path, body).await;
        wrong_owner.headers.extend(bearer(credentials[0]));
        denied(self.send("POST", path, &wrong_owner.headers, body.to_vec()).await, StatusCode::FORBIDDEN);
        self.consumed(&wrong_owner);
        denied(self.send("POST", path, &wrong_owner.headers, body.to_vec()).await, StatusCode::UNAUTHORIZED);
        self.credentials_usable(&credentials);
    }

    fn pending_challenge(&self, wallet: &str, sequence: u64, expires_at: u64) -> AuthPayload {
        AuthPayload {
            network: self.provider.offer.payload.network.clone(),
            provider_wallet: self.provider.state.wallet.clone(), wallet: wallet.into(),
            method: "GET".into(), path: OPEN.into(), body_hash: protocol::hash_hex(b""),
            nonce: format!("0x{sequence:064x}"), expires_at,
        }
    }

    fn seed_challenge_snapshot(&self, count: usize) {
        // Conditional restored-state ceiling, not 10,000 native issuances in one rate/TTL window.
        // Far-future snapshot expiries avoid wall-clock races; native lifetime is tested separately.
        let mut db = Connection::open(&self.provider.ledger_path).unwrap();
        let tx = db.transaction().unwrap();
        {
            let mut insert = tx.prepare(
                "INSERT INTO challenges(nonce,wallet,expires,record) VALUES(?1,?2,?3,?4)").unwrap();
            for index in 0..count {
                let wallet = format!("0x{:040x}", 0x10000 + index / 8);
                let payload = self.pending_challenge(&wallet, 100_000 + index as u64, i64::MAX as u64);
                insert.execute(params![payload.nonce, payload.wallet, payload.expires_at,
                    serde_json::to_string(&payload).unwrap()]).unwrap();
            }
        }
        tx.commit().unwrap();
    }

    fn challenge_records(&self) -> Vec<(String, String, u64, String)> {
        let db = Connection::open(&self.provider.ledger_path).unwrap();
        let mut statement = db.prepare(
            "SELECT nonce,wallet,expires,record FROM challenges ORDER BY nonce").unwrap();
        let records = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        records
    }

    fn race_challenges(&self, candidates: [&AuthPayload; 2], now: u64) -> usize {
        let network = &self.provider.offer.payload.network;
        self.provider.state.ledger.bind_provider(&self.provider.state.wallet).unwrap();
        let before_rates = rate_records(&self.provider.ledger_path);
        let barrier = Barrier::new(2);
        let results = thread::scope(|scope| {
            let handles = candidates.map(|candidate| {
                let barrier = &barrier;
                let path = &self.provider.ledger_path;
                scope.spawn(move || {
                    let ledger = BillingStore::open(path, network).unwrap();
                    barrier.wait();
                    ledger.create_challenge(candidate, now)
                })
            });
            handles.map(|handle| handle.join().unwrap())
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let winner = results.iter().position(|result| result.is_ok()).unwrap();
        let error = results[1 - winner].as_ref().unwrap_err();
        assert!(error.starts_with("rate_limit:"), "{error}");
        assert_eq!(serde_json::to_value(self.provider.state.ledger.challenge(&candidates[winner].nonce).unwrap()).unwrap(),
            serde_json::to_value(candidates[winner]).unwrap());
        assert!(self.provider.state.ledger.challenge(&candidates[1 - winner].nonce).is_err());

        // Compare with one successful admission, without pinning rate ceilings or window lengths.
        let control_path = self.provider.ledger_path.with_file_name("challenge-rate-control.sqlite");
        let control = BillingStore::open(&control_path, network).unwrap();
        control.bind_provider(&self.provider.state.wallet).unwrap();
        let db = Connection::open(&control_path).unwrap();
        for (bucket, window, count) in before_rates {
            db.execute("INSERT INTO rate_limits(bucket,window,count) VALUES(?1,?2,?3)",
                params![bucket, window, count]).unwrap();
        }
        control.create_challenge(candidates[winner], now).unwrap();
        assert_eq!(rate_records(&self.provider.ledger_path), rate_records(&control_path));
        winner
    }

    async fn accounting_account(&mut self, profiled: bool) -> (Signed<Quote>, OpenResult) {
        use chiral_network::performance;

        // Signed synthetic terms and post-verification payment are prerequisites,
        // not observed enrollment, hardware, engine or safe-chain evidence.
        let mut terms = self.paid_quote().await.payload;
        terms.id = protocol::random_id();
        terms.nonce = protocol::random_id();
        terms.funding_wei = "1000".into();
        if profiled {
            terms.model.serving = Some(protocol::ServingProfile {
                hf_repo: "test/proposal".into(), tokenizer_repo: "test/tokenizer".into(),
                artifact_manifest_sha256: "a".repeat(64),
                engine: "llama.cpp".into(), engine_version: "test-only".into(),
                runtime_sha256: "b".repeat(64), platform: "linux".into(),
                backend: "cuda".into(), memory_kind: "dedicated".into(),
                devices: vec![protocol::EngineDevice {
                    id: "GPU-ledger-test".into(), name: "synthetic-ledger-test-device".into(),
                    memory_mib: 8192, driver_version: "test".into(), engine_device: "CUDA0".into(),
                }],
                memory_reserve_mib: 256, gpu_layers: 999, split_mode: "none".into(),
                tensor_split: vec![], cache_type_k: "f16".into(), cache_type_v: "f16".into(),
                flash_attention: true, batch_size: 512, ubatch_size: 128,
                capacity_tier: "8gb".into(), region: "test".into(), max_concurrency: 2,
                arrival_milli_rps: 1000, output_tokens_per_second: 20,
                policy: performance::POLICY.into(),
                qualification: protocol::QualificationIdentity {
                    evidence_sha256: "c".repeat(64), measured_at: terms.issued_at,
                },
            });
            performance::validate_profile(&terms.model, terms.model.serving.as_ref().unwrap()).unwrap();
        }
        let mut offer = self.provider.offer.payload.clone();
        offer.revision += 1;
        offer.model = terms.model.clone();
        self.provider.offer = protocol::sign(protocol::OFFER_DOMAIN, offer,
            &self.provider.state.private_key).unwrap();
        self.provider.offer_id = self.provider.state.offers.insert(self.provider.offer.clone()).unwrap();
        terms.offer_id = self.provider.offer_id.clone();
        let quote = protocol::sign(protocol::QUOTE_DOMAIN, terms,
            &self.provider.state.private_key).unwrap();
        self.provider.state.ledger.store_quote(&quote, protocol::now().unwrap()).unwrap();
        let (raw, envelope) = self.signed_payment(
            &protocol::opening_data(&quote.payload).unwrap(), 1000, 0);
        self.acknowledge_fixture(&quote, &raw, &envelope);
        let payment = VerifiedPayment {
            network: quote.payload.network.clone(), tx_hash: envelope.tx_hash,
            from: envelope.from, to: envelope.to, value_wei: envelope.value_wei,
            data: envelope.data, raw_transaction: raw,
            block_hash: protocol::hash_hex(b"isolated performance accounting prerequisite"),
            block_number: 1,
        };
        let opened = self.provider.state.ledger.open_account(
            &quote, Some(&payment), protocol::now().unwrap()).unwrap();
        (quote, opened)
    }
}

#[tokio::test]
async fn every_control_read_requires_its_owner_or_an_account_scoped_read_bearer() {
    let directory = tempfile::tempdir().unwrap();
    for (index, route) in ROUTES.into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1000 + index as u64);
        let accounts = fixture.accounts().await;
        let path = route.path(&accounts.target.account.contract_id);
        denied(fixture.send("GET", &path, &HeaderMap::new(), Vec::new()).await, StatusCode::UNAUTHORIZED);
        for credential in [&accounts.infer, accounts.other.credential.as_ref().unwrap()] {
            denied(fixture.send("GET", &path, &bearer(credential), Vec::new()).await, StatusCode::FORBIDDEN);
        }
        fixture.read_ok(route, &accounts, &bearer(&accounts.read)).await;
        fixture.read_ok(route, &accounts, &bearer(accounts.target.credential.as_ref().unwrap())).await;
        let mut proof = fixture.proof(OWNER_KEY, "GET", &path, b"").await;
        fixture.read_ok(route, &accounts, &proof.headers).await;
        proof.headers.extend(bearer(&accounts.read));
        denied(fixture.send("GET", &path, &proof.headers, Vec::new()).await, StatusCode::UNAUTHORIZED);
        // The fallback bearer is still valid; refusal is due to the explicit, consumed owner proof.
        fixture.read_ok(route, &accounts, &bearer(&accounts.read)).await;
    }
}

#[tokio::test]
async fn explicit_incomplete_and_wrong_owner_proofs_never_downgrade_to_a_read_bearer() {
    let directory = tempfile::tempdir().unwrap();
    for (index, route) in ROUTES.into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1100 + index as u64);
        let accounts = fixture.accounts().await;
        let path = route.path(&accounts.target.account.contract_id);
        let mut incomplete = bearer(&accounts.read);
        incomplete.insert("x-chiral-owner", accounts.target.account.consumer_wallet.parse().unwrap());
        denied(fixture.send("GET", &path, &incomplete, Vec::new()).await, StatusCode::UNAUTHORIZED);
        let mut wrong_owner = fixture.proof(OTHER_KEY, "GET", &path, b"").await;
        wrong_owner.headers.extend(bearer(&accounts.read));
        denied(fixture.send("GET", &path, &wrong_owner.headers, Vec::new()).await, StatusCode::FORBIDDEN);
        assert!(fixture.provider.state.ledger.challenge(&wrong_owner.nonce).is_err());
        denied(fixture.send("GET", &path, &wrong_owner.headers, Vec::new()).await, StatusCode::UNAUTHORIZED);
        fixture.read_ok(route, &accounts, &bearer(&accounts.read)).await;
    }
}

#[tokio::test]
async fn every_control_get_rejects_nonempty_bodies_even_with_matching_authority() {
    let directory = tempfile::tempdir().unwrap();
    for (index, route) in ROUTES.into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1200 + index as u64);
        let accounts = fixture.accounts().await;
        let path = route.path(&accounts.target.account.contract_id);
        denied(fixture.send("GET", &path, &bearer(&accounts.read), b"{}".to_vec()).await, StatusCode::BAD_REQUEST);
        let proof = fixture.proof(OWNER_KEY, "GET", &path, b"{}").await;
        denied(fixture.send("GET", &path, &proof.headers, b"{}".to_vec()).await, StatusCode::BAD_REQUEST);
        fixture.read_ok(route, &accounts, &bearer(&accounts.read)).await;
    }
}

#[tokio::test]
async fn request_lists_reject_unsupported_cursors_with_bearer_or_bound_owner_authority() {
    let directory = tempfile::tempdir().unwrap();
    for (index, query) in ["before=", "before=-1", "before=1&before=2", "after=1",
        "before=18446744073709551616", "before=9223372036854775808", "before=%31", "before=1&extra=2"]
        .into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1300 + index as u64);
        let accounts = fixture.accounts().await;
        let path = format!("{}?{query}", ReadRoute::Requests.path(&accounts.target.account.contract_id));
        for headers in [bearer(&accounts.read), fixture.proof(OWNER_KEY, "GET", &path, b"").await.headers] {
            let response = fixture.send("GET", &path, &headers, Vec::new()).await;
            assert_eq!(response.1["error"]["code"], "invalid_query", "{query}: {}", response.1);
            denied(response, StatusCode::BAD_REQUEST);
        }
        fixture.read_ok(ReadRoute::Requests, &accounts, &bearer(&accounts.read)).await;
    }
}

#[tokio::test]
async fn request_cursor_bounds_filter_history_and_bind_the_literal_owner_target() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 1400);
    let accounts = fixture.accounts().await;
    let base = ReadRoute::Requests.path(&accounts.target.account.contract_id);
    for (before, included) in [(0, false), (accounts.target_request.created_at, false),
        (accounts.target_request.created_at + 1, true), (i64::MAX as u64, true)] {
        let (status, value) = fixture.send("GET", &format!("{base}?before={before}"),
            &bearer(&accounts.read), Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        if included { fixture.check_read(ReadRoute::Requests, &accounts, value); }
        else { assert_eq!(value, json!([])); }
    }
    let intended = format!("{base}?before=000{}", i64::MAX);
    let equivalent = format!("{base}?before={}", i64::MAX);
    let proof = fixture.proof(OWNER_KEY, "GET", &intended, b"").await;
    denied(fixture.send("GET", &equivalent, &proof.headers, Vec::new()).await, StatusCode::UNAUTHORIZED);
    assert!(fixture.provider.state.ledger.challenge(&proof.nonce).is_ok());
    let (status, value) = fixture.send("GET", &intended, &proof.headers, Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    fixture.check_read(ReadRoute::Requests, &accounts, value);
}

#[tokio::test]
async fn foreign_request_lookup_consumes_authority_without_exposing_another_account() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 1500);
    let accounts = fixture.accounts().await;
    let base = ReadRoute::Requests.path(&accounts.target.account.contract_id);
    let foreign = format!("{base}/{}", accounts.other_request.id);
    let proof = fixture.proof(OWNER_KEY, "GET", &foreign, b"").await;
    denied(fixture.send("GET", &foreign, &proof.headers, Vec::new()).await, StatusCode::NOT_FOUND);
    // A post-authentication business refusal must durably consume authority, not roll it back.
    let persisted = BillingStore::open(&fixture.provider.ledger_path, &fixture.provider.offer.payload.network).unwrap();
    assert!(persisted.challenge(&proof.nonce).is_err());
    assert_eq!(persisted.request_view(&accounts.other.account.contract_id, &accounts.other_request.id)
        .unwrap().status, "cancelled_unknown_usage");
    denied(fixture.send("GET", &foreign, &proof.headers, Vec::new()).await, StatusCode::UNAUTHORIZED);
    let own = format!("{base}/{}", accounts.target_request.id);
    let fresh = fixture.proof(OWNER_KEY, "GET", &own, b"").await;
    let (status, value) = fixture.send("GET", &own, &fresh.headers, Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    fixture.check_read(ReadRoute::Request, &accounts, value);
    // Equal client request keys on the two accounts must still resolve within the authorized account.
    fixture.read_ok(ReadRoute::Request, &accounts, &bearer(&accounts.read)).await;
}

#[tokio::test]
async fn credential_mutations_accept_only_the_account_owner_not_any_bearer() {
    let directory = tempfile::tempdir().unwrap();
    for (index, mutation) in MUTATIONS.into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1600 + index as u64);
        let accounts = fixture.accounts().await;
        let id = &accounts.target.account.contract_id;
        let before = fixture.credential_state(id);
        let other_before = fixture.credential_state(&accounts.other.account.contract_id);
        let (method, path, body) = mutation.request(id, &accounts.read.id, &credential_options());
        denied(fixture.send(method, &path, &HeaderMap::new(), body.clone()).await, StatusCode::UNAUTHORIZED);
        for credential in [accounts.target.credential.as_ref().unwrap(), &accounts.read, &accounts.infer, accounts.other.credential.as_ref().unwrap()] {
            denied(fixture.send(method, &path, &bearer(credential), body.clone()).await, StatusCode::UNAUTHORIZED);
        }
        let mut incomplete = bearer(&accounts.read);
        incomplete.insert("x-chiral-owner", accounts.target.account.consumer_wallet.parse().unwrap());
        denied(fixture.send(method, &path, &incomplete, body.clone()).await, StatusCode::UNAUTHORIZED);
        let mut wrong_owner = fixture.proof(OTHER_KEY, method, &path, &body).await;
        wrong_owner.headers.extend(bearer(&accounts.read));
        denied(fixture.send(method, &path, &wrong_owner.headers, body.clone()).await, StatusCode::FORBIDDEN);
        assert!(fixture.provider.state.ledger.challenge(&wrong_owner.nonce).is_err());
        denied(fixture.send(method, &path, &wrong_owner.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
        assert_eq!(fixture.credential_state(id), before);
        fixture.credentials_usable(&[accounts.target.credential.as_ref().unwrap(), &accounts.read, &accounts.infer, accounts.other.credential.as_ref().unwrap()]);

        let mut owner = fixture.proof(OWNER_KEY, method, &path, &body).await;
        let (status, value) = fixture.send(method, &path, &owner.headers, body.clone()).await;
        assert_eq!(status, StatusCode::OK, "{mutation:?}: {value}");
        fixture.check_mutation(mutation, &accounts.read, value);
        let after = fixture.credential_state(id);
        owner.headers.extend(bearer(accounts.target.credential.as_ref().unwrap()));
        denied(fixture.send(method, &path, &owner.headers, body).await, StatusCode::UNAUTHORIZED);
        assert_eq!(fixture.credential_state(id), after);
        assert_eq!(fixture.credential_state(&accounts.other.account.contract_id), other_before);
        fixture.credentials_usable(&[accounts.target.credential.as_ref().unwrap(), &accounts.infer, accounts.other.credential.as_ref().unwrap()]);
    }
}

#[tokio::test]
async fn credential_identifiers_cannot_escape_the_signed_account_even_for_the_same_owner() {
    let directory = tempfile::tempdir().unwrap();
    for (index, mutation) in [CredentialMutation::Revoke, CredentialMutation::Rotate].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1700 + index as u64);
        let accounts = fixture.accounts().await;
        let target = &accounts.target.account.contract_id;
        let other = &accounts.other.account.contract_id;
        let before = fixture.credential_state(target);
        let other_before = fixture.credential_state(other);
        let options = credential_options();
        let (method, path, body) = mutation.request(target, &accounts.other.credential.as_ref().unwrap().id, &options);
        let proof = fixture.proof(OWNER_KEY, method, &path, &body).await;
        denied(fixture.send(method, &path, &proof.headers, body.clone()).await, StatusCode::NOT_FOUND);
        let persisted = BillingStore::open(&fixture.provider.ledger_path, &fixture.provider.offer.payload.network).unwrap();
        assert!(persisted.challenge(&proof.nonce).is_err());
        denied(fixture.send(method, &path, &proof.headers, body).await, StatusCode::UNAUTHORIZED);
        assert_eq!(fixture.credential_state(target), before);
        assert_eq!(fixture.credential_state(other), other_before);
        fixture.credentials_usable(&[accounts.target.credential.as_ref().unwrap(), &accounts.read, &accounts.infer, accounts.other.credential.as_ref().unwrap()]);

        // The credential exists and belongs to this wallet, but requires its own account's signed target.
        let (method, path, body) = mutation.request(other, &accounts.other.credential.as_ref().unwrap().id, &options);
        let correct = fixture.proof(OWNER_KEY, method, &path, &body).await;
        let (status, value) = fixture.send(method, &path, &correct.headers, body).await;
        assert_eq!(status, StatusCode::OK, "{mutation:?}: {value}");
        fixture.check_mutation(mutation, accounts.other.credential.as_ref().unwrap(), value);
        assert_eq!(fixture.credential_state(target), before);
        fixture.credentials_usable(&[accounts.target.credential.as_ref().unwrap(), &accounts.read, &accounts.infer]);
    }
}

#[tokio::test]
async fn credential_business_refusals_consume_proof_without_partial_mutations() {
    let directory = tempfile::tempdir().unwrap();
    for (index, mutation) in MUTATIONS.into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1800 + index as u64);
        let accounts = fixture.accounts().await;
        let id = &accounts.target.account.contract_id;
        let before = fixture.credential_state(id);
        let options = credential_options();
        let mut invalid = options.clone();
        invalid["max_concurrency"] = json!(0);
        let (method, path, mut body) = mutation.request(id, &accounts.read.id, &invalid);
        if matches!(mutation, CredentialMutation::Revoke) { body = b"{}".to_vec(); }
        let proof = fixture.proof(OWNER_KEY, method, &path, &body).await;
        let response = fixture.send(method, &path, &proof.headers, body.clone()).await;
        assert_eq!(response.1["error"]["code"], if matches!(mutation, CredentialMutation::Revoke) {
            "invalid_request"
        } else { "invalid_credential" });
        denied(response, StatusCode::BAD_REQUEST);
        // Credential rollback must not roll back the independently consumed owner challenge.
        let persisted = BillingStore::open(&fixture.provider.ledger_path, &fixture.provider.offer.payload.network).unwrap();
        assert!(persisted.challenge(&proof.nonce).is_err());
        assert_eq!(serde_json::to_value(persisted.credentials(id).unwrap()).unwrap(), before["credentials"]);
        assert_eq!(fixture.credential_state(id), before);
        fixture.credentials_usable(&[accounts.target.credential.as_ref().unwrap(), &accounts.read, &accounts.infer, accounts.other.credential.as_ref().unwrap()]);
        denied(fixture.send(method, &path, &proof.headers, body).await, StatusCode::UNAUTHORIZED);

        let (method, path, body) = mutation.request(id, &accounts.read.id, &options);
        let fresh = fixture.proof(OWNER_KEY, method, &path, &body).await;
        let (status, value) = fixture.send(method, &path, &fresh.headers, body).await;
        assert_eq!(status, StatusCode::OK, "{mutation:?}: {value}");
        fixture.check_mutation(mutation, &accounts.read, value);
        fixture.credentials_usable(&[accounts.target.credential.as_ref().unwrap(), &accounts.infer, accounts.other.credential.as_ref().unwrap()]);
    }
}

#[tokio::test]
async fn opening_request_schema_rejects_invalid_shapes_before_owner_consumption() {
    let directory = tempfile::tempdir().unwrap();
    let invalid_bodies: &[(&str, fn(&str, Option<&str>) -> Vec<u8>)] = &[
        ("complete positional", |id, raw| serde_json::to_vec(&json!([id, raw])).unwrap()),
        ("short positional", |id, _| serde_json::to_vec(&json!([id])).unwrap()),
        ("invalid JSON", |_, _| b"{".to_vec()),
        ("null root", |_, _| b"null".to_vec()),
        ("boolean root", |_, _| b"true".to_vec()),
        ("numeric root", |_, _| b"1".to_vec()),
        ("string root", |id, _| serde_json::to_vec(id).unwrap()),
        ("missing identifier", |_, raw| serde_json::to_vec(&json!({"raw_transaction": raw})).unwrap()),
        ("null identifier", |_, raw| serde_json::to_vec(&json!({"quote_id": null, "raw_transaction": raw})).unwrap()),
        ("numeric identifier", |_, raw| serde_json::to_vec(&json!({"quote_id": 1, "raw_transaction": raw})).unwrap()),
        ("array identifier", |id, raw| serde_json::to_vec(&json!({"quote_id": [id], "raw_transaction": raw})).unwrap()),
        ("object identifier", |id, raw| serde_json::to_vec(&json!({"quote_id": {"id": id}, "raw_transaction": raw})).unwrap()),
        ("boolean payment", |id, _| serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": true})).unwrap()),
        ("integer payment", |id, _| serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": 1})).unwrap()),
        ("fractional payment", |id, _| serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": 1.5})).unwrap()),
        ("array payment", |id, raw| serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": [raw]})).unwrap()),
        ("object payment", |id, raw| serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": {"hex": raw}})).unwrap()),
        ("unknown member", |id, raw| serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": raw, "wallet": "ignored"})).unwrap()),
        ("duplicate identifier", |id, raw| format!(r#"{{"quote_id":"{id}","quote_id":"{id}","raw_transaction":{}}}"#,
            serde_json::to_string(&raw).unwrap()).into_bytes()),
        ("duplicate payment", |id, raw| {
            let raw = serde_json::to_string(&raw).unwrap();
            format!(r#"{{"quote_id":"{id}","raw_transaction":{raw},"raw_transaction":{raw}}}"#).into_bytes()
        }),
    ];
    for (state, replay) in [false, true].into_iter().enumerate() {
        for (index, (name, invalid_body)) in invalid_bodies.iter().enumerate() {
            // Isolate every unconsumed malformed proof from pending/rate ceilings.
            let mut fixture = Fixture::new(directory.path(),
                66_000 + (state * invalid_bodies.len() + index) as u64);
            let unrelated = fixture.open_free().await;
            let (quote, raw, mut accepted) = if replay {
                // Paid credit is a post-verification ledger prerequisite, not live chain evidence.
                let (opened, raw) = fixture.paid_account().await;
                let quote = fixture.provider.state.ledger.quote(&opened.account.contract_id).unwrap();
                (quote, Some(raw), Some(opened))
            } else {
                (fixture.quote("0").await, None, None)
            };
            let db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let body = invalid_body(&quote.payload.id, raw.as_deref());
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
            let before = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}, replay={replay}: {document}");
            assert_eq!(document["error"]["code"], "invalid_request", "{name}, replay={replay}");
            assert_eq!(ledger_snapshot(&db), before,
                "{name}, replay={replay}: schema refusal must preserve every row, including its proof");
            fixture.credentials_usable(&[unrelated.credential.as_ref().unwrap()]);
            if let Some(opened) = &accepted {
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap()]);
            } else {
                fixture.unopened(&quote);
            }

            // Omitted payment creates the free account; explicit null then recovers it.
            // Paid replay instead carries the exact acknowledged original string.
            let valid_bodies = if let Some(raw) = &raw {
                vec![serde_json::to_vec(&json!({"quote_id": quote.payload.id, "raw_transaction": raw})).unwrap()]
            } else {
                vec![
                    serde_json::to_vec(&json!({"quote_id": quote.payload.id})).unwrap(),
                    serde_json::to_vec(&json!({"quote_id": quote.payload.id, "raw_transaction": null})).unwrap(),
                ]
            };
            for body in valid_bodies {
                let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
                let before = ledger_snapshot(&db);
                let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
                assert_eq!(status, StatusCode::OK, "{name}, replay={replay}: {document}");
                let opened: OpenResult = serde_json::from_value(document).unwrap();
                assert_eq!(opened.account.contract_id, quote.payload.id);
                assert_eq!(opened.account.consumer_wallet, quote.payload.consumer_wallet);
                if let Some(previous) = &accepted {
                    assert_eq!(serde_json::to_value(&opened.account).unwrap(),
                        serde_json::to_value(&previous.account).unwrap());
                    assert!(opened.credential.is_none(), "replay must not issue another secret");
                    let after = ledger_snapshot(&db);
                    for row in [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13] {
                        assert_eq!(after[row], before[row], "{name}: replay changed table {row}");
                    }
                    fixture.credentials_usable(&[previous.credential.as_ref().unwrap()]);
                } else {
                    assert_eq!(opened.account.status, "free");
                    assert_eq!(opened.account.funded_wei, "0");
                    fixture.credentials_usable(&[opened.credential.as_ref().unwrap()]);
                    accepted = Some(opened);
                }
                fixture.consumed(&proof);
                fixture.credentials_usable(&[unrelated.credential.as_ref().unwrap()]);
            }
        }
    }
}

#[tokio::test]
async fn opening_payment_presence_and_original_guards_precede_chain_verification() {
    let directory = tempfile::tempdir().unwrap();
    for (case, (label, paid, registered, replay)) in [
        ("fresh free", false, false, false),
        ("existing free", false, false, true),
        ("unregistered paid", true, false, false),
        ("acknowledged paid", true, true, false),
        ("existing paid", true, true, true),
    ].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 67_000 + case as u64);
        let rpc_calls = Arc::new(Mutex::new(Vec::new()));
        let observed = rpc_calls.clone();
        let rpc_app = Router::new().fallback(move |request: Request| {
            let observed = observed.clone();
            async move {
                observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
                StatusCode::SERVICE_UNAVAILABLE
            }
        });
        fixture._listeners[0].set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
        let rpc_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });
        let unrelated = fixture.open_free().await;
        let (quote, opened) = if replay {
            if paid {
                // Accepted credit is a post-verification prerequisite, not observed chain execution.
                let (quote, opened) = fixture.accounting_account(false).await;
                (quote, Some(opened))
            } else {
                let opened = fixture.open_free().await;
                let quote = fixture.provider.state.ledger.quote(&opened.account.contract_id).unwrap();
                (quote, Some(opened))
            }
        } else {
            (if paid { fixture.paid_quote().await } else { fixture.quote("0").await }, None)
        };
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let data = protocol::opening_data(&quote.payload).unwrap();
        // Free negative controls supply a nonzero signed transfer, never a fabricated zero payment.
        let amount = if paid { quote.payload.funding_wei.parse::<u64>().unwrap() } else { 100 };
        let raw: String = if paid && replay {
            db.query_row("SELECT raw_transaction FROM funding_intents WHERE contract=?1",
                [&quote.payload.id], |row| row.get(0)).unwrap()
        } else {
            let (raw, envelope) = fixture.signed_payment(&data, amount, 0);
            if registered {
                fixture.acknowledge_fixture(&quote, &raw, &envelope);
            }
            raw
        };
        let body_for = |payment: Value| json!({"quote_id": quote.payload.id, "raw_transaction": payment});
        let mut refusals = vec![
            ("empty string", body_for(json!("")), "invalid_payment"),
            ("empty hex", body_for(json!("0x")), "invalid_payment"),
            ("malformed hex", body_for(json!("0xgg")), "invalid_payment"),
        ];
        if paid {
            refusals.push(("omitted", json!({"quote_id": quote.payload.id}), "funding_intent_required"));
            refusals.push(("null", body_for(Value::Null), "funding_intent_required"));
            if registered {
                let alternate_hex = format!("0x{}", raw[2..].to_ascii_uppercase());
                assert_ne!(alternate_hex, raw);
                assert_eq!(hex::decode(&alternate_hex[2..]).unwrap(), hex::decode(&raw[2..]).unwrap());
                refusals.push(("same bytes, different spelling", body_for(json!(alternate_hex)), "conflict"));
                let changed = fixture.signed_payment(&data, amount, 1).0;
                assert_ne!(changed, raw);
                refusals.push(("different signed nonce", body_for(json!(changed)), "conflict"));
            } else {
                refusals.push(("unregistered original", body_for(json!(raw)), "funding_intent_required"));
            }
        } else {
            refusals.push(("supplied signed transfer", body_for(json!(raw)), "invalid_payment"));
        }
        for (name, body, code) in refusals {
            let body = serde_json::to_vec(&body).unwrap();
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
            let before = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", OPEN, &proof.headers, body.clone()).await;
            let expected = if code == "invalid_payment" { StatusCode::BAD_REQUEST } else { StatusCode::CONFLICT };
            assert_eq!(status, expected, "{label}/{name}: {document}");
            assert_eq!(document["error"]["code"], code, "{label}/{name}");
            fixture.consumed(&proof);
            denied(fixture.send("POST", OPEN, &proof.headers, body).await, StatusCode::UNAUTHORIZED);
            let after = ledger_snapshot(&db);
            for table in [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13] {
                assert_eq!(after[table], before[table], "{label}/{name}: changed table {table}");
            }
            assert!(rpc_calls.lock().unwrap().is_empty(), "{label}/{name}: payment guard reached RPC/engine");
            fixture.credentials_usable(&[unrelated.credential.as_ref().unwrap()]);
            if let Some(opened) = &opened {
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap()]);
            } else {
                fixture.unopened(&quote);
            }
        }

        if paid && !registered {
            // Explicit fixture admission of the unchanged original, not an implicit HTTP registration.
            let envelope = fixture.provider.state.chain.validate_payment_envelope(
                &raw, &quote.payload.consumer_wallet, &quote.payload.provider_wallet,
                &quote.payload.funding_wei, &data).unwrap();
            fixture.acknowledge_fixture(&quote, &raw, &envelope);
        }
        let body = serde_json::to_vec(&body_for(if paid { json!(raw) } else { Value::Null })).unwrap();
        let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
        let before = ledger_snapshot(&db);
        let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
        fixture.consumed(&proof);
        if paid && !replay {
            // Exact acknowledged evidence reaches verification, but unavailable RPC cannot create credit.
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{label}: {document}");
            assert_eq!(document["error"]["code"], "settlement_unavailable");
            assert!(!rpc_calls.lock().unwrap().is_empty(), "{label}: verification was never attempted");
            fixture.unopened(&quote);
            let after = ledger_snapshot(&db);
            for table in [0, 1, 2, 3, 4, 6, 7, 8, 12, 13] {
                assert_eq!(after[table], before[table], "{label}: unavailable verification changed table {table}");
            }
            assert!(before[5].is_empty());
            assert_eq!(after[5].len(), 1);
            assert_eq!(after[5][0][0], protocol::hash_hex(&hex::decode(&raw[2..]).unwrap()));
            assert_eq!(after[5][0][1], quote.payload.id);
            assert_eq!(after[5][0][3], raw);
        } else {
            assert_eq!(status, StatusCode::OK, "{label}: {document}");
            let recovered: OpenResult = serde_json::from_value(document).unwrap();
            if let Some(opened) = &opened {
                assert_eq!(serde_json::to_value(&recovered.account).unwrap(),
                    serde_json::to_value(&opened.account).unwrap());
                assert!(recovered.credential.is_none());
                let after = ledger_snapshot(&db);
                for table in [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13] {
                    assert_eq!(after[table], before[table], "{label}: replay changed table {table}");
                }
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap()]);
            } else {
                assert_eq!(recovered.account.contract_id, quote.payload.id);
                assert_eq!(recovered.account.status, "free");
                assert_eq!(recovered.account.funded_wei, "0");
                fixture.credentials_usable(&[recovered.credential.as_ref().unwrap()]);
            }
            assert!(rpc_calls.lock().unwrap().is_empty(), "{label}: free opening/replay reached RPC/engine");
        }
        fixture.credentials_usable(&[unrelated.credential.as_ref().unwrap()]);
        rpc_server.abort();
        assert!(rpc_server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn opening_omission_and_null_require_their_own_exact_body_proofs() {
    let directory = tempfile::tempdir().unwrap();
    for (case, paid) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 67_100 + case as u64);
        let rpc_calls = Arc::new(Mutex::new(Vec::new()));
        let observed = rpc_calls.clone();
        let rpc_app = Router::new().fallback(move |request: Request| {
            let observed = observed.clone();
            async move {
                observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
                StatusCode::SERVICE_UNAVAILABLE
            }
        });
        fixture._listeners[0].set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
        let rpc_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });
        let opened = if paid { fixture.accounting_account(false).await.1 } else { fixture.open_free().await };
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let omitted = serde_json::to_vec(&json!({"quote_id": opened.account.contract_id})).unwrap();
        let explicit_null = serde_json::to_vec(&json!({"quote_id": opened.account.contract_id, "raw_transaction": null})).unwrap();
        for (original, substituted) in [(&omitted, &explicit_null), (&explicit_null, &omitted)] {
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, original).await;
            let before = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", OPEN, &proof.headers, substituted.clone()).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{document}");
            assert_eq!(document["error"]["code"], "invalid_api_key");
            assert_eq!(ledger_snapshot(&db), before, "body substitution consumed authority or changed state");

            // The exact bound body can still spend that proof, even when paid business rules then refuse it.
            let (status, document) = fixture.send("POST", OPEN, &proof.headers, original.clone()).await;
            if paid {
                assert_eq!(status, StatusCode::CONFLICT, "{document}");
                assert_eq!(document["error"]["code"], "funding_intent_required");
            } else {
                assert_eq!(status, StatusCode::OK, "{document}");
                let recovered: OpenResult = serde_json::from_value(document).unwrap();
                assert_eq!(serde_json::to_value(&recovered.account).unwrap(),
                    serde_json::to_value(&opened.account).unwrap());
                assert!(recovered.credential.is_none());
            }
            fixture.consumed(&proof);
            denied(fixture.send("POST", OPEN, &proof.headers, original.clone()).await, StatusCode::UNAUTHORIZED);
            let after = ledger_snapshot(&db);
            for table in [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13] {
                assert_eq!(after[table], before[table], "exact-body control changed table {table}");
            }
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap()]);
            assert!(rpc_calls.lock().unwrap().is_empty(), "body binding or absent payment reached RPC/engine");
        }
        rpc_server.abort();
        assert!(rpc_server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn free_and_paid_opening_require_the_quoted_owner_before_any_account_creation() {
    let directory = tempfile::tempdir().unwrap();
    for (index, paid) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1900 + index as u64);
        let accounts = fixture.accounts().await;
        let before = fixture.credential_state(&accounts.target.account.contract_id);
        let quote = if paid { fixture.paid_quote().await } else { fixture.quote("0").await };
        let raw = paid.then(|| fixture.signed_payment(&protocol::opening_data(&quote.payload).unwrap(), 100, 0).0);
        let body = serde_json::to_vec(&json!({"quote_id": quote.payload.id, "raw_transaction": raw})).unwrap();
        fixture.payment_denials(OPEN, &body,
            [&accounts.read, &accounts.infer, accounts.target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]).await;
        fixture.unopened(&quote);
        let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
        let response = fixture.send("POST", OPEN, &proof.headers, body.clone()).await;
        if paid {
            assert_eq!(response.1["error"]["code"], "funding_intent_required");
            denied(response, StatusCode::CONFLICT);
            fixture.unopened(&quote);
            assert!(fixture.provider.state.ledger.funding_intent(&quote.payload, raw.as_deref().unwrap()).unwrap().is_none());
        } else {
            assert_eq!(response.0, StatusCode::OK, "{}", response.1);
            let opened: OpenResult = serde_json::from_value(response.1).unwrap();
            assert_eq!(opened.account.contract_id, quote.payload.id);
            assert_eq!(opened.account.consumer_wallet, quote.payload.consumer_wallet);
            assert_eq!(opened.account.status, "free");
            assert_eq!(opened.account.funded_wei, "0");
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap()]);
        }
        fixture.consumed(&proof);
        denied(fixture.send("POST", OPEN, &proof.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
        if paid {
            let fresh = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
            let response = fixture.send("POST", OPEN, &fresh.headers, body).await;
            assert_eq!(response.1["error"]["code"], "funding_intent_required");
            denied(response, StatusCode::CONFLICT);
            fixture.consumed(&fresh);
            fixture.unopened(&quote);
        }
        assert_eq!(fixture.credential_state(&accounts.target.account.contract_id), before);
        fixture.credentials_usable(&[&accounts.read, &accounts.infer, accounts.target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]);
    }
}

#[tokio::test]
async fn funding_intent_authority_is_required_before_new_admission_and_cached_acknowledgement() {
    let directory = tempfile::tempdir().unwrap();
    for (index, registered) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 2000 + index as u64);
        let accounts = fixture.accounts().await;
        let before = fixture.credential_state(&accounts.target.account.contract_id);
        let quote = fixture.paid_quote().await;
        let (raw, envelope) = fixture.signed_payment(&protocol::opening_data(&quote.payload).unwrap(), 100, 0);
        let ack = registered.then(|| fixture.acknowledge_fixture(&quote, &raw, &envelope));
        fixture.provider.state.accepting.store(false, Ordering::Release);
        let path = format!("/v1/contracts/{}/funding-intent", quote.payload.id);
        let body = serde_json::to_vec(&json!({"raw_transaction": raw})).unwrap();
        fixture.payment_denials(&path, &body,
            [&accounts.read, &accounts.infer, accounts.target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]).await;
        // A lost acknowledgement response is recoverable with fresh proofs and identical signed bytes.
        for _ in 0..2 {
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
            let response = fixture.send("POST", &path, &proof.headers, body.clone()).await;
            if let Some(expected) = &ack {
                assert_eq!(response.0, StatusCode::OK, "{}", response.1);
                let recovered: Signed<FundingIntent> = serde_json::from_value(response.1).unwrap();
                protocol::verify(protocol::INTENT_DOMAIN, &recovered, &quote.payload.provider_wallet).unwrap();
                assert_eq!(serde_json::to_value(recovered).unwrap(), serde_json::to_value(expected).unwrap());
            } else {
                assert_eq!(response.1["error"]["code"], "provider_draining");
                denied(response, StatusCode::SERVICE_UNAVAILABLE);
            }
            fixture.consumed(&proof);
            denied(fixture.send("POST", &path, &proof.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
            fixture.unopened(&quote);
        }
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.funding_intent(&quote.payload, &raw).unwrap()).unwrap(),
            serde_json::to_value(ack).unwrap());
        assert_eq!(fixture.credential_state(&accounts.target.account.contract_id), before);
        fixture.credentials_usable(&[&accounts.read, &accounts.infer, accounts.target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]);
    }
}

#[tokio::test]
async fn funding_intent_object_fields_are_strict_before_new_admission_and_cached_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let invalid_bodies: &[(&str, fn(&str) -> Vec<u8>)] = &[
        ("missing", |_| br#"{}"#.to_vec()),
        ("null", |_| br#"{"raw_transaction":null}"#.to_vec()),
        ("boolean", |_| br#"{"raw_transaction":true}"#.to_vec()),
        ("integer", |_| br#"{"raw_transaction":1}"#.to_vec()),
        ("fraction", |_| br#"{"raw_transaction":1.5}"#.to_vec()),
        ("array field", |raw| serde_json::to_vec(&json!({"raw_transaction": [raw]})).unwrap()),
        ("object field", |raw| serde_json::to_vec(&json!({"raw_transaction": {"hex": raw}})).unwrap()),
        ("unknown field", |raw| serde_json::to_vec(&json!({"raw_transaction": raw, "quote_id": "another-agreement"})).unwrap()),
        ("duplicate field", |raw| format!(r#"{{"raw_transaction":"{raw}","raw_transaction":"{raw}"}}"#).into_bytes()),
        ("positional request", |raw| serde_json::to_vec(&json!([raw])).unwrap()),
    ];
    for (registration, registered) in [false, true].into_iter().enumerate() {
        for (index, (name, invalid_body)) in invalid_bodies.iter().enumerate() {
            // Separate ledgers keep challenge occupancy/rate ceilings from masking schema errors.
            let mut fixture = Fixture::new(directory.path(),
                2600 + (registration * invalid_bodies.len() + index) as u64);
            let opened = fixture.open_free().await;
            let account_id = &opened.account.contract_id;
            let read = fixture.scoped_credential(account_id, "read");
            let before = fixture.credential_state(account_id);
            let quote = fixture.paid_quote().await;
            let (raw, envelope) = fixture.signed_payment(
                &protocol::opening_data(&quote.payload).unwrap(), 100, 0);
            let ack = registered.then(|| fixture.acknowledge_fixture(&quote, &raw, &envelope));
            let expected_ack = serde_json::to_value(&ack).unwrap();
            // A valid new request stops at draining; cached recovery needs no chain admission.
            // Neither branch fabricates spendability, submits a payment or opens the paid account.
            fixture.provider.state.accepting.store(false, Ordering::Release);
            let path = format!("/v1/contracts/{}/funding-intent", quote.payload.id);
            for (invalid, body) in [
                (true, invalid_body(&raw)),
                (false, serde_json::to_vec(&json!({"raw_transaction": raw})).unwrap()),
            ] {
                let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
                let response = fixture.send("POST", &path, &proof.headers, body).await;
                if invalid {
                    assert_eq!(response.0, StatusCode::BAD_REQUEST, "{name}, registered={registered}: {}", response.1);
                    assert_eq!(response.1["error"]["code"], "invalid_request", "{name}, registered={registered}");
                } else if let Some(expected) = &ack {
                    assert_eq!(response.0, StatusCode::OK, "{name}: {}", response.1);
                    let recovered: Signed<FundingIntent> = serde_json::from_value(response.1).unwrap();
                    protocol::verify(protocol::INTENT_DOMAIN, &recovered, &quote.payload.provider_wallet).unwrap();
                    assert_eq!(recovered.payload, expected.payload);
                    assert_eq!(recovered.signature, expected.signature);
                } else {
                    assert_eq!(response.1["error"]["code"], "provider_draining", "{name}");
                    denied(response, StatusCode::SERVICE_UNAVAILABLE);
                }
                // Prove preservation after both refusal and a correctly shaped fresh-proof request.
                let persisted = BillingStore::open(&fixture.provider.ledger_path, &quote.payload.network).unwrap();
                assert_eq!(serde_json::to_value(persisted.funding_intent(&quote.payload, &raw).unwrap()).unwrap(),
                    expected_ack, "{name}, registered={registered}");
                assert_eq!(serde_json::to_value(persisted.quote(&quote.payload.id).unwrap()).unwrap(),
                    serde_json::to_value(&quote).unwrap());
                assert!(persisted.account(&quote.payload.id).unwrap_err().starts_with("not_found:"));
                assert_eq!(json!({"account": persisted.account(account_id).unwrap(),
                    "credentials": persisted.credentials(account_id).unwrap()}), before);
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &read]);
                let db = Connection::open(&fixture.provider.ledger_path).unwrap();
                let monetary_rows: (u64, u64) = db.query_row(
                    "SELECT (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations)",
                    [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
                assert_eq!(monetary_rows, (0, 0), "{name}, registered={registered}");
            }
        }
    }
}

#[tokio::test]
async fn existing_free_and_paid_openings_cannot_be_reentered_using_bearers_or_another_owner() {
    let directory = tempfile::tempdir().unwrap();
    for (index, paid) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 2100 + index as u64);
        let accounts = fixture.accounts().await;
        let paid_opening = if paid { Some(fixture.paid_account().await) } else { None };
        let target = paid_opening.as_ref().map(|(opened, _)| opened).unwrap_or(&accounts.target);
        let raw = paid_opening.as_ref().map(|(_, raw)| raw.as_str());
        let id = &target.account.contract_id;
        let read = fixture.scoped_credential(id, "read");
        let infer = fixture.scoped_credential(id, "infer");
        let before = fixture.credential_state(id);
        let funding = serde_json::to_value(fixture.provider.state.ledger.funding_history(id).unwrap()).unwrap();
        let body = serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": raw})).unwrap();
        fixture.payment_denials(OPEN, &body, [&read, &infer, target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]).await;
        assert_eq!(fixture.credential_state(id), before);
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.funding_history(id).unwrap()).unwrap(), funding);
        fixture.credentials_usable(&[&read, &infer, target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]);
    }
}

#[tokio::test]
async fn authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness() {
    let directory = tempfile::tempdir().unwrap();
    for (index, paid) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 2110 + index as u64);
        // Paid setup injects verified evidence only after the verification boundary;
        // every recovery and credential mutation below uses the real authenticated router.
        let opened = if paid {
            fixture.accounting_account(false).await.1
        } else {
            // Restore signed historical terms and pre-expiry acceptance, rather
            // than sleeping or rewriting timestamps on an accepted agreement.
            let template = fixture.quote("0").await;
            let issued_at = protocol::now().unwrap() - 600;
            let expires_at = issued_at + 300;
            let mut offer = fixture.provider.offer.payload.clone();
            offer.issued_at = issued_at;
            offer.expires_at = issued_at + 120;
            let offer = protocol::sign(protocol::OFFER_DOMAIN, offer, &fixture.provider.state.private_key).unwrap();
            let historical_offer_id = protocol::offer_id(&offer.payload).unwrap();
            let ledger = &fixture.provider.state.ledger;
            let db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let quotes: Vec<_> = (0..3).map(|_| {
                let mut payload = template.payload.clone();
                payload.id = protocol::random_id();
                payload.nonce = protocol::random_id();
                payload.offer_id = historical_offer_id.clone();
                payload.issued_at = issued_at;
                payload.expires_at = expires_at;
                let quote = protocol::sign(protocol::QUOTE_DOMAIN, payload, &fixture.provider.state.private_key).unwrap();
                ledger.store_quote(&quote, issued_at).unwrap();
                quote
            }).collect();
            ledger.check_open_funding(&quotes[0].payload, None, expires_at - 1).unwrap();
            let opened = ledger.open_account(&quotes[0], None, expires_at - 1).unwrap();
            let accepted = serde_json::to_value(&opened.account).unwrap();
            for (quote, at) in quotes[1..].iter().zip([expires_at, expires_at + 1]) {
                let before = ledger_snapshot(&db);
                assert!(ledger.check_open_funding(&quote.payload, None, at).unwrap_err().starts_with("expired_quote:"));
                assert!(ledger.open_account(quote, None, at).unwrap_err().starts_with("expired_quote:"));
                fixture.unopened(quote);
                ledger.check_open_funding(&quotes[0].payload, None, at).unwrap();
                let replay = ledger.open_account(&quotes[0], None, at).unwrap();
                assert!(replay.credential.is_none());
                assert_eq!(serde_json::to_value(replay.account).unwrap(), accepted);
                assert_eq!(ledger_snapshot(&db), before);
            }
            // A current public challenge prunes unaccepted expired quotes, but
            // must retain the account's original agreement for HTTP recovery.
            let body = serde_json::to_vec(&json!({"quote_id": quotes[1].payload.id})).unwrap();
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
            let before = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(document["error"]["code"], "not_found");
            fixture.consumed(&proof);
            let after = ledger_snapshot(&db);
            for index in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13] {
                assert_eq!(after[index], before[index]);
            }
            assert!(fixture.provider.state.ledger.account(&quotes[1].payload.id).is_err());
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger.quote(&quotes[0].payload.id).unwrap()).unwrap(),
                serde_json::to_value(&quotes[0]).unwrap());
            let body = serde_json::to_vec(&json!({"quote_id": template.payload.id})).unwrap();
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
            let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
            assert_eq!(status, StatusCode::OK);
            fixture.consumed(&proof);
            let fresh: OpenResult = serde_json::from_value(document).unwrap();
            assert_eq!(fresh.account.contract_id, template.payload.id);
            fixture.credentials_usable(&[fresh.credential.as_ref().unwrap(), opened.credential.as_ref().unwrap()]);
            opened
        };
        let root = opened.credential.as_ref().unwrap();
        let id = &opened.account.contract_id;
        let read = fixture.scoped_credential(id, "read");
        let now = protocol::now().unwrap();
        let ledger = &fixture.provider.state.ledger;
        let prior = ledger.reserve(id, &root.id, "before-opening-recovery", 1, 1, now, &opened.account.model).unwrap();
        ledger.settle(&prior.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", now).unwrap();
        let held = ledger.reserve(id, &root.id, "held-through-opening-recovery", 1000, 512, now, &opened.account.model).unwrap();
        let account = serde_json::to_value(ledger.account(id).unwrap()).unwrap();
        assert_eq!(account["funded_wei"], if paid { "1000" } else { "0" });
        assert_eq!(account["fee_wei"], if paid { "5" } else { "0" });
        assert_eq!(account["spent_wei"], if paid { "1" } else { "0" });
        assert_eq!(account["reserved_wei"], if paid { "3" } else { "0" });
        assert_eq!(account["available_wei"], if paid { "991" } else { "0" });
        let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
        // Restored exhaustion is a resource prerequisite, not 600 observed HTTP
        // issuances. Logical time stays ahead without changing any signed terms.
        let minute = now / 60 + 10;
        let tx = db.transaction().unwrap();
        for (bucket, watermark) in [
            ("quote:global", "quote_committed_minute"),
            ("quote-work:global", "quote_work_minute"),
        ] {
            assert_eq!(tx.execute("UPDATE rate_limits SET window=?1,count=600 WHERE bucket=?2",
                params![minute, bucket]).unwrap(), 1);
            assert_eq!(tx.execute("UPDATE metadata SET value=?1 WHERE key=?2",
                params![minute, watermark]).unwrap(), 1);
        }
        tx.commit().unwrap();
        let budgets = || {
            let mut statement = db.prepare(
                "SELECT json_array(bucket,window,count) FROM rate_limits
                 WHERE bucket IN ('quote:global','quote-work:global') ORDER BY bucket").unwrap();
            statement.query_map([], |row| row.get::<_, String>(0)).unwrap()
                .collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        let exhausted = budgets();
        let protected = || {
            let rows = ledger_snapshot(&db);
            // Challenge issuance/consumption has its own counters. Recovery must
            // leave every account, credential, request, funding and fee row exact.
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 13].map(|index| rows[index].clone())
        };
        let before = protected();
        assert_eq!(before[4].len(), usize::from(paid));
        assert_eq!(before[7].len(), usize::from(paid));
        let (status, refusal) = fixture.post("/v1/contracts/propose", json!({
            "offer_id":fixture.provider.offer_id,
            "consumer_wallet":opened.account.consumer_wallet,
            "funding_wei":if paid { "100" } else { "0" }
        })).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(refusal["error"]["code"], "rate_limit");
        assert_eq!(budgets(), exhausted);
        assert_eq!(protected(), before);
        let raw: Option<String> = if paid {
            Some(db.query_row("SELECT raw_transaction FROM funding_intents WHERE contract=?1",
                [id], |row| row.get(0)).unwrap())
        } else { None };
        if paid {
            let missing = serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": null})).unwrap();
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &missing).await;
            let (status, document) = fixture.send("POST", OPEN, &proof.headers, missing).await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(document["error"]["code"], "funding_intent_required");
            fixture.consumed(&proof);
            assert_eq!(protected(), before);
        }
        let body = serde_json::to_vec(&json!({"quote_id": id, "raw_transaction": raw})).unwrap();
        let mut previous_nonces = Vec::new();
        for attempt in 0..3 {
            // Reopen the actual connection and router. No new admission, current
            // model readiness or RPC/engine response exists to authorize recovery.
            fixture.app = Router::new();
            let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
            state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
            state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
            state.accepting.store(false, Ordering::Release);
            state.funding_ready.store(false, Ordering::Release);
            state.qualified_models.write().unwrap().clear();
            fixture.app = contracts::router(fixture.provider.state.clone());
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
            assert!(!previous_nonces.contains(&proof.nonce));
            previous_nonces.push(proof.nonce.clone());
            let rates = rate_records(&fixture.provider.ledger_path);
            let (status, document) = tokio::time::timeout(std::time::Duration::from_secs(2),
                fixture.send("POST", OPEN, &proof.headers, body.clone())).await.unwrap();
            assert_eq!(status, StatusCode::OK);
            assert_eq!(document.get("credential"), Some(&Value::Null));
            assert!(document.get("bearer").is_none());
            assert!(!document.to_string().contains(&root.bearer));
            assert!(!document.to_string().contains(&read.bearer));
            assert_eq!(document["account"], account);
            let recovered: OpenResult = serde_json::from_value(document).unwrap();
            assert!(recovered.credential.is_none());
            fixture.consumed(&proof);
            assert_eq!(rate_records(&fixture.provider.ledger_path), rates);
            assert_eq!(protected(), before);
            assert_eq!(budgets(), exhausted);
            fixture.credentials_usable(&[root, &read]);
            assert_eq!(fixture.provider.state.ledger.request_view(id, &held.id).unwrap().status, "reserved");
            for credential in [root, &read] {
                let (status, value) = fixture.send("GET", &format!("/v1/contracts/{id}"),
                    &bearer(credential), Vec::new()).await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(value, account);
            }
            denied(fixture.send("POST", OPEN, &proof.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
            assert_eq!(protected(), before, "retry {attempt} must not mutate accepted state");
        }
        let options = credential_options();
        let mut explicit = Vec::new();
        for mutation in [CredentialMutation::Issue, CredentialMutation::Rotate] {
            let (method, path, request) = mutation.request(id, &root.id, &options);
            let proof = fixture.proof(OWNER_KEY, method, &path, &request).await;
            let (status, value) = fixture.send(method, &path, &proof.headers, request).await;
            assert_eq!(status, StatusCode::OK);
            fixture.consumed(&proof);
            fixture.check_mutation(mutation, root, value.clone());
            explicit.push(serde_json::from_value::<Credential>(value).unwrap());
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger.account(id).unwrap()).unwrap(), account);
            let after = protected();
            for index in 0..before.len() {
                if index != 1 { assert_eq!(after[index], before[index]); }
            }
        }
        // Reopening after explicit rotation must neither resurrect the old root
        // nor silently rotate the replacement or separately issued application key.
        let after_mutations = protected();
        let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
        assert!(!previous_nonces.contains(&proof.nonce));
        let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(document.get("credential"), Some(&Value::Null));
        assert_eq!(document["account"], account);
        assert!(!document.to_string().contains(&root.bearer));
        for credential in &explicit { assert!(!document.to_string().contains(&credential.bearer)); }
        fixture.consumed(&proof);
        assert_eq!(protected(), after_mutations);
        fixture.credentials_usable(&[&explicit[0], &explicit[1], &read]);
        assert!(fixture.provider.state.ledger.authenticate(&root.bearer, protocol::now().unwrap()).is_err());
        // Even the held request's now-revoked credential retains settlement
        // authority; exhausted quoting cannot prevent release and accounting.
        let settled = fixture.provider.state.ledger.settle(&held.id,
            &TokenUsage { prompt_tokens: 1000, completion_tokens: 512 },
            "completed", protocol::now().unwrap()).unwrap();
        assert_eq!(settled.cost_wei, if paid { "3" } else { "0" });
        let after = fixture.provider.state.ledger.account(id).unwrap();
        assert_eq!(after.reserved_wei, "0");
        assert_eq!(after.spent_wei, if paid { "4" } else { "0" });
        assert_eq!(after.available_wei, if paid { "991" } else { "0" });
        let (status, recovered) = fixture.send("GET",
            &format!("/v1/contracts/{id}/requests/{}", held.id), &bearer(&read), Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(recovered, serde_json::to_value(settled).unwrap());
        assert_eq!(budgets(), exhausted);
    }
}

#[tokio::test]
async fn topup_request_schema_refuses_invalid_shapes_after_consuming_owner_authority() {
    let directory = tempfile::tempdir().unwrap();
    let invalid_bodies: &[(&str, fn(&str) -> Vec<u8>)] = &[
        ("complete positional", |raw| serde_json::to_vec(&json!([raw])).unwrap()),
        ("empty positional", |_| b"[]".to_vec()),
        ("invalid JSON", |_| b"{".to_vec()),
        ("null root", |_| b"null".to_vec()),
        ("boolean root", |_| b"true".to_vec()),
        ("numeric root", |_| b"1".to_vec()),
        ("string root", |raw| serde_json::to_vec(raw).unwrap()),
        ("missing payment", |_| b"{}".to_vec()),
        ("null payment", |_| br#"{"raw_transaction":null}"#.to_vec()),
        ("boolean payment", |_| br#"{"raw_transaction":true}"#.to_vec()),
        ("integer payment", |_| br#"{"raw_transaction":1}"#.to_vec()),
        ("fractional payment", |_| br#"{"raw_transaction":1.5}"#.to_vec()),
        ("array payment", |raw| serde_json::to_vec(&json!({"raw_transaction":[raw]})).unwrap()),
        ("object payment", |raw| serde_json::to_vec(&json!({"raw_transaction":{"hex":raw}})).unwrap()),
        ("unknown member", |raw| serde_json::to_vec(&json!({"raw_transaction":raw,"amount_wei":"1000"})).unwrap()),
        ("duplicate payment", |raw| {
            let raw = serde_json::to_string(raw).unwrap();
            format!(r#"{{"raw_transaction":{raw},"raw_transaction":{raw}}}"#).into_bytes()
        }),
    ];
    for (state, paid) in [false, true].into_iter().enumerate() {
        for (index, (name, invalid_body)) in invalid_bodies.iter().enumerate() {
            // Separate fixtures keep proof/rate ceilings from masking decoding.
            let mut fixture = Fixture::new(directory.path(), 69_000 + (state * invalid_bodies.len() + index) as u64);
            let rpc_calls = Arc::new(Mutex::new(Vec::new()));
            let observed = rpc_calls.clone();
            let rpc_app = Router::new().fallback(move |request: Request| {
                let observed = observed.clone();
                async move {
                    observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
                    StatusCode::SERVICE_UNAVAILABLE
                }
            });
            fixture._listeners[0].set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
            let rpc_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });
            let unrelated = fixture.open_free().await;
            // Accepted paid credit is a post-verification prerequisite, not chain evidence.
            let opened = if paid { fixture.paid_account().await.0 } else { fixture.open_free().await };
            let id = &opened.account.contract_id;
            let path = format!("/v1/contracts/{id}/topup");
            let read_path = format!("/v1/contracts/{}", unrelated.account.contract_id);
            let retained = fixture.proof(OWNER_KEY, "GET", &read_path, &[]).await;
            let (raw, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
            let body = invalid_body(&raw);
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
            let db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let mut expected = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", &path, &proof.headers, body.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}, paid={paid}: {document}");
            assert_eq!(document["error"]["code"], "invalid_request", "{name}, paid={paid}");
            fixture.consumed(&proof);
            expected[11].retain(|row| row[0].as_str() != Some(proof.nonce.as_str()));
            assert_eq!(ledger_snapshot(&db), expected, "{name}: only this proof may be consumed");
            denied(fixture.send("POST", &path, &proof.headers, body).await, StatusCode::UNAUTHORIZED);
            assert_eq!(ledger_snapshot(&db), expected);
            assert!(rpc_calls.lock().unwrap().is_empty());

            // The real writer's object still reaches eligibility/verification.
            let valid = protocol::TopupRequest { raw_transaction: raw };
            let body = serde_json::to_vec(&valid).unwrap();
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
            let mut expected = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", &path, &proof.headers, body).await;
            assert_eq!(status, if paid { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::CONFLICT }, "{document}");
            assert_eq!(document["error"]["code"], if paid { "settlement_unavailable" } else { "account_frozen" });
            fixture.consumed(&proof);
            expected[11].retain(|row| row[0].as_str() != Some(proof.nonce.as_str()));
            let after = ledger_snapshot(&db);
            for table in 0..expected.len() {
                if paid && table == 5 { continue; } // The genuine verification refusal records its original.
                assert_eq!(after[table], expected[table], "{name}: object control changed table {table}");
            }
            if paid {
                assert!(!rpc_calls.lock().unwrap().is_empty(), "valid paid evidence must reach verification");
                let attempt: (String, String, String, String) = db.query_row(
                    "SELECT tx_hash,contract,raw_transaction,json_extract(record,'$.status') FROM funding_attempts",
                    [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
                assert_eq!(attempt, (envelope.tx_hash, id.clone(), valid.raw_transaction, "unavailable".into()));
            } else {
                assert!(rpc_calls.lock().unwrap().is_empty());
            }
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), unrelated.credential.as_ref().unwrap()]);
            let (status, account) = fixture.send("GET", &read_path, &retained.headers, Vec::new()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(account, serde_json::to_value(&unrelated.account).unwrap());
            fixture.consumed(&retained);
            rpc_server.abort();
            assert!(rpc_server.await.unwrap_err().is_cancelled());
        }
    }
}

#[tokio::test]
async fn topup_authority_precedes_schema_and_binds_exact_body_and_target() {
    let directory = tempfile::tempdir().unwrap();
    for (index, paid) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 70_000 + index as u64);
        let rpc_calls = Arc::new(Mutex::new(Vec::new()));
        let observed = rpc_calls.clone();
        let rpc_app = Router::new().fallback(move |request: Request| {
            let observed = observed.clone();
            async move {
                observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
                StatusCode::SERVICE_UNAVAILABLE
            }
        });
        fixture._listeners[0].set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
        let rpc_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });
        let unrelated = fixture.open_free().await;
        let opened = if paid { fixture.paid_account().await.0 } else { fixture.open_free().await };
        let id = &opened.account.contract_id;
        let read = fixture.scoped_credential(id, "read");
        let path = format!("/v1/contracts/{id}/topup");
        let other_path = format!("/v1/contracts/{}/topup", unrelated.account.contract_id);
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let malformed = b"{}".to_vec();
        for headers in [HeaderMap::new(), bearer(opened.credential.as_ref().unwrap()), bearer(&read)] {
            let before = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", &path, &headers, malformed.clone()).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(document["error"]["code"], "invalid_api_key");
            assert_eq!(ledger_snapshot(&db), before);
        }
        let absent_id = format!("0x{:064x}", 1);
        assert!(fixture.provider.state.ledger.account(&absent_id).is_err());
        let absent_path = format!("/v1/contracts/{absent_id}/topup");
        for (key, target, status, code) in [
            (OTHER_KEY, path.as_str(), StatusCode::FORBIDDEN, "forbidden"),
            (OWNER_KEY, absent_path.as_str(), StatusCode::NOT_FOUND, "not_found"),
        ] {
            let proof = fixture.proof(key, "POST", target, &malformed).await;
            let mut expected = ledger_snapshot(&db);
            let (actual, document) = fixture.send("POST", target, &proof.headers, malformed.clone()).await;
            assert_eq!(actual, status);
            assert_eq!(document["error"]["code"], code);
            fixture.consumed(&proof);
            expected[11].retain(|row| row[0].as_str() != Some(proof.nonce.as_str()));
            assert_eq!(ledger_snapshot(&db), expected);
        }
        let invalid_path = "/v1/contracts/0x00/topup";
        let proof = fixture.proof(OWNER_KEY, "POST", invalid_path, &malformed).await;
        let before = ledger_snapshot(&db);
        let (status, document) = fixture.send("POST", invalid_path, &proof.headers, malformed).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(document["error"]["code"], "invalid_contract_id");
        assert_eq!(ledger_snapshot(&db), before, "route validation precedes proof consumption");
        assert!(rpc_calls.lock().unwrap().is_empty());

        let (raw, _) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
        let compact = serde_json::to_vec(&protocol::TopupRequest { raw_transaction: raw }).unwrap();
        let spaced = format!(" {}\n", std::str::from_utf8(&compact).unwrap()).into_bytes();
        let query_path = format!("{path}?cursor=1");
        for reverse in [false, true] {
            let (original, substituted) = if reverse { (&spaced, &compact) } else { (&compact, &spaced) };
            let proof = fixture.proof(OWNER_KEY, "POST", &path, original).await;
            let mut expected = ledger_snapshot(&db);
            let rpc_start = rpc_calls.lock().unwrap().len();
            for (target, body) in [(&path, substituted), (&other_path, original), (&query_path, original)] {
                let (status, document) = fixture.send("POST", target, &proof.headers, body.clone()).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED);
                assert_eq!(document["error"]["code"], "invalid_api_key");
                assert_eq!(ledger_snapshot(&db), expected, "binding refusal must retain the exact original authority");
                assert_eq!(rpc_calls.lock().unwrap().len(), rpc_start);
            }
            let (status, document) = fixture.send("POST", &path, &proof.headers, original.clone()).await;
            assert_eq!(status, if paid { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::CONFLICT }, "{document}");
            assert_eq!(document["error"]["code"], if paid { "settlement_unavailable" } else { "account_frozen" });
            fixture.consumed(&proof);
            expected[11].retain(|row| row[0].as_str() != Some(proof.nonce.as_str()));
            let after = ledger_snapshot(&db);
            for table in 0..expected.len() {
                if paid && table == 5 { continue; }
                assert_eq!(after[table], expected[table]);
            }
            let rpc_count = rpc_calls.lock().unwrap().len();
            if paid { assert!(rpc_count > rpc_start); }
            else { assert_eq!(rpc_count, rpc_start); }
            denied(fixture.send("POST", &path, &proof.headers, original.clone()).await, StatusCode::UNAUTHORIZED);
            assert_eq!(rpc_calls.lock().unwrap().len(), rpc_count);
            assert_eq!(ledger_snapshot(&db), after);
        }
        fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &read, unrelated.credential.as_ref().unwrap()]);
        rpc_server.abort();
        assert!(rpc_server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn topups_require_account_owner_and_business_refusals_consume_only_authority() {
    let directory = tempfile::tempdir().unwrap();
    for (index, paid) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 2200 + index as u64);
        let accounts = fixture.accounts().await;
        let paid_opening = if paid { Some(fixture.paid_account().await) } else { None };
        let target = paid_opening.as_ref().map(|(opened, _)| opened).unwrap_or(&accounts.target);
        let id = &target.account.contract_id;
        let read = fixture.scoped_credential(id, "read");
        let infer = fixture.scoped_credential(id, "infer");
        let before = fixture.credential_state(id);
        let other_before = fixture.credential_state(&accounts.other.account.contract_id);
        let history = serde_json::to_value(fixture.provider.state.ledger.funding_history(id).unwrap()).unwrap();
        let path = format!("/v1/contracts/{id}/topup");
        let (raw, _) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 100, 1);
        let body = serde_json::to_vec(&json!({"raw_transaction": raw})).unwrap();
        fixture.payment_denials(&path, &body, [&read, &infer, target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]).await;
        assert_eq!(fixture.credential_state(id), before);
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.funding_history(id).unwrap()).unwrap(), history);

        // Correct authority cannot make a different account's signed payment commitment acceptable.
        // The mismatch is rejected before any RPC; free accounts reject top-ups before verification.
        let (wrong_raw, rejected) = fixture.signed_payment(
            &protocol::topup_data(&accounts.other.account.contract_id).unwrap(), 100, 1);
        let wrong_body = serde_json::to_vec(&json!({"raw_transaction": wrong_raw})).unwrap();
        for _ in 0..2 {
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &wrong_body).await;
            let response = fixture.send("POST", &path, &proof.headers, wrong_body.clone()).await;
            assert_eq!(response.1["error"]["code"], if paid { "funding_rejected" } else { "account_frozen" });
            denied(response, if paid { StatusCode::BAD_REQUEST } else { StatusCode::CONFLICT });
            fixture.consumed(&proof);
            denied(fixture.send("POST", &path, &proof.headers, wrong_body.clone()).await, StatusCode::UNAUTHORIZED);
            assert_eq!(fixture.credential_state(id), before);
        }
        let after = fixture.provider.state.ledger.funding_history(id).unwrap();
        if paid {
            let attempt = after.iter().find(|entry| entry.tx_hash == rejected.tx_hash).unwrap();
            assert_eq!(attempt.contract_id, *id);
            assert_eq!(attempt.status, "rejected");
            assert!(attempt.value_wei.is_none() && attempt.fee_wei.is_none());
            assert_eq!(serde_json::to_value(after.iter().filter(|entry| entry.tx_hash != rejected.tx_hash)
                .collect::<Vec<_>>()).unwrap(), history);
        } else {
            assert_eq!(serde_json::to_value(after).unwrap(), history);
        }
        assert_eq!(fixture.credential_state(&accounts.other.account.contract_id), other_before);
        fixture.credentials_usable(&[&read, &infer, target.credential.as_ref().unwrap(), accounts.other.credential.as_ref().unwrap()]);
    }
}

#[tokio::test]
async fn topup_inactive_accounts_refuse_before_rpc_and_again_at_credit() {
    use chiral_network::chain_client::PaymentState;

    let directory = tempfile::tempdir().unwrap();
    for (case, status) in ["free", "frozen", "settlement_pending"].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 71_000 + case as u64);
        let unrelated = fixture.open_free().await;
        let opened = if status == "free" {
            fixture.open_free().await
        } else {
            fixture.paid_account().await.0
        };
        let id = &opened.account.contract_id;
        let quote = fixture.provider.state.ledger.quote(id).unwrap();
        // Supplied recheck observations establish the account-state prerequisite,
        // not evidence that this test observed a real chain regression.
        let original = if status == "free" { None } else {
            let original = fixture.provider.state.ledger.funding_to_recheck().unwrap().remove(0);
            let observation = if status == "frozen" { PaymentState::Orphaned } else { PaymentState::Pending };
            fixture.provider.state.ledger.apply_funding_recheck(
                &original.tx_hash, &Ok(observation), protocol::now().unwrap()).unwrap();
            Some(original)
        };
        assert_eq!(fixture.provider.state.ledger.account(id).unwrap().status, status);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let app = Router::new().fallback(move |request: Request| {
            observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
            async { StatusCode::SERVICE_UNAVAILABLE }
        });
        let listener = fixture._listeners[0].try_clone().unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let (raw, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
        let path = format!("/v1/contracts/{id}/topup");
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        for supplied in ["", raw.as_str()] {
            let body = serde_json::to_vec(&protocol::TopupRequest { raw_transaction: supplied.into() }).unwrap();
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
            let mut expected = ledger_snapshot(&db);
            let (response, document) = fixture.send("POST", &path, &proof.headers, body.clone()).await;
            assert_eq!(response, StatusCode::CONFLICT, "{document}");
            assert_eq!(document["error"]["code"], "account_frozen");
            fixture.consumed(&proof);
            expected[11].retain(|row| row[0].as_str() != Some(proof.nonce.as_str()));
            assert_eq!(ledger_snapshot(&db), expected);
            denied(fixture.send("POST", &path, &proof.headers, body).await, StatusCode::UNAUTHORIZED);
            assert_eq!(ledger_snapshot(&db), expected);
            assert!(calls.lock().unwrap().is_empty());
        }

        // Exercise the credit transaction independently of the HTTP precheck,
        // with explicit post-verification input rather than claimed RPC evidence.
        let payment = VerifiedPayment {
            network: quote.payload.network.clone(), tx_hash: envelope.tx_hash,
            from: envelope.from, to: envelope.to, value_wei: envelope.value_wei, data: envelope.data,
            block_hash: format!("0x{:064x}", 8), block_number: 8, raw_transaction: raw,
        };
        let reopened = BillingStore::open(&fixture.provider.ledger_path, &quote.payload.network).unwrap();
        let before = ledger_snapshot(&db);
        assert!(reopened.credit_topup(id, &payment, protocol::now().unwrap()).unwrap_err().starts_with("forbidden:"));
        assert_eq!(ledger_snapshot(&db), before);
        fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), unrelated.credential.as_ref().unwrap()]);
        let (response, account) = fixture.send("GET", &format!("/v1/contracts/{id}"),
            &bearer(opened.credential.as_ref().unwrap()), Vec::new()).await;
        assert_eq!(response, StatusCode::OK, "{account}");
        assert_eq!(account, serde_json::to_value(reopened.account(id).unwrap()).unwrap());
        assert_eq!(ledger_snapshot(&db), before);

        if let Some(original) = original {
            // Explicit fixture reconciliation is a positive ledger control, not
            // an automatic repair policy or a live canonical-restoration claim.
            reopened.apply_funding_recheck(&original.tx_hash,
                &Ok(PaymentState::Accepted(original.clone())), protocol::now().unwrap()).unwrap();
            let mut expected = serde_json::to_value(&opened.account).unwrap();
            expected["funded_wei"] = json!("1100");
            expected["fee_wei"] = json!("5");
            expected["available_wei"] = json!("1095");
            assert_eq!(serde_json::to_value(reopened.credit_topup(id, &payment, protocol::now().unwrap()).unwrap()).unwrap(), expected);
            let credited = ledger_snapshot(&db);
            assert_eq!(serde_json::to_value(reopened.credit_topup(id, &payment, protocol::now().unwrap()).unwrap()).unwrap(), expected);
            assert_eq!(ledger_snapshot(&db), credited);
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), unrelated.credential.as_ref().unwrap()]);
        }
        assert!(calls.lock().unwrap().is_empty());
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn topup_invalid_payment_and_zero_value_preserve_accepted_money() {
    let directory = tempfile::tempdir().unwrap();
    for (case, label) in ["empty", "empty hex", "malformed hex", "malformed envelope",
        "opening commitment", "other account", "zero value"].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 72_000 + case as u64);
        let unrelated = fixture.open_free().await;
        // Paid opening is a post-verification ledger prerequisite.
        let (opened, _) = fixture.paid_account().await;
        let id = &opened.account.contract_id;
        let quote = fixture.provider.state.ledger.quote(id).unwrap();
        let (zero, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 0, 1);
        let raw = match label {
            "empty" => String::new(),
            "empty hex" => "0x".into(),
            "malformed hex" => "0xnot-hex".into(),
            "malformed envelope" => "0x00".into(),
            "opening commitment" => fixture.signed_payment(&protocol::opening_data(&quote.payload).unwrap(), 1000, 1).0,
            "other account" => fixture.signed_payment(&protocol::topup_data(&unrelated.account.contract_id).unwrap(), 1000, 1).0,
            "zero value" => zero,
            _ => unreachable!(),
        };
        let recorded = matches!(label, "malformed envelope" | "opening commitment" | "other account");
        // Only zero value may reach this RPC. Complete healthy-safe observations
        // make the monetary guard, rather than absent chain evidence, decisive.
        let rpc = Arc::new(TopupRecoveryRpc {
            quote: quote.payload.clone(), tx_hash: envelope.tx_hash,
            phase: AtomicU64::new(2), timestamp: AtomicU64::new(protocol::now().unwrap()),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new().route("/", post(topup_recovery_rpc)).with_state(rpc.clone());
        let listener = fixture._listeners[0].try_clone().unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let path = format!("/v1/contracts/{id}/topup");
        let body = serde_json::to_vec(&protocol::TopupRequest { raw_transaction: raw.clone() }).unwrap();
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let mut first_attempt = None;
        for _ in 0..2 {
            rpc.timestamp.store(protocol::now().unwrap(), Ordering::Release);
            let start = rpc.requests.lock().unwrap().len();
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
            let mut expected = ledger_snapshot(&db);
            let (status, document) = fixture.send("POST", &path, &proof.headers, body.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {document}");
            assert_eq!(document["error"]["code"], if recorded { "funding_rejected" } else { "invalid_payment" });
            fixture.consumed(&proof);
            expected[11].retain(|row| row[0].as_str() != Some(proof.nonce.as_str()));
            let after = ledger_snapshot(&db);
            if recorded {
                assert_eq!(after[5].len(), 1);
                let attempt = &after[5][0];
                let hash = protocol::hash_hex(&hex::decode(raw.strip_prefix("0x").unwrap()).unwrap());
                assert_eq!(attempt[0], hash);
                assert_eq!(attempt[1], *id);
                assert_eq!(attempt[3], raw);
                assert_eq!(attempt[2], json!({"tx_hash":hash,"contract_id":id,"status":"rejected",
                    "value_wei":null,"fee_wei":null,"created_at":attempt[2]["created_at"]}));
                assert!(attempt[2]["created_at"].as_u64().is_some());
                if let Some(first) = &first_attempt { assert_eq!(&attempt[2], first); }
                else { first_attempt = Some(attempt[2].clone()); }
                expected[5] = after[5].clone();
            }
            assert_eq!(after, expected, "{label}");
            {
                let calls = rpc.requests.lock().unwrap();
                let calls = &calls[start..];
                if label == "zero value" {
                    for method in ["eth_getTransactionReceipt", "eth_syncing"] {
                        assert_eq!(calls.iter().filter(|call| call["method"] == method).count(), 2);
                    }
                    for tag in ["safe", "latest", "0x8"] {
                        assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBlockByNumber"
                            && call["params"][0] == tag).count(), 2);
                    }
                } else { assert!(calls.is_empty(), "{label} reached RPC"); }
            }
            let calls = rpc.requests.lock().unwrap().len();
            denied(fixture.send("POST", &path, &proof.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
            assert_eq!(rpc.requests.lock().unwrap().len(), calls);
            assert_eq!(ledger_snapshot(&db), after);
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), unrelated.credential.as_ref().unwrap()]);
        }
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn global_pending_challenge_ceiling_returns_429_without_eviction_or_budget_debit() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 2300);
    fixture.seed_challenge_snapshot(10_000);
    let before = fixture.challenge_records();
    let rates = rate_records(&fixture.provider.ledger_path);
    let owner = wallet::address_from_private_key(OWNER_KEY).unwrap();
    let request = json!({"wallet": owner, "method": "GET", "path": OPEN, "body_hash": protocol::hash_hex(b"")});
    let response = fixture.post("/v1/auth/challenge", request.clone()).await;
    assert_eq!(response.1["error"]["code"], "rate_limit");
    denied(response, StatusCode::TOO_MANY_REQUESTS);
    let after = fixture.challenge_records();
    assert_eq!(after.len(), 10_000);
    for (expected, actual) in before.iter().zip(&after) { assert_eq!(actual, expected); }
    assert_eq!(rate_records(&fixture.provider.ledger_path), rates);

    // Storage consumption follows authentication (covered in 2.3) and releases exactly one slot.
    let consumed: AuthPayload = serde_json::from_str(&before[0].3).unwrap();
    fixture.provider.state.ledger.consume_challenge(&consumed.nonce, &consumed, protocol::now().unwrap()).unwrap();
    let (status, value) = fixture.post("/v1/auth/challenge", request).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let admitted: Challenge = serde_json::from_value(value).unwrap();
    let saved = fixture.provider.state.ledger.challenge(&admitted.nonce).unwrap();
    assert_eq!(saved.wallet, owner);
    assert_eq!(saved.expires_at, admitted.expires_at);
    assert!(fixture.provider.state.ledger.challenge(&consumed.nonce).is_err());
    let remaining = fixture.challenge_records();
    assert_eq!(remaining.len(), 10_000);
    for (expected, actual) in before.iter().skip(1).zip(remaining.iter().filter(|row| row.0 != admitted.nonce)) {
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn challenge_slots_are_separate_from_proposals_and_cleanup_preserves_accepted_state() {
    let directory = tempfile::tempdir().unwrap();
    for (index, challenges_first) in [false, true].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 2310 + index as u64);
        let accounts = fixture.accounts().await;
        let quote = fixture.paid_quote().await;
        // Freeze the last native proposal's issuance time for all subsequent storage operations.
        let now = quote.payload.issued_at;
        let ledger = &fixture.provider.state.ledger;
        let (raw, envelope) = fixture.signed_payment(&protocol::opening_data(&quote.payload).unwrap(), 100, 0);
        let ack = ledger.register_funding_intent(&quote, &quote.payload.consumer_wallet,
            &raw, &envelope.tx_hash, envelope.nonce, &fixture.provider.state.private_key,
            || Ok(now), true, &quote.payload.model).unwrap();
        let account_before = fixture.credential_state(&accounts.target.account.contract_id);
        let other_before = fixture.credential_state(&accounts.other.account.contract_id);
        let proposal = |sequence: u64, issued_at: u64| {
            let mut payload = quote.payload.clone();
            payload.id = format!("0x{:064x}", 10_000 + sequence);
            payload.nonce = format!("0x{:064x}", 20_000 + sequence);
            payload.issued_at = issued_at;
            payload.expires_at = issued_at + 300;
            protocol::sign(protocol::QUOTE_DOMAIN, payload, &fixture.provider.state.private_key).unwrap()
        };
        let proposals: Vec<_> = (1..=13).map(|sequence| proposal(sequence, now)).collect();
        let challenges: Vec<_> = (1..=8).map(|sequence| fixture.pending_challenge(
            &quote.payload.consumer_wallet, sequence, now + if sequence == 1 { 119 } else { 120 })).collect();
        for challenge_phase in [challenges_first, !challenges_first] {
            if challenge_phase {
                for payload in &challenges { ledger.create_challenge(payload, now).unwrap(); }
            } else {
                for proposed in &proposals { ledger.store_quote(proposed, now).unwrap(); }
            }
        }
        // Crossing both former wallet quote ceilings does not consume challenge slots.
        let replacement = fixture.pending_challenge(&quote.payload.consumer_wallet, 9, now + 120);
        let before = fixture.challenge_records();
        let rates = rate_records(&fixture.provider.ledger_path);
        assert!(ledger.create_challenge(&replacement, now).unwrap_err().starts_with("rate_limit:"));
        assert_eq!(fixture.challenge_records(), before);
        assert_eq!(rate_records(&fixture.provider.ledger_path), rates);
        for proposed in &proposals {
            assert_eq!(serde_json::to_value(ledger.quote(&proposed.payload.id).unwrap()).unwrap(),
                serde_json::to_value(proposed).unwrap());
        }

        ledger.consume_challenge(&challenges[1].nonce, &challenges[1], now).unwrap();
        ledger.create_challenge(&replacement, now).unwrap();
        let after_expiry = fixture.pending_challenge(&quote.payload.consumer_wallet, 10, now + 239);
        ledger.create_challenge(&after_expiry, now + 119).unwrap();
        assert!(ledger.challenge(&challenges[0].nonce).is_err());
        assert!(ledger.challenge(&challenges[1].nonce).is_err());
        for expected in challenges.iter().skip(2).chain([&replacement, &after_expiry]) {
            assert_eq!(serde_json::to_value(ledger.challenge(&expected.nonce).unwrap()).unwrap(),
                serde_json::to_value(expected).unwrap());
        }
        assert_eq!(fixture.challenge_records().len(), 8);

        // Expiring the acknowledged admission releases resources, not its durable agreement.
        let cleanup_at = now + 300;
        let fresh = fixture.pending_challenge(&wallet::address_from_private_key(OTHER_KEY).unwrap(), 11, cleanup_at + 120);
        ledger.create_challenge(&fresh, cleanup_at).unwrap();
        assert_eq!(serde_json::to_value(ledger.funding_intent(&quote.payload, &raw).unwrap().unwrap()).unwrap(),
            serde_json::to_value(&ack).unwrap());
        fixture.unopened(&quote);
        let fresh_quote = proposal(14, cleanup_at);
        ledger.store_quote(&fresh_quote, cleanup_at).unwrap();
        assert_eq!(ledger.quote(&fresh_quote.payload.id).unwrap().payload, fresh_quote.payload);
        assert_eq!(fixture.credential_state(&accounts.target.account.contract_id), account_before);
        assert_eq!(fixture.credential_state(&accounts.other.account.contract_id), other_before);
        for opened in [&accounts.target, &accounts.other] {
            assert_eq!(ledger.authenticate(&opened.credential.as_ref().unwrap().bearer, cleanup_at).unwrap().account.contract_id,
                opened.account.contract_id);
        }
    }
}

#[test]
fn competing_connections_share_the_last_wallet_challenge_slot_without_budget_leakage() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 2400);
    let owner = wallet::address_from_private_key(OWNER_KEY).unwrap();
    let ledger = &fixture.provider.state.ledger;
    ledger.bind_provider(&fixture.provider.state.wallet).unwrap();
    let initial: Vec<_> = (1..=7).map(|sequence| fixture.pending_challenge(&owner, sequence, 240)).collect();
    for payload in &initial { ledger.create_challenge(payload, 120).unwrap(); }
    let candidates = [fixture.pending_challenge(&owner, 8, 240), fixture.pending_challenge(&owner, 9, 240)];
    let winner = fixture.race_challenges([&candidates[0], &candidates[1]], 120);
    for expected in &initial {
        assert_eq!(serde_json::to_value(ledger.challenge(&expected.nonce).unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap());
    }
    ledger.consume_challenge(&initial[0].nonce, &initial[0], 120).unwrap();
    ledger.create_challenge(&candidates[1 - winner], 120).unwrap();
    assert_eq!(serde_json::to_value(ledger.challenge(&candidates[1 - winner].nonce).unwrap()).unwrap(),
        serde_json::to_value(&candidates[1 - winner]).unwrap());
    assert_eq!(fixture.challenge_records().len(), 8);
}

#[test]
fn competing_wallets_share_the_last_global_challenge_slot_without_budget_leakage() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 2401);
    fixture.seed_challenge_snapshot(9_999);
    let before = fixture.challenge_records();
    let candidates = [
        fixture.pending_challenge(&wallet::address_from_private_key(OWNER_KEY).unwrap(), 1, 240),
        fixture.pending_challenge(&wallet::address_from_private_key(OTHER_KEY).unwrap(), 2, 240),
    ];
    let winner = fixture.race_challenges([&candidates[0], &candidates[1]], 120);
    let after = fixture.challenge_records();
    assert_eq!(after.len(), 10_000);
    for (expected, actual) in before.iter().zip(after.iter().filter(|row| row.0 != candidates[winner].nonce)) {
        assert_eq!(actual, expected);
    }
    let consumed: AuthPayload = serde_json::from_str(&before[0].3).unwrap();
    fixture.provider.state.ledger.consume_challenge(&consumed.nonce, &consumed, 120).unwrap();
    fixture.provider.state.ledger.create_challenge(&candidates[1 - winner], 120).unwrap();
    assert_eq!(serde_json::to_value(fixture.provider.state.ledger.challenge(&candidates[1 - winner].nonce).unwrap()).unwrap(),
        serde_json::to_value(&candidates[1 - winner]).unwrap());
    assert_eq!(fixture.challenge_records().len(), 10_000);
}

#[test]
fn nonce_collision_preserves_the_original_challenge_and_issuance_budget() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 2500);
    let ledger = &fixture.provider.state.ledger;
    ledger.bind_provider(&fixture.provider.state.wallet).unwrap();
    let original = fixture.pending_challenge(&wallet::address_from_private_key(OWNER_KEY).unwrap(), 1, 240);
    ledger.create_challenge(&original, 120).unwrap();
    let before = fixture.challenge_records();
    let rates = rate_records(&fixture.provider.ledger_path);
    let mut collision = fixture.pending_challenge(&wallet::address_from_private_key(OTHER_KEY).unwrap(), 1, 239);
    collision.method = "DELETE".into();
    collision.body_hash = protocol::hash_hex(b"{}");
    assert!(ledger.create_challenge(&collision, 120).is_err());
    assert_eq!(fixture.challenge_records(), before);
    assert_eq!(rate_records(&fixture.provider.ledger_path), rates);
    let mut headers = HeaderMap::new();
    headers.insert("x-chiral-owner", original.wallet.parse().unwrap());
    headers.insert("x-chiral-nonce", original.nonce.parse().unwrap());
    headers.insert("x-chiral-signature",
        wallet::sign_message(OWNER_KEY, &auth::challenge_payload(&original).unwrap()).unwrap().parse().unwrap());
    assert_eq!(auth::authenticate_owner(ledger, &headers, &original.network, &original.provider_wallet,
        "GET", OPEN, b"", 120).unwrap(), original.wallet);
    collision.nonce = format!("0x{:064x}", 2);
    ledger.create_challenge(&collision, 120).unwrap();
    assert_eq!(serde_json::to_value(ledger.challenge(&collision.nonce).unwrap()).unwrap(),
        serde_json::to_value(collision).unwrap());
}

#[test]
fn failed_challenge_insertion_rolls_back_expiry_cleanup_and_budget_changes() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(directory.path(), 2501);
    let ledger = &fixture.provider.state.ledger;
    ledger.bind_provider(&fixture.provider.state.wallet).unwrap();
    let owner = wallet::address_from_private_key(OWNER_KEY).unwrap();
    let expiring = fixture.pending_challenge(&owner, 1, 121);
    let survivor = fixture.pending_challenge(&owner, 2, 240);
    for payload in [&expiring, &survivor] { ledger.create_challenge(payload, 120).unwrap(); }
    let next = fixture.pending_challenge(&owner, 3, 241);
    let before = fixture.challenge_records();
    let rates = rate_records(&fixture.provider.ledger_path);
    let db = Connection::open(&fixture.provider.ledger_path).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER refuse_challenge AFTER INSERT ON challenges WHEN NEW.nonce='{}'
         BEGIN SELECT RAISE(ABORT,'injected challenge insertion failure'); END;", next.nonce)).unwrap();
    let error = ledger.create_challenge(&next, 121).unwrap_err();
    assert!(error.contains("injected challenge insertion failure"), "{error}");
    assert_eq!(fixture.challenge_records(), before);
    assert_eq!(rate_records(&fixture.provider.ledger_path), rates);
    let reopened = BillingStore::open(&fixture.provider.ledger_path, &next.network).unwrap();
    assert!(reopened.challenge(&next.nonce).is_err());
    assert_eq!(serde_json::to_value(reopened.challenge(&expiring.nonce).unwrap()).unwrap(),
        serde_json::to_value(&expiring).unwrap());
    assert!(reopened.consume_challenge(&expiring.nonce, &expiring, 121).is_err());
    db.execute_batch("DROP TRIGGER refuse_challenge").unwrap();
    reopened.create_challenge(&next, 121).unwrap();
    assert!(reopened.challenge(&expiring.nonce).is_err());
    for expected in [&survivor, &next] {
        assert_eq!(serde_json::to_value(reopened.challenge(&expected.nonce).unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap());
    }
    reopened.consume_challenge(&next.nonce, &next, 121).unwrap();
    assert!(reopened.challenge(&next.nonce).is_err());
}

fn restore_free_accounts(db: &mut Connection, fixture: &Fixture, seed: &OpenResult, count: u64, owner: Option<&str>) -> Vec<OpenResult> {
    // These complete signed account/root records are storage prerequisites, not
    // observed historical proposal, owner-authentication or opening admissions.
    let original_quote = fixture.provider.state.ledger.quote(&seed.account.contract_id).unwrap();
    let (created, root): (u64, String) = db.query_row(
        "SELECT accounts.created,credentials.record FROM accounts
         JOIN credentials ON credentials.contract=accounts.id
         WHERE accounts.id=?1 AND credentials.id=?2",
        params![seed.account.contract_id, seed.credential.as_ref().unwrap().id],
        |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    let original_root: CredentialView = serde_json::from_str(&root).unwrap();
    assert!(original_root.root && !original_root.revoked);
    let mut samples = Vec::with_capacity(3);
    let tx = db.transaction().unwrap();
    {
        let mut insert_account = tx.prepare(
            "INSERT INTO accounts(id,owner,quote,account,sequence,created) VALUES(?1,?2,?3,?4,0,?5)").unwrap();
        let mut insert_credential = tx.prepare(
            "INSERT INTO credentials(id,contract,token_hash,record,revoked) VALUES(?1,?2,?3,?4,0)").unwrap();
        for index in 0..count {
            let mut payload = original_quote.payload.clone();
            payload.id = protocol::random_id();
            payload.nonce = protocol::random_id();
            payload.consumer_wallet = owner.map(str::to_owned).unwrap_or_else(|| format!("0x{:040x}", 0x10000 + index));
            let quote = protocol::sign(protocol::QUOTE_DOMAIN, payload, &fixture.provider.state.private_key).unwrap();
            protocol::verify(protocol::QUOTE_DOMAIN, &quote, &fixture.provider.state.wallet).unwrap();
            let mut account = seed.account.clone();
            account.contract_id = quote.payload.id.clone();
            account.consumer_wallet = quote.payload.consumer_wallet.clone();
            let mut root = original_root.clone();
            root.id = protocol::random_id();
            root.contract_id = account.contract_id.clone();
            let credential = Credential {
                id: root.id.clone(), contract_id: account.contract_id.clone(),
                bearer: format!("chiral_{}", &protocol::random_id()[2..]), expires_at: root.expires_at,
            };
            insert_account.execute(params![account.contract_id, account.consumer_wallet,
                serde_json::to_string(&quote).unwrap(), serde_json::to_string(&account).unwrap(), created]).unwrap();
            insert_credential.execute(params![root.id, root.contract_id,
                protocol::hash_hex(credential.bearer.as_bytes()), serde_json::to_string(&root).unwrap()]).unwrap();
            if index == 0 || index == count / 2 || index + 1 == count {
                samples.push(OpenResult { account, credential: Some(credential) });
            }
        }
    }
    tx.commit().unwrap();
    samples
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_free_openings_compete_for_the_last_owned_account_slot() {
    free_account_final_slot(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_free_openings_compete_for_the_last_ledger_account_slot() {
    free_account_final_slot(true).await;
}

async fn free_account_final_slot(ledger_wide: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), if ledger_wide { 52_001 } else { 52_000 });
    let rpc_calls = Arc::new(Mutex::new(Vec::new()));
    let observed = rpc_calls.clone();
    let rpc_app = Router::new().fallback(move |request: Request| {
        let observed = observed.clone();
        async move {
            observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
            StatusCode::SERVICE_UNAVAILABLE
        }
    });
    fixture._listeners[0].set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
    let rpc_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });

    // Establish actual free accounts/credentials through the production owner route,
    // not constructed quota rows or synthetic paid-account prerequisites.
    let initial_owned: u64 = if ledger_wide { 1 } else { 7 };
    let mut earlier = Vec::with_capacity(if ledger_wide { 6 } else { 9 });
    for _ in 0..initial_owned { earlier.push(fixture.open_free().await); }
    let other_wallet = wallet::address_from_private_key(OTHER_KEY).unwrap();
    let (status, document) = fixture.post("/v1/contracts/propose", json!({
        "offer_id":fixture.provider.offer_id,"consumer_wallet":other_wallet,"funding_wei":"0"
    })).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let other_quote: Signed<Quote> = serde_json::from_value(document).unwrap();
    let body = serde_json::to_vec(&json!({"quote_id":other_quote.payload.id,"raw_transaction":null})).unwrap();
    let proof = fixture.proof(OTHER_KEY, "POST", OPEN, &body).await;
    let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    earlier.push(serde_json::from_value::<OpenResult>(document).unwrap());
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    if ledger_wide {
        earlier.extend(restore_free_accounts(&mut db, &fixture, &earlier[0], 9_997, None));
    }
    let keys = [OWNER_KEY, if ledger_wide { OTHER_KEY } else { OWNER_KEY }];
    let owners = keys.map(|key| wallet::address_from_private_key(key).unwrap());
    let first_quote = fixture.quote("0").await;
    let (status, document) = fixture.post("/v1/contracts/propose", json!({
        "offer_id":fixture.provider.offer_id,"consumer_wallet":owners[1],"funding_wei":"0"
    })).await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let quotes = [first_quote, serde_json::from_value::<Signed<Quote>>(document).unwrap()];
    for (quote, owner) in quotes.iter().zip(&owners) {
        protocol::verify(protocol::QUOTE_DOMAIN, quote, &fixture.provider.state.wallet).unwrap();
        assert_eq!(&quote.payload.consumer_wallet, owner);
        assert_eq!(quote.payload.funding_wei, "0");
        assert_eq!(quote.payload.price.input_wei_per_1k, "0");
        assert_eq!(quote.payload.price.output_wei_per_1k, "0");
    }
    let counts = || db.query_row(
        "SELECT (SELECT COUNT(*) FROM accounts WHERE owner=?1),
                (SELECT COUNT(*) FROM accounts WHERE owner=?2),(SELECT COUNT(*) FROM accounts)",
        params![owners[0], other_wallet],
        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?))).unwrap();
    let before_total = if ledger_wide { 9_999 } else { 8 };
    assert_eq!(counts(), (initial_owned, 1, before_total));
    assert_eq!(db.query_row("SELECT COUNT(*) FROM quotes", [], |row| row.get::<_, u64>(0)).unwrap(), 2,
        "account capacity is isolated from pending-quote retention");
    if ledger_wide {
        let max_owned: u64 = db.query_row(
            "SELECT MAX(owned) FROM (SELECT COUNT(*) AS owned FROM accounts GROUP BY owner)",
            [], |row| row.get(0)).unwrap();
        assert_eq!(max_owned, 1, "neither contender nor a restored owner is at the owned-account bound");
    }
    // Stream every persisted column; do not retain 10,000 signed records in memory.
    let records_except = |excluded: &str| [
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts WHERE id<>?1 ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials WHERE contract<>?1 ORDER BY id",
    ].map(|query| {
        let mut hash = Keccak::v256();
        let mut count = 0_u64;
        let mut statement = db.prepare(query).unwrap();
        let mut rows = statement.query([excluded]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            hash.update(row.get::<_, String>(0).unwrap().as_bytes());
            count += 1;
        }
        let mut digest = [0; 32];
        hash.finalize(&mut digest);
        (count, digest)
    });
    let earlier_records = records_except(&quotes[0].payload.id);
    assert_eq!((earlier_records[0].0, earlier_records[1].0), (before_total, before_total));
    let pending = |id: &str| db.query_row(
        "SELECT json_array(id,owner,expires,record) FROM quotes WHERE id=?1",
        [id], |row| row.get::<_, String>(0)).unwrap();
    let pending_before = quotes.each_ref().map(|quote| pending(&quote.payload.id));
    let bodies = quotes.each_ref().map(|quote|
        serde_json::to_vec(&json!({"quote_id":quote.payload.id,"raw_transaction":null})).unwrap());
    let proofs = [
        fixture.proof(keys[0], "POST", OPEN, &bodies[0]).await,
        fixture.proof(keys[1], "POST", OPEN, &bodies[1]).await,
    ];
    assert_ne!(proofs[0].nonce, proofs[1].nonce);

    // Share provider identity and wallet journal directory, but not the ledger/chain
    // connection or process-local mutex. Both owner-authorized requests rendezvous.
    let source = &fixture.provider.state;
    let mut second_state = Arc::new(ProviderState {
        chain: Arc::new(ChainClient::new(source.config.network.clone(), source.config.data_dir.clone()).unwrap()),
        ledger: Arc::new(BillingStore::open(&fixture.provider.ledger_path, &source.config.network.identity).unwrap()),
        config: source.config.clone(), private_key: source.private_key.clone(), wallet: source.wallet.clone(),
        offers: source.offers.clone(),
        accepting: AtomicBool::new(source.accepting.load(Ordering::Acquire)),
        funding_ready: AtomicBool::new(source.funding_ready.load(Ordering::Acquire)),
        qualified_models: std::sync::RwLock::new(source.qualified_models.read().unwrap().clone()),
        model_slots: source.model_slots.clone(), managed_engines: source.managed_engines.clone(),
    });
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let requests = [
        (fixture.app.clone(), proofs[0].headers.clone(), bodies[0].clone()),
        (contracts::router(second_state.clone()), proofs[1].headers.clone(), bodies[1].clone()),
    ].map(|(mut app, headers, body)| {
        let barrier = barrier.clone();
        tokio::spawn(async move {
            let mut request = Request::builder().method("POST").uri(OPEN)
                .header(header::CONTENT_TYPE, "application/json").body(Body::from(body)).unwrap();
            request.headers_mut().extend(headers);
            barrier.wait().await;
            let response = app.call(request).await.unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), 96 * 1024).await.unwrap();
            (status, serde_json::from_slice::<Value>(&body).unwrap())
        })
    });
    let [first, second] = requests;
    let (first, second) = tokio::join!(first, second);
    let results = [first.unwrap(), second.unwrap()];
    let winner = match (results[0].0, results[1].0) {
        (StatusCode::OK, StatusCode::TOO_MANY_REQUESTS) => 0,
        (StatusCode::TOO_MANY_REQUESTS, StatusCode::OK) => 1,
        other => panic!("one free account slot must admit exactly one opening: {other:?}, {results:?}"),
    };
    let loser = 1 - winner;
    assert_eq!(results[loser].1["error"]["code"], "rate_limit");
    for proof in &proofs { fixture.consumed(proof); }
    let after_counts = (initial_owned + u64::from(!ledger_wide || winner == 0),
        1 + u64::from(ledger_wide && winner == 1), before_total + 1);
    assert_eq!(counts(), after_counts);
    let opened: OpenResult = serde_json::from_value(results[winner].1.clone()).unwrap();
    assert_eq!(opened.account.contract_id, quotes[winner].payload.id);
    assert_eq!(opened.account.network, quotes[winner].payload.network);
    assert_eq!(opened.account.provider_wallet, quotes[winner].payload.provider_wallet);
    assert_eq!(opened.account.consumer_wallet, owners[winner]);
    assert_eq!(opened.account.model, quotes[winner].payload.model);
    assert_eq!(opened.account.price, quotes[winner].payload.price);
    assert_eq!(opened.account.status, "free");
    for amount in [&opened.account.funded_wei, &opened.account.fee_wei,
        &opened.account.spent_wei, &opened.account.reserved_wei, &opened.account.available_wei,
        &opened.account.reversed_wei, &opened.account.provider_loss_wei, &opened.account.waived_wei] {
        assert_eq!(amount, "0");
    }
    assert_eq!(records_except(&quotes[winner].payload.id), earlier_records);
    assert_eq!(pending(&quotes[loser].payload.id), pending_before[loser]);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM quotes WHERE id=?1",
        [&quotes[winner].payload.id], |row| row.get::<_, u64>(0)).unwrap(), 0);
    let losing_records = || db.query_row(
        "SELECT (SELECT COUNT(*) FROM accounts WHERE id=?1),
                (SELECT COUNT(*) FROM credentials WHERE contract=?1)",
        [&quotes[loser].payload.id], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))).unwrap();
    assert_eq!(losing_records(), (0, 0));
    let opened_records = records_except(&quotes[loser].payload.id);
    assert_eq!(opened_records[0].0, before_total + 1);
    assert_eq!(opened_records[1].0, before_total + 1, "one root per account, none for the refused quote");

    // Account exhaustion is not a public-quote reservation. This stays below the
    // separate, still-unmigrated named-wallet quote ceilings and claims no mitigation.
    let overbooked = fixture.quote("0").await;
    let overbooked_before = pending(&overbooked.payload.id);
    assert_eq!(counts(), after_counts);
    assert_eq!(records_except(&quotes[loser].payload.id), opened_records);
    let proof = fixture.proof(keys[loser], "POST", OPEN, &bodies[loser]).await;
    let refused = fixture.send("POST", OPEN, &proof.headers, bodies[loser].clone()).await;
    assert_eq!(refused.0, StatusCode::TOO_MANY_REQUESTS, "{}", refused.1);
    assert_eq!(refused.1["error"]["code"], "rate_limit");
    fixture.consumed(&proof);

    fixture.app = Router::new();
    for state in [&mut fixture.provider.state, &mut second_state] {
        let state = Arc::get_mut(state).unwrap();
        state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
    }
    fixture.app = contracts::router(fixture.provider.state.clone());
    let proof = fixture.proof(keys[loser], "POST", OPEN, &bodies[loser]).await;
    let refused = fixture.send("POST", OPEN, &proof.headers, bodies[loser].clone()).await;
    assert_eq!(refused.0, StatusCode::TOO_MANY_REQUESTS, "{}", refused.1);
    assert_eq!(refused.1["error"]["code"], "rate_limit");
    fixture.consumed(&proof);
    assert_eq!(counts(), after_counts);
    assert_eq!(losing_records(), (0, 0));
    assert_eq!(pending(&quotes[loser].payload.id), pending_before[loser]);
    assert_eq!(pending(&overbooked.payload.id), overbooked_before);
    // Read genuine issued bearers and representative restored credentials, never
    // reopen accepted accounts through the separately blocked root-rotation path.
    earlier.push(opened);
    for accepted in &earlier {
        for ledger in [&fixture.provider.state.ledger, &second_state.ledger] {
            let authenticated = ledger.authenticate(&accepted.credential.as_ref().unwrap().bearer, protocol::now().unwrap()).unwrap();
            assert_eq!(authenticated.credential_id, accepted.credential.as_ref().unwrap().id);
            assert_eq!(serde_json::to_value(authenticated.account).unwrap(), serde_json::to_value(&accepted.account).unwrap());
        }
        let (status, account) = fixture.send("GET",
            &format!("/v1/contracts/{}", accepted.account.contract_id), &bearer(accepted.credential.as_ref().unwrap()), Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{account}");
        assert_eq!(account, serde_json::to_value(&accepted.account).unwrap());
    }
    assert_eq!(records_except(&quotes[loser].payload.id), opened_records);
    let paid_rows: (u64, u64, u64, u64, u64) = db.query_row(
        "SELECT (SELECT COUNT(*) FROM funding_intents),(SELECT COUNT(*) FROM funding_attempts),
                (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations),
                (SELECT COUNT(*) FROM fee_batches)", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
    assert_eq!(paid_rows, (0, 0, 0, 0, 0));
    let journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let payment_rows: (u64, u64) = journal.query_row(
        "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)", [],
        |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(payment_rows, (0, 0));
    assert!(rpc_calls.lock().unwrap().is_empty(), "free opening must not access provider RPC or engine endpoints");
    rpc_server.abort();
    assert!(rpc_server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn failed_free_opening_rolls_back_account_root_and_quote_handoff() {
    let directory = tempfile::tempdir().unwrap();
    for (index, (point, event, contract_field)) in [
        ("account-insert", "AFTER INSERT ON accounts", "NEW.id"),
        ("root-insert", "AFTER INSERT ON credentials", "NEW.contract"),
        ("quote-delete", "AFTER DELETE ON quotes", "OLD.id"),
    ].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 53_000 + index as u64);
        let rpc_calls = Arc::new(Mutex::new(Vec::new()));
        let observed = rpc_calls.clone();
        let rpc_app = Router::new().fallback(move |request: Request| {
            let observed = observed.clone();
            async move {
                observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
                StatusCode::SERVICE_UNAVAILABLE
            }
        });
        fixture._listeners[0].set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
        let rpc_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });

        let first = fixture.open_free().await;
        let (status, document) = fixture.post("/v1/contracts/propose", json!({
            "offer_id":fixture.provider.offer_id,
            "consumer_wallet":wallet::address_from_private_key(OTHER_KEY).unwrap(),"funding_wei":"0"
        })).await;
        assert_eq!(status, StatusCode::OK, "{point}: {document}");
        let other_quote: Signed<Quote> = serde_json::from_value(document).unwrap();
        let body = serde_json::to_vec(&json!({"quote_id":other_quote.payload.id,"raw_transaction":null})).unwrap();
        let proof = fixture.proof(OTHER_KEY, "POST", OPEN, &body).await;
        let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
        assert_eq!(status, StatusCode::OK, "{point}: {document}");
        let mut accepted = vec![first, serde_json::from_value::<OpenResult>(document).unwrap()];
        let quote = fixture.quote("0").await;
        protocol::verify(protocol::QUOTE_DOMAIN, &quote, &fixture.provider.state.wallet).unwrap();
        let unrelated = fixture.quote("0").await;
        let body = serde_json::to_vec(&json!({"quote_id":quote.payload.id,"raw_transaction":null})).unwrap();
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let pending = || db.query_row(
            "SELECT json_array(id,owner,expires,record) FROM quotes WHERE id=?1",
            [&quote.payload.id], |row| row.get::<_, String>(0)).unwrap();
        let pending_before = pending();
        let other_records = || [
            "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts WHERE id<>?1 ORDER BY id",
            "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials WHERE contract<>?1 ORDER BY id",
            "SELECT json_array(id,owner,expires,record) FROM quotes WHERE id<>?1 ORDER BY id",
        ].map(|query| {
            let mut hash = Keccak::v256();
            let mut count = 0_u64;
            let mut statement = db.prepare(query).unwrap();
            let mut rows = statement.query([&quote.payload.id]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                hash.update(row.get::<_, String>(0).unwrap().as_bytes());
                count += 1;
            }
            let mut digest = [0; 32];
            hash.finalize(&mut digest);
            (count, digest)
        });
        let prior_records = other_records();
        assert_eq!(prior_records.map(|(count, _)| count), [2, 2, 1]);
        let target_counts = || db.query_row(
            "SELECT (SELECT COUNT(*) FROM accounts WHERE id=?1),
                    (SELECT COUNT(*) FROM credentials WHERE contract=?1)",
            [&quote.payload.id], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))).unwrap();

        // FAIL preserves the failed statement's writes inside its transaction.
        // AFTER hooks require the production transaction to undo the actual mutation,
        // including earlier account creation/quote removal before root insertion.
        db.execute_batch(&format!(
            "CREATE TRIGGER fail_free_opening {event} WHEN {contract_field}='{}'
             BEGIN SELECT RAISE(FAIL,'injected free opening persistence failure'); END;",
            quote.payload.id)).unwrap();
        for reopened in [false, true] {
            if reopened {
                fixture.app = Router::new();
                let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
                state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
                fixture.app = contracts::router(fixture.provider.state.clone());
            }
            let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
            let (status, error) = fixture.send("POST", OPEN, &proof.headers, body.clone()).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{point}, reopened={reopened}: {error}");
            assert_eq!(error["error"]["code"], "internal_error", "{point}");
            assert!(error.get("account").is_none() && error.get("credential").is_none(), "{point}: {error}");
            fixture.consumed(&proof);
            fixture.unopened(&quote);
            assert_eq!(target_counts(), (0, 0), "{point}, reopened={reopened}");
            assert_eq!(pending(), pending_before, "{point}, reopened={reopened}");
            assert_eq!(other_records(), prior_records, "{point}, reopened={reopened}");

            // Owner authorization is consumed in its own transaction, not revived
            // by rolling back first opening or by reopening the provider objects.
            let (status, replay) = fixture.send("POST", OPEN, &proof.headers, body.clone()).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{point}: {replay}");
            assert_eq!(replay["error"]["code"], "invalid_api_key", "{point}");
            assert_eq!(target_counts(), (0, 0), "{point}");
            assert_eq!(pending(), pending_before, "{point}");
            assert_eq!(other_records(), prior_records, "{point}");
            for earlier in &accepted {
                fixture.credentials_usable(&[earlier.credential.as_ref().unwrap()]);
                let (status, account) = fixture.send("GET",
                    &format!("/v1/contracts/{}", earlier.account.contract_id),
                    &bearer(earlier.credential.as_ref().unwrap()), Vec::new()).await;
                assert_eq!(status, StatusCode::OK, "{point}: {account}");
                assert_eq!(account, serde_json::to_value(&earlier.account).unwrap(), "{point}");
            }
        }

        // Explicit fixture repair is a positive control, not an automatic retry or
        // product recovery policy. Open the original quote with new owner authority.
        db.execute_batch("DROP TRIGGER fail_free_opening").unwrap();
        let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
        let (status, document) = fixture.send("POST", OPEN, &proof.headers, body).await;
        assert_eq!(status, StatusCode::OK, "{point}: {document}");
        let opened: OpenResult = serde_json::from_value(document).unwrap();
        let mut expected_account = accepted[0].account.clone();
        expected_account.contract_id = quote.payload.id.clone();
        assert_eq!(serde_json::to_value(&opened.account).unwrap(),
            serde_json::to_value(expected_account).unwrap(), "{point}");
        assert_eq!(opened.credential.as_ref().unwrap().contract_id, quote.payload.id, "{point}");
        fixture.consumed(&proof);
        assert_eq!(target_counts(), (1, 1), "{point}");
        assert_eq!(db.query_row("SELECT COUNT(*) FROM quotes WHERE id=?1",
            [&quote.payload.id], |row| row.get::<_, u64>(0)).unwrap(), 0, "{point}");
        assert_eq!(other_records(), prior_records, "{point}");
        accepted.push(opened);

        fixture.app = Router::new();
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
        fixture.app = contracts::router(fixture.provider.state.clone());
        assert_eq!(target_counts(), (1, 1), "{point}");
        assert_eq!(other_records(), prior_records, "{point}");
        for retained in [&quote, &unrelated] {
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger.quote(&retained.payload.id).unwrap()).unwrap(),
                serde_json::to_value(retained).unwrap(), "{point}");
        }
        // Recover by reading actual issued credentials, never by reopening an
        // accepted account through the separate root-rotation recovery blocker.
        for earlier in &accepted {
            fixture.credentials_usable(&[earlier.credential.as_ref().unwrap()]);
            let (status, account) = fixture.send("GET",
                &format!("/v1/contracts/{}", earlier.account.contract_id),
                &bearer(earlier.credential.as_ref().unwrap()), Vec::new()).await;
            assert_eq!(status, StatusCode::OK, "{point}: {account}");
            assert_eq!(account, serde_json::to_value(&earlier.account).unwrap(), "{point}");
        }
        assert_eq!(other_records(), prior_records, "{point}");
        let paid_rows: (u64, u64, u64, u64, u64) = db.query_row(
            "SELECT (SELECT COUNT(*) FROM funding_intents),(SELECT COUNT(*) FROM funding_attempts),
                    (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations),
                    (SELECT COUNT(*) FROM fee_batches)", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
        assert_eq!(paid_rows, (0, 0, 0, 0, 0), "{point}");
        let journal = Connection::open(fixture.provider.state.config.network
            .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
        let payment_rows: (u64, u64) = journal.query_row(
            "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)", [],
            |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(payment_rows, (0, 0), "{point}");
        assert!(rpc_calls.lock().unwrap().is_empty(), "{point}: free admission must not access provider RPC/engine");
        rpc_server.abort();
        assert!(rpc_server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn acknowledged_paid_ledger_opening_survives_the_owned_free_account_limit() {
    paid_ledger_opening_at_free_limit(false).await;
}

#[tokio::test]
async fn acknowledged_paid_ledger_opening_survives_the_global_free_account_limit() {
    paid_ledger_opening_at_free_limit(true).await;
}

async fn paid_ledger_opening_at_free_limit(ledger_wide: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), if ledger_wide { 54_001 } else { 54_000 });
    let rpc_calls = Arc::new(Mutex::new(Vec::new()));
    let observed = rpc_calls.clone();
    let rpc_app = Router::new().fallback(move |request: Request| {
        let observed = observed.clone();
        async move {
            observed.lock().unwrap().push((request.method().clone(), request.uri().to_string()));
            StatusCode::SERVICE_UNAVAILABLE
        }
    });
    fixture._listeners[0].set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(fixture._listeners[0].try_clone().unwrap()).unwrap();
    let rpc_server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });

    let mut free_offer = fixture.provider.offer.payload.clone();
    let unrelated = fixture.paid_quote().await;
    let quote = fixture.quote("1000").await;
    let (raw, envelope) = fixture.signed_payment(&protocol::opening_data(&quote.payload).unwrap(), 1000, 0);
    // The real ledger commits its signed acknowledgement, but current spendability
    // and owner admission are explicit prerequisites, not observed HTTP/RPC events.
    let acknowledgement = fixture.acknowledge_fixture(&quote, &raw, &envelope);
    protocol::verify(protocol::INTENT_DOMAIN, &acknowledgement, &fixture.provider.state.wallet).unwrap();
    fixture.unopened(&quote);
    // Only the post-verification ledger boundary is under test. This accepted-block
    // input is not execution, safe-frontier, broadcast or live payment evidence.
    let payment = VerifiedPayment {
        network: quote.payload.network.clone(), tx_hash: envelope.tx_hash.clone(),
        from: envelope.from.clone(), to: envelope.to.clone(), value_wei: envelope.value_wei.clone(),
        data: envelope.data.clone(), block_hash: protocol::hash_hex(b"paid capacity ledger prerequisite"),
        block_number: 1, raw_transaction: raw.clone(),
    };
    free_offer.revision = fixture.provider.offer.payload.revision + 1;
    fixture.provider.offer = protocol::sign(protocol::OFFER_DOMAIN, free_offer, &fixture.provider.state.private_key).unwrap();
    fixture.provider.offer_id = fixture.provider.state.offers.insert(fixture.provider.offer.clone()).unwrap();

    // Fill account capacity AFTER the acknowledgement. The owned case consists
    // entirely of real free HTTP openings; the global case restores bulk history.
    let owned = if ledger_wide { 1_u64 } else { 8 };
    let total = if ledger_wide { 10_000_u64 } else { 8 };
    let mut earlier = Vec::with_capacity(9);
    for _ in 0..owned { earlier.push(fixture.open_free().await); }
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    if ledger_wide {
        let restored = restore_free_accounts(&mut db, &fixture, &earlier[0], 9_999, None);
        earlier.extend(restored);
    }
    let refused = fixture.quote("0").await;
    let free_body = serde_json::to_vec(&json!({"quote_id":refused.payload.id,"raw_transaction":null})).unwrap();
    let paid_body = serde_json::to_vec(&json!({"quote_id":quote.payload.id,"raw_transaction":raw})).unwrap();
    let counts = || db.query_row(
        "SELECT (SELECT COUNT(*) FROM accounts WHERE owner=?1),(SELECT COUNT(*) FROM accounts)",
        [&quote.payload.consumer_wallet],
        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))).unwrap();
    assert_eq!(counts(), (owned, total));
    if ledger_wide {
        assert_eq!(db.query_row(
            "SELECT MAX(owned) FROM (SELECT COUNT(*) AS owned FROM accounts GROUP BY owner)",
            [], |row| row.get::<_, u64>(0)).unwrap(), 1,
            "the global boundary must not be masked by an owned-account limit");
    }
    let fingerprint = |query: &str| {
        let mut hash = Keccak::v256();
        let mut count = 0_u64;
        let mut statement = db.prepare(query).unwrap();
        let mut rows = statement.query([&quote.payload.id]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            hash.update(row.get::<_, String>(0).unwrap().as_bytes());
            count += 1;
        }
        let mut digest = [0; 32];
        hash.finalize(&mut digest);
        (count, digest)
    };
    let other_records = || [
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts WHERE id<>?1 ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials WHERE contract<>?1 ORDER BY id",
        "SELECT json_array(id,owner,expires,record) FROM quotes WHERE id<>?1 ORDER BY id",
    ].map(&fingerprint);
    let before = other_records();
    assert_eq!(before.map(|record| record.0), [total, total, 2]);
    let intent = || db.query_row(
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce),admission_until
         FROM funding_intents WHERE contract=?1", [&quote.payload.id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))).unwrap();
    let retained = intent();
    assert_eq!(retained.1, quote.payload.expires_at);
    let paid_counts = || db.query_row(
        "SELECT (SELECT COUNT(*) FROM accounts WHERE id=?1),
                (SELECT COUNT(*) FROM credentials WHERE contract=?1),
                (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM funding_attempts),
                (SELECT COUNT(*) FROM fee_obligations),(SELECT COUNT(*) FROM fee_batches)",
        [&quote.payload.id], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?,
            row.get::<_, u64>(2)?, row.get::<_, u64>(3)?, row.get::<_, u64>(4)?, row.get::<_, u64>(5)?))).unwrap();

    for reopened in [false, true] {
        if reopened {
            fixture.app = Router::new();
            let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
            state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
            state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
            fixture.app = contracts::router(fixture.provider.state.clone());
        }
        let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &free_body).await;
        let (status, document) = fixture.send("POST", OPEN, &proof.headers, free_body.clone()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{document}");
        assert_eq!(document["error"]["code"], "rate_limit");
        fixture.consumed(&proof);
        fixture.unopened(&refused);
        assert!(fixture.provider.state.ledger.credentials(&refused.payload.id).unwrap().is_empty());
        let wrong_owner = fixture.proof(OTHER_KEY, "POST", OPEN, &paid_body).await;
        let (status, document) = fixture.send("POST", OPEN, &wrong_owner.headers, paid_body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{document}");
        assert_eq!(document["error"]["code"], "forbidden");
        fixture.consumed(&wrong_owner);
        let ledger = &fixture.provider.state.ledger;
        let now = protocol::now().unwrap();
        assert!(ledger.check_open_funding(&quote.payload, None, now).unwrap_err()
            .starts_with("funding_intent_required:"));
        ledger.check_open_funding(&quote.payload, Some(&raw), now).unwrap();
        assert!(ledger.open_account(&quote, None, now).unwrap_err().starts_with("invalid_payment:"),
            "acknowledgement and full free capacity cannot replace verified payment evidence");
        fixture.unopened(&quote);
        assert_eq!(paid_counts(), (0, 0, 0, 0, 0, 0));
        assert_eq!(intent(), retained);
        assert_eq!(other_records(), before);
        assert_eq!(counts(), (owned, total));
    }

    let opened = fixture.provider.state.ledger.open_account(&quote, Some(&payment), protocol::now().unwrap()).unwrap();
    assert_eq!(opened.account.contract_id, quote.payload.id);
    assert_eq!(opened.account.network, quote.payload.network);
    assert_eq!(opened.account.provider_wallet, quote.payload.provider_wallet);
    assert_eq!(opened.account.consumer_wallet, quote.payload.consumer_wallet);
    assert_eq!(opened.account.model, quote.payload.model);
    assert_eq!(opened.account.price, quote.payload.price);
    assert_eq!(opened.account.max_concurrency, quote.payload.max_concurrency);
    assert_eq!(opened.account.status, "active");
    let account = serde_json::to_value(&opened.account).unwrap();
    for (field, amount) in [("funded_wei", "1000"), ("fee_wei", "5"), ("available_wei", "995"),
        ("spent_wei", "0"), ("reserved_wei", "0"), ("reversed_wei", "0"),
        ("provider_loss_wei", "0"), ("waived_wei", "0")] {
        assert_eq!(account[field], amount, "{field}");
    }
    assert_eq!(account["usage"], serde_json::to_value(TokenUsage::default()).unwrap());
    assert_eq!(counts(), (owned + 1, total + 1));
    assert_eq!(paid_counts(), (1, 1, 1, 0, 1, 0));
    assert_eq!(intent(), (retained.0.clone(), 0), "opening releases the lease, not its permanent ownership");
    assert_eq!(other_records(), before);
    let (evidence, status): (String, String) = db.query_row(
        "SELECT evidence,status FROM funding WHERE contract=?1", [&quote.payload.id],
        |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&evidence).unwrap(), serde_json::to_value(&payment).unwrap());
    assert_eq!(status, "accepted");
    let fee: String = db.query_row(
        "SELECT json_array(tx_hash,platform,amount,status,batch) FROM fee_obligations",
        [], |row| row.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&fee).unwrap(),
        json!([envelope.tx_hash, quote.payload.platform_wallet, "5", "pending", null]));
    let paid_records = || [
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts WHERE id=?1",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials WHERE contract=?1 ORDER BY id",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding WHERE contract=?1",
        "SELECT json_array(f.tx_hash,platform,amount,created,f.status,batch) FROM fee_obligations f JOIN funding p ON p.tx_hash=f.tx_hash WHERE p.contract=?1",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents WHERE contract=?1",
    ].map(&fingerprint);
    let credited = paid_records();
    assert_eq!(credited.map(|record| record.0), [1; 5]);

    fixture.app = Router::new();
    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
    state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
    fixture.app = contracts::router(fixture.provider.state.clone());
    let cached = fixture.provider.state.ledger.register_funding_intent(&quote, &quote.payload.consumer_wallet,
        &raw, &envelope.tx_hash, envelope.nonce, &fixture.provider.state.private_key,
        protocol::now, false, &quote.payload.model).unwrap();
    assert_eq!(serde_json::to_value(cached).unwrap(), serde_json::to_value(&acknowledgement).unwrap());
    for retained_quote in [&quote, &unrelated, &refused] {
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.quote(&retained_quote.payload.id).unwrap()).unwrap(),
            serde_json::to_value(retained_quote).unwrap());
    }
    // Read the issued roots; do not invoke the independently blocked re-opening
    // path that currently rotates a root after a lost unsaved opening response.
    earlier.push(opened);
    for accepted in &earlier {
        fixture.credentials_usable(&[accepted.credential.as_ref().unwrap()]);
        let (status, account) = fixture.send("GET", &format!("/v1/contracts/{}", accepted.account.contract_id),
            &bearer(accepted.credential.as_ref().unwrap()), Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{account}");
        assert_eq!(account, serde_json::to_value(&accepted.account).unwrap());
    }
    assert_eq!(paid_records(), credited);
    assert_eq!(other_records(), before);
    assert_eq!(paid_counts(), (1, 1, 1, 0, 1, 0));
    assert_eq!(counts(), (owned + 1, total + 1));
    let journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let journal_counts: (u64, u64) = journal.query_row(
        "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(journal_counts, (0, 0));
    assert!(rpc_calls.lock().unwrap().is_empty(), "post-verification ledger evidence must not invoke RPC/engine or broadcast");
    rpc_server.abort();
    assert!(rpc_server.await.unwrap_err().is_cancelled());
}

struct SafeOpeningRpc {
    quote: Signed<Quote>,
    raw: String,
    hash: String,
    saved: PathBuf,
    phase: AtomicU64,
    timestamp: AtomicU64,
    requests: Mutex<Vec<(bool, Value)>>,
}

async fn safe_opening_rpc(
    State((rpc, provider)): State<(Arc<SafeOpeningRpc>, bool)>,
    axum::Json(request): axum::Json<Value>,
) -> Response {
    rpc.requests.lock().unwrap().push((provider, request.clone()));
    let phase = rpc.phase.load(Ordering::Acquire);
    if !provider {
        // Inspect actual native persistence before every chain request, including
        // the initial transport failure immediately after fresh acknowledgement.
        let saved: Value = serde_json::from_slice(&std::fs::read(&rpc.saved).unwrap()).unwrap();
        assert_eq!(saved["quote"], serde_json::to_value(&rpc.quote).unwrap());
        assert_eq!(saved["raw_transaction"], rpc.raw);
        assert_eq!(saved["tx_hash"], rpc.hash);
        assert_eq!(saved["funding_broadcast_started"], true);
        let ack: Signed<FundingIntent> = serde_json::from_value(saved["funding_intent"].clone()).unwrap();
        protocol::verify(protocol::INTENT_DOMAIN, &ack, &rpc.quote.payload.provider_wallet).unwrap();
        assert_eq!(ack.payload.quote_id, rpc.quote.payload.id);
        assert_eq!(ack.payload.tx_hash, rpc.hash);
        if phase == 0 { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
    }
    let args = &request["params"];
    // Failed execution belongs to a different fork, not a mutable receipt in one block.
    let fork = if provider && phase == 4 { 0x40 } else { 0 };
    let result = match request["method"].as_str().unwrap() {
        "eth_chainId" => json!(format!("0x{:x}", rpc.quote.payload.network.chain_id)),
        "eth_syncing" => json!(false),
        "eth_getBlockByNumber" => {
            assert_eq!(args[1], false);
            let (height, hash) = match args[0].as_str().unwrap() {
                "0x0" => (0, rpc.quote.payload.network.genesis_hash.clone()),
                "safe" => (9, format!("0x{:064x}", 9 + fork)),
                "latest" => (10, format!("0x{:064x}", 10 + fork)),
                "0x8" => (8, format!("0x{:064x}", 8 + fork)),
                tag => panic!("unexpected safe-opening block selector: {tag}"),
            };
            let timestamp = if height == 0 { 0 } else { rpc.timestamp.load(Ordering::Acquire) };
            json!({"number":format!("0x{height:x}"),"hash":hash,"timestamp":format!("0x{timestamp:x}")})
        }
        "eth_getTransactionCount" => {
            assert!(provider && phase == 0, "only fresh spendability may ask for a nonce");
            assert_eq!(args[0], rpc.quote.payload.consumer_wallet);
            assert!(matches!(args[1].as_str(), Some("latest" | "pending")));
            json!("0x0")
        }
        "eth_getBalance" => {
            assert!(provider && phase == 0);
            assert_eq!(args, &json!([rpc.quote.payload.consumer_wallet, "latest"]));
            json!(format!("0x{:x}", u128::MAX))
        }
        "eth_getTransactionReceipt" => {
            assert_eq!(args, &json!([rpc.hash]));
            if provider && phase == 1 { Value::Null } else {
                let height = if provider && phase == 2 { 10 } else { 8 };
                let hash = if provider && phase == 3 { 0xdead } else { height + fork };
                json!({"transactionHash":rpc.hash,"from":rpc.quote.payload.consumer_wallet,
                    "to":rpc.quote.payload.provider_wallet,"type":"0x0","gasUsed":"0x10000",
                    "status":if provider && phase == 4 {"0x0"} else {"0x1"},
                    "blockNumber":format!("0x{height:x}"),"blockHash":format!("0x{hash:064x}")})
            }
        }
        method => panic!("unexpected RPC (including any broadcast): {method}"),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
}

#[tokio::test]
async fn native_paid_opening_checks_safe_credit_at_the_owned_free_limit() {
    native_paid_opening_at_free_limit(false).await;
}

#[tokio::test]
async fn native_paid_opening_checks_safe_credit_at_the_global_free_limit() {
    native_paid_opening_at_free_limit(true).await;
}

async fn native_paid_opening_at_free_limit(ledger_wide: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), if ledger_wide { 55_001 } else { 55_000 });
    let seed = fixture.open_free().await;
    let mut free_offer = fixture.provider.offer.payload.clone();
    let unrelated = fixture.paid_quote().await;
    let quote = fixture.quote("1000").await;
    let paid_offer = fixture.provider.offer.clone();
    let (raw, envelope) = fixture.signed_payment(&protocol::opening_data(&quote.payload).unwrap(), 1000, 0);
    let provider_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.app = Router::new();
    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
    let provider_config = Arc::get_mut(&mut state.config).unwrap();
    provider_config.network.rpc_url = format!("http://{}", provider_listener.local_addr().unwrap());
    state.chain = Arc::new(ChainClient::new(provider_config.network.clone(), provider_config.data_dir.clone()).unwrap());
    fixture.app = contracts::router(fixture.provider.state.clone());
    let mut network = fixture.provider.state.config.network.clone();
    network.rpc_url = format!("http://{}", client_listener.local_addr().unwrap());
    let config = ConsumerConfig { network, data_dir: directory.path().join("consumer"),
        catalog_url: None, development: true, dht: None };
    let mut consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, OWNER_KEY).unwrap();
    assert_eq!(consumer.wallet_import_file(&import).unwrap(), quote.payload.consumer_wallet);
    let scoped = config.network.scoped_dir(&config.data_dir);
    let account_path = scoped.join("consumer").join(&quote.payload.consumer_wallet)
        .join(format!("{}.json", hex::encode(protocol::digest(quote.payload.id.as_bytes()))));
    let journal = Connection::open(scoped.join("wallet-journal/transactions.sqlite3")).unwrap();
    // Restore only original preparation. Acknowledgement, local persistence,
    // chain verification and HTTP credit below all execute their production paths.
    journal.execute(
        "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at)
         VALUES(?1,?2,'0',?3,?4,'prepared',?5)",
        params![quote.payload.consumer_wallet, format!("open:{}", quote.payload.id),
            envelope.tx_hash, raw, quote.payload.issued_at]).unwrap();
    let original = || journal.query_row(
        "SELECT json_array(sender,operation,nonce,tx_hash,raw,created_at) FROM transactions",
        [], |row| row.get::<_, String>(0)).unwrap();
    let original_before = original();
    let rpc = Arc::new(SafeOpeningRpc { quote: quote.clone(), raw: raw.clone(),
        hash: envelope.tx_hash.clone(), saved: account_path.clone(), phase: AtomicU64::new(0),
        timestamp: AtomicU64::new(protocol::now().unwrap()), requests: Mutex::new(Vec::new()) });
    let provider_rpc = Router::new().route("/", post(safe_opening_rpc)).with_state((rpc.clone(), true));
    let client_rpc = Router::new().route("/", post(safe_opening_rpc)).with_state((rpc.clone(), false));
    let provider_rpc_server = tokio::spawn(async move { axum::serve(provider_listener, provider_rpc).await.unwrap(); });
    let client_rpc_server = tokio::spawn(async move { axum::serve(client_listener, client_rpc).await.unwrap(); });

    let exchanges = Arc::new(Mutex::new(Vec::new()));
    let listener = fixture._listeners[0].try_clone().unwrap();
    listener.set_nonblocking(true).unwrap();
    let serve = |app: Router| {
        let observed = exchanges.clone();
        let app = app.layer(from_fn(move |request: Request, next: Next| {
            let observed = observed.clone();
            async move {
                let (parts, body) = request.into_parts();
                let method = parts.method.to_string();
                let path = parts.uri.to_string();
                let nonce = parts.headers.get("x-chiral-nonce").map(|value| value.to_str().unwrap().to_owned());
                let body = to_bytes(body, 96 * 1024).await.unwrap();
                let response = next.run(Request::from_parts(parts, Body::from(body.clone()))).await;
                observed.lock().unwrap().push((method, path, body, nonce, response.status()));
                response
            }
        }));
        let listener = tokio::net::TcpListener::from_std(listener.try_clone().unwrap()).unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.unwrap(); }).await.unwrap();
        });
        (stop, server)
    };
    let assert_exchange = |ledger: &BillingStore, start: usize, method: &str, path: &str, body: Value, status: StatusCode| {
        let observed = exchanges.lock().unwrap();
        assert_eq!(observed.len(), start + 2, "one owner proof and one control request, without automatic retries");
        let challenge = &observed[start];
        let control = &observed[start + 1];
        assert_eq!((&challenge.0[..], &challenge.1[..], challenge.4), ("POST", "/v1/auth/challenge", StatusCode::OK));
        let requested: Value = serde_json::from_slice(&challenge.2).unwrap();
        assert_eq!(requested["wallet"], quote.payload.consumer_wallet);
        assert_eq!(requested["method"], method);
        assert_eq!(requested["path"], path);
        assert_eq!(requested["body_hash"], protocol::hash_hex(&control.2));
        assert_eq!((&control.0[..], &control.1[..], control.4), (method, path, status));
        if method == "GET" { assert!(control.2.is_empty()); }
        else { assert_eq!(serde_json::from_slice::<Value>(&control.2).unwrap(), body); }
        assert!(ledger.challenge(control.3.as_ref().unwrap()).is_err());
    };
    let (stop, server) = serve(fixture.app.clone());
    consumer.fund(&paid_offer, &quote).await.expect_err("client RPC is unavailable after genuine acknowledgement");
    assert_exchange(&fixture.provider.state.ledger, 0, "POST",
        &format!("/v1/contracts/{}/funding-intent", quote.payload.id),
        json!({"raw_transaction":raw}), StatusCode::OK);
    let acknowledgement = fixture.provider.state.ledger.funding_intent(&quote.payload, &raw).unwrap().unwrap();
    protocol::verify(protocol::INTENT_DOMAIN, &acknowledgement, &quote.payload.provider_wallet).unwrap();
    let pending_bytes = std::fs::read(&account_path).unwrap();
    let pending: Value = serde_json::from_slice(&pending_bytes).unwrap();
    assert_eq!(pending["funding_intent"], serde_json::to_value(&acknowledgement).unwrap());
    assert!(pending["account"].is_null());
    assert_eq!(pending["credentials"], json!([]));
    let requests = rpc.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|(provider, request)| *provider && request["method"] == "eth_getBalance").count(), 1);
    assert_eq!(requests.iter().filter(|(provider, request)| *provider && request["method"] == "eth_getTransactionCount").count(), 4);
    assert_eq!(requests.iter().filter(|(provider, _)| !provider).count(), 1);
    drop(requests);
    assert_eq!(original(), original_before);
    fixture.unopened(&quote);

    // Later pressure consists of complete signed restored account/root records,
    // not claimed historical admissions. Keep each challenge window unexhausted.
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    let mut earlier = restore_free_accounts(&mut db, &fixture, &seed,
        if ledger_wide { 9_999 } else { 7 },
        if ledger_wide { None } else { Some(&quote.payload.consumer_wallet) });
    earlier.push(seed);
    let total = if ledger_wide { 10_000_u64 } else { 8 };
    let owned = if ledger_wide { 1_u64 } else { 8 };
    let counts = || db.query_row(
        "SELECT (SELECT COUNT(*) FROM accounts WHERE owner=?1),(SELECT COUNT(*) FROM accounts)",
        [&quote.payload.consumer_wallet], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))).unwrap();
    assert_eq!(counts(), (owned, total));
    free_offer.revision = paid_offer.payload.revision + 1;
    fixture.provider.offer = protocol::sign(protocol::OFFER_DOMAIN, free_offer, &fixture.provider.state.private_key).unwrap();
    fixture.provider.offer_id = fixture.provider.state.offers.insert(fixture.provider.offer.clone()).unwrap();
    let refused = fixture.quote("0").await;
    let free_body = serde_json::to_vec(&json!({"quote_id":refused.payload.id,"raw_transaction":null})).unwrap();
    let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &free_body).await;
    let (status, document) = fixture.send("POST", OPEN, &proof.headers, free_body).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{document}");
    assert_eq!(document["error"]["code"], "rate_limit");
    fixture.consumed(&proof);
    fixture.unopened(&refused);
    let paid_body = serde_json::to_vec(&json!({"quote_id":quote.payload.id,"raw_transaction":raw})).unwrap();
    let before_rpc = rpc.requests.lock().unwrap().len();
    let wrong_owner = fixture.proof(OTHER_KEY, "POST", OPEN, &paid_body).await;
    let (status, document) = fixture.send("POST", OPEN, &wrong_owner.headers, paid_body).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{document}");
    fixture.consumed(&wrong_owner);
    assert_eq!(rpc.requests.lock().unwrap().len(), before_rpc, "wrong-owner proof cannot reach payment verification");
    let fingerprint = |query: &str| {
        let mut hash = Keccak::v256();
        let mut count = 0_u64;
        let mut statement = db.prepare(query).unwrap();
        let mut rows = statement.query([&quote.payload.id]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            hash.update(row.get::<_, String>(0).unwrap().as_bytes());
            count += 1;
        }
        let mut digest = [0; 32];
        hash.finalize(&mut digest);
        (count, digest)
    };
    let prior_records = || [
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts WHERE id<>?1 ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials WHERE contract<>?1 ORDER BY id",
        "SELECT json_array(id,owner,expires,record) FROM quotes WHERE id<>?1 ORDER BY id",
    ].map(&fingerprint);
    let prior = prior_records();
    assert_eq!(prior.map(|row| row.0), [total, total, 2]);
    let intent = || db.query_row(
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce),admission_until
         FROM funding_intents WHERE contract=?1", [&quote.payload.id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))).unwrap();
    let retained = intent();
    assert_eq!(retained.1, quote.payload.expires_at);
    let paid_counts = || db.query_row(
        "SELECT (SELECT COUNT(*) FROM accounts WHERE id=?1),(SELECT COUNT(*) FROM credentials WHERE contract=?1),
                (SELECT COUNT(*) FROM funding),(SELECT COUNT(*) FROM fee_obligations),(SELECT COUNT(*) FROM fee_batches)",
        [&quote.payload.id], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?,
            row.get::<_, u64>(2)?, row.get::<_, u64>(3)?, row.get::<_, u64>(4)?))).unwrap();
    assert_eq!(paid_counts(), (0, 0, 0, 0, 0));
    stop.send(()).unwrap();
    server.await.unwrap();
    fixture.app = Router::new();
    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
    state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
    state.accepting.store(false, Ordering::Release);
    state.funding_ready.store(false, Ordering::Release);
    state.qualified_models.write().unwrap().clear();
    fixture.app = contracts::router(fixture.provider.state.clone());
    let (stop, server) = serve(fixture.app.clone());

    for phase in 1..=5 {
        rpc.phase.store(phase, Ordering::Release);
        rpc.timestamp.store(protocol::now().unwrap(), Ordering::Release);
        consumer = Consumer::new(config.clone()).unwrap();
        let start = exchanges.lock().unwrap().len();
        let rpc_start = rpc.requests.lock().unwrap().len();
        let result = if (phase + u64::from(ledger_wide)) % 2 == 0 {
            consumer.fund(&paid_offer, &quote).await
        } else {
            consumer.resume(&quote.payload.id).await
        };
        let status = if phase == 5 { StatusCode::OK }
            else if phase == 4 { StatusCode::BAD_REQUEST } else { StatusCode::CONFLICT };
        assert_exchange(&fixture.provider.state.ledger, start, "POST", OPEN,
            json!({"quote_id":quote.payload.id,"raw_transaction":raw}), status);
        let requests = rpc.requests.lock().unwrap();
        let calls = &requests[rpc_start..];
        let observations = if phase <= 2 { 1 } else { 2 };
        for method in ["eth_getTransactionReceipt", "eth_syncing"] {
            assert_eq!(calls.iter().filter(|(provider, request)| *provider && request["method"] == method).count(),
                observations, "provider phase {phase}: {method}");
        }
        for tag in ["safe", "latest", "0x8"] {
            let expected = if tag == "0x8" && phase <= 2 { 0 } else { observations };
            assert_eq!(calls.iter().filter(|(provider, request)| *provider
                && request["method"] == "eth_getBlockByNumber" && request["params"][0] == tag).count(), expected);
        }
        assert!(calls.iter().any(|(provider, request)| !provider && request["method"] == "eth_getTransactionReceipt"));
        assert!(calls.iter().all(|(_, request)| !matches!(request["method"].as_str(),
            Some("eth_getBalance" | "eth_getTransactionCount" | "eth_sendRawTransaction"))));
        drop(requests);
        assert_eq!(original(), original_before);
        assert_eq!(prior_records(), prior);
        if phase < 5 {
            let error = result.unwrap_err();
            assert!(error.starts_with(if phase == 4 { "funding_rejected:" } else { "funding_pending:" }), "{error}");
            assert!(error.ends_with(&format!("(HTTP {})", status.as_u16())), "{error}");
            assert_eq!(std::fs::read(&account_path).unwrap(), pending_bytes);
            fixture.unopened(&quote);
            assert_eq!(paid_counts(), (0, 0, 0, 0, 0));
            assert_eq!(counts(), (owned, total));
            let (terms, lease) = intent();
            assert_eq!(terms, retained.0);
            assert_eq!(lease, if phase == 4 { 0 } else { retained.1 });
            let attempt: (String, String) = db.query_row(
                "SELECT raw_transaction,json_extract(record,'$.status') FROM funding_attempts WHERE contract=?1",
                [&quote.payload.id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
            assert_eq!(attempt, (raw.clone(), if phase == 4 { "rejected" } else { "pending" }.into()));
        } else {
            let account = result.unwrap();
            assert_eq!(serde_json::to_value(&account).unwrap(),
                serde_json::to_value(fixture.provider.state.ledger.account(&quote.payload.id).unwrap()).unwrap());
            assert_eq!(account.price, quote.payload.price);
            assert_eq!(account.model, quote.payload.model);
            assert_eq!(account.max_concurrency, quote.payload.max_concurrency);
            let value = serde_json::to_value(&account).unwrap();
            for (field, amount) in [("funded_wei", "1000"), ("fee_wei", "5"), ("available_wei", "995"),
                ("spent_wei", "0"), ("reserved_wei", "0"), ("reversed_wei", "0"),
                ("provider_loss_wei", "0"), ("waived_wei", "0")] { assert_eq!(value[field], amount); }
            assert_eq!(value["usage"], serde_json::to_value(TokenUsage::default()).unwrap());
        }
    }
    assert_eq!(paid_counts(), (1, 1, 1, 1, 0));
    assert_eq!(counts(), (owned + 1, total + 1));
    assert_eq!(intent(), (retained.0, 0));
    assert_eq!(db.query_row("SELECT COUNT(*) FROM funding_attempts", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
    let provider_evidence: String = db.query_row("SELECT evidence FROM funding WHERE contract=?1",
        [&quote.payload.id], |row| row.get(0)).unwrap();
    let native_evidence: String = journal.query_row("SELECT evidence FROM transactions WHERE status='accepted'",
        [], |row| row.get(0)).unwrap();
    let evidence: Value = serde_json::from_str(&provider_evidence).unwrap();
    assert_eq!(evidence, serde_json::from_str::<Value>(&native_evidence).unwrap());
    assert_eq!(evidence["raw_transaction"], raw);
    assert_eq!(evidence["tx_hash"], envelope.tx_hash);
    assert_eq!(evidence["block_number"], 8);
    assert_eq!(evidence["block_hash"], format!("0x{:064x}", 8));
    let fee: (String, String, String) = db.query_row("SELECT platform,amount,status FROM fee_obligations",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(fee, (quote.payload.platform_wallet.clone(), "5".into(), "pending".into()));
    let credited_records = || [
        "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts WHERE id=?1",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials WHERE contract=?1 ORDER BY id",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding WHERE contract=?1",
        "SELECT json_array(f.tx_hash,platform,amount,created,f.status,batch) FROM fee_obligations f JOIN funding p ON p.tx_hash=f.tx_hash WHERE p.contract=?1",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents WHERE contract=?1",
    ].map(&fingerprint);
    let credited = credited_records();
    let saved_bytes = std::fs::read(&account_path).unwrap();
    let exported = consumer.credential_export(&quote.payload.id, None).unwrap();
    stop.send(()).unwrap();
    server.await.unwrap();
    fixture.app = Router::new();
    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
    state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
    fixture.app = contracts::router(fixture.provider.state.clone());
    let (stop, server) = serve(fixture.app.clone());
    // Direct opening replay is distinct from native saved-account GET recovery.
    // The selected free-capacity limit remains full and provider readiness closed.
    let body = serde_json::to_vec(&json!({"quote_id": quote.payload.id, "raw_transaction": raw})).unwrap();
    let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
    let rpc_start = rpc.requests.lock().unwrap().len();
    let account = serde_json::to_value(fixture.provider.state.ledger.account(&quote.payload.id).unwrap()).unwrap();
    let (status, replay) = fixture.send("POST", OPEN, &proof.headers, body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["account"], account);
    assert_eq!(replay.get("credential"), Some(&Value::Null));
    fixture.consumed(&proof);
    assert_eq!(rpc.requests.lock().unwrap().len(), rpc_start);
    assert_eq!(credited_records(), credited);
    assert_eq!(prior_records(), prior);
    assert_eq!(original(), original_before);
    assert_eq!(std::fs::read(&account_path).unwrap(), saved_bytes);
    assert_eq!(consumer.credential_export(&quote.payload.id, None).unwrap(), exported);
    for repeat_fund in [false, true] {
        consumer = Consumer::new(config.clone()).unwrap();
        rpc.timestamp.store(protocol::now().unwrap(), Ordering::Release);
        let start = exchanges.lock().unwrap().len();
        let rpc_start = rpc.requests.lock().unwrap().len();
        let account = if repeat_fund { consumer.fund(&paid_offer, &quote).await }
            else { consumer.resume(&quote.payload.id).await }.unwrap();
        assert_exchange(&fixture.provider.state.ledger, start, "GET",
            &format!("/v1/contracts/{}", quote.payload.id), Value::Null, StatusCode::OK);
        assert!(rpc.requests.lock().unwrap()[rpc_start..].iter().all(|(provider, _)| !provider),
            "saved-account GET recovery must not reopen or reverify credit at the provider");
        assert_eq!(std::fs::read(&account_path).unwrap(), saved_bytes);
        assert_eq!(consumer.credential_export(&quote.payload.id, None).unwrap(), exported);
        let authenticated = fixture.provider.state.ledger.authenticate(exported["api_key"].as_str().unwrap(), protocol::now().unwrap()).unwrap();
        assert_eq!(serde_json::to_value(authenticated.account).unwrap(), serde_json::to_value(account).unwrap());
        assert_eq!(authenticated.credential_id, exported["credential_id"].as_str().unwrap());
        assert_eq!(credited_records(), credited);
        assert_eq!(prior_records(), prior);
        assert_eq!(original(), original_before);
    }
    for retained_quote in [&quote, &unrelated, &refused] {
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.quote(&retained_quote.payload.id).unwrap()).unwrap(),
            serde_json::to_value(retained_quote).unwrap());
    }
    for account in &earlier {
        fixture.credentials_usable(&[account.credential.as_ref().unwrap()]);
        let (status, value) = fixture.send("GET", &format!("/v1/contracts/{}", account.account.contract_id),
            &bearer(account.credential.as_ref().unwrap()), Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value, serde_json::to_value(&account.account).unwrap());
    }
    assert_eq!(credited_records(), credited);
    assert_eq!(prior_records(), prior);
    assert_eq!(paid_counts(), (1, 1, 1, 1, 0));
    assert_eq!(journal.query_row("SELECT COUNT(*) FROM transactions", [], |row| row.get::<_, u64>(0)).unwrap(), 1);
    assert_eq!(journal.query_row("SELECT COUNT(*) FROM cancellations", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
    let provider_journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let provider_rows: (u64, u64) = provider_journal.query_row(
        "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(provider_rows, (0, 0));
    assert!(rpc.requests.lock().unwrap().iter().all(|(_, request)| request["method"] != "eth_sendRawTransaction"));
    stop.send(()).unwrap();
    server.await.unwrap();
    provider_rpc_server.abort();
    client_rpc_server.abort();
    assert!(provider_rpc_server.await.unwrap_err().is_cancelled());
    assert!(client_rpc_server.await.unwrap_err().is_cancelled());
}

fn restore_full_quote_storage(db: &mut Connection, pending: &Signed<Quote>, key: &str) {
    assert_eq!(db.query_row("SELECT COUNT(*) FROM quotes", [], |row| row.get::<_, u64>(0)).unwrap(), 1);
    let tx = db.transaction().unwrap();
    {
        let mut insert = tx.prepare(
            "INSERT INTO quotes(id,owner,expires,record) VALUES(?1,?2,?3,?4)").unwrap();
        for sequence in 1..10_000_u64 {
            let mut payload = pending.payload.clone();
            payload.id = format!("0x{:064x}", 0xcafe_0000 + sequence);
            payload.nonce = format!("0x{:064x}", 0xbead_0000 + sequence);
            payload.consumer_wallet = format!("0x{:040x}", 0xa0000 + sequence);
            let signed = protocol::sign(protocol::QUOTE_DOMAIN, payload, key).unwrap();
            insert.execute(params![signed.payload.id, signed.payload.consumer_wallet,
                signed.payload.expires_at, serde_json::to_string(&signed).unwrap()]).unwrap();
        }
    }
    tx.commit().unwrap();
}

struct TopupRecoveryRpc {
    quote: Quote,
    tx_hash: String,
    phase: AtomicU64,
    timestamp: AtomicU64,
    requests: Mutex<Vec<Value>>,
}

async fn topup_recovery_rpc(
    State(rpc): State<Arc<TopupRecoveryRpc>>, Json(request): Json<Value>,
) -> Json<Value> {
    rpc.requests.lock().unwrap().push(request.clone());
    let phase = rpc.phase.load(Ordering::Acquire);
    let args = &request["params"];
    let result = match request["method"].as_str().unwrap() {
        "eth_chainId" => json!(format!("0x{:x}", rpc.quote.network.chain_id)),
        "eth_syncing" => json!(false),
        "eth_getBlockByNumber" => {
            assert_eq!(args[1], false);
            let (height, hash) = match args[0].as_str().unwrap() {
                "0x0" => (0, rpc.quote.network.genesis_hash.clone()),
                "safe" => (9, format!("0x{:064x}", 9)),
                "latest" => (10, format!("0x{:064x}", 10)),
                "0x8" => (8, format!("0x{:064x}", 8)),
                tag => panic!("unexpected top-up block selector: {tag}"),
            };
            let timestamp = if height == 0 { 0 } else { rpc.timestamp.load(Ordering::Acquire) };
            json!({"number":format!("0x{height:x}"),"hash":hash,"timestamp":format!("0x{timestamp:x}")})
        }
        "eth_getTransactionReceipt" => {
            assert_eq!(args, &json!([rpc.tx_hash]));
            if phase == 0 { Value::Null } else {
                let height = if phase == 1 { 10 } else { 8 };
                json!({"transactionHash":rpc.tx_hash,"from":rpc.quote.consumer_wallet,
                    "to":rpc.quote.provider_wallet,"type":"0x0","gasUsed":"0x10000","status":"0x1",
                    "blockNumber":format!("0x{height:x}"),"blockHash":format!("0x{height:064x}")})
            }
        }
        method => panic!("unexpected top-up RPC (including fresh admission or broadcast): {method}"),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

async fn native_paid_intent_capacity_rpc(
    State(rpc): State<Arc<TopupRecoveryRpc>>, Json(request): Json<Value>,
) -> Json<Value> {
    let args = &request["params"];
    let result = match request["method"].as_str().unwrap() {
        "eth_getTransactionCount" => {
            assert_eq!(args[0], rpc.quote.consumer_wallet);
            assert!(matches!(args[1].as_str(), Some("latest" | "pending")));
            json!("0x1")
        }
        "eth_getBalance" => {
            assert_eq!(args, &json!([rpc.quote.consumer_wallet, "latest"]));
            json!("0x100000000")
        }
        "eth_chainId" | "eth_syncing" | "eth_getBlockByNumber" => {
            return topup_recovery_rpc(State(rpc), Json(request)).await;
        }
        method => panic!("unexpected RPC at native paid-intent admission: {method}"),
    };
    rpc.requests.lock().unwrap().push(request.clone());
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

#[tokio::test]
async fn paid_topup_recovers_at_full_quote_storage_without_duplicate_credit() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 56_000);
    let other = fixture.open_free().await;
    // Opening admission/credit is an explicit ledger prerequisite, not this test's chain evidence.
    let (opened, _) = fixture.paid_account().await;
    let id = &opened.account.contract_id;
    let original = fixture.provider.state.ledger.quote(id).unwrap();
    let (raw, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
    let path = format!("/v1/contracts/{id}/topup");
    let body = serde_json::to_vec(&json!({"raw_transaction":raw})).unwrap();
    let mut expected_account = serde_json::to_value(&opened.account).unwrap();
    assert_eq!(expected_account["funded_wei"], "100");
    assert_eq!(expected_account["fee_wei"], "0");
    assert_eq!(expected_account["available_wei"], "100");

    // One actual pending quote plus complete signed restored records isolate global storage,
    // without claiming 10,000 historical public issuances or exhausting committed issuance.
    let pending = fixture.quote("100").await;
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    restore_full_quote_storage(&mut db, &pending, &fixture.provider.state.private_key);
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
    let protected_state = || [
        "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(id,owner,quote,sequence,created) FROM accounts ORDER BY id",
    ].map(&fingerprint);
    let quote_rates = || fingerprint(
        "SELECT json_array(bucket,window,count) FROM rate_limits WHERE bucket LIKE 'quote:%' ORDER BY bucket");
    let monetary_state = || [
        "SELECT json_array(id,account) FROM accounts ORDER BY id",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash",
        "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash",
        "SELECT json_array(id,record) FROM fee_batches ORDER BY id",
    ].map(&fingerprint);
    let full_quotes = || {
        let count: u64 = db.query_row("SELECT COUNT(*) FROM quotes WHERE expires>?1",
            [protocol::now().unwrap()], |row| row.get(0)).unwrap();
        assert_eq!(count, 10_000, "quote storage must still be full during recovery");
    };
    let protected = protected_state();
    let rates_before_refusal = quote_rates();
    assert_eq!(protected.map(|row| row.0), [10_000, 2, 1, 2]);
    let before_credit = monetary_state();
    assert_eq!(before_credit.map(|row| row.0), [2, 1, 0, 0]);
    full_quotes();
    let rates = rate_records(&fixture.provider.ledger_path);
    assert!(rates.iter().filter(|(bucket, _, _)| bucket.starts_with("quote:"))
        .all(|(bucket, _, count)| bucket == "quote:global" && *count < 600));
    let (status, refused) = fixture.post("/v1/contracts/propose", json!({
        "offer_id":fixture.provider.offer_id,
        "consumer_wallet":wallet::address_from_private_key(OTHER_KEY).unwrap(),"funding_wei":"100"
    })).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
    assert_eq!(refused["error"]["code"], "rate_limit");
    assert_eq!(protected_state(), protected);
    assert_eq!(quote_rates(), rates_before_refusal);
    assert_eq!(monetary_state(), before_credit);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rpc_url = format!("http://{}", listener.local_addr().unwrap());
    let rpc = Arc::new(TopupRecoveryRpc {
        quote: original.payload.clone(), tx_hash: envelope.tx_hash.clone(),
        phase: AtomicU64::new(0), timestamp: AtomicU64::new(protocol::now().unwrap()),
        requests: Mutex::new(Vec::new()),
    });
    let rpc_app = Router::new().route("/", post(topup_recovery_rpc)).with_state(rpc.clone());
    let server = tokio::spawn(async move { axum::serve(listener, rpc_app).await.unwrap(); });
    let mut credited = None;
    for (attempt, phase) in [0, 1, 2, 2].into_iter().enumerate() {
        // Reopen the actual store, chain client and router, including before the accepted retry.
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
        rpc.timestamp.store(protocol::now().unwrap(), Ordering::Release);
        full_quotes();
        if attempt == 0 {
            for headers in [HeaderMap::new(), bearer(opened.credential.as_ref().unwrap())] {
                denied(fixture.send("POST", &path, &headers, body.clone()).await, StatusCode::UNAUTHORIZED);
            }
            let wrong = fixture.proof(OTHER_KEY, "POST", &path, &body).await;
            denied(fixture.send("POST", &path, &wrong.headers, body.clone()).await, StatusCode::FORBIDDEN);
            fixture.consumed(&wrong);
            assert!(rpc.requests.lock().unwrap().is_empty());
            assert_eq!(monetary_state(), before_credit);
        }
        let start = rpc.requests.lock().unwrap().len();
        let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
        // Challenge housekeeping may expire old minute rows; top-up itself must not debit,
        // replenish or rewrite the quote counters present after obtaining authority.
        let rates_before_topup = quote_rates();
        let (status, document) = fixture.send("POST", &path, &proof.headers, body.clone()).await;
        fixture.consumed(&proof);
        let requests = rpc.requests.lock().unwrap();
        let calls = &requests[start..];
        let observations = if phase < 2 { 1 } else { 2 };
        for method in ["eth_getTransactionReceipt", "eth_syncing"] {
            assert_eq!(calls.iter().filter(|call| call["method"] == method).count(), observations);
        }
        for tag in ["safe", "latest", "0x8"] {
            assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBlockByNumber"
                && call["params"][0] == tag).count(), if tag == "0x8" && phase < 2 { 0 } else { observations });
        }
        drop(requests);
        // Missing receipts and unsafe execution must not change accepted balances or create fees.
        if phase < 2 {
            assert_eq!(status, StatusCode::CONFLICT, "{document}");
            assert_eq!(document["error"]["code"], "funding_pending");
            assert_eq!(monetary_state(), before_credit);
            let pending_attempt: (String, String, String) = db.query_row(
                "SELECT tx_hash,raw_transaction,json_extract(record,'$.status') FROM funding_attempts",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
            assert_eq!(pending_attempt, (envelope.tx_hash.clone(), raw.clone(), "pending".into()));
        } else {
            assert_eq!(status, StatusCode::OK, "{document}");
            expected_account["funded_wei"] = json!("1100");
            expected_account["fee_wei"] = json!("5");
            expected_account["available_wei"] = json!("1095");
            assert_eq!(document, expected_account, "only the exact top-up and funding fee change");
            let current = monetary_state();
            assert_eq!(current.map(|row| row.0), [2, 2, 1, 0]);
            if let Some(previous) = credited {
                assert_eq!(current, previous, "reopened retry cannot rewrite or duplicate accepted money records");
            } else {
                credited = Some(current);
            }
            assert_eq!(db.query_row("SELECT COUNT(*) FROM funding_attempts", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
            let evidence: String = db.query_row("SELECT evidence FROM funding WHERE tx_hash=?1",
                [&envelope.tx_hash], |row| row.get(0)).unwrap();
            // Expected evidence is compared after production verification, never injected as credit.
            let expected = VerifiedPayment {
                network: original.payload.network.clone(), tx_hash: envelope.tx_hash.clone(),
                from: envelope.from.clone(), to: envelope.to.clone(), value_wei: "1000".into(),
                data: envelope.data.clone(), block_hash: format!("0x{:064x}", 8),
                block_number: 8, raw_transaction: raw.clone(),
            };
            assert_eq!(serde_json::from_str::<Value>(&evidence).unwrap(), serde_json::to_value(expected).unwrap());
            let fee: (String, String, String) = db.query_row("SELECT platform,amount,status FROM fee_obligations",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
            assert_eq!(fee, (original.payload.platform_wallet.clone(), "5".into(), "pending".into()));
        }
        assert_eq!(protected_state(), protected);
        assert_eq!(quote_rates(), rates_before_topup);
        full_quotes();
        // Replaying a consumed proof cannot reach RPC, even after successful settlement.
        let rpc_count = rpc.requests.lock().unwrap().len();
        denied(fixture.send("POST", &path, &proof.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
        assert_eq!(rpc.requests.lock().unwrap().len(), rpc_count);
        for (credential, account) in [
            (opened.credential.as_ref().unwrap(), expected_account.clone()),
            (other.credential.as_ref().unwrap(), serde_json::to_value(&other.account).unwrap()),
        ] {
            let (status, value) = fixture.send("GET", &format!("/v1/contracts/{}", credential.contract_id),
                &bearer(credential), Vec::new()).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value, account);
        }
        assert_eq!(protected_state(), protected);
        assert_eq!(quote_rates(), rates_before_topup);
        assert_eq!(monetary_state(), credited.unwrap_or(before_credit));
    }
    let journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let operations: (u64, u64) = journal.query_row(
        "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(operations, (0, 0));
    assert_eq!(db.query_row("SELECT COUNT(*) FROM requests", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn paid_topup_recovers_under_each_quote_budget_with_spend_and_hold_intact() {
    let directory = tempfile::tempdir().unwrap();
    for (case, exhausted_bucket) in ["quote-work:global", "quote:global"].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 73_000 + case as u64);
        let unrelated = fixture.open_free().await;
        // Opening credit and supplied prior usage are ledger prerequisites,
        // not live-chain or engine evidence for this top-up.
        let (opened, _) = fixture.paid_account().await;
        let id = &opened.account.contract_id;
        let root = opened.credential.as_ref().unwrap();
        let read = fixture.scoped_credential(id, "read");
        let quote = fixture.provider.state.ledger.quote(id).unwrap();
        let pending = fixture.quote("100").await;
        let now = protocol::now().unwrap();
        let ledger = &fixture.provider.state.ledger;
        let spent = ledger.reserve(id, &root.id, "spent-before-topup", 1, 1, now, &opened.account.model).unwrap();
        ledger.settle(&spent.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", now).unwrap();
        let held = ledger.reserve(id, &root.id, "held-through-topup", 1000, 512, now, &opened.account.model).unwrap();
        let mut expected_account = serde_json::to_value(ledger.account(id).unwrap()).unwrap();
        assert_eq!(expected_account["funded_wei"], "100");
        assert_eq!(expected_account["fee_wei"], "0");
        assert_eq!(expected_account["spent_wei"], "1");
        assert_eq!(expected_account["reserved_wei"], "3");
        assert_eq!(expected_account["available_wei"], "96");
        let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
        // Restore one exhausted resource, not 600 observed public issuances.
        // Keep both logical minutes ahead without changing any signed timestamp.
        let minute = now / 60 + 10;
        let tx = db.transaction().unwrap();
        for (bucket, watermark) in [
            ("quote:global", "quote_committed_minute"),
            ("quote-work:global", "quote_work_minute"),
        ] {
            assert_eq!(tx.execute(
                "UPDATE rate_limits SET window=?1,count=CASE WHEN bucket=?2 THEN 600 ELSE count END WHERE bucket=?3",
                params![minute, exhausted_bucket, bucket]).unwrap(), 1);
            assert_eq!(tx.execute("UPDATE metadata SET value=?1 WHERE key=?2", params![minute, watermark]).unwrap(), 1);
        }
        tx.commit().unwrap();
        let budgets = || {
            let mut statement = db.prepare(
                "SELECT json_array(bucket,window,count) FROM rate_limits
                 WHERE bucket IN ('quote:global','quote-work:global') ORDER BY bucket").unwrap();
            let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap()
                .collect::<rusqlite::Result<Vec<_>>>().unwrap();
            rows
        };
        let mut before_refusal = ledger_snapshot(&db);
        assert_eq!(before_refusal[3].len(), 1, "quote storage is not the exhausted resource");
        for bucket in ["quote:global", "quote-work:global"] {
            let row = before_refusal[9].iter().find(|row| row[0] == bucket).unwrap();
            let count = row[2].as_u64().unwrap();
            if bucket == exhausted_bucket { assert_eq!(count, 600); }
            else { assert!((1..600).contains(&count)); }
        }
        let (status, document) = fixture.post("/v1/contracts/propose", json!({
            "offer_id":fixture.provider.offer_id,
            "consumer_wallet":opened.account.consumer_wallet,"funding_wei":"100"
        })).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{document}");
        assert_eq!(document["error"]["code"], "rate_limit");
        if exhausted_bucket == "quote:global" {
            // Committed-issuance refusal cannot refund the independently spent
            // signing permit; work exhaustion itself spends no extra permit.
            let work = before_refusal[9].iter_mut().find(|row| row[0] == "quote-work:global").unwrap();
            work[2] = json!(work[2].as_u64().unwrap() + 1);
        }
        assert_eq!(ledger_snapshot(&db), before_refusal);
        let exhausted = budgets();
        let (raw, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
        let path = format!("/v1/contracts/{id}/topup");
        let body = serde_json::to_vec(&protocol::TopupRequest { raw_transaction: raw.clone() }).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let rpc = Arc::new(TopupRecoveryRpc {
            quote: quote.payload.clone(), tx_hash: envelope.tx_hash.clone(),
            phase: AtomicU64::new(0), timestamp: AtomicU64::new(protocol::now().unwrap()),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new().route("/", post(topup_recovery_rpc)).with_state(rpc.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let mut credited = false;
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
            rpc.timestamp.store(protocol::now().unwrap(), Ordering::Release);
            assert_eq!(budgets(), exhausted);
            if attempt == 0 {
                let before = ledger_snapshot(&db);
                for headers in [HeaderMap::new(), bearer(root), bearer(&read)] {
                    denied(fixture.send("POST", &path, &headers, body.clone()).await, StatusCode::UNAUTHORIZED);
                    assert_eq!(ledger_snapshot(&db), before);
                }
                let wrong = fixture.proof(OTHER_KEY, "POST", &path, &body).await;
                let mut expected = ledger_snapshot(&db);
                denied(fixture.send("POST", &path, &wrong.headers, body.clone()).await, StatusCode::FORBIDDEN);
                fixture.consumed(&wrong);
                expected[11].retain(|row| row[0].as_str() != Some(wrong.nonce.as_str()));
                assert_eq!(ledger_snapshot(&db), expected);
                assert!(rpc.requests.lock().unwrap().is_empty());
            }
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
            let mut expected = ledger_snapshot(&db);
            let start = rpc.requests.lock().unwrap().len();
            let (status, document) = fixture.send("POST", &path, &proof.headers, body.clone()).await;
            fixture.consumed(&proof);
            expected[11].retain(|row| row[0].as_str() != Some(proof.nonce.as_str()));
            let after = ledger_snapshot(&db);
            {
                let calls = rpc.requests.lock().unwrap();
                let calls = &calls[start..];
                let observations = if phase < 2 { 1 } else { 2 };
                for method in ["eth_getTransactionReceipt", "eth_syncing"] {
                    assert_eq!(calls.iter().filter(|call| call["method"] == method).count(), observations);
                }
                for tag in ["safe", "latest", "0x8"] {
                    assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBlockByNumber"
                        && call["params"][0] == tag).count(), if phase < 2 && tag == "0x8" { 0 } else { observations });
                }
            }
            if phase < 2 {
                assert_eq!(status, StatusCode::CONFLICT, "{document}");
                assert_eq!(document["error"]["code"], "funding_pending");
                assert_eq!(after[5].len(), 1);
                let record = &after[5][0];
                assert_eq!(record[0], envelope.tx_hash);
                assert_eq!(record[1], *id);
                assert_eq!(record[3], raw);
                assert_eq!(record[2]["status"], "pending");
                assert_eq!(record[2]["value_wei"], Value::Null);
                assert_eq!(record[2]["fee_wei"], Value::Null);
                if let Some(previous) = &pending_record { assert_eq!(&record[2], previous); }
                else { pending_record = Some(record[2].clone()); }
                expected[5] = after[5].clone();
            } else {
                assert_eq!(status, StatusCode::OK, "{document}");
                expected_account["funded_wei"] = json!("1100");
                expected_account["fee_wei"] = json!("5");
                expected_account["available_wei"] = json!("1091");
                assert_eq!(document, expected_account);
                if !credited {
                    expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3] = expected_account.clone();
                    assert_eq!(after[4].len(), expected[4].len() + 1);
                    let funding = after[4].iter().find(|row| row[0] == envelope.tx_hash).unwrap();
                    assert_eq!(funding[1], *id);
                    assert_eq!(funding[2]["tx_hash"], envelope.tx_hash);
                    assert_eq!(funding[2]["contract_id"], *id);
                    assert_eq!(funding[2]["status"], "accepted");
                    assert_eq!(funding[2]["value_wei"], "1000");
                    assert_eq!(funding[2]["fee_wei"], "5");
                    assert!(funding[2]["created_at"].as_u64().is_some());
                    assert_eq!(funding[4], "accepted");
                    assert_eq!(funding[5], funding[2]["created_at"]);
                    let evidence = VerifiedPayment {
                        network: quote.payload.network.clone(), tx_hash: envelope.tx_hash.clone(),
                        from: envelope.from.clone(), to: envelope.to.clone(), value_wei: "1000".into(),
                        data: envelope.data.clone(), block_hash: format!("0x{:064x}", 8),
                        block_number: 8, raw_transaction: raw.clone(),
                    };
                    assert_eq!(funding[3], serde_json::to_value(evidence).unwrap());
                    expected[4].push(funding.clone());
                    expected[4].sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
                    assert!(expected[7].is_empty());
                    expected[7].push(json!([envelope.tx_hash, quote.payload.platform_wallet, "5",
                        funding[2]["created_at"], "pending", null]));
                    expected[5].clear();
                    credited = true;
                }
            }
            assert_eq!(after, expected, "{exhausted_bucket}, recovery phase {attempt}");
            assert_eq!(budgets(), exhausted);
            assert!(!fixture.provider.state.accepting.load(Ordering::Acquire));
            assert!(!fixture.provider.state.funding_ready.load(Ordering::Acquire));
            assert!(fixture.provider.state.qualified_models.read().unwrap().is_empty());
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger.quote(&pending.payload.id).unwrap()).unwrap(),
                serde_json::to_value(&pending).unwrap());
            assert_eq!(fixture.provider.state.ledger.request_view(id, &held.id).unwrap().status, "reserved");
            let calls = rpc.requests.lock().unwrap().len();
            denied(fixture.send("POST", &path, &proof.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
            assert_eq!(rpc.requests.lock().unwrap().len(), calls);
            for (credential, account) in [
                (root, expected_account.clone()), (&read, expected_account.clone()),
                (unrelated.credential.as_ref().unwrap(), serde_json::to_value(&unrelated.account).unwrap()),
            ] {
                let (status, document) = fixture.send("GET", &format!("/v1/contracts/{}", credential.contract_id),
                    &bearer(credential), Vec::new()).await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(document, account);
            }
            assert_eq!(ledger_snapshot(&db), after);
        }
        // Recovery neither releases the running hold nor spends it. Its original
        // terminal operation remains usable after credit and closed admission.
        let settled = fixture.provider.state.ledger.settle(&held.id,
            &TokenUsage { prompt_tokens: 1000, completion_tokens: 512 }, "completed", protocol::now().unwrap()).unwrap();
        assert_eq!(settled.cost_wei, "3");
        expected_account["spent_wei"] = json!("4");
        expected_account["reserved_wei"] = json!("0");
        expected_account["usage"] = json!({"prompt_tokens": 1001, "completion_tokens": 513});
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.account(id).unwrap()).unwrap(), expected_account);
        let (status, document) = fixture.send("GET", &format!("/v1/contracts/{id}/requests/{}", held.id),
            &bearer(&read), Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(document, serde_json::to_value(settled).unwrap());
        assert_eq!(budgets(), exhausted);
        fixture.credentials_usable(&[root, &read, unrelated.credential.as_ref().unwrap()]);
        let journal = Connection::open(fixture.provider.state.config.network
            .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
        let operations: (u64, u64) = journal.query_row(
            "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)",
            [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(operations, (0, 0));
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn paid_topup_recovers_at_each_free_opening_limit() {
    let directory = tempfile::tempdir().unwrap();
    for ledger_wide in [false, true] {
        let mut fixture = Fixture::new(directory.path(), if ledger_wide { 74_001 } else { 74_000 });
        let mut free_offer = fixture.provider.offer.payload.clone();
        let seed = fixture.open_free().await;
        // Give the free-eligibility probe a different real owner. The paid owner
        // needs seven setup, one refusal and four recovery challenges: exactly
        // twelve, without relying on a minute rollover to replenish its budget.
        let (status, document) = fixture.post("/v1/contracts/propose", json!({
            "offer_id":fixture.provider.offer_id,
            "consumer_wallet":wallet::address_from_private_key(OTHER_KEY).unwrap(),"funding_wei":"0"
        })).await;
        assert_eq!(status, StatusCode::OK, "{document}");
        let other_quote: Signed<Quote> = serde_json::from_value(document).unwrap();
        let other_body = serde_json::to_vec(&json!({"quote_id":other_quote.payload.id,"raw_transaction":null})).unwrap();
        let other_proof = fixture.proof(OTHER_KEY, "POST", OPEN, &other_body).await;
        let (status, document) = fixture.send("POST", OPEN, &other_proof.headers, other_body).await;
        assert_eq!(status, StatusCode::OK, "{document}");
        let other: OpenResult = serde_json::from_value(document).unwrap();
        // Paid opening and supplied prior usage are post-verification ledger
        // prerequisites, not the chain/engine observations under test here.
        let (opened, _) = fixture.paid_account().await;
        let id = &opened.account.contract_id;
        let root = opened.credential.as_ref().unwrap();
        let read = fixture.scoped_credential(id, "read");
        let original = fixture.provider.state.ledger.quote(id).unwrap();
        let now = protocol::now().unwrap();
        let ledger = &fixture.provider.state.ledger;
        let spent = ledger.reserve(id, &root.id, "spent-before-capacity", 1, 1, now, &opened.account.model).unwrap();
        ledger.settle(&spent.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", now).unwrap();
        let held = ledger.reserve(id, &root.id, "held-at-capacity", 1000, 512, now, &opened.account.model).unwrap();
        let mut expected_account = serde_json::to_value(ledger.account(id).unwrap()).unwrap();
        assert_eq!(expected_account["spent_wei"], "1");
        assert_eq!(expected_account["reserved_wei"], "3");
        assert_eq!(expected_account["available_wei"], "96");

        free_offer.revision = fixture.provider.offer.payload.revision + 1;
        fixture.provider.offer = protocol::sign(protocol::OFFER_DOMAIN, free_offer, &fixture.provider.state.private_key).unwrap();
        fixture.provider.offer_id = fixture.provider.state.offers.insert(fixture.provider.offer.clone()).unwrap();
        let mut earlier = vec![other, seed];
        let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
        db.pragma_update(None, "foreign_keys", true).unwrap();
        if ledger_wide {
            // Complete signed account/root records, not 9,997 observed HTTP openings.
            // Distinct restored owners keep the owned-account bound unexhausted.
            let restored = restore_free_accounts(&mut db, &fixture, &earlier[0], 9_997, None);
            earlier.extend(restored);
        } else {
            for _ in 0..6 { earlier.push(fixture.open_free().await); }
        }
        // Issue the control after restoration so its signed lease is fresh.
        let pending = fixture.quote("0").await;
        let free_body = serde_json::to_vec(&json!({"quote_id":pending.payload.id,"raw_transaction":null})).unwrap();
        let assert_capacity = || {
            let counts: (u64, u64, u64, u64) = db.query_row(
                "SELECT (SELECT COUNT(*) FROM accounts WHERE owner=?1),
                        (SELECT COUNT(*) FROM accounts),(SELECT COUNT(*) FROM quotes),
                        (SELECT COUNT(*) FROM funding_intents WHERE admission_until>?2
                         AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract))",
                params![opened.account.consumer_wallet, protocol::now().unwrap()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
            assert_eq!(counts, if ledger_wide { (2, 10_000, 1, 0) } else { (8, 9, 1, 0) });
            if ledger_wide {
                assert_eq!(db.query_row(
                    "SELECT MAX(owned) FROM (SELECT COUNT(*) AS owned FROM accounts GROUP BY owner)",
                    [], |row| row.get::<_, u64>(0)).unwrap(), 2);
            }
            let budgets: Vec<(String, u64)> = db.prepare(
                "SELECT bucket,count FROM rate_limits WHERE bucket IN ('quote:global','quote-work:global') ORDER BY bucket")
                .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
                .collect::<rusqlite::Result<_>>().unwrap();
            assert_eq!(budgets.len(), 2);
            assert!(budgets.iter().all(|(_, count)| (1..600).contains(count)));
        };
        // Stream every protected row instead of retaining 10,000 account documents.
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
            fingerprint("SELECT json_array(id,account) FROM accounts WHERE id<>?1 ORDER BY id", &[id]));
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
        assert_capacity();
        let protected = protected_state();
        let before_credit = monetary_state();
        let no_attempts = attempts();
        assert_eq!(before_credit.map(|row| row.0), [if ledger_wide { 10_000 } else { 9 }, 1, 0]);
        assert_eq!(no_attempts.0, 0);

        let (raw, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
        let path = format!("/v1/contracts/{id}/topup");
        let body = serde_json::to_vec(&protocol::TopupRequest { raw_transaction: raw.clone() }).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let rpc = Arc::new(TopupRecoveryRpc {
            quote: original.payload.clone(), tx_hash: envelope.tx_hash.clone(),
            phase: AtomicU64::new(0), timestamp: AtomicU64::new(protocol::now().unwrap()),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new().route("/", post(topup_recovery_rpc)).with_state(rpc.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        fixture.app = Router::new();
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        Arc::get_mut(&mut state.config).unwrap().network.rpc_url = rpc_url.clone();
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
        fixture.app = contracts::router(fixture.provider.state.clone());

        // Refuse an otherwise eligible, freshly authorized free opening before
        // closing readiness. The selected capacity bound must be the cause.
        let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &free_body).await;
        let remaining = challenges(&proof.nonce);
        let before_rates = rates();
        let (status, document) = fixture.send("POST", OPEN, &proof.headers, free_body).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{document}");
        assert_eq!(document["error"]["code"], "rate_limit");
        fixture.consumed(&proof);
        assert_eq!(challenges(""), remaining);
        fixture.unopened(&pending);
        assert_eq!(rates(), before_rates);
        assert_eq!(protected_state(), protected);
        assert_eq!(monetary_state(), before_credit);
        assert_eq!(attempts(), no_attempts);
        assert!(rpc.requests.lock().unwrap().is_empty());

        let mut credited = None;
        let mut pending_record = None;
        for (attempt, phase) in [0, 1, 2, 2].into_iter().enumerate() {
            fixture.app = Router::new();
            let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
            state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
            state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
            state.accepting.store(false, Ordering::Release);
            state.funding_ready.store(false, Ordering::Release);
            state.qualified_models.write().unwrap().clear();
            fixture.app = contracts::router(fixture.provider.state.clone());
            rpc.phase.store(phase, Ordering::Release);
            rpc.timestamp.store(protocol::now().unwrap(), Ordering::Release);
            assert_capacity();
            if attempt == 0 {
                let before_challenges = challenges("");
                let before_rates = rates();
                for headers in [HeaderMap::new(), bearer(root), bearer(&read)] {
                    denied(fixture.send("POST", &path, &headers, body.clone()).await, StatusCode::UNAUTHORIZED);
                }
                assert_eq!(challenges(""), before_challenges);
                assert_eq!(rates(), before_rates);
                let wrong = fixture.proof(OTHER_KEY, "POST", &path, &body).await;
                let remaining = challenges(&wrong.nonce);
                let before_rates = rates();
                denied(fixture.send("POST", &path, &wrong.headers, body.clone()).await, StatusCode::FORBIDDEN);
                fixture.consumed(&wrong);
                assert_eq!(challenges(""), remaining);
                assert_eq!(rates(), before_rates);
                // Full free-opening capacity cannot make an existing free account
                // eligible for paid top-up, even with its real owner's proof.
                let free_path = format!("/v1/contracts/{}/topup", earlier[0].account.contract_id);
                let (free_raw, _) = fixture.signed_payment_for(OTHER_KEY,
                    &protocol::topup_data(&earlier[0].account.contract_id).unwrap(), 1000, 2);
                let free_topup = serde_json::to_vec(&protocol::TopupRequest { raw_transaction: free_raw }).unwrap();
                let proof = fixture.proof(OTHER_KEY, "POST", &free_path, &free_topup).await;
                let remaining = challenges(&proof.nonce);
                let before_rates = rates();
                let (status, document) = fixture.send("POST", &free_path, &proof.headers, free_topup).await;
                assert_eq!(status, StatusCode::CONFLICT, "{document}");
                assert_eq!(document["error"]["code"], "account_frozen");
                fixture.consumed(&proof);
                assert_eq!(challenges(""), remaining);
                assert_eq!(rates(), before_rates);
                assert_eq!(protected_state(), protected);
                assert_eq!(monetary_state(), before_credit);
                assert_eq!(attempts(), no_attempts);
                assert!(rpc.requests.lock().unwrap().is_empty());
            }
            let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
            let remaining = challenges(&proof.nonce);
            let before_rates = rates();
            let start = rpc.requests.lock().unwrap().len();
            let (status, document) = fixture.send("POST", &path, &proof.headers, body.clone()).await;
            fixture.consumed(&proof);
            assert_eq!(challenges(""), remaining);
            {
                let calls = rpc.requests.lock().unwrap();
                let calls = &calls[start..];
                let observations = if phase < 2 { 1 } else { 2 };
                for method in ["eth_getTransactionReceipt", "eth_syncing"] {
                    assert_eq!(calls.iter().filter(|call| call["method"] == method).count(), observations);
                }
                for tag in ["safe", "latest", "0x8"] {
                    assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBlockByNumber"
                        && call["params"][0] == tag).count(), if phase < 2 && tag == "0x8" { 0 } else { observations });
                }
            }
            if phase < 2 {
                assert_eq!(status, StatusCode::CONFLICT, "{document}");
                assert_eq!(document["error"]["code"], "funding_pending");
                assert_eq!(monetary_state(), before_credit);
                assert_eq!(attempts().0, 1);
                let (hash, contract, stored_raw, record): (String, String, String, String) = db.query_row(
                    "SELECT tx_hash,contract,raw_transaction,record FROM funding_attempts", [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
                assert_eq!((hash, contract, stored_raw), (envelope.tx_hash.clone(), id.clone(), raw.clone()));
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
                assert_eq!(current.map(|row| row.0), [if ledger_wide { 10_000 } else { 9 }, 2, 1]);
                if let Some(previous) = credited { assert_eq!(current, previous); }
                else { credited = Some(current); }
                assert_eq!(attempts(), no_attempts);
                let evidence: String = db.query_row("SELECT evidence FROM funding WHERE tx_hash=?1",
                    [&envelope.tx_hash], |row| row.get(0)).unwrap();
                let expected = VerifiedPayment {
                    network: original.payload.network.clone(), tx_hash: envelope.tx_hash.clone(),
                    from: envelope.from.clone(), to: envelope.to.clone(), value_wei: "1000".into(),
                    data: envelope.data.clone(), block_hash: format!("0x{:064x}", 8),
                    block_number: 8, raw_transaction: raw.clone(),
                };
                assert_eq!(serde_json::from_str::<Value>(&evidence).unwrap(), serde_json::to_value(expected).unwrap());
                let fee: (String, String, String, String) = db.query_row(
                    "SELECT tx_hash,platform,amount,status FROM fee_obligations", [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
                assert_eq!(fee, (envelope.tx_hash.clone(), original.payload.platform_wallet.clone(), "5".into(), "pending".into()));
            }
            assert_eq!(protected_state(), protected);
            assert_eq!(rates(), before_rates);
            assert_capacity();
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger.account(id).unwrap()).unwrap(), expected_account);
            assert_eq!(fixture.provider.state.ledger.request_view(id, &held.id).unwrap().status, "reserved");
            assert!(!fixture.provider.state.accepting.load(Ordering::Acquire));
            assert!(!fixture.provider.state.funding_ready.load(Ordering::Acquire));
            assert!(fixture.provider.state.qualified_models.read().unwrap().is_empty());
            let current_attempts = attempts();
            let calls = rpc.requests.lock().unwrap().len();
            denied(fixture.send("POST", &path, &proof.headers, body.clone()).await, StatusCode::UNAUTHORIZED);
            assert_eq!(rpc.requests.lock().unwrap().len(), calls);
            for (credential, account) in [(root, expected_account.clone()), (&read, expected_account.clone())]
                .into_iter().chain(earlier.iter().map(|earlier|
                    (earlier.credential.as_ref().unwrap(), serde_json::to_value(&earlier.account).unwrap()))) {
                let (status, document) = fixture.send("GET", &format!("/v1/contracts/{}", credential.contract_id),
                    &bearer(credential), Vec::new()).await;
                assert_eq!(status, StatusCode::OK, "{document}");
                assert_eq!(document, account);
            }
            assert_eq!(protected_state(), protected);
            assert_eq!(monetary_state(), credited.unwrap_or(before_credit));
            assert_eq!(attempts(), current_attempts);
            assert_eq!(rates(), before_rates);
            assert_eq!(challenges(""), remaining);
        }
        let journal = Connection::open(fixture.provider.state.config.network
            .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
        let operations: (u64, u64) = journal.query_row(
            "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)", [],
            |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(operations, (0, 0));
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn native_topup_recovery_refuses_missing_or_different_original_journal() {
    use std::io::Write;

    let directory = tempfile::tempdir().unwrap();
    for (condition, missing) in [true, false].into_iter().enumerate() {
        for (entry, method) in ["resume", "topup", "fund"].into_iter().enumerate() {
            let case = 57_000 + (condition * 3 + entry) as u64;
            let mut fixture = Fixture::new(directory.path(), case);
            // Complete paid-opening state is restored setup, not observed chain execution.
            // The native refresh below validates that this is a usable saved agreement.
            let (opened, opening_raw) = fixture.paid_account().await;
            let id = &opened.account.contract_id;
            let quote = fixture.provider.state.ledger.quote(id).unwrap();
            let ack = fixture.provider.state.ledger.funding_intent(&quote.payload, &opening_raw)
                .unwrap().unwrap();
            let (raw, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
            let (different_raw, different) = fixture.signed_payment(
                &protocol::topup_data(id).unwrap(), 1000, 2);
            assert_ne!(raw, different_raw);
            assert_eq!(envelope.from, different.from);
            assert_eq!(envelope.to, different.to);
            assert_eq!(envelope.value_wei, different.value_wei);
            assert_eq!(envelope.data, different.data);
            let intent_id = uuid::Uuid::new_v4().to_string();
            let operation = format!("topup:{id}:{intent_id}");

            let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
            let observed_rpc = requests.clone();
            let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let rpc_url = format!("http://{}", rpc_listener.local_addr().unwrap());
            let rpc_app = Router::new().route("/", post(move |Json(request): Json<Value>| {
                let observed = observed_rpc.clone();
                async move {
                    observed.lock().unwrap().push(request);
                    StatusCode::SERVICE_UNAVAILABLE
                }
            }));
            let rpc_server = tokio::spawn(async move { axum::serve(rpc_listener, rpc_app).await.unwrap(); });
            let controls = Arc::new(Mutex::new(Vec::new()));
            let observed_controls = controls.clone();
            let app = fixture.app.clone().layer(from_fn(move |request: Request, next: Next| {
                let observed = observed_controls.clone();
                async move {
                    let method = request.method().to_string();
                    let path = request.uri().path().to_owned();
                    let response = next.run(request).await;
                    observed.lock().unwrap().push((method, path, response.status()));
                    response
                }
            }));
            let listener = fixture._listeners[0].try_clone().unwrap();
            listener.set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let provider_server = tokio::spawn(async move {
                axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.unwrap(); })
                    .await.unwrap();
            });
            let mut network = fixture.provider.state.config.network.clone();
            network.rpc_url = rpc_url;
            let config = ConsumerConfig {
                network, data_dir: directory.path().join(format!("consumer-{case}")),
                catalog_url: None, development: true, dht: None,
            };
            let mut consumer = Consumer::new(config.clone()).unwrap();
            let import = config.data_dir.join("public-test-scalar.key");
            wallet::save_private_key(&import, OWNER_KEY).unwrap();
            assert_eq!(consumer.wallet_import_file(&import).unwrap(), quote.payload.consumer_wallet);
            let scoped = config.network.scoped_dir(&config.data_dir);
            let account_path = scoped.join("consumer").join(&quote.payload.consumer_wallet)
                .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
            wallet::secure_directory(account_path.parent().unwrap()).unwrap();
            let saved = json!({
                "offer":fixture.provider.offer,"quote":quote,"account":opened.account,
                "raw_transaction":opening_raw,"tx_hash":ack.payload.tx_hash,"funding_intent":ack,
                "funding_broadcast_started":true,"funding_cancellation_requested":false,
                "funding_cancellation":null,"credentials":[opened.credential.as_ref().unwrap()],
                "active_credential":opened.credential.as_ref().unwrap().id,"receipt_sequence":0,
                "topup":{"id":intent_id,"amount_wei":"1000","raw_transaction":raw}
            });
            let mut file = wallet::create_secret_file(&account_path).unwrap();
            file.write_all(&serde_json::to_vec(&saved).unwrap()).unwrap();
            file.sync_all().unwrap();
            drop(file);
            let provider_db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let opening_evidence: String = provider_db.query_row(
                "SELECT evidence FROM funding WHERE tx_hash=?1",
                [&ack.payload.tx_hash], |row| row.get(0)).unwrap();
            let journal = Connection::open(scoped.join("wallet-journal/transactions.sqlite3")).unwrap();
            journal.execute(
                "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,evidence,created_at)
                 VALUES(?1,?2,'0',?3,?4,'accepted',?5,?6)",
                params![quote.payload.consumer_wallet, format!("open:{id}"), ack.payload.tx_hash,
                    opening_raw, opening_evidence, quote.payload.issued_at]).unwrap();
            // This read really authenticates against the provider; it neither resumes funding
            // nor replaces the pending top-up. Exported authority must remain usable throughout.
            assert_eq!(serde_json::to_value(consumer.account(id).await.unwrap()).unwrap(),
                serde_json::to_value(&opened.account).unwrap());
            assert_eq!(*controls.lock().unwrap(), [
                ("POST".into(), "/v1/auth/challenge".into(), StatusCode::OK),
                ("GET".into(), format!("/v1/contracts/{id}"), StatusCode::OK),
            ]);
            assert!(requests.lock().unwrap().is_empty());
            let exported = consumer.credential_export(id, None).unwrap();
            assert!(exported["api_key"] == opened.credential.as_ref().unwrap().bearer);
            let saved_bytes = std::fs::read(&account_path).unwrap();
            let saved: Value = serde_json::from_slice(&saved_bytes).unwrap();
            assert_eq!(saved["topup"]["raw_transaction"], raw);
            assert_eq!(saved["topup"]["id"], intent_id);
            assert_eq!(saved["topup"]["amount_wei"], "1000");
            drop(consumer);

            if !missing {
                // The mismatched row is internally consistent and has identical payment terms,
                // but a different genuinely signed nonce. Without the guard, reconciliation would
                // reach RPC rather than reject an unrelated amount/calldata mismatch locally.
                journal.execute(
                    "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at)
                     VALUES(?1,?2,'2',?3,?4,'submitted',?5)",
                    params![quote.payload.consumer_wallet, operation, different.tx_hash,
                        different_raw, quote.payload.issued_at]).unwrap();
            }
            let records = || {
                let mut statement = journal.prepare(
                    "SELECT json_array(sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at)
                     FROM transactions ORDER BY sender,operation").unwrap();
                let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>().unwrap();
                rows
            };
            let originals = || {
                let mut statement = journal.prepare(
                    "SELECT json_array(sender,operation,nonce,tx_hash,raw,created_at)
                     FROM transactions ORDER BY sender,operation").unwrap();
                let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>().unwrap();
                rows
            };
            let provider_records = || [
                "SELECT json_array(id,owner,quote,account,sequence,created) FROM accounts ORDER BY id",
                "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
                "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash",
                "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
                "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
                "SELECT json_array(bucket,window,count) FROM rate_limits ORDER BY bucket",
            ].map(|query| {
                let mut statement = provider_db.prepare(query).unwrap();
                let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>().unwrap();
                rows
            });
            let before = records();
            assert_eq!(before.len(), if missing { 1 } else { 2 });
            let provider_before = provider_records();
            let controls_before = controls.lock().unwrap().clone();
            let reopened = Consumer::new(config.clone()).unwrap();
            let result = match method {
                "resume" => reopened.resume(id).await,
                "topup" => reopened.topup(id, "1000").await,
                "fund" => reopened.fund(&fixture.provider.offer, &quote).await,
                _ => unreachable!(),
            };
            assert!(result.is_err(), "{method}, missing={missing}");
            assert!(requests.lock().unwrap().is_empty(), "journal disagreement must precede all chain access");
            assert_eq!(*controls.lock().unwrap(), controls_before);
            assert_eq!(records(), before, "refusal must not create, replace or reclassify journal operations");
            assert_eq!(std::fs::read(&account_path).unwrap(), saved_bytes);
            assert_eq!(reopened.credential_export(id, None).unwrap(), exported);
            assert_eq!(provider_records(), provider_before);
            drop(reopened);

            // Explicit isolated fixture repair, not automatic product behavior: restore only the
            // missing/wrong top-up row. The identical saved state must now reach chain recovery.
            journal.execute("DELETE FROM transactions WHERE sender=?1 AND operation=?2",
                params![quote.payload.consumer_wallet, operation]).unwrap();
            journal.execute(
                "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,created_at)
                 VALUES(?1,?2,'1',?3,?4,'submitted',?5)",
                params![quote.payload.consumer_wallet, operation, envelope.tx_hash, raw,
                    quote.payload.issued_at]).unwrap();
            let repaired = originals();
            assert_eq!(repaired.len(), 2);
            let reopened = Consumer::new(config).unwrap();
            let result = match method {
                "resume" => reopened.resume(id).await,
                "topup" => reopened.topup(id, "1000").await,
                "fund" => reopened.fund(&fixture.provider.offer, &quote).await,
                _ => unreachable!(),
            };
            assert!(result.is_err(), "the unavailable RPC stops the positive control before submission");
            let rpc = requests.lock().unwrap();
            assert!(!rpc.is_empty(), "restoring the exact original must unblock chain reconciliation");
            assert!(rpc.iter().all(|request| request["method"] != "eth_sendRawTransaction"));
            drop(rpc);
            assert_eq!(originals(), repaired, "recovery cannot replace either original or its nonce");
            assert_eq!(*controls.lock().unwrap(), controls_before);
            assert_eq!(std::fs::read(&account_path).unwrap(), saved_bytes);
            assert_eq!(reopened.credential_export(id, None).unwrap(), exported);
            assert_eq!(provider_records(), provider_before);
            assert_eq!(journal.query_row("SELECT COUNT(*) FROM cancellations",
                [], |row| row.get::<_, u64>(0)).unwrap(), 0);
            let untouched: (u64, u64, u64, u64) = provider_db.query_row(
                "SELECT (SELECT COUNT(*) FROM funding_attempts),(SELECT COUNT(*) FROM fee_obligations),
                        (SELECT COUNT(*) FROM fee_batches),(SELECT COUNT(*) FROM requests)",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
            assert_eq!(untouched, (0, 0, 0, 0));
            let (status, account) = fixture.send("GET", &format!("/v1/contracts/{id}"),
                &bearer(opened.credential.as_ref().unwrap()), Vec::new()).await;
            assert_eq!(status, StatusCode::OK, "{account}");
            assert_eq!(account, serde_json::to_value(&opened.account).unwrap());
            assert_eq!(provider_records(), provider_before);
            stop.send(()).unwrap();
            provider_server.await.unwrap();
            rpc_server.abort();
            assert!(rpc_server.await.unwrap_err().is_cancelled());
        }
    }
}

struct NativeTopupRpc {
    view: Arc<TopupRecoveryRpc>,
    opening: VerifiedPayment,
    raw: String,
    account_path: PathBuf,
    journal_path: PathBuf,
    submissions: AtomicU64,
}

async fn native_topup_rpc(
    State(rpc): State<Arc<NativeTopupRpc>>, Json(request): Json<Value>,
) -> Json<Value> {
    // Observe the actual durable intent before any native RPC, and the independently
    // committed exact journal bytes before submission. No top-up state is seeded here.
    let saved: Value = serde_json::from_slice(&std::fs::read(&rpc.account_path).unwrap()).unwrap();
    assert_eq!(saved["raw_transaction"], rpc.opening.raw_transaction);
    assert_eq!(saved["topup"]["amount_wei"], "1000");
    let intent = saved["topup"]["id"].as_str().unwrap();
    uuid::Uuid::parse_str(intent).unwrap();
    if !saved["topup"]["raw_transaction"].is_null() {
        assert_eq!(saved["topup"]["raw_transaction"], rpc.raw);
    }
    let args = &request["params"];
    let result = match request["method"].as_str().unwrap() {
        "eth_getTransactionCount" => {
            assert_eq!(args[0], rpc.opening.from);
            assert!(["latest", "pending"].contains(&args[1].as_str().unwrap()));
            json!("0x1")
        }
        "eth_getBalance" => {
            assert_eq!(args, &json!([rpc.opening.from, "latest"]));
            json!("0x100000000")
        }
        "eth_gasPrice" => json!("0x1"),
        "eth_estimateGas" => {
            let data = protocol::topup_data(&rpc.view.quote.id).unwrap();
            assert_eq!(args[0]["from"], rpc.opening.from);
            assert_eq!(args[0]["to"], rpc.opening.to);
            assert_eq!(args[0]["value"], "0x3e8");
            assert_eq!(args[0]["data"], format!("0x{}", hex::encode(data)));
            assert_eq!(args[0]["gasPrice"], "0x1");
            json!("0xf4240")
        }
        "eth_sendRawTransaction" => {
            assert_eq!(args, &json!([rpc.raw]));
            assert!(saved["topup"]["raw_transaction"].is_null());
            assert_eq!(rpc.submissions.fetch_add(1, Ordering::AcqRel), 0);
            let db = Connection::open(&rpc.journal_path).unwrap();
            let original: (String, String, String, String) = db.query_row(
                "SELECT nonce,tx_hash,raw,status FROM transactions WHERE sender=?1 AND operation=?2",
                params![rpc.opening.from, format!("topup:{}:{intent}", rpc.view.quote.id)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
            assert_eq!(original, ("1".into(), rpc.view.tx_hash.clone(), rpc.raw.clone(), "prepared".into()));
            json!(rpc.view.tx_hash)
        }
        "eth_getTransactionReceipt" if args[0] == rpc.opening.tx_hash => {
            json!({"transactionHash":rpc.opening.tx_hash,"from":rpc.opening.from,
                "to":rpc.opening.to,"type":"0x0","gasUsed":"0x10000","status":"0x1",
                "blockNumber":"0x1","blockHash":rpc.opening.block_hash})
        }
        "eth_getBlockByNumber" if args[0] == "0x1" => {
            assert_eq!(args[1], false);
            json!({"number":"0x1","hash":rpc.opening.block_hash,
                "timestamp":format!("0x{:x}", rpc.view.quote.issued_at)})
        }
        _ => return topup_recovery_rpc(State(rpc.view.clone()), Json(request)).await,
    };
    rpc.view.requests.lock().unwrap().push(request.clone());
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

fn native_payment_journal_snapshot(journal: &Connection) -> [Vec<String>; 2] {
    [
        "SELECT json_array(sender,operation,nonce,tx_hash,raw,status,error,evidence,created_at) FROM transactions ORDER BY sender,operation",
        "SELECT json_array(sender,operation,raw,status,error,evidence) FROM cancellations ORDER BY sender,operation",
    ].map(|query| journal.prepare(query).unwrap()
        .query_map([], |row| row.get::<_, String>(0)).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap())
}

async fn assert_native_topup_amount_refusals(
    consumer: &Consumer, id: &str, account_path: &std::path::Path,
    ledger: &Connection, journal: &Connection, traffic: impl Fn() -> [usize; 4],
) {
    let saved = std::fs::read(account_path).unwrap();
    let pending = !serde_json::from_slice::<Value>(&saved).unwrap()["topup"].is_null();
    let conflicting: &[&str] = if pending {
        &["1", "999", "1001", "340282366920938463463374607431768211455"]
    } else { &[] };
    let before = (
        ledger_snapshot(ledger), native_payment_journal_snapshot(journal),
        traffic(), consumer.credential_export(id, None).unwrap(),
    );
    for amount in [
        "", "0", "00", "01", "01000", "+1000", "-1", " 1000", "1000 ",
        "1000.0", "1e3", "0x3e8", "١٠٠٠", "340282366920938463463374607431768211456",
    ].into_iter().chain(conflicting.iter().copied()) {
        consumer.topup(id, amount).await.expect_err("invalid or conflicting native amount must be refused");
        assert_eq!(std::fs::read(account_path).unwrap(), saved, "amount {amount:?}");
        assert_eq!(ledger_snapshot(ledger), before.0, "amount {amount:?}");
        assert_eq!(native_payment_journal_snapshot(journal), before.1, "amount {amount:?}");
        assert_eq!(traffic(), before.2, "amount {amount:?}");
        assert_eq!(consumer.credential_export(id, None).unwrap(), before.3, "amount {amount:?}");
    }
}

enum NativeRecoveryPressure {
    QuoteStorage,
    QuoteSigningWork,
    QuoteCommittedIssuance,
    OwnedFreeOpening,
    GlobalFreeOpening,
    PaidIntent,
}

#[tokio::test]
async fn native_topup_recovers_lost_credit_response_at_full_quote_storage() {
    native_topup_recovery_under_pressure(NativeRecoveryPressure::QuoteStorage).await;
}

#[tokio::test]
async fn native_topup_rejects_invalid_amounts_and_recovers_at_full_quote_work() {
    native_topup_recovery_under_pressure(NativeRecoveryPressure::QuoteSigningWork).await;
}

#[tokio::test]
async fn native_topup_recovers_lost_credit_response_at_full_quote_issuance() {
    native_topup_recovery_under_pressure(NativeRecoveryPressure::QuoteCommittedIssuance).await;
}

#[tokio::test]
async fn native_topup_recovers_lost_credit_response_at_owned_free_opening_limit() {
    native_topup_recovery_under_pressure(NativeRecoveryPressure::OwnedFreeOpening).await;
}

#[tokio::test]
async fn native_topup_recovers_lost_credit_response_at_global_free_opening_limit() {
    native_topup_recovery_under_pressure(NativeRecoveryPressure::GlobalFreeOpening).await;
}

#[tokio::test]
async fn native_topup_recovers_lost_credit_response_at_paid_intent_limit() {
    native_topup_recovery_under_pressure(NativeRecoveryPressure::PaidIntent).await;
}

async fn native_topup_recovery_under_pressure(pressure: NativeRecoveryPressure) {
    use std::io::Write;
    use protocol::TokenUsage;

    let directory = tempfile::tempdir().unwrap();
    let (case, quote_count, exhausted_bucket, free_capacity, paid_capacity) = match pressure {
        NativeRecoveryPressure::QuoteStorage => (58_000, 10_000_u64, None, None, false),
        NativeRecoveryPressure::QuoteSigningWork => (76_000, 1, Some("quote-work:global"), None, false),
        NativeRecoveryPressure::QuoteCommittedIssuance => (76_001, 1, Some("quote:global"), None, false),
        NativeRecoveryPressure::OwnedFreeOpening => (77_000, 1, None, Some(false), false),
        NativeRecoveryPressure::GlobalFreeOpening => (77_001, 1, None, Some(true), false),
        NativeRecoveryPressure::PaidIntent => (78_000, 1, None, None, true),
    };
    let account_count = match free_capacity { Some(false) => 8_u64, Some(true) => 10_000, None => 2 };
    let intent_count = if paid_capacity { 10_001_u64 } else { 1 };
    let mut fixture = Fixture::new(directory.path(), case);
    let free_offer = free_capacity.map(|_| fixture.provider.offer.payload.clone());
    let other = fixture.open_free().await;
    // Original paid opening/native history and supplied usage are prerequisites.
    // The top-up intent, nonce, preparation, submission and verification run below.
    let (opened, opening_raw) = fixture.paid_account().await;
    let accepted_offer = free_capacity.map(|_| fixture.provider.offer.clone());
    let id = &opened.account.contract_id;
    let quote = fixture.provider.state.ledger.quote(id).unwrap();
    let ack = fixture.provider.state.ledger.funding_intent(&quote.payload, &opening_raw).unwrap().unwrap();
    let now = protocol::now().unwrap();
    let root = opened.credential.as_ref().unwrap();
    let ledger = &fixture.provider.state.ledger;
    let spent = ledger.reserve(id, &root.id, "spent-before-native-topup", 1, 1, now, &opened.account.model).unwrap();
    ledger.settle(&spent.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 }, "completed", now).unwrap();
    let held = ledger.reserve(id, &root.id, "held-through-native-topup", 1000, 512, now, &opened.account.model).unwrap();
    let original_account = ledger.account(id).unwrap();
    assert_eq!(original_account.spent_wei, "1");
    assert_eq!(original_account.reserved_wei, "3");
    assert_eq!(original_account.available_wei, "96");
    let (raw, envelope) = fixture.signed_payment(&protocol::topup_data(id).unwrap(), 1000, 1);
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    if paid_capacity {
        let amount = quote.payload.funding_wei.parse().unwrap();
        funding::restore_retained_intents(&mut db, &fixture.provider, &quote, 10_000, |quote, key, sequence| {
            fixture.signed_payment_for(key, &protocol::opening_data(quote).unwrap(), amount, sequence)
        });
    }
    let restored = if let Some(ledger_wide) = free_capacity {
        // Complete signed account/root prerequisites, not observed admissions.
        // Restoring six owned accounts also avoids spending the native owner's
        // challenge allowance on setup instead of the recovery being exercised.
        let restored = restore_free_accounts(&mut db, &fixture, &other, account_count - 2,
            if ledger_wide { None } else { Some(&opened.account.consumer_wallet) });
        let mut free_offer = free_offer.unwrap();
        free_offer.revision = fixture.provider.offer.payload.revision + 1;
        fixture.provider.offer = protocol::sign(protocol::OFFER_DOMAIN, free_offer, &fixture.provider.state.private_key).unwrap();
        fixture.provider.offer_id = fixture.provider.state.offers.insert(fixture.provider.offer.clone()).unwrap();
        restored
    } else { Vec::new() };
    // Issue the fresh-opening control after restoration; native history retains
    // the original paid offer even though the current publication is now free.
    let pending = fixture.quote(if free_capacity.is_some() { "0" } else { "100" }).await;
    if quote_count == 10_000 {
        restore_full_quote_storage(&mut db, &pending, &fixture.provider.state.private_key);
    }
    // Restored coherent logical minutes, not observed future signing history.
    // Exhaust only the selected budget; retain actual counts for other resources.
    // Future watermarks avoid a wall-minute rollover changing the refusal control.
    let minute = protocol::now().unwrap() / 60 + 10;
    let tx = db.transaction().unwrap();
    for (bucket, watermark) in [
        ("quote:global", "quote_committed_minute"),
        ("quote-work:global", "quote_work_minute"),
    ] {
        assert_eq!(tx.execute(
            "UPDATE rate_limits SET window=?1,count=CASE WHEN bucket=?2 THEN 600 ELSE count END WHERE bucket=?3",
            params![minute, exhausted_bucket, bucket]).unwrap(), 1);
        assert_eq!(tx.execute("UPDATE metadata SET value=?1 WHERE key=?2", params![minute, watermark]).unwrap(), 1);
    }
    tx.commit().unwrap();
    let quote_rates = || db.prepare(
        "SELECT json_array(bucket,window,count) FROM rate_limits WHERE bucket IN ('quote:global','quote-work:global') ORDER BY bucket")
        .unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap()
        .map(|row| serde_json::from_str::<Value>(&row.unwrap()).unwrap()).collect::<Vec<_>>();
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
        "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        "SELECT json_array(id,contract,token_hash,record,revoked) FROM credentials ORDER BY id",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(id,owner,quote,sequence,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,request_key,credential,status,created,record) FROM requests ORDER BY id",
        "SELECT json_array(key,value) FROM metadata WHERE key IN ('quote_committed_minute','quote_work_minute') ORDER BY key",
    ].map(|query| fingerprint(query, &[])),
        fingerprint("SELECT json_array(id,account) FROM accounts WHERE id<>?1 ORDER BY id", &[id]));
    let monetary_state = || [
        "SELECT json_array(id,account) FROM accounts ORDER BY id",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash",
        "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash",
        "SELECT json_array(id,record) FROM fee_batches ORDER BY id",
    ].map(|query| fingerprint(query, &[]));
    let assert_resources = || {
        assert_eq!(db.query_row(
            "SELECT COUNT(*) FROM quotes WHERE expires>?1", [protocol::now().unwrap()],
            |row| row.get::<_, u64>(0)).unwrap(), quote_count);
        let counts: (u64, u64, u64) = db.query_row(
            "SELECT (SELECT COUNT(*) FROM accounts WHERE owner=?1),
                    (SELECT COUNT(*) FROM accounts),
                    (SELECT COUNT(*) FROM funding_intents WHERE admission_until>?2
                     AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=contract))",
            params![opened.account.consumer_wallet, protocol::now().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(counts, (if free_capacity == Some(false) { 8 } else { 2 }, account_count, intent_count - 1));
        if free_capacity == Some(true) {
            assert_eq!(db.query_row(
                "SELECT MAX(owned) FROM (SELECT COUNT(*) AS owned FROM accounts GROUP BY owner)",
                [], |row| row.get::<_, u64>(0)).unwrap(), 2);
        }
    };
    let protected = protected_state();
    let before_credit = monetary_state();
    assert_eq!(protected.0.map(|entry| entry.0), [quote_count, account_count, intent_count, account_count, 2, 2]);
    assert_eq!(protected.1.0, account_count - 1);
    assert_eq!(before_credit.map(|entry| entry.0), [account_count, 1, 0, 0]);
    assert_resources();
    let mut expected_rates = quote_rates();
    assert_eq!(expected_rates.len(), 2);
    for row in &expected_rates {
        assert_eq!(row[1], minute);
        let count = row[2].as_u64().unwrap();
        if row[0].as_str() == exhausted_bucket { assert_eq!(count, 600); }
        else { assert!((1..600).contains(&count)); }
    }
    let (status, refused) = if paid_capacity {
        let (raw, refused_envelope) = fixture.signed_payment(&protocol::opening_data(&pending.payload).unwrap(), 100, 1);
        let rpc = Arc::new(TopupRecoveryRpc {
            quote: pending.payload.clone(), tx_hash: refused_envelope.tx_hash.clone(),
            phase: AtomicU64::new(0), timestamp: AtomicU64::new(protocol::now().unwrap()),
            requests: Mutex::new(Vec::new()),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().route("/", post(native_paid_intent_capacity_rpc)).with_state(rpc.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        fixture.app = Router::new();
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        Arc::get_mut(&mut state.config).unwrap().network.rpc_url = rpc_url;
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
        fixture.app = contracts::router(fixture.provider.state.clone());
        let path = format!("/v1/contracts/{}/funding-intent", pending.payload.id);
        let body = serde_json::to_vec(&json!({"raw_transaction":raw})).unwrap();
        let proof = fixture.proof(OWNER_KEY, "POST", &path, &body).await;
        let response = fixture.send("POST", &path, &proof.headers, body).await;
        fixture.consumed(&proof);
        {
            let calls = rpc.requests.lock().unwrap();
            assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBalance").count(), 1);
            assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getTransactionCount").count(), 4);
            assert!(calls.iter().all(|call| call["method"] != "eth_sendRawTransaction" && call["method"] != "eth_getTransactionReceipt"));
        }
        assert_eq!(db.query_row(
            "SELECT COUNT(*) FROM funding_intents WHERE contract=?1 OR tx_hash=?2 OR (sender=?3 AND nonce='1')",
            params![pending.payload.id, refused_envelope.tx_hash, pending.payload.consumer_wallet],
            |row| row.get::<_, u64>(0)).unwrap(), 0);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        response
    } else if free_capacity.is_some() {
        let body = serde_json::to_vec(&json!({"quote_id":pending.payload.id,"raw_transaction":null})).unwrap();
        let proof = fixture.proof(OWNER_KEY, "POST", OPEN, &body).await;
        let response = fixture.send("POST", OPEN, &proof.headers, body).await;
        fixture.consumed(&proof);
        response
    } else {
        fixture.post("/v1/contracts/propose", json!({
            "offer_id":fixture.provider.offer_id,"consumer_wallet":wallet::address_from_private_key(OTHER_KEY).unwrap(),
            "funding_wei":"100"
        })).await
    };
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
    assert_eq!(refused["error"]["code"], "rate_limit");
    if paid_capacity || free_capacity.is_some() {
        fixture.unopened(&pending);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM funding_attempts", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
    } else if exhausted_bucket != Some("quote-work:global") {
        // Storage/issuance refusal spends signing work; work exhaustion does not.
        let work = expected_rates.iter_mut().find(|row| row[0] == "quote-work:global").unwrap();
        work[2] = json!(work[2].as_u64().unwrap() + 1);
    }
    assert_eq!(quote_rates(), expected_rates);
    assert_eq!(protected_state(), protected);
    assert_eq!(monetary_state(), before_credit);

    let client_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_rpc_url = format!("http://{}", provider_listener.local_addr().unwrap());
    let mut network = fixture.provider.state.config.network.clone();
    network.rpc_url = format!("http://{}", client_listener.local_addr().unwrap());
    let config = ConsumerConfig {
        network, data_dir: directory.path().join("native-topup"),
        catalog_url: None, development: true, dht: None,
    };
    let mut consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, OWNER_KEY).unwrap();
    assert_eq!(consumer.wallet_import_file(&import).unwrap(), quote.payload.consumer_wallet);
    let scoped = config.network.scoped_dir(&config.data_dir);
    let account_path = scoped.join("consumer").join(&quote.payload.consumer_wallet)
        .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
    wallet::secure_directory(account_path.parent().unwrap()).unwrap();
    let saved_opening = json!({
        "offer":accepted_offer.as_ref().unwrap_or(&fixture.provider.offer),"quote":quote,"account":original_account,
        "raw_transaction":opening_raw,"tx_hash":ack.payload.tx_hash,"funding_intent":ack,
        "funding_broadcast_started":true,"funding_cancellation_requested":false,
        "funding_cancellation":null,"credentials":[opened.credential.as_ref().unwrap()],
        "active_credential":opened.credential.as_ref().unwrap().id,"receipt_sequence":0,"topup":null
    });
    let mut file = wallet::create_secret_file(&account_path).unwrap();
    file.write_all(&serde_json::to_vec(&saved_opening).unwrap()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let opening_evidence: String = db.query_row("SELECT evidence FROM funding WHERE tx_hash=?1",
        [&ack.payload.tx_hash], |row| row.get(0)).unwrap();
    let journal_path = scoped.join("wallet-journal/transactions.sqlite3");
    let journal = Connection::open(&journal_path).unwrap();
    journal.execute(
        "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,evidence,created_at)
         VALUES(?1,?2,'0',?3,?4,'accepted',?5,?6)",
        params![quote.payload.consumer_wallet, format!("open:{id}"), ack.payload.tx_hash,
            opening_raw, opening_evidence, quote.payload.issued_at]).unwrap();
    let originals = || {
        journal.prepare("SELECT json_array(sender,operation,nonce,tx_hash,raw,created_at)
            FROM transactions ORDER BY nonce").unwrap().query_map([], |row| row.get::<_, String>(0))
            .unwrap().collect::<Result<Vec<_>, _>>().unwrap()
    };
    let opening_original = originals();
    assert_eq!(opening_original.len(), 1);
    let exported = consumer.credential_export(id, None).unwrap();
    assert!(exported["api_key"] == opened.credential.as_ref().unwrap().bearer);
    drop(consumer);

    let timestamp = protocol::now().unwrap();
    let client_view = Arc::new(TopupRecoveryRpc {
        quote: quote.payload.clone(), tx_hash: envelope.tx_hash.clone(), phase: AtomicU64::new(0),
        timestamp: AtomicU64::new(timestamp), requests: Mutex::new(Vec::new()),
    });
    let provider_view = Arc::new(TopupRecoveryRpc {
        quote: quote.payload.clone(), tx_hash: envelope.tx_hash.clone(), phase: AtomicU64::new(0),
        timestamp: AtomicU64::new(timestamp), requests: Mutex::new(Vec::new()),
    });
    let client_rpc = Arc::new(NativeTopupRpc {
        view: client_view.clone(), opening: serde_json::from_str(&opening_evidence).unwrap(),
        raw: raw.clone(), account_path: account_path.clone(), journal_path, submissions: AtomicU64::new(0),
    });
    let client_app = Router::new().route("/", post(native_topup_rpc)).with_state(client_rpc.clone());
    let provider_app = Router::new().route("/", post(topup_recovery_rpc)).with_state(provider_view.clone());
    let client_server = tokio::spawn(async move { axum::serve(client_listener, client_app).await.unwrap(); });
    let provider_rpc_server = tokio::spawn(async move { axum::serve(provider_listener, provider_app).await.unwrap(); });
    let exchanges = Arc::new(Mutex::new(Vec::new()));
    let lose_next = Arc::new(AtomicBool::new(false));
    // Exercise the local input edges once, using the small retained-ledger case.
    let amount_cases = exhausted_bucket == Some("quote-work:global");
    let unavailable_requests = amount_cases.then(|| Arc::new(Mutex::new(Vec::<Value>::new())));
    let traffic = || [
        exchanges.lock().unwrap().len(), client_view.requests.lock().unwrap().len(),
        provider_view.requests.lock().unwrap().len(),
        unavailable_requests.as_ref().map_or(0, |requests| requests.lock().unwrap().len()),
    ];
    let listener = fixture._listeners[0].try_clone().unwrap();
    listener.set_nonblocking(true).unwrap();
    let serve = |app: Router| {
        let observed = exchanges.clone();
        let lose_next = lose_next.clone();
        let app = app.layer(from_fn(move |request: Request, next: Next| {
            let observed = observed.clone();
            let lose_next = lose_next.clone();
            async move {
                let method = request.method().to_string();
                let path = request.uri().path().to_owned();
                let (parts, body) = request.into_parts();
                let body = to_bytes(body, 96 * 1024).await.unwrap();
                let requested = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() };
                let response = next.run(Request::from_parts(parts, Body::from(body))).await;
                let (mut parts, body) = response.into_parts();
                let body = to_bytes(body, 96 * 1024).await.unwrap();
                let replied: Value = serde_json::from_slice(&body).unwrap();
                let topup = path.ends_with("/topup");
                observed.lock().unwrap().push((method, path, requested, parts.status, replied));
                if topup && parts.status == StatusCode::OK && lose_next.swap(false, Ordering::AcqRel) {
                    // Lose the genuine committed credit response, never fabricate success.
                    parts.headers.remove(header::CONTENT_LENGTH);
                    return Response::from_parts(parts, Body::from_stream(futures_util::stream::once(async {
                        Err::<axum::body::Bytes, _>(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
                    })));
                }
                Response::from_parts(parts, Body::from(body))
            }
        }));
        let listener = tokio::net::TcpListener::from_std(listener.try_clone().unwrap()).unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.unwrap(); })
                .await.unwrap();
        });
        (stop, server)
    };
    let mut retained_originals = None;
    let mut retained_saved = None;
    let mut retained_topup = Value::Null;
    let mut credited = None;
    let mut expected_account = serde_json::to_value(&original_account).unwrap();
    for (attempt, phase) in [0, 1, 2, 2].into_iter().enumerate() {
        // Reopen native state and actual provider store/chain/router at every recovery boundary.
        fixture.app = Router::new();
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        Arc::get_mut(&mut state.config).unwrap().network.rpc_url = provider_rpc_url.clone();
        state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path, &state.config.network.identity).unwrap());
        state.chain = Arc::new(ChainClient::new(state.config.network.clone(), state.config.data_dir.clone()).unwrap());
        state.accepting.store(false, Ordering::Release);
        state.funding_ready.store(false, Ordering::Release);
        state.qualified_models.write().unwrap().clear();
        fixture.app = contracts::router(fixture.provider.state.clone());
        provider_view.phase.store(phase, Ordering::Release);
        client_view.phase.store(if attempt == 0 { 0 } else { 2 }, Ordering::Release);
        lose_next.store(attempt == 2, Ordering::Release);
        assert_resources();
        assert_eq!(quote_rates(), expected_rates);
        let (stop, server) = serve(fixture.app.clone());
        let start = exchanges.lock().unwrap().len();
        let provider_start = provider_view.requests.lock().unwrap().len();
        let (consumer, unprepared_topup) = if amount_cases && attempt == 0 {
            // A real failed preparation leaves a durable intent without a signed
            // original. Do not manufacture or rewrite the native pending record.
            let requests = unavailable_requests.as_ref().unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut unavailable = config.clone();
            unavailable.network.rpc_url = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new().route("/", post(
                |State(requests): State<Arc<Mutex<Vec<Value>>>>, Json(request): Json<Value>| async move {
                    assert_ne!(request["method"], "eth_sendRawTransaction");
                    let id = request["id"].clone();
                    requests.lock().unwrap().push(request);
                    Json(json!({"jsonrpc":"2.0","id":id,
                        "error":{"code":-32000,"message":"isolated preparation RPC unavailable"}}))
                },
            )).with_state(requests.clone());
            let failed_server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
            let consumer = Consumer::new(unavailable).unwrap();
            assert_native_topup_amount_refusals(&consumer, id, &account_path, &db, &journal, &traffic).await;
            let before_journal = native_payment_journal_snapshot(&journal);
            consumer.topup(id, "1000").await.expect_err("unavailable RPC must prevent preparation");
            assert!(!requests.lock().unwrap().is_empty());
            assert_eq!(native_payment_journal_snapshot(&journal), before_journal);
            assert_eq!(protected_state(), protected);
            assert_eq!(monetary_state(), before_credit);
            let saved: Value = serde_json::from_slice(&std::fs::read(&account_path).unwrap()).unwrap();
            let intent = saved["topup"].clone();
            uuid::Uuid::parse_str(intent["id"].as_str().unwrap()).unwrap();
            assert_eq!(intent["amount_wei"], "1000");
            assert_eq!(intent["raw_transaction"], Value::Null);
            let mut expected = saved_opening.clone();
            expected["topup"] = intent.clone();
            assert_eq!(saved, expected);
            drop(consumer);
            failed_server.abort();
            assert!(failed_server.await.unwrap_err().is_cancelled());
            let consumer = Consumer::new(config.clone()).unwrap();
            assert_native_topup_amount_refusals(&consumer, id, &account_path, &db, &journal, &traffic).await;
            (consumer, Some(intent))
        } else { (Consumer::new(config.clone()).unwrap(), None) };
        let result = match attempt {
            0 | 2 => consumer.topup(id, "1000").await,
            1 => consumer.resume(id).await,
            _ => consumer.fund(accepted_offer.as_ref().unwrap_or(&fixture.provider.offer), &quote).await,
        };
        if attempt < 3 { result.expect_err("pending evidence or a disconnected accepted response"); }
        else {
            assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), expected_account);
        }
        let observed = exchanges.lock().unwrap();
        let calls = &observed[start..];
        assert_eq!(calls.len(), if attempt == 0 { 4 } else { 2 });
        for pair in calls.chunks_exact(2) {
            assert_eq!((&pair[0].0, pair[0].1.as_str(), pair[0].3),
                (&"POST".to_owned(), "/v1/auth/challenge", StatusCode::OK));
            let challenge: Challenge = serde_json::from_value(pair[0].4.clone()).unwrap();
            let remaining: u64 = db.query_row("SELECT COUNT(*) FROM challenges WHERE nonce=?1",
                [&challenge.nonce], |row| row.get(0)).unwrap();
            assert_eq!(remaining, 0);
        }
        if attempt == 0 {
            assert_eq!((&calls[1].0, &calls[1].1, &calls[1].2, calls[1].3, &calls[1].4),
                (&"GET".into(), &format!("/v1/contracts/{id}"), &Value::Null, StatusCode::OK, &expected_account));
        }
        let topup = calls.last().unwrap();
        assert_eq!((&topup.0, &topup.1, &topup.2), (&"POST".into(), &format!("/v1/contracts/{id}/topup"),
            &json!({"raw_transaction":raw})));
        if phase < 2 {
            assert_eq!(topup.3, StatusCode::CONFLICT);
            assert_eq!(topup.4["error"]["code"], "funding_pending");
            assert_eq!(monetary_state(), before_credit);
            let pending: (String, String, String) = db.query_row(
                "SELECT tx_hash,raw_transaction,json_extract(record,'$.status') FROM funding_attempts",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
            assert_eq!(pending, (envelope.tx_hash.clone(), raw.clone(), "pending".into()));
        } else {
            expected_account["funded_wei"] = json!("1100");
            expected_account["fee_wei"] = json!("5");
            expected_account["available_wei"] = json!("1091");
            assert_eq!(topup.3, StatusCode::OK);
            assert_eq!(topup.4, expected_account);
            assert!(!lose_next.load(Ordering::Acquire));
            let current = monetary_state();
            assert_eq!(current.map(|entry| entry.0), [account_count, 2, 1, 0]);
            if let Some(previous) = credited { assert_eq!(current, previous); }
            else { credited = Some(current); }
            assert_eq!(db.query_row("SELECT COUNT(*) FROM funding_attempts", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
            let evidence: String = db.query_row("SELECT evidence FROM funding WHERE tx_hash=?1",
                [&envelope.tx_hash], |row| row.get(0)).unwrap();
            let native_evidence: String = journal.query_row("SELECT evidence FROM transactions WHERE tx_hash=?1",
                [&envelope.tx_hash], |row| row.get(0)).unwrap();
            let expected = VerifiedPayment {
                network: quote.payload.network.clone(), tx_hash: envelope.tx_hash.clone(),
                from: envelope.from.clone(), to: envelope.to.clone(), value_wei: "1000".into(),
                data: envelope.data.clone(), block_hash: format!("0x{:064x}", 8), block_number: 8, raw_transaction: raw.clone(),
            };
            assert_eq!(serde_json::from_str::<Value>(&evidence).unwrap(), serde_json::to_value(&expected).unwrap());
            assert_eq!(serde_json::from_str::<Value>(&native_evidence).unwrap(), serde_json::to_value(expected).unwrap());
            let fee: (String, String, String) = db.query_row("SELECT platform,amount,status FROM fee_obligations",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
            assert_eq!(fee, (quote.payload.platform_wallet.clone(), "5".into(), "pending".into()));
        }
        drop(observed);
        let observed = provider_view.requests.lock().unwrap();
        let calls = &observed[provider_start..];
        for method in ["eth_getTransactionReceipt", "eth_syncing"] {
            assert_eq!(calls.iter().filter(|call| call["method"] == method).count(), if phase < 2 { 1 } else { 2 });
        }
        for tag in ["safe", "latest", "0x8"] {
            assert_eq!(calls.iter().filter(|call| call["method"] == "eth_getBlockByNumber"
                && call["params"][0] == tag).count(),
                if phase < 2 { usize::from(tag != "0x8") } else { 2 });
        }
        drop(observed);
        let observed = client_view.requests.lock().unwrap();
        for (method, count) in [("eth_getTransactionCount", 4), ("eth_getBalance", 1),
            ("eth_gasPrice", 1), ("eth_estimateGas", 1), ("eth_sendRawTransaction", 1)] {
            assert_eq!(observed.iter().filter(|call| call["method"] == method).count(), count);
        }
        drop(observed);
        assert_eq!(client_rpc.submissions.load(Ordering::Acquire), 1);
        let current_originals = originals();
        assert_eq!(current_originals.len(), 2);
        assert_eq!(current_originals[0], opening_original[0]);
        if let Some(previous) = &retained_originals { assert_eq!(&current_originals, previous); }
        else { retained_originals = Some(current_originals); }
        let bytes = std::fs::read(&account_path).unwrap();
        let saved: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(mut intent) = unprepared_topup {
            intent["raw_transaction"] = json!(raw);
            assert_eq!(saved["topup"], intent, "preparation must retain the reopened intent's identity and amount");
        }
        if attempt == 0 {
            retained_topup = saved["topup"].clone();
            assert_eq!(retained_topup["raw_transaction"], raw);
            assert_eq!(retained_topup["amount_wei"], "1000");
            retained_saved = Some(bytes.clone());
        }
        let mut expected_saved = saved_opening.clone();
        if attempt < 3 {
            expected_saved["topup"] = retained_topup.clone();
            assert_eq!(&bytes, retained_saved.as_ref().unwrap());
        } else { expected_saved["account"] = expected_account.clone(); }
        assert_eq!(saved, expected_saved, "only confirmed credit clears the original top-up intent");
        let status: String = journal.query_row("SELECT status FROM transactions WHERE tx_hash=?1",
            [&envelope.tx_hash], |row| row.get(0)).unwrap();
        assert_eq!(status, if attempt == 0 { "submitted" } else { "accepted" });
        assert_eq!(journal.query_row("SELECT COUNT(*) FROM cancellations", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
        assert_eq!(consumer.credential_export(id, None).unwrap(), exported);
        assert_eq!(protected_state(), protected);
        assert_resources();
        assert_eq!(quote_rates(), expected_rates);
        assert_eq!(fixture.provider.state.ledger.request_view(id, &held.id).unwrap().status, "reserved");
        for (credential, account) in [
            (opened.credential.as_ref().unwrap(), expected_account.clone()),
            (other.credential.as_ref().unwrap(), serde_json::to_value(&other.account).unwrap()),
        ].into_iter().chain(restored.iter().map(|sample| (
            sample.credential.as_ref().unwrap(), serde_json::to_value(&sample.account).unwrap(),
        ))) {
            let (status, value) = fixture.send("GET", &format!("/v1/contracts/{}", credential.contract_id),
                &bearer(credential), Vec::new()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(value, account);
        }
        assert_eq!(monetary_state(), credited.unwrap_or(before_credit));
        if amount_cases {
            assert_native_topup_amount_refusals(&consumer, id, &account_path, &db, &journal, &traffic).await;
        }
        drop(consumer);
        stop.send(()).unwrap();
        server.await.unwrap();
    }
    let provider_journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let operations: (u64, u64) = provider_journal.query_row(
        "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(operations, (0, 0));
    assert_eq!(db.query_row("SELECT COUNT(*) FROM requests", [], |row| row.get::<_, u64>(0)).unwrap(), 2);
    client_server.abort();
    provider_rpc_server.abort();
    assert!(client_server.await.unwrap_err().is_cancelled());
    assert!(provider_rpc_server.await.unwrap_err().is_cancelled());
}

fn restore_native_account(
    fixture: &Fixture, quote: &Signed<Quote>, opened: &OpenResult, directory: PathBuf,
) -> (ConsumerConfig, PathBuf, Connection, Value) {
    use std::io::Write;

    // Restore actual ledger prerequisites, not a newly observed native funding workflow.
    let id = &quote.payload.id;
    let credential = opened.credential.as_ref().unwrap();
    let db = Connection::open(&fixture.provider.ledger_path).unwrap();
    let evidence: String = db.query_row("SELECT evidence FROM funding WHERE contract=?1",
        [id], |row| row.get(0)).unwrap();
    let payment: VerifiedPayment = serde_json::from_str(&evidence).unwrap();
    let ack = fixture.provider.state.ledger.funding_intent(
        &quote.payload, &payment.raw_transaction).unwrap().unwrap();
    let config = ConsumerConfig {
        network: fixture.provider.state.config.network.clone(), data_dir: directory,
        catalog_url: None, development: true, dht: None,
    };
    let consumer = Consumer::new(config.clone()).unwrap();
    let import = config.data_dir.join("public-test-scalar.key");
    wallet::save_private_key(&import, OWNER_KEY).unwrap();
    assert_eq!(consumer.wallet_import_file(&import).unwrap(), quote.payload.consumer_wallet);
    let scoped = config.network.scoped_dir(&config.data_dir);
    let account_path = scoped.join("consumer").join(&quote.payload.consumer_wallet)
        .join(format!("{}.json", hex::encode(protocol::digest(id.as_bytes()))));
    wallet::secure_directory(account_path.parent().unwrap()).unwrap();
    let saved = json!({
        "offer":fixture.provider.offer,"quote":quote,"account":opened.account,
        "raw_transaction":payment.raw_transaction,"tx_hash":ack.payload.tx_hash,
        "funding_intent":ack,"funding_broadcast_started":true,
        "funding_cancellation_requested":false,"funding_cancellation":null,
        "credentials":[credential],"active_credential":credential.id,
        "receipt_sequence":0,"topup":null
    });
    let mut file = wallet::create_secret_file(&account_path).unwrap();
    file.write_all(&serde_json::to_vec(&saved).unwrap()).unwrap();
    file.sync_all().unwrap();
    let journal = Connection::open(scoped.join("wallet-journal/transactions.sqlite3")).unwrap();
    journal.execute(
        "INSERT INTO transactions(sender,operation,nonce,tx_hash,raw,status,evidence,created_at)
         VALUES(?1,?2,'0',?3,?4,'accepted',?5,?6)",
        params![quote.payload.consumer_wallet, format!("open:{id}"), payment.tx_hash,
            payment.raw_transaction, evidence, quote.payload.issued_at]).unwrap();
    (config, account_path, journal, saved)
}

#[derive(Debug)]
enum NativeSnapshotMutation {
    Term(String),
    Money(&'static str, &'static str),
    Unbalanced(&'static str),
    Field(&'static str, &'static str),
    Shape(&'static str),
}

const SNAPSHOT_FIELDS: [&str; 17] = [
    "contract_id", "network", "provider_wallet", "consumer_wallet", "model", "price", "status",
    "funded_wei", "fee_wei", "spent_wei", "reserved_wei", "available_wei", "reversed_wei",
    "provider_loss_wei", "usage", "waived_wei", "max_concurrency",
];
const SNAPSHOT_MONEY: [&str; 8] = [
    "funded_wei", "fee_wei", "spent_wei", "reserved_wei", "available_wei",
    "reversed_wei", "provider_loss_wei", "waived_wei",
];

fn snapshot_term_mutations(value: &Value, path: String, cases: &mut Vec<NativeSnapshotMutation>) {
    if let Value::Object(members) = value {
        for (name, value) in members {
            snapshot_term_mutations(value, format!("{path}/{name}"), cases);
        }
    } else {
        cases.push(NativeSnapshotMutation::Term(path.clone()));
        if let Value::Array(values) = value {
            for (index, value) in values.iter().enumerate() {
                snapshot_term_mutations(value, format!("{path}/{index}"), cases);
            }
        }
    }
}

fn wide_snapshot(account: &Value) -> Value {
    // A response-only claim, not a provider ledger transition or funding proof.
    // Both sides equal 2*u128::MAX; waived money is deliberately not part of that sum.
    let mut value = account.clone();
    for field in ["funded_wei", "provider_loss_wei", "reversed_wei", "waived_wei"] {
        value[field] = json!(u128::MAX.to_string());
    }
    value["fee_wei"] = json!("5");
    value["spent_wei"] = json!((u128::MAX - 15).to_string());
    value["reserved_wei"] = json!("7");
    value["available_wei"] = json!("3");
    value
}

fn snapshot_mutation_bytes(case: &NativeSnapshotMutation, original: &Value) -> Vec<u8> {
    let mut value = original.clone();
    match case {
        NativeSnapshotMutation::Term(path) => {
            let target = value.pointer_mut(path).unwrap();
            *target = match target {
                Value::String(text) => {
                    let changed = if path.contains("_wei_per_1k") {
                        (text.parse::<u128>().unwrap() + 1).to_string()
                    } else if text.starts_with("0x") {
                        format!("0x{}{}", if &text[2..3] == "a" { "b" } else { "a" }, &text[3..])
                    } else if [40, 64].contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit()) {
                        format!("{}{}", if &text[..1] == "a" { "b" } else { "a" }, &text[1..])
                    } else {
                        format!("{text}-different")
                    };
                    json!(changed)
                }
                Value::Number(number) => json!(number.as_u64().unwrap() + 1),
                Value::Bool(flag) => json!(!*flag),
                Value::Null => json!("Q8_0"),
                Value::Array(values) => if values.is_empty() { json!([1]) } else { json!([]) },
                Value::Object(_) => Value::Null, // The optional original serving profile.
            };
            // Term substitution must reach binding, not fail the wire's field decoder.
            serde_json::from_value::<protocol::AccountView>(value.clone()).unwrap();
        }
        NativeSnapshotMutation::Money(field, form) => {
            value = wide_snapshot(original);
            let amount = value[*field].as_str().unwrap();
            // Aliases preserve the mathematical value, so a balance mismatch cannot
            // hide permissive parsing of a particular monetary field.
            let malformed = match *form {
                "empty" => String::new(),
                "zero-prefix" => format!("0{amount}"),
                "plus" => format!("+{amount}"),
                "negative" => format!("-{amount}"),
                "left-space" => format!(" {amount}"),
                "right-space" => format!("{amount} "),
                "decimal" => format!("{amount}.0"),
                "exponent" => format!("{amount}e0"),
                "hex" => format!("0x{:x}", amount.parse::<u128>().unwrap()),
                "unicode" => amount.chars().map(|c| char::from_u32('٠' as u32 + c.to_digit(10).unwrap()).unwrap()).collect(),
                "newline" => format!("{amount}\n"),
                "tab" => format!("\t{amount}"),
                "overflow" => "340282366920938463463374607431768211456".into(),
                _ => panic!("unknown money form"),
            };
            value[*field] = json!(malformed);
        }
        NativeSnapshotMutation::Unbalanced(field) => {
            value = wide_snapshot(original);
            let amount = value[*field].as_str().unwrap().parse::<u128>().unwrap();
            value[*field] = json!(if amount == u128::MAX { amount - 1 } else { amount + 1 }.to_string());
        }
        NativeSnapshotMutation::Field(field, form) => match *form {
            "missing" => { value.as_object_mut().unwrap().remove(*field); }
            "null" => value[*field] = Value::Null,
            "type" => value[*field] = json!(false),
            "duplicate" => {
                let encoded = serde_json::to_string(&value).unwrap();
                return format!("{{\"{field}\":{},{}", value[*field], &encoded[1..]).into_bytes();
            }
            _ => panic!("unknown field mutation"),
        },
        NativeSnapshotMutation::Shape(form) => match *form {
            "null" => value = Value::Null,
            "scalar" => value = json!("account"),
            "array" => value = Value::Array(SNAPSHOT_FIELDS.iter().map(|field| value[*field].clone()).collect()),
            "extra" => value["unverified_label"] = json!("not an account member"),
            "usage-array" => value["usage"] = json!([value["usage"]["prompt_tokens"], value["usage"]["completion_tokens"]]),
            "usage-missing" => { value["usage"].as_object_mut().unwrap().remove("completion_tokens"); }
            "usage-extra" => value["usage"]["total_tokens"] = json!(0),
            "usage-duplicate" => {
                let encoded = serde_json::to_string(&value).unwrap();
                let usage = serde_json::to_string(&value["usage"]).unwrap();
                let duplicate = format!("{{\"prompt_tokens\":{},{}", value["usage"]["prompt_tokens"], &usage[1..]);
                return encoded.replacen(&format!("\"usage\":{usage}"), &format!("\"usage\":{duplicate}"), 1).into_bytes();
            }
            _ => panic!("unknown shape mutation"),
        },
    }
    serde_json::to_vec(&value).unwrap()
}

fn native_snapshot_mutations(original: &Value) -> Vec<NativeSnapshotMutation> {
    let mut cases = Vec::new();
    for field in ["contract_id", "network", "provider_wallet", "consumer_wallet", "model", "price", "max_concurrency"] {
        snapshot_term_mutations(&original[field], format!("/{field}"), &mut cases);
    }
    cases.push(NativeSnapshotMutation::Term("/model/serving".into()));
    for field in SNAPSHOT_MONEY {
        for form in ["empty", "zero-prefix", "plus", "negative", "left-space", "right-space",
            "decimal", "exponent", "hex", "unicode", "newline", "tab", "overflow"] {
            cases.push(NativeSnapshotMutation::Money(field, form));
        }
        if field != "waived_wei" { cases.push(NativeSnapshotMutation::Unbalanced(field)); }
    }
    for field in SNAPSHOT_FIELDS {
        for form in ["missing", "null", "type", "duplicate"] {
            cases.push(NativeSnapshotMutation::Field(field, form));
        }
    }
    for form in ["null", "scalar", "array", "extra", "usage-array", "usage-missing", "usage-extra", "usage-duplicate"] {
        cases.push(NativeSnapshotMutation::Shape(form));
    }
    cases
}

#[tokio::test]
async fn native_account_refresh_preserves_original_terms_money_and_recovery_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(&directory.path().join("provider-0"), 66_000);
    let (mut quote, mut opened) = fixture.accounting_account(true).await;
    let original = serde_json::to_value(&opened.account).unwrap();
    let cases = native_snapshot_mutations(&original);

    // Four successful controls plus eight refusals fit the real 12/minute owner
    // challenge budget. Each batch has independent state; never reset a budget,
    // alter an accepted timestamp, sleep for expiry or change a production limit.
    for (batch, cases) in cases.chunks(8).enumerate() {
        if batch != 0 {
            fixture = Fixture::new(&directory.path().join(format!("provider-{batch}")), 66_000 + batch as u64);
            (quote, opened) = fixture.accounting_account(true).await;
        }
        let id = &quote.payload.id;
        let original = serde_json::to_value(&opened.account).unwrap();
        let (config, account_path, journal, _) = restore_native_account(
            &fixture, &quote, &opened, directory.path().join(format!("native-{batch}")));
        let original_journal = native_payment_journal_snapshot(&journal);
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let financial_state = || ledger_snapshot(&db).into_iter().enumerate()
            // Owner-auth rate/clock/challenge bookkeeping is expected to advance.
            .filter(|(table, _)| ![9, 10, 11].contains(table)).map(|(_, rows)| rows).collect::<Vec<_>>();
        let replacement = Arc::new(Mutex::new(None::<Vec<u8>>));
        let delivered = Arc::new(AtomicU64::new(0));
        let app = fixture.app.clone().layer(from_fn({
            let replacement = replacement.clone();
            let delivered = delivered.clone();
            let path = format!("/v1/contracts/{id}");
            move |request: Request, next: Next| {
                let replacement = replacement.clone();
                let delivered = delivered.clone();
                let account_read = request.method() == axum::http::Method::GET && request.uri().path() == path;
                async move {
                    let response = next.run(request).await;
                    if !account_read || response.status() != StatusCode::OK { return response; }
                    delivered.fetch_add(1, Ordering::SeqCst);
                    let bytes = replacement.lock().unwrap().clone();
                    let Some(bytes) = bytes else { return response; };
                    // Modify only a successfully authorized production response.
                    let (mut parts, _) = response.into_parts();
                    parts.headers.remove(header::CONTENT_LENGTH);
                    Response::from_parts(parts, Body::from(bytes))
                }
            }
        }));
        let listener = fixture._listeners[0].try_clone().unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.unwrap(); })
                .await.unwrap();
        });
        let mut consumer = Consumer::new(config.clone()).unwrap();
        let exported = consumer.credential_export(id, None).unwrap();
        assert_eq!(exported["api_key"], opened.credential.as_ref().unwrap().bearer);
        assert_eq!(serde_json::to_value(consumer.account(id).await.unwrap()).unwrap(), original);
        let receipt = consumer.receipt(id).await.unwrap();
        assert_eq!(receipt.payload.sequence, 1);
        let provider_before = financial_state();
        let wide = wide_snapshot(&original);
        *replacement.lock().unwrap() = Some(serde_json::to_vec(&wide).unwrap());
        assert_eq!(serde_json::to_value(consumer.account(id).await.unwrap()).unwrap(), wide);
        let before = std::fs::read(&account_path).unwrap();
        let saved: Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(saved["account"], wide);
        assert_eq!(saved["receipt_sequence"], 1);
        for case in cases {
            *replacement.lock().unwrap() = Some(snapshot_mutation_bytes(case, &original));
            let count = delivered.load(Ordering::SeqCst);
            consumer.account(id).await.expect_err(&format!("{case:?}"));
            assert_eq!(delivered.load(Ordering::SeqCst), count + 1, "must reach snapshot validation: {case:?}");
            assert_eq!(std::fs::read(&account_path).unwrap(), before, "{case:?}");
            assert_eq!(financial_state(), provider_before, "{case:?}");
            assert_eq!(native_payment_journal_snapshot(&journal), original_journal, "{case:?}");
            assert_eq!(consumer.credential_export(id, None).unwrap(), exported, "{case:?}");
            drop(consumer);
            consumer = Consumer::new(config.clone()).unwrap();
            assert_eq!(std::fs::read(&account_path).unwrap(), before, "{case:?}");
            assert_eq!(serde_json::to_value(&consumer.accounts().unwrap()[0]).unwrap(), wide, "{case:?}");
            assert_eq!(consumer.credential_export(id, None).unwrap(), exported, "{case:?}");
        }
        *replacement.lock().unwrap() = None;
        assert_eq!(serde_json::to_value(consumer.account(id).await.unwrap()).unwrap(), original);
        let mut expected = saved;
        expected["account"] = original;
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&account_path).unwrap()).unwrap(), expected);
        assert_eq!(consumer.credential_export(id, None).unwrap(), exported);
        assert_eq!(financial_state(), provider_before);
        assert_eq!(native_payment_journal_snapshot(&journal), original_journal);
        stop.send(()).unwrap();
        server.await.unwrap();
    }
}

#[derive(Clone)]
enum NativeReceiptSnapshotReply {
    Original,
    Wide,
    Fault(usize),
    Replay(Vec<u8>),
}

#[tokio::test]
async fn native_signed_snapshots_preserve_terms_balances_and_receipt_watermarks() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(&directory.path().join("provider-0"), 67_000);
    let (mut quote, mut opened) = fixture.accounting_account(true).await;
    let cases = Arc::new(native_snapshot_mutations(&serde_json::to_value(&opened.account).unwrap()));

    // Seven faults plus original/wide/corrected/replay/recovery requests fit the
    // unchanged twelve-challenge budget. Each batch has independent ledger state.
    for (batch, batch_cases) in cases.chunks(7).enumerate() {
        if batch != 0 {
            fixture = Fixture::new(&directory.path().join(format!("provider-{batch}")), 67_000 + batch as u64);
            (quote, opened) = fixture.accounting_account(true).await;
        }
        let id = &quote.payload.id;
        let original = serde_json::to_value(&opened.account).unwrap();
        let (config, account_path, journal, initial) = restore_native_account(
            &fixture, &quote, &opened, directory.path().join(format!("native-{batch}")));
        let original_journal = native_payment_journal_snapshot(&journal);
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let financial_state = || ledger_snapshot(&db).into_iter().enumerate()
            .filter(|(table, _)| ![9, 10, 11].contains(table)).map(|(_, rows)| rows).collect::<Vec<_>>();
        let assert_success_state = |previous: &Value, receipt: &Signed<UsageReceipt>| {
            let bytes = std::fs::read(&account_path).unwrap();
            let saved: Value = serde_json::from_slice(&bytes).unwrap();
            let returned_account = serde_json::to_value(&receipt.payload.account).unwrap();
            // Do not require a successful receipt to refresh the cache. Either
            // retaining it or adopting the validated snapshot preserves this contract.
            assert!(saved["account"] == previous["account"] || saved["account"] == returned_account);
            let mut expected = previous.clone();
            expected["account"] = saved["account"].clone();
            expected["receipt_sequence"] = json!(receipt.payload.sequence);
            assert_eq!(saved, expected);
            bytes
        };
        let reply = Arc::new(Mutex::new(NativeReceiptSnapshotReply::Original));
        let delivered = Arc::new(AtomicU64::new(0));
        let app = fixture.app.clone().layer(from_fn({
            let reply = reply.clone();
            let delivered = delivered.clone();
            let cases = cases.clone();
            let signer = fixture.provider.state.private_key.clone();
            let provider = fixture.provider.state.wallet.clone();
            let path = format!("/v1/contracts/{id}/receipt");
            move |request: Request, next: Next| {
                let receipt_read = request.method() == axum::http::Method::GET && request.uri().path() == path;
                let reply = reply.clone();
                let delivered = delivered.clone();
                let cases = cases.clone();
                let signer = signer.clone();
                let provider = provider.clone();
                async move {
                    let response = next.run(request).await;
                    if !receipt_read || response.status() != StatusCode::OK { return response; }
                    let mode = reply.lock().unwrap().clone();
                    let (mut parts, body) = response.into_parts();
                    let bytes = to_bytes(body, 96 * 1024).await.unwrap();
                    let mut signed: Signed<UsageReceipt> = serde_json::from_slice(&bytes).unwrap();
                    protocol::verify(RECEIPT_DOMAIN, &signed, &provider).unwrap();
                    let wire = match mode {
                        NativeReceiptSnapshotReply::Original => bytes.to_vec(),
                        NativeReceiptSnapshotReply::Wide => {
                            signed.payload.account = serde_json::from_value(
                                wide_snapshot(&serde_json::to_value(&signed.payload.account).unwrap())).unwrap();
                            let signed = protocol::sign(RECEIPT_DOMAIN, signed.payload, &signer).unwrap();
                            protocol::verify(RECEIPT_DOMAIN, &signed, &provider).unwrap();
                            serde_json::to_vec(&signed).unwrap()
                        }
                        NativeReceiptSnapshotReply::Fault(index) => {
                            let case = &cases[index];
                            let account = snapshot_mutation_bytes(case,
                                &serde_json::to_value(&signed.payload.account).unwrap());
                            if matches!(case, NativeSnapshotMutation::Term(_)
                                | NativeSnapshotMutation::Money(_, _) | NativeSnapshotMutation::Unbalanced(_)) {
                                // Authentic counterparty claims: stale signatures must
                                // not mask original-term or monetary validation.
                                signed.payload.account = serde_json::from_slice(&account).unwrap();
                                let signed = protocol::sign(RECEIPT_DOMAIN, signed.payload, &signer).unwrap();
                                protocol::verify(RECEIPT_DOMAIN, &signed, &provider).unwrap();
                                serde_json::to_vec(&signed).unwrap()
                            } else {
                                // Preserve the original signature and raw duplicate
                                // members. A projecting decoder must not normalize and
                                // authenticate extra members or positional aliases.
                                let member = format!("\"account\":{}", serde_json::to_string(&signed.payload.account).unwrap());
                                serde_json::to_string(&signed).unwrap().replacen(&member,
                                    &format!("\"account\":{}", std::str::from_utf8(&account).unwrap()), 1).into_bytes()
                            }
                        }
                        NativeReceiptSnapshotReply::Replay(wire) => wire,
                    };
                    parts.headers.remove(header::CONTENT_LENGTH);
                    // Count only completed fault construction, not a middleware panic.
                    delivered.fetch_add(1, Ordering::SeqCst);
                    Response::from_parts(parts, Body::from(wire))
                }
            }
        }));
        let listener = fixture._listeners[0].try_clone().unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.unwrap(); })
                .await.unwrap();
        });
        let mut consumer = Consumer::new(config.clone()).unwrap();
        let exported = consumer.credential_export(id, None).unwrap();
        assert_eq!(exported["api_key"], opened.credential.as_ref().unwrap().bearer);
        let first = consumer.receipt(id).await.unwrap();
        assert_eq!(first.payload.sequence, 1);
        assert_eq!(serde_json::to_value(&first.payload.account).unwrap(), original);
        let first_saved: Value = serde_json::from_slice(&assert_success_state(&initial, &first)).unwrap();
        *reply.lock().unwrap() = NativeReceiptSnapshotReply::Wide;
        let wide = consumer.receipt(id).await.unwrap();
        assert_eq!(wide.payload.sequence, 2);
        assert_eq!(serde_json::to_value(&wide.payload.account).unwrap(), wide_snapshot(&original));
        let before = assert_success_state(&first_saved, &wide);
        let saved_before: Value = serde_json::from_slice(&before).unwrap();
        let mut provider_expected = financial_state();
        assert_eq!(provider_expected[0][0][0], *id);
        assert_eq!(provider_expected[0][0][4], 2);
        let mut issued = 2_u64;

        for (offset, case) in batch_cases.iter().enumerate() {
            *reply.lock().unwrap() = NativeReceiptSnapshotReply::Fault(batch * 7 + offset);
            let count = delivered.load(Ordering::SeqCst);
            consumer.receipt(id).await.expect_err(&format!("{case:?}"));
            assert_eq!(delivered.load(Ordering::SeqCst), count + 1, "must reach receipt acceptance: {case:?}");
            issued += 1;
            // Issuance commits at the provider even when native acceptance refuses it.
            provider_expected[0][0][4] = json!(issued);
            assert_eq!(financial_state(), provider_expected, "{case:?}");
            assert_eq!(std::fs::read(&account_path).unwrap(), before, "{case:?}");
            assert_eq!(native_payment_journal_snapshot(&journal), original_journal, "{case:?}");
            assert_eq!(consumer.credential_export(id, None).unwrap(), exported, "{case:?}");
            drop(consumer);
            consumer = Consumer::new(config.clone()).unwrap();
            assert_eq!(std::fs::read(&account_path).unwrap(), before, "{case:?}");
            assert_eq!(serde_json::to_value(&consumer.accounts().unwrap()[0]).unwrap(), saved_before["account"], "{case:?}");
            assert_eq!(consumer.credential_export(id, None).unwrap(), exported, "{case:?}");
        }
        *reply.lock().unwrap() = NativeReceiptSnapshotReply::Original;
        let corrected = consumer.receipt(id).await.unwrap();
        issued += 1;
        assert_eq!(corrected.payload.sequence, issued);
        assert_eq!(serde_json::to_value(&corrected.payload.account).unwrap(), original);
        let accepted = assert_success_state(&saved_before, &corrected);
        provider_expected[0][0][4] = json!(issued);
        assert_eq!(financial_state(), provider_expected);

        drop(consumer);
        consumer = Consumer::new(config.clone()).unwrap();
        *reply.lock().unwrap() = NativeReceiptSnapshotReply::Replay(serde_json::to_vec(&corrected).unwrap());
        let count = delivered.load(Ordering::SeqCst);
        consumer.receipt(id).await.expect_err("accepted replay after reopening");
        assert_eq!(delivered.load(Ordering::SeqCst), count + 1);
        issued += 1;
        provider_expected[0][0][4] = json!(issued);
        assert_eq!(financial_state(), provider_expected);
        assert_eq!(std::fs::read(&account_path).unwrap(), accepted);
        assert_eq!(consumer.credential_export(id, None).unwrap(), exported);
        assert_eq!(native_payment_journal_snapshot(&journal), original_journal);

        *reply.lock().unwrap() = NativeReceiptSnapshotReply::Original;
        let recovered = consumer.receipt(id).await.unwrap();
        issued += 1;
        assert_eq!(recovered.payload.sequence, issued);
        assert_eq!(serde_json::to_value(&recovered.payload.account).unwrap(), original);
        let after = assert_success_state(&serde_json::from_slice(&accepted).unwrap(), &recovered);
        provider_expected[0][0][4] = json!(issued);
        assert_eq!(financial_state(), provider_expected);
        drop(consumer);
        let consumer = Consumer::new(config).unwrap();
        assert_eq!(std::fs::read(&account_path).unwrap(), after);
        assert_eq!(consumer.credential_export(id, None).unwrap(), exported);
        assert_eq!(native_payment_journal_snapshot(&journal), original_journal);
        stop.send(()).unwrap();
        server.await.unwrap();
    }
}

#[tokio::test]
async fn native_receipts_preserve_renewed_evidence_through_settlement_and_reopening() {
    use chiral_network::performance;


    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 65_000);
    let (quote, opened) = fixture.accounting_account(true).await;
    let id = &quote.payload.id;
    let credential = opened.credential.as_ref().unwrap();
    let original = &quote.payload.model;
    let mut renewed = original.clone();
    let qualification = &mut renewed.serving.as_mut().unwrap().qualification;
    qualification.evidence_sha256 = "d".repeat(64);
    qualification.measured_at += 1;
    // Logical ledger times and synthetic reports exercise binding, not qualification.
    let now = qualification.measured_at;
    assert!(original.accepts_execution(&renewed));
    let usage = TokenUsage { prompt_tokens: 1, completion_tokens: 2 };
    let outcome = |model: &protocol::ModelSpec, miss: bool| {
        let profile = model.serving.as_ref().unwrap();
        performance::outcome(profile, true,
            Some(if miss { 2_000_001 } else { 1 }), Some(50_000), Some(&usage),
            "completed", false, Some(2_050_001), &profile.qualification.evidence_sha256)
    };
    for index in 0..3 {
        let ledger = &fixture.provider.state.ledger;
        let reservation = ledger.reserve(id, &credential.id, &format!("old-miss-{index}"),
            1, 2, now, original).unwrap();
        ledger.settle_measured(&reservation.id, &usage, "completed",
            Some(&outcome(original, true)), now).unwrap();
    }
    assert!(fixture.provider.state.ledger.performance_withdrawn(original).unwrap());

    let (config, account_path, journal, mut saved) = restore_native_account(
        &fixture, &quote, &opened, directory.path().join("native-receipts"));
    let original_journal = native_payment_journal_snapshot(&journal);
    let consumer = Consumer::new(config.clone()).unwrap();
    let exported = consumer.credential_export(id, None).unwrap();
    assert_eq!(exported["api_key"], credential.bearer);
    drop(consumer);

    let tamper = Arc::new(Mutex::new(""));
    let delivered = Arc::new(AtomicU64::new(0));
    let listener = fixture._listeners[0].try_clone().unwrap();
    listener.set_nonblocking(true).unwrap();
    let signer = fixture.provider.state.private_key.clone();
    let serve = |app: Router| {
        let tamper = tamper.clone();
        let delivered = delivered.clone();
        let signer = signer.clone();
        let app = app.layer(from_fn(move |request: Request, next: Next| {
            let tamper = tamper.clone();
            let delivered = delivered.clone();
            let signer = signer.clone();
            async move {
                let receipt = request.uri().path().ends_with("/receipt");
                let response = next.run(request).await;
                if !receipt || response.status() != StatusCode::OK { return response; }
                delivered.fetch_add(1, Ordering::SeqCst);
                let mutation = *tamper.lock().unwrap();
                if mutation.is_empty() { return response; }
                // Hostile signed counterparty claims, never mutations of ledger evidence.
                let (mut parts, body) = response.into_parts();
                let bytes = to_bytes(body, 96 * 1024).await.unwrap();
                let mut receipt: Signed<UsageReceipt> = serde_json::from_slice(&bytes).unwrap();
                let measured = receipt.payload.requests[0].performance.as_mut().unwrap();
                match mutation {
                    "short_digest" => measured.evidence_sha256 = "d".repeat(63),
                    "uppercase_digest" => measured.evidence_sha256 = "D".repeat(64),
                    "nonhex_digest" => measured.evidence_sha256 = "g".repeat(64),
                    "policy" => measured.policy = "unqualified".into(),
                    "price" => receipt.payload.account.price.input_wei_per_1k = "2".into(),
                    "signature" => {}
                    _ => panic!("unknown receipt mutation"),
                }
                receipt = protocol::sign(RECEIPT_DOMAIN, receipt.payload, &signer).unwrap();
                if mutation == "signature" { receipt.signature = "00".repeat(65); }
                parts.headers.remove(header::CONTENT_LENGTH);
                Response::from_parts(parts, Body::from(serde_json::to_vec(&receipt).unwrap()))
            }
        }));
        let listener = tokio::net::TcpListener::from_std(listener.try_clone().unwrap()).unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async move { stopped.await.unwrap(); })
                .await.unwrap();
        });
        (stop, server)
    };

    for phase in 0..5 {
        fixture.app = Router::new();
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        state.ledger = Arc::new(BillingStore::open(
            &fixture.provider.ledger_path, &state.config.network.identity).unwrap());
        fixture.app = contracts::router(fixture.provider.state.clone());
        let ledger = &fixture.provider.state.ledger;
        match phase {
            0 => {}
            1 => { ledger.reserve(id, &credential.id, "renewed-known", 1, 2, now, &renewed).unwrap(); }
            2 => {
                let request = ledger.request_view(id, "renewed-known").unwrap();
                ledger.settle_measured(&request.id, &usage, "completed",
                    Some(&outcome(&renewed, false)), now).unwrap();
            }
            3 => {
                let request = ledger.reserve(id, &credential.id, "renewed-waiver",
                    1, 2, now, &renewed).unwrap();
                ledger.settle_measured(&request.id, &usage, "completed",
                    Some(&outcome(&renewed, true)), now).unwrap();
            }
            4 => {
                assert_eq!(ledger.recover_requests(now).unwrap(), 1);
                assert_eq!(ledger.recover_requests(now).unwrap(), 0);
            }
            _ => unreachable!(),
        }
        assert!(ledger.performance_withdrawn(original).unwrap());
        assert!(!ledger.performance_withdrawn(&renewed).unwrap());
        let (stop, server) = serve(fixture.app.clone());
        let mut consumer = Consumer::new(config.clone()).unwrap();
        let receipt = consumer.receipt(id).await
            .unwrap_or_else(|error| panic!("native receipt phase {phase}: {error}"));
        assert_eq!(receipt.payload.sequence, phase + 1);
        assert_eq!(receipt.payload.account.model, *original);
        assert_eq!(receipt.payload.account.price, quote.payload.price);
        assert_eq!(receipt.payload.account.funded_wei, "1000");
        assert_eq!(receipt.payload.account.fee_wei, "5");
        assert_eq!(receipt.payload.account.spent_wei, if phase < 2 { "0" } else { "1" });
        assert_eq!(receipt.payload.account.reserved_wei, if phase == 1 { "1" } else { "0" });
        assert_eq!(receipt.payload.account.available_wei, if phase == 0 { "995" } else { "994" });
        assert_eq!(receipt.payload.account.waived_wei, if phase < 3 { "3" } else { "4" });
        assert_eq!((receipt.payload.account.usage.prompt_tokens, receipt.payload.account.usage.completion_tokens),
            if phase < 2 { (3, 6) } else if phase == 2 { (4, 8) } else { (5, 10) });
        assert_eq!(receipt.payload.requests.len(), [3, 4, 4, 5, 6][phase as usize]);
        for request in &receipt.payload.requests {
            let renewed_request = request.request_id.starts_with("renewed-");
            let measured = request.performance.as_ref().unwrap();
            assert_eq!(measured.evidence_sha256, if renewed_request { "d" } else { "c" }.repeat(64));
            if request.request_id == "renewed-known" {
                assert_eq!(request.status, if phase == 1 { "reserved" } else { "completed" });
                assert_eq!(request.cost_wei, if phase == 1 { "0" } else { "1" });
                assert_eq!(measured.attained, if phase == 1 { None } else { Some(true) });
            } else if request.request_id == "renewed-unknown" {
                assert_eq!(request.status, "restart_unknown_usage");
                assert_eq!((request.usage.prompt_tokens, request.usage.completion_tokens), (0, 0));
                assert_eq!(request.cost_wei, "0");
                assert_eq!(request.waived_wei, "0");
                assert_eq!(request.provider_loss_wei, "1");
                assert_eq!(measured.remedy, "inference_waiver");
            } else {
                assert_eq!((request.usage.prompt_tokens, request.usage.completion_tokens), (1, 2));
                assert_eq!(request.cost_wei, "0");
                assert_eq!(request.waived_wei, "1");
                assert_eq!(measured.remedy, "inference_waiver");
            }
        }
        saved["receipt_sequence"] = json!(receipt.payload.sequence);
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&account_path).unwrap()).unwrap(), saved);
        assert_eq!(consumer.credential_export(id, None).unwrap(), exported);

        if phase == 4 {
            let snapshot = || json!([ledger.account(id).unwrap(), ledger.requests(id, None).unwrap(),
                ledger.credentials(id).unwrap(), ledger.funding_history(id).unwrap()]);
            let before = snapshot();
            let bytes = std::fs::read(&account_path).unwrap();
            for mutation in ["short_digest", "uppercase_digest", "nonhex_digest", "policy", "price", "signature"] {
                *tamper.lock().unwrap() = mutation;
                let count = delivered.load(Ordering::SeqCst);
                consumer.receipt(id).await.expect_err(mutation);
                assert_eq!(delivered.load(Ordering::SeqCst), count + 1, "must reach receipt validation");
                assert_eq!(std::fs::read(&account_path).unwrap(), bytes);
                assert_eq!(snapshot(), before);
                drop(consumer);
                consumer = Consumer::new(config.clone()).unwrap();
                assert_eq!(consumer.credential_export(id, None).unwrap(), exported);
            }
            *tamper.lock().unwrap() = "";
            let corrected = consumer.receipt(id).await.unwrap();
            assert_eq!(corrected.payload.sequence, 12);
            assert_eq!(serde_json::to_value(&corrected.payload.requests).unwrap(),
                serde_json::to_value(&receipt.payload.requests).unwrap());
            assert_eq!(snapshot(), before);
            saved["receipt_sequence"] = json!(12);
            assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&account_path).unwrap()).unwrap(), saved);
        }
        assert_eq!(native_payment_journal_snapshot(&journal), original_journal);
        assert_eq!(serde_json::to_value(ledger.quote(id).unwrap()).unwrap(), serde_json::to_value(&quote).unwrap());
        if phase == 3 {
            ledger.reserve(id, &credential.id, "renewed-unknown", 1, 2, now, &renewed).unwrap();
        }
        drop(consumer);
        stop.send(()).unwrap();
        server.await.unwrap();
    }
}

#[tokio::test]
async fn request_accounting_and_receipts_recover_at_full_quote_storage() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 59_000);
    let other = fixture.open_free().await;
    // Opening credit is a post-verification prerequisite; this test neither runs an
    // engine nor observes chain execution. Settlement inputs stand for validated usage.
    let (opened, _) = fixture.paid_account().await;
    let id = &opened.account.contract_id;
    let now = protocol::now().unwrap();
    let app = fixture.provider.state.ledger.issue_credential(id, &CredentialIssueRequest {
        expires_at: now + 3600, spend_cap_wei: Some("2".into()),
        max_concurrency: 1, scopes: vec!["infer".into()],
    }, now).unwrap();
    let read = fixture.scoped_credential(id, "read");
    let other_before = fixture.credential_state(&other.account.contract_id);
    let mut expected_account = serde_json::to_value(&opened.account).unwrap();
    let mut expected_credentials = serde_json::to_value(
        fixture.provider.state.ledger.credentials(id).unwrap()).unwrap();
    let known = fixture.provider.state.ledger.reserve(id, &app.id, "pressure-known",
        1, 512, now, &opened.account.model).unwrap();
    assert_eq!(known.max_cost_wei, "2");
    let mut expected_known = json!({
        "id":known.id, "request_id":"pressure-known", "contract_id":id, "credential_id":app.id,
        "status":"reserved", "usage":{"prompt_tokens":0,"completion_tokens":0},
        "cost_wei":"0", "max_cost_wei":"2", "provider_loss_wei":"0",
        "prompt_tokens_reserved":1, "max_output_tokens":512,
        "created_at":now, "settled_at":null, "performance":null, "waived_wei":"0"
    });

    let pending = fixture.quote("100").await;
    let mut db = Connection::open(&fixture.provider.ledger_path).unwrap();
    // One genuine quote plus 9,999 complete signed restored records: storage
    // occupancy, not historical issuance or exhausted signing-work/rate budgets.
    restore_full_quote_storage(&mut db, &pending, &fixture.provider.state.private_key);
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
    let protected_state = || [
        "SELECT json_array(id,owner,expires,record) FROM quotes ORDER BY id",
        "SELECT json_array(contract,tx_hash,raw_transaction,ack,quote,sender,nonce,admission_until) FROM funding_intents ORDER BY contract",
        "SELECT json_array(tx_hash,contract,record,evidence,status,checked) FROM funding ORDER BY tx_hash",
        "SELECT json_array(tx_hash,platform,amount,created,status,batch) FROM fee_obligations ORDER BY tx_hash",
        "SELECT json_array(id,record) FROM fee_batches ORDER BY id",
        "SELECT json_array(id,owner,quote,created) FROM accounts ORDER BY id",
        "SELECT json_array(id,contract,token_hash,revoked,json_remove(record,'$.spent_wei','$.reserved_wei')) FROM credentials ORDER BY id",
    ].map(&fingerprint);
    let quote_rates = || fingerprint(
        "SELECT json_array(bucket,window,count) FROM rate_limits WHERE bucket LIKE 'quote:%' ORDER BY bucket");
    let full_quotes = || {
        assert_eq!(db.query_row("SELECT COUNT(*) FROM quotes WHERE expires>?1",
            [protocol::now().unwrap()], |row| row.get::<_, u64>(0)).unwrap(), 10_000);
    };
    let protected = protected_state();
    assert_eq!(protected.map(|row| row.0), [10_000, 1, 1, 0, 0, 2, 4]);
    let rates_before = quote_rates();
    full_quotes();
    assert!(rate_records(&fixture.provider.ledger_path).iter()
        .filter(|(bucket, _, _)| bucket.starts_with("quote:"))
        .all(|(bucket, _, count)| bucket == "quote:global" && *count < 600));
    let (status, refused) = fixture.post("/v1/contracts/propose", json!({
        "offer_id":fixture.provider.offer_id,
        "consumer_wallet":wallet::address_from_private_key(OTHER_KEY).unwrap(),"funding_wei":"100"
    })).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
    assert_eq!(refused["error"]["code"], "rate_limit");
    assert_eq!(protected_state(), protected);
    assert_eq!(quote_rates(), rates_before);

    // Fresh ledger reservations remain independent of quote storage, while their
    // own scope, balance, concurrency and cap checks remain in force.
    let unknown = fixture.provider.state.ledger.reserve(id, &opened.credential.as_ref().unwrap().id,
        "pressure-unknown", 1000, 512, now, &opened.account.model).unwrap();
    assert_eq!(unknown.max_cost_wei, "3");
    let mut expected_unknown = json!({
        "id":unknown.id, "request_id":"pressure-unknown", "contract_id":id,
        "credential_id":opened.credential.as_ref().unwrap().id, "status":"reserved",
        "usage":{"prompt_tokens":0,"completion_tokens":0},
        "cost_wei":"0", "max_cost_wei":"3", "provider_loss_wei":"0",
        "prompt_tokens_reserved":1000, "max_output_tokens":512,
        "created_at":now, "settled_at":null, "performance":null, "waived_wei":"0"
    });
    expected_account["available_wei"] = json!("95");
    expected_account["reserved_wei"] = json!("5");
    for credential in expected_credentials.as_array_mut().unwrap() {
        if credential["id"] == app.id { credential["reserved_wei"] = json!("2"); }
        if credential["id"] == opened.credential.as_ref().unwrap().id { credential["reserved_wei"] = json!("3"); }
    }
    for (credential, key, error) in [
        (&read, "read-cannot-infer", "forbidden:"),
        (opened.credential.as_ref().unwrap(), "account-saturated", "saturation:"),
        (&app, "application-saturated", "saturation:"),
    ] {
        let refused = fixture.provider.state.ledger.reserve(id, &credential.id, key,
            1, 512, now, &opened.account.model).unwrap_err();
        assert!(refused.starts_with(error), "{refused}");
        assert!(fixture.provider.state.ledger.request_view(id, key).unwrap_err().starts_with("not_found:"));
    }
    fixture.provider.state.accepting.store(false, Ordering::Release);
    fixture.provider.state.funding_ready.store(false, Ordering::Release);
    fixture.provider.state.qualified_models.write().unwrap().clear();
    assert_eq!(quote_rates(), rates_before);
    let usage = TokenUsage { prompt_tokens: 1, completion_tokens: 1 };
    for phase in 0..3 {
        let before_accounting = quote_rates();
        // Reopen the real ledger/router before each transition. Explicit invocation
        // of startup's recovery method is not a daemon/process-kill experiment.
        fixture.app = Router::new();
        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
        state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path,
            &state.config.network.identity).unwrap());
        fixture.app = contracts::router(fixture.provider.state.clone());
        full_quotes();
        if phase == 1 {
            expected_known["status"] = json!("completed");
            expected_known["usage"] = serde_json::to_value(&usage).unwrap();
            expected_known["cost_wei"] = json!("1");
            expected_known["settled_at"] = json!(now + 1);
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger
                .settle(&known.id, &usage, "completed", now + 1).unwrap()).unwrap(), expected_known);
            expected_account["available_wei"] = json!("96");
            expected_account["reserved_wei"] = json!("3");
            expected_account["spent_wei"] = json!("1");
            expected_account["usage"] = serde_json::to_value(&usage).unwrap();
            for credential in expected_credentials.as_array_mut().unwrap() {
                if credential["id"] == app.id {
                    credential["reserved_wei"] = json!("0");
                    credential["spent_wei"] = json!("1");
                }
            }
            let refused = fixture.provider.state.ledger.reserve(id, &app.id, "cap-after-spend",
                1, 512, now + 1, &opened.account.model).unwrap_err();
            assert!(refused.starts_with("spend_cap:"), "{refused}");
            assert!(fixture.provider.state.ledger.request_view(id, "cap-after-spend").is_err());
        } else if phase == 2 {
            assert_eq!(fixture.provider.state.ledger.recover_requests(now + 2).unwrap(), 1);
            assert_eq!(fixture.provider.state.ledger.recover_requests(now + 3).unwrap(), 0);
            expected_unknown["status"] = json!("restart_unknown_usage");
            expected_unknown["provider_loss_wei"] = json!("3");
            expected_unknown["settled_at"] = json!(now + 2);
            expected_account["available_wei"] = json!("99");
            expected_account["reserved_wei"] = json!("0");
            for credential in expected_credentials.as_array_mut().unwrap() {
                if credential["id"] == opened.credential.as_ref().unwrap().id { credential["reserved_wei"] = json!("0"); }
            }
            // Late plausible usage cannot turn an unknown terminal result into a debit.
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger.settle(&unknown.id,
                &TokenUsage { prompt_tokens:1000, completion_tokens:512 },
                "completed", now + 4).unwrap()).unwrap(), expected_unknown);
        }
        if phase > 0 {
            assert_eq!(serde_json::to_value(fixture.provider.state.ledger.settle(&known.id,
                &TokenUsage { prompt_tokens:1, completion_tokens:512 },
                "cancelled", now + 4).unwrap()).unwrap(), expected_known);
        }
        for (key, credential) in [("pressure-known", &app), ("pressure-unknown", opened.credential.as_ref().unwrap())] {
            let refused = fixture.provider.state.ledger.reserve(id, &credential.id, key,
                1, 1, now + 4, &opened.account.model).unwrap_err();
            assert!(refused.starts_with("duplicate_request:"), "{refused}");
        }
        assert_eq!(quote_rates(), before_accounting);
        let expected_requests = json!([expected_unknown, expected_known]);
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.account(id).unwrap()).unwrap(), expected_account);
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.credentials(id).unwrap()).unwrap(), expected_credentials);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM requests", [], |row| row.get::<_, u64>(0)).unwrap(), 2);

        let base = format!("/v1/contracts/{id}");
        for (path, expected) in [
            (base.clone(), expected_account.clone()),
            (format!("{base}/credentials"), expected_credentials.clone()),
            (format!("{base}/requests"), expected_requests.clone()),
            (format!("{base}/requests/{}", known.id), expected_known.clone()),
            (format!("{base}/requests/pressure-unknown"), expected_unknown.clone()),
        ] {
            let (status, value) = fixture.send("GET", &path, &bearer(&read), Vec::new()).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value, expected);
        }
        let request_path = format!("{base}/requests/pressure-known");
        let proof = fixture.proof(OWNER_KEY, "GET", &request_path, b"").await;
        let (status, value) = fixture.send("GET", &request_path, &proof.headers, Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value, expected_known);
        fixture.consumed(&proof);
        denied(fixture.send("GET", &request_path, &proof.headers, Vec::new()).await, StatusCode::UNAUTHORIZED);
        let receipt_path = format!("{base}/receipt");
        for path in [&request_path, &receipt_path] {
            denied(fixture.send("GET", path, &HeaderMap::new(), Vec::new()).await, StatusCode::UNAUTHORIZED);
            for credential in [other.credential.as_ref().unwrap(), &app] {
                denied(fixture.send("GET", path, &bearer(credential), Vec::new()).await, StatusCode::FORBIDDEN);
            }
        }
        // Both authorization forms recover the same accounting; each receipt alone
        // advances the persistent sequence, never money, request evidence or authority.
        let proof = fixture.proof(OWNER_KEY, "GET", &receipt_path, b"").await;
        let before_reads = quote_rates();
        for (index, headers) in [bearer(&read), proof.headers].into_iter().enumerate() {
            let (status, value) = fixture.send("GET", &receipt_path, &headers, Vec::new()).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            let receipt: Signed<UsageReceipt> = serde_json::from_value(value).unwrap();
            protocol::verify(RECEIPT_DOMAIN, &receipt, &fixture.provider.state.wallet).unwrap();
            assert_eq!(receipt.payload.network, fixture.provider.state.config.network.identity);
            assert_eq!(receipt.payload.provider_wallet, fixture.provider.state.wallet);
            assert_eq!(serde_json::to_value(&receipt.payload.account).unwrap(), expected_account);
            assert_eq!(serde_json::to_value(&receipt.payload.requests).unwrap(), expected_requests);
            assert_eq!(receipt.payload.sequence, phase * 2 + index as u64 + 1);
        }
        assert_eq!(db.query_row("SELECT sequence FROM accounts WHERE id=?1", [id],
            |row| row.get::<_, u64>(0)).unwrap(), (phase + 1) * 2);
        assert_eq!(fixture.credential_state(id), json!({"account":expected_account,"credentials":expected_credentials}));
        assert_eq!(fixture.credential_state(&other.account.contract_id), other_before);
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.requests(id, None).unwrap()).unwrap(), expected_requests);
        fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, &read, other.credential.as_ref().unwrap()]);
        assert_eq!(protected_state(), protected);
        assert_eq!(quote_rates(), before_reads);
        assert_eq!(db.query_row("SELECT sequence FROM accounts WHERE id=?1",
            [&other.account.contract_id], |row| row.get::<_, u64>(0)).unwrap(), 0);
        full_quotes();
    }
    let journal = Connection::open(fixture.provider.state.config.network
        .scoped_dir(&fixture.provider.state.config.data_dir).join("wallet-journal/transactions.sqlite3")).unwrap();
    let operations: (u64, u64) = journal.query_row(
        "SELECT (SELECT COUNT(*) FROM transactions),(SELECT COUNT(*) FROM cancellations)",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(operations, (0, 0));
}

#[tokio::test]
async fn reservation_write_failures_roll_back_request_and_both_holds() {
    const KEY: &str = "reservation-write-fault";
    let directory = tempfile::tempdir().unwrap();
    for (index, (point, event)) in [
        ("request-insert", "AFTER INSERT ON requests"),
        ("account-hold", "AFTER UPDATE ON accounts"),
        ("credential-hold", "AFTER UPDATE ON credentials"),
    ].into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 60_000 + index as u64);
        let other = fixture.open_free().await;
        // Opening credit is injected after payment verification for setup only.
        // Supplied settlement counts are not observed engine usage.
        let (opened, _) = fixture.paid_account().await;
        let id = &opened.account.contract_id;
        let now = protocol::now().unwrap();
        let app = fixture.provider.state.ledger.issue_credential(id, &CredentialIssueRequest {
            expires_at: now + 3600, spend_cap_wei: Some("2".into()),
            max_concurrency: 1, scopes: vec!["infer".into()],
        }, now).unwrap();
        let ledger = &fixture.provider.state.ledger;
        let usage = TokenUsage { prompt_tokens: 1, completion_tokens: 1 };
        let unrelated = ledger.reserve(&other.account.contract_id, &other.credential.as_ref().unwrap().id,
            KEY, 1, 512, now, &other.account.model).unwrap();
        ledger.settle(&unrelated.id, &usage, "completed", now).unwrap();
        let settled = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "prior-terminal",
            1, 512, now, &opened.account.model).unwrap();
        assert_eq!(ledger.settle(&settled.id, &usage, "completed", now).unwrap().cost_wei, "1");
        let held = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "prior-reservation",
            1000, 512, now, &opened.account.model).unwrap();
        assert_eq!(held.max_cost_wei, "3");
        let account = ledger.account(id).unwrap();
        assert_eq!((&account.available_wei[..], &account.reserved_wei[..], &account.spent_wei[..]),
            ("96", "3", "1"));
        let pending = fixture.quote("100").await;
        let db = Connection::open(&fixture.provider.ledger_path).unwrap();
        let snapshot = || ledger_snapshot(&db);
        let before = snapshot();
        assert_eq!((before[0].len(), before[1].len(), before[2].len()), (2, 3, 3));
        assert_eq!(before[3][0][0], pending.payload.id);
        let condition = match point {
            "request-insert" => format!("NEW.contract='{id}' AND NEW.request_key='{KEY}'"),
            "account-hold" => format!("NEW.id='{id}'"),
            "credential-hold" => format!("NEW.id='{}'", app.id),
            _ => unreachable!(),
        };
        // AFTER/FAIL retains the failed statement's mutation in the transaction;
        // only rollback of the production transaction can restore this snapshot.
        db.execute_batch(&format!(
            "CREATE TRIGGER fail_reservation_write {event} WHEN {condition}
             BEGIN SELECT RAISE(FAIL,'injected reservation {point}'); END;"
        )).unwrap();
        for reopened in [false, true] {
            if reopened {
                fixture.app = Router::new();
                let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path,
                    &state.config.network.identity).unwrap());
                fixture.app = contracts::router(fixture.provider.state.clone());
            }
            assert_eq!(snapshot(), before, "{point}, reopened={reopened}");
            let error = fixture.provider.state.ledger.reserve(id, &app.id, KEY,
                1, 512, now, &opened.account.model).unwrap_err();
            assert!(error.contains(&format!("injected reservation {point}")),
                "{point}, reopened={reopened}: {error}");
            assert_eq!(snapshot(), before, "{point}, reopened={reopened}");
            assert!(fixture.provider.state.ledger.request_view(id, KEY)
                .unwrap_err().starts_with("not_found:"));
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
            assert_eq!(snapshot(), before, "{point}: reads must preserve rolled-back state");
        }

        // Explicit fixture repair, not an automatic production retry policy.
        db.execute_batch("DROP TRIGGER fail_reservation_write").unwrap();
        let reserved = fixture.provider.state.ledger.reserve(id, &app.id, KEY,
            1, 512, now, &opened.account.model).unwrap();
        assert_eq!(reserved.max_cost_wei, "2");
        let request = json!({
            "id":reserved.id, "request_id":KEY, "contract_id":id, "credential_id":app.id,
            "status":"reserved", "usage":{"prompt_tokens":0,"completion_tokens":0},
            "cost_wei":"0", "max_cost_wei":"2", "provider_loss_wei":"0",
            "prompt_tokens_reserved":1, "max_output_tokens":512, "created_at":now,
            "settled_at":null, "performance":null, "waived_wei":"0"
        });
        let mut expected = before.clone();
        let account = expected[0].iter_mut().find(|row| row[0] == *id).unwrap();
        account[3]["available_wei"] = json!("94");
        account[3]["reserved_wei"] = json!("5");
        let credential = expected[1].iter_mut().find(|row| row[0] == app.id).unwrap();
        credential[3]["reserved_wei"] = json!("2");
        expected[2].push(json!([reserved.id, id, KEY, app.id, "reserved", now, request]));
        expected[2].sort_by(|a, b| a[0].as_str().unwrap().cmp(b[0].as_str().unwrap()));

        for reopened in [false, true] {
            if reopened {
                fixture.app = Router::new();
                let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path,
                    &state.config.network.identity).unwrap());
                fixture.app = contracts::router(fixture.provider.state.clone());
            }
            assert_eq!(snapshot(), expected, "{point}, accepted, reopened={reopened}");
            let ledger = &fixture.provider.state.ledger;
            assert_eq!(serde_json::to_value(ledger.request_view(id, KEY).unwrap()).unwrap(), request);
            assert_eq!(serde_json::to_value(ledger.request_view(id, &reserved.id).unwrap()).unwrap(), request);
            assert!(ledger.reserve(id, &app.id, KEY, 1, 512, now, &opened.account.model)
                .unwrap_err().starts_with("duplicate_request:"));
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
            assert_eq!(snapshot(), expected, "{point}: duplicate must not add another hold");
        }
    }
}

#[tokio::test]
async fn terminal_write_failures_preserve_holds_until_one_outcome_commits() {
    terminal_failure_matrix(&[
        ("account-release", "AFTER UPDATE ON accounts"),
        ("credential-release", "AFTER UPDATE ON credentials"),
        ("request-terminal", "AFTER UPDATE ON requests"),
    ]).await;
}

#[tokio::test]
async fn terminal_commit_refusal_preserves_original_accounting_recovery() {
    terminal_failure_matrix(&[("commit", "AFTER UPDATE ON requests")]).await;
}

async fn terminal_failure_matrix(faults: &[(&str, &str)]) {
    const KEY: &str = "terminal-write-fault";
    let directory = tempfile::tempdir().unwrap();
    for (mode_index, (mode, supplied_usage, status, terminal_status)) in [
        ("known", TokenUsage { prompt_tokens: 1, completion_tokens: 1 },
            "completed", "completed"),
        ("unknown", TokenUsage::default(), "backend_unknown_usage", "backend_unknown_usage"),
        ("invalid", TokenUsage { prompt_tokens: 1, completion_tokens: 513 },
            "completed", "invalid_usage_unknown_usage"),
    ].into_iter().enumerate() {
        for (point_index, (point, event)) in faults.iter().copied().enumerate() {
            let mut fixture = Fixture::new(directory.path(),
                61_000 + (mode_index * faults.len() + point_index) as u64);
            let other = fixture.open_free().await;
            // Post-verification opening credit and supplied usage are prerequisites,
            // not observed chain execution, cancellation or private-engine evidence.
            let (opened, _) = fixture.paid_account().await;
            let id = &opened.account.contract_id;
            let now = protocol::now().unwrap();
            let app = fixture.provider.state.ledger.issue_credential(id, &CredentialIssueRequest {
                expires_at: now + 3600, spend_cap_wei: Some("2".into()),
                max_concurrency: 1, scopes: vec!["infer".into()],
            }, now).unwrap();
            let ledger = &fixture.provider.state.ledger;
            let prior_usage = TokenUsage { prompt_tokens: 1, completion_tokens: 1 };
            let unrelated = ledger.reserve(&other.account.contract_id, &other.credential.as_ref().unwrap().id,
                KEY, 1, 512, now, &other.account.model).unwrap();
            ledger.settle(&unrelated.id, &prior_usage, "completed", now).unwrap();
            let prior = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "prior-terminal",
                1, 512, now, &opened.account.model).unwrap();
            assert_eq!(ledger.settle(&prior.id, &prior_usage, "completed", now).unwrap().cost_wei, "1");
            let held = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "prior-reservation",
                1000, 512, now, &opened.account.model).unwrap();
            assert_eq!(held.max_cost_wei, "3");
            let target = ledger.reserve(id, &app.id, KEY,
                1, 512, now, &opened.account.model).unwrap();
            assert_eq!(target.max_cost_wei, "2");
            let account = ledger.account(id).unwrap();
            assert_eq!((&account.available_wei[..], &account.reserved_wei[..], &account.spent_wei[..]),
                ("94", "5", "1"));
            let pending = fixture.quote("100").await;
            let db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let before = ledger_snapshot(&db);
            assert_eq!((before[0].len(), before[1].len(), before[2].len()), (2, 3, 4));
            assert_eq!(before[3][0][0], pending.payload.id);
            let target_id = match point {
                "account-release" => id,
                "credential-release" => &app.id,
                "request-terminal" | "commit" => &target.id,
                _ => unreachable!(),
            };
            if point == "commit" {
                // This deferred constraint permits every settlement statement.
                // Only COMMIT rejects the missing parent; there is no RAISE.
                // Keep a witness of all three writes in that same transaction.
                db.execute_batch(&format!(
                    "PRAGMA foreign_keys=ON;
                     CREATE TABLE terminal_commit_parent(request_id TEXT PRIMARY KEY);
                     CREATE TABLE terminal_commit_witness(
                         request_id TEXT PRIMARY KEY REFERENCES terminal_commit_parent(request_id)
                             DEFERRABLE INITIALLY DEFERRED,
                         account TEXT NOT NULL, credential TEXT NOT NULL, request TEXT NOT NULL);
                     CREATE TRIGGER fail_terminal_commit {event} WHEN NEW.id='{target_id}'
                     BEGIN
                         INSERT INTO terminal_commit_witness(request_id,account,credential,request)
                         SELECT NEW.id,
                             (SELECT account FROM accounts WHERE id=NEW.contract),
                             (SELECT record FROM credentials WHERE id=NEW.credential),
                             NEW.record;
                     END;"
                )).unwrap();
            } else {
                // FAIL retains the statement's writes inside the transaction. A
                // persistence error must roll back, unlike committed invalid usage.
                db.execute_batch(&format!(
                    "CREATE TRIGGER fail_terminal_write {event} WHEN NEW.id='{target_id}'
                     BEGIN SELECT RAISE(FAIL,'injected terminal {point}'); END;"
                )).unwrap();
            }
            let commit_witness = || {
                db.prepare("SELECT json_array(request_id,json(account),json(credential),json(request))
                    FROM terminal_commit_witness ORDER BY request_id").unwrap()
                    .query_map([], |row| row.get::<_, String>(0)).unwrap()
                    .map(|row| serde_json::from_str::<Value>(&row.unwrap()).unwrap())
                    .collect::<Vec<_>>()
            };
            if point == "commit" {
                assert!(commit_witness().is_empty());
            }
            for reopened in [false, true] {
                if reopened {
                    fixture.app = Router::new();
                    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path,
                        &state.config.network.identity).unwrap());
                    fixture.app = contracts::router(fixture.provider.state.clone());
                }
                assert_eq!(ledger_snapshot(&db), before, "{mode}/{point}, reopened={reopened}");
                let error = fixture.provider.state.ledger.settle(&target.id, &supplied_usage,
                    status, now + 1).unwrap_err();
                if point == "commit" {
                    assert!(error.contains("FOREIGN KEY constraint failed"),
                        "{mode}/{point}, reopened={reopened}: {error}");
                } else {
                    assert!(error.contains(&format!("injected terminal {point}")),
                        "{mode}/{point}, reopened={reopened}: {error}");
                }
                assert_eq!(ledger_snapshot(&db), before, "{mode}/{point}, reopened={reopened}");
                assert_eq!(fixture.provider.state.ledger.request_view(id, KEY).unwrap().status, "reserved");
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
                assert_eq!(ledger_snapshot(&db), before, "{mode}/{point}: retained holds and authority");
                if point == "commit" {
                    assert!(commit_witness().is_empty(), "{mode}: rejected commit left a witness");
                    assert_eq!(db.query_row("SELECT COUNT(*) FROM terminal_commit_parent", [],
                        |row| row.get::<_, u64>(0)).unwrap(), 0);
                }
            }

            // Repairing the fixture fault authorizes no automatic inference replay.
            // Reconcile the original reservation and keep its unrelated root hold.
            if point == "commit" {
                // Leave the trigger active: successful settlement must atomically
                // retain its witness, not bypass the previously failing write.
                assert_eq!(db.execute("INSERT INTO terminal_commit_parent(request_id) VALUES(?1)",
                    [&target.id]).unwrap(), 1);
            } else {
                db.execute_batch("DROP TRIGGER fail_terminal_write").unwrap();
            }
            let known = mode == "known";
            let request = json!({
                "id":target.id, "request_id":KEY, "contract_id":id, "credential_id":app.id,
                "status":terminal_status,
                "usage":{"prompt_tokens":if known { 1 } else { 0 },
                    "completion_tokens":if known { 1 } else { 0 }},
                "cost_wei":if known { "1" } else { "0" }, "max_cost_wei":"2",
                "provider_loss_wei":if known { "0" } else { "2" },
                "prompt_tokens_reserved":1, "max_output_tokens":512, "created_at":now,
                "settled_at":now + 1, "performance":null, "waived_wei":"0"
            });
            let mut expected = before.clone();
            let account = expected[0].iter_mut().find(|row| row[0] == *id).unwrap();
            account[3]["available_wei"] = json!(if known { "95" } else { "96" });
            account[3]["reserved_wei"] = json!("3");
            account[3]["spent_wei"] = json!(if known { "2" } else { "1" });
            account[3]["usage"] = json!({"prompt_tokens":if known { 2 } else { 1 },
                "completion_tokens":if known { 2 } else { 1 }});
            let credential = expected[1].iter_mut().find(|row| row[0] == app.id).unwrap();
            credential[3]["reserved_wei"] = json!("0");
            credential[3]["spent_wei"] = json!(if known { "1" } else { "0" });
            let record = expected[2].iter_mut().find(|row| row[0] == target.id).unwrap();
            record[4] = json!(terminal_status);
            record[6] = request.clone();
            let expected_commit_witness = (point == "commit").then(|| json!([
                target.id,
                expected[0].iter().find(|row| row[0] == *id).unwrap()[3],
                expected[1].iter().find(|row| row[0] == app.id).unwrap()[3],
                request
            ]));
            let result = fixture.provider.state.ledger.settle(&target.id, &supplied_usage,
                status, now + 1);
            if mode == "invalid" {
                assert!(result.unwrap_err().starts_with("usage_over_reservation:"));
            } else {
                assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), request);
            }
            assert_eq!(ledger_snapshot(&db), expected, "{mode}/{point}: first committed outcome");
            if let Some(witness) = &expected_commit_witness {
                assert_eq!(commit_witness().as_slice(), std::slice::from_ref(witness));
            }

            for reopened in [false, true] {
                if reopened {
                    fixture.app = Router::new();
                    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path,
                        &state.config.network.identity).unwrap());
                    fixture.app = contracts::router(fixture.provider.state.clone());
                }
                assert_eq!(ledger_snapshot(&db), expected, "{mode}/{point}, terminal, reopened={reopened}");
                let ledger = &fixture.provider.state.ledger;
                for lookup in [KEY, target.id.as_str()] {
                    assert_eq!(serde_json::to_value(ledger.request_view(id, lookup).unwrap()).unwrap(), request);
                }
                for (usage, later_status, offset) in [
                    (TokenUsage { prompt_tokens: 1, completion_tokens: 512 }, "cancelled", 2),
                    (TokenUsage::default(), "restart_unknown_usage", 3),
                    (TokenUsage { prompt_tokens: 1, completion_tokens: 513 }, "completed", 4),
                ] {
                    let replay = ledger.settle(&target.id, &usage, later_status, now + offset).unwrap();
                    assert_eq!(serde_json::to_value(replay).unwrap(), request);
                    assert_eq!(ledger_snapshot(&db), expected, "{mode}/{point}: first result must win");
                }
                assert!(ledger.reserve(id, &app.id, KEY, 1, 512, now + 5, &opened.account.model)
                    .unwrap_err().starts_with("duplicate_request:"));
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
                assert_eq!(ledger_snapshot(&db), expected, "{mode}/{point}: no new charge or release");
                if let Some(witness) = &expected_commit_witness {
                    assert_eq!(commit_witness().as_slice(), std::slice::from_ref(witness),
                        "{mode}: terminal replay must preserve the committed witness");
                    assert_eq!(db.query_row("SELECT request_id FROM terminal_commit_parent", [],
                        |row| row.get::<_, String>(0)).unwrap(), target.id);
                }
            }
        }
    }
}

#[tokio::test]
async fn performance_write_failures_preserve_bound_waivers_and_withdrawal_latches() {
    use chiral_network::performance;

    const KEY: &str = "performance-write-fault";
    let directory = tempfile::tempdir().unwrap();
    let outcome = |model: &protocol::ModelSpec, usage: Option<&TokenUsage>,
        status: &str, ttft: Option<u64>| {
        let profile = model.serving.as_ref().unwrap();
        performance::outcome(profile, true, ttft, ttft.map(|_| 1_000_000), usage,
            status, false, ttft.map(|time| time + 1_000_000),
            &profile.qualification.evidence_sha256)
    };
    for (mode_index, mode) in ["known", "unknown", "invalid"].into_iter().enumerate() {
        for (point_index, point) in ["observation", "withdrawal", "commit"].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(),
                62_000 + (mode_index * 3 + point_index) as u64);
            let other = fixture.open_free().await;
            let (quote, opened) = fixture.accounting_account(true).await;
            let now = protocol::now().unwrap();
            let id = &opened.account.contract_id;
            let original = &quote.payload.model;
            let mut current = original.clone();
            let qualification = &mut current.serving.as_mut().unwrap().qualification;
            qualification.evidence_sha256 = "d".repeat(64);
            qualification.measured_at = now + 1;
            let evidence = &current.serving.as_ref().unwrap().qualification.evidence_sha256;
            let ledger = &fixture.provider.state.ledger;
            let unrelated = ledger.reserve(&other.account.contract_id, &other.credential.as_ref().unwrap().id,
                KEY, 1, 1, now, &other.account.model).unwrap();
            ledger.settle(&unrelated.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 },
                "completed", now).unwrap();
            let historical_usage = TokenUsage { prompt_tokens: 1, completion_tokens: 256 };
            let mut observations = Vec::new();
            // Withdraw the original report, then retain one charge and two misses
            // under an otherwise identical newer report. The target is miss three.
            for (index, (model, missed)) in [
                (original, true), (original, true), (original, true),
                (&current, false), (&current, true), (&current, true),
            ].into_iter().enumerate() {
                let reservation = ledger.reserve(id, &opened.credential.as_ref().unwrap().id,
                    &format!("performance-history-{index}"), 1, 256, now + 2, model).unwrap();
                let measured = outcome(model, Some(&historical_usage), "completed",
                    Some(if missed { 2_000_001 } else { 1_000_000 }));
                let settled = ledger.settle_measured(&reservation.id, &historical_usage,
                    "completed", Some(&measured), now + 3).unwrap();
                assert_eq!(settled.cost_wei, if missed { "0" } else { "1" });
                assert_eq!(settled.waived_wei, if missed { "1" } else { "0" });
                assert_eq!((settled.usage.prompt_tokens, settled.usage.completion_tokens), (1, 256));
                assert_eq!(settled.performance.as_ref(), Some(&measured));
                observations.push(json!([index + 1, reservation.id, original.id,
                    measured.evidence_sha256, if missed { 1 } else { 0 }]));
            }
            assert!(ledger.performance_withdrawn(original).unwrap());
            assert!(!ledger.performance_withdrawn(&current).unwrap());
            let app = ledger.issue_credential(id, &CredentialIssueRequest {
                expires_at: now + 3600, spend_cap_wei: Some("2".into()),
                max_concurrency: 1, scopes: vec!["infer".into()],
            }, now + 4).unwrap();
            let live = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "performance-live",
                2048, 256, now + 4, &current).unwrap();
            let target = ledger.reserve(id, &app.id, KEY, 1000, 256, now + 4, &current).unwrap();
            assert_eq!(live.max_cost_wei, "3");
            assert_eq!(target.max_cost_wei, "2");
            assert_eq!(target.model, current);
            let account = ledger.account(id).unwrap();
            assert_eq!(account.model, *original);
            assert_eq!(account.price, quote.payload.price);
            assert_eq!((account.funded_wei.as_str(), account.fee_wei.as_str(),
                account.available_wei.as_str(), account.reserved_wei.as_str(),
                account.spent_wei.as_str(), account.waived_wei.as_str()),
                ("1000", "5", "989", "5", "1", "5"));
            assert_eq!((account.usage.prompt_tokens, account.usage.completion_tokens), (6, 1536));
            let db = Connection::open(&fixture.provider.ledger_path).unwrap();
            let before = ledger_snapshot(&db);
            assert_eq!(before[12], observations);
            assert_eq!(before[13], vec![json!([original.id, "c".repeat(64)])]);
            assert_eq!(before[7].len(), 1);
            assert_eq!(before[7][0][2], "5");
            let pending = ledger.request_view(id, KEY).unwrap();
            assert_eq!(pending.performance.as_ref().unwrap().evidence_sha256, *evidence);
            assert_eq!(pending.performance.as_ref().unwrap().eligibility, "pending");
            let known = mode == "known";
            let usage = if mode == "unknown" { TokenUsage::default() } else {
                TokenUsage { prompt_tokens: 1000, completion_tokens: if known { 128 } else { 257 } }
            };
            let status = if mode == "unknown" { "backend_unknown_usage" } else { "completed" };
            let measured = if known {
                outcome(&current, Some(&usage), status, Some(2_000_001))
            } else {
                outcome(&current, None, "backend_unknown_usage", None)
            };
            assert_eq!(measured.attained, Some(false));
            assert_eq!(measured.remedy, "inference_waiver");
            assert_eq!(measured.output_tokens, if known { Some(128) } else { None });
            if point == "commit" {
                // This AFTER-latch trigger allows all five production writes;
                // its deferred constraint rejects only COMMIT.
                db.execute_batch(&format!(
                    "PRAGMA foreign_keys=ON;
                     CREATE TABLE performance_commit_parent(id TEXT PRIMARY KEY);
                     CREATE TABLE performance_commit_witness(
                         id TEXT PRIMARY KEY REFERENCES performance_commit_parent(id)
                             DEFERRABLE INITIALLY DEFERRED);
                     CREATE TRIGGER fail_performance_commit AFTER INSERT ON performance_withdrawals
                     WHEN NEW.model='{}' AND NEW.evidence='{evidence}'
                     BEGIN INSERT INTO performance_commit_witness(id) VALUES('{}'); END;",
                    original.id, target.id
                )).unwrap();
            } else {
                let (table, predicate) = if point == "observation" {
                    ("performance_observations", format!("NEW.request='{}'", target.id))
                } else {
                    ("performance_withdrawals", format!("NEW.model='{}' AND NEW.evidence='{evidence}'", original.id))
                };
                db.execute_batch(&format!(
                    "CREATE TRIGGER fail_performance_write AFTER INSERT ON {table}
                     WHEN {predicate} BEGIN SELECT RAISE(FAIL,'injected performance {point}'); END;"
                )).unwrap();
            }
            for reopened in [false, true] {
                if reopened {
                    fixture.app = Router::new();
                    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path,
                        &state.config.network.identity).unwrap());
                    fixture.app = contracts::router(fixture.provider.state.clone());
                }
                let ledger = &fixture.provider.state.ledger;
                assert_eq!(ledger_snapshot(&db), before);
                for wrong_policy in [false, true] {
                    let mut wrong = measured.clone();
                    if wrong_policy { wrong.policy = "unqualified".into(); }
                    else { wrong.evidence_sha256 = "c".repeat(64); }
                    assert!(ledger.settle_measured(&target.id, &usage, status, Some(&wrong),
                        now + 5).unwrap_err().contains("differs from admitted execution evidence"));
                    assert_eq!(ledger_snapshot(&db), before, "{mode}/{point}: binding refusal");
                }
                let error = ledger.settle_measured(&target.id, &usage, status, Some(&measured),
                    now + 5).unwrap_err();
                if point == "commit" {
                    assert!(error.contains("FOREIGN KEY constraint failed"), "{error}");
                    assert_eq!(db.query_row("SELECT COUNT(*) FROM performance_commit_witness", [],
                        |row| row.get::<_, u64>(0)).unwrap(), 0);
                    assert_eq!(db.query_row("SELECT COUNT(*) FROM performance_commit_parent", [],
                        |row| row.get::<_, u64>(0)).unwrap(), 0);
                } else {
                    assert!(error.contains(&format!("injected performance {point}")), "{error}");
                }
                assert_eq!(ledger_snapshot(&db), before, "{mode}/{point}, reopened={reopened}");
                assert_eq!(serde_json::to_value(ledger.request_view(id, KEY).unwrap()).unwrap(),
                    serde_json::to_value(&pending).unwrap());
                assert!(ledger.performance_withdrawn(original).unwrap());
                assert!(!ledger.performance_withdrawn(&current).unwrap());
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
                assert_eq!(ledger_snapshot(&db), before);
            }
            // Explicit fixture repair permits accounting reconciliation, not
            // another inference. Keep the commit trigger active as a witness.
            if point == "commit" {
                assert_eq!(db.execute("INSERT INTO performance_commit_parent(id) VALUES(?1)",
                    [&target.id]).unwrap(), 1);
            } else {
                db.execute_batch("DROP TRIGGER fail_performance_write").unwrap();
            }
            let terminal_status = if mode == "invalid" { "invalid_usage_unknown_usage" } else { status };
            let mut expected = before.clone();
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["available_wei"] = json!("991");
            account["reserved_wei"] = json!("3");
            account["waived_wei"] = json!(if known { "7" } else { "5" });
            account["usage"] = json!({"prompt_tokens":if known { 1006 } else { 6 },
                "completion_tokens":if known { 1664 } else { 1536 }});
            expected[1].iter_mut().find(|row| row[0] == app.id).unwrap()[3]["reserved_wei"] = json!("0");
            let row = expected[2].iter_mut().find(|row| row[0] == target.id).unwrap();
            row[4] = json!(terminal_status);
            let request = &mut row[6];
            request["status"] = json!(terminal_status);
            request["usage"] = if known { serde_json::to_value(&usage).unwrap() }
                else { json!({"prompt_tokens":0,"completion_tokens":0}) };
            request["provider_loss_wei"] = json!(if known { "0" } else { "2" });
            request["settled_at"] = json!(now + 5);
            request["performance"] = serde_json::to_value(&measured).unwrap();
            request["waived_wei"] = json!(if known { "2" } else { "0" });
            let terminal = request.clone();
            expected[12].push(json!([7, target.id, original.id, evidence, 1]));
            expected[13].push(json!([original.id, evidence]));
            let result = fixture.provider.state.ledger.settle_measured(&target.id, &usage,
                status, Some(&measured), now + 5);
            if mode == "invalid" {
                assert!(result.unwrap_err().starts_with("usage_over_reservation:"));
            } else {
                assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), terminal);
            }
            assert_eq!(ledger_snapshot(&db), expected, "{mode}/{point}: first committed waiver");
            for reopened in [false, true] {
                if reopened {
                    fixture.app = Router::new();
                    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                    state.ledger = Arc::new(BillingStore::open(&fixture.provider.ledger_path,
                        &state.config.network.identity).unwrap());
                    fixture.app = contracts::router(fixture.provider.state.clone());
                }
                let ledger = &fixture.provider.state.ledger;
                assert!(ledger.performance_withdrawn(original).unwrap());
                assert!(ledger.performance_withdrawn(&current).unwrap());
                for lookup in [KEY, target.id.as_str()] {
                    assert_eq!(serde_json::to_value(ledger.request_view(id, lookup).unwrap()).unwrap(), terminal);
                }
                for (late_usage, late_status, late_outcome) in [
                    (TokenUsage::default(), "restart_unknown_usage", None),
                    (historical_usage.clone(), "completed",
                        Some(outcome(original, Some(&historical_usage), "completed", Some(1_000_000)))),
                ] {
                    let replay = ledger.settle_measured(&target.id, &late_usage, late_status,
                        late_outcome.as_ref(), now + 6).unwrap();
                    assert_eq!(serde_json::to_value(replay).unwrap(), terminal);
                    assert_eq!(ledger_snapshot(&db), expected);
                }
                assert!(ledger.reserve(id, &app.id, KEY, 1000, 256, now + 6, &current)
                    .unwrap_err().starts_with("duplicate_request:"));
                for model in [original, &current] {
                    assert!(ledger.reserve(id, &app.id, "after-withdrawal", 1, 256, now + 6, model)
                        .unwrap_err().starts_with("model_unqualified:"));
                }
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
                assert_eq!(ledger_snapshot(&db), expected, "{mode}/{point}: immutable result and latches");
                if point == "commit" {
                    assert_eq!(db.query_row("SELECT COUNT(*) FROM performance_commit_witness", [],
                        |row| row.get::<_, u64>(0)).unwrap(), 1);
                    assert_eq!(db.query_row("SELECT id FROM performance_commit_witness", [],
                        |row| row.get::<_, String>(0)).unwrap(), target.id);
                }
            }
            // New withdrawal cannot strand the other already-admitted hold, and
            // its later success cannot clear either durable withdrawal latch.
            let live_usage = TokenUsage { prompt_tokens: 2048, completion_tokens: 256 };
            let live_outcome = outcome(&current, Some(&live_usage), "completed", Some(1_000_000));
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["reserved_wei"] = json!("0");
            account["spent_wei"] = json!("4");
            account["usage"] = json!({"prompt_tokens":if known { 3054 } else { 2054 },
                "completion_tokens":if known { 1920 } else { 1792 }});
            let root = &mut expected[1].iter_mut().find(|row| row[0] == opened.credential.as_ref().unwrap().id).unwrap()[3];
            root["reserved_wei"] = json!("0");
            root["spent_wei"] = json!("4");
            let row = expected[2].iter_mut().find(|row| row[0] == live.id).unwrap();
            row[4] = json!("completed");
            row[6]["status"] = json!("completed");
            row[6]["usage"] = serde_json::to_value(&live_usage).unwrap();
            row[6]["cost_wei"] = json!("3");
            row[6]["settled_at"] = json!(now + 7);
            row[6]["performance"] = serde_json::to_value(&live_outcome).unwrap();
            let live_terminal = row[6].clone();
            expected[12].push(json!([8, live.id, original.id, evidence, 0]));
            let settled = fixture.provider.state.ledger.settle_measured(&live.id, &live_usage,
                "completed", Some(&live_outcome), now + 7).unwrap();
            assert_eq!(serde_json::to_value(settled).unwrap(), live_terminal);
            assert_eq!(ledger_snapshot(&db), expected);
            let reopened = BillingStore::open(&fixture.provider.ledger_path, &quote.payload.network).unwrap();
            assert!(reopened.performance_withdrawn(original).unwrap());
            assert!(reopened.performance_withdrawn(&current).unwrap());
            assert_eq!(serde_json::to_value(reopened.request_view(id, KEY).unwrap()).unwrap(), terminal);
            assert_eq!(serde_json::to_value(reopened.request_view(id, &live.id).unwrap()).unwrap(), live_terminal);
            assert_eq!(ledger_snapshot(&db), expected, "{mode}/{point}: retained charge, waiver and fees");
        }
    }
}

#[tokio::test]
async fn same_key_writers_preserve_one_reservation_and_its_original_bounds() {
    const KEY: &str = "competing-request-key";
    let directory = tempfile::tempdir().unwrap();
    for (credential_case, shared_credential) in [true, false].into_iter().enumerate() {
        for (terminal_case, known) in [true, false].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(),
                63_000 + (credential_case * 2 + terminal_case) as u64);
            let other = fixture.open_free().await;
            // Opening credit and supplied usage are post-verification ledger
            // prerequisites, not live-chain, engine or public-dispatch evidence.
            let (opened, _) = fixture.paid_account().await;
            let id = &opened.account.contract_id;
            let model = &opened.account.model;
            let now = protocol::now().unwrap();
            let ledger = &fixture.provider.state.ledger;
            let unrelated = ledger.reserve(&other.account.contract_id, &other.credential.as_ref().unwrap().id,
                KEY, 1, 1, now, &other.account.model).unwrap();
            ledger.settle(&unrelated.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 },
                "completed", now).unwrap();
            let prior = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "prior-race-spend",
                1, 1, now, model).unwrap();
            ledger.settle(&prior.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 },
                "completed", now).unwrap();
            let options = CredentialIssueRequest {
                expires_at: now + 3600, spend_cap_wei: Some("8".into()),
                max_concurrency: 2, scopes: vec!["infer".into()],
            };
            let first = ledger.issue_credential(id, &options, now).unwrap();
            let second = ledger.issue_credential(id, &options, now).unwrap();
            // Both token shapes fit independently and together: two account
            // slots, five wei total hold and enough shared credential cap.
            let candidates = [
                (first.id.as_str(), 1_u64, 2_u128),
                (if shared_credential { first.id.as_str() } else { second.id.as_str() },
                    1000_u64, 3_u128),
            ];
            let path = &fixture.provider.ledger_path;
            let network = &fixture.provider.offer.payload.network;
            let race = |keys: [&str; 2], created: u64| {
                // Open both independent connections before spawning: setup
                // failure cannot strand a worker at the two-party barrier.
                let writers = [0, 1].map(|index| (BillingStore::open(path, network).unwrap(), index));
                let barrier = Barrier::new(2);
                thread::scope(|scope| {
                    let handles = writers.map(|(writer, index)| {
                        let barrier = &barrier;
                        let (credential, prompt, _) = candidates[index];
                        let key = keys[index];
                        scope.spawn(move || {
                            barrier.wait();
                            writer.reserve(id, credential, key, prompt, 512, created, model)
                        })
                    });
                    handles.map(|handle| handle.join().unwrap())
                })
            };
            let request_row = |opaque: &str, key: &str, credential: &str,
                prompt: u64, maximum: u128, created: u64| {
                json!([opaque, id, key, credential, "reserved", created, {
                    "id":opaque, "request_id":key, "contract_id":id, "credential_id":credential,
                    "status":"reserved", "usage":{"prompt_tokens":0,"completion_tokens":0},
                    "cost_wei":"0", "max_cost_wei":maximum.to_string(), "provider_loss_wei":"0",
                    "prompt_tokens_reserved":prompt, "max_output_tokens":512,
                    "created_at":created, "settled_at":null, "performance":null, "waived_wei":"0"
                }])
            };
            let db = Connection::open(path).unwrap();
            let before = ledger_snapshot(&db);
            let account = &before[0].iter().find(|row| row[0] == *id).unwrap()[3];
            assert_eq!((account["funded_wei"].as_str(), account["available_wei"].as_str(),
                account["reserved_wei"].as_str(), account["spent_wei"].as_str()),
                (Some("100"), Some("99"), Some("0"), Some("1")));
            let results = race([KEY, KEY], now + 1);
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            let winner = results.iter().position(|result| result.is_ok()).unwrap();
            let loser = 1 - winner;
            let refusal = results[loser].as_ref().unwrap_err();
            assert!(refusal.starts_with("duplicate_request:"), "{refusal}");
            let reservation = results[winner].as_ref().unwrap();
            let (credential, prompt, maximum) = candidates[winner];
            assert_eq!(reservation.contract_id, *id);
            assert_eq!(reservation.model, *model);
            assert_eq!(reservation.price, opened.account.price);
            assert_eq!(reservation.max_cost_wei, maximum.to_string());
            let mut expected = before;
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["available_wei"] = json!((99 - maximum).to_string());
            account["reserved_wei"] = json!(maximum.to_string());
            expected[1].iter_mut().find(|row| row[0] == credential).unwrap()[3]["reserved_wei"]
                = json!(maximum.to_string());
            expected[2].push(request_row(&reservation.id, KEY, credential, prompt, maximum, now + 1));
            expected[2].sort_by(|left, right| left[0].as_str().cmp(&right[0].as_str()));
            assert_eq!(ledger_snapshot(&db), expected, "only the winning identity and hold may commit");
            let charge = if known { (winner + 1) as u128 } else { 0 };
            for terminal in [false, true] {
                if terminal {
                    let status = if known { "completed" } else { "backend_unknown_usage" };
                    let usage = if known { TokenUsage { prompt_tokens: prompt, completion_tokens: 1 } }
                        else { TokenUsage::default() };
                    let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
                    account["available_wei"] = json!((99 - charge).to_string());
                    account["reserved_wei"] = json!("0");
                    account["spent_wei"] = json!((1 + charge).to_string());
                    account["usage"] = json!({"prompt_tokens":if known { 1 + prompt } else { 1 },
                        "completion_tokens":if known { 2 } else { 1 }});
                    let credential_record = &mut expected[1].iter_mut()
                        .find(|row| row[0] == credential).unwrap()[3];
                    credential_record["reserved_wei"] = json!("0");
                    credential_record["spent_wei"] = json!(charge.to_string());
                    let row = expected[2].iter_mut().find(|row| row[0] == reservation.id).unwrap();
                    row[4] = json!(status);
                    row[6]["status"] = json!(status);
                    row[6]["usage"] = serde_json::to_value(&usage).unwrap();
                    row[6]["cost_wei"] = json!(charge.to_string());
                    row[6]["provider_loss_wei"] = json!(if known { "0".into() } else { maximum.to_string() });
                    row[6]["settled_at"] = json!(now + 2);
                    let settled = fixture.provider.state.ledger.settle(&reservation.id,
                        &usage, status, now + 2).unwrap();
                    assert_eq!(serde_json::to_value(settled).unwrap(), row[6]);
                    assert_eq!(ledger_snapshot(&db), expected);
                }
                for reopened in [false, true] {
                    if reopened {
                        fixture.app = Router::new();
                        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                        state.ledger = Arc::new(BillingStore::open(path, &state.config.network.identity).unwrap());
                        fixture.app = contracts::router(fixture.provider.state.clone());
                    }
                    // Both new writers must observe the same retained operation,
                    // including the caller whose differing token bounds lost.
                    for retry in race([KEY, KEY], now + 3) {
                        assert!(retry.unwrap_err().starts_with("duplicate_request:"));
                    }
                    let reader = &fixture.provider.state.ledger;
                    let original = &expected[2].iter().find(|row| row[0] == reservation.id).unwrap()[6];
                    for lookup in [KEY, reservation.id.as_str()] {
                        assert_eq!(serde_json::to_value(reader.request_view(id, lookup).unwrap()).unwrap(), *original);
                    }
                    if terminal {
                        let changed = TokenUsage { prompt_tokens: candidates[loser].1, completion_tokens: 512 };
                        let replay = reader.settle(&reservation.id, &changed, "cancelled", now + 4).unwrap();
                        assert_eq!(serde_json::to_value(replay).unwrap(), *original);
                    }
                    fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &first, &second, other.credential.as_ref().unwrap()]);
                    assert_eq!(ledger_snapshot(&db), expected,
                        "shared={shared_credential}, terminal={terminal}, reopened={reopened}");
                }
            }
            // Positive control: both original shapes/credentials can coexist
            // when their keys differ. No cap, slot or balance refusal explains
            // the earlier loser, and prior terminal identity is still retained.
            let keys = ["distinct-control-a", "distinct-control-b"];
            let controls = race(keys, now + 5).map(Result::unwrap);
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["available_wei"] = json!((94 - charge).to_string());
            account["reserved_wei"] = json!("5");
            for (index, control) in controls.iter().enumerate() {
                let (credential, prompt, maximum) = candidates[index];
                assert_eq!(control.contract_id, *id);
                assert_eq!(control.model, *model);
                assert_eq!(control.price, opened.account.price);
                assert_eq!(control.max_cost_wei, maximum.to_string());
                expected[2].push(request_row(&control.id, keys[index], credential, prompt, maximum, now + 5));
                expected[1].iter_mut().find(|row| row[0] == credential).unwrap()[3]["reserved_wei"]
                    = json!(if shared_credential { "5".into() } else { maximum.to_string() });
            }
            expected[2].sort_by(|left, right| left[0].as_str().cmp(&right[0].as_str()));
            assert_eq!(ledger_snapshot(&db), expected, "distinct keys must both reserve without rewriting the winner");
            for (index, control) in controls.iter().enumerate() {
                let (credential, _, maximum) = candidates[index];
                let remaining = if index == 0 { 3_u128 } else { 0 };
                let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
                account["available_wei"] = json!((99 - charge - remaining).to_string());
                account["reserved_wei"] = json!(remaining.to_string());
                expected[1].iter_mut().find(|row| row[0] == credential).unwrap()[3]["reserved_wei"]
                    = json!(if shared_credential { remaining.to_string() } else { "0".into() });
                let row = expected[2].iter_mut().find(|row| row[0] == control.id).unwrap();
                row[4] = json!("backend_unknown_usage");
                row[6]["status"] = json!("backend_unknown_usage");
                row[6]["provider_loss_wei"] = json!(maximum.to_string());
                row[6]["settled_at"] = json!(now + 6);
                let released = fixture.provider.state.ledger.settle(&control.id,
                    &TokenUsage::default(), "backend_unknown_usage", now + 6).unwrap();
                assert_eq!(serde_json::to_value(released).unwrap(), row[6]);
                assert_eq!(ledger_snapshot(&db), expected);
            }
            let reopened = BillingStore::open(path, network).unwrap();
            for (key, opaque) in [(KEY, reservation.id.as_str()),
                (keys[0], controls[0].id.as_str()), (keys[1], controls[1].id.as_str())] {
                let original = &expected[2].iter().find(|row| row[0] == opaque).unwrap()[6];
                assert_eq!(serde_json::to_value(reopened.request_view(id, key).unwrap()).unwrap(), *original);
            }
            for keys in [[KEY, KEY], keys] {
                for retry in race(keys, now + 7) {
                    assert!(retry.unwrap_err().starts_with("duplicate_request:"));
                }
            }
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &first, &second, other.credential.as_ref().unwrap()]);
            assert_eq!(ledger_snapshot(&db), expected, "reopened identities, charges and cap authority remain exact");
        }
    }
}

#[tokio::test]
async fn competing_reservations_preserve_cap_and_concurrency_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    for (limit_index, limit) in ["spend-cap", "credential-concurrency", "account-concurrency"]
        .into_iter().enumerate() {
        for (terminal_index, known) in [true, false].into_iter().enumerate() {
            let mut fixture = Fixture::new(directory.path(),
                64_000 + (limit_index * 2 + terminal_index) as u64);
            let other = fixture.open_free().await;
            // These are post-verification credit and supplied-usage prerequisites,
            // not engine execution, public admission or live-chain observations.
            let (opened, _) = fixture.paid_account().await;
            let id = &opened.account.contract_id;
            let model = &opened.account.model;
            let now = protocol::now().unwrap();
            let ledger = &fixture.provider.state.ledger;
            let options = CredentialIssueRequest {
                expires_at: now + 3600,
                spend_cap_wei: Some(if limit == "spend-cap" { "5" } else { "8" }.into()),
                max_concurrency: if limit == "credential-concurrency" { 1 } else { 2 },
                scopes: vec!["infer".into()],
            };
            let first = ledger.issue_credential(id, &options, now).unwrap();
            let second = ledger.issue_credential(id, &CredentialIssueRequest {
                spend_cap_wei: Some("8".into()), ..options
            }, now).unwrap();
            let prior = ledger.reserve(id, &first.id, "prior-limit-spend", 1, 1, now, model).unwrap();
            ledger.settle(&prior.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 },
                "completed", now).unwrap();
            let unrelated = ledger.reserve(&other.account.contract_id, &other.credential.as_ref().unwrap().id,
                "limit-candidate-a", 1, 1, now, &other.account.model).unwrap();
            ledger.settle(&unrelated.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 },
                "completed", now).unwrap();
            let root_live = if limit == "account-concurrency" {
                Some(ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "retained-root-hold",
                    2048, 256, now, model).unwrap())
            } else { None };
            let root_hold = if root_live.is_some() { 3_u128 } else { 0 };
            let candidates = [
                ("limit-candidate-a", first.id.as_str(), 1_u64, 2_u128),
                ("limit-candidate-b",
                    if root_live.is_some() { second.id.as_str() } else { first.id.as_str() },
                    1000_u64, 3_u128),
            ];
            let path = &fixture.provider.ledger_path;
            let network = &fixture.provider.offer.payload.network;
            let db = Connection::open(path).unwrap();
            let before = ledger_snapshot(&db);
            let account = &before[0].iter().find(|row| row[0] == *id).unwrap()[3];
            assert_eq!(account["available_wei"], (99 - root_hold).to_string());
            assert_eq!(account["reserved_wei"], root_hold.to_string());
            assert_eq!(account["spent_wei"], "1");
            assert_eq!(account["usage"], json!({"prompt_tokens":1,"completion_tokens":1}));
            assert_eq!(before[1].iter().find(|row| row[0] == first.id).unwrap()[3]["spent_wei"], "1");
            let writers = [0, 1].map(|index| (BillingStore::open(path, network).unwrap(), index));
            let barrier = Barrier::new(2);
            let results = thread::scope(|scope| {
                let handles = writers.map(|(writer, index)| {
                    let barrier = &barrier;
                    let (key, credential, prompt, _) = candidates[index];
                    scope.spawn(move || {
                        barrier.wait();
                        writer.reserve(id, credential, key, prompt, 512, now + 1, model)
                    })
                });
                handles.map(|handle| handle.join().unwrap())
            });
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1, "{limit}");
            let winner = results.iter().position(|result| result.is_ok()).unwrap();
            let loser = 1 - winner;
            let error_prefix = if limit == "spend-cap" { "spend_cap:" } else { "saturation:" };
            assert!(results[loser].as_ref().unwrap_err().starts_with(error_prefix),
                "{limit}: {:?}", results[loser]);
            let reservation = results[winner].as_ref().unwrap();
            let (key, credential, prompt, maximum) = candidates[winner];
            let (losing_key, losing_credential, losing_prompt, losing_maximum) = candidates[loser];
            assert_eq!(reservation.contract_id, *id);
            assert_eq!(reservation.model, *model);
            assert_eq!(reservation.price, opened.account.price);
            assert_eq!(reservation.max_cost_wei, maximum.to_string());
            let request_row = |opaque: &str, contract: &str, key: &str, credential: &str,
                prompt: u64, output: u64, maximum: u128, created: u64| {
                json!([opaque, contract, key, credential, "reserved", created, {
                    "id":opaque, "request_id":key, "contract_id":contract, "credential_id":credential,
                    "status":"reserved", "usage":{"prompt_tokens":0,"completion_tokens":0},
                    "cost_wei":"0", "max_cost_wei":maximum.to_string(), "provider_loss_wei":"0",
                    "prompt_tokens_reserved":prompt, "max_output_tokens":output,
                    "created_at":created, "settled_at":null, "performance":null, "waived_wei":"0"
                }])
            };
            let mut expected = before;
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["available_wei"] = json!((99 - root_hold - maximum).to_string());
            account["reserved_wei"] = json!((root_hold + maximum).to_string());
            expected[1].iter_mut().find(|row| row[0] == credential).unwrap()[3]["reserved_wei"]
                = json!(maximum.to_string());
            expected[2].push(request_row(&reservation.id, id, key, credential, prompt, 512, maximum, now + 1));
            expected[2].sort_by(|left, right| left[0].as_str().cmp(&right[0].as_str()));
            assert_eq!(ledger_snapshot(&db), expected, "{limit}: no partial losing reservation");
            for reopened in [false, true] {
                if reopened {
                    fixture.app = Router::new();
                    let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                    state.ledger = Arc::new(BillingStore::open(path, &state.config.network.identity).unwrap());
                    fixture.app = contracts::router(fixture.provider.state.clone());
                }
                let reader = &fixture.provider.state.ledger;
                assert!(reader.request_view(id, losing_key).unwrap_err().starts_with("not_found:"));
                assert!(reader.reserve(id, losing_credential, losing_key, losing_prompt,
                    512, now + 2, model).unwrap_err().starts_with(error_prefix));
                assert!(reader.reserve(id, credential, key, prompt, 512, now + 2, model)
                    .unwrap_err().starts_with("duplicate_request:"));
                let original = &expected[2].iter().find(|row| row[0] == reservation.id).unwrap()[6];
                for lookup in [key, reservation.id.as_str()] {
                    assert_eq!(serde_json::to_value(reader.request_view(id, lookup).unwrap()).unwrap(), *original);
                }
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &first, &second, other.credential.as_ref().unwrap()]);
                assert_eq!(ledger_snapshot(&db), expected, "{limit}, reopened={reopened}");
            }
            // Isolate the limiting dimension while the winner is still held:
            // a smaller hold fits the cap; another credential fits the same
            // account; another account remains available when this one is full.
            let (control_account, control_credential, control_model, control_maximum) = match limit {
                "spend-cap" => (id.as_str(), first.id.as_str(), model, 1_u128),
                "credential-concurrency" => (id.as_str(), second.id.as_str(), model, 1_u128),
                "account-concurrency" => (other.account.contract_id.as_str(), other.credential.as_ref().unwrap().id.as_str(),
                    &other.account.model, 0_u128),
                _ => unreachable!(),
            };
            let mut after_control = expected.clone();
            let control = fixture.provider.state.ledger.reserve(control_account, control_credential,
                "independent-limit-control", 1, 1, now + 3, control_model).unwrap();
            assert_eq!(control.max_cost_wei, control_maximum.to_string());
            if control_maximum != 0 {
                let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
                account["available_wei"] = json!((98 - root_hold - maximum).to_string());
                account["reserved_wei"] = json!((root_hold + maximum + 1).to_string());
                expected[1].iter_mut().find(|row| row[0] == control_credential).unwrap()[3]["reserved_wei"]
                    = json!(if limit == "spend-cap" { (maximum + 1).to_string() } else { "1".into() });
            }
            let mut control_row = request_row(&control.id, control_account, "independent-limit-control",
                control_credential, 1, 1, control_maximum, now + 3);
            expected[2].push(control_row.clone());
            expected[2].sort_by(|left, right| left[0].as_str().cmp(&right[0].as_str()));
            assert_eq!(ledger_snapshot(&db), expected, "{limit}: independent positive control");
            control_row[4] = json!("backend_unknown_usage");
            control_row[6]["status"] = json!("backend_unknown_usage");
            control_row[6]["provider_loss_wei"] = json!(control_maximum.to_string());
            control_row[6]["settled_at"] = json!(now + 4);
            let released = fixture.provider.state.ledger.settle(&control.id,
                &TokenUsage::default(), "backend_unknown_usage", now + 4).unwrap();
            assert_eq!(serde_json::to_value(released).unwrap(), control_row[6]);
            after_control[2].push(control_row);
            after_control[2].sort_by(|left, right| left[0].as_str().cmp(&right[0].as_str()));
            expected = after_control;
            assert!(fixture.provider.state.ledger.reserve(id, losing_credential, losing_key,
                losing_prompt, 512, now + 4, model).unwrap_err().starts_with(error_prefix));
            assert_eq!(ledger_snapshot(&db), expected, "{limit}: control release preserves winning hold");

            let charge = if known { (winner + 1) as u128 } else { 0 };
            let usage = if known { TokenUsage { prompt_tokens: prompt, completion_tokens: 1 } }
                else { TokenUsage::default() };
            let status = if known { "completed" } else { "backend_unknown_usage" };
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["available_wei"] = json!((99 - root_hold - charge).to_string());
            account["reserved_wei"] = json!(root_hold.to_string());
            account["spent_wei"] = json!((1 + charge).to_string());
            account["usage"] = json!({"prompt_tokens":if known { 1 + prompt } else { 1 },
                "completion_tokens":if known { 2 } else { 1 }});
            let credential_record = &mut expected[1].iter_mut().find(|row| row[0] == credential).unwrap()[3];
            credential_record["reserved_wei"] = json!("0");
            credential_record["spent_wei"] = json!((if credential == first.id { 1 } else { 0 } + charge).to_string());
            let row = expected[2].iter_mut().find(|row| row[0] == reservation.id).unwrap();
            row[4] = json!(status);
            row[6]["status"] = json!(status);
            row[6]["usage"] = serde_json::to_value(&usage).unwrap();
            row[6]["cost_wei"] = json!(charge.to_string());
            row[6]["provider_loss_wei"] = json!(if known { "0".into() } else { maximum.to_string() });
            row[6]["settled_at"] = json!(now + 5);
            let winner_terminal = row[6].clone();
            let settled = fixture.provider.state.ledger.settle(&reservation.id, &usage, status, now + 5).unwrap();
            assert_eq!(serde_json::to_value(settled).unwrap(), winner_terminal);
            assert_eq!(ledger_snapshot(&db), expected, "{limit}: winner releases only its own hold");

            // A definite refusal never consumed the losing key. Reopening and
            // retrying that exact candidate now uses the released allowance.
            let reopened = BillingStore::open(path, network).unwrap();
            let admitted = reopened.reserve(id, losing_credential, losing_key, losing_prompt,
                512, now + 6, model).unwrap();
            assert_eq!(admitted.max_cost_wei, losing_maximum.to_string());
            assert_eq!(admitted.model, *model);
            assert_eq!(admitted.price, opened.account.price);
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["available_wei"] = json!((99 - root_hold - charge - losing_maximum).to_string());
            account["reserved_wei"] = json!((root_hold + losing_maximum).to_string());
            expected[1].iter_mut().find(|row| row[0] == losing_credential).unwrap()[3]["reserved_wei"]
                = json!(losing_maximum.to_string());
            expected[2].push(request_row(&admitted.id, id, losing_key, losing_credential,
                losing_prompt, 512, losing_maximum, now + 6));
            expected[2].sort_by(|left, right| left[0].as_str().cmp(&right[0].as_str()));
            assert_eq!(ledger_snapshot(&db), expected, "{limit}: original refused key is newly admitted");
            let replay = reopened.settle(&reservation.id,
                &TokenUsage { prompt_tokens: losing_prompt, completion_tokens: 512 }, "cancelled", now + 7).unwrap();
            assert_eq!(serde_json::to_value(replay).unwrap(), winner_terminal);
            assert_eq!(ledger_snapshot(&db), expected, "{limit}: terminal replay cannot release the new hold");
            let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
            account["available_wei"] = json!((99 - root_hold - charge).to_string());
            account["reserved_wei"] = json!(root_hold.to_string());
            expected[1].iter_mut().find(|row| row[0] == losing_credential).unwrap()[3]["reserved_wei"] = json!("0");
            let row = expected[2].iter_mut().find(|row| row[0] == admitted.id).unwrap();
            row[4] = json!("backend_unknown_usage");
            row[6]["status"] = json!("backend_unknown_usage");
            row[6]["provider_loss_wei"] = json!(losing_maximum.to_string());
            row[6]["settled_at"] = json!(now + 8);
            let released = reopened.settle(&admitted.id, &TokenUsage::default(),
                "backend_unknown_usage", now + 8).unwrap();
            assert_eq!(serde_json::to_value(released).unwrap(), row[6]);
            assert_eq!(ledger_snapshot(&db), expected);
            if let Some(root) = &root_live {
                let root_usage = TokenUsage { prompt_tokens: 2048, completion_tokens: 256 };
                let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
                account["reserved_wei"] = json!("0");
                account["spent_wei"] = json!((4 + charge).to_string());
                account["usage"] = json!({"prompt_tokens":2049 + if known { prompt } else { 0 },
                    "completion_tokens":if known { 258 } else { 257 }});
                let root_credential = &mut expected[1].iter_mut()
                    .find(|row| row[0] == opened.credential.as_ref().unwrap().id).unwrap()[3];
                root_credential["reserved_wei"] = json!("0");
                root_credential["spent_wei"] = json!("3");
                let row = expected[2].iter_mut().find(|row| row[0] == root.id).unwrap();
                row[4] = json!("completed");
                row[6]["status"] = json!("completed");
                row[6]["usage"] = serde_json::to_value(&root_usage).unwrap();
                row[6]["cost_wei"] = json!("3");
                row[6]["settled_at"] = json!(now + 9);
                let settled = reopened.settle(&root.id, &root_usage, "completed", now + 9).unwrap();
                assert_eq!(serde_json::to_value(settled).unwrap(), row[6]);
                assert_eq!(ledger_snapshot(&db), expected, "unrelated root hold survives both contenders");
            }
            drop(reopened);
            let reopened = BillingStore::open(path, network).unwrap();
            for (request_key, request_id, credential, prompt) in [
                (key, reservation.id.as_str(), credential, prompt),
                (losing_key, admitted.id.as_str(), losing_credential, losing_prompt),
            ] {
                let original = &expected[2].iter().find(|row| row[0] == request_id).unwrap()[6];
                for lookup in [request_key, request_id] {
                    assert_eq!(serde_json::to_value(reopened.request_view(id, lookup).unwrap()).unwrap(), *original);
                }
                assert!(reopened.reserve(id, credential, request_key, prompt, 512, now + 10, model)
                    .unwrap_err().starts_with("duplicate_request:"));
            }
            fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &first, &second, other.credential.as_ref().unwrap()]);
            assert_eq!(ledger_snapshot(&db), expected, "{limit}: final identities, spend and authority");
        }
    }
}

#[tokio::test]
async fn competing_terminal_outcomes_commit_one_complete_original_result() {
    use chiral_network::performance;

    const KEY: &str = "terminal-race";
    const MODES: [&str; 5] = ["partial", "full", "unknown", "invalid", "waived"];
    let directory = tempfile::tempdir().unwrap();
    let mut case = 65_000;
    for profiled in [false, true] {
        let count = if profiled { 5 } else { 4 };
        for left in 0..count {
            for right in left + 1..count {
                let pair = [MODES[left], MODES[right]];
                let mut fixture = Fixture::new(directory.path(), case);
                case += 1;
                let other = fixture.open_free().await;
                let (quote, opened) = fixture.accounting_account(profiled).await;
                let id = &opened.account.contract_id;
                let now = protocol::now().unwrap();
                let original = &quote.payload.model;
                let mut current = original.clone();
                if let Some(profile) = current.serving.as_mut() {
                    profile.qualification.evidence_sha256 = "d".repeat(64);
                    profile.qualification.measured_at = now + 1;
                }
                let measure = |model: &protocol::ModelSpec, usage: Option<&TokenUsage>,
                    status: &str, missed: bool| {
                    model.serving.as_ref().map(|profile| {
                        let ttft = usage.map(|_| if missed { 2_000_001 } else { 1_000_000 });
                        performance::outcome(profile, true, ttft, ttft.map(|_| 1_000_000),
                            usage, status, false, ttft.map(|time| time + 1_000_000),
                            &profile.qualification.evidence_sha256)
                    })
                };
                let input = |mode: &str| {
                    let unknown = matches!(mode, "unknown" | "invalid");
                    let usage = if mode == "unknown" { TokenUsage::default() } else {
                        TokenUsage { prompt_tokens: 500, completion_tokens: match mode {
                            "full" => 256, "invalid" => 257, _ => 128,
                        }}
                    };
                    let status = match mode {
                        "unknown" => "backend_unknown_usage",
                        _ => "completed",
                    };
                    let measured = measure(&current, if unknown { None } else { Some(&usage) },
                        if unknown { "backend_unknown_usage" } else { status }, mode == "waived");
                    if let Some(outcome) = &measured {
                        assert_eq!(outcome.attained, Some(!unknown && mode != "waived"));
                        assert_eq!(outcome.remedy, if unknown || mode == "waived" {
                            "inference_waiver"
                        } else { "none" });
                        assert_eq!(outcome.output_tokens,
                            if unknown { None } else { Some(usage.completion_tokens) });
                    }
                    (usage, status, measured)
                };
                let ledger = &fixture.provider.state.ledger;
                let unrelated = ledger.reserve(&other.account.contract_id, &other.credential.as_ref().unwrap().id,
                    KEY, 1, 1, now, &other.account.model).unwrap();
                ledger.settle(&unrelated.id, &TokenUsage { prompt_tokens: 1, completion_tokens: 1 },
                    "completed", now).unwrap();
                let prior_usage = TokenUsage { prompt_tokens: 500, completion_tokens: 128 };
                if profiled {
                    // Preserve an old report's withdrawal while the race binds a newer report.
                    for index in 0..3 {
                        let reservation = ledger.reserve(id, &opened.credential.as_ref().unwrap().id,
                            &format!("old-terminal-miss-{index}"), 500, 256, now, original).unwrap();
                        let missed = measure(original, Some(&prior_usage), "completed", true);
                        let settled = ledger.settle_measured(&reservation.id, &prior_usage,
                            "completed", missed.as_ref(), now).unwrap();
                        assert_eq!((settled.cost_wei.as_str(), settled.waived_wei.as_str()), ("0", "1"));
                    }
                    assert!(ledger.performance_withdrawn(original).unwrap());
                }
                let prior = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "prior-terminal-spend",
                    500, 256, now + 2, &current).unwrap();
                let passed = measure(&current, Some(&prior_usage), "completed", false);
                assert_eq!(ledger.settle_measured(&prior.id, &prior_usage,
                    "completed", passed.as_ref(), now + 3).unwrap().cost_wei, "1");
                let app = ledger.issue_credential(id, &CredentialIssueRequest {
                    expires_at: now + 3600, spend_cap_wei: Some("20".into()),
                    max_concurrency: 1, scopes: vec!["infer".into()],
                }, now + 4).unwrap();
                let live = ledger.reserve(id, &opened.credential.as_ref().unwrap().id, "terminal-live",
                    2048, 256, now + 4, &current).unwrap();
                let target = ledger.reserve(id, &app.id, KEY, 500, 256, now + 4, &current).unwrap();
                assert_eq!((live.max_cost_wei.as_str(), target.max_cost_wei.as_str()), ("3", "2"));
                assert_eq!(target.model, current);
                let account = ledger.account(id).unwrap();
                assert_eq!(account.model, *original);
                assert_eq!(account.price, quote.payload.price);
                assert_eq!((account.funded_wei.as_str(), account.fee_wei.as_str(),
                    account.available_wei.as_str(), account.reserved_wei.as_str(),
                    account.spent_wei.as_str(), account.waived_wei.as_str()),
                    ("1000", "5", "989", "5", "1", if profiled { "3" } else { "0" }));
                assert_eq!((account.usage.prompt_tokens, account.usage.completion_tokens),
                    if profiled { (2000, 512) } else { (500, 128) });
                let path = fixture.provider.ledger_path.clone();
                let db = Connection::open(&path).unwrap();
                let before = ledger_snapshot(&db);
                assert_eq!(before[13], if profiled {
                    vec![json!([original.id, "c".repeat(64)])]
                } else { vec![] });
                let pending = serde_json::to_value(ledger.request_view(id, KEY).unwrap()).unwrap();
                if profiled {
                    assert_eq!(pending["performance"]["evidence_sha256"], "d".repeat(64));
                    assert_eq!(pending["performance"]["eligibility"], "pending");
                } else {
                    assert!(pending["performance"].is_null());
                }

                // Expected amounts are fixture arithmetic, not production usage_cost.
                // This applies only the fixed 500/256, two-wei candidate shape.
                let expect_terminal = |snapshot: &mut [Vec<Value>; 14],
                    request_id: &str, mode: &str, when: u64| {
                    let known = !matches!(mode, "unknown" | "invalid");
                    let cost = match mode { "partial" => 1, "full" => 2, _ => 0 };
                    let waived = u64::from(mode == "waived");
                    let output = if mode == "full" { 256 } else { 128 };
                    let account = &mut snapshot[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
                    for (field, increase, decrease) in [
                        ("available_wei", 2 - cost, 0), ("reserved_wei", 0, 2),
                        ("spent_wei", cost, 0), ("waived_wei", waived, 0),
                    ] {
                        let previous: u64 = account[field].as_str().unwrap().parse().unwrap();
                        account[field] = json!((previous + increase - decrease).to_string());
                    }
                    if known {
                        account["usage"]["prompt_tokens"] = json!(account["usage"]["prompt_tokens"].as_u64().unwrap() + 500);
                        account["usage"]["completion_tokens"] = json!(account["usage"]["completion_tokens"].as_u64().unwrap() + output);
                    }
                    let credential = &mut snapshot[1].iter_mut().find(|row| row[0] == app.id).unwrap()[3];
                    assert_eq!(credential["reserved_wei"], "2");
                    credential["reserved_wei"] = json!("0");
                    let spent: u64 = credential["spent_wei"].as_str().unwrap().parse().unwrap();
                    credential["spent_wei"] = json!((spent + cost).to_string());
                    let (_, status, measured) = input(mode);
                    let status = if mode == "invalid" { "invalid_usage_unknown_usage" } else { status };
                    let row = snapshot[2].iter_mut().find(|row| row[0] == request_id).unwrap();
                    row[4] = json!(status);
                    row[6]["status"] = json!(status);
                    row[6]["usage"] = json!({"prompt_tokens":if known { 500 } else { 0 },
                        "completion_tokens":if known { output } else { 0 }});
                    row[6]["cost_wei"] = json!(cost.to_string());
                    row[6]["waived_wei"] = json!(waived.to_string());
                    row[6]["provider_loss_wei"] = json!(if known { "0" } else { "2" });
                    row[6]["settled_at"] = json!(when);
                    row[6]["performance"] = serde_json::to_value(measured).unwrap();
                    let terminal = row[6].clone();
                    if profiled {
                        let evidence = &current.serving.as_ref().unwrap().qualification.evidence_sha256;
                        let sequence = snapshot[12].len() + 1;
                        let miss = u64::from(!known || mode == "waived");
                        snapshot[12].push(json!([sequence, request_id, original.id, evidence, miss]));
                        let misses = snapshot[12].iter().filter(|row|
                            row[3] == *evidence && row[4] == 1).count();
                        if misses == 3 && !snapshot[13].iter().any(|row| row[1] == *evidence) {
                            snapshot[13].push(json!([original.id, evidence]));
                        }
                    }
                    terminal
                };
                let inputs = pair.map(input);
                let race = |when: u64| {
                    // Open before spawning: a failed open cannot strand a barrier peer.
                    let writers = [0, 1].map(|index| (
                        BillingStore::open(&path, &quote.payload.network).unwrap(), index));
                    let barrier = Barrier::new(2);
                    thread::scope(|scope| {
                        let handles = writers.map(|(store, index)| {
                            let barrier = &barrier;
                            let request_id = &target.id;
                            let input = &inputs[index];
                            scope.spawn(move || {
                                barrier.wait();
                                store.settle_measured(request_id, &input.0, input.1,
                                    input.2.as_ref(), when + index as u64)
                            })
                        });
                        handles.map(|handle| handle.join().unwrap())
                    })
                };
                let results = race(now + 10);
                let observed = fixture.provider.state.ledger.request_view(id, KEY).unwrap();
                let winner = if observed.settled_at == Some(now + 10) { 0 } else {
                    assert_eq!(observed.settled_at, Some(now + 11));
                    1
                };
                let mut expected = before.clone();
                let terminal = expect_terminal(&mut expected, &target.id, pair[winner], now + 10 + winner as u64);
                assert_eq!(serde_json::to_value(observed).unwrap(), terminal);
                for (index, result) in results.into_iter().enumerate() {
                    if index == winner && pair[index] == "invalid" {
                        assert!(result.unwrap_err().starts_with("usage_over_reservation:"));
                    } else {
                        assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), terminal);
                    }
                }
                assert_eq!(ledger_snapshot(&db), expected, "profiled={profiled}, {pair:?}");
                for reopened in [false, true] {
                    if reopened {
                        fixture.app = Router::new();
                        let state = Arc::get_mut(&mut fixture.provider.state).unwrap();
                        state.ledger = Arc::new(BillingStore::open(&path, &quote.payload.network).unwrap());
                        fixture.app = contracts::router(fixture.provider.state.clone());
                    }
                    for result in race(now + 20) {
                        assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), terminal);
                    }
                    let ledger = &fixture.provider.state.ledger;
                    for lookup in [KEY, target.id.as_str()] {
                        assert_eq!(serde_json::to_value(ledger.request_view(id, lookup).unwrap()).unwrap(), terminal);
                    }
                    // Later conflicting report/status/usage cannot rewrite a terminal result.
                    let stale = measure(original, Some(&prior_usage), "completed", false);
                    assert_eq!(serde_json::to_value(ledger.settle_measured(&target.id,
                        &prior_usage, "completed", stale.as_ref(), now + 22).unwrap()).unwrap(), terminal);
                    assert!(ledger.reserve(id, &app.id, KEY, 500, 256, now + 22, &current)
                        .unwrap_err().starts_with("duplicate_request:"));
                    fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
                    assert_eq!(ledger_snapshot(&db), expected, "reopened={reopened}, {pair:?}");
                }
                // Each candidate independently commits on a fresh key, even if it lost
                // above. These are explicit ledger controls, not automatic inference retries.
                for (index, mode) in pair.into_iter().enumerate() {
                    let ledger = &fixture.provider.state.ledger;
                    let key = format!("terminal-control-{index}");
                    let control = ledger.reserve(id, &app.id, &key, 500, 256,
                        now + 30, &current).unwrap();
                    let mut control_record = pending.clone();
                    control_record["id"] = json!(control.id);
                    control_record["request_id"] = json!(key);
                    control_record["created_at"] = json!(now + 30);
                    expected[2].push(json!([control.id, id, key, app.id, "reserved",
                        now + 30, control_record]));
                    expected[2].sort_by(|a, b| a[0].as_str().unwrap().cmp(b[0].as_str().unwrap()));
                    let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
                    let available: u64 = account["available_wei"].as_str().unwrap().parse().unwrap();
                    assert_eq!(account["reserved_wei"], "3");
                    account["available_wei"] = json!((available - 2).to_string());
                    account["reserved_wei"] = json!("5");
                    expected[1].iter_mut().find(|row| row[0] == app.id).unwrap()[3]["reserved_wei"] = json!("2");
                    assert_eq!(ledger_snapshot(&db), expected);
                    let when = now + 31 + index as u64;
                    let control_terminal = expect_terminal(&mut expected, &control.id, mode, when);
                    let (usage, status, measured) = input(mode);
                    let result = ledger.settle_measured(&control.id, &usage, status, measured.as_ref(), when);
                    if mode == "invalid" {
                        assert!(result.unwrap_err().starts_with("usage_over_reservation:"));
                    } else {
                        assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), control_terminal);
                    }
                    assert_eq!(serde_json::to_value(ledger.request_view(id, &key).unwrap()).unwrap(), control_terminal);
                    assert_eq!(ledger_snapshot(&db), expected);
                }
                // Preserve the unrelated running hold through every race/control; its
                // own later charge must remain possible even after a withdrawal latch.
                let live_usage = TokenUsage { prompt_tokens: 2048, completion_tokens: 256 };
                let live_outcome = measure(&current, Some(&live_usage), "completed", false);
                let account = &mut expected[0].iter_mut().find(|row| row[0] == *id).unwrap()[3];
                assert_eq!(account["reserved_wei"], "3");
                account["reserved_wei"] = json!("0");
                let spent: u64 = account["spent_wei"].as_str().unwrap().parse().unwrap();
                account["spent_wei"] = json!((spent + 3).to_string());
                account["usage"]["prompt_tokens"] = json!(account["usage"]["prompt_tokens"].as_u64().unwrap() + 2048);
                account["usage"]["completion_tokens"] = json!(account["usage"]["completion_tokens"].as_u64().unwrap() + 256);
                let root = &mut expected[1].iter_mut().find(|row| row[0] == opened.credential.as_ref().unwrap().id).unwrap()[3];
                assert_eq!(root["reserved_wei"], "3");
                root["reserved_wei"] = json!("0");
                root["spent_wei"] = json!("4");
                let row = expected[2].iter_mut().find(|row| row[0] == live.id).unwrap();
                row[4] = json!("completed");
                row[6]["status"] = json!("completed");
                row[6]["usage"] = serde_json::to_value(&live_usage).unwrap();
                row[6]["cost_wei"] = json!("3");
                row[6]["settled_at"] = json!(now + 40);
                row[6]["performance"] = serde_json::to_value(&live_outcome).unwrap();
                let live_terminal = row[6].clone();
                if profiled {
                    let sequence = expected[12].len() + 1;
                    expected[12].push(json!([sequence, live.id,
                        original.id, current.serving.as_ref().unwrap().qualification.evidence_sha256, 0]));
                }
                assert_eq!(serde_json::to_value(fixture.provider.state.ledger.settle_measured(
                    &live.id, &live_usage, "completed", live_outcome.as_ref(), now + 40).unwrap()).unwrap(), live_terminal);
                let reopened = BillingStore::open(&path, &quote.payload.network).unwrap();
                for (key, request_id, record) in [(KEY, target.id.as_str(), &terminal),
                    ("terminal-live", live.id.as_str(), &live_terminal)] {
                    for lookup in [key, request_id] {
                        assert_eq!(serde_json::to_value(reopened.request_view(id, lookup).unwrap()).unwrap(), *record);
                    }
                    assert!(reopened.reserve(id, &app.id, key, 500, 256, now + 41, &current)
                        .unwrap_err().starts_with("duplicate_request:"));
                }
                if profiled {
                    assert!(reopened.performance_withdrawn(original).unwrap());
                    let all_missed = pair.iter().all(|mode| matches!(*mode, "unknown" | "invalid" | "waived"));
                    assert_eq!(reopened.performance_withdrawn(&current).unwrap(), all_missed);
                }
                for result in race(now + 42) {
                    assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), terminal);
                }
                fixture.credentials_usable(&[opened.credential.as_ref().unwrap(), &app, other.credential.as_ref().unwrap()]);
                assert_eq!(ledger_snapshot(&db), expected, "final profiled={profiled}, {pair:?}");
            }
        }
    }
}
