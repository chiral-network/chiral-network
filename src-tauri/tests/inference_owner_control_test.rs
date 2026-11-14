//! Account reads and mutations through the production control router and isolated disk ledgers.
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
    auth::{self, AuthPayload},
    billing_store::{BillingStore, CredentialIssueRequest, RequestView},
    chain_client::{ValidatedPaymentEnvelope, VerifiedPayment},
    contracts,
    protocol::{self, Challenge, Credential, FundingIntent, OpenResult, Quote, Signed, TokenUsage, UsageReceipt, RECEIPT_DOMAIN},
    wallet,
};
use proposal::PreparedProvider;
use rlp::RlpStream;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::{json, Value};
use std::{net::TcpListener, path::Path, sync::atomic::Ordering};
use tiny_keccak::{Hasher, Keccak};
use tower::Service;

// Public fixture scalars, never funded or used outside isolated test state.
const OWNER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000007";
const OTHER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000008";
const REQUEST_KEY: &str = "read-proof-request";
const OPEN: &str = "/v1/contracts/open";

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
