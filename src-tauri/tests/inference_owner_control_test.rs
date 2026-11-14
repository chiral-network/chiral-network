//! Owner control and challenge capacity through production routes and isolated disk ledgers.
//! Free accounts open over HTTP. Paid account prerequisites begin AFTER the chain verification boundary.
//! Synthetic accepted-block inputs are not chain qualification; no RPC, broadcast or engine is used.
#[path = "support/proposal.rs"]
mod proposal;

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{header, HeaderMap, StatusCode},
    Router,
};
use chiral_network::{
    app::ProviderState,
    auth::{self, AuthPayload},
    billing_store::{BillingStore, CredentialIssueRequest, CredentialView, RequestView},
    chain_client::{ChainClient, ValidatedPaymentEnvelope, VerifiedPayment},
    contracts,
    protocol::{self, Challenge, Credential, FundingIntent, OpenResult, Quote, Signed, TokenUsage, UsageReceipt, RECEIPT_DOMAIN},
    wallet,
};
use proposal::PreparedProvider;
use rlp::RlpStream;
use rusqlite::{params, Connection};
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{net::TcpListener, path::Path, sync::{atomic::{AtomicBool, Ordering}, Arc, Barrier, Mutex}, thread};
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
            let reservation = ledger.reserve(&opened.account.contract_id, &opened.credential.id,
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
                assert_ne!(issued.bearer, previous.bearer);
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
        let key = SecretKey::from_slice(&hex::decode(OWNER_KEY).unwrap()).unwrap();
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
        let owner = wallet::address_from_private_key(OWNER_KEY).unwrap();
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
}

