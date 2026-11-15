//! Public challenge issuance through the production HTTP router, with isolated disk ledgers.
//! No chain/engine requests, live wallets, performance qualification, or deployment claims.
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
    billing_store::BillingStore,
    contracts,
    protocol::{self, Challenge, Quote, Signed},
    wallet,
};
use proposal::PreparedProvider;
use serde_json::{json, Value};
use std::{net::TcpListener, path::Path};
use tower::Service;

const CHALLENGE: &str = "/v1/auth/challenge";
const OPEN: &str = "/v1/contracts/open";
const PREFIX: &str = "/v1/contracts/";
const PAYER: &str = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd";
// Public, deterministic fixture key; never funded or used outside the isolated ledger.
const OWNER_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000007";

struct Fixture {
    provider: PreparedProvider,
    app: Router,
    _listeners: [TcpListener; 2],
}

impl Fixture {
    fn new(directory: &Path, case: u64) -> Self {
        // Reserve loopback addresses without starting unused servers or making network calls.
        let public = TcpListener::bind("127.0.0.1:0").unwrap();
        let admin = TcpListener::bind("127.0.0.1:0").unwrap();
        let provider = PreparedProvider::new(directory, case, "0", "0",
            public.local_addr().unwrap(), admin.local_addr().unwrap());
        let app = contracts::router(provider.state.clone());
        Self { provider, app, _listeners: [public, admin] }
    }

    async fn send(&mut self, request: Request) -> (StatusCode, Value) {
        let response = self.app.call(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 96 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn post(&mut self, path: &str, body: &Value) -> (StatusCode, Value) {
        self.send(Request::builder().method("POST").uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap())).unwrap()).await
    }

    async fn issue(&mut self, request: &Value) -> AuthPayload {
        let (status, value) = self.post(CHALLENGE, request).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let challenge: Challenge = serde_json::from_value(value).unwrap();
        let nonce_hex = challenge.nonce.strip_prefix("0x").unwrap();
        assert_eq!(nonce_hex.len(), 64);
        assert!(nonce_hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        // Reconstruct from the intended request and selected signed offer, not returned bytes.
        // Framing and canonical JSON have independent vectors in inference_wire_foundation_test.
        let expected = AuthPayload {
            network: self.provider.offer.payload.network.clone(),
            provider_wallet: self.provider.offer.payload.provider_wallet.clone(),
            wallet: request["wallet"].as_str().unwrap().to_ascii_lowercase(),
            method: request["method"].as_str().unwrap().into(),
            path: request["path"].as_str().unwrap().into(),
            body_hash: request["body_hash"].as_str().unwrap().to_ascii_lowercase(),
            nonce: challenge.nonce,
            expires_at: challenge.expires_at,
        };
        let returned = hex::decode(challenge.payload_hex.strip_prefix("0x").unwrap()).unwrap();
        assert_eq!(returned, auth::challenge_payload(&expected).unwrap());
        expected
    }
}

fn intended(wallet: &str, method: &str, path: &str, body: &[u8]) -> Value {
    json!({"wallet": wallet, "method": method, "path": path,
        "body_hash": protocol::hash_hex(body)})
}

async fn reject(fixture: &mut Fixture, request: &Value, status: StatusCode, code: &str) {
    let (actual, body) = fixture.post(CHALLENGE, request).await;
    assert_eq!(actual, status, "request={request}; response={body}");
    assert_eq!(body["error"]["code"], code, "request={request}");
}

#[tokio::test]
async fn public_challenges_bind_canonical_requests_and_complete_provider_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 70);
    let alias = format!("0x{}", PAYER[2..].to_ascii_uppercase());
    let account = format!("0x{}", "12".repeat(32));
    let mut nonces = std::collections::HashSet::new();
    for (method, path, body) in [
        ("GET", format!("{PREFIX}{account}/requests?before=000200"), ""),
        ("POST", OPEN.into(), "{ \"note\": \"é\" }\n"),
        ("DELETE", format!("{PREFIX}{account}/credentials/fixture"), ""),
    ] {
        let mut request = intended(&alias, method, &path, body.as_bytes());
        let digest = request["body_hash"].as_str().unwrap();
        request["body_hash"] = json!(format!("0x{}", digest[2..].to_ascii_uppercase()));
        let payload = fixture.issue(&request).await;
        assert_eq!(payload.wallet, PAYER);
        assert_eq!(payload.body_hash, protocol::hash_hex(body.as_bytes()));
        assert!(nonces.insert(payload.nonce), "separate requests must not share a proof nonce");
    }
}

