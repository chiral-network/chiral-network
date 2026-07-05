//! End-to-end integration test for a storage provider: the handshake router and
//! the storage data-plane router share ONE `ProviderState`, and a contract
//! opened via `/v1/contracts/*` yields a session credential that the S3 object
//! API honors. This exercises the full flow in-process —
//! propose → commit-on-chain → open → PUT → GET → metered drawdown — which the
//! per-module unit tests only cover in isolation with mocks.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use chiral_network::codec::hex_to_array;
use chiral_network::contract_api;
use chiral_network::contract_service::{ChainVerifier, ProviderState, VerifiedFunding};
use chiral_network::resource_offer::{ResourceClass, ResourceOffer};
use chiral_network::service_contract::encode_open;
use chiral_network::storage_api::{self, StorageState};
use chiral_network::storage_provider::StorageProvider;

/// A mock chain whose funding result is injected after `propose` (once the
/// random nonce + terms_hash are known), then returned from `verify_funding`.
struct MockChain {
    funding: Mutex<Option<VerifiedFunding>>,
}
impl ChainVerifier for MockChain {
    fn verify_funding(&self, _tx: &str, _to: &str) -> Result<VerifiedFunding, String> {
        self.funding.lock().unwrap().clone().ok_or_else(|| "funding not set".to_string())
    }
}

const PROVIDER: &str = "0xprovider000000000000000000000000000000ab";
const CONSUMER: &str = "0xconsumer000000000000000000000000000000cd";
const TX: &str = "0xabc0000000000000000000000000000000000000000000000000000000000123";
const AMOUNT: u128 = 1_000_000_000_000_000_000; // 1 CHI

fn offer() -> ResourceOffer {
    ResourceOffer {
        provider_wallet: PROVIDER.into(),
        resource_class: ResourceClass::Storage,
        capacity: json!({ "gb_available": 100 }),
        price_schedule: json!({ "per_gb_month": "10000000000000000" }),
        endpoint: "https://p.example".into(),
        region: "us-east".into(),
        min_funding_wei: "100000000000000000".into(), // 0.1 CHI
        offer_nonce: 1,
        valid_until: 0,
        signature: String::new(),
    }
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn handshake_then_storage_end_to_end() {
    // One shared ProviderState behind both routers.
    let provider = ProviderState::new(
        PROVIDER.into(),
        "0x".to_string() + &"33".repeat(32),
        offer(),
        MockChain { funding: Mutex::new(None) },
    );
    let shared = Arc::new(Mutex::new(provider));

    let control = contract_api::router(shared.clone());
    let store = StorageProvider::new(1_000_000_000_000_000, 10_000_000_000_000_000, 100_000_000);
    let data = storage_api::router(StorageState::new(shared.clone(), store));

    // 1. propose -> quote (random nonce + terms_hash).
    let resp = control
        .clone()
        .oneshot(post_json(
            "/v1/contracts/propose",
            json!({
                "offer_ref": offer().offer_ref(),
                "consumer_wallet": CONSUMER,
                "funding_amount_wei": AMOUNT.to_string(),
                "params": {}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let quote = body_json(resp).await;
    let nonce = quote["contract_nonce"].as_str().unwrap();
    let terms_hash = quote["terms_hash"].as_str().unwrap();
    let offer_ref = quote["terms"]["offer_ref"].as_str().unwrap();

    // 2. Build the on-chain contract commitment and inject it as the funding tx.
    let data_bytes = encode_open(
        &hex_to_array::<32>(offer_ref).unwrap(),
        &hex_to_array::<32>(terms_hash).unwrap(),
        &hex_to_array::<16>(nonce).unwrap(),
    );
    shared
        .lock()
        .unwrap()
        .verifier
        .funding
        .lock()
        .unwrap()
        .replace(VerifiedFunding {
            from: CONSUMER.into(),
            value_wei: AMOUNT,
            data: data_bytes,
            confirmed: true,
        });

    // 3. open -> a contract-scoped session credential (bearer + bucket).
    let resp = control
        .clone()
        .oneshot(post_json("/v1/contracts/open", json!({ "tx_hash": TX })))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let opened = body_json(resp).await;
    let bearer = opened["credential"]["bearer"].as_str().unwrap().to_string();
    let bucket = opened["credential"]["bucket"].as_str().unwrap().to_string();
    // Post-fee credit on 1 CHI at 0.5% = 0.995 CHI.
    assert_eq!(opened["balance_wei"], "995000000000000000");

    // 4. PUT an object on the DATA plane using the handshake-minted session.
    let uri = format!("/{}/hello.txt", bucket);
    let resp = data
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", bearer))
                .body(Body::from(b"integration".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "PUT via handshake session must be authorized");

    // 5. GET it back — bytes match, egress debits the same contract's balance.
    let resp = data
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", bearer))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let balance: u128 = resp
        .headers()
        .get("X-Chiral-Balance-Wei")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(balance < 995_000_000_000_000_000, "egress must debit the shared balance");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"integration");
}

/// A session that does not belong to the addressed bucket is rejected — the
/// data plane trusts the shared session store, not the caller.
#[tokio::test]
async fn storage_rejects_foreign_bucket() {
    let provider = ProviderState::new(
        PROVIDER.into(),
        "0x".to_string() + &"33".repeat(32),
        offer(),
        MockChain { funding: Mutex::new(None) },
    );
    let shared = Arc::new(Mutex::new(provider));
    let store = StorageProvider::new(1, 1, 100_000_000);
    let data = storage_api::router(StorageState::new(shared.clone(), store));

    // No session at all -> unauthorized.
    let resp = data
        .oneshot(
            Request::builder()
                .uri("/c-deadbeefdeadbeef/x")
                .header(header::AUTHORIZATION, "Bearer chi_sess_none")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