#[tokio::test]
async fn every_control_read_requires_its_owner_or_an_account_scoped_read_bearer() {
    let directory = tempfile::tempdir().unwrap();
    for (index, route) in ROUTES.into_iter().enumerate() {
        let mut fixture = Fixture::new(directory.path(), 1000 + index as u64);
        let accounts = fixture.accounts().await;
        let path = route.path(&accounts.target.account.contract_id);
        denied(fixture.send("GET", &path, &HeaderMap::new(), Vec::new()).await, StatusCode::UNAUTHORIZED);
        for credential in [&accounts.infer, &accounts.other.credential] {
            denied(fixture.send("GET", &path, &bearer(credential), Vec::new()).await, StatusCode::FORBIDDEN);
        }
        fixture.read_ok(route, &accounts, &bearer(&accounts.read)).await;
        fixture.read_ok(route, &accounts, &bearer(&accounts.target.credential)).await;
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
        for credential in [&accounts.target.credential, &accounts.read, &accounts.infer, &accounts.other.credential] {
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
        fixture.credentials_usable(&[&accounts.target.credential, &accounts.read, &accounts.infer, &accounts.other.credential]);

        let mut owner = fixture.proof(OWNER_KEY, method, &path, &body).await;
        let (status, value) = fixture.send(method, &path, &owner.headers, body.clone()).await;
        assert_eq!(status, StatusCode::OK, "{mutation:?}: {value}");
        fixture.check_mutation(mutation, &accounts.read, value);
        let after = fixture.credential_state(id);
        owner.headers.extend(bearer(&accounts.target.credential));
        denied(fixture.send(method, &path, &owner.headers, body).await, StatusCode::UNAUTHORIZED);
        assert_eq!(fixture.credential_state(id), after);
        assert_eq!(fixture.credential_state(&accounts.other.account.contract_id), other_before);
        fixture.credentials_usable(&[&accounts.target.credential, &accounts.infer, &accounts.other.credential]);
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
        let (method, path, body) = mutation.request(target, &accounts.other.credential.id, &options);
        let proof = fixture.proof(OWNER_KEY, method, &path, &body).await;
        denied(fixture.send(method, &path, &proof.headers, body.clone()).await, StatusCode::NOT_FOUND);
        let persisted = BillingStore::open(&fixture.provider.ledger_path, &fixture.provider.offer.payload.network).unwrap();
        assert!(persisted.challenge(&proof.nonce).is_err());
        denied(fixture.send(method, &path, &proof.headers, body).await, StatusCode::UNAUTHORIZED);
        assert_eq!(fixture.credential_state(target), before);
        assert_eq!(fixture.credential_state(other), other_before);
        fixture.credentials_usable(&[&accounts.target.credential, &accounts.read, &accounts.infer, &accounts.other.credential]);

        // The credential exists and belongs to this wallet, but requires its own account's signed target.
        let (method, path, body) = mutation.request(other, &accounts.other.credential.id, &options);
        let correct = fixture.proof(OWNER_KEY, method, &path, &body).await;
        let (status, value) = fixture.send(method, &path, &correct.headers, body).await;
        assert_eq!(status, StatusCode::OK, "{mutation:?}: {value}");
        fixture.check_mutation(mutation, &accounts.other.credential, value);
        assert_eq!(fixture.credential_state(target), before);
        fixture.credentials_usable(&[&accounts.target.credential, &accounts.read, &accounts.infer]);
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
        fixture.credentials_usable(&[&accounts.target.credential, &accounts.read, &accounts.infer, &accounts.other.credential]);
        denied(fixture.send(method, &path, &proof.headers, body).await, StatusCode::UNAUTHORIZED);

        let (method, path, body) = mutation.request(id, &accounts.read.id, &options);
        let fresh = fixture.proof(OWNER_KEY, method, &path, &body).await;
        let (status, value) = fixture.send(method, &path, &fresh.headers, body).await;
        assert_eq!(status, StatusCode::OK, "{mutation:?}: {value}");
        fixture.check_mutation(mutation, &accounts.read, value);
        fixture.credentials_usable(&[&accounts.target.credential, &accounts.infer, &accounts.other.credential]);
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
            [&accounts.read, &accounts.infer, &accounts.target.credential, &accounts.other.credential]).await;
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
            fixture.credentials_usable(&[&opened.credential]);
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
        fixture.credentials_usable(&[&accounts.read, &accounts.infer, &accounts.target.credential, &accounts.other.credential]);
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
            [&accounts.read, &accounts.infer, &accounts.target.credential, &accounts.other.credential]).await;
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
        fixture.credentials_usable(&[&accounts.read, &accounts.infer, &accounts.target.credential, &accounts.other.credential]);
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
                fixture.credentials_usable(&[&opened.credential, &read]);
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
        fixture.payment_denials(OPEN, &body, [&read, &infer, &target.credential, &accounts.other.credential]).await;
        assert_eq!(fixture.credential_state(id), before);
        assert_eq!(serde_json::to_value(fixture.provider.state.ledger.funding_history(id).unwrap()).unwrap(), funding);
        fixture.credentials_usable(&[&read, &infer, &target.credential, &accounts.other.credential]);
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
        fixture.payment_denials(&path, &body, [&read, &infer, &target.credential, &accounts.other.credential]).await;
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
        fixture.credentials_usable(&[&read, &infer, &target.credential, &accounts.other.credential]);
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
        let proposals: Vec<_> = (1..=8).map(|sequence| proposal(sequence, now)).collect();
        let challenges: Vec<_> = (1..=8).map(|sequence| fixture.pending_challenge(
            &quote.payload.consumer_wallet, sequence, now + if sequence == 1 { 119 } else { 120 })).collect();
        for challenge_phase in [challenges_first, !challenges_first] {
            if challenge_phase {
                for payload in &challenges { ledger.create_challenge(payload, now).unwrap(); }
            } else {
                for proposed in &proposals { ledger.store_quote(proposed, now).unwrap(); }
            }
        }
        // Acknowledgement freed its proposal slot; eight standalone proposals fill the allowance.
        let rejected_quote = proposal(9, now);
        assert!(ledger.store_quote(&rejected_quote, now).unwrap_err().starts_with("rate_limit:"));
        assert!(ledger.quote(&rejected_quote.payload.id).is_err());
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
        let fresh_quote = proposal(9, cleanup_at);
        ledger.store_quote(&fresh_quote, cleanup_at).unwrap();
        assert_eq!(ledger.quote(&fresh_quote.payload.id).unwrap().payload, fresh_quote.payload);
        assert_eq!(fixture.credential_state(&accounts.target.account.contract_id), account_before);
        assert_eq!(fixture.credential_state(&accounts.other.account.contract_id), other_before);
        for opened in [&accounts.target, &accounts.other] {
            assert_eq!(ledger.authenticate(&opened.credential.bearer, cleanup_at).unwrap().account.contract_id,
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

fn restore_free_accounts(db: &mut Connection, fixture: &Fixture, seed: &OpenResult, count: u64) -> Vec<OpenResult> {
    // These complete signed account/root records are storage prerequisites, not
    // observed historical proposal, owner-authentication or opening admissions.
    let original_quote = fixture.provider.state.ledger.quote(&seed.account.contract_id).unwrap();
    let (created, root): (u64, String) = db.query_row(
        "SELECT accounts.created,credentials.record FROM accounts
         JOIN credentials ON credentials.contract=accounts.id
         WHERE accounts.id=?1 AND credentials.id=?2",
        params![seed.account.contract_id, seed.credential.id],
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
            payload.consumer_wallet = format!("0x{:040x}", 0x10000 + index);
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
                samples.push(OpenResult { account, credential });
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
        earlier.extend(restore_free_accounts(&mut db, &fixture, &earlier[0], 9_997));
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
            let authenticated = ledger.authenticate(&accepted.credential.bearer, protocol::now().unwrap()).unwrap();
            assert_eq!(authenticated.credential_id, accepted.credential.id);
            assert_eq!(serde_json::to_value(authenticated.account).unwrap(), serde_json::to_value(&accepted.account).unwrap());
        }
        let (status, account) = fixture.send("GET",
            &format!("/v1/contracts/{}", accepted.account.contract_id), &bearer(&accepted.credential), Vec::new()).await;
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
                fixture.credentials_usable(&[&earlier.credential]);
                let (status, account) = fixture.send("GET",
                    &format!("/v1/contracts/{}", earlier.account.contract_id),
                    &bearer(&earlier.credential), Vec::new()).await;
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
        assert_eq!(opened.credential.contract_id, quote.payload.id, "{point}");
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
            fixture.credentials_usable(&[&earlier.credential]);
            let (status, account) = fixture.send("GET",
                &format!("/v1/contracts/{}", earlier.account.contract_id),
                &bearer(&earlier.credential), Vec::new()).await;
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