#[tokio::test]
async fn public_challenge_schema_rejects_missing_mistyped_extra_and_duplicate_fields() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 71);
    let valid = intended(PAYER, "POST", OPEN, b"{}");
    for field in ["wallet", "method", "path", "body_hash"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        reject(&mut fixture, &missing, StatusCode::BAD_REQUEST, "invalid_request").await;
        for wrong in [Value::Null, json!(true), json!(42), json!(1.5), json!([]), json!({})] {
            let mut request = valid.clone();
            request[field] = wrong;
            reject(&mut fixture, &request, StatusCode::BAD_REQUEST, "invalid_request").await;
        }
    }
    for field in ["network", "provider_wallet", "nonce", "expires_at", "unexpected"] {
        let mut request = valid.clone();
        request[field] = json!("caller-must-not-supply-this");
        reject(&mut fixture, &request, StatusCode::BAD_REQUEST, "invalid_request").await;
    }
    for wrong in [
        Value::Null, json!([]), json!(true), json!(42), json!("request"),
        json!([valid["wallet"], valid["method"], valid["path"], valid["body_hash"]]),
    ] {
        reject(&mut fixture, &wrong, StatusCode::BAD_REQUEST, "invalid_request").await;
    }
    for field in ["wallet", "method", "path", "body_hash"] {
        let mut raw = serde_json::to_string(&valid).unwrap();
        raw.pop();
        raw.push_str(&format!(",\"{field}\":{}}}", valid[field]));
        let (status, body) = fixture.send(Request::builder().method("POST").uri(CHALLENGE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(raw)).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "duplicate {field}: {body}");
        assert_eq!(body["error"]["code"], "invalid_request");
    }
    // Invalid attempts cannot exhaust the legitimate request's issuance allowance.
    fixture.issue(&valid).await;
}

#[tokio::test]
async fn challenge_operation_validation_preserves_exact_paths_and_utf8_byte_limits() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 72);
    let valid = intended(PAYER, "GET", &format!("{PREFIX}unknown?b=%2F&a=1&a=2"), b"");
    for method in ["get", "post", "delete", "PUT", "PATCH", "HEAD", "OPTIONS", " POST", "POST "] {
        let mut request = valid.clone();
        request["method"] = json!(method);
        reject(&mut fixture, &request, StatusCode::BAD_REQUEST, "invalid_challenge").await;
    }
    for wallet in [
        String::new(), PAYER[2..].into(), format!("0X{}", &PAYER[2..]),
        format!("0x{}", "a".repeat(39)), format!("0x{}", "a".repeat(41)),
        format!("0x{}", "g".repeat(40)), format!("0x{}", "é".repeat(20)),
    ] {
        let mut request = valid.clone();
        request["wallet"] = json!(wallet);
        reject(&mut fixture, &request, StatusCode::UNAUTHORIZED, "invalid_wallet").await;
    }
    for digest in [
        String::new(), "ab".repeat(32), format!("0X{}", "ab".repeat(32)),
        format!("0x{}", "a".repeat(63)), format!("0x{}", "a".repeat(65)),
        format!("0x{}", "g".repeat(64)), format!("0x{}", "é".repeat(32)),
    ] {
        let mut request = valid.clone();
        request["body_hash"] = json!(digest);
        reject(&mut fixture, &request, StatusCode::BAD_REQUEST, "invalid_challenge").await;
    }
    let ascii_limit = format!("{PREFIX}{}", "a".repeat(512 - PREFIX.len()));
    let utf8_limit = format!("{PREFIX}{}", "é".repeat((512 - PREFIX.len()) / 2));
    let mut forbidden_paths = vec![
        "/v1/contracts".into(), "/v1/contractsX/".into(), CHALLENGE.into(),
        "https://provider.invalid/v1/contracts/open".into(), format!("{PREFIX}open#fragment"),
        format!("{ascii_limit}a"), format!("{utf8_limit}a"),
    ];
    for control in ['\0', '\t', '\n', '\r', '\x1f', '\x7f'] {
        forbidden_paths.push(format!("{PREFIX}open{control}"));
    }
    for path in forbidden_paths {
        let mut request = valid.clone();
        request["path"] = json!(path);
        reject(&mut fixture, &request, StatusCode::BAD_REQUEST, "invalid_challenge").await;
    }
    // Issuance preserves opaque targets, even when they do not name an implemented route.
    fixture.issue(&valid).await;
    for path in [ascii_limit, utf8_limit] {
        fixture.issue(&intended(PAYER, "GET", &path, b"")).await;
    }
}

