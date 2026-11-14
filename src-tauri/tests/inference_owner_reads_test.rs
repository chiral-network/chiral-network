//! Account-read authorization through the production control router and isolated disk ledgers.
//! Free accounts are opened over HTTP; scoped credentials and cancelled requests are ledger prerequisites.
//! No engine, chain RPC, paid funding, or simulated inference output.
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
    contracts,
    protocol::{self, Challenge, Credential, OpenResult, Quote, Signed, TokenUsage, UsageReceipt, RECEIPT_DOMAIN},
    wallet,
};
use proposal::PreparedProvider;
use serde_json::{json, Value};
use std::{net::TcpListener, path::Path};
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

    async fn open_free(&mut self) -> OpenResult {
        let owner = wallet::address_from_private_key(OWNER_KEY).unwrap();
        let (status, value) = self.post("/v1/contracts/propose", json!({
            "offer_id": self.provider.offer_id, "consumer_wallet": owner, "funding_wei": "0"
        })).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let quote: Signed<Quote> = serde_json::from_value(value).unwrap();
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
        let issue = |scope: &str| ledger.issue_credential(&target.account.contract_id,
            &CredentialIssueRequest { expires_at: now + 3600, spend_cap_wei: None,
                max_concurrency: 1, scopes: vec![scope.into()] }, now).unwrap();
        let read = issue("read");
        let infer = issue("infer");
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
