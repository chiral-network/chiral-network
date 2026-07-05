//! End-to-end integration tests for the three provider functions. Each mounts a
//! data-plane router over the SAME `ProviderState` as the handshake router, and
//! drives propose → commit-on-chain → open → use-the-data-plane in-process,
//! proving a session minted by the handshake is honored by every data plane
//! (storage / inference / compute). The per-module unit tests only cover each
//! piece in isolation with a directly-inserted session.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use chiral_network::codec::hex_to_array;
use chiral_network::container_api::{self, ContainerState};
use chiral_network::container_provider::{
    ContainerProvider, ContainerRates, ContainerRuntime, ResourceEnvelope, ResourceRequest,
};
use chiral_network::contract_api;
use chiral_network::contract_service::{ChainVerifier, ProviderState, VerifiedFunding};
use chiral_network::llm_api::{self, LlmState};
use chiral_network::llm_provider::{LlmProvider, TokenRates};
use chiral_network::resource_offer::{ResourceClass, ResourceOffer};
use chiral_network::service_contract::encode_open;
use chiral_network::storage_api::{self, StorageState};
use chiral_network::storage_provider::StorageProvider;

/// A mock chain whose funding is injected after `propose` (once the random nonce
/// + terms_hash are known), then returned from `verify_funding`.
struct MockChain {
    funding: Mutex<Option<VerifiedFunding>>,
}
impl ChainVerifier for MockChain {
    fn verify_funding(&self, _tx: &str, _to: &str) -> Result<VerifiedFunding, String> {
        self.funding.lock().unwrap().clone().ok_or_else(|| "funding not set".to_string())
    }
}

struct MockRt;
impl ContainerRuntime for MockRt {
    fn start(&self, id: &str, _image: &str, _req: ResourceRequest) -> Result<String, String> {
        Ok(format!("https://{}.host", id))
    }
    fn stop(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
}

const PROVIDER: &str = "0xprovider000000000000000000000000000000ab";
const CONSUMER: &str = "0xconsumer000000000000000000000000000000cd";
const TX: &str = "0xabc0000000000000000000000000000000000000000000000000000000000123";
const AMOUNT: u128 = 1_000_000_000_000_000_000; // 1 CHI

fn offer(class: ResourceClass, capacity: Value, price: Value) -> ResourceOffer {
    ResourceOffer {
        provider_wallet: PROVIDER.into(),
        resource_class: class,
        capacity,
        price_schedule: price,
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

fn get_authed(uri: &str, bearer: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {}", bearer))
        .body(Body::empty())
        .unwrap()
}

/// Run the full handshake against a fresh shared `ProviderState` for `offer`,
/// returning the shared state plus the minted session's bearer + bucket.
async fn open_via_handshake(offer: ResourceOffer) -> (Arc<Mutex<ProviderState<MockChain>>>, String, String) {
    let provider = ProviderState::new(
        PROVIDER.into(),
        "0x".to_string() + &"33".repeat(32),
        offer.clone(),
        MockChain { funding: Mutex::new(None) },
    );
    let shared = Arc::new(Mutex::new(provider));
    let control = contract_api::router(shared.clone());

    // propose
    let resp = control
        .clone()
        .oneshot(post_json(
            "/v1/contracts/propose",
            json!({
                "offer_ref": offer.offer_ref(),
                "consumer_wallet": CONSUMER,
                "funding_amount_wei": AMOUNT.to_string(),
                "params": {}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "propose");
    let quote = body_json(resp).await;
    let nonce = quote["contract_nonce"].as_str().unwrap();
    let terms_hash = quote["terms_hash"].as_str().unwrap();
    let offer_ref = quote["terms"]["offer_ref"].as_str().unwrap();

    // commit on-chain (inject the funding the mock will report)
    let data = encode_open(
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
        .replace(VerifiedFunding { from: CONSUMER.into(), value_wei: AMOUNT, data, confirmed: true });

    // open
    let resp = control
        .clone()
        .oneshot(post_json("/v1/contracts/open", json!({ "tx_hash": TX })))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "open");
    let opened = body_json(resp).await;
    assert_eq!(opened["balance_wei"], "995000000000000000"); // 1 CHI - 0.5% fee
    let bearer = opened["credential"]["bearer"].as_str().unwrap().to_string();
    let bucket = opened["credential"]["bucket"].as_str().unwrap().to_string();
    (shared, bearer, bucket)
}

#[tokio::test]
async fn handshake_then_storage() {
    let (shared, bearer, bucket) =
        open_via_handshake(offer(ResourceClass::Storage, json!({ "gb_available": 100 }), json!({}))).await;
    let store = StorageProvider::new(1_000_000_000_000_000, 10_000_000_000_000_000, 100_000_000);
    let data = storage_api::router(StorageState::new(shared, store));
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
    assert_eq!(resp.status(), StatusCode::OK, "PUT via handshake session");

    let resp = data.oneshot(get_authed(&uri, &bearer)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let balance: u128 = resp
        .headers()
        .get("X-Chiral-Balance-Wei")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(balance < 995_000_000_000_000_000, "egress debits the shared balance");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"integration");
}

#[tokio::test]
async fn handshake_then_llm() {
    let (shared, bearer, _) = open_via_handshake(offer(
        ResourceClass::Inference,
        json!({ "models": [ { "id": "llama" } ] }),
        json!({ "llama": { "per_1k_input_wei": "1000000", "per_1k_output_wei": "2000000" } }),
    ))
    .await;
    let llm = LlmProvider::new().with_model(
        "llama",
        TokenRates { per_1k_input_wei: 1_000_000, per_1k_output_wei: 2_000_000 },
    );
    let data = llm_api::router(LlmState::new(shared, llm, "http://127.0.0.1:1".into()));

    // The handshake-minted session authorizes the inference data plane.
    let resp = data.oneshot(get_authed("/v1/models", &bearer)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let j = body_json(resp).await;
    assert_eq!(j["data"][0]["id"], "llama");
}

#[tokio::test]
async fn handshake_then_container() {
    let (shared, bearer, _) =
        open_via_handshake(offer(ResourceClass::Container, json!({}), json!({}))).await;
    let containers = ContainerProvider::new(ContainerRates {
        per_vcpu_hour_wei: 3_600_000,
        per_gb_mem_hour_wei: 360_000,
        per_gpu_hour_wei: 36_000_000,
    });
    let data = container_api::router(ContainerState::new(
        shared,
        containers,
        MockRt,
        ResourceEnvelope { max_vcpu: 4, max_mem_gb: 8, max_gpu: 1 },
    ));

    // The handshake-minted session authorizes launching a container.
    let resp = data
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/containers")
                .header(header::AUTHORIZATION, format!("Bearer {}", bearer))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "image": "nginx", "resources": { "vcpu": 2, "mem_gb": 4 } }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let j = body_json(resp).await;
    assert!(j["endpoints"][0]["url"].as_str().unwrap().starts_with("https://"));
}

#[tokio::test]
async fn storage_rejects_unknown_session() {
    let (shared, _, _) =
        open_via_handshake(offer(ResourceClass::Storage, json!({}), json!({}))).await;
    let store = StorageProvider::new(1, 1, 100_000_000);
    let data = storage_api::router(StorageState::new(shared, store));
    let resp = data
        .oneshot(get_authed("/c-deadbeefdeadbeef/x", "chi_sess_bogus"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