#[tokio::test]
async fn wallet_hex_aliases_share_the_same_pending_challenge_allowance() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 73);
    let alias = format!("0x{}", PAYER[2..].to_ascii_uppercase());
    for index in 0..8 {
        let wallet = if index % 2 == 0 { PAYER } else { &alias };
        fixture.issue(&intended(wallet, "POST", OPEN, b"{}")).await;
    }
    for wallet in [PAYER, alias.as_str()] {
        reject(&mut fixture, &intended(wallet, "POST", OPEN, b"{}"),
            StatusCode::TOO_MANY_REQUESTS, "rate_limit").await;
    }
    let other_wallet = "0x1111111111111111111111111111111111111111";
    fixture.issue(&intended(other_wallet, "POST", OPEN, b"{}")).await;
}

#[tokio::test]
async fn issued_challenge_survives_restart_but_public_fields_do_not_authorize_opening() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(directory.path(), 74);
    let owner = wallet::address_from_private_key(OWNER_KEY).unwrap();
    let proposal = json!({"offer_id": fixture.provider.offer_id,
        "consumer_wallet": owner, "funding_wei": "0"});
    let (status, value) = fixture.post("/v1/contracts/propose", &proposal).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let quote: Signed<Quote> = serde_json::from_value(value).unwrap();
    let body = serde_json::to_vec(&json!({"quote_id": quote.payload.id, "raw_transaction": null})).unwrap();
    let payload = fixture.issue(&intended(&owner, "POST", OPEN, &body)).await;
    let (status, value) = fixture.send(Request::builder().method("POST").uri(OPEN)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-chiral-owner", &owner).header("x-chiral-nonce", &payload.nonce)
        .body(Body::from(body.clone())).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["error"]["code"], "invalid_api_key");
    assert!(fixture.provider.state.ledger.account(&quote.payload.id).is_err());

    let ledger_path = fixture.provider.ledger_path.clone();
    drop(fixture);
    let reopened = BillingStore::open(&ledger_path, &payload.network).unwrap();
    let persisted = reopened.challenge(&payload.nonce).unwrap();
    assert_eq!(serde_json::to_value(&persisted).unwrap(), serde_json::to_value(&payload).unwrap());
    let signature = wallet::sign_message(OWNER_KEY, &auth::challenge_payload(&payload).unwrap()).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("x-chiral-owner", owner.parse().unwrap());
    headers.insert("x-chiral-nonce", payload.nonce.parse().unwrap());
    headers.insert("x-chiral-signature", signature.parse().unwrap());
    // The authentication API's explicit time avoids sleeps or assumptions about test scheduling.
    let before_expiry = payload.expires_at.checked_sub(1).unwrap();
    assert_eq!(auth::authenticate_owner(&reopened, &headers, &payload.network,
        &payload.provider_wallet, "POST", OPEN, &body, before_expiry).unwrap(), owner);
    assert!(reopened.challenge(&payload.nonce).is_err());
    // Acquiring owner authority alone still has not committed an account-opening operation.
    assert!(reopened.account(&quote.payload.id).is_err());
}
